//! The `Core` facade: document, selection, history, clipboards and the per-frame tick (spec:
//! arch 6 (cuboid-core), arch 10.1).

use std::sync::Arc;

use cuboid_model::{
    ApplyMode, ApplySource, CameraState, CaptureScope, Categories, DocFlags, DocSnapshot, Document,
    EditCtx, EditEnv, Label, ObjRef, RenderPrefs, Rev, Selection, Snapshot, apply_document,
    capture,
};
use cuboid_num::EngineRng;
use cuboid_ops::transform::{self, AlignEdge as OpsEdge};
use cuboid_ops::voxel::VoxelError;
use cuboid_ops::{elements, primitives, textures, uv, voxel};
use glam::{Vec2, Vec3};

use crate::clipboard::{Clipboards, ObjectClipboard};
use crate::clock::Clock;
use crate::commands::{
    CmdOutcome, ColliderAction, Command, CommandDef, CommandKind, Done, Effects, FaceId, Guard,
    Refresh, Refusal, Target,
};
use crate::compat::Compat;
use crate::contract::{CustomTool, Stale, SwapKind};
use crate::history::{Applied, History};
use crate::interact::{self, InputFrame, Interaction, SelectMode, TickOutput, select};
use crate::query::AppQuery;
use crate::start::StartQueue;
use crate::status::StatusLine;
use crate::tools::{ToolColors, ToolSizes};

/// The headless editing core: the document, the selection, undo and redo, the clipboards, the
/// start queue, the interaction state, the compatibility switches, the clock, the random
/// generator, the status line and the unsaved-changes flag. Nothing in it draws, reads the OS or
/// waits.
///
/// The document is owned by the UI thread; workers get snapshots through
/// [`Core::export_snapshot`].
///
/// spec: arch 6 (cuboid-core), arch 8.1, arch 10.1
pub struct Core {
    pub(crate) doc: Document,
    pub(crate) env: EditEnv,
    pub(crate) sel: Selection,
    pub(crate) hist: History<Snapshot>,
    pub(crate) clip: Clipboards,
    pub(crate) start: StartQueue,
    pub(crate) interact: Interaction,
    pub(crate) tool: Option<Box<dyn CustomTool>>,
    pub(crate) colors: ToolColors,
    pub(crate) sizes: ToolSizes,
    pub(crate) compat: Compat,
    pub(crate) clock: Clock,
    pub(crate) rng: EngineRng,
    pub(crate) status: StatusLine,
    pub(crate) dirty: bool,
    /// Deferred commands already reported in the log.
    pub(crate) logged: Vec<&'static str>,
}

/// An error of a command that turned into an outcome.
fn failed(what: &str, e: impl std::fmt::Display) -> CmdOutcome {
    log::error!("{what}: {e}");
    CmdOutcome::Failed(format!("{what}: {e}"))
}

fn refused(r: Refusal) -> CmdOutcome {
    CmdOutcome::Refused(r)
}

fn voxel_error(e: VoxelError) -> CmdOutcome {
    refused(Refusal::Op(e.to_string()))
}

fn face_names(faces: &[FaceId]) -> Vec<cuboid_model::FaceName> {
    faces.iter().map(|&f| f.into()).collect()
}

fn op_error(e: cuboid_ops::OpError) -> CmdOutcome {
    match e {
        cuboid_ops::OpError::Apply(e) => failed("the model could not apply the change", e),
        other => CmdOutcome::Refused(Refusal::Op(other.to_string())),
    }
}

impl Core {
    /// A core over a fresh model (the state after File > New) with the given switches and a
    /// generator seeded with `seed`; production seeds from the clock.
    pub fn new(compat: Compat, seed: u64) -> Core {
        Core::with_document(Document::new(textures::default_list()), compat, seed)
    }

    /// A core over an existing document, with empty history and selection.
    pub fn with_document(doc: Document, compat: Compat, seed: u64) -> Core {
        let env = EditEnv {
            restrict_rotations: doc.props.general.restrict_rotations,
            render: RenderPrefs::default(),
            code_page: 1252,
        };
        Core {
            doc,
            env,
            sel: Selection::default(),
            hist: History::default(),
            clip: Clipboards::default(),
            start: StartQueue::default(),
            interact: Interaction::default(),
            tool: None,
            colors: ToolColors::default(),
            sizes: ToolSizes::default(),
            compat,
            clock: Clock::default(),
            rng: EngineRng::seeded(seed),
            status: StatusLine::default(),
            dirty: false,
            logged: Vec::new(),
        }
    }

    /// The document, read only.
    pub fn doc(&self) -> &Document {
        &self.doc
    }

    /// The document for the readers that drain its dirty sets, such as `Extractor::extract`
    /// (it takes the dirty chunks of the voxel world). Edits go through commands, never here.
    pub fn doc_for_extract(&mut self) -> &mut Document {
        &mut self.doc
    }

    /// The selection, read only.
    pub fn selection(&self) -> &Selection {
        &self.sel
    }

    /// The undo history, read only (labels for a history panel).
    pub fn history(&self) -> &History<Snapshot> {
        &self.hist
    }

    /// Changes the caps of the undo history: the most steps kept and the most memory the steps
    /// may own alone. The oldest steps go at once.
    pub fn set_history_limits(&mut self, max_entries: usize, max_bytes: usize) {
        self.hist.set_limits(max_entries, max_bytes);
    }

    /// The clipboards, read only.
    pub fn clipboards(&self) -> &Clipboards {
        &self.clip
    }

    /// The compatibility switches the core was made with.
    pub fn compat(&self) -> &Compat {
        &self.compat
    }

    /// The interaction state: hover, the grabbed axes, the snapping.
    pub fn interaction(&self) -> &Interaction {
        &self.interact
    }

    /// The snapping of drags.
    pub fn snap_mut(&mut self) -> &mut interact::SnapSettings {
        &mut self.interact.snap
    }

    /// The select mode: which gizmo the pointer drives.
    pub fn mode(&self) -> SelectMode {
        self.interact.mode
    }

    /// Chooses the gizmo of the pointer. Ignored while a tool is active.
    pub fn set_mode(&mut self, mode: SelectMode) {
        if self.tool.is_none() && mode != SelectMode::Custom {
            self.interact.mode = mode;
            self.interact.cancel();
        }
    }

    /// The settings operations read; the application updates them from the preferences.
    pub fn env_mut(&mut self) -> &mut EditEnv {
        &mut self.env
    }

    /// The left and right tool colours.
    pub fn colors_mut(&mut self) -> &mut ToolColors {
        &mut self.colors
    }

    /// The size fields of the tool panels.
    pub fn sizes_mut(&mut self) -> &mut ToolSizes {
        &mut self.sizes
    }

    /// The status line text at the core clock.
    pub fn status_text(&self) -> &str {
        self.status.text(self.clock.now)
    }

    /// The document has unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// The document was saved: the unsaved-changes flag goes.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// Forgets what changed in the document once everything that reads the journal has run.
    pub fn clear_journal(&mut self) {
        self.doc.journal.clear();
    }

    /// Stores the camera poses of the four views in the view properties, so a save or a
    /// snapshot carries them. Not an edit: no undo entry, no unsaved-changes flag.
    pub fn set_cameras(&mut self, cameras: [Option<CameraState>; 4]) {
        if self.doc.view.cameras == cameras {
            return;
        }
        Arc::make_mut(&mut self.doc.view).cameras = cameras;
        self.doc.journal.mark_doc(DocFlags::VIEW);
        self.doc.revision = Rev::next();
    }

    /// Switches the custom tool on or off: a tool enters custom mode, clears the selection and
    /// binds the stroke callbacks; `None` returns to move mode.
    ///
    /// spec: tools 8.7, tools 8.10
    pub fn set_tool(&mut self, tool: Option<Box<dyn CustomTool>>) {
        self.interact.cancel();
        if tool.is_some() {
            select::set(&mut self.doc, &mut self.sel, &[]);
            self.interact.mode = SelectMode::Custom;
        } else if self.interact.mode == SelectMode::Custom {
            self.interact.mode = SelectMode::Move;
        }
        self.tool = tool;
    }

    /// The edit context over the document, the selection, the environment and the generator.
    pub(crate) fn cx(&mut self) -> EditCtx<'_> {
        EditCtx {
            doc: &mut self.doc,
            sel: &mut self.sel,
            env: &self.env,
            rng: &mut self.rng,
        }
    }

    /// Pushes an undo entry and marks the document as changed.
    pub(crate) fn record(
        &mut self,
        label: Label,
        categories: Categories,
        snap: Snapshot,
        gesture: Option<u64>,
    ) {
        // Undo and redo of voxels leave the scene colliders stale.
        let callback = categories.contains(Categories::VOXELS).then(|| Effects {
            colliders: vec![ColliderAction::Refresh],
            ..Effects::default()
        });
        self.hist
            .push_gesture(label, categories, snap, callback, gesture);
        self.dirty = true;
    }

    /// The command target of an object.
    pub fn target_of(&self, obj: ObjRef) -> Option<Target> {
        match obj {
            ObjRef::Element(id) => self.doc.elements.position(id).map(Target::Element),
            ObjRef::Mesh(id) => self.doc.meshes.position(id).map(Target::Mesh),
            ObjRef::Blueprint => Some(Target::Blueprint),
            ObjRef::ModelOrigin => Some(Target::ModelOrigin),
        }
    }

    /// The object of a command target.
    pub fn object_of(&self, target: Target) -> Option<ObjRef> {
        match target {
            Target::Element(at) => self
                .doc
                .elements
                .ids()
                .get(at)
                .map(|&id| ObjRef::Element(id)),
            Target::Mesh(at) => self.doc.meshes.ids().get(at).map(|&id| ObjRef::Mesh(id)),
            Target::Blueprint => Some(ObjRef::Blueprint),
            Target::ModelOrigin => Some(ObjRef::ModelOrigin),
        }
    }

    /// The objects a command acts on: its targets, or the selection when it names none.
    fn resolve(&self, cmd: &Command, guard: Guard) -> Result<Vec<ObjRef>, Refusal> {
        let named = cmd.targets();
        let mut objs: Vec<ObjRef> = if named.is_empty() {
            select::list(&self.sel)
        } else {
            let mut objs = Vec::with_capacity(named.len());
            for &target in named {
                objs.push(self.object_of(target).ok_or(Refusal::BadTarget)?);
            }
            objs
        };
        let mut seen = Vec::with_capacity(objs.len());
        objs.retain(|obj| {
            let fresh = !seen.contains(obj);
            seen.push(*obj);
            fresh
        });
        let listed = objs
            .iter()
            .any(|o| matches!(o, ObjRef::Element(_) | ObjRef::Mesh(_)));
        let element = objs.iter().any(|o| matches!(o, ObjRef::Element(_)));
        match guard {
            Guard::None => {}
            Guard::Any if objs.is_empty() => return Err(Refusal::NothingSelected),
            Guard::Listed if !listed => return Err(Refusal::NothingSelected),
            Guard::Elements if !element => {
                return Err(if listed || !objs.is_empty() {
                    Refusal::NoElements
                } else {
                    Refusal::NothingSelected
                });
            }
            _ => {}
        }
        Ok(objs)
    }

    /// Runs one command as one undo step (selection changes, copy and the history commands
    /// leave none). Never panics: a refusal, a failed apply or a feature that is not built yet
    /// comes back as an outcome and leaves the document as it was.
    ///
    /// spec: arch 10.1, tools 15
    pub fn execute(&mut self, cmd: Command) -> CmdOutcome {
        self.run(cmd, None)
    }

    /// As [`Core::execute`] for a continuous edit, such as dragging a number field: commands of
    /// one `gesture` in a row share the undo step of the first.
    pub fn execute_merged(&mut self, cmd: Command, gesture: u64) -> CmdOutcome {
        self.run(cmd, Some(gesture))
    }

    fn run(&mut self, cmd: Command, gesture: Option<u64>) -> CmdOutcome {
        if let Err(r) = cmd.validate() {
            return refused(r);
        }
        let outcome = match cmd.kind() {
            CommandKind::Instant => self.run_instant(&cmd),
            CommandKind::Edit(def) => self.run_edit(&cmd, def, gesture),
            CommandKind::Deferred(why) => {
                let name = format!("{cmd:?}");
                let name = name.split([' ', '(', '{']).next().unwrap_or("command");
                if !self.logged.contains(&why) {
                    self.logged.push(why);
                    log::warn!("{name} is not available yet: {why}");
                }
                CmdOutcome::NotYet(why)
            }
        };
        if let CmdOutcome::Refused(r) = &outcome {
            self.status.set(r.to_string(), self.clock.now);
        }
        outcome
    }

    fn run_instant(&mut self, cmd: &Command) -> CmdOutcome {
        let mut done = Done::default();
        let before = (select::list(&self.sel), self.sel.primary);
        match cmd {
            Command::Undo => return self.undo(),
            Command::Redo => return self.redo(),
            Command::Copy => {
                let objs = match self.resolve(cmd, Guard::Listed) {
                    Ok(objs) => objs,
                    Err(r) => return refused(r),
                };
                self.copy_to_clipboard(&objs);
                done.affected = objs.len();
                done.effects.refresh |= Refresh::EDIT_MENU;
                return CmdOutcome::Done(done);
            }
            Command::PaletteAdd { color } => {
                return match voxel::palette_add(&mut self.doc, *color) {
                    Ok(true) => CmdOutcome::Done(Self::done(1)),
                    Ok(false) => CmdOutcome::Unchanged,
                    Err(e) => voxel_error(e),
                };
            }
            Command::Select { op, .. } => {
                let objs = match self.resolve(cmd, Guard::None) {
                    Ok(objs) => objs,
                    Err(r) => return refused(r),
                };
                let named = cmd.targets();
                let objs = if named.is_empty() { Vec::new() } else { objs };
                select::apply(&mut self.doc, &mut self.sel, &objs, *op);
            }
            Command::SelectAll => select::select_all(&mut self.doc, &mut self.sel),
            Command::SelectNone => select::set(&mut self.doc, &mut self.sel, &[]),
            Command::SelectInverse => select::select_inverse(&mut self.doc, &mut self.sel),
            _ => return CmdOutcome::Unchanged,
        }
        if before == (select::list(&self.sel), self.sel.primary) {
            return CmdOutcome::Unchanged;
        }
        done.affected = self.sel.list.len();
        done.effects.refresh |= Refresh::LISTS | Refresh::GIZMOS;
        CmdOutcome::Done(done)
    }

    fn run_edit(&mut self, cmd: &Command, def: CommandDef, gesture: Option<u64>) -> CmdOutcome {
        let objs = match self.resolve(cmd, def.guard) {
            Ok(objs) => objs,
            Err(r) => return refused(r),
        };
        let mut categories = def.categories;
        if objs.contains(&ObjRef::ModelOrigin) {
            categories |= Categories::MODEL_PROPERTIES;
        }
        let continuing = gesture.is_some_and(|g| self.hist.continues(g));
        let before = (!continuing).then(|| {
            capture(
                &self.doc,
                &self.sel,
                CaptureScope {
                    categories,
                    selection_only: false,
                },
            )
        });
        let rev0 = self.doc.revision;
        let was_dirty = self.dirty;
        match self.perform(cmd, &objs) {
            Ok(mut done) => {
                if self.doc.revision == rev0 {
                    return CmdOutcome::Unchanged;
                }
                let label = Label::from(def.label.into_owned());
                self.status.set(&*label, self.clock.now);
                if let Some(before) = before {
                    self.record(label, categories, before, gesture);
                }
                self.dirty = true;
                done.effects.refresh |= Refresh::EDIT_MENU | Refresh::LISTS | Refresh::GIZMOS;
                if categories.contains(Categories::TEXTURES) {
                    done.effects.refresh |= Refresh::TEXTURES;
                }
                if !was_dirty {
                    done.effects.refresh |= Refresh::TITLE;
                }
                CmdOutcome::Done(done)
            }
            Err(outcome) => {
                if self.doc.revision != rev0
                    && let Some(before) = before
                {
                    self.restore(&before, categories);
                }
                outcome
            }
        }
    }

    /// Puts a snapshot back after a command failed half way.
    fn restore(&mut self, snap: &Snapshot, categories: Categories) {
        let result = apply_document(
            &mut self.cx(),
            ApplySource::Snapshot(snap),
            ApplyMode::Restore(categories),
        );
        if let Err(e) = result {
            log::error!("could not roll back a failed command: {e}");
        }
    }

    fn copy_to_clipboard(&mut self, objs: &[ObjRef]) {
        let snapshot = elements::copy(&self.cx(), objs);
        let anchor = self
            .sel
            .primary
            .filter(|p| objs.contains(p))
            .or_else(|| objs.first().copied())
            .and_then(|obj| transform::position_of(&self.doc, obj))
            .unwrap_or(Vec3::ZERO);
        self.clip.voxels = None;
        self.clip.objects = Some(ObjectClipboard { snapshot, anchor });
    }

    /// Creates the objects a paste made: they start, and the outcome names them.
    fn created(&mut self, objs: Vec<ObjRef>) -> Done {
        self.start.push(objs.iter().copied());
        self.start.run(&mut EditCtx {
            doc: &mut self.doc,
            sel: &mut self.sel,
            env: &self.env,
            rng: &mut self.rng,
        });
        Done {
            created: objs.iter().filter_map(|&o| self.target_of(o)).collect(),
            affected: objs.len(),
            ..Done::default()
        }
    }

    fn done(affected: usize) -> Done {
        Done {
            affected,
            ..Done::default()
        }
    }

    /// The mutation of one edit command.
    fn perform(&mut self, cmd: &Command, objs: &[ObjRef]) -> Result<Done, CmdOutcome> {
        use Command as C;
        let none = |n: usize, why: Refusal| {
            if n == 0 {
                Err(refused(why))
            } else {
                Ok(Self::done(n))
            }
        };
        match cmd {
            C::Cut => {
                let snapshot = elements::copy(&self.cx(), objs);
                let removed = elements::delete(&mut self.cx(), objs);
                if removed == 0 {
                    return Err(refused(Refusal::NotDeletable));
                }
                let anchor = transform::position_of(&self.doc, objs[0]).unwrap_or(Vec3::ZERO);
                self.clip.voxels = None;
                self.clip.objects = Some(ObjectClipboard { snapshot, anchor });
                Ok(Self::done(removed))
            }
            C::Paste { at_blueprint } => {
                let Some(clip) = self.clip.objects.clone() else {
                    return Err(if self.clip.voxels.is_some() {
                        CmdOutcome::NotYet("pasting voxels arrives with L2-B")
                    } else {
                        refused(Refusal::ClipboardEmpty)
                    });
                };
                let offset = if *at_blueprint {
                    self.doc.blueprint.base.p - clip.anchor
                } else {
                    Vec3::ZERO
                };
                let created =
                    elements::paste(&mut self.cx(), &clip.snapshot, offset).map_err(op_error)?;
                Ok(self.created(created))
            }
            C::Duplicate { .. } => {
                let created = elements::duplicate(&mut self.cx(), objs).map_err(op_error)?;
                Ok(self.created(created))
            }
            C::AddElement(kind) => {
                let id = primitives::add_element(&mut self.cx(), (*kind).into());
                Ok(self.created(vec![ObjRef::Element(id)]))
            }
            C::Delete { .. } => none(
                elements::delete(&mut self.cx(), objs),
                Refusal::NotDeletable,
            ),
            C::Rename { name, .. } => Ok(Self::done(elements::rename(&mut self.cx(), objs, name))),
            C::SetHidden { hidden, .. } => Ok(Self::done(elements::set_hidden(
                &mut self.cx(),
                objs,
                *hidden,
            ))),
            C::SetLocked { locked, .. } => Ok(Self::done(elements::set_locked(
                &mut self.cx(),
                objs,
                *locked,
            ))),
            C::ShowAll => Ok(Self::done(elements::show_all(&mut self.cx()))),
            C::UnlockAll => Ok(Self::done(elements::unlock_all(&mut self.cx()))),
            C::MoveBy { delta, snap, .. } => Ok(Self::done(transform::translate(
                &mut self.cx(),
                objs,
                Vec3::from_array(*delta),
                *snap,
            ))),
            C::SetPosition { value, .. } => Ok(Self::done(transform::set_position(
                &mut self.cx(),
                objs,
                *value,
            ))),
            C::SetSize { value, .. } => {
                Ok(Self::done(transform::resize(&mut self.cx(), objs, *value)))
            }
            C::ScaleBy { factor, snap, .. } => {
                let pivot = objs
                    .first()
                    .and_then(|&o| transform::pivot_of(&self.doc, o))
                    .unwrap_or(Vec3::ZERO);
                Ok(Self::done(transform::scale_about(
                    &mut self.cx(),
                    objs,
                    pivot,
                    Vec3::from_array(*factor),
                    *snap,
                )))
            }
            C::RotateBy { delta, snap, .. } => {
                let r = transform::rotate_by(&mut self.cx(), objs, Vec3::from_array(*delta), *snap);
                Self::rotated(r)
            }
            C::SetRotation { value, .. } => {
                Self::rotated(transform::set_rotation_to(&mut self.cx(), objs, *value))
            }
            C::QuarterTurn { axis, .. } => Ok(Self::done(transform::quarter_turn(
                &mut self.cx(),
                objs,
                (*axis).into(),
            ))),
            C::ResetRotation { .. } => {
                Ok(Self::done(transform::reset_rotation(&mut self.cx(), objs)))
            }
            C::ResetPivot { .. } => Ok(Self::done(transform::reset_pivot(&mut self.cx(), objs))),
            C::Mirror { axis, copy, .. } => {
                let r = transform::mirror(&mut self.cx(), objs, (*axis).into(), *copy)
                    .map_err(op_error)?;
                if r.copies.is_empty() {
                    Ok(Self::done(r.mirrored))
                } else {
                    Ok(self.created(r.copies))
                }
            }
            C::Align { axis, edge, .. } => {
                let edge: OpsEdge = (*edge).into();
                Ok(Self::done(transform::align(
                    &mut self.cx(),
                    objs,
                    (*axis).into(),
                    edge,
                )))
            }
            C::CreateTexture {
                label,
                width,
                height,
                color,
            } => {
                textures::create(&mut self.cx(), label.as_deref(), *width, *height, *color)
                    .map_err(op_error)?;
                Ok(Self::done(1))
            }
            C::RemoveTexture { label } => {
                textures::remove(&mut self.cx(), label).map_err(op_error)?;
                Ok(Self::done(1))
            }
            C::AssignTexture { label, faces, .. } => {
                let faces: Vec<_> = faces.iter().map(|&f| f.into()).collect();
                let n = textures::assign(&mut self.cx(), objs, label, &faces).map_err(op_error)?;
                Ok(Self::done(n))
            }
            C::SetupVoxels { resolution } => {
                voxel::setup(&mut self.cx(), *resolution).map_err(voxel_error)?;
                Ok(Self::voxels_done(1))
            }
            C::PaletteReplace { from, to } => {
                let n = voxel::replace_color(&mut self.cx(), *from, *to).map_err(voxel_error)?;
                Ok(Self::voxels_done(n))
            }
            C::AutoUv { style, faces, .. } => Ok(Self::done(uv::auto_uv(
                &mut self.cx(),
                objs,
                &face_names(faces),
                (*style).into(),
            ))),
            C::RotateUv { faces, degrees, .. } => Ok(Self::done(uv::rotate(
                &mut self.cx(),
                objs,
                &face_names(faces),
                *degrees,
            ))),
            C::FlipUv { faces, axis, .. } => Ok(Self::done(uv::flip(
                &mut self.cx(),
                objs,
                &face_names(faces),
                (*axis).into(),
            ))),
            C::SetUvRect {
                faces,
                rect: [u0, v0, u1, v1],
                rotation,
                ..
            } => Ok(Self::done(uv::set_rect(
                &mut self.cx(),
                objs,
                &face_names(faces),
                Vec2::new(*u0, *v0),
                Vec2::new(*u1, *v1),
                *rotation,
            ))),
            C::SetUvSnap {
                enabled, mirror, ..
            } => {
                let option = if *mirror {
                    uv::SnapOption::Mirror
                } else {
                    uv::SnapOption::Snap
                };
                Ok(Self::done(uv::set_snap(
                    &mut self.cx(),
                    objs,
                    option,
                    *enabled,
                )))
            }
            _ => Err(CmdOutcome::Unchanged),
        }
    }

    /// The result of a command that changed voxels: the scene colliders must follow.
    fn voxels_done(affected: usize) -> Done {
        let mut done = Self::done(affected);
        done.effects.colliders.push(ColliderAction::Refresh);
        done
    }

    fn rotated(r: transform::Rotated) -> Result<Done, CmdOutcome> {
        if r.changed == 0 && r.rejected > 0 {
            return Err(refused(Refusal::RotationRestricted));
        }
        Ok(Self::done(r.changed))
    }

    /// Undo; restores the newest entry through the apply path. The selection comes back as it
    /// was at the time of the entry, the first selected object being the primary.
    ///
    /// spec: tools 15.1, tools 15.3
    pub fn undo(&mut self) -> CmdOutcome {
        let result = {
            let Core {
                doc,
                sel,
                env,
                rng,
                hist,
                ..
            } = self;
            hist.undo(|entry| swap(doc, sel, env, rng, entry.categories, &entry.snap))
        };
        self.after_history(result, Refusal::NothingToUndo, "Undo")
    }

    /// Redo; restores the state the last undo replaced.
    ///
    /// spec: tools 15.1, tools 15.3
    pub fn redo(&mut self) -> CmdOutcome {
        let result = {
            let Core {
                doc,
                sel,
                env,
                rng,
                hist,
                ..
            } = self;
            hist.redo(|entry| swap(doc, sel, env, rng, entry.categories, &entry.snap))
        };
        self.after_history(result, Refusal::NothingToRedo, "Redo")
    }

    fn after_history(
        &mut self,
        result: Option<Result<Applied, cuboid_model::ApplyError>>,
        nothing: Refusal,
        verb: &str,
    ) -> CmdOutcome {
        let applied = match result {
            None => {
                self.status.set(nothing.to_string(), self.clock.now);
                return refused(nothing);
            }
            Some(Err(e)) => return failed(&format!("{verb} failed"), e),
            Some(Ok(applied)) => applied,
        };
        self.env.restrict_rotations = self.doc.props.general.restrict_rotations;
        self.interact.cancel();
        select::prune(&self.doc, &mut self.sel);
        self.dirty = true;
        self.status
            .set(format!("{verb} {}", applied.label), self.clock.now);
        let mut effects = applied.callback.unwrap_or_default();
        effects.refresh |= Refresh::EDIT_MENU
            | Refresh::LISTS
            | Refresh::TEXTURES
            | Refresh::TITLE
            | Refresh::GIZMOS;
        CmdOutcome::Done(Done {
            effects,
            ..Done::default()
        })
    }

    /// One frame: advances the clock and runs the interaction state machine on `input`, with
    /// picking through `ray`.
    ///
    /// spec: arch 10.4, arch 10.5
    pub fn tick(&mut self, input: &InputFrame, ray: &dyn cuboid_scene::Raycaster) -> TickOutput {
        self.clock.advance(input.now);
        let mut interaction = std::mem::take(&mut self.interact);
        let out = interact::update(self, &mut interaction, input, ray);
        self.interact = interaction;
        out
    }

    /// The gizmo of the primary object for the current mode, for drawing; `None` when there is
    /// none to show.
    pub fn gizmo_state(&self) -> Option<cuboid_scene::GizmoState> {
        interact::gizmo_state(self, self.interact.mode)
    }

    /// A pointer copy of the document for workers (save, export, backup). Set the camera poses
    /// first with [`Core::set_cameras`].
    ///
    /// spec: arch 11.2, native 5.2
    pub fn export_snapshot(&self) -> Arc<DocSnapshot> {
        Arc::new(self.doc.clone())
    }

    /// Replaces the document by one a worker prepared, only if `base` is still the revision it
    /// was prepared from. `SwapKind::Open` also clears the history, the selection and the
    /// unsaved-changes flag; an import keeps the history.
    ///
    /// spec: arch 11.2, native 6.4
    pub fn swap_document(&mut self, doc: Document, base: Rev, kind: SwapKind) -> Result<(), Stale> {
        if base != self.doc.revision {
            return Err(Stale);
        }
        self.doc = doc;
        self.doc.revision = Rev::next();
        self.env.restrict_rotations = self.doc.props.general.restrict_rotations;
        self.interact.cancel();
        match kind {
            SwapKind::Open => {
                self.hist.clear();
                self.sel = Selection::default();
                self.dirty = false;
            }
            SwapKind::Import => {
                select::prune(&self.doc, &mut self.sel);
                self.dirty = true;
            }
        }
        self.doc
            .journal
            .mark_doc(DocFlags::LISTS | DocFlags::SELECTION | DocFlags::TEXTURES);
        Ok(())
    }

    /// What the menus, panels and status bar need to know.
    ///
    /// spec: menus 1.7, menus 4
    pub fn query(&self) -> AppQuery {
        AppQuery::of(self)
    }
}

/// Restores `snap` and returns a snapshot of what it replaced.
fn swap(
    doc: &mut Document,
    sel: &mut Selection,
    env: &EditEnv,
    rng: &mut EngineRng,
    categories: Categories,
    snap: &Snapshot,
) -> Result<Snapshot, cuboid_model::ApplyError> {
    let current = capture(
        doc,
        sel,
        CaptureScope {
            categories,
            selection_only: false,
        },
    );
    apply_document(
        &mut EditCtx { doc, sel, env, rng },
        ApplySource::Snapshot(snap),
        ApplyMode::Restore(categories),
    )?;
    Ok(current)
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core")
            .field("elements", &self.doc.elements.len())
            .field("meshes", &self.doc.meshes.len())
            .field("selected", &self.sel.list.len())
            .field("undo", &self.hist.undo_len())
            .field("dirty", &self.dirty)
            .finish_non_exhaustive()
    }
}
