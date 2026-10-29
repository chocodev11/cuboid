//! The seam between the GPU device of the shell and the renderer (spec: arch 13.1, 13.9).
//!
//! The renderer is created on the device the shell shares with egui. The per-frame path
//! (extract, sync, draw lists, `render_views`, the view textures to the viewport panel) is in
//! [`crate::frontend`].

use cuboid_render::{AdapterCaps, DeviceWatch, Renderer};
use egui_wgpu::RenderState;
use wgpu::{DownlevelFlags, Features};

use crate::startup::{self, Phase};

/// Device features the renderer uses when the adapter has them; the shell requests them.
pub const OPTIONAL_FEATURES: Features =
    Features::INDIRECT_FIRST_INSTANCE.union(Features::POLYGON_MODE_LINE);

pub struct RenderGlue {
    pub renderer: Renderer,
    /// The callbacks of the device of the window; the renderer of the thumbnails shares them,
    /// since wgpu keeps one set per device.
    pub watch: DeviceWatch,
    device: wgpu::Device,
    lost_reported: bool,
}

/// What the adapter and its device support.
pub fn adapter_caps(adapter: &wgpu::Adapter, device: &wgpu::Device) -> AdapterCaps {
    let flags = adapter.get_downlevel_capabilities().flags;
    let features = device.features();
    AdapterCaps {
        indirect_execution: flags.contains(DownlevelFlags::INDIRECT_EXECUTION),
        indirect_first_instance: features.contains(Features::INDIRECT_FIRST_INSTANCE),
        vertex_storage: flags.contains(DownlevelFlags::VERTEX_STORAGE),
        polygon_mode_line: features.contains(Features::POLYGON_MODE_LINE),
        timestamp_query: features.contains(Features::TIMESTAMP_QUERY),
        max_texture_size: device.limits().max_texture_dimension_2d,
    }
}

impl RenderGlue {
    /// Probes the adapter and creates the renderer, which logs wgpu errors instead of panicking
    /// and flags a lost device.
    pub fn new(gpu: &RenderState) -> RenderGlue {
        let caps = adapter_caps(&gpu.adapter, &gpu.device);
        let info = gpu.adapter.get_info();
        log::info!(
            "graphics: {} ({:?}, {:?}), format {:?}, {caps:?}",
            info.name,
            info.backend,
            info.device_type,
            gpu.target_format
        );
        let watch = DeviceWatch::new(&gpu.device);
        let renderer = Renderer::with_watch(&gpu.device, &gpu.queue, &caps, &watch);
        startup::mark(Phase::Renderer);
        RenderGlue {
            renderer,
            watch,
            device: gpu.device.clone(),
            lost_reported: false,
        }
    }

    /// True once after the device was lost.
    pub fn take_device_lost(&mut self) -> bool {
        let lost = self.watch.lost() && !self.lost_reported;
        self.lost_reported |= lost;
        lost
    }

    /// When the shell hands out another device than the renderer holds, the renderer recreates
    /// its resources on it. True when that happened (the view textures are new then).
    pub fn follow_device(&mut self, gpu: &RenderState) -> bool {
        if self.device == gpu.device {
            return false;
        }
        log::info!("the graphics device changed; rebuilding the renderer");
        let watch = DeviceWatch::new(&gpu.device);
        self.renderer.rebuild(
            &gpu.device,
            &gpu.queue,
            &adapter_caps(&gpu.adapter, &gpu.device),
            &watch,
        );
        self.watch = watch;
        self.device = gpu.device.clone();
        self.lost_reported = false;
        true
    }
}
