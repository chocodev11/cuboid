//! The port's extension entry `cuboid.json`: data that Cubik does not store (native 8.4).
//!
//! It holds the Minecraft item overrides and the project settings (the preset of the document).
//! The entry has its own version; Cubik ignores it and drops it when it saves the file again.
//! An entry of a newer version, or one that cannot be read, is not interpreted: the loader keeps
//! its bytes (see [`Read`]) and the save writes them back unchanged.

use std::sync::Arc;

use cuboid_model::OverrideCase;
use serde::{Deserialize, Serialize};

/// The version of the entry that this crate writes and reads. A change that an older build would
/// misread raises it.
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Entry {
    version: u32,
    #[serde(default)]
    overrides: Vec<Case>,
    /// Read apart from the entry, so a damaged member does not cost the overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project: Option<serde_json::Value>,
    /// The group tree (hierarchy A1); read apart, like the project, and interpreted by `tree`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    groups: Option<serde_json::Value>,
    /// The animations (animation A2); read apart and interpreted by `anim`, after the tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    animations: Option<serde_json::Value>,
}

/// The preset of a document, as the port stores it next to the Cubik payload. Names, not the
/// types of the application: this crate does not know them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMeta {
    /// `cubik` or `minecraft`.
    pub preset: String,
    /// The game version of the `minecraft` preset, such as `1.21.11`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The document's choice for negative sizes when it differs from the preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_negative_size: Option<bool>,
}

/// What the entry of a file held.
pub(crate) struct Extension {
    pub overrides: Arc<[OverrideCase]>,
    pub project: Option<ProjectMeta>,
    /// The entry has a `project` member that does not match, which is left out.
    pub project_damaged: bool,
    /// The `groups` member, for the group tree reader.
    pub groups: Option<serde_json::Value>,
    /// The `animations` member, for the animation reader.
    pub animations: Option<serde_json::Value>,
}

/// What reading an entry gave.
pub(crate) enum Read {
    Known(Box<Extension>),
    /// The entry has a newer version than [`VERSION`]; its members are not read.
    Newer(u64),
    /// The bytes are no entry.
    Damaged(serde_json::Error),
}

#[derive(Serialize, Deserialize, Default)]
struct Case {
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angle: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blocking: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    broken: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cast: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    damage: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    damaged: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lefthanded: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pull: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pulling: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    time: Option<f32>,
}

impl From<&OverrideCase> for Case {
    fn from(c: &OverrideCase) -> Self {
        Self {
            model: c.model.clone(),
            angle: c.angle,
            blocking: c.blocking,
            broken: c.broken,
            cast: c.cast,
            damage: c.damage,
            damaged: c.damaged,
            lefthanded: c.lefthanded,
            pull: c.pull,
            pulling: c.pulling,
            time: c.time,
        }
    }
}

impl From<Case> for OverrideCase {
    fn from(c: Case) -> Self {
        Self {
            model: c.model,
            angle: c.angle,
            blocking: c.blocking,
            broken: c.broken,
            cast: c.cast,
            damage: c.damage,
            damaged: c.damaged,
            lefthanded: c.lefthanded,
            pull: c.pull,
            pulling: c.pulling,
            time: c.time,
        }
    }
}

/// The text of the entry for these overrides, this project, this group tree and these animations.
/// Animations can be many keyframes, so an entry that has them is written compact.
pub(crate) fn write(
    overrides: &[OverrideCase],
    project: Option<&ProjectMeta>,
    groups: Option<&crate::tree::Tree>,
    animations: Option<&crate::anim::List>,
) -> Vec<u8> {
    let entry = Entry {
        version: VERSION,
        overrides: overrides.iter().map(Case::from).collect(),
        project: project.and_then(|meta| serde_json::to_value(meta).ok()),
        groups: groups.and_then(|tree| serde_json::to_value(tree).ok()),
        animations: animations.and_then(|list| serde_json::to_value(list).ok()),
    };
    // A tree of strings and numbers always serialises.
    if animations.is_some() {
        serde_json::to_vec(&entry).unwrap_or_default()
    } else {
        serde_json::to_vec_pretty(&entry).unwrap_or_default()
    }
}

/// The content of an entry. A project member that does not match is left out and reported.
pub(crate) fn read(bytes: &[u8]) -> Read {
    let value: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(error) => return Read::Damaged(error),
    };
    let newer = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .filter(|version| *version > u64::from(VERSION));
    if let Some(version) = newer {
        return Read::Newer(version);
    }
    let entry: Entry = match serde_json::from_value(value) {
        Ok(entry) => entry,
        Err(error) => return Read::Damaged(error),
    };
    let project = entry
        .project
        .map(|value| serde_json::from_value::<ProjectMeta>(value).ok());
    Read::Known(Box::new(Extension {
        overrides: entry
            .overrides
            .into_iter()
            .map(OverrideCase::from)
            .collect(),
        project_damaged: project == Some(None),
        project: project.flatten(),
        groups: entry.groups,
        animations: entry.animations,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(bytes: &[u8]) -> Extension {
        match read(bytes) {
            Read::Known(extension) => *extension,
            Read::Newer(version) => panic!("newer version {version}"),
            Read::Damaged(error) => panic!("damaged: {error}"),
        }
    }

    #[test]
    fn the_project_member_round_trips_and_a_damaged_one_is_reported() {
        let meta = ProjectMeta {
            preset: "minecraft".to_owned(),
            version: Some("1.21.11".to_owned()),
            allow_negative_size: Some(false),
        };
        let read_back = known(&write(&[], Some(&meta), None, None));
        assert_eq!(read_back.project, Some(meta));
        assert!(read_back.overrides.is_empty());
        assert!(!read_back.project_damaged);

        let plain = known(&write(&[], None, None, None));
        assert_eq!(plain.project, None);
        assert!(!plain.project_damaged);

        let damaged = br#"{"version":1,"overrides":[{"model":"item/x"}],"project":{"preset":7}}"#;
        let read_back = known(damaged);
        assert_eq!(read_back.project, None);
        assert!(read_back.project_damaged);
        assert_eq!(read_back.overrides.len(), 1);
    }

    #[test]
    fn a_newer_version_is_not_read_and_text_that_is_no_entry_is_damaged() {
        // The members of a newer version may mean something else, so they are not read at all.
        let newer = br#"{"version":2,"overrides":"changed shape","extra":[1]}"#;
        assert!(matches!(read(newer), Read::Newer(2)));
        assert!(matches!(read(b"{ not json"), Read::Damaged(_)));
        assert!(matches!(read(br#"{"overrides":[]}"#), Read::Damaged(_)));
    }
}
