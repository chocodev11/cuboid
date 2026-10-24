//! 176-byte element instances and vertex pulling (spec: app 8.3-8.4; arch 13.3).
//!
//! The instance buffer is a vertex buffer read at instance rate, so the path needs no storage
//! buffers and runs on every backend. A CPU shadow of the records keeps the incremental upload
//! (one `write_buffer` per run of changed slots) and the rebuild after device loss independent of
//! the scene.

use cuboid_scene::{INSTANCE_WORDS, InstanceUpdate};

use crate::contract::RenderError;
use crate::targets::{COLOR, DEPTH};
use crate::util::{guarded, to_u32, to_u64};

/// Bytes of one instance record.
pub const INSTANCE_BYTES: u64 = (INSTANCE_WORDS * 4) as u64;

/// Vertices drawn per instance: six faces of two triangles.
pub const VERTICES: u32 = 36;

/// Capacity of the first instance buffer.
const MIN_CAPACITY: u32 = 256;

/// The vertex layout of an instance record (see `elements.wgsl` for the word map).
const ATTRIBUTES: [wgpu::VertexAttribute; 11] = [
    attribute(0, 0, wgpu::VertexFormat::Uint32x4),
    attribute(1, 16, wgpu::VertexFormat::Uint32x4),
    attribute(2, 32, wgpu::VertexFormat::Float32x4),
    attribute(3, 48, wgpu::VertexFormat::Float32x4),
    attribute(4, 64, wgpu::VertexFormat::Float32x4),
    attribute(5, 80, wgpu::VertexFormat::Float32x4),
    attribute(6, 96, wgpu::VertexFormat::Float32x4),
    attribute(7, 112, wgpu::VertexFormat::Float32x4),
    attribute(8, 128, wgpu::VertexFormat::Float32x4),
    attribute(9, 144, wgpu::VertexFormat::Uint32x4),
    attribute(10, 160, wgpu::VertexFormat::Uint32x4),
];

const fn attribute(
    shader_location: u32,
    offset: u64,
    format: wgpu::VertexFormat,
) -> wgpu::VertexAttribute {
    wgpu::VertexAttribute {
        format,
        offset,
        shader_location,
    }
}

/// The element instances: the shadow, the device buffer and the slots waiting for upload.
#[derive(Debug)]
pub(crate) struct Elements {
    shadow: Vec<[u32; INSTANCE_WORDS]>,
    buffer: wgpu::Buffer,
    capacity: u32,
    dirty: Vec<u32>,
}

impl Elements {
    /// An empty pool with the first buffer.
    pub fn new(device: &wgpu::Device) -> Result<Elements, RenderError> {
        Ok(Elements {
            shadow: Vec::new(),
            buffer: create_buffer(device, MIN_CAPACITY)?,
            capacity: MIN_CAPACITY,
            dirty: Vec::new(),
        })
    }

    /// GPU bytes of the buffer.
    pub fn bytes(&self) -> u64 {
        u64::from(self.capacity) * INSTANCE_BYTES
    }

    /// Number of live instances.
    pub fn count(&self) -> u32 {
        to_u32(self.shadow.len())
    }

    /// The device buffer, for the instance-rate vertex slot.
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// Takes the new count and the changed records into the shadow. A record past the count is
    /// dropped.
    pub fn apply(&mut self, count: u32, updates: &[InstanceUpdate]) {
        self.shadow.resize(count as usize, [0; INSTANCE_WORDS]);
        for update in updates {
            if let Some(record) = self.shadow.get_mut(update.slot as usize) {
                *record = update.instance.0;
                self.dirty.push(update.slot);
            }
        }
    }

    /// Makes the device buffer large enough. Returns false when the memory budget or the device
    /// refuses; drawing is then limited to the instances that fit.
    pub fn grow(&mut self, device: &wgpu::Device, room: u64) -> bool {
        let need = self.count();
        if need <= self.capacity {
            return true;
        }
        let capacity = need.next_power_of_two();
        let extra = u64::from(capacity - self.capacity) * INSTANCE_BYTES;
        let limit = device.limits().max_buffer_size;
        if extra > room || u64::from(capacity) * INSTANCE_BYTES > limit {
            log::error!("no room for {need} element instances");
            return false;
        }
        match create_buffer(device, capacity) {
            Ok(buffer) => {
                self.buffer = buffer;
                self.capacity = capacity;
                self.dirty = (0..need).collect();
                true
            }
            Err(err) => {
                log::error!("cannot grow the instance buffer: {err}");
                false
            }
        }
    }

    /// Instances the device buffer can hold right now.
    pub fn drawable(&self) -> u32 {
        self.count().min(self.capacity)
    }

    /// Uploads changed slots in order, merging neighbours into one write, until `bytes` are used
    /// up or `out_of_time` says so; at least one run goes out per call. Returns the bytes sent.
    pub fn upload(
        &mut self,
        queue: &wgpu::Queue,
        bytes: usize,
        out_of_time: impl Fn() -> bool,
    ) -> usize {
        self.dirty.sort_unstable();
        self.dirty.dedup();
        let live = self.drawable();
        let mut used = 0usize;
        let mut at = 0;
        while at < self.dirty.len() {
            let first = self.dirty[at];
            let mut end = at + 1;
            while end < self.dirty.len() && self.dirty[end] == self.dirty[end - 1] + 1 {
                end += 1;
            }
            let last = self.dirty[end - 1];
            if first >= live {
                at = end;
                continue;
            }
            let stop = (last + 1).min(live);
            let cost = (stop - first) as usize * INSTANCE_WORDS * 4;
            if used > 0 && (used + cost > bytes || out_of_time()) {
                break;
            }
            let records = &self.shadow[first as usize..stop as usize];
            queue.write_buffer(
                &self.buffer,
                u64::from(first) * INSTANCE_BYTES,
                bytemuck::cast_slice(records),
            );
            used += cost;
            at = end;
        }
        self.dirty.drain(..at);
        used
    }

    /// Moves the records of the pool of a lost device into this one and queues them all.
    pub fn adopt(&mut self, device: &wgpu::Device, old: Elements) -> Result<(), RenderError> {
        self.shadow = old.shadow;
        self.rebuild(device)
    }

    /// Recreates the buffer on a new device and queues every record.
    fn rebuild(&mut self, device: &wgpu::Device) -> Result<(), RenderError> {
        self.capacity = self.count().next_power_of_two().max(MIN_CAPACITY);
        self.buffer = create_buffer(device, self.capacity)?;
        self.dirty = (0..self.count()).collect();
        Ok(())
    }

    /// Bytes waiting for upload.
    pub fn pending_bytes(&self) -> u64 {
        to_u64(self.dirty.len()) * INSTANCE_BYTES
    }
}

fn create_buffer(device: &wgpu::Device, capacity: u32) -> Result<wgpu::Buffer, RenderError> {
    guarded(device, "element instances", || {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("element_instances"),
            size: u64::from(capacity) * INSTANCE_BYTES,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    })
}

/// Which fragment entry point and which attachments a pipeline of the element shader uses.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Pass {
    /// Alpha-tested, depth-writing and blended: a run of binary objects.
    Cutout,
    /// Alpha-tested depth only, the first pass of a translucent object.
    Prepass,
    /// Blended colour with depth test and no depth write.
    Blend,
    /// Silhouette mask of the outline, single sample, no depth.
    Mask,
}

/// The depth state of a pass; reversed Z, so a nearer fragment compares greater.
pub(crate) fn depth_state(write: bool) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH,
        depth_write_enabled: Some(write),
        depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// Blending of the original: `SrcAlpha, OneMinusSrcAlpha` for colour and alpha (arch 13.2).
pub(crate) const BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// Front faces of the left-handed internal space appear clockwise on the screen.
pub(crate) const FRONT: wgpu::FrontFace = wgpu::FrontFace::Cw;

/// The pipeline of one element pass.
pub(crate) fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    pass: Pass,
    samples: u32,
) -> wgpu::RenderPipeline {
    let (entry, write_mask, depth) = match pass {
        Pass::Cutout => ("fs_cutout", wgpu::ColorWrites::ALL, Some(depth_state(true))),
        Pass::Prepass => (
            "fs_cutout",
            wgpu::ColorWrites::empty(),
            Some(depth_state(true)),
        ),
        Pass::Blend => ("fs_blend", wgpu::ColorWrites::ALL, Some(depth_state(false))),
        Pass::Mask => ("fs_mask", wgpu::ColorWrites::ALL, None),
    };
    let mask = pass == Pass::Mask;
    let blend = if mask { None } else { Some(BLEND) };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("elements"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: INSTANCE_BYTES,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &ATTRIBUTES,
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR,
                blend,
                write_mask,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            front_face: FRONT,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: depth,
        multisample: wgpu::MultisampleState {
            count: if mask { 1 } else { samples },
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,
    })
}
