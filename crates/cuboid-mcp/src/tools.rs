//! The catalogue: the 18 tools with their descriptions, schemas and the code that runs them, the
//! instructions the server gives, and the dispatch with its read-only rule and journal.

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::server::{Output, Server};
use crate::{brief, check, edit, history, inspect, project, texture, views};

/// What a client is told about the server (spec: agent toolkit R1, 5.4).
pub const INSTRUCTIONS: &str = "cuboid edits Minecraft Java block and item models in one project folder.\n\
1. Read brief_get first: the game version, the naming and texture rules, the target numbers and, under `style`, what the style references have in common. `typical` lists the references to study first.\n\
2. Study two or three of them: refs_get gives the outline of a reference, render_sheet with `ref` shows it.\n\
3. Make or open your model with project_open (create: item or block for a new one).\n\
4. Build in small batches with model_edit. One call is one undo step and a refused op undoes the whole call. Try with dry_run, and pass expected_revision to be sure nothing else changed the model.\n\
5. After each stage run validate (apply_fixes in model_edit does the mechanical fixes) and render_sheet, and look at the pictures.\n\
6. Paint with texture_paint, check the UV rectangles with texture_view, save often with project_save, and export last with project_export.\n\
Positions are px of a Java model (16 px a block, y up, z south), angles degrees, colours #RRGGBB or #RRGGBBAA. Objects are named by id (e12, g3), path (head/trim_l) or glob (head/**). Lists page with cursor. Paths are relative to the project folder.";

/// A tool: what a client sees of it and how it runs.
pub struct Tool {
    /// The name.
    pub name: &'static str,
    /// The name for people.
    pub title: &'static str,
    /// What it does and when to use it.
    pub description: &'static str,
    /// It changes nothing: no file, no model, no library.
    pub read_only: bool,
    /// It can overwrite a file that was there before.
    pub destructive: bool,
    /// The JSON Schema of its arguments, with examples.
    pub input: fn() -> Value,
    /// The JSON Schema of its structured result.
    pub output: fn() -> Value,
    /// Reads the arguments and runs it.
    pub run: fn(&mut Server<'_>, Value) -> Result<Output, ToolError>,
    /// Reads the arguments and does no more, to check examples.
    pub parse: fn(Value) -> Result<(), serde_json::Error>,
}

fn parses<T: DeserializeOwned>(arguments: Value) -> Result<(), serde_json::Error> {
    serde_json::from_value::<T>(arguments).map(drop)
}

macro_rules! tool {
    ($name:literal, $title:literal, $description:literal, $read_only:literal, $destructive:literal,
     $args:ty, $method:ident, $input:path, $output:path) => {
        Tool {
            name: $name,
            title: $title,
            description: $description,
            read_only: $read_only,
            destructive: $destructive,
            input: $input,
            output: $output,
            run: |server, arguments| server.$method(serde_json::from_value::<$args>(arguments)?),
            parse: parses::<$args>,
        }
    };
}

/// Every tool, in the order of the list the client gets.
pub const TOOLS: [Tool; 18] = [
    tool!(
        "brief_get",
        "Read the brief",
        "The project's brief: game version, targets, naming and texture rules, the style summary of the references (`typical` names the ones to study first) and the library entries, paged.",
        true,
        false,
        brief::BriefGet,
        brief_get,
        brief::brief_get_input,
        brief::brief_get_output
    ),
    tool!(
        "refs_import",
        "Import style references",
        "Remembers a folder of reference models and textures of the project as style references (read-only, with a licence). Importing a known folder reads it again, and drops the references whose file changed or is gone.",
        false,
        true,
        brief::RefsImport,
        refs_import,
        brief::refs_import_input,
        brief::refs_import_output
    ),
    tool!(
        "refs_get",
        "Read a style reference",
        "The outline of a stored reference model (groups, elements, faces, textures, display) with its style numbers, or the size, frames and palette of a reference texture. Elements are paged.",
        true,
        false,
        brief::RefsGet,
        refs_get,
        brief::refs_get_input,
        brief::refs_get_output
    ),
    tool!(
        "project_list",
        "List the model files",
        "The model files of the project folder (.cubik, .cub, .bbmodel and Minecraft model .json), with which are open and unsaved.",
        true,
        false,
        project::ProjectList,
        project_list,
        project::project_list_input,
        project::project_list_output
    ),
    tool!(
        "project_open",
        "Open or create a model",
        "Opens a model file in the server, or with `create` starts a new item or block model with the version and display preset of the brief. Other tools open a model on first use; at most 8 stay open. A new model, one with unsaved edits and one with checkpoints stay open; a saved model that closes to make room loses its undo steps.",
        false,
        false,
        project::ProjectOpen,
        project_open,
        project::project_open_input,
        project::project_open_output
    ),
    tool!(
        "project_save",
        "Save a model",
        "Saves the model as a .cubik file. A file that changed on disk since it was opened is E_FILE_CHANGED unless overwrite is true. Style references cannot be saved.",
        false,
        true,
        project::ProjectSave,
        project_save,
        project::project_save_input,
        project::project_save_output
    ),
    tool!(
        "project_export",
        "Export a model",
        "Writes a Minecraft Java model (JSON with its textures) or a resource pack folder for a game version, under the project folder. A file that is already there is never replaced unless overwrite is true: a second export to the same folder needs overwrite.",
        false,
        true,
        project::ProjectExport,
        project_export,
        project::project_export_input,
        project::project_export_output
    ),
    tool!(
        "model_outline",
        "Outline of a model",
        "The groups, elements (ids, paths, boxes, rotations, faces with texture and UV in texels, v down), textures and display slots of a model, 24 elements a page. Use `select` for one subtree.",
        true,
        false,
        inspect::ModelOutline,
        model_outline,
        inspect::model_outline_input,
        inspect::model_outline_output
    ),
    tool!(
        "model_get",
        "Read objects of a model",
        "The records of the selected elements and groups: box, rotation, pivot, faces, visibility. `fields` keeps only some keys.",
        true,
        false,
        inspect::ModelGet,
        model_get,
        inspect::model_get_input,
        inspect::model_get_output
    ),
    tool!(
        "model_find",
        "Find objects of a model",
        "Ids and paths of the objects that match a name glob, a group, a box they lie inside, a texture, or that have faces without a texture.",
        true,
        false,
        inspect::ModelFind,
        model_find,
        inspect::model_find_input,
        inspect::model_find_output
    ),
    tool!(
        "model_stats",
        "Numbers of a model",
        "Counts, bounds, the style numbers of the brief's summary for this model alone, and whether the model meets the targets of the brief.",
        true,
        false,
        inspect::ModelStats,
        model_stats,
        inspect::model_stats_input,
        inspect::model_stats_output
    ),
    tool!(
        "model_edit",
        "Edit a model",
        "Runs a list of ops (add_cube, group, set, move, delete, duplicate, mirror, align, reparent, set_face, auto_uv, flip_uv, add_texture, remove_texture, set_display, apply_fixes) as one batch and one undo step. A refused op undoes the call. A later op may name by path what an earlier one made.",
        false,
        false,
        edit::ModelEdit,
        model_edit,
        edit::model_edit_input,
        edit::model_edit_output
    ),
    tool!(
        "texture_paint",
        "Paint a texture",
        "Composes pixels (paint_grid from rows of characters, fill_rect, import_png) and replaces the picture of the texture in one step. Returns pictures of the changed regions.",
        false,
        false,
        texture::TexturePaint,
        texture_paint,
        texture::texture_paint_input,
        texture::texture_paint_output
    ),
    tool!(
        "texture_view",
        "View a texture",
        "A texture enlarged, with the UV rectangle of every face that uses it outlined, and the rectangles as data.",
        true,
        false,
        texture::TextureView,
        texture_view,
        texture::texture_view_input,
        texture::texture_view_output
    ),
    tool!(
        "validate",
        "Validate a model",
        "The issues of the validator for the model as it is open, against the target and the rules of the brief: code, severity, objects, message, hint and whether apply_fixes can fix it. self_test proves the rules still fire.",
        true,
        false,
        check::Validate,
        validate,
        check::validate_input,
        check::validate_output
    ),
    tool!(
        "render_view",
        "Render a view",
        "A picture of the model or of a style reference from one camera, saved under renders/. Returns the pixel bounds and the share of the picture the model covers.",
        false,
        false,
        views::RenderView,
        render_view,
        views::render_view_input,
        views::render_view_output
    ),
    tool!(
        "render_sheet",
        "Render a contact sheet",
        "Up to 9 views of the model or of a style reference in one grid picture, in the order of `views`.",
        false,
        false,
        views::RenderSheet,
        render_sheet,
        views::render_sheet_input,
        views::render_sheet_output
    ),
    tool!(
        "history",
        "Undo, redo, checkpoints",
        "Lists the undo steps, undoes or redoes steps, keeps named checkpoints of the model in memory and restores one as a single step.",
        false,
        false,
        history::HistoryCall,
        history,
        history::history_input,
        history::history_output
    ),
];

/// Whether a tool of that name exists.
pub fn exists(name: &str) -> bool {
    TOOLS.iter().any(|tool| tool.name == name)
}

/// The tools as `tools/list` gives them.
pub fn definitions() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "title": tool.title,
                "description": tool.description,
                "inputSchema": (tool.input)(),
                "outputSchema": (tool.output)(),
                "annotations": {
                    "readOnlyHint": tool.read_only,
                    "destructiveHint": tool.destructive,
                    "idempotentHint": tool.read_only,
                    "openWorldHint": false,
                },
            })
        })
        .collect()
}

/// Whether a call changes the project: a file, the library or a model. A dry run changes nothing.
/// (A render writes a picture, but a read-only server still draws it, without the file.)
fn writes(tool: &str, arguments: &Value) -> bool {
    match tool {
        "refs_import" | "project_save" | "project_export" => true,
        "model_edit" | "texture_paint" => arguments.get("dry_run") != Some(&Value::Bool(true)),
        "history" => arguments.get("action").and_then(Value::as_str) != Some("list"),
        "project_open" => arguments
            .get("create")
            .is_some_and(|create| !create.is_null()),
        _ => false,
    }
}

impl Server<'_> {
    /// Runs the tool `name` with `arguments`. A server that was started with `--read-only`
    /// refuses a call that writes. Each call that wrote adds a line to the journal: a render
    /// wrote when its result names the picture file.
    pub fn call(&mut self, name: &str, arguments: Value) -> Result<Output, ToolError> {
        let tool = TOOLS
            .iter()
            .find(|tool| tool.name == name)
            .ok_or_else(|| ToolError::input(format!("there is no tool {name}")))?;
        let writing = writes(name, &arguments);
        if writing && self.options.read_only {
            return Err(ToolError::read_only(
                "the server was started with --read-only",
            ));
        }
        let output = (tool.run)(self, arguments)?;
        let rendered = name.starts_with("render_") && output.value.get("path").is_some();
        if writing || rendered {
            self.journal(name, &output.value);
        }
        Ok(output)
    }
}
