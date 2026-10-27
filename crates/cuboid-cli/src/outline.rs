//! `outline`: the groups, elements, faces, textures and display settings of any model file,
//! including the stored copies of a library, as an agent reads them. Reads only; nothing is
//! written.

use std::fmt::Write as _;
use std::path::Path;

use cuboid_library::doc::load;
use cuboid_library::{OUTLINE_PAGE, Outline, outline};

use crate::args::Args;
use crate::{Fail, doc, dump};

fn numbers(values: &[f32]) -> String {
    let parts: Vec<String> = values.iter().map(ToString::to_string).collect();
    parts.join(" ")
}

/// One line per group, element and texture, the faces of an element on a line below it.
fn text(outline: &Outline) -> String {
    let mut out = String::new();
    for group in &outline.groups {
        let _ = writeln!(
            out,
            "group {} pivot {} [{}]",
            group.path,
            numbers(&group.pivot),
            group.children.join(", ")
        );
    }
    for element in &outline.elements {
        let _ = write!(
            out,
            "#{} {} from {} to {}",
            element.number,
            element.path,
            numbers(&element.from),
            numbers(&element.to)
        );
        if let (Some(rotation), Some(pivot)) = (&element.rotation, &element.pivot) {
            let _ = write!(
                out,
                " rotation {} around {}",
                numbers(rotation),
                numbers(pivot)
            );
        }
        let faces: Vec<String> = element
            .faces
            .0
            .iter()
            .map(|(name, face)| {
                let turn = if face.uv_rotation == 0 {
                    String::new()
                } else {
                    format!(" r{}", face.uv_rotation)
                };
                format!("{name} {} {}{turn}", face.texture, numbers(&face.uv))
            })
            .collect();
        out.push('\n');
        if !faces.is_empty() {
            let _ = writeln!(out, "    {}", faces.join("; "));
        }
    }
    for texture in &outline.textures {
        let [w, h] = texture.size;
        let frames = texture
            .frames
            .map_or_else(String::new, |count| format!(", {count} frames"));
        let _ = writeln!(
            out,
            "texture {} {w}x{h}{frames} {}",
            texture.name, texture.path
        );
    }
    for slot in &outline.display {
        let _ = writeln!(
            out,
            "display {} rotation {} translation {} scale {}",
            slot.slot,
            numbers(&slot.rotation),
            numbers(&slot.translation),
            numbers(&slot.scale)
        );
    }
    let shown = outline.elements.len();
    let _ = write!(out, "{shown} of {} elements", outline.total);
    if let Some(next) = outline.next {
        let _ = write!(out, " (more from {next}; --limit 0 lists all)");
    }
    out.push('\n');
    out
}

/// Prints the outline of the model, at most `--limit` elements (default one page of 24, 0 for
/// all).
pub fn run(args: &[String]) -> Result<(), Fail> {
    let args = Args::split(args, &["--json"], &["--limit"])?;
    let model = args
        .word("model")?
        .ok_or_else(|| Fail::Usage("no model given".to_owned()))?;
    let limit = match args.option("--limit") {
        None => OUTLINE_PAGE,
        Some(text) => match text.parse::<usize>() {
            Ok(0) => usize::MAX,
            Ok(count) => count,
            Err(_) => return Err(Fail::Usage(format!("`{text}` is not an element count"))),
        },
    };
    let opened = load(Path::new(model)).map_err(|err| doc::fail(err, "read"))?;
    for note in &opened.notes {
        eprintln!("cuboid-cli: note: {note}");
    }
    let outline = outline(&opened.doc).page(0, limit);
    let text = if args.flag("--json") {
        serde_json::to_string(&outline).map_err(|err| Fail::Error(err.to_string()))?
    } else {
        text(&outline)
    };
    dump::print(&text, 0).map_err(|err| Fail::Error(format!("cannot write the output: {err}")))
}
