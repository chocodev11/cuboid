//! Runtime atlas pages and incremental insertion (spec: tools 5.2; arch 13.6).
//!
//! The scene crate packs the textures and sends dirty rectangles; the renderer holds one RGBA
//! shadow per page (the source of a rebuild after device loss) and one array texture on the
//! device. The scene says the edge and the number of the pages with every extract; rows of a page
//! run bottom to top, so a texture coordinate `v` (up) is the row over the edge.
//!
//! Every update is a texture with a one-texel transparent ring (the scene's padding). The shadow
//! replaces that ring with copies of the texels next to it, a gutter, so a sample that lands just
//! outside the face rectangle (MSAA, float error) reads the edge colour instead of a hole.

use cuboid_scene::AtlasUpdate;

use crate::contract::RenderError;
use crate::targets::COLOR;
use crate::util::{guarded, to_u32, to_u64};

/// Edge of the placeholder page that exists before the first extract names the real one.
pub const DEFAULT_ATLAS_SIZE: u32 = 256;

/// Layers of a fresh array texture; a one-layer texture would become a plain 2D texture on the GL
/// backend and break the array view.
const MIN_LAYERS: u32 = 2;

/// Pages the renderer accepts (the page number has 5 bits in the face state); an update for a
/// higher page is dropped with an error log.
pub const MAX_ATLAS_PAGES: u32 = 32;

/// A rectangle of a page waiting for its upload.
#[derive(Copy, Clone, Debug)]
struct Pending {
    page: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// The atlas pages: CPU shadows, the device array texture and the rectangles still to upload.
#[derive(Debug)]
pub(crate) struct Atlas {
    size: u32,
    pages: Vec<Vec<u8>>,
    texture: wgpu::Texture,
    layers: u32,
    pub bind_group: wgpu::BindGroup,
    pending: Vec<Pending>,
}

/// The layout of the atlas bind group: the array texture and a nearest, clamped sampler.
pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("atlas_layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// The nearest-filtered, clamped sampler of the atlas and the mesh textures.
pub(crate) fn sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("nearest_clamp"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..wgpu::SamplerDescriptor::default()
    })
}

/// Bytes of one page.
fn page_bytes(size: u32) -> u64 {
    u64::from(size) * u64::from(size) * 4
}

impl Atlas {
    /// An atlas with empty layers on the device.
    pub fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        size: u32,
    ) -> Result<Atlas, RenderError> {
        let (texture, bind_group) = create_texture(device, layout, sampler, size, MIN_LAYERS)?;
        Ok(Atlas {
            size,
            pages: Vec::new(),
            texture,
            layers: MIN_LAYERS,
            bind_group,
            pending: Vec::new(),
        })
    }

    /// GPU bytes of the array texture.
    pub fn bytes(&self) -> u64 {
        page_bytes(self.size) * u64::from(self.layers)
    }

    /// Edge of a page.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Makes sure `pages` pages exist, empty ones if need be.
    pub fn reserve(&mut self, pages: u32) {
        let size = self.size as usize;
        while self.pages.len() < pages.min(MAX_ATLAS_PAGES) as usize {
            self.pages.push(vec![0; size * size * 4]);
        }
    }

    /// Copies an update into the shadow of its page and queues its upload. A rectangle that does
    /// not fit its page is clipped; data that is too short is dropped.
    pub fn apply(&mut self, update: &AtlasUpdate) {
        if update.page >= MAX_ATLAS_PAGES {
            log::error!("atlas page {} is above the limit", update.page);
            return;
        }
        let size = self.size as usize;
        let (x, y) = (update.x.min(self.size), update.y.min(self.size));
        let (w, h) = (update.w.min(self.size - x), update.h.min(self.size - y));
        let stride = update.w as usize * 4;
        if w == 0 || h == 0 || update.rgba.len() < stride * update.h as usize {
            return;
        }
        while self.pages.len() <= update.page as usize {
            self.pages.push(vec![0; size * size * 4]);
        }
        let Some(page) = self.pages.get_mut(update.page as usize) else {
            return;
        };
        let row_bytes = w as usize * 4;
        for row in 0..h as usize {
            let src = row * stride;
            let dst = ((y as usize + row) * size + x as usize) * 4;
            page[dst..dst + row_bytes].copy_from_slice(&update.rgba[src..src + row_bytes]);
        }
        if (w, h) == (update.w, update.h) {
            fill_gutter(
                page,
                size,
                (x as usize, y as usize),
                (w as usize, h as usize),
            );
        }
        self.pending.push(Pending {
            page: update.page,
            x,
            y,
            w,
            h,
        });
    }

    /// Makes room for the pages that have content: doubles the layers of the array texture while
    /// the memory allows. Returns false when a page cannot be shown.
    pub fn grow(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        room: u64,
    ) -> bool {
        let need = to_u32(self.pages.len());
        if need <= self.layers {
            return true;
        }
        let layers = need.next_power_of_two().min(MAX_ATLAS_PAGES);
        let extra = page_bytes(self.size) * u64::from(layers - self.layers);
        if extra > room {
            log::error!("no GPU memory budget left for {layers} atlas pages");
            return false;
        }
        match create_texture(device, layout, sampler, self.size, layers) {
            Ok((texture, bind_group)) => {
                self.texture = texture;
                self.bind_group = bind_group;
                self.layers = layers;
                self.queue_all();
                true
            }
            Err(err) => {
                log::error!("cannot grow the atlas: {err}");
                false
            }
        }
    }

    /// Queues every page in full.
    fn queue_all(&mut self) {
        let size = self.size;
        self.pending = (0..to_u32(self.pages.len()))
            .map(|page| Pending {
                page,
                x: 0,
                y: 0,
                w: size,
                h: size,
            })
            .collect();
    }

    /// Uploads waiting rectangles until `bytes` are used up or `out_of_time` says so; at least
    /// one goes out per call. Returns the bytes uploaded.
    pub fn upload(
        &mut self,
        queue: &wgpu::Queue,
        mut bytes: usize,
        out_of_time: impl Fn() -> bool,
    ) -> usize {
        let mut used = 0;
        let mut done = 0;
        let size = self.size as usize;
        for rect in &self.pending {
            let page = self.pages.get(rect.page as usize);
            let (Some(page), true) = (page, rect.page < self.layers) else {
                done += 1;
                continue;
            };
            let cost = rect.w as usize * rect.h as usize * 4;
            if used > 0 && (cost > bytes || out_of_time()) {
                break;
            }
            let at = (rect.y as usize * size + rect.x as usize) * 4;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: rect.x,
                        y: rect.y,
                        z: rect.page,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &page[at..],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.size * 4),
                    rows_per_image: Some(rect.h),
                },
                wgpu::Extent3d {
                    width: rect.w,
                    height: rect.h,
                    depth_or_array_layers: 1,
                },
            );
            used += cost;
            bytes = bytes.saturating_sub(cost);
            done += 1;
        }
        self.pending.drain(..done);
        used
    }

    /// The atlas of a lost device on a new one: the same pages, queued for upload.
    pub fn adopted(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        old: Atlas,
    ) -> Result<Atlas, RenderError> {
        let (texture, bind_group) = create_texture(device, layout, sampler, old.size, old.layers)?;
        let mut atlas = Atlas {
            size: old.size,
            pages: old.pages,
            texture,
            layers: old.layers,
            bind_group,
            pending: Vec::new(),
        };
        atlas.queue_all();
        Ok(atlas)
    }

    /// Bytes waiting for upload.
    pub fn pending_bytes(&self) -> u64 {
        self.pending
            .iter()
            .map(|r| to_u64(r.w as usize * r.h as usize * 4))
            .sum()
    }
}

/// Overwrites the outermost ring of the `size`-wide rectangle at `at` with copies of the texels
/// next to it: the sides first, then the top and bottom rows including the corners. A rectangle
/// under 3 x 3 has no ring to replace.
pub(crate) fn fill_gutter(
    page: &mut [u8],
    page_size: usize,
    at: (usize, usize),
    size: (usize, usize),
) {
    let ((x, y), (w, h)) = (at, size);
    if w < 3 || h < 3 {
        return;
    }
    let texel = |col: usize, row: usize| (row * page_size + col) * 4;
    for row in y + 1..y + h - 1 {
        let left = texel(x, row);
        page.copy_within(left + 4..left + 8, left);
        let right = texel(x + w - 1, row);
        page.copy_within(right - 4..right, right);
    }
    let width = w * 4;
    let (top, below) = (texel(x, y), texel(x, y + 1));
    page.copy_within(below..below + width, top);
    let (bottom, above) = (texel(x, y + h - 1), texel(x, y + h - 2));
    page.copy_within(above..above + width, bottom);
}

/// An array texture of `layers` empty pages with its bind group.
fn create_texture(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    size: u32,
    layers: u32,
) -> Result<(wgpu::Texture, wgpu::BindGroup), RenderError> {
    guarded(device, "atlas pages", || {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..wgpu::TextureViewDescriptor::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        (texture, bind_group)
    })
}
