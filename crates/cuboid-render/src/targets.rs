//! Shared attachments, per-view targets and frame keys (spec: arch 13.1).

use cuboid_scene::FrameKey;

use crate::contract::RenderError;
use crate::util::guarded;

/// The finished picture of one view: an `Rgba8Unorm` texture with alpha forced to 1 that the UI
/// registers once with egui (`register_native_texture`) and re-binds on resize.
///
/// spec: arch 13.1 (steps 2 and 3), arch 6 (cuboid-render)
#[derive(Clone, Debug)]
pub struct ViewTexture {
    /// The texture, for copies and read-backs.
    pub texture: wgpu::Texture,
    /// A view of the texture.
    pub view: wgpu::TextureView,
    /// Size in physical pixels.
    pub size: (u32, u32),
    /// Changes whenever the texture object was recreated, so a registration must be redone.
    pub generation: u64,
}

/// Format of the colour targets, the view textures and the PNG target.
pub(crate) const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Format of the reversed-Z depth buffer.
pub(crate) const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Attachment sizes are rounded up to this many pixels so a resize drag does not reallocate.
const BUCKET: u32 = 64;

/// The attachments one render pass set draws into. One set serves all four views because views
/// render one after another; only the final textures are per view.
#[derive(Debug)]
pub(crate) struct Attachments {
    /// The size of every texture of the set.
    pub size: (u32, u32),
    /// 1 or 4.
    pub samples: u32,
    /// Multisampled colour, when `samples` is 4.
    pub msaa: Option<wgpu::TextureView>,
    /// Single-sample colour: the resolve target, or the target itself without multisampling.
    pub resolve: wgpu::TextureView,
    /// Reversed-Z depth, `samples` samples.
    pub depth: wgpu::TextureView,
    /// The highlight mask of the outline, single sample.
    pub mask: wgpu::TextureView,
    /// GPU bytes of the set.
    pub bytes: u64,
}

/// GPU bytes of one set of the given size.
pub(crate) fn attachment_bytes(size: (u32, u32), samples: u32) -> u64 {
    let px = u64::from(size.0) * u64::from(size.1);
    let samples = u64::from(samples);
    // Resolve target and mask, the depth buffer and the multisampled colour.
    let msaa = if samples > 1 { px * samples * 4 } else { 0 };
    px * 8 + px * samples * 4 + msaa
}

impl Attachments {
    /// The bucketed size that holds `size`.
    pub fn bucket(size: (u32, u32)) -> (u32, u32) {
        let edge = |n: u32| n.max(1).div_ceil(BUCKET).saturating_mul(BUCKET);
        (edge(size.0), edge(size.1))
    }

    /// Creates a set of exactly `size`.
    pub fn new(
        device: &wgpu::Device,
        size: (u32, u32),
        samples: u32,
    ) -> Result<Attachments, RenderError> {
        guarded(device, "view attachments", || {
            let make = |label: &'static str, format, samples, usage| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width: size.0,
                            height: size.1,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage,
                        view_formats: &[],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default())
            };
            let attach = wgpu::TextureUsages::RENDER_ATTACHMENT;
            let sampled = attach | wgpu::TextureUsages::TEXTURE_BINDING;
            Attachments {
                size,
                samples,
                msaa: (samples > 1).then(|| make("scene_msaa", COLOR, samples, attach)),
                resolve: make("scene_resolve", COLOR, 1, sampled),
                depth: make("scene_depth", DEPTH, samples, attach),
                mask: make("outline_mask", COLOR, 1, sampled),
                bytes: attachment_bytes(size, samples),
            }
        })
    }
}

/// One view's own texture and what it was last rendered from.
#[derive(Debug, Default)]
pub(crate) struct ViewSlot {
    pub texture: Option<(wgpu::Texture, wgpu::TextureView)>,
    pub size: (u32, u32),
    pub generation: u64,
    /// The key of the last render; a draw list with the same key keeps the picture.
    pub key: Option<FrameKey>,
}

impl ViewSlot {
    /// Makes the texture `size` large, recreating it when the size differs. Returns whether the
    /// texture is new (and must be rendered).
    pub fn fit(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
        generation: &mut u64,
    ) -> Result<bool, RenderError> {
        if self.texture.is_some() && self.size == size {
            return Ok(false);
        }
        let texture = guarded(device, "view texture", || {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("view_texture"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: COLOR,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        })?;
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        *generation += 1;
        self.generation = *generation;
        self.texture = Some((texture, view));
        self.size = size;
        self.key = None;
        Ok(true)
    }

    /// Bytes of the texture.
    pub fn bytes(&self) -> u64 {
        if self.texture.is_some() {
            u64::from(self.size.0) * u64::from(self.size.1) * 4
        } else {
            0
        }
    }

    /// The texture handle for the UI; `None` before the first render.
    pub fn handle(&self) -> Option<ViewTexture> {
        let (texture, view) = self.texture.as_ref()?;
        Some(ViewTexture {
            texture: texture.clone(),
            view: view.clone(),
            size: self.size,
            generation: self.generation,
        })
    }
}
