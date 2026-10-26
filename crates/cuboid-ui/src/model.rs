//! What the app tells the UI each frame: a read-only view of the document and the session.
//!
//! The UI never touches `cuboid-core`. The app builds these structs from the core facade (the
//! document revision tells it when a cached [`NodeView`] list or [`PropsView`] is stale) and
//! hands them in through [`UiCx`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use cuboid_actions::ActionId;
use glam::{Vec2, Vec3};

/// Identity of an outliner node. The app maps it to and from its own object ids (for the
/// document model: `ObjId::get`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct NodeId(pub u64);

/// What a node is; decides its icon and which properties apply.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum NodeKind {
    /// A container without geometry.
    Group,
    /// A cuboid or plane element.
    Element,
    /// A mesh or primitive.
    Mesh,
    /// The voxel layer.
    Voxels,
}

/// One row of the outliner. The list is in sibling order: among the nodes with the same parent,
/// earlier means higher in the tree. A parent that is not in the list makes the node a root.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeView {
    /// Identity.
    pub id: NodeId,
    /// Parent group, if any.
    pub parent: Option<NodeId>,
    /// Display name.
    pub name: Arc<str>,
    /// Kind.
    pub kind: NodeKind,
    /// Shown in the viewport.
    pub visible: bool,
    /// Protected from edits and picking.
    pub locked: bool,
    /// A group above the node is hidden, so the node is not shown whatever its own flag says.
    pub hidden_above: bool,
}

/// An RGBA picture to show: texture previews and thumbnails. The UI uploads it to the GPU when
/// `rev` changes and keeps the upload otherwise, so an unchanged picture costs one pointer
/// compare per frame.
#[derive(Clone, Debug)]
pub struct ImageData {
    /// Width and height in pixels.
    pub size: [usize; 2],
    /// `width * height * 4` straight RGBA bytes; a wrong length is ignored.
    pub rgba: Arc<[u8]>,
    /// Changes whenever the pixels change.
    pub rev: u64,
}

/// One of the six faces of an element.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Face {
    /// +Y.
    Up,
    /// -Y.
    Down,
    /// -Z.
    North,
    /// +Z.
    South,
    /// -X.
    West,
    /// +X.
    East,
}

impl Face {
    /// All faces in display order.
    pub const ALL: [Face; 6] = [
        Face::Up,
        Face::Down,
        Face::North,
        Face::South,
        Face::West,
        Face::East,
    ];

    /// Position in [`Face::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Short name for buttons.
    pub fn name(self) -> &'static str {
        match self {
            Face::Up => "Up",
            Face::Down => "Down",
            Face::North => "North",
            Face::South => "South",
            Face::West => "West",
            Face::East => "East",
        }
    }
}

/// State of one face of the primary element.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceView {
    /// The face is drawn.
    pub enabled: bool,
    /// Label of the texture it uses.
    pub texture: Arc<str>,
    /// UV rectangle `[u0, v0, u1, v1]` in texture pixels.
    pub uv: [f32; 4],
    /// Tint index, -1 for none.
    pub tint: i32,
    /// Turn of the UV in degrees: 0, 90, 180 or 270.
    pub rotation: i32,
    /// The neighbouring face the game culls this face against, if any.
    pub cull: Option<Face>,
}

/// The primary selected object, with what the properties panel edits.
#[derive(Clone, Debug, PartialEq)]
pub struct PropsView {
    /// Kind of the primary.
    pub kind: NodeKind,
    /// Its name.
    pub name: Arc<str>,
    /// How many objects are selected in total.
    pub selected: usize,
    /// Position in model units.
    pub position: Vec3,
    /// Size in model units.
    pub size: Vec3,
    /// Rotation in degrees.
    pub rotation: Vec3,
    /// Pivot (rotation origin) in model units.
    pub pivot: Vec3,
    /// Colour, RGBA in 0..=1.
    pub color: [f32; 4],
    /// Shading is on.
    pub shade: bool,
    /// Face UVs follow the element when it moves.
    pub uv_snap: bool,
    /// The six faces for an element, in [`Face::ALL`] order.
    pub faces: Option<[FaceView; 6]>,
    /// The flags of a group.
    pub group: Option<GroupView>,
}

/// What the properties panel shows of a group besides its transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupView {
    /// Shown in the views, with everything below.
    pub visible: bool,
    /// Protected from drags, with everything below.
    pub locked: bool,
    /// Included in exports that know groups.
    pub export: bool,
    /// How many nodes the group holds directly.
    pub children: usize,
}

/// One step of the play order of an animated texture.
///
/// spec: textures T1 5
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct StepView {
    /// The tile shown, counted from the top of the strip.
    pub tile: u32,
    /// Ticks the step lasts when it has a time of its own.
    pub time: Option<u32>,
}

/// How an animated texture plays, for the Textures panel and the Frames section.
///
/// spec: textures T1 5
#[derive(Clone, Debug, PartialEq)]
pub struct FramesView {
    /// Height of a frame in pixels; the width is the width of the picture.
    pub frame_height: u32,
    /// How many frames the strip holds.
    pub tiles: u32,
    /// Ticks a step lasts that has no time of its own; 20 ticks are a second.
    pub frametime: u32,
    /// Each frame blends into the next one.
    pub interpolate: bool,
    /// The steps in play order. A plain loop lists every tile once.
    pub steps: Arc<[StepView]>,
    /// The play order was given as steps, not as the plain loop over the tiles.
    pub explicit: bool,
    /// The step the views show now.
    pub shown: usize,
}

/// An entry of the texture list.
#[derive(Clone, Debug)]
pub struct TextureView {
    /// Label that faces refer to.
    pub label: Arc<str>,
    /// Width and height in pixels; of one frame for an animated texture.
    pub size: [u32; 2],
    /// Pixels for the preview; the frame the views show for an animated texture.
    pub image: Option<ImageData>,
    /// The file the texture was loaded from, when it has one.
    pub source: Option<Arc<str>>,
    /// How the texture plays; `None` for a still one.
    pub animation: Option<FramesView>,
    /// The frame height "Animate" gives a still texture: its width, when its height is a whole
    /// number of them.
    pub animatable: Option<u32>,
}

/// The colour palette and the two tool colours.
#[derive(Clone, Debug, Default)]
pub struct PaletteView {
    /// Name of the palette.
    pub name: Arc<str>,
    /// Swatches, RGBA in 0..=1.
    pub colors: Arc<[[f32; 4]]>,
    /// Colour of the left button.
    pub left: [f32; 4],
    /// Colour of the right button.
    pub right: [f32; 4],
}

/// Undo history, oldest first.
#[derive(Clone, Debug, Default)]
pub struct HistoryView {
    /// Label of each entry.
    pub labels: Arc<[Arc<str>]>,
    /// How many entries are applied; the rest can be redone.
    pub cursor: usize,
}

/// One Minecraft display slot.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplaySlotView {
    /// The key of the game, such as "thirdperson_righthand".
    pub name: Arc<str>,
    /// The slot is exported.
    pub enabled: bool,
    /// The preset the values came from, such as "item/generated"; empty for none.
    pub preset: Arc<str>,
    /// Rotation in degrees.
    pub rotation: Vec3,
    /// Translation in 1/16 units.
    pub translation: Vec3,
    /// Scale factors.
    pub scale: Vec3,
}

/// The display settings of the document.
#[derive(Clone, Debug, Default)]
pub struct DisplayView {
    /// All slots.
    pub slots: Arc<[DisplaySlotView]>,
    /// The slot being edited and previewed.
    pub active: usize,
}

/// The model settings of the document: what the Model tab shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelView {
    /// Corners of the model box, which the voxel grid and the export frame.
    pub model_box: [Vec3; 2],
    /// The model origin, the pivot of an export.
    pub origin: Vec3,
    /// The blueprint position, where a new object starts.
    pub blueprint: Vec3,
    /// Voxels per block edge; `None` while the model has no voxel layer.
    pub voxel_resolution: Option<u32>,
}

/// What an animation does after its end.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LoopKind {
    /// Plays once; afterwards the model is at rest.
    Once,
    /// Starts again from the beginning.
    Loop,
    /// Plays once and stays in the last pose.
    Hold,
}

impl LoopKind {
    /// What the list says of the kind.
    pub fn name(self) -> &'static str {
        match self {
            LoopKind::Once => "plays once",
            LoopKind::Loop => "loops",
            LoopKind::Hold => "holds the end",
        }
    }
}

/// One animation of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationRow {
    /// Identity, echoed by a request.
    pub id: u64,
    /// Display name.
    pub name: Arc<str>,
    /// Length in seconds.
    pub length: f32,
    /// What happens after the end.
    pub loop_kind: LoopKind,
    /// Frames per second of the snapping grid.
    pub fps: u32,
}

/// The animations of the document and the state of the player that plays them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimationsView {
    /// The animations in list order.
    pub rows: Arc<[AnimationRow]>,
    /// The position in `rows` of the animation the player has.
    pub selected: Option<usize>,
    /// The player advances the playhead.
    pub playing: bool,
    /// The pose of the selected animation is shown in the views: playing, paused or scrubbed.
    pub previewing: bool,
    /// The playhead in seconds.
    pub time: f32,
    /// The speed factor of playing.
    pub speed: f32,
    /// The player starts again at the end whatever the animation says.
    pub looping: bool,
}

impl AnimationsView {
    /// The animation the player has.
    pub fn current(&self) -> Option<&AnimationRow> {
        self.rows.get(self.selected?)
    }

    /// The name of the animation whose pose the views show.
    pub fn preview_name(&self) -> Option<&str> {
        if self.previewing {
            self.current().map(|row| &*row.name)
        } else {
            None
        }
    }
}

/// How a keyframe goes on to the next one.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum KeyKind {
    /// Straight line.
    Linear,
    /// A smooth curve through the neighbouring keyframes.
    CatmullRom,
    /// A curve shaped by handles.
    Bezier,
    /// Keeps the value until the next keyframe.
    Step,
}

impl KeyKind {
    /// Every kind in menu order.
    pub const ALL: [KeyKind; 4] = [
        KeyKind::Linear,
        KeyKind::CatmullRom,
        KeyKind::Bezier,
        KeyKind::Step,
    ];

    /// Menu label.
    pub fn name(self) -> &'static str {
        match self {
            KeyKind::Linear => "Linear",
            KeyKind::CatmullRom => "Catmull-Rom",
            KeyKind::Bezier => "Bezier",
            KeyKind::Step => "Step",
        }
    }
}

/// A channel of a bone.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ChannelKind {
    /// Degrees added to the rotation of the bone.
    Rotation,
    /// Model units added to the position.
    Position,
    /// Factor applied to the size about the pivot.
    Scale,
}

impl ChannelKind {
    /// The three channels in row order.
    pub const ALL: [ChannelKind; 3] = [
        ChannelKind::Rotation,
        ChannelKind::Position,
        ChannelKind::Scale,
    ];

    /// Row label.
    pub fn name(self) -> &'static str {
        match self {
            ChannelKind::Rotation => "Rotation",
            ChannelKind::Position => "Position",
            ChannelKind::Scale => "Scale",
        }
    }

    /// Position in [`ChannelKind::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// One component of a keyframe value: a number, or the expression a file wrote.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyNumber {
    /// A number.
    Num(f32),
    /// An expression, as written.
    Expr(Arc<str>),
}

impl KeyNumber {
    /// The text a field shows and takes back unchanged.
    pub fn text(&self) -> String {
        match self {
            KeyNumber::Num(n) => n.to_string(),
            KeyNumber::Expr(text) => text.to_string(),
        }
    }
}

/// A keyframe of the timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyMark {
    /// Seconds from the start.
    pub time: f32,
    /// How the channel goes on from the keyframe.
    pub kind: KeyKind,
    /// The values on X, Y and Z.
    pub value: [KeyNumber; 3],
    /// The channel jumps here: it leaves with other values than it arrives with.
    pub jump: bool,
}

impl KeyMark {
    /// Whether a value is an expression, which playing counts as the last number before it.
    pub fn expression(&self) -> bool {
        self.value.iter().any(|v| matches!(v, KeyNumber::Expr(_)))
    }
}

/// A bone of the timeline with the keyframes of its channels.
#[derive(Clone, Debug, PartialEq)]
pub struct BoneRow {
    /// The group, as the outliner knows it.
    pub id: NodeId,
    /// Display name.
    pub name: Arc<str>,
    /// Levels below the top, for the indent.
    pub depth: usize,
    /// The bone is selected.
    pub selected: bool,
    /// The keyframes of rotation, position and scale, ordered by time.
    pub channels: [Arc<[KeyMark]>; 3],
}

/// What the Animate tab shows of the primary bone at the playhead.
#[derive(Clone, Debug, PartialEq)]
pub struct PoseView {
    /// The bone.
    pub bone: NodeId,
    /// Display name.
    pub name: Arc<str>,
    /// Rotation in degrees, position in model units and scale as the channels say at the
    /// playhead.
    pub values: [Vec3; 3],
    /// A keyframe lies at the playhead, per channel.
    pub keyed: [bool; 3],
}

/// The bones and keyframes of the chosen animation, built by the app when the animations, the
/// groups or the bone selection change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimelineView {
    /// Changes whenever `bones` does; the timeline rebuilds its rows only then.
    pub rev: u64,
    /// The bones that have a track in the animation plus the selected bones, in outliner order.
    pub bones: Arc<[BoneRow]>,
    /// The model has groups at all.
    pub has_groups: bool,
    /// The primary bone at the playhead, when one is selected.
    pub pose: Option<PoseView>,
}

/// A read-only view of the open document.
#[derive(Clone, Debug)]
pub struct DocView<'a> {
    /// Display name such as "tower.cubik"; "Untitled" for a new model.
    pub title: &'a str,
    /// File path when the document has one.
    pub path: Option<&'a Path>,
    /// There are unsaved changes.
    pub dirty: bool,
    /// Outliner nodes.
    pub nodes: &'a [NodeView],
    /// Changes whenever `nodes` changes; the outliner rebuilds its tree only then.
    pub nodes_rev: u64,
    /// Selected nodes.
    pub selection: &'a [NodeId],
    /// The primary selection.
    pub primary: Option<NodeId>,
    /// Properties of the primary.
    pub props: Option<&'a PropsView>,
    /// Texture list.
    pub textures: &'a [TextureView],
    /// Index into `textures` of the active texture.
    pub active_texture: Option<usize>,
    /// The animated textures play.
    pub texture_playing: bool,
    /// Palette and tool colours.
    pub palette: &'a PaletteView,
    /// Undo history.
    pub history: &'a HistoryView,
    /// Display settings, when the document has them.
    pub display: Option<&'a DisplayView>,
    /// The model settings; `None` hides the Model tab's fields.
    pub model: Option<&'a ModelView>,
    /// The animations and the player; `None` shows an empty Animations panel.
    pub animations: Option<&'a AnimationsView>,
    /// The bones and keyframes of the chosen animation for the Animate mode.
    pub timeline: Option<&'a TimelineView>,
    /// Text for the status bar, empty for the key hints.
    pub status: &'a str,
}

/// A recent file for the start screen.
#[derive(Clone, Debug)]
pub struct RecentEntry {
    /// Full path.
    pub path: PathBuf,
    /// File name.
    pub name: String,
    /// Second line, such as the folder or the time of the last change.
    pub detail: String,
    /// Thumbnail, if the app has one.
    pub thumbnail: Option<ImageData>,
    /// When the file was last changed, for "2 days ago"; the start screen shows `detail` while
    /// it is `None`.
    pub modified: Option<SystemTime>,
}

impl From<&cuboid_settings::RecentFile> for RecentEntry {
    fn from(file: &cuboid_settings::RecentFile) -> Self {
        let path = PathBuf::from(&file.path);
        let detail = path
            .parent()
            .map(|dir| dir.display().to_string())
            .unwrap_or_default();
        Self {
            path,
            name: file.name.clone(),
            detail,
            thumbnail: None,
            modified: None,
        }
    }
}

/// A long job the app runs off the UI thread.
#[derive(Clone, Debug)]
pub struct JobView {
    /// Identity, echoed by a cancel request.
    pub id: u64,
    /// What it does, such as "Saving tower.cubik".
    pub label: String,
    /// Progress in 0..=1, or `None` when unknown.
    pub fraction: Option<f32>,
    /// The job can be cancelled.
    pub cancellable: bool,
}

/// One of the four scene views; the numbers are those of `cuboid_scene::ViewId::index`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ViewSlot(pub u8);

impl ViewSlot {
    /// Looking down the Y axis (scene `Top`).
    pub const TOP: ViewSlot = ViewSlot(0);
    /// Looking along X (scene `Side`).
    pub const SIDE: ViewSlot = ViewSlot(1);
    /// Looking along Z (scene `Front`).
    pub const FRONT: ViewSlot = ViewSlot(2);
    /// The free perspective view (scene `Main`).
    pub const MAIN: ViewSlot = ViewSlot(3);
    /// All slots in scene order.
    pub const ALL: [ViewSlot; 4] = [Self::TOP, Self::SIDE, Self::FRONT, Self::MAIN];

    /// Name shown in the view header.
    pub fn name(self) -> &'static str {
        match self.0 {
            0 => "Top",
            1 => "Side",
            2 => "Front",
            _ => "Perspective",
        }
    }
}

/// The rendered picture of one view.
#[derive(Copy, Clone, Debug)]
pub struct ViewportFrame {
    /// The egui texture of the view texture (`egui_wgpu::Renderer::register_native_texture`).
    pub texture: Option<egui::TextureId>,
    /// Pixel size the texture was rendered at.
    pub size_px: [u32; 2],
}

/// A rubber-band rectangle drawn over a view, in view pixels.
#[derive(Copy, Clone, Debug)]
pub struct BandView {
    /// The view it belongs to.
    pub slot: ViewSlot,
    /// Two opposite corners.
    pub corners: [Vec2; 2],
}

/// The state of the window the UI is drawn in. The top bar is the title bar of a window the
/// system does not decorate.
///
/// spec: title bar 4
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WindowView {
    /// The window is maximized.
    pub maximized: bool,
    /// The window is in full screen.
    pub fullscreen: bool,
    /// The system draws the title bar and the border: the top bar has no window buttons.
    pub decorated: bool,
    /// The window has the keyboard focus; an inactive window shows weaker text in the bar.
    pub focused: bool,
}

impl Default for WindowView {
    /// A focused window with the system title bar.
    fn default() -> Self {
        Self {
            maximized: false,
            fullscreen: false,
            decorated: true,
            focused: true,
        }
    }
}

/// Everything the UI needs for one frame.
pub struct UiCx<'a> {
    /// The window the UI is drawn in.
    pub window: WindowView,
    /// The open document, `None` shows the start screen.
    pub doc: Option<DocView<'a>>,
    /// Recent files, newest first.
    pub recent: &'a [RecentEntry],
    /// Running jobs.
    pub jobs: &'a [JobView],
    /// Rendered views by [`ViewSlot`] index.
    pub views: [Option<ViewportFrame>; 4],
    /// Box-select rectangle to draw, if one is being dragged.
    pub band: Option<BandView>,
    /// A plain left press at the pointer would start a tool action: a gizmo handle, or a stroke of
    /// a paint or voxel tool over the model. The Blockbench-style camera orbits otherwise.
    pub tool_hot: bool,
    /// Whether an action can run now (selection guards); `None` enables everything.
    pub enabled: Option<&'a dyn Fn(ActionId) -> bool>,
    /// The library for the start screen and the Library panel; `None` hides it.
    pub library: Option<&'a crate::library::LibraryView>,
    /// The result of the last model check for the Validator panel; `None` shows that nothing is
    /// checked yet.
    pub validator: Option<&'a crate::validator::ValidatorView>,
}

impl<'a> UiCx<'a> {
    /// A context with nothing open: the start screen.
    pub fn empty(recent: &'a [RecentEntry]) -> Self {
        Self {
            window: WindowView::default(),
            doc: None,
            recent,
            jobs: &[],
            views: [None; 4],
            band: None,
            tool_hot: false,
            enabled: None,
            library: None,
            validator: None,
        }
    }

    pub(crate) fn can_run(&self, id: ActionId) -> bool {
        self.enabled.is_none_or(|f| f(id))
    }
}
