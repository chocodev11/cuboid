//! Folders of the port and OS values the settings crate takes as input (spec: arch 15).

use std::path::{Path, PathBuf};

use cuboid_settings::{Environment, FILE_NAME};
use directories::{BaseDirs, ProjectDirs, UserDirs};

/// A file next to the executable that keeps every folder next to it, for a portable install.
const PORTABLE_MARKER: &str = "cuboid.portable";
/// The environment variable that names the folder to keep every folder in.
const HOME_VARIABLE: &str = "CUBOID_HOME";

/// The folders of the port.
#[derive(Debug, Clone)]
pub struct Dirs {
    /// `settings.json`.
    pub config: PathBuf,
    /// `logs/`, `recovery/`.
    pub data: PathBuf,
}

impl Dirs {
    /// The folders of this installation: below the folder that `CUBOID_HOME` names, as the
    /// portable layout (the tests of `cuboid mcp` set it so that they never write into the
    /// user's log); else next to the executable when the portable marker is there, else the
    /// user's folders. Nothing is created here.
    pub fn detect() -> Dirs {
        if let Some(home) = std::env::var_os(HOME_VARIABLE) {
            return Dirs::under(Path::new(&home));
        }
        let portable = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_owned))
            .filter(|dir| dir.join(PORTABLE_MARKER).is_file());
        if let Some(root) = portable {
            return Dirs::under(&root);
        }
        match ProjectDirs::from("", "", "cuboid") {
            Some(dirs) => Dirs {
                config: dirs.config_dir().to_owned(),
                data: dirs.data_dir().to_owned(),
            },
            None => Dirs::under(&std::env::temp_dir().join("cuboid")),
        }
    }

    /// Both folders below one root.
    pub fn under(root: &Path) -> Dirs {
        Dirs {
            config: root.join("config"),
            data: root.join("data"),
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join(FILE_NAME)
    }

    pub fn logs(&self) -> PathBuf {
        self.data.join("logs")
    }

    pub fn recovery(&self) -> PathBuf {
        self.data.join("recovery")
    }
}

/// The values the settings crate cannot find itself: the Desktop folder and the Minecraft folder.
pub fn environment() -> Environment {
    let desktop_dir = UserDirs::new()
        .and_then(|dirs| dirs.desktop_dir().map(|dir| dir.display().to_string()))
        .unwrap_or_default();
    let minecraft_dir = BaseDirs::new()
        .map(|dirs| {
            if cfg!(target_os = "macos") {
                dirs.data_dir().join("minecraft")
            } else if cfg!(windows) {
                dirs.config_dir().join(".minecraft")
            } else {
                dirs.home_dir().join(".minecraft")
            }
        })
        .filter(|dir| dir.is_dir())
        .map(|dir| dir.display().to_string())
        .unwrap_or_default();
    Environment {
        desktop_dir,
        minecraft_dir,
    }
}

/// The ANSI code page for texture signatures (spec: arch 15). Windows-1252 off Windows.
pub fn ansi_code_page() -> u16 {
    #[cfg(windows)]
    {
        windows_registry::LOCAL_MACHINE
            .open(r"SYSTEM\CurrentControlSet\Control\Nls\CodePage")
            .and_then(|key| key.get_string("ACP"))
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(1252)
    }
    #[cfg(not(windows))]
    {
        1252
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_sit_below_the_root() {
        let dirs = Dirs::under(Path::new("root"));
        assert_eq!(dirs.settings_file(), Path::new("root/config/settings.json"));
        assert_eq!(dirs.logs(), Path::new("root/data/logs"));
        assert_eq!(dirs.recovery(), Path::new("root/data/recovery"));
    }

    #[test]
    fn the_code_page_is_a_plausible_number() {
        assert!(ansi_code_page() >= 437);
    }
}
