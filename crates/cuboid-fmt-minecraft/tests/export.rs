//! What a model writes as, and that it reads back unchanged.

mod common;

use std::sync::Arc;

use common::{Tree, elements, import_at, open, picture};
use cuboid_fmt_minecraft::json_export::{JsonOptions, export};
use cuboid_fmt_minecraft::{McError, McVersion, Notice};
use cuboid_model::{Document, ModelKind, TextureEntry};
use serde_json::Value;

const PATH: &str = "pack/assets/minecraft/models/block/m.json";

fn v(text: &str) -> McVersion {
    McVersion::parse(text).expect("version")
}

/// A model that uses most of what the format holds.
const FIXTURE: &str = r##"{
    "__comment": "A \"test\" model",
    "parent": "block/block",
    "ambientocclusion": false,
    "textures": { "all": "block/stone", "side": "block/side" },
    "elements": [
        { "__comment": "slab", "from": [0, 0, 0], "to": [16, 8, 16], "shade": false,
          "faces": {
            "down": { "uv": [0, 0, 16, 16], "texture": "#all", "cullface": "down" },
            "up": { "uv": [16, 16, 0, 0], "texture": "#side", "tintindex": 2 },
            "north": { "uv": [2, 4, 10, 8], "texture": "#all", "rotation": 90 }
          } },
        { "__comment": "turned", "from": [4, 0, 4], "to": [12, 8, 12],
          "rotation": { "origin": [8, 0, 8], "axis": "y", "angle": 22.5, "rescale": true },
          "faces": { "south": { "uv": [0, 0, 8, 8], "texture": "#all" } } },
        { "__comment": "inside out", "from": [15, 15, 15], "to": [10, 10, 10],
          "faces": { "west": { "uv": [8, 0, 0, 8], "texture": "#side" } } }
    ],
    "display": { "gui": { "rotation": [30, 225, 0], "scale": [0.625, 0.625, 0.625] },
                 "ground": { "translation": [0, 3, 0] } },
    "overrides": [ { "predicate": { "pulling": 1, "pull": 0.5 }, "model": "item/b" } ]
}"##;

fn fixture_doc() -> Document {
    let tree = Tree::default()
        .with(
            "pack/assets/minecraft/textures/block/stone.png",
            common::png(16, 16, [1, 2, 3, 255]),
        )
        .with(
            "pack/assets/minecraft/textures/block/side.png",
            common::png(16, 16, [4, 5, 6, 255]),
        );
    open(import_at(FIXTURE, PATH, &tree))
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).expect("the output is JSON")
}

#[test]
fn a_model_reads_back_unchanged() {
    let doc = fixture_doc();
    let out = export(&doc, &JsonOptions::new(v("1.21.3"))).expect("fits");
    assert!(out.files.is_empty(), "every texture has a path");
    let again = open(import_at(&out.json, PATH, &Tree::default()));

    let (a, b) = (elements(&doc), elements(&again));
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.base.label, y.base.label);
        assert!(
            x.base.p.abs_diff_eq(y.base.p, 1e-3),
            "{} {}",
            x.base.p,
            y.base.p
        );
        assert!(x.base.s.abs_diff_eq(y.base.s, 1e-3));
        assert!(x.base.o.abs_diff_eq(y.base.o, 1e-3));
        assert!(
            x.base.e.abs_diff_eq(y.base.e, 1e-2),
            "{} {}",
            x.base.e,
            y.base.e
        );
        assert_eq!(x.flags, y.flags);
        for (f, g) in x.faces.iter().zip(&y.faces) {
            assert_eq!(
                (f.active, f.tint, f.cull, f.uv.rot()),
                (g.active, g.tint, g.cull, g.uv.rot())
            );
            if f.active {
                assert_eq!(f.texture, g.texture);
                assert!(
                    f.uv.tl().abs_diff_eq(g.uv.tl(), 1e-4)
                        && f.uv.br().abs_diff_eq(g.uv.br(), 1e-4)
                );
            }
        }
    }
    assert_eq!(
        doc.props.general.description,
        again.props.general.description
    );
    assert_eq!(doc.props.minecraft.parent, again.props.minecraft.parent);
    assert_eq!(
        doc.props.minecraft.ambient_occlusion,
        again.props.minecraft.ambient_occlusion
    );
    assert_eq!(*doc.display, *again.display);
    assert_eq!(doc.overrides, again.overrides);
    let paths = |d: &Document| -> Vec<String> {
        d.textures
            .entries
            .iter()
            .map(|e| e.texture.source.mc_path.clone())
            .collect()
    };
    assert_eq!(paths(&doc), paths(&again));

    // The model writes the same text again.
    let twice = export(&again, &JsonOptions::new(v("1.21.3"))).expect("fits");
    assert_eq!(out.json, twice.json);
}

#[test]
fn the_text_follows_the_reference_layout() {
    let model = r##"{ "textures": { "t": "block/stone" },
        "elements": [ { "__comment": "a", "from": [0, 0, 0], "to": [8, 8, 8],
        "rotation": { "origin": [4, 4, 4], "axis": "x", "angle": -45 },
        "faces": { "up": { "uv": [0, 0, 8, 8], "texture": "#t", "cullface": "up" } } } ] }"##;
    let doc = open(import_at(model, PATH, &Tree::default()));
    let out = export(&doc, &JsonOptions::new(v("1.21.4"))).expect("fits");
    let expected = "{\n\t\"textures\": {\n\t\t\"t\": \"block/stone\"\n\t},\n\t\"elements\": [\n\t\t{\n\t\t\t\"__comment\": \"a\",\n\t\t\t\"from\": [ 0, 0, 0 ],\n\t\t\t\"to\": [ 8, 8, 8 ],\n\t\t\t\"rotation\": { \"origin\": [ 4, 4, 4 ], \"axis\": \"x\", \"angle\": -45 },\n\t\t\t\"faces\": {\n\t\t\t\t\"up\": { \"uv\": [ 0, 0, 8, 8 ], \"texture\": \"#t\", \"cullface\": \"up\" }\n\t\t\t}\n\t\t}\n\t]\n}";
    assert_eq!(out.json, expected);
}

#[test]
fn rotations_follow_the_version() {
    let turned = |rotation: &str| {
        let model = format!(
            r##"{{ "textures": {{ "t": "block/stone" }}, "elements": [ {{ "from": [4, 0, 4],
            "to": [12, 8, 12], "rotation": {rotation},
            "faces": {{ "up": {{ "texture": "#t" }} }} }} ] }}"##
        );
        open(import_at(&model, PATH, &Tree::default()))
    };
    let y30 = turned(r#"{ "origin": [8, 0, 8], "axis": "y", "angle": 30 }"#);
    let refused = export(&y30, &JsonOptions::new(v("1.21.3"))).err();
    assert!(
        matches!(refused, Some(McError::DoesNotFit { .. })),
        "{refused:?}"
    );
    let text = export(&y30, &JsonOptions::new(v("1.21.6")))
        .expect("30 degrees fit 1.21.6")
        .json;
    assert!(text.contains("\"axis\": \"y\", \"angle\": 30"), "{text}");
    // Forcing writes the model anyway.
    let forced = JsonOptions {
        force: true,
        ..JsonOptions::new(v("1.21.3"))
    };
    assert!(export(&y30, &forced).is_ok());

    let multi = turned(r#"{ "origin": [8, 0, 8], "x": 10, "y": 20, "z": 0 }"#);
    assert!(export(&multi, &JsonOptions::new(v("1.21.10"))).is_err());
    let text = export(&multi, &JsonOptions::new(v("1.21.11")))
        .expect("multi-axis")
        .json;
    let rotation = &json(&text)["elements"][0]["rotation"];
    assert!(
        rotation.get("axis").is_none() && rotation.get("x").is_some(),
        "{text}"
    );
    // One axis is still written the classic way.
    let single = export(&y30, &JsonOptions::new(v("26.3")))
        .expect("fits")
        .json;
    assert!(single.contains("\"axis\": \"y\""));
}

#[test]
fn textures_without_a_path_are_written_beside_the_model() {
    let mut doc = fixture_doc();
    let entry = TextureEntry {
        label: "My Tex".into(),
        texture: cuboid_model::Texture::new(picture(2, 2, [9, 9, 9, 255])),
        animation: None,
    };
    doc.textures.entries.push(entry);
    for (version, folder) in [("1.12.2", "blocks"), ("1.21.3", "block")] {
        let out = export(&doc, &JsonOptions::new(v(version))).expect("fits");
        let map = &json(&out.json)["textures"];
        assert_eq!(map["My Tex"], format!("{folder}/my_tex"), "{version}");
        assert_eq!(out.files.len(), 1);
        assert_eq!(out.files[0].path, "my_tex.png");
        assert_eq!(&out.files[0].bytes[1..4], b"PNG");
        assert!(out.notices.contains(&Notice::UnsavedTextures { count: 1 }));
    }
    // An item uses the item folder.
    Arc::make_mut(&mut doc.props).minecraft.kind = ModelKind::Item;
    let out = export(&doc, &JsonOptions::new(v("1.21.3"))).expect("fits");
    assert_eq!(json(&out.json)["textures"]["My Tex"], "item/my_tex");
}

#[test]
fn the_model_kind_the_version_and_the_content_are_checked() {
    let mut doc = fixture_doc();
    assert_eq!(
        export(&doc, &JsonOptions::new(v("1.8"))).err(),
        Some(McError::UnknownVersion("1.8".into()))
    );
    Arc::make_mut(&mut doc.props).minecraft.kind = ModelKind::Entity;
    assert_eq!(
        export(&doc, &JsonOptions::new(v("1.21.3"))).err(),
        Some(McError::Kind)
    );
    let empty = open(import_at(r#"{ "elements": [] }"#, PATH, &Tree::default()));
    assert_eq!(
        export(&empty, &JsonOptions::new(v("1.21.3"))).err(),
        Some(McError::Empty)
    );
}

#[test]
fn display_overrides_and_flags_follow_the_version() {
    let doc = fixture_doc();
    let old = json(
        &export(&doc, &JsonOptions::new(v("1.21.3")))
            .expect("fits")
            .json,
    );
    assert_eq!(old["ambientocclusion"], false);
    assert_eq!(old["parent"], "block/block");
    assert!(old["display"].get("gui").is_some() && old["display"].get("head").is_none());
    assert_eq!(
        old["display"]["ground"].get("rotation"),
        None,
        "defaults are left out"
    );
    assert_eq!(old["overrides"][0]["predicate"]["pull"], 0.5);
    assert_eq!(old["elements"][0]["shade"], false);
    assert!(
        old["elements"][1].get("shade").is_none(),
        "shade is written only when off"
    );
    assert_eq!(old["elements"][1]["rotation"]["rescale"], true);

    // From 1.21.4 an item definition replaces the overrides.
    let new = export(&doc, &JsonOptions::new(v("1.21.4"))).expect("fits");
    assert!(
        json(&new.json).get("overrides").is_none() || doc.props.minecraft.kind == ModelKind::Block
    );
    let mut item = doc.clone();
    Arc::make_mut(&mut item.props).minecraft.kind = ModelKind::Item;
    let new = export(&item, &JsonOptions::new(v("1.21.4"))).expect("fits");
    assert!(json(&new.json).get("overrides").is_none());
    assert!(
        new.notices
            .contains(&Notice::OverridesNotConverted { count: 1 })
    );
}

#[test]
fn hidden_elements_and_minify_follow_the_settings() {
    let mut doc = fixture_doc();
    {
        let props = Arc::make_mut(&mut doc.props);
        props.general.minify = true;
    }
    let text = export(&doc, &JsonOptions::new(v("1.21.3")))
        .expect("fits")
        .json;
    assert!(
        !text.contains('\n') && !text.contains("__comment\": \"slab"),
        "{text}"
    );
    assert_eq!(json(&text)["elements"].as_array().map(Vec::len), Some(3));
}

#[test]
fn strings_are_escaped() {
    let mut doc = fixture_doc();
    {
        let id = doc.elements.ids()[0];
        let mut journal = cuboid_model::ChangeJournal::default();
        let el = doc.elements.edit(id, &mut journal).expect("element");
        el.base.label = "a \"quoted\" \\ name\u{1}".into();
    }
    let out = export(&doc, &JsonOptions::new(v("1.21.3"))).expect("fits");
    let parsed = json(&out.json);
    assert_eq!(
        parsed["elements"][0]["__comment"],
        "a \"quoted\" \\ name\u{1}"
    );
}

#[test]
fn bleeding_prevention_pulls_the_edges_in() {
    let mut doc = fixture_doc();
    Arc::make_mut(&mut doc.props).general.prevent_bleeding = true;
    let text = export(&doc, &JsonOptions::new(v("1.21.3")))
        .expect("fits")
        .json;
    let down = &json(&text)["elements"][0]["faces"]["down"]["uv"];
    // A 16 texel texture: a step of 0.02, capped at 0.01.
    assert_eq!(down[0], 0.01);
    assert_eq!(down[2], 15.99);
}

#[test]
fn gui_light_is_written_against_the_default_of_the_parent() {
    use cuboid_model::GuiLight::{Front, Side};
    let write = |parent: &str, light: cuboid_model::GuiLight| {
        let mut doc = fixture_doc();
        let props = Arc::make_mut(&mut doc.props);
        props.minecraft.parent = parent.to_owned();
        props.minecraft.gui_light = light;
        let out = export(&doc, &JsonOptions::new(v("1.21.3"))).expect("fits");
        json(&out.json)
    };
    assert_eq!(write("block/block", Front)["gui_light"], "front");
    assert_eq!(write("", Front)["gui_light"], "front");
    assert!(write("block/block", Side).get("gui_light").is_none());
    assert!(write("item/generated", Front).get("gui_light").is_none());
    assert_eq!(write("item/handheld", Side)["gui_light"], "side");

    // It reads back.
    let mut doc = fixture_doc();
    Arc::make_mut(&mut doc.props).minecraft.gui_light = Front;
    let out = export(&doc, &JsonOptions::new(v("1.21.3"))).expect("fits");
    let again = open(import_at(&out.json, PATH, &Tree::default()));
    assert_eq!(again.props.minecraft.gui_light, Front);
}
