//! Reading and writing the settings file, and importing the original's stored values.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::cuboid::{HELPERS_WIDTH_MAX, HELPERS_WIDTH_MIN, MANAGERS_WIDTH_MAX, MANAGERS_WIDTH_MIN};
use crate::locations::Locations;
use crate::schema::{Environment, Settings};

/// Name of the settings file in the port's configuration folder.
///
/// spec: arch 15
pub const FILE_NAME: &str = "settings.json";

/// Format version written as `"version"`.
const VERSION: u32 = 1;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// Why a settings file is not usable as a whole.
///
/// spec: app 0.2 D10
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the top level is not a JSON object")]
    NotAnObject,
}

/// A file that could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("cannot read {}: {source}", .path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("cannot write {}: {source}", .path.display())]
    Write { path: PathBuf, source: io::Error },
}

/// Where loaded settings came from.
#[derive(Debug)]
pub enum Origin {
    /// The file was read. Keys with unusable values took their defaults.
    File,
    /// The file is missing, empty or `null`: the first-run settings.
    ///
    /// spec: app 1.2 step 5
    Fresh,
    /// The file is not valid JSON or not an object: the first-run settings. The file was moved to
    /// `kept_as` (the name with `.bad` appended) so that it is not lost, or stays where it is
    /// when that failed.
    ///
    /// spec: app 0.2 D10
    Rejected {
        error: ParseError,
        kept_as: Option<PathBuf>,
    },
}

/// The result of [`Settings::load`].
#[derive(Debug)]
pub struct Loaded {
    pub settings: Settings,
    pub origin: Origin,
}

/// Values of the original's per-user storage, read by the app on the first run and passed in.
/// Nothing is written back to the original's locations.
///
/// spec: app 10.4, arch 15
#[derive(Clone, Copy, Default)]
pub struct Legacy<'a> {
    /// The bytes of the original's `cubikdata.json`.
    pub data: Option<&'a [u8]>,
    /// The string `CubikFileBrowser` of the preference store.
    pub locations: Option<&'a str>,
    /// The integer `cubikFilebrowser`: not 0 once the default favourites were added.
    pub favourites_marker: Option<i32>,
    /// The int `managersWidth` or `ManagersWidth`, whichever the store has.
    pub managers_width: Option<i32>,
    /// The int `helpersWidth` or `HelpersWidth`, whichever the store has.
    pub helpers_width: Option<i32>,
}

/// Shows the size of the file instead of its content, which may hold the Sketchfab token.
impl fmt::Debug for Legacy<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Legacy")
            .field("data", &self.data.map(<[u8]>::len))
            .field("locations", &self.locations)
            .field("favourites_marker", &self.favourites_marker)
            .field("managers_width", &self.managers_width)
            .field("helpers_width", &self.helpers_width)
            .finish()
    }
}

/// The file as written: the format version, then the settings.
#[derive(Serialize)]
struct FileRef<'a> {
    version: u32,
    #[serde(flatten)]
    settings: &'a Settings,
}

impl Settings {
    /// Reads settings from the text of a file. `None` for empty text and for `null`, which mean
    /// the first run. Everything else must be a JSON object; its keys are read one by one, so a
    /// key with an unusable value costs only that key.
    ///
    /// spec: app 10.1, app 0.2 D10
    pub fn parse(bytes: &[u8], env: &Environment) -> Result<Option<Self>, ParseError> {
        let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
        if bytes.trim_ascii().is_empty() {
            return Ok(None);
        }
        match serde_json::from_slice(bytes)? {
            Value::Null => Ok(None),
            Value::Object(map) => Ok(Some(Self::from_map(map, env))),
            _ => Err(ParseError::NotAnObject),
        }
    }

    /// Loads the settings file. A missing file is not an error. A file that is not usable as a
    /// whole is moved aside as `<name>.bad`, and the first-run settings are returned with the
    /// reason in [`Origin::Rejected`].
    ///
    /// spec: app 10.1, app 0.2 D10
    pub fn load(path: &Path, env: &Environment) -> Result<Loaded, SettingsError> {
        let fresh = |origin| {
            log::info!("CubikData: Fresh data created");
            Loaded {
                settings: Self::first_run(env),
                origin,
            }
        };
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(fresh(Origin::Fresh)),
            Err(source) => {
                return Err(SettingsError::Read {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        match Self::parse(&bytes, env) {
            Ok(Some(settings)) => Ok(Loaded {
                settings,
                origin: Origin::File,
            }),
            Ok(None) => Ok(fresh(Origin::Fresh)),
            Err(error) => {
                let kept_as = keep_aside(path);
                Ok(fresh(Origin::Rejected { error, kept_as }))
            }
        }
    }

    /// Writes the whole settings file: pretty JSON, version first, the keys of the original in
    /// their order, `recentFiles`, `cuboid` and then the unknown keys. The write goes to a
    /// temporary file in the same folder, which replaces the old file, so a crash leaves either
    /// the old or the new content. Missing folders are created.
    ///
    /// spec: app 10.1, app 0.2 D8
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        let write_error = |source| SettingsError::Write {
            path: path.to_owned(),
            source,
        };
        let file = FileRef {
            version: VERSION,
            settings: self,
        };
        let mut text =
            serde_json::to_string_pretty(&file).map_err(|err| write_error(err.into()))?;
        text.push('\n');
        write_atomic(path, text.as_bytes()).map_err(write_error)
    }

    /// The settings of a first run that moves from the original: its `cubikdata.json` (read key by
    /// key like the settings file; an unusable file is logged and ignored), its file browser
    /// locations and its panel widths. What the original lacks stays at the first-run value.
    ///
    /// spec: app 10.4, arch 15, ui 2.7
    pub fn import_legacy(legacy: &Legacy, env: &Environment) -> Self {
        let parsed = legacy.data.map(|data| Self::parse(data, env));
        let mut settings = match parsed {
            Some(Ok(Some(settings))) => settings,
            Some(Err(err)) => {
                log::warn!("settings: the original's cubikdata.json is not usable: {err}");
                Self::first_run(env)
            }
            _ => Self::first_run(env),
        };
        let cuboid = &mut settings.cuboid;
        cuboid.locations = legacy
            .locations
            .map(Locations::from_legacy)
            .unwrap_or_default();
        cuboid.locations.defaults_added =
            legacy.favourites_marker.is_some_and(|marker| marker != 0);
        if let Some(width) = legacy.managers_width {
            cuboid.managers_width = width.clamp(MANAGERS_WIDTH_MIN, MANAGERS_WIDTH_MAX);
        }
        if let Some(width) = legacy.helpers_width {
            cuboid.helpers_width = width.clamp(HELPERS_WIDTH_MIN, HELPERS_WIDTH_MAX);
        }
        settings
    }
}

/// Moves a rejected file to `<path>.bad`, replacing an older one.
fn keep_aside(path: &Path) -> Option<PathBuf> {
    let kept = path.with_added_extension("bad");
    match fs::rename(path, &kept) {
        Ok(()) => Some(kept),
        Err(err) => {
            log::error!(
                "settings: cannot keep {} as {}: {err}",
                path.display(),
                kept.display()
            );
            None
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|err| err.error)?;
    Ok(())
}

#[cfg(test)]
mod tests;
