//! The single apply path for open, merge, restore and paste (spec: native 6.2-6.3; tools 15.3; arch
//! 9.1).

use std::sync::Arc;

use cuboid_voxel::{Chunk, ChunkSlot, I3, IBox3, VoxelSnap, VoxelWorld, WBox3, WriteKind, mapping};
use glam::Vec3;
use rustc_hash::{FxHashMap, FxHashSet};
use thiserror::Error;

use crate::contract::{Document, EditCtx, ModelBox, OverrideCase, Retained, ViewProps};
use crate::ids::{ObjId, ObjRef, Rev};
use crate::journal::{ChangeFlags, ChangeJournal, DocFlags};
use crate::list::{ListSnap, ObjList};
use crate::normal_form;
use crate::objects::{Element, ElementFlags, Mesh, TextureEntry, TextureList};
use crate::selection::Selection;
use crate::setters::set_model_box;
use crate::snapshot::{Categories, Snapshot};

/// The largest voxel resolution the editor itself makes: the setup allows 2 to 128 and upscaling
/// reaches 254 (native 4.7). Informational: the apply path accepts any resolution that fits the
/// memory budget.
pub const MAX_VOXEL_RES: u32 = 254;

/// The memory budget `apply_document` gives a voxel world: 1 GiB of slot table and chunk data.
/// A dense 254 world needs more, a sparse one far less (native 4.7, arch 12.3).
pub const DEFAULT_VOXEL_BUDGET: u64 = 1 << 30;

/// What the apply path does with its source.
///
/// spec: arch 6 (cuboid-model), native 6.1, tools 11.3, tools 15.3
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ApplyMode {
    /// File > Open: the scene is reset (properties, display, model box, lists, textures, voxels,
    /// selection and highlight, the item overrides and the retention data), then the sections
    /// present in the source are applied. The view properties and the blueprint stay as they
    /// are unless the file has `vp` or `bp` (native 4.1, 6.1). The caller resets what lives
    /// outside the document (history, clipboards, paint state) and sets `mp.sp` afterwards.
    Open,
    /// File > Merge: elements and meshes are appended, the other sections present are
    /// overwritten. The retention data stays.
    Merge,
    /// Undo, redo: restore the named categories, as far as the snapshot has them.
    Restore(Categories),
    /// Edit > Paste: create new objects from the elements and meshes of the snapshot, with the
    /// position and origin offset. The other categories are ignored (pasted voxels come from
    /// the voxel clipboard).
    Paste {
        /// Blueprint position now minus the copy anchor.
        offset: Vec3,
    },
}

/// The records of a voxel world in file order, for a loader whose chunk and voxel records are not
/// in slot and scan order or repeat a cell. Applying them replays every cell through the full
/// writer into a fresh world (native 4.7, 6.3), so the active box applies and the palette lists
/// every value in file order, a repeated cell's earlier value included. A [`VoxelSnap`] equals
/// that replay only for records in slot and scan order without repeats (see its documentation),
/// which is what an undo snapshot and a canonical file hold; the loader decides.
///
/// spec: native 4.7, native 6.3 (`vx`), arch 9.1 (Voxels)
#[derive(Clone, Debug)]
pub struct VoxelReplay {
    /// Voxels per block edge.
    pub res: u32,
    /// Global cell indices (`16 * chunk + local`) and packed colours in file order. Records of
    /// chunks without voxels, and zero colours, are left out by the loader.
    pub cells: Vec<(I3, u32)>,
}

/// A decoded file, ready to apply: the sections the file has, in the record shape of a snapshot
/// (`categories` says which are present; an absent section is not applied, and the voxel
/// section with no world resets an existing one), plus what a snapshot does not carry.
///
/// Loader rules the apply path relies on: the lists carry fresh `ObjId`s (`ListSnap::fresh`),
/// meshes have the permissions of their kind (tools 3.1), and `tx` entries come in file order
/// (a null `fp` is turned into an empty path here, repeated labels replace the earlier image).
///
/// spec: native 4.1, native 6.2, arch 9.1
#[derive(Clone, Debug)]
pub struct LoadedDoc {
    /// The sections of the file.
    pub sections: Snapshot,
    /// The voxel records in file order when they are not canonical. When set, the voxel
    /// category must be present in `sections` (an apply without it is `ApplyError::Invalid`); the
    /// replay then supplies the world and `sections.voxels` is ignored.
    pub voxel_replay: Option<VoxelReplay>,
    /// Item overrides from the port's own extension entry.
    pub overrides: Option<Arc<[OverrideCase]>>,
    /// Retention data for byte-identical re-save.
    pub retained: Option<Arc<Retained>>,
}

/// Where the apply path reads from.
///
/// spec: arch 6 (cuboid-model), arch 9.1
#[derive(Debug)]
pub enum ApplySource<'a> {
    /// A decoded file.
    Loaded(Box<LoadedDoc>),
    /// An undo or clipboard snapshot.
    Snapshot(&'a Snapshot),
}

/// What an apply did, for the caller's follow-up.
///
/// spec: tools 4.7, native 8.1
#[derive(Clone, Debug, Default)]
pub struct ApplyReport {
    /// Objects created or restored, in order (elements, then meshes): the start queue runs
    /// `default_uv(false)` on them before the next render.
    pub created: Vec<ObjRef>,
    /// Records that were skipped or normalised, for the log.
    pub warnings: Vec<String>,
}

/// Why an apply failed. The document is untouched in that case.
///
/// spec: native 6.4, arch 16
#[derive(Debug, Error)]
pub enum ApplyError {
    /// A texture record has an empty label ("Invalid Label").
    #[error("a texture record has an empty label")]
    EmptyTextureLabel,
    /// The voxel resolution is below 1 or its chunk table overflows.
    #[error("voxel resolution {0} is out of range")]
    VoxelResolution(u32),
    /// The voxel world needs more memory than the budget allows.
    #[error("a voxel world of resolution {res} needs {bytes} bytes, over the budget of {budget}")]
    VoxelBudget {
        /// The resolution of the refused world.
        res: u32,
        /// Its estimated memory.
        bytes: u64,
        /// The budget it exceeds.
        budget: u64,
    },
    /// A record the port cannot apply.
    #[error("{0}")]
    Invalid(String),
}

/// Where the voxel section of a source comes from.
enum VoxelSource<'a> {
    /// No world: an existing one is reset.
    Reset,
    /// The slot table of a snapshot or of a canonical file.
    Snap(&'a VoxelSnap),
    /// The records of a file in file order.
    Replay(&'a VoxelReplay),
}

/// The parts of a source the mode applies, checked against their categories.
struct Sections<'a> {
    props: Option<&'a Arc<crate::contract::ModelProps>>,
    view: Option<&'a Arc<ViewProps>>,
    elements: Option<&'a crate::snapshot::ElementsSnap>,
    textures: Option<&'a Arc<TextureList>>,
    voxels: Option<VoxelSource<'a>>,
    meshes: Option<&'a crate::snapshot::MeshesSnap>,
    display: Option<&'a Arc<crate::contract::DisplaySlots>>,
}

impl<'a> Sections<'a> {
    fn select(
        snap: &'a Snapshot,
        loaded: Option<&'a LoadedDoc>,
        cats: Categories,
    ) -> Result<Self, ApplyError> {
        let replay = loaded.and_then(|l| l.voxel_replay.as_ref());
        if replay.is_some() && !cats.contains(Categories::VOXELS) {
            return Err(ApplyError::Invalid(
                "a voxel replay needs the voxel category in the sections".to_owned(),
            ));
        }
        let voxels = if cats.contains(Categories::VOXELS) {
            Some(match (replay, &snap.voxels) {
                (Some(replay), _) => VoxelSource::Replay(replay),
                (None, Some(Some(world))) => VoxelSource::Snap(world),
                (None, Some(None)) => VoxelSource::Reset,
                (None, None) => return Err(missing(Categories::VOXELS)),
            })
        } else {
            None
        };
        Ok(Self {
            props: part(cats, Categories::MODEL_PROPERTIES, snap.props.as_ref())?,
            view: part(cats, Categories::VIEW_PROPERTIES, snap.view.as_ref())?,
            elements: part(cats, Categories::ELEMENTS, snap.elements.as_ref())?,
            textures: part(cats, Categories::TEXTURES, snap.textures.as_ref())?,
            voxels,
            meshes: part(cats, Categories::MESHES, snap.meshes.as_ref())?,
            display: part(cats, Categories::MODEL_DISPLAY, snap.display.as_ref())?,
        })
    }

    /// Everything that can fail, before the document changes (native 6.4).
    fn validate(&self, mode: ApplyMode, budget: u64) -> Result<(), ApplyError> {
        if let Some(textures) = self.textures
            && textures.entries.iter().any(|e| e.label.is_empty())
        {
            return Err(ApplyError::EmptyTextureLabel);
        }
        match &self.voxels {
            Some(VoxelSource::Snap(snap)) => {
                check_resolution(snap.res, 0, budget)?;
                let n = chunks_per_axis(snap.res);
                if snap.slots.len() != n * n * n {
                    return Err(ApplyError::Invalid(format!(
                        "a voxel snapshot of resolution {} holds {} chunk slots instead of {}",
                        snap.res,
                        snap.slots.len(),
                        n * n * n
                    )));
                }
                let chunks = snap
                    .slots
                    .iter()
                    .filter(|slot| matches!(slot, ChunkSlot::Data(_)))
                    .count();
                check_resolution(snap.res, chunks, budget)?;
            }
            Some(VoxelSource::Replay(replay)) => {
                let chunks: FxHashSet<I3> =
                    replay.cells.iter().map(|&(cell, _)| cell >> 4).collect();
                check_resolution(replay.res, chunks.len(), budget)?;
            }
            Some(VoxelSource::Reset) | None => {}
        }
        let unique = matches!(mode, ApplyMode::Open | ApplyMode::Restore(_));
        if let Some(elements) = self.elements {
            check_list("element", &elements.list, &elements.selected, unique)?;
        }
        if let Some(meshes) = self.meshes {
            check_list("mesh", &meshes.list, &meshes.selected, unique)?;
        }
        Ok(())
    }
}

/// The section a declared category should have.
fn part<T>(cats: Categories, flag: Categories, value: Option<T>) -> Result<Option<T>, ApplyError> {
    if !cats.contains(flag) {
        return Ok(None);
    }
    value.map(Some).ok_or_else(|| missing(flag))
}

fn missing(flag: Categories) -> ApplyError {
    ApplyError::Invalid(format!(
        "the source declares {flag:?} but holds no such section"
    ))
}

/// Chunk slots per axis of a world: `ceil(3R / 16)` (native 4.7). Only valid after
/// `voxel_world_bytes` accepted the resolution.
fn chunks_per_axis(res: u32) -> usize {
    (3 * res).div_ceil(16) as usize
}

/// The memory a voxel world of resolution `res` holds with `chunks` allocated chunks: the slot
/// table and the chunk data. `None` when `res` is 0 or the arithmetic overflows. A reader calls
/// it with 0 chunks on the header of a file, before it decodes any voxel (native 4.7 accepts any
/// resolution within a memory limit).
///
/// spec: native 4.7, arch 12.3
pub fn voxel_world_bytes(res: u32, chunks: usize) -> Option<u64> {
    if res == 0 {
        return None;
    }
    let axis = usize::try_from(res.checked_mul(3)?.div_ceil(16)).ok()?;
    let slots = axis.checked_pow(3)?;
    let table = slots.checked_mul(size_of::<ChunkSlot>())?;
    let data = chunks.min(slots).checked_mul(size_of::<Chunk>())?;
    u64::try_from(table.checked_add(data)?).ok()
}

fn check_resolution(res: u32, chunks: usize, budget: u64) -> Result<(), ApplyError> {
    let bytes = voxel_world_bytes(res, chunks).ok_or(ApplyError::VoxelResolution(res))?;
    if bytes > budget {
        return Err(ApplyError::VoxelBudget { res, bytes, budget });
    }
    Ok(())
}

fn check_list<T>(
    kind: &str,
    list: &ListSnap<T>,
    selected: &[bool],
    unique: bool,
) -> Result<(), ApplyError> {
    if list.ids.len() != list.items.len() || list.ids.len() != selected.len() {
        return Err(ApplyError::Invalid(format!(
            "the {kind} section has {} ids, {} records and {} selection flags",
            list.ids.len(),
            list.items.len(),
            selected.len()
        )));
    }
    if unique && list.ids.iter().collect::<FxHashSet<_>>().len() != list.ids.len() {
        return Err(ApplyError::Invalid(format!(
            "the {kind} section repeats an object id"
        )));
    }
    Ok(())
}

/// Applies a source to the document: section order, rotation re-quantisation under the current
/// restriction setting, size handling, `is` re-selection, palette rebuild, display-id
/// renumbering and the start queue are identical for open, merge, restore and paste. It checks
/// everything before it changes anything.
///
/// The sections are applied in the order of native 6.2 (`mp`, `vp`, `bp` and `el`, `tx`, `vx`,
/// `me`, `md`) after the selection is cleared. The rotation grid is the `g.rer` in effect after
/// the `mp` step (the properties of the document itself when the source has none); the caller
/// refreshes `EditEnv::restrict_rotations` afterwards. Elements and meshes the record normal
/// form leaves unchanged keep their `Arc`; the journal marks every object whose record differs
/// from the one it replaces.
///
/// spec: native 6.2-6.3, tools 11.3, tools 15.3, arch 9.1
pub fn apply_document(
    cx: &mut EditCtx<'_>,
    src: ApplySource<'_>,
    mode: ApplyMode,
) -> Result<ApplyReport, ApplyError> {
    apply_document_within(cx, src, mode, DEFAULT_VOXEL_BUDGET)
}

/// [`apply_document`] with the memory budget of a voxel world in bytes (see
/// [`voxel_world_bytes`]): a larger world is refused with `ApplyError::VoxelBudget` before the
/// document changes.
///
/// spec: native 4.7, native 6.2-6.3, arch 9.1, arch 12.3
pub fn apply_document_within(
    cx: &mut EditCtx<'_>,
    src: ApplySource<'_>,
    mode: ApplyMode,
    voxel_budget: u64,
) -> Result<ApplyReport, ApplyError> {
    let (snap, loaded) = match &src {
        ApplySource::Loaded(loaded) => (&loaded.sections, Some(&**loaded)),
        ApplySource::Snapshot(snap) => (*snap, None),
    };
    let cats = match mode {
        ApplyMode::Open | ApplyMode::Merge => snap.categories,
        ApplyMode::Restore(wanted) => wanted & snap.categories,
        ApplyMode::Paste { .. } => snap.categories & Categories::OBJECTS,
    };
    let sections = Sections::select(snap, loaded, cats)?;
    sections.validate(mode, voxel_budget)?;

    let doc = &mut *cx.doc;
    let sel = &mut *cx.sel;
    let mut report = ApplyReport::default();

    let had_selection = !sel.list.is_empty() || sel.primary.is_some();
    if matches!(mode, ApplyMode::Open) {
        reset_scene(doc, sel);
    }
    let clears = matches!(mode, ApplyMode::Open | ApplyMode::Merge)
        || sections.elements.is_some()
        || sections.meshes.is_some();
    if clears {
        sel.clear();
    }

    if let Some(props) = sections.props {
        let new = normal_form::share(props, normal_form::props(props));
        if *new != *doc.props {
            doc.props = new;
            doc.journal.mark_doc(DocFlags::PROPS);
        }
        if let Some(model_box) = doc.props.general.model_box {
            set_model_box(doc, model_box);
        }
    }

    if let Some(view) = sections.view {
        // Cameras that are null keep the live ones; `gu` and `gl` are not applied.
        let new = ViewProps {
            cameras: std::array::from_fn(|i| view.cameras[i].or(doc.view.cameras[i])),
            mouse_step: view.mouse_step,
            key_step: view.key_step,
            display_step: view.display_step,
            ..(*doc.view).clone()
        };
        if new != *doc.view {
            doc.view = Arc::new(new);
            doc.journal.mark_doc(DocFlags::VIEW);
        }
    }

    let restricted = doc.props.general.restrict_rotations;
    if let Some(elements) = sections.elements {
        let current = doc.blueprint.base.e;
        let blueprint = normal_form::blueprint(&elements.blueprint, restricted, current)
            .unwrap_or_else(|| elements.blueprint.clone());
        if blueprint != doc.blueprint {
            doc.blueprint = blueprint;
        }
        apply_list(
            ListParts {
                list: &mut doc.elements,
                journal: &mut doc.journal,
                sel,
                report: &mut report,
            },
            &elements.list,
            &elements.selected,
            mode,
            ObjRef::Element,
            |el| normal_form::element(el, restricted),
            place_element,
        );
    }

    // Tools 15.3 restores the textures before the elements; nothing reads them while the
    // elements are applied, so the order of native 6.2 is kept.
    if let Some(textures) = sections.textures {
        // A file without texture records leaves the list as it is (Open emptied it already).
        if loaded.is_none() || !textures.entries.is_empty() {
            doc.textures = build_textures(textures, loaded.is_some(), &mut report.warnings);
            doc.journal.mark_doc(DocFlags::TEXTURES | DocFlags::ATLAS);
        }
    }

    if let Some(voxels) = &sections.voxels {
        match voxels {
            VoxelSource::Reset => {
                if doc.voxels.take().is_some() {
                    doc.journal.mark_doc(DocFlags::VOXEL_CHUNKS);
                }
            }
            VoxelSource::Snap(snap) => {
                let world = build_world(doc, snap.res, |ao, active| {
                    VoxelWorld::from_snapshot(snap, ao, active)
                });
                doc.voxels = Some(world);
                doc.journal.mark_doc(DocFlags::VOXEL_CHUNKS);
            }
            VoxelSource::Replay(replay) => {
                let world = build_world(doc, replay.res, |ao, active| {
                    replay_world(replay, ao, active)
                });
                doc.voxels = Some(world);
                doc.journal.mark_doc(DocFlags::VOXEL_CHUNKS);
            }
        }
    }

    if let Some(meshes) = sections.meshes {
        apply_list(
            ListParts {
                list: &mut doc.meshes,
                journal: &mut doc.journal,
                sel,
                report: &mut report,
            },
            &meshes.list,
            &meshes.selected,
            mode,
            ObjRef::Mesh,
            normal_form::mesh,
            place_mesh,
        );
    }

    if let Some(display) = sections.display
        && *doc.display != **display
    {
        doc.display = Arc::clone(display);
        doc.journal.mark_doc(DocFlags::DISPLAY);
    }

    if let Some(loaded) = loaded {
        match mode {
            ApplyMode::Open => {
                doc.overrides = loaded.overrides.clone().unwrap_or_else(|| Arc::from([]));
                doc.retained.clone_from(&loaded.retained);
            }
            ApplyMode::Merge => {
                if let Some(overrides) = &loaded.overrides {
                    doc.overrides = Arc::clone(overrides);
                }
            }
            ApplyMode::Restore(_) | ApplyMode::Paste { .. } => {}
        }
    }

    prune_highlight(doc, sel);
    if clears && (had_selection || !sel.list.is_empty()) {
        doc.journal.mark_doc(DocFlags::SELECTION);
    }
    doc.revision = Rev::next();
    Ok(report)
}

/// File > Open's reset (native 6.1). The view properties and the blueprint are not on its list:
/// a file without `vp` or `bp` leaves them, and `gu` and `gl` never come from a file (native 4.1,
/// 4.3, 6.3).
fn reset_scene(doc: &mut Document, sel: &mut Selection) {
    doc.props = Arc::default();
    doc.model_box = ModelBox::EDITING_VOLUME;
    doc.elements.clear();
    doc.meshes.clear();
    doc.textures = TextureList::default();
    doc.voxels = None;
    doc.display = Arc::default();
    doc.overrides = Arc::from([]);
    doc.retained = None;
    sel.clear();
    sel.highlighted.clear();
    doc.journal.mark_doc(
        DocFlags::PROPS
            | DocFlags::MODEL_BOX
            | DocFlags::DISPLAY
            | DocFlags::TEXTURES
            | DocFlags::ATLAS
            | DocFlags::VOXEL_CHUNKS
            | DocFlags::LISTS
            | DocFlags::SELECTION
            | DocFlags::HIGHLIGHT,
    );
}

/// The pasted element: position and origin move by the offset, and a non-zero offset clears the
/// stored UV snap so the moved element keeps its stored UV (tools 11.3).
fn place_element(el: &mut Element, offset: Vec3) {
    el.base.p += offset;
    el.base.o += offset;
    if offset != Vec3::ZERO {
        el.flags.remove(ElementFlags::UV_SNAP);
    }
}

fn place_mesh(mesh: &mut Mesh, offset: Vec3) {
    mesh.base.p += offset;
    mesh.base.o += offset;
}

/// The borrows an object list update works on.
struct ListParts<'a, T> {
    list: &'a mut ObjList<T>,
    journal: &'a mut ChangeJournal,
    sel: &'a mut Selection,
    report: &'a mut ApplyReport,
}

/// Brings one object list to the state of the snapshot: Open and Restore replace the list and
/// keep the ids, Merge and Paste append new objects (Paste selects all of them).
fn apply_list<T: Clone>(
    parts: ListParts<'_, T>,
    snap: &ListSnap<T>,
    selected: &[bool],
    mode: ApplyMode,
    obj: fn(ObjId) -> ObjRef,
    normalise: impl Fn(&T) -> Option<T>,
    place: fn(&mut T, Vec3),
) {
    let ListParts {
        list,
        journal,
        sel,
        report,
    } = parts;
    let fixed = |item: &Arc<T>| normalise(item).unwrap_or_else(|| T::clone(item));
    let ids: Vec<ObjId> = match mode {
        ApplyMode::Open | ApplyMode::Restore(_) => {
            let items: Vec<Arc<T>> = snap
                .items
                .iter()
                .map(|item| normalise(item).map_or_else(|| Arc::clone(item), Arc::new))
                .collect();
            for (&id, item) in snap.ids.iter().zip(&items) {
                let kept = list
                    .position(id)
                    .and_then(|at| list.items().get(at))
                    .is_some_and(|old| Arc::ptr_eq(old, item));
                if !kept {
                    journal.mark_object(id, ChangeFlags::all());
                }
            }
            list.restore(&ListSnap {
                ids: snap.ids.clone(),
                items,
            });
            snap.ids.clone()
        }
        ApplyMode::Merge => append(list, journal, snap, fixed),
        ApplyMode::Paste { offset } => append(list, journal, snap, |item| {
            let mut out = fixed(item);
            place(&mut out, offset);
            out
        }),
    };
    journal.mark_doc(DocFlags::LISTS);
    let paste = matches!(mode, ApplyMode::Paste { .. });
    for (&id, &flag) in ids.iter().zip(selected) {
        if flag || paste {
            sel.add(obj(id));
        }
    }
    report.created.extend(ids.iter().map(|&id| obj(id)));
}

/// Pushes a new object for every record of the snapshot.
fn append<T: Clone>(
    list: &mut ObjList<T>,
    journal: &mut ChangeJournal,
    snap: &ListSnap<T>,
    make: impl Fn(&Arc<T>) -> T,
) -> Vec<ObjId> {
    snap.items
        .iter()
        .map(|item| {
            let id = list.push(make(item));
            journal.mark_object(id, ChangeFlags::all());
            id
        })
        .collect()
}

/// The texture list of the section: later records with a label replace the image of the earlier
/// one. A file's null source path becomes empty (native 6.3, `tx`), and the label `error`, which
/// Cubik routes to its internal entry, is listed as a normal texture with a warning (native 4.5).
fn build_textures(src: &TextureList, from_file: bool, warnings: &mut Vec<String>) -> TextureList {
    let mut entries: Vec<TextureEntry> = Vec::with_capacity(src.entries.len());
    let mut at: FxHashMap<&str, usize> = FxHashMap::default();
    for record in &src.entries {
        let mut entry = record.clone();
        if from_file {
            entry.texture.source.path.get_or_insert_with(String::new);
            if &*entry.label == "error" {
                warnings.push("the texture label `error` is listed as a normal texture".to_owned());
            }
        }
        if let Some(&first) = at.get(&*record.label) {
            warnings.push(format!(
                "texture label `{}` repeated: the later record replaces the image",
                record.label
            ));
            entries[first].texture = entry.texture;
        } else {
            at.insert(&record.label, entries.len());
            entries.push(entry);
        }
    }
    TextureList { entries }
}

/// The world of a voxel section. A world of the same resolution keeps its settings (active box,
/// ambient occlusion, setup corners): the original clears it and writes the cells again. Any
/// other case creates a world with the ambient occlusion of `mp.obj.ao`, the active box of the
/// model box and the model box as setup corners.
///
/// spec: native 6.3 (`vx`), tools 7.10
fn build_world(
    doc: &Document,
    res: u32,
    make: impl FnOnce(bool, IBox3) -> VoxelWorld,
) -> VoxelWorld {
    let (ao, active, corners): (bool, IBox3, WBox3) = match &doc.voxels {
        Some(live) if live.res == res => (live.ao, live.active_box, live.setup_corners),
        _ => (
            doc.props.mesh_export.ambient_occlusion,
            mapping::active_box(doc.model_box, res),
            doc.model_box,
        ),
    };
    let mut world = make(ao, active);
    world.setup_corners = corners;
    world
}

/// A world filled by the full writer from records in file order. It starts as a restored blank
/// world, so every chunk is dirty like after a restore.
fn replay_world(replay: &VoxelReplay, ao: bool, active: IBox3) -> VoxelWorld {
    let n = (3 * replay.res).div_ceil(16) as usize;
    let blank = VoxelSnap {
        res: replay.res,
        slots: vec![ChunkSlot::Unallocated; n * n * n].into(),
    };
    let mut world = VoxelWorld::from_snapshot(&blank, ao, active);
    world.write_many(&mut replay.cells.iter().copied(), WriteKind::Full);
    world
}

/// Drops highlighted objects that no longer exist.
fn prune_highlight(doc: &mut Document, sel: &mut Selection) {
    let before = sel.highlighted.len();
    sel.highlighted.retain(|obj| match *obj {
        ObjRef::Element(id) => doc.elements.get(id).is_some(),
        ObjRef::Mesh(id) => doc.meshes.get(id).is_some(),
        ObjRef::Blueprint | ObjRef::ModelOrigin => true,
    });
    if sel.highlighted.len() != before {
        doc.journal.mark_doc(DocFlags::HIGHLIGHT);
    }
}

#[cfg(test)]
mod tests {
    use cuboid_image::{Image, RgbaBuf};
    use glam::Vec2;

    use super::*;
    use crate::capture::{CaptureScope, capture};
    use crate::contract::{CameraState, DisplaySlotId, ModelProps};
    use crate::invariants::check_invariants;
    use crate::objects::{Blueprint, Label, MeshKind, Submesh, Texture};
    use crate::setters::fixture::World;
    use crate::snapshot::{ElementsSnap, MeshesSnap};
    use crate::uv::UvRect;

    const V0: u32 = 0xFF11_2233;
    const V1: u32 = 0xFF44_5566;
    const V2: u32 = 0xFF77_8899;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn snapshot_of(w: &World, cats: Categories) -> Snapshot {
        capture(
            &w.doc,
            &w.sel,
            CaptureScope {
                categories: cats,
                selection_only: false,
            },
        )
    }

    fn restore(
        w: &mut World,
        snap: &Snapshot,
        cats: Categories,
    ) -> Result<ApplyReport, ApplyError> {
        apply_document(
            &mut w.cx(),
            ApplySource::Snapshot(snap),
            ApplyMode::Restore(cats),
        )
    }

    fn apply_loaded(
        w: &mut World,
        sections: Snapshot,
        mode: ApplyMode,
    ) -> Result<ApplyReport, ApplyError> {
        let loaded = LoadedDoc {
            sections,
            voxel_replay: None,
            overrides: None,
            retained: None,
        };
        apply_document(&mut w.cx(), ApplySource::Loaded(Box::new(loaded)), mode)
    }

    fn empty(categories: Categories) -> Snapshot {
        Snapshot {
            categories,
            props: None,
            view: None,
            elements: None,
            textures: None,
            voxels: None,
            meshes: None,
            display: None,
        }
    }

    fn texture(label: &str, size: u32) -> TextureEntry {
        let image = Image::from_rgba(RgbaBuf::new(size, size));
        TextureEntry {
            label: Label::from(label),
            texture: Texture::new(Arc::new(image)),
        }
    }

    fn textures(entries: Vec<TextureEntry>) -> Arc<TextureList> {
        Arc::new(TextureList { entries })
    }

    fn elements_section(w: &World) -> ElementsSnap {
        snapshot_of(w, Categories::ELEMENTS).elements.unwrap()
    }

    fn world_with(res: u32, cells: &[(I3, u32)]) -> VoxelWorld {
        let mut world = VoxelWorld::new(res, true, mapping::grid_box(res));
        world.write_many(&mut cells.iter().copied(), WriteKind::Unchecked);
        world
    }

    fn cell(x: i32, y: i32, z: i32) -> I3 {
        I3::new(x, y, z)
    }

    // --- Apply order and the selection (native 6.2) --------------------------------------------

    #[test]
    fn a_restore_keeps_the_ids_renumbers_the_display_ids_and_reselects() {
        let mut w = World::new();
        let ids: Vec<_> = (0..3)
            .map(|i| w.add_element(v(i as f32, 0.0, 0.0), Vec3::ONE))
            .collect();
        let mesh = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        w.select(&[ObjRef::Element(ids[1]), ObjRef::Mesh(mesh)]);
        let snap = snapshot_of(&w, Categories::OBJECTS);

        w.doc.elements.remove_set(&FxHashSet::from_iter([ids[0]]));
        w.add_element(Vec3::ZERO, Vec3::ONE);
        w.sel.clear();
        w.sel.add(ObjRef::Element(ids[2]));

        let report = restore(&mut w, &snap, Categories::OBJECTS).unwrap();

        assert_eq!(w.doc.elements.ids(), ids.as_slice());
        assert_eq!(w.doc.elements.display_ids(), [1, 2, 3]);
        assert_eq!(w.doc.meshes.display_ids(), [1]);
        assert_eq!(
            w.sel.list.iter().copied().collect::<Vec<_>>(),
            [ObjRef::Element(ids[1]), ObjRef::Mesh(mesh)]
        );
        assert_eq!(w.sel.primary, Some(ObjRef::Element(ids[1])));
        let created: Vec<_> = ids
            .iter()
            .map(|&id| ObjRef::Element(id))
            .chain([ObjRef::Mesh(mesh)])
            .collect();
        assert_eq!(report.created, created);
        assert!(check_invariants(&w.doc, &w.sel).is_ok());
    }

    #[test]
    fn the_selection_is_cleared_only_when_elements_or_meshes_are_restored() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.select(&[ObjRef::Element(a)]);
        let snap = snapshot_of(&w, Categories::ALL);
        w.sel.clear();
        w.sel.add(ObjRef::Element(a));

        restore(
            &mut w,
            &snap,
            Categories::TEXTURES | Categories::MODEL_DISPLAY,
        )
        .unwrap();
        assert!(
            w.sel.contains(ObjRef::Element(a)),
            "textures and display keep it"
        );

        restore(&mut w, &snap, Categories::ELEMENTS).unwrap();
        assert_eq!(
            w.sel.list.len(),
            1,
            "cleared, then the record's own flag selects the element again"
        );

        w.sel.add(ObjRef::ModelOrigin);
        restore(&mut w, &snap, Categories::MESHES).unwrap();
        assert!(w.sel.list.is_empty(), "restoring meshes clears it too");
    }

    #[test]
    fn a_restore_touches_only_the_named_categories() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let m = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        let snap = snapshot_of(&w, Categories::ALL);

        w.edit_element(a, |el| el.base.hidden = true);
        w.doc.meshes.remove_set(&FxHashSet::from_iter([m]));
        Arc::make_mut(&mut w.doc.props).general.name = "changed".to_owned();

        restore(&mut w, &snap, Categories::ELEMENTS).unwrap();

        assert!(!w.element(a).base.hidden);
        assert!(
            w.doc.meshes.is_empty(),
            "meshes are not part of the request"
        );
        assert_eq!(w.doc.props.general.name, "changed");

        // A category the snapshot lacks is skipped, not an error.
        let only_elements = snapshot_of(&w, Categories::ELEMENTS);
        restore(&mut w, &only_elements, Categories::ALL).unwrap();
    }

    #[test]
    fn a_restore_shares_the_records_it_does_not_change() {
        let mut w = World::new();
        let fixed = w.add_element(v(0.0, 0.0, 0.0), Vec3::ONE);
        let off_grid = w.add_element(v(1.0, 0.0, 0.0), Vec3::ONE);
        w.edit_element(fixed, |el| el.base.e = v(0.0, 45.0, 0.0));
        w.edit_element(off_grid, |el| el.base.e = v(0.0, 30.0, 0.0));
        let snap = snapshot_of(&w, Categories::ELEMENTS);
        w.doc.journal.clear();

        restore(&mut w, &snap, Categories::ELEMENTS).unwrap();

        let list = &snap.elements.as_ref().unwrap().list;
        assert!(Arc::ptr_eq(&w.doc.elements.items()[0], &list.items[0]));
        assert!(!Arc::ptr_eq(&w.doc.elements.items()[1], &list.items[1]));
        assert_eq!(w.element(off_grid).base.e, v(0.0, 22.5, 0.0));
        assert_eq!(
            list.items[1].base.e,
            v(0.0, 30.0, 0.0),
            "the snapshot is not edited"
        );
        // The id survives, and only the changed record is journalled.
        assert_eq!(w.doc.elements.ids(), [fixed, off_grid]);
        assert!(!w.doc.journal.objects().contains_key(&fixed));
        assert!(w.doc.journal.objects().contains_key(&off_grid));
        assert!(w.doc.journal.doc().contains(DocFlags::LISTS));
    }

    // --- Model properties and view (native 6.3 `mp`, `vp`) -------------------------------------

    #[test]
    fn model_properties_are_copied_and_the_stored_model_box_is_applied() {
        let mut w = World::new();
        let mut props = ModelProps::default();
        props.general.name = "say \"hi\"".to_owned();
        props.general.origin = v(1.0, 2.0, 3.0);
        props.general.model_box = Some(ModelBox {
            min: v(-4.0, 0.0, -8.0),
            max: v(8.0, 12.0, 4.0),
        });
        props.save_path = "x.cubik".to_owned();
        let mut snap = empty(Categories::MODEL_PROPERTIES);
        snap.props = Some(Arc::new(props));

        restore(&mut w, &snap, Categories::ALL).unwrap();

        assert_eq!(w.doc.props.save_path, "x.cubik");
        assert_eq!(w.doc.props.general.origin, v(1.0, 2.0, 3.0));
        assert!(!w.doc.props.general.name.contains('"'));
        assert_eq!(w.doc.model_box.min, v(-4.0, 0.0, -8.0));
        assert_eq!(w.doc.model_box.max, v(8.0, 12.0, 4.0));
        assert!(
            w.doc
                .journal
                .doc()
                .contains(DocFlags::PROPS | DocFlags::MODEL_BOX)
        );
    }

    #[test]
    fn a_null_model_box_leaves_the_live_one_and_the_voxel_world_follows_a_stored_one() {
        let mut w = World::new();
        w.doc.voxels = Some(world_with(4, &[]));
        let mut snap = empty(Categories::MODEL_PROPERTIES);
        snap.props = Some(Arc::new(ModelProps::default()));
        crate::setters::set_model_box(
            &mut w.doc,
            ModelBox {
                min: v(-8.0, -8.0, -8.0),
                max: v(8.0, 8.0, 8.0),
            },
        );
        let before = w.doc.model_box;
        Arc::make_mut(&mut w.doc.props).general.model_box = None;

        restore(&mut w, &snap, Categories::ALL).unwrap();
        assert_eq!(w.doc.model_box, before, "no stored box, nothing to apply");
        assert!(w.doc.props.general.model_box.is_none());

        let mut props = ModelProps::default();
        props.general.model_box = Some(ModelBox::EDITING_VOLUME);
        snap.props = Some(Arc::new(props));
        restore(&mut w, &snap, Categories::ALL).unwrap();
        assert_eq!(w.doc.model_box, ModelBox::EDITING_VOLUME);
        let world = w.doc.voxels.as_ref().unwrap();
        assert_eq!(
            world.active_box,
            mapping::active_box(ModelBox::EDITING_VOLUME, 4)
        );
    }

    #[test]
    fn view_properties_apply_the_cameras_that_exist_and_ignore_gu_and_gl() {
        let mut w = World::new();
        let live_cam = CameraState {
            pos: v(1.0, 1.0, 1.0),
            rot: glam::Quat::IDENTITY,
            ortho: false,
            ortho_size: 5.0,
            fov_deg: 60.0,
        };
        let new_cam = CameraState {
            pos: v(9.0, 0.0, 0.0),
            ..live_cam
        };
        w.doc.view = Arc::new(ViewProps {
            cameras: [Some(live_cam), Some(live_cam), None, None],
            uv_step: 0.25,
            large_grid: true,
            ..ViewProps::default()
        });

        let src = ViewProps {
            cameras: [None, Some(new_cam), Some(new_cam), None],
            mouse_step: 2.0,
            key_step: 0.0625,
            display_step: 4.0,
            ..ViewProps::default()
        };
        let mut snap = empty(Categories::VIEW_PROPERTIES);
        snap.view = Some(Arc::new(src));

        restore(&mut w, &snap, Categories::VIEW_PROPERTIES).unwrap();

        let view = &w.doc.view;
        assert_eq!(
            view.cameras[0],
            Some(live_cam),
            "null keeps the live camera"
        );
        assert_eq!(view.cameras[1], Some(new_cam));
        assert_eq!(view.cameras[2], Some(new_cam));
        assert_eq!(view.cameras[3], None);
        assert_eq!(
            (view.mouse_step, view.key_step, view.display_step),
            (2.0, 0.0625, 4.0)
        );
        assert_eq!((view.uv_step, view.large_grid), (0.25, true));
        assert!(w.doc.journal.doc().contains(DocFlags::VIEW));
    }

    // --- Blueprint, elements and meshes (native 6.3 `bp`, `el`, `me`, rotations, sizes) --------

    #[test]
    fn the_blueprint_record_is_applied_with_the_size_setter_and_its_origin_follows() {
        let mut w = World::new();
        let mut section = elements_section(&w);
        let base = &mut section.blueprint.base;
        base.p = v(3.0, 4.0, -5.0);
        base.o = v(9.0, 9.0, 9.0);
        base.s = v(0.0, 2.0, 0.0005);
        base.e = v(0.0, 30.0, 0.0);
        base.hidden = true;
        base.locked = true;
        base.label = Label::from("Stage");
        let mut snap = empty(Categories::ELEMENTS);
        snap.elements = Some(section);

        restore(&mut w, &snap, Categories::ELEMENTS).unwrap();

        let bp = &w.doc.blueprint.base;
        assert_eq!(bp.p, v(3.0, 4.0, -5.0));
        assert_eq!(bp.o, bp.p);
        assert_eq!(bp.s, v(0.001, 2.0, 0.001));
        assert_eq!(bp.e, v(0.0, 22.5, 0.0), "restricted: the 22.5 degree grid");
        assert!(bp.hidden && bp.locked);
        assert_eq!(&*bp.label, "Stage");
    }

    #[test]
    fn a_blueprint_rotation_equal_to_the_live_one_is_not_normalised() {
        let mut w = World::new();
        w.doc.blueprint.base.e = v(0.0, 7.0, 0.0);
        let mut snap = empty(Categories::ELEMENTS);
        snap.elements = Some(elements_section(&w));
        restore(&mut w, &snap, Categories::ELEMENTS).unwrap();
        assert_eq!(w.doc.blueprint.base.e, v(0.0, 7.0, 0.0));

        w.doc.blueprint.base.e = Vec3::ZERO;
        restore(&mut w, &snap, Categories::ELEMENTS).unwrap();
        assert_eq!(
            w.doc.blueprint.base.e,
            Vec3::ZERO,
            "7 degrees on the 22.5 grid"
        );
    }

    #[test]
    fn element_records_get_their_colour_rotation_size_and_flags_from_the_record() {
        let mut w = World::new();
        let a = w.add_element(v(1.0, 2.0, 3.0), v(0.0005, 1.0, 2.0));
        w.edit_element(a, |el| {
            el.color = [0.5, 0.25, 1.0, 0.3];
            el.base.e = v(0.0, -400.0, 0.0);
            el.base.o = v(7.0, 7.0, 7.0);
            el.flags = ElementFlags::UV_SNAP | ElementFlags::SHADE;
        });
        let snap = snapshot_of(&w, Categories::ELEMENTS);
        let mut target = World::new();

        restore(&mut target, &snap, Categories::ELEMENTS).unwrap();

        let el = target.element(a);
        assert_eq!(el.color[3], 1.0, "alpha forced to 255");
        assert_eq!(el.color[2], 1.0);
        assert_eq!(
            el.base.s,
            v(0.0005, 1.0, 2.0),
            "sizes are applied as stored"
        );
        assert_eq!(el.base.p, v(1.0, 2.0, 3.0));
        assert_eq!(el.base.o, v(7.0, 7.0, 7.0), "the pivot is kept");
        assert_eq!(
            el.base.e,
            v(0.0, -45.0, 0.0),
            "(-400 + 360) % 360 = -40, then 22.5 steps"
        );
        assert_eq!(
            el.flags,
            ElementFlags::UV_SNAP | ElementFlags::SHADE,
            "the stored flags win over the rotation's UV snap reset"
        );
    }

    #[test]
    fn the_restriction_in_effect_after_the_property_step_picks_the_rotation_grid() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.edit_element(a, |el| el.base.e = v(0.0, 30.0, 0.0));
        let m = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        w.doc.meshes.edit(m, &mut w.doc.journal).unwrap().base.e = v(0.0, 22.5, 0.0);
        let mut snap = snapshot_of(&w, Categories::OBJECTS);
        let mut props = ModelProps::default();
        props.general.restrict_rotations = false;
        snap.props = Some(Arc::new(props));
        snap.categories |= Categories::MODEL_PROPERTIES;

        let mut open = World::new();
        assert!(open.doc.props.general.restrict_rotations);
        restore(&mut open, &snap, Categories::ALL).unwrap();
        assert_eq!(open.element(a).base.e.y, 30.0, "5 degrees once rer is off");
        assert_eq!(
            open.doc.meshes.get(m).unwrap().base.e.y,
            20.0,
            "meshes: 5 degrees"
        );

        let mut plain = snapshot_of(&w, Categories::OBJECTS);
        plain.props = None;
        let mut restricted = World::new();
        restore(&mut restricted, &plain, Categories::OBJECTS).unwrap();
        assert_eq!(restricted.element(a).base.e.y, 22.5);
        assert_eq!(restricted.doc.meshes.get(m).unwrap().base.e.y, 20.0);
    }

    #[test]
    fn mesh_records_keep_their_scale_and_geometry_and_use_five_degrees() {
        let mut w = World::new();
        let m = w.add_mesh(v(1.0, 0.0, 0.0), v(0.0, -2.0, 0.0001));
        {
            let doc = &mut w.doc;
            let mesh = doc.meshes.edit(m, &mut doc.journal).unwrap();
            mesh.kind = MeshKind::Custom;
            mesh.base.e = v(11.0, 0.0, 0.0);
            mesh.geometry = Arc::new(crate::objects::MeshGeometry {
                vertices: vec![Vec3::ONE],
                ..Default::default()
            });
            mesh.submeshes = Arc::from([Submesh {
                triangles: vec![0, 0, 0],
                color: [0.5; 4],
                texture: None,
            }]);
        }
        let snap = snapshot_of(&w, Categories::MESHES);
        let mut target = World::new();

        restore(&mut target, &snap, Categories::MESHES).unwrap();

        let mesh = target.doc.meshes.get(m).unwrap();
        assert_eq!(
            mesh.base.s,
            v(0.0, -2.0, 0.0001),
            "the scale is not clamped"
        );
        assert_eq!(mesh.base.e.x, 10.0);
        assert_eq!(mesh.kind, MeshKind::Custom);
        assert_eq!(mesh.geometry.vertices, [Vec3::ONE]);
        assert_eq!(mesh.submeshes.len(), 1);
        assert!(Arc::ptr_eq(
            &mesh.geometry,
            &snap.meshes.as_ref().unwrap().list.items[0].geometry
        ));
    }

    fn edit_mesh(w: &mut World, id: ObjId, f: impl FnOnce(&mut Mesh)) {
        let doc = &mut w.doc;
        f(doc.meshes.edit(id, &mut doc.journal).unwrap());
    }

    #[test]
    fn a_restored_or_pasted_primitive_takes_the_builder_tint() {
        let mut w = World::new();
        let prim = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        let custom = w.add_mesh(v(2.0, 0.0, 0.0), Vec3::ONE);
        let red = [1.0, 0.0, 0.0, 1.0];
        edit_mesh(&mut w, prim, |m| m.tint = red);
        edit_mesh(&mut w, custom, |m| {
            m.kind = MeshKind::Custom;
            m.tint = red;
        });
        let snap = snapshot_of(&w, Categories::MESHES);

        restore(&mut w, &snap, Categories::MESHES).unwrap();
        assert_eq!(w.doc.meshes.get(prim).unwrap().tint, [1.0; 4]);
        assert_eq!(w.doc.meshes.get(custom).unwrap().tint, red);

        let report = paste(&mut w, &snap, Vec3::ZERO);
        let tints: Vec<_> = report
            .created
            .iter()
            .map(|obj| match *obj {
                ObjRef::Mesh(id) => w.doc.meshes.get(id).unwrap().tint,
                _ => unreachable!("only meshes are pasted"),
            })
            .collect();
        assert_eq!(tints, [[1.0; 4], red]);
    }

    #[test]
    fn sub_mesh_colours_are_packed_on_restore_and_paste() {
        let mut w = World::new();
        let id = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        edit_mesh(&mut w, id, |m| {
            m.kind = MeshKind::Custom;
            m.submeshes = Arc::from([Submesh {
                triangles: vec![0, 1, 2],
                color: [0.5, 1.2, 0.0, 1.0],
                texture: None,
            }]);
        });
        let snap = snapshot_of(&w, Categories::MESHES);
        let packed = [127.0 / 255.0, 50.0 / 255.0, 0.0, 1.0];

        restore(&mut w, &snap, Categories::MESHES).unwrap();
        assert_eq!(w.doc.meshes.get(id).unwrap().submeshes[0].color, packed);

        let report = paste(&mut w, &snap, Vec3::ZERO);
        let ObjRef::Mesh(copy) = report.created[0] else {
            unreachable!("only meshes are pasted")
        };
        assert_eq!(w.doc.meshes.get(copy).unwrap().submeshes[0].color, packed);
        assert_eq!(
            snap.meshes.as_ref().unwrap().list.items[0].submeshes[0].color,
            [0.5, 1.2, 0.0, 1.0],
            "the snapshot keeps its colours"
        );
    }

    // --- Textures (native 6.3 `tx`) ------------------------------------------------------------

    #[test]
    fn a_restore_clears_the_texture_list_and_adds_every_entry() {
        let mut w = World::new();
        w.doc.textures.entries = vec![texture("old", 2)];
        let mut snap = empty(Categories::TEXTURES);
        snap.textures = Some(textures(vec![texture("a", 1), texture("b", 2)]));
        restore(&mut w, &snap, Categories::TEXTURES).unwrap();
        let labels: Vec<_> = w
            .doc
            .textures
            .entries
            .iter()
            .map(|e| e.label.to_string())
            .collect();
        assert_eq!(labels, ["a", "b"]);
        assert!(
            w.doc
                .journal
                .doc()
                .contains(DocFlags::TEXTURES | DocFlags::ATLAS)
        );

        snap.textures = Some(textures(Vec::new()));
        restore(&mut w, &snap, Categories::TEXTURES).unwrap();
        assert!(
            w.doc.textures.entries.is_empty(),
            "an undo of a clear empties the list"
        );
    }

    #[test]
    fn a_file_without_texture_records_leaves_the_list_and_one_with_records_replaces_it() {
        let mut w = World::new();
        w.doc.textures.entries = vec![texture("keep", 2)];
        let mut sections = empty(Categories::TEXTURES);
        sections.textures = Some(textures(Vec::new()));
        apply_loaded(&mut w, sections.clone(), ApplyMode::Merge).unwrap();
        assert_eq!(w.doc.textures.entries.len(), 1, "skipped when empty");

        let mut entry = texture("new", 1);
        entry.texture.source.path = None;
        entry.texture.source.file_name = Some("n".to_owned());
        sections.textures = Some(textures(vec![entry]));
        apply_loaded(&mut w, sections, ApplyMode::Merge).unwrap();
        assert_eq!(w.doc.textures.entries.len(), 1);
        let entry = &w.doc.textures.entries[0];
        assert_eq!(&*entry.label, "new");
        assert_eq!(
            entry.texture.source.path.as_deref(),
            Some(""),
            "a null `fp` becomes empty"
        );
    }

    #[test]
    fn a_repeated_label_replaces_the_earlier_image_and_error_is_listed_with_a_warning() {
        let mut w = World::new();
        let first = texture("a", 1);
        let second = texture("a", 3);
        let other = texture("error", 2);
        let wanted = Arc::clone(&second.texture.image);
        let mut sections = empty(Categories::TEXTURES);
        sections.textures = Some(textures(vec![first, other, second]));

        let report = apply_loaded(&mut w, sections, ApplyMode::Open).unwrap();

        let entries = &w.doc.textures.entries;
        assert_eq!(entries.len(), 2);
        assert_eq!(&*entries[0].label, "a");
        assert!(Arc::ptr_eq(&entries[0].texture.image, &wanted));
        assert_eq!(&*entries[1].label, "error");
        assert_eq!(report.warnings.len(), 2);
    }

    // --- Voxels (native 6.3 `vx`, tools 7.10) --------------------------------------------------

    fn voxel_snapshot(world: Option<&VoxelWorld>) -> Snapshot {
        let mut snap = empty(Categories::VOXELS);
        snap.voxels = Some(world.map(VoxelWorld::snapshot));
        snap
    }

    #[test]
    fn a_snapshot_without_a_world_resets_an_existing_one() {
        let mut w = World::new();
        w.doc.voxels = Some(world_with(2, &[(cell(1, 1, 1), V0)]));
        let snap = voxel_snapshot(None);
        w.doc.journal.clear();

        restore(&mut w, &snap, Categories::VOXELS).unwrap();
        assert!(w.doc.voxels.is_none());
        assert!(w.doc.journal.doc().contains(DocFlags::VOXEL_CHUNKS));

        w.doc.journal.clear();
        restore(&mut w, &snap, Categories::VOXELS).unwrap();
        assert!(w.doc.voxels.is_none());
        assert!(!w.doc.journal.doc().contains(DocFlags::VOXEL_CHUNKS));
    }

    #[test]
    fn a_world_is_created_with_the_settings_of_the_document() {
        let mut w = World::new();
        Arc::make_mut(&mut w.doc.props)
            .mesh_export
            .ambient_occlusion = false;
        crate::setters::set_model_box(
            &mut w.doc,
            ModelBox {
                min: v(-8.0, -8.0, -8.0),
                max: v(8.0, 8.0, 8.0),
            },
        );
        let source = world_with(4, &[(cell(3, 3, 3), V0), (cell(0, 0, 0), V1)]);
        let snap = voxel_snapshot(Some(&source));

        restore(&mut w, &snap, Categories::VOXELS).unwrap();

        let world = w.doc.voxels.as_ref().unwrap();
        assert!(!world.ao, "taken from mp.obj.ao");
        assert_eq!(world.active_box, mapping::active_box(w.doc.model_box, 4));
        assert_eq!(world.setup_corners, w.doc.model_box);
        assert_eq!(world.read(cell(3, 3, 3)), V0);
        assert_eq!(
            world.read(cell(0, 0, 0)),
            0,
            "outside the active box: dropped"
        );
        assert_eq!(world.palette.values(), [0, V0]);
        assert!(world.is_dirty(), "the whole world is meshed again");
    }

    #[test]
    fn a_world_of_the_same_resolution_keeps_its_active_box_ao_and_setup_corners() {
        let mut w = World::new();
        let mut live = world_with(4, &[(cell(5, 5, 5), V2)]);
        live.ao = false;
        let corners = ModelBox {
            min: v(-1.0, -1.0, -1.0),
            max: v(1.0, 1.0, 1.0),
        };
        live.setup_corners = corners;
        live.set_active_box(IBox3 {
            min: cell(2, 2, 2),
            max: cell(9, 9, 9),
        });
        w.doc.voxels = Some(live);
        let source = world_with(4, &[(cell(1, 1, 1), V0), (cell(4, 4, 4), V1)]);
        let snap = voxel_snapshot(Some(&source));

        restore(&mut w, &snap, Categories::VOXELS).unwrap();

        let world = w.doc.voxels.as_ref().unwrap();
        assert!(!world.ao);
        assert_eq!(world.setup_corners, corners);
        assert_eq!(world.active_box.min, cell(2, 2, 2));
        assert_eq!(world.read(cell(5, 5, 5)), 0, "the old cells are gone");
        assert_eq!(world.read(cell(4, 4, 4)), V1);
        assert_eq!(world.read(cell(1, 1, 1)), 0);
        assert_eq!(world.palette.values(), [0, V1]);

        // Another resolution takes the settings of the document instead.
        let other = voxel_snapshot(Some(&world_with(2, &[])));
        restore(&mut w, &other, Categories::VOXELS).unwrap();
        let world = w.doc.voxels.as_ref().unwrap();
        assert_eq!(world.res, 2);
        assert_eq!(world.setup_corners, w.doc.model_box);
    }

    #[test]
    fn a_restore_forgets_erased_chunks_and_rebuilds_the_palette_in_scan_order() {
        let mut source = world_with(
            8,
            &[
                (cell(20, 0, 0), V0),
                (cell(1, 5, 5), V1),
                (cell(1, 5, 4), V2),
                (cell(2, 2, 2), V2),
            ],
        );
        source.write_many(&mut [(cell(20, 0, 0), 0)].into_iter(), WriteKind::Unchecked);
        assert!(
            source
                .slots()
                .iter()
                .any(|s| matches!(s, ChunkSlot::EmptyWritten)),
            "the erased chunk is still listed by a save"
        );
        let snap = voxel_snapshot(Some(&source));
        let mut w = World::new();

        restore(&mut w, &snap, Categories::VOXELS).unwrap();

        let world = w.doc.voxels.as_ref().unwrap();
        assert_eq!(
            world.palette.values(),
            [0, V2, V1],
            "x, y, z scan order of what is left"
        );
        assert!(
            world
                .slots()
                .iter()
                .all(|s| !matches!(s, ChunkSlot::EmptyWritten))
        );
        assert_eq!(world.read(cell(20, 0, 0)), 0);
        assert_eq!(world.read(cell(1, 5, 4)), V2);
    }

    #[test]
    fn records_in_file_order_are_replayed_through_the_full_writer() {
        let mut w = World::new();
        let replay = VoxelReplay {
            res: 4,
            cells: vec![
                (cell(5, 5, 5), V1),
                (cell(1, 1, 1), V0),
                (cell(5, 5, 5), V2),
                (cell(40, 0, 0), V0),
            ],
        };
        let mut sections = empty(Categories::VOXELS);
        sections.voxels = Some(None);
        let loaded = LoadedDoc {
            sections,
            voxel_replay: Some(replay),
            overrides: None,
            retained: None,
        };

        apply_document(
            &mut w.cx(),
            ApplySource::Loaded(Box::new(loaded)),
            ApplyMode::Open,
        )
        .unwrap();

        let world = w.doc.voxels.as_ref().unwrap();
        assert_eq!(world.read(cell(5, 5, 5)), V2, "the later record wins");
        assert_eq!(
            world.palette.values(),
            [0, V1, V0, V2],
            "every value in file order, the overwritten one included"
        );
        assert!(world.is_dirty());
    }

    #[test]
    fn a_voxel_resolution_is_refused_only_above_the_memory_budget() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let mut snap = snapshot_of(&w, Categories::ELEMENTS);
        snap.categories |= Categories::VOXELS;
        let blank = |res: u32| {
            let n = (3 * res).div_ceil(16) as usize;
            VoxelSnap {
                res,
                slots: vec![ChunkSlot::Unallocated; n * n * n].into(),
            }
        };
        let within = |w: &mut World, snap: &Snapshot, budget| {
            apply_document_within(
                &mut w.cx(),
                ApplySource::Snapshot(snap),
                ApplyMode::Restore(Categories::ALL),
                budget,
            )
        };

        // Above the editor's 254 and within the budget: accepted (native 4.7).
        snap.voxels = Some(Some(blank(300)));
        within(&mut w, &snap, DEFAULT_VOXEL_BUDGET).unwrap();
        assert_eq!(w.doc.voxels.as_ref().map(|world| world.res), Some(300));

        // Above the budget: refused with the numbers, and nothing changes.
        w.doc.voxels = None;
        w.edit_element(a, |el| el.base.hidden = true);
        let revision = w.doc.revision;
        let bytes = voxel_world_bytes(300, 0).unwrap();
        let err = within(&mut w, &snap, bytes - 1).unwrap_err();
        assert!(matches!(
            err,
            ApplyError::VoxelBudget { res: 300, bytes: b, budget } if b == bytes && budget == bytes - 1
        ));
        assert!(err.to_string().contains("budget"));
        assert!(w.doc.voxels.is_none());
        assert_eq!(w.doc.revision, revision);
        assert!(w.element(a).base.hidden);
        within(&mut w, &snap, bytes).unwrap();

        // The default budget refuses a grid whose slot table alone is too large.
        snap.voxels = Some(Some(VoxelSnap {
            res: 100_000,
            slots: Arc::from([]),
        }));
        assert!(matches!(
            restore(&mut w, &snap, Categories::ALL),
            Err(ApplyError::VoxelBudget { res: 100_000, .. })
        ));

        // Zero and an overflowing grid are out of range; a short slot table is invalid.
        for res in [0, u32::MAX] {
            snap.voxels = Some(Some(VoxelSnap {
                res,
                slots: Arc::from([]),
            }));
            assert!(matches!(
                restore(&mut w, &snap, Categories::ALL),
                Err(ApplyError::VoxelResolution(r)) if r == res
            ));
        }
        snap.voxels = Some(Some(VoxelSnap {
            res: 4,
            slots: Arc::from([]),
        }));
        assert!(matches!(
            restore(&mut w, &snap, Categories::ALL),
            Err(ApplyError::Invalid(_))
        ));
    }

    #[test]
    fn a_replay_without_the_voxel_category_is_refused() {
        let mut w = World::new();
        let loaded = LoadedDoc {
            sections: empty(Categories::empty()),
            voxel_replay: Some(VoxelReplay {
                res: 4,
                cells: vec![],
            }),
            overrides: None,
            retained: None,
        };
        let result = apply_document(
            &mut w.cx(),
            ApplySource::Loaded(Box::new(loaded)),
            ApplyMode::Open,
        );
        assert!(matches!(result, Err(ApplyError::Invalid(_))));
        assert!(w.doc.voxels.is_none());
    }

    #[test]
    fn the_chunks_of_a_replay_count_against_the_budget() {
        let mut w = World::new();
        let replay = VoxelReplay {
            res: 8,
            cells: vec![
                (cell(0, 0, 0), V0),
                (cell(1, 1, 1), V1),
                (cell(16, 0, 0), V2),
            ],
        };
        let loaded = |replay: VoxelReplay| {
            let mut sections = empty(Categories::VOXELS);
            sections.voxels = Some(None);
            ApplySource::Loaded(Box::new(LoadedDoc {
                sections,
                voxel_replay: Some(replay),
                overrides: None,
                retained: None,
            }))
        };
        let two_chunks = voxel_world_bytes(8, 2).unwrap();
        assert!(
            apply_document_within(
                &mut w.cx(),
                loaded(replay.clone()),
                ApplyMode::Open,
                two_chunks - 1
            )
            .is_err(),
            "the cells fall into two chunks"
        );
        apply_document_within(&mut w.cx(), loaded(replay), ApplyMode::Open, two_chunks).unwrap();
        assert!(w.doc.voxels.is_some());
    }

    // --- Display (native 6.3 `md`) -------------------------------------------------------------

    #[test]
    fn display_slots_are_copied_back() {
        let mut w = World::new();
        let mut display = crate::contract::DisplaySlots::default();
        display[DisplaySlotId::Head].enabled = true;
        display[DisplaySlotId::Head].rotation = v(10.0, 20.0, 30.0);
        display[DisplaySlotId::Head].preset = "Custom".to_owned();
        let mut snap = empty(Categories::MODEL_DISPLAY);
        snap.display = Some(Arc::new(display.clone()));

        restore(&mut w, &snap, Categories::MODEL_DISPLAY).unwrap();

        assert_eq!(*w.doc.display, display);
        assert!(w.doc.journal.doc().contains(DocFlags::DISPLAY));
    }

    // --- Open and merge (native 6.1) -----------------------------------------------------------

    fn file_with_two_elements() -> (Snapshot, Vec<ObjId>) {
        let mut src = World::new();
        src.add_element(v(1.0, 0.0, 0.0), Vec3::ONE);
        let b = src.add_element(v(2.0, 0.0, 0.0), Vec3::ONE);
        src.select(&[ObjRef::Element(b)]);
        let mut snap = snapshot_of(&src, Categories::ELEMENTS | Categories::MESHES);
        // A loader hands fresh ids.
        let elements = snap.elements.as_mut().unwrap();
        elements.list = ListSnap::fresh(elements.list.items.iter().map(|el| (**el).clone()));
        let ids = elements.list.ids.clone();
        (snap, ids)
    }

    #[test]
    fn open_resets_the_scene_and_applies_what_the_file_has() {
        let mut w = World::new();
        w.add_element(Vec3::ZERO, Vec3::ONE);
        w.add_mesh(Vec3::ZERO, Vec3::ONE);
        w.doc.textures.entries = vec![texture("old", 1)];
        w.doc.voxels = Some(world_with(2, &[(cell(1, 1, 1), V0)]));
        Arc::make_mut(&mut w.doc.props).general.name = "old".to_owned();
        Arc::make_mut(&mut w.doc.display)[DisplaySlotId::Gui].enabled = true;
        w.doc.overrides = Arc::from([OverrideCase::default()]);
        w.sel.add(ObjRef::Blueprint);
        w.sel.highlighted.insert(ObjRef::Blueprint);
        let (sections, ids) = file_with_two_elements();

        apply_loaded(&mut w, sections, ApplyMode::Open).unwrap();

        assert_eq!(w.doc.elements.ids(), ids.as_slice());
        assert_eq!(w.doc.elements.display_ids(), [1, 2]);
        assert!(w.doc.meshes.is_empty());
        assert!(
            w.doc.textures.entries.is_empty(),
            "emptied, the file has none"
        );
        assert!(w.doc.voxels.is_none());
        assert_eq!(w.doc.props.general.name, "Cubik Model");
        assert!(!w.doc.display[DisplaySlotId::Gui].enabled);
        assert!(w.doc.overrides.is_empty());
        assert_eq!(
            w.sel.list.iter().copied().collect::<Vec<_>>(),
            [ObjRef::Element(ids[1])],
            "the record with `is` is selected, the rest cleared"
        );
        assert_eq!(
            w.sel.highlighted.iter().copied().collect::<Vec<_>>(),
            [ObjRef::Element(ids[1])],
            "the old highlight is gone, the selected record is highlighted"
        );
    }

    #[test]
    fn open_keeps_the_view_properties_and_the_blueprint_a_file_does_not_have() {
        let mut w = World::new();
        w.doc.view = Arc::new(ViewProps {
            uv_step: 0.125,
            large_grid: true,
            mouse_step: 2.0,
            ..ViewProps::default()
        });
        w.doc.blueprint.base.p = v(4.0, 0.0, 0.0);
        w.doc.blueprint.base.e = v(7.0, 0.0, 0.0);
        let view = Arc::clone(&w.doc.view);
        let blueprint = w.doc.blueprint.clone();

        // A file with neither `vp` nor `bp`.
        apply_loaded(&mut w, empty(Categories::empty()), ApplyMode::Open).unwrap();
        assert!(Arc::ptr_eq(&w.doc.view, &view));
        assert_eq!(w.doc.blueprint, blueprint);

        // `vp` replaces the steps but not `gu` and `gl`; `bp` compares its rotation with the
        // live one, so the odd 7 degrees stay.
        let mut sections = empty(Categories::VIEW_PROPERTIES | Categories::ELEMENTS);
        sections.view = Some(Arc::new(ViewProps {
            mouse_step: 4.0,
            ..ViewProps::default()
        }));
        sections.elements = Some(ElementsSnap {
            list: ListSnap::fresh(Vec::<Element>::new()),
            selected: vec![],
            blueprint: blueprint.clone(),
        });
        apply_loaded(&mut w, sections, ApplyMode::Open).unwrap();
        assert_eq!(w.doc.view.mouse_step, 4.0);
        assert_eq!((w.doc.view.uv_step, w.doc.view.large_grid), (0.125, true));
        assert_eq!(w.doc.blueprint.base.e, v(7.0, 0.0, 0.0));
    }

    #[test]
    fn open_takes_the_overrides_and_the_retention_of_the_file() {
        let mut w = World::new();
        let overrides: Arc<[OverrideCase]> = Arc::from([OverrideCase {
            model: "m".to_owned(),
            ..Default::default()
        }]);
        let retained = Arc::new(Retained {
            bytes: 7,
            data: Box::new(5_u32),
        });
        let loaded = LoadedDoc {
            sections: empty(Categories::empty()),
            voxel_replay: None,
            overrides: Some(Arc::clone(&overrides)),
            retained: Some(Arc::clone(&retained)),
        };
        apply_document(
            &mut w.cx(),
            ApplySource::Loaded(Box::new(loaded)),
            ApplyMode::Open,
        )
        .unwrap();
        assert!(Arc::ptr_eq(&w.doc.overrides, &overrides));
        assert!(Arc::ptr_eq(w.doc.retained.as_ref().unwrap(), &retained));

        // A restore touches neither.
        let snap = empty(Categories::empty());
        restore(&mut w, &snap, Categories::ALL).unwrap();
        assert!(Arc::ptr_eq(&w.doc.overrides, &overrides));
        assert!(w.doc.retained.is_some());
    }

    #[test]
    fn merge_appends_objects_and_overwrites_the_other_sections() {
        let mut w = World::new();
        let old = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.select(&[ObjRef::Element(old)]);
        w.doc.textures.entries = vec![texture("keep", 1)];
        w.doc.voxels = Some(world_with(2, &[(cell(1, 1, 1), V0)]));
        w.doc.retained = Some(Arc::new(Retained {
            bytes: 1,
            data: Box::new(()),
        }));
        let (mut sections, _) = file_with_two_elements();
        sections.categories |=
            Categories::VOXELS | Categories::TEXTURES | Categories::MODEL_PROPERTIES;
        sections.voxels = Some(None);
        sections.textures = Some(textures(Vec::new()));
        let mut props = ModelProps::default();
        props.general.name = "merged".to_owned();
        sections.props = Some(Arc::new(props));

        let report = apply_loaded(&mut w, sections, ApplyMode::Merge).unwrap();

        assert_eq!(w.doc.elements.len(), 3);
        assert_eq!(w.doc.elements.ids()[0], old);
        assert_eq!(w.doc.elements.display_ids(), [1, 2, 3]);
        assert_eq!(report.created.len(), 2);
        assert!(!report.created.contains(&ObjRef::Element(old)));
        assert_eq!(
            w.doc.textures.entries.len(),
            1,
            "no texture record: the list stays"
        );
        assert!(w.doc.voxels.is_none(), "deleted when the file has none");
        assert_eq!(w.doc.props.general.name, "merged");
        assert!(w.doc.retained.is_some());
        assert_eq!(
            w.sel.list.iter().copied().collect::<Vec<_>>(),
            [report.created[1]],
            "the selection was cleared, then the `is` record was selected"
        );
    }

    #[test]
    fn merge_continues_the_display_ids_of_the_live_lists() {
        let mut w = World::new();
        let ids: Vec<_> = (0..3)
            .map(|_| w.add_element(Vec3::ZERO, Vec3::ONE))
            .collect();
        w.doc.elements.remove_set(&FxHashSet::from_iter([ids[0]]));
        let (sections, _) = file_with_two_elements();
        apply_loaded(&mut w, sections, ApplyMode::Merge).unwrap();
        assert_eq!(w.doc.elements.display_ids(), [2, 3, 4, 5]);
    }

    // --- Paste (tools 11.3, 11.4) --------------------------------------------------------------

    fn paste(w: &mut World, snap: &Snapshot, offset: Vec3) -> ApplyReport {
        apply_document(
            &mut w.cx(),
            ApplySource::Snapshot(snap),
            ApplyMode::Paste { offset },
        )
        .unwrap()
    }

    #[test]
    fn a_paste_creates_new_selected_objects_with_the_offset_and_keeps_the_old_ones() {
        let mut w = World::new();
        let a = w.add_element(v(1.0, 2.0, 3.0), Vec3::ONE);
        let m = w.add_mesh(v(0.0, 1.0, 0.0), Vec3::ONE);
        w.edit_element(a, |el| el.base.o = v(5.0, 5.0, 5.0));
        w.doc.blueprint.base.p = v(10.0, 0.0, 0.0);
        w.select(&[ObjRef::Element(a), ObjRef::Mesh(m)]);
        let snap = capture(
            &w.doc,
            &w.sel,
            CaptureScope {
                categories: Categories::OBJECTS,
                selection_only: true,
            },
        );
        w.doc.blueprint.base.p = v(0.0, 0.0, 4.0);
        w.sel.clear();
        w.sel.add(ObjRef::Blueprint);

        let report = paste(&mut w, &snap, v(1.0, 0.0, -1.0));

        assert_eq!(w.doc.elements.len(), 2);
        let pasted = w.doc.elements.ids()[1];
        assert_ne!(pasted, a, "a paste allocates fresh ids");
        let el = w.element(pasted);
        assert_eq!(el.base.p, v(2.0, 2.0, 2.0));
        assert_eq!(el.base.o, v(6.0, 5.0, 4.0));
        assert_eq!(w.element(a).base.p, v(1.0, 2.0, 3.0), "the original stays");
        let mesh = w.doc.meshes.get(w.doc.meshes.ids()[1]).unwrap();
        assert_eq!(mesh.base.p, v(1.0, 1.0, -1.0));
        assert_eq!(w.doc.elements.display_ids(), [1, 2]);
        assert_eq!(
            w.doc.blueprint.base.p,
            v(10.0, 0.0, 0.0),
            "the blueprint jumps back to its copy-time transform"
        );
        assert_eq!(
            w.sel.list.len(),
            2,
            "the selection is cleared, the pasted ones selected"
        );
        assert!(w.sel.contains(ObjRef::Element(pasted)) && !w.sel.contains(ObjRef::Blueprint));
        assert_eq!(report.created.len(), 2);
    }

    #[test]
    fn a_paste_with_an_offset_clears_the_uv_snap_and_without_one_keeps_it() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.edit_element(a, |el| el.flags.insert(ElementFlags::UV_SNAP));
        w.select(&[ObjRef::Element(a)]);
        let snap = snapshot_of(&w, Categories::OBJECTS);

        paste(&mut w, &snap, Vec3::ZERO);
        paste(&mut w, &snap, v(0.0, 1.0, 0.0));

        let flags: Vec<bool> = w
            .doc
            .elements
            .items()
            .iter()
            .map(|el| el.flags.contains(ElementFlags::UV_SNAP))
            .collect();
        assert_eq!(flags, [true, true, false]);
    }

    #[test]
    fn a_paste_requantises_rotations_and_deep_copies_the_record() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.edit_element(a, |el| {
            el.base.e = v(0.0, 30.0, 0.0);
            el.faces[0].uv = UvRect::new(0, Vec2::new(-0.5, 0.0), Vec2::new(0.25, 1.0));
        });
        let snap = snapshot_of(&w, Categories::OBJECTS);

        paste(&mut w, &snap, Vec3::ZERO);

        let stored = &snap.elements.as_ref().unwrap().list.items[0];
        let pasted = Arc::clone(&w.doc.elements.items()[1]);
        assert_eq!(pasted.base.e, v(0.0, 22.5, 0.0));
        assert_eq!(
            pasted.faces[0].uv,
            crate::normal_form::uv(stored.faces[0].uv)
        );
        let ids = w.doc.elements.ids().to_vec();
        w.edit_element(ids[1], |el| el.faces[0].tint = 4);
        assert_eq!(
            stored.faces[0].tint, -1,
            "a later edit of the paste does not leak into the clipboard"
        );
        assert_eq!(w.element(ids[0]).faces[0].tint, -1, "nor into its sibling");
    }

    #[test]
    fn a_paste_ignores_the_categories_that_are_not_objects() {
        let mut w = World::new();
        w.add_element(Vec3::ZERO, Vec3::ONE);
        let mut snap = snapshot_of(&w, Categories::ALL);
        Arc::make_mut(snap.props.as_mut().unwrap()).general.name = "other".to_owned();
        snap.voxels = Some(Some(world_with(2, &[]).snapshot()));

        paste(&mut w, &snap, Vec3::ZERO);

        assert_eq!(w.doc.props.general.name, "Cubik Model");
        assert!(w.doc.voxels.is_none());
        assert_eq!(w.doc.elements.len(), 2);
    }

    // --- Failures (native 6.4) -----------------------------------------------------------------

    #[test]
    fn a_failing_apply_leaves_the_document_and_the_selection_untouched() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.select(&[ObjRef::Element(a)]);
        let mut snap = snapshot_of(&w, Categories::ELEMENTS | Categories::TEXTURES);
        w.edit_element(a, |el| el.base.hidden = true);
        snap.textures = Some(textures(vec![texture("", 1)]));
        let revision = w.doc.revision;

        for mode in [
            ApplyMode::Restore(Categories::ALL),
            ApplyMode::Open,
            ApplyMode::Merge,
        ] {
            let err = apply_loaded(&mut w, snap.clone(), mode).unwrap_err();
            assert!(matches!(err, ApplyError::EmptyTextureLabel));
        }
        assert!(w.element(a).base.hidden);
        assert_eq!(w.doc.elements.len(), 1);
        assert!(w.sel.contains(ObjRef::Element(a)));
        assert_eq!(w.doc.revision, revision);
    }

    #[test]
    fn a_declared_category_without_its_section_is_invalid() {
        let mut w = World::new();
        let snap = empty(Categories::ELEMENTS);
        assert!(matches!(
            restore(&mut w, &snap, Categories::ALL),
            Err(ApplyError::Invalid(_))
        ));
        let mut snap = snapshot_of(&w, Categories::OBJECTS);
        snap.meshes = Some(MeshesSnap {
            list: ListSnap::fresh([]),
            selected: vec![true],
        });
        assert!(matches!(
            restore(&mut w, &snap, Categories::ALL),
            Err(ApplyError::Invalid(_))
        ));
        let mut snap = snapshot_of(&w, Categories::OBJECTS);
        let id = ObjId::next();
        let element = Arc::new(Element::new(
            crate::objects::ElementType::Cube,
            &Blueprint::default(),
            1,
            [1.0; 4],
            &Label::from("t"),
        ));
        let elements = snap.elements.as_mut().unwrap();
        elements.list.ids = vec![id, id];
        elements.list.items = vec![Arc::clone(&element), element];
        elements.selected = vec![false; 2];
        assert!(matches!(
            restore(&mut w, &snap, Categories::ALL),
            Err(ApplyError::Invalid(_))
        ));
    }

    #[test]
    fn highlighted_objects_that_are_gone_are_dropped() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let snap = snapshot_of(&w, Categories::ELEMENTS);
        let b = w.add_element(Vec3::ZERO, Vec3::ONE);
        w.sel.highlighted.insert(ObjRef::Element(a));
        w.sel.highlighted.insert(ObjRef::Element(b));
        w.sel.highlighted.insert(ObjRef::Blueprint);

        restore(&mut w, &snap, Categories::ELEMENTS).unwrap();

        assert_eq!(
            w.sel.highlighted.iter().copied().collect::<Vec<_>>(),
            [ObjRef::Element(a), ObjRef::Blueprint]
        );
        assert!(w.doc.journal.doc().contains(DocFlags::HIGHLIGHT));
    }

    #[test]
    fn records_with_odd_values_restore_into_a_document_that_passes_the_invariants() {
        let mut w = World::new();
        let a = w.add_element(Vec3::ZERO, Vec3::ONE);
        let m = w.add_mesh(Vec3::ZERO, Vec3::ONE);
        w.edit_element(a, |el| {
            el.base.e = v(33.0, 0.0, 0.0);
            for face in &mut el.faces {
                face.uv = UvRect::new(450, Vec2::new(-0.75, 1.5), Vec2::new(2.0, -1.0));
            }
        });
        w.doc.meshes.edit(m, &mut w.doc.journal).unwrap().base.e = v(0.0, 0.0, 3.0);
        w.doc.blueprint.base.e = v(0.0, 8.0, 0.0);
        let snap = snapshot_of(&w, Categories::ALL);
        w.doc.blueprint.base.e = Vec3::ZERO;

        restore(&mut w, &snap, Categories::ALL).unwrap();

        assert!(check_invariants(&w.doc, &w.sel).is_ok());
    }
}
