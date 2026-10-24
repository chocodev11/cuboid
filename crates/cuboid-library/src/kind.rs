//! What a file is: the kind of a library entry, decided from the name and the content.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The five kinds of library entry.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// `.cubik`, `.cub`, `.bbmodel` and Minecraft model JSON.
    Model,
    /// Item definitions, blockstates, `pack.mcmeta` and lang files.
    Json,
    /// `.png`, with an optional `.png.mcmeta`.
    Texture,
    /// `.jpg`, `.jpeg`, `.webp` and `.gif` pictures to look at, not to use.
    Reference,
    /// `.md` and `.txt` notes about the project.
    Lore,
}

impl Kind {
    /// All kinds in display order.
    pub const ALL: [Kind; 5] = [
        Kind::Model,
        Kind::Texture,
        Kind::Json,
        Kind::Reference,
        Kind::Lore,
    ];

    /// Position in [`Kind::ALL`].
    pub fn index(self) -> usize {
        Kind::ALL.iter().position(|k| *k == self).unwrap_or(0)
    }

    /// Singular name.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Model => "model",
            Kind::Json => "JSON",
            Kind::Texture => "texture",
            Kind::Reference => "reference",
            Kind::Lore => "lore",
        }
    }

    /// Plural name, for counts.
    pub fn plural(self) -> &'static str {
        match self {
            Kind::Model => "models",
            Kind::Json => "JSON files",
            Kind::Texture => "textures",
            Kind::Reference => "references",
            Kind::Lore => "lore notes",
        }
    }
}

/// What a JSON file of kind [`Kind::Json`] is.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonClass {
    /// An item definition: a `model` object.
    ItemDefinition,
    /// A blockstate: `variants` or `multipart`.
    Blockstate,
    /// A `pack.mcmeta`: a `pack` entry.
    Pack,
    /// A lang file: a flat map of strings.
    Lang,
}

impl JsonClass {
    /// Name for lists.
    pub fn name(self) -> &'static str {
        match self {
            JsonClass::ItemDefinition => "item definition",
            JsonClass::Blockstate => "blockstate",
            JsonClass::Pack => "pack.mcmeta",
            JsonClass::Lang => "lang file",
        }
    }
}

/// The verdict on one file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Class {
    /// Becomes an entry.
    Entry {
        /// Its kind.
        kind: Kind,
        /// For [`Kind::Json`], what it is.
        json: Option<JsonClass>,
    },
    /// A `.png.mcmeta`: travels with its PNG and is no entry of its own.
    Companion,
    /// Not imported. The label groups the notices ("`.dll`", "`.json` (unrecognised)").
    Skip(String),
}

const MODEL_EXTENSIONS: [&str; 3] = ["cubik", "cub", "bbmodel"];
const REFERENCE_EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "webp", "gif"];
const LORE_EXTENSIONS: [&str; 2] = ["md", "txt"];
/// JSON files that are classified by their content.
const JSON_EXTENSIONS: [&str; 2] = ["json", "mcmeta"];
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Lower-case extension of a file name, without the dot; empty when there is none.
pub fn extension(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// Whether the file is worth reading at all. Everything else is skipped by its extension alone, so
/// a folder of large foreign files costs nothing.
pub fn is_candidate(name: &str) -> bool {
    let ext = extension(name);
    let ext = ext.as_str();
    MODEL_EXTENSIONS.contains(&ext)
        || REFERENCE_EXTENSIONS.contains(&ext)
        || LORE_EXTENSIONS.contains(&ext)
        || JSON_EXTENSIONS.contains(&ext)
        || ext == "png"
}

/// Whether the name is the metadata file of a PNG (`stone.png.mcmeta`).
pub fn is_companion(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".png.mcmeta")
}

/// The label of the notice for a skipped extension.
pub fn skip_label(name: &str) -> String {
    let ext = extension(name);
    if ext.is_empty() {
        "files without an extension".to_owned()
    } else {
        format!(".{ext}")
    }
}

/// Classifies a file by name and, where the name is not enough, by content. `bytes` are only read
/// for the JSON kinds, the PNG signature and the `.cubik` container signature.
pub fn classify(name: &str, bytes: &[u8]) -> Class {
    if is_companion(name) {
        return Class::Companion;
    }
    let ext = extension(name);
    let ext = ext.as_str();
    let entry = |kind| Class::Entry { kind, json: None };
    if ext == "png" {
        return if bytes.starts_with(&PNG_SIGNATURE) {
            entry(Kind::Texture)
        } else {
            Class::Skip(".png (not a PNG file)".to_owned())
        };
    }
    if ext == "cubik" {
        // A `.cubik` file is a zip container.
        return if bytes.starts_with(b"PK") {
            entry(Kind::Model)
        } else {
            Class::Skip(".cubik (not a Cubik file)".to_owned())
        };
    }
    if ext == "cub" || ext == "bbmodel" {
        return entry(Kind::Model);
    }
    if REFERENCE_EXTENSIONS.contains(&ext) {
        return entry(Kind::Reference);
    }
    if LORE_EXTENSIONS.contains(&ext) {
        return entry(Kind::Lore);
    }
    if JSON_EXTENSIONS.contains(&ext) {
        return classify_json(ext, bytes);
    }
    Class::Skip(skip_label(name))
}

/// The JSON kinds, by what the document holds. The first rule that fits wins: `elements` is a
/// model; a `model` object an item definition; `variants` or `multipart` a blockstate; a `pack`
/// object a `pack.mcmeta`; a non-empty map of strings a lang file.
fn classify_json(ext: &str, bytes: &[u8]) -> Class {
    let unrecognised = || Class::Skip(format!(".{ext} (unrecognised JSON)"));
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(bytes) else {
        return unrecognised();
    };
    let json = |class| Class::Entry {
        kind: Kind::Json,
        json: Some(class),
    };
    if map.get("elements").is_some_and(Value::is_array) {
        return Class::Entry {
            kind: Kind::Model,
            json: None,
        };
    }
    if map.get("model").is_some_and(Value::is_object) {
        return json(JsonClass::ItemDefinition);
    }
    if map.contains_key("variants") || map.contains_key("multipart") {
        return json(JsonClass::Blockstate);
    }
    if map.get("pack").is_some_and(Value::is_object) {
        return json(JsonClass::Pack);
    }
    if !map.is_empty() && map.values().all(Value::is_string) {
        return json(JsonClass::Lang);
    }
    unrecognised()
}

/// Counts skipped files by notice label.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Skipped(BTreeMap<String, usize>);

impl Skipped {
    /// Counts one more skipped file.
    pub fn add(&mut self, label: String) {
        *self.0.entry(label).or_insert(0) += 1;
    }

    /// One notice per label, such as "Skipped 3 files of type .dll".
    pub fn notices(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|(label, count)| {
                let files = if *count == 1 { "file" } else { "files" };
                format!("Skipped {count} {files}: {label}")
            })
            .collect()
    }

    /// Whether nothing was skipped.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Files skipped in total.
    pub fn total(&self) -> usize {
        self.0.values().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];

    fn entry(kind: Kind) -> Class {
        Class::Entry { kind, json: None }
    }

    fn json(class: JsonClass) -> Class {
        Class::Entry {
            kind: Kind::Json,
            json: Some(class),
        }
    }

    #[test]
    fn the_classification_table() {
        let cases: &[(&str, &[u8], Class)] = &[
            ("tower.cubik", b"PK\x03\x04", entry(Kind::Model)),
            ("old.cub", b"{}", entry(Kind::Model)),
            ("sword.bbmodel", b"{}", entry(Kind::Model)),
            ("block.json", br#"{"elements": []}"#, entry(Kind::Model)),
            (
                "gun.json",
                br#"{"model": {"type": "minecraft:model", "model": "item/gun"}}"#,
                json(JsonClass::ItemDefinition),
            ),
            (
                "stone.json",
                br#"{"variants": {"": {"model": "block/stone"}}}"#,
                json(JsonClass::Blockstate),
            ),
            (
                "fence.json",
                br#"{"multipart": []}"#,
                json(JsonClass::Blockstate),
            ),
            (
                "pack.mcmeta",
                br#"{"pack": {"pack_format": 46, "description": "x"}}"#,
                json(JsonClass::Pack),
            ),
            (
                "en_us.json",
                br#"{"item.gun": "Gun", "item.axe": "Axe"}"#,
                json(JsonClass::Lang),
            ),
            ("stone.png", PNG, entry(Kind::Texture)),
            ("STONE.PNG", PNG, entry(Kind::Texture)),
            ("photo.jpg", b"x", entry(Kind::Reference)),
            ("photo.JPEG", b"x", entry(Kind::Reference)),
            ("photo.webp", b"x", entry(Kind::Reference)),
            ("anim.gif", b"x", entry(Kind::Reference)),
            ("story.md", b"# x", entry(Kind::Lore)),
            ("notes.txt", b"x", entry(Kind::Lore)),
            ("stone.png.mcmeta", b"{}", Class::Companion),
        ];
        for (name, bytes, expected) in cases {
            assert_eq!(&classify(name, bytes), expected, "{name}");
        }
    }

    #[test]
    fn what_fits_nothing_is_skipped_with_a_label() {
        let skip = |name: &str, bytes: &[u8]| match classify(name, bytes) {
            Class::Skip(label) => label,
            other => panic!("{name}: {other:?}"),
        };
        assert_eq!(skip("tool.exe", b"MZ"), ".exe");
        assert_eq!(skip("README", b"x"), "files without an extension");
        assert_eq!(skip("a.png", b"not a png"), ".png (not a PNG file)");
        assert_eq!(skip("a.cubik", b"x"), ".cubik (not a Cubik file)");
        assert_eq!(
            skip("atlas.json", br#"{"sources": []}"#),
            ".json (unrecognised JSON)"
        );
        assert_eq!(skip("empty.json", b"{}"), ".json (unrecognised JSON)");
        assert_eq!(skip("list.json", b"[1]"), ".json (unrecognised JSON)");
        assert_eq!(skip("broken.json", b"{"), ".json (unrecognised JSON)");
        // A lang file holds strings only.
        assert_eq!(
            skip("mixed.json", br#"{"a": "x", "b": 1}"#),
            ".json (unrecognised JSON)"
        );
    }

    #[test]
    fn json_rules_apply_in_order_and_a_byte_order_mark_is_fine() {
        // `elements` beats `model`.
        let both = br#"{"elements": [], "model": {}}"#;
        assert_eq!(classify("a.json", both), entry(Kind::Model));
        let bom = b"\xEF\xBB\xBF{\"pack\": {}}";
        assert_eq!(classify("pack.mcmeta", bom), json(JsonClass::Pack));
    }

    #[test]
    fn candidates_are_found_by_extension_alone() {
        assert!(is_candidate("a.PNG"));
        assert!(is_candidate("pack.mcmeta"));
        assert!(!is_candidate("tool.exe"));
        assert!(!is_candidate("noext"));
        assert!(!is_candidate(".png"), "a dot file has no extension");
    }

    #[test]
    fn notices_are_one_per_label() {
        let mut skipped = Skipped::default();
        skipped.add(".dll".to_owned());
        skipped.add(".dll".to_owned());
        skipped.add(".exe".to_owned());
        assert_eq!(
            skipped.notices(),
            ["Skipped 2 files: .dll", "Skipped 1 file: .exe"]
        );
        assert_eq!(skipped.total(), 3);
    }
}
