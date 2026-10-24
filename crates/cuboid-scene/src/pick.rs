//! `Raycaster` implementation: model, gizmo and drag-plane queries (spec: tools 8.4, 9.2; app 5.4;
//! arch 13.7).
//!
//! Elements and meshes sit in one BVH over their world boxes. A ray walks it front to back and
//! tests the leaves exactly: the active faces of an element's box, the front-facing triangles
//! of a mesh through a per-geometry triangle BVH. Voxels are tested by the DDA of the collider
//! snapshot, and the nearest of all hits wins.

use std::sync::Arc;

use cuboid_model::{
    ChangeJournal, DocFlags, Document, Element, ElementFlags, FaceName, Mesh, MeshGeometry, ObjId,
    ObjRef,
};
use cuboid_voxel::{Ray, VoxelWorld, dda};
use glam::{Vec2, Vec3};
use rustc_hash::FxHashMap;

use crate::bvh::Bvh;
use crate::camera::Camera;
use crate::colliders::VoxelColliders;
use crate::contract::{Hit, HitTarget, Raycaster};
use crate::frame::Frame;
use crate::geom::{Aabb, Frustum, ray_plane, ray_sphere, ray_triangle_front, to_u32, unit_or};
use crate::gizmo::{self, GizmoHit, GizmoState};
use crate::views::ViewId;

/// Radius of the model-origin marker.
const ORIGIN_RADIUS: f32 = 0.4;

/// Tolerance of the face-bounds test, so a ray along a shared edge hits.
const EDGE_EPS: f32 = 1e-5;

/// The corners of the unit box, `c0` to `c7` of tools 4.3.
const UNIT_CORNERS: [Vec3; 8] = [
    Vec3::new(0.0, 0.0, 0.0),
    Vec3::new(1.0, 0.0, 0.0),
    Vec3::new(1.0, 0.0, -1.0),
    Vec3::new(0.0, 0.0, -1.0),
    Vec3::new(0.0, 1.0, 0.0),
    Vec3::new(1.0, 1.0, 0.0),
    Vec3::new(1.0, 1.0, -1.0),
    Vec3::new(0.0, 1.0, -1.0),
];

/// The four corners of each face slot (down, up, north, south, west, east) with outer normals.
const FACE_CORNERS: [[usize; 4]; 6] = [
    [2, 3, 0, 1],
    [7, 6, 5, 4],
    [4, 5, 1, 0],
    [6, 7, 3, 2],
    [7, 4, 0, 3],
    [5, 6, 2, 1],
];

/// As [`FACE_CORNERS`] for inverted normals: each slot sits on the opposite plane.
const FACE_CORNERS_INVERTED: [[usize; 4]; 6] = [
    [6, 7, 4, 5],
    [3, 2, 1, 0],
    [7, 6, 2, 3],
    [5, 4, 0, 1],
    [6, 5, 1, 2],
    [4, 7, 3, 0],
];

/// The face normals: the axis and the sign of each slot.
const FACE_AXIS: [(usize, f32); 6] = [
    (1, -1.0),
    (1, 1.0),
    (2, 1.0),
    (2, -1.0),
    (0, -1.0),
    (0, 1.0),
];

/// The twelve triangles of the unit box as indices into [`UNIT_CORNERS`], following the
/// triangle rule of tools 4.3.
fn unit_box_mesh() -> (Vec<Vec3>, Vec<u32>) {
    let mut verts = Vec::with_capacity(24);
    let mut tris = Vec::with_capacity(36);
    for corners in FACE_CORNERS {
        let b = to_u32(verts.len());
        verts.extend(corners.map(|c| UNIT_CORNERS[c]));
        tris.extend([b + 3, b + 1, b, b + 3, b + 2, b + 1]);
    }
    (verts, tris)
}

/// The triangles of one geometry in a BVH, shared by every mesh object that uses the geometry.
#[derive(Debug)]
pub struct MeshTree {
    geometry: Arc<MeshGeometry>,
    /// The unit box, for geometry that is missing or unusable (a primitive not generated yet).
    fallback: Option<(Vec<Vec3>, Vec<u32>)>,
    bvh: Bvh,
    local: Aabb,
}

impl MeshTree {
    fn new(geometry: Arc<MeshGeometry>) -> MeshTree {
        let usable = geometry.triangles.len() >= 3
            && geometry
                .triangles
                .iter()
                .all(|&i| (i as usize) < geometry.vertices.len());
        let fallback = (!usable).then(unit_box_mesh);
        let (verts, tris) = match &fallback {
            Some((v, t)) => (v.as_slice(), t.as_slice()),
            None => (geometry.vertices.as_slice(), geometry.triangles.as_slice()),
        };
        let bounds: Vec<Aabb> = tris
            .chunks_exact(3)
            .map(|t| {
                t.iter()
                    .fold(Aabb::EMPTY, |b, &i| b.with_point(verts[i as usize]))
            })
            .map(|b| if b.is_valid() { b } else { Aabb::EMPTY })
            .collect();
        let local = bounds.iter().fold(Aabb::EMPTY, |a, b| a.union(*b));
        let bvh = Bvh::build(&bounds);
        MeshTree {
            geometry,
            fallback,
            bvh,
            local,
        }
    }

    fn verts(&self) -> &[Vec3] {
        match &self.fallback {
            Some((v, _)) => v,
            None => &self.geometry.vertices,
        }
    }

    fn tris(&self) -> &[u32] {
        match &self.fallback {
            Some((_, t)) => t,
            None => &self.geometry.triangles,
        }
    }

    /// The triangle `index` as three vertex positions.
    fn triangle(&self, index: u32) -> Option<[Vec3; 3]> {
        let at = index as usize * 3;
        let ids = self.tris().get(at..at + 3)?;
        let verts = self.verts();
        Some([
            *verts.get(ids[0] as usize)?,
            *verts.get(ids[1] as usize)?,
            *verts.get(ids[2] as usize)?,
        ])
    }

    /// Nearest front-facing triangle for a ray in the mesh's local space.
    fn ray(&self, origin: Vec3, dir: Vec3, limit: f32) -> Option<(f32, u32, f32, f32)> {
        let mut found = None;
        self.bvh
            .nearest(origin, dir, limit, |tri, limit| {
                let [a, b, c] = self.triangle(tri)?;
                let hit = ray_triangle_front(origin, dir, a, b, c, limit)?;
                found = Some((hit.t, tri, hit.u, hit.v));
                Some(hit.t)
            })
            .and(found)
    }
}

enum Shape {
    Element(Arc<Element>),
    Mesh(Arc<Mesh>, Arc<MeshTree>),
}

struct Item {
    id: ObjId,
    shape: Shape,
    frame: Option<Frame>,
}

impl Item {
    fn hidden(&self) -> bool {
        match &self.shape {
            Shape::Element(e) => e.base.hidden,
            Shape::Mesh(m, _) => m.base.hidden,
        }
    }

    /// The world box; empty for a hidden object or one without a frame.
    fn world_bounds(&self) -> Aabb {
        let Some(frame) = self.frame else {
            return Aabb::EMPTY;
        };
        if self.hidden() {
            return Aabb::EMPTY;
        }
        match &self.shape {
            Shape::Element(_) => frame.obb().aabb(),
            Shape::Mesh(_, tree) => mesh_obb_corners(&frame, tree)
                .iter()
                .fold(Aabb::EMPTY, |b, &c| b.with_point(c)),
        }
    }

    /// The eight corners of the oriented box, for box selection.
    fn corners(&self) -> Option<[Vec3; 8]> {
        let frame = self.frame?;
        match &self.shape {
            Shape::Element(_) => Some(frame.obb().corners()),
            Shape::Mesh(_, tree) => Some(mesh_obb_corners(&frame, tree)),
        }
    }
}

/// The corners of the local box of a mesh in world space.
fn mesh_obb_corners(frame: &Frame, tree: &MeshTree) -> [Vec3; 8] {
    let (lo, hi) = if tree.local.is_valid() {
        (tree.local.min, tree.local.max)
    } else {
        (Vec3::ZERO, Vec3::ZERO)
    };
    std::array::from_fn(|i| {
        frame.to_world(Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        ))
    })
}

/// What a leaf test found, to build the [`Hit`] once the nearest is known.
#[derive(Copy, Clone)]
enum Found {
    Face { slot: usize, local_point: Vec3 },
    Triangle { tri: u32, u: f32, v: f32 },
}

/// The picking structures: a BVH over element and mesh bounds, a triangle BVH per mesh geometry
/// and the voxel collider snapshot. It is synchronised from the document and its change journal
/// once per frame.
///
/// spec: arch 13.7, arch 6 (cuboid-scene)
#[derive(Default)]
pub struct SceneIndex {
    items: Vec<Item>,
    bounds: Vec<Aabb>,
    bvh: Bvh,
    seen: [Option<cuboid_model::Rev>; 4],
    trees: FxHashMap<usize, Arc<MeshTree>>,
    blueprint: Option<(Frame, bool)>,
    origin: Option<Vec3>,
    origin_pickable: bool,
    colliders: VoxelColliders,
    collider_rev: Option<cuboid_voxel::Rev>,
    cameras: Option<[Camera; 4]>,
    drag_view: Option<ViewId>,
}

impl std::fmt::Debug for SceneIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneIndex")
            .field("items", &self.items.len())
            .field("voxels", &self.colliders.world.is_some())
            .finish_non_exhaustive()
    }
}

impl SceneIndex {
    /// Brings the index up to date with the document: refits moved objects, rebuilds after
    /// structural changes.
    pub fn sync(&mut self, doc: &Document, journal: &ChangeJournal) {
        self.sync_objects(doc);
        self.blueprint = Frame::of(&doc.blueprint.base).map(|f| (f, doc.blueprint.base.hidden));
        let origin = doc.props.general.origin;
        self.origin = origin.is_finite().then_some(origin);
        self.sync_colliders(doc, journal);
    }

    fn sync_objects(&mut self, doc: &Document) {
        let seen = [
            Some(doc.elements.rev()),
            Some(doc.elements.structure_rev()),
            Some(doc.meshes.rev()),
            Some(doc.meshes.structure_rev()),
        ];
        if seen == self.seen {
            return;
        }
        let structural = seen[1] != self.seen[1]
            || seen[3] != self.seen[3]
            || self.items.len() != doc.elements.len() + doc.meshes.len();
        self.seen = seen;
        if structural {
            self.rebuild(doc);
            return;
        }
        let mut changed = false;
        let n = doc.elements.len();
        for (i, arc) in doc.elements.items().iter().enumerate() {
            let same = matches!(&self.items[i].shape, Shape::Element(old) if Arc::ptr_eq(old, arc));
            if !same {
                self.items[i] = self.element_item(doc.elements.ids()[i], arc);
                self.bounds[i] = self.items[i].world_bounds();
                changed = true;
            }
        }
        for (i, arc) in doc.meshes.items().iter().enumerate() {
            let same =
                matches!(&self.items[n + i].shape, Shape::Mesh(old, _) if Arc::ptr_eq(old, arc));
            if !same {
                self.items[n + i] = self.mesh_item(doc.meshes.ids()[i], arc);
                self.bounds[n + i] = self.items[n + i].world_bounds();
                changed = true;
            }
        }
        if changed && (!self.bvh.refit(&self.bounds) || self.bvh.degraded()) {
            self.bvh = Bvh::build(&self.bounds);
        }
        if changed {
            self.prune_trees();
        }
    }

    fn rebuild(&mut self, doc: &Document) {
        self.items.clear();
        for (id, arc) in doc.elements.ids().iter().zip(doc.elements.items()) {
            let item = self.element_item(*id, arc);
            self.items.push(item);
        }
        for (id, arc) in doc.meshes.ids().iter().zip(doc.meshes.items()) {
            let item = self.mesh_item(*id, arc);
            self.items.push(item);
        }
        self.bounds = self.items.iter().map(Item::world_bounds).collect();
        self.bvh = Bvh::build(&self.bounds);
        self.prune_trees();
    }

    fn element_item(&self, id: ObjId, arc: &Arc<Element>) -> Item {
        Item {
            id,
            frame: Frame::of(&arc.base),
            shape: Shape::Element(Arc::clone(arc)),
        }
    }

    fn mesh_item(&mut self, id: ObjId, arc: &Arc<Mesh>) -> Item {
        let key = Arc::as_ptr(&arc.geometry) as usize;
        let tree = self
            .trees
            .entry(key)
            .or_insert_with(|| Arc::new(MeshTree::new(Arc::clone(&arc.geometry))))
            .clone();
        Item {
            id,
            frame: Frame::of(&arc.base),
            shape: Shape::Mesh(Arc::clone(arc), tree),
        }
    }

    /// Drops the triangle trees no mesh uses any more.
    fn prune_trees(&mut self) {
        self.trees.retain(|_, tree| Arc::strong_count(tree) > 1);
    }

    fn sync_colliders(&mut self, doc: &Document, journal: &ChangeJournal) {
        if self.colliders.frozen {
            return;
        }
        let Some(world) = &doc.voxels else {
            self.colliders.world = None;
            self.collider_rev = None;
            return;
        };
        let flagged = journal.doc().contains(DocFlags::VOXEL_CHUNKS);
        if flagged || self.collider_rev != Some(world.rev) || self.colliders.world.is_none() {
            self.colliders.world = Some(Arc::new(world.clone()));
            self.collider_rev = Some(world.rev);
        }
    }

    /// Freezes the voxel colliders when a stroke begins.
    ///
    /// spec: tools 12.2
    pub fn freeze_voxel_colliders(&mut self) {
        self.colliders.frozen = true;
    }

    /// Replaces the voxel colliders by `world` and lifts the freeze: a rebuild pass completed or
    /// the stroke ended.
    ///
    /// spec: tools 7.6, tools 12.2
    pub fn refresh_voxel_colliders(&mut self, world: &VoxelWorld) {
        self.colliders.world = Some(Arc::new(world.clone()));
        self.collider_rev = Some(world.rev);
        self.colliders.frozen = false;
    }

    /// Whether the model-origin marker can be hit (it exists while it is selected).
    pub fn set_origin_pickable(&mut self, pickable: bool) {
        self.origin_pickable = pickable;
    }

    /// Hands over the view cameras, which the gizmo hit volumes and the drag plane need: the
    /// gizmos keep their size on the screen, and the drag plane faces the camera of `active`.
    pub fn set_cameras(&mut self, cameras: [Camera; 4], active: ViewId) {
        self.cameras = Some(cameras);
        self.drag_view = Some(active);
    }

    fn camera(&self, view: ViewId) -> Camera {
        self.cameras
            .map_or_else(|| Camera::default_for(view), |c| c[view.index()])
    }

    /// The elements and meshes whose box the frustum touches (or, with `contain`, holds
    /// entirely), in list order. Hidden objects are skipped. The touch test is conservative at
    /// the corners of the frustum.
    pub fn select_frustum(&self, frustum: &Frustum, contain: bool) -> Vec<ObjRef> {
        let mut hits = Vec::new();
        self.bvh.query_frustum(frustum, |i| hits.push(i));
        hits.sort_unstable();
        hits.into_iter()
            .filter_map(|i| {
                let item = self.items.get(i as usize)?;
                if item.hidden() {
                    return None;
                }
                let corners = item.corners()?;
                let ok = if contain {
                    frustum.contains_corners(&corners)
                } else {
                    frustum.intersects_corners(&corners)
                };
                ok.then_some(match item.shape {
                    Shape::Element(_) => ObjRef::Element(item.id),
                    Shape::Mesh(..) => ObjRef::Mesh(item.id),
                })
            })
            .collect()
    }

    /// The union of the world boxes of the objects, for framing; `None` when none has a box.
    pub fn bounds_of(&self, objects: &[ObjRef]) -> Option<Aabb> {
        let wanted: rustc_hash::FxHashSet<ObjRef> = objects.iter().copied().collect();
        let all = self
            .items
            .iter()
            .zip(&self.bounds)
            .filter(|(item, _)| match item.shape {
                Shape::Element(_) => wanted.contains(&ObjRef::Element(item.id)),
                Shape::Mesh(..) => wanted.contains(&ObjRef::Mesh(item.id)),
            })
            .fold(Aabb::EMPTY, |a, (_, b)| a.union(*b));
        all.is_valid().then_some(all)
    }

    fn hit_element(
        &self,
        element: &Element,
        frame: &Frame,
        ray: Ray,
        limit: f32,
    ) -> Option<(f32, Found)> {
        let origin = frame.inv_rot * (ray.origin - frame.pos);
        let dir = frame.inv_rot * ray.dir;
        let active = std::array::from_fn(|i| element.faces[i].active);
        let (t, slot) = ray_box(
            origin,
            dir,
            frame.size,
            active,
            element.flags.contains(ElementFlags::INVERT_NORMALS),
            limit,
        )?;
        Some((
            t,
            Found::Face {
                slot,
                local_point: origin + dir * t,
            },
        ))
    }

    fn hit_mesh(
        &self,
        frame: &Frame,
        tree: &MeshTree,
        ray: Ray,
        limit: f32,
    ) -> Option<(f32, Found)> {
        let size = frame.size;
        if size.min_element() < 1e-9 {
            return None;
        }
        let origin = frame.inv_rot * (ray.origin - frame.pos) / size;
        let dir = frame.inv_rot * ray.dir / size;
        let (t, tri, u, v) = tree.ray(origin, dir, limit)?;
        Some((t, Found::Triangle { tri, u, v }))
    }

    fn model_hit(&self, ray: Ray, max_len: f32) -> Option<Hit> {
        let mut best: Option<(f32, usize, Found)> = None;
        self.bvh.nearest(ray.origin, ray.dir, max_len, |i, limit| {
            let item = &self.items[i as usize];
            let frame = item.frame.as_ref()?;
            let (t, found) = match &item.shape {
                Shape::Element(e) => self.hit_element(e, frame, ray, limit)?,
                Shape::Mesh(_, tree) => self.hit_mesh(frame, tree, ray, limit)?,
            };
            best = Some((t, i as usize, found));
            Some(t)
        })?;
        let (t, i, found) = best?;
        let item = &self.items[i];
        let frame = item.frame.as_ref()?;
        let point = ray.origin + ray.dir * t;
        Some(match (&item.shape, found) {
            (Shape::Element(e), Found::Face { slot, local_point }) => Hit {
                target: HitTarget::Element(item.id),
                point,
                normal: face_normal(frame, slot),
                face: FaceName::ALL.get(slot).copied(),
                uv: face_uv(e, slot, local_point),
            },
            (Shape::Mesh(m, tree), Found::Triangle { tri, u, v }) => {
                let [a, b, c] = tree.triangle(tri)?;
                let n_local = (b - a).cross(c - a) / frame.size.max(Vec3::splat(1e-9));
                let uv = (m.geometry.uvs.len() == m.geometry.vertices.len()
                    && tree.fallback.is_none())
                .then(|| triangle_uv(&m.geometry, tree, tri, u, v))
                .flatten();
                Hit {
                    target: HitTarget::Mesh(item.id),
                    point,
                    normal: unit_or(frame.rot * n_local, Vec3::Y),
                    face: None,
                    uv,
                }
            }
            _ => return None,
        })
    }

    fn blueprint_hit(&self, ray: Ray, max_len: f32) -> Option<Hit> {
        let (frame, hidden) = self.blueprint.as_ref()?;
        if *hidden {
            return None;
        }
        let origin = frame.inv_rot * (ray.origin - frame.pos);
        let dir = frame.inv_rot * ray.dir;
        let (t, slot) = ray_box(origin, dir, frame.size, [true; 6], false, max_len)?;
        Some(Hit {
            target: HitTarget::Blueprint,
            point: ray.origin + ray.dir * t,
            normal: face_normal(frame, slot),
            face: FaceName::ALL.get(slot).copied(),
            uv: None,
        })
    }

    fn origin_hit(&self, ray: Ray, max_len: f32) -> Option<Hit> {
        if !self.origin_pickable {
            return None;
        }
        let center = self.origin?;
        let t = ray_sphere(ray.origin, ray.dir, center, ORIGIN_RADIUS, max_len)?;
        let point = ray.origin + ray.dir * t;
        Some(Hit {
            target: HitTarget::ModelOrigin,
            point,
            normal: unit_or(point - center, Vec3::Y),
            face: None,
            uv: None,
        })
    }

    fn voxel_hit(&self, ray: Ray, max_len: f32) -> Option<(f32, Hit)> {
        let world = self.colliders.world.as_ref()?;
        let hit = dda(world, ray, max_len)?;
        Some((
            hit.distance,
            Hit {
                target: HitTarget::Voxel,
                point: hit.point,
                normal: hit.normal,
                face: None,
                uv: None,
            },
        ))
    }
}

/// The nearest front-facing active face of a box in its local frame, `x` in `0..size.x`, `y` in
/// `0..size.y` and `z` in `-size.z..0`. With `inverted`, each slot sits on the opposite plane
/// and keeps its normal, so it faces the inside. The ray direction need not be a unit vector.
fn ray_box(
    origin: Vec3,
    dir: Vec3,
    size: Vec3,
    active: [bool; 6],
    inverted: bool,
    limit: f32,
) -> Option<(f32, usize)> {
    let (min, max) = (Vec3::new(0.0, 0.0, -size.z), Vec3::new(size.x, size.y, 0.0));
    let mut best: Option<(f32, usize)> = None;
    for (slot, (&(axis, sign), on)) in FACE_AXIS.iter().zip(active).enumerate() {
        // Front-facing means the ray runs against the slot's normal.
        if !on || dir[axis] * sign >= 0.0 {
            continue;
        }
        let plane = if (sign > 0.0) != inverted {
            max[axis]
        } else {
            min[axis]
        };
        let t = (plane - origin[axis]) / dir[axis];
        if !(t >= 0.0 && t <= limit) || best.is_some_and(|(b, _)| b <= t) {
            continue;
        }
        let p = origin + dir * t;
        let inside = (0..3)
            .filter(|&a| a != axis)
            .all(|a| p[a] >= min[a] - EDGE_EPS && p[a] <= max[a] + EDGE_EPS);
        if inside {
            best = Some((t, slot));
        }
    }
    best
}

/// The world normal of a face slot.
fn face_normal(frame: &Frame, slot: usize) -> Vec3 {
    let (axis, sign) = FACE_AXIS.get(slot).copied().unwrap_or((1, 1.0));
    let mut n = Vec3::ZERO;
    n[axis] = sign;
    frame.rot * n
}

/// The texture coordinate of a point on a face in the face's own texture: the rotated UV corners
/// interpolated over the quad, vertices `v0..v3` carrying `C2, C3, C0, C1` (`C0..C3` for the top
/// face) as in tools 4.3.
fn face_uv(element: &Element, slot: usize, local_point: Vec3) -> Option<Vec2> {
    let face = element.faces.get(slot)?;
    let corners = face.uv.rotated_corners();
    let order: [usize; 4] = if slot == FaceName::Up as usize {
        [0, 1, 2, 3]
    } else {
        [2, 3, 0, 1]
    };
    let table = if element.flags.contains(ElementFlags::INVERT_NORMALS) {
        &FACE_CORNERS_INVERTED[slot]
    } else {
        &FACE_CORNERS[slot]
    };
    let size = element.base.s.abs();
    let v: [Vec3; 4] = std::array::from_fn(|i| UNIT_CORNERS[table[i]] * size);
    let (e1, e3) = (v[1] - v[0], v[3] - v[0]);
    let rel = local_point - v[0];
    let (l1, l3) = (e1.length_squared(), e3.length_squared());
    let s = if l1 > 1e-12 {
        (rel.dot(e1) / l1).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let t = if l3 > 1e-12 {
        (rel.dot(e3) / l3).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let c = |i: usize| corners[order[i]];
    let uv = c(0) * ((1.0 - s) * (1.0 - t))
        + c(1) * (s * (1.0 - t))
        + c(2) * (s * t)
        + c(3) * ((1.0 - s) * t);
    uv.is_finite().then_some(uv)
}

/// The interpolated texture coordinate on a mesh triangle.
fn triangle_uv(geometry: &MeshGeometry, tree: &MeshTree, tri: u32, u: f32, v: f32) -> Option<Vec2> {
    let at = tri as usize * 3;
    let ids = tree.tris().get(at..at + 3)?;
    let uv = |i: u32| geometry.uvs.get(i as usize).copied();
    Some(uv(ids[0])? * (1.0 - u - v) + uv(ids[1])? * u + uv(ids[2])? * v)
}

impl Raycaster for SceneIndex {
    fn pick_model(&self, ray: Ray, max_len: f32) -> Option<Hit> {
        if !(ray.origin.is_finite() && ray.dir.is_finite() && max_len.is_finite() && max_len > 0.0)
        {
            return None;
        }
        let dir = unit_or(ray.dir, Vec3::ZERO);
        if dir == Vec3::ZERO {
            return None;
        }
        let ray = Ray {
            origin: ray.origin,
            dir,
        };
        let mut best: Option<(f32, Hit)> = None;
        let mut consider = |hit: Option<Hit>, t: Option<f32>| {
            if let Some(hit) = hit {
                let t = t.unwrap_or_else(|| (hit.point - ray.origin).dot(ray.dir));
                if best.as_ref().is_none_or(|(b, _)| t < *b) {
                    best = Some((t, hit));
                }
            }
        };
        consider(self.model_hit(ray, max_len), None);
        consider(self.blueprint_hit(ray, max_len), None);
        consider(self.origin_hit(ray, max_len), None);
        if let Some((t, hit)) = self.voxel_hit(ray, max_len) {
            consider(Some(hit), Some(t));
        }
        best.map(|(_, hit)| hit)
    }

    fn pick_gizmo(&self, ray: Ray, view: ViewId, state: &GizmoState) -> Option<GizmoHit> {
        let dir = unit_or(ray.dir, Vec3::ZERO);
        if !ray.origin.is_finite() || dir == Vec3::ZERO {
            return None;
        }
        gizmo::pick(
            state,
            &self.camera(view),
            view,
            Ray {
                origin: ray.origin,
                dir,
            },
        )
    }

    fn drag_plane(&self, ray: Ray, through: Vec3, max_len: f32) -> Option<Vec3> {
        let dir = unit_or(ray.dir, Vec3::ZERO);
        if !(ray.origin.is_finite() && through.is_finite() && max_len.is_finite())
            || dir == Vec3::ZERO
        {
            return None;
        }
        let facing = self.drag_view.map_or(-dir, |v| -self.camera(v).forward());
        let t = ray_plane(ray.origin, dir, through, facing, max_len)?;
        Some(ray.origin + dir * t)
    }
}

#[cfg(test)]
mod tests {
    use cuboid_model::{ElementType, Label, MeshBounds, MeshKind, Perms, TextureList, uv::UvRect};
    use proptest::prelude::*;

    use super::*;

    fn doc_with(elements: &[(Vec3, Vec3, Vec3)]) -> Document {
        let mut doc = Document::new(TextureList::default());
        // The placement box would be hit like any other object.
        doc.blueprint.base.hidden = true;
        for &(p, s, e) in elements {
            let mut el = Element::new(
                ElementType::Cube,
                &doc.blueprint,
                1,
                [1.0; 4],
                &Label::from("t"),
            );
            el.base.p = p;
            el.base.s = s;
            el.base.e = e;
            doc.elements.push(el);
        }
        doc
    }

    fn index(doc: &Document) -> SceneIndex {
        let mut idx = SceneIndex::default();
        idx.sync(doc, &ChangeJournal::default());
        idx
    }

    fn down(x: f32, y: f32) -> Ray {
        Ray {
            origin: Vec3::new(x, y, 50.0),
            dir: Vec3::NEG_Z,
        }
    }

    #[test]
    fn a_ray_hits_the_front_face_of_an_axis_aligned_element() {
        let doc = doc_with(&[(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(4.0, 4.0, 4.0),
            Vec3::ZERO,
        )]);
        let idx = index(&doc);
        let hit = idx.pick_model(down(1.0, 1.0), 1000.0).unwrap();
        assert_eq!(hit.face, Some(FaceName::North));
        assert!((hit.point - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-5);
        assert_eq!(hit.normal, Vec3::Z);
        assert!(matches!(hit.target, HitTarget::Element(_)));
        // The box extends toward -Z from the position, so the ray from -Z hits the south face.
        let from_back = Ray {
            origin: Vec3::new(1.0, 1.0, -50.0),
            dir: Vec3::Z,
        };
        let hit = idx.pick_model(from_back, 1000.0).unwrap();
        assert_eq!(hit.face, Some(FaceName::South));
        assert!((hit.point.z + 4.0).abs() < 1e-5);
        assert!(idx.pick_model(down(9.0, 1.0), 1000.0).is_none());
    }

    #[test]
    fn the_ray_length_limits_the_hit() {
        let doc = doc_with(&[(Vec3::ZERO, Vec3::splat(4.0), Vec3::ZERO)]);
        let idx = index(&doc);
        assert!(idx.pick_model(down(1.0, 1.0), 49.0).is_none());
        assert!(idx.pick_model(down(1.0, 1.0), 51.0).is_some());
    }

    #[test]
    fn hidden_objects_and_inactive_faces_are_not_hit() {
        let mut doc = doc_with(&[(Vec3::ZERO, Vec3::splat(4.0), Vec3::ZERO)]);
        let id = doc.elements.ids()[0];
        let mut j = ChangeJournal::default();
        doc.elements.edit(id, &mut j).unwrap().faces[FaceName::North as usize].active = false;
        let idx = index(&doc);
        // The north face is off: the ray sees through to the inside of the south face, which
        // faces away, so nothing is hit.
        assert!(idx.pick_model(down(1.0, 1.0), 1000.0).is_none());
        doc.elements.edit(id, &mut j).unwrap().faces[FaceName::North as usize].active = true;
        doc.elements.edit(id, &mut j).unwrap().base.hidden = true;
        let idx = index(&doc);
        assert!(idx.pick_model(down(1.0, 1.0), 1000.0).is_none());
    }

    #[test]
    fn inverted_normals_are_seen_from_inside() {
        let mut doc = doc_with(&[(Vec3::ZERO, Vec3::splat(4.0), Vec3::ZERO)]);
        let id = doc.elements.ids()[0];
        let mut j = ChangeJournal::default();
        doc.elements.edit(id, &mut j).unwrap().flags |= ElementFlags::INVERT_NORMALS;
        let idx = index(&doc);
        // Seen from outside the near faces are culled and the inside of the far wall shows: the
        // north slot, drawn on the south plane, keeps its normal.
        let hit = idx.pick_model(down(1.0, 1.0), 1000.0).unwrap();
        assert_eq!(hit.face, Some(FaceName::North));
        assert!((hit.point.z + 4.0).abs() < 1e-5);
        let inside = Ray {
            origin: Vec3::new(1.0, 1.0, -1.0),
            dir: Vec3::NEG_Z,
        };
        let hit = idx.pick_model(inside, 100.0).unwrap();
        assert_eq!(hit.face, Some(FaceName::North));
        assert!((hit.point.z + 4.0).abs() < 1e-5);
        // Looking the other way from inside shows the south slot on the north plane.
        let inside_back = Ray {
            origin: Vec3::new(1.0, 1.0, -1.0),
            dir: Vec3::Z,
        };
        let hit = idx.pick_model(inside_back, 100.0).unwrap();
        assert_eq!(hit.face, Some(FaceName::South));
        assert!(hit.point.z.abs() < 1e-5);
    }

    #[test]
    fn the_hit_reports_the_texture_coordinate_of_the_face() {
        let mut doc = doc_with(&[(Vec3::ZERO, Vec3::new(4.0, 2.0, 4.0), Vec3::ZERO)]);
        let id = doc.elements.ids()[0];
        let mut j = ChangeJournal::default();
        doc.elements.edit(id, &mut j).unwrap().faces[FaceName::North as usize].uv =
            UvRect::new(0, Vec2::new(0.25, 0.5), Vec2::new(0.75, 1.0));
        let idx = index(&doc);
        // North face vertices: c4 (0,1,0) c5 (1,1,0) c1 (1,0,0) c0 (0,0,0) carry C2 C3 C0 C1,
        // that is (0.75,1) (0.25,1) (0.25,0.5) (0.75,0.5): seen from +Z, x grows to the left.
        let hit = idx.pick_model(down(0.0, 0.0), 1000.0).unwrap();
        let uv = hit.uv.unwrap();
        assert!((uv - Vec2::new(0.75, 0.5)).length() < 1e-5, "{uv:?}");
        let hit = idx
            .pick_model(down(4.0 - 1e-4, 2.0 - 1e-4), 1000.0)
            .unwrap();
        let uv = hit.uv.unwrap();
        assert!((uv - Vec2::new(0.25, 1.0)).length() < 1e-3, "{uv:?}");
        let hit = idx.pick_model(down(2.0, 1.0), 1000.0).unwrap();
        assert!((hit.uv.unwrap() - Vec2::new(0.5, 0.75)).length() < 1e-5);
    }

    #[test]
    fn rotated_elements_are_hit_by_their_rotated_faces() {
        // A 45 degree turn about Z puts a corner where the unrotated face centre was.
        let doc = doc_with(&[(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 2.0, 2.0),
            Vec3::new(0.0, 0.0, 45.0),
        )]);
        let idx = index(&doc);
        let centre = Frame::of(&doc.elements.items()[0].base)
            .unwrap()
            .obb()
            .center;
        let hit = idx.pick_model(down(centre.x, centre.y), 1000.0).unwrap();
        assert!((hit.point.z - centre.z - 1.0).abs() < 1e-4);
        assert!((hit.normal - Vec3::Z).length() < 1e-4);
        // Just outside the diamond, on the unrotated box's corner, nothing is hit.
        assert!(
            idx.pick_model(down(centre.x + 1.35, centre.y + 1.35), 1000.0)
                .is_none()
        );
    }

    #[test]
    fn the_nearest_of_several_objects_wins() {
        let doc = doc_with(&[
            (Vec3::new(0.0, 0.0, -10.0), Vec3::splat(2.0), Vec3::ZERO),
            (Vec3::new(0.0, 0.0, 10.0), Vec3::splat(2.0), Vec3::ZERO),
        ]);
        let idx = index(&doc);
        let hit = idx.pick_model(down(1.0, 1.0), 1000.0).unwrap();
        assert!((hit.point.z - 10.0).abs() < 1e-5);
        assert_eq!(hit.target, HitTarget::Element(doc.elements.ids()[1]));
    }

    #[test]
    fn the_blueprint_and_the_origin_marker_can_be_picked() {
        let mut doc = Document::new(TextureList::default());
        doc.blueprint.base.p = Vec3::new(0.0, 0.0, 0.0);
        doc.blueprint.base.s = Vec3::splat(2.0);
        let mut props = (*doc.props).clone();
        props.general.origin = Vec3::new(20.0, 0.0, 0.0);
        doc.props = Arc::new(props);
        let mut idx = index(&doc);
        let hit = idx.pick_model(down(1.0, 1.0), 1000.0).unwrap();
        assert_eq!(hit.target, HitTarget::Blueprint);
        assert!(idx.pick_model(down(20.0, 0.0), 1000.0).is_none());
        idx.set_origin_pickable(true);
        let hit = idx.pick_model(down(20.0, 0.0), 1000.0).unwrap();
        assert_eq!(hit.target, HitTarget::ModelOrigin);
    }

    #[test]
    fn a_stroke_freezes_the_voxel_colliders_until_they_are_refreshed() {
        use cuboid_voxel::mapping::grid_box;
        let mut doc = Document::new(TextureList::default());
        let mut world = VoxelWorld::new(16, false, grid_box(16));
        world.write_full(glam::IVec3::new(16, 16, 16), 0xFFFF_0000);
        doc.voxels = Some(world);
        let mut idx = SceneIndex::default();
        idx.sync(&doc, &ChangeJournal::default());
        let ray = Ray {
            origin: Vec3::new(0.5, 0.5, 50.0),
            dir: Vec3::NEG_Z,
        };
        let hit = idx.pick_model(ray, 1000.0).unwrap();
        assert_eq!(hit.target, HitTarget::Voxel);
        assert_eq!(hit.normal, Vec3::Z);

        idx.freeze_voxel_colliders();
        let world = doc.voxels.as_mut().unwrap();
        world.write_full(glam::IVec3::new(16, 16, 16), 0);
        idx.sync(&doc, &ChangeJournal::default());
        assert!(
            idx.pick_model(ray, 1000.0).is_some(),
            "frozen colliders keep the old world"
        );
        idx.refresh_voxel_colliders(doc.voxels.as_ref().unwrap());
        assert!(idx.pick_model(ray, 1000.0).is_none());
        // A voxel edit is seen on the next sync once the freeze is lifted.
        let world = doc.voxels.as_mut().unwrap();
        world.write_full(glam::IVec3::new(16, 16, 16), 0xFF00_FF00);
        idx.sync(&doc, &ChangeJournal::default());
        assert!(idx.pick_model(ray, 1000.0).is_some());
    }

    #[test]
    fn meshes_are_hit_through_their_triangles() {
        let mut doc = Document::new(TextureList::default());
        let geometry = Arc::new(MeshGeometry {
            vertices: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            normals: None,
            uvs: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0),
            ],
            triangles: vec![0, 1, 2],
        });
        let mesh = Mesh {
            base: cuboid_model::ObjectBase::new(
                "m",
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::splat(4.0),
                Perms::PRIMITIVE,
            ),
            kind: MeshKind::Custom,
            geometry,
            texture: None,
            submeshes: Arc::from([]),
            bounds: MeshBounds::default(),
            norm_factor: 1.0,
            import_offset: Vec3::ZERO,
            tint: [1.0; 4],
        };
        let id = doc.meshes.push(mesh);
        let idx = index(&doc);
        // The triangle spans x 2..6, y 0..4 at z 0; its normal (1,0,0)x(0,1,0) is +Z.
        let hit = idx.pick_model(down(3.0, 1.0), 1000.0).unwrap();
        assert_eq!(hit.target, HitTarget::Mesh(id));
        assert!((hit.normal - Vec3::Z).length() < 1e-5);
        let uv = hit.uv.unwrap();
        assert!((uv - Vec2::new(0.25, 0.25)).length() < 1e-5);
        assert!(idx.pick_model(down(5.5, 3.5), 1000.0).is_none());
        let back = Ray {
            origin: Vec3::new(3.0, 1.0, -50.0),
            dir: Vec3::Z,
        };
        assert!(
            idx.pick_model(back, 1000.0).is_none(),
            "back faces are not hit"
        );
    }

    #[test]
    fn a_primitive_without_stored_geometry_is_picked_as_its_box() {
        let mut doc = Document::new(TextureList::default());
        let mesh = Mesh {
            base: cuboid_model::ObjectBase::new(
                "m",
                Vec3::ZERO,
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
        doc.meshes.push(mesh);
        let idx = index(&doc);
        let hit = idx.pick_model(down(1.0, 1.0), 1000.0).unwrap();
        assert!((hit.point.z).abs() < 1e-5 && (hit.normal - Vec3::Z).length() < 1e-5);
    }

    #[test]
    fn edits_refit_and_structure_changes_rebuild() {
        let mut doc = doc_with(&[(Vec3::ZERO, Vec3::splat(2.0), Vec3::ZERO)]);
        let id = doc.elements.ids()[0];
        let mut idx = index(&doc);
        assert!(idx.pick_model(down(1.0, 1.0), 1000.0).is_some());
        let mut j = ChangeJournal::default();
        doc.elements.edit(id, &mut j).unwrap().base.p = Vec3::new(30.0, 0.0, 0.0);
        idx.sync(&doc, &j);
        assert!(idx.pick_model(down(1.0, 1.0), 1000.0).is_none());
        assert!(idx.pick_model(down(31.0, 1.0), 1000.0).is_some());
        let mut el = (*doc.elements.items()[0]).clone();
        el.base.p = Vec3::ZERO;
        doc.elements.push(el);
        idx.sync(&doc, &j);
        assert!(idx.pick_model(down(1.0, 1.0), 1000.0).is_some());
        assert_eq!(idx.items.len(), 2);
    }

    #[test]
    fn degenerate_rays_and_objects_never_hit_or_panic() {
        let mut doc = doc_with(&[
            (Vec3::new(f32::NAN, 0.0, 0.0), Vec3::splat(2.0), Vec3::ZERO),
            (Vec3::ZERO, Vec3::new(0.0, 0.0, 0.0), Vec3::ZERO),
            (Vec3::ZERO, Vec3::splat(f32::INFINITY), Vec3::ZERO),
        ]);
        let mut idx = index(&doc);
        for ray in [
            Ray {
                origin: Vec3::NAN,
                dir: Vec3::Z,
            },
            Ray {
                origin: Vec3::ZERO,
                dir: Vec3::ZERO,
            },
            Ray {
                origin: Vec3::ZERO,
                dir: Vec3::new(f32::NAN, 0.0, 1.0),
            },
            Ray {
                origin: Vec3::splat(1e30),
                dir: Vec3::Z,
            },
        ] {
            assert!(idx.pick_model(ray, 1000.0).is_none());
        }
        assert!(idx.pick_model(down(0.0, 0.0), f32::NAN).is_none());
        assert!(idx.pick_model(down(0.0, 0.0), -1.0).is_none());
        doc.elements.clear();
        idx.sync(&doc, &ChangeJournal::default());
        assert!(idx.pick_model(down(0.0, 0.0), 100.0).is_none());
    }

    #[test]
    fn the_drag_plane_faces_the_active_camera() {
        let mut idx = SceneIndex::default();
        let cams = ViewId::ALL.map(Camera::default_for);
        idx.set_cameras(cams, ViewId::Top);
        // The Top camera looks down: the plane is horizontal through the given point.
        let ray = Ray {
            origin: Vec3::new(3.0, 50.0, 4.0),
            dir: Vec3::new(0.0, -1.0, 0.0),
        };
        let p = idx
            .drag_plane(ray, Vec3::new(0.0, 5.0, 0.0), 200.0)
            .unwrap();
        assert!((p - Vec3::new(3.0, 5.0, 4.0)).length() < 1e-5);
        assert!(
            idx.drag_plane(ray, Vec3::new(0.0, 5.0, 0.0), 10.0)
                .is_none()
        );
        let sideways = Ray {
            origin: Vec3::new(3.0, 5.0, 4.0),
            dir: Vec3::X,
        };
        assert!(
            idx.drag_plane(sideways, Vec3::ZERO, 200.0).is_none(),
            "parallel to the plane"
        );
    }

    #[test]
    fn frustum_selection_returns_the_objects_in_list_order() {
        let doc = doc_with(&[
            (Vec3::new(-50.0, 0.0, 0.0), Vec3::splat(2.0), Vec3::ZERO),
            (Vec3::new(0.0, 0.0, 0.0), Vec3::splat(2.0), Vec3::ZERO),
            (Vec3::new(4.0, 0.0, 0.0), Vec3::splat(2.0), Vec3::ZERO),
        ]);
        let idx = index(&doc);
        let cam = Camera::default_for(ViewId::Front);
        let all = cam.frustum(1.0, Vec2::splat(-1.0), Vec2::ONE);
        let ids = doc.elements.ids();
        assert_eq!(
            idx.select_frustum(&all, false),
            [ObjRef::Element(ids[1]), ObjRef::Element(ids[2])]
        );
        // The Front view shows x = 8 + 12 * ndc.x and y = 8 + 12 * ndc.y; the second element
        // spans x 0..2 and y 0..2, so ndc -0.67..-0.5 on both axes.
        let part = cam.frustum(1.0, Vec2::new(-0.6, -0.8), Vec2::new(-0.45, -0.4));
        assert_eq!(idx.select_frustum(&part, false), [ObjRef::Element(ids[1])]);
        assert!(idx.select_frustum(&part, true).is_empty());
        let wide = cam.frustum(1.0, Vec2::new(-0.9, -0.9), Vec2::new(-0.3, -0.3));
        assert_eq!(idx.select_frustum(&wide, true), [ObjRef::Element(ids[1])]);
        assert!(idx.bounds_of(&[ObjRef::Element(ids[2])]).is_some());
    }

    /// The twelve world triangles of an element, the independent reference of the proptest.
    fn world_triangles(el: &Element) -> Vec<[Vec3; 3]> {
        let frame = Frame::of(&el.base).unwrap();
        let mut tris = Vec::new();
        for corners in FACE_CORNERS {
            let v = corners.map(|c| frame.to_world(UNIT_CORNERS[c]));
            tris.push([v[3], v[1], v[0]]);
            tris.push([v[3], v[2], v[1]]);
        }
        tris
    }

    fn mesh_doc(geometry: MeshGeometry, p: Vec3, s: Vec3, e: Vec3) -> Document {
        let mut doc = Document::new(TextureList::default());
        doc.blueprint.base.hidden = true;
        let mut base = cuboid_model::ObjectBase::new("m", p, s, Perms::PRIMITIVE);
        base.e = e;
        doc.meshes.push(Mesh {
            base,
            kind: MeshKind::Custom,
            geometry: Arc::new(geometry),
            texture: None,
            submeshes: Arc::from([]),
            bounds: MeshBounds::default(),
            norm_factor: 1.0,
            import_offset: Vec3::ZERO,
            tint: [1.0; 4],
        });
        doc
    }

    proptest! {
        #[test]
        fn mesh_picking_equals_a_brute_force_scan_of_the_triangles(
            verts in proptest::collection::vec((0.0f32..1.0, 0.0f32..1.0, -1.0f32..0.0), 3..40),
            tris in proptest::collection::vec((0usize..40, 0usize..40, 0usize..40), 1..60),
            p in (-10.0f32..10.0, -10.0f32..10.0, -10.0f32..10.0),
            s in (0.3f32..6.0, 0.3f32..6.0, 0.3f32..6.0),
            e in (0u8..16, 0u8..16, 0u8..16),
            origin in (-30.0f32..30.0, -30.0f32..30.0, -30.0f32..30.0),
            target in (0.0f32..1.0, 0.0f32..1.0, -1.0f32..0.0),
        ) {
            let vertices: Vec<Vec3> = verts.iter().map(|v| Vec3::new(v.0, v.1, v.2)).collect();
            let n = vertices.len();
            let triangles: Vec<u32> = tris
                .iter()
                .flat_map(|t| [t.0 % n, t.1 % n, t.2 % n])
                .map(|i| u32::try_from(i).unwrap())
                .collect();
            let geometry = MeshGeometry { vertices: vertices.clone(), normals: None, uvs: Vec::new(), triangles: triangles.clone() };
            let (p, s) = (Vec3::new(p.0, p.1, p.2), Vec3::new(s.0, s.1, s.2));
            let e = Vec3::new(f32::from(e.0) * 22.5, f32::from(e.1) * 22.5, f32::from(e.2) * 22.5);
            let doc = mesh_doc(geometry, p, s, e);
            let idx = index(&doc);
            let frame = Frame::of(&doc.meshes.items()[0].base).unwrap();
            let origin = Vec3::new(origin.0, origin.1, origin.2);
            // Aim at a point of the unit box so that rays do hit the mesh often.
            let aim = frame.to_world(Vec3::new(target.0, target.1, target.2));
            let dir = unit_or(aim - origin, Vec3::X);
            let ray = Ray { origin, dir };

            let world: Vec<Vec3> = vertices.iter().map(|&v| frame.to_world(v)).collect();
            let expected = triangles
                .chunks_exact(3)
                .filter_map(|t| {
                    ray_triangle_front(origin, dir, world[t[0] as usize], world[t[1] as usize], world[t[2] as usize], 1000.0)
                })
                .map(|h| h.t)
                .min_by(f32::total_cmp);
            let got = idx.pick_model(ray, 1000.0).map(|h| (h.point - origin).length());
            match (expected, got) {
                (None, None) => {}
                (Some(a), Some(b)) => prop_assert!((a - b).abs() < 2e-3, "{a} vs {b}"),
                (Some(a), None) | (None, Some(a)) => prop_assert!(false, "only one hit: {a}"),
            }
        }

        #[test]
        fn picking_equals_a_brute_force_scan_of_the_triangles(
            seed in proptest::collection::vec(
                ((-30.0f32..30.0, -30.0f32..30.0, -30.0f32..30.0), (0.2f32..8.0, 0.2f32..8.0, 0.2f32..8.0), (0u8..16, 0u8..16, 0u8..16)),
                1..120),
            origin in (-60.0f32..60.0, -60.0f32..60.0, -60.0f32..60.0),
            target in (-30.0f32..30.0, -30.0f32..30.0, -30.0f32..30.0),
        ) {
            let elements: Vec<_> = seed.iter().map(|&(p, s, e)| (
                Vec3::new(p.0, p.1, p.2),
                Vec3::new(s.0, s.1, s.2),
                Vec3::new(f32::from(e.0) * 22.5, f32::from(e.1) * 22.5, f32::from(e.2) * 22.5),
            )).collect();
            let doc = doc_with(&elements);
            let idx = index(&doc);
            let origin = Vec3::new(origin.0, origin.1, origin.2);
            let dir = unit_or(Vec3::new(target.0, target.1, target.2) - origin, Vec3::X);
            let ray = Ray { origin, dir };

            let expected = doc.elements.items().iter().flat_map(|el| world_triangles(el))
                .filter_map(|[a, b, c]| ray_triangle_front(origin, dir, a, b, c, 1000.0))
                .map(|h| h.t)
                .min_by(f32::total_cmp);
            let got = idx.pick_model(ray, 1000.0).map(|h| (h.point - origin).length());
            match (expected, got) {
                (None, None) => {}
                (Some(a), Some(b)) => prop_assert!((a - b).abs() < 2e-3, "{a} vs {b}"),
                // A graze along a shared edge may be accepted by one test and not the other.
                (Some(a), None) | (None, Some(a)) => prop_assert!(false, "only one hit: {a}"),
            }
        }
    }
}
