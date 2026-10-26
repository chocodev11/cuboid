//! What the UI asks the app to do: plain data, applied after the frame.

use std::path::PathBuf;

use cuboid_actions::ActionId;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::model::{Face, NodeId, ViewSlot};
use crate::prefs::Prefs;

/// The four workspaces behind the tabs of the menu bar.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Mode {
    /// Cuboids, meshes and groups.
    #[default]
    Model,
    /// Voxel sculpting.
    Voxel,
    /// Texture painting.
    Paint,
    /// Minecraft display settings.
    Display,
}

impl Mode {
    /// All modes in tab order.
    pub const ALL: [Mode; 4] = [Mode::Model, Mode::Voxel, Mode::Paint, Mode::Display];

    /// Tab label.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Model => "Model",
            Mode::Voxel => "Voxel",
            Mode::Paint => "Paint",
            Mode::Display => "Display",
        }
    }

    /// The tools of the mode's toolbar, first is the default.
    pub fn tools(self) -> &'static [Tool] {
        match self {
            Mode::Model => &[Tool::Select, Tool::Move, Tool::Rotate, Tool::Scale],
            Mode::Voxel => &[
                Tool::VoxelPencil,
                Tool::VoxelErase,
                Tool::VoxelFill,
                Tool::VoxelBox,
                Tool::VoxelLine,
                Tool::VoxelPicker,
            ],
            Mode::Paint => &[
                Tool::PaintPencil,
                Tool::PaintEraser,
                Tool::PaintFill,
                Tool::PaintPicker,
            ],
            Mode::Display => &[],
        }
    }

    /// The next mode in tab order, wrapping around.
    pub fn next(self) -> Mode {
        let i = Mode::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Mode::ALL[(i + 1) % Mode::ALL.len()]
    }
}

/// The active tool; each belongs to one [`Mode`].
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Tool {
    /// Click and box selection.
    #[default]
    Select,
    /// Move gizmo.
    Move,
    /// Rotate gizmo.
    Rotate,
    /// Scale gizmo.
    Scale,
    /// Place voxels.
    VoxelPencil,
    /// Remove voxels.
    VoxelErase,
    /// Flood fill voxels.
    VoxelFill,
    /// Fill a box of voxels.
    VoxelBox,
    /// Draw a line of voxels.
    VoxelLine,
    /// Pick a voxel colour.
    VoxelPicker,
    /// Paint texels.
    PaintPencil,
    /// Clear texels.
    PaintEraser,
    /// Flood fill texels.
    PaintFill,
    /// Pick a texel colour.
    PaintPicker,
}

impl Tool {
    /// Tooltip and palette label.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Move => "Move",
            Tool::Rotate => "Rotate",
            Tool::Scale => "Scale",
            Tool::VoxelPencil => "Voxel pencil",
            Tool::VoxelErase => "Voxel eraser",
            Tool::VoxelFill => "Voxel fill",
            Tool::VoxelBox => "Voxel box",
            Tool::VoxelLine => "Voxel line",
            Tool::VoxelPicker => "Voxel colour picker",
            Tool::PaintPencil => "Pencil",
            Tool::PaintEraser => "Eraser",
            Tool::PaintFill => "Fill",
            Tool::PaintPicker => "Colour picker",
        }
    }

    /// The mode whose toolbar holds the tool.
    pub fn mode(self) -> Mode {
        match self {
            Tool::Select | Tool::Move | Tool::Rotate | Tool::Scale => Mode::Model,
            Tool::VoxelPencil
            | Tool::VoxelErase
            | Tool::VoxelFill
            | Tool::VoxelBox
            | Tool::VoxelLine
            | Tool::VoxelPicker => Mode::Voxel,
            Tool::PaintPencil | Tool::PaintEraser | Tool::PaintFill | Tool::PaintPicker => {
                Mode::Paint
            }
        }
    }
}

/// Options of the voxel and paint tools.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolOptions {
    /// Brush edge in voxels or texels.
    pub brush: u32,
    /// Mirror across the X, Y and Z planes through the model centre.
    pub mirror: [bool; 3],
}

impl Default for ToolOptions {
    fn default() -> Self {
        Self {
            brush: 1,
            mirror: [false; 3],
        }
    }
}

/// A starting point for a new model.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Template {
    /// A block model: one full cube.
    Block,
    /// A flat item model.
    Item,
    /// A block with the Cubik preset: Cubik's rules and lighting.
    CubikClassic,
    /// A voxel model with this many voxels per block edge.
    Voxel {
        /// Resolution, 2 to 128.
        resolution: u32,
    },
    /// A model with nothing in it.
    Empty,
}

/// How a selection request combines with the current selection.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SelectOp {
    /// Replace the selection.
    Replace,
    /// Add to the selection.
    Add,
    /// Toggle membership.
    Toggle,
}

/// A transform field of the primary selection.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TransformField {
    /// Position.
    Position,
    /// Size.
    Size,
    /// Rotation in degrees.
    Rotation,
    /// Pivot.
    Pivot,
}

/// A boolean flag of an element.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ElementFlag {
    /// Directional shading.
    Shade,
    /// UV follow the element.
    UvSnap,
}

/// A change to one face of the primary element.
#[derive(Clone, Debug, PartialEq)]
pub enum FaceChange {
    /// Show or hide the face.
    Enabled(bool),
    /// Use another texture.
    Texture(String),
    /// New UV rectangle `[u0, v0, u1, v1]`; `commit` is false while dragging.
    Uv {
        /// The rectangle.
        uv: [f32; 4],
        /// The edit ended.
        commit: bool,
    },
    /// New tint index.
    Tint(i32),
    /// Derive the UV from the face size and position.
    AutoUv,
    /// Rotate the UV a quarter turn.
    RotateUv,
    /// Mirror the UV left to right.
    FlipU,
    /// Mirror the UV top to bottom.
    FlipV,
    /// Turn the UV to this angle: 0, 90, 180 or 270 degrees.
    Rotation(i32),
    /// Cull the face against this neighbour, or not at all.
    Cull(Option<Face>),
}

/// A built-in parent model whose display settings can be loaded into a slot.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum DisplayPreset {
    /// `item/generated`: a flat item.
    ItemGenerated,
    /// `item/handheld`: a tool held in the hand.
    ItemHandheld,
    /// `block/block`: a block.
    BlockBlock,
}

impl DisplayPreset {
    /// All presets in menu order.
    pub const ALL: [DisplayPreset; 3] = [
        DisplayPreset::ItemGenerated,
        DisplayPreset::ItemHandheld,
        DisplayPreset::BlockBlock,
    ];

    /// The id of the parent model, as a model file names it.
    pub fn id(self) -> &'static str {
        match self {
            DisplayPreset::ItemGenerated => "item/generated",
            DisplayPreset::ItemHandheld => "item/handheld",
            DisplayPreset::BlockBlock => "block/block",
        }
    }
}

/// Which tool colour.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ColorSlot {
    /// Left mouse button.
    Left,
    /// Right mouse button.
    Right,
}

/// A direct manipulation of the document from a panel. Edits with a `commit` flag arrive as
/// previews (`commit == false`) while a drag is running and once more with `true` when it ends;
/// an app without live preview applies only the committed one, so one drag is one undo step.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Change the selection.
    Select {
        /// Nodes to select (empty with `Replace` clears).
        ids: Vec<NodeId>,
        /// How to combine.
        op: SelectOp,
    },
    /// Rename a node.
    Rename {
        /// The node.
        id: NodeId,
        /// New name.
        name: String,
    },
    /// Show or hide a node.
    SetVisible {
        /// The node.
        id: NodeId,
        /// New state.
        visible: bool,
    },
    /// Lock or unlock a node.
    SetLocked {
        /// The node.
        id: NodeId,
        /// New state.
        locked: bool,
    },
    /// Move nodes under a new parent, before a sibling or at the end.
    Reparent {
        /// Nodes to move, in outliner order.
        ids: Vec<NodeId>,
        /// New parent, `None` for the root.
        parent: Option<NodeId>,
        /// Insert before this sibling; `None` appends.
        before: Option<NodeId>,
    },
    /// Put the selection into a new group.
    GroupSelection,
    /// Dissolve the selected groups.
    Ungroup,
    /// Select what a group holds directly.
    SelectChildren {
        /// The group.
        id: NodeId,
    },
    /// Include a group in the exports that know groups, or leave it out.
    SetExport {
        /// The group.
        id: NodeId,
        /// New state.
        export: bool,
    },
    /// Set a transform field of the primary selection.
    Transform {
        /// Which field.
        field: TransformField,
        /// The new value of the whole vector.
        value: Vec3,
        /// The edit ended.
        commit: bool,
    },
    /// Set the element colour.
    SetColor {
        /// RGBA in 0..=1.
        color: [f32; 4],
        /// The edit ended.
        commit: bool,
    },
    /// Set an element flag.
    SetFlag {
        /// Which flag.
        flag: ElementFlag,
        /// New value.
        on: bool,
    },
    /// Change a face of the primary element.
    Face {
        /// Which face.
        face: Face,
        /// What to change.
        change: FaceChange,
    },
    /// Make a texture the active one.
    SelectTexture(usize),
    /// Read a texture again from the file it came from.
    ReloadTexture(usize),
    /// Set a tool colour.
    SetToolColor {
        /// Which button.
        slot: ColorSlot,
        /// RGBA in 0..=1.
        color: [f32; 4],
    },
    /// Use a palette swatch as a tool colour.
    PickPalette {
        /// Swatch index.
        index: usize,
        /// Which button.
        slot: ColorSlot,
    },
    /// Append a colour to the palette.
    AddPaletteColor([f32; 4]),
    /// Remove a palette swatch.
    RemovePaletteColor(usize),
    /// Choose the display slot to edit.
    SelectDisplaySlot(usize),
    /// Set a display slot.
    SetDisplaySlot {
        /// Slot index.
        slot: usize,
        /// Rotation in degrees.
        rotation: Vec3,
        /// Translation in 1/16 units.
        translation: Vec3,
        /// Scale factors.
        scale: Vec3,
        /// The edit ended.
        commit: bool,
    },
    /// Export a display slot, or leave it out.
    SetDisplayEnabled {
        /// Slot index.
        slot: usize,
        /// New state.
        enabled: bool,
    },
    /// Fill a display slot with the values of a built-in parent model.
    LoadDisplayPreset {
        /// Slot index.
        slot: usize,
        /// The parent.
        preset: DisplayPreset,
    },
    /// Move the model origin.
    SetModelOrigin {
        /// The new position.
        value: Vec3,
        /// The edit ended.
        commit: bool,
    },
    /// Move the blueprint.
    SetBlueprint {
        /// The new position.
        value: Vec3,
        /// The edit ended.
        commit: bool,
    },
}

/// Modifier keys held during a pointer event.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct KeyMods {
    /// Ctrl, or Command on macOS.
    pub ctrl: bool,
    /// Shift.
    pub shift: bool,
    /// Alt.
    pub alt: bool,
}

/// A mouse button.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Button {
    /// Left.
    Primary,
    /// Right.
    Secondary,
    /// Wheel button.
    Middle,
}

/// A named camera direction.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ViewPreset {
    /// Look at the front (along -Z).
    Front,
    /// Look at the back.
    Back,
    /// Look from the right (along -X).
    Right,
    /// Look from the left.
    Left,
    /// Look down.
    Top,
    /// Look up.
    Bottom,
    /// Toggle between orthographic and perspective projection.
    ToggleProjection,
}

/// Pointer input over a view. Positions are in the view's pixels with the origin at its top left;
/// deltas are in the same pixels.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ViewportEvent {
    /// The pointer moved over a view, or left it (`pos` is `None`).
    Hover {
        /// The view.
        slot: ViewSlot,
        /// Position.
        pos: Option<Vec2>,
    },
    /// Orbit the camera.
    Orbit {
        /// The view.
        slot: ViewSlot,
        /// Pointer movement.
        delta: Vec2,
    },
    /// Pan the camera.
    Pan {
        /// The view.
        slot: ViewSlot,
        /// Pointer movement.
        delta: Vec2,
    },
    /// Zoom by wheel notches (positive zooms in) or by dragging.
    Zoom {
        /// The view.
        slot: ViewSlot,
        /// Notches, 1 per wheel click.
        amount: f32,
        /// Pointer position, for zooming towards it.
        pos: Vec2,
    },
    /// A button went down over a view and was not used for navigation.
    Press {
        /// The view.
        slot: ViewSlot,
        /// Which button.
        button: Button,
        /// Position.
        pos: Vec2,
        /// Modifiers.
        mods: KeyMods,
    },
    /// The pointer moved with a button held since a [`ViewportEvent::Press`].
    Drag {
        /// The view the press started in.
        slot: ViewSlot,
        /// Which button.
        button: Button,
        /// Position, also outside the view.
        pos: Vec2,
        /// Movement since the last event.
        delta: Vec2,
        /// Modifiers.
        mods: KeyMods,
    },
    /// The button of a press went up.
    Release {
        /// The view the press started in.
        slot: ViewSlot,
        /// Which button.
        button: Button,
        /// Position.
        pos: Vec2,
        /// Modifiers.
        mods: KeyMods,
    },
    /// Snap a view to a named direction.
    SetView {
        /// The view.
        slot: ViewSlot,
        /// Direction.
        preset: ViewPreset,
    },
    /// Frame the selection (or the whole model when nothing is selected).
    Frame {
        /// The view.
        slot: ViewSlot,
    },
    /// The view under focus changed.
    Activate {
        /// The view.
        slot: ViewSlot,
    },
}

/// The answer of the recovery dialog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryChoice {
    /// Open this recovery file.
    Restore(PathBuf),
    /// Delete this recovery file.
    Discard(PathBuf),
    /// Delete all recovery files.
    DiscardAll,
    /// Ask again at the next start.
    Later,
}

/// What the Animations panel asks of the player. None of it changes the document.
///
/// spec: animation A2 6
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum AnimationRequest {
    /// Choose the animation with this id; the player stops at its start.
    Select(u64),
    /// Play the chosen animation from the playhead.
    Play,
    /// Stop the playhead; the pose stays.
    Pause,
    /// Stop, put the playhead at the start and show the rest pose again.
    Stop,
    /// Put the playhead at this time in seconds and show the pose there.
    Seek(f32),
    /// Play at this multiple of the real speed.
    SetSpeed(f32),
    /// Start again at the end, or do not.
    SetLoop(bool),
}

/// A request for the app, in the order the user made them.
#[derive(Clone, Debug, PartialEq)]
pub enum UiRequest {
    /// Run a registry action (everything that has an [`ActionId`] and no more specific variant).
    Action(ActionId),
    /// Create a model from a template.
    NewDocument(Template),
    /// Show the file picker and open the chosen file.
    OpenDialog,
    /// Open this file (recent list, drop, recovery).
    Open(PathBuf),
    /// Save to the current path, or ask for one when there is none.
    Save,
    /// Save under a new name.
    SaveAs,
    /// Show the export dialog.
    ExportDialog,
    /// Run the request after saving succeeded (the answer "Save" of the unsaved-changes dialog).
    SaveThen(Box<UiRequest>),
    /// Leave the application; sent after the unsaved-changes dialog was answered.
    Quit,
    /// Remove a file from the recent list.
    RemoveRecent(PathBuf),
    /// A direct manipulation of the document.
    Edit(Edit),
    /// Pointer input over a view.
    Viewport(ViewportEvent),
    /// The mode tab changed.
    SetMode(Mode),
    /// The tool changed.
    SetTool(Tool),
    /// The tool options changed.
    SetToolOptions(ToolOptions),
    /// Go to this undo position: the number of applied history entries.
    HistoryJump(usize),
    /// A preference changed; persist it and apply what the app owns (snapping, autosave).
    PrefsChanged(Box<Prefs>),
    /// The recovery dialog was answered.
    Recovery(RecoveryChoice),
    /// Cancel a job.
    CancelJob(u64),
    /// The library panel or the start screen asks for something.
    Library(crate::library::LibraryRequest),
    /// The Validator panel asks for a fix or a new check.
    Validator(crate::validator::ValidatorRequest),
    /// The export dialog asks for a check or a write.
    Export(crate::export_dialog::ExportRequest),
    /// The render dialog asks for a picture.
    Render(crate::render_dialog::RenderRequest),
    /// Ask for a folder with the system dialog; the answer comes back with `Ui::set_folder`.
    BrowseFolder,
    /// Show the folder of this file (or this folder) in the system file manager.
    OpenFolder(PathBuf),
    /// A project setting changed: preset, game version, an override or the shading.
    Project(crate::project_settings::ProjectChange),
    /// The Animations panel drives the player.
    Animation(AnimationRequest),
}

/// Where a view was drawn this frame; the app sizes the view texture from it and maps pointer
/// positions through it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ViewportPlacement {
    /// The view.
    pub slot: ViewSlot,
    /// Left, top, width and height in egui points.
    pub rect: [f32; 4],
    /// Physical pixels per point.
    pub pixels_per_point: f32,
    /// The size to render at, in physical pixels (at least 1 x 1).
    pub size_px: [u32; 2],
}

/// The result of one frame.
#[derive(Clone, Debug, Default)]
pub struct UiOutput {
    /// Requests in order; the app applies them after the frame.
    pub requests: Vec<UiRequest>,
    /// Views drawn this frame.
    pub viewports: Vec<ViewportPlacement>,
    /// A text field or dialog holds the keyboard: the app must not run its own shortcuts.
    pub keyboard_captured: bool,
    /// The pointer is over UI rather than over a view.
    pub pointer_over_ui: bool,
    /// New window title, set only when it changed.
    pub title: Option<String>,
}
