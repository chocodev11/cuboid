//! Legacy `.cub` tests: the key stream, the text decoding, the dialect quirks and the import
//! pipeline (native 9, 10.1).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cuboid_model::image::{Image, RgbaBuf, encode_png};
use cuboid_model::{Categories, ElementFlags, FaceFlags, LoadedDoc, ModelKind};
use glam::Vec3;

use super::hex;
use crate::cub::{self, CubError, Notice, Refusal, TextureFiles};

struct Files(HashMap<PathBuf, Vec<u8>>);

impl TextureFiles for Files {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, Refusal> {
        Ok(self.0.get(path).cloned())
    }
}

fn no_files() -> Files {
    Files(HashMap::new())
}

/// The file bytes of a plain text: the obfuscation is its own inverse.
fn obfuscate(text: &str) -> Vec<u8> {
    cub::deobfuscate(text.as_bytes()).into_bytes()
}

fn import(text: &str, files: &Files) -> Result<cub::CubImport, CubError> {
    cub::import(&obfuscate(text), Path::new("dir/My Model.cub"), files)
}

fn props(doc: &LoadedDoc) -> &cuboid_model::ModelProps {
    doc.sections.props.as_ref().unwrap()
}

// --- Text and key stream (native 9.2, 9.3) ------------------------------------------------------

#[test]
fn the_test_vector_of_the_key_stream() {
    let bytes = hex("56 24 68 76 7B 62 24 7F 0E 75 68 74 3E 6D 7D 31 53 5C 63 35 34 20 7B");
    assert_eq!(cub::deobfuscate(&bytes), r#"{"name":"Box","type":1}"#);
    assert_eq!(obfuscate(r#"{"name":"Box","type":1}"#), bytes);
}

#[test]
fn the_combined_key_repeats_every_296_units() {
    let first: Vec<u8> = (0..16)
        .map(|i: usize| b"oBeeware"[i % 8] ^ b"BDcraft Cubik - Encryption System Key"[i % 37])
        .collect();
    assert_eq!(
        first,
        hex("2D 06 06 17 16 07 06 45 2C 37 07 0C 1C 41 5F 45")
    );
    let zeros = vec![0u8; 600];
    let stream = cub::deobfuscate(&zeros).into_bytes();
    assert_eq!(stream.get(..296), stream.get(296..592));
}

#[test]
fn a_utf8_byte_order_mark_is_skipped() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend(obfuscate(r#"{"name":"Box","type":1}"#));
    assert_eq!(cub::deobfuscate(&bytes), r#"{"name":"Box","type":1}"#);
}

#[test]
fn a_utf16_file_is_decoded_before_the_key_applies() {
    let plain = r#"{"name":"Box"}"#;
    let obfuscated = String::from_utf8(obfuscate(plain)).unwrap();
    let mut le = vec![0xFF, 0xFE];
    le.extend(obfuscated.encode_utf16().flat_map(u16::to_le_bytes));
    let mut be = vec![0xFE, 0xFF];
    be.extend(obfuscated.encode_utf16().flat_map(u16::to_be_bytes));
    assert_eq!(cub::deobfuscate(&le), plain);
    assert_eq!(cub::deobfuscate(&be), plain);
}

#[test]
fn non_ascii_and_astral_characters_survive() {
    let plain = "{\"name\":\"caf\u{e9} \u{1d11e} \u{4e2d}\",\"elements\":[]}";
    let imported = import(plain, &no_files()).unwrap();
    assert_eq!(
        props(&imported.doc).general.name,
        "caf\u{e9} \u{1d11e} \u{4e2d}"
    );
}

#[test]
fn invalid_utf8_becomes_the_replacement_character_before_the_key_applies() {
    // The second key byte is 0x06.
    let text = cub::deobfuscate(&[b'a', 0x80, b'b']);
    let units: Vec<u32> = text.chars().map(u32::from).collect();
    assert_eq!(
        units,
        [
            u32::from(b'a') ^ 0x2D,
            0xFFFD ^ 0x06,
            u32::from(b'b') ^ 0x06
        ]
    );
}

// --- Dialect quirks (native 9.4) ----------------------------------------------------------------

#[test]
fn escapes_are_kept_literally_except_unicode_escapes() {
    let imported = import(r#"{"name":"a\"b\nA","elements":[]}"#, &no_files()).unwrap();
    assert_eq!(props(&imported.doc).general.name, "a\\\"b\\nA");
}

#[test]
fn numbers_run_to_the_next_delimiter() {
    let text = r#"{"elements":[],"gridstep": 0.25 ,"gridmousestep":1e1,"gridkeystep":"x"}"#;
    let imported = import(text, &no_files()).unwrap();
    let view = imported.doc.sections.view.as_ref().unwrap();
    assert_eq!(view.display_step, 0.25);
    assert_eq!(view.mouse_step, 10.0);
    // A value of another type reads as 0.
    assert_eq!(view.key_step, 0.0);
}

#[test]
fn syntax_errors_and_missing_model_keys_are_distinct() {
    for bad in ["", "[]", "{\"a\":", "{\"a\":1,,}", "not json"] {
        assert_eq!(
            import(bad, &no_files()).unwrap_err(),
            CubError::Syntax,
            "{bad}"
        );
    }
    assert_eq!(
        import(r#"{"name":"x"}"#, &no_files()).unwrap_err(),
        CubError::NotAModel
    );
}

// --- The import (native 9.5) --------------------------------------------------------------------

fn png() -> Vec<u8> {
    let mut buf = RgbaBuf::new(2, 2);
    buf.pixels[0] = 200;
    buf.pixels[3] = 255;
    encode_png(&Image::from_rgba(buf)).unwrap()
}

const MODEL: &str = r##"{
  "name": "Boxy",
  "type": 2,
  "gridstep": 0.5,
  "textures_path": {"t": "textures/t.png", "u": "missing.png"},
  "elements": [{
    "from": [1, 2, 3], "to": [5, 2, 7],
    "rotation": {"origin": [8, 8, 8], "axis": "y", "angle": 45, "rescale": true},
    "shade": false,
    "label": "first",
    "color": [1, 0, 0, 1],
    "faces": {
      "north": {"uv": [0, 0, 16, 8], "texture": "#t", "cullface": "north", "rotation": 90, "tintindex": 2},
      "up": {"texture": "#fresh"}
    }
  }],
  "display": {"gui": {"rotation": [30, 225, 0], "scale": [0.5, 0.5, 0.5]}},
  "overrides": [{"predicate": {"pull": 0.5}, "model": "item/x"}, {"model": "no predicate"}],
  "voxel": {"resolution": 2, "voxels": [[0, 0, 0, -16777216], [99, 0, 0, 1], [1, 1, 1, 4294967295]]}
}"##;

#[test]
fn the_import_builds_the_properties_the_element_and_the_textures() {
    let mut files = no_files();
    files.0.insert(PathBuf::from("dir/textures/t.png"), png());
    let imported = import(MODEL, &files).unwrap();
    let doc = &imported.doc;
    let sections = &doc.sections;
    assert!(sections.categories.contains(
        Categories::MODEL_PROPERTIES
            | Categories::VIEW_PROPERTIES
            | Categories::ELEMENTS
            | Categories::TEXTURES
            | Categories::VOXELS
            | Categories::MODEL_DISPLAY
    ));
    assert!(imported.notices.is_empty());

    let props = props(doc);
    assert_eq!(props.general.name, "Boxy");
    assert_eq!(props.minecraft.kind, ModelKind::Item);
    assert!(props.minecraft.include_display && props.minecraft.keep_overrides);
    let view = sections.view.as_ref().unwrap();
    assert_eq!(
        (view.display_step, view.mouse_step, view.key_step),
        (0.5, 1.0, 1.0)
    );

    let elements = sections.elements.as_ref().unwrap();
    let el = &elements.list.items[0];
    assert_eq!(el.base.p, Vec3::new(1.0, 2.0, -3.0));
    // A zero size becomes 0.001.
    assert_eq!(el.base.s, Vec3::new(4.0, 0.001, 4.0));
    assert_eq!(el.base.o, Vec3::new(8.0, 8.0, -8.0));
    assert!((el.base.e.y - 315.0).abs() < 1e-3, "{:?}", el.base.e);
    assert_eq!(&*el.base.label, "first");
    assert!(el.flags.contains(ElementFlags::RESCALE));
    assert!(!el.flags.contains(ElementFlags::SHADE));
    let north = &el.faces[2];
    assert!(north.active);
    assert_eq!(north.cull, FaceFlags::NORTH);
    assert_eq!(north.tint, 2);
    assert_eq!(north.uv.rot(), 90);
    assert_eq!(north.uv.tl_raw(), glam::Vec2::new(0.0, 0.5));
    assert_eq!(north.uv.br_raw(), glam::Vec2::new(1.0, 1.0));
    assert!(!el.faces[0].active, "a face the file lacks is disabled");

    // Textures: the found file keeps its provenance, the missing one gets a picture of our own,
    // a label that was never declared gets a default entry.
    let list = &sections.textures.as_ref().unwrap().entries;
    let labels: Vec<&str> = list.iter().map(|e| &*e.label).collect();
    assert_eq!(labels, ["t", "u", "fresh"]);
    assert_eq!(
        list[0].texture.source.path.as_deref(),
        Some("dir/textures/t.png")
    );
    assert_eq!(list[0].texture.source.mc_path, "t");
    assert_eq!(
        list[0]
            .texture
            .source
            .signature
            .as_ref()
            .map(|s| s.chars().count()),
        Some(16)
    );
    assert_eq!(&*list[1].texture.name, "e_error");
    assert_eq!(list[1].texture.source.path, None);
    assert_eq!(&*list[2].texture.name, "e_texture");

    let display = sections.display.as_ref().unwrap();
    assert!(display.slots[4].enabled && !display.slots[0].enabled);
    assert_eq!(display.slots[4].rotation, Vec3::new(30.0, 225.0, 0.0));

    let overrides = doc.overrides.as_ref().unwrap();
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0].pull, Some(0.5));

    // The cell outside the grid (3R = 6) is ignored; a colour above 2^31 keeps its bits.
    let replay = doc.voxel_replay.as_ref().unwrap();
    assert_eq!(replay.res, 2);
    assert_eq!(replay.cells.len(), 2);
    assert_eq!(replay.cells[1].1, u32::MAX);
}

#[test]
fn a_missing_name_gives_an_empty_name_and_a_missing_label_an_empty_label() {
    let text = r#"{"textures_path":{},"elements":[{"from":[0,0,0],"to":[1,1,1]}]}"#;
    let imported = import(text, &no_files()).unwrap();
    assert_eq!(props(&imported.doc).general.name, "");
    let elements = imported.doc.sections.elements.as_ref().unwrap();
    assert_eq!(&*elements.list.items[0].base.label, "");
}

#[test]
fn a_file_without_textures_reports_it() {
    let imported = import(r#"{"elements":[]}"#, &no_files()).unwrap();
    assert_eq!(imported.notices, [Notice::NoTexture]);
    assert!(imported.doc.sections.textures.is_none());
}

#[test]
fn the_parent_decides_which_dialog_the_caller_shows() {
    let notice = |text: &str| import(text, &no_files()).map(|i| i.notices);
    assert_eq!(
        notice(r#"{"parent":"item/generated"}"#).unwrap_err(),
        CubError::Generated
    );
    assert!(
        notice(r#"{"parent":"block/block","elements":[],"textures":{}}"#)
            .unwrap()
            .contains(&Notice::Inherited {
                parent: "block/block".to_owned(),
                ask: false
            })
    );
    assert!(
        notice(r#"{"parent":"block/cube_all","elements":[]}"#)
            .unwrap()
            .contains(&Notice::Inherited {
                parent: "block/cube_all".to_owned(),
                ask: true
            })
    );
    assert!(
        notice(r#"{"parent":"block/block"}"#)
            .unwrap()
            .contains(&Notice::Inherited {
                parent: "block/block".to_owned(),
                ask: true
            })
    );
}

#[test]
fn errors_after_the_parse_abort_the_import() {
    let face_without_texture = r#"{"elements":[{"from":[0,0,0],"to":[1,1,1],"faces":{"up":{}}}]}"#;
    assert_eq!(
        import(face_without_texture, &no_files()).unwrap_err(),
        CubError::FaceWithoutTexture
    );
    assert_eq!(
        import(r#"{"elements":[],"voxel":{"resolution":4}}"#, &no_files()).unwrap_err(),
        CubError::MissingVoxels
    );
    assert_eq!(
        import(r#"{"elements":[],"name":5}"#, &no_files()).unwrap_err(),
        CubError::NotAString("name")
    );
}
