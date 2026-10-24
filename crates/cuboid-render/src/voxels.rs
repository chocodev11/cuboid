//! Voxel face pools, culling and submission tiers (spec: app 8.5; arch 13.5).
//!
//! Six pools (one per direction) hold the 8-byte face records of all chunks. The scene crate owns
//! the layout: every chunk mesh says where its records go (`offsets`), the extract gives the
//! capacity of each pool, and draw lists name the ranges to draw. The renderer keeps a CPU
//! shadow of every pool, so growth and the rebuild after device loss need nothing from the
//! scene, and uploads the changed ranges within the frame budget. Unwritten records are padding
//! (all four AO codes 3), which the shader collapses.

use cuboid_scene::{ChunkMesh, ChunkRange};
use cuboid_voxel::{Dir, Rev, VoxelFace, expand_face};
use glam::Vec3;
use rustc_hash::FxHashMap;

use crate::contract::RenderError;
use crate::elements::{BLEND, depth_state};
use crate::targets::COLOR;
use crate::util::{guarded, to_u32};

/// Bytes of one face record.
pub const FACE_BYTES: u64 = 8;

/// Records of the first buffer of a pool.
const MIN_CAPACITY: u32 = 4096;

/// Records one pool may hold: 512 MiB.
const MAX_CAPACITY: u32 = 1 << 26;

/// Stride of the direction table slots in its uniform buffer.
pub(crate) const DIR_STRIDE: u32 = 256;

/// A padding record: all four AO codes are 3, which the mesher never emits.
const PADDING: [u32; 2] = [u32::MAX, u32::MAX];

/// One direction pool: the shadow, the buffer and the ranges waiting for upload.
#[derive(Debug)]
struct Pool {
    shadow: Vec<[u32; 2]>,
    buffer: wgpu::Buffer,
    /// Records the buffer holds.
    capacity: u32,
    /// `(first, len)` ranges of the shadow not yet on the device, oldest first.
    dirty: Vec<(u32, u32)>,
}

/// A buffer of `capacity` records, all padding.
fn create_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    capacity: u32,
) -> Result<wgpu::Buffer, RenderError> {
    let buffer = guarded(device, "voxel pool", || {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("voxel_pool"),
            size: u64::from(capacity) * FACE_BYTES,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    })?;
    queue.write_buffer(
        &buffer,
        0,
        bytemuck::cast_slice(&vec![PADDING; capacity as usize]),
    );
    Ok(buffer)
}

impl Pool {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Pool, RenderError> {
        Ok(Pool {
            shadow: vec![PADDING; MIN_CAPACITY as usize],
            buffer: create_buffer(device, queue, MIN_CAPACITY)?,
            capacity: MIN_CAPACITY,
            dirty: Vec::new(),
        })
    }

    /// Makes the shadow hold at least `records`; false when that is more than a pool may hold.
    fn reserve(&mut self, records: usize) -> bool {
        if records > MAX_CAPACITY as usize {
            return false;
        }
        if records > self.shadow.len() {
            self.shadow.resize(records, PADDING);
        }
        true
    }

    fn bytes(&self) -> u64 {
        u64::from(self.capacity) * FACE_BYTES
    }
}

/// The six pools and what the chunks put into them.
#[derive(Debug)]
pub(crate) struct Voxels {
    pools: [Pool; 6],
    /// The newest revision applied per chunk slot, and its records per direction.
    chunks: FxHashMap<usize, (Rev, [u32; 6])>,
    /// Set when a pool could not grow for lack of memory.
    pub starved: bool,
}

impl Voxels {
    /// Six pools of the minimum size.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Voxels, RenderError> {
        Ok(Voxels {
            pools: [
                Pool::new(device, queue)?,
                Pool::new(device, queue)?,
                Pool::new(device, queue)?,
                Pool::new(device, queue)?,
                Pool::new(device, queue)?,
                Pool::new(device, queue)?,
            ],
            chunks: FxHashMap::default(),
            starved: false,
        })
    }

    /// GPU bytes of the six buffers.
    pub fn bytes(&self) -> u64 {
        self.pools.iter().map(Pool::bytes).sum()
    }

    /// Face records of the chunks applied so far.
    pub fn face_count(&self) -> u64 {
        self.chunks
            .values()
            .flat_map(|(_, counts)| counts)
            .map(|&n| u64::from(n))
            .sum()
    }

    /// The buffer of a direction pool.
    pub fn buffer(&self, dir: usize) -> &wgpu::Buffer {
        &self.pools[dir].buffer
    }

    /// Takes a chunk mesh into the shadows at the offsets the scene chose. A mesh older than the
    /// one applied for its slot is dropped.
    pub fn apply(&mut self, mesh: &ChunkMesh) {
        if let Some((rev, _)) = self.chunks.get(&mesh.slot)
            && mesh.rev < *rev
        {
            return;
        }
        let mut counts = [0u32; 6];
        for (dir, pool) in self.pools.iter_mut().enumerate() {
            let faces = &mesh.faces[dir];
            let first = mesh.offsets[dir] as usize;
            if faces.is_empty() {
                continue;
            }
            if !pool.reserve(first + faces.len()) {
                log::error!("a voxel pool cannot hold {} records", first + faces.len());
                continue;
            }
            for (dst, face) in pool.shadow[first..].iter_mut().zip(faces) {
                *dst = face.0;
            }
            pool.dirty.push((mesh.offsets[dir], to_u32(faces.len())));
            counts[dir] = to_u32(faces.len());
        }
        self.chunks.insert(mesh.slot, (mesh.rev, counts));
    }

    /// Keeps the shadows at least as large as the pools of the scene.
    pub fn reserve(&mut self, capacities: &[u32; 6]) {
        for (pool, &records) in self.pools.iter_mut().zip(capacities) {
            if !pool.reserve(records as usize) {
                log::error!("a voxel pool of {records} records is too large");
            }
        }
    }

    /// Bytes waiting for upload.
    pub fn pending_bytes(&self) -> u64 {
        self.pools
            .iter()
            .flat_map(|p| &p.dirty)
            .map(|&(_, len)| u64::from(len) * FACE_BYTES)
            .sum()
    }

    /// Makes every buffer as large as its shadow and writes a grown pool in full. Returns false
    /// when the memory budget or the device refused a pool.
    pub fn grow(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, mut room: u64) -> bool {
        let mut ok = true;
        for pool in &mut self.pools {
            let wanted = to_u32(pool.shadow.len());
            if wanted <= pool.capacity {
                continue;
            }
            let extra = u64::from(wanted - pool.capacity) * FACE_BYTES;
            if extra > room || u64::from(wanted) * FACE_BYTES > device.limits().max_buffer_size {
                log::error!("no room to grow a voxel pool to {wanted} records");
                ok = false;
                continue;
            }
            let created = guarded(device, "voxel pool", || {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("voxel_pool"),
                    size: u64::from(wanted) * FACE_BYTES,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            });
            match created {
                Ok(buffer) => {
                    queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&pool.shadow));
                    pool.buffer = buffer;
                    pool.capacity = wanted;
                    pool.dirty.clear();
                    room -= extra;
                }
                Err(err) => {
                    log::error!("cannot grow a voxel pool: {err}");
                    ok = false;
                }
            }
        }
        self.starved = !ok;
        ok
    }

    /// Uploads waiting ranges until `bytes` are used up or `out_of_time` says so; at least one
    /// goes out per call. Returns the bytes sent.
    pub fn upload(
        &mut self,
        queue: &wgpu::Queue,
        bytes: usize,
        out_of_time: impl Fn() -> bool,
    ) -> usize {
        let mut used = 0usize;
        for pool in &mut self.pools {
            let mut done = 0;
            while let Some(&(first, len)) = pool.dirty.get(done) {
                let last = first.saturating_add(len).min(pool.capacity);
                if first < last {
                    let cost = (last - first) as usize * 8;
                    if used > 0 && (used + cost > bytes || out_of_time()) {
                        break;
                    }
                    queue.write_buffer(
                        &pool.buffer,
                        u64::from(first) * FACE_BYTES,
                        bytemuck::cast_slice(&pool.shadow[first as usize..last as usize]),
                    );
                    used += cost;
                }
                done += 1;
            }
            pool.dirty.drain(..done);
        }
        used
    }

    /// Moves the pools of a lost device into these and queues them in full.
    pub fn adopt(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        old: Voxels,
    ) -> Result<(), RenderError> {
        self.chunks = old.chunks;
        for (pool, old) in self.pools.iter_mut().zip(old.pools) {
            pool.shadow = old.shadow;
            pool.capacity = to_u32(pool.shadow.len()).max(MIN_CAPACITY);
            pool.shadow.resize(pool.capacity as usize, PADDING);
            pool.buffer = create_buffer(device, queue, pool.capacity)?;
            pool.dirty = vec![(0, pool.capacity)];
        }
        Ok(())
    }

    /// The draws of a direction for the visible ranges.
    pub fn draws(&self, dir: usize, ranges: &[ChunkRange]) -> Vec<(u32, u32)> {
        merge_ranges(ranges, self.pools[dir].capacity)
    }
}

/// The ranges clipped to a pool of `capacity` records, sorted by position, with touching and
/// overlapping ones merged into one `(first, count)`. A gap is never bridged: the records in it
/// may be stale.
fn merge_ranges(ranges: &[ChunkRange], capacity: u32) -> Vec<(u32, u32)> {
    let mut spans: Vec<(u32, u32)> = ranges
        .iter()
        .filter_map(|r| {
            let end = r.first.saturating_add(r.count).min(capacity);
            (r.first < end).then_some((r.first, end))
        })
        .collect();
    spans.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(spans.len());
    for (first, end) in spans {
        match merged.last_mut() {
            Some((_, last_end)) if first <= *last_end => *last_end = (*last_end).max(end),
            _ => merged.push((first, end)),
        }
    }
    merged.into_iter().map(|(f, e)| (f, e - f)).collect()
}

/// The table of one direction as the shader reads it: the corner offsets of the four vertices
/// and the unit face normal, 16-byte aligned.
pub(crate) fn dir_table(dir: Dir) -> [f32; 20] {
    // Expanding a probe face at the first cell of the first chunk of a unit-cell world gives the
    // corner offsets: the origin of chunk 0 is (-16, -16, 16).
    let probe = expand_face(VoxelFace::new([0; 3], [0; 3], 0, [0; 4]), dir, 1.0);
    let origin = Vec3::new(-16.0, -16.0, 16.0);
    let mut table = [0.0; 20];
    for (i, v) in probe.iter().enumerate() {
        let off = v.pos - origin;
        table[i * 4..i * 4 + 3].copy_from_slice(&off.to_array());
    }
    table[16..19].copy_from_slice(&probe[0].normal.to_array());
    table
}

/// All six tables in a buffer of [`DIR_STRIDE`] slots.
pub(crate) fn dir_tables() -> Vec<u8> {
    let stride = DIR_STRIDE as usize;
    let mut bytes = vec![0u8; stride * 6];
    for (i, dir) in Dir::ALL.into_iter().enumerate() {
        let table = dir_table(dir);
        let at = i * stride;
        bytes[at..at + 80].copy_from_slice(bytemuck::cast_slice(&table));
    }
    bytes
}

/// The face winding of the mesher table relative to the elements': true when the quad
/// `(v0, v1, v2)` seen from outside runs the same way as the element triangles.
pub(crate) fn winding_matches_elements() -> bool {
    Dir::ALL.into_iter().all(|dir| {
        let q = expand_face(VoxelFace::new([0; 3], [0; 3], 0, [0; 4]), dir, 1.0);
        let n = (q[1].pos - q[0].pos).cross(q[2].pos - q[0].pos);
        n.dot(q[0].normal) > 0.0
    })
}

/// The voxel pipeline.
pub(crate) fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    samples: u32,
) -> wgpu::RenderPipeline {
    let front = if winding_matches_elements() {
        crate::elements::FRONT
    } else {
        wgpu::FrontFace::Ccw
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("voxels"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: FACE_BYTES,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Uint32x2,
                    offset: 0,
                    shader_location: 0,
                }],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR,
                blend: Some(BLEND),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            front_face: front,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: Some(depth_state(true)),
        multisample: wgpu::MultisampleState {
            count: samples,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mesher_winding_matches_the_element_winding() {
        assert!(winding_matches_elements());
    }

    #[test]
    fn corner_offsets_come_from_the_expansion() {
        // -Y: v0 is (x, y, -z - 1) and v1 (x + 1, y, -z - 1): one cell toward internal -Z.
        let table = dir_table(Dir::NegY);
        assert_eq!(&table[0..3], &[0.0, 0.0, -1.0]);
        assert_eq!(&table[4..7], &[1.0, 0.0, -1.0]);
        assert_eq!(&table[16..19], &[0.0, -1.0, 0.0]);
    }

    #[test]
    fn touching_ranges_merge_and_gaps_and_overflows_are_handled() {
        let range = |first, count| ChunkRange {
            slot: 0,
            first,
            count,
        };
        let ranges = [range(10, 5), range(0, 10), range(100, 4), range(4090, 20)];
        assert_eq!(merge_ranges(&ranges, 4096), [(0, 15), (100, 4), (4090, 6)]);
        assert!(merge_ranges(&[range(5000, 1)], 4096).is_empty());
    }
}
