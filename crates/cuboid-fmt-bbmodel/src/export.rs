//! Model to `.bbmodel`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cuboid_model::mc;
use cuboid_model::voxel::ChunkSlot;
use cuboid_model::{
    DisplaySlot, DisplaySlots, Document, Element, ElementFlags, GroupFrame, GroupId, GuiLight,
    MARKER_COLORS, MAX_DEPTH, MarkerColor, Mesh, ObjRef, Texture, TextureEntry,
};
use glam::{Vec2, Vec3};
use serde_json::{Map, Value, json};

use crate::json::{array3, number};
use crate::kept::BbKept;
use crate::{BbError, Notice, animations, frames, textures};

/// The format version that Blockbench 4 and 5 both read.
const FORMAT_VERSION: &str = "4.10";

/// The version of the Java block rules that fits every element the exporter marks as Java.
const JAVA_BLOCK_VERSION: &str = "1.9.0";

/// UV space of the written project: every face rectangle is in 16 x 16 units, as in the
/// Minecraft JSON, so both editors read it the same way.
const UNITS: Vec2 = mc::JSON_UV_UNITS;

/// The Java block box: elements outside it are clamped by Minecraft.
const JAVA_LIMIT: (f32, f32) = (-16.0, 32.0);

/// The display slots in the order of the model, with the names of the project.
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

/// An exported project.
#[derive(Debug)]
pub struct BbExport {
    /// The text of the `.bbmodel` file.
    pub bytes: Vec<u8>,
    /// What the project does not hold.
    pub notices: Vec<Notice>,
}

/// Writes the model as a Blockbench project: a `java_block` project when every element obeys the
/// Minecraft rules (one rotation axis in steps of 22.5 degrees up to 45, inside the block
/// limits), no group turns and the model has no mesh, a `free` project otherwise. The groups
/// are written as the outliner tree with the numbers of their own frame, and the uuids and the
/// members kept from an imported project come back as they were. The textures are embedded as
/// PNG data URLs. The voxels are left out.
pub fn export(doc: &Document) -> Result<BbExport, BbError> {
    let mut notices = Vec::new();
    if doc.voxels.as_ref().is_some_and(|world| {
        world
            .slots()
            .iter()
            .any(|slot| matches!(slot, ChunkSlot::Data(_)))
    }) {
        notices.push(Notice::Skipped {
            what: "voxel worlds".to_owned(),
            count: 1,
        });
    }

    let kept = doc.retained.as_deref().and_then(BbKept::of);
    let animated = !doc.animations.is_empty()
        || kept.is_some_and(|k| {
            k.members
                .iter()
                .any(|(key, _)| key.starts_with("animation"))
        });
    // Only the free format gives a texture a UV size of its own, which frames that are not
    // square need.
    let java = doc.meshes.is_empty()
        && !animated
        && doc.groups.items().iter().all(|g| g.rotation == Vec3::ZERO)
        && doc.elements.items().iter().all(|el| fits_java(el))
        && doc
            .textures
            .entries
            .iter()
            .all(|entry| units_of(entry) == UNITS);
    // The free format puts the origin at the middle of the block floor.
    let shift = if java {
        Vec3::ZERO
    } else {
        Vec3::new(8.0, 0.0, 8.0)
    };

    let mut sheet = Sheet {
        entries: doc.textures.entries.clone(),
        indices: HashMap::new(),
        pictures: Vec::new(),
    };
    for (index, entry) in sheet.entries.iter().enumerate() {
        sheet
            .indices
            .entry(entry.label.to_string())
            .or_insert(index);
        sheet.pictures.push(Arc::clone(&entry.texture.image));
    }

    let frames = doc.groups.frames();
    let frame_of = |obj: ObjRef| {
        doc.groups
            .parent_of(obj)
            .and_then(|parent| frames.get(&parent))
    };
    let mut uuids = Uuids::new(doc, kept);
    let mut elements: Vec<Value> = Vec::new();
    let mut written: HashMap<ObjRef, String> = HashMap::new();
    for (&id, el) in doc.elements.ids().iter().zip(doc.elements.items()) {
        let obj = ObjRef::Element(id);
        let uuid = uuids.node(obj);
        elements.push(cube_json(el, &uuid, &sheet, shift, java, frame_of(obj)));
        written.insert(obj, uuid);
    }
    for (&id, mesh) in doc.meshes.ids().iter().zip(doc.meshes.items()) {
        let obj = ObjRef::Mesh(id);
        let uuid = uuids.node(obj);
        match mesh_json(mesh, &uuid, &mut sheet, shift, frame_of(obj)) {
            Some(value) => {
                elements.push(value);
                written.insert(obj, uuid);
            }
            None => notices.push(Notice::Skipped {
                what: "meshes without geometry".to_owned(),
                count: 1,
            }),
        }
    }
    let order: Vec<Value> = doc
        .root_nodes()
        .into_iter()
        .filter_map(|node| outline_json(doc, node, &written, &frames, &mut uuids, shift, 0))
        .collect();

    let mut textures_json = Vec::with_capacity(sheet.entries.len());
    for (index, entry) in sheet.entries.iter().enumerate() {
        let raw = kept.and_then(|k| k.frames.get(&*entry.label));
        let (value, picture) = texture_json(entry, &uuid(2, index), raw);
        if !picture {
            notices.push(Notice::TextureMissing {
                name: entry.label.to_string(),
            });
        }
        textures_json.push(value);
    }

    let props = &doc.props;
    let mut meta = Map::new();
    meta.insert("format_version".to_owned(), json!(FORMAT_VERSION));
    meta.insert(
        "model_format".to_owned(),
        json!(if java { "java_block" } else { "free" }),
    );
    meta.insert("box_uv".to_owned(), json!(false));

    let mut root = Map::new();
    root.insert("meta".to_owned(), Value::Object(meta));
    root.insert("name".to_owned(), json!(props.general.name));
    if !props.general.description.is_empty() {
        root.insert("credit".to_owned(), json!(props.general.description));
    }
    if java {
        if !props.minecraft.parent.is_empty() {
            root.insert("parent".to_owned(), json!(props.minecraft.parent));
        }
        if !props.minecraft.ambient_occlusion {
            root.insert("ambientocclusion".to_owned(), json!(false));
        }
        if props.minecraft.gui_light == GuiLight::Front {
            root.insert("front_gui_light".to_owned(), json!(true));
        }
        root.insert("java_block_version".to_owned(), json!(JAVA_BLOCK_VERSION));
    }
    root.insert(
        "resolution".to_owned(),
        json!({"width": UNITS.x, "height": UNITS.y}),
    );
    root.insert("elements".to_owned(), Value::Array(elements));
    root.insert("groups".to_owned(), Value::Array(Vec::new()));
    root.insert("outliner".to_owned(), Value::Array(order));
    root.insert("textures".to_owned(), Value::Array(textures_json));
    if let Some(display) = display_json(&doc.display) {
        root.insert("display".to_owned(), display);
    }
    // The animations of the model, then the ones it had no room for.
    let leftover: Vec<Value> = kept
        .and_then(|k| k.members.iter().find(|(key, _)| key == "animations"))
        .and_then(|(_, value)| value.as_array().cloned())
        .unwrap_or_default();
    let mut taken = uuids.used.clone();
    taken.extend(uuids.wanted.iter().cloned());
    let animations = animations::write(
        doc,
        &leftover,
        |group| uuids.node(ObjRef::Group(group)),
        &taken,
    );
    if !animations.is_empty() {
        root.insert("animations".to_owned(), Value::Array(animations));
    }
    // What the model cannot hold yet comes back as the project had it.
    for (key, value) in kept.iter().flat_map(|k| &k.members) {
        if key != "animations" {
            root.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }

    let bytes = serde_json::to_vec(&Value::Object(root))
        .map_err(|error| BbError::Write(error.to_string()))?;
    Ok(BbExport { bytes, notices })
}

/// The texture list being written: the entries of the model, and the pictures of meshes that the
/// list does not hold, appended.
struct Sheet {
    entries: Vec<TextureEntry>,
    /// The position of each label.
    indices: HashMap<String, usize>,
    /// The picture of each entry, to find a mesh texture again.
    pictures: Vec<Arc<cuboid_model::image::Image>>,
}

impl Sheet {
    /// The position of the entry for a mesh texture; a picture that is new gets an entry.
    fn index_of_picture(&mut self, texture: &Texture, label: &str) -> usize {
        if let Some(found) = self
            .pictures
            .iter()
            .position(|image| Arc::ptr_eq(image, &texture.image))
        {
            return found;
        }
        let mut unique: String = label
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            .collect();
        if unique.is_empty() {
            unique = "mesh_texture".to_owned();
        }
        let label = unique.clone();
        let mut n = 1;
        while self.indices.contains_key(&unique) {
            unique = format!("{label}_{n}");
            n += 1;
        }
        let index = self.entries.len();
        self.indices.insert(unique.clone(), index);
        self.pictures.push(Arc::clone(&texture.image));
        self.entries.push(TextureEntry {
            label: unique.into(),
            texture: texture.clone(),
            animation: None,
        });
        index
    }
}

fn uuid(kind: u32, index: usize) -> String {
    format!("{kind:08x}-0000-4000-8000-{index:012x}")
}

/// The uuids of the objects and groups being written: the ones the project gave are kept, the
/// others are new, and no uuid goes to two nodes.
struct Uuids<'a> {
    doc: &'a Document,
    kept: Option<&'a BbKept>,
    /// Every uuid a node would like to keep, so a new one never takes it.
    wanted: HashSet<String>,
    used: HashSet<String>,
    given: HashMap<ObjRef, String>,
    next: usize,
}

impl<'a> Uuids<'a> {
    fn new(doc: &'a Document, kept: Option<&'a BbKept>) -> Self {
        let mut wanted: HashSet<String> = kept
            .into_iter()
            .flat_map(|k| k.uuids.values().cloned())
            .collect();
        wanted.extend(doc.groups.items().iter().filter_map(|g| g.uuid.clone()));
        Self {
            doc,
            kept,
            wanted,
            used: HashSet::new(),
            given: HashMap::new(),
            next: 0,
        }
    }

    /// The uuid of a node; the same one however often it is asked for.
    fn node(&mut self, obj: ObjRef) -> String {
        if let Some(uuid) = self.given.get(&obj) {
            return uuid.clone();
        }
        let preferred = match obj {
            ObjRef::Element(id) | ObjRef::Mesh(id) => {
                self.kept.and_then(|k| k.uuids.get(&id)).cloned()
            }
            ObjRef::Group(id) => self.doc.groups.get(id).and_then(|g| g.uuid.clone()),
            ObjRef::Blueprint | ObjRef::ModelOrigin => None,
        };
        let uuid = match preferred.filter(|p| !self.used.contains(p)) {
            Some(uuid) => uuid,
            None => loop {
                let fresh = uuid(1, self.next);
                self.next += 1;
                if !self.wanted.contains(&fresh) && !self.used.contains(&fresh) {
                    break fresh;
                }
            },
        };
        self.used.insert(uuid.clone());
        self.given.insert(obj, uuid.clone());
        uuid
    }
}

/// The outliner entry of a node: the uuid of an object, or a group with its children. `None` for
/// an object that was not written.
fn outline_json(
    doc: &Document,
    node: ObjRef,
    written: &HashMap<ObjRef, String>,
    frames: &HashMap<GroupId, GroupFrame>,
    uuids: &mut Uuids<'_>,
    shift: Vec3,
    depth: usize,
) -> Option<Value> {
    let ObjRef::Group(id) = node else {
        return written.get(&node).cloned().map(Value::String);
    };
    if depth >= MAX_DEPTH {
        return None;
    }
    let group = doc.groups.get(id)?;
    let frame = frames.get(&id)?;
    let children: Vec<Value> = doc
        .child_nodes(Some(id))
        .into_iter()
        .filter_map(|child| outline_json(doc, child, written, frames, uuids, shift, depth + 1))
        .collect();
    let uuid = uuids.node(node);
    let mut out = Map::new();
    out.insert("name".to_owned(), json!(&*group.name));
    out.insert(
        "origin".to_owned(),
        array3(mc::unplace_point(frame.local_origin.as_vec3()) - shift),
    );
    out.insert(
        "rotation".to_owned(),
        array3(mc::unplace_group_rotation(group.rotation)),
    );
    out.insert("color".to_owned(), json!(0));
    out.insert("uuid".to_owned(), json!(uuid));
    out.insert("export".to_owned(), json!(group.export));
    out.insert("isOpen".to_owned(), json!(true));
    out.insert("locked".to_owned(), json!(group.locked));
    out.insert("visibility".to_owned(), json!(group.visible));
    out.insert("autouv".to_owned(), json!(0));
    out.insert("children".to_owned(), Value::Array(children));
    Some(Value::Object(out))
}

/// Whether the Java block format holds the element as it is.
fn fits_java(el: &Element) -> bool {
    if !mc::rotation_fits_json(el.base.e) {
        return false;
    }
    let b = mc::unplace(
        el.base.p,
        el.base.s,
        el.base.o,
        el.base.e,
        el.flags.contains(ElementFlags::INVERT_NORMALS),
    );
    let (low, high) = JAVA_LIMIT;
    [b.from, b.to]
        .iter()
        .flat_map(|v| v.to_array())
        .all(|c| (low..=high).contains(&c))
}

/// The index of the marker colour nearest to a colour of the model, in either of its shades.
fn marker_index(color: [f32; 4]) -> usize {
    let distance = |shade: &[u8; 3]| {
        color
            .iter()
            .zip(shade)
            .map(|(c, &s)| (c - f32::from(s) / 255.0).powi(2))
            .sum::<f32>()
    };
    let nearest = |m: &MarkerColor| distance(&m.pastel).min(distance(&m.standard));
    MARKER_COLORS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| nearest(a).total_cmp(&nearest(b)))
        .map_or(0, |(index, _)| index)
}

fn uv_json(rect: [f32; 4]) -> Value {
    Value::Array(rect.into_iter().map(number).collect())
}

/// A cube as a project element. An element that no project gave a marker colour gets the marker
/// nearest to its colour.
fn cube_json(
    el: &Element,
    uuid: &str,
    sheet: &Sheet,
    shift: Vec3,
    java: bool,
    frame: Option<&GroupFrame>,
) -> Value {
    let base = &el.base;
    // The numbers of the file are the ones in the frame of the group that holds the element.
    let (p, o, e) = frame.map_or((base.p, base.o, base.e), |f| {
        f.local_fields(base.p, base.o, base.e)
    });
    let b = mc::unplace(
        p,
        base.s,
        o,
        e,
        el.flags.contains(ElementFlags::INVERT_NORMALS),
    );
    let mut faces = Map::new();
    for face in &el.faces {
        let key = mc::face_key(face.name).to_owned();
        if !face.active {
            faces.insert(key, json!({"uv": [0, 0, 0, 0], "texture": null}));
            continue;
        }
        let index = sheet.indices.get(&*face.texture);
        let units = index
            .and_then(|&i| sheet.entries.get(i))
            .map_or(UNITS, units_of);
        let rect = mc::uv_to_units(face.uv.tl(), face.uv.br(), units);
        let mut value = Map::new();
        value.insert("uv".to_owned(), uv_json(rect));
        if let Some(index) = index {
            value.insert("texture".to_owned(), json!(index));
        }
        let rotation = face.uv.rot().rem_euclid(360);
        if rotation != 0 {
            value.insert("rotation".to_owned(), json!(rotation));
        }
        if face.tint >= 0 {
            value.insert("tint".to_owned(), json!(face.tint));
        }
        if let Some(side) = mc::cull_key(face.cull) {
            value.insert("cullface".to_owned(), json!(side));
        }
        faces.insert(key, Value::Object(value));
    }

    let mut out = Map::new();
    out.insert("name".to_owned(), json!(&*base.label));
    out.insert("box_uv".to_owned(), json!(false));
    if java && el.flags.contains(ElementFlags::RESCALE) {
        out.insert("rescale".to_owned(), json!(true));
    }
    if base.locked {
        out.insert("locked".to_owned(), json!(true));
    }
    if !el.flags.contains(ElementFlags::SHADE) {
        out.insert("shade".to_owned(), json!(false));
    }
    if base.hidden {
        out.insert("visibility".to_owned(), json!(false));
    }
    out.insert("from".to_owned(), array3(b.from - shift));
    out.insert("to".to_owned(), array3(b.to - shift));
    out.insert("autouv".to_owned(), json!(0));
    let marker = match el.marker {
        Some(marker) => json!(marker),
        None => json!(marker_index(el.color)),
    };
    out.insert("color".to_owned(), marker);
    out.insert("origin".to_owned(), array3(b.pivot - shift));
    if b.rotation != Vec3::ZERO {
        out.insert("rotation".to_owned(), array3(b.rotation));
    }
    out.insert("faces".to_owned(), Value::Object(faces));
    out.insert("type".to_owned(), json!("cube"));
    out.insert("uuid".to_owned(), json!(uuid));
    Value::Object(out)
}

/// A mesh as a project mesh: one vertex per vertex of the model, one face per triangle. `None`
/// for a mesh without triangles.
fn mesh_json(
    mesh: &Mesh,
    uuid: &str,
    sheet: &mut Sheet,
    shift: Vec3,
    frame: Option<&GroupFrame>,
) -> Option<Value> {
    let geometry = &mesh.geometry;
    let count = geometry.vertices.len();
    if count == 0 || geometry.triangles.len() < 3 {
        return None;
    }
    let base = &mesh.base;
    let (p, _, e) = frame.map_or((base.p, base.p, base.e), |f| {
        f.local_fields(base.p, base.p, base.e)
    });
    let at = mc::unplace(p, Vec3::ZERO, p, e, false);

    let key = |i: u32| format!("v{i}");
    let mut vertices = Map::new();
    for (i, v) in geometry.vertices.iter().enumerate() {
        let scaled = *v * base.s;
        let point = Vec3::new(scaled.x, scaled.y, -scaled.z);
        vertices.insert(format!("v{i}"), array3(point));
    }

    // The triangles with the texture each one is drawn with.
    let main = mesh.texture.as_ref();
    let mut runs: Vec<(&[u32], Option<&Texture>)> = Vec::new();
    if mesh.submeshes.is_empty() {
        runs.push((&geometry.triangles, main));
    } else {
        for sub in mesh.submeshes.iter() {
            runs.push((&sub.triangles, sub.texture.as_ref().or(main)));
        }
    }
    let mut faces = Map::new();
    let mut number_of_faces = 0_usize;
    for (indices, texture) in runs {
        let texture_index = texture.map(|t| sheet.index_of_picture(t, &base.label));
        for triangle in indices.chunks_exact(3) {
            let [a, b, c] = [triangle.first(), triangle.get(1), triangle.get(2)]
                .map(|i| i.copied().filter(|i| (*i as usize) < count));
            let (Some(a), Some(b), Some(c)) = (a, b, c) else {
                continue;
            };
            let mut uv = Map::new();
            for i in [a, b, c] {
                let point = geometry
                    .uvs
                    .get(i as usize)
                    .map_or([0.0, 0.0], |uv| [uv.x * UNITS.x, (1.0 - uv.y) * UNITS.y]);
                uv.insert(
                    key(i),
                    Value::Array(point.into_iter().map(number).collect()),
                );
            }
            let mut face = Map::new();
            // The winding of the model's space is the opposite of the project's.
            face.insert("vertices".to_owned(), json!([key(a), key(c), key(b)]));
            face.insert("uv".to_owned(), Value::Object(uv));
            if let Some(index) = texture_index {
                face.insert("texture".to_owned(), json!(index));
            }
            faces.insert(format!("f{number_of_faces}"), Value::Object(face));
            number_of_faces += 1;
        }
    }
    if faces.is_empty() {
        return None;
    }

    let mut out = Map::new();
    out.insert("name".to_owned(), json!(&*base.label));
    out.insert("color".to_owned(), json!(0));
    out.insert("origin".to_owned(), array3(at.from - shift));
    if at.rotation != Vec3::ZERO {
        out.insert("rotation".to_owned(), array3(at.rotation));
    }
    if base.hidden {
        out.insert("visibility".to_owned(), json!(false));
    }
    out.insert("vertices".to_owned(), Value::Object(vertices));
    out.insert("faces".to_owned(), Value::Object(faces));
    out.insert("type".to_owned(), json!("mesh"));
    out.insert("uuid".to_owned(), json!(uuid));
    Some(Value::Object(out))
}

/// The UV size of a texture as the project writes it: that of one frame, which is shorter than it
/// is wide when the frames of a strip are not square.
fn units_of(entry: &TextureEntry) -> Vec2 {
    match &entry.animation {
        Some(a) if a.frame_height != a.frame_width => Vec2::new(
            UNITS.x,
            UNITS.x * a.frame_height as f32 / a.frame_width as f32,
        ),
        _ => UNITS,
    }
}

/// A texture entry and whether its picture could be embedded. `raw` is what the project that the
/// entry comes from had for the fields of its animation.
fn texture_json(entry: &TextureEntry, uuid: &str, raw: Option<&Value>) -> (Value, bool) {
    let (folder, namespace, name) = textures::names(entry);
    let image = &entry.texture.image;
    let units = units_of(entry);
    let mut out = Map::new();
    out.insert("path".to_owned(), json!(""));
    out.insert("name".to_owned(), json!(name));
    out.insert("folder".to_owned(), json!(folder));
    out.insert("namespace".to_owned(), json!(namespace));
    out.insert("id".to_owned(), json!(&*entry.label));
    out.insert("particle".to_owned(), json!(&*entry.label == "particle"));
    out.insert("width".to_owned(), json!(image.w));
    out.insert("height".to_owned(), json!(image.h));
    out.insert("uv_width".to_owned(), number(units.x));
    out.insert("uv_height".to_owned(), number(units.y));
    if let Some(animation) = &entry.animation {
        frames::write(&mut out, animation, animation.tiles(image.h), raw);
    }
    out.insert("visible".to_owned(), json!(true));
    out.insert("internal".to_owned(), json!(true));
    out.insert("saved".to_owned(), json!(false));
    out.insert("uuid".to_owned(), json!(uuid));
    let source = textures::data_url_of(entry);
    let embedded = source.is_some();
    if let Some(source) = source {
        out.insert("source".to_owned(), json!(source));
    }
    (Value::Object(out), embedded)
}

/// The enabled slots of the model; a slot that holds only the default values is not written, as
/// the editor does. `None` when no slot is written.
fn display_json(display: &DisplaySlots) -> Option<Value> {
    let mut out = Map::new();
    for (slot, key) in display.slots.iter().zip(SLOT_KEYS) {
        if let Some(value) = slot_json(slot) {
            out.insert(key.to_owned(), value);
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

fn slot_json(slot: &DisplaySlot) -> Option<Value> {
    if !slot.enabled {
        return None;
    }
    let mut out = Map::new();
    if slot.rotation != Vec3::ZERO {
        out.insert("rotation".to_owned(), array3(slot.rotation));
    }
    if slot.translation != Vec3::ZERO {
        out.insert("translation".to_owned(), array3(slot.translation));
    }
    if slot.scale != Vec3::ONE {
        out.insert("scale".to_owned(), array3(slot.scale));
    }
    (!out.is_empty()).then_some(Value::Object(out))
}
