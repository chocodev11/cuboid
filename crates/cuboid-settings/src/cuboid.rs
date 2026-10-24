//! The `cuboid` section: what the port keeps beyond the original's keys.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::json::take;
use crate::locations::Locations;

/// Width of the zone that holds the managers group, in pixels.
///
/// spec: ui 2.2
pub const MANAGERS_WIDTH_MIN: i32 = 180;
pub const MANAGERS_WIDTH_MAX: i32 = 300;
/// Width of the zone that holds the helpers group, in pixels.
///
/// spec: ui 2.2
pub const HELPERS_WIDTH_MIN: i32 = 350;
pub const HELPERS_WIDTH_MAX: i32 = 640;

/// Port-only settings, stored under the `cuboid` key of the settings file. Keys that this build
/// does not know are kept and written back.
///
/// spec: arch 15
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cuboid {
    /// The managers group is docked in the left zone and the helpers in the right zone. Written
    /// when a docking command runs.
    ///
    /// spec: ui 2.2, ui 2.7
    pub managers_on_left: bool,
    /// Between [`MANAGERS_WIDTH_MIN`] and [`MANAGERS_WIDTH_MAX`]. Written on grip release.
    ///
    /// spec: ui 2.3, ui 2.7
    pub managers_width: i32,
    /// Between [`HELPERS_WIDTH_MIN`] and [`HELPERS_WIDTH_MAX`]. Written on grip release.
    ///
    /// spec: ui 2.3, ui 2.7
    pub helpers_width: i32,
    /// Client size of the window in physical pixels, restored at the next start.
    ///
    /// spec: app 1.1
    pub window_width: u32,
    pub window_height: u32,
    /// Replaces the UI scale computed from the display dpi.
    ///
    /// spec: ui 1.2
    pub scale_override: Option<f32>,
    /// Folder of the user's own picture set that replaces the built-in art.
    ///
    /// spec: ui 15.1
    pub skin_pack: Option<String>,
    /// Export profile last chosen per format, by format key; the text is owned by `cuboid-io`.
    ///
    /// spec: interop 31.1
    pub profiles: BTreeMap<String, String>,
    /// Overrides of `Compat` switches by field name; a switch that is not listed keeps the
    /// `Compat::port()` value.
    ///
    /// spec: arch 17.1
    pub compat: BTreeMap<String, bool>,
    /// Recent and favourite folders of the built-in file browser.
    ///
    /// spec: menus 10.10
    pub locations: Locations,
    /// Replaces `%author` in the signature template; empty when unknown.
    ///
    /// spec: app 10.2, menus 9.1
    pub author: String,
    /// Replaces `%email` in the signature template; empty when unknown.
    ///
    /// spec: app 10.2, menus 9.1
    pub email: String,
    /// The user allowed the skin lookup by player name.
    ///
    /// spec: app 9.7
    pub skin_lookup: bool,
    /// The user allowed the update check.
    ///
    /// spec: app 18.2
    pub update_check: bool,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl Default for Cuboid {
    fn default() -> Self {
        Self {
            managers_on_left: false,
            managers_width: 220,
            helpers_width: 350,
            window_width: 1000,
            window_height: 700,
            scale_override: None,
            skin_pack: None,
            profiles: BTreeMap::new(),
            compat: BTreeMap::new(),
            locations: Locations::default(),
            author: String::new(),
            email: String::new(),
            skin_lookup: false,
            update_check: false,
            extra: Map::new(),
        }
    }
}

impl Cuboid {
    /// Reads the section key by key; an unusable value takes the default of its key and an
    /// out-of-range width is clamped.
    pub(crate) fn from_map(mut map: Map<String, Value>) -> Self {
        let first = Self::default();
        let mut cuboid = first.clone();
        macro_rules! read {
            ($($key:literal => $field:ident),* $(,)?) => {
                $(take(&mut map, $key, &mut cuboid.$field, &first.$field);)*
            };
        }
        read! {
            "managersOnLeft" => managers_on_left,
            "managersWidth" => managers_width,
            "helpersWidth" => helpers_width,
            "windowWidth" => window_width,
            "windowHeight" => window_height,
            "scaleOverride" => scale_override,
            "skinPack" => skin_pack,
            "profiles" => profiles,
            "compat" => compat,
            "author" => author,
            "email" => email,
            "skinLookup" => skin_lookup,
            "updateCheck" => update_check,
        }
        if let Some(value) = map.shift_remove("locations") {
            cuboid.locations = Locations::from_json(&value);
        }
        cuboid.managers_width = cuboid
            .managers_width
            .clamp(MANAGERS_WIDTH_MIN, MANAGERS_WIDTH_MAX);
        cuboid.helpers_width = cuboid
            .helpers_width
            .clamp(HELPERS_WIDTH_MIN, HELPERS_WIDTH_MAX);
        cuboid.extra = map;
        cuboid
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::locations::{Location, LocationKind};

    fn read(value: Value) -> Cuboid {
        match value {
            Value::Object(map) => Cuboid::from_map(map),
            other => panic!("not an object: {other}"),
        }
    }

    #[test]
    fn an_empty_section_is_the_default() {
        assert_eq!(read(json!({})), Cuboid::default());
    }

    #[test]
    fn the_profile_is_empty_and_the_opt_ins_are_off_by_default() {
        let cuboid = Cuboid::default();
        assert!(cuboid.author.is_empty() && cuboid.email.is_empty());
        assert!(!cuboid.skin_lookup && !cuboid.update_check);
    }

    #[test]
    fn every_key_round_trips() {
        let cuboid = Cuboid {
            managers_on_left: true,
            managers_width: 260,
            helpers_width: 500,
            window_width: 1280,
            window_height: 720,
            scale_override: Some(1.5),
            skin_pack: Some("D:\\skins".into()),
            profiles: BTreeMap::from([("json".into(), "modern:1.21".into())]),
            compat: BTreeMap::from([("dnd_unsaved_prompt".into(), false)]),
            locations: Locations {
                recent: vec![Location {
                    path: "C:\\m".into(),
                    hidden: false,
                    kind: LocationKind::Folder,
                    time: 7,
                }],
                favourites: Vec::new(),
                defaults_added: true,
            },
            author: "Alex".into(),
            email: "alex@example.org".into(),
            skin_lookup: true,
            update_check: true,
            extra: Map::new(),
        };
        let written = serde_json::to_value(&cuboid).unwrap();
        assert_eq!(read(written), cuboid);
    }

    #[test]
    fn widths_are_clamped_to_the_range_of_their_group() {
        let cuboid = read(json!({"managersWidth": 40, "helpersWidth": 9000}));
        assert_eq!(cuboid.managers_width, MANAGERS_WIDTH_MIN);
        assert_eq!(cuboid.helpers_width, HELPERS_WIDTH_MAX);
    }

    #[test]
    fn an_unusable_value_costs_only_its_own_key() {
        let cuboid = read(json!({
            "windowWidth": -5,
            "managersWidth": 250,
            "profiles": ["not", "a", "map"],
            "skinPack": "D:\\skins",
        }));
        assert_eq!(cuboid.window_width, 1000);
        assert_eq!(cuboid.managers_width, 250);
        assert!(cuboid.profiles.is_empty());
        assert_eq!(cuboid.skin_pack.as_deref(), Some("D:\\skins"));
    }

    #[test]
    fn unknown_keys_are_written_back() {
        let written = serde_json::to_value(read(json!({"future": {"a": 1}}))).unwrap();
        assert_eq!(written["future"], json!({"a": 1}));
    }
}
