//! The render tools: `render_view` and `render_sheet`. The renderer draws; the background, the
//! contact sheet, the numbers about the picture and the file under `renders/` are made here.

use cuboid_library::doc::load;
use cuboid_model::Document;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::inspect::decimal;
use crate::picture::{Picture, parse_color};
use crate::render::{Camera, RenderError};
use crate::schema::{array_of, boolean, integer, number, object, string};
use crate::server::{Output, Server};
use crate::texture::MAX_IMAGE;

/// The side of a view unless the caller asks for another.
const VIEW_SIZE: u32 = 512;
/// The largest side of a view; the adapter may allow less.
pub const MAX_VIEW: u32 = 2048;
/// The smallest side of a view.
const MIN_VIEW: u32 = 16;
/// The side of one view of a sheet unless the caller asks for another.
const CELL_SIZE: u32 = 256;
/// Views on a sheet.
pub const MAX_SHEET_VIEWS: usize = 9;
/// The views of a sheet unless the caller lists them.
const SHEET_VIEWS: [&str; 4] = ["front", "right", "top", "iso"];
/// The background unless the caller asks for another.
const BACKGROUND: [u8; 4] = [127, 127, 127, 255];
/// What each camera shows, for the descriptions of the schemas. A camera looks at the face it is
/// named after, so `front` is not the Front view of the application, which looks from the south.
const CAMERAS: &str = "front looks at the north face (-z; the app's Back view), back at the south face, left at the west face (-x), right at the east face, top from above with the north up, bottom from below, iso from above the north-east corner as the inventory shows a block, gui the inventory slot with the GUI light (emulated).";

/// Arguments of `render_view`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderView {
    model: Option<String>,
    #[serde(rename = "ref")]
    reference: Option<String>,
    camera: Option<String>,
    size: Option<u32>,
    background: Option<String>,
}

/// Arguments of `render_sheet`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderSheet {
    model: Option<String>,
    #[serde(rename = "ref")]
    reference: Option<String>,
    views: Option<Vec<String>>,
    size: Option<u32>,
    background: Option<String>,
}

fn camera_names() -> Vec<&'static str> {
    Camera::ALL.iter().map(|camera| camera.name()).collect()
}

fn subject_properties() -> Value {
    json!({
        "model": string("The model to draw, relative to the project folder; or `ref`."),
        "ref": string("The id of a style reference of the library, from brief_get; or `model`."),
    })
}

pub(crate) fn render_view_input() -> Value {
    let mut properties = subject_properties();
    properties["camera"] = json!({
        "type": "string",
        "enum": camera_names(),
        "description": format!("{CAMERAS} The default is iso."),
    });
    properties["size"] = integer("Pixels a side (default 512, at most 2048).");
    properties["background"] = string("#RRGGBB(AA) or transparent (default #7f7f7f).");
    let mut schema = object(properties, &[]);
    schema["examples"] = json!([
        {"model": "lamp.cubik"},
        {"ref": "3fa2c1d09b7e4410", "camera": "front", "size": 256, "background": "transparent"}
    ]);
    schema
}

pub(crate) fn render_view_output() -> Value {
    object(
        json!({
            "model": string("The model drawn."),
            "ref": string("The reference drawn."),
            "camera": string("The camera."),
            "size": array_of(json!({"type": "integer"})),
            "path": string("The PNG under renders/; left out in a read-only server."),
            "bounds": {
                "type": ["array", "null"],
                "items": {"type": "integer"},
                "description": "[x0, y0, x1, y1] of the pixels the model covers, the end exclusive; null when it covers none.",
            },
            "coverage": number("The share of the picture the model covers."),
            "emulated": boolean("The camera is an emulation of the game (gui)."),
        }),
        &["camera", "size", "bounds", "coverage", "emulated"],
    )
}

pub(crate) fn render_sheet_input() -> Value {
    let mut properties = subject_properties();
    properties["views"] = json!({
        "type": "array",
        "items": {"type": "string", "enum": camera_names()},
        "minItems": 1,
        "maxItems": 9,
        "description": format!("The cameras, in the order of the grid (default front, right, top, iso): {CAMERAS}"),
    });
    properties["size"] =
        integer("Pixels a side of one view (default 256); a sheet is at most 1024 px.");
    properties["background"] = string("#RRGGBB(AA) or transparent (default #7f7f7f).");
    let mut schema = object(properties, &[]);
    schema["examples"] = json!([
        {"model": "lamp.cubik"},
        {"model": "lamp.cubik", "views": ["front", "back", "left", "right", "top", "bottom", "iso", "gui"], "size": 200}
    ]);
    schema
}

pub(crate) fn render_sheet_output() -> Value {
    object(
        json!({
            "model": string("The model drawn."),
            "ref": string("The reference drawn."),
            "views": array_of(json!({"type": "string"})),
            "columns": integer("Views in a row of the grid; they fill it row by row."),
            "rows": integer("Rows of the grid."),
            "cell": integer("Pixels a side of one view."),
            "size": array_of(json!({"type": "integer"})),
            "path": string("The PNG under renders/; left out in a read-only server."),
            "emulated": boolean("A view is an emulation of the game (gui)."),
        }),
        &["views", "columns", "rows", "cell", "size", "emulated"],
    )
}

/// What a render draws, and the names its picture gets.
struct Subject {
    doc: Document,
    /// The start of the file name.
    stem: String,
    /// Changes when the picture would.
    stamp: String,
    /// `model` or `ref`, the key the result names it by.
    key: &'static str,
    name: String,
}

fn render_error(error: RenderError) -> ToolError {
    match error {
        RenderError::NoGpu => ToolError::no_gpu("there is no graphics adapter"),
        RenderError::TooLarge(limit) => ToolError::limit(format!(
            "the graphics adapter draws at most {limit} px a side"
        ))
        .hinted("Ask for a smaller size."),
        RenderError::Failed(text) => ToolError::io(format!("the render failed: {text}")),
    }
}

/// The background colour; transparent when the text says so.
fn background_of(text: Option<&str>) -> Result<[u8; 4], ToolError> {
    match text {
        None => Ok(BACKGROUND),
        Some("transparent") => Ok([0; 4]),
        Some(text) => parse_color(text),
    }
}

impl Server<'_> {
    /// The document a render draws: an open model or a style reference.
    fn subject(
        &mut self,
        model: Option<&str>,
        reference: Option<&str>,
    ) -> Result<Subject, ToolError> {
        match (model, reference) {
            (Some(name), None) => {
                let index = self.model(name)?;
                let open = &self.models[index];
                let stem = open
                    .path
                    .file_stem()
                    .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
                Ok(Subject {
                    doc: open.core.doc().clone(),
                    stem,
                    stamp: format!("r{}", open.core.doc().revision.get()),
                    key: "model",
                    name: open.name.clone(),
                })
            }
            (None, Some(id)) => {
                let library = self.read_library()?;
                let entry = library
                    .entry(id)
                    .ok_or_else(|| ToolError::no_match(format!("the library has no entry {id}")))?;
                if entry.kind != cuboid_library::Kind::Model {
                    return Err(ToolError::input(format!("{id} is not a model")));
                }
                let path = self.stored_copy(&library, entry)?;
                Ok(Subject {
                    doc: load(&path)?.doc,
                    stem: format!("ref-{id}"),
                    stamp: "ref".to_owned(),
                    key: "ref",
                    name: id.to_owned(),
                })
            }
            _ => Err(ToolError::input("give `model` or `ref`, one of them")),
        }
    }

    /// Writes a picture under `renders/`, unless the server is read-only. Returns its path.
    fn save_render(&self, picture: &Picture, name: &str) -> Result<Option<String>, ToolError> {
        if self.options.read_only {
            return Ok(None);
        }
        let path = self.root.resolve(&format!("renders/{name}.png"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        cuboid_io::write_atomic(&path, &picture.png()?)?;
        Ok(Some(self.root.relative(&path)))
    }

    pub(crate) fn render_view(&mut self, args: RenderView) -> Result<Output, ToolError> {
        let camera = match args.camera.as_deref() {
            None => Camera::Iso,
            Some(name) => Camera::from_name(name).ok_or_else(|| {
                ToolError::input(format!(
                    "`{name}` is not a camera: {}",
                    camera_names().join(", ")
                ))
            })?,
        };
        let size = args.size.unwrap_or(VIEW_SIZE);
        if size > MAX_VIEW {
            return Err(ToolError::limit(format!(
                "a view is at most {MAX_VIEW} px a side"
            )));
        }
        let background = background_of(args.background.as_deref())?;
        let subject = self.subject(args.model.as_deref(), args.reference.as_deref())?;
        let mut picture = self
            .renderer
            .render(&subject.doc, camera, size.max(MIN_VIEW))
            .map_err(render_error)?;
        let (bounds, coverage) = picture.coverage();
        picture.over(background);
        let file = format!("{}-{}-{}", subject.stem, camera.name(), subject.stamp);
        let path = self.save_render(&picture, &file)?;
        let mut value = json!({
            "camera": camera.name(),
            "size": [picture.width, picture.height],
            "bounds": bounds,
            "coverage": decimal((coverage * 1000.0).round() / 1000.0),
            "emulated": camera == Camera::Gui,
        });
        value[subject.key] = json!(subject.name);
        if let Some(path) = path {
            value["path"] = json!(path);
        }
        Ok(Output {
            value,
            images: vec![picture.fitted(MAX_IMAGE).png()?],
        })
    }

    pub(crate) fn render_sheet(&mut self, args: RenderSheet) -> Result<Output, ToolError> {
        let names = args
            .views
            .unwrap_or_else(|| SHEET_VIEWS.iter().map(|name| (*name).to_owned()).collect());
        if names.is_empty() || names.len() > MAX_SHEET_VIEWS {
            return Err(ToolError::limit(format!(
                "a sheet holds 1 to {MAX_SHEET_VIEWS} views"
            )));
        }
        let cameras: Vec<Camera> = names
            .iter()
            .map(|name| {
                Camera::from_name(name).ok_or_else(|| {
                    ToolError::input(format!(
                        "`{name}` is not a camera: {}",
                        camera_names().join(", ")
                    ))
                })
            })
            .collect::<Result<_, _>>()?;
        let background = background_of(args.background.as_deref())?;
        let columns = (1..=3_u32)
            .find(|n| (n * n) as usize >= cameras.len())
            .unwrap_or(3);
        let rows = (cameras.len() as u32).div_ceil(columns);
        let cell = args
            .size
            .unwrap_or(CELL_SIZE)
            .clamp(MIN_VIEW, MAX_IMAGE / columns);
        let subject = self.subject(args.model.as_deref(), args.reference.as_deref())?;
        let mut sheet = Picture::new(columns * cell, rows * cell, background);
        for (at, camera) in cameras.iter().enumerate() {
            let mut view = self
                .renderer
                .render(&subject.doc, *camera, cell)
                .map_err(render_error)?;
            view.over(background);
            let (column, row) = (at as u32 % columns, at as u32 / columns);
            sheet.paste(&view, column * cell, row * cell);
        }
        let path =
            self.save_render(&sheet, &format!("{}-sheet-{}", subject.stem, subject.stamp))?;
        let mut value = json!({
            "views": names,
            "columns": columns,
            "rows": rows,
            "cell": cell,
            "size": [sheet.width, sheet.height],
            "emulated": cameras.contains(&Camera::Gui),
        });
        value[subject.key] = json!(subject.name);
        if let Some(path) = path {
            value["path"] = json!(path);
        }
        Ok(Output {
            value,
            images: vec![sheet.png()?],
        })
    }
}
