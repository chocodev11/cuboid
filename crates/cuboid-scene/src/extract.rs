//! Render extracts drained from the model and the voxel world (spec: app 8; arch 13.1).
//!
//! The renderer does not read the document; it consumes what [`Extractor::extract`] returns. The
//! records below are the contract between the two. The extractor compares each object with the
//! one it saw last time, so an unchanged model produces an empty extract, an edit rewrites one
//! record, and a voxel edit meshes only the dirty chunks.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use cuboid_image::Image;
use cuboid_model::{
    Document, Element, ElementFlags, Mesh, MeshGeometry, MeshKind, ObjId, ObjRef, Pose,
    RenderPrefs, Submesh,
};
use cuboid_num::pack::to_byte;
use cuboid_voxel::{VoxelFace, VoxelWorld, mesh_chunk};
use fixedbitset::FixedBitSet;
use glam::{Affine3A, Mat4, Vec3};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::atlas_layout::AtlasLayout;
use crate::drawlist::{ChunkInfo, ElemInfo, MeshInfo, SceneCache};
use crate::frame::{Frame, geometry_bounds};
use crate::geom::{Aabb, to_u32};

/// Words of one element instance record (176 bytes). The layout, in words:
///
/// | words | content |
/// |---|---|
/// | 0-2 | position `p` (f32 bits) |
/// | 3 | flags: bits 0-3 view mask (bit per `ViewId::index`; 0 for a hidden element that is not drawn), bit 4 hover highlight, bit 5 inverted normals, bit 6 hidden element drawn as a faint box, bit 7 shade off (Minecraft `shade` false: no face light in the Minecraft shading modes) |
/// | 4-6 | size `s` (f32 bits) |
/// | 7 | the element's slot in the list |
/// | 8-11 | rotation quaternion `x, y, z, w` (f32 bits) |
/// | 12-35 | per face `tl.x, tl.y, br.x, br.y` in page space `0..1`, clamped to the atlas entry |
/// | 36-41 | per face vertex colour, `r | g << 8 | b << 16 | a << 24` |
/// | 42-43 | per face state byte: bit 0 active, bits 1-2 UV rotation in quarter turns, bits 3-7 atlas page; faces 0-3 in word 42, faces 4-5 in the low half of word 43 |
///
/// Face order is the slot order down, up, north, south, west, east.
///
/// spec: arch 13.3
pub const INSTANCE_WORDS: usize = 44;

/// The GPU record of one element.
///
/// spec: arch 13.3
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ElementInstance(pub [u32; INSTANCE_WORDS]);

/// A new or changed instance record. An edit rewrites exactly one record.
///
/// spec: arch 13.3, arch 13.1
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct InstanceUpdate {
    /// Position in the instance buffer (list order).
    pub slot: u32,
    /// The record.
    pub instance: ElementInstance,
}

/// A dirty rectangle of an atlas page to upload.
///
/// spec: arch 13.6, tools 5.2
#[derive(Clone, Debug)]
pub struct AtlasUpdate {
    /// Atlas page.
    pub page: u32,
    /// Left texel of the rectangle.
    pub x: u32,
    /// Top texel of the rectangle (page row order).
    pub y: u32,
    /// Width in texels.
    pub w: u32,
    /// Height in texels.
    pub h: u32,
    /// `w * h * 4` bytes, RGBA8.
    pub rgba: Arc<[u8]>,
}

/// A finished chunk mesh: the face records per direction, ready for the pools. A newer mesh of
/// a slot replaces an older one: a mesh whose `rev` is older than one already applied for the
/// slot is stale and dropped. A mesh without faces empties the slot.
///
/// spec: arch 13.5 (Jobs)
#[derive(Clone, Debug)]
pub struct ChunkMesh {
    /// Slot of the chunk in the world's table.
    pub slot: usize,
    /// The world revision the mesh was built from; it grows with every mesh of a slot.
    pub rev: cuboid_voxel::Rev,
    /// Where the records of each direction go in its pool, in records. The scene owns the pool
    /// layout, so a draw list's `ChunkRange.first` is valid without asking the renderer.
    pub offsets: [u32; 6],
    /// Face records per direction in pool order.
    pub faces: [Vec<VoxelFace>; 6],
}

/// A new or changed mesh object's geometry and texture.
///
/// spec: arch 13.6
#[derive(Clone, Debug)]
pub struct MeshUpdate {
    /// The mesh object.
    pub id: ObjId,
    /// Its geometry, shared with the document.
    pub geometry: Arc<MeshGeometry>,
    /// Its main texture, shared with the document.
    pub texture: Option<Arc<Image>>,
    /// Local to world: scale by the size, rotate, move to the position.
    pub model: Mat4,
    /// The material colour that multiplies the texture, RGBA in `0..1`.
    pub tint: [f32; 4],
    /// The mesh is hidden and must not be drawn.
    pub hidden: bool,
    /// The materials of an imported mesh.
    pub submeshes: Arc<[Submesh]>,
}

/// Everything the renderer must upload for one frame, within the per-frame budget (leftovers
/// carry over).
///
/// spec: arch 13.1 (step 1), arch 6 (cuboid-scene)
#[derive(Clone, Debug, Default)]
pub struct RenderExtract {
    /// Number of live element instances after the updates.
    pub instance_count: u32,
    /// Changed instance records.
    pub instances: Vec<InstanceUpdate>,
    /// Changed atlas rectangles.
    pub atlas: Vec<AtlasUpdate>,
    /// Number of atlas pages after the updates; pages are square.
    pub atlas_pages: u32,
    /// Texels per atlas page edge.
    pub atlas_page_size: u32,
    /// Finished chunk meshes in slot order.
    pub chunks: Vec<ChunkMesh>,
    /// Capacity of each voxel pool in records after the updates; a pool that grew keeps its old
    /// content.
    pub voxel_pools: [u32; 6],
    /// Changed mesh objects.
    pub meshes: Vec<MeshUpdate>,
    /// Mesh objects that are gone.
    pub removed_meshes: Vec<ObjId>,
}

impl RenderExtract {
    /// Whether the extract changes nothing.
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
            && self.atlas.is_empty()
            && self.chunks.is_empty()
            && self.meshes.is_empty()
            && self.removed_meshes.is_empty()
    }
}

/// How much one [`Extractor::extract`] call may do. Leftovers carry over to the next call.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ExtractBudget {
    /// Texture bytes per call; at least one rectangle always goes.
    pub atlas_bytes: usize,
    /// Chunks meshed per call; at least one batch always goes.
    pub chunks: usize,
    /// Milliseconds of meshing per call, checked between batches.
    pub millis: f32,
}

impl Default for ExtractBudget {
    /// 8 MiB of texture, 512 chunks, 4 ms.
    fn default() -> Self {
        Self {
            atlas_bytes: 8 * 1024 * 1024,
            chunks: 512,
            millis: 4.0,
        }
    }
}

impl ExtractBudget {
    /// No limit: everything dirty is done in one call. Tests and PNG renders use it.
    pub const UNLIMITED: ExtractBudget = ExtractBudget {
        atlas_bytes: usize::MAX,
        chunks: usize::MAX,
        millis: f32::INFINITY,
    };
}

/// Chunks meshed together on the worker pool.
const BATCH: usize = 32;

/// The tint colours of tint indices 0 to 5 and the colour of any other index.
const TINTS: [[u8; 3]; 7] = [
    [106, 170, 64],
    [255, 105, 180],
    [100, 149, 237],
    [255, 215, 0],
    [210, 105, 30],
    [72, 209, 204],
    [147, 112, 219],
];

/// One element as the extractor remembers it.
struct Seen {
    arc: Arc<Element>,
    instance: ElementInstance,
    hover: bool,
    /// Hidden by its own flag or by a group above it.
    hidden: bool,
    /// The record holds the placement of a pose, not the one the element has at rest.
    posed: bool,
}

/// The place of an element in the pose when its group has none.
const NO_POSE: u32 = u32::MAX;

/// A pool of face records of one direction: capacity and free blocks.
#[derive(Default)]
struct Pool {
    capacity: u32,
    /// Free blocks `(start, len)` ordered by start, never touching.
    free: Vec<(u32, u32)>,
}

impl Pool {
    fn alloc(&mut self, len: u32) -> u32 {
        if len == 0 {
            return 0;
        }
        loop {
            if let Some(i) = self.free.iter().position(|&(_, l)| l >= len) {
                let (start, l) = self.free[i];
                if l == len {
                    self.free.remove(i);
                } else {
                    self.free[i] = (start + len, l - len);
                }
                return start;
            }
            // Grow: double, or just enough, and open the new tail as a free block.
            let grown = self
                .capacity
                .saturating_mul(2)
                .max(self.capacity.saturating_add(len))
                .max(1024);
            let tail = (self.capacity, grown - self.capacity);
            self.capacity = grown;
            self.release(tail.0, tail.1);
        }
    }

    fn release(&mut self, start: u32, len: u32) {
        if len == 0 {
            return;
        }
        let at = self.free.partition_point(|&(s, _)| s < start);
        self.free.insert(at, (start, len));
        // Merge with the next block, then with the previous one.
        if at + 1 < self.free.len() && self.free[at].0 + self.free[at].1 == self.free[at + 1].0 {
            self.free[at].1 += self.free[at + 1].1;
            self.free.remove(at + 1);
        }
        if at > 0 && self.free[at - 1].0 + self.free[at - 1].1 == self.free[at].0 {
            self.free[at - 1].1 += self.free[at].1;
            self.free.remove(at);
        }
    }
}

/// Where a chunk's records sit in the pools: start and block length per direction.
#[derive(Copy, Clone, Default)]
struct ChunkBlocks {
    start: [u32; 6],
    block: [u32; 6],
}

/// Turns the document into [`RenderExtract`]s, remembering what the renderer already has.
pub struct Extractor {
    atlas: AtlasLayout,
    atlas_seen: u64,
    pending_atlas: VecDeque<crate::extract::AtlasUpdate>,
    elements: Vec<Seen>,
    elements_rev: Option<cuboid_model::Rev>,
    /// The groups at the last call: a hidden group hides what it holds.
    groups_rev: Option<cuboid_model::Rev>,
    prefs_key: Option<u8>,
    hover_seen: Option<ObjId>,
    /// The meshes the renderer has, with whether each one is hidden.
    meshes: FxHashMap<ObjId, (Arc<Mesh>, bool, Mat4)>,
    meshes_rev: Option<cuboid_model::Rev>,
    mesh_groups_rev: Option<cuboid_model::Rev>,
    /// The pose being shown and whether there is one; see [`Extractor::set_pose`].
    pose: Pose,
    pose_on: bool,
    /// Changes with every pose set; the elements and the meshes remember the one they saw.
    pose_rev: u64,
    elements_pose_seen: u64,
    meshes_pose_seen: u64,
    /// The place in the pose of the group of each element, in list order.
    elem_pose: Vec<u32>,
    elem_pose_key: Option<(cuboid_model::Rev, cuboid_model::Rev, usize)>,
    pools: [Pool; 6],
    blocks: FxHashMap<usize, ChunkBlocks>,
    pending_chunks: FixedBitSet,
    world_shape: Option<(u32, usize)>,
    cache: SceneCache,
}

impl Default for Extractor {
    fn default() -> Self {
        Self::new(AtlasLayout::default())
    }
}

impl std::fmt::Debug for Extractor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Extractor")
            .field("elements", &self.elements.len())
            .field("meshes", &self.meshes.len())
            .field("chunks", &self.blocks.len())
            .finish_non_exhaustive()
    }
}

fn prefs_key(p: &RenderPrefs) -> u8 {
    u8::from(p.use_elements_color)
        | u8::from(p.emulate_tint_colors) << 1
        | u8::from(p.hidden_as_box) << 2
        | u8::from(p.hidden_in_main_view) << 3
}

impl Extractor {
    /// An extractor over an atlas layout, which fixes the page size.
    pub fn new(atlas: AtlasLayout) -> Self {
        Self {
            atlas,
            atlas_seen: u64::MAX,
            pending_atlas: VecDeque::new(),
            elements: Vec::new(),
            elements_rev: None,
            groups_rev: None,
            prefs_key: None,
            hover_seen: None,
            meshes: FxHashMap::default(),
            meshes_rev: None,
            mesh_groups_rev: None,
            pose: Pose::new(),
            pose_on: false,
            pose_rev: 0,
            elements_pose_seen: 0,
            meshes_pose_seen: 0,
            elem_pose: Vec::new(),
            elem_pose_key: None,
            pools: Default::default(),
            blocks: FxHashMap::default(),
            pending_chunks: FixedBitSet::new(),
            world_shape: None,
            cache: SceneCache::default(),
        }
    }

    /// The scene-side caches the draw lists are compiled from.
    pub fn cache(&self) -> &SceneCache {
        &self.cache
    }

    /// The scene-side caches, mutable, for the overlay state.
    pub fn cache_mut(&mut self) -> &mut SceneCache {
        &mut self.cache
    }

    /// Shows the pose of an animation: the elements and meshes of the groups that move are drawn
    /// where the pose carries them, and nothing in the document changes. `None`, or a pose in which
    /// no group moves, shows the rest pose again. The next [`Extractor::extract`] writes only the
    /// records that differ from the ones the renderer has; picking and the gizmo do not read the
    /// pose.
    ///
    /// spec: animation A2 6
    pub fn set_pose(&mut self, pose: Option<&Pose>) {
        match pose.filter(|p| !p.is_rest()) {
            Some(pose) => {
                self.pose.copy_from(pose);
                self.pose_on = true;
                self.pose_rev += 1;
            }
            None if self.pose_on => {
                self.pose_on = false;
                self.pose.clear();
                self.pose_rev += 1;
            }
            None => {}
        }
    }

    /// Sets the texture tick that animated textures show. The next [`Extractor::extract`] sends
    /// the rectangle of each texture whose picture the tick changes, and nothing else.
    ///
    /// spec: textures T1 4
    pub fn set_texture_tick(&mut self, tick: u32) {
        self.atlas.set_tick(tick);
    }

    /// The motion of the group of the element at `slot`, when a pose is shown and moves it.
    fn delta_at(&self, slot: usize) -> Option<Affine3A> {
        if !self.pose_on {
            return None;
        }
        let at = *self.elem_pose.get(slot)?;
        if at == NO_POSE {
            return None;
        }
        self.pose.delta_at(at as usize).copied()
    }

    /// Finds the place in the pose of the group of every element, when the elements, the groups or
    /// the rig of the pose changed.
    fn refresh_pose_map(&mut self, doc: &Document) {
        let key = (
            doc.elements.structure_rev(),
            doc.groups.rev(),
            self.pose.shape(),
        );
        if self.elem_pose_key == Some(key) {
            return;
        }
        self.elem_pose_key = Some(key);
        let pose = &self.pose;
        self.elem_pose.clear();
        self.elem_pose.extend(doc.elements.ids().iter().map(|&id| {
            doc.groups
                .parent_of(ObjRef::Element(id))
                .and_then(|group| pose.index_of(group))
                .map_or(NO_POSE, to_u32)
        }));
    }

    /// Puts the element at `slot` where `delta` carries it, or at rest for `None`, and writes its
    /// record when that differs from the record the renderer has. Returns whether the scene
    /// changed.
    fn place_element(
        &mut self,
        slot: usize,
        delta: Option<&Affine3A>,
        out: &mut RenderExtract,
    ) -> bool {
        let Some(seen) = self.elements.get(slot) else {
            return false;
        };
        let (Some(rest), Some(info)) = (Frame::of(&seen.arc.base), self.cache.info(slot).copied())
        else {
            return false;
        };
        let (mut instance, mut next) = (seen.instance, info);
        pose_record(
            &mut instance,
            &mut next,
            delta.map_or(rest, |d| rest.posed(d)),
        );
        let moved = instance != seen.instance;
        if let Some(seen) = self.elements.get_mut(slot) {
            seen.posed = delta.is_some();
            seen.instance = instance;
        }
        if !moved && next == info {
            return false;
        }
        self.cache.set_element(slot, instance, next);
        if moved {
            out.instances.push(InstanceUpdate {
                slot: to_u32(slot),
                instance,
            });
        }
        true
    }

    /// The runtime atlas layout.
    pub fn atlas(&self) -> &AtlasLayout {
        &self.atlas
    }

    /// Whether dirty chunks or atlas rectangles are waiting for a later call.
    pub fn has_leftovers(&self) -> bool {
        self.pending_chunks.count_ones(..) > 0 || !self.pending_atlas.is_empty()
    }

    /// Drains the changes since the last call into an extract. The document is only read, except
    /// that the dirty set of the voxel world is taken. Pass [`ExtractBudget::UNLIMITED`] to
    /// finish everything in one call.
    pub fn extract(
        &mut self,
        doc: &mut Document,
        prefs: &RenderPrefs,
        budget: &ExtractBudget,
    ) -> RenderExtract {
        let mut out = RenderExtract::default();
        let mut changed = self.extract_atlas(doc, budget, &mut out);
        changed |= self.extract_elements(doc, prefs, &mut out);
        changed |= self.extract_meshes(doc, &mut out);
        changed |= self.extract_voxels(doc, budget, &mut out);
        out.atlas_pages = self.atlas.pages();
        out.atlas_page_size = self.atlas.page_size();
        out.voxel_pools = std::array::from_fn(|d| self.pools[d].capacity);
        let blueprint = if doc.blueprint.base.hidden {
            None
        } else {
            Frame::of(&doc.blueprint.base).map(|f| f.model_matrix())
        };
        changed |= self
            .cache
            .set_scene(blueprint, doc.model_box, doc.props.general.origin);
        if changed {
            self.cache.bump();
        }
        out
    }

    fn extract_atlas(
        &mut self,
        doc: &Document,
        budget: &ExtractBudget,
        out: &mut RenderExtract,
    ) -> bool {
        let mut fresh = Vec::new();
        self.atlas.sync(&doc.textures, &mut fresh);
        self.pending_atlas.extend(fresh);
        let mut bytes = 0_usize;
        while let Some(front) = self.pending_atlas.front() {
            let size = front.rgba.len();
            if bytes > 0 && bytes.saturating_add(size) > budget.atlas_bytes {
                break;
            }
            bytes = bytes.saturating_add(size);
            out.atlas.extend(self.pending_atlas.pop_front());
        }
        !out.atlas.is_empty()
    }

    fn extract_elements(
        &mut self,
        doc: &Document,
        prefs: &RenderPrefs,
        out: &mut RenderExtract,
    ) -> bool {
        let list = &doc.elements;
        let key = prefs_key(prefs);
        let generation = self.atlas.generation();
        let hovered = self.cache.hovered_element();
        let pose_changed = self.elements_pose_seen != self.pose_rev;
        let stale = self.elements_rev != Some(list.rev())
            || self.groups_rev != Some(doc.groups.rev())
            || self.prefs_key != Some(key)
            || self.atlas_seen != generation
            || self.hover_seen != hovered
            || pose_changed;
        out.instance_count = to_u32(list.len());
        if !stale {
            return false;
        }
        if self.pose_on {
            self.refresh_pose_map(doc);
        }
        let force = self.prefs_key != Some(key) || self.atlas_seen != generation;
        let structure_changed = self.elements.len() != list.len()
            || self.elements_rev.is_none()
            || self.cache.elem_ids != list.ids();
        let mut changed = structure_changed;
        for (i, arc) in list.items().iter().enumerate() {
            let id = list.ids()[i];
            let hover = hovered == Some(id);
            let hidden = arc.base.hidden || doc.groups.hidden_by_group(ObjRef::Element(id));
            let delta = self.delta_at(i);
            if let Some(seen) = self.elements.get(i)
                && !force
                && Arc::ptr_eq(&seen.arc, arc)
                && seen.hover == hover
                && seen.hidden == hidden
                && self.cache.elem_ids.get(i) == Some(&id)
            {
                // Only the pose can differ, and only for an element that is or was placed by it.
                if pose_changed && (delta.is_some() || seen.posed) {
                    changed |= self.place_element(i, delta.as_ref(), out);
                }
                continue;
            }
            let (mut instance, mut info) =
                build_instance(arc, to_u32(i), hover, hidden, &self.atlas, prefs);
            if let (Some(delta), Some(rest)) = (delta, info.frame) {
                pose_record(&mut instance, &mut info, rest.posed(&delta));
            }
            let same = self.elements.get(i).is_some_and(|s| s.instance == instance);
            if !same {
                out.instances.push(InstanceUpdate {
                    slot: to_u32(i),
                    instance,
                });
                changed = true;
            }
            // The draw lists read more than the record: the alpha class decides the queue.
            changed |= self.cache.info(i) != Some(&info);
            let seen = Seen {
                arc: Arc::clone(arc),
                instance,
                hover,
                hidden,
                posed: delta.is_some(),
            };
            if i < self.elements.len() {
                self.elements[i] = seen;
                self.cache.set_element(i, instance, info);
            } else {
                self.elements.push(seen);
                self.cache.push_element(instance, info);
            }
        }
        self.elements.truncate(list.len());
        self.cache.truncate_elements(list.len());
        if structure_changed {
            self.cache.set_element_ids(list.ids());
        }
        self.elements_rev = Some(list.rev());
        self.groups_rev = Some(doc.groups.rev());
        self.prefs_key = Some(key);
        self.atlas_seen = generation;
        self.hover_seen = hovered;
        self.elements_pose_seen = self.pose_rev;
        changed
    }

    /// The motion of the group of a mesh, when a pose is shown and moves it.
    fn mesh_delta(&self, doc: &Document, id: ObjId) -> Option<Affine3A> {
        if !self.pose_on {
            return None;
        }
        let group = doc.groups.parent_of(ObjRef::Mesh(id))?;
        self.pose.delta_of(group).copied()
    }

    /// Puts the meshes where the pose carries them (or at rest) when nothing else about them
    /// changed: only a mesh whose model differs from the one the renderer has is sent again.
    fn repose_meshes(&mut self, doc: &Document, out: &mut RenderExtract) -> bool {
        let mut changed = false;
        let list = &doc.meshes;
        for (slot, (id, arc)) in list.ids().iter().zip(list.items()).enumerate() {
            let Some(frame) = Frame::of(&arc.base).map(Frame::unsigned) else {
                continue;
            };
            let delta = self.mesh_delta(doc, *id);
            let model = delta.map_or(frame.model_matrix(), |d| {
                Mat4::from(d) * frame.model_matrix()
            });
            let Some((_, hidden, held)) = self.meshes.get_mut(id) else {
                continue;
            };
            if *held == model {
                continue;
            }
            *held = model;
            let hidden = *hidden;
            changed = true;
            out.meshes.push(MeshUpdate {
                id: *id,
                geometry: Arc::clone(&arc.geometry),
                texture: arc.texture.as_ref().map(|t| Arc::clone(&t.image)),
                model,
                tint: arc.tint,
                hidden,
                submeshes: Arc::clone(&arc.submeshes),
            });
            if let Some(info) = self.cache.meshes.get_mut(slot) {
                info.model = model;
                info.bounds = mesh_bounds(&frame, &arc.geometry, delta);
            }
        }
        changed
    }

    fn extract_meshes(&mut self, doc: &Document, out: &mut RenderExtract) -> bool {
        let list = &doc.meshes;
        let pose_changed = self.meshes_pose_seen != self.pose_rev;
        if self.meshes_rev == Some(list.rev()) && self.mesh_groups_rev == Some(doc.groups.rev()) {
            if !pose_changed {
                return false;
            }
            self.meshes_pose_seen = self.pose_rev;
            return self.repose_meshes(doc, out);
        }
        self.meshes_rev = Some(list.rev());
        self.mesh_groups_rev = Some(doc.groups.rev());
        self.meshes_pose_seen = self.pose_rev;
        let mut changed = false;
        let mut infos = Vec::with_capacity(list.len());
        let mut live: FxHashSet<ObjId> = FxHashSet::default();
        for (id, arc) in list.ids().iter().zip(list.items()) {
            live.insert(*id);
            let frame = Frame::of(&arc.base).map(Frame::unsigned);
            // Where a pose carries the mesh, if it moves with a group.
            let delta = self.mesh_delta(doc, *id);
            let rest = frame.map_or(Mat4::IDENTITY, |f| f.model_matrix());
            let model = delta.map_or(rest, |d| Mat4::from(d) * rest);
            // Hidden by its own flag or by a group above it.
            let hidden = arc.base.hidden || doc.groups.hidden_by_group(ObjRef::Mesh(*id));
            let unchanged = self
                .meshes
                .get(id)
                .is_some_and(|(old, was_hidden, old_model)| {
                    Arc::ptr_eq(old, arc) && *was_hidden == hidden && *old_model == model
                });
            if !unchanged {
                out.meshes.push(MeshUpdate {
                    id: *id,
                    geometry: Arc::clone(&arc.geometry),
                    texture: arc.texture.as_ref().map(|t| Arc::clone(&t.image)),
                    model,
                    tint: arc.tint,
                    hidden,
                    submeshes: Arc::clone(&arc.submeshes),
                });
                self.meshes.insert(*id, (Arc::clone(arc), hidden, model));
                changed = true;
            }
            let bounds = frame.map_or(Aabb::EMPTY, |f| mesh_bounds(&f, &arc.geometry, delta));
            infos.push(MeshInfo {
                id: *id,
                bounds,
                hidden,
                translucent: mesh_translucent(arc),
                submeshes: to_u32(arc.submeshes.len().max(1)),
                model,
                primitive: arc.kind != MeshKind::Custom,
            });
        }
        let gone: Vec<ObjId> = self
            .meshes
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in gone {
            self.meshes.remove(&id);
            out.removed_meshes.push(id);
            changed = true;
        }
        self.cache.meshes = infos;
        changed
    }

    fn extract_voxels(
        &mut self,
        doc: &mut Document,
        budget: &ExtractBudget,
        out: &mut RenderExtract,
    ) -> bool {
        let Some(world) = doc.voxels.as_mut() else {
            if self.world_shape.take().is_none() {
                return false;
            }
            self.clear_voxels(out);
            return true;
        };
        let shape = (world.res, world.slots().len());
        if self.world_shape != Some(shape) {
            // A new world: forget the old chunks and mesh everything.
            if self.world_shape.is_some() {
                self.clear_voxels(out);
            }
            self.world_shape = Some(shape);
            self.pending_chunks = FixedBitSet::with_capacity(shape.1);
            self.pending_chunks.insert_range(..);
            world.take_dirty();
        } else {
            self.pending_chunks.union_with(&world.take_dirty());
        }
        // A slot that never held cells and has nothing at the renderer needs no mesh.
        let idle: Vec<usize> = self
            .pending_chunks
            .ones()
            .filter(|slot| {
                !matches!(
                    world.slots().get(*slot),
                    Some(cuboid_voxel::ChunkSlot::Data(_))
                ) && !self.blocks.contains_key(slot)
            })
            .collect();
        for slot in idle {
            self.pending_chunks.set(slot, false);
        }
        if self.pending_chunks.count_ones(..) == 0 {
            return false;
        }
        let started = Instant::now();
        let mut done = 0_usize;
        let world: &VoxelWorld = world;
        while done < budget.chunks {
            let batch: Vec<usize> = self
                .pending_chunks
                .ones()
                .take(BATCH.min(budget.chunks - done))
                .collect();
            if batch.is_empty() {
                break;
            }
            let meshed: Vec<[Vec<VoxelFace>; 6]> = batch
                .par_iter()
                .map(|&slot| {
                    let mut faces: [Vec<VoxelFace>; 6] = Default::default();
                    mesh_chunk(world, slot, &mut faces);
                    faces
                })
                .collect();
            for (&slot, faces) in batch.iter().zip(meshed) {
                self.pending_chunks.set(slot, false);
                self.place_chunk(world, slot, faces, out);
            }
            done += batch.len();
            if started.elapsed().as_secs_f32() * 1000.0 >= budget.millis {
                break;
            }
        }
        out.chunks.sort_unstable_by_key(|c| c.slot);
        true
    }

    /// Gives a meshed chunk its pool blocks and queues it for the renderer.
    fn place_chunk(
        &mut self,
        world: &VoxelWorld,
        slot: usize,
        faces: [Vec<VoxelFace>; 6],
        out: &mut RenderExtract,
    ) {
        let had = self.blocks.get(&slot).copied();
        let lens: [u32; 6] = std::array::from_fn(|d| to_u32(faces[d].len()));
        if had.is_none() && lens.iter().all(|&l| l == 0) {
            // Nothing there before, nothing now.
            return;
        }
        let mut blocks = had.unwrap_or_default();
        for (d, pool) in self.pools.iter_mut().enumerate() {
            // Keep the block while the records fit and are not far smaller; else move.
            let fits = lens[d] <= blocks.block[d] && lens[d].saturating_mul(2) >= blocks.block[d];
            if !fits {
                pool.release(blocks.start[d], blocks.block[d]);
                blocks.start[d] = pool.alloc(lens[d]);
                blocks.block[d] = lens[d];
            }
        }
        out.chunks.push(ChunkMesh {
            slot,
            rev: world.rev,
            offsets: blocks.start,
            faces,
        });
        if lens.iter().all(|&l| l == 0) {
            for (d, pool) in self.pools.iter_mut().enumerate() {
                pool.release(blocks.start[d], blocks.block[d]);
            }
            self.blocks.remove(&slot);
            self.cache.chunks.remove(&to_u32(slot));
        } else {
            self.blocks.insert(slot, blocks);
            self.cache.chunks.insert(
                to_u32(slot),
                ChunkInfo {
                    bounds: chunk_bounds(world, slot),
                    first: blocks.start,
                    count: lens,
                },
            );
        }
    }

    /// Empties every chunk the renderer holds and resets the pools.
    fn clear_voxels(&mut self, out: &mut RenderExtract) {
        let rev = cuboid_voxel::Rev::next();
        let mut slots: Vec<usize> = self.blocks.keys().copied().collect();
        slots.sort_unstable();
        let cleared: FxHashSet<usize> = slots.iter().copied().collect();
        out.chunks.retain(|c| !cleared.contains(&c.slot));
        for slot in slots {
            out.chunks.push(ChunkMesh {
                slot,
                rev,
                offsets: [0; 6],
                faces: Default::default(),
            });
        }
        self.blocks.clear();
        self.pools = Default::default();
        self.pending_chunks = FixedBitSet::new();
        self.cache.chunks = BTreeMap::new();
    }
}

/// The world box of a chunk slot.
fn chunk_bounds(world: &VoxelWorld, slot: usize) -> Aabb {
    let n = world.chunks_per_axis() as usize;
    let (cx, cy, cz) = (slot / (n * n), slot / n % n, slot % n);
    let c = 16.0 / world.res as f32;
    let edge = 16.0 * c;
    let origin = Vec3::new(
        cx as f32 * edge - 16.0,
        cy as f32 * edge - 16.0,
        16.0 - cz as f32 * edge,
    );
    Aabb {
        min: origin - Vec3::new(0.0, 0.0, edge),
        max: origin + Vec3::new(edge, edge, 0.0),
    }
}

/// Whether a chunk slot holds cells; used by tests of the world shape.
#[cfg(test)]
fn slot_has_cells(world: &VoxelWorld, slot: usize) -> bool {
    matches!(
        world.slots().get(slot),
        Some(cuboid_voxel::ChunkSlot::Data(_))
    )
}

/// The vertex colour of a face.
///
/// spec: app 8.4, tools 4.3
fn face_color(element: &Element, tint: i32, prefs: &RenderPrefs) -> [u8; 4] {
    let c = element.color;
    let own = if prefs.use_elements_color {
        [to_byte(c[0]), to_byte(c[1]), to_byte(c[2]), to_byte(c[3])]
    } else {
        [255, 255, 255, to_byte(c[3])]
    };
    if !prefs.emulate_tint_colors {
        return own;
    }
    match tint {
        -1 if prefs.use_elements_color => own,
        -1 => [255, 255, 255, 255],
        i => {
            let [r, g, b] = usize::try_from(i)
                .ok()
                .and_then(|i| TINTS.get(i))
                .copied()
                .unwrap_or(TINTS[6]);
            [r, g, b, 255]
        }
    }
}

/// The instance record of an element and what the draw lists need to know about it.
fn build_instance(
    arc: &Arc<Element>,
    slot: u32,
    hover: bool,
    hidden: bool,
    atlas: &AtlasLayout,
    prefs: &RenderPrefs,
) -> (ElementInstance, ElemInfo) {
    let el: &Element = arc;
    let frame = Frame::of(&el.base);
    let mut w = [0_u32; INSTANCE_WORDS];
    let bits = |v: f32| v.to_bits();

    let base = &el.base;
    // A hidden element (by its flag or by a group above it) is drawn normally in the Main view,
    // or as a faint box in all views, or not at all (spec: app 8.9).
    let (mask, faint) = match (hidden, prefs.hidden_in_main_view, prefs.hidden_as_box) {
        (false, ..) => (0b1111_u32, false),
        (true, true, _) => (0b1000, false),
        (true, false, true) => (0b1111, true),
        (true, false, false) => (0, false),
    };
    let rot = frame.map_or(glam::Quat::IDENTITY, |f| f.rot);
    let size = frame.map_or(Vec3::ZERO, |f| f.size);
    w[0..3].copy_from_slice(&[bits(base.p.x), bits(base.p.y), bits(base.p.z)]);
    w[3] = mask
        | u32::from(hover) << 4
        | u32::from(el.flags.contains(ElementFlags::INVERT_NORMALS)) << 5
        | u32::from(faint) << 6
        | u32::from(!el.flags.contains(ElementFlags::SHADE)) << 7;
    w[7] = slot;
    w[4..7].copy_from_slice(&[bits(size.x), bits(size.y), bits(size.z)]);
    w[8..12].copy_from_slice(&[bits(rot.x), bits(rot.y), bits(rot.z), bits(rot.w)]);

    let mut translucent = false;
    let mut state = [0_u32; 6];
    for (i, face) in el.faces.iter().enumerate() {
        let rect = atlas.rect(&face.texture);
        let ps = atlas.page_size() as f32;
        let (x0, y0) = (rect.x as f32 / ps, rect.y as f32 / ps);
        let (dw, dh) = (rect.w as f32 / ps, rect.h as f32 / ps);
        let (tl, br) = (face.uv.tl(), face.uv.br());
        let map = |c: glam::Vec2| {
            [
                (x0 + c.x * dw).clamp(x0, x0 + dw),
                (y0 + c.y * dh).clamp(y0, y0 + dh),
            ]
        };
        let (a, b) = (map(tl), map(br));
        w[12 + 4 * i..16 + 4 * i].copy_from_slice(&[
            bits(a[0]),
            bits(a[1]),
            bits(b[0]),
            bits(b[1]),
        ]);
        let color = face_color(el, face.tint, prefs);
        w[36 + i] = u32::from_le_bytes(color);
        let quarter = u32::try_from((face.uv.rot() / 90).rem_euclid(4)).unwrap_or(0);
        state[i] = u32::from(face.active) | quarter << 1 | rect.page.min(31) << 3;
        if face.active {
            translucent |= color[3] < 255 || atlas.any_partial(&face.texture, tl, br);
        }
    }
    w[42] = state[0] | state[1] << 8 | state[2] << 16 | state[3] << 24;
    w[43] = state[4] | state[5] << 8;

    let bounds = match (frame, mask) {
        (Some(f), 1..) => f.obb().aabb(),
        _ => Aabb::EMPTY,
    };
    let info = ElemInfo {
        frame,
        bounds,
        // A faint box blends over the scene.
        translucent: translucent || faint,
        faint,
        view_mask: u8::try_from(mask).unwrap_or(0),
    };
    (ElementInstance(w), info)
}

/// The world box of the geometry of a mesh placed by `frame`, then carried by a pose.
fn mesh_bounds(frame: &Frame, geometry: &MeshGeometry, delta: Option<Affine3A>) -> Aabb {
    let b = geometry_bounds(geometry);
    (0..8).fold(Aabb::EMPTY, |acc, i| {
        let at = frame.to_world(Vec3::new(
            if i & 1 == 0 { b.min.x } else { b.max.x },
            if i & 2 == 0 { b.min.y } else { b.max.y },
            if i & 4 == 0 { b.min.z } else { b.max.z },
        ));
        acc.with_point(delta.map_or(at, |d| d.transform_point3(at)))
    })
}

/// Writes the position, the size and the rotation of a placement into a record.
fn write_frame(w: &mut [u32; INSTANCE_WORDS], f: &Frame) {
    let bits = f32::to_bits;
    w[0..3].copy_from_slice(&[bits(f.pos.x), bits(f.pos.y), bits(f.pos.z)]);
    w[4..7].copy_from_slice(&[bits(f.size.x), bits(f.size.y), bits(f.size.z)]);
    w[8..12].copy_from_slice(&[bits(f.rot.x), bits(f.rot.y), bits(f.rot.z), bits(f.rot.w)]);
}

/// Gives a record, and what the draw lists know of the element, another placement: the box of the
/// element in a pose, or at rest. An element that no view draws keeps its empty bounds.
fn pose_record(instance: &mut ElementInstance, info: &mut ElemInfo, frame: Frame) {
    write_frame(&mut instance.0, &frame);
    info.frame = Some(frame);
    if info.bounds.is_valid() {
        info.bounds = frame.obb().aabb();
    }
}

/// Whether a mesh needs the sorted translucent path.
fn mesh_translucent(mesh: &Mesh) -> bool {
    let partial = |img: &Image| img.alpha().any_partial(0, 0, img.w, img.h);
    mesh.tint[3] < 1.0
        || mesh.texture.as_ref().is_some_and(|t| partial(&t.image))
        || mesh
            .submeshes
            .iter()
            .any(|s| s.color[3] < 1.0 || s.texture.as_ref().is_some_and(|t| partial(&t.image)))
}

#[cfg(test)]
mod tests {
    use cuboid_image::RgbaBuf;
    use cuboid_model::{
        ElementType, FaceName, Label, MeshBounds, MeshKind, ObjectBase, Perms, Texture,
        TextureEntry, TextureList,
    };
    use cuboid_voxel::mapping::grid_box;

    use super::*;

    fn solid_image(value: [u8; 4]) -> Arc<Image> {
        let mut buf = RgbaBuf::new(16, 16);
        buf.pixels
            .chunks_exact_mut(4)
            .for_each(|p| p.copy_from_slice(&value));
        Arc::new(Image::from_rgba(buf))
    }

    fn doc(n: usize) -> Document {
        let mut textures = TextureList::default();
        textures.entries.push(TextureEntry {
            label: Label::from("texture"),
            texture: Texture::new(solid_image([255, 255, 255, 255])),
            animation: None,
        });
        let mut doc = Document::new(textures);
        for i in 0..n {
            let mut el = Element::new(
                ElementType::Cube,
                &doc.blueprint,
                1,
                [1.0, 0.5, 0.25, 1.0],
                &Label::from("texture"),
            );
            el.base.p = Vec3::new(i as f32 * 2.0, 0.0, 0.0);
            el.base.s = Vec3::ONE;
            doc.elements.push(el);
        }
        doc
    }

    fn run(ex: &mut Extractor, doc: &mut Document) -> RenderExtract {
        ex.extract(doc, &RenderPrefs::default(), &ExtractBudget::UNLIMITED)
    }

    #[test]
    fn the_first_extract_sends_everything_and_an_unchanged_model_sends_nothing() {
        let mut d = doc(5);
        let mut ex = Extractor::default();
        let first = run(&mut ex, &mut d);
        assert_eq!(first.instance_count, 5);
        assert_eq!(first.instances.len(), 5);
        assert!(!first.atlas.is_empty() && first.atlas_pages == 1);
        let slots: Vec<u32> = first.instances.iter().map(|u| u.slot).collect();
        assert_eq!(slots, [0, 1, 2, 3, 4]);
        for _ in 0..3 {
            let again = run(&mut ex, &mut d);
            assert!(again.is_empty(), "{again:?}");
            assert_eq!(again.instance_count, 5);
        }
    }

    #[test]
    fn an_edit_rewrites_exactly_one_record() {
        let mut d = doc(100);
        let mut ex = Extractor::default();
        run(&mut ex, &mut d);
        let id = d.elements.ids()[42];
        let mut j = cuboid_model::ChangeJournal::default();
        d.elements.edit(id, &mut j).unwrap().base.p = Vec3::new(9.0, 9.0, 9.0);
        let out = run(&mut ex, &mut d);
        assert_eq!(out.instances.len(), 1);
        assert_eq!(out.instances[0].slot, 42);
        assert_eq!(
            f32::from_bits(out.instances[0].instance.0[0]).to_bits(),
            9.0_f32.to_bits()
        );
        // An edit that leaves the record alone (a label) sends nothing.
        d.elements.edit(id, &mut j).unwrap().base.label = Label::from("renamed");
        assert!(run(&mut ex, &mut d).instances.is_empty());
    }

    #[test]
    fn adding_removing_and_reordering_update_the_affected_slots() {
        let mut d = doc(4);
        let mut ex = Extractor::default();
        run(&mut ex, &mut d);
        let mut el = (*d.elements.items()[0]).clone();
        el.base.p.y = 5.0;
        d.elements.push(el);
        let out = run(&mut ex, &mut d);
        assert_eq!((out.instance_count, out.instances.len()), (5, 1));
        assert_eq!(out.instances[0].slot, 4);
        let first = d.elements.ids()[0];
        d.elements.remove_set(&[first].into_iter().collect());
        let out = run(&mut ex, &mut d);
        assert_eq!(out.instance_count, 4);
        // Every record shifted down one slot, and the last slot is gone.
        assert_eq!(out.instances.len(), 4);
        let ids = d.elements.ids().to_vec();
        d.elements.stable_partition(|_, id, _| id == ids[3]);
        let out = run(&mut ex, &mut d);
        assert_eq!(out.instances.len(), 4);
        assert!(run(&mut ex, &mut d).is_empty());
    }

    #[test]
    fn the_record_follows_the_documented_layout() {
        let mut d = doc(1);
        let id = d.elements.ids()[0];
        let mut j = cuboid_model::ChangeJournal::default();
        {
            let el = d.elements.edit(id, &mut j).unwrap();
            el.base.p = Vec3::new(1.0, 2.0, 3.0);
            el.base.s = Vec3::new(4.0, 5.0, 6.0);
            el.faces[FaceName::West as usize].tint = 1;
            el.faces[FaceName::Up as usize].active = false;
            el.faces[FaceName::Down as usize].uv.set_rot(90);
        }
        let mut ex = Extractor::default();
        let w = run(&mut ex, &mut d).instances[0].instance.0;
        let f = |i: usize| f32::from_bits(w[i]);
        assert_eq!((f(0), f(1), f(2)), (1.0, 2.0, 3.0));
        assert_eq!((f(4), f(5), f(6)), (4.0, 5.0, 6.0));
        assert_eq!(w[3] & 0b1111, 0b1111);
        assert_eq!(w[7], 0, "an opaque texture is binary");
        assert!((f(11) - 1.0).abs() < 1e-6, "identity rotation");
        // Face colours: the element colour, the tint 1 pink on the west face.
        assert_eq!(
            w[36 + FaceName::North as usize].to_le_bytes(),
            [255, 127, 63, 255]
        );
        assert_eq!(
            w[36 + FaceName::West as usize].to_le_bytes(),
            [255, 105, 180, 255]
        );
        // State bytes: up is inactive; down has one quarter turn.
        let state = |face: usize| {
            if face < 4 {
                w[42] >> (8 * face) & 0xFF
            } else {
                w[43] >> (8 * (face - 4)) & 0xFF
            }
        };
        assert_eq!(state(FaceName::Up as usize) & 1, 0);
        assert_eq!(state(FaceName::North as usize) & 1, 1);
        assert_eq!(state(FaceName::Down as usize) >> 1 & 3, 1);
        // Texture coordinates stay inside the page and inside the entry.
        let rect = ex.atlas().rect("texture");
        let ps = ex.atlas().page_size() as f32;
        for face in 0..6 {
            let (u0, v0, u1, v1) = (
                f(12 + 4 * face),
                f(13 + 4 * face),
                f(14 + 4 * face),
                f(15 + 4 * face),
            );
            for (u, lo, hi) in [(u0, rect.x, rect.x + rect.w), (u1, rect.x, rect.x + rect.w)] {
                assert!(u >= lo as f32 / ps - 1e-6 && u <= hi as f32 / ps + 1e-6);
            }
            for v in [v0, v1] {
                assert!(
                    v >= rect.y as f32 / ps - 1e-6 && v <= (rect.y + rect.h) as f32 / ps + 1e-6
                );
            }
        }
    }

    #[test]
    fn preferences_recolour_without_touching_the_geometry_words() {
        let mut d = doc(3);
        let mut ex = Extractor::default();
        run(&mut ex, &mut d);
        let prefs = RenderPrefs {
            use_elements_color: false,
            ..RenderPrefs::default()
        };
        let out = ex.extract(&mut d, &prefs, &ExtractBudget::UNLIMITED);
        assert_eq!(out.instances.len(), 3);
        assert_eq!(
            out.instances[0].instance.0[36..42],
            [u32::from_le_bytes([255; 4]); 6]
        );
        let out = ex.extract(&mut d, &prefs, &ExtractBudget::UNLIMITED);
        assert!(out.is_empty());
    }

    #[test]
    fn hidden_elements_follow_the_preferences_of_app_8_9() {
        let mut d = doc(1);
        let id = d.elements.ids()[0];
        let mut j = cuboid_model::ChangeJournal::default();
        d.elements.edit(id, &mut j).unwrap().base.hidden = true;
        let mut ex = Extractor::default();
        // The view mask and the faint-box bit of the flags word.
        let flags = |out: &RenderExtract| {
            let w = out.instances[0].instance.0[3];
            (w & 0b1111, w >> 6 & 1)
        };
        let mut prefs = RenderPrefs::default();
        assert_eq!(
            flags(&ex.extract(&mut d, &prefs, &ExtractBudget::UNLIMITED)),
            (0, 0)
        );
        prefs.hidden_as_box = true;
        assert_eq!(
            flags(&ex.extract(&mut d, &prefs, &ExtractBudget::UNLIMITED)),
            (0b1111, 1)
        );
        prefs.hidden_in_main_view = true;
        assert_eq!(
            flags(&ex.extract(&mut d, &prefs, &ExtractBudget::UNLIMITED)),
            (0b1000, 0)
        );
    }

    #[test]
    fn a_hidden_group_hides_what_it_holds_and_showing_it_again_redraws() {
        use cuboid_model::{ChangeFlags, Group, GroupsBuilder, ObjRef};

        let mut d = doc(2);
        let ids = d.elements.ids().to_vec();
        let mut b = GroupsBuilder::new();
        let g = b.add_group(Group::new("g", Vec3::ZERO), None).unwrap();
        b.add_object(ObjRef::Element(ids[0]), Some(g)).unwrap();
        d.groups = b.finish();
        let mut ex = Extractor::default();
        let prefs = RenderPrefs::default();
        let view = |out: &RenderExtract, slot: usize| {
            out.instances
                .iter()
                .find(|u| u.slot as usize == slot)
                .map(|u| u.instance.0[3] & 0b1111)
        };
        let out = ex.extract(&mut d, &prefs, &ExtractBudget::UNLIMITED);
        assert_eq!(view(&out, 0), Some(0b1111));

        let mut j = cuboid_model::ChangeJournal::default();
        d.groups
            .edit(g, ChangeFlags::VISIBILITY, &mut j)
            .unwrap()
            .visible = false;
        let out = ex.extract(&mut d, &prefs, &ExtractBudget::UNLIMITED);
        assert_eq!(view(&out, 0), Some(0), "the element in the group is hidden");
        assert_eq!(view(&out, 1), None, "the other one is untouched");

        d.groups
            .edit(g, ChangeFlags::VISIBILITY, &mut j)
            .unwrap()
            .visible = true;
        let out = ex.extract(&mut d, &prefs, &ExtractBudget::UNLIMITED);
        assert_eq!(view(&out, 0), Some(0b1111));
    }

    #[test]
    fn an_element_with_shade_off_carries_bit_7() {
        let mut d = doc(2);
        let id = d.elements.ids()[1];
        let mut j = cuboid_model::ChangeJournal::default();
        d.elements
            .edit(id, &mut j)
            .unwrap()
            .flags
            .remove(cuboid_model::ElementFlags::SHADE);
        let out = Extractor::default().extract(
            &mut d,
            &RenderPrefs::default(),
            &ExtractBudget::UNLIMITED,
        );
        let bit = |i: usize| out.instances[i].instance.0[3] >> 7 & 1;
        assert_eq!((bit(0), bit(1)), (0, 1));
    }

    #[test]
    fn the_hovered_element_alone_gets_the_hover_bit() {
        let mut d = doc(3);
        let ids = d.elements.ids().to_vec();
        let mut ex = Extractor::default();
        run(&mut ex, &mut d);
        let hover = |ex: &mut Extractor, d: &mut Document, over: Option<usize>, sel: &[usize]| {
            ex.cache_mut().set_overlay(crate::OverlayState {
                hovered: over.map(|i| cuboid_model::ObjRef::Element(ids[i])),
                selected: sel
                    .iter()
                    .map(|&i| cuboid_model::ObjRef::Element(ids[i]))
                    .collect(),
                ..crate::OverlayState::default()
            });
            run(ex, d)
        };
        let out = hover(&mut ex, &mut d, Some(1), &[]);
        assert_eq!(out.instances.len(), 1);
        assert_eq!(out.instances[0].slot, 1);
        assert_eq!(out.instances[0].instance.0[3] >> 4 & 1, 1);
        // Moving the pointer to another element rewrites the two records.
        let out = hover(&mut ex, &mut d, Some(2), &[]);
        let mut slots: Vec<u32> = out.instances.iter().map(|u| u.slot).collect();
        slots.sort_unstable();
        assert_eq!(slots, [1, 2]);
        // A selected element keeps the selection colour, not the hover bit.
        let out = hover(&mut ex, &mut d, Some(2), &[2]);
        assert_eq!(out.instances.len(), 1);
        assert_eq!(out.instances[0].instance.0[3] >> 4 & 1, 0);
        assert!(hover(&mut ex, &mut d, Some(2), &[2]).is_empty());
    }

    #[test]
    fn translucent_faces_and_colours_pick_the_translucent_class() {
        let mut d = doc(2);
        let ids = d.elements.ids().to_vec();
        let mut j = cuboid_model::ChangeJournal::default();
        d.elements.edit(ids[1], &mut j).unwrap().color[3] = 0.5;
        let mut ex = Extractor::default();
        run(&mut ex, &mut d);
        let class = |ex: &Extractor| -> Vec<bool> { ex.cache().translucent_flags() };
        assert_eq!(class(&ex), [false, true]);
        let rev = ex.cache().scene_rev();
        // A texture with partial alpha makes every element that shows it translucent. The
        // records stay as they are; the draw lists see the new class.
        let mut buf = RgbaBuf::new(16, 16);
        buf.pixels
            .chunks_exact_mut(4)
            .for_each(|p| p.copy_from_slice(&[9, 9, 9, 128]));
        d.textures.entries[0].texture.image = Arc::new(Image::from_rgba(buf));
        let out = run(&mut ex, &mut d);
        assert_eq!(out.atlas.len(), 1);
        assert!(out.instances.is_empty());
        assert_eq!(class(&ex), [true, true]);
        assert_ne!(ex.cache().scene_rev(), rev);
    }

    #[test]
    fn a_painted_texture_uploads_its_rectangle_and_leaves_the_instances_alone() {
        let mut d = doc(3);
        let mut ex = Extractor::default();
        run(&mut ex, &mut d);
        d.textures.entries[0].texture.image = solid_image([1, 2, 3, 255]);
        let out = run(&mut ex, &mut d);
        assert_eq!(out.atlas.len(), 1);
        assert!(out.instances.is_empty());
    }

    #[test]
    fn a_texture_tick_uploads_the_frame_rectangle_and_leaves_the_instances_alone() {
        let mut d = doc(3);
        // The one texture becomes a strip of two frames of 16 x 16 texels.
        let mut buf = RgbaBuf::new(16, 32);
        buf.pixels.fill(255);
        let entry = &mut d.textures.entries[0];
        entry.texture.image = Arc::new(Image::from_rgba(buf));
        let mut animation = cuboid_model::TextureAnimation::new(16, 16);
        animation.frametime = 2;
        entry.animation = Some(animation);
        let mut ex = Extractor::default();
        let first = run(&mut ex, &mut d);
        assert_eq!(first.atlas.len(), 2, "the error texture and one frame");
        assert_eq!(
            (first.atlas[1].w, first.atlas[1].h),
            (18, 18),
            "a frame and its padding, not the strip"
        );

        // The same tick again changes nothing; the tick that starts the second frame uploads
        // that rectangle, and the faces keep their records because the rectangle did not move.
        ex.set_texture_tick(1);
        let same = run(&mut ex, &mut d);
        assert!(same.atlas.is_empty() && same.instances.is_empty());
        ex.set_texture_tick(2);
        let next = run(&mut ex, &mut d);
        assert_eq!(next.atlas.len(), 1);
        assert!(next.instances.is_empty());
    }

    #[test]
    fn the_atlas_upload_budget_carries_leftovers_over() {
        let mut d = doc(1);
        for i in 0..5 {
            d.textures.entries.push(TextureEntry {
                label: Label::from(format!("t{i}")),
                texture: Texture::new(solid_image([i, 0, 0, 255])),
                animation: None,
            });
        }
        let mut ex = Extractor::default();
        let budget = ExtractBudget {
            atlas_bytes: 2 * 18 * 18 * 4,
            ..ExtractBudget::UNLIMITED
        };
        let mut total = 0;
        let mut calls = 0;
        loop {
            let out = ex.extract(&mut d, &RenderPrefs::default(), &budget);
            total += out.atlas.len();
            calls += 1;
            if !ex.has_leftovers() {
                break;
            }
            assert!(calls < 20);
        }
        assert_eq!(total, 7, "the error texture and six textures");
        assert!(calls >= 3);
    }

    #[test]
    fn meshes_are_sent_when_new_or_changed_and_reported_when_gone() {
        let mut d = Document::new(TextureList::default());
        let mesh = Mesh {
            base: ObjectBase::new(
                "m",
                Vec3::new(1.0, 2.0, 3.0),
                Vec3::splat(2.0),
                Perms::PRIMITIVE,
            ),
            kind: MeshKind::Cube,
            geometry: Arc::default(),
            texture: None,
            submeshes: Arc::from([]),
            bounds: MeshBounds::default(),
            norm_factor: 1.0,
            import_offset: Vec3::ZERO,
            tint: [1.0; 4],
        };
        let id = d.meshes.push(mesh);
        let mut ex = Extractor::default();
        let out = run(&mut ex, &mut d);
        assert_eq!(out.meshes.len(), 1);
        assert_eq!(
            out.meshes[0].model.transform_point3(Vec3::ZERO),
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert!(run(&mut ex, &mut d).is_empty());
        let mut j = cuboid_model::ChangeJournal::default();
        d.meshes.edit(id, &mut j).unwrap().base.p.x = 10.0;
        let out = run(&mut ex, &mut d);
        assert_eq!(out.meshes.len(), 1);
        assert!(out.removed_meshes.is_empty());
        d.meshes.remove_set(&[id].into_iter().collect());
        let out = run(&mut ex, &mut d);
        assert_eq!(out.removed_meshes, [id]);
        assert!(run(&mut ex, &mut d).is_empty());
    }

    // --- A pose (animation A2 6) ---------------------------------------------------------------

    use cuboid_model::{Animation, Group, GroupId, GroupsBuilder, Keyframe, Rig};

    /// Four cubes at x = 0, 2, 4 and 6, the first two in a group "g" with its pivot at the
    /// origin, the third in a group "h" with its pivot at (4, 0, 0), the last one loose; and an
    /// animation of 1 second that has no track yet.
    fn rigged() -> (Document, [GroupId; 2], Animation) {
        let mut d = doc(4);
        let ids = d.elements.ids().to_vec();
        let mut b = GroupsBuilder::new();
        let g = b.add_group(Group::new("g", Vec3::ZERO), None).unwrap();
        b.add_object(ObjRef::Element(ids[0]), Some(g)).unwrap();
        b.add_object(ObjRef::Element(ids[1]), Some(g)).unwrap();
        let h = b
            .add_group(Group::new("h", Vec3::new(4.0, 0.0, 0.0)), None)
            .unwrap();
        b.add_object(ObjRef::Element(ids[2]), Some(h)).unwrap();
        d.groups = b.finish();
        (d, [g, h], Animation::new("a", 1.0))
    }

    fn posed(d: &Document, a: &Animation, t: f32) -> Pose {
        let mut pose = Pose::new();
        Rig::new(&d.groups).pose(a, t, &mut pose);
        pose
    }

    fn constant(value: [f32; 3]) -> Vec<Keyframe> {
        vec![Keyframe::new(0.0, value), Keyframe::new(1.0, value)]
    }

    fn word(update: &InstanceUpdate, at: usize) -> f32 {
        f32::from_bits(update.instance.0[at])
    }

    #[test]
    fn a_pose_rewrites_the_records_of_the_groups_that_move_and_no_others() {
        let (mut d, [g, _], mut anim) = rigged();
        anim.track_mut(g).position = vec![
            Keyframe::new(0.0, [0.0; 3]),
            Keyframe::new(1.0, [0.0, 4.0, 0.0]),
        ];
        let mut ex = Extractor::default();
        let rest = run(&mut ex, &mut d);
        assert_eq!(rest.instances.len(), 4);

        ex.set_pose(Some(&posed(&d, &anim, 0.5)));
        let out = run(&mut ex, &mut d);
        let mut slots: Vec<u32> = out.instances.iter().map(|u| u.slot).collect();
        slots.sort_unstable();
        assert_eq!(slots, [0, 1], "the cubes of the group that moves");
        for update in &out.instances {
            assert!((word(update, 1) - 2.0).abs() < 1e-5, "y moved by 2");
        }
        assert!(run(&mut ex, &mut d).is_empty(), "nothing changed since");

        // The same pose again writes nothing; a later time writes the two records again.
        ex.set_pose(Some(&posed(&d, &anim, 0.5)));
        assert!(run(&mut ex, &mut d).is_empty());
        ex.set_pose(Some(&posed(&d, &anim, 1.0)));
        let out = run(&mut ex, &mut d);
        assert_eq!(out.instances.len(), 2);
        assert!((word(&out.instances[0], 1) - 4.0).abs() < 1e-5);

        // The rest pose comes back exactly, record for record, and the document was never touched.
        let revision = d.revision;
        ex.set_pose(None);
        let back = run(&mut ex, &mut d);
        assert_eq!(back.instances.len(), 2);
        for update in &back.instances {
            let before = rest
                .instances
                .iter()
                .find(|u| u.slot == update.slot)
                .unwrap();
            assert_eq!(update.instance, before.instance);
        }
        assert_eq!(d.revision, revision);
        ex.set_pose(None);
        assert!(run(&mut ex, &mut d).is_empty());
    }

    #[test]
    fn a_pose_turns_and_scales_the_boxes_about_the_pivot_of_their_group() {
        let (mut d, [g, h], mut anim) = rigged();
        anim.track_mut(g).rotation = constant([0.0, 0.0, 90.0]);
        anim.track_mut(h).scale = constant([2.0, 2.0, 2.0]);
        let mut ex = Extractor::default();
        run(&mut ex, &mut d);
        ex.set_pose(Some(&posed(&d, &anim, 0.5)));
        let out = run(&mut ex, &mut d);
        assert_eq!(out.instances.len(), 3);
        let at = |slot: u32| out.instances.iter().find(|u| u.slot == slot).unwrap();
        // Cube 1 at (2, 0, 0) turned 90 degrees about Z through the origin: its corner is at
        // (0, 2, 0) and the box is turned with it.
        let turned = at(1);
        assert!((word(turned, 0)).abs() < 1e-4 && (word(turned, 1) - 2.0).abs() < 1e-4);
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!((word(turned, 10) - half).abs() < 1e-4, "z of the rotation");
        assert!((word(turned, 11) - half).abs() < 1e-4, "w of the rotation");
        assert!((word(turned, 4) - 1.0).abs() < 1e-5, "the size stays");
        // Cube 2 at (4, 0, 0) is at the pivot of its group, which doubles it.
        let grown = at(2);
        assert!((word(grown, 0) - 4.0).abs() < 1e-5);
        assert!((word(grown, 4) - 2.0).abs() < 1e-5);
        // The draw lists see the pose: the outline and the culling box follow the record.
        let info = ex.cache().info(2).unwrap();
        assert!((info.frame.unwrap().size.x - 2.0).abs() < 1e-5);
        assert!(info.bounds.max.x > 5.9, "{:?}", info.bounds);
        assert!(ex.cache().info(3).unwrap().bounds.max.x < 7.1);
    }

    #[test]
    fn an_edit_during_a_pose_keeps_the_pose_and_a_group_that_stops_moving_goes_back_to_rest() {
        let (mut d, [g, h], mut anim) = rigged();
        anim.track_mut(g).position = constant([0.0, 3.0, 0.0]);
        anim.track_mut(h).position = constant([0.0, 5.0, 0.0]);
        let mut ex = Extractor::default();
        run(&mut ex, &mut d);
        ex.set_pose(Some(&posed(&d, &anim, 0.0)));
        run(&mut ex, &mut d);

        // A new element of the loose kind arrives and an element of the group is edited.
        let ids = d.elements.ids().to_vec();
        let mut j = cuboid_model::ChangeJournal::default();
        d.elements.edit(ids[0], &mut j).unwrap().base.p.x = 1.0;
        let out = run(&mut ex, &mut d);
        assert_eq!(out.instances.len(), 1);
        assert!((word(&out.instances[0], 0) - 1.0).abs() < 1e-5);
        assert!(
            (word(&out.instances[0], 1) - 3.0).abs() < 1e-5,
            "still posed"
        );

        // The animation moves one group only now; the other one returns to rest.
        anim.track_mut(h).position.clear();
        ex.set_pose(Some(&posed(&d, &anim, 0.0)));
        let out = run(&mut ex, &mut d);
        assert_eq!(out.instances.len(), 1);
        assert_eq!(out.instances[0].slot, 2);
        assert!(word(&out.instances[0], 1).abs() < 1e-5);
    }

    #[test]
    fn a_pose_moves_the_meshes_of_a_group_and_a_rest_pose_puts_them_back() {
        let (mut d, [g, _], mut anim) = rigged();
        let mesh = Mesh {
            base: ObjectBase::new(
                "m",
                Vec3::new(1.0, 2.0, 3.0),
                Vec3::splat(2.0),
                Perms::PRIMITIVE,
            ),
            kind: MeshKind::Cube,
            geometry: Arc::default(),
            texture: None,
            submeshes: Arc::from([]),
            bounds: MeshBounds::default(),
            norm_factor: 1.0,
            import_offset: Vec3::ZERO,
            tint: [1.0; 4],
        };
        let id = d.meshes.push(mesh);
        // The mesh joins the group `g`: its record in the tree is rebuilt around it.
        d.move_nodes(&[ObjRef::Mesh(id)], Some(g), None).unwrap();
        anim.track_mut(g).position = constant([0.0, 10.0, 0.0]);
        let mut ex = Extractor::default();
        let first = run(&mut ex, &mut d);
        let rest = first.meshes[0].model;

        ex.set_pose(Some(&posed(&d, &anim, 0.5)));
        let out = run(&mut ex, &mut d);
        assert_eq!(out.meshes.len(), 1);
        assert_eq!(
            out.meshes[0].model.transform_point3(Vec3::ZERO),
            Vec3::new(1.0, 12.0, 3.0)
        );
        assert!(Arc::ptr_eq(
            &out.meshes[0].geometry,
            &first.meshes[0].geometry
        ));
        assert!(run(&mut ex, &mut d).meshes.is_empty());
        ex.set_pose(None);
        let out = run(&mut ex, &mut d);
        assert_eq!(out.meshes[0].model, rest);
    }

    #[test]
    fn a_pose_in_which_nothing_moves_is_the_rest_pose_and_costs_nothing() {
        let (mut d, _, anim) = rigged();
        let mut ex = Extractor::default();
        run(&mut ex, &mut d);
        let rev = ex.cache().scene_rev();
        ex.set_pose(Some(&posed(&d, &anim, 0.5)));
        assert!(run(&mut ex, &mut d).is_empty());
        assert_eq!(ex.cache().scene_rev(), rev);
    }

    fn voxel_doc(res: u32) -> Document {
        let mut d = Document::new(TextureList::default());
        d.voxels = Some(VoxelWorld::new(res, true, grid_box(res)));
        d
    }

    fn set(d: &mut Document, i: (i32, i32, i32), v: u32) {
        d.voxels
            .as_mut()
            .unwrap()
            .write_full(glam::IVec3::new(i.0, i.1, i.2), v);
    }

    #[test]
    fn only_dirty_chunks_are_meshed() {
        let mut d = voxel_doc(16);
        set(&mut d, (20, 20, 20), 0xFFFF_0000);
        let mut ex = Extractor::default();
        let out = run(&mut ex, &mut d);
        assert_eq!(out.chunks.len(), 1);
        let faces: usize = out.chunks[0].faces.iter().map(Vec::len).sum();
        assert_eq!(faces, 6, "a lone voxel shows six faces");
        assert!(run(&mut ex, &mut d).is_empty());
        // A voxel in the interior of a chunk dirties that chunk only.
        set(&mut d, (21, 20, 20), 0xFFFF_0000);
        let out = run(&mut ex, &mut d);
        assert_eq!(out.chunks.len(), 1);
        let faces: usize = out.chunks[0].faces.iter().map(Vec::len).sum();
        assert_eq!(faces, 10);
        assert!(out.chunks[0].rev > cuboid_voxel::Rev::NONE);
        assert!(run(&mut ex, &mut d).is_empty());
    }

    #[test]
    fn a_later_mesh_of_a_chunk_has_a_newer_revision() {
        let mut d = voxel_doc(16);
        set(&mut d, (20, 20, 20), 0xFFFF_0000);
        let mut ex = Extractor::default();
        let first = run(&mut ex, &mut d).chunks[0].rev;
        set(&mut d, (22, 20, 20), 0xFFFF_0000);
        let second = run(&mut ex, &mut d).chunks[0].rev;
        assert!(second > first);
    }

    #[test]
    fn pool_blocks_are_reused_and_never_overlap() {
        let mut d = voxel_doc(16);
        let mut ex = Extractor::default();
        // Fill two chunks, then empty one and grow the other.
        for x in 0..4 {
            set(&mut d, (x, 0, 0), 0xFFFF_0000);
            set(&mut d, (20 + x, 0, 0), 0xFF00_FF00);
        }
        let out = run(&mut ex, &mut d);
        assert_eq!(out.chunks.len(), 2);
        let used = |chunks: &[ChunkMesh]| {
            let mut spans: Vec<(usize, u32, u32)> = Vec::new();
            for c in chunks {
                for dir in 0..6 {
                    if !c.faces[dir].is_empty() {
                        spans.push((dir, c.offsets[dir], to_u32(c.faces[dir].len())));
                    }
                }
            }
            spans.sort_unstable();
            spans
                .windows(2)
                .for_each(|w| assert!(w[0].0 != w[1].0 || w[0].1 + w[0].2 <= w[1].1, "{w:?}"));
        };
        used(&out.chunks);
        for x in 0..4 {
            set(&mut d, (x, 0, 0), 0);
        }
        let out = run(&mut ex, &mut d);
        assert!(
            out.chunks.iter().any(|c| c.faces.iter().all(Vec::is_empty)),
            "the emptied chunk is cleared"
        );
        for y in 0..6 {
            set(&mut d, (20, y + 2, 0), 0xFF00_00FF);
        }
        let out = run(&mut ex, &mut d);
        assert!(out.voxel_pools.iter().all(|&c| c >= 1024));
        assert!(!out.chunks.is_empty());
        assert!(run(&mut ex, &mut d).is_empty());
    }

    #[test]
    fn chunk_meshing_respects_the_budget_and_finishes_later() {
        let mut d = voxel_doc(32);
        for c in 0..12 {
            set(&mut d, (c * 8, c * 4, 3), 0xFFFF_0000);
        }
        let mut ex = Extractor::default();
        let budget = ExtractBudget {
            chunks: 3,
            ..ExtractBudget::UNLIMITED
        };
        let mut seen = FxHashSet::default();
        for _ in 0..40 {
            let out = ex.extract(&mut d, &RenderPrefs::default(), &budget);
            assert!(out.chunks.len() <= 3);
            out.chunks.iter().for_each(|c| {
                seen.insert(c.slot);
            });
            if !ex.has_leftovers() {
                break;
            }
        }
        assert!(!ex.has_leftovers());
        let expected: usize = d
            .voxels
            .as_ref()
            .unwrap()
            .slots()
            .iter()
            .enumerate()
            .filter(|(i, _)| slot_has_cells(d.voxels.as_ref().unwrap(), *i))
            .count();
        assert_eq!(seen.len(), expected);
    }

    #[test]
    fn replacing_or_removing_the_world_clears_what_the_renderer_holds() {
        let mut d = voxel_doc(16);
        set(&mut d, (20, 20, 20), 0xFFFF_0000);
        let mut ex = Extractor::default();
        let first = run(&mut ex, &mut d);
        let slot = first.chunks[0].slot;
        let old_world = d.voxels.take();
        let out = run(&mut ex, &mut d);
        assert_eq!(out.chunks.len(), 1);
        assert!(out.chunks[0].faces.iter().all(Vec::is_empty) && out.chunks[0].slot == slot);
        assert!(run(&mut ex, &mut d).is_empty());
        // Back with a restored world: every slot is dirty and meshed again.
        d.voxels = old_world.map(|w| VoxelWorld::from_snapshot(&w.snapshot(), true, grid_box(16)));
        let out = run(&mut ex, &mut d);
        assert!(
            out.chunks
                .iter()
                .any(|c| c.slot == slot && c.faces.iter().any(|f| !f.is_empty()))
        );
    }

    #[test]
    fn the_draw_cache_follows_the_extract() {
        let mut d = doc(3);
        set_world(&mut d);
        let mut ex = Extractor::default();
        run(&mut ex, &mut d);
        assert_eq!(ex.cache().instance_count(), 3);
        assert_eq!(ex.cache().chunk_count(), 1);
        let before = ex.cache().scene_rev();
        run(&mut ex, &mut d);
        assert_eq!(ex.cache().scene_rev(), before, "nothing changed");
    }

    fn set_world(d: &mut Document) {
        d.voxels = Some(VoxelWorld::new(16, true, grid_box(16)));
        set(d, (20, 20, 20), 0xFFFF_0000);
    }
}
