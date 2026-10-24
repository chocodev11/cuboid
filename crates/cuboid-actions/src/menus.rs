//! The menu tables (menus 1-3): every row of the bar menus, popups, submenus and list
//! dropdowns with its label, shortcut, icon role, target and flags.
//!
//! Labels carry `{App}` where the original names its product; the UI substitutes the product
//! name (arch 14.7). Icon roles are the picture names of the specification, `app_logo` and
//! `project_logo` replace the two brand marks of the Help menu.

use crate::{ActionId as Id, MenuDef, MenuId as Menu, RowDef, RowFlags, RowTarget};

/// Flag letters of the tables: A always active, U works in UI, L linked toolbar button.
const A: RowFlags = RowFlags::ALWAYS_ACTIVE;
const AU: RowFlags = A.union(RowFlags::WORKS_IN_UI);
const L: RowFlags = RowFlags::LINKED;
const AL: RowFlags = A.union(L);

const fn row(label: &'static str, target: RowTarget) -> RowDef {
    RowDef {
        label,
        shortcut: None,
        icon: None,
        target,
        flags: RowFlags::empty(),
    }
}

/// A row that runs an action.
const fn act(label: &'static str, id: Id) -> RowDef {
    row(label, RowTarget::Action(id))
}

/// A row that opens a child submenu.
const fn sub(label: &'static str, menu: Menu) -> RowDef {
    row(label, RowTarget::Child(menu))
}

/// A row without an action: the entries of list dropdowns and the placeholder rows.
const fn entry(label: &'static str) -> RowDef {
    row(label, RowTarget::None)
}

/// A separator: the label starts with "-" (menus 1.4).
const SEP: RowDef = row("-", RowTarget::Separator);

impl RowDef {
    const fn key(mut self, shortcut: &'static str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    const fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    const fn flags(mut self, flags: RowFlags) -> Self {
        self.flags = flags;
        self
    }
}

const fn menu(id: Menu, min_width: u16, rows: &'static [RowDef]) -> MenuDef {
    MenuDef {
        id,
        min_width,
        starts_disabled: false,
        rows,
    }
}

impl MenuDef {
    const fn disabled(mut self) -> Self {
        self.starts_disabled = true;
        self
    }
}

/// Bar order, minimum widths, rows and flags of every menu, popup, submenu and list dropdown,
/// in the declaration order of [`crate::MenuId`] so that [`crate::MenuId::def`] indexes it.
///
/// The rows of the recent-files submenu, the preset list and the texture label list are
/// built at run time; the tables hold their initial content. Objects and the recent-files
/// submenu start disabled. The original also starts Tools disabled, but every Tools row is
/// always active, so cuboid starts it enabled (menus 2.6).
///
/// The shortcut text of a row is also its binding definition (menus 6.1). The Help menu omits
/// the registration row (menus 2.8, 9.13).
///
/// spec: menus 1.3, menus 1.4, menus 1.6, menus 2, menus 3
pub static MENUS: &[MenuDef] = &[
    // The bar menus in bar order (menus 1.1, 2.1-2.8).
    menu(
        Menu::File,
        196,
        &[
            act("New file", Id::NewFile)
                .key("Ctrl+N")
                .icon("page_white")
                .flags(A),
            SEP,
            act("Open", Id::Open)
                .key("Ctrl+O")
                .icon("folder_open")
                .flags(A),
            act("Merge", Id::Merge)
                .key("Ctrl+M")
                .icon("folder_merge")
                .flags(A),
            sub("Open recent files", Menu::RecentFiles),
            SEP,
            act("Save", Id::Save).key("Ctrl+S").icon("save").flags(AU),
            act("Save As...", Id::SaveAs).icon("save_as").flags(A),
            SEP,
            act("Import", Id::Import)
                .key("Ctrl+I")
                .icon("import")
                .flags(A),
            act("Export", Id::Export).key("Ctrl+E").icon("export"),
            SEP,
            act("Exit", Id::Exit).icon("close_window").flags(A),
        ],
    ),
    menu(
        Menu::Edit,
        280,
        &[
            act("Undo", Id::Undo).key("Ctrl+Z").icon("arrow_undo"),
            act("Redo", Id::Redo).key("Ctrl+Y").icon("arrow_redo"),
            SEP,
            act("Cut", Id::Cut).key("Ctrl+X").icon("cut"),
            act("Copy", Id::Copy).key("Ctrl+C").icon("page_copy"),
            act("Paste", Id::Paste).key("Ctrl+V").icon("page_paste"),
            SEP,
            act("Delete", Id::Delete)
                .key("Ctrl+Del")
                .icon("cross")
                .flags(L),
            SEP,
            act("Select by name", Id::SelectName)
                .key("Ctrl+F")
                .icon("search"),
            act("Select all", Id::SelectAllElements)
                .key("Ctrl+A")
                .icon("select_all"),
            act("Deselect", Id::SelectNone).key("Ctrl+D"),
            act("Select Inverse", Id::SelectInverse).key("Ctrl+Shift+I"),
            SEP,
            act("Model Properties", Id::ModelProperties)
                .key("Ctrl+P")
                .icon("cog")
                .flags(AL),
            act("Set Model Origin", Id::ModelOrigin)
                .key("Ctrl+K")
                .icon("origin")
                .flags(L),
            act("Set Model Blueprint", Id::ModelBlueprint)
                .key("Ctrl+B")
                .icon("blueprint")
                .flags(L),
            SEP,
            act("Rotate model +90° on X axis", Id::RotateModelX).icon("icon_rotateX"),
            act("Rotate model +90° on Y axis", Id::RotateModelY).icon("icon_rotateY2"),
            act("Rotate model +90° on Z axis", Id::RotateModelZ).icon("icon_rotateZ"),
        ],
    ),
    menu(
        Menu::Objects,
        256,
        &[
            act("Next", Id::Next).key("PgUp").icon("bullet_go").flags(A),
            act("Previous", Id::Previous)
                .key("PgDown")
                .icon("bullet_back")
                .flags(A),
            SEP,
            act("Element Properties...", Id::ElementProperties)
                .key("Shift+P")
                .icon("shape_square_wrench"),
            act("Element Faces...", Id::ElementFaces)
                .key("Shift+F")
                .icon("cube_faces"),
            act("Element UV Mapping...", Id::ElementUv)
                .key("Shift+U")
                .icon("cube_uv"),
            SEP,
            act("Rename", Id::Rename)
                .key("Shift+N")
                .icon("textfield_rename"),
            act("Move", Id::Move).key("Shift+M").icon("arrow_out"),
            act("Scale", Id::Scale).key("Shift+S").icon("resize"),
            act("Clone", Id::Clone).key("Shift+C").icon("clone"),
            SEP,
            act("Toggle Visibility", Id::ToggleVisibility)
                .key("Shift+V")
                .icon("lightbulb_off")
                .flags(L),
            act("Toggle Lock", Id::ToggleLock)
                .key("Shift+L")
                .icon("lock")
                .flags(L),
            act("Focus Lock", Id::FocusLock)
                .key("Shift+K")
                .icon("lock_go")
                .flags(L),
            act("Reset Origin/Rotation", Id::ResetRotation)
                .key("Shift+R")
                .icon("anchor")
                .flags(L),
        ],
    )
    .disabled(),
    menu(
        Menu::Recover,
        280,
        &[
            act("Show all Elements", Id::ShowAll)
                .key("Ctrl+Shift+S")
                .icon("lightbulb")
                .flags(L),
            act("Unlock all Elements", Id::UnlockAll)
                .key("Ctrl+Shift+U")
                .icon("lock_open")
                .flags(L),
            SEP,
            act("Reset Blueprint to default", Id::ResetBlueprint)
                .key("Ctrl+Shift+P")
                .icon("blueprint")
                .flags(L),
            act("Reset Views to default", Id::ResetCamera)
                .key("Ctrl+Shift+C")
                .icon("camera_go")
                .flags(L),
            SEP,
            act("Get back Far Elements", Id::GetBackFarElements)
                .key("Ctrl+Shift+F")
                .icon("recover_elements")
                .flags(L),
        ],
    ),
    menu(
        Menu::Views,
        296,
        &[
            act("Zoom+", Id::ZoomPlus)
                .key("+")
                .icon("magnifier_zoom_in"),
            act("Zoom-", Id::ZoomMinus)
                .key("-")
                .icon("magifier_zoom_out"),
            SEP,
            act("Wireframe mode", Id::WireframeMode)
                .key("Shift+W")
                .icon("eye"),
            act("Grid settings", Id::GridSettings)
                .key("Shift+G")
                .icon("grid"),
            SEP,
            act("Load background for Up view...", Id::LoadBgUp).icon("image_add"),
            act("Load background for West view...", Id::LoadBgWest).icon("image_add"),
            act("Load background for South view...", Id::LoadBgSouth).icon("image_add"),
            SEP,
            act("Load background for all views...", Id::LoadBgAll)
                .key("Shift+A")
                .icon("images"),
            SEP,
            act("Toggle backgrounds size", Id::ToggleBgSize)
                .key("Shift+T")
                .icon("images"),
            act("Delete backgrounds", Id::DeleteBgAll)
                .key("Shift+B")
                .icon("images_delete"),
        ],
    ),
    menu(
        Menu::Tools,
        256,
        &[
            act("Element tools", Id::ElementTools)
                .key("F5")
                .icon("cube")
                .flags(A),
            act("Voxel tools", Id::VoxelTools)
                .key("F6")
                .icon("voxel")
                .flags(A),
            act("Paint tools", Id::PaintTools)
                .key("F7")
                .icon("draw_pixel")
                .flags(A),
            act("Color tools", Id::ColorTools)
                .key("F8")
                .icon("color_picker")
                .flags(A),
            act("Color Palette", Id::ColorPalette)
                .key("F9")
                .icon("color_palette")
                .flags(A),
            act("Elements List", Id::ElementsList)
                .key("F10")
                .icon("elements_manager")
                .flags(A),
            act("Elements Material", Id::ElementsMaterial)
                .key("F11")
                .icon("materials_manager")
                .flags(A),
            act("Gizmos tools", Id::GizmosTools)
                .key("F12")
                .icon("bounding_box")
                .flags(A),
            SEP,
            act("Share on Sketchfab", Id::ShareSketchfab)
                .icon("link")
                .flags(A),
            SEP,
            act("Browse {App} Folder", Id::BrowseAppFolder)
                .icon("folder_open")
                .flags(A),
            act("Browse {App} Data Folder", Id::BrowseDataFolder)
                .icon("folder_open")
                .flags(A),
            act("Browse last save/export Folder", Id::BrowseLastExportFolder)
                .icon("folder_open")
                .flags(A),
            SEP,
            act("Preferences...", Id::Preferences).icon("cog").flags(A),
        ],
    ),
    menu(
        Menu::Window,
        380,
        &[
            act("Move Managers on Left", Id::MoveManagersOnLeft)
                .key("Ctrl+1 (Num. keypad)")
                .icon("move_managers_left"),
            act("Move Managers on Right", Id::MoveManagersOnRight)
                .key("Ctrl+3 (Num. keypad)")
                .icon("move_managers_right"),
            SEP,
            act("Close Managers", Id::HideManagers).key("Ctrl+2 (Num. keypad)"),
            act("Close Helpers", Id::HideHelpers).key("Ctrl+8 (Num. keypad)"),
            SEP,
            act("Close All", Id::HideAll)
                .key("Ctrl+0 (Num. keypad)")
                .icon("close_window"),
        ],
    ),
    menu(
        Menu::Help,
        200,
        &[
            act("About {App}...", Id::About).icon("information"),
            SEP,
            act("{App} Support...", Id::Support)
                .key("F1")
                .icon("comments"),
            act("{App} Details...", Id::Details).icon("app_logo"),
            act("{App} Changelog...", Id::Changelog).icon("changelog"),
            act("Project website", Id::ProjectSite).icon("project_logo"),
        ],
    ),
    // The submenu of File > Open recent files and the popups (menus 2.1, 3.1-3.7).
    menu(Menu::RecentFiles, 196, &[entry("No recent files...")]).disabled(),
    menu(
        Menu::MeshesPopup,
        180,
        &[
            sub("Mesh", Menu::Mesh).icon("sphere"),
            sub("Transform", Menu::MeshTransform).icon("arrow_out"),
            SEP,
            act("Convert to voxels...", Id::StartVoxelization).icon("export"),
            SEP,
            sub("Texture", Menu::MeshTexture).icon("image"),
            SEP,
            sub("Copy", Menu::MeshCopy).icon("page_copy"),
            sub("Paste", Menu::MeshPaste).icon("page_paste"),
        ],
    ),
    menu(
        Menu::ElementsPopup,
        180,
        &[
            sub("Element", Menu::Element).icon("cube"),
            sub("Transform", Menu::ElementTransform).icon("arrow_out"),
            SEP,
            sub("Faces", Menu::Faces),
            sub("Textures", Menu::Textures),
            sub("UV Mapping", Menu::UvMapping).icon("cube_uv"),
            sub("Tint", Menu::Tint),
            sub("Culling", Menu::Culling),
            sub("Render", Menu::Render),
            SEP,
            sub("Copy", Menu::ElementCopy).icon("page_copy"),
            sub("Paste", Menu::ElementPaste).icon("page_paste"),
        ],
    ),
    menu(
        Menu::TexturePopup,
        160,
        &[
            act("Load texture...", Id::LoadTexture),
            act("Save texture...", Id::SaveTexture),
            SEP,
            act("Set as shortcut", Id::SetAsShortcut).icon("image_link"),
            act("Edit label...", Id::SetTextureLabel),
            act("Rename texture...", Id::RenameTexture),
            SEP,
            act("Apply to selection", Id::ApplyTextureToSelection),
            SEP,
            act("Edit texture...", Id::EditTexture).icon("draw_paint"),
            act("Empty texture", Id::EmptyTexture).icon("image_delete"),
            act("Delete texture", Id::DeleteTexture).icon("cross"),
        ],
    ),
    menu(
        Menu::MaterialPopup,
        210,
        &[
            act("Add missing textures", Id::NewMatAuto),
            act("Remove unused textures", Id::NewMatOptim),
            act("Delete all textures", Id::NewMatEmpty),
            SEP,
            sub("Replace textures list", Menu::NewTexturesList).icon("materials_manager"),
            SEP,
            act("Create atlas, blank textures", Id::CreateAtlasBlank),
            act("Create atlas, keep textures", Id::CreateAtlasTextures),
        ],
    ),
    menu(
        Menu::VoxelPopup,
        180,
        &[
            act("Setup", Id::VoxelSetup).icon("cog"),
            SEP,
            sub("Convert to elements", Menu::VoxelConvert).icon("export"),
            sub("Transform", Menu::VoxelTransform).icon("arrow_out"),
            sub("Blueprint", Menu::VoxelBlueprint).icon("blueprint"),
            SEP,
            act("Clear voxels", Id::ClearVoxels).icon("bullet_delete"),
            act("Reset", Id::ResetVoxels).icon("cross"),
        ],
    ),
    menu(
        Menu::PalettePopup,
        170,
        &[
            act("RGB palette (default)", Id::DefaultPalette),
            act("HSL palette", Id::HslPalette),
            act("JMP palette", Id::JmpPalette),
            act("Generic16 palette", Id::Generic16Palette),
            act("Pico8 palette", Id::Pico8Palette),
            act("EDG32 palette", Id::Edg32Palette),
            act("EDG16 palette", Id::Edg16Palette),
            SEP,
            act("Load new palette", Id::LoadPalette),
            act("Save palette", Id::SavePalette),
        ],
    ),
    // The submenus of the Meshes popup (menus 3.1).
    menu(
        Menu::Mesh,
        170,
        &[
            act("Rename...", Id::Rename).icon("textfield_rename"),
            SEP,
            act("Set Color...", Id::SelectMeshTint).icon("color_wheel"),
            act("Apply Color", Id::ApplyMeshTint).icon("color_wheel_go"),
            SEP,
            act("Move to top", Id::MeshMoveToTop).icon("arrow_up"),
            act("Move to bottom", Id::MeshMoveToBottom).icon("arrow_down"),
            SEP,
            act("Hide", Id::Hide).icon("lightbulb_off"),
            act("Show", Id::Show).icon("lightbulb"),
            SEP,
            act("Lock", Id::Lock).icon("lock"),
            act("Unlock", Id::Unlock).icon("lock_open"),
            SEP,
            act("Clone", Id::Clone).icon("clone"),
            SEP,
            act("Delete", Id::Delete).icon("cross"),
        ],
    ),
    menu(
        Menu::MeshTransform,
        160,
        &[
            act("Set Position...", Id::SetPosition),
            act("Move...", Id::Move),
            SEP,
            act("Set size...", Id::SetSize),
            act("Scale...", Id::Scale),
            act("Apply rescale", Id::ApplyRescale),
            SEP,
            act("Rotate 90° around X", Id::RotateX),
            act("Rotate 90° around Y", Id::RotateY),
            act("Rotate 90° around Z", Id::RotateZ),
            SEP,
            act("Randomize...", Id::Randomize),
            SEP,
            act("Center on X axis", Id::CenterX),
            act("Center on Y axis", Id::CenterY),
            act("Center on Z axis", Id::CenterZ),
        ],
    ),
    menu(
        Menu::MeshTexture,
        160,
        &[
            act("Load texture...", Id::LoadMeshTexture),
            act("Save texture...", Id::SaveMeshTexture),
            SEP,
            act("Remove alpha", Id::RemoveAlpha),
            SEP,
            act("Clear texture", Id::ClearMeshTexture),
        ],
    ),
    menu(
        Menu::MeshCopy,
        140,
        &[
            act("Texture", Id::CopyMeshTexture),
            act("Mesh color", Id::CopyMeshColor),
        ],
    ),
    menu(
        Menu::MeshPaste,
        140,
        &[
            act("Texture", Id::PasteMeshTexture),
            act("Mesh color", Id::PasteMeshColor),
        ],
    ),
    // The submenus of the Elements popup (menus 3.2).
    menu(
        Menu::Element,
        170,
        &[
            act("Rename...", Id::ElementRename).icon("textfield_rename"),
            SEP,
            act("Set Color...", Id::ElementColor).icon("color_wheel"),
            act("Apply random Color", Id::ElementColorRandom).icon("color_wheel_go"),
            SEP,
            act("Move to top", Id::ElementMoveToTop).icon("arrow_up"),
            act("Move to bottom", Id::ElementMoveToBottom).icon("arrow_down"),
            SEP,
            act("Hide", Id::ElementHide).icon("lightbulb_off"),
            act("Show", Id::ElementShow).icon("lightbulb"),
            SEP,
            act("Lock", Id::ElementLock).icon("lock"),
            act("Unlock", Id::ElementUnlock).icon("lock_open"),
            SEP,
            act("Clone", Id::ElementClone).icon("clone"),
            SEP,
            act("Delete", Id::ElementDelete).icon("cross"),
        ],
    ),
    menu(
        Menu::ElementTransform,
        160,
        &[
            act("Set Position...", Id::SetPosition),
            act("Move...", Id::Move),
            SEP,
            act("Set size...", Id::SetSize),
            act("Scale...", Id::Scale),
            act("Apply rescale", Id::ApplyRescale),
            act("Inflate...", Id::Inflate),
            SEP,
            act("Rotate 90° around X", Id::RotateX),
            act("Rotate 90° around Y", Id::RotateY),
            act("Rotate 90° around Z", Id::RotateZ),
            SEP,
            act("Randomize...", Id::Randomize),
            SEP,
            act("Center on X axis", Id::CenterX),
            act("Center on Y axis", Id::CenterY),
            act("Center on Z axis", Id::CenterZ),
        ],
    ),
    menu(
        Menu::Faces,
        128,
        &[
            sub("Enable", Menu::FacesEnable).icon("bullet_check"),
            sub("Disable", Menu::FacesDisable).icon("bullet_uncheck"),
        ],
    ),
    menu(
        Menu::FacesEnable,
        128,
        &[
            act("Top", Id::FacesEnableUp),
            act("Down", Id::FacesEnableDown),
            SEP,
            act("North", Id::FacesEnableNorth),
            act("South", Id::FacesEnableSouth),
            act("West", Id::FacesEnableWest),
            act("East", Id::FacesEnableEast),
            SEP,
            act("All", Id::FacesEnableAll),
        ],
    ),
    menu(
        Menu::FacesDisable,
        128,
        &[
            act("Top", Id::FacesDisableUp),
            act("Down", Id::FacesDisableDown),
            SEP,
            act("North", Id::FacesDisableNorth),
            act("South", Id::FacesDisableSouth),
            act("West", Id::FacesDisableWest),
            act("East", Id::FacesDisableEast),
            SEP,
            act("All", Id::FacesDisableAll),
        ],
    ),
    menu(
        Menu::Textures,
        270,
        &[
            act("texture (default)", Id::TexDefault),
            SEP,
            act("side + up + down", Id::TexSud),
            act("side + cap", Id::TexSc),
            act("front + side + cap", Id::TexFsc),
            act("down + up + north + south + west + east", Id::TexDunswe),
            SEP,
            act("Customize...", Id::TexCustom).icon("bullet_wrench"),
        ],
    ),
    menu(
        Menu::UvMapping,
        196,
        &[
            act("Enable Snap", Id::UvSnapEnable),
            act("Disable Snap", Id::UvSnapDisable),
            SEP,
            act("Enable Symmetry", Id::UvSnapMirrorEnable),
            act("Disable Symmetry", Id::UvSnapMirrorDisable),
            SEP,
            act("Apply Default UV", Id::UvApplyDefault),
        ],
    ),
    menu(
        Menu::Tint,
        128,
        &[
            act("Top", Id::TintUp),
            act("Down", Id::TintDown),
            act("Sides", Id::TintSides),
            SEP,
            act("All", Id::TintAll),
            act("None", Id::TintNone),
        ],
    ),
    menu(
        Menu::Culling,
        128,
        &[
            act("Top", Id::CullingUp),
            act("Down", Id::CullingDown),
            act("Sides", Id::CullingSides),
            SEP,
            act("All", Id::CullingAll),
            act("None", Id::CullingNone),
        ],
    ),
    menu(
        Menu::Render,
        128,
        &[
            act("Enable shading", Id::ShadeEnable),
            act("Disable shading", Id::ShadeDisable),
            SEP,
            act("Outer normals", Id::InvertFalse),
            act("Inner normals", Id::InvertTrue),
        ],
    ),
    menu(
        Menu::ElementCopy,
        140,
        &[
            act("Faces visibility", Id::CopyFaces),
            act("Textures", Id::CopyTextures),
            act("UV Mapping", Id::CopyUv),
            act("Tint", Id::CopyTint),
            act("Culling", Id::CopyCulling),
            act("Shading", Id::CopyShade),
        ],
    ),
    menu(
        Menu::ElementPaste,
        140,
        &[
            act("Faces visibility", Id::PasteFaces),
            act("Textures", Id::PasteTextures),
            act("UV Mapping", Id::PasteUv),
            act("Tint", Id::PasteTint),
            act("Culling", Id::PasteCulling),
            act("Shading", Id::PasteShade),
        ],
    ),
    // The submenus of the Material and Voxel popups (menus 3.4, 3.6).
    menu(
        Menu::NewTexturesList,
        300,
        &[
            act("Texture (default)", Id::NewMatTexture),
            act("Side + Up + Down", Id::NewMatSud),
            act("Side + Cap", Id::NewMatSc),
            act("Front + Side + Cap", Id::NewMatFsc),
            act("Down + Up + North + South + West + East", Id::NewMatDunswe),
        ],
    ),
    menu(
        Menu::VoxelConvert,
        220,
        &[
            act("1 element per voxel (cubes)", Id::ConvertToElementsCubes),
            act("1+ elements per color (palette)", Id::ConvertToElements),
            act(
                "1+ elements per surface (atlas)",
                Id::ConvertToElementsAtlas,
            ),
        ],
    ),
    menu(
        Menu::VoxelTransform,
        180,
        &[
            act("Upscale x2", Id::VoxelUpscale),
            act("Downscale /2", Id::VoxelDownscale),
            SEP,
            act("Flip on X axis", Id::VoxelFlipX),
            act("Flip on Y axis", Id::VoxelFlipY),
            act("Flip on Z axis", Id::VoxelFlipZ),
            SEP,
            act("Rotate 90° around X axis", Id::VoxelRotateX),
            act("Rotate 90° around Y axis", Id::VoxelRotateY),
            act("Rotate 90° around Z axis", Id::VoxelRotateZ),
            SEP,
            act("Color all", Id::VoxelResetColor),
            act("Fill volume", Id::VoxelFillVolume),
            act("Hollow volume", Id::VoxelHollowVolume),
            SEP,
            act("Crop voxels outside model box", Id::VoxelCropOutside),
        ],
    ),
    menu(
        Menu::VoxelBlueprint,
        220,
        &[
            act("Create voxels", Id::FillBlueprint),
            act("Delete voxels", Id::ClearBlueprint),
            SEP,
            act("Copy voxels", Id::CopyInBlueprint),
            act("Paste voxels", Id::PasteAtBlueprint),
            SEP,
            act("Flip voxels on X axis", Id::FlipXBlueprint),
            act("Flip voxels on Y axis", Id::FlipYBlueprint),
            act("Flip voxels on Z axis", Id::FlipZBlueprint),
        ],
    ),
    // The list dropdowns (menus 3.5, 3.8): the consumers read the chosen label.
    menu(Menu::ThemeList, 96, &[entry("Light"), entry("Dark")]),
    menu(
        Menu::FaceCullingList,
        96,
        &[
            entry("none"),
            SEP,
            entry("down"),
            entry("up"),
            entry("north"),
            entry("south"),
            entry("west"),
            entry("east"),
        ],
    ),
    menu(
        Menu::UvFaceList,
        96,
        &[
            entry("Down"),
            entry("Up"),
            entry("North"),
            entry("South"),
            entry("West"),
            entry("East"),
        ],
    ),
    menu(Menu::PresetList, 128, &[]),
    menu(Menu::TextureLabelList, 212, &[]),
];

/// The rows that the run-time rebuild of the recent-files submenu puts after the file rows,
/// newest file first (menus 2.1).
///
/// spec: menus 2.1
pub static RECENT_FILES_FOOTER: [RowDef; 2] =
    [SEP, act("Clear recent file list", Id::ClearRecentFiles)];

/// The label of Edit > Paste while objects are on the clipboard.
///
/// spec: menus 1.7
pub const PASTE_OBJECTS: &str = "Paste objects";

/// The label of Edit > Paste while voxels are on the clipboard.
///
/// spec: menus 1.7
pub const PASTE_VOXELS: &str = "Paste voxels";

impl Menu {
    /// The table of the menu.
    pub fn def(self) -> &'static MenuDef {
        &MENUS[self as usize]
    }
}

#[cfg(test)]
mod tests;
