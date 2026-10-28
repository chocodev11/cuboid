//! The files of a project: `project_list`, `project_open`, `project_save` and `project_export`.

use std::fs;
use std::path::{Component, Path};

use cuboid_assets::mc_versions::McVersion;
use cuboid_fmt_cubik::{SaveOptions, save_path};
use cuboid_fmt_minecraft::{JsonOptions, PackOptions, export_pack};
use cuboid_library::brief::minecraft_template;
use cuboid_library::kind::{Class, classify, extension};
use cuboid_library::meta::document_meta;
use cuboid_library::{Kind, PROJECT_DIR};
use cuboid_model::{CaptureScope, Categories, Document, capture};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::brief::offset_of;
use crate::error::ToolError;
use crate::inspect::{java_bounds, texture_list};
use crate::schema::{
    array_of, boolean, choice, expected_revision, integer, model, object, paging, string,
};
use crate::select::matches;
use crate::server::{OpenModel, Output, Server, stamp};

/// Entries of the folder `project_list` looks at, and how deep.
const MAX_VISITED: usize = 20_000;
const MAX_DEPTH: usize = 16;
/// The largest `.json` file read to tell a model from the other kinds.
const MAX_JSON: u64 = 2 << 20;
/// Files on a page of `project_list`.
const LIST_PAGE: usize = 50;
const LIST_MAX: usize = 150;
/// Paths and notices a result lists.
const LISTED: usize = 50;
/// Folders the walk does not enter: the library, which holds the stored copies of the
/// references, and the pictures of the render tools.
const SKIPPED: [&str; 2] = [PROJECT_DIR, "renders"];

/// Arguments of `project_list`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectList {
    glob: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}

/// The kind of model `project_open` makes.
#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Template {
    Item,
    Block,
}

/// Arguments of `project_open`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectOpen {
    model: String,
    create: Option<Template>,
}

/// Arguments of `project_save`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSave {
    model: String,
    path: Option<String>,
    expected_revision: Option<String>,
    overwrite: Option<bool>,
}

/// What `project_export` writes.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExportFormat {
    JavaModel,
    ResourcePack,
}

/// Arguments of `project_export`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectExport {
    model: String,
    format: ExportFormat,
    folder: Option<String>,
    version: Option<String>,
    namespace: Option<String>,
    id: Option<String>,
    overwrite: Option<bool>,
}

pub(crate) fn project_list_input() -> Value {
    let mut properties = json!({
        "glob": string("Glob on the path (with a /) or on the file name (without), such as models/** or *.cubik."),
    });
    paging(&mut properties, "Files", LIST_PAGE);
    let mut schema = object(properties, &[]);
    schema["examples"] = json!([{}, {"glob": "*.bbmodel", "limit": 20}]);
    schema
}

pub(crate) fn project_list_output() -> Value {
    object(
        json!({
            "total": integer("Model files that match."),
            "offset": integer("Position of the first file listed."),
            "next": string("Cursor of the next page; left out on the last."),
            "files": array_of(object(
                json!({
                    "path": string("Relative to the project folder."),
                    "format": string("cubik, cub, bbmodel or json."),
                    "size": integer("Bytes on disk; 0 for a model never saved."),
                    "open": boolean("Open in the server."),
                    "unsaved": boolean("Edited since it was opened or saved."),
                    "revision": string("Of an open model."),
                }),
                &["path", "format", "size", "open", "unsaved"],
            )),
        }),
        &["total", "offset", "files"],
    )
}

pub(crate) fn project_open_input() -> Value {
    let mut schema = object(
        json!({
            "model": model(),
            "create": choice(
                &["item", "block"],
                "Start a new Minecraft model at this .cubik path, with the version and display preset of the brief. The file must not exist.",
            ),
        }),
        &["model"],
    );
    schema["examples"] = json!([
        {"model": "lamp.cubik"},
        {"model": "models/torch.cubik", "create": "item"}
    ]);
    schema
}

pub(crate) fn project_open_output() -> Value {
    object(
        json!({
            "model": string("The model, as the other tools name it."),
            "revision": string("The revision of the model."),
            "created": boolean("A new model that is not on disk yet."),
            "format": string("cubik, cub, bbmodel or json."),
            "elements": integer("Elements."),
            "groups": integer("Groups."),
            "meshes": integer("Meshes."),
            "bounds": {"type": ["object", "null"], "description": "The box around the elements, in model space."},
            "textures": array_of(object(
                json!({"name": string("Label."), "size": array_of(json!({"type": "integer"}))}),
                &["name", "size"],
            )),
            "read_only": boolean("A style reference or a read-only server: edits are refused."),
            "notes": array_of(json!({"type": "string"})),
        }),
        &[
            "model",
            "revision",
            "created",
            "format",
            "elements",
            "groups",
            "meshes",
            "textures",
            "read_only",
        ],
    )
}

pub(crate) fn project_save_input() -> Value {
    let mut schema = object(
        json!({
            "model": model(),
            "path": string("Where to save, a .cubik path. Default: the model's own file, or its name with .cubik."),
            "expected_revision": expected_revision(),
            "overwrite": boolean("Replace a file that changed on disk since the model was opened or saved."),
        }),
        &["model"],
    );
    schema["examples"] = json!([
        {"model": "lamp.cubik"},
        {"model": "lamp.bbmodel", "path": "lamp.cubik", "overwrite": true}
    ]);
    schema
}

pub(crate) fn project_save_output() -> Value {
    object(
        json!({
            "model": string("The model; a model saved to another path is named by it from now on."),
            "revision": string("The revision that was saved."),
            "path": string("The file written, relative to the project folder."),
            "bytes": integer("Size of the file."),
        }),
        &["model", "revision", "path", "bytes"],
    )
}

pub(crate) fn project_export_input() -> Value {
    let mut schema = object(
        json!({
            "model": model(),
            "format": choice(
                &["java_model", "resource_pack"],
                "A Minecraft JSON model with its textures, or a resource pack folder.",
            ),
            "folder": string("Folder under the project folder to write into (default exports/<model name>)."),
            "version": string("Game version such as 1.21.6 (default: the brief's, else the newest known)."),
            "namespace": string("Resource namespace of a pack (default: the brief's, else minecraft)."),
            "id": string("Id of the model in a pack (default: the model's file name)."),
            "overwrite": boolean("Replace files that are already there; without it nothing is written when one is. A second export to the same folder needs overwrite."),
        }),
        &["model", "format"],
    );
    schema["examples"] = json!([
        {"model": "lamp.cubik", "format": "java_model"},
        {"model": "lamp.cubik", "format": "resource_pack", "folder": "build/pack", "version": "1.21.6"},
        {"model": "lamp.cubik", "format": "java_model", "overwrite": true}
    ]);
    schema
}

pub(crate) fn project_export_output() -> Value {
    object(
        json!({
            "model": string("The model."),
            "revision": string("The revision that was exported."),
            "format": string("java_model or resource_pack."),
            "version": string("The game version."),
            "pack_format": string("The pack format of a resource pack."),
            "count": integer("Files written."),
            "paths": array_of(json!({"type": "string"})),
            "warnings": array_of(json!({"type": "string"})),
        }),
        &[
            "model", "revision", "format", "version", "count", "paths", "warnings",
        ],
    )
}

/// What a model file is called in a listing, `None` for a file that is no model.
fn model_format(path: &Path, size: u64) -> Option<&'static str> {
    let name = path.file_name()?.to_string_lossy();
    match extension(&name).as_str() {
        "cubik" => Some("cubik"),
        "cub" => Some("cub"),
        "bbmodel" => Some("bbmodel"),
        "json" if size <= MAX_JSON => {
            let bytes = fs::read(path).ok()?;
            matches!(
                classify(&name, &bytes),
                Class::Entry {
                    kind: Kind::Model,
                    ..
                }
            )
            .then_some("json")
        }
        _ => None,
    }
}

/// Whether the path stays inside the folder it is joined to.
fn stays_inside(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn pack_format_text(major: u32, minor: u32) -> String {
    if minor == 0 {
        major.to_string()
    } else {
        format!("{major}.{minor}")
    }
}

impl Server<'_> {
    pub(crate) fn project_list(&mut self, args: ProjectList) -> Result<Output, ToolError> {
        let offset = offset_of(args.cursor.as_deref())?;
        let mut found: Vec<(String, &'static str, u64)> = Vec::new();
        let mut visited = 0;
        let mut stack = vec![(self.root.path().to_owned(), 0_usize)];
        while let Some((folder, depth)) = stack.pop() {
            let Ok(entries) = fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.flatten() {
                visited += 1;
                if visited > MAX_VISITED {
                    break;
                }
                // `file_type` does not follow links, so a link is neither walked nor listed.
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let name = entry.file_name().to_string_lossy().into_owned();
                if kind.is_dir() && depth < MAX_DEPTH {
                    if !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                        stack.push((entry.path(), depth + 1));
                    }
                } else if kind.is_file() {
                    let size = entry.metadata().map_or(0, |meta| meta.len());
                    if let Some(format) = model_format(&entry.path(), size) {
                        found.push((self.root.relative(&entry.path()), format, size));
                    }
                }
            }
        }
        for open in &self.models {
            if open.stamp.is_none() && !found.iter().any(|(path, ..)| *path == open.name) {
                found.push((open.name.clone(), "cubik", 0));
            }
        }
        if let Some(glob) = &args.glob {
            found.retain(|(path, ..)| {
                let subject = if glob.contains('/') {
                    path.as_str()
                } else {
                    path.rsplit('/').next().unwrap_or(path)
                };
                matches(glob, subject)
            });
        }
        found.sort_by_key(|(path, ..)| path.to_lowercase());
        let total = found.len();
        let offset = offset.min(total);
        let limit = args.limit.unwrap_or(LIST_PAGE).clamp(1, LIST_MAX);
        let end = (offset + limit).min(total);
        let files: Vec<Value> = found
            .get(offset..end)
            .unwrap_or_default()
            .iter()
            .map(|(path, format, size)| {
                let open = self.find(path).map(|index| &self.models[index]);
                let mut file = json!({
                    "path": path,
                    "format": format,
                    "size": size,
                    "open": open.is_some(),
                    "unsaved": open.is_some_and(|model| model.core.is_dirty()),
                });
                if let Some(model) = open {
                    file["revision"] = json!(self.token(model.core.doc()));
                }
                file
            })
            .collect();
        let mut value = json!({"total": total, "offset": offset, "files": files});
        if end < total {
            value["next"] = json!(end.to_string());
        }
        Ok(Output::json(value))
    }

    pub(crate) fn project_open(&mut self, args: ProjectOpen) -> Result<Output, ToolError> {
        let created = args.create.is_some();
        let index = match args.create {
            None => self.model(&args.model)?,
            Some(template) => {
                let path = self.root.resolve(&args.model)?;
                if extension(&path.to_string_lossy()) != "cubik" {
                    return Err(ToolError::path("a new model is saved as a .cubik file"));
                }
                self.outside_library(&path)?;
                let name = self.root.relative(&path);
                if path.exists() || self.find(&name).is_some() {
                    return Err(ToolError::path(format!(
                        "{name} exists or is open; open it without `create`"
                    )));
                }
                let project = self.minecraft_project();
                let version = project
                    .version
                    .as_deref()
                    .and_then(McVersion::parse)
                    .unwrap_or_else(cuboid_fmt_minecraft::latest_version);
                let display = self.read_library()?.brief().display_preset;
                let mut doc = Document::new(cuboid_ops::textures::default_list());
                let item = matches!(template, Template::Item);
                minecraft_template(&mut doc, version, item, display);
                let core = self.core_of(doc, Some(&project), &path);
                self.insert(OpenModel::new(name, path, core, Some(project), Vec::new()))?
            }
        };
        let model = &self.models[index];
        let doc = model.core.doc();
        let format = extension(&model.path.to_string_lossy());
        let meta = document_meta(&format, doc);
        Ok(Output::json(json!({
            "model": model.name,
            "revision": self.token(doc),
            "created": created,
            "format": format,
            "elements": doc.elements.len(),
            "groups": doc.groups.len(),
            "meshes": doc.meshes.len(),
            "bounds": java_bounds(meta.bounds),
            "textures": texture_list(doc),
            "read_only": self.options.read_only || self.writable(index).is_err(),
            "notes": model.notes.iter().take(LISTED).collect::<Vec<_>>(),
        })))
    }

    pub(crate) fn project_save(&mut self, args: ProjectSave) -> Result<Output, ToolError> {
        let index = self.model(&args.model)?;
        self.writable(index)?;
        let model = &self.models[index];
        self.check_revision(model.core.doc(), args.expected_revision.as_deref())?;
        let target = match &args.path {
            Some(text) => self.root.resolve(text)?,
            None => model.path.with_extension("cubik"),
        };
        if extension(&target.to_string_lossy()) != "cubik" {
            return Err(ToolError::path(
                "models are saved as .cubik files; project_export writes Minecraft JSON",
            ));
        }
        self.outside_library(&target)?;
        let name = self.root.relative(&target);
        if self.find(&name).is_some_and(|other| other != index) {
            return Err(ToolError::path(format!("{name} is open as another model")));
        }
        let own = target == model.path && model.stamp.is_some();
        if target.exists()
            && !args.overwrite.unwrap_or(false)
            && !(own && stamp(&target) == model.stamp)
        {
            return Err(ToolError::file_changed(format!(
                "{name} was not written by this server, or changed on disk since"
            )));
        }
        let doc = model.core.doc();
        let snapshot = capture(
            doc,
            model.core.selection(),
            CaptureScope {
                categories: Categories::ALL,
                selection_only: false,
            },
        );
        let options = SaveOptions {
            project: model.project.clone(),
            verify: true,
            ..SaveOptions::default()
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        save_path(
            &target,
            &snapshot,
            doc.retained.as_deref(),
            &doc.overrides,
            &options,
        )
        .map_err(|error| ToolError::io(error.to_string()))?;
        let revision = self.token(doc);
        let bytes = fs::metadata(&target).map_or(0, |meta| meta.len());
        let model = &mut self.models[index];
        model.core.mark_saved();
        model.stamp = stamp(&target);
        model.path = target;
        model.name.clone_from(&name);
        Ok(Output::json(json!({
            "model": name,
            "revision": revision,
            "path": name,
            "bytes": bytes,
        })))
    }

    pub(crate) fn project_export(&mut self, args: ProjectExport) -> Result<Output, ToolError> {
        let index = self.model(&args.model)?;
        let model = &self.models[index];
        let doc = model.core.doc();
        let stem = model
            .path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let library = self.read_library()?;
        let brief = library.brief();
        let version = match args.version.as_deref() {
            Some(text) => McVersion::parse(text)
                .ok_or_else(|| ToolError::input(format!("`{text}` is not a game version")))?,
            None => model
                .project
                .as_ref()
                .and_then(|project| project.version.as_deref())
                .and_then(McVersion::parse)
                .or_else(|| McVersion::parse(&brief.mc_version))
                .unwrap_or_else(cuboid_fmt_minecraft::latest_version),
        };
        let folder = args.folder.unwrap_or_else(|| format!("exports/{stem}"));
        // A folder that leaves the project is refused before anything is built.
        self.root.resolve(&folder)?;
        let export_error =
            |error: cuboid_fmt_minecraft::McError| ToolError::export(error.to_string());
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        let mut pack_format = None;
        match args.format {
            ExportFormat::JavaModel => {
                let exported =
                    cuboid_fmt_minecraft::json_export::export(doc, &JsonOptions::new(version))
                        .map_err(export_error)?;
                files.push((format!("{stem}.json"), exported.json.into_bytes()));
                files.extend(
                    exported
                        .files
                        .into_iter()
                        .map(|file| (file.path, file.bytes)),
                );
                warnings.extend(exported.notices.iter().map(ToString::to_string));
            }
            ExportFormat::ResourcePack => {
                let mut options = PackOptions::new(version);
                if let Some(namespace) = args
                    .namespace
                    .or_else(|| (!brief.namespace.is_empty()).then(|| brief.namespace.clone()))
                {
                    options.namespace = namespace;
                }
                if let Some(id) = args.id {
                    options.id = id;
                } else if !stem.is_empty() {
                    // A resource id has lower case letters, digits and these.
                    options.id = stem
                        .to_lowercase()
                        .chars()
                        .map(|c| {
                            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                                c
                            } else {
                                '_'
                            }
                        })
                        .collect();
                }
                let pack = export_pack(doc, &options).map_err(export_error)?;
                pack_format = Some(pack_format_text(pack.format.major, pack.format.minor));
                files.extend(pack.files.into_iter().map(|file| (file.path, file.bytes)));
                warnings.extend(pack.notices.iter().map(ToString::to_string));
            }
        }
        let mut targets = Vec::with_capacity(files.len());
        for (path, _) in &files {
            if !stays_inside(path) {
                return Err(ToolError::export(format!(
                    "the export names the path `{path}`"
                )));
            }
            let target = self.root.resolve(&format!("{folder}/{path}"))?;
            self.outside_library(&target)?;
            targets.push(target);
        }
        // As in the export dialog, a file that is there is not replaced unless asked for, and
        // then nothing is written at all.
        let taken: Vec<String> = targets
            .iter()
            .filter(|target| target.exists())
            .map(|target| self.root.relative(target))
            .collect();
        if !taken.is_empty() && !args.overwrite.unwrap_or(false) {
            let listed = taken.iter().take(LISTED).cloned().collect::<Vec<_>>();
            return Err(ToolError::file_changed(format!(
                "{} of the {} files are already there, nothing was written: {}",
                taken.len(),
                files.len(),
                listed.join(", ")
            ))
            .hinted("Pass overwrite: true to replace them, or export to another folder."));
        }
        let mut paths = Vec::with_capacity(files.len());
        for ((_, bytes), target) in files.iter().zip(&targets) {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            cuboid_io::write_atomic(target, bytes)?;
            paths.push(self.root.relative(target));
        }
        let mut value = json!({
            "model": model.name,
            "revision": self.token(doc),
            "format": match args.format {
                ExportFormat::JavaModel => "java_model",
                ExportFormat::ResourcePack => "resource_pack",
            },
            "version": version.to_string(),
            "count": paths.len(),
            "paths": paths.iter().take(LISTED).collect::<Vec<_>>(),
            "warnings": warnings.iter().take(LISTED).collect::<Vec<_>>(),
        });
        if let Some(pack_format) = pack_format {
            value["pack_format"] = json!(pack_format);
        }
        Ok(Output::json(value))
    }
}
