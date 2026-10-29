//! What the UI needs before its first frame: the interface font and the stored preferences.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cuboid_ui::{FontSet, Prefs};

use crate::platform::Dirs;

/// File name of the interface preferences, next to `settings.json`.
const PREFS_FILE: &str = "ui.json";

/// Files tried for the interface font, first readable wins. All of them cover Vietnamese.
fn font_candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();
    if cfg!(windows) {
        let root =
            std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), Into::into);
        for name in ["segoeui.ttf", "arial.ttf", "tahoma.ttf"] {
            list.push(root.join("Fonts").join(name));
        }
    } else if cfg!(target_os = "macos") {
        for name in [
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ] {
            list.push(PathBuf::from(name));
        }
    } else {
        for name in [
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
        ] {
            list.push(PathBuf::from(name));
        }
    }
    list
}

/// The interface font of the platform with Vietnamese coverage. Without one the set is empty and
/// the kit looks at its own list of system fonts; egui is built without bundled fonts.
pub fn fonts() -> FontSet {
    let ui = font_candidates()
        .into_iter()
        .find_map(|path| std::fs::read(path).ok())
        .map(Arc::new);
    if ui.is_none() {
        log::warn!("no system interface font found");
    }
    FontSet {
        ui,
        ..FontSet::default()
    }
}

pub fn prefs_file(dirs: &Dirs) -> PathBuf {
    dirs.config.join(PREFS_FILE)
}

/// Loads the interface preferences; a missing, oversize or broken file gives the defaults.
pub fn load_prefs(path: &Path) -> Prefs {
    match cuboid_settings::read_capped(path, cuboid_settings::MAX_FILE) {
        Ok(bytes) => Prefs::from_json(&String::from_utf8_lossy(&bytes)),
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!("cannot read {}: {error}", path.display());
            }
            Prefs::default()
        }
    }
}

/// Writes the preferences atomically; runs on the I/O thread.
pub fn write_prefs(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    cuboid_io::write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_survive_a_write_and_a_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join(PREFS_FILE);
        assert_eq!(load_prefs(&path), Prefs::default());
        let prefs = Prefs {
            ui_scale: 1.25,
            ..Prefs::default()
        };
        write_prefs(&path, &prefs.to_json()).unwrap();
        assert_eq!(load_prefs(&path), prefs);
        std::fs::write(&path, b"{ broken").unwrap();
        assert_eq!(load_prefs(&path), Prefs::default());
    }

    #[test]
    fn an_oversize_preferences_file_is_refused_and_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PREFS_FILE);
        let prefs = Prefs {
            ui_scale: 1.25,
            ..Prefs::default()
        };
        // Valid JSON that would load, padded over the cap.
        let mut text = prefs.to_json();
        text.push_str(&" ".repeat(usize::try_from(cuboid_settings::MAX_FILE).unwrap()));
        std::fs::write(&path, text).unwrap();
        assert_eq!(load_prefs(&path), Prefs::default());
        std::fs::write(&path, prefs.to_json()).unwrap();
        assert_eq!(load_prefs(&path), prefs);
    }

    #[test]
    fn the_settings_writer_names_its_temporary_files_like_the_atomic_writer() {
        // The settings crate cannot use cuboid-io, and the cleanup of the atomic writer has to
        // find the leftovers of a killed settings write in the shared config folder.
        assert_eq!(cuboid_settings::TEMP_PREFIX, cuboid_io::TEMP_PREFIX);
    }

    #[test]
    fn an_old_file_gets_the_new_accent_and_is_not_asked_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PREFS_FILE);
        std::fs::write(&path, br#"{"accent": [79, 140, 255], "nav": "Blender"}"#).unwrap();
        let prefs = load_prefs(&path);
        assert_eq!(prefs.accent, Prefs::default().accent);
        assert_eq!(prefs.nav, cuboid_ui::NavPreset::Blender);
        assert!(prefs.nav_chosen);
        // No file at all: the start screen asks.
        assert!(!load_prefs(&dir.path().join("none.json")).nav_chosen);
    }

    #[test]
    fn a_windows_machine_has_a_vietnamese_font() {
        if cfg!(windows) {
            assert!(fonts().ui.is_some());
        }
    }
}
