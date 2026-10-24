//! Public contract stubs of this crate: cross-crate types and traits with their spec references
//! (W0.2).
//!
//! This file holds the vocabulary the interaction state machine and the tools share and the
//! small types of the document swap. The `Core` facade, the history, the commands, the clock and
//! the other state live in their modules next to the stubs W2.1 and W2.9 replace; `lib.rs`
//! re-exports everything at the crate root.

use cuboid_model::ObjRef;
use cuboid_scene::Hit;
use thiserror::Error;

use crate::tools::ToolCx;

/// A mouse button of a stroke.
///
/// spec: tools 8.7, tools 12.2
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Button {
    /// Uses the left colour; adds.
    Left,
    /// Uses the right colour; removes or recolours.
    Right,
}

/// What a tool wants after a callback.
///
/// spec: tools 12.2, tools 13.2, tools 12.3 (Wrap)
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StrokeStep {
    /// Keep the stroke going.
    Continue,
    /// End the stroke: an error was logged or the tool is done.
    End,
    /// Select this object 0.1 s later, which becomes the primary and switches the tool off
    /// (the Wrap tool selects the blueprint).
    SelectLater(ObjRef),
}

/// A paint, colour-picker or voxel tool. The interaction state machine calls `begin` on the
/// first callback of a press, `drag` on every later frame while the button is armed and the hover
/// ray hits something, and `end` on release. Stroke lifecycle details (Left Alt eyedropper,
/// same-face gate, dedupe, plane pin, preview cache) are the tool's.
///
/// spec: arch 6 (cuboid-core), arch 10.5, tools 12.2, tools 13.2
pub trait CustomTool {
    /// The first callback of a press: undo is pushed, the hit normal captured, collider updates
    /// suspended.
    fn begin(&mut self, cx: &mut ToolCx<'_>, button: Button, hit: &Hit) -> StrokeStep;

    /// A later frame of the stroke; `hit` is `None` when the ray hits nothing.
    fn drag(&mut self, cx: &mut ToolCx<'_>, hit: Option<&Hit>) -> StrokeStep;

    /// The release: collider updates resume and all chunk colliders are refreshed.
    fn end(&mut self, cx: &mut ToolCx<'_>);
}

/// How a document replaces the live one.
///
/// spec: arch 11.2, native 6.1, interop 5.3
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum SwapKind {
    /// A native open or a model reset: history and dirty flag are cleared, the selection is
    /// cleared, the document path becomes the opened path.
    Open,
    /// An import that adds to or resets part of the model: history is kept.
    Import,
}

/// The base revision of a prepared document no longer matches the live one: something changed
/// while the worker built it, so the result is dropped.
///
/// spec: arch 11.2
#[derive(Copy, Clone, Debug, Eq, PartialEq, Error)]
#[error("the document changed while the result was prepared")]
pub struct Stale;

#[cfg(test)]
mod tests {
    use super::*;

    /// The interaction state machine keeps the active tool as a trait object.
    fn _custom_tool_is_object_safe(_: &dyn CustomTool) {}
}
