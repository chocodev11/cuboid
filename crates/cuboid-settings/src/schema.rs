//! The settings object: every key of the original's preferences with its name, value format and
//! defaults.

use std::fmt;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::cuboid::Cuboid;
use crate::json::take;
use crate::recent::RecentFiles;

/// Names `appSkin` may have, matched exactly. The Preferences dialog offers the first two.
///
/// spec: ui 12.5
pub const THEMES: [&str; 3] = ["Light", "Dark", "Rainbow"];

/// The default model signature. The original adds ` - https://cubik.studio`; cuboid has no
/// address of its own yet.
///
/// spec: app 10.2, menus 9.1
pub const DEFAULT_SIGNATURE: &str = "Designed by %author with %cubik";

/// The key whose value `Debug` hides.
const SECRET_KEY: &str = "sketchfabToken";

/// Window positions this far from the origin, or at 0, are not restored.
const WINDOW_POSITION_LIMIT: u32 = 30_000;

/// Facts only the operating system knows. The app finds them and passes them in; this crate calls
/// no OS API.
///
/// spec: app 10.2, app 17.1
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    /// The user's Desktop folder, the first-run value of the five default folders.
    pub desktop_dir: String,
    /// The detected Minecraft folder, empty when there is none.
    pub minecraft_dir: String,
}

/// Declares `Settings` from one table, so the fields, the key names, the first-run values and the
/// reader cannot drift apart. Rows are in the order the original writes its file.
macro_rules! settings {
    ($env:ident;
     $($(#[$doc:meta])* $field:ident: $ty:ty = $key:literal => $first:expr;)*) => {
        /// All settings: the keys of the original's `cubikdata.json` with their names, value
        /// formats and write order, so that imported files round-trip, then `recentFiles`, the
        /// port's [`Cuboid`] section and the keys no build of cuboid knows.
        ///
        /// Strings that were never set are `None` and written as `null`.
        ///
        /// spec: app 10.1, app 10.2, arch 15
        #[derive(Clone, PartialEq, Serialize)]
        pub struct Settings {
            $($(#[$doc])* #[serde(rename = $key)] pub $field: $ty,)*
            /// Files opened or saved lately.
            ///
            /// spec: app 11
            #[serde(rename = "recentFiles")]
            pub recent_files: RecentFiles,
            /// What the port stores beyond the original.
            pub cuboid: Cuboid,
            /// Keys no build of cuboid knows, kept in file order and written back.
            #[serde(flatten)]
            pub(crate) extra: Map<String, Value>,
        }

        /// Hides the Sketchfab token (arch 16: tokens are never logged).
        impl fmt::Debug for Settings {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut shown = f.debug_struct("Settings");
                $(
                    if $key == SECRET_KEY {
                        shown.field(stringify!($field), &format_args!("<redacted>"));
                    } else {
                        shown.field(stringify!($field), &self.$field);
                    }
                )*
                shown
                    .field("recent_files", &self.recent_files)
                    .field("cuboid", &self.cuboid)
                    .field("extra", &self.extra)
                    .finish()
            }
        }

        impl Settings {
            /// The settings of a first run: no file, an empty file or one that cannot be used.
            ///
            /// spec: menus 9.1
            pub fn first_run($env: &Environment) -> Self {
                Self {
                    $($field: $first,)*
                    recent_files: RecentFiles::default(),
                    cuboid: Cuboid::default(),
                    extra: Map::new(),
                }
            }

            /// Moves every known key out of `map`. A key the file lacks takes its value from
            /// `missing`, an unusable value the first-run value of its key.
            fn read_keys(map: &mut Map<String, Value>, first: &Self, missing: Self) -> Self {
                let mut settings = missing;
                $(take(map, $key, &mut settings.$field, &first.$field);)*
                settings
            }
        }
    };
}

settings! { env;
    /// Template of the default model signature; `%author`, `%email` and `%cubik` are replaced.
    /// At most 256 characters in the dialog.
    signature_template: String = "signatureTemplate" => DEFAULT_SIGNATURE.to_owned();
    /// Use the operating system's file dialogs.
    use_native_file_browser: bool = "useNativeFileBrowser" => false;
    /// The built-in browser lists hidden entries.
    show_hidden_files: bool = "showHiddenFiles" => false;
    /// Add the default favourites at every launch.
    readd_default_favourites: bool = "readdDefaultFavourites" => false;
    /// Minutes between backups. The dialog stores at least 5; loading does not clamp.
    backup_every_minutes: i32 = "backupEveryMinutes" => 10;
    /// Window position at the last quit or Preferences OK; see [`Settings::saved_window_position`].
    window_x: i32 = "windowX" => 0;
    window_y: i32 = "windowY" => 0;
    /// Panels shown while no edit mode is active.
    open_element_create: bool = "openElementCreate" => true;
    open_element_tools: bool = "openElementTools" => false;
    open_elements_list: bool = "openElementsList" => false;
    open_elements_material: bool = "openElementsMaterial" => false;
    open_voxel_tools: bool = "openVoxelTools" => false;
    open_gizmos_tools: bool = "openGizmosTools" => false;
    open_mesh_create: bool = "openMeshCreate" => false;
    open_mesh_tools: bool = "openMeshTools" => false;
    open_meshes_list: bool = "openMeshesList" => false;
    open_paint_tools: bool = "openPaintTools" => false;
    open_color_tools: bool = "openColorTools" => false;
    open_palette: bool = "openPalette" => false;
    /// Draw hidden elements as white transparent boxes. A key the file lacks counts as on.
    render_hidden_elements: bool = "renderHiddenElements" => false;
    /// Draw hidden elements in the main view too.
    render_hidden_elements_main_view: bool = "renderHiddenElementsMainView" => false;
    /// Maximize the main view when the preview opens.
    maximize_3d_view_on_preview: bool = "maximize3dViewOnPreview" => true;
    /// Show tooltips on hover.
    show_classic_tooltips: bool = "showClassicTooltips" => true;
    /// Player name for the third-person preview; `None` and empty both mean the bundled skin.
    minecraft_skin: Option<String> = "minecraftSkin" => None;
    /// Theme name, one of [`THEMES`]. Anything else is read as `Light` (app 0.2 D10).
    app_skin: String = "appSkin" => "Light".to_owned();
    /// Start folders of the file dialogs; `None` means the Desktop.
    path_load_save: Option<String> = "pathLoadSave" => Some(env.desktop_dir.clone());
    path_import: Option<String> = "pathImport" => Some(env.desktop_dir.clone());
    path_export: Option<String> = "pathExport" => Some(env.desktop_dir.clone());
    path_textures: Option<String> = "pathTextures" => Some(env.desktop_dir.clone());
    path_meshes: Option<String> = "pathMeshes" => Some(env.desktop_dir.clone());
    /// Minecraft folder; `None` counts as a folder that does not exist.
    path_minecraft: Option<String> = "pathMinecraft" => Some(env.minecraft_dir.clone());
    /// Executable that edits textures.
    path_texture_editor: Option<String> = "pathTextureEditor" => Some(String::new());
    /// Save textures as PNG beside the Minecraft JSON export.
    minecraft_export_textures: bool = "minecraftExportTextures" => false;
    /// Centre model and origin on OBJ import. A key the file lacks counts as off.
    obj_center_on_import: bool = "objCenterOnImport" => true;
    /// Sketchfab API token, letters and digits.
    sketchfab_token: Option<String> = "sketchfabToken" => Some(String::new());
    /// Hide format groups in the Import and Export dialogs.
    format_hide_minecraft: bool = "formatHideMinecraft" => false;
    format_hide_stonehearth: bool = "formatHideStonehearth" => false;
    format_hide_staxel: bool = "formatHideStaxel" => false;
    format_hide_voxel: bool = "formatHideVoxel" => false;
    format_hide_mesh: bool = "formatHideMesh" => false;
    /// Pad the textures of a generated atlas.
    adv_atlas_padding: bool = "advAtlasPadding" => false;
    /// Always generate a square atlas. A key the file lacks counts as off.
    adv_atlas_square: bool = "advAtlasSquare" => true;
    /// Key bindings as comma-separated key names (menus 6.6). Empty or `None` means the built-in
    /// keys; a key the file lacks is `None`.
    adv_keys_rotate: Option<String> = "advKeysRotate" => Some("Alpha1, Keypad1".to_owned());
    adv_keys_scale: Option<String> = "advKeysScale" => Some("Alpha2, Keypad2".to_owned());
    adv_keys_clone: Option<String> =
        "advKeysClone" => Some("Alpha3, Keypad3, LeftAlt, RightAlt".to_owned());
    adv_keys_pan: Option<String> = "advKeysPan" => Some("Mouse2, Space".to_owned());
    adv_keys_focus: Option<String> = "advKeysFocus" => Some("F".to_owned());
    /// Orthographic projection in the main view.
    main_cam_ortho: bool = "mainCamOrtho" => false;
    /// Show the colour of elements.
    use_elements_color: bool = "useElementsColor" => true;
    /// Emulate the tint colours of elements.
    emulate_element_tint_color: bool = "emulateElementTintColor" => true;
    /// Marker at the centre of 3D space.
    center_point: bool = "centerPoint" => false;
    /// Soft shadows for voxels.
    render_shadows: bool = "renderShadows" => false;
    /// 4x MSAA.
    render_antialiasing: bool = "renderAntialiasing" => false;
}

impl Settings {
    /// The values of keys that an existing file lacks: the field initialisers of the original,
    /// which differ from the first-run values for a few keys.
    ///
    /// spec: menus 9.1
    fn missing_keys(first: &Self) -> Self {
        Self {
            render_hidden_elements: true,
            obj_center_on_import: false,
            adv_atlas_square: false,
            adv_keys_rotate: None,
            adv_keys_scale: None,
            adv_keys_clone: None,
            adv_keys_pan: None,
            adv_keys_focus: None,
            ..first.clone()
        }
    }

    /// Builds settings from the root object of a file: the original's keys, then `recentFiles`
    /// and `cuboid`; whatever is left is kept as unknown keys. Never fails: each key is read on
    /// its own.
    ///
    /// spec: app 10.1, app 0.2 D10
    pub(crate) fn from_map(mut map: Map<String, Value>, env: &Environment) -> Self {
        let first = Self::first_run(env);
        let mut settings = Self::read_keys(&mut map, &first, Self::missing_keys(&first));
        if !THEMES.contains(&settings.app_skin.as_str()) {
            log::warn!(
                "settings: unknown theme `{}`, using Light",
                settings.app_skin
            );
            settings.app_skin = first.app_skin;
        }
        // Written fresh by every save.
        map.shift_remove("version");
        if let Some(value) = map.shift_remove("recentFiles") {
            settings.recent_files = RecentFiles::from_json(&value);
        }
        match map.shift_remove("cuboid") {
            Some(Value::Object(section)) => settings.cuboid = Cuboid::from_map(section),
            Some(_) => log::warn!("settings: unusable value for `cuboid`, using the default"),
            None => {}
        }
        settings.extra = map;
        settings
    }

    /// Where to put the window at start-up, unless the user holds Left Alt: the saved top-left
    /// corner, or `None` when either coordinate is 0 or has a magnitude of 30000 or more (the
    /// window was never moved, or was last seen off screen).
    ///
    /// spec: app 1.2 step 5
    pub fn saved_window_position(&self) -> Option<(i32, i32)> {
        let usable = |v: i32| v != 0 && v.unsigned_abs() < WINDOW_POSITION_LIMIT;
        (usable(self.window_x) && usable(self.window_y)).then_some((self.window_x, self.window_y))
    }
}

#[cfg(test)]
mod tests;
