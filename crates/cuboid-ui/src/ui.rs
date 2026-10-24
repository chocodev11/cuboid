//! The window: ties the panels together and turns user input into requests.

use cuboid_actions::ActionId;
use cuboid_ui_kit::{FontSet, Level, RADIUS_LG, Toasts, Tokens, install_fonts, tab_button};
use egui::{Context, Panel};

use crate::commands::PaletteState;
use crate::common::{ImageCache, Sink};
use crate::dialogs::{Recovery, RecoveryInfo, Unsaved, UnsavedAnswer};
use crate::history;
use crate::keymap::{Chord, Command, Keymap, UiCommand, pressed_chords};
use crate::layout::{BOTTOM_MAX, BOTTOM_MIN, PanelId, SIDE_MAX, SIDE_MIN, Zone};
use crate::menubar::{self, MenuEvent};
use crate::model::{DocView, UiCx};
use crate::outliner::OutlinerState;
use crate::prefs::{ColorMode, Prefs};
use crate::prefs_ui::PrefsWindow;
use crate::properties::PropsState;
use crate::request::{Edit, Mode, Tool, UiOutput, UiRequest, ViewPreset, ViewportEvent};
use crate::start::{StartEvent, StartState};
use crate::statusbar;
use crate::textures;
use crate::toolbar::{self, ToolbarEvent};
use crate::uv::UvState;
use crate::viewport::ViewportState;

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
    start_forced: bool,
    prefs_win: PrefsWindow,
    recovery: Option<Recovery>,
    unsaved: Option<Unsaved>,
    images: ImageCache,
    title: String,
    doc_name: String,
    doc_dirty: bool,
    prefs_dirty: bool,
    size_dirty: bool,
    layout_generation: u32,
    paint_hint_shown: bool,
}

impl Ui {
    /// A UI with the stored preferences. `fonts` are installed on the first frame.
    pub fn new(prefs: Prefs, fonts: FontSet) -> Ui {
        let mut prefs = prefs;
        prefs.sanitize();
        let mode = prefs.mode;
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
            start_forced: false,
            prefs_win: PrefsWindow::default(),
            recovery: None,
            unsaved: None,
            images: ImageCache::default(),
            title: String::new(),
            doc_name: String::new(),
            doc_dirty: false,
            prefs_dirty: false,
            size_dirty: false,
            layout_generation: 0,
            paint_hint_shown: false,
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

    /// The active workspace.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Switches workspace without sending a request (the app already knows).
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        if self.tool.mode() != mode {
            self.tool = default_tool(mode);
        }
    }

    /// The active tool.
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// Chooses the tool without sending a request.
    pub fn set_tool(&mut self, tool: Tool) {
        self.mode = tool.mode();
        self.tool = tool;
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
        self.prefs.theme().apply(&ctx);
        let tokens = Tokens::of(&ctx);
        let mut sink = Sink::default();
        let mut out = UiOutput::default();

        self.doc_dirty = cx.doc.as_ref().is_some_and(|d| d.dirty);
        if let Some(doc) = cx.doc.as_ref()
            && self.doc_name != doc.title
        {
            self.doc_name = doc.title.to_owned();
        }
        let modal_open = self.unsaved.is_some() || self.recovery.is_some();
        let typing = ctx.egui_wants_keyboard_input();

        if !modal_open && !self.palette.is_open() && !self.prefs_win.capturing() {
            self.handle_keys(&ctx, cx, typing, &mut sink);
        }
        self.handle_drops(&ctx, cx, &mut sink);

        let show_start = cx.doc.is_none() || self.start_forced;

        // Menu bar.
        let mut menu_events = Vec::new();
        Panel::top("cuboid_menu")
            .frame(bar_frame(&tokens, 4))
            .show_separator_line(false)
            .show(ui, |ui| {
                menu_events = menubar::show(
                    ui,
                    &self.keymap,
                    self.mode,
                    cx,
                    &self.prefs,
                    self.palette.is_open(),
                );
            });
        for event in menu_events {
            match event {
                MenuEvent::Run(command) => self.run_command(command, cx, &mut sink),
                MenuEvent::OpenRecent(path) => {
                    self.guard_dirty(UiRequest::Open(path), cx, &mut sink)
                }
                MenuEvent::Mode(mode) => self.switch_mode(mode, &mut sink),
            }
        }

        // Tool row.
        if cx.doc.is_some() && !show_start {
            let mut events = Vec::new();
            Panel::top("cuboid_tools")
                .frame(bar_frame(&tokens, 4))
                .exact_size(38.0)
                .show(ui, |ui| {
                    events = toolbar::show(
                        ui,
                        self.mode,
                        self.tool,
                        self.prefs.tool_options,
                        &self.keymap,
                        &self.prefs.layout,
                    );
                });
            for event in events {
                match event {
                    ToolbarEvent::Tool(tool) => {
                        self.run_ui_command(UiCommand::SetTool(tool), cx, &mut sink)
                    }
                    ToolbarEvent::Run(command) => self.run_command(command, cx, &mut sink),
                    ToolbarEvent::Options(options) => {
                        self.prefs.tool_options = options;
                        self.prefs_dirty = true;
                        sink.push(UiRequest::SetToolOptions(options));
                    }
                    ToolbarEvent::ToggleZone(zone) => {
                        let state = self.prefs.layout.zone_mut(zone);
                        state.open = !state.open;
                        self.prefs_dirty = true;
                    }
                }
            }
        }

        // Status bar.
        Panel::bottom("cuboid_status")
            .frame(bar_frame(&tokens, 4))
            .exact_size(24.0)
            .show(ui, |ui| {
                statusbar::show(ui, cx, self.mode, self.tool, &self.prefs, &mut sink)
            });

        // Zones and the centre.
        match (cx.doc.as_ref(), show_start) {
            (Some(doc), false) => {
                self.zone(ui, Zone::Left, doc, &mut sink);
                self.zone(ui, Zone::Right, doc, &mut sink);
                self.zone(ui, Zone::Bottom, doc, &mut sink);
                egui::CentralPanel::no_frame().show(ui, |ui| {
                    let result =
                        self.viewport
                            .show(ui, cx, &self.prefs, &mut sink, &mut out.viewports);
                    out.pointer_over_ui = !result.pointer_over_view;
                    if let Some((index, slot)) = result.set_cell {
                        self.set_cell(index, slot);
                    }
                    if let Some(layout) = result.set_layout {
                        self.prefs.view_layout = layout;
                        self.prefs_dirty = true;
                    }
                });
            }
            _ => {
                egui::CentralPanel::default().show(ui, |ui| {
                    let events = self.start.show(ui, cx, &mut self.images, cx.doc.is_some());
                    for event in events {
                        self.start_event(event, cx, &mut sink);
                    }
                });
                out.pointer_over_ui = true;
            }
        }

        // Overlays.
        if let Some(command) = self.palette.show(&ctx, &self.keymap, self.mode, cx) {
            self.run_command(command, cx, &mut sink);
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
            RADIUS_LG,
            egui::Stroke::new(2.0, tokens.accent),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop a file to open it",
            egui::FontId::proportional(22.0),
            tokens.text_strong,
        );
    }

    fn dialogs(&mut self, ctx: &Context, _cx: &UiCx<'_>, sink: &mut Sink) {
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

    fn start_event(&mut self, event: StartEvent, cx: &UiCx<'_>, sink: &mut Sink) {
        match event {
            StartEvent::New(template) => {
                self.start_forced = false;
                self.guard_dirty(UiRequest::NewDocument(template), cx, sink);
            }
            StartEvent::Open(path) => {
                self.start_forced = false;
                self.guard_dirty(UiRequest::Open(path), cx, sink);
            }
            StartEvent::OpenDialog => self.guard_dirty(UiRequest::OpenDialog, cx, sink),
            StartEvent::RemoveRecent(path) => sink.push(UiRequest::RemoveRecent(path)),
            StartEvent::Preferences => self.prefs_win.open(),
            StartEvent::Close => self.start_forced = false,
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
        if self.tool.mode() != mode {
            self.tool = default_tool(mode);
            if !mode.tools().is_empty() {
                sink.push(UiRequest::SetTool(self.tool));
            }
        }
        if mode == Mode::Paint && !self.paint_hint_shown {
            self.paint_hint_shown = true;
            self.prefs.layout.reveal(PanelId::Uv);
            self.prefs_dirty = true;
        }
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
            ActionId::Export if has_doc => sink.push(UiRequest::ExportDialog),
            ActionId::Save | ActionId::SaveAs | ActionId::Export => {}
            ActionId::Exit => self.guard_dirty(UiRequest::Quit, cx, sink),
            ActionId::Preferences => self.prefs_win.open(),
            ActionId::Rename => {
                if let Some(doc) = cx.doc.as_ref() {
                    self.rename_primary(doc);
                }
            }
            ActionId::ElementTools | ActionId::GizmosTools => self.switch_mode(Mode::Model, sink),
            ActionId::VoxelTools => self.switch_mode(Mode::Voxel, sink),
            ActionId::PaintTools => self.switch_mode(Mode::Paint, sink),
            ActionId::ColorTools | ActionId::ColorPalette => self.reveal(PanelId::Properties),
            ActionId::ElementsList => self.reveal(PanelId::Outliner),
            ActionId::ElementsMaterial => self.reveal(PanelId::Textures),
            ActionId::MoveManagersOnLeft => self.move_panel(PanelId::Outliner, Zone::Left),
            ActionId::MoveManagersOnRight => self.move_panel(PanelId::Outliner, Zone::Right),
            ActionId::HideManagers => self.set_zone_open(Zone::Left, false),
            ActionId::HideHelpers => self.set_zone_open(Zone::Right, false),
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
                self.prefs.layout = crate::layout::Layout::default();
                self.layout_generation += 1;
                self.prefs_dirty = true;
            }
            UiCommand::SetViewLayout(layout) => {
                self.prefs.view_layout = layout;
                self.prefs_dirty = true;
            }
            UiCommand::SetMode(mode) if has_doc => self.switch_mode(mode, sink),
            UiCommand::NextMode if has_doc => self.switch_mode(self.mode.next(), sink),
            UiCommand::SetTool(tool) if has_doc => {
                if tool.mode() != self.mode {
                    self.switch_mode(tool.mode(), sink);
                }
                self.tool = tool;
                sink.push(UiRequest::SetTool(tool));
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

    /// One of the three zones around the viewport.
    fn zone(&mut self, ui: &mut egui::Ui, zone: Zone, doc: &DocView<'_>, sink: &mut Sink) {
        let tokens = Tokens::of(ui.ctx());
        let state = self.prefs.layout.zone(zone).clone();
        if !state.open || state.panels.is_empty() {
            return;
        }
        let id = egui::Id::unique(("cuboid_zone", zone.name(), self.layout_generation));
        let (panel, range) = match zone {
            Zone::Left => (Panel::left(id), SIDE_MIN..=SIDE_MAX),
            Zone::Right => (Panel::right(id), SIDE_MIN..=SIDE_MAX),
            Zone::Bottom => (Panel::bottom(id), BOTTOM_MIN..=BOTTOM_MAX),
        };
        let frame = egui::Frame::new()
            .fill(tokens.bg_panel)
            .inner_margin(egui::Margin::symmetric(8, 8))
            .stroke(egui::Stroke::new(1.0, tokens.border));
        let mut move_request: Option<(PanelId, Zone)> = None;
        let shown = panel
            .resizable(true)
            .size_range(range)
            .default_size(state.size)
            .frame(frame)
            .show(ui, |ui| {
                let active = state.active.min(state.panels.len() - 1);
                let mut chosen = active;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (i, panel_id) in state.panels.iter().enumerate() {
                        let resp = tab_button(ui, panel_id.title(), i == active);
                        if resp.clicked() {
                            chosen = i;
                        }
                        resp.context_menu(|ui| {
                            for target in Zone::ALL {
                                if target != zone
                                    && ui
                                        .button(format!("Move to the {} zone", target.name()))
                                        .clicked()
                                {
                                    move_request = Some((*panel_id, target));
                                    ui.close();
                                }
                            }
                        });
                    }
                });
                ui.add_space(4.0);
                if chosen != active {
                    self.prefs.layout.zone_mut(zone).active = chosen;
                    self.prefs_dirty = true;
                }
                let current = state.panels[chosen.min(state.panels.len() - 1)];
                self.panel_body(ui, current, doc, sink);
            });
        let size = match zone {
            Zone::Bottom => shown.response.rect.height(),
            _ => shown.response.rect.width(),
        };
        let stored = &mut self.prefs.layout.zone_mut(zone).size;
        if (size - *stored).abs() > 0.5 && size.is_finite() {
            *stored = size;
            self.size_dirty = true;
        }
        if let Some((panel_id, target)) = move_request {
            self.move_panel(panel_id, target);
        }
    }

    fn panel_body(
        &mut self,
        ui: &mut egui::Ui,
        panel: PanelId,
        doc: &DocView<'_>,
        sink: &mut Sink,
    ) {
        match panel {
            PanelId::Outliner => self.outliner.show(ui, doc, sink),
            PanelId::Properties => {
                self.props
                    .show(ui, self.mode, doc, &self.prefs, &mut self.images, sink)
            }
            PanelId::Uv => {
                let face = self.props.face;
                self.uv
                    .show(ui, doc, doc.props, face, &mut self.images, sink);
            }
            PanelId::Textures => textures::panel(ui, doc, &mut self.images, sink),
            PanelId::History => history::show(ui, doc.history, sink),
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

fn bar_frame(tokens: &Tokens, vertical: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(tokens.bg_panel)
        .inner_margin(egui::Margin::symmetric(8, vertical))
        .stroke(egui::Stroke::new(1.0, tokens.border))
}

fn window_title(cx: &UiCx<'_>) -> String {
    match cx.doc.as_ref() {
        Some(doc) if doc.dirty => format!("* {} - Cuboid", doc.title),
        Some(doc) => format!("{} - Cuboid", doc.title),
        None => "Cuboid".to_owned(),
    }
}
