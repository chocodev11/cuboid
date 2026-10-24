//! The Minecraft Java version table (interop 31.2 to 31.4, 33.4): tiers of versions that share
//! the model rules, and the game-version to pack-format map.
//!
//! The data lives in `data/mc_versions.json`, is embedded at build time and parsed once. A fact
//! the specs do not give is absent; a fact the specs mark UNVERIFIED, or that the file infers, is
//! named in [`Tier::unverified`].

use std::cmp::Ordering;
use std::fmt;
use std::sync::OnceLock;

use serde::Deserialize;

/// A game version such as `1.21.4` or `26.3`: one to three numeric parts.
///
/// Parts that were not written read as 0 when two versions are ordered. A range end written with
/// fewer parts covers every version that starts with them (`1.12` covers `1.12.2`).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct McVersion {
    parts: [u16; 3],
    len: u8,
}

impl McVersion {
    /// A version with three parts.
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            parts: [major, minor, patch],
            len: 3,
        }
    }

    /// Parses `major`, `major.minor` or `major.minor.patch`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let mut parts = [0u16; 3];
        let mut len = 0u8;
        let mut pieces = text.split('.');
        for slot in &mut parts {
            let Some(piece) = pieces.next() else { break };
            *slot = piece.parse().ok()?;
            len += 1;
        }
        if len == 0 || pieces.next().is_some() {
            return None;
        }
        Some(Self { parts, len })
    }

    /// The first part.
    pub fn major(&self) -> u16 {
        self.parts[0]
    }

    /// Whether this version is at or after `from` and, when `to` is given, within it.
    fn within(&self, from: &McVersion, to: Option<&McVersion>) -> bool {
        if self.parts < from.parts {
            return false;
        }
        to.is_none_or(|to| {
            let keep = usize::from(to.len);
            self.parts.iter().take(keep).le(to.parts.iter().take(keep))
        })
    }
}

impl TryFrom<String> for McVersion {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        McVersion::parse(&text).ok_or_else(|| format!("not a game version: {text:?}"))
    }
}

impl PartialOrd for McVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for McVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts)
    }
}

impl fmt::Display for McVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown: Vec<String> = self
            .parts
            .iter()
            .take(usize::from(self.len))
            .map(u16::to_string)
            .collect();
        f.write_str(&shown.join("."))
    }
}

/// Which element rotations a tier accepts.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationRule {
    /// One axis; 0, 22.5 or 45 degrees either way (interop 2.2.2).
    Classic,
    /// One axis; any angle up to [`Tier::rotation_max_deg`] either way.
    SingleAxisFree,
    /// Any rotation on all three axes; the specs give no angle range.
    MultiAxisFree,
}

/// The folder names of block and item textures.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureDirs {
    /// `blocks/` and `items/`.
    Plural,
    /// `block/` and `item/`.
    Singular,
}

/// A range of game versions that share the model rules (one row of interop 31.2).
#[derive(Clone, Debug, Deserialize)]
pub struct Tier {
    /// The profile name, for example `java-1.21.6`.
    pub id: String,
    /// First version of the tier.
    pub from: McVersion,
    /// Last version of the tier, or none while the tier is open.
    #[serde(default)]
    pub to: Option<McVersion>,
    /// The rotation rule.
    pub rotation: RotationRule,
    /// The angle limit of [`RotationRule::SingleAxisFree`].
    #[serde(default)]
    pub rotation_max_deg: Option<f32>,
    /// Item models are described by `assets/<ns>/items/<id>.json` files (1.21.4 and later).
    pub item_definitions: bool,
    /// The texture folder names.
    pub texture_dirs: TextureDirs,
    /// The display slot names of the version.
    pub display_contexts: Vec<String>,
    /// Names of the fields above that the specs do not confirm.
    #[serde(default)]
    pub unverified: Vec<String>,
    /// Where each value comes from, and what is inferred.
    #[serde(default)]
    pub note: String,
}

impl Tier {
    /// Whether the specs confirm the field (`rotation`, `item_definitions`, `texture_dirs`,
    /// `display_contexts`).
    pub fn is_verified(&self, field: &str) -> bool {
        !self.unverified.iter().any(|name| name == field)
    }

    /// Whether the tier has a display slot of this name.
    pub fn has_display_context(&self, name: &str) -> bool {
        self.display_contexts.iter().any(|ctx| ctx == name)
    }
}

/// A resource-pack format number: `major` or `major.minor`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Deserialize)]
pub struct PackFormat {
    /// The whole number.
    pub major: u32,
    /// The part after the dot, 0 when absent.
    #[serde(default)]
    pub minor: u32,
}

/// The pack format of a range of game versions.
#[derive(Clone, Debug, Deserialize)]
pub struct PackFormatEntry {
    /// First version.
    pub from: McVersion,
    /// Last version.
    pub to: McVersion,
    /// The format number.
    #[serde(flatten)]
    pub format: PackFormat,
    /// Whether a spec confirms it.
    pub verified: bool,
    /// The spec section.
    pub source: String,
}

/// The whole table.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct VersionTable {
    /// Pack formats above this number need `min_format` and `max_format` in `pack.mcmeta`
    /// (interop 31.4).
    pub min_max_format_above: u32,
    /// The tiers, oldest first.
    pub tiers: Vec<Tier>,
    /// The known pack formats; versions missing here have no known format.
    pub pack_formats: Vec<PackFormatEntry>,
}

impl VersionTable {
    /// The tier that covers a version.
    pub fn tier_for(&self, version: McVersion) -> Option<&Tier> {
        self.tiers
            .iter()
            .find(|tier| version.within(&tier.from, tier.to.as_ref()))
    }

    /// The tier with this id.
    pub fn tier_by_id(&self, id: &str) -> Option<&Tier> {
        self.tiers.iter().find(|tier| tier.id == id)
    }

    /// The pack format of a version, when the specs give one.
    pub fn pack_format_for(&self, version: McVersion) -> Option<&PackFormatEntry> {
        self.pack_formats
            .iter()
            .find(|entry| version.within(&entry.from, Some(&entry.to)))
    }
}

static TABLE: OnceLock<VersionTable> = OnceLock::new();

/// The embedded table, parsed on first use.
pub fn mc_versions() -> &'static VersionTable {
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("../data/mc_versions.json")).unwrap_or_else(|err| {
            log::error!("data/mc_versions.json: {err}");
            VersionTable::default()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> McVersion {
        McVersion::parse(text).expect("version")
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(v("1.21.4").to_string(), "1.21.4");
        assert_eq!(v("26.3").to_string(), "26.3");
        assert!(v("1.21.11") > v("1.21.4"));
        assert!(v("26.1") > v("1.21.11"));
        for bad in ["", "1.", "a.b", "1.2.3.4", "1..2", "-1"] {
            assert!(McVersion::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn the_table_parses_and_its_tiers_do_not_overlap() {
        let table = mc_versions();
        assert_eq!(table.tiers.len(), 6);
        assert_eq!(table.min_max_format_above, 64);
        for pair in table.tiers.windows(2) {
            let [a, b] = pair else { continue };
            let end = a.to.expect("closed tier");
            assert!(end < b.from, "{} overlaps {}", a.id, b.id);
        }
        assert!(table.tiers.last().is_some_and(|tier| tier.to.is_none()));
    }

    #[test]
    fn tiers_follow_interop_31_2() {
        let table = mc_versions();
        let id = |text: &str| table.tier_for(v(text)).map(|tier| tier.id.as_str());
        assert_eq!(id("1.12"), Some("java-1.12"));
        assert_eq!(id("1.12.2"), Some("java-1.12"));
        assert_eq!(id("1.13"), Some("java-1.13"));
        assert_eq!(id("1.21.3"), Some("java-1.13"));
        assert_eq!(id("1.21.4"), Some("java-1.21.4"));
        assert_eq!(id("1.21.5"), Some("java-1.21.4"));
        assert_eq!(id("1.21.6"), Some("java-1.21.6"));
        assert_eq!(id("1.21.10"), Some("java-1.21.6"));
        assert_eq!(id("1.21.11"), Some("java-1.21.11"));
        assert_eq!(id("26.2"), Some("java-1.21.11"));
        assert_eq!(id("26.3"), Some("java-26.3"));
        assert_eq!(id("27.1"), Some("java-26.3"));
        assert_eq!(id("1.8"), None);
    }

    #[test]
    fn unverified_fields_are_marked() {
        let table = mc_versions();
        let tier = table.tier_by_id("java-1.21.6").expect("tier");
        assert_eq!(tier.rotation, RotationRule::SingleAxisFree);
        assert_eq!(tier.rotation_max_deg, Some(45.0));
        assert!(!tier.is_verified("rotation"));
        assert!(tier.is_verified("item_definitions"));
        let last = table.tier_by_id("java-26.3").expect("tier");
        assert!(last.has_display_context("on_shelf") && last.is_verified("display_contexts"));
    }

    #[test]
    fn pack_formats_are_only_the_confirmed_ones() {
        let table = mc_versions();
        let format = |text: &str| table.pack_format_for(v(text)).map(|e| e.format);
        assert_eq!(format("1.12.2").map(|f| f.major), Some(3));
        assert_eq!(format("1.11"), Some(PackFormat { major: 3, minor: 0 }));
        assert_eq!(
            format("26.3"),
            Some(PackFormat {
                major: 97,
                minor: 1
            })
        );
        assert_eq!(
            format("1.21.4"),
            Some(PackFormat {
                major: 46,
                minor: 0
            })
        );
        assert_eq!(
            format("1.21.11"),
            Some(PackFormat {
                major: 75,
                minor: 0
            })
        );
        assert_eq!(format("1.8"), None);
        assert_eq!(format("26.4"), None);
        assert!(
            table
                .pack_formats
                .iter()
                .all(|entry| !entry.source.is_empty())
        );
        assert!(table.pack_formats.iter().all(|entry| entry.verified));
    }
}
