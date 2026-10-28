//! Reading a model: `model_outline`, `model_get`, `model_find` and `model_stats`.

use std::collections::HashSet;

use cuboid_library::brief::{DENSITY_TOLERANCE, colours_outside, outside_palette};
use cuboid_library::meta::document_meta;
use cuboid_library::{
    Entry, Kind, Meta, OUTLINE_PAGE, Outline, Role, UNKNOWN_LICENCE, outline, style_summary,
};
use cuboid_model::{Document, ElementFlags, ObjRef, mc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::brief::offset_of;
use crate::error::ToolError;
use crate::schema::{array_of, boolean, integer, model, object, paging, select, string};
use crate::select::{Selector, Tree, id_of};
use crate::server::{Output, Server};

/// Levels of groups a result lists unless the caller asks for more.
pub const GROUP_DEPTH: usize = 3;
/// Names a group lists of its children; the elements list their own paths.
const GROUP_CHILDREN: usize = 24;
/// Groups the first page of an outline lists; the rest is left out like the elements of a page.
const GROUPS_LISTED: usize = 60;
/// Elements of an outline page at most: two pages of the library.
pub const OUTLINE_MAX: usize = OUTLINE_PAGE * 2;
/// Textures a result lists.
const TEXTURES_LISTED: usize = 100;
/// Elements `model_get` returns on a page.
const GET_PAGE: usize = 16;
const GET_MAX: usize = 32;
/// Matches `model_find` returns on a page.
const FIND_PAGE: usize = 100;
const FIND_MAX: usize = 300;
/// The keys of a record that `fields` may name.
const FIELDS: [&str; 10] = [
    "name", "from", "to", "rotation", "pivot", "faces", "visible", "locked", "disabled", "children",
];

/// Arguments of `model_outline`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelOutline {
    model: String,
    select: Option<Selector>,
    depth: Option<usize>,
    cursor: Option<String>,
    limit: Option<usize>,
}

/// Arguments of `model_get`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelGet {
    model: String,
    select: Selector,
    fields: Option<Vec<String>>,
    cursor: Option<String>,
    limit: Option<usize>,
}

/// Arguments of `model_find`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFind {
    model: String,
    name: Option<String>,
    group: Option<String>,
    inside: Option<[[f32; 3]; 2]>,
    texture: Option<String>,
    untextured: Option<bool>,
    cursor: Option<String>,
    limit: Option<usize>,
}

/// Arguments of `model_stats`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelStats {
    model: String,
}

/// The textures of a document with their sizes, at most [`TEXTURES_LISTED`].
pub fn texture_list(doc: &Document) -> Vec<Value> {
    doc.textures
        .entries
        .iter()
        .take(TEXTURES_LISTED)
        .map(|entry| {
            let (w, h) = entry.frame_size();
            json!({"name": entry.label.to_string(), "size": [w, h]})
        })
        .collect()
}

/// A number as a result should read it: whole numbers without a fraction, the others as the
/// shortest decimal of the `f32`. (`json!` widens an `f32` to an `f64`, which makes 0.8 read
/// 0.800000011920929.)
pub fn decimal(value: f32) -> Value {
    if value.fract() == 0.0 && value.abs() < 1e9 {
        return json!(value as i64);
    }
    json!(value.to_string().parse::<f64>().unwrap_or_default())
}

/// A value the way its JSON text writes it, so that its `f32` numbers read as short as they were
/// written and whole numbers have no fraction, as in `cuboid-cli --json`.
pub fn value_of(value: &impl Serialize) -> Result<Value, ToolError> {
    Ok(serde_json::from_str(&serde_json::to_string(value)?)?)
}

/// The box of `ModelMeta::bounds` in model space: the document's z is the mirror of the z of a
/// Java model.
pub fn java_bounds(bounds: Option<[f32; 6]>) -> Value {
    bounds.map_or(Value::Null, |[x0, y0, z0, x1, y1, z1]| {
        json!({
            "from": [decimal(x0), decimal(y0), decimal(-z1)],
            "to": [decimal(x1), decimal(y1), decimal(-z0)],
        })
    })
}

/// An outline as JSON. With `ids` every group and element carries its `id`; groups deeper than
/// `depth` levels are left out, the list stops at [`GROUPS_LISTED`] groups (`groups_total` says
/// how many there are) and long lists of children are cut. `next` is the cursor of the next
/// page, a string.
pub fn outline_value(outline: &Outline, ids: bool, depth: usize) -> Result<Value, ToolError> {
    let mut value = value_of(outline)?;
    if let Some(next) = outline.next
        && let Some(object) = value.as_object_mut()
    {
        object.insert("next".to_owned(), json!(next.to_string()));
    }
    let mut groups_total = 0;
    if let Some(Value::Array(groups)) = value.get_mut("groups") {
        let mut kept = Vec::new();
        for (group, shown) in outline.groups.iter().zip(groups.drain(..)) {
            if group.path.matches('/').count() >= depth {
                continue;
            }
            groups_total += 1;
            if kept.len() >= GROUPS_LISTED {
                continue;
            }
            let Value::Object(mut shown) = shown else {
                continue;
            };
            if let Some(Value::Array(children)) = shown.get_mut("children")
                && children.len() > GROUP_CHILDREN
            {
                let total = children.len();
                children.truncate(GROUP_CHILDREN);
                shown.insert("children_total".to_owned(), json!(total));
            }
            if ids && let Some(id) = id_of(group.node) {
                shown.insert("id".to_owned(), json!(id));
            }
            kept.push(Value::Object(shown));
        }
        *groups = kept;
    }
    if groups_total > GROUPS_LISTED {
        value["groups_total"] = json!(groups_total);
    }
    if ids && let Some(Value::Array(elements)) = value.get_mut("elements") {
        for (element, shown) in outline.elements.iter().zip(elements) {
            if let (Some(id), Some(shown)) = (id_of(element.node), shown.as_object_mut()) {
                shown.insert("id".to_owned(), json!(id));
            }
        }
    }
    Ok(value)
}

pub(crate) fn model_outline_input() -> Value {
    let mut properties = json!({
        "model": model(),
        "select": select(),
        "depth": integer("Levels of groups to list (default 3). Elements always list their full path."),
    });
    paging(&mut properties, "Elements", OUTLINE_PAGE);
    let mut schema = object(properties, &["model"]);
    schema["examples"] = json!([
        {"model": "lamp.cubik"},
        {"model": "lamp.cubik", "select": "head/**", "cursor": "24"}
    ]);
    schema
}

pub(crate) fn model_outline_output() -> Value {
    object(
        json!({
            "model": string("The model."),
            "revision": string("The revision of the model."),
            "format": string("Always cuboid-outline."),
            "version": integer("Version of this output."),
            "total": integer("Elements in the selection."),
            "offset": integer("Position of the first element listed."),
            "next": string("Cursor of the next page; left out on the last."),
            "meshes": integer("Meshes, which are not listed."),
            "groups": array_of(json!({"type": "object"})),
            "groups_total": integer("Groups there are within `depth`, when only the first 60 are listed: narrow with `select`."),
            "elements": array_of(json!({"type": "object"})),
            "textures": array_of(json!({"type": "object"})),
            "display": array_of(json!({"type": "object"})),
        }),
        &["model", "revision", "format", "total", "offset", "elements"],
    )
}

pub(crate) fn model_get_input() -> Value {
    let mut properties = json!({
        "model": model(),
        "select": select(),
        "fields": array_of(json!({"type": "string", "enum": FIELDS})),
    });
    paging(&mut properties, "Objects", GET_PAGE);
    let mut schema = object(properties, &["model", "select"]);
    schema["examples"] = json!([
        {"model": "lamp.cubik", "select": "e12"},
        {"model": "lamp.cubik", "select": ["head/**"], "fields": ["from", "to", "faces"]}
    ]);
    schema
}

pub(crate) fn model_get_output() -> Value {
    object(
        json!({
            "model": string("The model."),
            "revision": string("The revision of the model."),
            "total": integer("Objects the selector names."),
            "offset": integer("Position of the first object listed."),
            "next": string("Cursor of the next page; left out on the last."),
            "objects": array_of(json!({"type": "object"})),
        }),
        &["model", "revision", "total", "offset", "objects"],
    )
}

pub(crate) fn model_find_input() -> Value {
    let mut properties = json!({
        "model": model(),
        "name": string("Glob on the name of the object, or on its path when it has a /."),
        "group": string("A group, by id or path: only what is inside it."),
        "inside": {
            "type": "array",
            "description": "[[x, y, z], [x, y, z]]: elements whose box lies inside this one, in model space.",
            "items": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
            "minItems": 2,
            "maxItems": 2,
        },
        "texture": string("Elements with a face that uses this texture."),
        "untextured": boolean("true: elements with a face that is on and has no texture; false: the others."),
    });
    paging(&mut properties, "Matches", FIND_PAGE);
    let mut schema = object(properties, &["model"]);
    schema["examples"] = json!([
        {"model": "lamp.cubik", "name": "trim*"},
        {"model": "lamp.cubik", "untextured": true},
        {"model": "lamp.cubik", "inside": [[0, 0, 0], [8, 8, 8]], "group": "head"}
    ]);
    schema
}

pub(crate) fn model_find_output() -> Value {
    object(
        json!({
            "model": string("The model."),
            "revision": string("The revision of the model."),
            "total": integer("Objects that match."),
            "offset": integer("Position of the first match listed."),
            "next": string("Cursor of the next page; left out on the last."),
            "matches": array_of(object(
                json!({"id": string("e12 or g3."), "path": string("The path.")}),
                &["id", "path"],
            )),
        }),
        &["model", "revision", "total", "offset", "matches"],
    )
}

pub(crate) fn model_stats_input() -> Value {
    let mut schema = object(json!({"model": model()}), &["model"]);
    schema["examples"] = json!([{"model": "lamp.cubik"}]);
    schema
}

pub(crate) fn model_stats_output() -> Value {
    object(
        json!({
            "model": string("The model."),
            "revision": string("The revision of the model."),
            "format": string("cubik, cub, bbmodel or json."),
            "elements": integer("Elements."),
            "meshes": integer("Meshes."),
            "groups": integer("Groups."),
            "textures": array_of(object(
                json!({"name": string("Label."), "size": array_of(json!({"type": "integer"}))}),
                &["name", "size"],
            )),
            "bounds": {
                "type": ["object", "null"],
                "description": "The box around the elements and meshes, in model space.",
            },
            "style": {
                "type": ["object", "null"],
                "description": "The fields of the style summary of the brief, for this model alone.",
            },
            "targets": array_of(object(
                json!({
                    "name": string("What the brief asks for."),
                    "target": {"description": "The value in the brief."},
                    "actual": {"description": "The value of the model; null when unknown. For the palette, the colours of the model outside it."},
                    "met": {"type": ["boolean", "null"], "description": "Whether the model meets it. The palette is met when fewer than half of the model's colours are outside it, the rule of the brief's suggestions."},
                }),
                &["name", "target", "actual", "met"],
            )),
        }),
        &[
            "model", "revision", "format", "elements", "meshes", "groups", "textures", "targets",
        ],
    )
}

/// The objects a selector names, with the groups' descendants when `deep`.
fn chosen(doc: &Document, selector: &Selector, deep: bool) -> Result<HashSet<ObjRef>, ToolError> {
    let mut keep: HashSet<ObjRef> = Tree::of(doc).select(selector)?.into_iter().collect();
    if deep {
        let groups: Vec<_> = keep
            .iter()
            .filter_map(|obj| match obj {
                ObjRef::Group(id) => Some(*id),
                _ => None,
            })
            .collect();
        keep.extend(doc.groups.descendants(&groups));
    }
    Ok(keep)
}

/// `ModelStats` of a document: the model's own copy of the style summary of the brief.
fn style_entry(doc: &Document, format: &str) -> Entry {
    Entry {
        id: String::new(),
        kind: Kind::Model,
        name: String::new(),
        file: String::new(),
        companion: None,
        source: String::new(),
        sha256: String::new(),
        size: 0,
        tags: Vec::new(),
        licence: UNKNOWN_LICENCE.to_owned(),
        role: Role::Style,
        added: 0,
        meta: Some(Meta::Model(document_meta(format, doc))),
    }
}

impl Server<'_> {
    pub(crate) fn model_outline(&mut self, args: ModelOutline) -> Result<Output, ToolError> {
        let offset = offset_of(args.cursor.as_deref())?;
        let index = self.model(&args.model)?;
        let model = &self.models[index];
        let doc = model.core.doc();
        let mut whole = outline(doc);
        if let Some(selector) = &args.select {
            let keep = chosen(doc, selector, true)?;
            whole.groups.retain(|group| keep.contains(&group.node));
            whole
                .elements
                .retain(|element| keep.contains(&element.node));
            whole.total = whole.elements.len();
        }
        let limit = args.limit.unwrap_or(OUTLINE_PAGE).clamp(1, OUTLINE_MAX);
        let page = whole.page(offset, limit);
        let mut value = outline_value(&page, true, args.depth.unwrap_or(GROUP_DEPTH))?;
        if let Some(object) = value.as_object_mut() {
            let mut head = Map::new();
            head.insert("model".to_owned(), json!(model.name));
            head.insert("revision".to_owned(), json!(self.token(doc)));
            head.append(object);
            *object = head;
        }
        Ok(Output::json(value))
    }

    pub(crate) fn model_get(&mut self, args: ModelGet) -> Result<Output, ToolError> {
        let offset = offset_of(args.cursor.as_deref())?;
        if let Some(bad) = args
            .fields
            .iter()
            .flatten()
            .find(|field| !FIELDS.contains(&field.as_str()))
        {
            return Err(ToolError::input(format!("`{bad}` is not a field")));
        }
        let index = self.model(&args.model)?;
        let model = &self.models[index];
        let doc = model.core.doc();
        let tree = Tree::of(doc);
        let picked = chosen(doc, &args.select, false)?;
        let whole = outline(doc);
        let mut records: Vec<Value> = Vec::new();
        for obj in tree.objects().iter().filter(|obj| picked.contains(obj)) {
            let record = match obj {
                ObjRef::Element(_) => {
                    let Some(element) = whole.elements.iter().find(|e| e.node == *obj) else {
                        continue;
                    };
                    element_record(doc, element)?
                }
                ObjRef::Group(_) => {
                    let Some(group) = whole.groups.iter().find(|g| g.node == *obj) else {
                        continue;
                    };
                    group_record(doc, group)?
                }
                _ => continue,
            };
            records.push(record);
        }
        let total = records.len();
        let offset = offset.min(total);
        let limit = args.limit.unwrap_or(GET_PAGE).clamp(1, GET_MAX);
        let end = (offset + limit).min(total);
        let objects: Vec<Value> = records
            .drain(offset..end)
            .map(|mut record| {
                if let (Some(fields), Some(object)) = (&args.fields, record.as_object_mut()) {
                    object.retain(|key, _| {
                        key == "id" || key == "path" || fields.iter().any(|field| field == key)
                    });
                }
                record
            })
            .collect();
        let mut value = json!({
            "model": model.name,
            "revision": self.token(doc),
            "total": total,
            "offset": offset,
            "objects": objects,
        });
        if end < total {
            value["next"] = json!(end.to_string());
        }
        Ok(Output::json(value))
    }

    pub(crate) fn model_find(&mut self, args: ModelFind) -> Result<Output, ToolError> {
        let offset = offset_of(args.cursor.as_deref())?;
        let index = self.model(&args.model)?;
        let model = &self.models[index];
        let doc = model.core.doc();
        let tree = Tree::of(doc);
        // What is inside the group, not the group itself.
        let inside_group = match &args.group {
            None => None,
            Some(group) => {
                let selector = Selector::One(group.clone());
                let mut inside = chosen(doc, &selector, true)?;
                for obj in tree.select(&selector)? {
                    inside.remove(&obj);
                }
                Some(inside)
            }
        };
        let elements_only =
            args.inside.is_some() || args.texture.is_some() || args.untextured.is_some();
        let mut found: Vec<(String, &str)> = Vec::new();
        for &obj in tree.objects() {
            let (Some(id), Some(path)) = (id_of(obj), tree.path_of(obj)) else {
                continue;
            };
            if inside_group
                .as_ref()
                .is_some_and(|group| !group.contains(&obj))
            {
                continue;
            }
            if let Some(glob) = &args.name {
                let subject = if glob.contains('/') {
                    path
                } else {
                    path.rsplit('/').next().unwrap_or(path)
                };
                if !crate::select::matches(glob, subject) {
                    continue;
                }
            }
            match obj {
                ObjRef::Element(element) => {
                    let Some(element) = doc.elements.get(element) else {
                        continue;
                    };
                    if let Some([low, high]) = args.inside {
                        let base = &element.base;
                        let invert = element.flags.contains(ElementFlags::INVERT_NORMALS);
                        let boxed = mc::unplace(base.p, base.s, base.o, base.e, invert);
                        let (from, to) = (boxed.from.min(boxed.to), boxed.from.max(boxed.to));
                        let within = (0..3).all(|axis| {
                            from[axis] >= low[axis] - 1e-4 && to[axis] <= high[axis] + 1e-4
                        });
                        if !within {
                            continue;
                        }
                    }
                    let active = || element.faces.iter().filter(|face| face.active);
                    if let Some(label) = &args.texture
                        && !active().any(|face| &*face.texture == label.as_str())
                    {
                        continue;
                    }
                    if args.untextured == Some(true)
                        && !active().any(|face| face.texture.is_empty())
                    {
                        continue;
                    }
                    if args.untextured == Some(false)
                        && active().any(|face| face.texture.is_empty())
                    {
                        continue;
                    }
                }
                _ if elements_only => continue,
                _ => {}
            }
            found.push((id, path));
        }
        let total = found.len();
        let offset = offset.min(total);
        let limit = args.limit.unwrap_or(FIND_PAGE).clamp(1, FIND_MAX);
        let end = (offset + limit).min(total);
        let matches: Vec<Value> = found
            .get(offset..end)
            .unwrap_or_default()
            .iter()
            .map(|(id, path)| json!({"id": id, "path": path}))
            .collect();
        let mut value = json!({
            "model": model.name,
            "revision": self.token(doc),
            "total": total,
            "offset": offset,
            "matches": matches,
        });
        if end < total {
            value["next"] = json!(end.to_string());
        }
        Ok(Output::json(value))
    }

    pub(crate) fn model_stats(&mut self, args: ModelStats) -> Result<Output, ToolError> {
        let library = self.read_library()?;
        let index = self.model(&args.model)?;
        let model = &self.models[index];
        let doc = model.core.doc();
        let format = model
            .path
            .extension()
            .map_or_else(String::new, |ext| ext.to_string_lossy().to_lowercase());
        let meta = document_meta(&format, doc);
        let summary = style_summary(&[style_entry(doc, &format)]);
        let bounds = java_bounds(meta.bounds);
        let brief = library.brief();
        let mut targets = Vec::new();
        let mut target = |name: &str, goal: Value, actual: Value, met: Option<bool>| {
            targets.push(json!({"name": name, "target": goal, "actual": actual, "met": met}));
        };
        if let Some(most) = brief.max_elements {
            target(
                "max_elements",
                json!(most),
                json!(meta.elements),
                Some(meta.elements <= most),
            );
        }
        if let Some(goal) = brief.texel_density {
            let met = meta
                .density
                .map(|density| (density - goal).abs() <= goal * DENSITY_TOLERANCE);
            target(
                "texel_density",
                decimal(goal),
                meta.density.map_or(Value::Null, decimal),
                met,
            );
        }
        if let Some(goal) = brief.texture_size {
            let met = meta.texture_size.map(|size| size <= goal);
            target("texture_size", json!(goal), json!(meta.texture_size), met);
        }
        if !brief.palette.is_empty()
            && let Some(style) = &meta.style
        {
            target(
                "palette",
                json!(brief.palette.len()),
                json!(colours_outside(&brief.palette, &style.palette)),
                Some(!outside_palette(&brief.palette, &style.palette)),
            );
        }
        let textures = texture_list(doc);
        Ok(Output::json(json!({
            "model": model.name,
            "revision": self.token(doc),
            "format": format,
            "elements": meta.elements,
            "meshes": meta.meshes,
            "groups": doc.groups.len(),
            "textures": textures,
            "bounds": bounds,
            "style": value_of(&summary)?,
            "targets": targets,
        })))
    }
}

/// The record of an element for `model_get`: the outline entry, the name and what the outline
/// leaves out.
fn element_record(
    doc: &Document,
    element: &cuboid_library::OutlineElement,
) -> Result<Value, ToolError> {
    let mut record = value_of(element)?;
    let Some(object) = record.as_object_mut() else {
        return Ok(record);
    };
    object.remove("number");
    let mut fresh = Map::new();
    fresh.insert("id".to_owned(), json!(id_of(element.node)));
    if let ObjRef::Element(id) = element.node
        && let Some(source) = doc.elements.get(id)
    {
        object.insert("name".to_owned(), json!(source.base.label.to_string()));
        // The outline names the pivot of a turned element only; an edit may need any.
        if !object.contains_key("pivot") {
            let base = &source.base;
            let invert = source.flags.contains(ElementFlags::INVERT_NORMALS);
            let pivot = mc::unplace(base.p, base.s, base.o, base.e, invert).pivot;
            object.insert("pivot".to_owned(), json!(pivot.to_array().map(decimal)));
        }
        object.insert("visible".to_owned(), json!(!doc.is_hidden(element.node)));
        object.insert("locked".to_owned(), json!(doc.is_locked(element.node)));
        let disabled: Vec<&str> = source
            .faces
            .iter()
            .filter(|face| !face.active)
            .map(|face| mc::face_key(face.name))
            .collect();
        if !disabled.is_empty() {
            object.insert("disabled".to_owned(), json!(disabled));
        }
    }
    fresh.append(object);
    Ok(Value::Object(fresh))
}

/// The record of a group for `model_get`.
fn group_record(doc: &Document, group: &cuboid_library::OutlineGroup) -> Result<Value, ToolError> {
    let mut record = value_of(group)?;
    let Some(object) = record.as_object_mut() else {
        return Ok(record);
    };
    let mut fresh = Map::new();
    fresh.insert("id".to_owned(), json!(id_of(group.node)));
    object.insert("visible".to_owned(), json!(!doc.is_hidden(group.node)));
    object.insert("locked".to_owned(), json!(doc.is_locked(group.node)));
    if let Some(Value::Array(children)) = object.get_mut("children") {
        children.truncate(GROUP_CHILDREN);
    }
    fresh.append(object);
    Ok(Value::Object(fresh))
}
