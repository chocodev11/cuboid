//! Helpers of the tests: a fake renderer, models made in the test, a driver for the protocol and
//! a checker for the subset of JSON Schema that the tools use.

#![allow(dead_code)]

use std::fs;
use std::io::Cursor;
use std::path::Path;

use cuboid_image::{Image, RgbaBuf, encode_png};
use cuboid_mcp::{Camera, Options, Picture, RenderError, Renderer, Server, serve};
use serde_json::{Value, json};
use tempfile::TempDir;

/// A renderer that draws a square whose colour tells the camera, and remembers what it was asked.
#[derive(Default)]
pub struct Fake {
    pub calls: Vec<(Camera, u32)>,
    pub fail: Option<RenderError>,
}

impl Renderer for Fake {
    fn render(
        &mut self,
        doc: &cuboid_model::Document,
        camera: Camera,
        size: u32,
    ) -> Result<Picture, RenderError> {
        self.calls.push((camera, size));
        if let Some(error) = &self.fail {
            return Err(error.clone());
        }
        let mut picture = Picture::new(size, size, [0; 4]);
        let at = Camera::ALL.iter().position(|c| *c == camera).unwrap_or(0) as u8;
        let side = if doc.elements.is_empty() { 0 } else { size / 2 };
        for y in size / 4..size / 4 + side {
            for x in size / 4..size / 4 + side {
                picture.set(x, y, [40 + 25 * at, 100, 200, 255]);
            }
        }
        Ok(picture)
    }
}

/// A renderer whose drawing panics, for the panic boundary of the protocol layer.
pub struct Panics;

impl Renderer for Panics {
    fn render(
        &mut self,
        _doc: &cuboid_model::Document,
        _camera: Camera,
        _size: u32,
    ) -> Result<Picture, RenderError> {
        panic!("the renderer broke");
    }
}

/// A 16 x 16 picture of one colour as the data URL a bbmodel holds.
pub fn texture_url() -> String {
    let mut buf = RgbaBuf::new(16, 16);
    for texel in buf.pixels.chunks_exact_mut(4) {
        texel.copy_from_slice(&[200, 20, 20, 255]);
    }
    let png = encode_png(&Image::from_rgba(buf)).expect("encodes");
    cuboid_fmt_bbmodel::data_url::png_url(&png)
}

/// A lamp: a base, an arm in a group turned by 22.5 degrees, and a shade, with a 16 x 16 texture
/// named `lamp`.
pub fn lamp() -> String {
    let url = texture_url();
    format!(
        r#"{{"meta": {{"format_version": "4.10", "model_format": "java_block", "box_uv": false}},
        "name": "lamp", "resolution": {{"width": 16, "height": 16}},
        "elements": [
            {{"name": "base", "from": [4, 0, 4], "to": [12, 2, 12], "origin": [8, 0, 8],
              "uuid": "e1", "faces": {{
                "north": {{"uv": [0, 0, 8, 2], "texture": 0}},
                "up": {{"uv": [0, 0, 8, 8], "texture": 0, "rotation": 90}}}}}},
            {{"name": "arm_left", "from": [7, 2, 7], "to": [9, 10, 9], "origin": [8, 2, 8],
              "rotation": [0, 0, 22.5], "uuid": "e2",
              "faces": {{"east": {{"uv": [0, 0, 2, 8], "texture": 0}}}}}},
            {{"name": "shade", "from": [5, 10, 5], "to": [11, 11, 11], "origin": [8, 10, 8],
              "uuid": "e3", "faces": {{"down": {{"uv": [2, 2, 8, 8], "texture": 0}}}}}}],
        "outliner": ["e1", {{"name": "lamp_arm", "origin": [8, 2, 8], "uuid": "g1",
            "children": ["e2", "e3"]}}],
        "textures": [{{"name": "lamp", "id": "0", "source": "{url}"}}]}}"#
    )
}

/// A model with one wall whose north face shows the top left quarter of a texture of one colour,
/// `side` x `side` texels.
pub fn big_model(side: u32) -> String {
    let mut buf = RgbaBuf::new(side, side);
    for texel in buf.pixels.chunks_exact_mut(4) {
        texel.copy_from_slice(&[20, 120, 200, 255]);
    }
    let png = encode_png(&Image::from_rgba(buf)).expect("encodes");
    let url = cuboid_fmt_bbmodel::data_url::png_url(&png);
    let half = side / 2;
    format!(
        r#"{{"meta": {{"format_version": "4.10", "model_format": "java_block", "box_uv": false}},
        "name": "big", "resolution": {{"width": {side}, "height": {side}}},
        "elements": [{{"name": "wall", "from": [0, 0, 0], "to": [16, 16, 1], "origin": [8, 0, 0],
            "uuid": "e1", "faces": {{"north": {{"uv": [0, 0, {half}, {half}], "texture": 0}}}}}}],
        "outliner": ["e1"],
        "textures": [{{"name": "big", "id": "0", "source": "{url}"}}]}}"#
    )
}

/// A project folder with the lamp in it.
pub fn project() -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::write(dir.path().join("lamp.bbmodel"), lamp()).expect("writes the lamp");
    dir
}

/// The tools as a client calls them, on a server over a folder.
pub struct Client<'r> {
    pub server: Server<'r>,
}

impl<'r> Client<'r> {
    pub fn new(root: &Path, read_only: bool, renderer: &'r mut Fake) -> Client<'r> {
        let server = Server::new(root, Options { read_only }, renderer).expect("a server");
        Client { server }
    }

    /// The structured result of a call that works.
    pub fn ok(&mut self, tool: &str, arguments: Value) -> Value {
        match self.server.call(tool, arguments.clone()) {
            Ok(output) => output.value,
            Err(error) => panic!("{tool} {arguments}: {error:?}"),
        }
    }

    /// The pictures of a call that works.
    pub fn pictures(&mut self, tool: &str, arguments: Value) -> (Value, Vec<Vec<u8>>) {
        match self.server.call(tool, arguments.clone()) {
            Ok(output) => (output.value, output.images),
            Err(error) => panic!("{tool} {arguments}: {error:?}"),
        }
    }

    /// The code of a call that fails.
    pub fn code(&mut self, tool: &str, arguments: Value) -> String {
        match self.server.call(tool, arguments.clone()) {
            Ok(output) => panic!("{tool} {arguments} worked: {}", output.value),
            Err(error) => error.code.to_owned(),
        }
    }

    /// Runs `ops` on the model and returns the result.
    pub fn edit(&mut self, model: &str, ops: Value) -> Value {
        self.ok("model_edit", json!({"model": model, "ops": ops}))
    }

    /// The code of an edit that fails.
    pub fn edit_code(&mut self, model: &str, ops: Value) -> String {
        self.code("model_edit", json!({"model": model, "ops": ops}))
    }

    /// The record of one object.
    pub fn get(&mut self, model: &str, select: &str) -> Value {
        let found = self.ok("model_get", json!({"model": model, "select": select}));
        found["objects"][0].clone()
    }

    /// The revision of a model.
    pub fn revision(&mut self, model: &str) -> String {
        self.ok("model_stats", json!({"model": model}))["revision"]
            .as_str()
            .expect("a revision")
            .to_owned()
    }

    /// The labels of the undo steps of a model, newest first.
    pub fn undo_labels(&mut self, model: &str) -> Vec<String> {
        let list = self.ok("history", json!({"model": model, "action": "list"}));
        list["undo"]
            .as_array()
            .expect("labels")
            .iter()
            .filter_map(|label| label.as_str().map(ToOwned::to_owned))
            .collect()
    }
}

/// Runs the server over the lines and returns what it wrote, a line each.
pub fn transcript(root: &Path, lines: &[&str]) -> Vec<String> {
    let mut input = lines.join("\n");
    input.push('\n');
    run(root, input.as_bytes(), Options::default())
}

pub fn run(root: &Path, input: &[u8], options: Options) -> Vec<String> {
    let mut output = Vec::new();
    let mut fake = Fake::default();
    serve(root, options, &mut fake, Cursor::new(input), &mut output).expect("serves");
    String::from_utf8(output)
        .expect("utf-8")
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}

/// A request line of the modern era.
pub fn modern(id: u64, method: &str, params: Value) -> String {
    let mut params = params;
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "1"},
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

/// Checks a value against the subset of JSON Schema that the tools use: `type`, `properties`,
/// `required`, `additionalProperties`, `items`, `enum`, `const`, `oneOf`, `anyOf`, `minimum`,
/// `maximum`, `minItems`, `maxItems` and local `$ref`.
pub fn conforms(schema: &Value, value: &Value) -> Result<(), String> {
    check(schema, value, schema, "$")
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn is_type(value: &Value, name: &str) -> bool {
    match name {
        "integer" => value
            .as_f64()
            .is_some_and(|n| value.is_number() && n.fract() == 0.0),
        "number" => value.is_number(),
        other => kind_of(value) == other,
    }
}

fn check(schema: &Value, value: &Value, root: &Value, at: &str) -> Result<(), String> {
    let Some(schema) = schema.as_object() else {
        return Ok(());
    };
    if let Some(Value::String(reference)) = schema.get("$ref") {
        let target = reference
            .strip_prefix('#')
            .and_then(|pointer| root.pointer(pointer))
            .ok_or_else(|| format!("{at}: unknown reference {reference}"))?;
        return check(target, value, root, at);
    }
    match schema.get("type") {
        Some(Value::String(name)) if !is_type(value, name) => {
            return Err(format!("{at}: {} is not {name}", kind_of(value)));
        }
        Some(Value::Array(names))
            if !names
                .iter()
                .any(|n| n.as_str().is_some_and(|n| is_type(value, n))) =>
        {
            return Err(format!("{at}: {} is none of {names:?}", kind_of(value)));
        }
        _ => {}
    }
    if let Some(Value::Array(allowed)) = schema.get("enum")
        && !allowed.contains(value)
    {
        return Err(format!("{at}: {value} is not one of {allowed:?}"));
    }
    if let Some(expected) = schema.get("const")
        && expected != value
    {
        return Err(format!("{at}: {value} is not {expected}"));
    }
    if let (Some(low), Some(number)) = (
        schema.get("minimum").and_then(Value::as_f64),
        value.as_f64(),
    ) && number < low
    {
        return Err(format!("{at}: {number} is below {low}"));
    }
    if let (Some(high), Some(number)) = (
        schema.get("maximum").and_then(Value::as_f64),
        value.as_f64(),
    ) && number > high
    {
        return Err(format!("{at}: {number} is above {high}"));
    }
    if let Some(Value::Array(branches)) = schema.get("oneOf") {
        let fits = branches
            .iter()
            .filter(|b| check(b, value, root, at).is_ok())
            .count();
        if fits != 1 {
            return Err(format!("{at}: {fits} of the oneOf branches fit"));
        }
    }
    if let Some(Value::Array(branches)) = schema.get("anyOf")
        && !branches.iter().any(|b| check(b, value, root, at).is_ok())
    {
        return Err(format!("{at}: none of the anyOf branches fit"));
    }
    if let Value::Object(object) = value {
        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(Value::Array(required)) = schema.get("required") {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    return Err(format!("{at}: {key} is missing"));
                }
            }
        }
        for (key, member) in object {
            let place = format!("{at}.{key}");
            match (
                properties.and_then(|p| p.get(key)),
                schema.get("additionalProperties"),
            ) {
                (Some(known), _) => check(known, member, root, &place)?,
                (None, Some(Value::Bool(false))) => return Err(format!("{place}: not allowed")),
                (None, Some(extra @ Value::Object(_))) => check(extra, member, root, &place)?,
                _ => {}
            }
        }
    }
    if let Value::Array(items) = value {
        let count = items.len() as u64;
        if schema
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|min| count < min)
        {
            return Err(format!("{at}: fewer items than minItems"));
        }
        if schema
            .get("maxItems")
            .and_then(Value::as_u64)
            .is_some_and(|max| count > max)
        {
            return Err(format!("{at}: more items than maxItems"));
        }
        if let Some(item) = schema.get("items") {
            for (nth, member) in items.iter().enumerate() {
                check(item, member, root, &format!("{at}[{nth}]"))?;
            }
        }
    }
    Ok(())
}
