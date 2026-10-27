//! What an import keeps for the writer: the project members the model cannot hold yet, as the
//! project had them, and the uuids of its objects (spec: hierarchy A1 5).
//!
//! The data lives in `Document::retained` behind `Retained::data`. A save to `.bbmodel` writes
//! the members back unchanged and reuses the uuids; any other writer ignores it.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use cuboid_model::{ObjId, Retained};
use serde_json::Value;

/// The most bytes of project text the kept members may take together; the rest is left out.
pub(crate) const KEEP_LIMIT: u64 = 64 << 20;

/// The members the importer and the exporter handle themselves.
const KNOWN: [&str; 14] = [
    "meta",
    "name",
    "credit",
    "parent",
    "ambientocclusion",
    "front_gui_light",
    "java_block_version",
    "resolution",
    "elements",
    "groups",
    "outliner",
    "textures",
    "display",
    "animations",
];

/// The kept members that hold animation data, which the user is told about. The `animations`
/// member is the animations the model had no room for; the others are not modelled at all.
const ANIMATION: [&str; 3] = [
    "animations",
    "animation_controllers",
    "animation_variable_placeholders",
];

/// The project members that stay as they are, and the identities of the objects.
#[derive(Debug, Default)]
pub struct BbKept {
    /// Top-level members the model does not hold, in the order of the project. The `animations`
    /// member, the animations the model had no room for, is held in the signs of version 4.10
    /// that the writer gives (see `signs`); the other members are as the project had them.
    pub members: Vec<(String, Value)>,
    /// The uuid the project gave an element or a mesh.
    pub uuids: HashMap<ObjId, String>,
    /// The frame fields the project gave an animated texture, by the label of its entry.
    pub frames: HashMap<String, Value>,
}

impl BbKept {
    /// The kept data of a document, `None` when it does not come from a `.bbmodel` import.
    pub fn of(retained: &Retained) -> Option<&BbKept> {
        retained.data.downcast_ref::<BbKept>()
    }
}

/// The members taken from a project, with their size in bytes of text.
pub(crate) struct Sections {
    pub members: Vec<(String, Value)>,
    pub bytes: u64,
}

struct Count(u64);

impl io::Write for Count {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The bytes of text of a value, without building the text.
fn json_len(value: &Value) -> u64 {
    let mut count = Count(0);
    // Counting cannot fail.
    let _ = serde_json::to_writer(&mut count, value);
    count.0
}

/// How many entries a member has: the items of a list or the keys of an object.
pub(crate) fn count_of(value: &Value) -> usize {
    match value {
        Value::Array(items) => items.len(),
        Value::Object(map) => map.len(),
        _ => 1,
    }
}

/// The members of a project that the model does not hold, up to [`KEEP_LIMIT`] bytes together with
/// the `used` bytes of text that the animations of the model keep. `animations` are the ones the
/// model had no room for, which stay a member of their own. The second value names the members that
/// did not fit, with their entries.
pub(crate) fn sections_of(
    root: &Value,
    used: u64,
    animations: Vec<Value>,
) -> (Sections, Vec<(String, usize)>) {
    sections_within(root, KEEP_LIMIT, used, animations)
}

fn sections_within(
    root: &Value,
    limit: u64,
    used: u64,
    animations: Vec<Value>,
) -> (Sections, Vec<(String, usize)>) {
    let mut kept = Sections {
        members: Vec::new(),
        bytes: used,
    };
    let mut left_out = Vec::new();
    let Some(map) = root.as_object() else {
        return (kept, left_out);
    };
    let mut keep = |key: &str, value: &Value| {
        let size = json_len(value);
        if kept.bytes.saturating_add(size) > limit {
            left_out.push((key.replace('_', " "), count_of(value)));
            return;
        }
        kept.bytes += size;
        kept.members.push((key.to_owned(), value.clone()));
    };
    for (key, value) in map {
        if !KNOWN.contains(&key.as_str()) {
            keep(key, value);
        }
    }
    if !animations.is_empty() {
        keep("animations", &Value::Array(animations));
    }
    (kept, left_out)
}

/// The animation members among the kept ones, named for a message, with their entries.
pub(crate) fn summary(kept: &Sections) -> Vec<(String, usize)> {
    kept.members
        .iter()
        .filter(|(key, _)| ANIMATION.contains(&key.as_str()))
        .map(|(key, value)| (key.replace('_', " "), count_of(value)))
        .filter(|(_, count)| *count > 0)
        .collect()
}

/// The retention data of an import, `None` when there is nothing to keep.
pub(crate) fn retain(
    kept: Sections,
    uuids: HashMap<ObjId, String>,
    frames: HashMap<String, Value>,
) -> Option<Arc<Retained>> {
    if kept.members.is_empty() && uuids.is_empty() && frames.is_empty() {
        return None;
    }
    let bytes = usize::try_from(kept.bytes)
        .unwrap_or(usize::MAX)
        .saturating_add(uuids.values().map(|u| u.len() + 48).sum::<usize>())
        .saturating_add(frames.len() * 256);
    Some(Arc::new(Retained {
        bytes,
        data: Box::new(BbKept {
            members: kept.members,
            uuids,
            frames,
        }),
    }))
}

/// The animation members a document keeps from its `.bbmodel` file and writes back to `.bbmodel`
/// only, with their entries. A writer of another format tells the user that they are not
/// included.
pub fn kept_animations(retained: Option<&Retained>) -> Vec<(String, usize)> {
    let Some(kept) = retained.and_then(BbKept::of) else {
        return Vec::new();
    };
    kept.members
        .iter()
        .filter(|(key, _)| ANIMATION.contains(&key.as_str()))
        .map(|(key, value)| (key.replace('_', " "), count_of(value)))
        .filter(|(_, count)| *count > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn unknown_members_are_kept_and_the_known_ones_are_not() {
        let root = json!({
            "meta": {}, "elements": [], "name": "x",
            "animations": [{"name": "walk"}, {"name": "idle"}],
            "animation_controllers": [],
            "visible_box": [1, 1, 0],
        });
        // The animations are the model's own; only what it had no room for is kept.
        let (kept, left_out) = sections_of(&root, 0, Vec::new());
        let mut keys: Vec<&str> = kept.members.iter().map(|(k, _)| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["animation_controllers", "visible_box"]);
        assert!(left_out.is_empty());
        assert!(summary(&kept).is_empty(), "the controllers are empty");
        assert!(kept.bytes > 0);

        let (kept, _) = sections_of(&root, 0, vec![json!({"name": "walk"})]);
        assert_eq!(summary(&kept), [("animations".to_owned(), 1)]);
    }

    #[test]
    fn a_member_over_the_limit_is_left_out_with_its_entries() {
        let root = json!({"animation_controllers": [1, 2, 3], "huge": "x".repeat(100)});
        let (kept, left_out) = sections_within(&root, 50, 0, Vec::new());
        assert_eq!(kept.members.len(), 1);
        assert_eq!(left_out, [("huge".to_owned(), 1)]);
        assert!(
            sections_of(&root, 0, Vec::new()).1.is_empty(),
            "both fit under the real limit"
        );
        // The text the animations of the model keep counts against the same limit.
        let (kept, left_out) = sections_within(&root, 50, 45, Vec::new());
        assert!(kept.members.is_empty());
        assert_eq!(left_out.len(), 2);
    }

    #[test]
    fn nothing_to_keep_gives_no_retention() {
        let none = || Sections {
            members: Vec::new(),
            bytes: 0,
        };
        assert!(retain(none(), HashMap::new(), HashMap::new()).is_none());
        let kept = retain(
            none(),
            HashMap::from([(ObjId::next(), "u".to_owned())]),
            HashMap::new(),
        )
        .expect("a uuid is worth keeping");
        assert!(BbKept::of(&kept).is_some());
        let framed = retain(
            none(),
            HashMap::new(),
            HashMap::from([("door".to_owned(), serde_json::json!({"frame_time": 2}))]),
        )
        .expect("frame fields are worth keeping");
        assert!(BbKept::of(&framed).is_some_and(|k| k.frames.len() == 1));
        assert!(kept_animations(Some(&kept)).is_empty());
        assert!(kept_animations(None).is_empty());
    }
}
