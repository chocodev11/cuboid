//! What the validator reports: issues with stable codes, and the model edits that fix them.

use cuboid_model::{DisplaySlotId, FaceFlags, FaceName, Label, ModelKind, ObjId};
use glam::{Vec2, Vec3};

/// How bad an issue is.
///
/// An error means the target rejects or breaks the model. A warning means the game or the format
/// changes the model silently (clamps, drops, renders badly). Info is worth knowing and legal.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Severity {
    /// The target cannot take the model as it is.
    Error,
    /// The target changes or mis-renders the model.
    Warning,
    /// A fact, not a defect.
    Info,
}

impl Severity {
    /// Orders by importance: error 2, warning 1, info 0.
    pub fn rank(self) -> u8 {
        match self {
            Severity::Error => 2,
            Severity::Warning => 1,
            Severity::Info => 0,
        }
    }
}

/// A model edit that removes an issue, as plain data. The application maps each to its command;
/// the validator never edits a document.
#[derive(Clone, Debug, PartialEq)]
pub enum FixOp {
    /// Set the element rotation through the rotation setter. `rotation` is in stored degrees
    /// (components from 0 to 360), so the setter re-places the corner about the pivot.
    SetRotation {
        /// The element.
        id: ObjId,
        /// The new Euler rotation.
        rotation: Vec3,
    },
    /// Move the element so that its box, in the Minecraft frame, lies within `min..max` on every
    /// axis; shrink it when it is larger than the range.
    ClampIntoBounds {
        /// The element.
        id: ObjId,
        /// Lower limit in model units.
        min: f32,
        /// Upper limit in model units.
        max: f32,
    },
    /// Point a face at an existing texture.
    AssignTexture {
        /// The element.
        id: ObjId,
        /// The face.
        face: FaceName,
        /// The texture label.
        label: Label,
    },
    /// Set both corners of a face rectangle (normalised, Y up).
    SetUv {
        /// The element.
        id: ObjId,
        /// The face.
        face: FaceName,
        /// First corner.
        tl: Vec2,
        /// Second corner.
        br: Vec2,
    },
    /// Set the cull direction of a face.
    SetCull {
        /// The element.
        id: ObjId,
        /// The face.
        face: FaceName,
        /// The new cull direction; empty for none.
        cull: FaceFlags,
    },
    /// Delete a texture entry.
    RemoveTexture {
        /// The texture label.
        label: Label,
    },
    /// Replace the Minecraft path of a texture entry.
    SetTexturePath {
        /// The texture label.
        label: Label,
        /// The new path.
        path: String,
    },
    /// Set the translation and scale of a display slot.
    SetDisplay {
        /// The slot.
        slot: DisplaySlotId,
        /// The new translation.
        translation: Vec3,
        /// The new scale.
        scale: Vec3,
    },
    /// Set the Minecraft model kind.
    SetModelKind {
        /// The new kind.
        kind: ModelKind,
    },
}

/// One finding.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    /// Stable code, for example `GEO_ROTATION`. A code never changes its meaning.
    pub code: &'static str,
    /// How bad it is.
    pub severity: Severity,
    /// The elements concerned, empty for a finding about the model, a texture or the pack.
    pub objects: Vec<ObjId>,
    /// What is wrong, naming the element, face or texture.
    pub message: String,
    /// How to remove it.
    pub hint: String,
    /// Edits that remove it, when they can be computed.
    pub fix: Option<Vec<FixOp>>,
}

impl Issue {
    /// An issue without a fix.
    pub(crate) fn new(
        code: &'static str,
        severity: Severity,
        objects: Vec<ObjId>,
        message: String,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity,
            objects,
            message,
            hint: hint.into(),
            fix: None,
        }
    }

    /// The same issue with edits that fix it.
    pub(crate) fn fixed_by(mut self, fix: Vec<FixOp>) -> Self {
        self.fix = (!fix.is_empty()).then_some(fix);
        self
    }
}
