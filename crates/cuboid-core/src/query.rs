//! `AppQuery`: what the outliner, the properties panel, the menus and the status bar read
//! (spec: menus 1.6-1.7, 4).

use cuboid_actions::ActionId;
use cuboid_model::{
    Document, ElementFlags, ElementType, FaceName, MeshKind, ObjRef, ObjectBase, Perms, Selection,
};
use glam::Vec3;

use crate::clipboard::ClipboardKind;
use crate::commands::{Command, CommandKind, Guard};
use crate::core::Core;
use crate::interact::SelectMode;

/// What kind of object a row or the primary is.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ObjectKind {
    /// A cuboid or plane element.
    Element(ElementType),
    /// A primitive or imported mesh.
    Mesh(MeshKind),
    /// The placement box.
    Blueprint,
    /// The model-origin marker.
    ModelOrigin,
}

/// How many objects of each kind are selected.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct SelectionSummary {
    /// Selected elements.
    pub elements: usize,
    /// Selected meshes.
    pub meshes: usize,
    /// The blueprint is selected.
    pub blueprint: bool,
    /// The model origin is selected.
    pub model_origin: bool,
    /// Selected objects that are hidden.
    pub hidden: usize,
    /// Selected objects that are locked.
    pub locked: usize,
}

impl SelectionSummary {
    /// Selected objects of every kind.
    pub fn total(&self) -> usize {
        self.elements + self.meshes + usize::from(self.blueprint) + usize::from(self.model_origin)
    }
}

/// One face of the primary element.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceProps {
    /// Which face.
    pub name: FaceName,
    /// The face is drawn and exported.
    pub active: bool,
    /// The label of its texture.
    pub texture: String,
}

/// The fields the properties panel shows for the primary object, in model space.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectProps {
    /// The object.
    pub obj: ObjRef,
    /// Its kind.
    pub kind: ObjectKind,
    /// Its label.
    pub label: String,
    /// The box corner (the marker position for the model origin).
    pub position: Vec3,
    /// The box size.
    pub size: Vec3,
    /// The rotation in degrees.
    pub rotation: Vec3,
    /// The rotation pivot.
    pub pivot: Vec3,
    /// The object is not drawn or picked.
    pub hidden: bool,
    /// Drags are refused.
    pub locked: bool,
    /// What the gizmos may do with it.
    pub perms: Perms,
    /// The colour of an element, RGBA in 0..=1.
    pub color: Option<[f32; 4]>,
    /// The faces of an element.
    pub faces: Option<[FaceProps; 6]>,
    /// The element follows its position with its UV mapping.
    pub uv_snap: Option<bool>,
}

/// One row of the outliner.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlinerRow {
    /// The object.
    pub obj: ObjRef,
    /// Its kind.
    pub kind: ObjectKind,
    /// Its label.
    pub label: String,
    /// The number the list shows.
    pub display_id: u32,
    /// Hidden objects are greyed out.
    pub hidden: bool,
    /// Locked objects show a lock.
    pub locked: bool,
    /// The object is selected.
    pub selected: bool,
}

/// What the UI needs to know to enable and label its rows and fill its panels: built once per
/// frame, then the UI touches the core only to execute commands.
///
/// spec: arch 6 (cuboid-core), menus 1.7
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppQuery {
    /// The label of the entry undo would restore; `None` disables Edit > Undo.
    pub undo_label: Option<String>,
    /// The label of the entry redo would restore; `None` disables Edit > Redo.
    pub redo_label: Option<String>,
    /// What Paste would paste.
    pub clipboard: ClipboardKind,
    /// Number of selected objects of every kind.
    pub selection_count: usize,
    /// The selected objects by kind.
    pub selection: SelectionSummary,
    /// The primary selected object.
    pub primary: Option<ObjRef>,
    /// The fields of the primary object.
    pub primary_props: Option<ObjectProps>,
    /// The Objects menu is enabled: the primary is an element or a mesh.
    pub objects_menu: bool,
    /// The voxel world is set up.
    pub voxels_set_up: bool,
    /// The document has unsaved changes.
    pub dirty: bool,
    /// Elements in the document.
    pub element_count: usize,
    /// Meshes in the document.
    pub mesh_count: usize,
    /// Entries of the texture list.
    pub texture_count: usize,
    /// Which gizmo the pointer drives; `None` while a tool is active.
    pub gizmo_mode: Option<SelectMode>,
    /// The status line text.
    pub status: String,
}

impl AppQuery {
    pub(crate) fn of(core: &Core) -> AppQuery {
        let doc = &core.doc;
        let selection = summarize(doc, &core.sel);
        let primary = core.sel.primary;
        AppQuery {
            undo_label: core.hist.undo_label().map(str::to_owned),
            redo_label: core.hist.redo_label().map(str::to_owned),
            clipboard: core.clip.kind(),
            selection_count: selection.total(),
            selection,
            primary,
            primary_props: primary.and_then(|obj| props_of(doc, obj)),
            objects_menu: matches!(primary, Some(ObjRef::Element(_) | ObjRef::Mesh(_))),
            voxels_set_up: doc.voxels.is_some(),
            dirty: core.dirty,
            element_count: doc.elements.len(),
            mesh_count: doc.meshes.len(),
            texture_count: doc.textures.entries.len(),
            gizmo_mode: Some(core.interact.mode).filter(|m| *m != SelectMode::Custom),
            status: core.status_text().to_owned(),
        }
    }

    /// Whether the command could run now: its guard holds on the selection.
    pub fn command_enabled(&self, cmd: &Command) -> bool {
        let listed = self.selection.elements + self.selection.meshes;
        match cmd {
            Command::Undo => self.undo_label.is_some(),
            Command::Redo => self.redo_label.is_some(),
            Command::Paste { .. } => self.clipboard != ClipboardKind::Empty,
            Command::SelectAll | Command::SelectInverse => self.element_count + self.mesh_count > 0,
            Command::SelectNone => self.selection_count > 0,
            // A command that names its objects does not depend on the selection.
            _ if !cmd.targets().is_empty() => true,
            Command::Copy => listed > 0,
            _ => match cmd.kind() {
                CommandKind::Edit(def) => match def.guard {
                    Guard::None => true,
                    Guard::Any => self.selection_count > 0,
                    Guard::Listed => listed > 0,
                    Guard::Elements => self.selection.elements > 0,
                },
                CommandKind::Instant => true,
                CommandKind::Deferred(_) => false,
            },
        }
    }

    /// Whether a menu row, key binding or palette entry is enabled; `None` for actions the core
    /// does not own (file, view and window actions).
    pub fn action_enabled(&self, action: ActionId) -> Option<bool> {
        Command::from_action(action).map(|cmd| self.command_enabled(&cmd))
    }
}

fn kind_of(doc: &Document, obj: ObjRef) -> Option<ObjectKind> {
    match obj {
        ObjRef::Element(id) => doc.elements.get(id).map(|e| ObjectKind::Element(e.kind)),
        ObjRef::Mesh(id) => doc.meshes.get(id).map(|m| ObjectKind::Mesh(m.kind)),
        ObjRef::Blueprint => Some(ObjectKind::Blueprint),
        ObjRef::ModelOrigin => Some(ObjectKind::ModelOrigin),
    }
}

fn summarize(doc: &Document, sel: &Selection) -> SelectionSummary {
    let mut out = SelectionSummary::default();
    for &obj in &sel.list {
        match obj {
            ObjRef::Element(_) => out.elements += 1,
            ObjRef::Mesh(_) => out.meshes += 1,
            ObjRef::Blueprint => out.blueprint = true,
            ObjRef::ModelOrigin => out.model_origin = true,
        }
        if let Some(base) = cuboid_ops::transform::base_of(doc, obj) {
            out.hidden += usize::from(base.hidden);
            out.locked += usize::from(base.locked);
        }
    }
    out
}

fn props_of(doc: &Document, obj: ObjRef) -> Option<ObjectProps> {
    let kind = kind_of(doc, obj)?;
    let from_base = |base: &ObjectBase| ObjectProps {
        obj,
        kind,
        label: base.label.to_string(),
        position: base.p,
        size: base.s,
        rotation: base.e,
        pivot: base.o,
        hidden: base.hidden,
        locked: base.locked,
        perms: base.perms,
        color: None,
        faces: None,
        uv_snap: None,
    };
    match obj {
        ObjRef::Element(id) => {
            let el = doc.elements.get(id)?;
            Some(ObjectProps {
                color: Some(el.color),
                faces: Some(std::array::from_fn(|i| FaceProps {
                    name: el.faces[i].name,
                    active: el.faces[i].active,
                    texture: el.faces[i].texture.to_string(),
                })),
                uv_snap: Some(el.flags.contains(ElementFlags::UV_SNAP)),
                ..from_base(&el.base)
            })
        }
        ObjRef::Mesh(id) => doc.meshes.get(id).map(|m| from_base(&m.base)),
        ObjRef::Blueprint => Some(from_base(&doc.blueprint.base)),
        ObjRef::ModelOrigin => {
            let origin = doc.props.general.origin;
            Some(ObjectProps {
                obj,
                kind,
                label: "Model Origin".to_owned(),
                position: origin,
                size: Vec3::ZERO,
                rotation: Vec3::ZERO,
                pivot: origin,
                hidden: false,
                locked: false,
                perms: Perms::MODEL_ORIGIN,
                color: None,
                faces: None,
                uv_snap: None,
            })
        }
    }
}

/// The rows of the outliner: elements, then meshes, in list order. A UI rebuilds them when
/// `doc.elements.rev()` or `doc.meshes.rev()` changes, or the selection does.
pub fn outliner(doc: &Document, sel: &Selection) -> Vec<OutlinerRow> {
    let elements = doc
        .elements
        .ids()
        .iter()
        .zip(doc.elements.items())
        .zip(doc.elements.display_ids())
        .map(|((&id, el), &display_id)| {
            (
                ObjRef::Element(id),
                ObjectKind::Element(el.kind),
                &el.base,
                display_id,
            )
        });
    let meshes = doc
        .meshes
        .ids()
        .iter()
        .zip(doc.meshes.items())
        .zip(doc.meshes.display_ids())
        .map(|((&id, mesh), &display_id)| {
            (
                ObjRef::Mesh(id),
                ObjectKind::Mesh(mesh.kind),
                &mesh.base,
                display_id,
            )
        });
    elements
        .chain(meshes)
        .map(|(obj, kind, base, display_id)| OutlinerRow {
            obj,
            kind,
            label: base.label.to_string(),
            display_id,
            hidden: base.hidden,
            locked: base.locked,
            selected: sel.contains(obj),
        })
        .collect()
}
