//! Device-level resources, adapter capabilities and `Renderer::new` (spec: app 8; arch 13.9).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cuboid_model::ObjId;
use cuboid_scene::RenderExtract;

use crate::atlas::{self, Atlas, DEFAULT_ATLAS_SIZE};
use crate::contract::{
    AdapterCaps, Health, ReadbackId, RenderError, RenderSettings, UploadBudget, ViewParams,
};
use crate::elements::Elements;
use crate::grid::plane_data;
use crate::meshes::Meshes;
use crate::pipelines::{DIR_BYTES, Layouts, PLANE_BYTES, Pipelines, VIEW_BYTES};
use crate::readback::Pending;
use crate::targets::{Attachments, ViewSlot, attachment_bytes};
use crate::util::{guarded, to_u64};
use crate::voxels::{Voxels, dir_tables};
use crate::{outline, overlays};

/// Stride of the view uniform slots: four views and the PNG render.
pub(crate) const VIEW_STRIDE: u64 = 256;

/// Number of view uniform slots.
pub(crate) const VIEW_SLOTS: usize = 5;

/// The slot of the PNG render.
pub(crate) const PNG_SLOT: usize = 4;

/// How long the renderer waits before it tries to create missing pipelines again.
const PIPELINE_RETRY: Duration = Duration::from_secs(2);

/// What the device callbacks tell the renderer.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    pub lost: AtomicBool,
    error: Mutex<Option<String>>,
}

impl Shared {
    fn set_error(&self, message: String) {
        *self.error.lock().unwrap_or_else(PoisonError::into_inner) = Some(message);
    }

    fn error(&self) -> Option<String> {
        self.error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// A vertex buffer that grows to what a view needs each frame.
#[derive(Debug, Default)]
pub(crate) struct Scratch {
    pub buffer: Option<wgpu::Buffer>,
    capacity: u64,
}

impl Scratch {
    /// Writes `bytes` at the start, recreating the buffer when it is too small. False when the
    /// buffer cannot be had.
    pub fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) -> bool {
        let need = to_u64(bytes.len());
        if need == 0 {
            return true;
        }
        if need > self.capacity || self.buffer.is_none() {
            let capacity = need.next_power_of_two().max(1 << 14);
            let buffer = guarded(device, "overlay lines", || {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("overlay_lines"),
                    size: capacity,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            });
            match buffer {
                Ok(buffer) => {
                    self.buffer = Some(buffer);
                    self.capacity = capacity;
                }
                Err(err) => {
                    log::error!("{err}");
                    return false;
                }
            }
        }
        if let Some(buffer) = &self.buffer {
            queue.write_buffer(buffer, 0, bytes);
        }
        true
    }
}

/// Everything the renderer holds on the device; absent when the device could not provide it.
#[derive(Debug)]
pub(crate) struct Gpu {
    pub layouts: Layouts,
    pub sampler: wgpu::Sampler,
    pub pipes: Option<Pipelines>,
    pub elements: Elements,
    pub voxels: Voxels,
    pub atlas: Atlas,
    pub meshes: Meshes,
    pub view_buf: wgpu::Buffer,
    pub view_bg: wgpu::BindGroup,
    pub dir_bg: wgpu::BindGroup,
    pub plane_bg: wgpu::BindGroup,
    pub attach: Option<Attachments>,
    pub composite_bg: Option<wgpu::BindGroup>,
    pub lines: [Scratch; VIEW_SLOTS],
}

/// A uniform buffer filled once with `contents`, and its dynamic-offset bind group.
fn static_uniform(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    label: &'static str,
    contents: &[u8],
    binding_size: u64,
) -> Result<wgpu::BindGroup, RenderError> {
    guarded(device, label, || {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: to_u64(contents.len()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, contents);
        uniform_group(device, layout, &buffer, binding_size)
    })
}

fn uniform_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
    size: u64,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("uniform"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: wgpu::BufferSize::new(size),
            }),
        }],
    })
}

/// The pipelines for the sample count of the setting, or `None` (logged) when the device refuses.
fn build_pipelines(device: &wgpu::Device, layouts: &Layouts, msaa: bool) -> Option<Pipelines> {
    match Pipelines::new(device, layouts, if msaa { 4 } else { 1 }) {
        Ok(pipes) => Some(pipes),
        Err(err) => {
            log::error!("cannot create the pipelines: {err}");
            None
        }
    }
}

impl Gpu {
    /// Creates the layouts, pools, uniforms and pipelines. The shadows of `old`, when given, move
    /// into the new pools and queue their upload.
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        settings: &RenderSettings,
        old: Option<Gpu>,
    ) -> Result<Gpu, RenderError> {
        let layouts = Layouts::new(device);
        let sampler = atlas::sampler(device);
        let mut elements = Elements::new(device)?;
        let mut voxels = Voxels::new(device, queue)?;
        let mut atlas = Atlas::new(device, &layouts.atlas, &sampler, DEFAULT_ATLAS_SIZE)?;
        let mut meshes = Meshes::new();
        if let Some(old) = old {
            elements.adopt(device, old.elements)?;
            voxels.adopt(device, queue, old.voxels)?;
            atlas = Atlas::adopted(device, &layouts.atlas, &sampler, old.atlas)?;
            meshes.adopt(old.meshes);
        }
        let view_buf = guarded(device, "view uniforms", || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("view_uniforms"),
                size: VIEW_STRIDE * VIEW_SLOTS as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })?;
        let view_bg = uniform_group(device, &layouts.view, &view_buf, VIEW_BYTES);
        let dir_bg = static_uniform(
            device,
            queue,
            &layouts.dir,
            "voxel_directions",
            &dir_tables(),
            DIR_BYTES,
        )?;
        let plane_bg = static_uniform(
            device,
            queue,
            &layouts.plane,
            "sub_grid_planes",
            &plane_data(),
            PLANE_BYTES,
        )?;
        let pipes = build_pipelines(device, &layouts, settings.msaa);
        Ok(Gpu {
            layouts,
            sampler,
            pipes,
            elements,
            voxels,
            atlas,
            meshes,
            view_buf,
            view_bg,
            dir_bg,
            plane_bg,
            attach: None,
            composite_bg: None,
            lines: Default::default(),
        })
    }

    /// GPU bytes held.
    pub fn bytes(&self) -> u64 {
        self.elements.bytes()
            + self.voxels.bytes()
            + self.atlas.bytes()
            + self.meshes.bytes()
            + self.attach.as_ref().map_or(0, |a| a.bytes)
    }
}

/// The wgpu renderer: device-level resources, the element, mesh and voxel pools, the atlas
/// pages, the shared attachments and the four view targets. It consumes scene extracts and draw
/// lists and never reads the document.
///
/// The methods of the facade are spread over their modules: `new` and `sync` here,
/// `render_views` in `composite`, `start_png_render` in `offscreen` and `poll_readbacks` in
/// `readback`.
///
/// spec: arch 6 (cuboid-render), arch 13.1
#[derive(Debug)]
pub struct Renderer {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) caps: AdapterCaps,
    pub(crate) settings: RenderSettings,
    pub(crate) shared: Arc<Shared>,
    pub(crate) gpu: Option<Gpu>,
    pub(crate) slots: [ViewSlot; 4],
    pub(crate) generation: u64,
    pub(crate) readbacks: Vec<Pending>,
    pub(crate) next_readback: u64,
    pub(crate) degraded: bool,
    /// The mesh drawn with the hover colour in the outline.
    pub(crate) hovered_mesh: Option<ObjId>,
    /// The multisampling the application asked for last; `settings.msaa` is what is in force.
    msaa_wanted: bool,
    /// When missing pipelines were last tried.
    pipes_retry: Option<Instant>,
}

/// Routes the device-lost and uncaptured-error callbacks to the shared state. The default
/// uncaptured-error handler of wgpu panics, which the frame loop must never do.
fn watch(device: &wgpu::Device, shared: &Arc<Shared>) {
    let lost = Arc::clone(shared);
    device.set_device_lost_callback(move |reason, message| {
        log::error!("the graphics device was lost ({reason:?}): {message}");
        lost.set_error(format!("device lost: {message}"));
        lost.lost.store(true, Ordering::Release);
    });
    let errors = Arc::clone(shared);
    device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
        log::error!("graphics error: {error}");
        errors.set_error(error.to_string());
    }));
}

impl Renderer {
    /// Creates the pipelines and pools for a device. The device is shared with the egui painter.
    /// It installs a device-lost callback and an uncaptured-error handler on the device that log
    /// and flag the renderer instead of panicking.
    ///
    /// spec: arch 13.9, arch 14.1
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, caps: &AdapterCaps) -> Renderer {
        let shared = Arc::new(Shared::default());
        watch(device, &shared);
        let settings = RenderSettings::default();
        let gpu = match Gpu::new(device, queue, &settings, None) {
            Ok(gpu) => Some(gpu),
            Err(err) => {
                log::error!("cannot create the renderer: {err}");
                shared.set_error(err.to_string());
                None
            }
        };
        Renderer {
            device: device.clone(),
            queue: queue.clone(),
            caps: *caps,
            settings,
            shared,
            gpu,
            slots: Default::default(),
            generation: 0,
            readbacks: Vec::new(),
            next_readback: 0,
            degraded: false,
            hovered_mesh: None,
            msaa_wanted: settings.msaa,
            pipes_retry: None,
        }
    }

    /// Whether the device was lost; `rebuild` with a new device recovers.
    pub fn device_lost(&self) -> bool {
        self.shared.lost.load(Ordering::Acquire)
    }

    /// Recreates every device resource on a new device from the shadows of the old ones: the
    /// instances, the voxel faces, the atlas pages and the meshes upload again over the next
    /// `sync` calls, so the scene sends nothing new. View textures are new (their generation
    /// changes) and every view renders again. Pending PNG renders fail with `DeviceLost`.
    ///
    /// spec: arch 13.9, roadmap 7
    pub fn rebuild(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, caps: &AdapterCaps) {
        let shared = Arc::new(Shared::default());
        watch(device, &shared);
        let old = self.gpu.take();
        self.device = device.clone();
        self.queue = queue.clone();
        self.caps = *caps;
        self.shared = shared;
        for pending in &mut self.readbacks {
            pending.fail(RenderError::DeviceLost);
        }
        self.slots = Default::default();
        self.gpu = match Gpu::new(device, queue, &self.settings, old) {
            Ok(gpu) => Some(gpu),
            Err(err) => {
                log::error!("cannot rebuild the renderer: {err}");
                self.shared.set_error(err.to_string());
                None
            }
        };
    }

    /// How the renderer is doing.
    pub fn health(&self) -> Health {
        Health {
            device_lost: self.device_lost(),
            degraded: self.degraded || self.gpu.as_ref().is_some_and(|g| g.voxels.starved),
            gpu_bytes: self.gpu.as_ref().map_or(0, Gpu::bytes),
            elements: self.gpu.as_ref().map_or(0, |g| g.elements.count()),
            voxel_faces: self.gpu.as_ref().map_or(0, |g| g.voxels.face_count()),
            last_error: self.shared.error(),
        }
    }

    /// The preferences in force.
    pub fn settings(&self) -> RenderSettings {
        self.settings
    }

    /// Applies new preferences. Switching multisampling rebuilds the pipelines and attachments
    /// and renders every view again. Multisampling that the memory budget dropped stays off until
    /// the application asks for the other value or raises the budget.
    pub fn set_settings(&mut self, settings: RenderSettings) {
        let more_room = settings.memory_budget > self.settings.memory_budget;
        let asked = settings.msaa != self.msaa_wanted || (more_room && settings.msaa);
        self.msaa_wanted = settings.msaa;
        let msaa = if asked {
            settings.msaa
        } else {
            self.settings.msaa
        };
        let msaa_changed = msaa != self.settings.msaa;
        self.settings = RenderSettings { msaa, ..settings };
        if msaa_changed {
            self.degraded = false;
            if let Some(gpu) = &mut self.gpu {
                gpu.attach = None;
                gpu.composite_bg = None;
                gpu.pipes = build_pipelines(&self.device, &gpu.layouts, msaa);
            }
        }
        for slot in &mut self.slots {
            slot.key = None;
        }
    }

    /// Sets the mesh that gets the hover colour in the outline; every other mesh of an outline
    /// list is drawn in the selection colour. The mesh must not be selected as well. Renders the
    /// views again when it changes.
    pub fn set_hovered_mesh(&mut self, id: Option<ObjId>) {
        if self.hovered_mesh == id {
            return;
        }
        self.hovered_mesh = id;
        for slot in &mut self.slots {
            slot.key = None;
        }
    }

    /// Tries to create missing pipelines again, at most every few seconds.
    pub(crate) fn retry_pipelines(&mut self) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        if gpu.pipes.is_some()
            || self.shared.lost.load(Ordering::Acquire)
            || self
                .pipes_retry
                .is_some_and(|t| t.elapsed() < PIPELINE_RETRY)
        {
            return;
        }
        self.pipes_retry = Some(Instant::now());
        gpu.pipes = build_pipelines(&self.device, &gpu.layouts, self.settings.msaa);
        if gpu.pipes.is_some() {
            gpu.attach = None;
            gpu.composite_bg = None;
            for slot in &mut self.slots {
                slot.key = None;
            }
        }
    }

    /// Bytes of GPU memory the budget still allows.
    pub(crate) fn room(&self) -> u64 {
        let used = self.gpu.as_ref().map_or(0, Gpu::bytes)
            + self.slots.iter().map(ViewSlot::bytes).sum::<u64>();
        self.settings.memory_budget.saturating_sub(used)
    }

    /// Uploads what changed within the budget: instance ranges, finished chunk meshes and
    /// texture dirty rectangles. The content goes into the shadows at once; what does not fit the
    /// budget stays queued and goes out over the following calls (at least one item per kind
    /// and call), so call it every frame, with the extractor's output (an extract with
    /// `instance_count` 0 removes every instance), until [`Renderer::needs_frame`] is false.
    /// Whatever it uploads renders the views again at the next [`Renderer::render_views`]. On a
    /// lost device the shadows still take the extract and nothing is uploaded. The queue is the
    /// one the renderer holds (the argument is kept for compatibility).
    ///
    /// spec: arch 13.1 (step 1)
    pub fn sync(&mut self, _queue: &wgpu::Queue, ex: &RenderExtract, budget: UploadBudget) {
        let start = Instant::now();
        let lost = self.device_lost();
        let used_by_views: u64 = self.slots.iter().map(ViewSlot::bytes).sum();
        let limit = self.settings.memory_budget;
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        if !lost
            && ex.atlas_page_size >= 16
            && ex.atlas_page_size != gpu.atlas.size()
            && ex.atlas_page_size <= self.caps.max_texture_size
        {
            match Atlas::new(
                &self.device,
                &gpu.layouts.atlas,
                &gpu.sampler,
                ex.atlas_page_size,
            ) {
                Ok(atlas) => gpu.atlas = atlas,
                Err(err) => log::error!("cannot resize the atlas pages: {err}"),
            }
        }
        gpu.elements.apply(ex.instance_count, &ex.instances);
        gpu.atlas.reserve(ex.atlas_pages);
        for update in &ex.atlas {
            gpu.atlas.apply(update);
        }
        for chunk in &ex.chunks {
            gpu.voxels.apply(chunk);
        }
        gpu.voxels.reserve(&ex.voxel_pools);
        for mesh in &ex.meshes {
            gpu.meshes.apply(mesh);
        }
        for id in &ex.removed_meshes {
            gpu.meshes.remove(*id);
        }
        if lost {
            return;
        }

        let room = |gpu: &Gpu| limit.saturating_sub(gpu.bytes() + used_by_views);
        let held = gpu.bytes();
        let mut fits = gpu.elements.grow(&self.device, room(gpu));
        let space = room(gpu);
        fits &= gpu
            .atlas
            .grow(&self.device, &gpu.layouts.atlas, &gpu.sampler, space);
        let space = room(gpu);
        fits &= gpu.voxels.grow(&self.device, &self.queue, space);
        if !fits {
            self.degraded = true;
        }
        let grew = gpu.bytes() != held;

        let queue = &self.queue;
        let out_of_time = || start.elapsed().as_secs_f32() * 1000.0 >= budget.millis;
        let mut bytes = budget.bytes;
        let mut sent = gpu.elements.upload(queue, bytes, out_of_time);
        bytes = bytes.saturating_sub(sent);
        let step = gpu.atlas.upload(queue, bytes, out_of_time);
        bytes = bytes.saturating_sub(step);
        sent += step;
        let space = room(gpu);
        let step = gpu.meshes.upload(
            &self.device,
            queue,
            &gpu.layouts.mesh,
            &gpu.sampler,
            space,
            bytes,
            out_of_time,
        );
        bytes = bytes.saturating_sub(step);
        sent += step;
        sent += gpu.voxels.upload(queue, bytes, out_of_time);
        let moved = std::mem::take(&mut gpu.meshes.wrote);

        // Nothing in a view's key says what the pools held when it rendered.
        if sent > 0 || grew || moved {
            for slot in &mut self.slots {
                slot.key = None;
            }
        }
    }

    /// Whether uploads are still waiting: keep calling [`Renderer::sync`] and rendering the views
    /// every frame until it is false, because each frame only uploads what the budget allows.
    pub fn needs_frame(&self) -> bool {
        self.pending_upload() > 0
    }

    /// Bytes still waiting for upload, for the status display and for tests that drain the queue.
    pub fn pending_upload(&self) -> u64 {
        self.gpu.as_ref().map_or(0, |g| {
            g.elements.pending_bytes()
                + g.atlas.pending_bytes()
                + g.voxels.pending_bytes()
                + g.meshes.pending_bytes()
        })
    }

    /// Fits the shared attachments to `size` (bucketed), dropping multisampling when the memory
    /// budget does not allow it. Returns false when no attachments can be had.
    pub(crate) fn fit_attachments(&mut self, size: (u32, u32)) -> bool {
        let room = self.room();
        let Some(gpu) = &mut self.gpu else {
            return false;
        };
        let samples = gpu.pipes.as_ref().map_or(1, |p| p.samples);
        let want = Attachments::bucket(size);
        if let Some(a) = &gpu.attach
            && a.size.0 >= size.0
            && a.size.1 >= size.1
            && a.samples == samples
        {
            return true;
        }
        let held = gpu.attach.as_ref().map_or(0, |a| a.bytes);
        let want = gpu
            .attach
            .as_ref()
            .map_or(want, |a| (want.0.max(a.size.0), want.1.max(a.size.1)));
        gpu.attach = None;
        gpu.composite_bg = None;
        if attachment_bytes(want, samples) > room + held {
            if samples > 1 {
                log::error!("no GPU memory budget left for multisampling; it is switched off");
                self.degraded = true;
                self.settings.msaa = false;
                gpu.pipes = build_pipelines(&self.device, &gpu.layouts, false);
                return self.fit_attachments(size);
            }
            log::error!("no GPU memory budget left for a view of {size:?}");
            self.degraded = true;
            return false;
        }
        match Attachments::new(&self.device, want, samples) {
            Ok(attach) => {
                gpu.composite_bg = Some(outline::bind_group(
                    &self.device,
                    &gpu.layouts.composite,
                    &attach,
                ));
                gpu.attach = Some(attach);
                true
            }
            Err(err) => {
                log::error!("cannot create the view attachments: {err}");
                self.degraded = true;
                false
            }
        }
    }

    /// Writes the uniform of a view slot.
    pub(crate) fn write_view(&self, slot: usize, params: &ViewParams) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        let mut words = [0u32; 28];
        for (word, value) in words.iter_mut().zip(params.view_proj.to_cols_array()) {
            *word = value.to_bits();
        }
        let cam = params.camera_pos;
        words[16..19].copy_from_slice(&[cam.x.to_bits(), cam.y.to_bits(), cam.z.to_bits()]);
        words[20] = u32::from(params.view_index);
        words[21] = params.size.0;
        words[22] = params.size.1;
        words[24] = self.settings.voxel_cell.to_bits();
        words[25] = self.settings.grid_step.to_bits();
        self.queue.write_buffer(
            &gpu.view_buf,
            slot as u64 * VIEW_STRIDE,
            bytemuck::cast_slice(&words),
        );
    }

    /// The id of the next PNG render.
    pub(crate) fn next_id(&mut self) -> ReadbackId {
        self.next_readback += 1;
        ReadbackId(self.next_readback)
    }

    /// The lines of a list's overlays, uploaded to the slot's buffer. Returns the vertex counts
    /// of the depth-tested and the over-everything lines.
    pub(crate) fn upload_lines(
        &mut self,
        slot: usize,
        view: u8,
        items: &[cuboid_scene::OverlayItem],
    ) -> (u32, u32) {
        let lines = overlays::build(items, view);
        let (tested, over) = (
            crate::util::to_u32(lines.tested.len()),
            crate::util::to_u32(lines.over.len()),
        );
        if tested + over == 0 {
            return (0, 0);
        }
        let mut words = Vec::with_capacity((lines.tested.len() + lines.over.len()) * 4);
        for vertex in lines.tested.iter().chain(&lines.over) {
            words.extend_from_slice(vertex);
        }
        let Some(gpu) = &mut self.gpu else {
            return (0, 0);
        };
        if gpu.lines[slot].write(&self.device, &self.queue, bytemuck::cast_slice(&words)) {
            (tested, over)
        } else {
            (0, 0)
        }
    }
}
