//! The brief and the style references: `brief_get`, `refs_import` and `refs_get`.

use std::path::PathBuf;

use cuboid_assets::mc_versions::McVersion;
use cuboid_library::doc::load;
use cuboid_library::{
    Entry, INDEX_FILE, ImportOptions, Kind, Library, Quiet, Role, Scope, UNKNOWN_LICENCE, outline,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::inspect::{GROUP_DEPTH, OUTLINE_MAX, outline_value, value_of};
use crate::schema::{array_of, integer, object, string};
use crate::server::{Output, Server};

/// Entries of the brief on a page; 25 are about 11 KB.
const ENTRIES_PAGE: usize = 25;
/// Most entries a page may hold.
const ENTRIES_MAX: usize = 50;
/// Names kept of a lore note, which can list hundreds.
const LORE_NAMES: usize = 12;
/// Ids and notices a result lists.
const LISTED: usize = 50;

/// Arguments of `brief_get`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BriefGet {
    cursor: Option<String>,
    limit: Option<usize>,
}

/// Arguments of `refs_import`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefsImport {
    folder: String,
    licence: Option<String>,
    tags: Option<Vec<String>>,
}

/// Arguments of `refs_get`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefsGet {
    id: String,
    cursor: Option<String>,
    limit: Option<usize>,
}

/// A position from a `cursor`: the number the previous page gave as `next`.
pub(crate) fn offset_of(cursor: Option<&str>) -> Result<usize, ToolError> {
    match cursor {
        None => Ok(0),
        Some(text) => text
            .parse()
            .map_err(|_| ToolError::input(format!("`{text}` is not a cursor"))),
    }
}

pub(crate) fn brief_get_input() -> Value {
    let mut schema = object(
        json!({
            "cursor": string("The `next` of the previous page."),
            "limit": integer("Entries on a page (default 25, at most 50)."),
        }),
        &[],
    );
    schema["examples"] = json!([{}, {"cursor": "25", "limit": 25}]);
    schema
}

pub(crate) fn brief_get_output() -> Value {
    object(
        json!({
            "format": string("Always cuboid-brief."),
            "version": integer("Version of this output."),
            "offset": integer("Position of the first entry listed."),
            "total_entries": integer("Entries in the library."),
            "next": string("Cursor of the next page; left out on the last."),
            "brief": {"type": "object", "description": "The project: version, targets, naming, rules."},
            "observed": {"type": "object", "description": "What the library's own work shows: palette, texel density."},
            "style_sources": array_of(json!({"type": "object"})),
            "style": {"type": "object", "description": "What the style references share (first page)."},
            "entries": array_of(json!({"type": "object"})),
            "lore": array_of(json!({"type": "object"})),
            "suggestions": array_of(json!({"type": "object"})),
        }),
        &["format", "version", "offset", "total_entries", "entries"],
    )
}

pub(crate) fn refs_import_input() -> Value {
    let mut schema = object(
        json!({
            "folder": string("Folder under the project folder that holds reference models and textures."),
            "licence": string("Licence of the references (default unknown)."),
            "tags": array_of(json!({"type": "string"})),
        }),
        &["folder"],
    );
    schema["examples"] = json!([
        {"folder": "refs/lamps"},
        {"folder": "refs/swords", "licence": "CC0", "tags": ["sword", "pixel"]}
    ]);
    schema
}

pub(crate) fn refs_import_output() -> Value {
    object(
        json!({
            "folder": string("The folder, relative to the project folder."),
            "added": integer("References added."),
            "ids": array_of(json!({"type": "string"})),
            "more": integer("Ids not listed."),
            "duplicates": integer("Files whose content the library had."),
            "dropped": integer("References taken out because their file is gone or changed."),
            "skipped": integer("Files that are no models or textures."),
            "notices": array_of(json!({"type": "string"})),
            "summary": string("One line."),
        }),
        &["folder", "added", "ids", "summary"],
    )
}

pub(crate) fn refs_get_input() -> Value {
    let mut schema = object(
        json!({
            "id": string("The id of a reference, from brief_get."),
            "cursor": string("The `next` of the previous page of elements."),
            "limit": integer("Elements on a page (default 24)."),
        }),
        &["id"],
    );
    schema["examples"] =
        json!([{"id": "3fa2c1d09b7e4410"}, {"id": "3fa2c1d09b7e4410", "cursor": "24"}]);
    schema
}

pub(crate) fn refs_get_output() -> Value {
    object(
        json!({
            "id": string("The id."),
            "name": string("The file name it was imported under."),
            "kind": string("model or texture."),
            "role": string("style or work."),
            "licence": string("Licence of the file."),
            "tags": array_of(json!({"type": "string"})),
            "source": string("Where it was imported from."),
            "meta": {"type": ["object", "null"], "description": "Counts, bounds, density and style of a model; size, frames and palette of a texture."},
            "outline": {"type": "object", "description": "The outline of a model, as model_outline gives it, without ids."},
        }),
        &["id", "name", "kind", "role", "licence", "meta"],
    )
}

impl Server<'_> {
    /// The project library read from disk; an empty one when the project has none yet.
    pub(crate) fn read_library(&self) -> Result<Library, ToolError> {
        let dir = self.library_dir()?;
        if !dir.join(INDEX_FILE).is_file() {
            return Ok(Library::new(dir, Scope::Project));
        }
        Ok(Library::read(&dir, Scope::Project)?)
    }

    /// The stored copy of a library entry. The path goes through the resolver like every other,
    /// so that a link out of the project folder is refused.
    pub(crate) fn stored_copy(
        &self,
        library: &Library,
        entry: &Entry,
    ) -> Result<PathBuf, ToolError> {
        let path = library
            .file_path(entry)
            .ok_or_else(|| ToolError::io("the index names a stored copy that is no file"))?;
        self.root.resolve(&self.root.relative(&path))
    }

    /// The game version the brief names, if it names one.
    pub(crate) fn brief_version(&self) -> Option<McVersion> {
        let library = self.read_library().ok()?;
        McVersion::parse(&library.brief().mc_version)
    }

    pub(crate) fn brief_get(&mut self, args: BriefGet) -> Result<Output, ToolError> {
        let offset = offset_of(args.cursor.as_deref())?;
        let limit = args.limit.unwrap_or(ENTRIES_PAGE).clamp(1, ENTRIES_MAX);
        let library = self.read_library()?;
        let mut brief: Value = serde_json::from_str(&library.brief_json()?)?;
        let Some(brief) = brief.as_object_mut() else {
            return Err(ToolError::io("the brief is not an object"));
        };
        let entries = match brief.remove("entries") {
            Some(Value::Array(entries)) => entries,
            _ => Vec::new(),
        };
        let total = entries.len();
        let offset = offset.min(total);
        let end = (offset + limit).min(total);
        if offset > 0 {
            brief.retain(|key, _| key == "format" || key == "version");
        }
        if let Some(Value::Array(lore)) = brief.get_mut("lore") {
            for note in lore {
                if let Some(Value::Array(names)) = note.get_mut("names") {
                    names.truncate(LORE_NAMES);
                }
            }
        }
        brief.insert("offset".to_owned(), json!(offset));
        brief.insert("total_entries".to_owned(), json!(total));
        if end < total {
            brief.insert("next".to_owned(), json!(end.to_string()));
        }
        brief.insert(
            "entries".to_owned(),
            Value::Array(
                entries
                    .into_iter()
                    .skip(offset)
                    .take(end - offset)
                    .collect(),
            ),
        );
        Ok(Output::json(Value::Object(std::mem::take(brief))))
    }

    pub(crate) fn refs_import(&mut self, args: RefsImport) -> Result<Output, ToolError> {
        let folder = self.root.resolve(&args.folder)?;
        let dir = self.library_dir()?;
        let (mut library, _) = Library::open(&dir, Scope::Project)?;
        let options = ImportOptions {
            tags: args.tags.unwrap_or_default(),
            licence: args.licence.unwrap_or_else(|| UNKNOWN_LICENCE.to_owned()),
            ..ImportOptions::default()
        };
        let report = library.import_style(&folder, &options, &Quiet)?;
        if let Err(error) = library.write_brief_md() {
            log::warn!("cannot write brief.md: {error}");
        }
        let notices = report.notices();
        let more = report.added.len().saturating_sub(LISTED);
        Ok(Output::json(json!({
            "folder": self.root.relative(&folder),
            "added": report.added.len(),
            "ids": report.added.iter().take(LISTED).collect::<Vec<_>>(),
            "more": more,
            "duplicates": report.duplicates,
            "dropped": report.dropped,
            "skipped": report.skipped.total(),
            "notices": notices.iter().take(LISTED).collect::<Vec<_>>(),
            "summary": report.summary(),
        })))
    }

    pub(crate) fn refs_get(&mut self, args: RefsGet) -> Result<Output, ToolError> {
        let offset = offset_of(args.cursor.as_deref())?;
        let library = self.read_library()?;
        let entry = library
            .entry(&args.id)
            .ok_or_else(|| ToolError::no_match(format!("the library has no entry {}", args.id)))?;
        let mut value = json!({
            "id": entry.id,
            "name": entry.name,
            "kind": entry.kind.name(),
            "role": if entry.role == Role::Style { "style" } else { "work" },
            "licence": entry.licence,
            "tags": entry.tags,
            "source": entry.source,
            "meta": value_of(&entry.meta)?,
        });
        if entry.kind == Kind::Model {
            let opened = load(&self.stored_copy(&library, entry)?)?;
            let limit = args
                .limit
                .unwrap_or(cuboid_library::OUTLINE_PAGE)
                .clamp(1, OUTLINE_MAX);
            let page = outline(&opened.doc).page(offset, limit);
            value["outline"] = outline_value(&page, false, GROUP_DEPTH)?;
        } else if entry.kind != Kind::Texture {
            return Err(ToolError::input("only models and textures can be read"));
        }
        Ok(Output::json(value))
    }
}
