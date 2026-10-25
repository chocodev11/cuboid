//! `check`: validates a model file against a target and the rules of the project brief, as the
//! Validator panel does, without the app. Reads only; nothing is written.

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

use cuboid_library::{INDEX_FILE, Library, PROJECT_DIR, Scope};
use cuboid_model::{Document, ModelKind, ObjId};
use cuboid_validate::{Issue, McVersion, Profile, Severity, Target, validate};
use serde::Serialize;

use crate::args::Args;
use crate::{Fail, brief, doc, dump};

/// Version of the JSON output.
const JSON_VERSION: u32 = 1;

/// The output a command line asks for.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Want {
    Block,
    Item,
    Pack,
}

fn parse_target(text: &str) -> Result<Want, Fail> {
    match text {
        "block" => Ok(Want::Block),
        "item" => Ok(Want::Item),
        "pack" => Ok(Want::Pack),
        other => Err(Fail::Usage(format!(
            "`{other}` is not a target; use block, item or pack"
        ))),
    }
}

#[derive(Serialize)]
struct Object {
    number: u32,
    name: String,
}

#[derive(Serialize)]
struct Found {
    code: &'static str,
    severity: &'static str,
    objects: Vec<Object>,
    message: String,
    hint: String,
    fix: bool,
}

#[derive(Serialize)]
struct Report {
    format: &'static str,
    version: u32,
    model: String,
    target: String,
    project: Option<String>,
    errors: usize,
    warnings: usize,
    infos: usize,
    notes: Vec<String>,
    issues: Vec<Found>,
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

/// The library of the project: the folder `--brief` names, else the project library next to the
/// model, if there is one.
fn project_library(model: &Path, folder: Option<&Path>) -> Result<Option<Library>, Fail> {
    if let Some(folder) = folder {
        return brief::open(folder).map(Some);
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
        .map_err(|err| Fail::Error(format!("{}: {err}", root.display())))
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

/// Opens the model and validates it.
fn check(model: &Path, brief: Option<&Path>, want: Option<Want>) -> Result<Report, Fail> {
    let opened = doc::load(model)?;
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

fn plural(count: usize, word: &str) -> String {
    format!("{count} {word}{}", if count == 1 { "" } else { "s" })
}

impl Report {
    /// One line per issue: severity, code, the elements, the message and whether a fix exists,
    /// then a summary line.
    fn text(&self) -> String {
        let mut out = format!(
            "{} against {}{}\n",
            self.model,
            self.target,
            self.project
                .as_ref()
                .map_or_else(String::new, |root| format!(", project rules from {root}"))
        );
        for issue in &self.issues {
            let names: Vec<&str> = issue.objects.iter().map(|o| o.name.as_str()).collect();
            let objects = if names.is_empty() {
                String::new()
            } else {
                format!(" [{}]", names.join(", "))
            };
            let fix = if issue.fix { " (fix available)" } else { "" };
            out.push_str(&format!(
                "{} {}{objects} {}{fix}\n",
                issue.severity, issue.code, issue.message
            ));
        }
        out.push_str(&format!(
            "{}, {}, {}\n",
            plural(self.errors, "error"),
            plural(self.warnings, "warning"),
            plural(self.infos, "note")
        ));
        out
    }
}

/// Prints the issues in the format asked for. Exit code 0 without errors, 4 with errors.
pub fn run(args: &[String]) -> Result<ExitCode, Fail> {
    let args = Args::split(args, &["--json"], &["--brief", "--target"])?;
    let model = args
        .word("model")?
        .ok_or_else(|| Fail::Usage("no model given".to_owned()))?;
    let want = args.option("--target").map(parse_target).transpose()?;
    let report = check(
        Path::new(model),
        args.option("--brief").map(Path::new),
        want,
    )?;
    let text = if args.flag("--json") {
        serde_json::to_string_pretty(&report).map_err(|err| Fail::Error(err.to_string()))?
    } else {
        for note in &report.notes {
            eprintln!("cuboid-cli: note: {note}");
        }
        report.text()
    };
    dump::print(&text, 0).map_err(|err| Fail::Error(format!("cannot write the output: {err}")))?;
    Ok(if report.errors > 0 {
        ExitCode::from(4)
    } else {
        ExitCode::SUCCESS
    })
}
