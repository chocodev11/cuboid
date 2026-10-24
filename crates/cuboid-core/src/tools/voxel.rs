//! The voxel tools: pencil, erase, fill, box, line and colour picker, with a cube brush and a
//! mirror on X, Y and Z (spec: tools 12.1-12.3, 13.3, 13.4).
//!
//! A stroke is one undo step, pushed on its first change. Strokes pick through the hit the
//! interaction state machine gives them; the colliders are frozen while a stroke runs, so the
//! hits keep naming the voxels the stroke began on.

use cuboid_model::{Categories, Snapshot};
use cuboid_num::pack::{from_byte, to_byte, unpack_opaque, voxel_tool};
use cuboid_ops::voxel::{
    self, Brush, FILL_LIMIT, MAX_BRUSH, Put, VoxelError, box_cells, hit_cells, line_cells,
};
use cuboid_scene::Hit;

use super::ToolCx;
use crate::commands::ColliderAction;
use crate::contract::{Button, CustomTool, StrokeStep};
use crate::tools::ToolColors;
use cuboid_model::voxel::I3;

/// What a voxel tool does.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum VoxelToolKind {
    /// Left adds voxels of the left colour on the face under the pointer; right recolours the
    /// voxel under it with the right colour, which erases while that is transparent.
    Pencil,
    /// Removes the voxels under the pointer with either button.
    Erase,
    /// Recolours the voxels connected to the one clicked and of its colour (left colour on the
    /// left button, right colour on the right).
    Fill,
    /// Drag a cuboid from the first to the current cell; left adds, right recolours what exists.
    Box,
    /// Drag a line from the first to the current cell; left adds, right recolours what exists.
    Line,
    /// Sets the left (left button) or right (right button) colour to the voxel under the
    /// pointer.
    Picker,
}

/// The settings of a voxel tool.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VoxelOptions {
    /// Edge of the cube brush in voxels, 1 to 16; the line, the pencil and the eraser use it.
    pub brush: i32,
    /// Mirror across the planes through the middle of the active box, per axis.
    pub mirror: [bool; 3],
    /// The most voxels one fill may change; a larger region is left alone.
    pub fill_limit: usize,
}

impl Default for VoxelOptions {
    fn default() -> Self {
        Self {
            brush: 1,
            mirror: [false; 3],
            fill_limit: FILL_LIMIT,
        }
    }
}

/// The state of the stroke in progress.
struct Stroke {
    /// Writes the colour on the outward cell (left button) or recolours the inward one.
    add: bool,
    /// The packed colour the stroke writes; 0 erases.
    value: u32,
    /// The face the stroke began on, as axis and direction: a pencil only adds on this face.
    face: (usize, bool),
    /// The plane a pencil adds on, as the index along the axis of `face`.
    pin: i32,
    /// The first cell of a line or a box.
    anchor: I3,
    /// The last target cell, to act once per cell.
    last: Option<I3>,
    /// The old values of the preview of a line or a box, to roll it back.
    log: Vec<(I3, u32)>,
    /// The state before the stroke; pushed as the undo step by the first change.
    before: Option<Snapshot>,
}

/// A voxel tool for [`Core::set_tool`](crate::Core::set_tool).
pub struct VoxelTool {
    kind: VoxelToolKind,
    options: VoxelOptions,
    stroke: Option<Stroke>,
    cells: Vec<I3>,
}

impl VoxelTool {
    /// A tool of this kind.
    pub fn new(kind: VoxelToolKind, options: VoxelOptions) -> Self {
        Self {
            kind,
            options,
            stroke: None,
            cells: Vec::new(),
        }
    }

    fn label(kind: VoxelToolKind, add: bool) -> &'static str {
        match (kind, add) {
            (VoxelToolKind::Fill, _) => "Fill voxels",
            (VoxelToolKind::Line, true) => "Voxel add line",
            (VoxelToolKind::Line, false) => "Voxel sub line",
            (VoxelToolKind::Box, true) => "Voxel add cuboid",
            (VoxelToolKind::Box, false) => "Voxel sub cuboid",
            (_, true) => "Voxel add",
            (_, false) => "Voxel sub",
        }
    }

    fn brush(&self) -> Brush {
        let size = match self.kind {
            VoxelToolKind::Box | VoxelToolKind::Fill => 1,
            _ => self.options.brush.clamp(1, MAX_BRUSH),
        };
        Brush {
            size,
            mirror: self.options.mirror,
        }
    }

    /// Acts on the cell a hit selects. `first` is the begin of the stroke.
    fn step(&mut self, cx: &mut ToolCx<'_>, hit: &Hit, first: bool) -> StrokeStep {
        let Some(world) = cx.edit.doc.voxels.as_ref() else {
            cx.say(VoxelError::NoWorld.to_string());
            return StrokeStep::End;
        };
        let active = world.active_box;
        let Some(face) = hit_cells(hit.point, hit.normal, world.res) else {
            return StrokeStep::Continue;
        };
        let brush = self.brush();
        let (kind, fill_limit) = (self.kind, self.options.fill_limit);
        let Some(stroke) = self.stroke.as_mut() else {
            return StrokeStep::End;
        };
        let planar = kind == VoxelToolKind::Pencil && stroke.add;
        if first {
            stroke.face = (face.axis, face.positive);
            stroke.pin = face.outward[face.axis];
        } else if planar && stroke.face != (face.axis, face.positive) {
            // A pencil adds on the face it began on only.
            return StrokeStep::Continue;
        }
        let mut target = if stroke.add {
            face.outward
        } else {
            face.inward
        };
        if planar {
            target[stroke.face.0] = stroke.pin;
        }
        if first {
            stroke.anchor = target;
        } else if stroke.last == Some(target) {
            return StrokeStep::Continue;
        }
        let put = Put {
            value: stroke.value,
            existing_only: !stroke.add,
        };
        self.cells.clear();
        let result = match kind {
            VoxelToolKind::Fill if world_read(cx, face.inward) == 0 => Ok(0),
            VoxelToolKind::Fill => {
                let mut log = Vec::new();
                voxel::fill_region(
                    &mut cx.edit,
                    face.inward,
                    stroke.value,
                    fill_limit,
                    &mut log,
                )
            }
            VoxelToolKind::Pencil | VoxelToolKind::Erase => {
                match stroke.last.filter(|_| planar) {
                    Some(last) => line_cells(last, target, &mut self.cells),
                    None => self.cells.push(target),
                }
                voxel::write_cells(&mut cx.edit, &self.cells, brush, put, &mut stroke.log)
                    .inspect(|_| stroke.log.clear())
            }
            VoxelToolKind::Line | VoxelToolKind::Box => {
                // The rubber band: take the last preview back, draw the new one.
                let rolled = if stroke.log.is_empty() {
                    Ok(())
                } else {
                    voxel::rollback(&mut cx.edit, &mut stroke.log)
                };
                if let Err(e) = rolled {
                    cx.say(e.to_string());
                    return StrokeStep::End;
                }
                let drawn = if kind == VoxelToolKind::Line {
                    line_cells(stroke.anchor, target, &mut self.cells);
                    Ok(())
                } else {
                    box_cells(stroke.anchor, target, active, &mut self.cells)
                };
                drawn.and_then(|()| {
                    voxel::write_cells(&mut cx.edit, &self.cells, brush, put, &mut stroke.log)
                })
            }
            VoxelToolKind::Picker => Ok(0),
        };
        stroke.last = Some(target);
        match result {
            Ok(0) => {}
            Ok(_) => {
                if let Some(before) = stroke.before.take() {
                    let add = stroke.add;
                    cx.push_undo(Self::label(kind, add), before);
                }
            }
            Err(e @ VoxelError::TooLarge(_)) => cx.say(e.to_string()),
            Err(e) => {
                cx.say(e.to_string());
                return StrokeStep::End;
            }
        }
        if kind == VoxelToolKind::Fill {
            StrokeStep::End
        } else {
            StrokeStep::Continue
        }
    }

    /// The colour picker: the voxel under the pointer becomes the colour of the button.
    fn pick(cx: &mut ToolCx<'_>, button: Button, hit: &Hit) {
        let Some(world) = cx.edit.doc.voxels.as_ref() else {
            cx.say(VoxelError::NoWorld.to_string());
            return;
        };
        let Some(face) = hit_cells(hit.point, hit.normal, world.res) else {
            return;
        };
        let value = world.read(face.inward);
        if value == 0 {
            return;
        }
        let color = unpack_opaque(value).map(from_byte);
        match button {
            Button::Left => cx.colors.left = color,
            Button::Right => cx.colors.right = color,
        }
        cx.say(format!("Picked {}", cuboid_num::pack::hex(value)));
    }
}

/// The value of a cell of the voxel world; 0 without a world.
fn world_read(cx: &ToolCx<'_>, cell: I3) -> u32 {
    cx.edit
        .doc
        .voxels
        .as_ref()
        .map_or(0, |world| world.read(cell))
}

/// The packed voxel colour of a tool colour: opaque, or 0 when fully transparent.
fn packed(color: [f32; 4]) -> u32 {
    voxel_tool(color.map(to_byte))
}

impl CustomTool for VoxelTool {
    fn begin(&mut self, cx: &mut ToolCx<'_>, button: Button, hit: &Hit) -> StrokeStep {
        self.stroke = None;
        if self.kind == VoxelToolKind::Picker {
            Self::pick(cx, button, hit);
            return StrokeStep::End;
        }
        let ToolColors { left, right } = *cx.colors;
        let (add, value) = match (self.kind, button) {
            (VoxelToolKind::Erase, _) => (false, 0),
            (VoxelToolKind::Fill, Button::Left) => (false, packed(left)),
            (VoxelToolKind::Fill, Button::Right) => (false, packed(right)),
            (_, Button::Left) => (true, packed(left)),
            (_, Button::Right) => (false, packed(right)),
        };
        cx.effects.colliders.push(ColliderAction::Freeze);
        self.stroke = Some(Stroke {
            add,
            value,
            face: (0, false),
            pin: 0,
            anchor: I3::ZERO,
            last: None,
            log: Vec::new(),
            before: Some(cx.snapshot_of(Categories::VOXELS)),
        });
        self.step(cx, hit, true)
    }

    fn drag(&mut self, cx: &mut ToolCx<'_>, hit: Option<&Hit>) -> StrokeStep {
        match hit {
            Some(hit) => self.step(cx, hit, false),
            None => StrokeStep::Continue,
        }
    }

    fn end(&mut self, cx: &mut ToolCx<'_>) {
        if self.stroke.take().is_some() {
            cx.effects.colliders.push(ColliderAction::Refresh);
        }
        self.cells.clear();
    }
}
