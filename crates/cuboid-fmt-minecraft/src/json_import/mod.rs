//! Minecraft Java block and item model import (spec: interop 8, 31.2).
//!
//! A model is read in the permissive dialect of the game: standard JSON first, then the
//! reference dialect (trailing commas). The chain of parent models is followed through the
//! folder tree and the built-in parents, merging `textures` and `display` per key with the child
//! first. An `item/generated` model becomes its sprite in elements. Boxes keep their `from` and
//! `to` as they are (an inverted box has a negative size), rotations are not snapped and face
//! rectangles are not clamped, so a model writes back as it was read.

mod builtin;
mod elements;
mod parents;
mod sprite;
mod textures;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use cuboid_io::text::jsonlite;
use cuboid_io::texture_files::TextureFiles;
use cuboid_model::mc;
use cuboid_model::{
    Blueprint, Categories, DisplaySlot, DisplaySlots, ElementsSnap, GuiLight, ListSnap, LoadedDoc,
    ModelKind, ModelProps, OverrideCase, Snapshot, TextureList,
};
use glam::{Vec2, Vec3};
use serde_json::{Map, Value};

use crate::jv::{int_at, list, num, num_at, text, vec3_at};
use crate::{Limits, McError, Notice};
pub(crate) use builtin::is_generated;
use parents::{Chain, resolve};
use textures::{Merged, Table};

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

/// Root keys that carry nothing the model can hold.
const IGNORED_KEYS: [&str; 2] = ["render_type", "groups"];

/// An imported model.
#[derive(Debug)]
pub struct JsonImport {
    /// The sections, to apply with `ApplyMode::Open`.
    pub doc: LoadedDoc,
    /// What was left out or changed.
    pub notices: Vec<Notice>,
}

/// Imports a model with the default [`Limits`]; see [`import_with`].
pub fn import(bytes: &[u8], path: &Path, files: &dyn TextureFiles) -> Result<JsonImport, McError> {
    import_with(bytes, path, files, &Limits::default())
}

/// Imports a model file. `path` is its location: the file name gives the model name, its folder
/// is where parents and textures are looked for. The size of the text, the number of values,
/// elements, textures, texels and overrides are checked against `limits` before they are built.
pub fn import_with(
    bytes: &[u8],
    path: &Path,
    files: &dyn TextureFiles,
    limits: &Limits,
) -> Result<JsonImport, McError> {
    let root = parse(bytes, limits)?;
    if !["elements", "textures", "parent"]
        .iter()
        .any(|key| root.get(key).is_some())
    {
        return Err(McError::NotAModel);
    }
    let mut notices = Vec::new();
    let full = path.to_string_lossy().replace('\\', "/");
    let folder = match full.rfind('/').and_then(|at| full.get(..=at)) {
        Some(folder) => folder.to_owned(),
        None => String::new(),
    };
    let chain = resolve(root, folder.clone(), &full, files, limits, &mut notices);
    build(chain, path, &folder, files, limits, notices)
}

/// Reads the text: standard JSON, and when that fails the reference dialect, which accepts a
/// trailing comma. The root must be an object.
pub(crate) fn parse(bytes: &[u8], limits: &Limits) -> Result<Value, McError> {
    if bytes.len() as u64 > limits.max_text {
        return Err(McError::Limit("the file size"));
    }
    let text = cuboid_io::text::decode_bom(bytes);
    // Every value of the tree follows a separator, so this bounds the memory of the tree.
    let separators = text.bytes().filter(|b| *b == b',').count() as u64;
    if separators > limits.max_values {
        return Err(McError::Limit("values"));
    }
    let root = match serde_json::from_str::<Value>(&text) {
        Ok(root) => root,
        Err(_) => from_lite(&jsonlite::parse(&text).map_err(|_| McError::Syntax)?),
    };
    if root.is_object() {
        Ok(root)
    } else {
        Err(McError::Syntax)
    }
}

fn from_lite(value: &cuboid_io::text::Value) -> Value {
    use cuboid_io::text::Value as Lite;
    match value {
        Lite::Null => Value::Null,
        Lite::Bool(b) => Value::Bool(*b),
        Lite::Number(n) => serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number),
        Lite::String(s) => Value::String(s.clone()),
        Lite::Array(items) => Value::Array(items.iter().map(from_lite).collect()),
        Lite::Object(members) => Value::Object(
            members
                .iter()
                .map(|(k, v)| (k.clone(), from_lite(v)))
                .collect::<Map<String, Value>>(),
        ),
    }
}

/// The first value of a root key in the chain, the model before its parents.
fn inherited<'a>(chain: &'a Chain, key: &str) -> Option<&'a Value> {
    chain.layers.iter().find_map(|layer| layer.json.get(key))
}

/// The merged `textures`: the parents' entries first, the child's replacing them by key.
fn merge_textures(chain: &Chain) -> Result<Vec<Merged>, McError> {
    let mut merged: Vec<Merged> = Vec::new();
    for layer in chain.layers.iter().rev() {
        let Some(Value::Object(entries)) = layer.json.get("textures") else {
            continue;
        };
        for (key, value) in entries {
            let value = match value {
                Value::String(s) => s.clone(),
                Value::Object(o) => match o.get("sprite").and_then(Value::as_str) {
                    Some(s) => s.to_owned(),
                    None => return Err(McError::TextureValue(key.clone())),
                },
                _ => return Err(McError::TextureValue(key.clone())),
            };
            let entry = Merged {
                key: key.clone(),
                value,
                folder: layer.folder.clone(),
                builtin: layer.builtin,
            };
            match merged.iter_mut().find(|m| m.key == *key) {
                Some(old) => *old = entry,
                None => merged.push(entry),
            }
        }
    }
    Ok(merged)
}

fn build(
    chain: Chain,
    path: &Path,
    folder: &str,
    files: &dyn TextureFiles,
    limits: &Limits,
    mut notices: Vec<Notice>,
) -> Result<JsonImport, McError> {
    let root = chain.layers.first().map(|layer| &layer.json);
    let Some(root) = root else {
        return Err(McError::Syntax);
    };
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();

    let merged = merge_textures(&chain)?;
    let mut table = textures::read(&merged, files, limits, &mut notices)?;
    let default_label = TextureList {
        entries: table.entries.clone(),
    }
    .default_face_label();

    // The UV space of the file: 16 x 16 unless it says otherwise.
    let units = inherited(&chain, "texture_size")
        .and_then(Value::as_array)
        .and_then(|size| {
            let side = |i: usize| size.get(i).and_then(num).filter(|v| *v > 0.0);
            Some(Vec2::new(side(0)?, side(1)?))
        })
        .unwrap_or(mc::JSON_UV_UNITS);

    // The first model in the chain that has elements gives them.
    let element_items = chain
        .layers
        .iter()
        .find(|layer| layer.json.get("elements").is_some())
        .map_or(&[][..], |layer| list(&layer.json, "elements"));
    if element_items.len() > limits.max_elements as usize {
        return Err(McError::Limit("elements"));
    }
    let mut elements = elements::build(
        element_items,
        &mut table,
        units,
        &default_label,
        &mut skipped,
        &mut notices,
    )?;

    let mut parent = text(root, "parent").unwrap_or_default().to_owned();
    if chain.generated && elements.is_empty() {
        extrude_layers(&merged, &mut table, &mut elements, limits, &mut notices);
        if !elements.is_empty() {
            // The sprite replaces what the game would generate.
            parent.clear();
        }
    }
    prune_builtin(&mut table, &elements);

    for key in IGNORED_KEYS {
        if root.get(key).is_some() {
            skipped.insert(key.replace('_', " "), 1);
        }
    }
    for (what, count) in skipped {
        notices.push(Notice::Skipped { what, count });
    }

    let mut props = ModelProps::default();
    props.general.name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(comment) = text(root, "__comment").or_else(|| text(root, "credit")) {
        comment
            .replace('"', "\u{201D}")
            .clone_into(&mut props.general.description);
    }
    props.minecraft.kind = kind_of(&parent, folder);
    parent.clone_into(&mut props.minecraft.parent);
    props.minecraft.auto_vanilla_parent = !parent.is_empty();
    if let Some(occlusion) = inherited(&chain, "ambientocclusion") {
        props.minecraft.ambient_occlusion = occlusion.as_bool().unwrap_or(false);
    }

    if inherited(&chain, "gui_light").and_then(Value::as_str) == Some("front") {
        props.minecraft.gui_light = GuiLight::Front;
    }

    let display_json = merged_display(&chain);
    let (display, left_out) = display(&display_json);
    // Only the display settings of the model's own files are written back; a built-in parent
    // brings its own when the game loads the model.
    props.minecraft.include_display = chain.layers.iter().any(|layer| {
        !layer.builtin
            && layer
                .json
                .get("display")
                .and_then(Value::as_object)
                .is_some_and(|slots| !slots.is_empty())
    });
    if left_out > 0 {
        notices.push(Notice::Skipped {
            what: "display slots".to_owned(),
            count: left_out,
        });
    }

    let overrides = overrides(root, limits, &mut notices)?;
    props.minecraft.keep_overrides = root.get("overrides").is_some();

    // The rotations stay as the file has them (`keep_exact_rotations`). The model restricts them
    // only when every one of them is within the Minecraft rule.
    props.general.restrict_rotations = elements.iter().all(|e| mc::rotation_fits_json(e.base.e));

    let has_textures = !table.entries.is_empty();
    let element_list = ListSnap::fresh(elements);
    let element_selected = vec![false; element_list.ids.len()];
    let mut categories =
        Categories::MODEL_PROPERTIES | Categories::ELEMENTS | Categories::MODEL_DISPLAY;
    categories.set(Categories::TEXTURES, has_textures);
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
        meshes: None,
        display: Some(Arc::new(display)),
    };
    Ok(JsonImport {
        doc: LoadedDoc {
            sections,
            voxel_replay: None,
            overrides: overrides.map(Arc::from),
            retained: None,
            keep_exact_rotations: true,
        },
        notices,
    })
}

/// The block or item type (interop 8.3): an item parent, or else the folder of the file.
fn kind_of(parent: &str, folder: &str) -> ModelKind {
    let id = parent.strip_prefix("minecraft:").unwrap_or(parent);
    if id.starts_with("item/") {
        return ModelKind::Item;
    }
    if !id.is_empty() {
        return ModelKind::Block;
    }
    let (item, block) = (folder.contains("item"), folder.contains("block"));
    if item && !block {
        ModelKind::Item
    } else {
        ModelKind::Block
    }
}

/// The sprite of every `layerN` texture, as the game's `generated` model builds it.
fn extrude_layers(
    merged: &[Merged],
    table: &mut Table,
    elements: &mut Vec<cuboid_model::Element>,
    limits: &Limits,
    notices: &mut Vec<Notice>,
) {
    let mut layers: Vec<(i32, &Merged)> = merged
        .iter()
        .filter_map(|m| sprite::layer_index(&m.key).map(|n| (n, m)))
        .collect();
    layers.sort_by_key(|(n, _)| *n);
    for (tint, item) in layers {
        let Some(label) = table.labels.get(&item.key).cloned() else {
            continue;
        };
        let Some(entry) = table.entries.iter().find(|e| e.label == label) else {
            continue;
        };
        let room = (limits.max_elements as usize).saturating_sub(elements.len());
        let (mut parts, slab) = sprite::extrude(
            &entry.texture.image,
            &label,
            tint,
            limits.max_sprite_texels,
            elements.len(),
        );
        parts.truncate(room);
        if slab {
            notices.push(Notice::SpriteSlab);
        }
        elements.append(&mut parts);
    }
    if !elements.is_empty() {
        notices.push(Notice::Extruded {
            elements: elements.len(),
        });
    }
}

/// Drops the entries a built-in parent brought that no face uses, except `particle`. An alias
/// that is used keeps its target.
fn prune_builtin(table: &mut Table, elements: &[cuboid_model::Element]) {
    let mut used: std::collections::HashSet<String> = elements
        .iter()
        .flat_map(|e| e.faces.iter())
        .map(|f| f.texture.to_string())
        .collect();
    used.insert("particle".to_owned());
    loop {
        let before = used.len();
        for entry in &table.entries {
            if used.contains(&*entry.label)
                && let Some(target) = entry.texture.source.mc_path.strip_prefix('#')
            {
                used.insert(target.to_owned());
            }
        }
        if used.len() == before {
            break;
        }
    }
    let mut keep = table.builtin.iter().copied();
    table
        .entries
        .retain(|entry| !keep.next().unwrap_or(false) || used.contains(&*entry.label));
}

/// The `display` slots of the chain: the parents' first, a slot of the child replacing the
/// parent's whole.
fn merged_display(chain: &Chain) -> Map<String, Value> {
    let mut merged = Map::new();
    for layer in chain.layers.iter().rev() {
        if let Some(Value::Object(slots)) = layer.json.get("display") {
            for (key, value) in slots {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    merged
}

/// Present slots are enabled with their values, absent ones are disabled. The second value counts
/// the slots of the file that the model has no place for.
fn display(slots: &Map<String, Value>) -> (DisplaySlots, usize) {
    let mut out = DisplaySlots::default();
    for (slot, key) in out.slots.iter_mut().zip(SLOT_KEYS) {
        let Some(spec) = slots.get(key) else {
            *slot = DisplaySlot::default();
            continue;
        };
        slot.enabled = true;
        slot.rotation = vec3_at(spec, "rotation").unwrap_or(Vec3::ZERO);
        slot.translation = vec3_at(spec, "translation").unwrap_or(Vec3::ZERO);
        slot.scale = vec3_at(spec, "scale").unwrap_or(Vec3::ONE);
    }
    let left_out = slots
        .keys()
        .filter(|k| !SLOT_KEYS.contains(&k.as_str()))
        .count();
    (out, left_out)
}

/// The item override cases of the model itself (interop 8.3 step 9). `None` when the file has no
/// `overrides`. A predicate key other than the ten the model knows is counted as skipped.
fn overrides(
    root: &Value,
    limits: &Limits,
    notices: &mut Vec<Notice>,
) -> Result<Option<Vec<OverrideCase>>, McError> {
    let Some(items) = root.get("overrides").and_then(Value::as_array) else {
        return Ok(None);
    };
    if items.len() > limits.max_overrides as usize {
        return Err(McError::Limit("override cases"));
    }
    const KNOWN: [&str; 10] = [
        "angle",
        "blocking",
        "broken",
        "cast",
        "damage",
        "damaged",
        "lefthanded",
        "pull",
        "pulling",
        "time",
    ];
    let mut other = 0;
    let mut cases = Vec::new();
    for item in items {
        let Some(predicate) = item.get("predicate").filter(|p| p.is_object()) else {
            continue;
        };
        if let Some(keys) = predicate.as_object() {
            other += keys.keys().filter(|k| !KNOWN.contains(&k.as_str())).count();
        }
        cases.push(OverrideCase {
            model: text(item, "model").unwrap_or_default().to_owned(),
            angle: num_at(predicate, "angle"),
            blocking: int_at(predicate, "blocking"),
            broken: int_at(predicate, "broken"),
            cast: int_at(predicate, "cast"),
            damage: num_at(predicate, "damage"),
            damaged: int_at(predicate, "damaged"),
            lefthanded: int_at(predicate, "lefthanded"),
            pull: num_at(predicate, "pull"),
            pulling: int_at(predicate, "pulling"),
            time: num_at(predicate, "time"),
        });
    }
    if other > 0 {
        notices.push(Notice::Skipped {
            what: "override predicates of other kinds".to_owned(),
            count: other,
        });
    }
    Ok(Some(cases))
}
