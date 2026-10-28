//! The fixes of the validator as commands of the core. The Validator panel of the application and
//! the `apply_fixes` op map a [`FixOp`] to the same commands; all of them run as one transaction,
//! so a fix is one undo step.

use std::convert::Infallible;

use cuboid_core::{Batch, CmdOutcome, Command, Core, FaceId};
use cuboid_model::{FaceName, ObjId, ObjRef, mc};
use cuboid_validate::{FixOp, Issue};
use glam::Vec3;

use crate::error::ToolError;

/// What a set of fixes did.
#[derive(Debug, Default)]
pub struct FixSummary {
    /// Edits that changed the model.
    pub applied: usize,
    /// Why edits were refused or failed.
    pub failed: Vec<String>,
    /// The elements of the issues whose fixes changed the model.
    pub fixed: Vec<ObjId>,
}

/// Whether the commands of the core can run this fix.
fn supported(op: &FixOp) -> bool {
    matches!(
        op,
        FixOp::SetRotation { .. }
            | FixOp::ClampIntoBounds { .. }
            | FixOp::AssignTexture { .. }
            | FixOp::SetUv { .. }
            | FixOp::RemoveTexture { .. }
            | FixOp::RenameElement { .. }
    )
}

/// Whether all the fixes of an issue can be run.
pub fn fixable(issue: &Issue) -> bool {
    issue
        .fix
        .as_ref()
        .is_some_and(|ops| !ops.is_empty() && ops.iter().all(supported))
}

pub(crate) fn face_id(face: FaceName) -> FaceId {
    match face {
        FaceName::Down => FaceId::Down,
        FaceName::Up => FaceId::Up,
        FaceName::North => FaceId::North,
        FaceName::South => FaceId::South,
        FaceName::West => FaceId::West,
        FaceName::East => FaceId::East,
    }
}

/// Runs a command; a command that is refused or fails is recorded and the fixes go on.
fn run(batch: &mut Batch<'_>, command: Command, summary: &mut FixSummary) {
    match batch.execute(command) {
        Ok(CmdOutcome::Done(_)) => summary.applied += 1,
        Ok(_) => {}
        Err(outcome) => summary.failed.push(
            ToolError::of(&outcome)
                .map_or_else(|| "a command failed".to_owned(), |error| error.message),
        ),
    }
}

/// Moves and shrinks an element until its box, in the Minecraft frame, lies within `min..max`.
fn clamp_into_bounds(
    batch: &mut Batch<'_>,
    id: ObjId,
    (min, max): (f32, f32),
    summary: &mut FixSummary,
) {
    let Some(target) = batch.core().target_of(ObjRef::Element(id)) else {
        return;
    };
    let mc_box = |core: &Core| {
        core.doc().elements.get(id).map(|el| {
            let b = &el.base;
            let boxed = mc::unplace(b.p, b.s, b.o, b.e, false);
            (b.s, boxed.from.min(boxed.to), boxed.from.max(boxed.to))
        })
    };
    let Some((size, lo, hi)) = mc_box(batch.core()) else {
        return;
    };
    let range = max - min;
    let extent = hi - lo;
    if extent.max_element() > range {
        let fit = |axis: usize| (extent[axis] > range).then(|| range.copysign(size[axis]));
        let command = Command::SetSize {
            targets: vec![target],
            value: [fit(0), fit(1), fit(2)],
        };
        run(batch, command, summary);
    }
    let Some((_, lo, hi)) = mc_box(batch.core()) else {
        return;
    };
    let shift = |axis: usize| {
        if lo[axis] < min {
            min - lo[axis]
        } else if hi[axis] > max {
            max - hi[axis]
        } else {
            0.0
        }
    };
    let moved = Vec3::new(shift(0), shift(1), shift(2));
    if moved != Vec3::ZERO {
        // The stored z runs the other way (`E(x, y, z) = M(x, y, -z)`).
        let command = Command::MoveBy {
            targets: vec![target],
            delta: [moved.x, moved.y, -moved.z],
            snap: None,
        };
        run(batch, command, summary);
    }
}

fn apply_op(batch: &mut Batch<'_>, op: &FixOp, summary: &mut FixSummary) {
    let element = |batch: &Batch<'_>, id: ObjId| batch.core().target_of(ObjRef::Element(id));
    match op {
        FixOp::SetRotation { id, rotation } => {
            if let Some(target) = element(batch, *id) {
                let command = Command::SetRotation {
                    targets: vec![target],
                    value: [Some(rotation.x), Some(rotation.y), Some(rotation.z)],
                };
                run(batch, command, summary);
            }
        }
        FixOp::ClampIntoBounds { id, min, max } => {
            clamp_into_bounds(batch, *id, (*min, *max), summary);
        }
        FixOp::AssignTexture { id, face, label } => {
            if let Some(target) = element(batch, *id) {
                let command = Command::AssignTexture {
                    targets: vec![target],
                    label: label.to_string(),
                    faces: vec![face_id(*face)],
                };
                run(batch, command, summary);
            }
        }
        FixOp::SetUv { id, face, tl, br } => {
            if let Some(target) = element(batch, *id) {
                let command = Command::SetUvRect {
                    targets: vec![target],
                    faces: vec![face_id(*face)],
                    rect: [tl.x, tl.y, br.x, br.y],
                    rotation: None,
                };
                run(batch, command, summary);
            }
        }
        FixOp::RemoveTexture { label } => {
            let command = Command::RemoveTexture {
                label: label.to_string(),
            };
            run(batch, command, summary);
        }
        FixOp::RenameElement { id, name } => {
            if let Some(target) = element(batch, *id) {
                let command = Command::Rename {
                    targets: vec![target],
                    name: name.clone(),
                };
                run(batch, command, summary);
            }
        }
        other => summary
            .failed
            .push(format!("no automatic fix for {other:?}")),
    }
}

/// Runs the fixes of `issues` in the transaction `batch`. A fix that cannot be run is recorded in
/// the summary and the rest go on.
pub fn apply_in(batch: &mut Batch<'_>, issues: &[&Issue], summary: &mut FixSummary) {
    for issue in issues {
        let before = summary.applied;
        for op in issue.fix.iter().flatten() {
            apply_op(batch, op, summary);
        }
        if summary.applied > before {
            summary.fixed.extend(&issue.objects);
        }
    }
}

/// Runs the fixes of `issues` as one undo step.
pub fn apply_fixes(core: &mut Core, issues: &[&Issue]) -> FixSummary {
    let mut summary = FixSummary::default();
    let Ok(()) = core.batch::<(), Infallible>("Fix validation issues", |batch| {
        apply_in(batch, issues, &mut summary);
        Ok(())
    });
    summary
}
