//! Recent and favourite folders of the built-in file browser.

use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Recent locations kept, the oldest dropped first.
pub const MAX_RECENT: usize = 5;

/// What a location is, with the names the original stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocationKind {
    Root,
    File,
    Folder,
    LogicalDrive,
}

/// One entry of the Recent or Favourites list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub path: String,
    pub hidden: bool,
    pub kind: LocationKind,
    /// Last access in seconds since 1970-01-01 00:00:00. The original stores local wall-clock
    /// time without an offset; only the order of the stamps matters, so it is read as UTC.
    pub time: i64,
}

/// The two lists of the built-in file browser, kept in the order they are stored.
///
/// spec: menus 10.10
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Locations {
    pub recent: Vec<Location>,
    pub favourites: Vec<Location>,
    /// The default favourites were added once (the original's `cubikFilebrowser` marker).
    pub defaults_added: bool,
}

/// Whether two stored paths name the same place: Windows ignores ASCII case, other systems compare
/// the paths as written. The app passes paths in the form the system gave them.
pub(crate) fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

impl Locations {
    /// Reads the stored value list by list and entry by entry, so that a location that cannot be
    /// read costs only itself. A value that is not an object is the default.
    ///
    /// spec: app 0.2 D10
    pub(crate) fn from_json(value: &Value) -> Self {
        let Value::Object(object) = value else {
            log::warn!("settings: unusable value for `locations`, using the default");
            return Self::default();
        };
        let list = |key: &str| -> Vec<Location> {
            let Some(Value::Array(entries)) = object.get(key) else {
                return Vec::new();
            };
            entries.iter().filter_map(location_from_json).collect()
        };
        Self {
            recent: list("recent"),
            favourites: list("favourites"),
            defaults_added: object
                .get("defaultsAdded")
                .and_then(Value::as_bool)
                .unwrap_or_default(),
        }
    }

    /// Adds a recent location. A path that is listed already changes nothing, not even its time;
    /// when the list is full the oldest entry goes first.
    ///
    /// spec: menus 10.10
    pub fn add_recent(&mut self, location: Location) {
        let listed = |known: &Location| same_path(&known.path, &location.path);
        if self.recent.iter().any(listed) {
            return;
        }
        while self.recent.len() >= MAX_RECENT {
            let Some((oldest, _)) = self
                .recent
                .iter()
                .enumerate()
                .min_by_key(|(_, known)| known.time)
            else {
                break;
            };
            self.recent.remove(oldest);
        }
        self.recent.push(location);
    }

    /// Reads the text the original keeps under `CubikFileBrowser`: line feeds are removed, the
    /// text between `SavedLocationEntry {` and `}` is split at commas and needs exactly five
    /// non-empty fields (kind of list, path, hidden flag, location type, local date and time).
    /// An entry that cannot be read is skipped. `defaults_added` stays unset.
    ///
    /// spec: menus 10.10
    pub fn from_legacy(text: &str) -> Self {
        let text = text.replace('\n', "");
        let mut locations = Self::default();
        for chunk in text.split("SavedLocationEntry {").skip(1) {
            let body = chunk.split('}').next().unwrap_or_default();
            let fields: Vec<&str> = body.split(',').collect();
            let entry = match fields[..] {
                [list, path, hidden, kind, time] if !fields.contains(&"") => {
                    legacy_entry(list, path, hidden, kind, time)
                }
                _ => {
                    log::warn!("Not enough arguments");
                    continue;
                }
            };
            match entry {
                Ok((LegacyList::Recent, location)) => locations.recent.push(location),
                Ok((LegacyList::Favourite, location)) => locations.favourites.push(location),
                Err(why) => log::warn!("settings: saved location skipped: {why}"),
            }
        }
        locations
    }
}

fn location_from_json(value: &Value) -> Option<Location> {
    Location::deserialize(value)
        .inspect_err(|_| log::warn!("settings: unusable saved location dropped"))
        .ok()
}

enum LegacyList {
    Recent,
    Favourite,
}

/// The five fields of a saved location; the error says which one is not a value the original
/// writes.
fn legacy_entry(
    list: &str,
    path: &str,
    hidden: &str,
    kind: &str,
    time: &str,
) -> Result<(LegacyList, Location), &'static str> {
    let list = match list {
        "Recent" => LegacyList::Recent,
        "Favorite" => LegacyList::Favourite,
        _ => return Err("unknown list name"),
    };
    let hidden = match hidden {
        "True" => true,
        "False" => false,
        _ => return Err("unknown hidden flag"),
    };
    let kind = match kind {
        "Root" => LocationKind::Root,
        "File" => LocationKind::File,
        "Folder" => LocationKind::Folder,
        "LogicalDrive" => LocationKind::LogicalDrive,
        _ => return Err("unknown location type"),
    };
    let time = DateTime::strptime("%m/%d/%Y %H:%M:%S", time)
        .and_then(|local| local.to_zoned(TimeZone::UTC))
        .map_err(|_| "unreadable date")?
        .timestamp()
        .as_second();
    let path = path.to_owned();
    Ok((
        list,
        Location {
            path,
            hidden,
            kind,
            time,
        },
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::test_log;

    fn place(path: &str, time: i64) -> Location {
        Location {
            path: path.to_owned(),
            hidden: false,
            kind: LocationKind::Folder,
            time,
        }
    }

    fn paths(locations: &[Location]) -> Vec<&str> {
        locations.iter().map(|l| l.path.as_str()).collect()
    }

    #[test]
    fn the_original_text_is_read_into_both_lists() {
        let text = "SavedLocations {\n\
            SavedLocationEntry {\nRecent,\nC:\\Models,\nFalse,\nFolder,\n10/18/2026 09:30:15\n}\n\
            SavedLocationEntry {\nFavorite,\nC:\\Users\\me\\Desktop,\nTrue,\nFolder,\n01/02/2020 00:00:00\n}\n\
            SavedLocationEntry {\nFavorite,\nD:\\,\nFalse,\nLogicalDrive,\n01/02/2020 00:00:01\n}\n\
            }\n";
        let locations = Locations::from_legacy(text);
        assert!(!locations.defaults_added);
        assert_eq!(
            locations.recent,
            [Location {
                path: "C:\\Models".into(),
                hidden: false,
                kind: LocationKind::Folder,
                time: 1_792_315_815,
            }]
        );
        assert_eq!(
            paths(&locations.favourites),
            ["C:\\Users\\me\\Desktop", "D:\\"]
        );
        assert!(locations.favourites[0].hidden);
        assert_eq!(locations.favourites[1].kind, LocationKind::LogicalDrive);
        assert_eq!(
            locations.favourites[1].time - locations.favourites[0].time,
            1
        );
    }

    #[test]
    fn entries_without_five_non_empty_fields_are_skipped() {
        let text = "SavedLocations {\n\
            SavedLocationEntry {\nRecent,\nC:\\a,\nFalse,\nFolder\n}\n\
            SavedLocationEntry {\nRecent,\n,\nFalse,\nFolder,\n01/02/2020 00:00:00\n}\n\
            SavedLocationEntry {\nRecent,\nC:\\b,\nFalse,\nFolder,\n01/02/2020 00:00:00,\nextra\n}\n\
            SavedLocationEntry {\nRecent,\nC:\\ok,\nFalse,\nFolder,\n01/02/2020 00:00:00\n}\n\
            }\n";
        test_log::capture();
        let locations = Locations::from_legacy(text);
        assert_eq!(paths(&locations.recent), ["C:\\ok"]);
        assert!(locations.favourites.is_empty());
        assert_eq!(Locations::from_legacy(""), Locations::default());
        assert_eq!(test_log::lines(), ["Not enough arguments"; 3]);
    }

    #[test]
    fn an_unknown_value_is_skipped_and_is_not_reported_as_missing_arguments() {
        let text = "SavedLocationEntry {\nWeird,\nC:\\a,\nFalse,\nFolder,\n01/02/2020 00:00:00\n}\n\
            SavedLocationEntry {\nRecent,\nC:\\b,\nMaybe,\nFolder,\n01/02/2020 00:00:00\n}\n\
            SavedLocationEntry {\nRecent,\nC:\\c,\nFalse,\nShelf,\n01/02/2020 00:00:00\n}\n\
            SavedLocationEntry {\nRecent,\nC:\\d,\nFalse,\nFolder,\n2020-01-02\n}\n";
        test_log::capture();
        assert_eq!(Locations::from_legacy(text), Locations::default());
        let lines = test_log::lines();
        assert_eq!(lines.len(), 4);
        assert!(
            lines
                .iter()
                .all(|line| line.starts_with("settings: saved location skipped: "))
        );
    }

    #[test]
    fn recent_holds_five_and_drops_the_oldest() {
        let mut locations = Locations::default();
        for (n, time) in [50, 10, 40, 20, 30].into_iter().enumerate() {
            locations.add_recent(place(&format!("/r{n}"), time));
        }
        locations.add_recent(place("/new", 60));
        assert_eq!(
            paths(&locations.recent),
            ["/r0", "/r2", "/r3", "/r4", "/new"]
        );
    }

    #[test]
    fn a_listed_recent_path_is_not_refreshed() {
        let mut locations = Locations::default();
        locations.add_recent(place("/a", 10));
        locations.add_recent(place("/b", 20));
        locations.add_recent(place("/a", 99));
        assert_eq!(locations.recent, [place("/a", 10), place("/b", 20)]);
    }

    #[test]
    fn case_matters_only_where_the_system_cares() {
        let mut locations = Locations::default();
        locations.add_recent(place("C:\\Models", 10));
        locations.add_recent(place("c:\\models", 20));
        assert_eq!(locations.recent.len(), if cfg!(windows) { 1 } else { 2 });
    }

    #[test]
    fn one_unreadable_location_costs_only_itself() {
        let stored = json!({
            "recent": [
                {"path": "/a", "hidden": false, "kind": "Folder", "time": 1},
                {"path": "/b", "hidden": false, "kind": "Shelf", "time": 2},
                {"path": "/c"},
                7,
            ],
            "favourites": [{"path": "/f", "hidden": true, "kind": "Root", "time": 3}],
            "defaultsAdded": true,
        });
        let locations = Locations::from_json(&stored);
        assert_eq!(paths(&locations.recent), ["/a"]);
        assert_eq!(paths(&locations.favourites), ["/f"]);
        assert!(locations.defaults_added);

        for unusable in [json!(null), json!([1]), json!("x")] {
            assert_eq!(Locations::from_json(&unusable), Locations::default());
        }
        let wrong_lists = json!({"recent": "x", "favourites": {}, "defaultsAdded": 1});
        assert_eq!(Locations::from_json(&wrong_lists), Locations::default());
    }
}
