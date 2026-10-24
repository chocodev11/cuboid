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

use cuboid_ui_kit::{Icon, RADIUS, Tokens};
use egui::{Context, Id, Modal, RichText, Stroke, Ui, vec2};

use crate::common::Sink;
use crate::request::UiRequest;
use crate::validator::IssueView;

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

    /// Draws the two fields and the browse button; true when a text changed.
    pub fn show(&mut self, ui: &mut Ui, extension: &str, sink: &mut Sink) {
        let tokens = Tokens::of(ui.ctx());
        egui::Grid::new("cuboid_location")
            .num_columns(2)
            .spacing(vec2(8.0, 6.0))
            .show(ui, |ui| {
                ui.label("Folder");
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 90.0).max(120.0);
                    ui.add(egui::TextEdit::singleline(&mut self.folder).desired_width(width));
                    if ui.button("Browse...").clicked() {
                        sink.push(UiRequest::BrowseFolder);
                    }
                });
                ui.end_row();
                ui.label("Name");
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 70.0).max(120.0);
                    ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(width));
                    if !extension.is_empty() {
                        ui.label(RichText::new(format!(".{extension}")).color(tokens.text_weak));
                    }
                });
                ui.end_row();
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
            ui.label(RichText::new("This will replace the existing file.").color(tokens.warn));
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

/// Whether the user asked to leave a modal: a click on the backdrop, or Escape, while no popup
/// is open. Unlike `ModalResponse::should_close` it ignores the `close` of a popup inside the
/// modal and the click that chooses an entry of a combo box, so picking an entry leaves the
/// dialog open.
pub(crate) fn modal_dismissed<T>(ctx: &Context, modal: &egui::ModalResponse<T>) -> bool {
    if modal.any_popup_open {
        return false;
    }
    modal.backdrop_response.clicked()
        || (modal.is_top_modal
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)))
}

/// What the dialog wants after a frame.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ExportEvent {
    None,
    Close,
    /// Continue in the render dialog.
    ToRender,
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

    pub fn show(&mut self, ctx: &Context, info: &ExportInfo, sink: &mut Sink) -> ExportEvent {
        self.take_report(sink);
        let tokens = Tokens::of(ctx);
        let mut event = ExportEvent::None;
        let modal = Modal::new(Id::unique("cuboid_export_dialog")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::hover());
                Icon::Export.paint(ui.painter(), rect, tokens.accent);
                ui.label(
                    RichText::new("Export")
                        .size(16.0)
                        .strong()
                        .color(tokens.text_strong),
                );
            });
            ui.add_space(8.0);
            match self.stage.clone() {
                ExportStage::Setup => event = self.setup(ui, info, &tokens, sink),
                ExportStage::Checking => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label("Checking the model...");
                    });
                    ui.add_space(12.0);
                    if ui.button("Cancel").clicked() {
                        sink.push(UiRequest::Export(ExportRequest::CancelCheck));
                        event = ExportEvent::Close;
                    }
                }
                ExportStage::Review(report) => event = self.review(ui, &report, &tokens, sink),
                ExportStage::Writing => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label(format!("Writing {}...", self.path().display()));
                    });
                    ui.add_space(12.0);
                    if ui.button("Cancel").clicked() {
                        sink.push(UiRequest::CancelJob(1));
                        event = ExportEvent::Close;
                    }
                }
                ExportStage::Done { path, notices } => {
                    event = Self::done(ui, &path, &notices, &tokens, sink);
                }
                ExportStage::Failed(text) => {
                    ui.label(RichText::new(text).color(tokens.danger));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("Back").clicked() {
                            self.stage = ExportStage::Setup;
                        }
                        if ui.button("Close").clicked() {
                            event = ExportEvent::Close;
                        }
                    });
                }
            }
        });
        if event == ExportEvent::None && modal_dismissed(ctx, &modal) {
            match self.stage {
                ExportStage::Checking => sink.push(UiRequest::Export(ExportRequest::CancelCheck)),
                ExportStage::Writing => sink.push(UiRequest::CancelJob(1)),
                _ => {}
            }
            event = ExportEvent::Close;
        }
        event
    }

    fn setup(
        &mut self,
        ui: &mut Ui,
        info: &ExportInfo,
        tokens: &Tokens,
        sink: &mut Sink,
    ) -> ExportEvent {
        let mut event = ExportEvent::None;
        // Plain buttons, not combo boxes: a popup inside a modal loses its clicks to the backdrop.
        ui.label("Format");
        egui::Grid::new("cuboid_export_formats")
            .num_columns(2)
            .spacing(vec2(6.0, 4.0))
            .show(ui, |ui| {
                for (i, format) in ExportFormat::ALL.into_iter().enumerate() {
                    let shown = self.settings.format == format;
                    if ui
                        .add_sized(
                            vec2(228.0, 24.0),
                            egui::Button::selectable(shown, format.label()),
                        )
                        .clicked()
                    {
                        self.settings.format = format;
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        if self.settings.format.uses_version() {
            ui.add_space(6.0);
            ui.label("Game version");
            egui::ScrollArea::vertical()
                .id_salt("cuboid_export_versions")
                .max_height(104.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for version in &info.versions {
                        let shown = self.settings.version == version.id;
                        if ui
                            .add_sized(
                                vec2(ui.available_width(), 22.0),
                                egui::Button::selectable(shown, version.label()),
                            )
                            .clicked()
                        {
                            self.settings.version.clone_from(&version.id);
                        }
                    }
                });
        }
        ui.add_space(6.0);
        ui.label(RichText::new(self.settings.format.hint()).color(tokens.text_weak));
        ui.add_space(8.0);
        if self.settings.format != ExportFormat::Png {
            self.location
                .show(ui, self.settings.format.extension(), sink);
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let ready = self.settings.format == ExportFormat::Png || self.location.is_complete();
            let label = if self.settings.format == ExportFormat::Png {
                "Continue"
            } else {
                "Export"
            };
            let go = ui.add_enabled(
                ready,
                egui::Button::new(RichText::new(label).color(tokens.on_accent))
                    .fill(tokens.accent)
                    .stroke(Stroke::NONE)
                    .min_size(vec2(96.0, 28.0)),
            );
            // Enter presses the button while no text field is being edited.
            let enter = ui.memory(|m| m.focused().is_none())
                && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if go.clicked() || (ready && enter) {
                event = self.begin(sink);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(vec2(80.0, 28.0)))
                .clicked()
            {
                event = ExportEvent::Close;
            }
        });
        event
    }

    fn review(
        &mut self,
        ui: &mut Ui,
        report: &ExportReport,
        tokens: &Tokens,
        sink: &mut Sink,
    ) -> ExportEvent {
        let mut event = ExportEvent::None;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
            Icon::Error.paint(ui.painter(), rect, tokens.danger);
            let warnings = if report.warnings == 0 {
                String::new()
            } else {
                format!(
                    " and {} warning{}",
                    report.warnings,
                    plural(report.warnings)
                )
            };
            ui.label(
                RichText::new(format!(
                    "{} error{}{} for {}",
                    report.errors,
                    plural(report.errors),
                    warnings,
                    report.target
                ))
                .strong()
                .color(tokens.text_strong),
            );
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .max_height(220.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for issue in &report.issues {
                    egui::Frame::new()
                        .fill(tokens.bg_panel)
                        .stroke(Stroke::new(1.0, tokens.border))
                        .corner_radius(RADIUS)
                        .inner_margin(egui::Margin::symmetric(8, 5))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new(&*issue.code)
                                    .font(egui::FontId::monospace(11.5))
                                    .color(tokens.text_weak),
                            );
                            ui.label(&*issue.message);
                        });
                    ui.add_space(2.0);
                }
                let more = report.errors.saturating_sub(report.issues.len());
                if more > 0 {
                    ui.label(RichText::new(format!("and {more} more")).color(tokens.text_weak));
                }
            });
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Exporting anyway writes the file as it is; the game may reject or change it.",
            )
            .color(tokens.text_weak),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if report.fixable > 0
                && ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("Fix {}", report.fixable))
                                .color(tokens.on_accent),
                        )
                        .fill(tokens.accent)
                        .stroke(Stroke::NONE)
                        .min_size(vec2(80.0, 28.0)),
                    )
                    .clicked()
            {
                self.stage = ExportStage::Checking;
                sink.push(UiRequest::Export(ExportRequest::FixErrors(
                    self.settings.clone(),
                )));
            }
            if ui
                .add(
                    egui::Button::new(RichText::new("Export anyway").color(tokens.danger))
                        .min_size(vec2(112.0, 28.0)),
                )
                .clicked()
            {
                self.write(true, sink);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(vec2(80.0, 28.0)))
                .clicked()
            {
                event = ExportEvent::Close;
            }
        });
        event
    }

    fn done(
        ui: &mut Ui,
        path: &Path,
        notices: &[String],
        tokens: &Tokens,
        sink: &mut Sink,
    ) -> ExportEvent {
        let mut event = ExportEvent::None;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
            Icon::Check.paint(ui.painter(), rect, tokens.ok);
            ui.label(
                RichText::new(format!("Exported {}", path.display()))
                    .strong()
                    .color(tokens.text_strong),
            );
        });
        ui.add_space(6.0);
        if notices.is_empty() {
            ui.label(
                RichText::new("The file holds everything in the model.").color(tokens.text_weak),
            );
        } else {
            ui.label(format!(
                "{} notice{} from the exporter:",
                notices.len(),
                plural(notices.len())
            ));
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for notice in notices {
                        ui.label(RichText::new(format!("- {notice}")).color(tokens.text));
                    }
                });
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new("Open folder").min_size(vec2(96.0, 28.0)))
                .clicked()
            {
                sink.push(UiRequest::OpenFolder(path.to_path_buf()));
            }
            if ui
                .add(egui::Button::new("Close").min_size(vec2(80.0, 28.0)))
                .clicked()
            {
                event = ExportEvent::Close;
            }
        });
        event
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
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
}
