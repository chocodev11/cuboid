//! The outline of a model: the groups, the elements with their faces, the textures and the
//! display settings as an agent reads them. Positions are in model space as a Java model has them
//! (16 px a block, y up, z south, rotations in degrees), rounded to a thousandth, and texture
//! rectangles are in texels of their texture with v down. One function serves the reference
//! reader, the open model and `cuboid-cli outline`.
//!
//! The JSON is compact: whole numbers have no `.0` and a face is one array. A page of
//! [`OUTLINE_PAGE`] elements with six textured faces each and a rotation stays under 12 KB (see
//! the test `a_page_of_the_worst_elements_stays_under_the_result_budget`).

use std::collections::HashMap;

use cuboid_model::{Document, ElementFlags, ObjId, ObjRef, mc};
use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, Serializer};

use crate::meta::round3;

/// Elements on a page of the outline.
pub const OUTLINE_PAGE: usize = 24;

/// The display slots in the order of the model, with the names of the JSON.
const SLOTS: [&str; 8] = [
    "thirdperson_righthand",
    "thirdperson_lefthand",
    "firstperson_righthand",
    "firstperson_lefthand",
    "gui",
    "head",
    "ground",
    "fixed",
];

/// A number that is written without `.0` when it is whole.
struct Number(f32);

impl Serialize for Number {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.fract().abs() < 1e-6 && self.0.abs() < 1e9 {
            serializer.serialize_i64(self.0 as i64)
        } else {
            serializer.serialize_f32(self.0)
        }
    }
}

fn numbers<S: Serializer>(values: &[f32], serializer: S) -> Result<S::Ok, S::Error> {
    let mut seq = serializer.serialize_seq(Some(values.len()))?;
    for value in values {
        seq.serialize_element(&Number(*value))?;
    }
    seq.end()
}

fn maybe_numbers<S: Serializer>(
    values: &Option<[f32; 3]>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match values {
        Some(values) => numbers(values, serializer),
        None => serializer.serialize_none(),
    }
}

/// A group.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OutlineGroup {
    /// The node of the document, for a caller that names objects by id; not in the JSON.
    #[serde(skip)]
    pub node: ObjRef,
    /// The names from the top level down, joined with `/`.
    pub path: String,
    /// The pivot.
    #[serde(serialize_with = "numbers")]
    pub pivot: [f32; 3],
    /// The names of what is inside, in outliner order.
    pub children: Vec<String>,
}

/// A face that is enabled and has a texture. In the JSON an array of the texture label, the four
/// numbers of `uv` and, when it is not 0, `uv_rotation`: `["body", 0, 0, 8, 2]`.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineFace {
    /// The label of the texture.
    pub texture: String,
    /// `[u0, v0, u1, v1]` in texels of the texture (of one frame, for an animation).
    pub uv: [f32; 4],
    /// The turn of the picture in the rectangle: 90, 180 or 270; 0 for none.
    pub uv_rotation: i32,
}

impl Serialize for OutlineFace {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let len = if self.uv_rotation == 0 { 5 } else { 6 };
        let mut seq = serializer.serialize_seq(Some(len))?;
        seq.serialize_element(&self.texture)?;
        for value in self.uv {
            seq.serialize_element(&Number(value))?;
        }
        if self.uv_rotation != 0 {
            seq.serialize_element(&self.uv_rotation)?;
        }
        seq.end()
    }
}

/// The faces by name, in the order down, up, north, south, west, east.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineFaces(pub Vec<(&'static str, OutlineFace)>);

impl Serialize for OutlineFaces {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, face) in &self.0 {
            map.serialize_entry(name, face)?;
        }
        map.end()
    }
}

/// An element.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OutlineElement {
    /// The node of the document, for a caller that names objects by id; not in the JSON.
    #[serde(skip)]
    pub node: ObjRef,
    /// The number in the element list, the one `cuboid-cli check` names an element by.
    pub number: u32,
    /// The names from the top level down, joined with `/`.
    pub path: String,
    /// The `from` corner.
    #[serde(serialize_with = "numbers")]
    pub from: [f32; 3],
    /// The `to` corner.
    #[serde(serialize_with = "numbers")]
    pub to: [f32; 3],
    /// The angles about x, y and z; left out without a rotation.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "maybe_numbers"
    )]
    pub rotation: Option<[f32; 3]>,
    /// The pivot of the rotation; left out without a rotation.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "maybe_numbers"
    )]
    pub pivot: Option<[f32; 3]>,
    /// The faces that are enabled and have a texture.
    pub faces: OutlineFaces,
}

/// A texture.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OutlineTexture {
    /// The label the faces use.
    pub name: String,
    /// Width and height in texels, of one frame for an animation.
    pub size: [u32; 2],
    /// The frames of an animation; left out for a still texture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frames: Option<u32>,
    /// The Minecraft texture id; left out when there is none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
}

/// An enabled display slot.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OutlineDisplay {
    /// `gui`, `head`, `thirdperson_righthand` and so on.
    pub slot: &'static str,
    /// The rotation.
    #[serde(serialize_with = "numbers")]
    pub rotation: [f32; 3],
    /// The translation.
    #[serde(serialize_with = "numbers")]
    pub translation: [f32; 3],
    /// The scale.
    #[serde(serialize_with = "numbers")]
    pub scale: [f32; 3],
}

/// The outline of a model, or a page of it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Outline {
    /// Always `cuboid-outline`.
    pub format: &'static str,
    /// Version of this output.
    pub version: u32,
    /// Elements in the whole model.
    pub total: usize,
    /// Position of the first element listed.
    pub offset: usize,
    /// Position of the first element of the next page; left out on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<usize>,
    /// Meshes, which the outline does not list; left out without meshes.
    #[serde(skip_serializing_if = "is_no_mesh")]
    pub meshes: usize,
    /// The groups, parents first; on the first page only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<OutlineGroup>,
    /// The elements, in outliner order.
    pub elements: Vec<OutlineElement>,
    /// The textures; on the first page only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub textures: Vec<OutlineTexture>,
    /// The display slots that are enabled; on the first page only.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub display: Vec<OutlineDisplay>,
}

fn is_no_mesh(count: &usize) -> bool {
    *count == 0
}

impl Outline {
    /// The page of `limit` elements from position `offset`. The groups, textures and display
    /// slots stay on the first page only. A limit of 0 is read as 1, so that `next` always moves
    /// on.
    pub fn page(mut self, offset: usize, limit: usize) -> Outline {
        let offset = offset.min(self.total);
        let end = offset.saturating_add(limit.max(1)).min(self.total);
        self.elements = self.elements.drain(offset..end).collect();
        self.offset = offset;
        self.next = (end < self.total).then_some(end);
        if offset > 0 {
            self.groups.clear();
            self.textures.clear();
            self.display.clear();
        }
        self
    }
}

fn point(value: [f32; 3]) -> [f32; 3] {
    value.map(round3)
}

/// The name of a node of the outliner.
fn label_of(doc: &Document, node: ObjRef) -> Option<String> {
    match node {
        ObjRef::Element(id) => doc.elements.get(id).map(|e| e.base.label.to_string()),
        ObjRef::Mesh(id) => doc.meshes.get(id).map(|m| m.base.label.to_string()),
        ObjRef::Group(id) => doc.groups.get(id).map(|g| g.name.to_string()),
        ObjRef::Blueprint | ObjRef::ModelOrigin => None,
    }
}

/// The outline of the whole model; [`Outline::page`] cuts it down.
pub fn outline(doc: &Document) -> Outline {
    let sizes: HashMap<&str, (u32, u32)> = doc
        .textures
        .entries
        .iter()
        .map(|entry| (&*entry.label, entry.frame_size()))
        .collect();
    let mut paths: HashMap<ObjId, String> = HashMap::new();
    let mut groups = Vec::new();
    let mut elements = Vec::new();
    let mut meshes = 0;
    for (node, parent) in doc.outline() {
        let above = parent.and_then(|id| paths.get(&id));
        let path_of = |name: &str| match above {
            Some(above) => format!("{above}/{name}"),
            None => name.to_owned(),
        };
        match node {
            ObjRef::Group(id) => {
                let Some(group) = doc.groups.get(id) else {
                    continue;
                };
                let path = path_of(&group.name);
                groups.push(OutlineGroup {
                    node,
                    path: path.clone(),
                    pivot: point(mc::unplace_point(group.origin).to_array()),
                    children: doc
                        .child_nodes(Some(id))
                        .into_iter()
                        .filter_map(|child| label_of(doc, child))
                        .collect(),
                });
                paths.insert(id, path);
            }
            ObjRef::Element(id) => {
                let (Some(element), Some(number)) =
                    (doc.elements.get(id), doc.elements.display_id(id))
                else {
                    continue;
                };
                let base = &element.base;
                let invert = element.flags.contains(ElementFlags::INVERT_NORMALS);
                let boxed = mc::unplace(base.p, base.s, base.o, base.e, invert);
                let turned = boxed.rotation.to_array().iter().any(|angle| *angle != 0.0);
                let faces = element
                    .faces
                    .iter()
                    .filter(|face| face.active && !face.texture.is_empty())
                    .map(|face| {
                        let (w, h) = sizes.get(&*face.texture).copied().unwrap_or((16, 16));
                        let (w, h) = (w as f32, h as f32);
                        let (tl, br) = (face.uv.tl(), face.uv.br());
                        let face_out = OutlineFace {
                            texture: face.texture.to_string(),
                            uv: [tl.x * w, (1.0 - br.y) * h, br.x * w, (1.0 - tl.y) * h]
                                .map(round3),
                            uv_rotation: face.uv.rot().rem_euclid(360),
                        };
                        (mc::face_key(face.name), face_out)
                    })
                    .collect();
                elements.push(OutlineElement {
                    node,
                    number,
                    path: path_of(&base.label),
                    from: point(boxed.from.to_array()),
                    to: point(boxed.to.to_array()),
                    rotation: turned.then(|| point(boxed.rotation.to_array())),
                    pivot: turned.then(|| point(boxed.pivot.to_array())),
                    faces: OutlineFaces(faces),
                });
            }
            ObjRef::Mesh(_) => meshes += 1,
            ObjRef::Blueprint | ObjRef::ModelOrigin => {}
        }
    }
    let textures = doc
        .textures
        .entries
        .iter()
        .map(|entry| {
            let (w, h) = entry.frame_size();
            OutlineTexture {
                name: entry.label.to_string(),
                size: [w, h],
                frames: entry
                    .animation
                    .as_ref()
                    .map(|animation| animation.tiles(entry.texture.image.h)),
                path: entry.texture.source.mc_path.clone(),
            }
        })
        .collect();
    let display = SLOTS
        .into_iter()
        .zip(&doc.display.slots)
        .filter(|(_, slot)| slot.enabled)
        .map(|(name, slot)| OutlineDisplay {
            slot: name,
            rotation: point(slot.rotation.to_array()),
            translation: point(slot.translation.to_array()),
            scale: point(slot.scale.to_array()),
        })
        .collect();
    Outline {
        format: "cuboid-outline",
        version: 1,
        total: elements.len(),
        offset: 0,
        next: None,
        meshes,
        groups,
        elements,
        textures,
        display,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_cuts_the_elements_and_keeps_the_rest_on_the_first_page() {
        let element = |number| OutlineElement {
            node: ObjRef::Element(ObjId::next()),
            number,
            path: format!("e{number}"),
            from: [0.0; 3],
            to: [1.0; 3],
            rotation: None,
            pivot: None,
            faces: OutlineFaces(Vec::new()),
        };
        let whole = Outline {
            format: "cuboid-outline",
            version: 1,
            total: 5,
            offset: 0,
            next: None,
            meshes: 0,
            groups: vec![OutlineGroup {
                node: ObjRef::Group(ObjId::next()),
                path: "g".to_owned(),
                pivot: [0.0; 3],
                children: Vec::new(),
            }],
            elements: (1..=5).map(element).collect(),
            textures: Vec::new(),
            display: Vec::new(),
        };
        let first = whole.clone().page(0, 2);
        assert_eq!((first.offset, first.next), (0, Some(2)));
        assert_eq!(first.elements.len(), 2);
        assert_eq!(first.groups.len(), 1);
        let last = whole.clone().page(4, 2);
        assert_eq!((last.offset, last.next, last.elements.len()), (4, None, 1));
        assert!(last.groups.is_empty());
        let past = whole.clone().page(9, 2);
        assert_eq!((past.offset, past.next, past.elements.len()), (5, None, 0));
        // A limit of 0 still moves on, so that a client that follows `next` ends.
        let none = whole.page(1, 0);
        assert_eq!((none.elements.len(), none.next), (1, Some(2)));
    }

    #[test]
    fn faces_are_written_as_arrays_in_slot_order_and_whole_numbers_have_no_fraction() {
        let face = |texture: &str, uv_rotation| OutlineFace {
            texture: texture.to_owned(),
            uv: [0.0, 0.5, 4.0, 2.25],
            uv_rotation,
        };
        let faces = OutlineFaces(vec![("down", face("a", 0)), ("west", face("b", 90))]);
        let text = serde_json::to_string(&faces).expect("encodes");
        assert_eq!(
            text,
            r#"{"down":["a",0,0.5,4,2.25],"west":["b",0,0.5,4,2.25,90]}"#
        );
    }
}
