//! Element, face, mesh, blueprint and texture-entry records (spec: tools 3.1, 4.1, 4.4, 5.1, 6.1;
//! native 4).

use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use bitflags::bitflags;
use cuboid_image::Image;
use glam::{Vec2, Vec3};

use crate::uv::UvRect;

/// A texture label or an object name. Cheap to clone; one allocation per distinct text within a
/// document (the interning is a list-level concern).
///
/// spec: arch 8.3
#[derive(Clone, Debug, Default, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Label(Arc<str>);

impl Deref for Label {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for Label {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Label {
    fn from(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

impl From<String> for Label {
    fn from(s: String) -> Self {
        Self(Arc::from(s))
    }
}

bitflags! {
    /// What the user may do with an object: the gates of the gizmos and drags, the participation
    /// in restricted-rotation mode, scale snapping and deletion. The named combinations are the
    /// prefab values per object kind; they never change at run time except that an imported
    /// mesh gets its own at import.
    ///
    /// spec: tools 3.1
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub struct Perms: u8 {
        /// `canMove`.
        const MOVE = 1;
        /// `canRotate`.
        const ROTATE = 2;
        /// `canScale`.
        const SCALE = 4;
        /// Takes part in restricted-rotation mode (22.5 degree grid, single axis).
        const RESTRICTABLE = 8;
        /// Scale drags snap to the mouse step.
        const GRID_SNAP_ON_SCALE = 16;
        /// May be deleted.
        const DELETABLE = 32;

        /// An element: everything.
        const ELEMENT = Self::MOVE.bits() | Self::ROTATE.bits() | Self::SCALE.bits()
            | Self::RESTRICTABLE.bits() | Self::GRID_SNAP_ON_SCALE.bits() | Self::DELETABLE.bits();
        /// The blueprint: an element that cannot be deleted.
        const BLUEPRINT = Self::MOVE.bits() | Self::ROTATE.bits() | Self::SCALE.bits()
            | Self::RESTRICTABLE.bits() | Self::GRID_SNAP_ON_SCALE.bits();
        /// A built-in primitive mesh: never restrictable, snaps on scale.
        const PRIMITIVE = Self::MOVE.bits() | Self::ROTATE.bits() | Self::SCALE.bits()
            | Self::GRID_SNAP_ON_SCALE.bits() | Self::DELETABLE.bits();
        /// An imported mesh (and a mesh record adopted on load, undo or paste): never
        /// restrictable, does not snap on scale.
        const IMPORTED_MESH = Self::MOVE.bits() | Self::ROTATE.bits() | Self::SCALE.bits()
            | Self::DELETABLE.bits();
        /// The model-origin marker: move only.
        const MODEL_ORIGIN = Self::MOVE.bits();
    }
}

/// The smallest size component the size setter stores (and a plane's thickness).
///
/// spec: tools 3.3, tools 4.2
pub const MIN_SIZE: f32 = 0.001;

/// The fields every scene object shares. The display id is not stored here; it lives in the
/// parallel vector of the owning [`ObjList`](crate::list::ObjList).
///
/// spec: tools 3.1, arch 6 (`ObjectBase`)
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectBase {
    /// Display name.
    pub label: Label,
    /// Position `p`: the min-x, min-y, max-z corner of the unrotated box after the rotation
    /// about the pivot (tools 1.3). Internal space.
    pub p: Vec3,
    /// Size `s`; each component is at least 0.001 when set through the size setter.
    pub s: Vec3,
    /// Rotation `e` as Euler degrees, wrapped and quantised by the rotation setter.
    pub e: Vec3,
    /// Pivot `o`.
    pub o: Vec3,
    /// Not rendered and not pickable in the views.
    pub hidden: bool,
    /// Blocks mouse drags (not keyboard nudges).
    pub locked: bool,
    /// Gates and prefab flags.
    pub perms: Perms,
    /// Multiplies scale-drag deltas; imported meshes keep their normalisation factor here.
    pub drag_sensitivity: f32,
    /// Scale gizmo extent multiplier (meshes).
    pub gizmo_size: Vec3,
    /// Scale gizmo offset (meshes: the import offset).
    pub gizmo_pivot: Vec3,
}

impl ObjectBase {
    /// The base of a new object: rotation 0, origin at the position, visible, unlocked, drag
    /// sensitivity 1 and the default gizmo values.
    ///
    /// spec: tools 3.1, tools 10.1
    pub fn new(label: impl Into<Label>, p: Vec3, s: Vec3, perms: Perms) -> Self {
        Self {
            label: label.into(),
            p,
            s,
            e: Vec3::ZERO,
            o: p,
            hidden: false,
            locked: false,
            perms,
            drag_sensitivity: 1.0,
            gizmo_size: Vec3::ONE,
            gizmo_pivot: Vec3::ZERO,
        }
    }
}

/// The element kinds. The discriminants are the persisted `t` values.
///
/// spec: tools 4.2, native 4.9
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum ElementType {
    /// A box with six faces.
    Cube = 0,
    /// A box 0.001 thick along X with west and east faces.
    PlaneX = 1,
    /// A box 0.001 thick along Y with down and up faces.
    PlaneY = 2,
    /// A box 0.001 thick along Z with north and south faces.
    PlaneZ = 3,
}

/// The six faces in slot order, which is also the file order (`d`, `u`, `n`, `s`, `w`, `e`).
///
/// spec: tools 4.3, tools 4.4, native 4.4.1
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum FaceName {
    /// -Y.
    Down = 0,
    /// +Y.
    Up = 1,
    /// +Z internal (-Z in Minecraft space).
    North = 2,
    /// -Z internal.
    South = 3,
    /// -X.
    West = 4,
    /// +X.
    East = 5,
}

impl FaceName {
    /// All faces in slot order.
    pub const ALL: [FaceName; 6] = [
        FaceName::Down,
        FaceName::Up,
        FaceName::North,
        FaceName::South,
        FaceName::West,
        FaceName::East,
    ];
}

bitflags! {
    /// Face directions as flags: the cull direction of a face and the enabled-face mask.
    ///
    /// spec: tools 1.2, native 4.9 (FaceType)
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub struct FaceFlags: u8 {
        /// Down.
        const DOWN = 1;
        /// Up.
        const UP = 2;
        /// North.
        const NORTH = 4;
        /// South.
        const SOUTH = 8;
        /// West.
        const WEST = 16;
        /// East.
        const EAST = 32;
        /// Any or all faces (63).
        const ALL = Self::DOWN.bits() | Self::UP.bits() | Self::NORTH.bits()
            | Self::SOUTH.bits() | Self::WEST.bits() | Self::EAST.bits();
    }
}

/// One face of an element.
///
/// spec: tools 4.4, native 4.4.2
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// Its slot's name; face copies keep it unless asked otherwise.
    pub name: FaceName,
    /// Drawn and exported.
    pub active: bool,
    /// Minecraft tint index, -1 for none.
    pub tint: i32,
    /// Minecraft cull direction; a single direction is expected.
    pub cull: FaceFlags,
    /// Key into the texture list.
    pub texture: Label,
    /// UV rectangle in the face's own texture.
    pub uv: UvRect,
}

impl Face {
    /// A face as an editor creates it: no tint, no cull direction and the full texture.
    ///
    /// spec: tools 4.4
    pub fn new(name: FaceName, active: bool, texture: Label) -> Self {
        Self {
            name,
            active,
            tint: -1,
            cull: FaceFlags::empty(),
            texture,
            uv: UvRect::default(),
        }
    }

    /// Face copy (tools 4.4): copies active, tint, cull, texture and uv onto `dest`, and the
    /// name unless `keep_name`. The UV corners are copied folded (tools 4.5).
    pub fn copy_to(&self, dest: &mut Face, keep_name: bool) {
        let name = dest.name;
        dest.clone_from(self);
        dest.uv = self.uv.folded();
        if keep_name {
            dest.name = name;
        }
    }
}

bitflags! {
    /// The boolean options of an element.
    ///
    /// spec: tools 4.1
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub struct ElementFlags: u8 {
        /// `suv`: the default UV mapping follows position and size.
        const UV_SNAP = 1;
        /// `suvm`: the mirrored variant of the automatic mapping.
        const UV_SNAP_MIRROR = 2;
        /// `rs`: Minecraft "rescale"; stored, used by exporters only.
        const RESCALE = 4;
        /// `sh`: Minecraft "shade" (true by default).
        const SHADE = 8;
        /// `in`: inverted (inner) normals.
        const INVERT_NORMALS = 16;
    }
}

/// A cuboid or plane element.
///
/// spec: tools 4.1, native 4.4, arch 6 (cuboid-model)
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// The shared object fields.
    pub base: ObjectBase,
    /// Cube or plane orientation.
    pub kind: ElementType,
    /// RGBA in 0..=1; alpha is 1 unless the colour is fully transparent.
    pub color: [f32; 4],
    /// The six faces in slot order.
    pub faces: [Face; 6],
    /// UV snap, shade and the other options.
    pub flags: ElementFlags,
}

impl Element {
    /// A new element at the blueprint's position and scale, as the creation commands make it:
    /// label by type and `display_id` (`ObjList::next_display_id`), origin at the position,
    /// rotation 0, the faces of the type enabled, UV snap and shade on. The caller supplies the
    /// random colour; the default UV mapping is applied when the element starts
    /// (`ElementMut::default_uv`).
    ///
    /// spec: tools 4.2, tools 10.1
    pub fn new(
        kind: ElementType,
        bp: &Blueprint,
        display_id: u32,
        color: [f32; 4],
        texture: &Label,
    ) -> Self {
        let (prefix, enabled) = match kind {
            ElementType::Cube => ("Box", FaceFlags::ALL),
            ElementType::PlaneX => ("PlaneX", FaceFlags::WEST | FaceFlags::EAST),
            ElementType::PlaneY => ("PlaneY", FaceFlags::DOWN | FaceFlags::UP),
            ElementType::PlaneZ => ("PlaneZ", FaceFlags::NORTH | FaceFlags::SOUTH),
        };
        // A plane's thin axis is set to 0 and clamped to the minimum size.
        let s = match kind {
            ElementType::Cube => bp.base.s,
            ElementType::PlaneX => Vec3::new(MIN_SIZE, bp.base.s.y, bp.base.s.z),
            ElementType::PlaneY => Vec3::new(bp.base.s.x, MIN_SIZE, bp.base.s.z),
            ElementType::PlaneZ => Vec3::new(bp.base.s.x, bp.base.s.y, MIN_SIZE),
        };
        let faces = FaceName::ALL.map(|name| {
            let flag = FaceFlags::from_bits_truncate(1 << (name as u8));
            Face::new(name, enabled.contains(flag), texture.clone())
        });
        Self {
            base: ObjectBase::new(
                format!("{prefix}{display_id}"),
                bp.base.p,
                s,
                Perms::ELEMENT,
            ),
            kind,
            color,
            faces,
            flags: ElementFlags::UV_SNAP | ElementFlags::SHADE,
        }
    }
}

/// The placement box. Its origin is always its position.
///
/// spec: tools 2.3
#[derive(Clone, Debug, PartialEq)]
pub struct Blueprint {
    /// The shared object fields (label "Blueprint", perms [`Perms::BLUEPRINT`]). The origin is
    /// kept equal to the position by the setters.
    pub base: ObjectBase,
}

impl Default for Blueprint {
    /// The scene default, which "Reset Blueprint to default" restores: position `(-2, 0, 2)`,
    /// size 1, rotation 0.
    ///
    /// spec: tools 2.3
    fn default() -> Self {
        Self {
            base: ObjectBase::new(
                "Blueprint",
                Vec3::new(-2.0, 0.0, 2.0),
                Vec3::ONE,
                Perms::BLUEPRINT,
            ),
        }
    }
}

/// The mesh kinds. The discriminants are the persisted `tp` values; an unknown value loads as
/// a cube primitive.
///
/// spec: tools 6.1, native 4.9
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum MeshKind {
    /// An imported mesh with stored geometry.
    Custom = 0,
    /// Primitive cube.
    Cube = 1,
    /// Primitive sphere.
    Sphere = 2,
    /// Primitive cylinder.
    Cylinder = 3,
    /// Primitive cone.
    Cone = 4,
    /// Primitive pyramid.
    Pyramid = 5,
    /// Primitive slope.
    Slope = 6,
    /// Primitive inner corner.
    CornerIn = 7,
    /// Primitive outer corner.
    CornerOut = 8,
    /// Primitive torus.
    Torus = 9,
    /// Primitive sprite (extruded from a PNG).
    Sprite = 10,
}

/// The triangle mesh of an imported mesh, in mesh-local units. Primitive geometry is generated
/// from the kind and shared; it is never stored.
///
/// spec: native 4.6.1, tools 6.5
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshGeometry {
    /// Vertex positions.
    pub vertices: Vec<Vec3>,
    /// One normal per vertex, or `None` to have them computed.
    pub normals: Option<Vec<Vec3>>,
    /// One UV per vertex, or empty.
    pub uvs: Vec<Vec2>,
    /// Vertex indices of the triangles of the whole mesh (sub-meshes concatenated).
    pub triangles: Vec<u32>,
}

/// A texture record (`TextureRecord` and `RawTexture` of native 4.5, 4.5.1): the image with
/// the provenance metadata that a round trip must keep. One type serves the texture list, the
/// main texture of a mesh and the texture of a submesh, so the image name `n` survives
/// everywhere. The image itself is a pure raster ([`Image`]).
///
/// spec: native 4.5, native 4.5.1, tools 5.1, arch 6 (cuboid-model)
#[derive(Clone, Debug)]
pub struct Texture {
    /// The informational image name (`tx.n`, for example `e_particle`); empty when unnamed.
    pub name: Label,
    /// Provenance (`fp`, `fn`, `fm`, `cs`); the default for textures of meshes.
    pub source: TextureSource,
    /// The pixels, shared between snapshots.
    pub image: Arc<Image>,
}

impl Texture {
    /// An unnamed texture over `image` with no source: an image assigned in memory.
    pub fn new(image: Arc<Image>) -> Self {
        Self {
            name: Label::default(),
            source: TextureSource::default(),
            image,
        }
    }
}

/// One material range of an imported mesh.
///
/// spec: native 4.6.3
#[derive(Clone, Debug)]
pub struct Submesh {
    /// Triangle indices of this sub-mesh.
    pub triangles: Vec<u32>,
    /// Material colour, RGBA in 0..=1.
    pub color: [f32; 4],
    /// Material texture.
    pub texture: Option<Texture>,
}

/// Bounds recorded when a mesh was imported.
///
/// spec: native 4.6.2
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct MeshBounds {
    /// Centre.
    pub center: Vec3,
    /// Full extent.
    pub size: Vec3,
}

/// A primitive or imported mesh object.
///
/// spec: tools 6.1, native 4.6, arch 6 (cuboid-model)
#[derive(Clone, Debug)]
pub struct Mesh {
    /// The shared object fields.
    pub base: ObjectBase,
    /// Custom or the primitive kind.
    pub kind: MeshKind,
    /// Imported geometry, or the shared primitive geometry for the kind.
    pub geometry: Arc<MeshGeometry>,
    /// Main texture; primitives hold a 128 x 128 paint texture. A change from the section 6
    /// sketch (`Option<Arc<Image>>`): the record keeps the image name `n` of native 4.5.1.
    pub texture: Option<Texture>,
    /// Sub-meshes of an imported mesh.
    pub submeshes: Arc<[Submesh]>,
    /// Bounds captured at import.
    pub bounds: MeshBounds,
    /// `1 / mean(import collider size)`, used by "block size".
    pub norm_factor: f32,
    /// `(minX - px, minY - py, maxZ - pz)` of the import bounds.
    pub import_offset: Vec3,
    /// Material colour that multiplies the texture; not persisted for primitives (QUIRK).
    /// An addition to the section 6 sketch: the mesh tint commands and the renderer need it.
    pub tint: [f32; 4],
}

/// Where a texture came from; persisted as `fp`, `fn`, `fm` and `cs`.
///
/// `path` and `signature` are `None` for images assigned in memory, and `Some("")` after a
/// reopen; the distinction is kept for byte-identical re-saves.
///
/// spec: tools 5.1, native 4.5
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TextureSource {
    /// Absolute source file path with `/` separators (`fp`).
    pub path: Option<String>,
    /// File name without extension (`fn`).
    pub file_name: Option<String>,
    /// Minecraft texture id or a `#label` shortcut (`fm`).
    pub mc_path: String,
    /// MD5 of the source file decoded with the ANSI code page (`cs`), compared for equality only.
    pub signature: Option<String>,
    /// Byte length of the source file, display only.
    pub file_size: u64,
}

/// One entry of the texture list.
///
/// spec: tools 5.1, native 4.5, arch 6 (cuboid-model)
#[derive(Clone, Debug)]
pub struct TextureEntry {
    /// The unique key faces refer to.
    pub label: Label,
    /// The entry's image with its name and provenance.
    pub texture: Texture,
}

/// The ordered texture list. The hidden "error" entry of the runtime atlas is not part of it.
///
/// spec: tools 5.1
#[derive(Clone, Debug, Default)]
pub struct TextureList {
    /// Entries in list order.
    pub entries: Vec<TextureEntry>,
}

impl TextureList {
    /// The texture label a new element's faces get: "texture" for an empty list, else the
    /// first entry unless it is "particle", else the second entry, else "particle".
    ///
    /// spec: tools 4.4
    pub fn default_face_label(&self) -> Label {
        match self.entries.as_slice() {
            [] => Label::from("texture"),
            [first, ..] if &*first.label != "particle" => first.label.clone(),
            [_, second, ..] => second.label.clone(),
            [_] => Label::from("particle"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_flag_values_follow_the_spec() {
        let singles = [
            FaceFlags::DOWN,
            FaceFlags::UP,
            FaceFlags::NORTH,
            FaceFlags::SOUTH,
            FaceFlags::WEST,
            FaceFlags::EAST,
        ];
        let bits: Vec<u8> = singles.iter().map(|f| f.bits()).collect();
        assert_eq!(bits, [1, 2, 4, 8, 16, 32]);
        assert_eq!(FaceFlags::ALL.bits(), 63);
        assert_eq!(FaceFlags::empty().bits(), 0);
    }

    #[test]
    fn persisted_enum_values_follow_the_spec() {
        assert_eq!(ElementType::PlaneZ as u8, 3);
        assert_eq!(MeshKind::Sprite as u8, 10);
        assert_eq!(MeshKind::CornerIn as u8, 7);
        let slots: Vec<u8> = FaceName::ALL.iter().map(|f| *f as u8).collect();
        assert_eq!(slots, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn object_flags_follow_the_table_of_tools_3_1() {
        assert!(Perms::ELEMENT.contains(Perms::RESTRICTABLE | Perms::DELETABLE));
        assert!(Perms::BLUEPRINT.contains(Perms::RESTRICTABLE));
        assert!(!Perms::BLUEPRINT.contains(Perms::DELETABLE));
        assert!(!Perms::PRIMITIVE.contains(Perms::RESTRICTABLE));
        assert!(Perms::PRIMITIVE.contains(Perms::GRID_SNAP_ON_SCALE));
        assert!(!Perms::IMPORTED_MESH.contains(Perms::GRID_SNAP_ON_SCALE));
        assert_eq!(Perms::MODEL_ORIGIN, Perms::MOVE);
    }

    #[test]
    fn one_texture_record_serves_the_list_the_mesh_and_the_submesh() {
        use cuboid_image::{Image, RgbaBuf};

        let image = Arc::new(Image::from_rgba(RgbaBuf::new(2, 2)));
        let mut texture = Texture::new(image.clone());
        assert_eq!(&*texture.name, "");
        assert_eq!(texture.source, TextureSource::default());
        texture.name = Label::from("e_particle");
        texture.source.mc_path = "block/stone".to_owned();

        let entry = TextureEntry {
            label: Label::from("particle"),
            texture: texture.clone(),
        };
        let submesh = Submesh {
            triangles: Vec::new(),
            color: [1.0; 4],
            texture: Some(texture),
        };
        assert_eq!(&*entry.texture.name, "e_particle");
        let kept = submesh.texture.as_ref().unwrap();
        assert_eq!(&*kept.name, "e_particle");
        assert_eq!(kept.source.mc_path, "block/stone");
        assert!(Arc::ptr_eq(&kept.image, &image), "the pixels are shared");
    }
}
