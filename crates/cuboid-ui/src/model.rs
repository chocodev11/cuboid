//! What the app tells the UI each frame: a read-only view of the document and the session.
//!
//! The UI never touches `cuboid-core`. The app builds these structs from the core facade (the
//! document revision tells it when a cached [`NodeView`] list or [`PropsView`] is stale) and
//! hands them in through [`UiCx`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

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
}

/// An entry of the texture list.
#[derive(Clone, Debug)]
pub struct TextureView {
    /// Label that faces refer to.
    pub label: Arc<str>,
    /// Width and height in pixels.
    pub size: [u32; 2],
    /// Pixels for the preview.
    pub image: Option<ImageData>,
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
    /// Name such as "thirdperson_righthand".
    pub name: Arc<str>,
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
    /// Palette and tool colours.
    pub palette: &'a PaletteView,
    /// Undo history.
    pub history: &'a HistoryView,
    /// Display settings, when the document has them.
    pub display: Option<&'a DisplayView>,
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

/// Everything the UI needs for one frame.
pub struct UiCx<'a> {
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
    /// Whether an action can run now (selection guards); `None` enables everything.
    pub enabled: Option<&'a dyn Fn(ActionId) -> bool>,
}

impl<'a> UiCx<'a> {
    /// A context with nothing open: the start screen.
    pub fn empty(recent: &'a [RecentEntry]) -> Self {
        Self {
            doc: None,
            recent,
            jobs: &[],
            views: [None; 4],
            band: None,
            enabled: None,
        }
    }

    pub(crate) fn can_run(&self, id: ActionId) -> bool {
        self.enabled.is_none_or(|f| f(id))
    }
}
