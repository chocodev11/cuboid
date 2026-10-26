//! Mutation guards with the position, size and rotation setter semantics (spec: tools 3.2-3.5).

use cuboid_num::{engine_eq, euler_to_quat, snap_angle, trunc_i32, wrap360};
use cuboid_voxel::mapping;
use glam::Vec3;

use crate::contract::{Document, EditCtx, EditEnv, ModelBox};
use crate::ids::{ObjId, ObjRef, Rev};
use crate::journal::{ChangeFlags, DocFlags};
use crate::normal_form;
use crate::objects::{Element, ElementFlags, Face, FaceName, MIN_SIZE, ObjectBase, Perms};
use crate::selection::Selection;
use crate::uv::default_uv_rects;

/// The centre of the central block, the pivot of quarter-turn bakes.
const BLOCK_CENTER: Vec3 = Vec3::new(8.0, 8.0, -8.0);

/// A coordinate axis, for the quarter-turn bakes.
///
/// spec: tools 4.9
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Axis {
    /// The X axis.
    X,
    /// The Y axis.
    Y,
    /// The Z axis.
    Z,
}

/// Options of the rotation setter.
///
/// spec: tools 3.4
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RotOpts {
    /// Rotate the other selected objects rigidly about the primary's pivot when this object is
    /// the primary.
    pub propagate: bool,
    /// Never reject the change: loads, undo, redo, paste and quarter turns.
    pub force: bool,
    /// Rotate the transform about the pivot (`p` moves) instead of replacing the orientation.
    pub about_pivot: bool,
}

/// What the rotation setter did.
///
/// spec: tools 3.4
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RotResult {
    /// The setter ran to the end: the quantised rotation was stored (it may equal the old one
    /// when the request was within half a step of it).
    Applied,
    /// The request equals the stored rotation; nothing happened.
    Unchanged,
    /// Restricted mode refused a second rotated axis.
    Rejected,
}

/// The state every guard shares: the document, the selection that drives propagation, the
/// environment and the object the guard stands for. The setter logic lives here once and takes
/// the object it acts on as an argument, because propagation applies it to the other selected
/// objects.
struct Guard<'a> {
    doc: &'a mut Document,
    sel: &'a Selection,
    env: &'a EditEnv,
    target: ObjRef,
}

impl<'a> Guard<'a> {
    fn new(cx: &'a mut EditCtx<'_>, target: ObjRef) -> Self {
        Self {
            doc: &mut *cx.doc,
            sel: &*cx.sel,
            env: cx.env,
            target,
        }
    }

    fn is_primary(&self, t: ObjRef) -> bool {
        self.sel.primary == Some(t)
    }

    fn base(&self, t: ObjRef) -> Option<&ObjectBase> {
        match t {
            ObjRef::Element(id) => self.doc.elements.get(id).map(|e| &e.base),
            ObjRef::Mesh(id) => self.doc.meshes.get(id).map(|m| &m.base),
            ObjRef::Blueprint => Some(&self.doc.blueprint.base),
            ObjRef::Group(_) | ObjRef::ModelOrigin => None,
        }
    }

    /// The base of `t` for writing, with its journal entry and a new document revision. The
    /// blueprint and the model origin have no `ObjId`, so the journal has no entry for them.
    fn base_mut(&mut self, t: ObjRef, flags: ChangeFlags) -> Option<&mut ObjectBase> {
        let doc = &mut *self.doc;
        let base = match t {
            ObjRef::Element(id) => doc
                .elements
                .edit_marked(id, flags, &mut doc.journal)
                .map(|e| &mut e.base),
            ObjRef::Mesh(id) => doc
                .meshes
                .edit_marked(id, flags, &mut doc.journal)
                .map(|m| &mut m.base),
            ObjRef::Blueprint => Some(&mut doc.blueprint.base),
            ObjRef::Group(_) | ObjRef::ModelOrigin => None,
        };
        if base.is_some() {
            doc.revision = Rev::next();
        }
        base
    }

    fn element_mut(&mut self, id: ObjId, flags: ChangeFlags) -> Option<&mut Element> {
        let doc = &mut *self.doc;
        let element = doc.elements.edit_marked(id, flags, &mut doc.journal);
        if element.is_some() {
            doc.revision = Rev::next();
        }
        element
    }

    /// The blueprint's origin is always its position.
    fn pin_blueprint_origin(&mut self, t: ObjRef) {
        if t == ObjRef::Blueprint {
            let base = &mut self.doc.blueprint.base;
            base.o = base.p;
        }
    }

    /// The position-changed and size-changed notification of an element.
    fn refresh_uv(&mut self, t: ObjRef) {
        if let ObjRef::Element(id) = t {
            self.default_uv(id, false);
        }
    }

    fn default_uv(&mut self, id: ObjId, force: bool) {
        let Some(el) = self.doc.elements.get(id) else {
            return;
        };
        if !force && !el.flags.contains(ElementFlags::UV_SNAP) {
            return;
        }
        let mirror = el.flags.contains(ElementFlags::UV_SNAP_MIRROR);
        let rects = default_uv_rects(el.base.p, el.base.s, mirror);
        if el
            .faces
            .iter()
            .zip(&rects)
            .all(|(face, rect)| face.uv == *rect)
        {
            return;
        }
        if let Some(el) = self.element_mut(id, ChangeFlags::UV) {
            for (face, rect) in el.faces.iter_mut().zip(rects) {
                face.uv = rect;
            }
        }
    }

    fn set_position(&mut self, t: ObjRef, p: Vec3, propagate: bool) {
        let Some(old) = self.base(t).map(|b| b.p) else {
            return;
        };
        if engine_eq(p, old) {
            return;
        }
        if propagate && self.is_primary(t) {
            let delta = p - old;
            let sel = self.sel;
            for &other in sel.list.iter().filter(|&&o| o != t) {
                self.translate(other, delta);
            }
        }
        if let Some(base) = self.base_mut(t, ChangeFlags::TRANSFORM) {
            base.p = p;
        }
        self.pin_blueprint_origin(t);
        self.refresh_uv(t);
    }

    /// Moves a propagated object by `delta`, position and origin alike.
    fn translate(&mut self, t: ObjRef, delta: Vec3) {
        if t == ObjRef::ModelOrigin {
            let origin = self.doc.props.general.origin + delta;
            set_model_origin(self.doc, origin);
            return;
        }
        if let Some(base) = self.base_mut(t, ChangeFlags::TRANSFORM) {
            base.p += delta;
            base.o += delta;
        }
        self.refresh_uv(t);
    }

    fn set_origin_move(&mut self, t: ObjRef, o: Vec3, propagate: bool) {
        let Some((offset, old)) = self.base(t).map(|b| (b.p - b.o, b.o)) else {
            return;
        };
        if old != o
            && let Some(base) = self.base_mut(t, ChangeFlags::TRANSFORM)
        {
            base.o = o;
        }
        self.set_position(t, o + offset, propagate);
        self.pin_blueprint_origin(t);
    }

    fn set_size(&mut self, t: ObjRef, n: Vec3) {
        let Some((s, o, p)) = self.base(t).map(|b| (b.s, b.o, b.p)) else {
            return;
        };
        if engine_eq(n, s) {
            return;
        }
        if self.is_primary(t) {
            let delta = n - s;
            let sel = self.sel;
            for &other in sel.list.iter().filter(|&&o| o != t) {
                if let Some(other_size) = self.base(other).map(|b| b.s) {
                    self.set_size(other, other_size + delta);
                }
            }
        }
        // The ratio uses the request before the clamp (tools 3.3 step 2).
        let ratio = n / s;
        let signed = self.env.allow_negative_size;
        if let Some(base) = self.base_mut(t, ChangeFlags::TRANSFORM) {
            base.p = o + (p - o) * ratio;
            base.s = n.map(|c| clamp_size(c, signed));
        }
        self.pin_blueprint_origin(t);
        self.refresh_uv(t);
    }

    fn set_rotation(&mut self, t: ObjRef, r: Vec3, opts: RotOpts) -> RotResult {
        let Some((old, origin, perms)) = self.base(t).map(|b| (b.e, b.o, b.perms)) else {
            return RotResult::Unchanged;
        };
        if engine_eq(r, old) {
            return RotResult::Unchanged;
        }
        if opts.propagate && self.is_primary(t) {
            let delta = r - old;
            let sel = self.sel;
            for &other in sel.list.iter().filter(|&&o| o != t) {
                self.rotate_with_primary(other, origin, delta, opts);
            }
        }
        let stored = if self.env.restrict_rotations {
            let wrapped = r.map(wrap360);
            let restricted = perms.contains(Perms::RESTRICTABLE);
            if restricted && !opts.force && second_axis_rotated(old, wrapped) {
                return RotResult::Rejected;
            }
            wrapped.map(|c| snap_angle(c, restricted))
        } else {
            normal_form::free_rotation(r)
        };
        if let Some(base) = self.base_mut(t, ChangeFlags::TRANSFORM) {
            if opts.about_pivot {
                let turn = euler_to_quat(stored) * euler_to_quat(old).inverse();
                base.p = base.o + turn * (base.p - base.o);
            }
            base.e = stored;
        }
        if let ObjRef::Element(id) = t {
            self.turn_uv_snap_off(id);
        }
        self.pin_blueprint_origin(t);
        RotResult::Applied
    }

    /// Clears the UV snap flag of the element; nothing is written when it is already off.
    fn turn_uv_snap_off(&mut self, id: ObjId) {
        let on = self
            .doc
            .elements
            .get(id)
            .is_some_and(|el| el.flags.contains(ElementFlags::UV_SNAP));
        if on && let Some(el) = self.element_mut(id, ChangeFlags::TRANSFORM) {
            el.flags.remove(ElementFlags::UV_SNAP);
        }
    }

    /// The propagation step of the rotation setter: `other` takes the primary's pivot and then
    /// the rotation change through its own setter. The blueprint keeps its origin at its
    /// position and the model origin does not rotate.
    fn rotate_with_primary(&mut self, other: ObjRef, pivot: Vec3, delta: Vec3, opts: RotOpts) {
        let moved = self.base(other).is_some_and(|b| b.o != pivot);
        if other != ObjRef::Blueprint
            && moved
            && let Some(base) = self.base_mut(other, ChangeFlags::TRANSFORM)
        {
            base.o = pivot;
        }
        let Some(current) = self.base(other).map(|b| b.e) else {
            return;
        };
        let own = RotOpts {
            propagate: false,
            ..opts
        };
        self.set_rotation(other, current + delta, own);
    }

    /// Rotates the element by a multiple of 90 degrees about `axis` of the block centre,
    /// keeping it axis-aligned (tools 4.9). The rotation of the element is reset first. With
    /// `faces_only` only the faces and their UV rotations turn: no geometry, no rotation reset
    /// and UV snap stays as it is.
    fn quarter_turn(&mut self, id: ObjId, axis: Axis, degrees: i32, faces_only: bool) {
        let turns = (degrees / 90 * 90 + 360).rem_euclid(360) / 90;
        if turns == 0 || self.doc.elements.get(id).is_none() {
            return;
        }
        if !faces_only {
            let reset = RotOpts {
                propagate: false,
                force: true,
                about_pivot: true,
            };
            self.set_rotation(ObjRef::Element(id), Vec3::ZERO, reset);
        }
        let steps: &[(Axis, i32)] = match (axis, turns) {
            (Axis::Z, 1) => &[(Axis::X, 1), (Axis::Y, 1), (Axis::X, 3)],
            (Axis::Z, 2) => &[(Axis::X, 2), (Axis::Y, 2)],
            (Axis::Z, _) => &[(Axis::X, 3), (Axis::Y, 1), (Axis::X, 1)],
            (axis, turns) => &[(axis, turns)],
        };
        for &(axis, turns) in steps {
            for _ in 0..turns {
                self.quarter_step(id, axis, faces_only);
            }
        }
        if !faces_only {
            self.turn_uv_snap_off(id);
        }
    }

    /// One +90 degree turn about X or Y: the box, the pivot, the size, the faces and their UV.
    fn quarter_step(&mut self, id: ObjId, axis: Axis, faces_only: bool) {
        if faces_only {
            if let Some(el) = self.element_mut(id, ChangeFlags::FACES | ChangeFlags::UV) {
                cycle_faces(&mut el.faces, axis);
            }
            return;
        }
        let Some(el) = self.element_mut(
            id,
            ChangeFlags::TRANSFORM | ChangeFlags::FACES | ChangeFlags::UV,
        ) else {
            return;
        };
        let base = &mut el.base;
        let turn = |v: Vec3| match axis {
            Axis::X => Vec3::new(v.x, -v.z, v.y),
            _ => Vec3::new(v.z, v.y, -v.x),
        };
        let s = base.s;
        let (fix, size) = match axis {
            Axis::X => (Vec3::new(0.0, 0.0, s.y), Vec3::new(s.x, s.z, s.y)),
            _ => (Vec3::new(-s.z, 0.0, 0.0), Vec3::new(s.z, s.y, s.x)),
        };
        base.p = turn(base.p - BLOCK_CENTER) + BLOCK_CENTER + fix;
        base.o = turn(base.o - BLOCK_CENTER) + BLOCK_CENTER;
        base.s = size;
        cycle_faces(&mut el.faces, axis);
    }

    /// "Restrict rotation of one element" for the element `id` alone (tools 3.5 steps 2-6).
    fn bake_restricted_rotation(&mut self, id: ObjId) {
        let t = ObjRef::Element(id);
        let Some((e, origin)) = self.base(t).map(|b| (b.e, b.o)) else {
            return;
        };
        let mut v = e;
        if v.y != 0.0 && (v.x != 0.0 || v.z != 0.0) {
            v = Vec3::new(0.0, v.y, 0.0);
        }
        if v.x != 0.0 && v.z != 0.0 {
            v = Vec3::new(v.x, 0.0, 0.0);
        }
        v = v.map(|c| snap_angle(c, true));
        for axis in [Axis::Y, Axis::X, Axis::Z] {
            let c = component(v, axis);
            if c > 45.0 && c < 315.0 {
                let k = trunc_i32(c - 45.0) / 90 + 1;
                let q = k * 90 % 360;
                v = with_component(v, axis, c - q as f32);
                self.quarter_turn(id, axis, q, false);
            }
        }
        let force = RotOpts {
            propagate: false,
            force: true,
            about_pivot: true,
        };
        self.set_rotation(t, v, force);
        if self.base(t).is_some_and(|b| !engine_eq(b.o, origin)) {
            self.set_origin_move(t, origin, false);
        }
    }
}

/// The face half of a +90 degree turn about X or Y (tools 4.9): the face cycle without moving
/// the names, the arriving faces flipped, and the UV spins of the side faces. Face copies fold
/// the UV corners, so a swap is a swap of the folded values (tools 4.5).
fn cycle_faces(faces: &mut [Face; 6], axis: Axis) {
    use FaceName::{Down, East, North, South, Up, West};
    // (new slot, old slot): the face now at the first slot came from the second.
    let (cycle, flipped, spin_minus, spin_plus) = match axis {
        Axis::X => (
            [(South, Down), (Down, North), (North, Up), (Up, South)],
            &[Down, North][..],
            West,
            East,
        ),
        _ => (
            [(North, West), (West, South), (South, East), (East, North)],
            &[][..],
            Down,
            Up,
        ),
    };
    let old = faces.clone();
    for (new, from) in cycle {
        let face = &mut faces[new as usize];
        old[from as usize].copy_to(face, true);
        if flipped.contains(&new) {
            let (tl, br) = (face.uv.tl_raw(), face.uv.br_raw());
            face.uv.set_unclamped(br, tl);
        }
    }
    let spin = |face: &mut Face, by: i32| face.uv.set_rot(face.uv.rot() + by);
    spin(&mut faces[spin_minus as usize], -90);
    spin(&mut faces[spin_plus as usize], 90);
}

fn component(v: Vec3, axis: Axis) -> f32 {
    match axis {
        Axis::X => v.x,
        Axis::Y => v.y,
        Axis::Z => v.z,
    }
}

fn with_component(mut v: Vec3, axis: Axis, c: f32) -> Vec3 {
    match axis {
        Axis::X => v.x = c,
        Axis::Y => v.y = c,
        Axis::Z => v.z = c,
    }
    v
}

/// A size component after the setter: at least [`MIN_SIZE`], or with `signed` at least
/// [`MIN_SIZE`] away from zero on either side (a zero keeps the positive side).
fn clamp_size(c: f32, signed: bool) -> f32 {
    if !signed || c.is_nan() {
        c.max(MIN_SIZE)
    } else if c.abs() < MIN_SIZE {
        MIN_SIZE.copysign(c)
    } else {
        c
    }
}

/// Whether the current rotation is non-zero on some axis while the new one is non-zero on
/// another: what restricted mode refuses (tools 3.4 step 3). A stored 360 counts as non-zero.
fn second_axis_rotated(current: Vec3, new: Vec3) -> bool {
    let (current, new) = (current.to_array(), new.to_array());
    (0..3).any(|a| current[a] != 0.0 && (0..3).any(|b| b != a && new[b] != 0.0))
}

/// Writes the model origin `g.o`, as the origin marker does whenever its position changes.
///
/// spec: tools 2.3
pub fn set_model_origin(doc: &mut Document, o: Vec3) {
    if doc.props.general.origin == o {
        return;
    }
    std::sync::Arc::make_mut(&mut doc.props).general.origin = o;
    doc.journal.mark_doc(DocFlags::PROPS);
    doc.revision = Rev::next();
}

/// Sets the model box through the six clamped setters in the order xmin, ymin, zmin, xmax,
/// ymax, zmax, stores it in `g.mb` and gives the voxel world its new active box, which dirties
/// the chunks the change exposes or hides. The handle drag commits through this; model box
/// edits are not part of undo (tools 2.4).
///
/// spec: tools 2.4, tools 7.2
pub fn set_model_box(doc: &mut Document, requested: ModelBox) {
    let clamp = |v: f32, lo: f32, hi: f32| v.max(lo).min(hi);
    let mut b = doc.model_box;
    b.min.x = clamp(requested.min.x, -16.0, b.max.x);
    b.min.y = clamp(requested.min.y, -16.0, b.max.y);
    b.min.z = clamp(requested.min.z, -32.0, b.max.z);
    b.max.x = clamp(requested.max.x, b.min.x, 32.0);
    b.max.y = clamp(requested.max.y, b.min.y, 32.0);
    b.max.z = clamp(requested.max.z, b.min.z, 16.0);
    if b == doc.model_box && doc.props.general.model_box == Some(b) {
        return;
    }
    doc.model_box = b;
    std::sync::Arc::make_mut(&mut doc.props).general.model_box = Some(b);
    doc.journal.mark_doc(DocFlags::MODEL_BOX | DocFlags::PROPS);
    if let Some(world) = &mut doc.voxels {
        world.set_active_box(mapping::active_box(b, world.res));
        if world.is_dirty() {
            doc.journal.mark_doc(DocFlags::VOXEL_CHUNKS);
        }
    }
    doc.revision = Rev::next();
}

/// Generates the setters every guard shares; each one acts on the guard's own object.
macro_rules! base_setters {
    () => {
        /// Sets the position; when `propagate` and this is the primary selection, every other
        /// selected object moves by the same delta (position and origin).
        pub fn set_position(&mut self, p: Vec3, propagate: bool) {
            let t = self.g.target;
            self.g.set_position(t, p, propagate);
        }

        /// "Set origin and move": the origin becomes `o` and the position follows by the old
        /// offset between them.
        pub fn set_origin_move(&mut self, o: Vec3, propagate: bool) {
            let t = self.g.target;
            self.g.set_origin_move(t, o, propagate);
        }

        /// Sets the size: pivot-relative rescale of the position, components clamped to at
        /// least 0.001 (to at least 0.001 in magnitude when the environment allows negative
        /// sizes), propagated to the other selected objects as a delta when this is the
        /// primary.
        pub fn set_size(&mut self, s: Vec3) {
            let t = self.g.target;
            self.g.set_size(t, s);
        }

        /// Sets the rotation: propagation, then with "Restrict rotations" on the wrap,
        /// quantisation (22.5 degrees for elements, 5 for meshes) and the refusal of a second
        /// axis, and with it off the angles as given (only a turn beyond 360 degrees is wrapped).
        /// For elements UV snap goes off.
        pub fn set_rotation(&mut self, r: Vec3, o: RotOpts) -> RotResult {
            let t = self.g.target;
            self.g.set_rotation(t, r, o)
        }
    };
}

/// Writes to an element with Cubik semantics: position propagation to the other selected
/// objects, the pivot-relative resize with the 0.001 clamp, rotation wrap, quantisation,
/// restricted rejection and pivot rotation, the UV-snap triggers, and the journal entries.
/// Every write that has these semantics goes through a guard; the paths that bypass the setters
/// on purpose (the transform dialogs, tools 10.13) are explicit functions in `cuboid-ops`.
///
/// Equality tests ("nothing happens when the new value equals the old") use the engine's
/// vector equality of `cuboid-num`.
///
/// spec: tools 3.2-3.5, tools 4.7, arch 6 (cuboid-model), arch 8.3
pub struct ElementMut<'a> {
    g: Guard<'a>,
}

impl<'a> ElementMut<'a> {
    /// The guard for the element `id`, or `None` when the document has no such element.
    pub fn new(cx: &'a mut EditCtx<'_>, id: ObjId) -> Option<Self> {
        cx.doc.elements.get(id)?;
        Some(Self {
            g: Guard::new(cx, ObjRef::Element(id)),
        })
    }

    base_setters!();

    /// "Restrict rotation of one element" (the bake): keeps one rotated axis within 45 degrees
    /// on the 22.5 degree grid and expresses the rest as quarter-turn face permutations. As
    /// the primary selection it first does the same for the other selected elements.
    ///
    /// spec: tools 3.5, tools 4.9
    pub fn bake_restricted_rotation(&mut self) {
        let t = self.g.target;
        if self.g.is_primary(t) {
            let sel = self.g.sel;
            for &other in sel.list.iter().filter(|&&o| o != t) {
                if let ObjRef::Element(id) = other {
                    self.g.bake_restricted_rotation(id);
                }
            }
        }
        if let ObjRef::Element(id) = t {
            self.g.bake_restricted_rotation(id);
        }
    }

    /// Rotates the element by a multiple of 90 degrees about `axis` of the block centre
    /// `(8, 8, -8)`, keeping it axis-aligned by permuting faces and swapping size axes. The
    /// element's own rotation is reset first and UV snap goes off.
    ///
    /// spec: tools 4.9
    pub fn quarter_turn(&mut self, axis: Axis, degrees: i32) {
        if let ObjRef::Element(id) = self.g.target {
            self.g.quarter_turn(id, axis, degrees, false);
        }
    }

    /// The faces-only variant of [`ElementMut::quarter_turn`] used by the UV projections: the
    /// faces and their UV rotations turn, the geometry, the rotation and UV snap stay.
    ///
    /// spec: tools 4.9, tools 10.17
    pub fn quarter_turn_faces(&mut self, axis: Axis, degrees: i32) {
        if let ObjRef::Element(id) = self.g.target {
            self.g.quarter_turn(id, axis, degrees, true);
        }
    }

    /// Recomputes the default UV mapping when UV snap is on or `force` is set.
    ///
    /// spec: tools 4.7
    pub fn default_uv(&mut self, force: bool) {
        if let ObjRef::Element(id) = self.g.target {
            self.g.default_uv(id, force);
        }
    }
}

/// The guard for a mesh; the base setters of [`ElementMut`] without the element-only parts.
///
/// spec: tools 3.2-3.4, tools 6.1
pub struct MeshMut<'a> {
    g: Guard<'a>,
}

impl<'a> MeshMut<'a> {
    /// The guard for the mesh `id`, or `None` when the document has no such mesh.
    pub fn new(cx: &'a mut EditCtx<'_>, id: ObjId) -> Option<Self> {
        cx.doc.meshes.get(id)?;
        Some(Self {
            g: Guard::new(cx, ObjRef::Mesh(id)),
        })
    }

    base_setters!();
}

/// The guard for the blueprint; its origin is always its position. The journal has no entry
/// for it, because it has no `ObjId`.
///
/// spec: tools 2.3, tools 3.2-3.4
pub struct BlueprintMut<'a> {
    g: Guard<'a>,
}

impl<'a> BlueprintMut<'a> {
    /// The guard for the blueprint of the document.
    pub fn new(cx: &'a mut EditCtx<'_>) -> Self {
        Self {
            g: Guard::new(cx, ObjRef::Blueprint),
        }
    }

    base_setters!();
}

/// Shared fixture of the tests of this crate: a document with its selection and environment.
#[cfg(test)]
pub(crate) mod fixture {
    use cuboid_num::EngineRng;
    use glam::Vec3;

    use super::*;
    use crate::contract::RenderPrefs;
    use crate::objects::{ElementType, Label, Mesh, MeshBounds, MeshKind, TextureList};

    pub struct World {
        pub doc: Document,
        pub sel: Selection,
        pub env: EditEnv,
        pub rng: EngineRng,
    }

    impl World {
        pub fn new() -> Self {
            Self {
                doc: Document::new(TextureList::default()),
                sel: Selection::default(),
                env: EditEnv {
                    restrict_rotations: true,
                    render: RenderPrefs::default(),
                    code_page: 1252,
                    allow_negative_size: false,
                },
                rng: EngineRng::seeded(1),
            }
        }

        pub fn cx(&mut self) -> EditCtx<'_> {
            EditCtx {
                doc: &mut self.doc,
                sel: &mut self.sel,
                env: &self.env,
                rng: &mut self.rng,
            }
        }

        /// A cube with its origin at its position and UV snap off.
        pub fn add_element(&mut self, p: Vec3, s: Vec3) -> ObjId {
            let mut el = Element::new(
                ElementType::Cube,
                &self.doc.blueprint,
                self.doc.elements.next_display_id(),
                [1.0; 4],
                &Label::from("texture"),
            );
            el.base.p = p;
            el.base.o = p;
            el.base.s = s;
            el.flags.remove(ElementFlags::UV_SNAP);
            self.doc.elements.push(el)
        }

        pub fn add_mesh(&mut self, p: Vec3, s: Vec3) -> ObjId {
            let mesh = Mesh {
                base: ObjectBase::new("Mesh", p, s, Perms::PRIMITIVE),
                kind: MeshKind::Cube,
                geometry: Default::default(),
                texture: None,
                submeshes: std::sync::Arc::from([]),
                bounds: MeshBounds::default(),
                norm_factor: 1.0,
                import_offset: Vec3::ZERO,
                tint: [1.0; 4],
            };
            self.doc.meshes.push(mesh)
        }

        pub fn select(&mut self, objs: &[ObjRef]) {
            for &obj in objs {
                self.sel.add(obj);
            }
        }

        pub fn element(&self, id: ObjId) -> &Element {
            self.doc.elements.get(id).expect("element")
        }

        pub fn edit_element(&mut self, id: ObjId, f: impl FnOnce(&mut Element)) {
            let doc = &mut self.doc;
            f(doc.elements.edit(id, &mut doc.journal).expect("element"));
        }
    }
}

#[cfg(test)]
mod tests {
    use cuboid_image::{Image, RgbaBuf};
    use cuboid_num::euler_to_quat;
    use cuboid_voxel::VoxelWorld;
    use glam::{Vec2, Vec3};

    use super::fixture::World;
    use super::*;
    use crate::invariants::check_invariants;
    use crate::objects::{Blueprint, ElementType, Label, Texture, TextureEntry, TextureList};
    use crate::uv::UvRect;

    const PLAIN: RotOpts = RotOpts {
        propagate: false,
        force: false,
        about_pivot: false,
    };
    const FORCED: RotOpts = RotOpts {
        propagate: false,
        force: true,
        about_pivot: true,
    };

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).abs().max_element() < 1e-3
    }

    #[track_caller]
    fn assert_close(a: Vec3, b: Vec3) {
        assert!(close(a, b), "{a} is not {b}");
    }

    fn rotate(w: &mut World, id: ObjId, r: Vec3, opts: RotOpts) -> RotResult {
        ElementMut::new(&mut w.cx(), id)
            .unwrap()
            .set_rotation(r, opts)
    }

    // --- Creation (tools 17 item 1) ------------------------------------------------------------

    #[test]
    fn a_new_cube_sits_at_the_default_blueprint_with_a_snapped_mapping() {
        let mut w = World::new();
        let blueprint = Blueprint::default();
        assert_eq!(blueprint.base.p, v(-2.0, 0.0, 2.0));
        assert_eq!(blueprint.base.s, Vec3::ONE);
        assert_eq!(blueprint.base.o, blueprint.base.p);
        assert_eq!(&*blueprint.base.label, "Blueprint");
        assert_eq!(w.doc.blueprint.base, blueprint.base);

        let label = w.doc.textures.default_face_label();
        let cube = Element::new(
            ElementType::Cube,
            &w.doc.blueprint,
            w.doc.elements.next_display_id(),
            [0.7; 4],
            &label,
        );
        let id = w.doc.elements.push(cube);

        let el = w.element(id);
        assert_eq!(&*el.base.label, "Box1");
        assert_eq!(el.base.p, v(-2.0, 0.0, 2.0));
        assert_eq!(
            (el.base.s, el.base.e, el.base.o),
            (Vec3::ONE, Vec3::ZERO, el.base.p)
        );
        assert!(el.flags.contains(ElementFlags::UV_SNAP));
        assert!(
            el.faces
                .iter()
                .all(|f| f.active && f.tint == -1 && &*f.texture == "texture")
        );

        ElementMut::new(&mut w.cx(), id).unwrap().default_uv(false);

        // bpos = (-0.125, 0, -0.125), bsize = 0.0625: south is (x, y) .. (x+w, y+h).
        let south = &w.element(id).faces[FaceName::South as usize];
        assert_eq!(south.uv.tl_raw(), Vec2::new(-0.125, 0.0));
        assert_eq!(south.uv.br_raw(), Vec2::new(-0.0625, 0.0625));
        assert_eq!(south.uv.tl(), Vec2::new(0.875, 0.0));
        assert_eq!(south.uv.br(), Vec2::new(0.9375, 0.0625));
        assert_eq!(w.doc.journal.objects()[&id], ChangeFlags::UV);
    }

    #[test]
    fn planes_have_two_faces_and_a_thin_axis() {
        let bp = Blueprint::default();
        let label = Label::from("texture");
        let cases = [
            (ElementType::PlaneX, "PlaneX3", v(0.001, 1.0, 1.0), [4, 5]),
            (ElementType::PlaneY, "PlaneY3", v(1.0, 0.001, 1.0), [0, 1]),
            (ElementType::PlaneZ, "PlaneZ3", v(1.0, 1.0, 0.001), [2, 3]),
        ];
        for (kind, name, size, active) in cases {
            let el = Element::new(kind, &bp, 3, [1.0; 4], &label);
            assert_eq!(&*el.base.label, name);
            assert_eq!(el.base.s, size);
            let on: Vec<usize> = (0..6).filter(|&i| el.faces[i].active).collect();
            assert_eq!(on, active, "{kind:?}");
            assert!(
                el.flags
                    .contains(ElementFlags::UV_SNAP | ElementFlags::SHADE)
            );
        }
    }

    #[test]
    fn the_default_face_label_follows_the_texture_list() {
        let entry = |label: &str| TextureEntry {
            label: Label::from(label),
            texture: Texture::new(std::sync::Arc::new(Image::from_rgba(RgbaBuf::new(1, 1)))),
        };
        let list = |labels: &[&str]| TextureList {
            entries: labels.iter().map(|l| entry(l)).collect(),
        };
        assert_eq!(&*list(&[]).default_face_label(), "texture");
        assert_eq!(&*list(&["stone", "particle"]).default_face_label(), "stone");
        assert_eq!(
            &*list(&["particle", "texture"]).default_face_label(),
            "texture"
        );
        assert_eq!(
            &*list(&["particle", "dirt", "x"]).default_face_label(),
            "dirt"
        );
        assert_eq!(&*list(&["particle"]).default_face_label(), "particle");
    }

    #[test]
    fn a_new_document_starts_at_the_scene_defaults() {
        let doc = Document::new(TextureList::default());
        assert_eq!(doc.model_box, ModelBox::EDITING_VOLUME);
        assert!(doc.props.general.model_box.is_none());
        assert!(doc.elements.is_empty() && doc.meshes.is_empty() && doc.voxels.is_none());
        assert_eq!(doc.blueprint.base.p, v(-2.0, 0.0, 2.0));
        assert!(doc.journal.is_empty());
        assert_ne!(Document::new(TextureList::default()).revision, doc.revision);
    }

    // --- Position ------------------------------------------------------------------------------

    #[test]
    fn the_position_setter_moves_the_other_selected_objects_by_the_same_delta() {
        let mut w = World::new();
        let a = w.add_element(v(0.0, 0.0, 0.0), Vec3::ONE);
        let b = w.add_element(v(1.0, 1.0, 1.0), Vec3::ONE);
        let m = w.add_mesh(v(5.0, 5.0, 5.0), Vec3::ONE);
        w.edit_element(b, |el| el.base.o = v(2.0, 2.0, 2.0));
        w.select(&[ObjRef::Element(a), ObjRef::Element(b), ObjRef::Mesh(m)]);
        w.doc.journal.clear();

        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_position(v(3.0, 0.0, -1.0), true);

        assert_eq!(w.element(a).base.p, v(3.0, 0.0, -1.0));
        assert_eq!(w.element(a).base.o, Vec3::ZERO, "the own origin stays");
        assert_eq!(w.element(b).base.p, v(4.0, 1.0, 0.0));
        assert_eq!(w.element(b).base.o, v(5.0, 2.0, 1.0));
        let mesh = w.doc.meshes.get(m).unwrap();
        assert_eq!(
            (mesh.base.p, mesh.base.o),
            (v(8.0, 5.0, 4.0), v(8.0, 5.0, 4.0))
        );
        assert_eq!(w.doc.journal.objects()[&b], ChangeFlags::TRANSFORM);
    }

    #[test]
    fn only_the_primary_propagates_and_only_when_asked() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let b = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.select(&[ObjRef::Element(b), ObjRef::Element(a)]);

        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_position(v(1.0, 0.0, 0.0), true);
        assert_eq!(w.element(b).base.p, Vec3::ZERO, "a is not the primary");

        ElementMut::new(&mut w.cx(), b)
            .unwrap()
            .set_position(v(0.0, 2.0, 0.0), false);
        assert_eq!(w.element(a).base.p, v(1.0, 0.0, 0.0), "not requested");
    }

    #[test]
    fn an_equal_position_changes_nothing() {
        let mut w = World::new();
        let a = w.add_element(v(1.0, 2.0, 3.0), Vec3::ONE);
        w.doc.journal.clear();
        let rev = w.doc.revision;
        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_position(v(1.0, 2.0, 3.0), true);
        assert!(w.doc.journal.is_empty());
        assert_eq!(w.doc.revision, rev);
    }

    #[test]
    fn set_origin_and_move_keeps_the_offset_between_position_and_origin() {
        let mut w = World::new();
        let a = w.add_element(v(2.0, 0.0, 0.0), Vec3::ONE);
        w.edit_element(a, |el| el.base.o = v(1.0, 1.0, 1.0));
        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_origin_move(v(5.0, 5.0, 5.0), false);
        assert_eq!(w.element(a).base.o, v(5.0, 5.0, 5.0));
        assert_eq!(w.element(a).base.p, v(6.0, 4.0, 4.0));
    }

    #[test]
    fn the_model_origin_follows_a_propagated_move_into_g_o() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.select(&[ObjRef::Element(a), ObjRef::ModelOrigin]);
        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_position(v(1.0, 2.0, 3.0), true);
        assert_eq!(w.doc.props.general.origin, v(1.0, 2.0, 3.0));
        assert!(w.doc.journal.doc().contains(DocFlags::PROPS));
    }

    #[test]
    fn a_setter_does_not_touch_a_shared_snapshot() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let snap = w.doc.elements.snapshot();
        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_position(v(1.0, 0.0, 0.0), false);
        assert_eq!(snap.items[0].base.p, Vec3::ZERO);
        assert_eq!(w.element(a).base.p, v(1.0, 0.0, 0.0));
    }

    #[test]
    fn a_missing_object_has_no_guard() {
        let mut w = World::new();
        assert!(ElementMut::new(&mut w.cx(), ObjId::next()).is_none());
        assert!(MeshMut::new(&mut w.cx(), ObjId::next()).is_none());
    }

    // --- Size (tools 17 item 2) ----------------------------------------------------------------

    #[test]
    fn the_size_setter_rescales_the_position_about_the_pivot_and_clamps_afterwards() {
        let mut w = World::new();
        let a = w.add_element(v(4.0, 6.0, -8.0), v(2.0, 2.0, 2.0));
        w.edit_element(a, |el| el.base.o = Vec3::ZERO);

        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_size(v(4.0, 1.0, 2.0));
        assert_eq!(w.element(a).base.s, v(4.0, 1.0, 2.0));
        assert_eq!(w.element(a).base.p, v(8.0, 3.0, -8.0));

        // The ratio uses the request before the clamp: (0, -1, 2.5).
        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_size(v(0.0, -1.0, 5.0));
        assert_eq!(w.element(a).base.s, v(0.001, 0.001, 5.0));
        assert_eq!(w.element(a).base.p, v(0.0, -3.0, -20.0));
    }

    #[test]
    fn the_size_may_cross_zero_when_the_environment_allows_negative_sizes() {
        let mut w = World::new();
        w.env.allow_negative_size = true;
        let a = w.add_element(v(4.0, 6.0, -8.0), v(2.0, 2.0, 2.0));
        w.edit_element(a, |el| el.base.o = Vec3::ZERO);
        let size = |w: &mut World, s: Vec3| ElementMut::new(&mut w.cx(), a).unwrap().set_size(s);

        // Crossing zero flips the box about the pivot: the position mirrors with the ratio -1.
        size(&mut w, v(-2.0, 2.0, 2.0));
        assert_eq!(w.element(a).base.s, v(-2.0, 2.0, 2.0));
        assert_eq!(w.element(a).base.p, v(-4.0, 6.0, -8.0));
        assert!(check_invariants(&w.doc, &w.sel).is_ok());

        // A component too close to zero keeps its side at 0.001 in magnitude, a zero the
        // positive one.
        size(&mut w, v(-0.0004, 0.0, 0.0005));
        assert_eq!(w.element(a).base.s, v(-0.001, 0.001, 0.001));
        assert!(check_invariants(&w.doc, &w.sel).is_ok());

        // Back through zero to a larger size.
        size(&mut w, v(3.0, -3.0, 3.0));
        assert_eq!(w.element(a).base.s, v(3.0, -3.0, 3.0));
        assert!(check_invariants(&w.doc, &w.sel).is_ok());
    }

    #[test]
    fn negative_sizes_propagate_and_apply_to_meshes_and_the_blueprint_too() {
        let mut w = World::new();
        w.env.allow_negative_size = true;
        let a = w.add_element(Vec3::ZERO, v(2.0, 2.0, 2.0));
        let b = w.add_element(v(4.0, 0.0, 0.0), v(2.0, 2.0, 2.0));
        w.select(&[ObjRef::Element(a), ObjRef::Element(b)]);
        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_size(v(-1.0, 2.0, 2.0));
        // The follower takes the delta (-3, 0, 0).
        assert_eq!(w.element(b).base.s, v(-1.0, 2.0, 2.0));
        let m = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        MeshMut::new(&mut w.cx(), m)
            .unwrap()
            .set_size(v(1.0, -2.0, 1.0));
        assert_eq!(w.doc.meshes.get(m).unwrap().base.s, v(1.0, -2.0, 1.0));
        BlueprintMut::new(&mut w.cx()).set_size(v(1.0, 1.0, -1.0));
        assert_eq!(w.doc.blueprint.base.s, v(1.0, 1.0, -1.0));
    }

    #[test]
    fn without_the_option_the_size_keeps_the_clamp() {
        let mut w = World::new();
        assert!(!w.env.allow_negative_size);
        let a = w.add_element(Vec3::ZERO, v(2.0, 2.0, 2.0));
        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_size(v(-3.0, f32::NAN, 0.0));
        assert_eq!(w.element(a).base.s, v(0.001, 0.001, 0.001));
    }

    #[test]
    fn the_size_setter_ignores_an_equal_size() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.doc.journal.clear();
        ElementMut::new(&mut w.cx(), a).unwrap().set_size(Vec3::ONE);
        assert!(w.doc.journal.is_empty());
    }

    #[test]
    fn the_primary_passes_its_size_delta_to_the_other_selected_objects() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, v(2.0, 2.0, 2.0));
        let b = w.add_element(v(4.0, 0.0, 0.0), v(2.0, 2.0, 2.0));
        let m = w.add_mesh(Vec3::ZERO, v(1.0, 1.0, 1.0));
        w.edit_element(b, |el| el.base.o = Vec3::ZERO);
        w.select(&[
            ObjRef::Element(a),
            ObjRef::Element(b),
            ObjRef::Mesh(m),
            ObjRef::ModelOrigin,
        ]);

        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_size(v(3.0, 2.0, 2.0));

        assert_eq!(w.element(a).base.s, v(3.0, 2.0, 2.0));
        assert_eq!(w.element(b).base.s, v(3.0, 2.0, 2.0));
        assert_eq!(
            w.element(b).base.p,
            v(6.0, 0.0, 0.0),
            "rescale of b about its pivot"
        );
        assert_eq!(w.doc.meshes.get(m).unwrap().base.s, v(2.0, 1.0, 1.0));

        // A non-primary does not propagate.
        ElementMut::new(&mut w.cx(), b)
            .unwrap()
            .set_size(v(1.0, 2.0, 2.0));
        assert_eq!(w.element(a).base.s, v(3.0, 2.0, 2.0));
    }

    #[test]
    fn a_snapped_element_remaps_its_uv_when_moved_or_resized() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, v(4.0, 4.0, 4.0));
        w.edit_element(a, |el| el.flags.insert(ElementFlags::UV_SNAP));
        let south = |w: &World| w.element(a).faces[FaceName::South as usize].uv;

        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_position(v(8.0, 0.0, 0.0), false);
        assert_eq!(south(&w).tl_raw(), Vec2::new(0.5, 0.0));

        // The origin is still at 0, so the position scales with the size: x 8 -> 16.
        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_size(v(8.0, 4.0, 4.0));
        assert_eq!(south(&w).br_raw(), Vec2::new(1.5, 0.25));
    }

    #[test]
    fn the_default_mapping_reads_the_stored_position_of_a_rotated_element() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, v(4.0, 4.0, 4.0));
        w.edit_element(a, |el| el.base.o = v(8.0, 0.0, 0.0));
        rotate(&mut w, a, v(0.0, 90.0, 0.0), FORCED);
        let el = w.element(a);
        assert_ne!(el.base.p, Vec3::ZERO, "the corner moved about the pivot");
        let expected = default_uv_rects(el.base.p, el.base.s, false);

        ElementMut::new(&mut w.cx(), a).unwrap().default_uv(true);

        let faces = &w.element(a).faces;
        assert!(
            faces
                .iter()
                .zip(&expected)
                .all(|(face, rect)| face.uv == *rect)
        );
        assert_ne!(
            expected,
            default_uv_rects(Vec3::ZERO, v(4.0, 4.0, 4.0), false)
        );
    }

    #[test]
    fn propagated_moves_and_resizes_remap_the_snapped_followers() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, v(4.0, 4.0, 4.0));
        let b = w.add_element(Vec3::ZERO, v(4.0, 4.0, 4.0));
        w.edit_element(b, |el| el.flags.insert(ElementFlags::UV_SNAP));
        w.select(&[ObjRef::Element(a), ObjRef::Element(b)]);
        let south = |w: &World| w.element(b).faces[FaceName::South as usize].uv;

        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_position(v(8.0, 0.0, 0.0), true);
        assert_eq!(w.element(b).base.p, v(8.0, 0.0, 0.0));
        assert_eq!(south(&w).tl_raw(), Vec2::new(0.5, 0.0));

        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_size(v(8.0, 4.0, 4.0));
        assert_eq!(w.element(b).base.s, v(8.0, 4.0, 4.0));
        assert_eq!(south(&w).br_raw(), Vec2::new(1.0, 0.25));
    }

    #[test]
    fn an_origin_move_to_the_stored_origin_writes_nothing() {
        let mut w = World::new();
        let a = w.add_element(v(1.0, 2.0, 3.0), Vec3::ONE);
        w.doc.journal.clear();
        let revision = w.doc.revision;

        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_origin_move(v(1.0, 2.0, 3.0), false);

        assert!(w.doc.journal.is_empty());
        assert_eq!(w.doc.revision, revision);
    }

    #[test]
    fn an_unsnapped_element_keeps_its_uv_until_forced() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, v(16.0, 16.0, 16.0));
        let south = |w: &World| w.element(a).faces[FaceName::South as usize].uv;

        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .set_position(v(8.0, 0.0, 0.0), false);
        ElementMut::new(&mut w.cx(), a).unwrap().default_uv(false);
        assert_eq!(south(&w), UvRect::default());

        ElementMut::new(&mut w.cx(), a).unwrap().default_uv(true);
        assert_eq!(south(&w).tl_raw(), Vec2::new(0.5, 0.0));
        assert_eq!(south(&w).br_raw(), Vec2::new(1.5, 1.0));
    }

    // --- Rotation (tools 17 items 3 and 24) ----------------------------------------------------

    #[test]
    fn rotation_wraps_and_rounds_half_to_even_on_the_five_degree_grid() {
        // Meshes are not restrictable: with the restriction on they use the 5 degree grid.
        let mut w = World::new();
        let a = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        let rotate =
            |w: &mut World, r: Vec3| MeshMut::new(&mut w.cx(), a).unwrap().set_rotation(r, PLAIN);
        // (request, stored)
        let cases = [
            (7.5, 10.0),
            (2.5, 0.0),
            (12.5, 10.0),
            (17.5, 20.0),
            (-1.0, 360.0),
            (-5.0, 355.0),
            (358.0, 360.0),
            (360.0, 0.0),
            (725.0, 5.0),
            // Below -360 the remainder keeps its sign (tools 3.4 quirk).
            (-400.0, -40.0),
        ];
        for (request, stored) in cases {
            rotate(&mut w, Vec3::ZERO);
            let result = rotate(&mut w, v(request, 0.0, 0.0));
            assert_eq!(result, RotResult::Applied);
            assert_eq!(
                w.doc.meshes.get(a).unwrap().base.e,
                v(stored, 0.0, 0.0),
                "request {request}"
            );
        }
    }

    #[test]
    fn restricted_rotation_rounds_to_the_22_5_degree_grid() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let cases = [
            (11.25, 0.0),
            (11.3, 22.5),
            (33.75, 45.0),
            (56.25, 45.0),
            (348.75, 360.0),
        ];
        for (request, stored) in cases {
            rotate(&mut w, a, Vec3::ZERO, PLAIN);
            rotate(&mut w, a, v(0.0, request, 0.0), PLAIN);
            assert_eq!(
                w.element(a).base.e,
                v(0.0, stored, 0.0),
                "request {request}"
            );
        }
    }

    #[test]
    fn an_equal_rotation_is_unchanged() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        assert_eq!(rotate(&mut w, a, Vec3::ZERO, PLAIN), RotResult::Unchanged);
        assert_eq!(
            rotate(&mut w, a, v(0.0, 45.0, 0.0), PLAIN),
            RotResult::Applied
        );
        w.doc.journal.clear();
        assert_eq!(
            rotate(&mut w, a, v(0.0, 45.0, 0.0), PLAIN),
            RotResult::Unchanged
        );
        assert!(w.doc.journal.is_empty());
    }

    #[test]
    fn restricted_mode_refuses_a_second_rotated_axis_unless_forced() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        rotate(&mut w, a, v(0.0, 90.0, 0.0), PLAIN);

        assert_eq!(
            rotate(&mut w, a, v(30.0, 90.0, 0.0), PLAIN),
            RotResult::Rejected
        );
        assert_eq!(w.element(a).base.e, v(0.0, 90.0, 0.0));
        // The test reads the wrapped, unrounded value: 5 rounds to 0 but still counts.
        assert_eq!(
            rotate(&mut w, a, v(5.0, 90.0, 0.0), PLAIN),
            RotResult::Rejected
        );

        // The same axis, a return to zero and a forced change are allowed.
        assert_eq!(
            rotate(&mut w, a, v(0.0, 100.0, 0.0), PLAIN),
            RotResult::Applied
        );
        let force = RotOpts {
            force: true,
            ..PLAIN
        };
        assert_eq!(
            rotate(&mut w, a, v(30.0, 90.0, 0.0), force),
            RotResult::Applied
        );
        assert_eq!(w.element(a).base.e, v(22.5, 90.0, 0.0));
        assert_eq!(rotate(&mut w, a, Vec3::ZERO, PLAIN), RotResult::Applied);
        assert_eq!(
            rotate(&mut w, a, v(30.0, 0.0, 0.0), PLAIN),
            RotResult::Applied
        );
    }

    #[test]
    fn a_stored_360_counts_as_a_rotated_axis() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        rotate(&mut w, a, v(0.0, 355.0, 0.0), PLAIN);
        assert_eq!(w.element(a).base.e.y, 360.0);
        assert_eq!(
            rotate(&mut w, a, v(45.0, 0.0, 0.0), PLAIN),
            RotResult::Rejected
        );
    }

    #[test]
    fn a_model_without_the_restriction_never_rejects_and_takes_the_angles_as_given() {
        let mut w = World::new();
        w.env.restrict_rotations = false;
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        rotate(&mut w, a, v(0.0, 90.0, 0.0), PLAIN);
        assert_eq!(
            rotate(&mut w, a, v(30.0, 90.0, 12.3), PLAIN),
            RotResult::Applied
        );
        assert_eq!(w.element(a).base.e, v(30.0, 90.0, 12.3));
        // Elements, meshes and the blueprint alike; only a turn beyond a full circle wraps, with
        // the exact remainder.
        let m = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        MeshMut::new(&mut w.cx(), m)
            .unwrap()
            .set_rotation(v(22.5, 53.0, 400.5), PLAIN);
        assert_eq!(w.doc.meshes.get(m).unwrap().base.e, v(22.5, 53.0, 40.5));
        rotate(&mut w, a, v(-5.0, 360.0, -400.0), PLAIN);
        assert_eq!(w.element(a).base.e, v(-5.0, 360.0, -40.0));
        BlueprintMut::new(&mut w.cx()).set_rotation(v(1.0, 2.0, 3.0), PLAIN);
        assert_eq!(w.doc.blueprint.base.e, v(1.0, 2.0, 3.0));
    }

    #[test]
    fn a_free_rotation_turns_the_position_about_the_pivot_by_the_angle_given() {
        let mut w = World::new();
        w.env.restrict_rotations = false;
        let a = w.add_element(v(1.0, 0.0, 0.0), Vec3::ONE);
        w.edit_element(a, |el| el.base.o = Vec3::ZERO);
        let about = RotOpts {
            about_pivot: true,
            ..PLAIN
        };
        rotate(&mut w, a, v(0.0, 53.0, 0.0), about);
        assert_eq!(w.element(a).base.e, v(0.0, 53.0, 0.0));
        let turn = euler_to_quat(v(0.0, 53.0, 0.0)) * v(1.0, 0.0, 0.0);
        assert_close(w.element(a).base.p, turn);
    }

    #[test]
    fn meshes_always_use_the_five_degree_grid() {
        let mut w = World::new();
        assert!(w.env.restrict_rotations);
        let m = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        let stored = |w: &World| w.doc.meshes.get(m).unwrap().base.e;
        let set =
            |w: &mut World, r: Vec3| MeshMut::new(&mut w.cx(), m).unwrap().set_rotation(r, PLAIN);
        // Tools 17 item 24: 22.5 loads as 20, 67.5 as 70, 358 as 360, and nothing is rejected.
        assert_eq!(set(&mut w, v(22.5, 0.0, 0.0)), RotResult::Applied);
        assert_eq!(stored(&w), v(20.0, 0.0, 0.0));
        assert_eq!(set(&mut w, v(22.5, 67.5, 358.0)), RotResult::Applied);
        assert_eq!(stored(&w), v(20.0, 70.0, 360.0));
    }

    #[test]
    fn rotation_about_the_pivot_moves_the_position() {
        let mut w = World::new();
        let a = w.add_element(v(1.0, 0.0, 0.0), Vec3::ONE);
        w.edit_element(a, |el| el.base.o = Vec3::ZERO);
        let about = RotOpts {
            about_pivot: true,
            ..PLAIN
        };
        rotate(&mut w, a, v(0.0, 90.0, 0.0), about);
        assert_close(w.element(a).base.p, v(0.0, 0.0, -1.0));
        // A second rotation turns by the difference only.
        rotate(&mut w, a, v(0.0, 180.0, 0.0), about);
        assert_close(w.element(a).base.p, v(-1.0, 0.0, 0.0));

        // Without the pivot flag the orientation alone changes.
        rotate(&mut w, a, v(0.0, 270.0, 0.0), PLAIN);
        assert_close(w.element(a).base.p, v(-1.0, 0.0, 0.0));
        assert_eq!(w.element(a).base.e, v(0.0, 270.0, 0.0));
    }

    #[test]
    fn any_rotation_change_turns_uv_snap_off() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.edit_element(a, |el| {
            el.flags
                .insert(ElementFlags::UV_SNAP | ElementFlags::UV_SNAP_MIRROR)
        });
        rotate(&mut w, a, v(0.0, 45.0, 0.0), PLAIN);
        let flags = w.element(a).flags;
        assert!(!flags.contains(ElementFlags::UV_SNAP));
        assert!(flags.contains(ElementFlags::UV_SNAP_MIRROR));
    }

    #[test]
    fn a_multi_selection_rotates_about_the_primary_pivot_and_the_others_keep_it() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let b = w.add_element(v(2.0, 0.0, 0.0), Vec3::ONE);
        w.edit_element(b, |el| el.base.o = v(5.0, 5.0, 5.0));
        w.select(&[ObjRef::Element(a), ObjRef::Element(b)]);
        let opts = RotOpts {
            propagate: true,
            force: false,
            about_pivot: true,
        };

        assert_eq!(
            rotate(&mut w, a, v(0.0, 90.0, 0.0), opts),
            RotResult::Applied
        );

        assert_eq!(w.element(b).base.e, v(0.0, 90.0, 0.0));
        assert_eq!(
            w.element(b).base.o,
            Vec3::ZERO,
            "the pivot of b was overwritten"
        );
        assert_close(w.element(b).base.p, v(0.0, 0.0, -2.0));
    }

    #[test]
    fn the_blueprint_keeps_its_origin_at_its_position_and_obeys_the_restriction() {
        let mut w = World::new();
        {
            let mut cx = w.cx();
            let mut bp = BlueprintMut::new(&mut cx);
            bp.set_position(v(1.0, 2.0, 3.0), false);
            assert_eq!(
                bp.set_rotation(v(0.0, 90.0, 0.0), PLAIN),
                RotResult::Applied
            );
            assert_eq!(
                bp.set_rotation(v(30.0, 90.0, 0.0), PLAIN),
                RotResult::Rejected
            );
            bp.set_size(v(2.0, 2.0, 2.0));
            bp.set_origin_move(v(4.0, 4.0, 4.0), false);
        }
        let base = &w.doc.blueprint.base;
        assert_eq!(base.p, v(4.0, 4.0, 4.0));
        assert_eq!(base.o, base.p);
        assert_eq!(base.s, v(2.0, 2.0, 2.0));
    }

    // --- Bake and quarter turns (tools 17 items 4 and 5) ---------------------------------------

    /// The eight corners of the box in world space (tools 1.3).
    fn world_corners(base: &ObjectBase) -> Vec<Vec3> {
        let rot = euler_to_quat(base.e);
        let mut corners = Vec::new();
        for x in [0.0, 1.0] {
            for y in [0.0, 1.0] {
                for z in [-1.0, 0.0] {
                    corners.push(base.p + rot * (v(x, y, z) * base.s));
                }
            }
        }
        corners
    }

    const NORMALS: [Vec3; 6] = [
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    ];

    /// The world normal of every face label.
    fn face_normals(el: &Element) -> Vec<(String, Vec3)> {
        let rot = euler_to_quat(el.base.e);
        el.faces
            .iter()
            .zip(NORMALS)
            .map(|(face, n)| (face.texture.to_string(), rot * n))
            .collect()
    }

    /// An element whose faces are labelled `face0` to `face5` in slot order.
    fn labelled_element(w: &mut World, p: Vec3, s: Vec3, o: Vec3) -> ObjId {
        let id = w.add_element(p, s);
        w.edit_element(id, |el| {
            el.base.o = o;
            for (i, face) in el.faces.iter_mut().enumerate() {
                face.texture = Label::from(format!("face{i}"));
            }
        });
        id
    }

    #[test]
    fn the_restriction_bake_keeps_the_geometry_and_leaves_one_small_axis() {
        // The Z turn of tools 4.9 is the engine rotation by -90 (see the next test), so the bake
        // preserves the geometry about Z only where the turn is a half turn or absent.
        for (axis, angles) in [
            (Axis::X, [67.5, 90.0, 180.0, 292.5, 315.0]),
            (Axis::Y, [67.5, 90.0, 180.0, 292.5, 315.0]),
            (Axis::Z, [180.0, 315.0, 0.0, 22.5, 45.0]),
        ] {
            for angle in angles {
                let mut w = World::new();
                let id = labelled_element(
                    &mut w,
                    v(1.0, 2.0, -3.0),
                    v(1.0, 2.0, 3.0),
                    v(3.0, 5.0, -4.0),
                );
                rotate(&mut w, id, with_component(Vec3::ZERO, axis, angle), FORCED);
                let corners = world_corners(&w.element(id).base);
                let normals = face_normals(w.element(id));

                ElementMut::new(&mut w.cx(), id)
                    .unwrap()
                    .bake_restricted_rotation();

                let el = w.element(id);
                let after = world_corners(&el.base);
                for corner in &corners {
                    assert!(
                        after.iter().any(|c| close(*c, *corner)),
                        "{axis:?} {angle}: corner {corner}"
                    );
                }
                let normals_after = face_normals(el);
                for (label, normal) in &normals {
                    let (_, n) = normals_after.iter().find(|(l, _)| l == label).unwrap();
                    assert_close(*n, *normal);
                }
                let e = el.base.e;
                assert!(
                    e.to_array().iter().filter(|&&c| c != 0.0).count() <= 1,
                    "{e}"
                );
                for c in e.to_array() {
                    assert!(c <= 45.0 || c >= 315.0, "{axis:?} {angle}: {e}");
                    assert_eq!(c % 22.5, 0.0);
                }
            }
        }
    }

    /// Tools 4.9 defines the Z quarter turn as (x, y, z) -> (y, -x, z), which is the engine
    /// rotation by -90 degrees about Z (the table's X and Y turns are the +90 rotations). The
    /// bake of tools 3.5 uses it as written, so a Z rotation in (45, 315) other than 180 changes
    /// the geometry. Unverified against the original (ledger note).
    #[test]
    fn the_z_quarter_turn_of_the_table_is_the_engine_rotation_by_minus_90() {
        assert_close(
            euler_to_quat(v(0.0, 0.0, 90.0)) * v(1.0, 0.0, 0.0),
            v(0.0, 1.0, 0.0),
        );
        let (p, s, o) = (v(1.0, 2.0, -3.0), v(1.0, 2.0, 3.0), v(3.0, 5.0, -4.0));
        for (angle, q) in [(67.5, 90.0), (90.0, 90.0), (292.5, 270.0)] {
            let mut w = World::new();
            let id = labelled_element(&mut w, p, s, o);
            rotate(&mut w, id, v(0.0, 0.0, angle), FORCED);
            ElementMut::new(&mut w.cx(), id)
                .unwrap()
                .bake_restricted_rotation();
            // The box turned by -q about the pivot, then by the residual angle.
            let turn = euler_to_quat(v(0.0, 0.0, angle - q)) * euler_to_quat(v(0.0, 0.0, -q));
            let after = world_corners(&w.element(id).base);
            for x in [0.0, 1.0] {
                for y in [0.0, 1.0] {
                    for z in [-1.0, 0.0] {
                        let expected = o + turn * (p + v(x, y, z) * s - o);
                        assert!(
                            after.iter().any(|c| close(*c, expected)),
                            "{angle}: {expected}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_restriction_bake_rewrites_a_quarter_turn_as_faces_and_sizes() {
        let mut w = World::new();
        let id = labelled_element(
            &mut w,
            v(1.0, 2.0, -3.0),
            v(1.0, 2.0, 3.0),
            v(1.0, 2.0, -3.0),
        );
        rotate(&mut w, id, v(0.0, 90.0, 0.0), FORCED);
        ElementMut::new(&mut w.cx(), id)
            .unwrap()
            .bake_restricted_rotation();
        let el = w.element(id);
        assert_eq!(el.base.e, Vec3::ZERO);
        assert_eq!(el.base.s, v(3.0, 2.0, 1.0));
        assert_eq!(el.base.o, v(1.0, 2.0, -3.0), "the pivot is restored");
        // Y turn: north <- west, west <- south, south <- east, east <- north.
        let labels: Vec<&str> = el.faces.iter().map(|f| &*f.texture).collect();
        assert_eq!(
            labels,
            ["face0", "face1", "face4", "face5", "face3", "face2"]
        );
    }

    #[test]
    fn the_primary_bakes_the_other_selected_elements_first() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let b = w.add_element(v(3.0, 0.0, 0.0), v(1.0, 2.0, 3.0));
        rotate(&mut w, a, v(0.0, 45.0, 0.0), FORCED);
        rotate(&mut w, b, v(180.0, 0.0, 0.0), FORCED);
        w.select(&[ObjRef::Element(a), ObjRef::Element(b)]);
        let before = w.element(a).base.clone();

        ElementMut::new(&mut w.cx(), a)
            .unwrap()
            .bake_restricted_rotation();

        assert_eq!(w.element(a).base, before, "45 degrees is already valid");
        assert_eq!(w.element(b).base.e, Vec3::ZERO);
        assert_eq!(
            w.element(b).base.s,
            v(1.0, 2.0, 3.0),
            "two X turns swap the axes back"
        );
    }

    /// A quarter turn about `axis`, as a map of integer vectors.
    fn turn_map(axis: Axis, degrees: i32, n: Vec3) -> Vec3 {
        let step = |n: Vec3| match axis {
            Axis::X => v(n.x, -n.z, n.y),
            Axis::Y => v(n.z, n.y, -n.x),
            Axis::Z => v(n.y, -n.x, n.z),
        };
        (0..degrees / 90).fold(n, |n, _| step(n))
    }

    #[test]
    fn quarter_turns_move_the_box_and_cycle_the_faces_about_the_block_centre() {
        let center = v(8.0, 8.0, -8.0);
        let (p, s) = (v(2.0, 3.0, -4.0), v(1.0, 2.0, 3.0));
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            for degrees in [90, 180, 270] {
                let mut w = World::new();
                let id = labelled_element(&mut w, p, s, p);
                ElementMut::new(&mut w.cx(), id)
                    .unwrap()
                    .quarter_turn(axis, degrees);
                let el = w.element(id);

                // The box: the eight corners turned about the centre.
                let mut min = Vec3::MAX;
                let mut max = Vec3::MIN;
                for x in [0.0, 1.0] {
                    for y in [0.0, 1.0] {
                        for z in [-1.0, 0.0] {
                            let c = turn_map(axis, degrees, p + v(x, y, z) * s - center) + center;
                            min = min.min(c);
                            max = max.max(c);
                        }
                    }
                }
                let case = format!("{axis:?} {degrees}");
                assert_eq!(el.base.p, v(min.x, min.y, max.z), "{case} position");
                assert_eq!(el.base.s, max - min, "{case} size");
                let origin = turn_map(axis, degrees, p - center) + center;
                assert_eq!(el.base.o, origin, "{case} origin");

                // The faces: each label sits on the slot its normal turns into.
                for (i, n) in NORMALS.iter().enumerate() {
                    let target = turn_map(axis, degrees, *n);
                    let slot = NORMALS.iter().position(|m| *m == target).unwrap();
                    assert_eq!(&*el.faces[slot].texture, format!("face{i}"), "{case}");
                    assert_eq!(el.faces[slot].name, FaceName::ALL[slot], "{case}");
                }
                assert!(!el.flags.contains(ElementFlags::UV_SNAP));
            }
        }
    }

    #[test]
    fn four_quarter_turns_restore_the_element() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let mut w = World::new();
            let id = labelled_element(
                &mut w,
                v(2.0, 3.0, -4.0),
                v(1.0, 2.0, 3.0),
                v(2.5, 3.0, -4.0),
            );
            w.edit_element(id, |el| {
                el.faces[0].uv = UvRect::new(90, Vec2::new(0.25, 0.0), Vec2::new(0.75, 0.5));
            });
            let before = w.element(id).clone();
            for _ in 0..4 {
                ElementMut::new(&mut w.cx(), id)
                    .unwrap()
                    .quarter_turn(axis, 90);
            }
            let after = w.element(id);
            assert_eq!(after.base.p, before.base.p, "{axis:?}");
            assert_eq!((after.base.s, after.base.o), (before.base.s, before.base.o));
            assert_eq!(after.faces, before.faces, "{axis:?}");
        }
    }

    #[test]
    fn quarter_turns_change_the_uv_rotations_and_flip_the_arriving_faces() {
        let mut w = World::new();
        let id = labelled_element(&mut w, Vec3::ZERO, Vec3::ONE, Vec3::ZERO);
        w.edit_element(id, |el| {
            for face in &mut el.faces {
                face.uv.set(Vec2::new(0.0, 0.0), Vec2::new(0.5, 1.0));
            }
        });
        let uv = |w: &World, name: FaceName| w.element(id).faces[name as usize].uv;

        ElementMut::new(&mut w.cx(), id)
            .unwrap()
            .quarter_turn(Axis::X, 90);
        assert_eq!(uv(&w, FaceName::West).rot(), 270);
        assert_eq!(uv(&w, FaceName::East).rot(), 90);
        // The faces arriving at down and north have their corners swapped.
        assert_eq!(uv(&w, FaceName::Down).tl_raw(), Vec2::new(0.5, 1.0));
        assert_eq!(uv(&w, FaceName::North).br_raw(), Vec2::new(0.0, 0.0));
        assert_eq!(uv(&w, FaceName::South).tl_raw(), Vec2::new(0.0, 0.0));

        ElementMut::new(&mut w.cx(), id)
            .unwrap()
            .quarter_turn(Axis::Y, 90);
        assert_eq!(uv(&w, FaceName::Down).rot(), 270);
        assert_eq!(uv(&w, FaceName::Up).rot(), 90);
    }

    #[test]
    fn a_quarter_turn_discards_the_rotation_of_the_element() {
        let mut w = World::new();
        let id = labelled_element(&mut w, Vec3::ZERO, Vec3::ONE, Vec3::ZERO);
        rotate(&mut w, id, v(0.0, 45.0, 0.0), PLAIN);
        ElementMut::new(&mut w.cx(), id)
            .unwrap()
            .quarter_turn(Axis::Z, 90);
        assert_eq!(w.element(id).base.e, Vec3::ZERO);
    }

    #[test]
    fn a_quarter_turn_moves_the_folded_corners_of_straddling_rectangles() {
        let mut w = World::new();
        let id = labelled_element(&mut w, Vec3::ZERO, Vec3::ONE, Vec3::ZERO);
        let v2 = Vec2::new;
        let set = |w: &mut World, name: FaceName, tl: Vec2, br: Vec2| {
            w.edit_element(id, |el| el.faces[name as usize].uv.set(tl, br));
        };
        // Raw rectangles that straddle 0 and 1 (and a dragged one) read back folded.
        set(&mut w, FaceName::North, v2(-0.25, 0.0), v2(0.5, 1.0));
        set(&mut w, FaceName::Up, v2(0.75, -0.5), v2(1.5, 0.25));
        set(&mut w, FaceName::Down, v2(1.25, 0.0), v2(1.75, 1.0));
        let uv = |w: &World, name: FaceName| w.element(id).faces[name as usize].uv;
        let (north, up, down) = (
            uv(&w, FaceName::North),
            uv(&w, FaceName::Up),
            uv(&w, FaceName::Down),
        );
        assert_eq!((north.tl(), north.br()), (v2(0.0, 0.0), v2(0.5, 1.0)));

        ElementMut::new(&mut w.cx(), id)
            .unwrap()
            .quarter_turn(Axis::X, 90);

        // North arrives at down and up at north, both with the folded corners swapped.
        for (arrived, from) in [(FaceName::Down, north), (FaceName::North, up)] {
            let got = uv(&w, arrived);
            assert_eq!((got.tl(), got.br()), (from.br(), from.tl()), "{arrived:?}");
            assert_eq!((got.tl_raw(), got.br_raw()), (from.br(), from.tl()));
        }
        assert_eq!(uv(&w, FaceName::Down).tl(), v2(0.5, 1.0));
        assert_eq!(uv(&w, FaceName::Down).br(), v2(0.0, 0.0));
        // Down arrives at south unswapped, stored as the folded values.
        let south = uv(&w, FaceName::South);
        assert_eq!((south.tl_raw(), south.br_raw()), (down.tl(), down.br()));
        assert_eq!(
            (south.tl_raw(), south.br_raw()),
            (v2(0.25, 0.0), v2(0.75, 1.0))
        );
    }

    #[test]
    fn the_faces_only_quarter_turn_leaves_geometry_rotation_and_snap_alone() {
        let mut w = World::new();
        let id = labelled_element(&mut w, v(1.0, 2.0, -3.0), v(1.0, 2.0, 3.0), Vec3::ZERO);
        w.edit_element(id, |el| {
            el.flags.insert(ElementFlags::UV_SNAP);
            el.base.e = v(0.0, 20.0, 0.0);
        });
        let before = w.element(id).clone();

        ElementMut::new(&mut w.cx(), id)
            .unwrap()
            .quarter_turn_faces(Axis::X, 90);

        let el = w.element(id);
        assert_eq!(el.base, before.base);
        assert!(el.flags.contains(ElementFlags::UV_SNAP));
        let label = |name: FaceName| el.faces[name as usize].texture.clone();
        assert_eq!(
            label(FaceName::South),
            before.faces[FaceName::Down as usize].texture
        );
        assert_eq!(
            label(FaceName::Down),
            before.faces[FaceName::North as usize].texture
        );
        assert_eq!(el.faces[FaceName::West as usize].uv.rot(), 270);
        assert_eq!(el.faces[FaceName::East as usize].uv.rot(), 90);

        let mut w2 = World::new();
        let id2 = labelled_element(&mut w2, v(1.0, 2.0, -3.0), v(1.0, 2.0, 3.0), Vec3::ZERO);
        let mut cx = w2.cx();
        let mut guard = ElementMut::new(&mut cx, id2).unwrap();
        guard.quarter_turn_faces(Axis::Z, 90);
        let faces_z = w2.element(id2).faces.clone();
        let mut w3 = World::new();
        let id3 = labelled_element(&mut w3, v(1.0, 2.0, -3.0), v(1.0, 2.0, 3.0), Vec3::ZERO);
        let mut cx = w3.cx();
        let mut guard = ElementMut::new(&mut cx, id3).unwrap();
        guard.quarter_turn_faces(Axis::X, 90);
        guard.quarter_turn_faces(Axis::Y, 90);
        guard.quarter_turn_faces(Axis::X, 270);
        assert_eq!(
            w3.element(id3).faces,
            faces_z,
            "Z is X(+90), Y(+90), X(-90)"
        );
    }

    // --- Model box and origin ------------------------------------------------------------------

    #[test]
    fn the_model_box_setter_clamps_in_order_and_notifies_the_voxel_world() {
        let mut w = World::new();
        let res = 16;
        w.doc.voxels = Some(VoxelWorld::new(
            res,
            true,
            cuboid_voxel::mapping::grid_box(res),
        ));
        assert!(!w.doc.voxels.as_ref().unwrap().is_dirty());

        let requested = ModelBox {
            min: v(0.0, 0.0, -16.0),
            max: v(16.0, 16.0, 0.0),
        };
        set_model_box(&mut w.doc, requested);

        assert_eq!(w.doc.model_box, requested);
        assert_eq!(w.doc.props.general.model_box, Some(requested));
        let world = w.doc.voxels.as_ref().unwrap();
        assert_eq!(
            world.active_box,
            cuboid_voxel::mapping::active_box(requested, res)
        );
        assert!(world.is_dirty(), "the exposed and hidden chunks are dirty");
        let flags = w.doc.journal.doc();
        assert!(flags.contains(DocFlags::MODEL_BOX | DocFlags::VOXEL_CHUNKS));

        // Out-of-range requests clamp: xmin >= -16, xmax <= 32, min <= max in the setter order.
        let wild = ModelBox {
            min: v(-100.0, 100.0, -100.0),
            max: v(100.0, -100.0, 100.0),
        };
        set_model_box(&mut w.doc, wild);
        let b = w.doc.model_box;
        assert_eq!((b.min.x, b.max.x), (-16.0, 32.0));
        assert_eq!((b.min.z, b.max.z), (-32.0, 16.0));
        assert_eq!((b.min.y, b.max.y), (16.0, 16.0));
    }

    #[test]
    fn an_unchanged_model_box_does_nothing() {
        let mut w = World::new();
        set_model_box(&mut w.doc, ModelBox::EDITING_VOLUME);
        w.doc.journal.clear();
        let rev = w.doc.revision;
        set_model_box(&mut w.doc, ModelBox::EDITING_VOLUME);
        assert!(w.doc.journal.is_empty());
        assert_eq!(w.doc.revision, rev);
    }
}
