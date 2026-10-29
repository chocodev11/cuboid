//! What the UI needs before its first frame: the interface font, the logo and the stored
//! preferences.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cuboid_assets::brand::{self, Raster};
use cuboid_model::image::{ImageFileKind, decode_file};
use cuboid_shell::RgbaImage;
use cuboid_ui::{FontSet, ImageData, LogoSet, Prefs};

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

/// A raster of the logo as its width, height and RGBA pixels with the rows from the top, or
/// `None` and a warning when the embedded file cannot be decoded. `Image` keeps its rows from the
/// bottom.
fn decode(raster: &Raster) -> Option<(u32, u32, Vec<u8>)> {
    let image = decode_file(raster.png, ImageFileKind::Other)
        .inspect_err(|err| log::warn!("cannot decode the {} pixel logo: {err}", raster.size))
        .ok()?;
    let row = image.w as usize * 4;
    let rgba = image.rgba().chunks_exact(row).rev().flatten().copied();
    Some((image.w, image.h, rgba.collect()))
}

/// The rasters of the logo that the interface draws, decoded once for the UI (logo.md 5). A raster
/// that cannot be decoded is left out; the UI shows the word alone without any.
pub fn logo() -> LogoSet {
    let rasters = brand::LOGO
        .iter()
        .filter(|raster| raster.size <= brand::UI_LARGEST)
        .filter_map(|raster| {
            let (w, h, rgba) = decode(raster)?;
            Some(ImageData {
                size: [w as usize, h as usize],
                rgba: Arc::from(rgba),
                rev: 1,
            })
        })
        .collect();
    LogoSet::new(rasters)
}

/// The icon of the window where the platform takes none from the executable: the 64 pixel logo.
/// Windows has the icon resource of `build.rs`, so nothing is decoded there (logo.md 4).
pub fn window_icon() -> Option<RgbaImage> {
    if cfg!(windows) {
        return None;
    }
    let (w, h, pixels) = decode(brand::raster(64)?)?;
    Some(RgbaImage {
        size: (w, h),
        pixels,
    })
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
    fn the_logo_has_the_interface_sizes_with_its_rows_from_the_top() {
        let logo = logo();
        for edge in [16, 20, 24, 32, 40, 48, 64, 96, 128] {
            let raster = logo.pick(edge as f32).unwrap();
            assert_eq!(raster.size, [edge, edge]);
            assert_eq!(raster.rgba.len(), edge * edge * 4);
        }
        // The 256 pixel raster is for the icon files: the interface stops at 128.
        let largest = logo.pick(200.0).unwrap();
        assert_eq!(largest.size, [128, 128]);
        // The left face of the box is in the lower half of the picture.
        let at = (80 * 128 + 48) * 4;
        assert_eq!(largest.rgba[at..at + 4], [227, 187, 59, 255]);
    }

    #[test]
    fn only_a_platform_without_an_icon_resource_gets_a_window_picture() {
        let icon = window_icon();
        assert_eq!(icon.is_some(), !cfg!(windows));
        if let Some(icon) = icon {
            assert_eq!(icon.size, (64, 64));
            assert_eq!(icon.pixels.len(), 64 * 64 * 4);
        }
    }

    #[test]
    fn a_windows_machine_has_a_vietnamese_font() {
        if cfg!(windows) {
            assert!(fonts().ui.is_some());
        }
    }
}
