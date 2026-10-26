//! The layout of the resource pack per version, as a folder and as a zip.

mod common;

use std::io::Read;
use std::sync::Arc;

use common::{Tree, import_at, open};
use cuboid_fmt_minecraft::{
    McError, McVersion, Notice, PackFile, PackFormat, PackOptions, export_pack, latest_version,
};
use cuboid_model::{Document, ModelKind, TextureEntry};
use cuboid_validate::{PACK_FORMAT, PACK_ITEMDEF};
use serde_json::Value;

const PATH: &str = "pack/assets/minecraft/models/block/m.json";

fn v(text: &str) -> McVersion {
    McVersion::parse(text).expect("version")
}

fn model() -> Document {
    let model = r##"{ "textures": { "all": "block/stone" },
        "elements": [ { "from": [0, 0, 0], "to": [16, 16, 16],
            "faces": { "up": { "uv": [0, 0, 16, 16], "texture": "#all" } } } ] }"##;
    let tree = Tree::default().with(
        "pack/assets/minecraft/textures/block/stone.png",
        common::png(16, 16, [1, 2, 3, 255]),
    );
    open(import_at(model, PATH, &tree))
}

fn paths(files: &[PackFile]) -> Vec<&str> {
    files.iter().map(|f| f.path.as_str()).collect()
}

fn text<'a>(files: &'a [PackFile], path: &str) -> &'a str {
    let file = files.iter().find(|f| f.path == path).expect(path);
    std::str::from_utf8(&file.bytes).expect("text")
}

fn json(files: &[PackFile], path: &str) -> Value {
    serde_json::from_str(text(files, path)).expect(path)
}

fn only(codes: &[&str], pack: &cuboid_fmt_minecraft::PackExport) -> Vec<String> {
    pack.notices
        .iter()
        .filter_map(|n| match n {
            Notice::Validation(issue) if codes.contains(&issue.code) => Some(issue.code.to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_pack_of_1_12_has_the_reference_layout() {
    let pack = export_pack(&model(), &PackOptions::new(v("1.12.2"))).expect("fits");
    assert_eq!(pack.format, PackFormat { major: 3, minor: 0 });
    assert_eq!(
        paths(&pack.files),
        [
            "pack.mcmeta",
            "assets/minecraft/blockstates/glass.json",
            "assets/minecraft/blockstates/white_stained_glass.json",
            "assets/minecraft/models/block/cubik_model.json",
            "assets/minecraft/models/item/glass.json",
            "assets/minecraft/models/item/white_stained_glass.json",
            "assets/minecraft/textures/block/stone.png",
        ]
    );
    // The model's texture path has the folder names of 1.13: the validator says so.
    assert!(
        pack.notices
            .iter()
            .any(|n| matches!(n, Notice::Validation(i) if i.code == cuboid_validate::TEX_LAYOUT)),
        "{:?}",
        pack.notices
    );
    // A texture without a path gets the folder of the version.
    let mut doc = model();
    doc.textures.entries.push(TextureEntry {
        label: "glow".into(),
        texture: cuboid_model::Texture::new(common::picture(2, 2, [9, 9, 9, 255])),
        animation: None,
    });
    let pack = export_pack(&doc, &PackOptions::new(v("1.12.2"))).expect("fits");
    assert!(paths(&pack.files).contains(&"assets/minecraft/textures/blocks/cubik_model/glow.png"));
    assert_eq!(json(&pack.files, "pack.mcmeta")["pack"]["pack_format"], 3);
    let state = json(&pack.files, "assets/minecraft/blockstates/glass.json");
    assert_eq!(state["variants"]["normal"]["model"], "cubik_model");
    let item = json(&pack.files, "assets/minecraft/models/item/glass.json");
    assert_eq!(item["parent"], "block/cubik_model");
}

#[test]
fn the_flattened_pack_of_1_21_3_uses_the_empty_variant() {
    let pack = export_pack(&model(), &PackOptions::new(v("1.21.3"))).expect("fits");
    assert_eq!(
        pack.format,
        PackFormat {
            major: 42,
            minor: 0
        }
    );
    let state = json(&pack.files, "assets/minecraft/blockstates/glass.json");
    assert_eq!(
        state["variants"][""]["model"],
        "minecraft:block/cubik_model"
    );
    assert_eq!(
        json(&pack.files, "assets/minecraft/models/item/glass.json")["parent"],
        "minecraft:block/cubik_model"
    );
    assert!(paths(&pack.files).contains(&"assets/minecraft/textures/block/stone.png"));
    assert_eq!(json(&pack.files, "pack.mcmeta")["pack"]["pack_format"], 42);
}

#[test]
fn from_1_21_4_items_have_a_definition() {
    let mut doc = model();
    Arc::make_mut(&mut doc.props).minecraft.kind = ModelKind::Item;
    let mut options = PackOptions::new(v("1.21.4"));
    options.namespace = "cuboid".to_owned();
    options.id = "gem".to_owned();
    let pack = export_pack(&doc, &options).expect("fits");
    assert_eq!(pack.format.major, 46);
    assert_eq!(
        paths(&pack.files),
        [
            "pack.mcmeta",
            "assets/cuboid/models/item/gem.json",
            "assets/cuboid/items/gem.json",
            "assets/minecraft/textures/block/stone.png",
        ]
    );
    let definition = json(&pack.files, "assets/cuboid/items/gem.json");
    assert_eq!(definition["model"]["type"], "minecraft:model");
    assert_eq!(definition["model"]["model"], "cuboid:item/gem");
    assert!(only(&[PACK_ITEMDEF], &pack).is_empty());

    // Replacing an item adds its definition in the vanilla namespace.
    options.replace = Some(vec!["stick".to_owned()]);
    let pack = export_pack(&doc, &options).expect("fits");
    assert_eq!(
        json(&pack.files, "assets/minecraft/items/stick.json")["model"]["model"],
        "cuboid:item/gem"
    );
    // Before 1.21.4 the replaced item gets a model with the parent.
    let old = export_pack(
        &doc,
        &PackOptions {
            replace: Some(vec!["stick".to_owned()]),
            ..PackOptions::new(v("1.21.3"))
        },
    )
    .expect("fits");
    assert!(!paths(&old.files).iter().any(|p| p.contains("/items/")));
    assert_eq!(
        json(&old.files, "assets/minecraft/models/item/stick.json")["parent"],
        "minecraft:item/cubik_model"
    );
}

#[test]
fn formats_above_64_use_min_and_max_format() {
    let doc = model();
    let at = |version: &str| {
        let pack = export_pack(&doc, &PackOptions::new(v(version))).expect("fits");
        (json(&pack.files, "pack.mcmeta")["pack"].clone(), pack)
    };
    let (meta, pack) = at("1.21.7");
    assert_eq!(meta["pack_format"], 64);
    assert!(meta.get("min_format").is_none());
    assert!(only(&[PACK_FORMAT], &pack).is_empty());

    let (meta, _) = at("1.21.10");
    assert!(meta.get("pack_format").is_none());
    assert_eq!(
        (&meta["min_format"], &meta["max_format"]),
        (&Value::from(69), &Value::from(69))
    );

    // A format with a minor part is a pair.
    let (meta, pack) = at("26.3");
    assert_eq!(meta["min_format"], serde_json::json!([97, 1]));
    assert_eq!(meta["max_format"], serde_json::json!([97, 1]));
    assert_eq!(
        pack.format,
        PackFormat {
            major: 97,
            minor: 1
        }
    );
    assert!(
        only(
            &[
                PACK_FORMAT,
                cuboid_validate::PACK_FORMAT_KEYS,
                cuboid_validate::PACK_MCMETA
            ],
            &pack
        )
        .is_empty()
    );
}

#[test]
fn every_version_of_the_table_has_a_pack() {
    let doc = model();
    for (version, major) in [
        ("1.21.4", 46),
        ("1.21.5", 55),
        ("1.21.6", 63),
        ("1.21.8", 64),
        ("1.21.9", 69),
        ("1.21.11", 75),
        ("26.1", 84),
        ("26.1.2", 84),
        ("26.2", 88),
        ("26.3", 97),
    ] {
        let pack = export_pack(&doc, &PackOptions::new(v(version))).expect(version);
        assert_eq!(pack.format.major, major, "{version}");
    }
    assert_eq!(latest_version(), v("26.3"));
}

#[test]
fn a_version_without_a_pack_format_needs_one_given() {
    let doc = model();
    let unknown = export_pack(&doc, &PackOptions::new(v("26.4")));
    assert_eq!(unknown.err(), Some(McError::PackFormat("26.4".to_owned())));
    let given = PackOptions {
        pack_format: Some(PackFormat {
            major: 100,
            minor: 0,
        }),
        ..PackOptions::new(v("26.4"))
    };
    let pack = export_pack(&doc, &given).expect("given");
    assert_eq!(json(&pack.files, "pack.mcmeta")["pack"]["min_format"], 100);
}

#[test]
fn textures_without_a_path_stay_out_of_the_vanilla_names() {
    let mut doc = model();
    doc.textures.entries.push(TextureEntry {
        label: "glow".into(),
        texture: cuboid_model::Texture::new(common::picture(2, 2, [9, 9, 9, 255])),
        animation: None,
    });
    let pack = export_pack(&doc, &PackOptions::new(v("1.21.3"))).expect("fits");
    assert!(paths(&pack.files).contains(&"assets/minecraft/textures/block/cubik_model/glow.png"));
    assert!(pack.notices.contains(&Notice::UnsavedTextures { count: 1 }));
    let model = text(
        &pack.files,
        "assets/minecraft/models/block/cubik_model.json",
    );
    assert!(
        model.contains("\"glow\": \"block/cubik_model/glow\""),
        "{model}"
    );
}

#[test]
fn the_validator_findings_come_back_as_notices() {
    let mut doc = model();
    // A texture path with an upper-case letter is refused by the game.
    Arc::make_mut(&mut doc.props).minecraft.kind = ModelKind::Block;
    doc.textures.entries[0].texture.source.mc_path = "block/Stone".to_owned();
    let pack = export_pack(&doc, &PackOptions::new(v("1.21.3"))).expect("fits");
    assert!(
        pack.notices
            .iter()
            .any(|n| matches!(n, Notice::Validation(i) if i.code == cuboid_validate::ID_CHARS)),
        "{:?}",
        pack.notices
    );
}

#[test]
fn hostile_names_are_refused() {
    let doc = model();
    for (namespace, id) in [
        ("..", "x"),
        ("a/b", "x"),
        ("minecraft", "../x"),
        ("minecraft", "A"),
        ("", "x"),
    ] {
        let options = PackOptions {
            namespace: namespace.to_owned(),
            id: id.to_owned(),
            ..PackOptions::new(v("1.21.3"))
        };
        assert!(
            matches!(export_pack(&doc, &options), Err(McError::UnsafePath(_))),
            "{namespace} {id}"
        );
    }
    let options = PackOptions {
        replace: Some(vec!["../x".to_owned()]),
        ..PackOptions::new(v("1.21.3"))
    };
    assert!(matches!(
        export_pack(&doc, &options),
        Err(McError::UnsafePath(_))
    ));

    let mut doc = model();
    doc.textures.entries[0].texture.source.mc_path = "../../outside".to_owned();
    assert!(matches!(
        export_pack(&doc, &PackOptions::new(v("1.21.3"))),
        Err(McError::UnsafePath(_))
    ));
    let pack = cuboid_fmt_minecraft::PackExport {
        files: vec![PackFile {
            path: "../escape".to_owned(),
            bytes: Vec::new(),
        }],
        format: PackFormat { major: 3, minor: 0 },
        notices: Vec::new(),
    };
    assert!(pack.to_zip().is_err());
    let dir = tempfile::tempdir().expect("dir");
    assert!(pack.write_folder(dir.path()).is_err());
}

#[test]
fn the_zip_is_deterministic_and_in_file_order() {
    let pack = export_pack(&model(), &PackOptions::new(v("1.21.3"))).expect("fits");
    let bytes = pack.to_zip().expect("zip");
    assert_eq!(bytes, pack.to_zip().expect("zip"));
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("archive");
    let names: Vec<String> = (0..archive.len())
        .map(|at| {
            archive
                .by_index(at)
                .expect("entry")
                .name()
                .expect("name")
                .into_owned()
        })
        .collect();
    assert_eq!(names, paths(&pack.files));
    assert!(names.iter().all(|n| !n.starts_with('/')));
    for (at, file) in pack.files.iter().enumerate() {
        let mut entry = archive.by_index(at).expect("entry");
        let time = entry.last_modified().expect("time");
        assert_eq!((time.year(), time.month(), time.day()), (1980, 1, 1));
        let mut content = Vec::new();
        entry.read_to_end(&mut content).expect("reads");
        assert_eq!(content, file.bytes);
    }
}

#[test]
fn the_folder_holds_the_same_files() {
    let pack = export_pack(&model(), &PackOptions::new(v("1.21.3"))).expect("fits");
    let dir = tempfile::tempdir().expect("dir");
    pack.write_folder(dir.path()).expect("written");
    for file in &pack.files {
        let on_disk = std::fs::read(dir.path().join(&file.path)).expect(&file.path);
        assert_eq!(on_disk, file.bytes);
    }
    // The pack reads back as a model through the folder tree.
    let model_path = dir
        .path()
        .join("assets/minecraft/models/block/cubik_model.json");
    let bytes = std::fs::read(&model_path).expect("model");
    let imported =
        cuboid_fmt_minecraft::import(&bytes, &model_path, &cuboid_fmt_minecraft::DiskFiles)
            .expect("imports");
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    let doc = open(imported);
    assert_eq!(doc.elements.len(), 1);
    assert_eq!(doc.textures.entries[0].texture.image.w, 16);
}
