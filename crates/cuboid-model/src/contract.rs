//! Public contract stubs of this crate: cross-crate types and traits with their spec references
//! (W0.2).
//!
//! This file holds the document aggregate and the types that no single module of the crate
//! owns: the model and view properties, the display slots, the edit context and the small
//! vocabulary other crates share with the model (render preferences and requests, user
//! messages). Objects, lists, selection, journal, snapshots and the apply path live in their
//! modules next to the stubs W1.5 and W1.6 replace; `lib.rs` re-exports everything at the crate
//! root.

use std::any::Any;
use std::fmt;
use std::ops::{Index, IndexMut};
use std::sync::Arc;

use cuboid_num::EngineRng;
use cuboid_voxel::{VoxelWorld, WBox3};
use glam::{Quat, Vec3};

use crate::ids::Rev;
use crate::journal::ChangeJournal;
use crate::list::ObjList;
use crate::objects::{Blueprint, Element, Mesh, TextureList};
use crate::selection::Selection;

/// The model box: an axis-aligned box with six face handles. The default is the editing volume.
///
/// spec: tools 2.4
pub type ModelBox = WBox3;

/// The Minecraft model kind (`mc.t`).
///
/// spec: native 4.9
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum ModelKind {
    /// No kind.
    None = 0,
    /// Block model.
    Block = 1,
    /// Item model.
    Item = 2,
    /// Entity model.
    Entity = 3,
    /// OptiFine part model.
    Optifine = 4,
    /// Map.
    Map = 5,
}

/// The Staxel object kind (`st.t`).
///
/// spec: native 4.9
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum StaxelKind {
    /// No kind.
    None = 0,
    /// Tile.
    Tile = 1,
    /// Item.
    Item = 2,
    /// Accessory.
    Accessory = 3,
    /// Other.
    Other = 4,
}

/// General model settings (`mp.g`). Fresh values are the `Default`; the description is empty
/// there because its default is the expansion of the user's signature template.
///
/// Double quotes in `description` and `name` are replaced by U+201D whenever the strings are
/// assigned, including when a file is read.
///
/// spec: native 4.2.1, tools 2.2
#[derive(Clone, Debug, PartialEq)]
pub struct GeneralSettings {
    /// Model origin, the export pivot (`o`).
    pub origin: Vec3,
    /// The model box stored by the last handle drag (`mb`); `None` until then.
    pub model_box: Option<ModelBox>,
    /// Exclude elements from exports (`ee`).
    pub exclude_elements: bool,
    /// Exclude voxels from exports (`ev`).
    pub exclude_voxels: bool,
    /// Exclude meshes from exports (`em`).
    pub exclude_meshes: bool,
    /// Minify exported text (`me`).
    pub minify: bool,
    /// Restrict element rotations to Minecraft rules: single axis, 22.5 degree steps (`rer`).
    pub restrict_rotations: bool,
    /// Exclude hidden elements from exports (`ehe`).
    pub exclude_hidden: bool,
    /// Prevent texture bleeding by adjusting UVs (`pbt`).
    pub prevent_bleeding: bool,
    /// Avoid z-fighting with small random offsets (`pzf`).
    pub prevent_zfighting: bool,
    /// Description or signature (`d`).
    pub description: String,
    /// Model name (`n`).
    pub name: String,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            origin: Vec3::ZERO,
            model_box: None,
            exclude_elements: false,
            exclude_voxels: false,
            exclude_meshes: false,
            minify: false,
            restrict_rotations: true,
            exclude_hidden: false,
            prevent_bleeding: false,
            prevent_zfighting: false,
            description: String::new(),
            name: "Cubik Model".to_owned(),
        }
    }
}

/// Minecraft export settings (`mp.mc`).
///
/// spec: native 4.2.2, interop 2.7
#[derive(Clone, Debug, PartialEq)]
pub struct MinecraftSettings {
    /// Model kind (`t`).
    pub kind: ModelKind,
    /// Ambient occlusion flag of the exported model (`ao`).
    pub ambient_occlusion: bool,
    /// Include element names as comments (`uen`).
    pub element_names: bool,
    /// Automatic parent for vanilla display settings (`uvds`).
    pub auto_vanilla_parent: bool,
    /// Include display settings (`uds`).
    pub include_display: bool,
    /// Keep imported item overrides (`uos`).
    pub keep_overrides: bool,
    /// Entity: attach bone name (`eab`).
    pub entity_attach_bone: String,
    /// Entity: generated texture resolution (`etr`).
    pub entity_resolution: i32,
    /// Entity: texture offset X (`etx`).
    pub entity_offset_x: i32,
    /// Entity: texture offset Y (`ety`).
    pub entity_offset_y: i32,
    /// Entity: minimum texture width (`etw`).
    pub entity_min_width: i32,
    /// Entity: minimum texture height (`eth`).
    pub entity_min_height: i32,
    /// Entity: group all elements as one bone (`esb`).
    pub entity_same_bone: bool,
    /// Entity: reuse the texture for duplicate elements (`est`).
    pub entity_reuse_texture: bool,
    /// Entity: use element labels as cube names (`ecn`).
    pub entity_cube_names: bool,
    /// Voxel colour to block mapping table (`mt`).
    pub block_table: String,
    /// Minecraft parent model (`p`); never null.
    pub parent: String,
}

impl Default for MinecraftSettings {
    fn default() -> Self {
        Self {
            kind: ModelKind::Block,
            ambient_occlusion: true,
            element_names: true,
            auto_vanilla_parent: false,
            include_display: false,
            keep_overrides: false,
            entity_attach_bone: String::new(),
            entity_resolution: 16,
            entity_offset_x: 0,
            entity_offset_y: 0,
            entity_min_width: 64,
            entity_min_height: 64,
            entity_same_bone: true,
            entity_reuse_texture: true,
            entity_cube_names: false,
            block_table: "empty = air\ndefault = stone\n#5A9643 = grass\n#A46939 = dirt".to_owned(),
            parent: String::new(),
        }
    }
}

/// Stonehearth export settings (`mp.sh`).
///
/// spec: native 4.2.3
#[derive(Clone, Debug, PartialEq)]
pub struct StonehearthSettings {
    /// Placeable on ground (`pog`).
    pub on_ground: bool,
    /// Placeable on walls (`pow`).
    pub on_walls: bool,
    /// Value in gold (`v`).
    pub gold: i32,
    /// Rarity (`r`).
    pub rarity: String,
    /// Buyable (`b`).
    pub buyable: bool,
    /// Sellable (`s`).
    pub sellable: bool,
}

impl Default for StonehearthSettings {
    fn default() -> Self {
        Self {
            on_ground: false,
            on_walls: false,
            gold: 10,
            rarity: "common".to_owned(),
            buyable: true,
            sellable: true,
        }
    }
}

/// Staxel export settings (`mp.st`).
///
/// spec: native 4.2.3
#[derive(Clone, Debug, PartialEq)]
pub struct StaxelSettings {
    /// Object kind (`t`).
    pub kind: StaxelKind,
}

impl Default for StaxelSettings {
    fn default() -> Self {
        Self {
            kind: StaxelKind::Accessory,
        }
    }
}

/// Qubicle export settings (`mp.qb`).
///
/// spec: native 4.2.3
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QubicleSettings {
    /// Write RGB instead of BGR colour order (`rgb`).
    pub rgb: bool,
    /// Right-handed axes, inverted Z (`aiz`).
    pub inverted_z: bool,
}

/// Mesh (OBJ and similar) export settings (`mp.obj`).
///
/// spec: native 4.2.3, interop 2.7
#[derive(Clone, Debug, PartialEq)]
pub struct MeshExportSettings {
    /// Voxels: detailed mesh with coloured vertices (`gmv`).
    pub voxel_colored: bool,
    /// Voxels: detailed mesh with a texture palette (`gmt`).
    pub voxel_palette_texture: bool,
    /// Voxels: optimised mesh with a texture atlas (`omt`).
    pub voxel_atlas_texture: bool,
    /// Weld vertices at identical positions (`wv`). QUIRK: the polarity is inverted, true means
    /// no welding (interop 16.4).
    pub weld_vertices: bool,
    /// Merge all objects into one mesh (`mo`).
    pub merge_objects: bool,
    /// Add padding to voxel atlas textures (`pt`).
    pub atlas_padding: bool,
    /// Render fake ambient occlusion; also drives voxel ambient occlusion in the editor (`ao`).
    pub ambient_occlusion: bool,
    /// Smooth voxel vertices on export (`sv`).
    pub smooth_vertices: bool,
}

impl Default for MeshExportSettings {
    fn default() -> Self {
        Self {
            voxel_colored: true,
            voxel_palette_texture: false,
            voxel_atlas_texture: false,
            weld_vertices: true,
            merge_objects: false,
            atlas_padding: false,
            ambient_occlusion: true,
            smooth_vertices: false,
        }
    }
}

/// PNG render export settings (`mp.png`). Only `main_view` and `antialias` influence the render.
///
/// spec: native 4.2.3, app 15
#[derive(Clone, Debug, PartialEq)]
pub struct PngRenderSettings {
    /// Render type "GUI display preview" (`gp`).
    pub gui_preview: bool,
    /// Render type "Main view" (`mv`).
    pub main_view: bool,
    /// Anti-aliasing (`aa`).
    pub antialias: bool,
}

impl Default for PngRenderSettings {
    fn default() -> Self {
        Self {
            gui_preview: true,
            main_view: false,
            antialias: true,
        }
    }
}

/// The model-level settings (`mp`). The two empty groups `ht` and `tr` are written as `{}` by
/// the file writer and have no field.
///
/// spec: native 4.2, arch 6 (cuboid-model)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelProps {
    /// Absolute path of the document (`sp`).
    pub save_path: String,
    /// Last import or export path (`ep`).
    pub export_path: String,
    /// General settings (`g`).
    pub general: GeneralSettings,
    /// Minecraft settings (`mc`).
    pub minecraft: MinecraftSettings,
    /// Stonehearth settings (`sh`).
    pub stonehearth: StonehearthSettings,
    /// Staxel settings (`st`).
    pub staxel: StaxelSettings,
    /// Qubicle settings (`qb`).
    pub qubicle: QubicleSettings,
    /// Mesh export settings (`obj`).
    pub mesh_export: MeshExportSettings,
    /// PNG render settings (`png`).
    pub png: PngRenderSettings,
}

/// A camera as the file stores it.
///
/// spec: native 4.3.1
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CameraState {
    /// World position (`p`).
    pub pos: Vec3,
    /// World rotation (`r`).
    pub rot: Quat,
    /// Orthographic projection (`o`).
    pub ortho: bool,
    /// Half of the vertical view extent in world units (`os`).
    pub ortho_size: f32,
    /// Vertical field of view in degrees (`fv`).
    pub fov_deg: f32,
}

/// Grid steps and the four view cameras (`vp`). `uv_step` and `large_grid` are stored with
/// their defaults and are not applied on load.
///
/// spec: native 4.3, app 6.1, arch 6 (cuboid-model)
#[derive(Clone, Debug, PartialEq)]
pub struct ViewProps {
    /// Cameras in view order: Top (`ctl`), Side (`ctr`), Front (`cbl`), Main (`cbr`).
    pub cameras: [Option<CameraState>; 4],
    /// Grid step for mouse moves (`gm`).
    pub mouse_step: f32,
    /// Grid step for keyboard moves (`gk`).
    pub key_step: f32,
    /// Grid step for UV editing (`gu`).
    pub uv_step: f32,
    /// Displayed sub-grid step (`gd`).
    pub display_step: f32,
    /// Large grid mode (`gl`).
    pub large_grid: bool,
}

impl Default for ViewProps {
    fn default() -> Self {
        Self {
            cameras: [None; 4],
            mouse_step: 0.5,
            key_step: 0.125,
            uv_step: 0.5,
            display_step: 1.0,
            large_grid: false,
        }
    }
}

/// The eight Minecraft display slots in file order (`md` keys `tr`, `tl`, `fr`, `fl`, `u`, `h`,
/// `g`, `i`).
///
/// spec: native 4.8, app 9.4
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum DisplaySlotId {
    /// `thirdperson_righthand`.
    ThirdRight = 0,
    /// `thirdperson_lefthand`.
    ThirdLeft = 1,
    /// `firstperson_righthand`.
    FirstRight = 2,
    /// `firstperson_lefthand`.
    FirstLeft = 3,
    /// `gui`.
    Gui = 4,
    /// `head`.
    Head = 5,
    /// `ground`.
    Ground = 6,
    /// `fixed` (item frame).
    Fixed = 7,
}

/// One display slot. The helper angle of the panel is not stored (`a` is always 0).
///
/// spec: native 4.8, app 9.4
#[derive(Clone, Debug, PartialEq)]
pub struct DisplaySlot {
    /// Name of the preset the slot was set from (`n`).
    pub preset: String,
    /// The preset can be removed (`c`).
    pub removable: bool,
    /// Slot enabled, exported (`e`).
    pub enabled: bool,
    /// Rotation in degrees, Minecraft axis order (`r`).
    pub rotation: Vec3,
    /// Translation in 1/16 block units (`t`).
    pub translation: Vec3,
    /// Scale (`s`); a zero component is written as 0.0001.
    pub scale: Vec3,
}

impl Default for DisplaySlot {
    fn default() -> Self {
        Self {
            preset: "Default".to_owned(),
            removable: true,
            enabled: false,
            rotation: Vec3::ZERO,
            translation: Vec3::ZERO,
            scale: Vec3::ONE,
        }
    }
}

/// The eight display slots, indexed by [`DisplaySlotId`].
///
/// spec: native 4.8, arch 6 (cuboid-model)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplaySlots {
    /// The slots in [`DisplaySlotId`] order.
    pub slots: [DisplaySlot; 8],
}

impl Index<DisplaySlotId> for DisplaySlots {
    type Output = DisplaySlot;

    fn index(&self, id: DisplaySlotId) -> &DisplaySlot {
        &self.slots[id as usize]
    }
}

impl IndexMut<DisplaySlotId> for DisplaySlots {
    fn index_mut(&mut self, id: DisplaySlotId) -> &mut DisplaySlot {
        &mut self.slots[id as usize]
    }
}

/// One case of the Minecraft item overrides, kept after a JSON import. Not stored in `.cubik`
/// and captured by no undo category.
///
/// spec: interop 7.8, interop 8.3 (step 9), tools 2.5
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverrideCase {
    /// The override model.
    pub model: String,
    /// Predicate `angle`.
    pub angle: Option<f32>,
    /// Predicate `blocking`.
    pub blocking: Option<i32>,
    /// Predicate `broken`.
    pub broken: Option<i32>,
    /// Predicate `cast`.
    pub cast: Option<i32>,
    /// Predicate `damage`.
    pub damage: Option<f32>,
    /// Predicate `damaged`.
    pub damaged: Option<i32>,
    /// Predicate `lefthanded`.
    pub lefthanded: Option<i32>,
    /// Predicate `pull`.
    pub pull: Option<f32>,
    /// Predicate `pulling`.
    pub pulling: Option<i32>,
    /// Predicate `time`.
    pub time: Option<f32>,
}

/// What a file loader keeps to write unchanged parts back byte for byte. The model does not
/// interpret it: `cuboid-fmt-cubik` puts its payload, spans and per-record revisions behind
/// `data` and downcasts it on save.
///
/// spec: native 8.3, arch 12.3 (Retention)
pub struct Retained {
    /// Memory held, for the retention cap (default 512 MiB).
    pub bytes: usize,
    /// The loader's own retention data.
    pub data: Box<dyn Any + Send + Sync>,
}

impl fmt::Debug for Retained {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Retained")
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

/// The preferences that change what the views draw and how elements are coloured. The edit
/// environment carries them to the operations; the draw-list compiler reads them.
///
/// spec: app 10.2, app 8.4, app 8.9, app 5.9
#[derive(Clone, Debug, PartialEq)]
pub struct RenderPrefs {
    /// "Show elements color" (`useElementsColor`).
    pub use_elements_color: bool,
    /// "Emulate elements tint colors" (`emulateElementTintColor`).
    pub emulate_tint_colors: bool,
    /// "Display marker at the center of 3D space" (`centerPoint`).
    pub center_marker: bool,
    /// "Show hidden elements as white transparent box" (`renderHiddenElements`).
    pub hidden_as_box: bool,
    /// "Show hidden elements in main view" (`renderHiddenElementsMainView`).
    pub hidden_in_main_view: bool,
    /// "Enable anti-aliasing" (`renderAntialiasing`).
    pub antialiasing: bool,
    /// "Enable shadows for voxels" (`renderShadows`).
    pub shadows: bool,
    /// Wireframe mode, a session flag that is not saved.
    pub wireframe: bool,
}

impl Default for RenderPrefs {
    fn default() -> Self {
        Self {
            use_elements_color: true,
            emulate_tint_colors: true,
            center_marker: false,
            hidden_as_box: false,
            hidden_in_main_view: false,
            antialiasing: false,
            shadows: false,
            wireframe: false,
        }
    }
}

/// The settings an operation reads besides the document.
///
/// spec: arch 6 (cuboid-model), tools 3.4
#[derive(Clone, Debug)]
pub struct EditEnv {
    /// "Restrict rotations" of the model (`g.rer`) as the rotation setters see it.
    pub restrict_rotations: bool,
    /// Rendering preferences.
    pub render: RenderPrefs,
    /// The ANSI code page for texture signatures; Windows-1252 off Windows.
    pub code_page: u16,
}

/// What every operation gets: the document, the selection, the environment and the random
/// generator it must draw from (tools 16.5).
///
/// spec: arch 6 (cuboid-model), arch 7 (Randomness)
pub struct EditCtx<'a> {
    /// The document.
    pub doc: &'a mut Document,
    /// The selection.
    pub sel: &'a mut Selection,
    /// Settings.
    pub env: &'a EditEnv,
    /// The injected, seeded generator.
    pub rng: &'a mut EngineRng,
}

/// The document: a flat set of lists and small aggregates. Small aggregates sit behind `Arc`
/// and change through `Arc::make_mut`, so a clone is a snapshot that costs one pointer copy per
/// object and shares everything untouched. The UI thread is the only writer.
///
/// spec: tools 2.1, arch 6 (cuboid-model), arch 8.1
#[derive(Clone, Debug)]
pub struct Document {
    /// Model properties.
    pub props: Arc<ModelProps>,
    /// Grid steps and cameras.
    pub view: Arc<ViewProps>,
    /// The placement box.
    pub blueprint: Blueprint,
    /// The live model box.
    pub model_box: ModelBox,
    /// Elements in list order.
    pub elements: ObjList<Element>,
    /// Meshes in list order.
    pub meshes: ObjList<Mesh>,
    /// The texture list.
    pub textures: TextureList,
    /// The voxel world, if set up.
    pub voxels: Option<VoxelWorld>,
    /// The display slots.
    pub display: Arc<DisplaySlots>,
    /// Minecraft item overrides.
    pub overrides: Arc<[OverrideCase]>,
    /// Retained file data for byte-identical re-save.
    pub retained: Option<Arc<Retained>>,
    /// What changed since the journal was last cleared.
    pub journal: ChangeJournal,
    /// Changes with every edit of the document.
    pub revision: Rev,
}

impl Document {
    /// A fresh document, as at start-up: default properties and view, the blueprint at its
    /// scene default, the editing volume as model box, empty lists, no voxels. `textures` are the
    /// two default textures `particle` and `texture`, which the application supplies from its
    /// own pictures.
    ///
    /// File > New is not this function alone: it keeps the current blueprint and the camera
    /// poses (tools 2.3, app 1.3), so the caller carries those over.
    ///
    /// spec: app 1.3, tools 2.3, native 4.5.2
    pub fn new(textures: TextureList) -> Document {
        Document {
            props: Arc::default(),
            view: Arc::default(),
            blueprint: Blueprint::default(),
            model_box: ModelBox::EDITING_VOLUME,
            elements: ObjList::default(),
            meshes: ObjList::default(),
            textures,
            voxels: None,
            display: Arc::default(),
            overrides: Arc::from([]),
            retained: None,
            journal: ChangeJournal::default(),
            revision: Rev::next(),
        }
    }
}

/// An immutable view of the document for workers, taken on the UI thread after the cameras were
/// captured into the view properties. Exporters and the save path read the document through it.
///
/// spec: arch 6 (cuboid-io), arch 11.2
pub type DocSnapshot = Document;

/// What a PNG render shows.
///
/// spec: app 15
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RenderKind {
    /// "GUI display preview": the model posed by its `gui` slot, orthographic half-height 8.
    GuiPreview,
    /// "Main view": the Main camera of the snapshot at 16:9.
    MainView,
}

/// A PNG render an exporter asks for (model render, resource-pack icon). The application
/// answers it on the UI thread through the renderer.
///
/// spec: app 15, arch 6 (cuboid-io), arch 11.2
#[derive(Clone, Debug)]
pub struct RenderRequest {
    /// The model to draw.
    pub snapshot: Arc<DocSnapshot>,
    /// What to draw.
    pub kind: RenderKind,
    /// Size of the final picture: 512 x 512 for a GUI preview, 1920 x 1080 for the Main view,
    /// 128 x 128 for `pack.png`. With anti-aliasing the target is twice as large.
    pub size: (u32, u32),
    /// Render at twice the size and downscale.
    pub antialias: bool,
    /// Tint emulation and the other colour preferences ("Show elements color" is off for
    /// renders).
    pub prefs: RenderPrefs,
}

/// The box kinds of the message box.
///
/// spec: menus 8.3
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum MessageKind {
    /// Check.
    Check,
    /// Question.
    Question,
    /// Warning.
    Warning,
    /// Error.
    Error,
    /// Information.
    Information,
    /// Help.
    Help,
    /// Parameters.
    Parameters,
    /// Idea.
    Idea,
    /// Serial.
    Serial,
    /// System.
    System,
    /// Lock.
    Lock,
    /// Unlock.
    Unlock,
    /// Wait.
    Wait,
    /// Sketchfab.
    Sketchfab,
}

/// A message for the user: a box kind, the original's title and text, and the button labels.
/// Errors map to the original's titles and lead sentences through values of this type; the
/// detail text is the port's own.
///
/// spec: arch 16, menus 8.4, menus 8.7
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserMessage {
    /// Icon and sound.
    pub kind: MessageKind,
    /// Title.
    pub title: String,
    /// Message text; may contain line feeds.
    pub text: String,
    /// One to three button labels; empty means the standard labels of the kind ("OK", or "Yes"
    /// and "No" for a question).
    pub buttons: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn document_is_send_and_sync() {
        assert_send_sync::<Document>();
        assert_send_sync::<crate::snapshot::Snapshot>();
        assert_send_sync::<RenderRequest>();
    }

    #[test]
    fn fresh_model_properties_follow_native_4_2() {
        let props = ModelProps::default();
        assert_eq!(props.general.name, "Cubik Model");
        assert!(props.general.restrict_rotations);
        assert!(props.general.model_box.is_none());
        assert_eq!(props.minecraft.kind, ModelKind::Block);
        assert!(props.minecraft.ambient_occlusion && props.minecraft.element_names);
        assert_eq!(props.minecraft.entity_resolution, 16);
        assert_eq!(
            (
                props.minecraft.entity_min_width,
                props.minecraft.entity_min_height
            ),
            (64, 64)
        );
        assert!(
            props
                .minecraft
                .block_table
                .starts_with("empty = air\ndefault = stone\n")
        );
        assert_eq!(props.stonehearth.gold, 10);
        assert_eq!(props.stonehearth.rarity, "common");
        assert_eq!(props.staxel.kind, StaxelKind::Accessory);
        assert!(props.mesh_export.voxel_colored && props.mesh_export.weld_vertices);
        assert!(props.png.gui_preview && props.png.antialias && !props.png.main_view);
    }

    #[test]
    fn fresh_view_properties_follow_native_4_3() {
        let view = ViewProps::default();
        assert!(view.cameras.iter().all(Option::is_none));
        assert_eq!(
            (
                view.mouse_step,
                view.key_step,
                view.uv_step,
                view.display_step
            ),
            (0.5, 0.125, 0.5, 1.0)
        );
        assert!(!view.large_grid);
    }

    #[test]
    fn fresh_display_slots_follow_native_4_8() {
        let display = DisplaySlots::default();
        let gui = &display[DisplaySlotId::Gui];
        assert_eq!(gui.preset, "Default");
        assert!(gui.removable && !gui.enabled);
        assert_eq!(gui.scale, Vec3::ONE);
        assert_eq!(DisplaySlotId::Fixed as usize, display.slots.len() - 1);
    }

    #[test]
    fn render_preference_defaults_follow_app_10_2() {
        let prefs = RenderPrefs::default();
        assert!(prefs.use_elements_color && prefs.emulate_tint_colors);
        assert!(!prefs.center_marker && !prefs.hidden_as_box && !prefs.hidden_in_main_view);
        assert!(!prefs.antialiasing && !prefs.shadows && !prefs.wireframe);
    }
}
