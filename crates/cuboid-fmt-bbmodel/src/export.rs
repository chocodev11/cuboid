//! Model to `.bbmodel`.

use std::collections::HashMap;
use std::sync::Arc;

use cuboid_model::mc;
use cuboid_model::voxel::ChunkSlot;
use cuboid_model::{
    DisplaySlot, DisplaySlots, Document, Element, ElementFlags, GuiLight, Mesh, Texture,
    TextureEntry,
};
use glam::{Vec2, Vec3};
use serde_json::{Map, Value, json};

use crate::import::MARKER_COLORS;
use crate::json::{array3, number};
use crate::{BbError, Notice, textures};

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
/// limits) and the model has no mesh, a `free` project otherwise. The textures are embedded as
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

    let java = doc.meshes.is_empty() && doc.elements.items().iter().all(|el| fits_java(el));
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

    let mut elements: Vec<Value> = Vec::new();
    let mut order: Vec<Value> = Vec::new();
    for (index, el) in doc.elements.items().iter().enumerate() {
        let uuid = uuid(1, index);
        elements.push(cube_json(el, &uuid, &sheet, shift, java));
        order.push(Value::String(uuid));
    }
    let base = elements.len();
    for (index, mesh) in doc.meshes.items().iter().enumerate() {
        let uuid = uuid(1, base + index);
        match mesh_json(mesh, &uuid, &mut sheet, shift) {
            Some(value) => {
                elements.push(value);
                order.push(Value::String(uuid));
            }
            None => notices.push(Notice::Skipped {
                what: "meshes without geometry".to_owned(),
                count: 1,
            }),
        }
    }

    let mut textures_json = Vec::with_capacity(sheet.entries.len());
    for (index, entry) in sheet.entries.iter().enumerate() {
        let (value, picture) = texture_json(entry, &uuid(2, index));
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
        });
        index
    }
}

fn uuid(kind: u32, index: usize) -> String {
    format!("{kind:08x}-0000-4000-8000-{index:012x}")
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

/// The index of the marker colour nearest to a colour of the model.
fn marker_index(color: [f32; 4]) -> usize {
    let distance = |m: &[f32; 3]| {
        (0..3)
            .map(|i| {
                let d = color.get(i).copied().unwrap_or(1.0) - m.get(i).copied().unwrap_or(1.0);
                d * d
            })
            .sum::<f32>()
    };
    MARKER_COLORS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)))
        .map_or(0, |(index, _)| index)
}

fn uv_json(rect: [f32; 4]) -> Value {
    Value::Array(rect.into_iter().map(number).collect())
}

fn cube_json(el: &Element, uuid: &str, sheet: &Sheet, shift: Vec3, java: bool) -> Value {
    let base = &el.base;
    let b = mc::unplace(
        base.p,
        base.s,
        base.o,
        base.e,
        el.flags.contains(ElementFlags::INVERT_NORMALS),
    );
    let mut faces = Map::new();
    for face in &el.faces {
        let key = mc::face_key(face.name).to_owned();
        if !face.active {
            faces.insert(key, json!({"uv": [0, 0, 0, 0], "texture": null}));
            continue;
        }
        let rect = mc::uv_to_units(face.uv.tl(), face.uv.br(), UNITS);
        let mut value = Map::new();
        value.insert("uv".to_owned(), uv_json(rect));
        if let Some(index) = sheet.indices.get(&*face.texture) {
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
    out.insert("color".to_owned(), json!(marker_index(el.color)));
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
fn mesh_json(mesh: &Mesh, uuid: &str, sheet: &mut Sheet, shift: Vec3) -> Option<Value> {
    let geometry = &mesh.geometry;
    let count = geometry.vertices.len();
    if count == 0 || geometry.triangles.len() < 3 {
        return None;
    }
    let base = &mesh.base;
    let at = mc::unplace(base.p, Vec3::ZERO, base.p, base.e, false);

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

/// A texture entry and whether its picture could be embedded.
fn texture_json(entry: &TextureEntry, uuid: &str) -> (Value, bool) {
    let (folder, namespace, name) = textures::names(entry);
    let image = &entry.texture.image;
    let mut out = Map::new();
    out.insert("path".to_owned(), json!(""));
    out.insert("name".to_owned(), json!(name));
    out.insert("folder".to_owned(), json!(folder));
    out.insert("namespace".to_owned(), json!(namespace));
    out.insert("id".to_owned(), json!(&*entry.label));
    out.insert("particle".to_owned(), json!(&*entry.label == "particle"));
    out.insert("width".to_owned(), json!(image.w));
    out.insert("height".to_owned(), json!(image.h));
    out.insert("uv_width".to_owned(), json!(UNITS.x));
    out.insert("uv_height".to_owned(), json!(UNITS.y));
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
