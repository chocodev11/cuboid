//! Plain data types of this crate shared by the dispatcher, the registry and the menu tables:
//! gates, held keys, held bindings, [`ActionId`] and the menu definition types.
//!
//! Behaviour lives in the sibling modules: key names in `keys`, chords and binding strings in
//! `binding`, the dispatcher in `dispatch`, the held map in `held`, the action registry in
//! `registry` and the menu tables in `menus`.

use bitflags::bitflags;
use rustc_hash::FxHashSet;

use crate::EngineKey;

bitflags! {
    /// Which bar menus are enabled; the dispatcher lets a binding fire only when its menu is
    /// enabled or the binding is always active.
    ///
    /// spec: menus 1.6, menus 6.2
    #[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
    pub struct MenuMask: u8 {
        /// File.
        const FILE = 1;
        /// Edit.
        const EDIT = 2;
        /// Objects (enabled iff the primary selected object is an element or a mesh).
        const OBJECTS = 4;
        /// Recover.
        const RECOVER = 8;
        /// Views.
        const VIEWS = 16;
        /// Tools.
        const TOOLS = 32;
        /// Window.
        const WINDOW = 64;
        /// Help.
        const HELP = 128;
    }
}

impl MenuMask {
    /// The bit of a bar menu; empty for popups and submenus, which register no bindings.
    pub const fn of(menu: MenuId) -> Self {
        match menu {
            MenuId::File => Self::FILE,
            MenuId::Edit => Self::EDIT,
            MenuId::Objects => Self::OBJECTS,
            MenuId::Recover => Self::RECOVER,
            MenuId::Views => Self::VIEWS,
            MenuId::Tools => Self::TOOLS,
            MenuId::Window => Self::WINDOW,
            MenuId::Help => Self::HELP,
            _ => Self::empty(),
        }
    }
}

/// The state of the application that gates shortcut dispatch.
///
/// spec: menus 6.2 (step 1 and step 5), menus 6.7, arch 10.3
#[derive(Copy, Clone, Debug)]
pub struct DispatchGate {
    /// The global modal flag: a message box, a modal dialog, the About box or the file browser.
    pub modal: bool,
    /// A UI control holds keyboard selection; only works-in-UI bindings fire then.
    pub ui_holds_selection: bool,
    /// The enabled bar menus.
    pub enabled_menus: MenuMask,
}

/// The keys held right now, from the shell's engine key events.
///
/// spec: menus 6.4
#[derive(Clone, Debug, Default)]
pub struct HeldKeys(FxHashSet<EngineKey>);

impl HeldKeys {
    /// Whether `k` is held.
    pub fn is_down(&self, k: EngineKey) -> bool {
        self.0.contains(&k)
    }

    /// Records a key press (`down`) or release.
    pub fn set(&mut self, k: EngineKey, down: bool) {
        if down {
            self.0.insert(k);
        } else {
            self.0.remove(&k);
        }
    }
}

/// The held bindings of the input map: a binding is held when any of its keys is held.
///
/// spec: menus 6.4, tools 8.2, app 5.6
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum HeldBinding {
    /// Hold for rotate mode (customisable).
    ModeRotate,
    /// Hold for scale mode (customisable).
    ModeScale,
    /// Hold for clone mode (customisable).
    ModeClone,
    /// Pan the view while held and dragging (customisable).
    PanView,
    /// Focus the views on the object under the pointer (customisable).
    Focus,
    /// Toggle-select on click: Ctrl or Command, either side.
    ModifySelection,
    /// Add to the selection, range-select on list rows: Shift, either side.
    ModifySelectionMultiple,
    /// Orbit the perspective view: Alt or Ctrl, either side.
    RotateView,
    /// Right arrow.
    MoveRight,
    /// Left arrow.
    MoveLeft,
    /// Up arrow.
    MoveUp,
    /// Down arrow.
    MoveDown,
    /// Return or keypad Enter.
    Submit,
    /// Escape.
    Cancel,
}

impl HeldBinding {
    /// Every binding in declaration order.
    pub const ALL: [Self; 14] = [
        Self::ModeRotate,
        Self::ModeScale,
        Self::ModeClone,
        Self::PanView,
        Self::Focus,
        Self::ModifySelection,
        Self::ModifySelectionMultiple,
        Self::RotateView,
        Self::MoveRight,
        Self::MoveLeft,
        Self::MoveUp,
        Self::MoveDown,
        Self::Submit,
        Self::Cancel,
    ];
}

/// Every action a menu row, popup row, hidden binding or toolbar button can run. Names that
/// the original reuses across handlers are prefixed with their handler (`Element...`,
/// `Mesh...`); `ACTIONS` maps each id to the original action name.
///
/// The actions of the tool panels (ui 6-8) that are not shared with a menu row are not covered
/// yet; only W1.9 or the integration role may add them to this enum and to `ACTIONS`.
///
/// spec: menus 2, menus 3, menus 5, arch 10.2, arch 6 (cuboid-actions)
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ActionId {
    // File (menus 2.1, 5.1)
    /// File > New file (Ctrl+N).
    NewFile,
    /// File > Open (Ctrl+O).
    Open,
    /// File > Merge (Ctrl+M).
    Merge,
    /// File > Save (Ctrl+S).
    Save,
    /// File > Save As...
    SaveAs,
    /// File > Import (Ctrl+I).
    Import,
    /// File > Export (Ctrl+E).
    Export,
    /// File > Exit.
    Exit,
    /// File > Open recent files > Clear recent file list.
    ClearRecentFiles,
    // Edit (menus 2.2, 5.3)
    /// Edit > Undo (Ctrl+Z).
    Undo,
    /// Edit > Redo (Ctrl+Y).
    Redo,
    /// Edit > Cut (Ctrl+X).
    Cut,
    /// Edit > Copy (Ctrl+C).
    Copy,
    /// Edit > Paste (Ctrl+V).
    Paste,
    /// Edit > Delete (Ctrl+Del) and the Delete rows of the popups; no confirmation.
    Delete,
    /// The hidden Del binding and the Selection toolbar Delete button; asks first.
    DeleteConfirm,
    /// Edit > Select by name (Ctrl+F, hidden F3).
    SelectName,
    /// Edit > Select all (Ctrl+A).
    SelectAllElements,
    /// Edit > Deselect (Ctrl+D).
    SelectNone,
    /// Edit > Select Inverse (Ctrl+Shift+I).
    SelectInverse,
    /// Edit > Model Properties (Ctrl+P).
    ModelProperties,
    /// Edit > Set Model Origin (Ctrl+K).
    ModelOrigin,
    /// Edit > Set Model Blueprint (Ctrl+B).
    ModelBlueprint,
    /// Edit > Rotate model +90 degrees on X axis.
    RotateModelX,
    /// Edit > Rotate model +90 degrees on Y axis.
    RotateModelY,
    /// Edit > Rotate model +90 degrees on Z axis.
    RotateModelZ,
    // Objects menu and the "objects" handler (menus 2.3, 5.4)
    /// Objects > Next (PgUp).
    Next,
    /// Objects > Previous (PgDown).
    Previous,
    /// Objects > Element Properties... (Shift+P).
    ElementProperties,
    /// Objects > Element Faces... (Shift+F).
    ElementFaces,
    /// Objects > Element UV Mapping... (Shift+U).
    ElementUv,
    /// Objects > Rename (Shift+N, hidden F2) and the Mesh submenu.
    Rename,
    /// Objects > Move (Shift+M).
    Move,
    /// Objects > Scale (Shift+S).
    Scale,
    /// Objects > Clone (Shift+C) and the Mesh submenu.
    Clone,
    /// Objects > Toggle Visibility (Shift+V).
    ToggleVisibility,
    /// Objects > Toggle Lock (Shift+L).
    ToggleLock,
    /// Objects > Focus Lock (Shift+K).
    FocusLock,
    /// Objects > Reset Origin/Rotation (Shift+R).
    ResetRotation,
    /// Mesh submenu > Hide.
    Hide,
    /// Mesh submenu > Show.
    Show,
    /// Mesh submenu > Lock.
    Lock,
    /// Mesh submenu > Unlock.
    Unlock,
    /// Transform submenus > Set Position...
    SetPosition,
    /// Transform submenus > Set size...
    SetSize,
    /// Transform submenus > Apply rescale.
    ApplyRescale,
    /// Element transform submenu > Inflate...
    Inflate,
    /// Transform submenus > Rotate 90 degrees around X.
    RotateX,
    /// Transform submenus > Rotate 90 degrees around Y.
    RotateY,
    /// Transform submenus > Rotate 90 degrees around Z.
    RotateZ,
    /// Transform submenus > Randomize...
    Randomize,
    /// Transform submenus > Center on X axis.
    CenterX,
    /// Transform submenus > Center on Y axis.
    CenterY,
    /// Transform submenus > Center on Z axis.
    CenterZ,
    // Recover (menus 2.4, 5.6)
    /// Recover > Show all Elements (Ctrl+Shift+S).
    ShowAll,
    /// Recover > Unlock all Elements (Ctrl+Shift+U).
    UnlockAll,
    /// Recover > Reset Blueprint to default (Ctrl+Shift+P).
    ResetBlueprint,
    /// Recover > Reset Views to default (Ctrl+Shift+C).
    ResetCamera,
    /// Recover > Get back Far Elements (Ctrl+Shift+F).
    GetBackFarElements,
    // Views (menus 2.5, 5.7)
    /// Views > Zoom+ (keypad plus).
    ZoomPlus,
    /// Views > Zoom- (keypad minus).
    ZoomMinus,
    /// Views > Wireframe mode (Shift+W).
    WireframeMode,
    /// Views > Grid settings (Shift+G).
    GridSettings,
    /// Views > Load background for Up view... (original LoadBgTL).
    LoadBgUp,
    /// Views > Load background for West view... (original LoadBgTR).
    LoadBgWest,
    /// Views > Load background for South view... (original LoadBgBL).
    LoadBgSouth,
    /// Views > Load background for all views... (Shift+A).
    LoadBgAll,
    /// Views > Toggle backgrounds size (Shift+T).
    ToggleBgSize,
    /// Views > Delete backgrounds (Shift+B).
    DeleteBgAll,
    // Tools (menus 2.6, 5.8)
    /// Tools > Element tools (F5).
    ElementTools,
    /// Tools > Voxel tools (F6).
    VoxelTools,
    /// Tools > Paint tools (F7).
    PaintTools,
    /// Tools > Color tools (F8).
    ColorTools,
    /// Tools > Color Palette (F9).
    ColorPalette,
    /// Tools > Elements List (F10).
    ElementsList,
    /// Tools > Elements Material (F11).
    ElementsMaterial,
    /// Tools > Gizmos tools (F12).
    GizmosTools,
    /// Tools > Share on Sketchfab.
    ShareSketchfab,
    /// Tools > Browse the application folder (original CubikFolder).
    BrowseAppFolder,
    /// Tools > Browse the data folder (original CubikDataFolder).
    BrowseDataFolder,
    /// Tools > Browse last save/export Folder (original LastExportFolder).
    BrowseLastExportFolder,
    /// Tools > Preferences...
    Preferences,
    // Window (menus 2.7, 5.9)
    /// Window > Move Managers on Left (Ctrl+Keypad1).
    MoveManagersOnLeft,
    /// Window > Move Managers on Right (Ctrl+Keypad3).
    MoveManagersOnRight,
    /// Window > Close Managers (Ctrl+Keypad2).
    HideManagers,
    /// Window > Close Helpers (Ctrl+Keypad8).
    HideHelpers,
    /// Window > Close All (Ctrl+Keypad0).
    HideAll,
    // Help (menus 2.8, 5.10); the registration row is omitted
    /// Help > About...
    About,
    /// Help > Support... (F1).
    Support,
    /// Help > Details... (original CubikPage).
    Details,
    /// Help > Changelog...
    Changelog,
    /// Help > the project site (original BDcraftPage).
    ProjectSite,
    // Meshes popup (menus 3.1, 5.4)
    /// Mesh > Set Color...
    SelectMeshTint,
    /// Mesh > Apply Color.
    ApplyMeshTint,
    /// Mesh > Move to top (meshes handler, original MoveToTop).
    MeshMoveToTop,
    /// Mesh > Move to bottom (meshes handler, original MoveToBottom).
    MeshMoveToBottom,
    /// Meshes popup > Convert to voxels...
    StartVoxelization,
    /// Mesh texture > Load texture...
    LoadMeshTexture,
    /// Mesh texture > Save texture...
    SaveMeshTexture,
    /// Mesh texture > Remove alpha.
    RemoveAlpha,
    /// Mesh texture > Clear texture.
    ClearMeshTexture,
    /// Mesh copy > Texture.
    CopyMeshTexture,
    /// Mesh copy > Mesh color.
    CopyMeshColor,
    /// Mesh paste > Texture.
    PasteMeshTexture,
    /// Mesh paste > Mesh color.
    PasteMeshColor,
    // Elements popup (menus 3.2, 5.5)
    /// Element > Rename... (elements handler, original Rename).
    ElementRename,
    /// Element > Set Color... (original Color).
    ElementColor,
    /// Element > Apply random Color (original ColorRandom).
    ElementColorRandom,
    /// Element > Move to top (original MoveToTop).
    ElementMoveToTop,
    /// Element > Move to bottom (original MoveToBottom).
    ElementMoveToBottom,
    /// Element > Hide (elements handler).
    ElementHide,
    /// Element > Show (elements handler).
    ElementShow,
    /// Element > Lock (elements handler).
    ElementLock,
    /// Element > Unlock (elements handler).
    ElementUnlock,
    /// Element > Clone (elements handler).
    ElementClone,
    /// Element > Delete (elements handler).
    ElementDelete,
    /// Faces > Enable > Up.
    FacesEnableUp,
    /// Faces > Enable > Down.
    FacesEnableDown,
    /// Faces > Enable > North.
    FacesEnableNorth,
    /// Faces > Enable > South.
    FacesEnableSouth,
    /// Faces > Enable > West.
    FacesEnableWest,
    /// Faces > Enable > East.
    FacesEnableEast,
    /// Faces > Enable > All.
    FacesEnableAll,
    /// Faces > Disable > Up.
    FacesDisableUp,
    /// Faces > Disable > Down.
    FacesDisableDown,
    /// Faces > Disable > North.
    FacesDisableNorth,
    /// Faces > Disable > South.
    FacesDisableSouth,
    /// Faces > Disable > West.
    FacesDisableWest,
    /// Faces > Disable > East.
    FacesDisableEast,
    /// Faces > Disable > All.
    FacesDisableAll,
    /// Textures > texture (default).
    TexDefault,
    /// Textures > side + up + down.
    TexSud,
    /// Textures > side + cap.
    TexSc,
    /// Textures > front + side + cap.
    TexFsc,
    /// Textures > down + up + north + south + west + east.
    TexDunswe,
    /// Textures > Customize...
    TexCustom,
    /// UV Mapping > Enable Snap.
    UvSnapEnable,
    /// UV Mapping > Disable Snap.
    UvSnapDisable,
    /// UV Mapping > Enable Symmetry.
    UvSnapMirrorEnable,
    /// UV Mapping > Disable Symmetry.
    UvSnapMirrorDisable,
    /// UV Mapping > Apply Default UV: a dead row that does nothing.
    UvApplyDefault,
    /// Tint > Top.
    TintUp,
    /// Tint > Down.
    TintDown,
    /// Tint > Sides.
    TintSides,
    /// Tint > All.
    TintAll,
    /// Tint > None.
    TintNone,
    /// Culling > Top.
    CullingUp,
    /// Culling > Down.
    CullingDown,
    /// Culling > Sides.
    CullingSides,
    /// Culling > All.
    CullingAll,
    /// Culling > None.
    CullingNone,
    /// Render > Enable shading.
    ShadeEnable,
    /// Render > Disable shading.
    ShadeDisable,
    /// Render > Outer normals.
    InvertFalse,
    /// Render > Inner normals.
    InvertTrue,
    /// Element copy > Faces visibility.
    CopyFaces,
    /// Element copy > Textures.
    CopyTextures,
    /// Element copy > UV Mapping.
    CopyUv,
    /// Element copy > Tint.
    CopyTint,
    /// Element copy > Culling.
    CopyCulling,
    /// Element copy > Shading.
    CopyShade,
    /// Element paste > Faces visibility.
    PasteFaces,
    /// Element paste > Textures.
    PasteTextures,
    /// Element paste > UV Mapping.
    PasteUv,
    /// Element paste > Tint: a dead row that does nothing.
    PasteTint,
    /// Element paste > Culling.
    PasteCulling,
    /// Element paste > Shading.
    PasteShade,
    // Texture and Material popups (menus 3.3, 3.4, 5.11)
    /// Texture popup > Load texture...
    LoadTexture,
    /// Texture popup > Save texture...
    SaveTexture,
    /// Texture popup > Set as shortcut.
    SetAsShortcut,
    /// Texture popup > Edit label...
    SetTextureLabel,
    /// Texture popup > Rename texture...
    RenameTexture,
    /// Texture popup > Apply to selection.
    ApplyTextureToSelection,
    /// Texture popup > Edit texture...
    EditTexture,
    /// Texture popup > Empty texture.
    EmptyTexture,
    /// Texture popup > Delete texture.
    DeleteTexture,
    /// The "+" button of the Elements Material panel.
    AddTexture,
    /// Material popup > Add missing textures (original NewMat_Auto).
    NewMatAuto,
    /// Material popup > Remove unused textures (original NewMat_Optim).
    NewMatOptim,
    /// Material popup > Delete all textures (original NewMat_Empty).
    NewMatEmpty,
    /// Replace textures list > Texture (default).
    NewMatTexture,
    /// Replace textures list > Side + Up + Down.
    NewMatSud,
    /// Replace textures list > Side + Cap.
    NewMatSc,
    /// Replace textures list > Front + Side + Cap.
    NewMatFsc,
    /// Replace textures list > Down + Up + North + South + West + East.
    NewMatDunswe,
    /// Material popup > Create atlas, blank textures.
    CreateAtlasBlank,
    /// Material popup > Create atlas, keep textures.
    CreateAtlasTextures,
    // Voxel popup (menus 3.6, 5.12)
    /// Voxel popup > Setup (original Setup).
    VoxelSetup,
    /// Convert to elements > 1 element per voxel (cubes).
    ConvertToElementsCubes,
    /// Convert to elements > 1+ elements per color (palette).
    ConvertToElements,
    /// Convert to elements > 1+ elements per surface (atlas).
    ConvertToElementsAtlas,
    /// Voxel transform > Upscale x2 (original Upscale).
    VoxelUpscale,
    /// Voxel transform > Downscale /2 (original Downscale).
    VoxelDownscale,
    /// Voxel transform > Flip on X axis.
    VoxelFlipX,
    /// Voxel transform > Flip on Y axis.
    VoxelFlipY,
    /// Voxel transform > Flip on Z axis.
    VoxelFlipZ,
    /// Voxel transform > Rotate 90 degrees around X axis.
    VoxelRotateX,
    /// Voxel transform > Rotate 90 degrees around Y axis.
    VoxelRotateY,
    /// Voxel transform > Rotate 90 degrees around Z axis.
    VoxelRotateZ,
    /// Voxel transform > Color all (original ResetColor).
    VoxelResetColor,
    /// Voxel transform > Fill volume (original FillVolume).
    VoxelFillVolume,
    /// Voxel transform > Hollow volume (original HollowVolume).
    VoxelHollowVolume,
    /// Voxel transform > Crop voxels outside model box (original CropVoxelsOutside).
    VoxelCropOutside,
    /// Voxel blueprint > Create voxels.
    FillBlueprint,
    /// Voxel blueprint > Delete voxels.
    ClearBlueprint,
    /// Voxel blueprint > Copy voxels.
    CopyInBlueprint,
    /// Voxel blueprint > Paste voxels.
    PasteAtBlueprint,
    /// Voxel blueprint > Flip voxels on X axis.
    FlipXBlueprint,
    /// Voxel blueprint > Flip voxels on Y axis.
    FlipYBlueprint,
    /// Voxel blueprint > Flip voxels on Z axis.
    FlipZBlueprint,
    /// Voxel popup > Clear voxels.
    ClearVoxels,
    /// Voxel popup > Reset.
    ResetVoxels,
    // Palette popup (menus 3.7, 5.13)
    /// Palette popup > RGB palette (default).
    DefaultPalette,
    /// Palette popup > HSL palette.
    HslPalette,
    /// Palette popup > JMP palette.
    JmpPalette,
    /// Palette popup > Generic16 palette.
    Generic16Palette,
    /// Palette popup > Pico8 palette.
    Pico8Palette,
    /// Palette popup > EDG32 palette.
    Edg32Palette,
    /// Palette popup > EDG16 palette.
    Edg16Palette,
    /// Palette popup > Load new palette.
    LoadPalette,
    /// Palette popup > Save palette.
    SavePalette,
    // Create buttons of the left tool strip (menus 5.14)
    /// Element create > Add Cube.
    CreateElementCube,
    /// Element create > Add Plane on X axis.
    CreateElementPlaneX,
    /// Element create > Add Plane on Y axis.
    CreateElementPlaneY,
    /// Element create > Add Plane on Z axis.
    CreateElementPlaneZ,
    /// Primitive create > Add Cube primitive.
    CreateMeshCube,
    /// Primitive create > Add Sphere primitive.
    CreateMeshSphere,
    /// Primitive create > Add Cylinder primitive.
    CreateMeshCylinder,
    /// Primitive create > Add Cone primitive.
    CreateMeshCone,
    /// Primitive create > Add Pyramid primitive.
    CreateMeshPyramid,
    /// Primitive create > Add Slope primitive.
    CreateMeshSlope,
    /// Primitive create > Add Corner-In primitive.
    CreateMeshCornerIn,
    /// Primitive create > Add Corner-Out primitive.
    CreateMeshCornerOut,
    /// Primitive create > Add Torus primitive.
    CreateMeshTorus,
}

/// Which part of the application runs an action.
///
/// spec: arch 10.2, menus 5
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum HandlerGroup {
    /// The menu-specific handlers: File, Recover, Views, Tools, Window, Help and the toolbar
    /// create buttons.
    App,
    /// Edit menu.
    Edit,
    /// The "objects" handler shared by the Objects menu, the transform submenus and the Mesh
    /// submenu.
    Objects,
    /// The "elements" handler of the Elements popup.
    Elements,
    /// The "meshes" handler of the Meshes popup.
    Meshes,
    /// Texture and Material popups.
    Textures,
    /// Voxel popup.
    Voxel,
    /// Palette popup.
    Palette,
}

/// Whether an action does anything.
///
/// spec: menus 1.5
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ActionKind {
    /// Runs its handler.
    Live,
    /// A row whose action name the owning handler does not implement: it exists and does
    /// nothing.
    Dead,
}

/// One entry of the action registry.
///
/// spec: arch 10.2
#[derive(Copy, Clone, Debug)]
pub struct ActionDef {
    /// The action.
    pub id: ActionId,
    /// The original's action name, for traceability.
    pub name: &'static str,
    /// The handler.
    pub group: HandlerGroup,
    /// Live or dead.
    pub kind: ActionKind,
}

/// The menus, popups and submenus that the menu data describes.
///
/// spec: menus 2, menus 3
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum MenuId {
    /// File menu.
    File,
    /// Edit menu.
    Edit,
    /// Objects menu.
    Objects,
    /// Recover menu.
    Recover,
    /// Views menu.
    Views,
    /// Tools menu.
    Tools,
    /// Window menu.
    Window,
    /// Help menu.
    Help,
    /// File > Open recent files (rebuilt at run time).
    RecentFiles,
    /// Meshes popup.
    MeshesPopup,
    /// Elements popup.
    ElementsPopup,
    /// Texture popup.
    TexturePopup,
    /// Material popup.
    MaterialPopup,
    /// Voxel popup.
    VoxelPopup,
    /// Palette popup.
    PalettePopup,
    /// Meshes popup > Mesh.
    Mesh,
    /// Meshes popup > Transform.
    MeshTransform,
    /// Meshes popup > Texture.
    MeshTexture,
    /// Meshes popup > Copy.
    MeshCopy,
    /// Meshes popup > Paste.
    MeshPaste,
    /// Elements popup > Element.
    Element,
    /// Elements popup > Transform.
    ElementTransform,
    /// Elements popup > Faces.
    Faces,
    /// Faces > Enable.
    FacesEnable,
    /// Faces > Disable.
    FacesDisable,
    /// Elements popup > Textures.
    Textures,
    /// Elements popup > UV Mapping.
    UvMapping,
    /// Elements popup > Tint.
    Tint,
    /// Elements popup > Culling.
    Culling,
    /// Elements popup > Render.
    Render,
    /// Elements popup > Copy.
    ElementCopy,
    /// Elements popup > Paste.
    ElementPaste,
    /// Material popup > Replace textures list.
    NewTexturesList,
    /// Voxel popup > Convert to elements.
    VoxelConvert,
    /// Voxel popup > Transform.
    VoxelTransform,
    /// Voxel popup > Blueprint.
    VoxelBlueprint,
    /// Theme list dropdown of Preferences.
    ThemeList,
    /// Face culling list dropdown of the element settings panel.
    FaceCullingList,
    /// UV face list dropdown of the element settings panel.
    UvFaceList,
    /// Display-settings preset list dropdowns; the rows are filled at run time.
    PresetList,
    /// Texture label list dropdown; the rows are filled at run time.
    TextureLabelList,
}

bitflags! {
    /// The row flags of the menu tables.
    ///
    /// spec: menus 1.4, menus 2
    #[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
    pub struct RowFlags: u8 {
        /// `A`: the row and its shortcut stay enabled when the menu is disabled.
        const ALWAYS_ACTIVE = 1;
        /// `U`: the shortcut also fires while a UI control has keyboard selection.
        const WORKS_IN_UI = 2;
        /// `L`: a toolbar button follows the row's enabled state.
        const LINKED = 4;
    }
}

/// What a menu row does when clicked.
///
/// spec: menus 1.4, menus 1.5
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RowTarget {
    /// A separator (the label starts with "-").
    Separator,
    /// A row without an action (the recent-file rows act through "row selected").
    None,
    /// Runs an action 0.05 s later.
    Action(ActionId),
    /// Opens a child submenu.
    Child(MenuId),
}

/// One row of a menu.
///
/// spec: menus 1.4, arch 6 (cuboid-actions)
#[derive(Copy, Clone, Debug)]
pub struct RowDef {
    /// The label, typos included.
    pub label: &'static str,
    /// The displayed shortcut, which is also the binding definition.
    pub shortcut: Option<&'static str>,
    /// The icon role; cuboid ships its own art for it.
    pub icon: Option<&'static str>,
    /// What a click does.
    pub target: RowTarget,
    /// Always active, works in UI, linked button.
    pub flags: RowFlags,
}

/// One menu or submenu.
///
/// spec: menus 1.3, arch 6 (cuboid-actions)
#[derive(Copy, Clone, Debug)]
pub struct MenuDef {
    /// Which menu.
    pub id: MenuId,
    /// Minimum row width in canvas pixels.
    pub min_width: u16,
    /// Whether the menu starts disabled (menus 1.6).
    pub starts_disabled: bool,
    /// The rows in order.
    pub rows: &'static [RowDef],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_keys_follow_presses_and_releases() {
        let mut held = HeldKeys::default();
        assert!(!held.is_down(EngineKey::LeftAlt));
        held.set(EngineKey::LeftAlt, true);
        held.set(EngineKey::Keypad1, true);
        assert!(held.is_down(EngineKey::LeftAlt) && held.is_down(EngineKey::Keypad1));
        assert!(!held.is_down(EngineKey::Alpha1));
        held.set(EngineKey::LeftAlt, false);
        assert!(!held.is_down(EngineKey::LeftAlt));
    }

    #[test]
    fn keypad_keys_differ_from_top_row_keys_and_modifiers_have_sides() {
        assert_ne!(EngineKey::Keypad1, EngineKey::Alpha1);
        assert_ne!(EngineKey::KeypadPlus, EngineKey::Equals);
        assert_ne!(EngineKey::LeftControl, EngineKey::RightControl);
    }
}
