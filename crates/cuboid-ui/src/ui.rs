//! The window: ties the panels together and turns user input into requests.

use std::path::{Path, PathBuf};

use cuboid_actions::ActionId;
use cuboid_ui_kit::metrics::{height, radius};
use cuboid_ui_kit::{FontSet, Level, Toasts, Tokens, Type, install_fonts};
use egui::{Context, Frame, Panel};

use crate::animations::AnimationsPanel;
use crate::commands::PaletteState;
use crate::common::{ImageCache, Sink};
use crate::dialogs::{Recovery, RecoveryInfo, Unsaved, UnsavedAnswer};
use crate::export_dialog::{
    ExportDialog, ExportEvent, ExportFormat, ExportInfo, ExportOutcome, ExportReport,
    ExportSettings, ExportStage,
};
use crate::keymap::{Chord, Command, Keymap, UiCommand, pressed_chords};
use crate::layout::{Layout, PanelId, Zone};
use crate::library::{self, LibraryPanel};
use crate::model::{DocView, UiCx};
use crate::neutral;
use crate::outliner::OutlinerState;
use crate::prefs::{ColorMode, NavPreset, Prefs, TIMELINE_MIN};
use crate::prefs_ui::PrefsWindow;
use crate::project_settings::{ProjectView, ProjectWindow};
use crate::properties::PropsState;
use crate::props_animate::AnimateTab;
use crate::props_area::PropsTab;
use crate::render_dialog::{ActionToast, RenderDialog, RenderEvent, RenderSettings};
use crate::request::{
    AnimateEdit, AnimationRequest, Edit, Mode, Tool, UiOutput, UiRequest, ViewPreset, ViewportEvent,
};
use crate::start::{StartEvent, StartState};
use crate::statusbar::{self, BarState};
use crate::timeline::TimelineState;
use crate::toolbar::{self, ToolbarEvent};
use crate::topbar::{self, MenuEvent};
use crate::uv::UvState;
use crate::validator::{ValidatorState, ValidatorView};
use crate::viewport::ViewportState;

mod dock;

/// The share of the window height that the timeline may take at most.
const TIMELINE_SHARE: f32 = 0.6;

/// The whole interface. Create once with [`Ui::new`], call [`Ui::frame`] every frame.
pub struct Ui {
    prefs: Prefs,
    keymap: Keymap,
    mode: Mode,
    tool: Tool,
    fonts: Option<FontSet>,
    toasts: Toasts,
    palette: PaletteState,
    outliner: OutlinerState,
    props: PropsState,
    uv: UvState,
    viewport: ViewportState,
    start: StartState,
    library: LibraryPanel,
    start_forced: bool,
    /// The first-run panel (camera and accent) is shown on the start screen.
    first_run: bool,
    /// The neutral highlight is installed in place of the accent.
    neutral_on: bool,
    prefs_win: PrefsWindow,
    /// The settings of the open document, described by the app each frame.
    project: ProjectView,
    project_win: ProjectWindow,
    recovery: Option<Recovery>,
    unsaved: Option<Unsaved>,
    images: ImageCache,
    title: String,
    doc_name: String,
    doc_dirty: bool,
    prefs_dirty: bool,
    size_dirty: bool,
    layout_generation: u32,
    /// The tab of the properties area that is shown.
    props_tab: PropsTab,
    /// The pointer was over a viewport in the last frame.
    view_hovered: bool,
    export: Option<ExportDialog>,
    export_info: ExportInfo,
    export_last: ExportSettings,
    render: Option<RenderDialog>,
    render_last: RenderSettings,
    /// The folder of the last export or render.
    last_folder: Option<PathBuf>,
    validator: ValidatorState,
    /// The result of the last check, copied from the frame's context for the panel tab.
    validator_view: Option<ValidatorView>,
    action_toast: Option<ActionToast>,
    /// The timeline of the Animate workspace: zoom, folds and the selected keyframes.
    timeline: TimelineState,
    animate_tab: AnimateTab,
    animations_panel: AnimationsPanel,
}

impl Ui {
    /// A UI with the stored preferences. `fonts` are installed on the first frame.
    pub fn new(prefs: Prefs, fonts: FontSet) -> Ui {
        let mut prefs = prefs;
        prefs.sanitize();
        let mode = prefs.mode;
        let prefs_nav_chosen = prefs.nav_chosen;
        // The accent is optional: the question is answered with the app the user knows.
        let first_run = !prefs.nav_chosen;
        Ui {
            keymap: Keymap::with_overrides(prefs.keymap, &prefs.key_overrides),
            tool: default_tool(mode),
            mode,
            prefs,
            fonts: Some(fonts),
            toasts: Toasts::default(),
            palette: PaletteState::default(),
            outliner: OutlinerState::default(),
            props: PropsState::default(),
            uv: UvState::default(),
            viewport: ViewportState::default(),
            start: StartState::default(),
            library: LibraryPanel::default(),
            start_forced: !prefs_nav_chosen,
            first_run,
            neutral_on: false,
            prefs_win: PrefsWindow::default(),
            project: ProjectView::default(),
            project_win: ProjectWindow::default(),
            recovery: None,
            unsaved: None,
            images: ImageCache::default(),
            title: String::new(),
            doc_name: String::new(),
            doc_dirty: false,
            prefs_dirty: false,
            size_dirty: false,
            layout_generation: 0,
            props_tab: PropsTab::for_mode(mode),
            view_hovered: false,
            export: None,
            export_info: ExportInfo::default(),
            export_last: ExportSettings::default(),
            render: None,
            render_last: RenderSettings::default(),
            last_folder: None,
            validator: ValidatorState::default(),
            validator_view: None,
            action_toast: None,
            timeline: TimelineState::default(),
            animate_tab: AnimateTab::default(),
            animations_panel: AnimationsPanel::default(),
        }
    }

    /// The preferences as they are now; store them when [`UiRequest::PrefsChanged`] arrives and
    /// when the app quits.
    pub fn prefs(&self) -> &Prefs {
        &self.prefs
    }

    /// Replaces the preferences, for example after the user edited the file.
    pub fn set_prefs(&mut self, prefs: Prefs) {
        let mut prefs = prefs;
        prefs.sanitize();
        self.keymap = Keymap::with_overrides(prefs.keymap, &prefs.key_overrides);
        self.prefs = prefs;
        self.layout_generation += 1;
    }

    /// Describes the settings of the open document: the menus and the status bar show them.
    pub fn set_project(&mut self, project: ProjectView) {
        self.project = project;
    }

    /// Sets the movement snap to this grid step in whole steps of the grid, not whole units. The
    /// preset of a Minecraft document asks for 1/16.
    pub fn set_snap_grid(&mut self, grid: f32) {
        let snap = &mut self.prefs.snap;
        if snap.whole_units || (snap.grid - grid).abs() > f32::EPSILON {
            snap.whole_units = false;
            snap.grid = grid;
            self.prefs_dirty = true;
        }
    }

    /// The active workspace.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Switches workspace without sending a request (the app already knows).
    pub fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.arrange_for(mode);
        }
        self.mode = mode;
        if !mode.tools().contains(&self.tool) {
            self.tool = default_tool(mode);
        }
    }

    /// The active tool.
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// Chooses the tool without sending a request.
    pub fn set_tool(&mut self, tool: Tool) {
        if !self.mode.tools().contains(&tool) {
            self.set_mode(tool.mode());
        }
        self.tool = tool;
    }

    /// The validator tab is shown: the app runs its live check only then.
    pub fn validator_visible(&self) -> bool {
        self.props_tab == PropsTab::Validator && self.prefs.layout.is_shown(PanelId::Properties)
    }

    /// Opens the validator tab of the properties area.
    pub fn show_validator(&mut self) {
        self.props_tab = PropsTab::Validator;
        self.prefs.layout.reveal(PanelId::Properties);
        self.prefs_dirty = true;
    }

    /// Shows the start screen until the user leaves it. The app calls it at start-up when no
    /// file was named on the command line.
    pub fn show_start(&mut self) {
        self.start_forced = true;
    }

    /// Shows a message in the corner.
    pub fn toast(&mut self, level: Level, text: impl Into<String>) {
        self.toasts.push(level, text);
    }

    /// Opens the recovery dialog.
    pub fn open_recovery(&mut self, info: RecoveryInfo) {
        if info.entries.is_empty() {
            return;
        }
        self.recovery = Some(Recovery { info });
    }

    /// The window's close button was pressed. Returns `true` when the app may quit at once;
    /// otherwise the unsaved-changes dialog is shown and a [`UiRequest::Quit`] follows its
    /// answer.
    pub fn close_requested(&mut self) -> bool {
        if !self.doc_dirty {
            return true;
        }
        self.unsaved = Some(Unsaved {
            then: UiRequest::Quit,
            title: self.doc_name.clone(),
        });
        false
    }

    /// Builds the window on an egui context.
    pub fn frame(&mut self, ctx: &Context, cx: &UiCx<'_>) -> UiOutput {
        ctx.root_ui(|ui| self.show(ui, cx))
    }

    /// Builds the window inside a root `Ui`.
    pub fn show(&mut self, ui: &mut egui::Ui, cx: &UiCx<'_>) -> UiOutput {
        let ctx = ui.ctx().clone();
        if let Some(fonts) = self.fonts.take() {
            install_fonts(&ctx, &fonts);
        }
        if self.neutral_on && self.prefs.accent_chosen {
            self.neutral_on = false;
            neutral::forget(&ctx);
        }
        self.prefs.theme().apply(&ctx);
        if !self.prefs.accent_chosen {
            self.neutral_on = true;
            neutral::apply(&ctx);
        }
        let tokens = Tokens::of(&ctx);
        let mut sink = Sink::default();
        let mut out = UiOutput::default();

        self.doc_dirty = cx.doc.as_ref().is_some_and(|d| d.dirty);
        if let Some(doc) = cx.doc.as_ref()
            && self.doc_name != doc.title
        {
            self.doc_name = doc.title.to_owned();
        }
        self.validator_view = cx.validator.cloned();
        let modal_open = self.unsaved.is_some()
            || self.recovery.is_some()
            || self.export.is_some()
            || self.render.is_some();
        let typing = ctx.egui_wants_keyboard_input();

        if !modal_open && !self.palette.is_open() && !self.prefs_win.capturing() {
            self.handle_keys(&ctx, cx, typing, &mut sink);
        }
        self.handle_drops(&ctx, cx, &mut sink);

        let show_start = cx.doc.is_none() || self.start_forced;
        let window = ctx.content_rect().width();

        // Top bar.
        let mut menu_events = Vec::new();
        Panel::top("cuboid_topbar")
            .frame(Frame::new().fill(tokens.bg_panel))
            .exact_size(height::ACTION)
            .resizable(false)
            .show(ui, |ui| {
                menu_events = topbar::show(
                    ui,
                    &self.keymap,
                    self.mode,
                    cx,
                    &self.prefs,
                    &self.project,
                    window,
                );
            });
        for event in menu_events {
            match event {
                MenuEvent::Run(command) => self.run_command(command, cx, &mut sink),
                MenuEvent::OpenRecent(path) => {
                    self.guard_dirty(UiRequest::Open(path), cx, &mut sink)
                }
                MenuEvent::Mode(mode) => self.switch_mode(mode, &mut sink),
                MenuEvent::Window(request) => out.window.push(request),
                MenuEvent::Project(change) => sink.push(UiRequest::Project(change)),
                MenuEvent::ProjectSettings => self.project_win.open(),
            }
        }

        // Status bar.
        let bar = BarState {
            view_hovered: self.view_hovered && !show_start,
        };
        Panel::bottom("cuboid_status")
            .frame(Frame::new().fill(tokens.bg_panel))
            .exact_size(height::CONTROL)
            .resizable(false)
            .show(ui, |ui| {
                statusbar::show(ui, cx, self.mode, self.tool, &self.prefs, &bar, &mut sink);
            });

        // Tool strip, areas and the centre.
        match (cx.doc.as_ref(), show_start) {
            (Some(doc), false) => {
                let mut events = Vec::new();
                Panel::left("cuboid_tools")
                    .frame(Frame::new().fill(tokens.bg_panel))
                    .exact_size(height::STRIP)
                    .resizable(false)
                    .show(ui, |ui| {
                        events = toolbar::show(
                            ui,
                            self.mode,
                            self.tool,
                            &self.prefs.snap,
                            &self.keymap,
                            cx,
                        );
                    });
                for event in events {
                    match event {
                        ToolbarEvent::Tool(tool) => {
                            self.run_ui_command(UiCommand::SetTool(tool), cx, &mut sink)
                        }
                        ToolbarEvent::Run(command) => self.run_command(command, cx, &mut sink),
                    }
                }
                self.zone(ui, Zone::Left, doc, cx.library, window, &mut sink);
                self.zone(ui, Zone::Right, doc, cx.library, window, &mut sink);
                if self.mode == Mode::Animate {
                    self.timeline_panel(ui, doc, &tokens, &mut sink);
                }
                egui::CentralPanel::no_frame().show(ui, |ui| {
                    let result = self.viewport.show(
                        ui,
                        cx,
                        &self.prefs,
                        &self.project,
                        self.mode,
                        &mut sink,
                        &mut out.viewports,
                    );
                    out.pointer_over_ui = !result.pointer_over_view;
                    if result.pointer_over_view != self.view_hovered {
                        // The status bar was drawn before the views; show its hint next frame.
                        self.view_hovered = result.pointer_over_view;
                        ctx.request_repaint();
                    }
                    if let Some((index, slot)) = result.set_cell {
                        self.set_cell(index, slot);
                    }
                    if let Some(layout) = result.set_layout {
                        self.prefs.view_layout = layout;
                        self.prefs_dirty = true;
                    }
                    for change in result.project_changes {
                        sink.push(UiRequest::Project(change));
                    }
                    if let Some(options) = result.tool_options {
                        self.prefs.tool_options = options;
                        self.prefs_dirty = true;
                        sink.push(UiRequest::SetToolOptions(options));
                    }
                });
            }
            _ => {
                self.view_hovered = false;
                let events = self.start.show(
                    ui,
                    cx,
                    &mut self.images,
                    &self.prefs,
                    cx.doc.is_some(),
                    self.first_run,
                    &self.export_info.versions,
                );
                for event in events {
                    self.start_event(event, cx, &mut sink);
                }
                out.pointer_over_ui = true;
            }
        }

        library::finish_drag(&ctx, &out.viewports, &mut sink);
        if std::mem::take(&mut sink.project_settings) {
            self.project_win.open();
        }

        // Overlays.
        if let Some(command) = self.palette.show(&ctx, &self.keymap, self.mode, cx) {
            self.run_command(command, cx, &mut sink);
        }
        if cx.doc.is_some() {
            self.project_win.show(&ctx, &self.project, &mut sink);
        }
        let outcome = self.prefs_win.show(&ctx, &mut self.prefs, &mut self.keymap);
        if outcome.changed {
            self.prefs_dirty = true;
        }
        self.dialogs(&ctx, cx, &mut sink);
        self.drop_overlay(&ctx, &tokens);
        self.toasts.show(&ctx);

        // Close the start screen again once a document replaced it.
        if cx.doc.is_none() {
            self.start_forced = false;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.start_forced && !typing {
            self.settle_nav();
            self.start_forced = false;
        }
        self.images.end_frame();

        let size_settled = !ctx.input(|i| i.pointer.any_down());
        if self.prefs_dirty || (self.size_dirty && size_settled) {
            self.prefs_dirty = false;
            self.size_dirty = false;
            sink.push(UiRequest::PrefsChanged(Box::new(self.prefs.clone())));
        }

        let title = window_title(cx);
        if title != self.title {
            self.title.clone_from(&title);
            out.title = Some(title);
        }
        out.keyboard_captured = ctx.egui_wants_keyboard_input()
            || self.palette.is_open()
            || self.prefs_win.capturing()
            || modal_open;
        out.requests = sink.requests;
        out
    }

    /// The strip below the views in the Animate workspace: a resizable panel between the two
    /// areas. Its height is a preference, at most 60 % of the window.
    fn timeline_panel(
        &mut self,
        ui: &mut egui::Ui,
        doc: &DocView<'_>,
        tokens: &Tokens,
        sink: &mut Sink,
    ) {
        let most = (ui.ctx().content_rect().height() * TIMELINE_SHARE).max(TIMELINE_MIN);
        let shown = Panel::bottom("cuboid_timeline")
            .frame(Frame::new().fill(tokens.bg_panel))
            .resizable(true)
            .size_range(TIMELINE_MIN..=most)
            .default_size(self.prefs.timeline_height.min(most))
            .show(ui, |ui| {
                self.timeline.show(ui, doc.animations, doc.timeline, sink);
            });
        // Only a drag of the user is a new height: a small window clamps the shown one, and a
        // clamped height is not the user's.
        let size = shown.response.rect.height();
        let dragging = ui.input(|i| i.pointer.any_down());
        let clamped = size >= most - 0.5;
        if dragging
            && !clamped
            && size.is_finite()
            && (size - self.prefs.timeline_height).abs() > 0.5
        {
            self.prefs.timeline_height = size;
            self.size_dirty = true;
        }
    }

    fn set_cell(&mut self, index: usize, slot: crate::model::ViewSlot) {
        let cells = self.prefs.view_layout.cells();
        let old = self.prefs.view_cells[index.min(3)];
        for (i, cell) in self.prefs.view_cells.iter_mut().enumerate().take(cells) {
            if i != index && *cell == slot.0 {
                *cell = old;
            }
        }
        self.prefs.view_cells[index.min(3)] = slot.0;
        self.prefs_dirty = true;
    }

    fn handle_keys(&mut self, ctx: &Context, cx: &UiCx<'_>, typing: bool, sink: &mut Sink) {
        for chord in ctx.input(pressed_chords) {
            if typing && !chord.works_while_typing() {
                continue;
            }
            // The Cubik camera pans while Space is held: the key does not also play.
            if chord.key == egui::Key::Space && self.prefs.nav == NavPreset::Cubik {
                continue;
            }
            let Some(command) = self.keymap.lookup(chord, self.mode) else {
                continue;
            };
            consume(ctx, chord);
            self.run_command(command, cx, sink);
        }
    }

    fn handle_drops(&mut self, ctx: &Context, cx: &UiCx<'_>, sink: &mut Sink) {
        let dropped: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        // Files dropped on the Library panel are imported by the panel.
        let over_library = self.library.rect.take().is_some_and(|rect| {
            ctx.pointer_latest_pos()
                .is_some_and(|pos| rect.contains(pos))
        });
        if over_library {
            return;
        }
        let count = dropped.len();
        if let Some(path) = dropped.into_iter().next() {
            if count > 1 {
                self.toasts.push(
                    Level::Info,
                    "Only one file can be opened at a time; opening the first.",
                );
            }
            self.guard_dirty(UiRequest::Open(path), cx, sink);
        }
    }

    fn drop_overlay(&self, ctx: &Context, tokens: &Tokens) {
        if !ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("cuboid_drop"),
        ));
        let rect = ctx.content_rect();
        painter.rect_filled(rect, 0.0, tokens.bg_app.gamma_multiply(0.8));
        painter.rect_stroke(
            rect.shrink(16.0),
            radius::CARD,
            egui::Stroke::new(2.0, tokens.accent),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop a file to open it",
            Type::Display.font(),
            tokens.text_strong,
        );
    }

    fn dialogs(&mut self, ctx: &Context, _cx: &UiCx<'_>, sink: &mut Sink) {
        self.export_dialogs(ctx, sink);
        if let Some(dialog) = self.unsaved.as_ref() {
            match dialog.show(ctx) {
                UnsavedAnswer::Pending => {}
                UnsavedAnswer::Save(then) => {
                    self.unsaved = None;
                    sink.push(UiRequest::SaveThen(Box::new(then)));
                }
                UnsavedAnswer::Discard(then) => {
                    self.unsaved = None;
                    sink.push(then);
                }
                UnsavedAnswer::Cancel => self.unsaved = None,
            }
        }
        if let Some(dialog) = self.recovery.as_mut() {
            let (choice, close) = dialog.show(ctx);
            if let Some(choice) = choice {
                sink.push(UiRequest::Recovery(choice));
            }
            if close {
                self.recovery = None;
            }
        }
    }

    /// The export, render and result windows of this frame.
    fn export_dialogs(&mut self, ctx: &Context, sink: &mut Sink) {
        if let Some(mut dialog) = self.export.take() {
            match dialog.show(ctx, &self.export_info, sink) {
                ExportEvent::None => self.export = Some(dialog),
                ExportEvent::Close => self.remember_export(&dialog),
                ExportEvent::ToRender => {
                    self.remember_export(&dialog);
                    self.render = Some(RenderDialog::new(dialog.location, self.render_last));
                }
            }
        }
        if let Some(dialog) = self.render.as_mut() {
            match dialog.show(ctx, sink) {
                RenderEvent::None => {}
                RenderEvent::Close => {
                    self.render_last = dialog.settings;
                    let folder = dialog.location.folder.clone();
                    self.remember_folder(&folder);
                    self.render = None;
                }
            }
        }
        if let Some(toast) = self.action_toast.as_mut()
            && !toast.show(ctx, sink)
        {
            self.action_toast = None;
        }
    }

    fn remember_export(&mut self, dialog: &ExportDialog) {
        self.export_last = dialog.settings.clone();
        self.remember_folder(&dialog.location.folder);
    }

    fn remember_folder(&mut self, folder: &str) {
        let folder = folder.trim();
        if !folder.is_empty() {
            self.last_folder = Some(PathBuf::from(folder));
        }
    }

    /// Opens the export dialog for a document called `name`, starting in `folder` (the folder of
    /// the last export wins over it). `format` preselects a format; without it the dialog starts
    /// with the one used last.
    pub fn open_export(&mut self, name: &str, folder: Option<&Path>, format: Option<ExportFormat>) {
        let start = self
            .last_folder
            .clone()
            .or_else(|| folder.map(Path::to_path_buf))
            .unwrap_or_else(|| self.export_info.folder.clone());
        let name = if name.is_empty() { "model" } else { name };
        let mut last = self.export_last.clone();
        if let Some(format) = format {
            last.format = format;
        }
        self.export = Some(ExportDialog::new(&self.export_info, &start, name, &last));
    }

    /// Tells the export dialog which game versions to offer and where to start. Call once at
    /// start-up.
    pub fn set_export_info(&mut self, info: ExportInfo) {
        self.export_info = info;
    }

    /// Where the export dialog is, `None` while it is closed.
    pub fn export_stage(&self) -> Option<&ExportStage> {
        self.export.as_ref().map(|dialog| &dialog.stage)
    }

    /// Whether the render dialog is open.
    pub fn render_open(&self) -> bool {
        self.render.is_some()
    }

    /// The answer to [`ExportRequest::Check`](crate::ExportRequest) or `FixErrors`.
    pub fn export_checked(&mut self, report: ExportReport) {
        if let Some(dialog) = self.export.as_mut() {
            dialog.checked(report);
        }
    }

    /// The export ended. The dialog shows the result list; when it was closed meanwhile the
    /// result is a toast.
    pub fn export_finished(&mut self, outcome: ExportOutcome) {
        if let Some(dialog) = self.export.as_mut() {
            dialog.finished(outcome);
            return;
        }
        match outcome {
            ExportOutcome::Done { path, notices } => {
                let text = match notices.len() {
                    0 => format!("Exported {}", path.display()),
                    count => format!("Exported {} with {count} notices", path.display()),
                };
                self.action_toast = Some(ActionToast::new(
                    Level::Success,
                    text,
                    "Open folder".to_owned(),
                    UiRequest::OpenFolder(path),
                ));
            }
            ExportOutcome::Failed(text) => self.toast(Level::Error, text),
            ExportOutcome::Declined => {}
        }
    }

    /// A render ended: a toast with "Open folder" for a file, the dialog's error text otherwise.
    pub fn render_finished(&mut self, result: Result<PathBuf, String>) {
        match result {
            Ok(path) => {
                if let Some(dialog) = self.render.take() {
                    self.render_last = dialog.settings;
                    self.remember_folder(&dialog.location.folder);
                }
                self.action_toast = Some(ActionToast::new(
                    Level::Success,
                    format!("Rendered {}", path.display()),
                    "Open folder".to_owned(),
                    UiRequest::OpenFolder(path),
                ));
            }
            Err(text) => match self.render.as_mut() {
                Some(dialog) => dialog.failed(text),
                None => self.toast(Level::Error, text),
            },
        }
    }

    /// The answer to [`UiRequest::BrowseFolder`]: puts the folder into the open dialog.
    pub fn set_folder(&mut self, folder: &Path) {
        let text = folder.display().to_string();
        if let Some(dialog) = self.export.as_mut() {
            dialog.location.folder = text;
        } else if let Some(dialog) = self.render.as_mut() {
            dialog.location.folder = text;
        }
    }

    fn start_event(&mut self, event: StartEvent, cx: &UiCx<'_>, sink: &mut Sink) {
        match event {
            StartEvent::ComingFrom(from) => {
                self.prefs.apply_coming_from(from);
                self.layout_generation += 1;
                self.keymap = Keymap::with_overrides(self.prefs.keymap, &self.prefs.key_overrides);
                self.prefs_dirty = true;
            }
            StartEvent::Accent(accent) => {
                self.prefs.accent = accent;
                self.prefs.accent_chosen = true;
                self.prefs_dirty = true;
            }
            StartEvent::Continue => {
                self.settle_nav();
            }
            StartEvent::New(template) => {
                self.settle_nav();
                self.start_forced = false;
                self.guard_dirty(UiRequest::NewDocument(template), cx, sink);
            }
            StartEvent::Open(path) => {
                self.settle_nav();
                self.start_forced = false;
                self.guard_dirty(UiRequest::Open(path), cx, sink);
            }
            StartEvent::OpenDialog => self.guard_dirty(UiRequest::OpenDialog, cx, sink),
            StartEvent::RemoveRecent(path) => sink.push(UiRequest::RemoveRecent(path)),
            StartEvent::Library(request) => sink.push(UiRequest::Library(request)),
            StartEvent::Close => {
                self.settle_nav();
                self.start_forced = false;
            }
        }
    }

    /// Leaving the start screen answers the first-run question with the app that was preselected.
    fn settle_nav(&mut self) {
        self.first_run = false;
        if !self.prefs.nav_chosen {
            self.prefs.apply_coming_from(self.prefs.coming_from);
            self.layout_generation += 1;
            self.keymap = Keymap::with_overrides(self.prefs.keymap, &self.prefs.key_overrides);
            self.prefs_dirty = true;
        }
    }

    /// Runs `request` now, or after the unsaved-changes question when the document is dirty.
    fn guard_dirty(&mut self, request: UiRequest, cx: &UiCx<'_>, sink: &mut Sink) {
        if cx.doc.as_ref().is_some_and(|d| d.dirty) {
            self.unsaved = Some(Unsaved {
                then: request,
                title: self.doc_name.clone(),
            });
        } else {
            sink.push(request);
        }
    }

    fn switch_mode(&mut self, mode: Mode, sink: &mut Sink) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        sink.push(UiRequest::SetMode(mode));
        if !mode.tools().contains(&self.tool) {
            self.tool = default_tool(mode);
            if !mode.tools().is_empty() {
                sink.push(UiRequest::SetTool(self.tool));
            }
        }
        self.arrange_for(mode);
    }

    /// Puts the panels a workspace works with in front: the palette in Voxel, the UV editor and
    /// the palette in Paint, and the tab of the properties that suits it. Closed areas stay
    /// closed.
    fn arrange_for(&mut self, mode: Mode) {
        let layout = &mut self.prefs.layout;
        layout.bring_forward(PanelId::Properties);
        if mode == Mode::Paint {
            layout.bring_forward(PanelId::Uv);
        }
        if matches!(mode, Mode::Voxel | Mode::Paint) {
            layout.bring_forward(PanelId::Palette);
        }
        if mode == Mode::Animate {
            layout.bring_forward(PanelId::Animations);
        }
        self.props_tab = PropsTab::for_mode(mode);
        self.prefs_dirty = true;
    }

    fn run_command(&mut self, command: Command, cx: &UiCx<'_>, sink: &mut Sink) {
        match command {
            Command::Action(id) => {
                if cx.can_run(id) {
                    self.run_action(id, cx, sink);
                }
            }
            Command::Ui(ui_command) => self.run_ui_command(ui_command, cx, sink),
        }
    }

    fn run_action(&mut self, id: ActionId, cx: &UiCx<'_>, sink: &mut Sink) {
        let has_doc = cx.doc.is_some();
        match id {
            ActionId::NewFile => self.start_forced = true,
            ActionId::Open => self.guard_dirty(UiRequest::OpenDialog, cx, sink),
            ActionId::Save if has_doc => sink.push(UiRequest::Save),
            ActionId::SaveAs if has_doc => sink.push(UiRequest::SaveAs),
            ActionId::Export if has_doc => {
                let doc = cx.doc.as_ref();
                let name = Path::new(doc.map_or("", |d| d.title))
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                let folder = doc.and_then(|d| d.path).and_then(Path::parent);
                self.open_export(&name, folder, None);
            }
            ActionId::Save | ActionId::SaveAs | ActionId::Export => {}
            ActionId::Exit => self.guard_dirty(UiRequest::Quit, cx, sink),
            ActionId::Preferences => self.prefs_win.open(),
            ActionId::Rename if self.mode == Mode::Animate => {
                self.reveal(PanelId::Animations);
                self.animations_panel.begin_rename();
            }
            ActionId::Rename => {
                if let Some(doc) = cx.doc.as_ref() {
                    self.rename_primary(doc);
                }
            }
            // In Animate the keys of the timeline act on the selected keyframes first.
            ActionId::Delete | ActionId::DeleteConfirm | ActionId::ElementDelete
                if self.keyframes_selected() =>
            {
                let keys = self.timeline.take_selection();
                sink.push(UiRequest::Animate(AnimateEdit::Delete { keys }));
            }
            ActionId::Copy if self.keyframes_selected() => {
                let keys = self.timeline.selected().to_vec();
                sink.push(UiRequest::Animate(AnimateEdit::Copy { keys }));
            }
            ActionId::Paste if self.keyframes_selected() => {
                let bone = cx.doc.as_ref().and_then(|d| d.primary);
                sink.push(UiRequest::Animate(AnimateEdit::Paste { bone }));
            }
            ActionId::ElementTools | ActionId::GizmosTools => self.switch_mode(Mode::Model, sink),
            ActionId::VoxelTools => self.switch_mode(Mode::Voxel, sink),
            ActionId::PaintTools => self.switch_mode(Mode::Paint, sink),
            ActionId::ColorTools | ActionId::ColorPalette => self.reveal(PanelId::Palette),
            ActionId::ElementsList => self.reveal(PanelId::Outliner),
            ActionId::ElementsMaterial => self.reveal(PanelId::Textures),
            ActionId::MoveManagersOnLeft => self.move_panel(PanelId::Outliner, Zone::Left),
            ActionId::MoveManagersOnRight => self.move_panel(PanelId::Outliner, Zone::Right),
            ActionId::HideManagers => self.hide_panel_area(PanelId::Outliner),
            ActionId::HideHelpers => self.hide_panel_area(PanelId::Properties),
            ActionId::HideAll => {
                for zone in Zone::ALL {
                    self.set_zone_open(zone, false);
                }
            }
            other => sink.action(other),
        }
    }

    fn run_ui_command(&mut self, command: UiCommand, cx: &UiCx<'_>, sink: &mut Sink) {
        let has_doc = cx.doc.is_some();
        match command {
            UiCommand::Palette => self.palette.open(),
            UiCommand::ShowStart => self.start_forced = true,
            UiCommand::Preferences => self.prefs_win.open(),
            UiCommand::KeyboardShortcuts => self.prefs_win.open_keymap(),
            UiCommand::TogglePanel(panel) => {
                self.prefs.layout.toggle(panel);
                self.prefs_dirty = true;
            }
            UiCommand::ResetLayout => {
                self.prefs.layout = Layout::arranged(self.prefs.coming_from.arrangement());
                self.layout_generation += 1;
                self.prefs_dirty = true;
            }
            UiCommand::ShowValidator => self.show_validator(),
            UiCommand::SetViewLayout(layout) => {
                self.prefs.view_layout = layout;
                self.prefs_dirty = true;
            }
            UiCommand::SetMode(mode) if has_doc => self.switch_mode(mode, sink),
            UiCommand::NextMode if has_doc => self.switch_mode(self.mode.next(), sink),
            UiCommand::SetTool(tool) if has_doc => {
                if !self.mode.tools().contains(&tool) {
                    self.switch_mode(tool.mode(), sink);
                }
                self.tool = tool;
                sink.push(UiRequest::SetTool(tool));
            }
            UiCommand::PlayPause if has_doc => {
                let playing = cx
                    .doc
                    .as_ref()
                    .is_some_and(|d| d.animations.is_some_and(|a| a.playing));
                let request = if playing {
                    AnimationRequest::Pause
                } else {
                    AnimationRequest::Play
                };
                sink.push(UiRequest::Animation(request));
            }
            UiCommand::PreviousFrame
            | UiCommand::NextFrame
            | UiCommand::FirstFrame
            | UiCommand::LastFrame
                if has_doc =>
            {
                if let Some(time) = cx.doc.as_ref().and_then(|d| step_playhead(command, d)) {
                    sink.push(UiRequest::Animation(AnimationRequest::Seek(time)));
                }
            }
            UiCommand::InsertKeyframe if has_doc => {
                sink.push(UiRequest::Animate(AnimateEdit::KeySelected));
            }
            UiCommand::NewAnimation if has_doc => {
                sink.push(UiRequest::Animate(AnimateEdit::NewAnimation));
            }
            UiCommand::View(preset) if has_doc => self.view_event(preset, sink),
            UiCommand::FrameSelected if has_doc => {
                sink.push(UiRequest::Viewport(ViewportEvent::Frame {
                    slot: self.viewport.active,
                }));
            }
            UiCommand::ToggleSnap => {
                self.prefs.snap.enabled = !self.prefs.snap.enabled;
                self.prefs_dirty = true;
            }
            UiCommand::GroupSelection if has_doc => sink.edit(Edit::GroupSelection),
            UiCommand::Ungroup if has_doc => sink.edit(Edit::Ungroup),
            UiCommand::ToggleTheme => {
                let dark = self.prefs.theme().is_dark(None);
                self.prefs.color_mode = if dark {
                    ColorMode::Light
                } else {
                    ColorMode::Dark
                };
                self.prefs_dirty = true;
            }
            _ => {}
        }
    }

    fn view_event(&mut self, preset: ViewPreset, sink: &mut Sink) {
        sink.push(UiRequest::Viewport(ViewportEvent::SetView {
            slot: self.viewport.active,
            preset,
        }));
    }

    fn rename_primary(&mut self, doc: &DocView<'_>) {
        let Some(primary) = doc.primary else { return };
        let name = doc
            .nodes
            .iter()
            .find(|n| n.id == primary)
            .map_or_else(String::new, |n| n.name.to_string());
        self.reveal(PanelId::Outliner);
        self.outliner.begin_rename(primary, &name);
    }

    /// Keyframes of the timeline are selected, so Delete, Copy and Paste mean them.
    fn keyframes_selected(&self) -> bool {
        self.mode == Mode::Animate && !self.timeline.selected().is_empty()
    }

    fn reveal(&mut self, panel: PanelId) {
        self.prefs.layout.reveal(panel);
        self.prefs_dirty = true;
    }

    fn move_panel(&mut self, panel: PanelId, zone: Zone) {
        self.prefs.layout.move_panel(panel, zone);
        self.layout_generation += 1;
        self.prefs_dirty = true;
    }

    fn set_zone_open(&mut self, zone: Zone, open: bool) {
        self.prefs.layout.zone_mut(zone).open = open;
        self.prefs_dirty = true;
    }

    /// Hides the area that holds `panel`.
    fn hide_panel_area(&mut self, panel: PanelId) {
        if let Some(zone) = self.prefs.layout.zone_of(panel) {
            self.set_zone_open(zone, false);
        }
    }
}

fn consume(ctx: &Context, chord: Chord) {
    let modifiers = egui::Modifiers {
        alt: chord.alt,
        ctrl: chord.ctrl,
        shift: chord.shift,
        mac_cmd: false,
        command: chord.ctrl,
    };
    ctx.input_mut(|i| i.consume_key(modifiers, chord.key));
}

fn default_tool(mode: Mode) -> Tool {
    mode.tools().first().copied().unwrap_or(Tool::Select)
}

/// The time a frame command puts the playhead at, in the chosen animation.
fn step_playhead(command: UiCommand, doc: &DocView<'_>) -> Option<f32> {
    let animations = doc.animations?;
    let current = animations.current()?;
    let frame = 1.0 / current.fps.max(1) as f32;
    let now = (animations.time / frame).round();
    let time = match command {
        UiCommand::PreviousFrame => (now - 1.0) * frame,
        UiCommand::NextFrame => (now + 1.0) * frame,
        UiCommand::FirstFrame => 0.0,
        _ => current.length,
    };
    Some(time.clamp(0.0, current.length))
}

fn window_title(cx: &UiCx<'_>) -> String {
    match cx.doc.as_ref() {
        Some(doc) => format!("{} - Cuboid", topbar::document_label(doc)),
        None => "Cuboid".to_owned(),
    }
}
