//! `CustomTool` implementations for the voxel, paint and colour tools, with the stroke lifecycle
//! (spec: tools 12.2, 13).

use cuboid_model::{CaptureScope, Categories, EditCtx, Label, Snapshot, capture};
use cuboid_num::pack::from_byte;

use crate::commands::{ColliderAction, Effects, Refresh};
use crate::history::History;

mod paint;
mod voxel;

pub use paint::{PaintOptions, PaintTool, PaintToolKind};
pub use voxel::{VoxelOptions, VoxelTool, VoxelToolKind};

/// The left and right tool colours, RGBA in 0..=1.
///
/// spec: tools 13.4, tools 12.1
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ToolColors {
    /// Used by left strokes, mesh tint and voxel fills; `#808080`.
    pub left: [f32; 4],
    /// Used by right strokes; `#00000000`, so right-button tools erase by default.
    pub right: [f32; 4],
}

impl Default for ToolColors {
    fn default() -> Self {
        Self {
            left: [from_byte(128), from_byte(128), from_byte(128), 1.0],
            right: [0.0; 4],
        }
    }
}

/// The size fields of the tool panels, as the integers they are read as (empty or decimal text
/// reads 0; there is no clamp).
///
/// spec: tools 8.10 (Size fields), tools 12.1, tools 13.1
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ToolSizes {
    /// Voxel sphere brush diameter, 7.
    pub voxel_sphere: i32,
    /// Voxel cube brush diameter, 7.
    pub voxel_cube: i32,
    /// Paint line diameter, 1.
    pub paint_line: i32,
    /// Paint round brush diameter, 7.
    pub paint_brush: i32,
    /// Paint square brush diameter, 7.
    pub paint_square: i32,
    /// Paint sprayer diameter, 7.
    pub paint_sprayer: i32,
    /// Paint noise amount, 32.
    pub paint_noise: i32,
}

impl Default for ToolSizes {
    fn default() -> Self {
        Self {
            voxel_sphere: 7,
            voxel_cube: 7,
            paint_line: 1,
            paint_brush: 7,
            paint_square: 7,
            paint_sprayer: 7,
            paint_noise: 32,
        }
    }
}

/// What a tool stroke may touch: the edit context, the history (a stroke pushes its undo entry on
/// its first effective frame), the dirty flag, the effects of the tick (a stroke asks for the
/// voxel colliders to be frozen and refreshed) and the tool settings.
///
/// spec: arch 10.5, tools 12.2, tools 13.2
pub struct ToolCx<'a> {
    /// The document, selection, environment and generator.
    pub edit: EditCtx<'a>,
    /// The undo history.
    pub history: &'a mut History<Snapshot>,
    /// The unsaved-changes flag, set by every undo push.
    pub dirty: &'a mut bool,
    /// The effects of this tick; the stroke adds its collider requests here.
    pub effects: &'a mut Effects,
    /// The left and right colours; the colour pickers set them.
    pub colors: &'a mut ToolColors,
    /// The size fields.
    pub sizes: &'a ToolSizes,
}

impl ToolCx<'_> {
    /// The state of the parts a stroke edits, taken before its first change.
    pub(crate) fn snapshot_of(&self, categories: Categories) -> Snapshot {
        capture(
            self.edit.doc,
            self.edit.sel,
            CaptureScope {
                categories,
                selection_only: false,
            },
        )
    }

    /// Pushes the undo step of a stroke on its first effective change: `before` is the state
    /// taken before it. A step that changed voxels refreshes the colliders when undone or redone.
    pub(crate) fn push_undo(&mut self, label: &'static str, before: Snapshot) {
        let categories = before.categories;
        let callback = categories.contains(Categories::VOXELS).then(|| Effects {
            colliders: vec![ColliderAction::Refresh],
            ..Effects::default()
        });
        self.history
            .push(Label::from(label), categories, before, callback);
        *self.dirty = true;
        self.effects.refresh |= Refresh::EDIT_MENU | Refresh::TITLE;
        if categories.intersects(Categories::TEXTURES) {
            self.effects.refresh |= Refresh::TEXTURES;
        }
        self.effects.status = Some(label.to_owned());
    }

    /// Shows a message in the status line.
    pub(crate) fn say(&mut self, text: impl Into<String>) {
        self.effects.status = Some(text.into());
    }
}

#[cfg(test)]
mod tests {
    use cuboid_num::pack::to_byte;

    use super::*;

    #[test]
    fn tool_defaults_follow_tools_8_10_and_13_4() {
        let colors = ToolColors::default();
        assert_eq!(colors.left.map(to_byte), [128, 128, 128, 255]);
        assert_eq!(colors.right.map(to_byte), [0, 0, 0, 0]);
        let sizes = ToolSizes::default();
        assert_eq!(
            [
                sizes.voxel_sphere,
                sizes.voxel_cube,
                sizes.paint_line,
                sizes.paint_brush,
                sizes.paint_square,
                sizes.paint_sprayer,
                sizes.paint_noise,
            ],
            [7, 7, 1, 7, 7, 7, 32]
        );
    }
}
