//! The window of the app each frame: the UI of `cuboid-ui`, what it asks for, the scene index,
//! the extract, the renderer and the view textures the UI shows (spec: arch 10.4, 13.1).
//!
//! One frame runs: jobs and intents ([`State::step`]), the UI, the requests of the UI (file
//! requests become intents, edits become core commands, pointer events become navigation and
//! core ticks), then the scene extract, the upload, the draw lists and the render into the view
//! textures. The UI draws the textures of the frame before; the painter of the shell reads them
//! after the frame returns, so they are current when they reach the screen.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use cuboid_core::{
    ButtonState, ColliderAction, Core, InputFrame, Modifiers, Playhead, SelectMode, TickOutput,
    ViewInput as PointerView,
};
use cuboid_model::ObjRef;
use cuboid_render::{RenderSettings, UploadBudget, ViewInput, ViewParams};
use cuboid_scene::{
    Axes, Camera, ExtractBudget, Extractor, GizmoKind, GizmoState, OverlayState, Raycaster, Rect,
    SceneIndex, ViewDrawList, ViewId, ViewSet, compile_draw_list, gizmo_scale,
};
use cuboid_shell::{FrameCx, WindowCommand};
use cuboid_ui::{
    AnimationsView, BandView, DocView, ExportInfo, FontSet, JobView, Mode, Prefs, RecentEntry,
    RecoveryEntry, RecoveryInfo, Tool, ToolOptions, Ui, UiCx, UiOutput, ViewSlot, ViewportFrame,
    ViewportPlacement, WindowRequest, WindowView,
};
use cuboid_ui_kit::Tokens;
use glam::{Mat4, Vec2, Vec3};

use crate::flows::ExportFlow;
use crate::frontend::playback::Player;
use crate::frontend::textures::TextureClock;
use crate::frontend::timeline::TimelineModel;
use crate::preset::display_light;
use crate::prompts;
use crate::render_glue::RenderGlue;
use crate::state::{Dialog, Level, State};
use crate::thumbs::ThumbRig;
use crate::uimodel::Model;
use crate::validation::{
    LiveValidator, check_profile, live_target, model_name, project_version, version_choices,
};

/// An egui texture that shows a view texture of the renderer.
#[derive(Copy, Clone)]
struct Shown {
    id: egui::TextureId,
    generation: u64,
    size: (u32, u32),
}

/// The pointer as the viewport events of the UI describe it.
#[derive(Default)]
pub struct Pointer {
    /// The view the pointer is in or the press started in.
    pub slot: Option<usize>,
    /// The position in the pixels of that view.
    pub pos: Option<Vec2>,
    pub left: bool,
    pub right: bool,
    pub mods: Modifiers,
    /// The pointer is over UI rather than over a view.
    pub over_ui: bool,
}

/// How long a modification time read from the disk is kept.
const RECENT_TIME_KEEP: Duration = Duration::from_secs(30);

/// When the recent files were last changed. The disk is asked once per file and then at most every
/// 30 seconds, so a frame does not touch it.
#[derive(Default)]
struct RecentTimes {
    seen: HashMap<PathBuf, (Option<SystemTime>, Instant)>,
}

impl RecentTimes {
    fn modified(&mut self, path: &Path, now: Instant) -> Option<SystemTime> {
        if let Some((time, at)) = self.seen.get(path)
            && now.saturating_duration_since(*at) < RECENT_TIME_KEEP
        {
            return *time;
        }
        let time = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        self.seen.insert(path.to_path_buf(), (time, now));
        time
    }
}

pub struct Frontend {
    pub ui: Ui,
    pub views: ViewSet,
    pub index: SceneIndex,
    extractor: Extractor,
    glue: Option<RenderGlue>,
    /// Draws the pictures of models for the start screen and the library.
    rig: ThumbRig,
    shown: [Option<Shown>; 4],
    pub placements: Vec<ViewportPlacement>,
    pub pointer: Pointer,
    pub model: Model,
    pub tick: TickOutput,
    /// The error messages shown to the user, for the log and the tests.
    pub errors: Vec<String>,
    /// Identifies the continuous edit that is running, so a drag is one undo step.
    pub gesture: u64,
    pub options: ToolOptions,
    pub tool: Tool,
    /// The core still has to be told about the tool the UI starts with.
    tool_unsent: bool,
    recovery_open: bool,
    doc_id: u64,
    generation: u64,
    prefs_path: PathBuf,
    title: Option<String>,
    /// The window as the shell reported it for this frame.
    pub window: WindowView,
    /// Window commands for the shell: the title bar's requests and a changed title bar choice.
    pub window_commands: Vec<WindowCommand>,
    /// The "Use the system title bar" choice the window was last told.
    pub system_title_bar: bool,
    /// Another frame is needed at once: uploads or extraction are unfinished, or a drag runs.
    pub more: bool,
    /// When the recent files were last changed, for the start screen.
    recent_times: RecentTimes,
    /// The model check behind the Validator panel.
    pub validation: LiveValidator,
    /// The checks of the export dialog.
    pub export: ExportFlow,
    /// Plays an animation: the pose the views show, which the document does not hold.
    pub player: Player,
    /// The texture tick the views show, and the clock that moves it while textures play.
    pub textures: TextureClock,
    /// What the Animations panel shows.
    animations: AnimationsView,
    /// What the timeline and the Animate tab show.
    timeline: TimelineModel,
    /// The Animate workspace is open, as the player was last told.
    animating: bool,
    /// The pose of the player that the extractor has.
    pose_sent: Option<u64>,
    /// The game version of the preset the export dialog was last told about.
    export_info_for: Option<Option<cuboid_assets::mc_versions::McVersion>>,
    /// The picture the render dialog asked for, while it is made.
    png: Option<render_job::PngRender>,
    /// The folders "Open folder" was asked for; the tests do not start a file manager.
    #[cfg(test)]
    pub revealed: Vec<PathBuf>,
}

impl Frontend {
    pub fn new(prefs: Prefs, fonts: FontSet, prefs_path: PathBuf) -> Frontend {
        let options = prefs.tool_options;
        let system_title_bar = prefs.system_title_bar;
        let ui = Ui::new(prefs, fonts);
        let tool = ui.tool();
        Frontend {
            ui,
            views: ViewSet::default(),
            index: SceneIndex::default(),
            extractor: Extractor::default(),
            glue: None,
            rig: ThumbRig::default(),
            shown: [None; 4],
            placements: Vec::new(),
            pointer: Pointer::default(),
            model: Model::default(),
            tick: TickOutput::default(),
            errors: Vec::new(),
            gesture: 1,
            options,
            tool,
            tool_unsent: true,
            recovery_open: false,
            doc_id: 0,
            generation: 0,
            prefs_path,
            title: None,
            window: WindowView::default(),
            window_commands: Vec::new(),
            system_title_bar,
            more: false,
            recent_times: RecentTimes::default(),
            validation: LiveValidator::default(),
            export: ExportFlow::default(),
            player: Player::default(),
            textures: TextureClock::default(),
            animations: AnimationsView::default(),
            timeline: TimelineModel::default(),
            animating: false,
            pose_sent: None,
            export_info_for: None,
            png: None,
            #[cfg(test)]
            revealed: Vec::new(),
        }
    }

    /// True while a picture of a model is being drawn or waits.
    #[cfg(test)]
    pub fn thumbs_busy(&self, state: &State) -> bool {
        self.rig.busy(&state.library)
    }

    pub fn prefs_path(&self) -> &std::path::Path {
        &self.prefs_path
    }

    /// The window title, once, when it changed.
    pub fn take_title(&mut self) -> Option<String> {
        self.title.take()
    }

    /// The window commands of the frame, once.
    pub fn take_window_commands(&mut self) -> Vec<WindowCommand> {
        std::mem::take(&mut self.window_commands)
    }

    /// One frame of the window. Runs inside the panic boundary of the app.
    pub fn frame(&mut self, state: &mut State, cx: &FrameCx<'_>) {
        let gpu = cx.gpu;
        self.more = false;
        self.window = WindowView {
            maximized: cx.window.maximized,
            fullscreen: cx.window.fullscreen,
            decorated: cx.window.decorated,
            focused: cx.window.focused,
        };
        let glue = self.glue.get_or_insert_with(|| RenderGlue::new(gpu));
        if glue.follow_device(gpu) {
            self.shown = [None; 4];
        }
        if glue.take_device_lost() {
            state.on_device_lost();
        }
        if !self.update(state, cx.egui, cx.now) {
            return;
        }
        self.render(state, cx);
        self.step_png(state, gpu);
        state.editor.cameras = Some(self.views.camera_states());
        let caps = cuboid_render::AdapterCaps::detect(&gpu.adapter, &gpu.device);
        self.rig.step(state, &gpu.device, &gpu.queue, &caps);
        if self.rig.busy(&state.library) {
            self.more = true;
        } else if let Some(wait) = self.rig.release_in(state.now) {
            cx.egui.request_repaint_after(wait);
        }
        // Playing textures ask for a frame at the rate of the game's ticks, and nothing else does.
        if let Some(wait) = self.texture_wake() {
            cx.egui.request_repaint_after(wait);
        }
    }

    /// Everything of a frame before the renderer: jobs, the UI, its requests and the core tick.
    /// False in safe mode, where nothing reads the document.
    fn update(&mut self, state: &mut State, ctx: &egui::Context, now: Instant) -> bool {
        state.step(now);
        for toast in state.toasts.drain(..) {
            if toast.level == Level::Error && self.errors.len() < 10 {
                self.errors.push(toast.text.clone());
            }
            self.ui.toast(
                match toast.level {
                    Level::Info => cuboid_ui::Level::Info,
                    Level::Warn => cuboid_ui::Level::Warning,
                    Level::Error => cuboid_ui::Level::Error,
                },
                toast.text,
            );
        }
        self.adopt_document(state);
        self.sync_recovery(state);
        self.poll_export(state);
        if std::mem::take(&mut self.tool_unsent) {
            self.send_tool(state);
        }
        if !state.safe_mode {
            self.step_textures(state);
        }

        let out = self.run_ui(state, ctx);
        for intent in prompts::show(ctx, state) {
            state.dispatch(intent);
        }
        self.window_commands
            .extend(out.window.into_iter().map(window_command));
        self.pointer.over_ui = out.pointer_over_ui;
        if out.title.is_some() {
            self.title = out.title;
        }
        if !out.requests.is_empty() {
            self.more = true;
        }
        if state.safe_mode {
            // Nothing that reads the document runs: only the recovery dialog and the file requests.
            for request in out.requests {
                self.apply(state, request);
            }
            return false;
        }
        self.placements = out.viewports;
        self.prepare_scene(state);
        for request in out.requests {
            self.apply(state, request);
        }
        self.send_texture_tick(state);
        self.step_player(state);
        // Picking sees the pose that the views show.
        self.sync_index(state.editor.core.doc());
        self.run_tick(state, ButtonState::default(), ButtonState::default());
        true
    }

    /// Moves the player to the clock of the frame and hands the extractor, the picking index and
    /// the core the pose when it changed. While the animation plays another frame follows at
    /// once; a paused or stopped player asks for none.
    pub(crate) fn step_player(&mut self, state: &mut State) {
        let animating = self.ui.mode() == Mode::Animate;
        let toggled = animating != self.animating;
        if toggled {
            self.animating = animating;
            self.player.animate(animating);
        }
        let core = &mut state.editor.core;
        self.player.tick(core.doc(), state.now);
        let moved = self.pose_sent != Some(self.player.pose_rev());
        if moved {
            self.pose_sent = Some(self.player.pose_rev());
            self.extractor.set_pose(self.player.pose());
        }
        // Picking sees the pose in Animate only: elsewhere a preview leaves the rest pose to edit.
        if moved || toggled {
            self.index
                .set_pose(self.player.pose().filter(|_| animating));
        }
        self.animations = self.player.view(core.doc());
        // The core works on the pose of the playhead while the Animate workspace is open.
        core.set_animating(animating.then(|| Playhead {
            animation: self.player.animation(),
            time: self.player.time(),
        }));
        self.timeline
            .update(core, self.player.animation(), self.player.time());
        if self.player.playing() {
            self.more = true;
        }
    }

    fn run_ui(&mut self, state: &mut State, ctx: &egui::Context) -> UiOutput {
        let mut recent: Vec<RecentEntry> = state
            .settings
            .recent_files
            .newest_first()
            .map(RecentEntry::from)
            .collect();
        for entry in &mut recent {
            entry.modified = self.recent_times.modified(&entry.path, state.now);
        }
        let paths: Vec<PathBuf> = recent.iter().map(|entry| entry.path.clone()).collect();
        for (entry, thumb) in recent.iter_mut().zip(state.library.recent_thumbs(&paths)) {
            entry.thumbnail = thumb;
        }
        if state.safe_mode {
            let cx = UiCx {
                window: self.window,
                ..UiCx::empty(&recent)
            };
            return self.ui.frame(ctx, &cx);
        }
        let query = state.editor.core.query();
        self.model.update(state, &query);
        let jobs: Vec<JobView> = state
            .doc_job
            .iter()
            .map(|job| JobView {
                id: 1,
                label: job.title.clone(),
                fraction: Some(job.handle.progress()),
                cancellable: true,
            })
            .chain(state.library.jobs())
            .collect();
        self.send_export_info(state);
        let wanted = self.validator_visible();
        let doc = state.editor.core.doc();
        let (target, label) = live_target(&state.editor.project, doc);
        let path = state.editor.path();
        let profile = check_profile(
            target,
            state.library.project_brief(),
            &model_name(path.as_deref()),
        );
        self.validation
            .step(doc, (profile, label), state.now, wanted);
        if self.export.is_checking()
            || wanted && (self.validation.is_busy() || self.validation.view().stale)
        {
            self.more = true;
        }
        self.ui
            .set_project(state.editor.project.view(state.editor.doc()));
        let views = self.view_frames();
        let band = self.band();
        let enabled = |id| query.action_enabled(id).unwrap_or(true);
        let model = &self.model;
        let doc = DocView {
            title: &model.title,
            path: model.path.as_deref(),
            dirty: state.editor.is_dirty(),
            nodes: &model.nodes,
            nodes_rev: model.nodes_rev,
            selection: &model.selection,
            primary: model.primary,
            props: model.props.as_ref(),
            textures: &model.textures,
            active_texture: (!model.textures.is_empty()).then_some(model.active_texture),
            texture_playing: self.textures.playing(),
            palette: &model.palette,
            history: &model.history,
            display: Some(&model.display),
            model: Some(&model.settings),
            animations: Some(&self.animations),
            timeline: Some(self.timeline.view()),
            status: &model.status,
        };
        let cx = UiCx {
            window: self.window,
            doc: Some(doc),
            recent: &recent,
            jobs: &jobs,
            views,
            band,
            tool_hot: self.tool_hot(state),
            enabled: Some(&enabled),
            library: Some(state.library.view()),
            validator: Some(self.validation.view()),
        };
        self.ui.frame(ctx, &cx)
    }

    /// The validator tab is shown: the live check runs only then.
    fn validator_visible(&self) -> bool {
        self.ui.validator_visible()
    }

    /// Tells the export dialog the game versions, the version of the preset and the folder to
    /// start in; again when the preset changes.
    fn send_export_info(&mut self, state: &State) {
        let version = project_version(&state.editor.project);
        if self.export_info_for == Some(version) {
            return;
        }
        self.export_info_for = Some(version);
        let folder = state.settings.path_export.clone().unwrap_or_default();
        self.ui.set_export_info(ExportInfo {
            versions: version_choices(),
            default_version: version.map(|v| v.to_string()).unwrap_or_default(),
            folder: PathBuf::from(folder),
        });
    }

    /// A plain left press at the pointer would start a tool action: it grabs a gizmo handle, or
    /// it is a stroke of a paint or voxel tool over the model. Otherwise the Blockbench-style
    /// camera orbits with the left button.
    fn tool_hot(&self, state: &State) -> bool {
        let core = &state.editor.core;
        if core.interaction().active_axis != Axes::NONE {
            return true;
        }
        if core.mode() != SelectMode::Custom {
            return false;
        }
        let pointer = &self.pointer;
        let (Some(slot), Some(pos)) = (pointer.slot, pointer.pos) else {
            return false;
        };
        let view = &self.views.views[slot];
        let ray = view.camera.pixel_ray(pos, view.rect);
        ray.is_some_and(|ray| self.index.pick_model(ray, 1000.0).is_some())
    }

    /// The views the UI can show: those that were rendered.
    fn view_frames(&self) -> [Option<ViewportFrame>; 4] {
        self.shown.map(|shown| {
            shown.map(|s| ViewportFrame {
                texture: Some(s.id),
                size_px: [s.size.0, s.size.1],
            })
        })
    }

    /// The rubber band of a box selection.
    fn band(&self) -> Option<BandView> {
        let rect = self.tick.select_box?;
        let slot = self.pointer.slot?;
        Some(BandView {
            slot: ViewSlot(u8::try_from(slot).unwrap_or(3)),
            corners: [
                Vec2::new(rect.x, rect.y),
                Vec2::new(rect.x + rect.w, rect.y + rect.h),
            ],
        })
    }

    /// A document put in place brings its cameras; a restore or a new document rebuilds the
    /// scene caches, because the dirty sets they trust belong to the old document.
    fn adopt_document(&mut self, state: &State) {
        let editor = &state.editor;
        if editor.doc_id() != self.doc_id {
            self.doc_id = editor.doc_id();
            self.views.apply_camera_states(&editor.doc().view.cameras);
            if let Some(grid) = editor.project.snap_grid() {
                self.ui.set_snap_grid(grid);
            }
            self.model.active_texture = 0;
            self.pointer = Pointer::default();
            // Another document has other animations: the rest pose is shown, and its animated
            // textures start at their first step.
            self.player = Player::default();
            self.textures = TextureClock::default();
            self.animating = false;
            // The timeline view is not reset: its revision must keep growing, or the timeline
            // could take the rows of the new document for those it built for the old one.
        }
        if editor.generation != self.generation {
            self.generation = editor.generation;
            self.extractor = Extractor::default();
            self.tick = TickOutput::default();
            // The new extractor has no pose yet.
            self.pose_sent = None;
        }
    }

    /// Opens the recovery dialog of the UI when the state asks for one.
    fn sync_recovery(&mut self, state: &State) {
        match &state.dialog {
            Some(Dialog::Recovery { candidates }) if !self.recovery_open => {
                let entries = candidates
                    .iter()
                    .map(|candidate| RecoveryEntry {
                        path: candidate.cubik.clone(),
                        name: candidate.title.clone(),
                        saved: saved_text(candidate.saved_ms),
                        note: if candidate.source_changed {
                            "The file was changed after this autosave.".to_owned()
                        } else {
                            candidate.reason.clone()
                        },
                    })
                    .collect();
                let message = if state.safe_mode {
                    "Cuboid hit repeated errors. Your work was saved."
                } else {
                    "Cuboid did not close properly last time. These autosaves were found."
                };
                self.ui.open_recovery(RecoveryInfo {
                    message: message.to_owned(),
                    entries,
                });
                self.recovery_open = true;
            }
            Some(Dialog::Recovery { .. }) => {}
            _ => self.recovery_open = false,
        }
    }

    /// Places the views in the pixels the UI gave them and brings the picking index up to date.
    fn prepare_scene(&mut self, state: &State) {
        for view in &mut self.views.views {
            view.rect = Rect::default();
        }
        for placement in &self.placements {
            let view = &mut self.views.views[slot_index(placement.slot)];
            view.rect = Rect {
                x: 0.0,
                y: 0.0,
                w: placement.size_px[0] as f32,
                h: placement.size_px[1] as f32,
            };
        }
        self.sync_index(state.editor.core.doc());
    }

    fn sync_index(&mut self, doc: &cuboid_model::Document) {
        self.index.sync(doc, &doc.journal);
        let cameras = self.views.views.each_ref().map(|view| view.camera);
        self.index.set_cameras(cameras, self.views.active);
    }

    /// Runs the interaction of the core for the pointer as it is, with the given button
    /// transitions, and applies what comes out.
    pub fn run_tick(&mut self, state: &mut State, left: ButtonState, right: ButtonState) {
        let pointer = &self.pointer;
        let view = pointer
            .slot
            .map(|slot| &self.views.views[slot])
            .filter(|view| !view.rect.is_empty())
            .map(|view| PointerView {
                id: view.id,
                rect: view.rect,
                view_proj: view.camera.view_proj(view.rect.aspect()),
                ray: pointer
                    .pos
                    .and_then(|pos| view.camera.pixel_ray(pos, view.rect)),
            });
        let held = pointer.left || pointer.right;
        let input = InputFrame {
            now: state.now,
            pointer: pointer.pos,
            left: if left == ButtonState::default() && pointer.left {
                ButtonState::HELD
            } else {
                left
            },
            right: if right == ButtonState::default() && pointer.right {
                ButtonState::HELD
            } else {
                right
            },
            mods: pointer.mods,
            // A drag outside the view still belongs to the view.
            over_ui: pointer.over_ui && !held,
            view,
        };
        let core = &mut state.editor.core;
        let out = core.tick(&input, &self.index);
        apply_colliders(core, &mut self.index, &out.effects.colliders);
        for notice in &out.effects.notices {
            state.toast(Level::Info, notice.text.clone());
        }
        if state.editor.core.interaction().busy() {
            self.more = true;
        }
        self.tick = out;
    }

    /// Extract, upload, draw lists, render and registration of the view textures.
    fn render(&mut self, state: &mut State, cx: &FrameCx<'_>) {
        let gpu = cx.gpu;
        let Some(glue) = self.glue.as_mut() else {
            return;
        };
        let core = &mut state.editor.core;
        let prefs = core.env_mut().render.clone();
        let doc = core.doc();
        let voxel_cell = doc
            .voxels
            .as_ref()
            .map_or(1.0, |world| 16.0 / world.res.max(1) as f32);
        let grid_step = doc.view.display_step;
        let grid_step = if grid_step.is_finite() && grid_step > 0.0 {
            grid_step
        } else {
            1.0
        };
        let current = glue.renderer.settings();
        let viewport = Tokens::of(cx.egui).bg_viewport;
        let wanted = state.editor.project.render(RenderSettings {
            msaa: prefs.antialiasing,
            voxel_cell,
            grid_step,
            clear: [viewport.r(), viewport.g(), viewport.b()],
            ..current
        });
        if wanted != current {
            glue.renderer.set_settings(wanted);
        }
        // The Display workspace shows the model as the inventory lights it (its `gui_light`).
        let gui_light = (self.ui.mode() == Mode::Display)
            .then(|| display_light(true, doc.props.minecraft.gui_light));

        // A selected group is outlined through the objects below it.
        let mut selected: Vec<ObjRef> = Vec::new();
        for &obj in &core.selection().list {
            match obj {
                ObjRef::Group(id) => selected.extend(
                    doc.groups
                        .descendants(&[id])
                        .into_iter()
                        .filter(|n| matches!(n, ObjRef::Element(_) | ObjRef::Mesh(_))),
                ),
                other => selected.push(other),
            }
        }
        // The orange outline of a hovered mesh; a selected mesh has its own.
        let hovered_mesh = match self.tick.hovered {
            Some(ObjRef::Mesh(id)) if !selected.contains(&ObjRef::Mesh(id)) => Some(id),
            _ => None,
        };
        glue.renderer.set_hovered_mesh(hovered_mesh);
        let base = OverlayState {
            model_origin: selected.contains(&ObjRef::ModelOrigin),
            selected,
            hovered: self.tick.hovered,
            gizmo: None,
            voxel_pointer: None,
            grid: true,
            model_box: self.ui.mode() == Mode::Voxel,
        };
        self.extractor.cache_mut().set_overlay(base.clone());
        self.index.sync(core.doc(), &core.doc().journal);
        let extract =
            self.extractor
                .extract(core.doc_for_extract(), &prefs, &ExtractBudget::default());
        // The index and the extractor have read the journal.
        core.clear_journal();
        glue.renderer
            .sync(&gpu.queue, &extract, UploadBudget::default());
        if self.extractor.has_leftovers() || glue.renderer.needs_frame() {
            self.more = true;
        }

        let mut lists: [Option<ViewDrawList>; 4] = Default::default();
        for placement in &self.placements {
            let i = slot_index(placement.slot);
            if lists[i].is_some() {
                continue;
            }
            let view = &self.views.views[i];
            let overlay = OverlayState {
                gizmo: self
                    .tick
                    .gizmo
                    .as_ref()
                    .and_then(|gizmo| gizmo_overlay(gizmo, &view.camera)),
                ..base.clone()
            };
            self.extractor.cache_mut().set_overlay(overlay.clone());
            let mut list = compile_draw_list(view, self.extractor.cache(), &prefs);
            // The scene numbers the overlay state in the order it was set; this key depends on
            // the content alone, so a view whose overlay did not change keeps its picture.
            list.key.overlay = overlay_key(&overlay);
            lists[i] = Some(list);
        }
        let inputs: [Option<ViewInput<'_>>; 4] = std::array::from_fn(|i| {
            let list = lists[i].as_ref()?;
            let placement = self.placements.iter().find(|p| slot_index(p.slot) == i)?;
            let size = (placement.size_px[0], placement.size_px[1]);
            let params = ViewParams::from_camera(
                &self.views.views[i].camera,
                u8::try_from(i).unwrap_or(3),
                size,
            );
            Some(ViewInput {
                params: gui_light.map_or(params, |light| params.with_item_light(light)),
                list,
            })
        });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cuboid_views"),
            });
        let textures = glue.renderer.render_views(&mut encoder, &inputs);
        gpu.queue.submit([encoder.finish()]);

        let mut egui_renderer = gpu.renderer.write();
        for (i, texture) in textures.iter().enumerate() {
            if inputs[i].is_none() {
                continue;
            }
            let shown = &mut self.shown[i];
            match shown {
                Some(s) if s.generation == texture.generation => {}
                Some(s) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(
                        &gpu.device,
                        &texture.view,
                        wgpu::FilterMode::Linear,
                        s.id,
                    );
                    s.generation = texture.generation;
                    s.size = texture.size;
                }
                None => {
                    let id = egui_renderer.register_native_texture(
                        &gpu.device,
                        &texture.view,
                        wgpu::FilterMode::Linear,
                    );
                    *shown = Some(Shown {
                        id,
                        generation: texture.generation,
                        size: texture.size,
                    });
                }
            }
        }
    }
}

/// The shell's command for what the title bar asked of the window.
fn window_command(request: WindowRequest) -> WindowCommand {
    match request {
        WindowRequest::Move => WindowCommand::Move,
        WindowRequest::ToggleMaximize => WindowCommand::ToggleMaximize,
        WindowRequest::Minimize => WindowCommand::Minimize,
    }
}

pub fn slot_index(slot: ViewSlot) -> usize {
    usize::from(slot.0).min(3)
}

pub fn view_id(slot: ViewSlot) -> ViewId {
    ViewId::ALL[slot_index(slot)]
}

fn saved_text(ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms).map_or_else(
        |_| String::new(),
        |stamp| {
            stamp
                .to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        },
    )
}

/// What the core asked of the voxel colliders of the index during a tick.
fn apply_colliders(core: &Core, index: &mut SceneIndex, actions: &[ColliderAction]) {
    for action in actions {
        match action {
            ColliderAction::Freeze => index.freeze_voxel_colliders(),
            ColliderAction::Refresh => {
                if let Some(world) = &core.doc().voxels {
                    index.refresh_voxel_colliders(world);
                }
            }
        }
    }
}

/// The transform of the gizmo overlay in a view. The scene's draw list takes a matrix; the
/// gizmo keeps its size on the screen, so the scale follows the camera of the view.
fn gizmo_overlay(gizmo: &GizmoState, camera: &Camera) -> Option<(GizmoKind, Mat4)> {
    let kind = gizmo.kind?;
    if !(gizmo.origin.is_finite() && gizmo.size.is_finite()) {
        return None;
    }
    let f = gizmo_scale(camera, gizmo.origin);
    let matrix = match kind {
        // The size factors of the hit areas in the scene crate: arrows 0.6, rings 0.7.
        GizmoKind::Move => {
            Mat4::from_translation(gizmo.origin) * Mat4::from_scale(Vec3::splat(f * 0.6))
        }
        GizmoKind::Rotate => {
            Mat4::from_translation(gizmo.origin) * Mat4::from_scale(Vec3::splat(f * 0.7))
        }
        GizmoKind::Scale => {
            let root = gizmo.origin + gizmo.rotation * (gizmo.gizmo_pivot * gizmo.size);
            Mat4::from_scale_rotation_translation(
                gizmo.size * gizmo.gizmo_size,
                gizmo.rotation,
                root,
            )
        }
    };
    matrix.is_finite().then_some((kind, matrix))
}

/// A hash of the content of an overlay state, for the frame key of a view.
fn overlay_key(overlay: &OverlayState) -> u64 {
    let mut hasher = DefaultHasher::new();
    overlay.selected.hash(&mut hasher);
    overlay.hovered.hash(&mut hasher);
    if let Some((kind, matrix)) = overlay.gizmo {
        kind.hash(&mut hasher);
        matrix.to_cols_array().map(f32::to_bits).hash(&mut hasher);
    }
    overlay.voxel_pointer.is_some().hash(&mut hasher);
    (overlay.grid, overlay.model_box, overlay.model_origin).hash(&mut hasher);
    hasher.finish()
}

/// The select mode of the core that goes with a tool of the model workspace.
pub fn select_mode(tool: Tool) -> SelectMode {
    match tool {
        Tool::Rotate => SelectMode::Rotate,
        Tool::Scale => SelectMode::Scale,
        _ => SelectMode::Move,
    }
}

#[cfg(test)]
impl Frontend {
    /// The view slots whose picture the UI can show.
    pub fn rendered_slots(&self) -> Vec<usize> {
        (0..4).filter(|&i| self.shown[i].is_some()).collect()
    }

    /// What the renderer says about itself, once it exists.
    pub fn render_health(&self) -> Option<cuboid_render::Health> {
        self.glue.as_ref().map(|glue| glue.renderer.health())
    }
}

mod animate_edit;
#[cfg(test)]
mod export_tests;
#[cfg(test)]
mod library_tests;
mod playback;
mod render_job;
#[cfg(test)]
mod tests;
mod textures;
mod timeline;
