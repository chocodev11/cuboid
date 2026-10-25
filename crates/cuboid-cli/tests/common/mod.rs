//! Helpers of the `brief` and `check` tests: the binary, a small project library and model files
//! made in the test, so that no private file is needed.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;

use cuboid_fmt_cubik::{SaveOptions, save_bytes};
use cuboid_library::{Brief, ImportOptions, Library, Quiet, Scope};
use cuboid_model::image::{Image, RgbaBuf, encode_png};
use cuboid_model::{
    CaptureScope, Categories, Document, Element, ElementType, Label, Selection, Texture,
    TextureEntry, TextureList, TextureSource, capture,
};

pub fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cuboid-cli"))
        .args(args)
        .output()
        .expect("the binary starts")
}

pub fn cli_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cuboid-cli"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("the binary starts")
}

pub fn path_arg(path: &Path) -> &str {
    path.to_str().expect("UTF-8 temporary path")
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn code(output: &Output) -> i32 {
    output.status.code().expect("exited normally")
}

/// The names in a folder, sorted.
pub fn names(dir: &Path) -> Vec<String> {
    let mut list: Vec<String> = fs::read_dir(dir)
        .expect("lists")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    list.sort();
    list
}

/// A PNG of one colour.
pub fn png(side: u32, color: [u8; 4]) -> Vec<u8> {
    let mut buf = RgbaBuf::new(side, side);
    for texel in buf.pixels.chunks_exact_mut(4) {
        texel.copy_from_slice(&color);
    }
    encode_png(&Image::from_rgba(buf)).expect("encodes")
}

/// A project folder with a library of a texture, a lore note and a model JSON, and the given
/// brief. Returns the project folder; the library is in `cuboid-library` below it.
pub fn project(dir: &Path, brief: Brief) -> PathBuf {
    let sources = dir.join("sources");
    fs::create_dir_all(&sources).expect("dir");
    fs::write(sources.join("stone.png"), png(4, [200, 20, 20, 255])).expect("write");
    fs::write(
        sources.join("story.md"),
        "# The Ink Wars\n## Weapons\n- Gun\n",
    )
    .expect("write");
    fs::write(
        sources.join("cube.json"),
        r#"{"elements": [{"from": [0, 0, 0], "to": [16, 16, 16]}]}"#,
    )
    .expect("write");
    let project = dir.join("project");
    let root = project.join(cuboid_library::PROJECT_DIR);
    let mut library = Library::new(root, Scope::Project);
    let report = library.import(&[sources], &ImportOptions::default(), &Quiet);
    assert_eq!(report.added.len(), 3, "{report:?}");
    library.set_brief(brief);
    library.save().expect("saves");
    project
}

/// A `.cubik` file of a document that `build` fills.
pub fn cubik(dir: &Path, name: &str, build: impl FnOnce(&mut Document)) -> PathBuf {
    let mut doc = Document::new(TextureList::default());
    build(&mut doc);
    let snapshot = capture(
        &doc,
        &Selection::default(),
        CaptureScope {
            categories: Categories::ALL,
            selection_only: false,
        },
    );
    let options = SaveOptions {
        use_retention: false,
        ..SaveOptions::default()
    };
    let bytes = save_bytes(&snapshot, None, &[], &options).expect("saves");
    let path = dir.join(name);
    fs::write(&path, bytes).expect("writes");
    path
}

/// Adds an opaque one-colour texture with a Minecraft path.
pub fn add_texture(doc: &mut Document, label: &str, mc_path: &str, side: u32, color: [u8; 3]) {
    let mut buf = RgbaBuf::new(side, side);
    for texel in buf.pixels.chunks_exact_mut(4) {
        texel.copy_from_slice(&[color[0], color[1], color[2], 255]);
    }
    let texture = Texture {
        source: TextureSource {
            mc_path: mc_path.to_owned(),
            ..TextureSource::default()
        },
        ..Texture::new(Arc::new(Image::from_rgba(buf)))
    };
    doc.textures.entries.push(TextureEntry {
        label: Label::from(label),
        texture,
    });
}

/// Adds a cube that wears the texture, named `name`, with its corner `x` units from the origin.
pub fn add_cube(doc: &mut Document, name: &str, texture: &str, x: f32) {
    let label = Label::from(texture);
    let element = Element::new(
        ElementType::Cube,
        &doc.blueprint,
        doc.elements.next_display_id(),
        [1.0; 4],
        &label,
    );
    let id = doc.elements.push(element);
    if let Some(element) = doc.elements.edit(id, &mut doc.journal) {
        element.base.label = Label::from(name);
        element.base.p.x = x;
        element.base.o.x = x;
    }
}
