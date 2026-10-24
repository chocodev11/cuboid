//! Gizmo drags: the pointer movement of a press-drag-release becomes edits through
//! `cuboid-ops` (spec: tools 9.1-9.3).

use cuboid_model::{
    CaptureScope, Categories, Document, Label, ObjRef, Perms, Rev, RotOpts, Snapshot, capture,
};
use cuboid_num::euler_to_quat;
use cuboid_ops::transform::{
    bake_restricted, base_of, clean_after_drag, pivot_of, set_origin_move, set_rotation, set_size,
    snap_to, world_center,
};
use cuboid_scene::{Axes, GizmoHit, GizmoKind, GizmoPart, GizmoState, Ray, Raycaster};
use glam::{Quat, Vec2, Vec3};

use super::{SelectMode, SnapSettings, select};
use crate::core::Core;

/// The reach of the drag plane ray in units.
const DRAG_RAY_LEN: f32 = 200.0;

fn needed_perm(kind: GizmoKind) -> Perms {
    match kind {
        GizmoKind::Move => Perms::MOVE,
        GizmoKind::Rotate => Perms::ROTATE,
        GizmoKind::Scale => Perms::SCALE,
    }
}

fn perms_of(doc: &Document, obj: ObjRef) -> Perms {
    base_of(doc, obj).map_or(Perms::MODEL_ORIGIN, |b| b.perms)
}

/// The gizmo the mode shows for the object, if the object allows it.
fn kind_for(doc: &Document, obj: ObjRef, mode: SelectMode) -> Option<GizmoKind> {
    let kind = match mode {
        SelectMode::Move => GizmoKind::Move,
        SelectMode::Rotate => GizmoKind::Rotate,
        SelectMode::Scale => GizmoKind::Scale,
        SelectMode::Custom => return None,
    };
    perms_of(doc, obj)
        .contains(needed_perm(kind))
        .then_some(kind)
}

/// What picking and drawing need to know about the gizmo of the primary object; `None` with no
/// primary, with a tool active, or when the object does not allow the mode's transform.
pub(crate) fn gizmo_state(core: &Core, mode: SelectMode) -> Option<GizmoState> {
    let primary = core.sel.primary?;
    let kind = kind_for(&core.doc, primary, mode)?;
    let state = match base_of(&core.doc, primary) {
        Some(base) => GizmoState {
            kind: Some(kind),
            origin: base.o,
            rotation: euler_to_quat(base.e),
            size: base.s,
            gizmo_size: base.gizmo_size,
            gizmo_pivot: base.gizmo_pivot,
            box_handles: None,
        },
        None => GizmoState {
            kind: Some(kind),
            origin: core.doc.props.general.origin,
            rotation: Quat::IDENTITY,
            size: Vec3::ONE,
            gizmo_size: Vec3::ONE,
            gizmo_pivot: Vec3::ZERO,
            box_handles: None,
        },
    };
    Some(state)
}

fn mask_of(axes: Axes) -> Vec3 {
    let on = |flag: bool| if flag { 1.0 } else { 0.0 };
    Vec3::new(on(axes.x), on(axes.y), on(axes.z))
}

fn side(v: f32) -> f32 {
    if v < 0.0 { -1.0 } else { 1.0 }
}

/// A gizmo drag in progress. The snapshot before the first change waits here until the drag
/// changes something, then becomes the undo entry.
#[derive(Debug)]
pub(crate) struct Drag {
    kind: GizmoKind,
    primary: ObjRef,
    mask: Vec3,
    /// The plane of the pointer ray passes through here.
    through: Vec3,
    start_hit: Vec3,
    start_origin: Vec3,
    start_size: Vec3,
    start_rot: Vec3,
    /// The rotation of the object, to bring the drag into its local axes.
    frame: Quat,
    /// Pointer `x + y` at the press (rotation drags).
    start_sum: f32,
    /// Which side of its box a scale handle sits on, per axis.
    sign: Vec3,
    sensitivity: f32,
    grid_snap_on_scale: bool,
    gesture: u64,
    before: Option<Snapshot>,
    categories: Categories,
    rev0: Rev,
    pushed: bool,
}

impl Drag {
    /// Starts a drag on the handle `hit` of the primary object. `None` when the object is
    /// locked, hidden, lacks the permission, or the drag plane cannot be found.
    pub(crate) fn start(
        core: &Core,
        hit: &GizmoHit,
        ray: Ray,
        pointer: Vec2,
        rc: &dyn Raycaster,
        mode: SelectMode,
        gesture: u64,
    ) -> Option<Self> {
        if matches!(hit.part, GizmoPart::BoxHandle(_)) {
            return None;
        }
        let doc = &core.doc;
        let primary = core.sel.primary?;
        let kind = kind_for(doc, primary, mode)?;
        let base = base_of(doc, primary);
        if base.is_some_and(|b| b.locked || b.hidden) {
            return None;
        }
        let origin = pivot_of(doc, primary)?;
        let start_hit = rc.drag_plane(ray, origin, DRAG_RAY_LEN)?;
        let (size, rot, sensitivity, snaps) =
            base.map_or((Vec3::ONE, Vec3::ZERO, 1.0, false), |b| {
                (
                    b.s,
                    b.e,
                    b.drag_sensitivity,
                    b.perms.contains(Perms::GRID_SNAP_ON_SCALE),
                )
            });
        let frame = euler_to_quat(rot);
        let sign = base.map_or(Vec3::ONE, |b| {
            let local = frame.inverse() * (hit.point - world_center(b));
            Vec3::new(side(local.x), side(local.y), side(local.z))
        });
        let categories = if primary == ObjRef::ModelOrigin {
            Categories::OBJECTS | Categories::MODEL_PROPERTIES
        } else {
            Categories::OBJECTS
        };
        Some(Self {
            kind,
            primary,
            mask: mask_of(hit.axes),
            through: origin,
            start_hit,
            start_origin: origin,
            start_size: size,
            start_rot: rot,
            frame,
            start_sum: pointer.x + pointer.y,
            sign,
            sensitivity,
            grid_snap_on_scale: snaps,
            gesture,
            before: Some(capture(
                doc,
                &core.sel,
                CaptureScope {
                    categories,
                    selection_only: false,
                },
            )),
            categories,
            rev0: doc.revision,
            pushed: false,
        })
    }

    /// Whether the undo entry of the drag was pushed already.
    pub(crate) fn pushed(&self) -> bool {
        self.pushed
    }

    fn label(&self) -> &'static str {
        match self.kind {
            GizmoKind::Move => "Move selection",
            GizmoKind::Rotate => "Rotate selection",
            GizmoKind::Scale => "Scale selection",
        }
    }

    /// Applies the pointer position of this frame. Returns whether the document changed.
    pub(crate) fn update(
        &mut self,
        core: &mut Core,
        rc: &dyn Raycaster,
        ray: Ray,
        pointer: Vec2,
        snap: SnapSettings,
    ) -> bool {
        let mut cx = core.cx();
        match self.kind {
            GizmoKind::Move => {
                let Some(hit) = rc.drag_plane(ray, self.through, DRAG_RAY_LEN) else {
                    return false;
                };
                let mut target = self.start_origin + (hit - self.start_hit) * self.mask;
                if snap.enabled {
                    target = self.snapped(target, snap.grid);
                }
                set_origin_move(&mut cx, self.primary, target, true);
            }
            GizmoKind::Rotate => {
                let d = pointer.x + pointer.y - self.start_sum;
                let mut target = self.start_rot + self.mask * Vec3::new(d, -d, -d);
                if snap.enabled {
                    target = self.snapped(target, snap.angle);
                }
                let opts = RotOpts {
                    propagate: true,
                    force: false,
                    about_pivot: true,
                };
                set_rotation(&mut cx, self.primary, target, opts);
            }
            GizmoKind::Scale => {
                let Some(hit) = rc.drag_plane(ray, self.through, DRAG_RAY_LEN) else {
                    return false;
                };
                let local = self.frame.inverse() * (hit - self.start_hit);
                let mut target = self.start_size + local * self.sign * self.mask * self.sensitivity;
                if snap.enabled && self.grid_snap_on_scale {
                    target = self.snapped(target, snap.grid);
                }
                set_size(&mut cx, self.primary, target, true);
            }
        }
        let changed = core.doc.revision != self.rev0;
        if changed && !self.pushed {
            if let Some(before) = self.before.take() {
                core.record(
                    Label::from(self.label()),
                    self.categories,
                    before,
                    Some(self.gesture),
                );
            }
            self.pushed = true;
        }
        changed
    }

    /// Snaps the masked components only.
    fn snapped(&self, v: Vec3, step: f32) -> Vec3 {
        let pick = |c: f32, m: f32, snapped: f32| if m > 0.0 { snapped } else { c };
        Vec3::new(
            pick(v.x, self.mask.x, snap_to(v.x, step)),
            pick(v.y, self.mask.y, snap_to(v.y, step)),
            pick(v.z, self.mask.z, snap_to(v.z, step)),
        )
    }

    /// Ends the drag: a rotation drag bakes the restricted rotation, an unrotated object gets
    /// its position cleaned to four decimals.
    pub(crate) fn finish(self, core: &mut Core) {
        if !self.pushed {
            return;
        }
        let objs = select::list(&core.sel);
        let mut cx = core.cx();
        if self.kind == GizmoKind::Rotate && cx.env.restrict_rotations {
            bake_restricted(&mut cx, self.primary);
        }
        clean_after_drag(&mut cx, &objs);
    }
}
