//! Draw-list compilation per view: voxel ranges, items, overlays and outline (spec: app 8; arch
//! 13.1-13.4).

use std::collections::BTreeMap;
use std::hash::Hasher;

use cuboid_model::{ModelBox, ObjId, ObjRef, RenderPrefs};
use glam::{Mat4, Quat, Vec2, Vec3};
use rustc_hash::{FxHashMap, FxHasher};

use crate::camera::Camera;
use crate::extract::ElementInstance;
use crate::frame::Frame;
use crate::geom::{Aabb, Frustum, to_u32};
use crate::gizmo::GizmoKind;
use crate::views::{View, ViewId};

/// Everything that decides whether a view must be re-rendered. A view whose key did not change
/// keeps its texture; a frame where only the UI changed costs egui's pass plus four quads.
///
/// FrameKey lives here and not in `cuboid-render` because a draw list carries it.
///
/// spec: arch 13.1, arch 6 (cuboid-render)
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub struct FrameKey {
    /// Scene content: instance, mesh, voxel and atlas revisions.
    pub scene: u64,
    /// The view camera.
    pub camera: u64,
    /// Overlay state: selection, hover, grid, guides, gizmos.
    pub overlay: u64,
    /// Rendering preferences.
    pub prefs: u64,
    /// Viewport size in physical pixels.
    pub size: (u32, u32),
    /// Animation frame (the 50 Hz floor animation).
    pub anim: u32,
}

/// A contiguous range of face records of one direction pool, one per allocated chunk that is
/// visible: the camera can see a face plane of that direction and the chunk is inside the
/// frustum.
///
/// spec: arch 13.5 (Pools and culling)
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ChunkRange {
    /// Slot of the chunk in the world's table.
    pub slot: u32,
    /// First record in the pool.
    pub first: u32,
    /// Number of records.
    pub count: u32,
}

/// The three sub-grid planes.
///
/// spec: app 6.3
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum GridPlane {
    /// `y = -0.001`, all views.
    Floor,
    /// `x = -0.001`, Side view.
    Side,
    /// `z = +0.001`, Front view.
    Front,
}

/// What one entry of the sorted transparent queue draws.
///
/// spec: app 8.1, app 8.6, arch 13.4
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DrawTarget {
    /// A run of consecutive binary-alpha elements, drawn as one instanced draw.
    Elements {
        /// First instance of the run.
        first: u32,
        /// Number of instances.
        count: u32,
    },
    /// One mesh sub-mesh.
    Mesh {
        /// The mesh object.
        id: ObjId,
        /// Sub-mesh index.
        submesh: u32,
    },
    /// A sub-grid plane, drawn additively in sequence.
    SubGrid(GridPlane),
    /// The background plane of an orthographic view.
    Background(ViewId),
}

/// One entry of a view's transparent queue, in back-to-front order.
///
/// spec: arch 13.4, arch 6 (cuboid-scene)
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DrawItem {
    /// What to draw.
    pub target: DrawTarget,
    /// Drawn exactly as the original: depth pre-pass, then the blended colour pass. Binary
    /// objects use the single alpha-tested pass.
    pub translucent: bool,
}

/// The overlay content of a view.
///
/// spec: app 6.3-6.5, app 8.8-8.10
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OverlayKind {
    /// The analytic sub-grid shader.
    SubGrid(GridPlane),
    /// The main grid lines of the view's plane.
    MainGrid,
    /// The guides.
    Guides,
    /// The marker at the centre of 3D space.
    CenterMarker,
    /// The north pointer.
    NorthPointer,
    /// The blueprint box.
    Blueprint,
    /// The model-origin marker.
    ModelOrigin,
    /// A faint box for a hidden element.
    HiddenBox,
    /// The wireframe box of a selected object (at most four).
    SelectionBox,
    /// The model box outline.
    ModelBox,
    /// A transform gizmo, drawn without depth.
    Gizmo(GizmoKind),
    /// The plate on the hit face of the voxel under the pointer.
    VoxelPointer,
}

/// One overlay draw.
///
/// spec: arch 13.6
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct OverlayItem {
    /// What it is.
    pub kind: OverlayKind,
    /// Model matrix.
    pub transform: Mat4,
}

/// What the renderer draws for one view, in order: voxel pools by direction, the sorted
/// transparent queue, overlays, and the objects whose silhouettes form the outline.
///
/// spec: arch 6 (cuboid-scene), arch 13.1, arch 13.4, arch 13.5
#[derive(Clone, Debug)]
pub struct ViewDrawList {
    /// Re-render only when this changed.
    pub key: FrameKey,
    /// Visible chunk ranges per direction pool.
    pub voxel_ranges: [Vec<ChunkRange>; 6],
    /// The transparent queue, back to front.
    pub items: Vec<DrawItem>,
    /// Overlays.
    pub overlays: Vec<OverlayItem>,
    /// The objects of the outline mask pass: the selected ones, then the hovered one unless it
    /// is selected. The hovered element carries the hover bit in its instance record, which
    /// picks the hover colour.
    pub outline: Vec<DrawItem>,
}

/// What the application tells the scene about the interaction, so the overlays and the outline
/// can be compiled. Changing it changes the overlay revision of the frame keys.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayState {
    /// The selected objects.
    pub selected: Vec<ObjRef>,
    /// The hovered object.
    pub hovered: Option<ObjRef>,
    /// The transform gizmo and its placement.
    pub gizmo: Option<(GizmoKind, Mat4)>,
    /// The plate of the voxel pointer.
    pub voxel_pointer: Option<Mat4>,
    /// Draw the grids.
    pub grid: bool,
    /// Draw the outline of the model box.
    pub model_box: bool,
    /// Draw the model-origin marker.
    pub model_origin: bool,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            selected: Vec::new(),
            hovered: None,
            gizmo: None,
            voxel_pointer: None,
            grid: true,
            model_box: false,
            model_origin: false,
        }
    }
}

/// One element as the draw lists need it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct ElemInfo {
    pub frame: Option<Frame>,
    /// World box, empty while the element is not drawn in any view.
    pub bounds: Aabb,
    pub translucent: bool,
    /// A hidden element drawn as a faint box.
    pub faint: bool,
    pub view_mask: u8,
}

/// One mesh as the draw lists need it.
#[derive(Copy, Clone, Debug)]
pub(crate) struct MeshInfo {
    pub id: ObjId,
    pub bounds: Aabb,
    pub hidden: bool,
    pub translucent: bool,
    pub submeshes: u32,
    pub model: Mat4,
    pub primitive: bool,
}

/// One chunk that has faces.
#[derive(Copy, Clone, Debug)]
pub(crate) struct ChunkInfo {
    pub bounds: Aabb,
    pub first: [u32; 6],
    pub count: [u32; 6],
}

/// The scene-side caches draw-list compilation reads: instance records, the runtime atlas
/// layout and the alpha classification of objects. The [`Extractor`](crate::Extractor) keeps it
/// in step with the document.
///
/// spec: arch 13.3, arch 13.4, arch 13.6
#[derive(Debug, Default)]
pub struct SceneCache {
    overlay: OverlayState,
    rev: u64,
    overlay_rev: u64,
    instances: Vec<ElementInstance>,
    infos: Vec<ElemInfo>,
    pub(crate) elem_ids: Vec<ObjId>,
    elem_index: FxHashMap<ObjId, u32>,
    pub(crate) meshes: Vec<MeshInfo>,
    pub(crate) chunks: BTreeMap<u32, ChunkInfo>,
    blueprint: Option<Mat4>,
    model_box: Option<ModelBox>,
    origin: Vec3,
}

impl SceneCache {
    /// The number of element instances.
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// The number of chunks that have faces.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Changes whenever the scene content a view draws changes.
    pub fn scene_rev(&self) -> u64 {
        self.rev
    }

    /// The element hovered by the pointer and not selected: it gets the hover bit.
    pub fn hovered_element(&self) -> Option<ObjId> {
        match self.overlay.hovered {
            Some(ObjRef::Element(id)) if !self.overlay.selected.contains(&ObjRef::Element(id)) => {
                Some(id)
            }
            _ => None,
        }
    }

    pub(crate) fn info(&self, slot: usize) -> Option<&ElemInfo> {
        self.infos.get(slot)
    }

    /// The alpha class of each element in list order, `true` for the sorted translucent queue.
    pub fn translucent_flags(&self) -> Vec<bool> {
        self.infos.iter().map(|e| e.translucent).collect()
    }

    /// The instance record of an element, by list position.
    pub fn instance(&self, slot: usize) -> Option<&ElementInstance> {
        self.instances.get(slot)
    }

    /// Replaces the interaction state; the overlay revision changes when it differs.
    pub fn set_overlay(&mut self, state: OverlayState) {
        if self.overlay != state {
            self.overlay = state;
            self.overlay_rev = self.overlay_rev.wrapping_add(1);
        }
    }

    pub(crate) fn bump(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    pub(crate) fn set_element(&mut self, slot: usize, instance: ElementInstance, info: ElemInfo) {
        if let (Some(i), Some(m)) = (self.instances.get_mut(slot), self.infos.get_mut(slot)) {
            *i = instance;
            *m = info;
        }
    }

    pub(crate) fn push_element(&mut self, instance: ElementInstance, info: ElemInfo) {
        self.instances.push(instance);
        self.infos.push(info);
    }

    pub(crate) fn truncate_elements(&mut self, len: usize) {
        self.instances.truncate(len);
        self.infos.truncate(len);
    }

    pub(crate) fn set_element_ids(&mut self, ids: &[ObjId]) {
        self.elem_ids = ids.to_vec();
        self.elem_index = ids
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, to_u32(i)))
            .collect();
    }

    /// Stores the blueprint, model box and origin; returns whether any of them changed.
    pub(crate) fn set_scene(
        &mut self,
        blueprint: Option<Mat4>,
        model_box: ModelBox,
        origin: Vec3,
    ) -> bool {
        let changed = self.blueprint != blueprint
            || self.model_box != Some(model_box)
            || self.origin != origin;
        self.blueprint = blueprint;
        self.model_box = Some(model_box);
        self.origin = origin;
        changed
    }
}

/// The pool directions in pool order as the axis and sign of their outward normal.
const POOL_NORMALS: [(usize, f32); 6] = [
    (1, -1.0),
    (1, 1.0),
    (2, 1.0),
    (2, -1.0),
    (0, -1.0),
    (0, 1.0),
];

/// Whether a face plane of pool `dir` inside `bounds` can face the camera.
fn pool_faces_camera(dir: usize, camera: &Camera, bounds: &Aabb) -> bool {
    let (axis, sign) = POOL_NORMALS[dir];
    if camera.ortho {
        -camera.forward()[axis] * sign > 1e-4
    } else if sign > 0.0 {
        camera.pos[axis] > bounds.min[axis]
    } else {
        camera.pos[axis] < bounds.max[axis]
    }
}

fn hash_bits(values: impl IntoIterator<Item = u32>) -> u64 {
    let mut h = FxHasher::default();
    values.into_iter().for_each(|v| h.write_u32(v));
    h.finish()
}

fn camera_key(c: &Camera) -> u64 {
    hash_bits(
        [
            c.pos.x,
            c.pos.y,
            c.pos.z,
            c.rot.x,
            c.rot.y,
            c.rot.z,
            c.rot.w,
            c.ortho_size,
            c.fov_deg,
            c.near,
            c.far,
        ]
        .into_iter()
        .map(f32::to_bits)
        .chain([u32::from(c.ortho)]),
    )
}

fn prefs_key(p: &RenderPrefs) -> u64 {
    hash_bits([
        u32::from(p.use_elements_color),
        u32::from(p.emulate_tint_colors),
        u32::from(p.center_marker),
        u32::from(p.hidden_as_box),
        u32::from(p.hidden_in_main_view),
        u32::from(p.antialiasing),
        u32::from(p.shadows),
        u32::from(p.wireframe),
    ])
}

/// The size of a rectangle in whole pixels.
fn pixel_size(rect: crate::Rect) -> (u32, u32) {
    let px = |v: f32| u32::try_from(cuboid_num::trunc_i32(v.round()).max(0)).unwrap_or(0);
    (px(rect.w), px(rect.h))
}

/// Compiles the draw list of a view: culls, sorts the transparent queue back to front (ties keep
/// list order), batches runs of binary objects and collects overlays and the outline set.
///
/// Binary elements go first as instanced runs of consecutive list positions: their depth
/// pre-pass makes their order irrelevant. Translucent elements, meshes and the sub-grid planes
/// follow, sorted by the depth of the centre of their box.
///
/// spec: arch 13.1-13.4, app 8.6
pub fn compile_draw_list(view: &View, cache: &SceneCache, prefs: &RenderPrefs) -> ViewDrawList {
    let camera = &view.camera;
    let aspect = view.rect.aspect();
    let frustum = Frustum::from_view_proj(camera.view_proj(aspect), Vec2::splat(-1.0), Vec2::ONE);
    let bit = 1_u8 << view.id.index();
    let overlay = &cache.overlay;

    let mut voxel_ranges: [Vec<ChunkRange>; 6] = Default::default();
    for (&slot, chunk) in &cache.chunks {
        if !frustum.intersects_aabb(&chunk.bounds) {
            continue;
        }
        for (dir, ranges) in voxel_ranges.iter_mut().enumerate() {
            if chunk.count[dir] > 0 && pool_faces_camera(dir, camera, &chunk.bounds) {
                ranges.push(ChunkRange {
                    slot,
                    first: chunk.first[dir],
                    count: chunk.count[dir],
                });
            }
        }
    }

    let mut items = Vec::new();
    let mut run: Option<(u32, u32)> = None;
    let mut sorted: Vec<(f32, DrawItem)> = Vec::new();
    for (i, info) in cache.infos.iter().enumerate() {
        let drawn = info.view_mask & bit != 0 && info.bounds.is_valid();
        if drawn && !info.translucent {
            run = match run {
                Some((first, count)) if first + count == to_u32(i) => Some((first, count + 1)),
                Some(done) => {
                    items.push(binary_run(done));
                    Some((to_u32(i), 1))
                }
                None => Some((to_u32(i), 1)),
            };
            continue;
        }
        if drawn && frustum.intersects_aabb(&info.bounds) {
            let depth = camera.sort_depth(info.bounds.center());
            let target = DrawTarget::Elements {
                first: to_u32(i),
                count: 1,
            };
            sorted.push((
                depth,
                DrawItem {
                    target,
                    translucent: true,
                },
            ));
        }
    }
    items.extend(run.map(binary_run));
    for mesh in cache
        .meshes
        .iter()
        .filter(|m| !m.hidden && m.bounds.is_valid())
    {
        if !frustum.intersects_aabb(&mesh.bounds) {
            continue;
        }
        let depth = camera.sort_depth(mesh.bounds.center());
        for submesh in 0..mesh.submeshes {
            let target = DrawTarget::Mesh {
                id: mesh.id,
                submesh,
            };
            sorted.push((
                depth,
                DrawItem {
                    target,
                    translucent: mesh.translucent,
                },
            ));
        }
    }
    if overlay.grid {
        for (plane, center) in [
            (GridPlane::Floor, Vec3::new(8.0, 0.0, -8.0)),
            (GridPlane::Side, Vec3::new(0.0, 8.0, -8.0)),
            (GridPlane::Front, Vec3::new(8.0, 8.0, 0.0)),
        ] {
            let shown = match plane {
                GridPlane::Floor => true,
                GridPlane::Side => view.id == ViewId::Side,
                GridPlane::Front => view.id == ViewId::Front,
            };
            if shown {
                let item = DrawItem {
                    target: DrawTarget::SubGrid(plane),
                    translucent: true,
                };
                sorted.push((camera.sort_depth(center), item));
            }
        }
    }
    // Far to near; the sort is stable, so ties keep list order.
    sorted.sort_by(|a, b| b.0.total_cmp(&a.0));
    items.extend(sorted.into_iter().map(|(_, item)| item));

    ViewDrawList {
        key: FrameKey {
            scene: cache.rev,
            camera: camera_key(camera),
            overlay: cache.overlay_rev,
            prefs: prefs_key(prefs),
            size: pixel_size(view.rect),
            anim: 0,
        },
        voxel_ranges,
        items,
        overlays: compile_overlays(view.id, cache, prefs),
        outline: outline_items(
            cache,
            overlay
                .selected
                .iter()
                .copied()
                .chain(overlay.hovered.filter(|h| !overlay.selected.contains(h))),
        ),
    }
}

fn binary_run((first, count): (u32, u32)) -> DrawItem {
    DrawItem {
        target: DrawTarget::Elements { first, count },
        translucent: false,
    }
}

/// The draw items of the silhouettes of objects.
fn outline_items(cache: &SceneCache, objects: impl Iterator<Item = ObjRef>) -> Vec<DrawItem> {
    let mut out = Vec::new();
    for obj in objects {
        match obj {
            ObjRef::Element(id) => {
                let Some(&i) = cache.elem_index.get(&id) else {
                    continue;
                };
                let Some(info) = cache.infos.get(i as usize).filter(|e| e.view_mask != 0) else {
                    continue;
                };
                let target = DrawTarget::Elements { first: i, count: 1 };
                out.push(DrawItem {
                    target,
                    translucent: info.translucent,
                });
            }
            ObjRef::Mesh(id) => {
                let Some(mesh) = cache.meshes.iter().find(|m| m.id == id && !m.hidden) else {
                    continue;
                };
                out.extend((0..mesh.submeshes).map(|submesh| DrawItem {
                    target: DrawTarget::Mesh { id, submesh },
                    translucent: mesh.translucent,
                }));
            }
            // The caller names the objects below a selected group.
            ObjRef::Group(_) | ObjRef::Blueprint | ObjRef::ModelOrigin => {}
        }
    }
    out
}

fn compile_overlays(view: ViewId, cache: &SceneCache, prefs: &RenderPrefs) -> Vec<OverlayItem> {
    let overlay = &cache.overlay;
    let mut out = Vec::new();
    let mut push = |kind, transform| out.push(OverlayItem { kind, transform });
    if overlay.grid {
        push(OverlayKind::MainGrid, Mat4::IDENTITY);
    }
    if prefs.center_marker {
        push(
            OverlayKind::CenterMarker,
            Mat4::from_translation(Vec3::new(8.0, 8.0, -8.0)),
        );
    }
    if view != ViewId::Front {
        push(
            OverlayKind::NorthPointer,
            Mat4::from_translation(Vec3::new(8.0, 0.0, 2.0)),
        );
    }
    if let Some(bp) = cache.blueprint {
        push(OverlayKind::Blueprint, bp);
    }
    if overlay.model_origin {
        push(
            OverlayKind::ModelOrigin,
            Mat4::from_translation(cache.origin),
        );
    }
    if overlay.model_box
        && let Some(b) = cache.model_box
    {
        push(
            OverlayKind::ModelBox,
            Mat4::from_scale_rotation_translation(b.max - b.min, Quat::IDENTITY, b.min),
        );
    }
    if overlay.selected.len() <= 4 {
        for obj in &overlay.selected {
            let model = match *obj {
                ObjRef::Element(id) => cache
                    .elem_index
                    .get(&id)
                    .and_then(|&i| cache.infos.get(i as usize))
                    .and_then(|e| e.frame)
                    .map(|f| f.model_matrix()),
                ObjRef::Mesh(id) => cache
                    .meshes
                    .iter()
                    .find(|m| m.id == id && m.primitive)
                    .map(|m| m.model),
                ObjRef::Group(_) | ObjRef::Blueprint | ObjRef::ModelOrigin => None,
            };
            if let Some(model) = model {
                push(OverlayKind::SelectionBox, model);
            }
        }
    }
    if let Some((kind, transform)) = overlay.gizmo {
        push(OverlayKind::Gizmo(kind), transform);
    }
    if let Some(plate) = overlay.voxel_pointer {
        push(OverlayKind::VoxelPointer, plate);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cuboid_image::{Image, RgbaBuf};
    use cuboid_model::{Document, Element, ElementType, Label, Texture, TextureEntry, TextureList};

    use super::*;
    use crate::extract::{ExtractBudget, Extractor};
    use crate::views::{Rect, ViewSet};

    fn image(alpha: u8) -> Arc<Image> {
        let mut buf = RgbaBuf::new(16, 16);
        buf.pixels
            .chunks_exact_mut(4)
            .for_each(|p| p.copy_from_slice(&[9, 9, 9, alpha]));
        Arc::new(Image::from_rgba(buf))
    }

    fn doc(positions: &[(Vec3, bool)]) -> Document {
        let mut textures = TextureList::default();
        textures.entries.push(TextureEntry {
            label: Label::from("opaque"),
            texture: Texture::new(image(255)),
            animation: None,
        });
        textures.entries.push(TextureEntry {
            label: Label::from("glass"),
            texture: Texture::new(image(128)),
            animation: None,
        });
        let mut doc = Document::new(textures);
        for &(p, glass) in positions {
            let label = Label::from(if glass { "glass" } else { "opaque" });
            let mut el = Element::new(ElementType::Cube, &doc.blueprint, 1, [1.0; 4], &label);
            el.base.p = p;
            el.base.s = Vec3::ONE;
            doc.elements.push(el);
        }
        doc
    }

    fn compile(
        doc: &mut Document,
        view: ViewId,
        tweak: impl FnOnce(&mut Extractor),
    ) -> ViewDrawList {
        let mut ex = Extractor::default();
        ex.extract(doc, &RenderPrefs::default(), &ExtractBudget::UNLIMITED);
        tweak(&mut ex);
        let mut views = ViewSet::default();
        views.arrange(Rect {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
        });
        views.set_layout(
            crate::ViewLayout::Split4,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
        );
        compile_draw_list(views.view(view), ex.cache(), &RenderPrefs::default())
    }

    fn target_first(item: &DrawItem) -> u32 {
        match item.target {
            DrawTarget::Elements { first, .. } => first,
            _ => u32::MAX,
        }
    }

    #[test]
    fn binary_elements_form_runs_and_translucent_ones_sort_back_to_front() {
        // Front looks along +Z from z = -54: larger z is farther.
        let mut d = doc(&[
            (Vec3::new(0.0, 0.0, -1.0), false),
            (Vec3::new(2.0, 0.0, -1.0), false),
            (Vec3::new(4.0, 0.0, -5.0), true),
            (Vec3::new(6.0, 0.0, 10.0), true),
            (Vec3::new(8.0, 0.0, -1.0), false),
        ]);
        let list = compile(&mut d, ViewId::Front, |_| {});
        let runs: Vec<_> = list.items.iter().filter(|i| !i.translucent).collect();
        assert_eq!(runs.len(), 2, "{:?}", list.items);
        assert_eq!(runs[0].target, DrawTarget::Elements { first: 0, count: 2 });
        assert_eq!(runs[1].target, DrawTarget::Elements { first: 4, count: 1 });
        let glass: Vec<u32> = list
            .items
            .iter()
            .filter(|i| i.translucent && target_first(i) != u32::MAX)
            .map(target_first)
            .collect();
        assert_eq!(glass, [3, 2], "the farther pane first");
    }

    #[test]
    fn ties_keep_the_list_order() {
        let mut d = doc(&[
            (Vec3::new(0.0, 0.0, 0.0), true),
            (Vec3::new(2.0, 0.0, 0.0), true),
            (Vec3::new(4.0, 0.0, 0.0), true),
        ]);
        let list = compile(&mut d, ViewId::Front, |_| {});
        let order: Vec<u32> = list
            .items
            .iter()
            .map(target_first)
            .filter(|&f| f != u32::MAX)
            .collect();
        assert_eq!(order, [0, 1, 2]);
    }

    #[test]
    fn grids_follow_the_view_and_the_overlay_switch() {
        let mut d = doc(&[]);
        let planes = |list: &ViewDrawList| -> Vec<GridPlane> {
            list.items
                .iter()
                .filter_map(|i| {
                    if let DrawTarget::SubGrid(p) = i.target {
                        Some(p)
                    } else {
                        None
                    }
                })
                .collect()
        };
        assert_eq!(
            planes(&compile(&mut d, ViewId::Main, |_| {})),
            [GridPlane::Floor]
        );
        assert_eq!(planes(&compile(&mut d, ViewId::Side, |_| {})).len(), 2);
        assert_eq!(planes(&compile(&mut d, ViewId::Front, |_| {})).len(), 2);
        let off = compile(&mut d, ViewId::Main, |ex| {
            ex.cache_mut().set_overlay(OverlayState {
                grid: false,
                ..OverlayState::default()
            });
        });
        assert!(planes(&off).is_empty());
        assert!(!off.overlays.iter().any(|o| o.kind == OverlayKind::MainGrid));
    }

    #[test]
    fn hidden_elements_are_skipped_or_drawn_as_faint_boxes() {
        let mut d = doc(&[(Vec3::ZERO, false), (Vec3::new(3.0, 0.0, 0.0), false)]);
        let id = d.elements.ids()[1];
        let mut j = cuboid_model::ChangeJournal::default();
        d.elements.edit(id, &mut j).unwrap().base.hidden = true;
        let list = compile(&mut d, ViewId::Main, |_| {});
        let drawn = |list: &ViewDrawList| {
            list.items
                .iter()
                .filter(|i| target_first(i) != u32::MAX)
                .count()
        };
        assert_eq!(drawn(&list), 1);
        assert_eq!(
            list.items[0].target,
            DrawTarget::Elements { first: 0, count: 1 }
        );

        // As a box the element joins the translucent queue in every view; its record says so.
        let mut ex = Extractor::default();
        let prefs = RenderPrefs {
            hidden_as_box: true,
            ..RenderPrefs::default()
        };
        ex.extract(&mut d, &prefs, &ExtractBudget::UNLIMITED);
        let mut views = ViewSet::default();
        views.arrange(Rect {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
        });
        let list = compile_draw_list(views.view(ViewId::Main), ex.cache(), &prefs);
        assert_eq!(drawn(&list), 2);
        assert!(
            list.items
                .iter()
                .any(|i| i.translucent && target_first(i) == 1)
        );
    }

    #[test]
    fn the_outline_lists_selected_then_hovered_objects() {
        let mut d = doc(&[(Vec3::ZERO, false), (Vec3::new(3.0, 0.0, 0.0), true)]);
        let ids = d.elements.ids().to_vec();
        let list = compile(&mut d, ViewId::Main, |ex| {
            ex.cache_mut().set_overlay(OverlayState {
                selected: vec![ObjRef::Element(ids[1]), ObjRef::Blueprint],
                hovered: Some(ObjRef::Element(ids[0])),
                gizmo: Some((GizmoKind::Move, Mat4::IDENTITY)),
                ..OverlayState::default()
            });
        });
        let firsts: Vec<u32> = list.outline.iter().map(target_first).collect();
        assert_eq!(firsts, [1, 0]);
        assert!(
            list.overlays
                .iter()
                .any(|o| o.kind == OverlayKind::SelectionBox)
        );
        assert!(
            list.overlays
                .iter()
                .any(|o| o.kind == OverlayKind::Gizmo(GizmoKind::Move))
        );
        // A hovered object that is selected is not outlined twice.
        let list = compile(&mut d, ViewId::Main, |ex| {
            ex.cache_mut().set_overlay(OverlayState {
                selected: vec![ObjRef::Element(ids[0])],
                hovered: Some(ObjRef::Element(ids[0])),
                ..OverlayState::default()
            });
        });
        assert_eq!(list.outline.len(), 1);
    }

    #[test]
    fn selection_boxes_stop_at_five_objects() {
        let mut d = doc(&(0..6)
            .map(|i| (Vec3::new(i as f32 * 2.0, 0.0, 0.0), false))
            .collect::<Vec<_>>());
        let ids = d.elements.ids().to_vec();
        let boxes = |n: usize, d: &mut Document| {
            compile(d, ViewId::Main, |ex| {
                ex.cache_mut().set_overlay(OverlayState {
                    selected: ids.iter().take(n).map(|&i| ObjRef::Element(i)).collect(),
                    ..OverlayState::default()
                });
            })
            .overlays
            .iter()
            .filter(|o| o.kind == OverlayKind::SelectionBox)
            .count()
        };
        assert_eq!(boxes(4, &mut d), 4);
        assert_eq!(boxes(5, &mut d), 0);
    }

    #[test]
    fn the_frame_key_changes_only_with_what_a_view_shows() {
        let mut d = doc(&[(Vec3::ZERO, false)]);
        let mut ex = Extractor::default();
        ex.extract(&mut d, &RenderPrefs::default(), &ExtractBudget::UNLIMITED);
        let mut views = ViewSet::default();
        views.arrange(Rect {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
        });
        let prefs = RenderPrefs::default();
        let key = |views: &ViewSet, ex: &Extractor, prefs: &RenderPrefs| {
            compile_draw_list(views.view(ViewId::Main), ex.cache(), prefs).key
        };
        let base = key(&views, &ex, &prefs);
        assert_eq!(base.size, (800, 600));
        ex.extract(&mut d, &prefs, &ExtractBudget::UNLIMITED);
        assert_eq!(
            key(&views, &ex, &prefs),
            base,
            "an unchanged scene keeps the key"
        );
        views.view_mut(ViewId::Main).camera.pos.x += 1.0;
        assert_ne!(key(&views, &ex, &prefs).camera, base.camera);
        views.view_mut(ViewId::Main).camera.pos.x -= 1.0;
        assert_eq!(key(&views, &ex, &prefs), base);
        let wire = RenderPrefs {
            wireframe: true,
            ..RenderPrefs::default()
        };
        assert_ne!(key(&views, &ex, &wire).prefs, base.prefs);
        let id = d.elements.ids()[0];
        let mut j = cuboid_model::ChangeJournal::default();
        d.elements.edit(id, &mut j).unwrap().base.p.x = 5.0;
        ex.extract(&mut d, &prefs, &ExtractBudget::UNLIMITED);
        assert_ne!(key(&views, &ex, &prefs).scene, base.scene);
    }

    #[test]
    fn voxel_ranges_skip_hidden_directions_and_chunks_outside_the_frustum() {
        use cuboid_voxel::{VoxelWorld, mapping::grid_box};
        let mut d = doc(&[]);
        let mut world = VoxelWorld::new(16, false, grid_box(16));
        world.write_full(glam::IVec3::new(20, 20, 20), 0xFFFF_0000);
        world.write_full(glam::IVec3::new(36, 20, 20), 0xFFFF_0000);
        d.voxels = Some(world);
        let top = compile(&mut d, ViewId::Top, |_| {});
        // Looking down -Y only the +Y faces face the camera.
        let counts: Vec<usize> = top.voxel_ranges.iter().map(Vec::len).collect();
        assert_eq!(counts, [0, 2, 0, 0, 0, 0]);
        let main = compile(&mut d, ViewId::Main, |_| {});
        assert!(main.voxel_ranges.iter().map(Vec::len).sum::<usize>() >= 3);
        // The ranges point into the pools at the offsets the extract gave.
        let ranges = &top.voxel_ranges[1];
        assert!(ranges.iter().all(|r| r.count == 1));
        assert_ne!(ranges[0].first, ranges[1].first);
    }
}
