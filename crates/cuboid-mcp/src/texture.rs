//! Textures: `texture_paint` composes pixels and sends whole pictures to the core, since the
//! commands replace pictures; `texture_view` draws a texture enlarged with the rectangles of the
//! faces that use it.

use std::collections::BTreeMap;

use cuboid_core::{Batch, Command};
use cuboid_io::texture_files::{DiskFiles, TextureFiles};
use cuboid_library::meta::png_size;
use cuboid_library::outline;
use cuboid_model::Document;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::edit::MAX_TEXTURE;
use crate::error::ToolError;
use crate::inspect::decimal;
use crate::picture::{Picture, parse_color};
use crate::schema::{
    array_of, boolean, dry_run, expected_revision, integer, model, object, select, set, string,
};
use crate::select::{Selector, Tree, id_of};
use crate::server::{OpenModel, Output, Server};

/// The largest grid or rectangle of one op, per side.
const MAX_GRID: usize = 512;
/// Ops in a call.
const MAX_OPS: usize = 500;
/// The side a picture of a changed region is enlarged to, at most.
const PREVIEW: u32 = 256;
/// Pictures of changed regions in a result.
const MAX_PREVIEWS: usize = 4;
/// Rectangles `texture_view` lists.
const MAX_RECTS: usize = 120;
/// The longest side of a picture in a result.
pub const MAX_IMAGE: u32 = 1024;
/// Colours of the outlines of the rectangles, by element.
const OUTLINES: [[u8; 4]; 8] = [
    [230, 25, 75, 255],
    [60, 180, 75, 255],
    [255, 225, 25, 255],
    [0, 130, 200, 255],
    [245, 130, 48, 255],
    [145, 30, 180, 255],
    [70, 240, 240, 255],
    [240, 50, 230, 255],
];
const CHECKER: ([u8; 4], [u8; 4]) = ([204, 204, 204, 255], [153, 153, 153, 255]);

/// Arguments of `texture_paint`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TexturePaint {
    model: String,
    texture: Option<String>,
    ops: Vec<PaintOp>,
    expected_revision: Option<String>,
    dry_run: Option<bool>,
}

/// One change to a picture.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum PaintOp {
    PaintGrid {
        texture: Option<String>,
        at: [u32; 2],
        legend: BTreeMap<String, Option<String>>,
        rows: Vec<String>,
    },
    FillRect {
        texture: Option<String>,
        rect: [u32; 4],
        color: Option<String>,
    },
    ImportPng {
        texture: Option<String>,
        path: String,
    },
}

impl PaintOp {
    fn texture(&self) -> Option<&str> {
        match self {
            PaintOp::PaintGrid { texture, .. }
            | PaintOp::FillRect { texture, .. }
            | PaintOp::ImportPng { texture, .. } => texture.as_deref(),
        }
    }
}

/// Arguments of `texture_view`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureView {
    model: String,
    texture: String,
    scale: Option<u32>,
    select: Option<Selector>,
}

pub(crate) fn texture_paint_input() -> Value {
    let top = string("The texture the ops paint when they name none.");
    let mut schema = object(
        json!({
            "model": model(),
            "texture": top,
            "ops": {
                "type": "array",
                "minItems": 1,
                "maxItems": 500,
                "description": "Pixels are composed in order and sent as one picture per texture.",
                "items": {"oneOf": [
                    {
                        "type": "object",
                        "description": "One character per pixel from the legend; null in the legend is transparent. Rows run top down, x to the right.",
                        "properties": {
                            "op": {"const": "paint_grid"},
                            "texture": string("Label."),
                            "at": {"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 2, "maxItems": 2, "description": "[x, y] of the top left pixel."},
                            "legend": {"type": "object", "description": "One character to #RRGGBB(AA) or null.", "additionalProperties": {"type": ["string", "null"]}},
                            "rows": {"type": "array", "items": {"type": "string"}, "maxItems": 512},
                        },
                        "required": ["op", "at", "legend", "rows"],
                        "additionalProperties": false,
                    },
                    {
                        "type": "object",
                        "description": "Fills a rectangle.",
                        "properties": {
                            "op": {"const": "fill_rect"},
                            "texture": string("Label."),
                            "rect": {"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 4, "maxItems": 4, "description": "[x, y, width, height]."},
                            "color": {"type": ["string", "null"], "description": "#RRGGBB(AA); null is transparent."},
                        },
                        "required": ["op", "rect"],
                        "additionalProperties": false,
                    },
                    {
                        "type": "object",
                        "description": "Replaces the picture with a PNG file of the project folder, or adds the texture when there is none of that name. At most 1024 px a side.",
                        "properties": {
                            "op": {"const": "import_png"},
                            "texture": string("Label."),
                            "path": string("The PNG, relative to the project folder."),
                        },
                        "required": ["op", "path"],
                        "additionalProperties": false,
                    },
                ]},
            },
            "expected_revision": expected_revision(),
            "dry_run": dry_run(),
        }),
        &["model", "ops"],
    );
    let examples = json!([
        {
            "model": "lamp.cubik",
            "texture": "body",
            "ops": [
                {"op": "fill_rect", "rect": [0, 0, 4, 4], "color": "#1d1b24"},
                {
                    "op": "paint_grid",
                    "at": [0, 0],
                    "legend": {".": null, "k": "#1d1b24", "s": "#7b8496", "h": "#d6dde8"},
                    "rows": ["..kk", ".ksk", "kshk", "kssk"]
                }
            ]
        },
        {"model": "lamp.cubik", "dry_run": true, "ops": [{"op": "import_png", "texture": "shade", "path": "art/shade.png"}]}
    ]);
    set(&mut schema, "examples", examples);
    schema
}

pub(crate) fn texture_paint_output() -> Value {
    object(
        json!({
            "model": string("The model."),
            "revision": string("The revision after the edit; the one before for a dry run."),
            "dry_run": boolean("The picture was only composed: the model is not changed."),
            "changed": array_of(object(
                json!({
                    "texture": string("Label."),
                    "size": array_of(json!({"type": "integer"})),
                    "region": array_of(json!({"type": "integer"})),
                    "new": boolean("The texture did not exist before."),
                }),
                &["texture", "size", "region", "new"],
            )),
        }),
        &["model", "revision", "dry_run", "changed"],
    )
}

pub(crate) fn texture_view_input() -> Value {
    let mut only = select();
    set(
        &mut only,
        "description",
        json!(
            "Outline only the faces of these objects: an id (e12, g3), a path, a glob or a list of them (default: all)."
        ),
    );
    let mut schema = object(
        json!({
            "model": model(),
            "texture": string("Label."),
            "scale": integer("Pixels per texel, 1 to 16 (default: the picture fits 512 px). The picture is at most 1024 px, so a large texture gets less."),
            "select": only,
        }),
        &["model", "texture"],
    );
    let examples = json!([
        {"model": "lamp.cubik", "texture": "body"},
        {"model": "lamp.cubik", "texture": "body", "scale": 8, "select": "lamp/**"}
    ]);
    set(&mut schema, "examples", examples);
    schema
}

pub(crate) fn texture_view_output() -> Value {
    object(
        json!({
            "model": string("The model."),
            "revision": string("The revision of the model."),
            "texture": string("Label."),
            "size": array_of(json!({"type": "integer"})),
            "scale": integer("Pixels per texel in the picture; 1 for a texture over 1024 px, which is reduced to fit."),
            "rects": array_of(object(
                json!({
                    "id": string("Of the element."),
                    "path": string("Of the element."),
                    "face": string("down, up, north, south, west or east."),
                    "uv": array_of(json!({"type": "number"})),
                    "rotation": integer("Turn of the picture in the rectangle, when not 0."),
                }),
                &["id", "path", "face", "uv"],
            )),
            "more": integer("Rectangles not listed."),
        }),
        &["model", "revision", "texture", "size", "scale", "rects"],
    )
}

/// A picture being composed.
struct Work {
    label: String,
    picture: Picture,
    new: bool,
    /// `[x0, y0, x1, y1]` of what the ops touched, the end exclusive.
    region: Option<[u32; 4]>,
}

impl Work {
    fn touch(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        let [a, b, c, d] = self.region.get_or_insert([x0, y0, x1, y1]);
        (*a, *b, *c, *d) = ((*a).min(x0), (*b).min(y0), (*c).max(x1), (*d).max(y1));
    }
}

/// The colour of a legend entry or a fill; `None` is transparent.
fn color_of(text: Option<&str>) -> Result<[u8; 4], ToolError> {
    text.map_or(Ok([0; 4]), parse_color)
}

impl Server<'_> {
    /// Composes the ops over the pictures of `doc`.
    fn compose(
        &self,
        doc: &Document,
        top: Option<&str>,
        ops: &[PaintOp],
    ) -> Result<Vec<Work>, ToolError> {
        let mut work: Vec<Work> = Vec::new();
        for (index, op) in ops.iter().enumerate() {
            let label = op.texture().or(top).ok_or_else(|| {
                ToolError::input("name the texture in the op or in `texture`").at(index)
            })?;
            let part = match work.iter().position(|part| part.label == label) {
                Some(at) => work.get_mut(at),
                None => {
                    let entry = doc
                        .textures
                        .entries
                        .iter()
                        .find(|entry| &*entry.label == label);
                    let part = match (entry, op) {
                        (Some(entry), _) => Work {
                            label: label.to_owned(),
                            picture: Picture::of_texture(&entry.texture.image),
                            new: false,
                            region: None,
                        },
                        (None, PaintOp::ImportPng { .. }) => Work {
                            label: label.to_owned(),
                            picture: Picture::new(1, 1, [0; 4]),
                            new: true,
                            region: None,
                        },
                        (None, _) => {
                            return Err(ToolError::no_match(format!(
                                "the model has no texture {label}"
                            ))
                            .at(index));
                        }
                    };
                    work.push(part);
                    work.last_mut()
                }
            }
            .ok_or_else(|| ToolError::internal("a texture of the call is gone"))?;
            self.paint(part, op).map_err(|error| error.at(index))?;
        }
        Ok(work)
    }

    fn paint(&self, part: &mut Work, op: &PaintOp) -> Result<(), ToolError> {
        let (width, height) = (part.picture.width, part.picture.height);
        match op {
            PaintOp::PaintGrid {
                at, legend, rows, ..
            } => {
                let mut colors: BTreeMap<char, [u8; 4]> = BTreeMap::new();
                for (key, value) in legend {
                    let mut chars = key.chars();
                    let (Some(symbol), None) = (chars.next(), chars.next()) else {
                        return Err(ToolError::input(format!(
                            "the legend key `{key}` is not one character"
                        )));
                    };
                    colors.insert(symbol, color_of(value.as_deref())?);
                }
                let widest = rows
                    .iter()
                    .map(|row| row.chars().count())
                    .max()
                    .unwrap_or(0);
                if rows.is_empty() || rows.len() > MAX_GRID || widest > MAX_GRID {
                    return Err(ToolError::limit(format!(
                        "a grid is 1 to {MAX_GRID} rows of at most {MAX_GRID} pixels"
                    )));
                }
                if u64::from(at[0]) + widest as u64 > u64::from(width)
                    || u64::from(at[1]) + rows.len() as u64 > u64::from(height)
                {
                    return Err(ToolError::input(format!(
                        "the grid of {widest} x {} at {},{} reaches outside the texture of {width} x {height}",
                        rows.len(),
                        at[0],
                        at[1]
                    )));
                }
                for (dy, row) in rows.iter().enumerate() {
                    for (dx, symbol) in row.chars().enumerate() {
                        let color = colors.get(&symbol).ok_or_else(|| {
                            ToolError::input(format!("`{symbol}` is not in the legend"))
                        })?;
                        part.picture
                            .set(at[0] + dx as u32, at[1] + dy as u32, *color);
                    }
                }
                part.touch(
                    at[0],
                    at[1],
                    at[0] + widest as u32,
                    at[1] + rows.len() as u32,
                );
            }
            PaintOp::FillRect {
                rect: [x, y, w, h],
                color,
                ..
            } => {
                let color = color_of(color.as_deref())?;
                if *w == 0
                    || *h == 0
                    || u64::from(*x) + u64::from(*w) > u64::from(width)
                    || u64::from(*y) + u64::from(*h) > u64::from(height)
                {
                    return Err(ToolError::input(format!(
                        "the rectangle {w} x {h} at {x},{y} is empty or reaches outside the texture of {width} x {height}"
                    )));
                }
                for row in *y..y + h {
                    for column in *x..x + w {
                        part.picture.set(column, row, color);
                    }
                }
                part.touch(*x, *y, x + w, y + h);
            }
            PaintOp::ImportPng { path, .. } => {
                let file = self.root.resolve(path)?;
                let bytes = DiskFiles
                    .read(&file)
                    .map_err(|refusal| ToolError::path(format!("{path}: {refusal}")))?
                    .ok_or_else(|| ToolError::no_match(format!("{path} is not a file")))?;
                // The size is in the header: a picture over the limit is not decoded.
                if let Some((w, h)) = png_size(&bytes)
                    && (w > MAX_TEXTURE || h > MAX_TEXTURE)
                {
                    return Err(ToolError::limit(format!(
                        "{path} is {w} x {h}; a texture is at most {MAX_TEXTURE} px a side"
                    )));
                }
                let picture = Picture::from_png(&bytes)?;
                part.touch(0, 0, picture.width, picture.height);
                part.picture = picture;
            }
        }
        Ok(())
    }

    pub(crate) fn texture_paint(&mut self, args: TexturePaint) -> Result<Output, ToolError> {
        if args.ops.is_empty() || args.ops.len() > MAX_OPS {
            return Err(ToolError::input(format!("ops holds 1 to {MAX_OPS} ops")));
        }
        let dry = args.dry_run.unwrap_or(false);
        let index = self.model(&args.model)?;
        if !dry {
            self.writable(index)?;
        }
        let doc = self.open(index)?.core.doc();
        self.check_revision(doc, args.expected_revision.as_deref())?;
        let mut work = self.compose(doc, args.texture.as_deref(), &args.ops)?;
        work.retain(|part| part.region.is_some());
        let mut labels: Vec<String> = work.iter().map(|part| part.label.clone()).collect();
        let label = format!(
            "Agent paint ({} op{})",
            args.ops.len(),
            if args.ops.len() == 1 { "" } else { "s" }
        );
        let body = |batch: &mut Batch<'_>| {
            for (part, shown) in work.iter().zip(labels.iter_mut()) {
                let (width, height) = (part.picture.width, part.picture.height);
                let rgba = part.picture.texture_pixels();
                let command = if part.new {
                    Command::AddTexture {
                        label: part.label.clone(),
                        width,
                        height,
                        rgba,
                    }
                } else {
                    Command::ReplaceTexture {
                        label: part.label.clone(),
                        width,
                        height,
                        rgba,
                        file: None,
                    }
                };
                match batch.execute(command) {
                    Ok(_) => {}
                    Err(outcome) => {
                        return Err(ToolError::of(&outcome)
                            .unwrap_or_else(|| ToolError::io("a command failed")));
                    }
                }
                if part.new
                    && let Some(entry) = batch.core().doc().textures.entries.last()
                {
                    *shown = entry.label.to_string();
                }
            }
            Ok(())
        };
        // A dry run sends the same commands to a copy, so that it refuses what a real run would.
        let OpenModel { core, .. } = self.open_mut(index)?;
        if dry {
            core.scratch().batch(&label, body)?;
        } else {
            core.batch(&label, body)?;
        }
        let mut images = Vec::new();
        let mut changed = Vec::new();
        for (part, shown) in work.iter().zip(&labels) {
            let Some([x0, y0, x1, y1]) = part.region else {
                continue;
            };
            let mut crop = Picture::new(x1 - x0, y1 - y0, [0; 4]);
            for y in y0..y1 {
                for x in x0..x1 {
                    crop.set(x - x0, y - y0, part.picture.get(x, y));
                }
            }
            if images.len() < MAX_PREVIEWS {
                // Fit first, so that a large region is neither copied nor enlarged at full size.
                let fitted = crop.fitted(MAX_IMAGE);
                let scale = (PREVIEW / fitted.width.max(fitted.height)).clamp(1, 16);
                let mut shown_crop = fitted.enlarged(scale);
                shown_crop.over_checker(8, CHECKER.0, CHECKER.1);
                images.push(shown_crop.png()?);
            }
            changed.push(json!({
                "texture": shown,
                "size": [part.picture.width, part.picture.height],
                "region": [x0, y0, x1 - x0, y1 - y0],
                "new": part.new,
            }));
        }
        let model = self.open(index)?;
        Ok(Output {
            value: json!({
                "model": model.name,
                "revision": self.token(model.core.doc()),
                "dry_run": dry,
                "changed": changed,
            }),
            images,
        })
    }

    pub(crate) fn texture_view(&mut self, args: TextureView) -> Result<Output, ToolError> {
        let index = self.model(&args.model)?;
        let model = self.open(index)?;
        let doc = model.core.doc();
        let entry = doc
            .textures
            .entries
            .iter()
            .find(|entry| *entry.label == *args.texture)
            .ok_or_else(|| {
                ToolError::no_match(format!("the model has no texture {}", args.texture))
            })?;
        let image = &entry.texture.image;
        let (width, height) = (image.w, image.h);
        let longest = width.max(height).max(1);
        // The picture is at most MAX_IMAGE px, so a scale that would exceed it is lowered before
        // anything is enlarged; a texture over it is reduced instead and shows at scale 1.
        let scale = args
            .scale
            .unwrap_or_else(|| 512 / longest)
            .clamp(1, 16)
            .min(MAX_IMAGE / longest)
            .max(1);
        let mut shown = Picture::of_texture(image).fitted(MAX_IMAGE).enlarged(scale);
        shown.over_checker(8, CHECKER.0, CHECKER.1);
        let tree = Tree::of(doc);
        let only = args
            .select
            .as_ref()
            .map(|selector| tree.select(selector))
            .transpose()?;
        let mut rects = Vec::new();
        let mut more = 0;
        let whole = outline(doc);
        for (nth, element) in whole.elements.iter().enumerate() {
            if only
                .as_ref()
                .is_some_and(|only| !only.contains(&element.node))
            {
                continue;
            }
            for (face, drawn) in &element.faces.0 {
                if drawn.texture != args.texture {
                    continue;
                }
                if rects.len() >= MAX_RECTS {
                    more += 1;
                    continue;
                }
                let [u0, v0, u1, v1] = drawn.uv;
                let colour = OUTLINES
                    .get(nth % OUTLINES.len())
                    .copied()
                    .unwrap_or_default();
                outline_rect(&mut shown, [u0, v0, u1, v1], [width, height], colour);
                let mut rect = json!({
                    "id": id_of(element.node),
                    "path": element.path,
                    "face": face,
                    "uv": drawn.uv.map(decimal),
                });
                if drawn.uv_rotation != 0 {
                    set(&mut rect, "rotation", json!(drawn.uv_rotation));
                }
                rects.push(rect);
            }
        }
        let (frame_width, frame_height) = entry.frame_size();
        let mut value = json!({
            "model": model.name,
            "revision": self.token(doc),
            "texture": args.texture,
            "size": [frame_width, frame_height],
            "scale": scale,
            "rects": rects,
        });
        if more > 0 {
            set(&mut value, "more", json!(more));
        }
        Ok(Output {
            value,
            images: vec![shown.fitted(MAX_IMAGE).png()?],
        })
    }
}

/// Draws the outline of a rectangle of texels, whichever corner comes first. `texture` is the
/// size of the texture in texels: the picture may be a reduced or enlarged copy of it.
fn outline_rect(picture: &mut Picture, uv: [f32; 4], texture: [u32; 2], color: [u8; 4]) {
    let per_x = picture.width as f32 / texture[0] as f32;
    let per_y = picture.height as f32 / texture[1] as f32;
    let scaled = |value: f32, per_texel: f32| (value * per_texel).round().max(0.0) as u32;
    let (x0, x1) = (
        scaled(uv[0].min(uv[2]), per_x),
        scaled(uv[0].max(uv[2]), per_x),
    );
    let (y0, y1) = (
        scaled(uv[1].min(uv[3]), per_y),
        scaled(uv[1].max(uv[3]), per_y),
    );
    let (last_x, last_y) = (
        picture.width.saturating_sub(1),
        picture.height.saturating_sub(1),
    );
    let (x1, y1) = (x1.min(last_x), y1.min(last_y));
    for x in x0.min(last_x)..=x1 {
        picture.set(x, y0.min(last_y), color);
        picture.set(x, y1, color);
    }
    for y in y0.min(last_y)..=y1 {
        picture.set(x0.min(last_x), y, color);
        picture.set(x1, y, color);
    }
}
