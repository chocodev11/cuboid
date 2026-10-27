//! The model check: validates a model file against a target and the rules of the project brief,
//! as the Validator panel does, without the app. Reads only; nothing is written.

use std::collections::HashMap;
use std::path::Path;

use cuboid_model::{Document, ModelKind, ObjId};
use cuboid_validate::{Issue, McVersion, Profile, Severity, Target, validate};
use serde::Serialize;

use crate::doc::{LoadError, load};
use crate::store::{INDEX_FILE, Library, PROJECT_DIR, Scope};

/// Version of the JSON output.
const JSON_VERSION: u32 = 1;

/// The output a caller asks for.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Want {
    /// A Java block model.
    Block,
    /// A Java item model.
    Item,
    /// A resource pack.
    Pack,
}

/// An element an issue is about.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Object {
    /// The number the element has in the element list.
    pub number: u32,
    /// Its name.
    pub name: String,
}

/// One issue.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Found {
    /// The stable code of the rule.
    pub code: &'static str,
    /// `error`, `warning` or `info`.
    pub severity: &'static str,
    /// The elements it is about.
    pub objects: Vec<Object>,
    /// What is wrong.
    pub message: String,
    /// What to do about it.
    pub hint: String,
    /// Whether the Validator panel offers a fix.
    pub fix: bool,
}

/// The result of a check, in the order and with the names of `cuboid-cli check --json`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    /// Always `cuboid-check`.
    pub format: &'static str,
    /// Version of this output.
    pub version: u32,
    /// The model file as it was given.
    pub model: String,
    /// What the model was checked against.
    pub target: String,
    /// The library the project rules came from.
    pub project: Option<String>,
    /// Issues of the severity error.
    pub errors: usize,
    /// Issues of the severity warning.
    pub warnings: usize,
    /// Issues of the severity info.
    pub infos: usize,
    /// What the importer left out or changed.
    pub notes: Vec<String>,
    /// The issues.
    pub issues: Vec<Found>,
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

/// What the validator checks for, as the Validator panel names it.
fn target_label(target: &Target) -> String {
    match target {
        Target::Cubik => "a Cubik file".to_owned(),
        Target::JavaBlock { version } => format!("Java block {version}"),
        Target::JavaItem { version } => format!("Java item {version}"),
        Target::Bbmodel { .. } => "a Blockbench project".to_owned(),
        Target::ResourcePack { version } => format!("a resource pack for {version}"),
    }
}

/// The library of the project: the folder `brief` names, else the project library next to the
/// model, if there is one.
fn project_library(model: &Path, folder: Option<&Path>) -> Result<Option<Library>, LoadError> {
    if let Some(folder) = folder {
        let (root, scope) = Library::locate(folder)
            .map_err(|err| LoadError::Failed(format!("{}: {err}", folder.display())))?;
        return Library::read(&root, scope)
            .map(Some)
            .map_err(|err| LoadError::Failed(format!("{}: {err}", root.display())));
    }
    let dir = model
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let root = dir.join(PROJECT_DIR);
    if !root.join(INDEX_FILE).is_file() {
        return Ok(None);
    }
    Library::read(&root, Scope::Project)
        .map(Some)
        .map_err(|err| LoadError::Failed(format!("{}: {err}", root.display())))
}

/// The target of a document as the app derives it from the file: `.bbmodel` and Minecraft JSON
/// are Minecraft models, a `.cubik` file is one when its project settings say so, and the block
/// or the item is the model kind of the document.
fn target_of(doc: &Document, minecraft: bool, version: McVersion, want: Option<Want>) -> Target {
    let item = doc.props.minecraft.kind == ModelKind::Item;
    match want {
        Some(Want::Block) => Target::JavaBlock { version },
        Some(Want::Item) => Target::JavaItem { version },
        Some(Want::Pack) => Target::ResourcePack { version },
        None if !minecraft => Target::Cubik,
        None if item => Target::JavaItem { version },
        None => Target::JavaBlock { version },
    }
}

/// Opens the model and validates it against `want` (by default what the file is) and the rules
/// of the brief in the folder `brief` names, else of the project library beside the model.
pub fn check(model: &Path, brief: Option<&Path>, want: Option<Want>) -> Result<Report, LoadError> {
    let opened = load(model)?;
    let library = project_library(model, brief)?;
    let extension = model
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let meta = opened.project.as_ref();
    let minecraft = match extension.as_str() {
        "bbmodel" | "json" => true,
        "cubik" => meta.is_some_and(|meta| meta.preset == "minecraft"),
        _ => false,
    };
    // The version of the file, else the one of the project, else the newest the program knows.
    let version = meta
        .and_then(|meta| meta.version.as_deref())
        .or(library.as_ref().map(|lib| lib.brief().mc_version.as_str()))
        .and_then(McVersion::parse)
        .unwrap_or_else(cuboid_fmt_minecraft::latest_version);
    let target = target_of(&opened.doc, minecraft, version, want);
    let mut profile = Profile::new(target);
    let stem = model
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    match &library {
        Some(library) => library.brief().fill_profile(&mut profile, &stem),
        None => profile.project.model_name = stem,
    }
    let issues = validate(&opened.doc, &profile);
    Ok(report(
        model,
        &target,
        library.as_ref(),
        opened.notes,
        &opened.doc,
        &issues,
    ))
}

fn report(
    model: &Path,
    target: &Target,
    library: Option<&Library>,
    notes: Vec<String>,
    doc: &Document,
    issues: &[Issue],
) -> Report {
    let numbered: HashMap<ObjId, (u32, String)> = doc
        .elements
        .ids()
        .iter()
        .zip(doc.elements.items())
        .zip(doc.elements.display_ids())
        .map(|((&id, el), &no)| (id, (no, el.base.label.to_string())))
        .collect();
    let count = |severity| issues.iter().filter(|i| i.severity == severity).count();
    Report {
        format: "cuboid-check",
        version: JSON_VERSION,
        model: model.display().to_string(),
        target: target_label(target),
        project: library.map(|library| library.root().display().to_string()),
        errors: count(Severity::Error),
        warnings: count(Severity::Warning),
        infos: count(Severity::Info),
        notes,
        issues: issues
            .iter()
            .map(|issue| Found {
                code: issue.code,
                severity: severity_name(issue.severity),
                objects: issue
                    .objects
                    .iter()
                    .filter_map(|id| numbered.get(id))
                    .map(|(number, name)| Object {
                        number: *number,
                        name: name.clone(),
                    })
                    .collect(),
                message: issue.message.clone(),
                hint: issue.hint.clone(),
                fix: issue.fix.is_some(),
            })
            .collect(),
    }
}
