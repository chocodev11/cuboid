//! What the UI asks for, applied to the app: file requests become intents and flows, edits and
//! actions become core commands, pointer events become camera moves and core ticks.

use cuboid_actions::ActionId;
use cuboid_core::{
    AutoUvStyle, ButtonState, Command, DisplayContext, FaceId, Modifiers, PaintOptions, PaintTool,
    PaintToolKind, SelectOp, Target, UvAxis, VoxelOptions, VoxelTool, VoxelToolKind,
};
use cuboid_model::{DisplaySlotId, ObjRef};
use cuboid_num::pack::to_byte;
use cuboid_scene::{StandardView, ViewSet, frame_bounds, orbit, pan, set_ortho, snap_view, zoom};
use cuboid_shell::WindowCommand;
use cuboid_ui::{
    AnimationRequest, Button, ColorSlot, Edit, ElementFlag, Face, FaceChange, KeyMods,
    LibraryRequest, Mode, Prefs, ProjectChange, RecoveryChoice, SelectOp as UiSelectOp, Tool,
    ToolOptions, TransformField, UiRequest, ViewPreset, ViewSlot, ViewportEvent, action_label,
};

use crate::flows::PickPurpose;
use crate::frontend::{Frontend, select_mode, slot_index, view_id};
use crate::jobs::JobError;
use crate::state::{Dialog, Intent, Level, Pending, State};
use crate::ui_setup::write_prefs;
use crate::uimodel::{DISPLAY_SLOTS, components, object_of, target_of, texture_size};

/// A request that would drop unsaved changes arrives after the UI asked about them. The pending
/// action that goes with it, when it has one.
fn pending_of(request: &UiRequest) -> Option<Pending> {
    match request {
        UiRequest::NewDocument(template) => Some(Pending::Template(*template)),
        UiRequest::OpenDialog => Some(Pending::PickOpen),
        UiRequest::Open(path) => Some(Pending::Open(path.clone())),
        UiRequest::Quit => Some(Pending::Quit),
        _ => None,
    }
}

fn face_id(face: Face) -> FaceId {
    match face {
        Face::Up => FaceId::Up,
        Face::Down => FaceId::Down,
        Face::North => FaceId::North,
        Face::South => FaceId::South,
        Face::West => FaceId::West,
        Face::East => FaceId::East,
    }
}

fn to_core_mods(mods: KeyMods) -> Modifiers {
    Modifiers {
        shift: mods.shift,
        ctrl: mods.ctrl,
        alt: mods.alt,
    }
}

fn color_bytes(color: [f32; 4]) -> [u8; 4] {
    color.map(to_byte)
}

/// The display context behind a slot index of the Display tab.
fn display_slot(index: usize) -> Option<DisplayContext> {
    DISPLAY_SLOTS.get(index).map(|(context, _)| *context)
}

impl Frontend {
    pub fn apply(&mut self, state: &mut State, request: UiRequest) {
        match request {
            UiRequest::Action(id) => self.action(state, id),
            UiRequest::NewDocument(_)
            | UiRequest::OpenDialog
            | UiRequest::Open(_)
            | UiRequest::Quit => {
                if let Some(pending) = pending_of(&request) {
                    state.confirmed(pending);
                }
            }
            UiRequest::Save => state.dispatch(Intent::Save),
            UiRequest::SaveAs => state.dispatch(Intent::SaveAs(None)),
            UiRequest::ExportDialog => state.dispatch(Intent::ExportCopy(None)),
            UiRequest::SaveThen(inner) => {
                if let Some(pending) = pending_of(&inner) {
                    state.save_then(pending);
                }
            }
            UiRequest::RemoveRecent(path) => {
                state
                    .settings
                    .recent_files
                    .remove(&path.display().to_string());
                state.settings_dirty = true;
            }
            UiRequest::Edit(edit) => self.edit(state, edit),
            UiRequest::Viewport(event) => self.viewport(state, event),
            UiRequest::SetMode(mode) => self.set_mode(state, mode),
            UiRequest::SetTool(tool) => {
                self.tool = tool;
                self.send_tool(state);
            }
            UiRequest::SetToolOptions(options) => {
                self.options = options;
                self.send_tool(state);
            }
            UiRequest::HistoryJump(position) => history_jump(state, position),
            UiRequest::Animation(request) => {
                self.player.request(state.editor.core.doc(), request);
            }
            UiRequest::Animate(edit) => self.animate_edit(state, edit),
            UiRequest::PrefsChanged(prefs) => self.prefs_changed(state, &prefs),
            UiRequest::Recovery(choice) => recovery(state, choice),
            UiRequest::CancelJob(id) if id == crate::library::JOB_ID => state.library.cancel(),
            UiRequest::CancelJob(_) => state.dispatch(Intent::CancelJob),
            UiRequest::Library(request) => self.library_request(state, request),
            UiRequest::Validator(request) => self.validator_request(state, request),
            UiRequest::Export(request) => self.export_request(state, request),
            UiRequest::Render(request) => self.render_request(state, request),
            UiRequest::BrowseFolder => state.pick(PickPurpose::Folder),
            UiRequest::OpenFolder(path) => self.reveal(&path),
            UiRequest::Project(change) => self.project_changed(state, change),
        }
    }

    /// A change of the preset or the lighting. A new preset also brings the snap step it asks
    /// for.
    fn project_changed(&mut self, state: &mut State, change: ProjectChange) {
        let preset = matches!(
            change,
            ProjectChange::SetPreset(_) | ProjectChange::SetVersion(_)
        );
        if let Some(note) = state.editor.change_project(change) {
            state.toast(Level::Warn, note);
        }
        if preset && let Some(grid) = state.editor.project.snap_grid() {
            self.ui.set_snap_grid(grid);
        }
    }

    /// The requests of the library panel and the start screen. The service handles most of
    /// them; the two that reach into the document or the views are answered here.
    fn library_request(&mut self, state: &mut State, request: LibraryRequest) {
        match request {
            LibraryRequest::UseTexture(id) => crate::library::use_texture(state, &id),
            LibraryRequest::PinReference(_) => {
                Self::not_yet(state, "Pinning a reference to a view");
            }
            other => state.library.apply(other),
        }
    }

    fn not_yet(state: &mut State, what: &str) {
        state.toast(Level::Info, format!("{what} is not available yet."));
    }

    fn action(&mut self, state: &mut State, id: ActionId) {
        match id {
            ActionId::AddTexture => state.run_command(Command::CreateTexture {
                label: None,
                width: 16,
                height: 16,
                color: [255, 255, 255, 255],
            }),
            ActionId::VoxelSetup => state.run_command(Command::SetupVoxels { resolution: 16 }),
            ActionId::ResetCamera => self.views.reset_views(),
            ActionId::ZoomPlus | ActionId::ZoomMinus => {
                let notches = if id == ActionId::ZoomPlus { 1.0 } else { -1.0 };
                let ViewSet {
                    views,
                    pivot,
                    active,
                    ..
                } = &mut self.views;
                zoom(&mut views[active.index()].camera, *pivot, notches);
            }
            ActionId::WireframeMode => {
                let render = &mut state.editor.core.env_mut().render;
                render.wireframe = !render.wireframe;
            }
            ActionId::ClearRecentFiles => {
                state.settings.recent_files.list.clear();
                state.settings_dirty = true;
            }
            other => match Command::from_action(other) {
                Some(command) => state.run_command(command),
                None => Self::not_yet(state, &action_label(other)),
            },
        }
    }

    fn edit(&mut self, state: &mut State, edit: Edit) {
        let doc = state.editor.core.doc();
        let target = |node| target_of(doc, node);
        // A continuous edit is one undo step; the committed value ends it.
        let mut commit = true;
        let command = match edit {
            Edit::Select { ids, op } => {
                let targets: Vec<Target> = ids.into_iter().filter_map(target).collect();
                match (op, targets.is_empty()) {
                    (UiSelectOp::Replace, true) => Some(Command::SelectNone),
                    (_, true) => None,
                    (op, false) => Some(Command::Select {
                        targets,
                        op: match op {
                            UiSelectOp::Replace => SelectOp::Replace,
                            UiSelectOp::Add => SelectOp::Add,
                            UiSelectOp::Toggle => SelectOp::Toggle,
                        },
                    }),
                }
            }
            Edit::Rename { id, name } => target(id).map(|t| Command::Rename {
                targets: vec![t],
                name,
            }),
            Edit::SetVisible { id, visible } => target(id).map(|t| Command::SetHidden {
                targets: vec![t],
                hidden: !visible,
            }),
            Edit::SetLocked { id, locked } => target(id).map(|t| Command::SetLocked {
                targets: vec![t],
                locked,
            }),
            Edit::Reparent {
                ids,
                parent,
                before,
            } => {
                let moved: Vec<ObjRef> = ids.iter().filter_map(|&id| object_of(doc, id)).collect();
                let parent_group = parent.and_then(|id| match object_of(doc, id) {
                    Some(ObjRef::Group(group)) => Some(group),
                    _ => None,
                });
                // The place is counted among the children that stay.
                let index = before
                    .and_then(|id| object_of(doc, id))
                    .and_then(|sibling| {
                        doc.child_nodes(parent_group)
                            .into_iter()
                            .filter(|child| !moved.contains(child))
                            .position(|child| child == sibling)
                    });
                // A parent that is no group is not a drop target.
                (parent.is_none() || parent_group.is_some()).then(|| Command::Reparent {
                    targets: ids.into_iter().filter_map(target).collect(),
                    parent: parent.and_then(target),
                    index,
                })
            }
            Edit::GroupSelection => Some(Command::Group {
                targets: Vec::new(),
                name: "Group".to_owned(),
            }),
            Edit::Ungroup => Some(Command::Ungroup {
                targets: Vec::new(),
            }),
            Edit::SelectChildren { id } => match object_of(doc, id) {
                Some(ObjRef::Group(group)) => {
                    let children: Vec<Target> = doc
                        .child_nodes(Some(group))
                        .into_iter()
                        .filter_map(|child| state.editor.core.target_of(child))
                        .collect();
                    (!children.is_empty()).then_some(Command::Select {
                        targets: children,
                        op: SelectOp::Replace,
                    })
                }
                _ => None,
            },
            Edit::SetExport { id, export } => target(id).map(|group| Command::SetGroup {
                group,
                name: None,
                origin: None,
                visible: None,
                locked: None,
                export: Some(export),
            }),
            Edit::Transform {
                field,
                value,
                commit: done,
            } => {
                commit = done;
                match field {
                    TransformField::Position => Some(Command::SetPosition {
                        targets: Vec::new(),
                        value: components(value),
                    }),
                    TransformField::Size => Some(Command::SetSize {
                        targets: Vec::new(),
                        value: components(value),
                    }),
                    TransformField::Rotation => Some(Command::SetRotation {
                        targets: Vec::new(),
                        value: components(value),
                    }),
                    TransformField::Pivot => Some(Command::SetPivot {
                        targets: Vec::new(),
                        value: components(value),
                    }),
                }
            }
            Edit::SetColor {
                color,
                commit: done,
            } => {
                commit = done;
                Some(Command::SetColor {
                    targets: Vec::new(),
                    color: [color[0], color[1], color[2]],
                })
            }
            Edit::SetFlag { flag, on } => match flag {
                ElementFlag::UvSnap => Some(Command::SetUvSnap {
                    targets: Vec::new(),
                    enabled: on,
                    mirror: false,
                }),
                ElementFlag::Shade => Some(Command::SetShade {
                    targets: Vec::new(),
                    enabled: on,
                }),
            },
            Edit::Face { face, change } => {
                let faces = vec![face_id(face)];
                match change {
                    FaceChange::Texture(label) => Some(Command::AssignTexture {
                        targets: Vec::new(),
                        label,
                        faces,
                    }),
                    FaceChange::Uv { uv, commit: done } => {
                        commit = done;
                        self.face_uv(state, face, uv)
                            .map(|rect| Command::SetUvRect {
                                targets: Vec::new(),
                                faces,
                                rect,
                                rotation: None,
                            })
                    }
                    FaceChange::AutoUv => Some(Command::AutoUv {
                        targets: Vec::new(),
                        style: AutoUvStyle::Box,
                        faces,
                    }),
                    FaceChange::RotateUv => Some(Command::RotateUv {
                        targets: Vec::new(),
                        faces,
                        degrees: 90,
                    }),
                    FaceChange::FlipU => Some(Command::FlipUv {
                        targets: Vec::new(),
                        faces,
                        axis: UvAxis::Horizontal,
                    }),
                    FaceChange::FlipV => Some(Command::FlipUv {
                        targets: Vec::new(),
                        faces,
                        axis: UvAxis::Vertical,
                    }),
                    FaceChange::Rotation(degrees) => {
                        let current = self
                            .model
                            .props
                            .as_ref()
                            .and_then(|props| props.faces.as_ref())
                            .map(|all| all[face.index()].uv);
                        current
                            .and_then(|uv| self.face_uv(state, face, uv))
                            .map(|rect| Command::SetUvRect {
                                targets: Vec::new(),
                                faces,
                                rect,
                                rotation: Some(degrees),
                            })
                    }
                    FaceChange::Enabled(enabled) => Some(Command::SetFaceEnabled {
                        targets: Vec::new(),
                        faces,
                        enabled,
                    }),
                    FaceChange::Tint(tint) => Some(Command::SetFaceTint {
                        targets: Vec::new(),
                        faces,
                        tint,
                    }),
                    FaceChange::Cull(cull) => Some(Command::SetFaceCull {
                        targets: Vec::new(),
                        faces,
                        cull: cull.map(face_id),
                    }),
                }
            }
            Edit::SelectTexture(index) => {
                self.model.active_texture = index;
                None
            }
            Edit::ReloadTexture(index) => {
                state.reload_texture(index);
                None
            }
            Edit::SetToolColor { slot, color } => {
                set_tool_color(state, slot, color);
                None
            }
            Edit::PickPalette { index, slot } => {
                if let Some(color) = self.model.palette.colors.get(index) {
                    set_tool_color(state, slot, *color);
                }
                None
            }
            Edit::AddPaletteColor(color) => Some(Command::PaletteAdd {
                color: color_bytes(color),
            }),
            Edit::RemovePaletteColor(index) => {
                self.model
                    .palette
                    .colors
                    .get(index)
                    .map(|color| Command::PaletteRemove {
                        color: color_bytes(*color),
                    })
            }
            Edit::SelectDisplaySlot(slot) => {
                self.model.display_active = slot;
                None
            }
            Edit::SetDisplaySlot {
                slot,
                rotation,
                translation,
                scale,
                commit: done,
            } => {
                commit = done;
                display_slot(slot).map(|slot| Command::SetDisplaySlot {
                    slot,
                    enabled: None,
                    rotation: Some(rotation.to_array()),
                    translation: Some(translation.to_array()),
                    scale: Some(scale.to_array()),
                    preset: None,
                })
            }
            Edit::SetDisplayEnabled { slot, enabled } => {
                display_slot(slot).map(|slot| Command::SetDisplaySlot {
                    slot,
                    enabled: Some(enabled),
                    rotation: None,
                    translation: None,
                    scale: None,
                    preset: None,
                })
            }
            Edit::LoadDisplayPreset { slot, preset } => {
                let loaded = display_slot(slot).and_then(|context| {
                    let parent = cuboid_fmt_minecraft::builtin_display(preset.id())?;
                    let values = &parent[DisplaySlotId::from(context)];
                    values.enabled.then(|| Command::SetDisplaySlot {
                        slot: context,
                        enabled: Some(true),
                        rotation: Some(values.rotation.to_array()),
                        translation: Some(values.translation.to_array()),
                        scale: Some(values.scale.to_array()),
                        preset: Some(preset.id().to_owned()),
                    })
                });
                if loaded.is_none() {
                    state.toast(
                        Level::Info,
                        format!("{} has no values for this slot.", preset.id()),
                    );
                }
                loaded
            }
            Edit::SetModelOrigin {
                value,
                commit: done,
            } => {
                commit = done;
                Some(Command::SetPosition {
                    targets: vec![Target::ModelOrigin],
                    value: components(value),
                })
            }
            Edit::SetBlueprint {
                value,
                commit: done,
            } => {
                commit = done;
                Some(Command::SetPosition {
                    targets: vec![Target::Blueprint],
                    value: components(value),
                })
            }
        };
        let Some(command) = command else {
            return;
        };
        if matches!(command, Command::Select { .. } | Command::SelectNone) {
            state.run_command(command);
            return;
        }
        state.run_merged(command, self.gesture);
        if commit {
            self.gesture += 1;
        }
    }

    /// The UV rectangle of the UI (texels, rows from the top) in the texture coordinates of the
    /// commands (0 to 1, V up) for the face of the primary element.
    fn face_uv(&self, state: &State, face: Face, uv: [f32; 4]) -> Option<[f32; 4]> {
        let props = self.model.props.as_ref()?;
        let label = &props.faces.as_ref()?[face.index()].texture;
        let [w, h] = texture_size(state.editor.doc(), label);
        Some([uv[0] / w, 1.0 - uv[1] / h, uv[2] / w, 1.0 - uv[3] / h])
    }

    pub fn set_mode(&mut self, state: &mut State, mode: Mode) {
        if mode == Mode::Display {
            state.editor.core.set_tool(None);
        }
    }

    /// Gives the core the tool and the options the UI has.
    pub fn send_tool(&mut self, state: &mut State) {
        let core = &mut state.editor.core;
        let ToolOptions { brush, mirror } = self.options;
        let brush = i32::try_from(brush).unwrap_or(1);
        let voxel = |kind| {
            let options = VoxelOptions {
                brush,
                mirror,
                ..VoxelOptions::default()
            };
            Some(Box::new(VoxelTool::new(kind, options)) as Box<dyn cuboid_core::CustomTool>)
        };
        let paint = |kind| {
            let options = PaintOptions {
                brush,
                ..PaintOptions::default()
            };
            Some(Box::new(PaintTool::new(kind, options)) as Box<dyn cuboid_core::CustomTool>)
        };
        let custom = match self.tool {
            Tool::Select | Tool::Move | Tool::Rotate | Tool::Scale => None,
            Tool::VoxelPencil => voxel(VoxelToolKind::Pencil),
            Tool::VoxelErase => voxel(VoxelToolKind::Erase),
            Tool::VoxelFill => voxel(VoxelToolKind::Fill),
            Tool::VoxelBox => voxel(VoxelToolKind::Box),
            Tool::VoxelLine => voxel(VoxelToolKind::Line),
            Tool::VoxelPicker => voxel(VoxelToolKind::Picker),
            Tool::PaintPencil => paint(PaintToolKind::Pencil),
            Tool::PaintEraser => paint(PaintToolKind::Eraser),
            Tool::PaintFill => paint(PaintToolKind::Fill),
            Tool::PaintPicker => paint(PaintToolKind::Picker),
        };
        let custom_tool = custom.is_some();
        core.set_tool(custom);
        if !custom_tool {
            core.set_mode(select_mode(self.tool));
        }
    }

    fn prefs_changed(&mut self, state: &mut State, prefs: &Prefs) {
        let snap = state.editor.core.snap_mut();
        snap.enabled = prefs.snap.enabled;
        snap.grid = if prefs.snap.whole_units {
            1.0
        } else {
            prefs.snap.grid
        };
        snap.angle = prefs.snap.rotation;
        state.editor.core.env_mut().render.antialiasing = prefs.antialiasing;
        if prefs.system_title_bar != self.system_title_bar {
            self.system_title_bar = prefs.system_title_bar;
            self.window_commands
                .push(WindowCommand::SetDecorations(prefs.system_title_bar));
        }
        let minutes = i32::try_from(prefs.autosave_minutes).unwrap_or(10);
        if state.settings.backup_every_minutes != minutes {
            state.settings.backup_every_minutes = minutes;
            state.settings_dirty = true;
        }
        let path = self.prefs_path().to_owned();
        let text = prefs.to_json();
        drop(state.io.submit(move |_| {
            write_prefs(&path, &text).map_err(|error| JobError::Failed(error.to_string()))
        }));
    }

    fn viewport(&mut self, state: &mut State, event: ViewportEvent) {
        match event {
            ViewportEvent::Hover { slot, pos } => {
                if self.pointer.left || self.pointer.right {
                    return;
                }
                self.pointer.slot = pos.map(|_| slot_index(slot));
                self.pointer.pos = pos;
            }
            ViewportEvent::Orbit { slot, delta } => {
                let i = slot_index(slot);
                let ViewSet { views, pivot, .. } = &mut self.views;
                orbit(&mut views[i].camera, *pivot, delta.x, delta.y);
            }
            ViewportEvent::Pan { slot, delta } => {
                let i = slot_index(slot);
                let height = self.views.views[i].rect.h.max(1.0);
                let ViewSet { views, pivot, .. } = &mut self.views;
                pan(&mut views[i].camera, pivot, delta.x, delta.y, height);
            }
            ViewportEvent::Zoom { slot, amount, .. } => {
                let i = slot_index(slot);
                let ViewSet { views, pivot, .. } = &mut self.views;
                zoom(&mut views[i].camera, *pivot, amount);
            }
            ViewportEvent::SetView { slot, preset } => self.set_view(slot, preset),
            ViewportEvent::Frame { slot } => self.frame_view(state, slot),
            ViewportEvent::Activate { slot } => self.views.active = view_id(slot),
            ViewportEvent::Press {
                slot,
                button,
                pos,
                mods,
            } => {
                self.pointer.slot = Some(slot_index(slot));
                self.pointer.pos = Some(pos);
                self.pointer.mods = to_core_mods(mods);
                // A drag in the views pauses the player: the pose that stays is the one keyed.
                if button == Button::Primary
                    && self.player.playing()
                    && self.ui.mode() == Mode::Animate
                {
                    self.player
                        .request(state.editor.core.doc(), AnimationRequest::Pause);
                    self.step_player(state);
                }
                let pressed = self.press(button, true);
                self.tick_buttons(state, pressed);
            }
            ViewportEvent::Drag { pos, mods, .. } => {
                self.pointer.pos = Some(pos);
                self.pointer.mods = to_core_mods(mods);
            }
            ViewportEvent::Release {
                button, pos, mods, ..
            } => {
                self.pointer.pos = Some(pos);
                self.pointer.mods = to_core_mods(mods);
                let released = self.press(button, false);
                self.tick_buttons(state, released);
            }
        }
    }

    /// Records a button going down or up; the transition for the tick.
    fn press(&mut self, button: Button, down: bool) -> (ButtonState, ButtonState) {
        let transition = if down {
            ButtonState::PRESSED
        } else {
            ButtonState::RELEASED
        };
        match button {
            Button::Primary => {
                self.pointer.left = down;
                (transition, ButtonState::default())
            }
            Button::Secondary => {
                self.pointer.right = down;
                (ButtonState::default(), transition)
            }
            Button::Middle => (ButtonState::default(), ButtonState::default()),
        }
    }

    fn tick_buttons(&mut self, state: &mut State, (left, right): (ButtonState, ButtonState)) {
        if left == ButtonState::default() && right == ButtonState::default() {
            return;
        }
        let doc = state.editor.core.doc();
        self.index.sync(doc, &doc.journal);
        self.run_tick(state, left, right);
        if !self.pointer.left && !self.pointer.right && self.pointer.pos.is_none() {
            self.pointer.slot = None;
        }
    }

    fn set_view(&mut self, slot: ViewSlot, preset: ViewPreset) {
        let i = slot_index(slot);
        let ViewSet { views, pivot, .. } = &mut self.views;
        let camera = &mut views[i].camera;
        let standard = match preset {
            ViewPreset::Front => StandardView::Front,
            ViewPreset::Back => StandardView::Back,
            ViewPreset::Right => StandardView::Right,
            ViewPreset::Left => StandardView::Left,
            ViewPreset::Top => StandardView::Top,
            ViewPreset::Bottom => StandardView::Bottom,
            ViewPreset::ToggleProjection => {
                let ortho = !camera.ortho;
                set_ortho(camera, *pivot, ortho);
                return;
            }
        };
        snap_view(camera, *pivot, standard);
    }

    /// Frames the selection, or the whole model when nothing is selected.
    fn frame_view(&mut self, state: &State, slot: ViewSlot) {
        let core = &state.editor.core;
        let doc = core.doc();
        self.index.sync(doc, &doc.journal);
        // A selected group is framed through the objects below it.
        let mut objects: Vec<cuboid_model::ObjRef> = Vec::new();
        for &obj in &core.selection().list {
            match obj {
                cuboid_model::ObjRef::Group(id) => objects.extend(doc.groups.descendants(&[id])),
                other => objects.push(other),
            }
        }
        if objects.is_empty() {
            objects.extend(
                doc.elements
                    .ids()
                    .iter()
                    .map(|id| cuboid_model::ObjRef::Element(*id)),
            );
            objects.extend(
                doc.meshes
                    .ids()
                    .iter()
                    .map(|id| cuboid_model::ObjRef::Mesh(*id)),
            );
        }
        let i = slot_index(slot);
        let aspect = self.views.views[i].rect.aspect();
        let bounds = self.index.bounds_of(&objects);
        let ViewSet { views, pivot, .. } = &mut self.views;
        match bounds {
            Some(b) => frame_bounds(&mut views[i].camera, pivot, b.min, b.max, aspect),
            None => *pivot = ViewSet::DEFAULT_PIVOT,
        }
    }
}

fn set_tool_color(state: &mut State, slot: ColorSlot, color: [f32; 4]) {
    let colors = state.editor.core.colors_mut();
    match slot {
        ColorSlot::Left => colors.left = color,
        ColorSlot::Right => colors.right = color,
    }
}

fn history_jump(state: &mut State, position: usize) {
    let hist = state.editor.core.history();
    let (undo, redo) = (hist.undo_len(), hist.redo_len());
    if position < undo {
        for _ in position..undo {
            state.run_command(Command::Undo);
        }
    } else {
        for _ in undo..position.min(undo + redo) {
            state.run_command(Command::Redo);
        }
    }
}

fn recovery(state: &mut State, choice: RecoveryChoice) {
    let index_of = |state: &State, path: &std::path::Path| match &state.dialog {
        Some(Dialog::Recovery { candidates }) => candidates
            .iter()
            .position(|candidate| candidate.cubik == path),
        _ => None,
    };
    match choice {
        RecoveryChoice::Restore(path) => {
            if let Some(i) = index_of(state, &path) {
                state.dispatch(Intent::RecoveryAccept(i));
            }
        }
        RecoveryChoice::Discard(path) => {
            if let Some(i) = index_of(state, &path) {
                state.dispatch(Intent::RecoveryDiscard(i));
            }
        }
        RecoveryChoice::DiscardAll => {
            while matches!(&state.dialog, Some(Dialog::Recovery { candidates }) if !candidates.is_empty())
            {
                state.dispatch(Intent::RecoveryDiscard(0));
            }
        }
        RecoveryChoice::Later => state.dispatch(Intent::RecoveryLater),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use cuboid_core::SelectMode;
    use cuboid_settings::{Environment, Settings};
    use cuboid_ui::{FontSet, NodeId, Template};
    use glam::{Vec2, Vec3};

    use super::*;
    use crate::io_thread::IoThread;
    use crate::platform::Dirs;

    fn setup() -> (tempfile::TempDir, State, Frontend) {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::first_run(&Environment {
            desktop_dir: String::new(),
            minecraft_dir: String::new(),
        });
        let state = State::new(
            Dirs::under(dir.path()),
            settings,
            IoThread::spawn().unwrap(),
            1252,
            Instant::now(),
        );
        let front = Frontend::new(
            Prefs::default(),
            FontSet::default(),
            dir.path().join("ui.json"),
        );
        (dir, state, front)
    }

    fn add_cube(state: &mut State, front: &mut Frontend) {
        front.apply(state, UiRequest::Action(ActionId::CreateElementCube));
    }

    #[test]
    fn a_drag_in_the_properties_panel_is_one_undo_step() {
        let (_dir, mut state, mut front) = setup();
        add_cube(&mut state, &mut front);
        let transform = |x, commit| {
            UiRequest::Edit(Edit::Transform {
                field: TransformField::Position,
                value: Vec3::new(x, 0.0, 0.0),
                commit,
            })
        };
        for (x, commit) in [(1.0, false), (2.0, false), (3.0, true)] {
            front.apply(&mut state, transform(x, commit));
        }
        let position = |state: &State| state.editor.doc().elements.items()[0].base.p;
        assert!((position(&state).x - 3.0).abs() < 1e-6);
        front.apply(&mut state, UiRequest::Action(ActionId::Undo));
        assert!(
            (position(&state).x + 2.0).abs() < 1e-6,
            "back at the blueprint"
        );
        front.apply(&mut state, UiRequest::Action(ActionId::Undo));
        assert_eq!(state.editor.doc().elements.len(), 0, "the creation is next");
    }

    #[test]
    fn the_outliner_selects_renames_and_hides() {
        let (_dir, mut state, mut front) = setup();
        add_cube(&mut state, &mut front);
        add_cube(&mut state, &mut front);
        let id = state.editor.doc().elements.ids()[0];
        let first = crate::uimodel::node_id(cuboid_model::ObjRef::Element(id)).unwrap();
        let select = |ids| {
            UiRequest::Edit(Edit::Select {
                ids,
                op: UiSelectOp::Replace,
            })
        };
        front.apply(&mut state, select(vec![first]));
        assert_eq!(state.editor.core.selection().list.len(), 1);
        let rename = Edit::Rename {
            id: first,
            name: "arm".to_owned(),
        };
        front.apply(&mut state, UiRequest::Edit(rename));
        let hide = Edit::SetVisible {
            id: first,
            visible: false,
        };
        front.apply(&mut state, UiRequest::Edit(hide));
        let element = &state.editor.doc().elements.items()[0];
        assert_eq!(&*element.base.label, "arm");
        assert!(element.base.hidden);
        front.apply(&mut state, select(Vec::new()));
        assert!(state.editor.core.selection().list.is_empty());
        // A node that is gone changes nothing and does not panic.
        let gone = Edit::Rename {
            id: NodeId(999_999),
            name: "x".to_owned(),
        };
        front.apply(&mut state, UiRequest::Edit(gone));
    }

    #[test]
    fn the_outliner_groups_moves_selects_and_dissolves() {
        let (_dir, mut state, mut front) = setup();
        for _ in 0..3 {
            add_cube(&mut state, &mut front);
        }
        let nodes = |state: &State| -> Vec<NodeId> {
            let doc = state.editor.doc();
            doc.elements
                .ids()
                .iter()
                .map(|&id| crate::uimodel::node_id(ObjRef::Element(id)).unwrap())
                .collect()
        };
        let select = |ids| {
            UiRequest::Edit(Edit::Select {
                ids,
                op: UiSelectOp::Replace,
            })
        };
        let cubes = nodes(&state);
        front.apply(&mut state, select(cubes[..2].to_vec()));
        front.apply(&mut state, UiRequest::Edit(Edit::GroupSelection));
        assert_eq!(state.editor.doc().groups.len(), 1);
        let group = state.editor.doc().groups.ids()[0];
        let group_node = crate::uimodel::node_id(ObjRef::Group(group)).unwrap();
        let first = ObjRef::Element(state.editor.doc().elements.ids()[0]);
        assert_eq!(state.editor.doc().groups.parent_of(first), Some(group));

        // The third cube joins the group; a non-group parent is refused.
        let before = state.editor.core.history().undo_len();
        let third = UiRequest::Edit(Edit::Reparent {
            ids: vec![cubes[2]],
            parent: Some(cubes[0]),
            before: None,
        });
        front.apply(&mut state, third);
        assert_eq!(state.editor.core.history().undo_len(), before);
        let third = UiRequest::Edit(Edit::Reparent {
            ids: vec![cubes[2]],
            parent: Some(group_node),
            before: None,
        });
        front.apply(&mut state, third);
        assert_eq!(state.editor.doc().groups.items()[0].children().len(), 3);

        // Hiding the group hides what it holds, and the children keep their own flag.
        let hide = Edit::SetVisible {
            id: group_node,
            visible: false,
        };
        front.apply(&mut state, UiRequest::Edit(hide));
        let doc = state.editor.doc();
        assert!(doc.is_hidden(ObjRef::Element(doc.elements.ids()[2])));
        assert!(!doc.elements.items()[2].base.hidden);

        front.apply(&mut state, select(Vec::new()));
        front.apply(
            &mut state,
            UiRequest::Edit(Edit::SelectChildren { id: group_node }),
        );
        assert_eq!(state.editor.core.selection().list.len(), 3);

        let export = Edit::SetExport {
            id: group_node,
            export: false,
        };
        front.apply(&mut state, UiRequest::Edit(export));
        assert!(!state.editor.doc().groups.items()[0].export);

        front.apply(&mut state, select(vec![group_node]));
        front.apply(&mut state, UiRequest::Edit(Edit::Ungroup));
        assert_eq!(state.editor.doc().groups.len(), 0);
        assert_eq!(state.editor.doc().elements.len(), 3);
        front.apply(&mut state, UiRequest::Action(ActionId::Undo));
        assert_eq!(state.editor.doc().groups.len(), 1);
    }

    #[test]
    fn templates_make_a_clean_model_with_content() {
        let (_dir, mut state, mut front) = setup();
        let voxel = UiRequest::NewDocument(Template::Voxel { resolution: 16 });
        front.apply(&mut state, voxel);
        assert!(state.editor.doc().voxels.is_some());
        assert!(!state.editor.is_dirty());
        front.apply(&mut state, UiRequest::NewDocument(Template::Block));
        assert_eq!(state.editor.doc().elements.len(), 1);
        assert!(!state.editor.is_dirty());
    }

    #[test]
    fn tools_reach_the_core() {
        let (_dir, mut state, mut front) = setup();
        let mode = |state: &State| state.editor.core.mode();
        front.apply(&mut state, UiRequest::SetTool(Tool::VoxelPencil));
        assert_eq!(mode(&state), SelectMode::Custom);
        front.apply(&mut state, UiRequest::SetTool(Tool::Rotate));
        assert_eq!(mode(&state), SelectMode::Rotate);
        front.apply(&mut state, UiRequest::SetTool(Tool::PaintFill));
        assert_eq!(mode(&state), SelectMode::Custom);
        front.apply(&mut state, UiRequest::SetTool(Tool::Select));
        assert_eq!(mode(&state), SelectMode::Move);
    }

    #[test]
    fn pointer_events_move_the_camera() {
        let (_dir, mut state, mut front) = setup();
        let slot = ViewSlot::MAIN;
        let before = front.views.views[3].camera;
        let orbit = ViewportEvent::Orbit {
            slot,
            delta: Vec2::new(30.0, 10.0),
        };
        front.apply(&mut state, UiRequest::Viewport(orbit));
        let orbited = front.views.views[3].camera;
        assert_ne!(orbited.pos, before.pos);
        let zoom = ViewportEvent::Zoom {
            slot,
            amount: 2.0,
            pos: Vec2::ZERO,
        };
        front.apply(&mut state, UiRequest::Viewport(zoom));
        assert_ne!(front.views.views[3].camera.pos, orbited.pos);
        let top = ViewportEvent::SetView {
            slot,
            preset: ViewPreset::Top,
        };
        front.apply(&mut state, UiRequest::Viewport(top));
        assert!(front.views.views[3].camera.forward().y < -0.99);
    }

    #[test]
    fn a_click_in_a_view_selects_what_is_under_it() {
        let (_dir, mut state, mut front) = setup();
        add_cube(&mut state, &mut front);
        state.editor.core.execute(Command::SelectNone);
        // The front view, 200 x 200 pixels, looking at the cube.
        for view in &mut front.views.views {
            view.rect = cuboid_scene::Rect::default();
        }
        front.views.views[2].rect = cuboid_scene::Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        };
        let cube = state.editor.doc().elements.items()[0].base.clone();
        let centre = cube.p + Vec3::new(0.5, 0.5, -0.5) * cube.s;
        let (ndc, _) = front.views.views[2].camera.project(centre, 1.0).unwrap();
        let at = Vec2::new((ndc.x + 1.0) * 100.0, (1.0 - ndc.y) * 100.0);
        let cameras = front.views.views.each_ref().map(|view| view.camera);
        let doc = state.editor.core.doc();
        front.index.sync(doc, &doc.journal);
        front
            .index
            .set_cameras(cameras, cuboid_scene::ViewId::Front);
        let slot = ViewSlot::FRONT;
        let mods = KeyMods::default();
        let hover = ViewportEvent::Hover {
            slot,
            pos: Some(at),
        };
        front.apply(&mut state, UiRequest::Viewport(hover));
        front.run_tick(&mut state, ButtonState::default(), ButtonState::default());
        let button = Button::Primary;
        let press = ViewportEvent::Press {
            slot,
            button,
            pos: at,
            mods,
        };
        front.apply(&mut state, UiRequest::Viewport(press));
        let release = ViewportEvent::Release {
            slot,
            button,
            pos: at,
            mods,
        };
        front.apply(&mut state, UiRequest::Viewport(release));
        assert_eq!(state.editor.core.selection().list.len(), 1);
    }

    #[test]
    fn changed_preferences_reach_the_core_and_the_disk() {
        let (dir, mut state, mut front) = setup();
        let mut prefs = Prefs::default();
        prefs.snap.enabled = false;
        prefs.autosave_minutes = 7;
        prefs.antialiasing = false;
        front.apply(&mut state, UiRequest::PrefsChanged(Box::new(prefs.clone())));
        assert!(!state.editor.core.interaction().snap.enabled);
        assert_eq!(state.settings.backup_every_minutes, 7);
        assert!(!state.editor.core.env_mut().render.antialiasing);
        // Dropping the state joins the I/O thread, which wrote the file.
        drop(state);
        let stored = crate::ui_setup::load_prefs(&dir.path().join("ui.json"));
        assert_eq!(stored, prefs);
    }
}
