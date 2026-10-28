//! `model_edit`: the ops of agent toolkit R1, 5.3, run as one transaction of the core, so that a
//! call is one undo step and a refused op rolls the whole call back.
//!
//! Positions are in model space as a Java model has them (16 px a block, y up, z south, angles in
//! degrees); the commands of the core take the stored fields, whose z is the mirror of that. A
//! later op may name by path what an earlier one of the batch created.

use cuboid_core::{
    AlignEdge, AutoUvStyle, Axis, Batch, CmdOutcome, Command, Core, DisplayContext, Done,
    ElementKind, FaceId, Target, UvAxis,
};
use cuboid_fmt_cubik::ProjectMeta;
use cuboid_library::Library;
use cuboid_library::check::check_document;
use cuboid_model::{Document, ElementFlags, FaceName, ObjId, ObjRef, mc};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::path::Path;

use crate::check::TargetWord;
use crate::error::ToolError;
use crate::fixes::{FixSummary, apply_in, face_id, fixable};
use crate::picture::parse_color;
use crate::schema::{boolean, expected_revision, model, object, select, string, vec3};
use crate::select::{Selector, Tree, id_of};
use crate::server::{OpenModel, Output, Server};

/// Ops in a call.
const MAX_OPS: usize = 500;
/// Commands in a call; one op can expand to many.
const MAX_COMMANDS: usize = 20_000;
/// The largest texture a tool makes, per side.
pub const MAX_TEXTURE: u32 = 1024;
/// A rotation that differs this much from the one asked for was changed by the rotation rule.
const ROTATION_TOLERANCE: f32 = 0.01;

/// Arguments of `model_edit`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEdit {
    model: String,
    ops: Vec<Op>,
    expected_revision: Option<String>,
    dry_run: Option<bool>,
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AxisWord {
    X,
    Y,
    Z,
}

impl From<AxisWord> for Axis {
    fn from(axis: AxisWord) -> Self {
        match axis {
            AxisWord::X => Axis::X,
            AxisWord::Y => Axis::Y,
            AxisWord::Z => Axis::Z,
        }
    }
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum EdgeWord {
    Min,
    Center,
    Max,
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum FaceWord {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl From<FaceWord> for FaceId {
    fn from(face: FaceWord) -> Self {
        match face {
            FaceWord::Down => FaceId::Down,
            FaceWord::Up => FaceId::Up,
            FaceWord::North => FaceId::North,
            FaceWord::South => FaceId::South,
            FaceWord::West => FaceId::West,
            FaceWord::East => FaceId::East,
        }
    }
}

/// The neighbour a face is culled against, or `none`.
#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum CullWord {
    None,
    Down,
    Up,
    North,
    South,
    West,
    East,
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StyleWord {
    Box,
    PerFace,
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum UvAxisWord {
    U,
    V,
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ContextWord {
    ThirdpersonRighthand,
    ThirdpersonLefthand,
    FirstpersonRighthand,
    FirstpersonLefthand,
    Gui,
    Head,
    Ground,
    Fixed,
}

impl From<ContextWord> for DisplayContext {
    fn from(context: ContextWord) -> Self {
        match context {
            ContextWord::ThirdpersonRighthand => DisplayContext::ThirdRight,
            ContextWord::ThirdpersonLefthand => DisplayContext::ThirdLeft,
            ContextWord::FirstpersonRighthand => DisplayContext::FirstRight,
            ContextWord::FirstpersonLefthand => DisplayContext::FirstLeft,
            ContextWord::Gui => DisplayContext::Gui,
            ContextWord::Head => DisplayContext::Head,
            ContextWord::Ground => DisplayContext::Ground,
            ContextWord::Fixed => DisplayContext::Fixed,
        }
    }
}

/// The size of a texture: one number for a square, or width and height.
#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(untagged)]
enum Size {
    Square(u32),
    Sides([u32; 2]),
}

/// One edit.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Op {
    AddCube {
        name: Option<String>,
        from: [f32; 3],
        to: [f32; 3],
        parent: Option<String>,
        texture: Option<String>,
    },
    Group {
        select: Selector,
        name: String,
        pivot: Option<[f32; 3]>,
    },
    Set {
        select: Selector,
        name: Option<String>,
        from: Option<[f32; 3]>,
        to: Option<[f32; 3]>,
        rotation: Option<[f32; 3]>,
        pivot: Option<[f32; 3]>,
        visible: Option<bool>,
        locked: Option<bool>,
    },
    Move {
        select: Selector,
        by: [f32; 3],
    },
    Delete {
        select: Selector,
    },
    Duplicate {
        select: Selector,
    },
    Mirror {
        select: Selector,
        axis: AxisWord,
        copy: Option<bool>,
    },
    Align {
        select: Selector,
        axis: AxisWord,
        edge: EdgeWord,
    },
    Reparent {
        select: Selector,
        parent: Option<String>,
        index: Option<usize>,
    },
    SetFace {
        select: Selector,
        faces: Option<Vec<FaceWord>>,
        texture: Option<String>,
        uv: Option<[f32; 4]>,
        uv_rotation: Option<i32>,
        enabled: Option<bool>,
        cull: Option<CullWord>,
        tint: Option<i32>,
    },
    AutoUv {
        select: Selector,
        faces: Option<Vec<FaceWord>>,
        style: Option<StyleWord>,
    },
    FlipUv {
        select: Selector,
        faces: Option<Vec<FaceWord>>,
        axis: UvAxisWord,
    },
    AddTexture {
        name: Option<String>,
        size: Size,
        fill: Option<String>,
    },
    RemoveTexture {
        name: String,
    },
    SetDisplay {
        context: ContextWord,
        rotation: Option<[f32; 3]>,
        translation: Option<[f32; 3]>,
        scale: Option<[f32; 3]>,
        preset: Option<String>,
    },
    ApplyFixes {
        codes: Option<Vec<String>>,
        target: Option<TargetWord>,
    },
}

pub(crate) fn model_edit_input() -> Value {
    let face_list = json!({
        "type": "array",
        "items": {"type": "string", "enum": ["down", "up", "north", "south", "west", "east"]},
        "description": "Faces; default all six.",
    });
    let op = |name: &str, properties: Value, required: &[&str]| {
        let mut schema = object(properties, required);
        schema["properties"]["op"] = json!({"const": name});
        let mut needed = vec!["op"];
        needed.extend_from_slice(required);
        schema["required"] = json!(needed);
        schema
    };
    let ops = json!([
        op(
            "add_cube",
            json!({
                "name": string("Name of the element."),
                "from": vec3("Low corner in model space."),
                "to": vec3("High corner."),
                "parent": string("A group, by id or path; default the top level."),
                "texture": string("Texture label for all six faces."),
            }),
            &["from", "to"],
        ),
        op(
            "group",
            json!({
                "select": select(),
                "name": string("Name of the new group."),
                "pivot": vec3("Pivot; default the centre of what is grouped."),
            }),
            &["select", "name"],
        ),
        op(
            "set",
            json!({
                "select": select(),
                "name": string("New name for every object selected."),
                "from": vec3("Low corner of one element."),
                "to": vec3("High corner of one element."),
                "rotation": vec3("Degrees about x, y, z; one axis, in steps the version allows. Turns about the pivot."),
                "pivot": vec3("Pivot. With a rotation, the point it turns about; alone, it moves without moving the geometry. Default for a first rotation: the current pivot."),
                "visible": boolean("Show or hide."),
                "locked": boolean("Lock or unlock."),
            }),
            &["select"],
        ),
        op(
            "move",
            json!({"select": select(), "by": vec3("Offset in px.")}),
            &["select", "by"],
        ),
        op("delete", json!({"select": select()}), &["select"]),
        op("duplicate", json!({"select": select()}), &["select"]),
        op(
            "mirror",
            json!({
                "select": select(),
                "axis": {"type": "string", "enum": ["x", "y", "z"]},
                "copy": boolean("Mirror copies and keep the originals."),
            }),
            &["select", "axis"],
        ),
        op(
            "align",
            json!({
                "select": select(),
                "axis": {"type": "string", "enum": ["x", "y", "z"]},
                "edge": {"type": "string", "enum": ["min", "center", "max"]},
            }),
            &["select", "axis", "edge"],
        ),
        op(
            "reparent",
            json!({
                "select": select(),
                "parent": {"type": ["string", "null"], "description": "A group, by id or path; null for the top level."},
                "index": {"type": "integer", "minimum": 0, "description": "Place among the children that stay; default the end."},
            }),
            &["select"],
        ),
        op(
            "set_face",
            json!({
                "select": select(),
                "faces": face_list,
                "texture": string("Texture label."),
                "uv": {
                    "type": "array",
                    "items": {"type": "number"},
                    "minItems": 4,
                    "maxItems": 4,
                    "description": "[u0, v0, u1, v1] in texels of the texture, v down, as model_get shows them.",
                },
                "uv_rotation": {"type": "integer", "description": "0, 90, 180 or 270."},
                "enabled": boolean("Draw and export the face."),
                "cull": {"type": "string", "enum": ["none", "down", "up", "north", "south", "west", "east"]},
                "tint": {"type": "integer", "minimum": -1, "description": "Tint index, -1 for none."},
            }),
            &["select"],
        ),
        op(
            "auto_uv",
            json!({
                "select": select(),
                "faces": face_list,
                "style": {"type": "string", "enum": ["box", "per_face"], "description": "box (default) or per_face."},
            }),
            &["select"],
        ),
        op(
            "flip_uv",
            json!({
                "select": select(),
                "faces": face_list,
                "axis": {"type": "string", "enum": ["u", "v"], "description": "u flips left to right."},
            }),
            &["select", "axis"],
        ),
        op(
            "add_texture",
            json!({
                "name": string("Label; a number is added when it is taken."),
                "size": {
                    "description": "Width and height in texels, at most 1024; one number for a square.",
                    "oneOf": [
                        {"type": "integer", "minimum": 1, "maximum": 1024},
                        {"type": "array", "items": {"type": "integer", "minimum": 1, "maximum": 1024}, "minItems": 2, "maxItems": 2},
                    ],
                },
                "fill": string("#RRGGBB or #RRGGBBAA; default transparent."),
            }),
            &["size"],
        ),
        op(
            "remove_texture",
            json!({"name": string("Label.")}),
            &["name"],
        ),
        op(
            "set_display",
            json!({
                "context": {"type": "string", "enum": ["thirdperson_righthand", "thirdperson_lefthand", "firstperson_righthand", "firstperson_lefthand", "gui", "head", "ground", "fixed"]},
                "rotation": vec3("Degrees."),
                "translation": vec3("In 1/16 block."),
                "scale": vec3("Factors."),
                "preset": string("A built-in preset such as item/handheld; the values above replace its own."),
            }),
            &["context"],
        ),
        op(
            "apply_fixes",
            json!({
                "codes": {"type": "array", "items": {"type": "string"}, "description": "Issue codes to fix; default every issue that has a fix."},
                "target": {"type": "string", "enum": ["block", "item", "pack"], "description": "What to validate for; default what the model is."},
            }),
            &[],
        ),
    ]);
    let mut schema = object(
        json!({
            "model": model(),
            "ops": {
                "type": "array",
                "minItems": 1,
                "maxItems": 500,
                "description": "Run in order as one batch and one undo step; a refused op undoes the whole call. A later op may name by path what an earlier one made.",
                "items": {"oneOf": ops},
            },
            "expected_revision": expected_revision(),
            "dry_run": crate::schema::dry_run(),
        }),
        &["model", "ops"],
    );
    schema["examples"] = json!([
        {
            "model": "lamp.cubik",
            "expected_revision": "1a2b3c4d-41",
            "ops": [
                {"op": "add_cube", "name": "base", "from": [4, 0, 4], "to": [12, 2, 12], "texture": "texture"},
                {"op": "group", "select": "base", "name": "lamp", "pivot": [8, 0, 8]},
                {"op": "set", "select": "lamp/base", "rotation": [0, 22.5, 0], "pivot": [8, 0, 8]},
                {"op": "move", "select": "lamp", "by": [0, 1, 0]},
                {"op": "duplicate", "select": "lamp/base"},
                {"op": "mirror", "select": "lamp/base", "axis": "x", "copy": true},
                {"op": "align", "select": ["lamp/base", "e12"], "axis": "y", "edge": "min"},
                {"op": "reparent", "select": "e12", "parent": null}
            ]
        },
        {
            "model": "lamp.cubik",
            "dry_run": true,
            "ops": [
                {"op": "add_texture", "name": "body", "size": [16, 16], "fill": "#808080"},
                {"op": "set_face", "select": "lamp/**", "faces": ["north", "south"], "texture": "body", "uv": [0, 0, 8, 2], "uv_rotation": 90},
                {"op": "auto_uv", "select": "e12", "style": "per_face"},
                {"op": "flip_uv", "select": "e12", "faces": ["up"], "axis": "u"},
                {"op": "set_display", "context": "gui", "rotation": [30, 225, 0], "scale": [0.8, 0.8, 0.8]},
                {"op": "remove_texture", "name": "particle"},
                {"op": "delete", "select": "e12"},
                {"op": "apply_fixes", "codes": ["GEO_BOUNDS"], "target": "block"}
            ]
        }
    ]);
    schema
}

pub(crate) fn model_edit_output() -> Value {
    object(
        json!({
            "model": string("The model."),
            "revision": string("The revision after the edit; the one before for a dry run."),
            "dry_run": boolean("The edit was only tried on a copy: its ids are not the ones a real run makes."),
            "changed": object(
                json!({
                    "added": {"type": "integer"},
                    "updated": {"type": "integer"},
                    "removed": {"type": "integer"},
                }),
                &["added", "updated", "removed"],
            ),
            "ids": {
                "type": "object",
                "description": "Id of each object the call made, by its path.",
                "additionalProperties": {"type": "string"},
            },
            "textures": {"type": "array", "items": {"type": "string"}, "description": "Labels of the textures the call added."},
            "warnings": {"type": "array", "items": {"type": "object"}},
        }),
        &[
            "model", "revision", "dry_run", "changed", "ids", "textures", "warnings",
        ],
    )
}

/// What a call did, collected while its ops run.
#[derive(Default)]
struct Report {
    added: usize,
    removed: usize,
    /// The objects that commands changed, each once; the ones the call made or removed are left
    /// out when the result is counted.
    touched: HashSet<ObjRef>,
    /// Commands that were done.
    done: usize,
    commands: usize,
    created: Vec<ObjRef>,
    textures: Vec<String>,
    warnings: Vec<Value>,
}

/// What `apply_fixes` needs to check the model.
struct FixContext<'a> {
    path: &'a Path,
    project: Option<&'a ProjectMeta>,
    library: Option<&'a Library>,
}

/// The ops of one call, running in a transaction.
struct Exec<'x, 'c> {
    batch: &'x mut Batch<'c>,
    report: &'x mut Report,
    fix: &'x FixContext<'x>,
    /// The op that runs.
    index: usize,
}

/// A change to the box of an element, in model space.
#[derive(Default)]
struct BoxPatch {
    from: Option<[f32; 3]>,
    to: Option<[f32; 3]>,
    rotation: Option<[f32; 3]>,
    pivot: Option<[f32; 3]>,
}

fn point(value: [f32; 3]) -> glam::Vec3 {
    glam::Vec3::from_array(value)
}

fn check_box(from: [f32; 3], to: [f32; 3]) -> Result<(), ToolError> {
    if (0..3).any(|axis| to[axis] < from[axis]) {
        return Err(ToolError::input("`to` is below `from` on an axis"));
    }
    Ok(())
}

/// The face ids of a list; all six when it is absent.
fn face_ids(faces: Option<&Vec<FaceWord>>) -> Vec<FaceId> {
    faces.map_or_else(Vec::new, |faces| {
        faces.iter().map(|&face| face.into()).collect()
    })
}

/// The six faces, or the ones listed.
fn face_names(faces: Option<&Vec<FaceWord>>) -> Vec<FaceName> {
    if faces.is_none_or(|faces| faces.is_empty()) {
        return FaceName::ALL.to_vec();
    }
    face_ids(faces).into_iter().map(Into::into).collect()
}

impl Exec<'_, '_> {
    fn core(&self) -> &Core {
        self.batch.core()
    }

    fn doc(&self) -> &Document {
        self.batch.core().doc()
    }

    /// Runs a command. `None` when it changed nothing. A command that is not done ends the call.
    fn run(&mut self, command: Command) -> Result<Option<Done>, ToolError> {
        self.report.commands += 1;
        if self.report.commands > MAX_COMMANDS {
            return Err(ToolError::limit(format!(
                "the call needs more than {MAX_COMMANDS} commands"
            )));
        }
        let deleting = matches!(command, Command::Delete { .. });
        let named: Vec<ObjRef> = command
            .targets()
            .iter()
            .filter_map(|&target| self.core().object_of(target))
            .collect();
        match self.batch.execute(command) {
            Ok(CmdOutcome::Done(done)) => {
                self.report.done += 1;
                if done.created.is_empty() {
                    if deleting {
                        self.report.removed += done.affected;
                    } else {
                        self.report.touched.extend(named);
                    }
                }
                for target in &done.created {
                    self.report.added += 1;
                    if let Some(obj) = self.core().object_of(*target) {
                        self.report.created.push(obj);
                    }
                }
                Ok(Some(done))
            }
            Ok(_) => Ok(None),
            Err(outcome) => {
                Err(ToolError::of(&outcome).unwrap_or_else(|| ToolError::io("a command failed")))
            }
        }
    }

    fn warn(&mut self, code: &str, ids: &[String], hint: String) {
        self.report.warnings.push(json!({
            "code": code,
            "op": self.index,
            "ids": ids,
            "hint": hint,
        }));
    }

    /// The objects a selector names now, and their command targets.
    fn pick(&self, selector: &Selector) -> Result<(Vec<ObjRef>, Vec<Target>), ToolError> {
        let objs = Tree::of(self.doc()).select(selector)?;
        let targets = crate::select::targets(self.core(), &objs);
        Ok((objs, targets))
    }

    /// The group a selector names, as a command target.
    fn group_target(&self, selector: &str) -> Result<Target, ToolError> {
        let obj = Tree::of(self.doc()).one(&Selector::One(selector.to_owned()))?;
        match obj {
            ObjRef::Group(_) => self
                .core()
                .target_of(obj)
                .ok_or_else(|| ToolError::no_match(format!("{selector} is gone"))),
            _ => Err(ToolError::input(format!("{selector} is not a group"))),
        }
    }

    /// The elements among the objects and inside the groups among them.
    fn elements(&self, objs: &[ObjRef]) -> Result<Vec<ObjId>, ToolError> {
        let doc = self.doc();
        let groups: Vec<ObjId> = objs
            .iter()
            .filter_map(|obj| match obj {
                ObjRef::Group(id) => Some(*id),
                _ => None,
            })
            .collect();
        let mut found: Vec<ObjId> = Vec::new();
        for obj in objs.iter().copied().chain(doc.groups.descendants(&groups)) {
            if let ObjRef::Element(id) = obj
                && !found.contains(&id)
            {
                found.push(id);
            }
        }
        if found.is_empty() {
            return Err(ToolError::no_match("the selection holds no element"));
        }
        Ok(found)
    }

    fn element_targets(&self, ids: &[ObjId]) -> Vec<Target> {
        ids.iter()
            .filter_map(|&id| self.core().target_of(ObjRef::Element(id)))
            .collect()
    }

    fn need_texture(&self, label: &str) -> Result<(), ToolError> {
        let known = self
            .doc()
            .textures
            .entries
            .iter()
            .any(|entry| &*entry.label == label);
        if known {
            Ok(())
        } else {
            Err(ToolError::no_match(format!(
                "the model has no texture {label}"
            )))
        }
    }

    fn apply(&mut self, op: &Op) -> Result<(), ToolError> {
        match op {
            Op::AddCube {
                name,
                from,
                to,
                parent,
                texture,
            } => self.add_cube(
                name.as_deref(),
                *from,
                *to,
                parent.as_deref(),
                texture.as_deref(),
            ),
            Op::Group {
                select,
                name,
                pivot,
            } => self.group(select, name, *pivot),
            Op::Set {
                select,
                name,
                from,
                to,
                rotation,
                pivot,
                visible,
                locked,
            } => {
                let patch = BoxPatch {
                    from: *from,
                    to: *to,
                    rotation: *rotation,
                    pivot: *pivot,
                };
                self.set(select, name.as_deref(), patch, *visible, *locked)
            }
            Op::Move { select, by } => {
                let (_, targets) = self.pick(select)?;
                // The stored z runs the other way.
                let delta = [by[0], by[1], -by[2]];
                self.run(Command::MoveBy {
                    targets,
                    delta,
                    snap: None,
                })?;
                Ok(())
            }
            Op::Delete { select } => {
                let (_, targets) = self.pick(select)?;
                self.run(Command::Delete { targets })?;
                Ok(())
            }
            Op::Duplicate { select } => {
                let (_, targets) = self.pick(select)?;
                self.run(Command::Duplicate { targets })?;
                Ok(())
            }
            Op::Mirror { select, axis, copy } => {
                let (_, targets) = self.pick(select)?;
                let copy = copy.unwrap_or(false);
                self.run(Command::Mirror {
                    targets,
                    axis: (*axis).into(),
                    copy,
                })?;
                Ok(())
            }
            Op::Align { select, axis, edge } => {
                let (_, targets) = self.pick(select)?;
                // The lower z of a Java model is the higher stored z.
                let edge =
                    match (edge, axis) {
                        (EdgeWord::Center, _) => AlignEdge::Center,
                        (EdgeWord::Min, AxisWord::Z)
                        | (EdgeWord::Max, AxisWord::X | AxisWord::Y) => AlignEdge::Max,
                        (EdgeWord::Max, AxisWord::Z)
                        | (EdgeWord::Min, AxisWord::X | AxisWord::Y) => AlignEdge::Min,
                    };
                self.run(Command::Align {
                    targets,
                    axis: (*axis).into(),
                    edge,
                })?;
                Ok(())
            }
            Op::Reparent {
                select,
                parent,
                index,
            } => {
                let (_, targets) = self.pick(select)?;
                let parent = parent
                    .as_deref()
                    .map(|text| self.group_target(text))
                    .transpose()?;
                self.run(Command::Reparent {
                    targets,
                    parent,
                    index: *index,
                })?;
                Ok(())
            }
            Op::SetFace {
                select,
                faces,
                texture,
                uv,
                uv_rotation,
                enabled,
                cull,
                tint,
            } => self.set_face(
                select,
                faces.as_ref(),
                texture.as_deref(),
                *uv,
                *uv_rotation,
                *enabled,
                *cull,
                *tint,
            ),
            Op::AutoUv {
                select,
                faces,
                style,
            } => {
                let (objs, _) = self.pick(select)?;
                let targets = self.element_targets(&self.elements(&objs)?);
                let style = match style {
                    Some(StyleWord::PerFace) => AutoUvStyle::PerFace,
                    _ => AutoUvStyle::Box,
                };
                self.run(Command::AutoUv {
                    targets,
                    style,
                    faces: face_ids(faces.as_ref()),
                })?;
                Ok(())
            }
            Op::FlipUv {
                select,
                faces,
                axis,
            } => {
                let (objs, _) = self.pick(select)?;
                let targets = self.element_targets(&self.elements(&objs)?);
                let axis = match axis {
                    UvAxisWord::U => UvAxis::Horizontal,
                    UvAxisWord::V => UvAxis::Vertical,
                };
                self.run(Command::FlipUv {
                    targets,
                    faces: face_ids(faces.as_ref()),
                    axis,
                })?;
                Ok(())
            }
            Op::AddTexture { name, size, fill } => {
                self.add_texture(name.as_deref(), *size, fill.as_deref())
            }
            Op::RemoveTexture { name } => {
                self.need_texture(name)?;
                self.run(Command::RemoveTexture {
                    label: name.clone(),
                })?;
                Ok(())
            }
            Op::SetDisplay {
                context,
                rotation,
                translation,
                scale,
                preset,
            } => {
                self.run(Command::SetDisplaySlot {
                    slot: (*context).into(),
                    enabled: Some(true),
                    rotation: *rotation,
                    translation: *translation,
                    scale: *scale,
                    preset: preset.clone(),
                })?;
                Ok(())
            }
            Op::ApplyFixes { codes, target } => self.apply_fixes(codes.as_deref(), *target),
        }
    }

    fn add_cube(
        &mut self,
        name: Option<&str>,
        from: [f32; 3],
        to: [f32; 3],
        parent: Option<&str>,
        texture: Option<&str>,
    ) -> Result<(), ToolError> {
        check_box(from, to)?;
        let parent = parent.map(|text| self.group_target(text)).transpose()?;
        if let Some(label) = texture {
            self.need_texture(label)?;
        }
        let made = self.run(Command::AddElement(ElementKind::Cube))?;
        let Some(&target) = made.as_ref().and_then(|done| done.created.first()) else {
            return Err(ToolError::io("the element was not made"));
        };
        let centre = [0, 1, 2].map(|axis| (from[axis] + to[axis]) / 2.0);
        let patch = BoxPatch {
            from: Some(from),
            to: Some(to),
            rotation: None,
            pivot: Some(centre),
        };
        self.place(target, &patch)?;
        self.run(Command::Rename {
            targets: vec![target],
            name: name.unwrap_or("cube").to_owned(),
        })?;
        if parent.is_some() {
            self.run(Command::Reparent {
                targets: vec![target],
                parent,
                index: None,
            })?;
        }
        if let Some(label) = texture {
            self.run(Command::AssignTexture {
                targets: vec![target],
                label: label.to_owned(),
                faces: Vec::new(),
            })?;
        }
        Ok(())
    }

    fn group(
        &mut self,
        select: &Selector,
        name: &str,
        pivot: Option<[f32; 3]>,
    ) -> Result<(), ToolError> {
        let (_, targets) = self.pick(select)?;
        let made = self.run(Command::Group {
            targets,
            name: name.to_owned(),
        })?;
        let Some(&group) = made.as_ref().and_then(|done| done.created.first()) else {
            return Ok(());
        };
        if let Some(pivot) = pivot {
            self.run(Command::SetGroup {
                group,
                name: None,
                origin: Some([pivot[0], pivot[1], -pivot[2]]),
                visible: None,
                locked: None,
                export: None,
            })?;
        }
        Ok(())
    }

    fn set(
        &mut self,
        select: &Selector,
        name: Option<&str>,
        patch: BoxPatch,
        visible: Option<bool>,
        locked: Option<bool>,
    ) -> Result<(), ToolError> {
        let (objs, targets) = self.pick(select)?;
        if let Some(name) = name {
            self.run(Command::Rename {
                targets: targets.clone(),
                name: name.to_owned(),
            })?;
        }
        if let Some(visible) = visible {
            self.run(Command::SetHidden {
                targets: targets.clone(),
                hidden: !visible,
            })?;
        }
        if let Some(locked) = locked {
            self.run(Command::SetLocked {
                targets: targets.clone(),
                locked,
            })?;
        }
        let boxed = patch.from.is_some() || patch.to.is_some() || patch.rotation.is_some();
        if !boxed && patch.pivot.is_none() {
            return Ok(());
        }
        let elements: Vec<&ObjRef> = objs
            .iter()
            .filter(|obj| matches!(obj, ObjRef::Element(_)))
            .collect();
        if (patch.from.is_some() || patch.to.is_some()) && elements.len() > 1 {
            return Err(ToolError::input(
                "from and to describe one box: select one element",
            ));
        }
        for obj in &objs {
            let target = self.core().target_of(*obj);
            match (obj, target) {
                (ObjRef::Element(_), Some(target)) => self.place(target, &patch)?,
                (ObjRef::Group(_), Some(group)) if !boxed => {
                    let pivot = patch.pivot.map(|p| [p[0], p[1], -p[2]]);
                    self.run(Command::SetGroup {
                        group,
                        name: None,
                        origin: pivot,
                        visible: None,
                        locked: None,
                        export: None,
                    })?;
                }
                _ => {
                    return Err(ToolError::input(
                        "from, to and rotation apply to elements; a group takes a pivot",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Gives an element the box, rotation and pivot of the patch, the rest staying as it is.
    fn place(&mut self, target: Target, patch: &BoxPatch) -> Result<(), ToolError> {
        let Some(ObjRef::Element(id)) = self.core().object_of(target) else {
            return Err(ToolError::input("only elements have a box"));
        };
        let current = self.java_box(id)?;
        let boxed = patch.from.is_some() || patch.to.is_some();
        if boxed {
            let from = patch.from.map_or(current.from, point);
            let to = patch.to.map_or(current.to, point);
            check_box(from.to_array(), to.to_array())?;
        }
        if let Some(rotation) = patch.rotation {
            self.rotate(id, target, &current, rotation)?;
        }
        if !(boxed || (patch.pivot.is_some() && patch.rotation.is_some())) {
            if patch.rotation.is_none()
                && let Some(pivot) = patch.pivot
            {
                // Alone, the pivot moves and the geometry stays.
                self.run(Command::SetPivot {
                    targets: vec![target],
                    value: [Some(pivot[0]), Some(pivot[1]), Some(-pivot[2])],
                })?;
            }
            return Ok(());
        }
        // The box that was asked for, turned by the rotation the element has now: that is the
        // one the rotation rule left.
        let actual = self.java_box(id)?;
        let wanted = mc::McBox {
            from: patch.from.map_or(current.from, point),
            to: patch.to.map_or(current.to, point),
            pivot: patch.pivot.map_or(current.pivot, point),
            rotation: actual.rotation,
        };
        let placed = mc::place(&wanted, mc::Rigid::IDENTITY);
        self.run(Command::SetSize {
            targets: vec![target],
            value: placed.s.to_array().map(Some),
        })?;
        self.run(Command::SetPosition {
            targets: vec![target],
            value: placed.p.to_array().map(Some),
        })?;
        self.run(Command::SetPivot {
            targets: vec![target],
            value: placed.o.to_array().map(Some),
        })?;
        Ok(())
    }

    /// The box of an element as a Java model has it.
    fn java_box(&self, id: ObjId) -> Result<mc::McBox, ToolError> {
        let element = self
            .doc()
            .elements
            .get(id)
            .ok_or_else(|| ToolError::no_match("the element is gone"))?;
        let base = &element.base;
        let invert = element.flags.contains(ElementFlags::INVERT_NORMALS);
        Ok(mc::unplace(base.p, base.s, base.o, base.e, invert))
    }

    /// Turns an element to a rotation about its pivot. The rotation rule of the version may keep
    /// the old angle or round the new one; the op says so in a warning.
    fn rotate(
        &mut self,
        id: ObjId,
        target: Target,
        current: &mc::McBox,
        rotation: [f32; 3],
    ) -> Result<(), ToolError> {
        let wanted = mc::McBox {
            rotation: point(rotation),
            ..*current
        };
        let stored = mc::place(&wanted, mc::Rigid::IDENTITY).e;
        let have = self.doc().elements.get(id).map(|element| element.base.e);
        if have.is_some_and(|have| have != glam::Vec3::ZERO && have != stored) {
            // One rotated axis at a time: change the axis through zero.
            self.run(Command::SetRotation {
                targets: vec![target],
                value: [Some(0.0); 3],
            })?;
        }
        self.run(Command::SetRotation {
            targets: vec![target],
            value: stored.to_array().map(Some),
        })?;
        let after = self.java_box(id)?.rotation;
        let differs = (0..3).any(|axis| (after[axis] - rotation[axis]).abs() > ROTATION_TOLERANCE);
        if differs {
            let ids = vec![id_of(ObjRef::Element(id)).unwrap_or_default()];
            self.warn(
                "W_ROTATION_SNAPPED",
                &ids,
                format!(
                    "the rotation is {:?}, not {rotation:?}: the rotation rule of this model's version only allows the rotation it kept (Restrict rotations: one rotated axis, in steps of 22.5 degrees)",
                    after.to_array()
                ),
            );
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn set_face(
        &mut self,
        select: &Selector,
        faces: Option<&Vec<FaceWord>>,
        texture: Option<&str>,
        uv: Option<[f32; 4]>,
        uv_rotation: Option<i32>,
        enabled: Option<bool>,
        cull: Option<CullWord>,
        tint: Option<i32>,
    ) -> Result<(), ToolError> {
        if texture.is_none()
            && uv.is_none()
            && uv_rotation.is_none()
            && enabled.is_none()
            && cull.is_none()
            && tint.is_none()
        {
            return Err(ToolError::input(
                "set_face needs one of texture, uv, uv_rotation, enabled, cull, tint",
            ));
        }
        let (objs, _) = self.pick(select)?;
        let ids = self.elements(&objs)?;
        let targets = self.element_targets(&ids);
        let listed = face_ids(faces);
        if let Some(label) = texture {
            self.need_texture(label)?;
            self.run(Command::AssignTexture {
                targets: targets.clone(),
                label: label.to_owned(),
                faces: listed.clone(),
            })?;
        }
        if uv.is_some() || uv_rotation.is_some() {
            for &id in &ids {
                let Some(target) = self.core().target_of(ObjRef::Element(id)) else {
                    continue;
                };
                for face in face_names(faces) {
                    let rect = self.uv_rect(id, face, uv);
                    self.run(Command::SetUvRect {
                        targets: vec![target],
                        faces: vec![face_id(face)],
                        rect,
                        rotation: uv_rotation,
                    })?;
                }
            }
        }
        if let Some(enabled) = enabled {
            self.run(Command::SetFaceEnabled {
                targets: targets.clone(),
                faces: listed.clone(),
                enabled,
            })?;
        }
        if let Some(cull) = cull {
            let cull = match cull {
                CullWord::None => None,
                CullWord::Down => Some(FaceId::Down),
                CullWord::Up => Some(FaceId::Up),
                CullWord::North => Some(FaceId::North),
                CullWord::South => Some(FaceId::South),
                CullWord::West => Some(FaceId::West),
                CullWord::East => Some(FaceId::East),
            };
            self.run(Command::SetFaceCull {
                targets: targets.clone(),
                faces: listed.clone(),
                cull,
            })?;
        }
        if let Some(tint) = tint {
            self.run(Command::SetFaceTint {
                targets,
                faces: listed,
                tint,
            })?;
        }
        Ok(())
    }

    /// The rectangle `SetUvRect` takes for a face: normalised, v up. `uv` is in texels of the
    /// texture the face has, v down; without it the face keeps its rectangle.
    fn uv_rect(&self, id: ObjId, face: FaceName, uv: Option<[f32; 4]>) -> [f32; 4] {
        let doc = self.doc();
        let Some(source) = doc
            .elements
            .get(id)
            .map(|element| &element.faces[face as usize])
        else {
            return [0.0, 0.0, 1.0, 1.0];
        };
        let Some(uv) = uv else {
            let (tl, br) = (source.uv.tl(), source.uv.br());
            return [tl.x, tl.y, br.x, br.y];
        };
        let (w, h) = doc
            .textures
            .entries
            .iter()
            .find(|entry| entry.label == source.texture)
            .map_or((16, 16), |entry| entry.frame_size());
        let (w, h) = (w as f32, h as f32);
        [uv[0] / w, 1.0 - uv[3] / h, uv[2] / w, 1.0 - uv[1] / h]
    }

    fn add_texture(
        &mut self,
        name: Option<&str>,
        size: Size,
        fill: Option<&str>,
    ) -> Result<(), ToolError> {
        let (width, height) = match size {
            Size::Square(side) => (side, side),
            Size::Sides([w, h]) => (w, h),
        };
        if !(1..=MAX_TEXTURE).contains(&width) || !(1..=MAX_TEXTURE).contains(&height) {
            return Err(ToolError::limit(format!(
                "a texture is at most {MAX_TEXTURE} px a side"
            )));
        }
        let color = fill.map_or(Ok([0; 4]), parse_color)?;
        self.run(Command::CreateTexture {
            label: name.map(ToOwned::to_owned),
            width,
            height,
            color,
        })?;
        if let Some(entry) = self.doc().textures.entries.last() {
            let label = entry.label.to_string();
            self.report.textures.push(label);
        }
        Ok(())
    }

    fn apply_fixes(
        &mut self,
        codes: Option<&[String]>,
        target: Option<TargetWord>,
    ) -> Result<(), ToolError> {
        if let Some(bad) = codes.into_iter().flatten().find(|code| {
            !cuboid_validate::rules()
                .iter()
                .any(|rule| rule.code == code.as_str())
        }) {
            return Err(ToolError::input(format!(
                "`{bad}` is not a code of the validator"
            )));
        }
        let fix = self.fix;
        let checked = check_document(
            fix.path,
            self.doc(),
            fix.project,
            Vec::new(),
            fix.library,
            target.map(Into::into),
        );
        let issues: Vec<_> = checked
            .issues
            .iter()
            .filter(|issue| fixable(issue))
            .filter(|issue| codes.is_none_or(|codes| codes.iter().any(|code| code == issue.code)))
            .collect();
        let mut summary = FixSummary::default();
        apply_in(self.batch, &issues, &mut summary);
        self.report.done += summary.applied;
        let fixed = summary.fixed.iter().map(|&id| ObjRef::Element(id));
        self.report.touched.extend(fixed);
        for why in summary.failed {
            self.warn("W_FIX_FAILED", &[], why);
        }
        Ok(())
    }
}

/// What a call left in the document: the id of each object it made, by its path, and the number
/// of objects it changed that were there before it and still are.
fn result_of(doc: &Document, report: &Report) -> (Map<String, Value>, usize) {
    let tree = Tree::of(doc);
    let mut ids = Map::new();
    for &obj in &report.created {
        let (Some(path), Some(id)) = (tree.path_of(obj), id_of(obj)) else {
            continue;
        };
        let mut key = path.to_owned();
        let mut copy = 1;
        while ids.contains_key(&key) {
            copy += 1;
            key = format!("{path}~{copy}");
        }
        ids.insert(key, json!(id));
    }
    let updated = report
        .touched
        .iter()
        .filter(|&&obj| !report.created.contains(&obj) && tree.path_of(obj).is_some())
        .count();
    (ids, updated)
}

impl Server<'_> {
    pub(crate) fn model_edit(&mut self, args: ModelEdit) -> Result<Output, ToolError> {
        if args.ops.is_empty() {
            return Err(ToolError::input("ops is empty"));
        }
        if args.ops.len() > MAX_OPS {
            return Err(ToolError::limit(format!(
                "a call takes at most {MAX_OPS} ops"
            )));
        }
        let dry = args.dry_run.unwrap_or(false);
        let index = self.model(&args.model)?;
        if !dry {
            self.writable(index)?;
        }
        self.check_revision(
            self.models[index].core.doc(),
            args.expected_revision.as_deref(),
        )?;
        let library = args
            .ops
            .iter()
            .any(|op| matches!(op, Op::ApplyFixes { .. }))
            .then(|| self.read_library())
            .transpose()?;
        let label = format!(
            "Agent edit ({} op{})",
            args.ops.len(),
            if args.ops.len() == 1 { "" } else { "s" }
        );
        let mut report = Report::default();
        let OpenModel {
            core,
            path,
            project,
            ..
        } = &mut self.models[index];
        let fix = FixContext {
            path,
            project: project.as_ref(),
            library: library.as_ref(),
        };
        let body = |batch: &mut Batch<'_>| -> Result<(), ToolError> {
            let mut exec = Exec {
                batch,
                report: &mut report,
                fix: &fix,
                index: 0,
            };
            for (index, op) in args.ops.iter().enumerate() {
                exec.index = index;
                let before = exec.report.done;
                exec.apply(op).map_err(|error| error.at(index))?;
                if exec.report.done == before {
                    exec.warn("W_UNCHANGED", &[], "the op changed nothing".to_owned());
                }
            }
            Ok(())
        };
        let (ids, updated) = if dry {
            let mut scratch = core.scratch();
            scratch.batch("Agent edit", body)?;
            result_of(scratch.doc(), &report)
        } else {
            core.batch(&label, body)?;
            result_of(core.doc(), &report)
        };
        let doc = self.models[index].core.doc();
        Ok(Output::json(json!({
            "model": self.models[index].name,
            "revision": self.token(doc),
            "dry_run": dry,
            "changed": {"added": report.added, "updated": updated, "removed": report.removed},
            "ids": ids,
            "textures": report.textures,
            "warnings": report.warnings,
        })))
    }
}
