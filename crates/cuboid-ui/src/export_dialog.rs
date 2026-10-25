//! The export dialog: one window for every export. The user picks a format, a game version for
//! the Minecraft formats and a place; the app checks the model for that target on a worker and
//! the dialog shows what it found before anything is written.
//!
//! The dialog owns only the choices and the stage; the work is the app's. It asks through
//! [`ExportRequest`] and is told the answers with [`Ui::export_checked`](crate::Ui) and
//! [`Ui::export_finished`](crate::Ui).

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cuboid_ui_kit::metrics::{height, space};
use cuboid_ui_kit::{
    Button, Icon, Modal, ModalButton, PropertyGrid, Tokens, Type, dropdown, progress, text,
    text_field,
};
use egui::{Align, Context, Label, Layout, Ui};

use crate::common::Sink;
use crate::dialogs::{card, fixed_body, status_line};
use crate::request::UiRequest;
use crate::validator::IssueView;

/// Height of the body of the dialog, the same in every stage.
const BODY: f32 = height::ROW * 8.0;

/// The kinds of export.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum ExportFormat {
    /// A `.cubik` copy; the open document stays as it is.
    #[default]
    Cubik,
    /// A Minecraft Java model JSON with the textures beside it.
    MinecraftJson,
    /// A resource pack as a zip archive.
    ResourcePack,
    /// A resource pack as a folder.
    ResourcePackFolder,
    /// A Blockbench project.
    Bbmodel,
    /// Wavefront OBJ with its material.
    Obj,
    /// glTF with a buffer file.
    Gltf,
    /// Binary glTF.
    Glb,
    /// A MagicaVoxel model.
    Vox,
    /// A rendered picture; the render dialog takes over.
    Png,
}

impl ExportFormat {
    /// Every format in list order.
    pub const ALL: [ExportFormat; 10] = [
        ExportFormat::Cubik,
        ExportFormat::MinecraftJson,
        ExportFormat::ResourcePack,
        ExportFormat::ResourcePackFolder,
        ExportFormat::Bbmodel,
        ExportFormat::Obj,
        ExportFormat::Gltf,
        ExportFormat::Glb,
        ExportFormat::Vox,
        ExportFormat::Png,
    ];

    /// Name in the list.
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Cubik => "Cubik copy (.cubik)",
            ExportFormat::MinecraftJson => "Minecraft model (.json)",
            ExportFormat::ResourcePack => "Minecraft resource pack (.zip)",
            ExportFormat::ResourcePackFolder => "Minecraft resource pack (folder)",
            ExportFormat::Bbmodel => "Blockbench project (.bbmodel)",
            ExportFormat::Obj => "Wavefront OBJ (.obj)",
            ExportFormat::Gltf => "glTF (.gltf)",
            ExportFormat::Glb => "glTF binary (.glb)",
            ExportFormat::Vox => "MagicaVoxel (.vox)",
            ExportFormat::Png => "PNG picture (.png)",
        }
    }

    /// One line on what the file is.
    pub fn hint(self) -> &'static str {
        match self {
            ExportFormat::Cubik => "A copy that Cubik Studio opens. This document stays open.",
            ExportFormat::MinecraftJson => {
                "A block or item model for a game version; textures without a game path are saved beside it."
            }
            ExportFormat::ResourcePack => {
                "A pack that replaces a vanilla block or item with the model."
            }
            ExportFormat::ResourcePackFolder => "The same pack, written as a folder.",
            ExportFormat::Bbmodel => "Opens in Blockbench with elements, textures and UV.",
            ExportFormat::Obj => "Geometry and a material file, for other 3D tools.",
            ExportFormat::Gltf => "Geometry, a buffer file and the textures.",
            ExportFormat::Glb => "Geometry and textures in one file.",
            ExportFormat::Vox => "The voxel layer as a palette model.",
            ExportFormat::Png => "A picture of the model. The next step sets size and camera.",
        }
    }

    /// File extension without the dot; empty for a folder.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Cubik => "cubik",
            ExportFormat::MinecraftJson => "json",
            ExportFormat::ResourcePack => "zip",
            ExportFormat::ResourcePackFolder => "",
            ExportFormat::Bbmodel => "bbmodel",
            ExportFormat::Obj => "obj",
            ExportFormat::Gltf => "gltf",
            ExportFormat::Glb => "glb",
            ExportFormat::Vox => "vox",
            ExportFormat::Png => "png",
        }
    }

    /// The format depends on a game version.
    pub fn uses_version(self) -> bool {
        matches!(
            self,
            ExportFormat::MinecraftJson
                | ExportFormat::ResourcePack
                | ExportFormat::ResourcePackFolder
        )
    }

    /// The model is checked before this format is written.
    pub fn is_checked(self) -> bool {
        matches!(
            self,
            ExportFormat::Cubik
                | ExportFormat::MinecraftJson
                | ExportFormat::ResourcePack
                | ExportFormat::ResourcePackFolder
                | ExportFormat::Bbmodel
        )
    }
}

/// A game version the version table knows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionChoice {
    /// The version as text, such as `1.21.11`.
    pub id: String,
    /// The pack format as text, `None` when the table has none for this version.
    pub pack_format: Option<String>,
}

impl VersionChoice {
    /// The line in the picker; a version without a known pack format is marked.
    pub fn label(&self) -> String {
        match &self.pack_format {
            Some(format) => format!("{}  (pack format {format})", self.id),
            None => format!("{}  (no known pack format)", self.id),
        }
    }
}

/// What the app knows that the dialog needs; set once with [`Ui::set_export_info`](crate::Ui).
#[derive(Clone, Debug, Default)]
pub struct ExportInfo {
    /// The versions to offer, newest first.
    pub versions: Vec<VersionChoice>,
    /// The version the document's preset names; the dialog starts with it. Empty for none.
    pub default_version: String,
    /// The folder to start in when the document has no file.
    pub folder: PathBuf,
}

/// The choices that decide what is written and what it is checked against.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExportSettings {
    /// The kind of file.
    pub format: ExportFormat,
    /// The game version as text; ignored by formats without one.
    pub version: String,
}

/// What the dialog asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum ExportRequest {
    /// Check the model for the target of these settings.
    Check(ExportSettings),
    /// Apply the fixes of the error issues of the last check, then check again.
    FixErrors(ExportSettings),
    /// Write the file. `force` writes although the check found errors.
    Write {
        /// What to write.
        settings: ExportSettings,
        /// The file, or the folder of a pack folder.
        path: PathBuf,
        /// Ignore the errors of the check.
        force: bool,
    },
    /// The user left while a check ran.
    CancelCheck,
}

/// The findings of a check, as the dialog's summary shows them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportReport {
    /// What was checked, such as "Java block 1.21.11".
    pub target: String,
    /// Number of errors.
    pub errors: usize,
    /// Number of warnings.
    pub warnings: usize,
    /// How many errors the app can fix.
    pub fixable: usize,
    /// The errors, worst first; may be cut short.
    pub issues: Vec<IssueView>,
}

/// How an export ended.
#[derive(Clone, Debug, PartialEq)]
pub enum ExportOutcome {
    /// Written. `notices` say what the file does not hold.
    Done {
        /// The file or folder.
        path: PathBuf,
        /// What the exporter left out or changed.
        notices: Vec<String>,
    },
    /// Not written; the text is for the user.
    Failed(String),
    /// The user chose not to replace existing files: back to the settings.
    Declined,
}

/// Where the dialog is.
#[derive(Clone, Debug, PartialEq)]
pub enum ExportStage {
    /// The user chooses.
    Setup,
    /// The app checks the model.
    Checking,
    /// The check found errors: the user decides.
    Review(ExportReport),
    /// The app writes the file.
    Writing,
    /// The file is written.
    Done {
        /// The file or folder.
        path: PathBuf,
        /// What the exporter left out or changed.
        notices: Vec<String>,
    },
    /// The export failed.
    Failed(String),
}

/// How long the path rests before the dialog asks whether the file exists.
const EXISTS_DEBOUNCE: Duration = Duration::from_millis(300);

/// Folder and name fields, shared by the export and the render dialog.
pub(crate) struct Location {
    pub folder: String,
    pub name: String,
    /// The last answer of the existence check.
    seen: Option<(PathBuf, bool)>,
    /// The path the text names now and since when; the check starts when it rests.
    resting: Option<(PathBuf, Instant)>,
    /// The check on its thread. A network path can take seconds to answer, so the frame never
    /// waits for it.
    asking: Option<(PathBuf, mpsc::Receiver<bool>)>,
}

impl Location {
    pub fn new(folder: &Path, name: &str) -> Self {
        Self {
            folder: folder.display().to_string(),
            name: name.to_owned(),
            seen: None,
            resting: None,
            asking: None,
        }
    }

    /// The target path for an extension (no extension for a folder).
    pub fn path(&self, extension: &str) -> PathBuf {
        let name = match self.name.trim() {
            "" => "model",
            name => name,
        };
        let file = if extension.is_empty() || ends_with_ci(name, extension) {
            name.to_owned()
        } else {
            format!("{name}.{extension}")
        };
        Path::new(self.folder.trim()).join(file)
    }

    pub fn is_complete(&self) -> bool {
        !self.folder.trim().is_empty() && !self.name.trim().is_empty()
    }

    /// Draws the folder and name rows and a warning under them when the file exists.
    pub fn show(&mut self, ui: &mut Ui, extension: &str, sink: &mut Sink) {
        let tokens = Tokens::of(ui.ctx());
        PropertyGrid::new("cuboid_location").show(ui, |grid| {
            grid.row("Folder", |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(Button::new("Browse...")).clicked() {
                        sink.push(UiRequest::BrowseFolder);
                    }
                    text_field(ui, "location_folder", &mut self.folder, "Folder");
                });
            });
            grid.row("Name", |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !extension.is_empty() {
                        ui.label(text(Type::Label, format!(".{extension}")));
                    }
                    text_field(ui, "location_name", &mut self.name, "Name of the file");
                });
            });
        });
        if !self.is_complete() {
            return;
        }
        let path = self.path(extension);
        self.check_exists(ui.ctx(), &path);
        if self
            .seen
            .as_ref()
            .is_some_and(|(seen, exists)| *seen == path && *exists)
        {
            let warning = text(Type::Caption, "This will replace the existing file.");
            ui.add(Label::new(warning.color(tokens.warn)).wrap());
        }
    }

    /// Keeps `seen` up to date for `path`: one check per path that rests for a moment, on a
    /// thread, one at a time.
    fn check_exists(&mut self, ctx: &Context, path: &Path) {
        if let Some((asked, reply)) = &self.asking {
            match reply.try_recv() {
                Ok(exists) => self.seen = Some((asked.clone(), exists)),
                Err(mpsc::TryRecvError::Disconnected) => self.seen = Some((asked.clone(), false)),
                // The answer wakes the frame.
                Err(mpsc::TryRecvError::Empty) => return,
            }
            self.asking = None;
        }
        if self.seen.as_ref().is_some_and(|(seen, _)| seen == path) {
            return;
        }
        let since = match &self.resting {
            Some((resting, since)) if resting == path => *since,
            _ => {
                let now = Instant::now();
                self.resting = Some((path.to_owned(), now));
                now
            }
        };
        if let Some(wait) = EXISTS_DEBOUNCE.checked_sub(since.elapsed()) {
            ctx.request_repaint_after(wait);
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (target, wake) = (path.to_owned(), ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("cuboid-exists".to_owned())
            .spawn(move || {
                let _ = tx.send(target.exists());
                wake.request_repaint();
            });
        match spawned {
            Ok(_) => self.asking = Some((path.to_owned(), rx)),
            Err(_) => self.seen = Some((path.to_owned(), false)),
        }
    }
}

fn ends_with_ci(name: &str, extension: &str) -> bool {
    let dot = name.len().checked_sub(extension.len() + 1);
    dot.is_some_and(|at| {
        name.is_char_boundary(at)
            && name[at..].starts_with('.')
            && name[at + 1..].eq_ignore_ascii_case(extension)
    })
}

/// What the dialog wants after a frame.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ExportEvent {
    None,
    Close,
    /// Continue in the render dialog.
    ToRender,
}

/// What a footer button, Enter or Esc asks for.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Act {
    /// Start the export, or go on to the render dialog.
    Go,
    /// Apply the fixes of the review, then check again.
    Fix,
    /// Write although the check found errors.
    Anyway,
    /// Back from a failure to the choices.
    Back,
    /// Show the written file in the file manager.
    OpenFolder,
    /// Leave, and stop the job that runs.
    Close,
}

pub(crate) struct ExportDialog {
    pub settings: ExportSettings,
    pub location: Location,
    pub stage: ExportStage,
    /// A report from the app that the next frame turns into a stage.
    report: Option<ExportReport>,
}

impl ExportDialog {
    pub fn new(info: &ExportInfo, folder: &Path, name: &str, last: &ExportSettings) -> Self {
        let mut settings = last.clone();
        let known = |id: &str| info.versions.iter().any(|v| v.id == id);
        if known(&info.default_version) {
            settings.version.clone_from(&info.default_version);
        } else if !known(&settings.version) {
            settings.version = info
                .versions
                .first()
                .map(|v| v.id.clone())
                .unwrap_or_default();
        }
        Self {
            settings,
            location: Location::new(folder, name),
            stage: ExportStage::Setup,
            report: None,
        }
    }

    pub fn path(&self) -> PathBuf {
        self.location.path(self.settings.format.extension())
    }

    /// The app answered a check.
    pub fn checked(&mut self, report: ExportReport) {
        if self.stage == ExportStage::Checking {
            self.report = Some(report);
        }
    }

    /// The app finished the export.
    pub fn finished(&mut self, outcome: ExportOutcome) {
        self.stage = match outcome {
            ExportOutcome::Done { path, notices } => ExportStage::Done { path, notices },
            ExportOutcome::Failed(text) => ExportStage::Failed(text),
            ExportOutcome::Declined => ExportStage::Setup,
        };
    }

    fn write(&mut self, force: bool, sink: &mut Sink) {
        self.stage = ExportStage::Writing;
        sink.push(UiRequest::Export(ExportRequest::Write {
            settings: self.settings.clone(),
            path: self.path(),
            force,
        }));
    }

    /// The answer of a check becomes the next stage.
    fn take_report(&mut self, sink: &mut Sink) {
        let Some(report) = self.report.take() else {
            return;
        };
        if report.errors == 0 {
            self.write(false, sink);
        } else {
            self.stage = ExportStage::Review(report);
        }
    }

    fn begin(&mut self, sink: &mut Sink) -> ExportEvent {
        if self.settings.format == ExportFormat::Png {
            return ExportEvent::ToRender;
        }
        if self.settings.format.is_checked() {
            self.stage = ExportStage::Checking;
            sink.push(UiRequest::Export(ExportRequest::Check(
                self.settings.clone(),
            )));
        } else {
            self.write(false, sink);
        }
        ExportEvent::None
    }

    /// The footer of the stage, left to right, primary last. Enter takes the last button unless
    /// the stage has no button that is safe to press unasked: a job that runs and a review that
    /// can only export anyway.
    fn footer<'a>(&self, ready: bool, fix: &'a str) -> (Vec<(Act, ModalButton<'a>)>, bool) {
        let cancel = (Act::Close, ModalButton::new("Cancel"));
        match &self.stage {
            ExportStage::Setup => {
                let label = if self.settings.format == ExportFormat::Png {
                    "Continue"
                } else {
                    "Export"
                };
                let go = ModalButton::primary(label).enabled(ready);
                (vec![cancel, (Act::Go, go)], true)
            }
            ExportStage::Checking | ExportStage::Writing => (vec![cancel], false),
            ExportStage::Review(report) => {
                let mut buttons = vec![cancel, (Act::Anyway, ModalButton::danger("Export anyway"))];
                if report.fixable > 0 {
                    buttons.push((Act::Fix, ModalButton::primary(fix)));
                }
                (buttons, report.fixable > 0)
            }
            ExportStage::Done { .. } => (
                vec![
                    (Act::OpenFolder, ModalButton::new("Open folder")),
                    (Act::Close, ModalButton::primary("Close")),
                ],
                true,
            ),
            ExportStage::Failed(_) => (
                vec![
                    (Act::Back, ModalButton::new("Back")),
                    (Act::Close, ModalButton::primary("Close")),
                ],
                true,
            ),
        }
    }

    pub fn show(&mut self, ctx: &Context, info: &ExportInfo, sink: &mut Sink) -> ExportEvent {
        self.take_report(sink);
        let tokens = Tokens::of(ctx);
        let ready = self.settings.format == ExportFormat::Png || self.location.is_complete();
        let fix = match &self.stage {
            ExportStage::Review(report) => format!("Fix {}", report.fixable),
            _ => String::new(),
        };
        let (acts, enter) = self.footer(ready, &fix);
        let buttons: Vec<ModalButton> = acts.iter().map(|(_, button)| *button).collect();
        let last = acts.last().map(|(act, _)| *act);
        let acts: Vec<Act> = acts.into_iter().map(|(act, _)| act).collect();
        let stage = self.stage.clone();
        let outcome = Modal::new("export_dialog", "Export")
            .width(520.0)
            .buttons(&buttons)
            .show(ctx, |ui| {
                fixed_body(ui, "export_body", BODY, |ui| match &stage {
                    ExportStage::Setup => self.setup(ui, info, sink),
                    ExportStage::Checking => working(ui, "Checking the model...".to_owned()),
                    ExportStage::Writing => {
                        working(ui, format!("Writing {}...", self.path().display()));
                    }
                    ExportStage::Review(report) => review(ui, report, &tokens),
                    ExportStage::Done { path, notices } => done(ui, path, notices, &tokens),
                    ExportStage::Failed(text) => {
                        status_line(ui, Icon::Error, tokens.danger, text.clone());
                    }
                });
            });
        let act = match outcome.button {
            Some(index) => acts.get(index).copied(),
            None if outcome.confirmed && enter => last,
            None if outcome.cancelled => Some(Act::Close),
            None => None,
        };
        let mut event = ExportEvent::None;
        match act {
            Some(Act::Go) => event = self.begin(sink),
            Some(Act::Fix) => {
                self.stage = ExportStage::Checking;
                sink.push(UiRequest::Export(ExportRequest::FixErrors(
                    self.settings.clone(),
                )));
            }
            Some(Act::Anyway) => self.write(true, sink),
            Some(Act::Back) => self.stage = ExportStage::Setup,
            Some(Act::OpenFolder) => {
                if let ExportStage::Done { path, .. } = &stage {
                    sink.push(UiRequest::OpenFolder(path.clone()));
                }
            }
            Some(Act::Close) => {
                match self.stage {
                    ExportStage::Checking => {
                        sink.push(UiRequest::Export(ExportRequest::CancelCheck));
                    }
                    ExportStage::Writing => sink.push(UiRequest::CancelJob(1)),
                    _ => {}
                }
                event = ExportEvent::Close;
            }
            None => {}
        }
        event
    }

    /// The choices: format, game version, and where the file goes.
    fn setup(&mut self, ui: &mut Ui, info: &ExportInfo, sink: &mut Sink) {
        PropertyGrid::new("export_setup").show(ui, |grid| {
            grid.row("Format", |ui| {
                let labels = ExportFormat::ALL.map(ExportFormat::label);
                let mut at = ExportFormat::ALL
                    .iter()
                    .position(|f| *f == self.settings.format)
                    .unwrap_or(0);
                if dropdown(ui, "export_format", &mut at, &labels).changed() {
                    self.settings.format = ExportFormat::ALL[at.min(ExportFormat::ALL.len() - 1)];
                }
            });
            if self.settings.format.uses_version() {
                grid.row("Game version", |ui| {
                    let labels: Vec<String> =
                        info.versions.iter().map(VersionChoice::label).collect();
                    let options: Vec<&str> = labels.iter().map(String::as_str).collect();
                    let mut at = info
                        .versions
                        .iter()
                        .position(|v| v.id == self.settings.version)
                        .unwrap_or(0);
                    if dropdown(ui, "export_version", &mut at, &options).changed()
                        && let Some(version) = info.versions.get(at)
                    {
                        self.settings.version.clone_from(&version.id);
                    }
                });
            }
        });
        ui.add_space(space::SM);
        ui.add(Label::new(text(Type::Caption, self.settings.format.hint())).wrap());
        if self.settings.format != ExportFormat::Png {
            ui.add_space(space::MD);
            self.location
                .show(ui, self.settings.format.extension(), sink);
        }
    }
}

/// A job that runs: what it does, and a bar of unknown length.
pub(crate) fn working(ui: &mut Ui, message: String) {
    ui.add(Label::new(text(Type::Body, message)).wrap());
    ui.add_space(space::SM);
    progress(ui, None, ui.available_width());
}

/// The findings of a check: what was found, the warning about exporting anyway and the issues.
fn review(ui: &mut Ui, report: &ExportReport, tokens: &Tokens) {
    let warnings = if report.warnings == 0 {
        String::new()
    } else {
        format!(
            " and {} warning{}",
            report.warnings,
            plural(report.warnings)
        )
    };
    let summary = format!(
        "{} error{}{} for {}",
        report.errors,
        plural(report.errors),
        warnings,
        report.target
    );
    status_line(ui, Icon::Error, tokens.danger, summary);
    ui.add(
        Label::new(text(
            Type::Caption,
            "Exporting anyway writes the file as it is; the game may reject or change it.",
        ))
        .wrap(),
    );
    ui.add_space(space::SM);
    ui.spacing_mut().item_spacing.y = space::XS;
    for issue in &report.issues {
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(Label::new(text(Type::Caption, &*issue.code)).truncate());
            ui.add(Label::new(text(Type::Body, &*issue.message)).wrap());
        });
    }
    let more = report.errors.saturating_sub(report.issues.len());
    if more > 0 {
        ui.label(text(Type::Caption, format!("and {more} more")));
    }
}

/// The written file and what the exporter left out.
fn done(ui: &mut Ui, path: &Path, notices: &[String], tokens: &Tokens) {
    status_line(
        ui,
        Icon::CheckCircle,
        tokens.ok,
        format!("Exported {}", path.display()),
    );
    ui.add_space(space::SM);
    if notices.is_empty() {
        ui.label(text(Type::Label, "The file holds everything in the model."));
        return;
    }
    ui.label(text(
        Type::Body,
        format!(
            "{} notice{} from the exporter:",
            notices.len(),
            plural(notices.len())
        ),
    ));
    for notice in notices {
        ui.add(Label::new(text(Type::Body, format!("- {notice}"))).wrap());
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use egui::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    fn versions() -> Vec<VersionChoice> {
        vec![
            VersionChoice {
                id: "26.4".into(),
                pack_format: None,
            },
            VersionChoice {
                id: "1.21.11".into(),
                pack_format: Some("75".into()),
            },
        ]
    }

    fn info() -> ExportInfo {
        ExportInfo {
            versions: versions(),
            default_version: String::new(),
            folder: PathBuf::from("out"),
        }
    }

    fn dialog() -> ExportDialog {
        ExportDialog::new(
            &info(),
            Path::new("out"),
            "chair",
            &ExportSettings::default(),
        )
    }

    #[test]
    fn a_version_without_a_pack_format_is_marked() {
        let v = versions();
        assert!(v[0].label().contains("no known pack format"));
        assert!(v[1].label().contains("pack format 75"));
    }

    #[test]
    fn the_path_takes_the_extension_of_the_format() {
        let mut location = Location::new(Path::new("out"), "chair");
        assert_eq!(location.path("json"), Path::new("out").join("chair.json"));
        location.name = "chair.JSON".into();
        assert_eq!(location.path("json"), Path::new("out").join("chair.JSON"));
        assert_eq!(location.path(""), Path::new("out").join("chair.JSON"));
        location.name = "  ".into();
        assert!(!location.is_complete());
        assert_eq!(location.path("obj"), Path::new("out").join("model.obj"));
    }

    #[test]
    fn the_first_known_version_is_the_default() {
        let d = dialog();
        assert_eq!(d.settings.version, "26.4");
        let mut last = ExportSettings {
            version: "1.21.11".into(),
            ..ExportSettings::default()
        };
        let d = ExportDialog::new(&info(), Path::new("out"), "a", &last);
        assert_eq!(d.settings.version, "1.21.11");
        last.version = "0.0".into();
        let d = ExportDialog::new(&info(), Path::new("out"), "a", &last);
        assert_eq!(d.settings.version, "26.4");
        // The version of the document's preset wins over the last choice.
        let mut preset = info();
        preset.default_version = "1.21.11".into();
        last.version = "26.4".into();
        let d = ExportDialog::new(&preset, Path::new("out"), "a", &last);
        assert_eq!(d.settings.version, "1.21.11");
    }

    #[test]
    fn a_checked_format_asks_for_a_check_and_a_clean_report_writes() {
        let mut d = dialog();
        d.settings.format = ExportFormat::MinecraftJson;
        let mut sink = Sink::default();
        assert_eq!(d.begin(&mut sink), ExportEvent::None);
        assert_eq!(d.stage, ExportStage::Checking);
        assert!(matches!(
            sink.requests.as_slice(),
            [UiRequest::Export(ExportRequest::Check(_))]
        ));
        d.checked(ExportReport {
            target: "Java block 26.4".into(),
            ..ExportReport::default()
        });
        let mut sink = Sink::default();
        d.take_report(&mut sink);
        assert_eq!(d.stage, ExportStage::Writing);
        assert!(matches!(
            sink.requests.as_slice(),
            [UiRequest::Export(ExportRequest::Write { force: false, .. })]
        ));
    }

    #[test]
    fn errors_stop_at_the_review_and_export_anyway_forces() {
        let mut d = dialog();
        d.settings.format = ExportFormat::ResourcePack;
        let mut sink = Sink::default();
        d.begin(&mut sink);
        d.checked(ExportReport {
            target: "Resource pack 26.4".into(),
            errors: 2,
            fixable: 1,
            ..ExportReport::default()
        });
        let mut sink = Sink::default();
        d.take_report(&mut sink);
        assert!(matches!(d.stage, ExportStage::Review(_)));
        assert!(sink.requests.is_empty());
        d.write(true, &mut sink);
        assert!(matches!(
            sink.requests.as_slice(),
            [UiRequest::Export(ExportRequest::Write { force: true, .. })]
        ));
    }

    #[test]
    fn an_answer_that_nobody_waits_for_is_ignored() {
        let mut d = dialog();
        d.checked(ExportReport {
            errors: 1,
            ..ExportReport::default()
        });
        let mut sink = Sink::default();
        d.take_report(&mut sink);
        assert_eq!(d.stage, ExportStage::Setup);
    }

    #[test]
    fn unchecked_formats_write_at_once_and_png_hands_over() {
        let mut d = dialog();
        d.settings.format = ExportFormat::Obj;
        let mut sink = Sink::default();
        d.begin(&mut sink);
        assert_eq!(d.stage, ExportStage::Writing);
        let mut d = dialog();
        d.settings.format = ExportFormat::Png;
        assert_eq!(d.begin(&mut sink), ExportEvent::ToRender);
        assert_eq!(d.stage, ExportStage::Setup);
    }

    #[test]
    fn the_result_keeps_the_notices() {
        let mut d = dialog();
        d.stage = ExportStage::Writing;
        d.finished(ExportOutcome::Done {
            path: PathBuf::from("out/chair.json"),
            notices: vec!["skipped 2 overrides".into()],
        });
        assert!(matches!(&d.stage, ExportStage::Done { notices, .. } if notices.len() == 1));
        d.finished(ExportOutcome::Failed("disk full".into()));
        assert_eq!(d.stage, ExportStage::Failed("disk full".into()));
    }

    #[test]
    fn the_dialog_keeps_its_size_in_every_stage() {
        struct Open {
            dialog: ExportDialog,
            info: ExportInfo,
        }
        let state = Open {
            dialog: dialog(),
            info: info(),
        };
        let mut harness = Harness::builder()
            .with_context_setup(crate::dialogs::prepare)
            .build_ui_state(
                |ui, state: &mut Open| {
                    let mut sink = Sink::default();
                    state.dialog.show(ui.ctx(), &state.info, &mut sink);
                },
                state,
            );
        harness.run_steps(8);
        let footer = |harness: &Harness<'static, Open>| {
            harness
                .get_by_role_and_label(Role::Button, "Cancel")
                .rect()
                .top()
        };
        let first = footer(&harness);
        let issue = |n: usize| IssueView {
            code: format!("GEO_{n}").into(),
            level: crate::validator::IssueLevel::Error,
            objects: Vec::new(),
            message: "A message that is long enough to wrap onto a second line when it must."
                .repeat(2)
                .into(),
            hint: "".into(),
            fixable: false,
        };
        let stages = [
            ExportStage::Checking,
            ExportStage::Review(ExportReport {
                target: "Java block 26.4".into(),
                errors: 40,
                issues: (0..40).map(issue).collect(),
                ..ExportReport::default()
            }),
            ExportStage::Done {
                path: PathBuf::from("out/chair.json"),
                notices: (0..30).map(|n| format!("notice number {n}")).collect(),
            },
            ExportStage::Failed("disk full ".repeat(40)),
            ExportStage::Setup,
        ];
        for stage in stages {
            let name = format!("{stage:?}");
            harness.state_mut().dialog.stage = stage;
            harness.run_steps(6);
            let label = if name.starts_with("Done") || name.starts_with("Failed") {
                "Close"
            } else {
                "Cancel"
            };
            let top = harness
                .get_by_role_and_label(Role::Button, label)
                .rect()
                .top();
            assert!((top - first).abs() < 0.5, "{}", &name[..name.len().min(20)]);
        }
    }
}
