//! The window of the app each frame: the UI of `cuboid-ui`, what it asks for, the scene index,
//! the extract, the renderer and the view textures the UI shows (spec: arch 10.4, 13.1).
//!
//! One frame runs: jobs and intents ([`State::step`]), the UI, the requests of the UI (file
//! requests become intents, edits become core commands, pointer events become navigation and
//! core ticks), then the scene extract, the upload, the draw lists and the render into the view
//! textures. The UI draws the textures of the frame before; the painter of the shell reads them
//! after the frame returns, so they are current when they reach the screen.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use cuboid_core::{
    ButtonState, ColliderAction, Core, InputFrame, Modifiers, SelectMode, TickOutput,
    ViewInput as PointerView,
};
use cuboid_model::ObjRef;
use cuboid_render::{RenderSettings, UploadBudget, ViewInput, ViewParams};
use cuboid_scene::{
    Camera, ExtractBudget, Extractor, GizmoKind, GizmoState, OverlayState, Rect, SceneIndex,
    ViewDrawList, ViewId, ViewSet, compile_draw_list, gizmo_scale,
};
use cuboid_shell::FrameCx;
use cuboid_ui::{
    BandView, DocView, FontSet, JobView, Mode, Prefs, RecentEntry, RecoveryEntry, RecoveryInfo,
    Tool, ToolOptions, Ui, UiCx, UiOutput, ViewSlot, ViewportFrame, ViewportPlacement,
};
use glam::{Mat4, Vec2, Vec3};

use crate::prompts;
use crate::render_glue::RenderGlue;
use crate::state::{Dialog, Level, State};
use crate::uimodel::Model;

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

pub struct Frontend {
    pub ui: Ui,
    pub views: ViewSet,
    pub index: SceneIndex,
    extractor: Extractor,
    glue: Option<RenderGlue>,
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
    /// Another frame is needed at once: uploads or extraction are unfinished, or a drag runs.
    pub more: bool,
}

impl Frontend {
    pub fn new(prefs: Prefs, fonts: FontSet, prefs_path: PathBuf) -> Frontend {
        let options = prefs.tool_options;
        let ui = Ui::new(prefs, fonts);
        let tool = ui.tool();
        Frontend {
            ui,
            views: ViewSet::default(),
            index: SceneIndex::default(),
            extractor: Extractor::default(),
            glue: None,
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
            more: false,
        }
    }

    pub fn prefs_path(&self) -> &std::path::Path {
        &self.prefs_path
    }

    /// The window title, once, when it changed.
    pub fn take_title(&mut self) -> Option<String> {
        self.title.take()
    }

    /// One frame of the window. Runs inside the panic boundary of the app.
    pub fn frame(&mut self, state: &mut State, cx: &FrameCx<'_>) {
        let gpu = cx.gpu;
        self.more = false;
        let glue = self.glue.get_or_insert_with(|| RenderGlue::new(gpu));
        if glue.follow_device(gpu) {
            self.shown = [None; 4];
        }
        if glue.take_device_lost() {
            state.on_device_lost();
        }
        state.step(cx.now);
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
        if std::mem::take(&mut self.tool_unsent) {
            self.send_tool(state);
        }

        let out = self.run_ui(state, cx.egui);
        for intent in prompts::show(cx.egui, state) {
            state.dispatch(intent);
        }
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
            return;
        }
        self.placements = out.viewports;
        self.prepare_scene(state);
        for request in out.requests {
            self.apply(state, request);
        }
        self.run_tick(state, ButtonState::default(), ButtonState::default());
        self.render(state, cx);
        state.editor.cameras = Some(self.views.camera_states());
    }

    fn run_ui(&mut self, state: &mut State, ctx: &egui::Context) -> UiOutput {
        let recent: Vec<RecentEntry> = state
            .settings
            .recent_files
            .newest_first()
            .map(RecentEntry::from)
            .collect();
        if state.safe_mode {
            return self.ui.frame(ctx, &UiCx::empty(&recent));
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
            .collect();
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
            palette: &model.palette,
            history: &model.history,
            display: None,
            status: &model.status,
        };
        let cx = UiCx {
            doc: Some(doc),
            recent: &recent,
            jobs: &jobs,
            views,
            band,
            enabled: Some(&enabled),
        };
        self.ui.frame(ctx, &cx)
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
            self.model.active_texture = 0;
            self.pointer = Pointer::default();
        }
        if editor.generation != self.generation {
            self.generation = editor.generation;
            self.extractor = Extractor::default();
            self.tick = TickOutput::default();
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
        let wanted = RenderSettings {
            msaa: prefs.antialiasing,
            voxel_cell,
            grid_step,
            ..current
        };
        if wanted != current {
            glue.renderer.set_settings(wanted);
        }

        let selected: Vec<ObjRef> = core.selection().list.iter().copied().collect();
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
            Some(ViewInput {
                params: ViewParams::from_camera(
                    &self.views.views[i].camera,
                    u8::try_from(i).unwrap_or(3),
                    size,
                ),
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
