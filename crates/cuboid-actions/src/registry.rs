//! The action registry (arch 10.2).

use crate::{ActionDef, ActionId, ActionKind, HandlerGroup};

/// A live registry entry; the name defaults to the variant name.
macro_rules! action {
    ($id:ident, $group:ident) => {
        action!($id = stringify!($id), $group)
    };
    ($id:ident = $name:expr, $group:ident) => {
        ActionDef {
            id: ActionId::$id,
            name: $name,
            group: HandlerGroup::$group,
            kind: ActionKind::Live,
        }
    };
}

/// An entry of a row whose handler does not implement it.
macro_rules! dead {
    ($id:ident, $group:ident) => {
        ActionDef {
            id: ActionId::$id,
            name: stringify!($id),
            group: HandlerGroup::$group,
            kind: ActionKind::Dead,
        }
    };
}

/// Every action with its original name, handler group and kind, in the declaration order of
/// [`ActionId`], so that [`ActionId::def`] indexes it.
///
/// The names are those of the action columns of menus 2, 3 and 5. The create buttons of
/// menus 5.14 and the two dead rows have no recorded original name; the variant name stands in.
/// Delete is implemented by the Edit and by the objects handler alike (menus 5.3, 5.4); its
/// entry names the Edit handler.
///
/// spec: arch 10.2, menus 1.5, menus 5
pub static ACTIONS: &[ActionDef] = &[
    // File (menus 2.1, 5.1)
    action!(NewFile, App),
    action!(Open, App),
    action!(Merge, App),
    action!(Save, App),
    action!(SaveAs, App),
    action!(Import, App),
    action!(Export, App),
    action!(Exit, App),
    action!(ClearRecentFiles, App),
    // Edit (menus 2.2, 5.3)
    action!(Undo, Edit),
    action!(Redo, Edit),
    action!(Cut, Edit),
    action!(Copy, Edit),
    action!(Paste, Edit),
    action!(Delete, Edit),
    action!(DeleteConfirm, Edit),
    action!(SelectName, Edit),
    action!(SelectAllElements, Edit),
    action!(SelectNone, Edit),
    action!(SelectInverse, Edit),
    action!(ModelProperties, Edit),
    action!(ModelOrigin, Edit),
    action!(ModelBlueprint, Edit),
    action!(RotateModelX, Edit),
    action!(RotateModelY, Edit),
    action!(RotateModelZ, Edit),
    // Objects menu and the "objects" handler (menus 2.3, 5.4)
    action!(Next, Objects),
    action!(Previous, Objects),
    action!(ElementProperties, Objects),
    action!(ElementFaces, Objects),
    action!(ElementUv = "ElementUV", Objects),
    action!(Rename, Objects),
    action!(Move, Objects),
    action!(Scale, Objects),
    action!(Clone, Objects),
    action!(ToggleVisibility, Objects),
    action!(ToggleLock, Objects),
    action!(FocusLock, Objects),
    action!(ResetRotation, Objects),
    action!(Hide, Objects),
    action!(Show, Objects),
    action!(Lock, Objects),
    action!(Unlock, Objects),
    action!(SetPosition, Objects),
    action!(SetSize, Objects),
    action!(ApplyRescale, Objects),
    action!(Inflate, Objects),
    action!(RotateX, Objects),
    action!(RotateY, Objects),
    action!(RotateZ, Objects),
    action!(Randomize, Objects),
    action!(CenterX, Objects),
    action!(CenterY, Objects),
    action!(CenterZ, Objects),
    // Recover (menus 2.4, 5.6)
    action!(ShowAll, App),
    action!(UnlockAll, App),
    action!(ResetBlueprint, App),
    action!(ResetCamera, App),
    action!(GetBackFarElements, App),
    // Views (menus 2.5, 5.7)
    action!(ZoomPlus, App),
    action!(ZoomMinus, App),
    action!(WireframeMode, App),
    action!(GridSettings, App),
    action!(LoadBgUp = "LoadBgTL", App),
    action!(LoadBgWest = "LoadBgTR", App),
    action!(LoadBgSouth = "LoadBgBL", App),
    action!(LoadBgAll, App),
    action!(ToggleBgSize, App),
    action!(DeleteBgAll, App),
    // Tools (menus 2.6, 5.8)
    action!(ElementTools, App),
    action!(VoxelTools, App),
    action!(PaintTools, App),
    action!(ColorTools, App),
    action!(ColorPalette, App),
    action!(ElementsList, App),
    action!(ElementsMaterial, App),
    action!(GizmosTools, App),
    action!(ShareSketchfab, App),
    action!(BrowseAppFolder = "CubikFolder", App),
    action!(BrowseDataFolder = "CubikDataFolder", App),
    action!(BrowseLastExportFolder = "LastExportFolder", App),
    action!(Preferences, App),
    // Window (menus 2.7, 5.9)
    action!(MoveManagersOnLeft, App),
    action!(MoveManagersOnRight, App),
    action!(HideManagers, App),
    action!(HideHelpers, App),
    action!(HideAll, App),
    // Help (menus 2.8, 5.10)
    action!(About, App),
    action!(Support, App),
    action!(Details = "CubikPage", App),
    action!(Changelog, App),
    action!(ProjectSite = "BDcraftPage", App),
    // Meshes popup (menus 3.1, 5.4)
    action!(SelectMeshTint, Objects),
    action!(ApplyMeshTint, Objects),
    action!(MeshMoveToTop = "MoveToTop", Meshes),
    action!(MeshMoveToBottom = "MoveToBottom", Meshes),
    action!(StartVoxelization, Objects),
    action!(LoadMeshTexture, Objects),
    action!(SaveMeshTexture, Objects),
    action!(RemoveAlpha, Objects),
    action!(ClearMeshTexture, Objects),
    action!(CopyMeshTexture, Objects),
    action!(CopyMeshColor, Objects),
    action!(PasteMeshTexture, Objects),
    action!(PasteMeshColor, Objects),
    // Elements popup (menus 3.2, 5.5)
    action!(ElementRename = "Rename", Elements),
    action!(ElementColor = "Color", Elements),
    action!(ElementColorRandom = "ColorRandom", Elements),
    action!(ElementMoveToTop = "MoveToTop", Elements),
    action!(ElementMoveToBottom = "MoveToBottom", Elements),
    action!(ElementHide = "Hide", Elements),
    action!(ElementShow = "Show", Elements),
    action!(ElementLock = "Lock", Elements),
    action!(ElementUnlock = "Unlock", Elements),
    action!(ElementClone = "Clone", Elements),
    action!(ElementDelete = "Delete", Elements),
    action!(FacesEnableUp, Elements),
    action!(FacesEnableDown, Elements),
    action!(FacesEnableNorth, Elements),
    action!(FacesEnableSouth, Elements),
    action!(FacesEnableWest, Elements),
    action!(FacesEnableEast, Elements),
    action!(FacesEnableAll, Elements),
    action!(FacesDisableUp, Elements),
    action!(FacesDisableDown, Elements),
    action!(FacesDisableNorth, Elements),
    action!(FacesDisableSouth, Elements),
    action!(FacesDisableWest, Elements),
    action!(FacesDisableEast, Elements),
    action!(FacesDisableAll, Elements),
    action!(TexDefault, Elements),
    action!(TexSud = "TexSUD", Elements),
    action!(TexSc = "TexSC", Elements),
    action!(TexFsc = "TexFSC", Elements),
    action!(TexDunswe = "TexDUNSWE", Elements),
    action!(TexCustom, Elements),
    action!(UvSnapEnable = "UVSnapEnable", Elements),
    action!(UvSnapDisable = "UVSnapDisable", Elements),
    action!(UvSnapMirrorEnable = "UVSnapMirrorEnable", Elements),
    action!(UvSnapMirrorDisable = "UVSnapMirrorDisable", Elements),
    dead!(UvApplyDefault, Elements),
    action!(TintUp, Elements),
    action!(TintDown, Elements),
    action!(TintSides, Elements),
    action!(TintAll, Elements),
    action!(TintNone, Elements),
    action!(CullingUp, Elements),
    action!(CullingDown, Elements),
    action!(CullingSides, Elements),
    action!(CullingAll, Elements),
    action!(CullingNone, Elements),
    action!(ShadeEnable, Elements),
    action!(ShadeDisable, Elements),
    action!(InvertFalse, Elements),
    action!(InvertTrue, Elements),
    action!(CopyFaces, Elements),
    action!(CopyTextures, Elements),
    action!(CopyUv = "CopyUV", Elements),
    action!(CopyTint, Elements),
    action!(CopyCulling, Elements),
    action!(CopyShade, Elements),
    action!(PasteFaces, Elements),
    action!(PasteTextures, Elements),
    action!(PasteUv = "PasteUV", Elements),
    dead!(PasteTint, Elements),
    action!(PasteCulling, Elements),
    action!(PasteShade, Elements),
    // Texture and Material popups (menus 3.3, 3.4, 5.11)
    action!(LoadTexture, Textures),
    action!(SaveTexture, Textures),
    action!(SetAsShortcut, Textures),
    action!(SetTextureLabel, Textures),
    action!(RenameTexture, Textures),
    action!(ApplyTextureToSelection, Textures),
    action!(EditTexture, Textures),
    action!(EmptyTexture, Textures),
    action!(DeleteTexture, Textures),
    action!(AddTexture, Textures),
    action!(NewMatAuto = "NewMat_Auto", Textures),
    action!(NewMatOptim = "NewMat_Optim", Textures),
    action!(NewMatEmpty = "NewMat_Empty", Textures),
    action!(NewMatTexture = "NewMat_Texture", Textures),
    action!(NewMatSud = "NewMat_SUD", Textures),
    action!(NewMatSc = "NewMat_SC", Textures),
    action!(NewMatFsc = "NewMat_FSC", Textures),
    action!(NewMatDunswe = "NewMat_DUNSWE", Textures),
    action!(CreateAtlasBlank, Textures),
    action!(CreateAtlasTextures, Textures),
    // Voxel popup (menus 3.6, 5.12)
    action!(VoxelSetup = "Setup", Voxel),
    action!(ConvertToElementsCubes, Voxel),
    action!(ConvertToElements, Voxel),
    action!(ConvertToElementsAtlas, Voxel),
    action!(VoxelUpscale = "Upscale", Voxel),
    action!(VoxelDownscale = "Downscale", Voxel),
    action!(VoxelFlipX = "FlipX", Voxel),
    action!(VoxelFlipY = "FlipY", Voxel),
    action!(VoxelFlipZ = "FlipZ", Voxel),
    action!(VoxelRotateX = "RotateX", Voxel),
    action!(VoxelRotateY = "RotateY", Voxel),
    action!(VoxelRotateZ = "RotateZ", Voxel),
    action!(VoxelResetColor = "ResetColor", Voxel),
    action!(VoxelFillVolume = "FillVolume", Voxel),
    action!(VoxelHollowVolume = "HollowVolume", Voxel),
    action!(VoxelCropOutside = "CropVoxelsOutside", Voxel),
    action!(FillBlueprint, Voxel),
    action!(ClearBlueprint, Voxel),
    action!(CopyInBlueprint, Voxel),
    action!(PasteAtBlueprint, Voxel),
    action!(FlipXBlueprint, Voxel),
    action!(FlipYBlueprint, Voxel),
    action!(FlipZBlueprint, Voxel),
    action!(ClearVoxels, Voxel),
    action!(ResetVoxels, Voxel),
    // Palette popup (menus 3.7, 5.13)
    action!(DefaultPalette, Palette),
    action!(HslPalette, Palette),
    action!(JmpPalette, Palette),
    action!(Generic16Palette, Palette),
    action!(Pico8Palette, Palette),
    action!(Edg32Palette, Palette),
    action!(Edg16Palette, Palette),
    action!(LoadPalette, Palette),
    action!(SavePalette, Palette),
    // Create buttons of the left tool strip (menus 5.14)
    action!(CreateElementCube, App),
    action!(CreateElementPlaneX, App),
    action!(CreateElementPlaneY, App),
    action!(CreateElementPlaneZ, App),
    action!(CreateMeshCube, App),
    action!(CreateMeshSphere, App),
    action!(CreateMeshCylinder, App),
    action!(CreateMeshCone, App),
    action!(CreateMeshPyramid, App),
    action!(CreateMeshSlope, App),
    action!(CreateMeshCornerIn, App),
    action!(CreateMeshCornerOut, App),
    action!(CreateMeshTorus, App),
];

impl ActionId {
    /// The registry entry of the action.
    pub fn def(self) -> &'static ActionDef {
        &ACTIONS[self as usize]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn the_registry_lists_every_action_in_declaration_order() {
        for (index, def) in ACTIONS.iter().enumerate() {
            assert_eq!(def.id as usize, index, "{}", def.name);
        }
        // CreateMeshTorus is the last variant of ActionId.
        assert_eq!(ACTIONS.len(), ActionId::CreateMeshTorus as usize + 1);
    }

    #[test]
    fn names_are_unique_within_a_handler() {
        let mut seen = HashSet::new();
        for def in ACTIONS {
            assert!(
                seen.insert((def.group, def.name)),
                "{:?} {}",
                def.group,
                def.name
            );
        }
    }

    /// spec: menus 1.5, arch 10.2
    #[test]
    fn exactly_the_dead_rows_do_nothing() {
        let dead: Vec<_> = ACTIONS
            .iter()
            .filter(|def| def.kind == ActionKind::Dead)
            .map(|def| def.id)
            .collect();
        assert_eq!(dead, [ActionId::UvApplyDefault, ActionId::PasteTint]);
    }
}
