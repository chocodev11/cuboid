//! The library of one scope: the `library.json` index, the stored copies and the editing of
//! entries.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use cuboid_io::texture_files::{DiskFiles, TextureFiles};
use serde::{Deserialize, Serialize};

use crate::brief::Brief;
use crate::kind::{Kind, Role};
use crate::meta::{Meta, model_meta};

/// Version of the index file. A file with a higher version is never opened or overwritten.
/// Version 2 added the targets and rules to the brief, version 3 the role of an entry, the style
/// of a model and the style sources of the brief; older files read with the defaults.
pub const SCHEMA: u32 = 3;
/// Name of the index inside a library folder.
pub const INDEX_FILE: &str = "library.json";
/// Largest index file that is read.
const MAX_INDEX_BYTES: u64 = 64 << 20;
/// Folder of the stored copies.
pub(crate) const FILES_DIR: &str = "files";
/// Folder of the 160 px pictures of the entries.
const THUMBS_DIR: &str = "thumbs";
/// Name of the project library folder next to a document.
pub const PROJECT_DIR: &str = "cuboid-library";
const MAX_TAGS: usize = 32;
const MAX_TAG_LEN: usize = 40;
/// The licence of an entry nobody has described.
pub const UNKNOWN_LICENCE: &str = "unknown";

/// Which library.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Scope {
    /// Under the data folder of the program, shared by every project.
    Global,
    /// Beside the document, for one project.
    Project,
}

/// Why a library cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    /// A file or folder could not be read or written.
    #[error("{0}")]
    Io(#[from] io::Error),
    /// The index was written by a newer version of the program.
    #[error("the library was written by a newer version of cuboid (schema {0})")]
    Newer(u32),
    /// The index is not a library index.
    #[error("the library index cannot be read ({0})")]
    Corrupt(String),
    /// The index could not be written as text.
    #[error("{0}")]
    Encode(#[from] serde_json::Error),
    /// A style folder that cannot be used; the text says why.
    #[error("{0}")]
    Source(String),
}

/// One file of the library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// First 16 hex digits of the SHA-256; unique in a library because files are deduplicated.
    pub id: String,
    /// What the file is.
    pub kind: Kind,
    /// The name the file had when it was imported.
    pub name: String,
    /// The stored copy, a plain file name inside `files/`.
    pub file: String,
    /// The `.png.mcmeta` copy of a texture, a plain file name inside `files/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion: Option<String>,
    /// The path the file was imported from. For a style reference that came from a folder of the
    /// project it is relative to the project, with `/`, and stays so when the entry becomes the
    /// project's own work.
    pub source: String,
    /// SHA-256 of the content, 64 hex digits.
    pub sha256: String,
    /// Size in bytes.
    pub size: u64,
    /// Tags the user or the import gave.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Free text; `unknown` when nobody said.
    #[serde(default = "unknown_licence")]
    pub licence: String,
    /// The project's own work or a style reference; only models and textures are references.
    #[serde(default)]
    pub role: Role,
    /// Seconds since 1970-01-01 UTC.
    pub added: i64,
    /// What the worker read from the file.
    #[serde(default)]
    pub meta: Option<Meta>,
}

fn unknown_licence() -> String {
    UNKNOWN_LICENCE.to_owned()
}

impl Entry {
    /// The file name without its extension.
    pub fn stem(&self) -> &str {
        self.name
            .rsplit_once('.')
            .map_or(&*self.name, |(stem, _)| stem)
    }
}

#[derive(Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    #[serde(default)]
    brief: Brief,
    #[serde(default)]
    entries: Vec<Entry>,
}

/// A text search: every word must occur in the name, tags, licence, kind or lore title of an
/// entry, in any case.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Query {
    /// Words to find.
    pub text: String,
    /// Only this kind.
    pub kind: Option<Kind>,
    /// Only entries with this tag (any case).
    pub tag: Option<String>,
}

/// The library of one scope, in memory. Reading and writing files is explicit: [`Library::open`],
/// [`Library::save`], and the import in [`crate::import`].
#[derive(Clone, Debug)]
pub struct Library {
    root: PathBuf,
    scope: Scope,
    brief: Brief,
    /// In the order they were added.
    entries: Vec<Entry>,
    by_sha: HashMap<String, usize>,
}

impl Library {
    /// An empty library that lives in `root`. Nothing is written until [`Library::save`].
    pub fn new(root: PathBuf, scope: Scope) -> Library {
        Library {
            root,
            scope,
            brief: Brief::default(),
            entries: Vec::new(),
            by_sha: HashMap::new(),
        }
    }

    /// Reads the library in `root`. A missing index is an empty library. An index that cannot be
    /// read is moved aside as `library.json.bad` and the library starts empty; the stored files
    /// stay. The text says what happened.
    pub fn open(root: &Path, scope: Scope) -> Result<(Library, Option<String>), LibraryError> {
        let path = root.join(INDEX_FILE);
        let Some(text) = read_index(&path)? else {
            return Ok((Library::new(root.to_owned(), scope), None));
        };
        match Library::parse(root, scope, &text) {
            Ok(library) => Ok((library, None)),
            Err(LibraryError::Corrupt(error)) => {
                let bad = root.join("library.json.bad");
                let moved = fs::rename(&path, &bad).is_ok();
                let note = format!(
                    "The library index could not be read ({error}). {}",
                    if moved {
                        "It was kept as library.json.bad and the library starts empty."
                    } else {
                        "The library starts empty."
                    }
                );
                Ok((Library::new(root.to_owned(), scope), Some(note)))
            }
            Err(other) => Err(other),
        }
    }

    /// Reads the library in `root` and changes nothing on disk: a missing index, an index that
    /// cannot be read and one of a newer schema are errors.
    pub fn read(root: &Path, scope: Scope) -> Result<Library, LibraryError> {
        let text = read_index(&root.join(INDEX_FILE))?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} has no {INDEX_FILE}", root.display()),
            )
        })?;
        Library::parse(root, scope, &text)
    }

    /// The library that `folder` names: the folder itself when it holds an index, else its
    /// project library `cuboid-library`. A folder with neither is a not-found error. Reads
    /// nothing but the folder; [`Library::read`] reads the library.
    pub fn locate(folder: &Path) -> Result<(PathBuf, Scope), LibraryError> {
        let project = folder.join(PROJECT_DIR);
        if folder.join(INDEX_FILE).is_file() {
            let is_project = folder.file_name().is_some_and(|name| name == PROJECT_DIR);
            let scope = if is_project {
                Scope::Project
            } else {
                Scope::Global
            };
            return Ok((folder.to_owned(), scope));
        }
        if project.join(INDEX_FILE).is_file() {
            return Ok((project, Scope::Project));
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no {INDEX_FILE} here or in {PROJECT_DIR}/"),
        )
        .into())
    }

    fn parse(root: &Path, scope: Scope, text: &[u8]) -> Result<Library, LibraryError> {
        let file: IndexFile = serde_json::from_slice(text)
            .map_err(|error| LibraryError::Corrupt(error.to_string()))?;
        if file.version > SCHEMA {
            return Err(LibraryError::Newer(file.version));
        }
        let mut library = Library::new(root.to_owned(), scope);
        library.brief = file.brief.cleaned();
        for entry in file.entries {
            library.push(entry);
        }
        Ok(library)
    }

    /// Writes the index atomically.
    pub fn save(&self) -> Result<(), LibraryError> {
        fs::create_dir_all(&self.root)?;
        let file = IndexFile {
            version: SCHEMA,
            brief: self.brief.clone(),
            entries: self.entries.clone(),
        };
        let text = serde_json::to_vec_pretty(&file)?;
        cuboid_io::write_atomic(&self.root.join(INDEX_FILE), &text)?;
        Ok(())
    }

    /// The folder of the library.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Which library this is.
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// The brief inputs.
    pub fn brief(&self) -> &Brief {
        &self.brief
    }

    /// Replaces the brief inputs.
    pub fn set_brief(&mut self, brief: Brief) {
        self.brief = brief.cleaned();
    }

    /// Every entry, oldest first.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry with this id.
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// The entry with this content, if the library has it.
    pub fn by_sha(&self, sha256: &str) -> Option<&Entry> {
        self.by_sha.get(sha256).map(|at| &self.entries[*at])
    }

    /// Entries per kind, in [`Kind::ALL`] order.
    pub fn counts(&self) -> [usize; 5] {
        let mut counts = [0; 5];
        for entry in &self.entries {
            counts[entry.kind.index()] += 1;
        }
        counts
    }

    pub(crate) fn push(&mut self, entry: Entry) {
        self.by_sha.insert(entry.sha256.clone(), self.entries.len());
        self.entries.push(entry);
    }

    /// Where the stored copy of an entry is. `None` when the index names something other than a
    /// plain file name.
    pub fn file_path(&self, entry: &Entry) -> Option<PathBuf> {
        plain_name(&entry.file).then(|| self.files_dir().join(&entry.file))
    }

    /// Where the `.png.mcmeta` copy of a texture is.
    pub fn companion_path(&self, entry: &Entry) -> Option<PathBuf> {
        let name = entry.companion.as_deref()?;
        plain_name(name).then(|| self.files_dir().join(name))
    }

    pub(crate) fn files_dir(&self) -> PathBuf {
        self.root.join(FILES_DIR)
    }

    /// The folder of the 160 px pictures.
    pub fn thumbs_dir(&self) -> PathBuf {
        self.root.join(THUMBS_DIR)
    }

    /// Where the 160 px picture of an entry is, or will be. `None` for an id that is not a plain
    /// name.
    pub fn thumb_path(&self, id: &str) -> Option<PathBuf> {
        plain_name(id).then(|| self.thumbs_dir().join(format!("{id}.png")))
    }

    /// Entries that match, newest first, at most `limit`.
    pub fn search(&self, query: &Query, limit: usize) -> Vec<&Entry> {
        let words: Vec<String> = query
            .text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let tag = query.tag.as_ref().map(|tag| tag.to_lowercase());
        self.entries
            .iter()
            .rev()
            .filter(|entry| query.kind.is_none_or(|kind| entry.kind == kind))
            .filter(|entry| {
                tag.as_ref()
                    .is_none_or(|tag| entry.tags.iter().any(|t| t.to_lowercase() == *tag))
            })
            .filter(|entry| {
                if words.is_empty() {
                    return true;
                }
                let haystack = haystack(entry);
                words.iter().all(|word| haystack.contains(word))
            })
            .take(limit)
            .collect()
    }

    /// Every tag in use with the number of entries that carry it, most used first.
    pub fn tags(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for tag in self.entries.iter().flat_map(|entry| &entry.tags) {
            *counts.entry(tag.clone()).or_insert(0) += 1;
        }
        let mut list: Vec<_> = counts.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        list
    }

    /// Replaces the tags of an entry. Tags are trimmed, duplicates (in any case) and empty ones
    /// dropped. False when the id is unknown.
    pub fn set_tags(&mut self, id: &str, tags: &[String]) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        entry.tags = clean_tags(tags);
        true
    }

    /// Sets the licence text of an entry; empty means `unknown`. False when the id is unknown.
    pub fn set_licence(&mut self, id: &str, licence: &str) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        entry.licence = clean_licence(licence);
        true
    }

    /// Makes a model or texture the project's own work or a style reference. A model that has no
    /// style data yet (an index of schema 2, or a file that could not be read then) gets it from
    /// its stored copy when it becomes a reference. Its `source` stays what it is, so no style
    /// folder removes it. False when the id is unknown or the entry is neither a model nor a
    /// texture.
    pub fn set_role(&mut self, id: &str, role: Role) -> bool {
        let Some(at) = self
            .entries
            .iter()
            .position(|entry| entry.id == id && matches!(entry.kind, Kind::Model | Kind::Texture))
        else {
            return false;
        };
        let entry = &self.entries[at];
        let style = match &entry.meta {
            Some(Meta::Model(meta)) if role == Role::Style && meta.style.is_none() => self
                .file_path(entry)
                .and_then(|path| DiskFiles.read(&path).ok().flatten())
                .and_then(|bytes| model_meta(&entry.name, &bytes).ok())
                .and_then(|meta| meta.style),
            _ => None,
        };
        let entry = &mut self.entries[at];
        entry.role = role;
        if let (Some(style), Some(Meta::Model(meta))) = (style, &mut entry.meta) {
            meta.style = Some(style);
        }
        true
    }

    /// Takes an entry out of the index and deletes its stored copy and its picture. The files
    /// are deleted on a best-effort basis; the entry is gone either way.
    pub fn remove(&mut self, id: &str) -> Option<Entry> {
        let at = self.entries.iter().position(|entry| entry.id == id)?;
        let entry = self.entries.remove(at);
        self.by_sha = self
            .entries
            .iter()
            .enumerate()
            .map(|(at, entry)| (entry.sha256.clone(), at))
            .collect();
        let doomed = [
            self.file_path(&entry),
            self.companion_path(&entry),
            self.thumb_path(&entry.id),
        ];
        for path in doomed.into_iter().flatten() {
            if let Err(error) = fs::remove_file(&path)
                && error.kind() != io::ErrorKind::NotFound
            {
                log::warn!("cannot delete {}: {error}", path.display());
            }
        }
        Some(entry)
    }
}

/// The bytes of the index; `None` when there is no index. A file over [`MAX_INDEX_BYTES`] is an
/// error and is never read whole.
fn read_index(path: &Path) -> Result<Option<Vec<u8>>, LibraryError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let size = file.metadata()?.len();
    if size > MAX_INDEX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the index is {size} bytes, over the limit of {MAX_INDEX_BYTES}"),
        )
        .into());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    file.take(MAX_INDEX_BYTES).read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

/// Lower-case text that a search looks in.
fn haystack(entry: &Entry) -> String {
    let mut text = format!("{} {} {}", entry.name, entry.licence, entry.kind.name());
    for tag in &entry.tags {
        text.push(' ');
        text.push_str(tag);
    }
    match &entry.meta {
        Some(Meta::Json { class }) => text.push_str(class.name()),
        Some(Meta::Lore(lore)) => {
            if let Some(title) = &lore.title {
                text.push(' ');
                text.push_str(title);
            }
        }
        _ => {}
    }
    text.to_lowercase()
}

/// Trimmed tags without empties and duplicates (in any case), at most [`MAX_TAGS`] of at most
/// [`MAX_TAG_LEN`] characters.
pub fn clean_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag: String = tag.trim().chars().take(MAX_TAG_LEN).collect();
        if tag.is_empty() || out.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            continue;
        }
        out.push(tag);
        if out.len() == MAX_TAGS {
            break;
        }
    }
    out
}

/// The licence text trimmed to 200 characters; empty means `unknown`.
pub(crate) fn clean_licence(licence: &str) -> String {
    let text = licence.trim();
    if text.is_empty() {
        UNKNOWN_LICENCE.to_owned()
    } else {
        text.chars().take(200).collect()
    }
}

/// A file name that cannot leave its folder.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}

/// The name a copy is stored under: the id, then the original name reduced to safe characters.
pub fn stored_name(id: &str, name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let keep = safe.len().saturating_sub(80);
    format!("{id}-{}", &safe[keep..])
}

/// Hex text of bytes.
pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Seconds since the epoch; 0 if the clock is before it.
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::LoreMeta;

    fn entry(id: &str, kind: Kind, name: &str, tags: &[&str]) -> Entry {
        Entry {
            id: id.to_owned(),
            kind,
            name: name.to_owned(),
            file: stored_name(id, name),
            companion: None,
            source: format!("C:/src/{name}"),
            sha256: format!("{id:0>64}"),
            size: 10,
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            licence: UNKNOWN_LICENCE.to_owned(),
            role: Role::Work,
            added: 100,
            meta: None,
        }
    }

    fn library() -> Library {
        let mut library = Library::new(PathBuf::from("lib"), Scope::Global);
        library.push(entry("a1", Kind::Model, "Gun.cubik", &["weapon"]));
        library.push(entry(
            "a2",
            Kind::Texture,
            "gun_blue.png",
            &["weapon", "Blue"],
        ));
        let mut lore = entry("a3", Kind::Lore, "story.md", &[]);
        lore.meta = Some(Meta::Lore(LoreMeta {
            title: Some("The Ink Wars".to_owned()),
            ..LoreMeta::default()
        }));
        library.push(lore);
        library
    }

    #[test]
    fn search_matches_words_kind_and_tag_newest_first() {
        let library = library();
        let ids = |query: &Query| -> Vec<String> {
            library
                .search(query, 10)
                .iter()
                .map(|e| e.id.clone())
                .collect()
        };
        assert_eq!(ids(&Query::default()), ["a3", "a2", "a1"]);
        let text = |text: &str| Query {
            text: text.to_owned(),
            ..Query::default()
        };
        assert_eq!(ids(&text("GUN")), ["a2", "a1"]);
        assert_eq!(ids(&text("gun blue")), ["a2"]);
        assert_eq!(ids(&text("ink wars")), ["a3"], "the lore title is searched");
        assert_eq!(ids(&text("texture")), ["a2"], "so is the kind");
        assert!(ids(&text("nothing")).is_empty());
        let kind = Query {
            kind: Some(Kind::Model),
            ..Query::default()
        };
        assert_eq!(ids(&kind), ["a1"]);
        let tag = Query {
            tag: Some("BLUE".to_owned()),
            ..Query::default()
        };
        assert_eq!(ids(&tag), ["a2"]);
        assert_eq!(library.search(&Query::default(), 2).len(), 2);
    }

    #[test]
    fn tags_are_cleaned_and_counted() {
        let mut library = library();
        let tags = ["  Red ", "red", "", "Pixel"].map(String::from);
        assert!(library.set_tags("a1", &tags));
        assert_eq!(
            library.entry("a1").map(|e| e.tags.clone()),
            Some(vec!["Red".to_owned(), "Pixel".to_owned()])
        );
        assert!(!library.set_tags("zz", &tags));
        let counts = library.tags();
        assert_eq!(counts[0], ("Blue".to_owned(), 1));
        assert_eq!(counts.len(), 4);
    }

    #[test]
    fn the_licence_defaults_to_unknown() {
        let mut library = library();
        assert!(library.set_licence("a1", " CC0 "));
        assert_eq!(library.entry("a1").map(|e| e.licence.as_str()), Some("CC0"));
        assert!(library.set_licence("a1", "  "));
        assert_eq!(
            library.entry("a1").map(|e| e.licence.as_str()),
            Some("unknown")
        );
    }

    #[test]
    fn stored_names_are_safe_and_bounded() {
        assert_eq!(stored_name("ab", "my gun (1).png"), "ab-my_gun__1_.png");
        let long = "x".repeat(300);
        assert_eq!(stored_name("ab", &long).len(), 3 + 80);
        assert!(plain_name("ab-x.png"));
        for bad in ["", "..", "a/b", r"a\b", "C:x", "a:b"] {
            assert!(!plain_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_index_round_trips_and_a_newer_one_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut library = Library::new(dir.path().join("lib"), Scope::Global);
        library.push(entry("a1", Kind::Model, "Gun.cubik", &["weapon"]));
        library.set_brief(Brief {
            project_name: "Ink".to_owned(),
            ..Brief::default()
        });
        library.save().expect("saves");
        let (again, warning) = Library::open(library.root(), Scope::Global).expect("opens");
        assert_eq!(warning, None);
        assert_eq!(again.entries(), library.entries());
        assert_eq!(again.brief().project_name, "Ink");
        assert!(again.by_sha(&format!("{:0>64}", "a1")).is_some());

        let newer = dir.path().join("new");
        fs::create_dir_all(&newer).expect("dir");
        fs::write(newer.join(INDEX_FILE), br#"{"version": 99, "entries": []}"#).expect("write");
        assert!(matches!(
            Library::open(&newer, Scope::Global),
            Err(LibraryError::Newer(99))
        ));
    }

    #[test]
    fn a_schema_1_index_reads_with_default_targets_and_is_saved_as_schema_3() {
        let dir = tempfile::tempdir().expect("temp dir");
        let old = r#"{
  "version": 1,
  "brief": {"project_name": "Ink", "mc_version": "1.21.6", "style_tags": ["pixel"]},
  "entries": [{"id": "a1", "kind": "model", "name": "Gun.cubik", "file": "a1-Gun.cubik",
    "source": "C:/src/Gun.cubik", "sha256": "00", "size": 10, "added": 100,
    "meta": {"type": "model", "format": "cubik", "elements": 3, "meshes": 0, "textures": 1,
      "voxels": false, "bounds": null, "density": 16.0}}]
}"#;
        fs::write(dir.path().join(INDEX_FILE), old).expect("write");
        let (library, warning) = Library::open(dir.path(), Scope::Project).expect("opens");
        assert_eq!(warning, None);
        let brief = library.brief();
        assert_eq!(
            (brief.project_name.as_str(), brief.mc_version.as_str()),
            ("Ink", "1.21.6")
        );
        assert_eq!(brief.style_tags, ["pixel"]);
        assert_eq!(
            Brief {
                project_name: "Ink".to_owned(),
                mc_version: "1.21.6".to_owned(),
                style_tags: vec!["pixel".to_owned()],
                ..Brief::default()
            },
            *brief,
            "every new field has its default"
        );
        assert!(matches!(
            library.entry("a1").and_then(|e| e.meta.as_ref()),
            Some(Meta::Model(m)) if m.texture_size.is_none()
        ));

        library.save().expect("saves");
        let text = fs::read_to_string(dir.path().join(INDEX_FILE)).expect("reads");
        let saved: serde_json::Value = serde_json::from_str(&text).expect("JSON");
        assert_eq!(saved["version"], 3);
        assert_eq!(saved["brief"]["platform"], "vanilla");
        assert_eq!(saved["brief"]["naming"]["case"], "any");
        assert_eq!(saved["entries"][0]["role"], "work");
        let (again, _) = Library::open(dir.path(), Scope::Project).expect("opens");
        assert_eq!(again.brief(), library.brief());
        assert_eq!(again.entries(), library.entries());
    }

    #[test]
    fn a_schema_2_index_reads_as_work_without_style_and_is_saved_as_schema_3() {
        let dir = tempfile::tempdir().expect("temp dir");
        let old = r#"{
  "version": 2,
  "brief": {"project_name": "Ink", "texel_density": 16.0},
  "entries": [{"id": "a1", "kind": "model", "name": "Gun.cubik", "file": "a1-Gun.cubik",
    "source": "C:/src/Gun.cubik", "sha256": "00", "size": 10, "added": 100,
    "meta": {"type": "model", "format": "cubik", "elements": 3, "meshes": 0, "textures": 1,
      "voxels": false, "bounds": null, "density": 16.0, "texture_size": 16}}]
}"#;
        fs::write(dir.path().join(INDEX_FILE), old).expect("write");
        let (library, warning) = Library::open(dir.path(), Scope::Project).expect("opens");
        assert_eq!(warning, None);
        let entry = library.entry("a1").expect("the entry");
        assert_eq!(entry.role, Role::Work);
        assert!(matches!(&entry.meta, Some(Meta::Model(m)) if m.style.is_none()));
        assert!(library.brief().style_sources.is_empty());
        assert_eq!(library.brief().texel_density, Some(16.0));

        library.save().expect("saves");
        let text = fs::read_to_string(dir.path().join(INDEX_FILE)).expect("reads");
        let saved: serde_json::Value = serde_json::from_str(&text).expect("JSON");
        assert_eq!(saved["version"], 3);
        assert_eq!(saved["entries"][0]["role"], "work");
        let (again, _) = Library::open(dir.path(), Scope::Project).expect("opens");
        assert_eq!(again.entries(), library.entries());
    }

    #[test]
    fn the_targets_and_rules_round_trip_through_the_index() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut library = Library::new(dir.path().to_owned(), Scope::Project);
        library.set_brief(Brief {
            goal: "Ink weapons".to_owned(),
            platform: crate::brief::Platform::Oraxen,
            namespace: "splat".to_owned(),
            texel_density: Some(16.0),
            texture_size: Some(32),
            palette: vec![[1, 2, 3], [4, 5, 6]],
            palette_strict: true,
            max_elements: Some(64),
            naming: crate::brief::Naming {
                case: crate::brief::NameCase::Kebab,
                prefix: "ink-".to_owned(),
            },
            display_preset: Some(crate::brief::DisplayPreset::BlockBlock),
            rules: vec!["No gradients".to_owned()],
            ..Brief::default()
        });
        library.save().expect("saves");
        let (again, _) = Library::open(dir.path(), Scope::Project).expect("opens");
        assert_eq!(again.brief(), library.brief());
        assert_eq!(
            again.brief().display_preset,
            Some(crate::brief::DisplayPreset::BlockBlock)
        );
    }

    #[test]
    fn a_hand_edited_index_is_cleaned_when_it_is_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let text = r#"{"version": 2, "brief": {"texel_density": -4, "texture_size": 20,
            "namespace": "Bad Name!", "rules": ["  one  ", ""]}}"#;
        fs::write(dir.path().join(INDEX_FILE), text).expect("write");
        let library = Library::read(dir.path(), Scope::Project).expect("reads");
        let brief = library.brief();
        assert_eq!((brief.texel_density, brief.texture_size), (None, None));
        assert_eq!(brief.namespace, "badname");
        assert_eq!(brief.rules, ["one"]);
    }

    #[test]
    fn read_changes_nothing_and_refuses_what_open_would_move_aside() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(matches!(
            Library::read(dir.path(), Scope::Project),
            Err(LibraryError::Io(error)) if error.kind() == io::ErrorKind::NotFound
        ));
        fs::write(dir.path().join(INDEX_FILE), b"{ not json").expect("write");
        assert!(matches!(
            Library::read(dir.path(), Scope::Project),
            Err(LibraryError::Corrupt(_))
        ));
        assert!(dir.path().join(INDEX_FILE).is_file());
        assert!(!dir.path().join("library.json.bad").exists());
        fs::write(dir.path().join(INDEX_FILE), br#"{"version": 4}"#).expect("write");
        assert!(matches!(
            Library::read(dir.path(), Scope::Project),
            Err(LibraryError::Newer(4))
        ));
    }

    #[test]
    fn a_folder_names_a_library_root_or_the_project_that_holds_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = dir.path().join("project");
        let root = project.join(PROJECT_DIR);
        let mut library = Library::new(root.clone(), Scope::Project);
        library.push(entry("a1", Kind::Model, "Gun.cubik", &[]));
        library.save().expect("saves");
        for folder in [&project, &root] {
            let (found, scope) = Library::locate(folder).expect("finds");
            assert_eq!((&*found, scope), (&*root, Scope::Project));
            let read = Library::read(&found, scope).expect("reads");
            assert_eq!(read.entries().len(), 1);
        }
        let global = Library::new(dir.path().join("global"), Scope::Global);
        global.save().expect("saves");
        let (found, scope) = Library::locate(global.root()).expect("finds");
        assert_eq!((&*found, scope), (global.root(), Scope::Global));
        let empty = dir.path().join("empty");
        fs::create_dir_all(&empty).expect("dir");
        assert!(matches!(
            Library::locate(&empty),
            Err(LibraryError::Io(error)) if error.kind() == io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn an_index_over_the_size_limit_is_never_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = fs::File::create(dir.path().join(INDEX_FILE)).expect("create");
        file.set_len(MAX_INDEX_BYTES + 1).expect("sparse");
        drop(file);
        assert!(matches!(
            Library::open(dir.path(), Scope::Global),
            Err(LibraryError::Io(error)) if error.kind() == io::ErrorKind::InvalidData
        ));
        assert!(
            dir.path().join(INDEX_FILE).is_file(),
            "and it stays where it is"
        );
    }

    #[test]
    fn a_broken_index_is_kept_aside_and_the_library_starts_empty() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join(INDEX_FILE), b"{ not json").expect("write");
        let (library, warning) = Library::open(dir.path(), Scope::Project).expect("opens");
        assert!(library.entries().is_empty());
        assert!(warning.is_some_and(|text| text.contains("library.json.bad")));
        assert!(dir.path().join("library.json.bad").is_file());
        assert!(!dir.path().join(INDEX_FILE).exists());
    }

    #[test]
    fn a_missing_index_is_an_empty_library_and_writes_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("none");
        let (library, warning) = Library::open(&root, Scope::Global).expect("opens");
        assert!(library.entries().is_empty() && warning.is_none());
        assert!(!root.exists());
    }

    #[test]
    fn remove_deletes_the_copies_and_forgets_the_content() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut library = Library::new(dir.path().to_owned(), Scope::Global);
        let item = entry("a1", Kind::Texture, "t.png", &[]);
        fs::create_dir_all(library.files_dir()).expect("dir");
        fs::create_dir_all(library.thumbs_dir()).expect("dir");
        let file = library.file_path(&item).expect("plain");
        let thumb = library.thumb_path("a1").expect("plain");
        fs::write(&file, b"x").expect("write");
        fs::write(&thumb, b"x").expect("write");
        library.push(item);
        assert!(library.remove("zz").is_none());
        let gone = library.remove("a1").expect("removed");
        assert!(!file.exists() && !thumb.exists());
        assert!(library.by_sha(&gone.sha256).is_none() && library.entries().is_empty());
    }

    #[test]
    fn an_index_cannot_point_outside_the_library() {
        let mut item = entry("a1", Kind::Texture, "t.png", &[]);
        item.file = "../../secret.png".to_owned();
        let library = Library::new(PathBuf::from("lib"), Scope::Global);
        assert_eq!(library.file_path(&item), None);
        assert_eq!(library.thumb_path("../x"), None);
    }
}
