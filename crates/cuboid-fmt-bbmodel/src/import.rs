//! `.bbmodel` to model sections.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use cuboid_io::texture_files::TextureFiles;
use cuboid_model::mc::{self, McBox, Rigid};
use cuboid_model::{
    Blueprint, Categories, DisplaySlot, DisplaySlots, Element, ElementFlags, ElementType,
    ElementsSnap, Face, FaceFlags, FaceName, Group, GroupError, GroupId, GroupsBuilder, GuiLight,
    Label, ListSnap, LoadedDoc, Mesh, MeshesSnap, ModelKind, ModelProps, ObjId, ObjRef, ObjectBase,
    Perms, Selection, Snapshot, TextureList,
};
use glam::{Vec2, Vec3};
use serde_json::Value;

use crate::json::{flag, int_at, list, num, num_at, text, vec2_at, vec3_at};
use crate::textures::{self, Table, UvSpace};
use crate::{BbError, Limits, Notice, animations, kept};

/// An imported project.
#[derive(Debug)]
pub struct BbImport {
    /// The sections, to apply with `ApplyMode::Open`.
    pub doc: LoadedDoc,
    /// What was left out or changed.
    pub notices: Vec<Notice>,
}

/// The display slots in the order of the model.
const SLOT_KEYS: [&str; 8] = [
    "thirdperson_righthand",
    "thirdperson_lefthand",
    "firstperson_righthand",
    "firstperson_lefthand",
    "gui",
    "head",
    "ground",
    "fixed",
];

/// The marker colours of Blockbench, by the index a project stores. A writer picks the nearest
/// one for an element that has no marker of its own.
pub(crate) const MARKER_COLORS: [[f32; 3]; 8] = [
    [1.0, 0.55, 0.55],
    [1.0, 0.78, 0.55],
    [1.0, 1.0, 0.55],
    [0.7, 1.0, 0.55],
    [0.55, 1.0, 0.8],
    [0.55, 0.8, 1.0],
    [0.7, 0.55, 1.0],
    [1.0, 0.55, 0.9],
];

/// What the model format of a project says about the space its coordinates live in.
#[derive(Clone, Copy)]
pub(crate) struct Format {
    /// The origin of the project is the centre of the block floor, not its corner.
    centered: bool,
    /// Groups move what they hold.
    bone_rig: bool,
    /// Every texture has a UV size of its own.
    per_texture_uv: bool,
    /// Box UV sizes are not rounded down.
    float_box_size: bool,
    /// A tall picture is a strip of frames.
    animated_textures: bool,
}

fn format_of(id: &str) -> Format {
    let (centered, bone_rig, per_texture_uv) = match id {
        "java_block" => (false, false, false),
        "image" => (false, false, true),
        "free" | "optifine_entity" | "optifine_part" => (true, true, true),
        // Bedrock, modded entity, skin and formats this crate does not know.
        _ => (true, true, false),
    };
    Format {
        centered,
        bone_rig,
        per_texture_uv,
        float_box_size: id == "modded_entity",
        animated_textures: matches!(
            id,
            "java_block" | "free" | "image" | "bedrock" | "bedrock_old"
        ),
    }
}

/// The version text `4.10` as a pair of numbers.
fn version_of(text: &str) -> (u32, u32) {
    let mut parts = text
        .split('.')
        .map(|p| p.trim().parse::<u32>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// Where the walk of the outliner is: the transform of the groups above, in the JSON's space, and
/// the group that holds what the walk reaches.
#[derive(Clone)]
pub(crate) struct Ctx {
    pub outer: Rigid,
    pub parent: Option<GroupId>,
}

/// The longest own name of an element, a group or an animation, in characters.
pub(crate) const NAME_CHARS: usize = 64;
/// The longest uuid kept, in bytes.
const UUID_BYTES: usize = 64;

/// `text` without control characters, cut to `max` characters with an ellipsis. The flag says
/// whether it was cut. Only `max` characters are read and kept, however long `text` is.
pub(crate) fn clipped(text: &str, max: usize) -> (String, bool) {
    let mut chars = text.chars().filter(|c| !c.is_control());
    let mut out: String = chars.by_ref().take(max).collect();
    let cut = chars.next().is_some();
    if cut {
        out.pop();
        out.push('\u{2026}');
    }
    (out, cut)
}

/// The state of one import.
pub(crate) struct Importer<'a> {
    pub root: &'a Value,
    pub limits: &'a Limits,
    pub fmt: Format,
    pub version: (u32, u32),
    pub box_uv: bool,
    pub space: UvSpace,
    pub table: Table,
    pub default_label: Label,
    pub shift: Vec3,
    pub elements: Vec<Element>,
    pub element_ids: Vec<ObjId>,
    pub meshes: Vec<Mesh>,
    pub mesh_ids: Vec<ObjId>,
    /// The groups and the order of what they hold.
    pub groups: GroupsBuilder,
    /// The uuid the project gave an element or a mesh.
    pub uuids: HashMap<ObjId, String>,
    /// The marker colour the project gave an element.
    pub markers: HashMap<ObjId, i32>,
    pub mesh_items: u64,
    /// Bytes of labels and group names kept so far.
    pub label_bytes: u64,
    /// Elements and groups whose name lost text.
    pub shortened: usize,
    pub skipped: BTreeMap<String, usize>,
}

/// Imports a project with the default [`Limits`]; see [`import_with`].
pub fn import(bytes: &[u8], path: &Path, files: &dyn TextureFiles) -> Result<BbImport, BbError> {
    import_with(bytes, path, files, &Limits::default())
}

/// Imports a project. `path` is the location of the file: its name gives the model name when the
/// project has none, and its folder is where relative texture paths start. The size of the text,
/// the number of values, elements, textures, texels and mesh items are checked against `limits`
/// before they are built.
pub fn import_with(
    bytes: &[u8],
    path: &Path,
    files: &dyn TextureFiles,
    limits: &Limits,
) -> Result<BbImport, BbError> {
    let root = parse(bytes, limits)?;
    build(&root, path, files, limits)
}

fn parse(bytes: &[u8], limits: &Limits) -> Result<Value, BbError> {
    if bytes.len() as u64 > limits.max_text {
        return Err(BbError::Limit("the file size"));
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let lead = bytes.iter().take_while(|b| b.is_ascii_whitespace()).count();
    if bytes
        .get(lead..)
        .is_some_and(|rest| rest.starts_with(b"<lz>"))
    {
        return Err(BbError::Compressed);
    }
    // Every value of the tree follows a separator, so this bounds the memory of the tree.
    let separators = bytes.iter().filter(|b| **b == b',').count() as u64;
    if separators > limits.max_values {
        return Err(BbError::Limit("values"));
    }
    let root: Value =
        serde_json::from_slice(bytes).map_err(|error| BbError::Syntax(error.to_string()))?;
    if !root.get("meta").is_some_and(Value::is_object) {
        return Err(BbError::NotAProject);
    }
    Ok(root)
}

fn build(
    root: &Value,
    path: &Path,
    files: &dyn TextureFiles,
    limits: &Limits,
) -> Result<BbImport, BbError> {
    let mut notices = Vec::new();
    let meta = root.get("meta").unwrap_or(root);
    let version_text = text(meta, "format_version")
        .or_else(|| text(meta, "format"))
        .unwrap_or("4.0");
    let version = version_of(version_text);
    if version > (5, 0) {
        notices.push(Notice::NewerVersion(version_text.to_owned()));
    }
    let model_format =
        text(meta, "model_format").unwrap_or(if flag(meta, "bone_rig") == Some(true) {
            "bedrock_old"
        } else {
            "java_block"
        });
    let fmt = format_of(model_format);

    let resolution = root.get("resolution").map_or(Vec2::splat(16.0), |r| {
        let side = |key| num_at(r, key).filter(|v| *v > 0.0).unwrap_or(16.0);
        Vec2::new(side("width"), side("height"))
    });
    let space = UvSpace {
        project: resolution,
        per_texture: fmt.per_texture_uv,
        strips: fmt.animated_textures,
    };

    let texture_items = list(root, "textures");
    if texture_items.len() > limits.max_textures as usize {
        return Err(BbError::Limit("texture entries"));
    }
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let table = textures::read(texture_items, dir, space, files, limits, &mut notices);
    let default_label = TextureList {
        entries: table.entries.clone(),
    }
    .default_face_label();

    let items = list(root, "elements");
    if items.len() > limits.max_objects as usize {
        return Err(BbError::Limit("elements"));
    }

    let mut importer = Importer {
        root,
        limits,
        fmt,
        version,
        box_uv: flag(meta, "box_uv").unwrap_or(false),
        space,
        table,
        default_label,
        shift: if fmt.centered {
            Vec3::new(8.0, 0.0, 8.0)
        } else {
            Vec3::ZERO
        },
        elements: Vec::new(),
        element_ids: Vec::new(),
        meshes: Vec::new(),
        mesh_ids: Vec::new(),
        groups: GroupsBuilder::new(),
        uuids: HashMap::new(),
        markers: HashMap::new(),
        mesh_items: 0,
        label_bytes: 0,
        shortened: 0,
        skipped: BTreeMap::new(),
    };
    importer.walk_all(items)?;

    let Importer {
        table,
        elements,
        element_ids,
        meshes,
        mesh_ids,
        groups,
        uuids,
        markers,
        skipped,
        shortened,
        ..
    } = importer;

    for (what, count) in skipped {
        notices.push(Notice::Skipped { what, count });
    }
    if shortened > 0 {
        notices.push(Notice::LabelsShortened { count: shortened });
    }
    let tree = groups.finish();
    let read = animations::read(root, &tree, kept::KEEP_LIMIT);
    notices.extend(read.notices);
    let (kept, left_out) = kept::sections_of(root, read.bytes, read.leftover);
    for (what, count) in left_out {
        notices.push(Notice::Skipped { what, count });
    }
    for (name, count) in kept::summary(&kept) {
        notices.push(Notice::Kept { what: name, count });
    }

    let mut props = ModelProps::default();
    props.general.name = text(root, "name")
        .map(str::to_owned)
        .or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    if let Some(credit) = text(root, "credit") {
        credit.clone_into(&mut props.general.description);
    }
    if let Some(parent) = text(root, "parent") {
        parent.clone_into(&mut props.minecraft.parent);
        props.minecraft.kind = if parent.trim_start_matches("minecraft:") == "item/handheld" {
            ModelKind::Item
        } else {
            ModelKind::Block
        };
    }
    if let Some(occlusion) = flag(root, "ambientocclusion") {
        props.minecraft.ambient_occlusion = occlusion;
    }
    if flag(root, "front_gui_light") == Some(true) {
        props.minecraft.gui_light = GuiLight::Front;
    }

    let (display, left_out) = display(root);
    props.minecraft.include_display = display.slots.iter().any(|slot| slot.enabled);
    if left_out > 0 {
        notices.push(Notice::Skipped {
            what: "display slots".to_owned(),
            count: left_out,
        });
    }

    // The rotations stay as the project has them (`keep_exact_rotations`). The model restricts
    // them only when every one of them is within the Minecraft rule (one axis, 22.5 degree
    // steps) and the meshes sit on the 5 degree grid, so the restriction holds for what was read.
    let on_five = |e: Vec3| e.to_array().iter().all(|c| (c / 5.0).fract() == 0.0);
    props.general.restrict_rotations = elements.iter().all(|e| mc::rotation_fits_json(e.base.e))
        && meshes.iter().all(|m| on_five(m.base.e));

    let has_textures = !table.entries.is_empty();
    let has_meshes = !meshes.is_empty();
    let element_list = ListSnap {
        ids: element_ids,
        items: elements.into_iter().map(Arc::new).collect(),
    };
    let element_selected = vec![false; element_list.ids.len()];
    let mesh_list = ListSnap {
        ids: mesh_ids,
        items: meshes.into_iter().map(Arc::new).collect(),
    };
    let mesh_selected = vec![false; mesh_list.ids.len()];
    // A project without groups keeps the model flat: the top level is the list order.
    let has_groups = !tree.is_empty();
    let has_animations = !read.animations.is_empty();

    let mut categories =
        Categories::MODEL_PROPERTIES | Categories::ELEMENTS | Categories::MODEL_DISPLAY;
    categories.set(Categories::TEXTURES, has_textures);
    categories.set(Categories::MESHES, has_meshes);
    categories.set(Categories::HIERARCHY, has_groups);
    categories.set(Categories::ANIMATION, has_animations);
    let sections = Snapshot {
        categories,
        props: Some(Arc::new(props)),
        view: None,
        elements: Some(ElementsSnap {
            blueprint: Blueprint::default(),
            list: element_list,
            selected: element_selected,
        }),
        textures: has_textures.then(|| {
            Arc::new(TextureList {
                entries: table.entries,
            })
        }),
        voxels: None,
        meshes: has_meshes.then_some(MeshesSnap {
            list: mesh_list,
            selected: mesh_selected,
        }),
        display: Some(Arc::new(display)),
        groups: has_groups.then(|| tree.snapshot(&Selection::default())),
        animations: has_animations.then(|| ListSnap::fresh(read.animations)),
    };
    Ok(BbImport {
        doc: LoadedDoc {
            sections,
            voxel_replay: None,
            overrides: None,
            retained: kept::retain(kept, uuids, markers, table.frames),
            keep_exact_rotations: true,
        },
        notices,
    })
}

/// Present slots are enabled with their values, absent ones are disabled. The second value counts
/// the slots of the project that the model has no place for.
fn display(root: &Value) -> (DisplaySlots, usize) {
    let mut out = DisplaySlots::default();
    let Some(Value::Object(slots)) = root.get("display") else {
        return (out, 0);
    };
    for (slot, key) in out.slots.iter_mut().zip(SLOT_KEYS) {
        let Some(spec) = slots.get(key) else {
            *slot = DisplaySlot::default();
            continue;
        };
        slot.enabled = true;
        slot.rotation = vec3_at(spec, "rotation", Vec3::ZERO);
        slot.translation = vec3_at(spec, "translation", Vec3::ZERO);
        slot.scale = vec3_at(spec, "scale", Vec3::ONE);
    }
    let left_out = slots
        .keys()
        .filter(|k| !SLOT_KEYS.contains(&k.as_str()))
        .count();
    (out, left_out)
}

impl Importer<'_> {
    /// Walks the outliner and builds what it reaches, groups and objects in the order of the
    /// project, then the elements that no group lists.
    fn walk_all(&mut self, items: &[Value]) -> Result<(), BbError> {
        let mut by_uuid: HashMap<&str, usize> = HashMap::new();
        for (index, item) in items.iter().enumerate() {
            if let Some(uuid) = text(item, "uuid") {
                by_uuid.entry(uuid).or_insert(index);
            }
        }
        let mut groups: HashMap<&str, &Value> = HashMap::new();
        let root = self.root;
        for group in list(root, "groups") {
            if let Some(uuid) = text(group, "uuid") {
                groups.entry(uuid).or_insert(group);
            }
        }
        let mut visited = vec![false; items.len()];
        let root_ctx = Ctx {
            outer: Rigid::IDENTITY,
            parent: None,
        };
        let outliner = list(root, "outliner");
        let mut walk = Walk {
            items,
            by_uuid,
            groups,
            visited: &mut visited,
        };
        self.walk(outliner, &root_ctx, 0, &mut walk)?;
        for (item, seen) in items.iter().zip(visited.iter()) {
            if !seen {
                self.item(item, &root_ctx)?;
            }
        }
        Ok(())
    }

    fn walk(
        &mut self,
        children: &[Value],
        ctx: &Ctx,
        depth: u32,
        walk: &mut Walk<'_, '_>,
    ) -> Result<(), BbError> {
        if depth > self.limits.max_depth {
            return Err(BbError::Limit("group depth"));
        }
        for child in children {
            match child {
                Value::String(uuid) => {
                    if let Some(&index) = walk.by_uuid.get(uuid.as_str()) {
                        self.reach(index, ctx, walk)?;
                    }
                }
                // The oldest projects list the elements by their position.
                Value::Number(n) => {
                    if let Some(index) = n.as_u64().and_then(|i| usize::try_from(i).ok()) {
                        self.reach(index, ctx, walk)?;
                    }
                }
                Value::Object(_) => {
                    let data = text(child, "uuid")
                        .and_then(|uuid| walk.groups.get(uuid).copied())
                        .unwrap_or(child);
                    let (group, inner) = self.group(data, child, ctx.outer)?;
                    let id =
                        self.groups
                            .add_group(group, ctx.parent)
                            .map_err(|error| match error {
                                GroupError::TooDeep => BbError::Limit("group depth"),
                                _ => BbError::Limit("groups"),
                            })?;
                    let inner_ctx = Ctx {
                        outer: inner,
                        parent: Some(id),
                    };
                    let nested = ["children", "content"]
                        .into_iter()
                        .map(|key| list(child, key))
                        .find(|c| !c.is_empty())
                        .unwrap_or_default();
                    self.walk(nested, &inner_ctx, depth + 1, walk)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Builds an element, once, when the outliner reaches it.
    fn reach(&mut self, index: usize, ctx: &Ctx, walk: &mut Walk<'_, '_>) -> Result<(), BbError> {
        let Some(seen) = walk.visited.get_mut(index) else {
            return Ok(());
        };
        if std::mem::replace(seen, true) {
            return Ok(());
        }
        match walk.items.get(index) {
            Some(item) => self.item(item, ctx),
            None => Ok(()),
        }
    }

    /// The group a project member describes, and the transform for what it holds. `data` has the
    /// fields; `node` is the outliner entry, which may hold them as well.
    fn group(
        &mut self,
        data: &Value,
        node: &Value,
        outer: Rigid,
    ) -> Result<(Group, Rigid), BbError> {
        let (name, cut) = clipped(text(data, "name").unwrap_or("group"), NAME_CHARS);
        self.charge_label(name.len(), cut)?;
        let fallback = if self.fmt.centered {
            Vec3::ZERO
        } else {
            Vec3::splat(8.0)
        };
        let origin = vec3_at(data, "origin", fallback) + self.shift;
        // A format without bones keeps the groups as folders: they move nothing.
        let rotation = if self.fmt.bone_rig {
            vec3_at(data, "rotation", Vec3::ZERO)
        } else {
            Vec3::ZERO
        };
        let placed = mc::place_group(origin, rotation, outer);
        let mut group = Group::new(name, placed.pivot);
        group.rotation = placed.rotation;
        group.visible = flag(data, "visibility") != Some(false);
        group.locked = flag(data, "locked") == Some(true);
        group.export = flag(data, "export") != Some(false);
        group.uuid = uuid_of(node).or_else(|| uuid_of(data));
        let inner = if self.fmt.bone_rig {
            outer.then(Rigid::about_stored(origin, placed.rotation))
        } else {
            outer
        };
        Ok((group, inner))
    }

    /// Builds one element of the project and puts it in its group.
    fn item(&mut self, item: &Value, ctx: &Ctx) -> Result<(), BbError> {
        let kind = text(item, "type").unwrap_or("cube");
        match kind {
            "cube" if item.is_object() => {
                let element = self.cube(item, ctx)?;
                let id = ObjId::next();
                self.element_ids.push(id);
                self.elements.push(element);
                if let Some(marker) = int_at(item, "color") {
                    self.markers.insert(id, marker);
                }
                self.attach(ObjRef::Element(id), uuid_of(item), ctx)?;
            }
            "mesh" if item.is_object() => self.mesh(item, ctx)?,
            other => *self.skipped.entry(other.replace('_', " ")).or_default() += 1,
        }
        Ok(())
    }

    /// Lists a new object in its group and remembers the uuid the project gave it.
    pub(crate) fn attach(
        &mut self,
        obj: ObjRef,
        uuid: Option<String>,
        ctx: &Ctx,
    ) -> Result<(), BbError> {
        if let (ObjRef::Element(id) | ObjRef::Mesh(id), Some(uuid)) = (obj, uuid) {
            self.uuids.insert(id, uuid);
        }
        self.groups
            .add_object(obj, ctx.parent)
            .map_err(|_| BbError::Limit("groups"))
    }

    /// Charges the bytes of a name to the label budget.
    fn charge_label(&mut self, bytes: usize, cut: bool) -> Result<(), BbError> {
        self.label_bytes = self.label_bytes.saturating_add(bytes as u64);
        if self.label_bytes > self.limits.max_label_bytes {
            return Err(BbError::Limit("element names"));
        }
        if cut {
            self.shortened += 1;
        }
        Ok(())
    }

    /// The label of an element: its own name, cut to a bounded length. The bytes are charged to
    /// the label budget before the label is built.
    pub(crate) fn label(&mut self, name: Option<&str>, default: &str) -> Result<String, BbError> {
        let (own, cut) = clipped(name.unwrap_or(default), NAME_CHARS);
        self.charge_label(own.len(), cut)?;
        Ok(own)
    }

    fn cube(&mut self, item: &Value, ctx: &Ctx) -> Result<Element, BbError> {
        let label = self.label(text(item, "name"), "cube")?;

        let from = vec3_at(item, "from", Vec3::ZERO) + self.shift;
        let to = vec3_at(item, "to", Vec3::ZERO) + self.shift;
        let origin = vec3_at(item, "origin", Vec3::ZERO) + self.shift;
        let rotation = vec3_at(item, "rotation", Vec3::ZERO);

        // The box that is drawn: inflated and stretched about its centre.
        let inflate = num_at(item, "inflate").unwrap_or(0.0);
        let stretch = vec3_at(item, "stretch", Vec3::ONE);
        let (drawn_from, drawn_to) = if inflate != 0.0 || stretch != Vec3::ONE {
            let center = (from + to) * 0.5;
            let half = (to - from) * 0.5 + Vec3::splat(inflate);
            (center - half * stretch, center + half * stretch)
        } else {
            (from, to)
        };
        let placed = mc::place(
            &McBox {
                from: drawn_from,
                to: drawn_to,
                pivot: origin,
                rotation,
            },
            ctx.outer,
        );

        let mut base = ObjectBase::new(label, placed.p, placed.s, Perms::ELEMENT);
        base.o = placed.o;
        base.e = placed.e;
        base.hidden = flag(item, "visibility") == Some(false);
        base.locked = flag(item, "locked") == Some(true);

        let mut flags = ElementFlags::empty();
        let shade = flag(item, "shade").unwrap_or(true);
        flags.set(ElementFlags::SHADE, shade);
        flags.set(ElementFlags::RESCALE, flag(item, "rescale") == Some(true));

        let box_uv = flag(item, "box_uv").unwrap_or(self.box_uv);
        // Projects before 4.5 mirrored the box UV of a cube that does not shade.
        let mirror = flag(item, "mirror_uv").unwrap_or(false)
            || (box_uv && !shade && self.box_uv && self.version < (4, 5));
        let rects = box_uv.then(|| {
            let mut size = to - from;
            if !self.fmt.float_box_size {
                size = size.map(|c| (c + 1e-7).floor());
            }
            box_uv_rects(size, vec2_at(item, "uv_offset", Vec2::ZERO), mirror)
        });

        let faces: [Face; 6] = std::array::from_fn(|slot| {
            let name = FaceName::ALL.get(slot).copied().unwrap_or(FaceName::Down);
            self.face(
                item,
                name,
                rects.as_ref().and_then(|r| r.get(slot)).copied(),
            )
        });

        // The marker colour only labels the cube in the editor; it never tints the texture.
        Ok(Element {
            base,
            kind: ElementType::Cube,
            color: [1.0; 4],
            faces,
            flags,
        })
    }

    /// One face of a cube. `box_rect` is its rectangle in a box UV layout.
    fn face(&self, item: &Value, name: FaceName, box_rect: Option<[f32; 4]>) -> Face {
        let spec = item
            .get("faces")
            .and_then(|faces| faces.get(mc::face_key(name)));
        let mut face = Face::new(name, true, self.default_label.clone());
        let Some(spec) = spec else {
            return face;
        };
        let texture = self.texture_of(spec.get("texture"));
        let units = match texture {
            TexRef::Index(index) if self.space.per_texture => self
                .table
                .units
                .get(index)
                .copied()
                .unwrap_or(self.space.project),
            _ => self.space.project,
        };
        match texture {
            TexRef::Off => face.active = false,
            TexRef::Index(index) => {
                if let Some(entry) = self.table.entries.get(index) {
                    face.texture = entry.label.clone();
                }
            }
            TexRef::Untextured => {}
        }
        let rect = box_rect.or_else(|| {
            let uv = spec.get("uv")?.as_array()?;
            let at = |i: usize| uv.get(i).and_then(num).unwrap_or(0.0);
            Some([at(0), at(1), at(2), at(3)])
        });
        if let Some(rect) = rect {
            let (tl, br) = mc::uv_from_units(rect, units);
            face.uv.set_unclamped(tl, br);
        }
        if box_rect.is_none() {
            face.uv.set_rot(int_at(spec, "rotation").unwrap_or(0));
        }
        face.tint = int_at(spec, "tint").unwrap_or(-1);
        face.cull = text(spec, "cullface").map_or(FaceFlags::empty(), mc::cull_from_key);
        face
    }

    /// The texture a face names: by position in the list, or by `uuid`.
    pub(crate) fn texture_of(&self, value: Option<&Value>) -> TexRef {
        match value {
            Some(Value::Null) => TexRef::Off,
            Some(Value::Number(n)) => n
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|i| *i < self.table.entries.len())
                .map_or(TexRef::Untextured, TexRef::Index),
            Some(Value::String(uuid)) => self
                .table
                .by_uuid
                .get(uuid)
                .copied()
                .map_or(TexRef::Untextured, TexRef::Index),
            _ => TexRef::Untextured,
        }
    }
}

/// What a face says about its texture.
#[derive(Clone, Copy)]
pub(crate) enum TexRef {
    /// No texture is assigned: the face is drawn with the default label.
    Untextured,
    /// The face is switched off.
    Off,
    /// An entry of the texture list.
    Index(usize),
}

/// The groups and elements the walk of the outliner knows.
struct Walk<'a, 'b> {
    items: &'a [Value],
    by_uuid: HashMap<&'a str, usize>,
    groups: HashMap<&'a str, &'a Value>,
    visited: &'b mut Vec<bool>,
}

/// The uuid of a project member, when it is a sane one.
pub(crate) fn uuid_of(value: &Value) -> Option<String> {
    text(value, "uuid")
        .filter(|u| !u.is_empty() && u.len() <= UUID_BYTES && !u.chars().any(char::is_control))
        .map(str::to_owned)
}

/// The rectangles of the six faces of a box UV layout in UV units, `[u0, v0, u1, v1]` in the
/// order down, up, north, south, west, east. `size` is the box size rounded as the format asks.
///
/// The layout is the strip of the Minecraft entity models: west, north, east and south side by
/// side under the up and down faces, which sit above the north face.
pub(crate) fn box_uv_rects(size: Vec3, offset: Vec2, mirror: bool) -> [[f32; 4]; 6] {
    let (w, h, d) = (size.x, size.y, size.z);
    // (u, v, du, dv) of each face before the offset.
    let mut places = [
        (d + w * 2.0, 0.0, -w, d), // down
        (d + w, d, -w, -d),        // up
        (d, d, w, h),              // north
        (d * 2.0 + w, d, w, h),    // south
        (d + w, d, d, h),          // west
        (0.0, d, d, h),            // east
    ];
    if mirror {
        for place in &mut places {
            place.0 += place.2;
            place.2 = -place.2;
        }
        places.swap(4, 5);
    }
    places.map(|(u, v, du, dv)| {
        [
            u + offset.x,
            v + offset.y,
            u + du + offset.x,
            v + dv + offset.y,
        ]
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]
    use super::*;

    #[test]
    fn the_box_uv_layout_of_a_16_cube() {
        let rects = box_uv_rects(Vec3::splat(16.0), Vec2::ZERO, false);
        // north: the front of the strip, east on its left, south at the far end.
        assert_eq!(rects[2], [16.0, 16.0, 32.0, 32.0]);
        assert_eq!(rects[5], [0.0, 16.0, 16.0, 32.0]);
        assert_eq!(rects[4], [32.0, 16.0, 48.0, 32.0]);
        assert_eq!(rects[3], [48.0, 16.0, 64.0, 32.0]);
        // up and down are above, and run backwards in u.
        assert_eq!(rects[1], [32.0, 16.0, 16.0, 0.0]);
        assert_eq!(rects[0], [48.0, 0.0, 32.0, 16.0]);
    }

    #[test]
    fn mirroring_swaps_the_sides_and_the_direction() {
        let plain = box_uv_rects(Vec3::new(4.0, 6.0, 2.0), Vec2::new(10.0, 20.0), false);
        let mirrored = box_uv_rects(Vec3::new(4.0, 6.0, 2.0), Vec2::new(10.0, 20.0), true);
        // North keeps its place and is read backwards.
        assert_eq!(plain[2], [12.0, 22.0, 16.0, 28.0]);
        assert_eq!(mirrored[2], [16.0, 22.0, 12.0, 28.0]);
        // East and west trade places.
        assert_eq!(plain[5][1..], [22.0, 12.0, 28.0]);
        assert_eq!(mirrored[4], [12.0, 22.0, 10.0, 28.0]);
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(version_of("4.10") > version_of("4.5"));
        assert!(version_of("5.0") > version_of("4.10"));
        assert_eq!(version_of("garbage"), (0, 0));
    }
}
