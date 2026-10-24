//! The port's extension entry `cuboid.json`: data that Cubik does not store (native 8.4).
//!
//! It holds the Minecraft item overrides. The entry has its own version; Cubik ignores it and
//! drops it when it saves the file again.

use std::sync::Arc;

use cuboid_model::OverrideCase;
use serde::{Deserialize, Serialize};

/// The version of the entry that this crate writes.
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Entry {
    version: u32,
    #[serde(default)]
    overrides: Vec<Case>,
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

/// The text of the entry for these overrides.
pub fn write(overrides: &[OverrideCase]) -> Vec<u8> {
    let entry = Entry {
        version: VERSION,
        overrides: overrides.iter().map(Case::from).collect(),
    };
    // A tree of strings and numbers always serialises.
    serde_json::to_vec_pretty(&entry).unwrap_or_default()
}

/// The overrides of an entry. A newer version is read as far as it matches.
pub fn read(bytes: &[u8]) -> Result<Arc<[OverrideCase]>, serde_json::Error> {
    let entry: Entry = serde_json::from_slice(bytes)?;
    Ok(entry
        .overrides
        .into_iter()
        .map(OverrideCase::from)
        .collect())
}
