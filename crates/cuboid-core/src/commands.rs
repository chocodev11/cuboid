//! `Command`: every undoable edit and selection change as plain, serialisable data, with its
//! undo label, categories, guard and outcome types (spec: arch 10.1, tools 15).
//!
//! All vectors are in model (internal) space, the space of the stored fields; a UI that shows
//! Minecraft space negates Z (`ui = (x, y, -z)`, tools 1.2). Angles are degrees, sizes and
//! positions are in units (16 units = 1 block).
//!
//! A command that names objects carries `targets`. They are positions in the element or mesh
//! list at the time the command runs; an empty list means "the current selection". Objects the
//! command cannot act on (the blueprint of a delete, a mesh of a mirror) are skipped.

use std::borrow::Cow;

use bitflags::bitflags;
use cuboid_actions::ActionId;
use cuboid_model::{
    AnimationError, Categories, Channel, DisplaySlotId, ElementType, FaceName, GroupError, Interp,
    LoopMode, MAX_EXPRESSION_BYTES, MAX_LENGTH, MeshKind, UserMessage, Value,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A world axis.
///
/// spec: tools 10.12
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Axis {
    /// X.
    X,
    /// Y.
    Y,
    /// Z.
    Z,
}

impl From<Axis> for cuboid_ops::transform::Axis {
    fn from(axis: Axis) -> Self {
        match axis {
            Axis::X => Self::X,
            Axis::Y => Self::Y,
            Axis::Z => Self::Z,
        }
    }
}

/// An object of a command, by position in its list.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Target {
    /// The element at this position of the element list.
    Element(usize),
    /// The mesh at this position of the mesh list.
    Mesh(usize),
    /// The group at this position of the group list.
    Group(usize),
    /// The placement box.
    Blueprint,
    /// The model-origin marker.
    ModelOrigin,
}

/// A channel of a bone.
///
/// spec: animation A2 4
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BoneChannel {
    /// Degrees added to the rotation of the bone.
    Rotation,
    /// Model units added to the position of the bone.
    Position,
    /// Factor applied to the size of the bone about its pivot.
    Scale,
}

impl From<BoneChannel> for Channel {
    fn from(channel: BoneChannel) -> Self {
        match channel {
            BoneChannel::Rotation => Channel::Rotation,
            BoneChannel::Position => Channel::Position,
            BoneChannel::Scale => Channel::Scale,
        }
    }
}

impl From<Channel> for BoneChannel {
    fn from(channel: Channel) -> Self {
        match channel {
            Channel::Rotation => BoneChannel::Rotation,
            Channel::Position => BoneChannel::Position,
            Channel::Scale => BoneChannel::Scale,
        }
    }
}

/// How a channel goes from a keyframe to the next one.
///
/// spec: animation A2 2
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum KeyInterp {
    /// Straight line.
    Linear,
    /// A smooth curve through the neighbouring keyframes.
    CatmullRom,
    /// A curve shaped by the handles of the keyframes.
    Bezier,
    /// Keeps the value until the next keyframe.
    Step,
}

impl From<KeyInterp> for Interp {
    fn from(interp: KeyInterp) -> Self {
        match interp {
            KeyInterp::Linear => Interp::Linear,
            KeyInterp::CatmullRom => Interp::CatmullRom,
            KeyInterp::Bezier => Interp::Bezier,
            KeyInterp::Step => Interp::Step,
        }
    }
}

/// What an animation does after its end.
///
/// spec: animation A2 2
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum AnimationLoop {
    /// Plays once; afterwards the model is at rest.
    Once,
    /// Starts again from the beginning.
    Loop,
    /// Plays once and stays in the last pose.
    Hold,
}

impl From<AnimationLoop> for LoopMode {
    fn from(mode: AnimationLoop) -> Self {
        match mode {
            AnimationLoop::Once => LoopMode::Once,
            AnimationLoop::Loop => LoopMode::Loop,
            AnimationLoop::Hold => LoopMode::Hold,
        }
    }
}

/// One component of a keyframe value in a command: a number, or text. Text that is a number is
/// that number; any other text is kept as the expression it is (Molang) and played as the last
/// number before it. In JSON a number is a number and text is a string, so `[1, 2, 3]` reads.
///
/// spec: animate A3 7
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeyValue {
    /// A number.
    Num(f32),
    /// Text: a number written out, or an expression.
    Text(String),
}

impl KeyValue {
    /// Three plain numbers.
    pub fn nums(values: [f32; 3]) -> [KeyValue; 3] {
        values.map(KeyValue::Num)
    }

    /// The value the model keeps.
    pub fn value(&self) -> Value {
        match self {
            KeyValue::Num(n) => Value::Num(*n),
            KeyValue::Text(text) => match text.trim().parse::<f32>() {
                Ok(n) if n.is_finite() => Value::Num(n),
                _ => Value::from_text(text),
            },
        }
    }

    fn check(&self) -> Result<(), Refusal> {
        match self {
            KeyValue::Num(n) if !n.is_finite() => Err(Refusal::NotFinite),
            KeyValue::Text(text) if text.trim().is_empty() || text.len() > MAX_EXPRESSION_BYTES => {
                Err(Refusal::OutOfRange)
            }
            _ => Ok(()),
        }
    }
}

/// A keyframe by its place when a command runs: the group, the channel and the position among
/// the keyframes of the channel, which are ordered by time.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyAt {
    /// The animated group.
    pub group: Target,
    /// The channel.
    pub channel: BoneChannel,
    /// The position among the keyframes of the channel.
    pub index: usize,
}

/// How a selection command combines its targets with the selection.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum SelectOp {
    /// The targets become the selection.
    Replace,
    /// The targets are added.
    Add,
    /// Each target is added when absent and removed when present.
    Toggle,
    /// The targets are removed.
    Remove,
}

/// The element kinds a command can create.
///
/// spec: tools 4.2
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ElementKind {
    /// A box with six faces.
    Cube,
    /// A plane facing along X.
    PlaneX,
    /// A plane facing along Y.
    PlaneY,
    /// A plane facing along Z.
    PlaneZ,
}

/// The mesh kind of a command shape.
impl From<MeshShape> for MeshKind {
    fn from(shape: MeshShape) -> Self {
        match shape {
            MeshShape::Cube => Self::Cube,
            MeshShape::Sphere => Self::Sphere,
            MeshShape::Cylinder => Self::Cylinder,
            MeshShape::Cone => Self::Cone,
            MeshShape::Pyramid => Self::Pyramid,
            MeshShape::Slope => Self::Slope,
            MeshShape::CornerIn => Self::CornerIn,
            MeshShape::CornerOut => Self::CornerOut,
            MeshShape::Torus => Self::Torus,
        }
    }
}

impl From<ElementKind> for ElementType {
    fn from(kind: ElementKind) -> Self {
        match kind {
            ElementKind::Cube => Self::Cube,
            ElementKind::PlaneX => Self::PlaneX,
            ElementKind::PlaneY => Self::PlaneY,
            ElementKind::PlaneZ => Self::PlaneZ,
        }
    }
}

/// The six faces of an element.
///
/// spec: tools 4.3
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum FaceId {
    /// -Y.
    Down,
    /// +Y.
    Up,
    /// +Z internal.
    North,
    /// -Z internal.
    South,
    /// -X.
    West,
    /// +X.
    East,
}

impl From<FaceId> for FaceName {
    fn from(face: FaceId) -> Self {
        match face {
            FaceId::Down => Self::Down,
            FaceId::Up => Self::Up,
            FaceId::North => Self::North,
            FaceId::South => Self::South,
            FaceId::West => Self::West,
            FaceId::East => Self::East,
        }
    }
}

/// Which edge of the first target the other targets line up with.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum AlignEdge {
    /// The low side of the axis.
    Min,
    /// The middle.
    Center,
    /// The high side.
    Max,
}

impl From<AlignEdge> for cuboid_ops::transform::AlignEdge {
    fn from(edge: AlignEdge) -> Self {
        match edge {
            AlignEdge::Min => Self::Min,
            AlignEdge::Center => Self::Center,
            AlignEdge::Max => Self::Max,
        }
    }
}

/// How an automatic UV mapping lays the faces out.
///
/// spec: tools 4.7, tools 10.17
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum AutoUvStyle {
    /// Box style: every face takes the part of the texture its place and size in the block cover.
    #[default]
    Box,
    /// Per face: every face covers its whole texture.
    PerFace,
}

impl From<AutoUvStyle> for cuboid_ops::uv::AutoUvStyle {
    fn from(style: AutoUvStyle) -> Self {
        match style {
            AutoUvStyle::Box => Self::Box,
            AutoUvStyle::PerFace => Self::PerFace,
        }
    }
}

/// The axis of a UV flip.
///
/// spec: tools 10.17
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum UvAxis {
    /// Left to right: U.
    Horizontal,
    /// Top to bottom: V.
    Vertical,
}

impl From<UvAxis> for cuboid_ops::uv::FlipAxis {
    fn from(axis: UvAxis) -> Self {
        match axis {
            UvAxis::Horizontal => Self::Horizontal,
            UvAxis::Vertical => Self::Vertical,
        }
    }
}

/// The eight Minecraft display contexts, in the order of the file.
///
/// spec: native 4.8, app 9.4
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum DisplayContext {
    /// `thirdperson_righthand`.
    ThirdRight,
    /// `thirdperson_lefthand`.
    ThirdLeft,
    /// `firstperson_righthand`.
    FirstRight,
    /// `firstperson_lefthand`.
    FirstLeft,
    /// `gui`.
    Gui,
    /// `head`.
    Head,
    /// `ground`.
    Ground,
    /// `fixed`, the item frame.
    Fixed,
}

impl From<DisplayContext> for DisplaySlotId {
    fn from(context: DisplayContext) -> Self {
        match context {
            DisplayContext::ThirdRight => Self::ThirdRight,
            DisplayContext::ThirdLeft => Self::ThirdLeft,
            DisplayContext::FirstRight => Self::FirstRight,
            DisplayContext::FirstLeft => Self::FirstLeft,
            DisplayContext::Gui => Self::Gui,
            DisplayContext::Head => Self::Head,
            DisplayContext::Ground => Self::Ground,
            DisplayContext::Fixed => Self::Fixed,
        }
    }
}

/// The primitive mesh shapes a command can create (no menu command creates a sprite).
///
/// spec: tools 6.1
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum MeshShape {
    /// Cube.
    Cube,
    /// Sphere.
    Sphere,
    /// Cylinder.
    Cylinder,
    /// Cone.
    Cone,
    /// Pyramid.
    Pyramid,
    /// Slope.
    Slope,
    /// Inner corner.
    CornerIn,
    /// Outer corner.
    CornerOut,
    /// Torus.
    Torus,
}

/// An edit of the document or of the selection. `Core::execute` runs one command as one undo
/// step (selection changes, copy and undo itself leave no step). Commands derive serde, so a
/// log of them can be stored, replayed or produced by a script.
///
/// spec: arch 10.1, tools 15, arch 6 (cuboid-core)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    // History.
    /// Steps back one undo entry.
    Undo,
    /// Steps forward one undone entry.
    Redo,

    // Selection; never an undo step.
    /// Changes the selection by the targets. The first target becomes the primary of a
    /// replace.
    Select {
        /// The objects.
        targets: Vec<Target>,
        /// How they combine with the selection.
        op: SelectOp,
    },
    /// Selects every visible element and mesh.
    SelectAll,
    /// Clears the selection.
    SelectNone,
    /// Selects the visible elements and meshes that are not selected.
    SelectInverse,

    // Clipboard.
    /// Copies the selected elements and meshes into the clipboard; no undo step.
    Copy,
    /// Copies, then deletes. Undo "Cut".
    Cut,
    /// Pastes the clipboard as new objects and selects them. Undo "Paste".
    Paste {
        /// Place the pasted objects by the blueprint instead of where they were copied.
        #[serde(default)]
        at_blueprint: bool,
    },
    /// Copies the objects in place and selects the copies. Undo "Duplicate".
    Duplicate {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
    },

    // Create and remove.
    /// Creates an element at the blueprint and selects it. Undo "Create Cube element" and its
    /// siblings.
    AddElement(ElementKind),
    /// Removes the elements and meshes. Undo "Delete".
    Delete {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
    },

    // Organise.
    /// Gives every object the same name. Undo "Rename to NAME".
    Rename {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The new label.
        name: String,
    },
    /// Hides or shows elements and meshes. Undo "Hide" or "Show".
    SetHidden {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The new state.
        hidden: bool,
    },
    /// Locks or unlocks elements and meshes. Undo "Lock" or "Unlock".
    SetLocked {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The new state.
        locked: bool,
    },
    /// Unhides every element and mesh. Undo "Show all".
    ShowAll,
    /// Unlocks every element and mesh. Undo "Unlock all".
    UnlockAll,

    // Transform.
    /// Moves the objects by an offset. With `snap` the new pivot is rounded to that grid.
    /// Undo "Move".
    MoveBy {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The offset in units.
        delta: [f32; 3],
        /// Grid step in units.
        #[serde(default)]
        snap: Option<f32>,
    },
    /// Sets the position (box corner) component-wise; `null` keeps a component. Undo "Set
    /// position".
    SetPosition {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The new components.
        value: [Option<f32>; 3],
    },
    /// Sets the size component-wise through the size setter. Undo "Set size".
    SetSize {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The new components.
        value: [Option<f32>; 3],
    },
    /// Scales the objects about the pivot of the first one. With `snap` the new sizes are
    /// rounded to that grid. Undo "Scale".
    ScaleBy {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The factors; all above zero.
        factor: [f32; 3],
        /// Grid step in units.
        #[serde(default)]
        snap: Option<f32>,
    },
    /// Adds degrees to the rotation, about each pivot. With `snap` the delta is first rounded
    /// to that many degrees. Undo "Rotate".
    RotateBy {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The change in degrees.
        delta: [f32; 3],
        /// Angle step in degrees.
        #[serde(default)]
        snap: Option<f32>,
    },
    /// Sets the rotation component-wise in degrees; `null` keeps a component. Undo "Set
    /// rotation".
    SetRotation {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The new components.
        value: [Option<f32>; 3],
    },
    /// Sets the pivot component-wise; `null` keeps a component. The geometry stays where it is:
    /// only what later rotations turn about changes. Elements and meshes. Undo "Set pivot".
    SetPivot {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The new components.
        value: [Option<f32>; 3],
    },
    /// Turns elements a quarter turn about a world axis of the block centre. Undo "Rotate
    /// around X axis" and its siblings.
    QuarterTurn {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The axis.
        axis: Axis,
    },
    /// Sets the rotation to zero about the pivot and the pivot of elements to their position.
    /// Undo "Reset rotation".
    ResetRotation {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
    },
    /// Moves the pivot to the centre of the box without moving the geometry. Undo "Reset
    /// pivot".
    ResetPivot {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
    },
    /// Mirrors elements across the centre plane of their bounds. Undo "Mirror on X axis" and
    /// its siblings.
    Mirror {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The axis the plane is perpendicular to.
        axis: Axis,
        /// Mirror copies and select them, leaving the originals alone.
        #[serde(default)]
        copy: bool,
    },
    /// Lines the targets up with the first one. Undo "Align X" and its siblings.
    Align {
        /// The objects, the first being the reference; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The axis.
        axis: Axis,
        /// The edge.
        edge: AlignEdge,
    },

    // Textures.
    /// Appends a texture filled with one colour. Undo "Add new texture".
    CreateTexture {
        /// The label; empty picks the first free default name.
        #[serde(default)]
        label: Option<String>,
        /// Width in texels.
        width: u32,
        /// Height in texels.
        height: u32,
        /// RGBA colour of every texel.
        color: [u8; 4],
    },
    /// Appends a texture made of these pixels, such as one dropped from the library. The label
    /// goes through the label rule and gets a number when the list has it already. Undo "Add
    /// texture".
    AddTexture {
        /// The wanted label; empty picks `texture`.
        label: String,
        /// Width in texels.
        width: u32,
        /// Height in texels.
        height: u32,
        /// `width * height * 4` bytes, R, G, B, A.
        rgba: Vec<u8>,
    },
    /// Removes a texture from the list; faces that use it keep the label. Undo "Delete
    /// texture".
    RemoveTexture {
        /// The label.
        label: String,
    },
    /// Points faces of elements at a texture. Undo "Apply texture".
    AssignTexture {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The texture label.
        label: String,
        /// The faces; empty is all six.
        #[serde(default)]
        faces: Vec<FaceId>,
    },
    /// Replaces the pixels of a texture, such as when it is read again from its file; the label
    /// stays, so the faces that use it keep it. Undo "Replace texture".
    ReplaceTexture {
        /// The label of the texture.
        label: String,
        /// Width in texels.
        width: u32,
        /// Height in texels.
        height: u32,
        /// `width * height * 4` bytes, R, G, B, A.
        rgba: Vec<u8>,
        /// The signature and the length of the file the pixels come from, which the texture
        /// records; `None` keeps what it has.
        #[serde(default)]
        file: Option<(String, u64)>,
    },

    // Element properties. The faces are those of the elements among the objects; empty `faces` is
    // all six.
    /// Gives elements a colour; the alpha stays opaque and each channel is at least 0.25. Undo
    /// "Set color".
    SetColor {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// RGB in 0 to 1.
        color: [f32; 3],
    },
    /// Turns the directional shading of elements on or off. Undo "Enable shade" or "Disable
    /// shade".
    SetShade {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The new state.
        enabled: bool,
    },
    /// Switches faces on or off. Undo "Enable Up face" and its siblings.
    SetFaceEnabled {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The faces; empty is all six.
        #[serde(default)]
        faces: Vec<FaceId>,
        /// Drawn and exported (`true`) or not.
        enabled: bool,
    },
    /// Sets the Minecraft tint index of faces. Undo "Set tint".
    SetFaceTint {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The faces; empty is all six.
        #[serde(default)]
        faces: Vec<FaceId>,
        /// The index, 0 or more, or -1 for none.
        tint: i32,
    },
    /// Sets the Minecraft cull face of faces. Undo "Set cull face".
    SetFaceCull {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The faces; empty is all six.
        #[serde(default)]
        faces: Vec<FaceId>,
        /// The neighbour the face is culled against, `None` for no culling.
        cull: Option<FaceId>,
    },

    // Voxels.
    /// Sets up the voxel world at this resolution over the model box. Undo "Setup voxels".
    SetupVoxels {
        /// Voxels per block edge, 2 to 128.
        resolution: u32,
    },
    /// Gives every voxel of one colour another colour and renames the palette entry, in the
    /// whole grid. Undo "Replace voxels color".
    PaletteReplace {
        /// The colour to replace, RGBA; opaque.
        from: [u8; 4],
        /// The new colour, RGBA; opaque.
        to: [u8; 4],
    },
    /// Adds a colour to the palette of the voxel world. No undo step: the palette is derived from
    /// the voxels and rebuilt by undo, redo and load.
    PaletteAdd {
        /// The colour, RGBA; opaque.
        color: [u8; 4],
    },
    /// Takes a colour out of the palette of the voxel world, whatever the alpha of its entry. No
    /// undo step, for the reason of `PaletteAdd`; an entry that voxels still use comes back
    /// with the next undo, redo or load.
    PaletteRemove {
        /// The colour, RGBA; opaque.
        color: [u8; 4],
    },

    // UV. The faces are those of the elements among the objects; empty `faces` is all six.
    /// Re-derives the UV mapping. Undo "Apply default UV" or "Apply full UV mapping".
    AutoUv {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The layout.
        #[serde(default)]
        style: AutoUvStyle,
        /// The faces; empty is all six.
        #[serde(default)]
        faces: Vec<FaceId>,
    },
    /// Turns the UV of faces by degrees. Undo "Rotate UV".
    RotateUv {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The faces; empty is all six.
        #[serde(default)]
        faces: Vec<FaceId>,
        /// A multiple of 90; positive turns clockwise.
        degrees: i32,
    },
    /// Mirrors the UV of faces. Undo "Flip UV".
    FlipUv {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The faces; empty is all six.
        #[serde(default)]
        faces: Vec<FaceId>,
        /// The axis.
        axis: UvAxis,
    },
    /// Sets the UV rectangle of faces and turns UV snap off. Undo "Set UV".
    SetUvRect {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The faces; empty is all six.
        #[serde(default)]
        faces: Vec<FaceId>,
        /// `[u0, v0, u1, v1]`: the first corner and the second corner in texture coordinates,
        /// V up, clamped to -1 to 2.
        rect: [f32; 4],
        /// The rotation in degrees; `None` keeps each face's own.
        #[serde(default)]
        rotation: Option<i32>,
    },
    /// Turns UV snap, or its mirrored variant, on or off. Undo "Enable UV-snap" and its
    /// siblings.
    SetUvSnap {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The new state.
        enabled: bool,
        /// Set the mirrored variant instead of UV snap itself.
        #[serde(default)]
        mirror: bool,
    },

    /// A primitive mesh with generated geometry at the blueprint, its paint texture filled with
    /// the left colour. Undo "Create Cube primitive" and its siblings.
    AddMesh(MeshShape),

    // Display.
    /// Changes one display slot: each value that is given replaces the slot's own. Undo "Set
    /// display slot".
    SetDisplaySlot {
        /// The slot.
        slot: DisplayContext,
        /// Exported (`true`) or left out of the export.
        #[serde(default)]
        enabled: Option<bool>,
        /// Rotation in degrees, Minecraft axis order.
        #[serde(default)]
        rotation: Option<[f32; 3]>,
        /// Translation in 1/16 block units.
        #[serde(default)]
        translation: Option<[f32; 3]>,
        /// Scale factors.
        #[serde(default)]
        scale: Option<[f32; 3]>,
        /// The name of the preset the values come from, such as `item/generated`; `None` keeps
        /// the slot's own.
        #[serde(default)]
        preset: Option<String>,
    },

    // Groups.
    /// Makes a group of the objects and selects it. The pivot is the centre of their bounds and
    /// the group sits where the first of them was. Undo "Group".
    Group {
        /// The objects (elements, meshes and groups); empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The group name.
        name: String,
    },
    /// Dissolves groups: their children take their place and are selected. Undo "Ungroup".
    Ungroup {
        /// The groups; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
    },
    /// Moves objects and groups into a group or to the top level. A group cannot go into itself
    /// or into a group below it. Undo "Move to group" or "Move to top level".
    Reparent {
        /// The objects; empty is the selection.
        #[serde(default)]
        targets: Vec<Target>,
        /// The new parent group, `None` for the top level.
        #[serde(default)]
        parent: Option<Target>,
        /// The place among the children that stay, `None` for the end.
        #[serde(default)]
        index: Option<usize>,
    },
    /// Changes the fields of a group; each value that is given replaces the group's own. Moving
    /// the origin moves the pivot only. Undo "Edit group".
    SetGroup {
        /// The group.
        group: Target,
        /// The name.
        #[serde(default)]
        name: Option<String>,
        /// The pivot in model units.
        #[serde(default)]
        origin: Option<[f32; 3]>,
        /// Shown in the views, with everything below.
        #[serde(default)]
        visible: Option<bool>,
        /// Protected from drags, with everything below.
        #[serde(default)]
        locked: Option<bool>,
        /// Included in exports that know groups.
        #[serde(default)]
        export: Option<bool>,
    },

    // Animations. An animation is named by its position in the animation list.
    /// Adds an animation without keyframes. Undo "Add animation".
    AddAnimation {
        /// The name; an empty one becomes `animation`.
        name: String,
        /// The length in seconds, up to an hour.
        length: f32,
    },
    /// Removes an animation. Undo "Delete animation".
    RemoveAnimation {
        /// The animation.
        animation: usize,
    },
    /// Inserts a deep copy of an animation after it, named `<name>_copy` and without the
    /// identities of a `.bbmodel` project. Undo "Duplicate animation".
    DuplicateAnimation {
        /// The animation.
        animation: usize,
    },
    /// Changes the fields of an animation; each value that is given replaces the animation's own.
    /// Undo "Edit animation".
    SetAnimation {
        /// The animation.
        animation: usize,
        /// The name.
        #[serde(default)]
        name: Option<String>,
        /// The length in seconds, up to an hour.
        #[serde(default)]
        length: Option<f32>,
        /// What happens after the end.
        #[serde(default)]
        loop_mode: Option<AnimationLoop>,
        /// The frame rate of the snapping grid, 10 to 500.
        #[serde(default)]
        snapping: Option<u32>,
    },
    /// Sets the value of a channel of a group at a time: a keyframe there is replaced, otherwise
    /// one is inserted. Undo "Set keyframe".
    SetKeyframe {
        /// The animation.
        animation: usize,
        /// The animated group.
        group: Target,
        /// The channel.
        channel: BoneChannel,
        /// Seconds from the start.
        time: f32,
        /// The values on X, Y and Z: numbers or expressions.
        value: [KeyValue; 3],
        /// How the channel goes on from the keyframe; `null` keeps a keyframe's own and gives a
        /// new one the linear kind.
        #[serde(default)]
        interp: Option<KeyInterp>,
    },
    /// Changes how the channel goes on from keyframes. Undo "Set interpolation".
    SetKeyframeInterp {
        /// The animation.
        animation: usize,
        /// The keyframes.
        keys: Vec<KeyAt>,
        /// The interpolation they get.
        interp: KeyInterp,
    },
    /// Removes keyframes. Undo "Delete keyframes".
    RemoveKeyframes {
        /// The animation.
        animation: usize,
        /// The keyframes.
        keys: Vec<KeyAt>,
    },
    /// Puts keyframes, with everything they hold, on the keyframe clipboard. No undo step.
    CopyKeyframes {
        /// The animation.
        animation: usize,
        /// The keyframes.
        keys: Vec<KeyAt>,
    },
    /// Puts the keyframes of the clipboard into an animation, the earliest at `time` and the
    /// others at their distance from it. They go to the bones they came from, or all to `group`
    /// when they came from one bone; a keyframe on a keyframe replaces it. Undo "Paste
    /// keyframes".
    PasteKeyframes {
        /// The animation.
        animation: usize,
        /// Seconds from the start.
        time: f32,
        /// The bone that takes keyframes of a single bone.
        #[serde(default)]
        group: Option<Target>,
    },
    /// Moves keyframes in time; a moved keyframe replaces one it lands on. Undo "Move keyframes".
    MoveKeyframes {
        /// The animation.
        animation: usize,
        /// The keyframes.
        keys: Vec<KeyAt>,
        /// Seconds, positive or negative.
        delta: f32,
    },
}

/// The precondition of a command on the objects it names.
///
/// spec: menus 4.2
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Guard {
    /// No object needed.
    None,
    /// At least one object, the blueprint and the model origin included.
    Any,
    /// At least one element, mesh or group.
    Listed,
    /// At least one element.
    Elements,
    /// At least one group.
    Groups,
}

/// The undo entry of a command: its menu label, what it snapshots and its precondition.
///
/// spec: arch 10.1, tools 15.1
#[derive(Clone, Debug)]
pub struct CommandDef {
    /// The label of the Edit menu row, after "Undo" or "Redo".
    pub label: Cow<'static, str>,
    /// What the undo entry snapshots.
    pub categories: Categories,
    /// The precondition.
    pub guard: Guard,
}

/// How `Core::execute` treats a command.
#[derive(Clone, Debug)]
pub enum CommandKind {
    /// An edit with an undo entry.
    Edit(CommandDef),
    /// Runs without an undo entry: selection, copy and the history commands.
    Instant,
    /// Not built yet; the string says what is missing.
    Deferred(&'static str),
}

impl Command {
    /// How the command runs.
    pub fn kind(&self) -> CommandKind {
        use Command as C;
        let edit = |label: Cow<'static, str>, categories: Categories, guard: Guard| {
            CommandKind::Edit(CommandDef {
                label,
                categories,
                guard,
            })
        };
        let object = |label: &'static str, guard: Guard| {
            edit(Cow::Borrowed(label), Categories::OBJECTS, guard)
        };
        // The commands that can reach a group snapshot the tree with the objects.
        let tree = |label: &'static str, guard: Guard| {
            edit(
                Cow::Borrowed(label),
                Categories::OBJECTS | Categories::HIERARCHY,
                guard,
            )
        };
        // A removal also takes the tracks of the groups it removes.
        let removal = |label: &'static str, guard: Guard| {
            edit(
                Cow::Borrowed(label),
                Categories::OBJECTS | Categories::HIERARCHY | Categories::ANIMATION,
                guard,
            )
        };
        let animation =
            |label: &'static str| edit(Cow::Borrowed(label), Categories::ANIMATION, Guard::None);
        let element =
            |label: &'static str| edit(Cow::Borrowed(label), Categories::ELEMENTS, Guard::Elements);
        match self {
            C::Undo
            | C::Redo
            | C::Select { .. }
            | C::SelectAll
            | C::SelectNone
            | C::SelectInverse
            | C::Copy
            | C::CopyKeyframes { .. } => CommandKind::Instant,
            C::Cut => removal("Cut", Guard::Listed),
            C::Paste { .. } => object("Paste", Guard::None),
            C::Duplicate { .. } => tree("Duplicate", Guard::Listed),
            C::AddElement(kind) => object(
                match kind {
                    ElementKind::Cube => "Create Cube element",
                    ElementKind::PlaneX => "Create PlaneX element",
                    ElementKind::PlaneY => "Create PlaneY element",
                    ElementKind::PlaneZ => "Create PlaneZ element",
                },
                Guard::None,
            ),
            C::Delete { .. } => removal("Delete", Guard::Listed),
            C::Rename { name, .. } => edit(
                Cow::Owned(format!("Rename to {name}")),
                Categories::OBJECTS | Categories::HIERARCHY,
                Guard::Listed,
            ),
            C::SetHidden { hidden, .. } => {
                tree(if *hidden { "Hide" } else { "Show" }, Guard::Listed)
            }
            C::SetLocked { locked, .. } => {
                tree(if *locked { "Lock" } else { "Unlock" }, Guard::Listed)
            }
            C::ShowAll => tree("Show all", Guard::None),
            C::UnlockAll => tree("Unlock all", Guard::None),
            C::MoveBy { .. } => tree("Move", Guard::Any),
            C::SetPosition { .. } => tree("Set position", Guard::Any),
            C::SetSize { .. } => object("Set size", Guard::Any),
            C::ScaleBy { .. } => tree("Scale", Guard::Any),
            C::RotateBy { .. } => tree("Rotate", Guard::Any),
            C::SetRotation { .. } => tree("Set rotation", Guard::Any),
            C::SetPivot { .. } => tree("Set pivot", Guard::Listed),
            C::QuarterTurn { axis, .. } => object(
                match axis {
                    Axis::X => "Rotate around X axis",
                    Axis::Y => "Rotate around Y axis",
                    Axis::Z => "Rotate around Z axis",
                },
                Guard::Elements,
            ),
            C::ResetRotation { .. } => tree("Reset rotation", Guard::Any),
            C::ResetPivot { .. } => tree("Reset pivot", Guard::Listed),
            C::Mirror { axis, .. } => object(
                match axis {
                    Axis::X => "Mirror on X axis",
                    Axis::Y => "Mirror on Y axis",
                    Axis::Z => "Mirror on Z axis",
                },
                Guard::Elements,
            ),
            C::Align { axis, .. } => object(
                match axis {
                    Axis::X => "Align X",
                    Axis::Y => "Align Y",
                    Axis::Z => "Align Z",
                },
                Guard::Any,
            ),
            C::CreateTexture { .. } => edit(
                Cow::Borrowed("Add new texture"),
                Categories::TEXTURES,
                Guard::None,
            ),
            C::AddTexture { .. } => edit(
                Cow::Borrowed("Add texture"),
                Categories::TEXTURES,
                Guard::None,
            ),
            C::RemoveTexture { .. } => edit(
                Cow::Borrowed("Delete texture"),
                Categories::TEXTURES,
                Guard::None,
            ),
            C::AssignTexture { .. } => edit(
                Cow::Borrowed("Apply texture"),
                Categories::ELEMENTS,
                Guard::Elements,
            ),
            C::ReplaceTexture { .. } => edit(
                Cow::Borrowed("Replace texture"),
                Categories::TEXTURES,
                Guard::None,
            ),
            C::SetColor { .. } => element("Set color"),
            C::SetShade { enabled, .. } => element(if *enabled {
                "Enable shade"
            } else {
                "Disable shade"
            }),
            C::SetFaceEnabled { faces, enabled, .. } => {
                let name = match faces.as_slice() {
                    [FaceId::Down] => "Down face",
                    [FaceId::Up] => "Up face",
                    [FaceId::North] => "North face",
                    [FaceId::South] => "South face",
                    [FaceId::West] => "West face",
                    [FaceId::East] => "East face",
                    [] => "all faces",
                    _ => "faces",
                };
                let verb = if *enabled { "Enable" } else { "Disable" };
                edit(
                    Cow::Owned(format!("{verb} {name}")),
                    Categories::ELEMENTS,
                    Guard::Elements,
                )
            }
            C::SetFaceTint { .. } => element("Set tint"),
            C::SetFaceCull { .. } => element("Set cull face"),
            C::AddMesh(shape) => object(
                match shape {
                    MeshShape::Cube => "Create Cube primitive",
                    MeshShape::Sphere => "Create Sphere primitive",
                    MeshShape::Cylinder => "Create Cylinder primitive",
                    MeshShape::Cone => "Create Cone primitive",
                    MeshShape::Pyramid => "Create Pyramid primitive",
                    MeshShape::Slope => "Create Slope primitive",
                    MeshShape::CornerIn => "Create Corner-in primitive",
                    MeshShape::CornerOut => "Create Corner-out primitive",
                    MeshShape::Torus => "Create Torus primitive",
                },
                Guard::None,
            ),
            // The model properties are in the step because the edit may turn on `include_display`.
            C::SetDisplaySlot { .. } => edit(
                Cow::Borrowed("Set display slot"),
                Categories::MODEL_DISPLAY | Categories::MODEL_PROPERTIES,
                Guard::None,
            ),
            C::Group { .. } => edit(Cow::Borrowed("Group"), Categories::HIERARCHY, Guard::Listed),
            C::Ungroup { .. } => edit(
                Cow::Borrowed("Ungroup"),
                Categories::HIERARCHY | Categories::ANIMATION,
                Guard::Groups,
            ),
            C::Reparent { parent, .. } => edit(
                Cow::Borrowed(if parent.is_some() {
                    "Move to group"
                } else {
                    "Move to top level"
                }),
                Categories::HIERARCHY,
                Guard::Listed,
            ),
            C::SetGroup { .. } => edit(
                Cow::Borrowed("Edit group"),
                Categories::HIERARCHY,
                Guard::None,
            ),
            C::AddAnimation { .. } => animation("Add animation"),
            C::RemoveAnimation { .. } => animation("Delete animation"),
            C::DuplicateAnimation { .. } => animation("Duplicate animation"),
            C::SetAnimation { .. } => animation("Edit animation"),
            C::SetKeyframe { .. } => animation("Set keyframe"),
            C::SetKeyframeInterp { .. } => animation("Set interpolation"),
            C::RemoveKeyframes { .. } => animation("Delete keyframes"),
            C::MoveKeyframes { .. } => animation("Move keyframes"),
            C::PasteKeyframes { .. } => animation("Paste keyframes"),
            C::SetupVoxels { .. } => edit(
                Cow::Borrowed("Setup voxels"),
                Categories::VOXELS,
                Guard::None,
            ),
            C::PaletteReplace { .. } => edit(
                Cow::Borrowed("Replace voxels color"),
                Categories::VOXELS,
                Guard::None,
            ),
            C::PaletteAdd { .. } | C::PaletteRemove { .. } => CommandKind::Instant,
            C::AutoUv { style, .. } => element(match style {
                AutoUvStyle::Box => "Apply default UV",
                AutoUvStyle::PerFace => "Apply full UV mapping",
            }),
            C::RotateUv { .. } => element("Rotate UV"),
            C::FlipUv { .. } => element("Flip UV"),
            C::SetUvRect { .. } => element("Set UV"),
            C::SetUvSnap {
                enabled, mirror, ..
            } => element(match (enabled, mirror) {
                (true, false) => "Enable UV-snap",
                (false, false) => "Disable UV-snap",
                (true, true) => "Enable UV-snap mirror",
                (false, true) => "Disable UV-snap mirror",
            }),
        }
    }

    /// The objects the command names; empty for the selection and for commands without
    /// objects.
    pub fn targets(&self) -> &[Target] {
        use Command as C;
        match self {
            C::Select { targets, .. }
            | C::Duplicate { targets }
            | C::Delete { targets }
            | C::Rename { targets, .. }
            | C::SetHidden { targets, .. }
            | C::SetLocked { targets, .. }
            | C::MoveBy { targets, .. }
            | C::SetPosition { targets, .. }
            | C::SetSize { targets, .. }
            | C::ScaleBy { targets, .. }
            | C::RotateBy { targets, .. }
            | C::SetRotation { targets, .. }
            | C::SetPivot { targets, .. }
            | C::QuarterTurn { targets, .. }
            | C::ResetRotation { targets }
            | C::ResetPivot { targets }
            | C::Mirror { targets, .. }
            | C::Align { targets, .. }
            | C::AssignTexture { targets, .. }
            | C::SetColor { targets, .. }
            | C::SetShade { targets, .. }
            | C::SetFaceEnabled { targets, .. }
            | C::SetFaceTint { targets, .. }
            | C::SetFaceCull { targets, .. }
            | C::Group { targets, .. }
            | C::Ungroup { targets }
            | C::Reparent { targets, .. }
            | C::AutoUv { targets, .. }
            | C::RotateUv { targets, .. }
            | C::FlipUv { targets, .. }
            | C::SetUvRect { targets, .. }
            | C::SetUvSnap { targets, .. } => targets,
            C::SetGroup { group, .. } | C::SetKeyframe { group, .. } => std::slice::from_ref(group),
            C::PasteKeyframes { group, .. } => group.as_slice(),
            _ => &[],
        }
    }

    /// Checks the numbers of the payload: finite values, factors above zero, steps above zero.
    pub fn validate(&self) -> Result<(), Refusal> {
        use Command as C;
        let finite = |values: &[f32]| {
            values
                .iter()
                .all(|v| v.is_finite())
                .then_some(())
                .ok_or(Refusal::NotFinite)
        };
        let optional = |values: &[Option<f32>; 3]| {
            let present: Vec<f32> = values.iter().flatten().copied().collect();
            finite(&present)
        };
        let step = |snap: &Option<f32>| match snap {
            Some(step) if !(step.is_finite() && *step > 0.0) => Err(Refusal::OutOfRange),
            _ => Ok(()),
        };
        // A length or a time of an animation: from 0 to an hour.
        let length_ok = |seconds: f32| {
            finite(&[seconds])?;
            (0.0..=MAX_LENGTH)
                .contains(&seconds)
                .then_some(())
                .ok_or(Refusal::OutOfRange)
        };
        match self {
            C::MoveBy { delta, snap, .. } | C::RotateBy { delta, snap, .. } => {
                finite(delta)?;
                step(snap)
            }
            C::SetPosition { value, .. }
            | C::SetSize { value, .. }
            | C::SetRotation { value, .. }
            | C::SetPivot { value, .. } => optional(value),
            C::SetColor { color, .. } => finite(color),
            C::SetFaceTint { tint, .. } if *tint < -1 => Err(Refusal::OutOfRange),
            C::ScaleBy { factor, snap, .. } => {
                finite(factor)?;
                if factor.iter().any(|f| *f <= 0.0) {
                    return Err(Refusal::OutOfRange);
                }
                step(snap)
            }
            C::SetUvRect { rect, .. } => finite(rect),
            C::SetDisplaySlot {
                rotation,
                translation,
                scale,
                ..
            } => {
                let values: Vec<f32> = [rotation, translation, scale]
                    .into_iter()
                    .flatten()
                    .flatten()
                    .copied()
                    .collect();
                finite(&values)
            }
            C::AddTexture {
                width,
                height,
                rgba,
                ..
            }
            | C::ReplaceTexture {
                width,
                height,
                rgba,
                ..
            } => {
                let sized = cuboid_model::image::check_size(*width, *height).is_ok()
                    && *width > 0
                    && *height > 0
                    && u64::from(*width) * u64::from(*height) * 4 == rgba.len() as u64;
                sized.then_some(()).ok_or(Refusal::OutOfRange)
            }
            C::RotateUv { degrees, .. } if degrees % 90 != 0 => Err(Refusal::OutOfRange),
            C::SetGroup {
                origin: Some(origin),
                ..
            } => finite(origin),
            C::AddAnimation { length, .. } => length_ok(*length),
            C::SetAnimation {
                length, snapping, ..
            } => {
                if let Some(length) = length {
                    length_ok(*length)?;
                }
                match snapping {
                    Some(rate) if !(10..=500).contains(rate) => Err(Refusal::OutOfRange),
                    _ => Ok(()),
                }
            }
            C::SetKeyframe { time, value, .. } => {
                value.iter().try_for_each(KeyValue::check)?;
                length_ok(*time)
            }
            C::PasteKeyframes { time, .. } => length_ok(*time),
            C::MoveKeyframes { delta, .. } => finite(&[*delta]),
            _ => Ok(()),
        }
    }

    /// The command a menu row, key binding or palette entry stands for, when the core owns it
    /// and it needs no further input.
    pub fn from_action(action: ActionId) -> Option<Command> {
        use ActionId as A;
        let none = Vec::new;
        Some(match action {
            A::Undo => Command::Undo,
            A::Redo => Command::Redo,
            A::Cut => Command::Cut,
            A::Copy => Command::Copy,
            A::Paste => Command::Paste {
                at_blueprint: false,
            },
            A::Delete | A::DeleteConfirm | A::ElementDelete => Command::Delete { targets: none() },
            A::SelectAllElements => Command::SelectAll,
            A::SelectNone => Command::SelectNone,
            A::SelectInverse => Command::SelectInverse,
            A::Clone | A::ElementClone => Command::Duplicate { targets: none() },
            A::Hide | A::ElementHide => Command::SetHidden {
                targets: none(),
                hidden: true,
            },
            A::Show | A::ElementShow => Command::SetHidden {
                targets: none(),
                hidden: false,
            },
            A::Lock | A::ElementLock => Command::SetLocked {
                targets: none(),
                locked: true,
            },
            A::Unlock | A::ElementUnlock => Command::SetLocked {
                targets: none(),
                locked: false,
            },
            A::ShowAll => Command::ShowAll,
            A::UnlockAll => Command::UnlockAll,
            A::CreateElementCube => Command::AddElement(ElementKind::Cube),
            A::CreateElementPlaneX => Command::AddElement(ElementKind::PlaneX),
            A::CreateElementPlaneY => Command::AddElement(ElementKind::PlaneY),
            A::CreateElementPlaneZ => Command::AddElement(ElementKind::PlaneZ),
            A::ResetRotation => Command::ResetRotation { targets: none() },
            A::RotateX => Command::QuarterTurn {
                targets: none(),
                axis: Axis::X,
            },
            A::RotateY => Command::QuarterTurn {
                targets: none(),
                axis: Axis::Y,
            },
            A::RotateZ => Command::QuarterTurn {
                targets: none(),
                axis: Axis::Z,
            },
            A::CenterX => Command::Align {
                targets: none(),
                axis: Axis::X,
                edge: AlignEdge::Center,
            },
            A::CenterY => Command::Align {
                targets: none(),
                axis: Axis::Y,
                edge: AlignEdge::Center,
            },
            A::CenterZ => Command::Align {
                targets: none(),
                axis: Axis::Z,
                edge: AlignEdge::Center,
            },
            A::UvSnapEnable => Command::SetUvSnap {
                targets: none(),
                enabled: true,
                mirror: false,
            },
            A::UvSnapDisable => Command::SetUvSnap {
                targets: none(),
                enabled: false,
                mirror: false,
            },
            A::UvSnapMirrorEnable => Command::SetUvSnap {
                targets: none(),
                enabled: true,
                mirror: true,
            },
            A::UvSnapMirrorDisable => Command::SetUvSnap {
                targets: none(),
                enabled: false,
                mirror: true,
            },
            _ => return None,
        })
    }
}

bitflags! {
    /// Parts of the window that must be refreshed after a command; data, because the core
    /// never draws.
    ///
    /// spec: arch 10.1
    #[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
    pub struct Refresh: u8 {
        /// The Edit menu rows (undo and redo labels, paste label, enabled state).
        const EDIT_MENU = 1;
        /// The element and mesh lists.
        const LISTS = 2;
        /// The texture list and previews.
        const TEXTURES = 4;
        /// The window title.
        const TITLE = 8;
        /// The gizmos.
        const GIZMOS = 16;
    }
}

/// What to do to the voxel colliders of the scene index, in order. The core does not own the
/// index; it asks.
///
/// spec: tools 12.2, arch 10.5
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ColliderAction {
    /// A stroke begins: freeze the collider snapshot.
    Freeze,
    /// A stroke ended or a rebuild pass completed: refresh the snapshot from the document.
    Refresh,
}

/// UI requests a command produced, as data.
///
/// spec: arch 10.1
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effects {
    /// A new status line text.
    pub status: Option<String>,
    /// Messages to show without waiting for an answer.
    pub notices: Vec<UserMessage>,
    /// What to refresh.
    pub refresh: Refresh,
    /// What to do to the voxel colliders of the scene index.
    pub colliders: Vec<ColliderAction>,
}

/// Why a command was refused. The document is untouched.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum Refusal {
    /// The command needs objects and none were named or selected.
    #[error("nothing is selected")]
    NothingSelected,
    /// The command needs at least one element.
    #[error("select at least one element")]
    NoElements,
    /// A target is not in the document.
    #[error("the target is not in the model")]
    BadTarget,
    /// The history has no entry to undo.
    #[error("nothing to undo")]
    NothingToUndo,
    /// The history has no entry to redo.
    #[error("nothing to redo")]
    NothingToRedo,
    /// Paste with an empty clipboard.
    #[error("the clipboard has no objects")]
    ClipboardEmpty,
    /// Delete over the blueprint or the model origin only.
    #[error("the selected objects cannot be deleted")]
    NotDeletable,
    /// A number of the payload is NaN or infinite.
    #[error("a value is not a finite number")]
    NotFinite,
    /// A number of the payload is out of its range.
    #[error("a value is out of range")]
    OutOfRange,
    /// The restricted-rotation rule refused the rotation.
    #[error("restricted rotation allows one rotated axis")]
    RotationRestricted,
    /// The command needs at least one group.
    #[error("select at least one group")]
    NoGroups,
    /// The change of the group tree is not allowed.
    #[error("{0}")]
    Group(GroupError),
    /// The change of the animations is not allowed.
    #[error("{0}")]
    Animation(AnimationError),
    /// An operation refused, with its message.
    #[error("{0}")]
    Op(String),
}

/// What a command did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Done {
    /// The objects the command created or pasted, in order; they are selected.
    pub created: Vec<Target>,
    /// How many objects, entries or labels the command changed.
    pub affected: usize,
    /// UI requests.
    pub effects: Effects,
}

/// The result of `Core::execute`: always a value, never a panic.
///
/// spec: arch 6 (cuboid-core), arch 10.1
#[derive(Clone, Debug, PartialEq)]
pub enum CmdOutcome {
    /// The command ran.
    Done(Done),
    /// The command was valid but left the document as it was; no undo entry.
    Unchanged,
    /// The command was refused; nothing changed.
    Refused(Refusal),
    /// The feature is not built yet.
    NotYet(&'static str),
    /// The command failed inside; the document is back to its state before the command.
    Failed(String),
}

impl CmdOutcome {
    /// Whether the command ran and changed something.
    pub fn is_done(&self) -> bool {
        matches!(self, CmdOutcome::Done(_))
    }

    /// The result of a successful command.
    pub fn done(&self) -> Option<&Done> {
        match self {
            CmdOutcome::Done(done) => Some(done),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_round_trip_through_json() {
        let commands = [
            Command::MoveBy {
                targets: vec![Target::Element(2), Target::Blueprint],
                delta: [1.0, 0.0, -0.5],
                snap: Some(0.5),
            },
            Command::SetPosition {
                targets: vec![],
                value: [Some(1.0), None, Some(3.0)],
            },
            Command::Paste { at_blueprint: true },
            Command::CreateTexture {
                label: Some("stone".into()),
                width: 16,
                height: 16,
                color: [1, 2, 3, 255],
            },
            Command::Mirror {
                targets: vec![Target::Mesh(0)],
                axis: Axis::Y,
                copy: true,
            },
            Command::AddElement(ElementKind::PlaneZ),
            Command::SetDisplaySlot {
                slot: DisplayContext::Head,
                enabled: Some(true),
                rotation: None,
                translation: Some([0.0, 13.0, 7.0]),
                scale: None,
                preset: Some("item/generated".into()),
            },
            Command::SetPivot {
                targets: vec![Target::Element(1)],
                value: [Some(1.0), None, Some(-2.5)],
            },
            Command::SetColor {
                targets: vec![],
                color: [0.25, 0.5, 1.0],
            },
            Command::SetFaceCull {
                targets: vec![Target::Element(0)],
                faces: vec![FaceId::Up],
                cull: Some(FaceId::Down),
            },
            Command::ReplaceTexture {
                label: "stone".into(),
                width: 1,
                height: 1,
                rgba: vec![1, 2, 3, 4],
                file: Some(("sig".into(), 12)),
            },
            Command::PaletteRemove {
                color: [1, 2, 3, 255],
            },
            Command::AddAnimation {
                name: "walk".into(),
                length: 1.5,
            },
            Command::SetAnimation {
                animation: 1,
                name: None,
                length: Some(2.0),
                loop_mode: Some(AnimationLoop::Hold),
                snapping: Some(30),
            },
            Command::SetKeyframe {
                animation: 0,
                group: Target::Group(2),
                channel: BoneChannel::Position,
                time: 0.25,
                value: [
                    KeyValue::Num(1.0),
                    KeyValue::Text("math.sin(q.anim_time)".into()),
                    KeyValue::Num(-2.0),
                ],
                interp: Some(KeyInterp::Step),
            },
            Command::SetKeyframeInterp {
                animation: 0,
                keys: vec![KeyAt {
                    group: Target::Group(1),
                    channel: BoneChannel::Rotation,
                    index: 0,
                }],
                interp: KeyInterp::CatmullRom,
            },
            Command::DuplicateAnimation { animation: 2 },
            Command::PasteKeyframes {
                animation: 0,
                time: 1.5,
                group: Some(Target::Group(3)),
            },
            Command::MoveKeyframes {
                animation: 0,
                keys: vec![KeyAt {
                    group: Target::Group(2),
                    channel: BoneChannel::Scale,
                    index: 3,
                }],
                delta: -0.5,
            },
        ];
        for command in commands {
            let text = serde_json::to_string(&command).expect("serialises");
            let back: Command = serde_json::from_str(&text).expect("deserialises");
            assert_eq!(back, command, "{text}");
        }
    }

    #[test]
    fn a_keyframe_value_reads_numbers_and_text_and_old_commands_still_read() {
        let old = r#"{"SetKeyframe":{"animation":0,"group":{"Group":1},"channel":"Position",
            "time":0.5,"value":[1,2.5,-3]}}"#;
        let Command::SetKeyframe { value, interp, .. } = serde_json::from_str(old).expect("reads")
        else {
            panic!("wrong variant");
        };
        assert_eq!(value, KeyValue::nums([1.0, 2.5, -3.0]));
        assert_eq!(interp, None);
        let mixed: [KeyValue; 3] =
            serde_json::from_str(r#"[1, "2.5", "math.sin(q.life_time)"]"#).expect("reads");
        // Text that is a number is that number, other text stays an expression.
        assert_eq!(mixed[0].value(), Value::Num(1.0));
        assert_eq!(mixed[1].value(), Value::Num(2.5));
        assert!(mixed[2].value().is_expression());
        assert!(mixed.iter().all(|v| v.check().is_ok()));
        for bad in [
            KeyValue::Num(f32::NAN),
            KeyValue::Text(String::new()),
            KeyValue::Text("x".repeat(MAX_EXPRESSION_BYTES + 1)),
        ] {
            assert!(bad.check().is_err());
        }
    }

    #[test]
    fn omitted_optional_fields_take_their_defaults() {
        let command: Command =
            serde_json::from_str(r#"{"MoveBy":{"delta":[1.0,2.0,3.0]}}"#).expect("parses");
        let Command::MoveBy {
            targets,
            delta,
            snap,
        } = command
        else {
            panic!("wrong variant");
        };
        assert!(targets.is_empty() && snap.is_none());
        assert!(
            delta
                .iter()
                .zip([1.0, 2.0, 3.0])
                .all(|(a, b)| (a - b).abs() < 1e-9)
        );
    }

    #[test]
    fn bad_numbers_are_refused_by_validation() {
        let nan = Command::MoveBy {
            targets: vec![],
            delta: [f32::NAN, 0.0, 0.0],
            snap: None,
        };
        assert_eq!(nan.validate(), Err(Refusal::NotFinite));
        let flat = Command::ScaleBy {
            targets: vec![],
            factor: [1.0, 0.0, 1.0],
            snap: None,
        };
        assert_eq!(flat.validate(), Err(Refusal::OutOfRange));
        let step = Command::RotateBy {
            targets: vec![],
            delta: [0.0; 3],
            snap: Some(0.0),
        };
        assert_eq!(step.validate(), Err(Refusal::OutOfRange));
        assert_eq!(Command::SelectAll.validate(), Ok(()));
    }

    #[test]
    fn labels_follow_the_catalogue() {
        let label = |c: Command| match c.kind() {
            CommandKind::Edit(def) => def.label.into_owned(),
            _ => String::new(),
        };
        assert_eq!(
            label(Command::AddElement(ElementKind::PlaneX)),
            "Create PlaneX element"
        );
        assert_eq!(
            label(Command::Rename {
                targets: vec![],
                name: "arm".into()
            }),
            "Rename to arm"
        );
        assert_eq!(
            label(Command::SetHidden {
                targets: vec![],
                hidden: false
            }),
            "Show"
        );
        assert_eq!(
            label(Command::SetDisplaySlot {
                slot: DisplayContext::Gui,
                enabled: None,
                rotation: None,
                translation: None,
                scale: None,
                preset: None,
            }),
            "Set display slot"
        );
        let enable = |faces: Vec<FaceId>, enabled| Command::SetFaceEnabled {
            targets: vec![],
            faces,
            enabled,
        };
        assert_eq!(
            label(enable(vec![FaceId::West], false)),
            "Disable West face"
        );
        assert_eq!(label(enable(vec![], true)), "Enable all faces");
        assert_eq!(
            label(Command::SetShade {
                targets: vec![],
                enabled: true
            }),
            "Enable shade"
        );
    }
}
