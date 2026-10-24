//! The recent-files list of File > "Open recent files".

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::json::Json;

/// Entries kept when the stored `max` is missing or unusable.
const DEFAULT_MAX: i32 = 10;

/// One recent file. `name` is the file name with extension, as the menu shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecentFile {
    pub name: String,
    pub path: String,
}

/// The `recentFiles` value, `{"max": 10, "list": [{"name": .., "path": ..}, ..]}`.
///
/// `list` is kept oldest first, as the original stores it; the menu shows it newest first
/// ([`RecentFiles::newest_first`]). The original identifies an entry by its name, so two files of
/// the same name in different folders replace each other. The port identifies by full path.
///
/// spec: app 11, app 10.1
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecentFiles {
    /// Entries kept; a stored value other than 10 is honoured, one below 1 keeps nothing.
    pub max: i32,
    pub list: Vec<RecentFile>,
}

impl Default for RecentFiles {
    fn default() -> Self {
        Self {
            max: DEFAULT_MAX,
            list: Vec::new(),
        }
    }
}

impl RecentFiles {
    /// Reads the stored value. `null` is an empty list with `max` 10; an unusable value and an
    /// unusable `max` take the same defaults, and entries without a path are dropped.
    ///
    /// spec: app 10.1
    pub(crate) fn from_json(value: &Value) -> Self {
        let object = match value {
            Value::Object(object) => object,
            Value::Null => return Self::default(),
            _ => {
                log::warn!("settings: unusable value for `recentFiles`, using the default");
                return Self::default();
            }
        };
        let max = object
            .get("max")
            .and_then(i32::from_json)
            .unwrap_or(DEFAULT_MAX);
        let list = object
            .get("list")
            .and_then(Value::as_array)
            .map(|entries| entries.iter().filter_map(entry_from_json).collect())
            .unwrap_or_default();
        Self { max, list }
    }

    /// Lists `path` as the newest file: an entry of the same full path is removed, the new one is
    /// appended and the oldest entries are dropped beyond `max`. Opening an entry of the menu
    /// calls this too, which moves it to the top. Shortcuts (`.lnk`) are never added.
    ///
    /// spec: app 11
    pub fn add(&mut self, path: &str) {
        if Path::new(path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
        {
            return;
        }
        self.list.retain(|entry| entry.path != path);
        self.list.push(RecentFile {
            name: file_name(path).to_owned(),
            path: path.to_owned(),
        });
        let max = usize::try_from(self.max).unwrap_or(0);
        let excess = self.list.len().saturating_sub(max);
        self.list.drain(..excess);
    }

    /// Removes the entry of this full path, for a file that no longer exists.
    ///
    /// spec: app 11
    pub fn remove(&mut self, path: &str) {
        self.list.retain(|entry| entry.path != path);
    }

    /// The entries in menu order: the most recently added file first.
    ///
    /// spec: app 11
    pub fn newest_first(&self) -> impl Iterator<Item = &RecentFile> {
        self.list.iter().rev()
    }
}

/// The part of the path after the last `/` or `\`, whichever system wrote it.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// An entry needs a path; a missing or `null` name is derived from it.
fn entry_from_json(value: &Value) -> Option<RecentFile> {
    let Some(path) = value.get("path").and_then(Value::as_str) else {
        log::warn!("settings: recent file without a path dropped");
        return None;
    };
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_else(|| file_name(path));
    Some(RecentFile {
        name: name.to_owned(),
        path: path.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn paths(recent: &RecentFiles) -> Vec<&str> {
        recent.list.iter().map(|e| e.path.as_str()).collect()
    }

    #[test]
    fn added_files_go_to_the_end_and_the_menu_lists_them_newest_first() {
        let mut recent = RecentFiles::default();
        recent.add("C:\\models\\a.cubik");
        recent.add("C:\\models\\b.cubik");
        assert_eq!(
            paths(&recent),
            ["C:\\models\\a.cubik", "C:\\models\\b.cubik"]
        );
        let menu: Vec<_> = recent.newest_first().map(|e| e.name.as_str()).collect();
        assert_eq!(menu, ["b.cubik", "a.cubik"]);
    }

    #[test]
    fn a_listed_path_moves_to_the_end() {
        let mut recent = RecentFiles::default();
        for path in ["/a.cubik", "/b.cubik", "/c.cubik"] {
            recent.add(path);
        }
        recent.add("/a.cubik");
        assert_eq!(paths(&recent), ["/b.cubik", "/c.cubik", "/a.cubik"]);
    }

    #[test]
    fn identity_is_the_full_path_not_the_name() {
        let mut recent = RecentFiles::default();
        recent.add("/one/model.cubik");
        recent.add("/two/model.cubik");
        assert_eq!(paths(&recent), ["/one/model.cubik", "/two/model.cubik"]);
        assert!(recent.list.iter().all(|e| e.name == "model.cubik"));
    }

    #[test]
    fn names_are_cut_at_either_separator() {
        let mut recent = RecentFiles::default();
        recent.add("C:\\x\\y\\win.cubik");
        recent.add("/x/y/unix.cubik");
        recent.add("bare.cubik");
        let names: Vec<_> = recent.list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["win.cubik", "unix.cubik", "bare.cubik"]);
    }

    #[test]
    fn the_oldest_entries_are_dropped_beyond_max() {
        let mut recent = RecentFiles::default();
        for n in 0..12 {
            recent.add(&format!("/f{n}.cubik"));
        }
        assert_eq!(recent.list.len(), 10);
        assert_eq!(
            recent.list.first().map(|e| e.path.as_str()),
            Some("/f2.cubik")
        );
        assert_eq!(
            recent.list.last().map(|e| e.path.as_str()),
            Some("/f11.cubik")
        );
    }

    #[test]
    fn a_stored_max_is_honoured() {
        let mut recent = RecentFiles::from_json(&json!({"max": 2, "list": []}));
        for path in ["/a", "/b", "/c"] {
            recent.add(path);
        }
        assert_eq!(paths(&recent), ["/b", "/c"]);

        let mut none = RecentFiles::from_json(&json!({"max": -3, "list": []}));
        none.add("/a");
        assert!(none.list.is_empty());
    }

    #[test]
    fn shortcuts_are_never_listed() {
        let mut recent = RecentFiles::default();
        recent.add("C:\\links\\model.lnk");
        recent.add("C:\\links\\MODEL.LNK");
        recent.add("C:\\links\\model.lnk.cubik");
        assert_eq!(paths(&recent), ["C:\\links\\model.lnk.cubik"]);
    }

    #[test]
    fn a_missing_file_is_removed_by_its_path() {
        let mut recent = RecentFiles::default();
        recent.add("/one/model.cubik");
        recent.add("/two/model.cubik");
        recent.remove("/one/model.cubik");
        assert_eq!(paths(&recent), ["/two/model.cubik"]);
    }

    #[test]
    fn null_and_unusable_values_are_an_empty_list_with_max_10() {
        let empty = RecentFiles::default();
        assert_eq!(RecentFiles::from_json(&Value::Null), empty);
        assert_eq!(RecentFiles::from_json(&json!("abc")), empty);
        assert_eq!(
            RecentFiles::from_json(&json!({"max": null, "list": null})),
            empty
        );
        assert_eq!(RecentFiles::from_json(&json!({})), empty);
    }

    #[test]
    fn stored_entries_are_read_in_order_and_unusable_ones_are_dropped() {
        let stored = json!({"max": "5", "list": [
            {"name": "a.cubik", "path": "C:\\a.cubik"},
            {"name": "b.cubik"},
            {"name": null, "path": "C:\\dir\\c.cubik"},
            7,
        ]});
        let recent = RecentFiles::from_json(&stored);
        assert_eq!(recent.max, 5);
        let entries: Vec<_> = recent.list.iter().map(|e| (&*e.name, &*e.path)).collect();
        assert_eq!(
            entries,
            [("a.cubik", "C:\\a.cubik"), ("c.cubik", "C:\\dir\\c.cubik")]
        );
    }
}
