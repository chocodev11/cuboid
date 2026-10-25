//! What a model file imports as, with hand-written models.

mod common;

use common::{Tree, elements, import_at, open, png};
use cuboid_fmt_minecraft::{Limits, McError, Notice, import, import_with};
use cuboid_model::mc;
use cuboid_model::{DisplaySlotId, FaceFlags, FaceName, ModelKind};
use glam::Vec3;
use std::path::Path;

const STONE: &str = r##"{
    "ambientocclusion": false,
    "textures": { "all": "block/stone", "particle": "block/stone" },
    "elements": [
        { "__comment": "body", "from": [0, 0, 0], "to": [16, 8, 16],
          "faces": {
            "down": { "uv": [0, 0, 16, 16], "texture": "#all", "cullface": "down" },
            "up": { "uv": [0, 0, 16, 16], "texture": "#all", "tintindex": 1 },
            "north": { "uv": [2, 4, 10, 8], "texture": "#all", "rotation": 90, "cullface": "bottom" },
            "south": { "uv": [0, 0, 16, 8], "texture": "#all" }
          } }
    ],
    "display": { "gui": { "rotation": [30, 225, 0], "scale": [0.625, 0.625, 0.625] } }
}"##;

fn stone_tree() -> Tree {
    Tree::default().with(
        "pack/assets/minecraft/textures/block/stone.png",
        png(16, 16, [9, 8, 7, 255]),
    )
}

const STONE_PATH: &str = "pack/assets/minecraft/models/block/stone.json";

#[test]
fn a_block_model_maps_like_the_minecraft_json() {
    let imported = import_at(STONE, STONE_PATH, &stone_tree());
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    assert!(imported.doc.keep_exact_rotations);
    let doc = open(imported);
    assert_eq!(&*doc.props.general.name, "stone");
    assert_eq!(doc.props.minecraft.kind, ModelKind::Block);
    assert!(!doc.props.minecraft.ambient_occlusion);
    assert!(doc.props.minecraft.include_display);

    let tex = &doc.textures.entries;
    assert_eq!(tex.len(), 2);
    assert_eq!(&*tex[0].label, "all");
    assert_eq!(tex[0].texture.source.mc_path, "block/stone");
    assert_eq!(tex[0].texture.image.w, 16);
    // `particle` shares the picture of `all` by the same path, not by alias.
    assert_eq!(tex[1].texture.source.mc_path, "block/stone");

    let el = &elements(&doc)[0];
    assert_eq!(&*el.base.label, "body");
    // The stored position is the `from` corner: Z is negated, and from.z = 0.
    assert_eq!(el.base.p, Vec3::ZERO);
    assert_eq!(el.base.s, Vec3::new(16.0, 8.0, 16.0));
    let down = &el.faces[FaceName::Down as usize];
    assert!(down.active);
    assert_eq!(down.cull, FaceFlags::DOWN);
    assert_eq!(&*down.texture, "all");
    assert_eq!(el.faces[FaceName::Up as usize].tint, 1);
    let north = &el.faces[FaceName::North as usize];
    assert_eq!(north.uv.rot(), 90);
    // The alias `bottom` of cullface is the down side.
    assert_eq!(north.cull, FaceFlags::DOWN);
    // uv [2, 4, 10, 8]: u from 2/16 to 10/16, v from the top 4/16 to 8/16.
    assert!((north.uv.tl().x - 0.125).abs() < 1e-6 && (north.uv.br().x - 0.625).abs() < 1e-6);
    assert!((north.uv.tl().y - 0.5).abs() < 1e-6 && (north.uv.br().y - 0.75).abs() < 1e-6);
    // Faces the file does not list are off.
    assert!(!el.faces[FaceName::West as usize].active);

    let gui = &doc.display[DisplaySlotId::Gui];
    assert!(gui.enabled);
    assert_eq!(gui.rotation, Vec3::new(30.0, 225.0, 0.0));
    assert!(!doc.display[DisplaySlotId::Head].enabled);
}

#[test]
fn inverted_elements_and_mirrored_uvs_are_kept() {
    let model = r##"{
        "textures": { "t": "block/stone" },
        "elements": [
          { "from": [16, 16, 16], "to": [0, 0, 0],
            "faces": { "north": { "uv": [16, 0, 0, 16], "texture": "#t" },
                       "up": { "uv": [0, 16, 16, 0], "texture": "#t" } } }
        ]
    }"##;
    let doc = open(import_at(model, STONE_PATH, &stone_tree()));
    let el = &elements(&doc)[0];
    assert_eq!(el.base.s, Vec3::splat(-16.0));
    let b = mc::unplace(el.base.p, el.base.s, el.base.o, el.base.e, false);
    assert_eq!((b.from, b.to), (Vec3::splat(16.0), Vec3::ZERO));
    let north = &el.faces[FaceName::North as usize];
    assert!(north.uv.tl().x > north.uv.br().x, "u runs backwards");
    let up = &el.faces[FaceName::Up as usize];
    assert!(up.uv.tl().y > up.uv.br().y, "v runs backwards");
}

#[test]
fn rotations_are_kept_exactly() {
    let model = r##"{
        "textures": { "t": "block/stone" },
        "elements": [
          { "from": [4, 0, 4], "to": [12, 8, 12],
            "rotation": { "origin": [8, 0, 8], "axis": "y", "angle": 30 },
            "faces": { "north": { "texture": "#t" } } },
          { "from": [0, 0, 0], "to": [4, 4, 4],
            "rotation": { "origin": [2, 2, 2], "x": 10, "y": 20, "z": 30 },
            "faces": { "north": { "texture": "#t" } } },
          { "from": [0, 0, 0], "to": [4, 4, 4],
            "rotation": { "origin": [2, 2, 2], "axis": "w", "angle": 45 },
            "faces": { "north": { "texture": "#t" } } }
        ]
    }"##;
    let imported = import_at(model, STONE_PATH, &stone_tree());
    assert!(imported.doc.keep_exact_rotations);
    let props = imported.doc.sections.props.clone().expect("props");
    assert!(
        !props.general.restrict_rotations,
        "30 degrees is no Minecraft rotation"
    );
    let doc = open(imported);
    let els = elements(&doc);
    let angles = |i: usize| {
        let el = &els[i];
        mc::unplace(el.base.p, el.base.s, el.base.o, el.base.e, false)
    };
    assert!(
        angles(0)
            .rotation
            .abs_diff_eq(Vec3::new(0.0, 30.0, 0.0), 1e-2)
    );
    assert!(angles(0).pivot.abs_diff_eq(Vec3::new(8.0, 0.0, 8.0), 1e-3));
    assert!(angles(0).from.abs_diff_eq(Vec3::new(4.0, 0.0, 4.0), 1e-3));
    let multi = angles(1).rotation;
    assert!(
        mc::rotation_m(multi).abs_diff_eq(mc::rotation_m(Vec3::new(10.0, 20.0, 30.0)), 1e-3),
        "{multi}"
    );
    // An unknown axis gives no rotation, but the pivot stays.
    assert_eq!(els[2].base.e, Vec3::ZERO);
    assert!(angles(2).pivot.abs_diff_eq(Vec3::splat(2.0), 1e-3));
}

#[test]
fn a_parent_in_the_folder_tree_gives_what_the_child_lacks() {
    let parent = r##"{
        "textures": { "side": "block/stone", "top": "block/other" },
        "display": { "gui": { "scale": [0.5, 0.5, 0.5] }, "head": { "translation": [0, 1, 0] } },
        "elements": [ { "from": [0, 0, 0], "to": [16, 16, 16],
            "faces": { "north": { "texture": "#side" }, "up": { "texture": "#top" } } } ]
    }"##;
    let child = r##"{
        "parent": "block/base",
        "textures": { "top": "block/stone" },
        "display": { "gui": { "scale": [2, 2, 2] } }
    }"##;
    let tree = stone_tree().with("pack/assets/minecraft/models/block/base.json", parent);
    let imported = import_at(
        child,
        "pack/assets/minecraft/models/block/child.json",
        &tree,
    );
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    let doc = open(imported);
    assert_eq!(doc.elements.len(), 1, "the elements come from the parent");
    assert_eq!(doc.props.minecraft.parent, "block/base");
    let labels: Vec<&str> = doc.textures.entries.iter().map(|e| &*e.label).collect();
    assert_eq!(labels, ["side", "top"]);
    // The child's texture wins by key: `top` is block/stone, not block/other.
    assert_eq!(
        doc.textures.entries[1].texture.source.mc_path,
        "block/stone"
    );
    // The child's slot replaces the parent's; the parent's other slot stays.
    assert_eq!(doc.display[DisplaySlotId::Gui].scale, Vec3::splat(2.0));
    assert_eq!(doc.display[DisplaySlotId::Head].translation, Vec3::Y);
}

#[test]
fn the_common_block_parents_are_built_in() {
    let model = r#"{ "parent": "minecraft:block/cube_all", "textures": { "all": "block/stone" } }"#;
    let imported = import_at(model, STONE_PATH, &stone_tree());
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    let doc = open(imported);
    assert_eq!(doc.elements.len(), 1);
    let el = &elements(&doc)[0];
    assert_eq!(el.base.s, Vec3::splat(16.0));
    // The faces name `down`, `up` and so on, which are aliases of `all`; the unused ones are
    // dropped, `all` stays because they refer to it.
    let entries = &doc.textures.entries;
    assert!(entries.iter().any(|e| &*e.label == "all"));
    assert!(entries.iter().any(|e| e.texture.source.mc_path == "#all"));
    for face in &el.faces {
        assert!(entries.iter().any(|e| e.label == face.texture));
        assert_eq!(face.uv.tl().x.min(face.uv.br().x), 0.0);
        assert_eq!(face.cull.bits().count_ones(), 1);
    }
    assert_eq!(doc.props.minecraft.parent, "minecraft:block/cube_all");
}

#[test]
fn a_cross_parent_has_two_rescaled_planes() {
    let model = r#"{ "parent": "block/cross", "textures": { "cross": "block/stone" } }"#;
    let doc = open(import_at(model, STONE_PATH, &stone_tree()));
    let els = elements(&doc);
    assert_eq!(els.len(), 2);
    assert!(
        els.iter()
            .all(|e| !e.flags.contains(cuboid_model::ElementFlags::SHADE))
    );
    assert!(
        els.iter()
            .all(|e| e.flags.contains(cuboid_model::ElementFlags::RESCALE))
    );
}

#[test]
fn an_item_generated_model_becomes_its_extruded_sprite() {
    // A plus: one pixel wide arms, so the rows merge into three rectangles.
    let mut buf = cuboid_model::image::RgbaBuf::new(3, 3);
    for (x, y) in [(1, 0), (0, 1), (1, 1), (2, 1), (1, 2)] {
        // Rows run bottom to top; set takes the image's own row.
        buf.set(x, 2 - y, [200, 100, 50, 255]);
    }
    let gem = cuboid_model::image::encode_png(&cuboid_model::image::Image::from_rgba(buf))
        .expect("encodes");
    let tree = Tree::default().with("pack/assets/minecraft/textures/item/gem.png", gem);
    let model = r#"{ "parent": "item/generated", "textures": { "layer0": "item/gem" } }"#;
    let imported = import_at(model, "pack/assets/minecraft/models/item/gem.json", &tree);
    assert!(
        imported.notices.contains(&Notice::Extruded { elements: 3 }),
        "{:?}",
        imported.notices
    );
    let doc = open(imported);
    assert_eq!(doc.props.minecraft.kind, ModelKind::Item);
    assert!(doc.props.minecraft.parent.is_empty());
    let els = elements(&doc);
    assert_eq!(els.len(), 3);
    // The middle row spans the sprite, one third of a pixel unit apart in z: 7.5 to 8.5.
    let wide = els
        .iter()
        .find(|e| (e.base.s.x - 16.0).abs() < 1e-3)
        .expect("the middle row");
    let b = mc::unplace(wide.base.p, wide.base.s, wide.base.o, wide.base.e, false);
    assert!((b.from.z - 7.5).abs() < 1e-3 && (b.to.z - 8.5).abs() < 1e-3);
    // South is the front, north reads the picture from behind.
    let south = &wide.faces[FaceName::South as usize];
    let north = &wide.faces[FaceName::North as usize];
    assert!(south.active && north.active);
    assert!(south.uv.tl().x < south.uv.br().x && north.uv.tl().x > north.uv.br().x);
    // Faces that touch another box are off: the plus's middle row hides nothing at its ends but
    // its top and bottom faces are covered where the arms stand.
    assert!(wide.faces[FaceName::West as usize].active);
}

#[test]
fn a_layered_sprite_tints_by_layer() {
    let tree = Tree::default()
        .with(
            "pack/assets/minecraft/textures/item/a.png",
            png(2, 2, [1, 2, 3, 255]),
        )
        .with(
            "pack/assets/minecraft/textures/item/b.png",
            png(2, 2, [4, 5, 6, 255]),
        );
    let model = r#"{ "parent": "item/handheld",
        "textures": { "layer0": "item/a", "layer1": "item/b" } }"#;
    let doc = open(import_at(
        model,
        "pack/assets/minecraft/models/item/t.json",
        &tree,
    ));
    let els = elements(&doc);
    assert_eq!(els.len(), 2);
    assert_eq!(els[0].faces[FaceName::South as usize].tint, 0);
    assert_eq!(&*els[1].faces[FaceName::South as usize].texture, "layer1");
    assert_eq!(els[1].faces[FaceName::South as usize].tint, 1);
}

#[test]
fn an_unknown_parent_is_a_notice() {
    let model = r#"{ "parent": "mod:block/special", "elements": [
        { "from": [0, 0, 0], "to": [1, 1, 1] } ] }"#;
    let imported = import_at(model, STONE_PATH, &Tree::default());
    assert_eq!(
        imported.notices,
        [Notice::ParentNotFound {
            parent: "mod:block/special".to_owned()
        }]
    );
    assert_eq!(open(imported).elements.len(), 1);
}

#[test]
fn parents_that_loop_are_cut() {
    let tree = Tree::default()
        .with("m/a.json", r#"{ "parent": "b" }"#)
        .with(
            "m/b.json",
            r#"{ "parent": "a", "elements": [ { "from": [0,0,0], "to": [1,1,1] } ] }"#,
        );
    let imported = import_at(r#"{ "parent": "a" }"#, "m/start.json", &tree);
    assert!(
        imported
            .notices
            .iter()
            .any(|n| matches!(n, Notice::ParentChain { .. })),
        "{:?}",
        imported.notices
    );
    assert_eq!(open(imported).elements.len(), 1);
}

#[test]
fn a_long_parent_chain_stops_at_the_limit() {
    let mut tree = Tree::default();
    for n in 0..40 {
        tree = tree.with(
            &format!("m/p{n}.json"),
            format!(r#"{{ "parent": "p{}" }}"#, n + 1),
        );
    }
    let imported = import_at(r#"{ "parent": "p0" }"#, "m/start.json", &tree);
    assert!(
        imported
            .notices
            .iter()
            .any(|n| matches!(n, Notice::ParentChain { .. }))
    );
}

#[test]
fn overrides_and_texture_size_and_aliases() {
    let model = r##"{
        "texture_size": [32, 32],
        "textures": { "a": "block/stone", "b": "#a", "c": { "sprite": "block/stone" } },
        "elements": [ { "from": [0, 0, 0], "to": [16, 16, 16],
            "faces": { "north": { "uv": [0, 0, 16, 16], "texture": "#b" } } } ],
        "overrides": [
            { "predicate": { "pulling": 1, "pull": 0.65, "custom_model_data": 3 }, "model": "item/bow_1" },
            { "model": "ignored" } ]
    }"##;
    let imported = import_at(model, STONE_PATH, &stone_tree());
    assert!(
        imported.notices.contains(&Notice::Skipped {
            what: "override predicates of other kinds".to_owned(),
            count: 1
        }),
        "{:?}",
        imported.notices
    );
    let doc = open(imported);
    // `b` is an alias of `a`: same picture, shortcut path.
    let b = doc
        .textures
        .entries
        .iter()
        .find(|e| &*e.label == "b")
        .expect("b");
    assert_eq!(b.texture.source.mc_path, "#a");
    assert_eq!(doc.overrides.len(), 1);
    assert_eq!(doc.overrides[0].pulling, Some(1));
    assert_eq!(doc.overrides[0].pull, Some(0.65));
    assert_eq!(&doc.overrides[0].model, "item/bow_1");
    assert!(doc.props.minecraft.keep_overrides);
    // The rectangle is measured by the 32 x 32 space of the file.
    let north = &elements(&doc)[0].faces[FaceName::North as usize];
    assert!((north.uv.br().x - 0.5).abs() < 1e-6);
}

#[test]
fn the_reference_dialect_is_the_fallback() {
    let model = r#"{ "textures": { "t": "block/stone", }, "elements": [
        { "from": [0, 0, 0], "to": [1, 1, 1], }, ], }"#;
    let doc = open(import_at(model, STONE_PATH, &stone_tree()));
    assert_eq!(doc.elements.len(), 1);
}

#[test]
fn the_type_follows_the_parent_then_the_folder() {
    let kind = |text: &str, path: &str| {
        open(import_at(text, path, &Tree::default()))
            .props
            .minecraft
            .kind
    };
    let body = r#""elements": [ { "from": [0,0,0], "to": [1,1,1] } ]"#;
    assert_eq!(
        kind(&format!("{{ {body} }}"), "p/models/item/a.json"),
        ModelKind::Item
    );
    assert_eq!(
        kind(&format!("{{ {body} }}"), "p/models/block/a.json"),
        ModelKind::Block
    );
    assert_eq!(kind(&format!("{{ {body} }}"), "p/a.json"), ModelKind::Block);
    assert_eq!(
        kind(
            &format!("{{ \"parent\": \"item/handheld\", {body} }}"),
            "p/models/block/a.json"
        ),
        ModelKind::Item
    );
}

#[test]
fn hostile_models_are_errors_or_notices() {
    let err = |text: &str| import(text.as_bytes(), Path::new("m/x.json"), &Tree::default()).err();
    assert_eq!(err(""), Some(McError::Syntax));
    assert_eq!(err("[1, 2]"), Some(McError::Syntax));
    assert_eq!(err("{ nope"), Some(McError::Syntax));
    assert_eq!(err(r#"{ "credit": "x" }"#), Some(McError::NotAModel));
    assert_eq!(
        err(r#"{ "textures": { "a": 5 } }"#),
        Some(McError::TextureValue("a".to_owned()))
    );
    assert!(matches!(
        err(r##"{ "elements": [ { "from": [0,0,0], "to": [1,1,1], "faces": { "up": {} } } ] }"##),
        Some(McError::FaceTexture { .. })
    ));
    // Binary garbage and deep nesting end in errors, not panics.
    let garbage: Vec<u8> = (0..=255).collect();
    assert!(import(&garbage, Path::new("m/x.json"), &Tree::default()).is_err());
    let deep = format!("{{ \"elements\": {}{}", "[".repeat(5000), "]".repeat(5000));
    assert!(import(deep.as_bytes(), Path::new("m/x.json"), &Tree::default()).is_err());
}

#[test]
fn texture_paths_cannot_leave_the_folder() {
    let model = r##"{ "textures": { "a": "../../secret", "b": "C:/secret", "c": "x/../../y",
        "d": "ns:../z", "e": "/abs" },
        "elements": [ { "from": [0,0,0], "to": [1,1,1], "faces": { "up": { "texture": "#a" } } } ] }"##;
    let imported = import_at(model, "m/x.json", &Tree::default());
    let missing = imported
        .notices
        .iter()
        .filter(|n| matches!(n, Notice::TextureMissing { .. }))
        .count();
    assert_eq!(missing, 5);
}

#[test]
fn the_limits_are_checked_before_the_data_is_built() {
    let limits = Limits {
        max_elements: 2,
        max_textures: 1,
        max_overrides: 1,
        ..Limits::default()
    };
    let at = |text: &str| {
        import_with(
            text.as_bytes(),
            Path::new("m/x.json"),
            &Tree::default(),
            &limits,
        )
    };
    let cube = r#"{ "from": [0,0,0], "to": [1,1,1] }"#;
    assert!(at(&format!("{{ \"elements\": [{cube}, {cube}] }}")).is_ok());
    assert_eq!(
        at(&format!("{{ \"elements\": [{cube}, {cube}, {cube}] }}")).err(),
        Some(McError::Limit("elements"))
    );
    assert_eq!(
        at(r#"{ "textures": { "a": "x", "b": "y" } }"#).err(),
        Some(McError::Limit("texture entries"))
    );
    assert_eq!(
        at(r#"{ "elements": [], "overrides": [ {"predicate": {}}, {"predicate": {}} ] }"#).err(),
        Some(McError::Limit("override cases"))
    );
    let tiny = Limits {
        max_text: 10,
        ..Limits::default()
    };
    assert_eq!(
        import_with(
            b"{ \"elements\": [] }",
            Path::new("m/x.json"),
            &Tree::default(),
            &tiny
        )
        .err(),
        Some(McError::Limit("the file size"))
    );
}

#[test]
fn a_huge_number_reads_as_missing_not_as_a_crash() {
    let model = r#"{ "elements": [ { "from": [1e999, 0, 0], "to": [1e30, 1, 1] } ] }"#;
    let doc = open(import_at(model, "m/x.json", &Tree::default()));
    let el = &elements(&doc)[0];
    assert!(el.base.p.is_finite() && el.base.s.is_finite());
}

#[test]
fn textures_are_found_beside_the_model_when_there_is_no_pack_layout() {
    let model = r#"{ "textures": { "a": "block/a", "b": "block/b", "c": "mod:block/c" },
        "elements": [ { "from": [0,0,0], "to": [1,1,1] } ] }"#;
    let tree = Tree::default()
        .with("dir/textures/block/a.png", png(2, 2, [1, 1, 1, 255]))
        .with("dir/block/b.png", png(4, 4, [2, 2, 2, 255]))
        .with("dir/c.png", png(8, 8, [3, 3, 3, 255]));
    let imported = import_at(model, "dir/m.json", &tree);
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    let doc = open(imported);
    let sizes: Vec<u32> = doc
        .textures
        .entries
        .iter()
        .map(|e| e.texture.image.w)
        .collect();
    assert_eq!(sizes, [2, 4, 8]);
    // The namespace stays in the path of another namespace than minecraft.
    assert_eq!(
        doc.textures.entries[2].texture.source.mc_path,
        "mod:block/c"
    );
}

/// The path an element face's texture entry stands for, such as `#side`.
fn target(doc: &cuboid_model::Document, face: &cuboid_model::Face) -> String {
    doc.textures
        .entries
        .iter()
        .find(|e| e.label == face.texture)
        .map(|e| e.texture.source.mc_path.clone())
        .expect("the face has an entry")
}

fn slot(doc: &cuboid_model::Document, id: DisplaySlotId) -> cuboid_model::DisplaySlot {
    doc.display[id].clone()
}

#[test]
fn block_cube_is_one_culled_element_under_block_block() {
    let model = r#"{ "parent": "block/cube", "textures": { "down": "block/stone", "up": "block/stone",
        "north": "block/stone", "south": "block/stone", "west": "block/stone", "east": "block/stone" } }"#;
    let doc = open(import_at(model, STONE_PATH, &stone_tree()));
    let els = elements(&doc);
    assert_eq!(els.len(), 1);
    let el = &els[0];
    assert_eq!(el.base.s, Vec3::splat(16.0));
    let b = mc::unplace(el.base.p, el.base.s, el.base.o, el.base.e, false);
    assert_eq!((b.from, b.to), (Vec3::ZERO, Vec3::splat(16.0)));
    for face in &el.faces {
        let key = mc::face_key(face.name);
        assert!(face.active);
        assert_eq!(face.cull, mc::cull_from_key(key), "{key}");
        assert_eq!(&*face.texture, key);
        let (tl, br) = (face.uv.tl(), face.uv.br());
        assert_eq!((tl.x.min(br.x), tl.y.min(br.y)), (0.0, 0.0));
        assert_eq!((tl.x.max(br.x), tl.y.max(br.y)), (1.0, 1.0));
    }
    // block/block gives the model its inventory lighting and gui display; they are not written
    // back, the game brings them with the parent.
    assert_eq!(doc.props.minecraft.gui_light, cuboid_model::GuiLight::Side);
    assert!(!doc.props.minecraft.include_display);
    let gui = slot(&doc, DisplaySlotId::Gui);
    assert!(gui.enabled);
    assert_eq!(gui.rotation, Vec3::new(30.0, 225.0, 0.0));
    assert_eq!(gui.scale, Vec3::splat(0.625));
}

#[test]
fn cube_all_and_cube_column_assign_their_faces() {
    let all = r#"{ "parent": "block/cube_all", "textures": { "all": "block/stone" } }"#;
    let doc = open(import_at(all, STONE_PATH, &stone_tree()));
    for face in &elements(&doc)[0].faces {
        assert_eq!(target(&doc, face), "#all");
    }

    let column = r#"{ "parent": "block/cube_column",
        "textures": { "end": "block/stone", "side": "block/stone" } }"#;
    let doc = open(import_at(column, STONE_PATH, &stone_tree()));
    for face in &elements(&doc)[0].faces {
        let want = match face.name {
            FaceName::Down | FaceName::Up => "#end",
            _ => "#side",
        };
        assert_eq!(target(&doc, face), want, "{:?}", face.name);
    }
}

#[test]
fn block_cross_matches_the_client() {
    let model = r#"{ "parent": "block/cross", "textures": { "cross": "block/stone" } }"#;
    let doc = open(import_at(model, STONE_PATH, &stone_tree()));
    assert!(!doc.props.minecraft.ambient_occlusion);
    let els = elements(&doc);
    assert_eq!(els.len(), 2);
    let planes = [
        (
            Vec3::new(0.8, 0.0, 8.0),
            Vec3::new(15.2, 16.0, 8.0),
            [FaceName::North, FaceName::South],
        ),
        (
            Vec3::new(8.0, 0.0, 0.8),
            Vec3::new(8.0, 16.0, 15.2),
            [FaceName::West, FaceName::East],
        ),
    ];
    for (el, (from, to, names)) in els.iter().zip(planes) {
        assert!(!el.flags.contains(cuboid_model::ElementFlags::SHADE));
        assert!(el.flags.contains(cuboid_model::ElementFlags::RESCALE));
        let b = mc::unplace(el.base.p, el.base.s, el.base.o, el.base.e, false);
        // A flat plane keeps a thickness of 0.001.
        assert!(b.from.abs_diff_eq(from, 2e-3), "{} {from}", b.from);
        assert!(b.to.abs_diff_eq(to, 2e-3), "{} {to}", b.to);
        assert!(b.pivot.abs_diff_eq(Vec3::splat(8.0), 1e-4));
        assert!(b.rotation.abs_diff_eq(Vec3::new(0.0, 45.0, 0.0), 1e-3));
        for face in &el.faces {
            assert_eq!(face.active, names.contains(&face.name), "{:?}", face.name);
            if face.active {
                assert_eq!(&*face.texture, "cross");
                let (tl, br) = (face.uv.tl(), face.uv.br());
                assert_eq!((tl.x.min(br.x), tl.y.min(br.y)), (0.0, 0.0));
                assert_eq!((tl.x.max(br.x), tl.y.max(br.y)), (1.0, 1.0));
            }
        }
    }
}

#[test]
fn item_generated_brings_front_light_and_its_display() {
    let mut buf = cuboid_model::image::RgbaBuf::new(1, 1);
    buf.set(0, 0, [200, 100, 50, 255]);
    let gem = cuboid_model::image::encode_png(&cuboid_model::image::Image::from_rgba(buf))
        .expect("encodes");
    let tree = Tree::default().with("pack/assets/minecraft/textures/item/gem.png", gem);
    let model = r#"{ "parent": "item/generated", "textures": { "layer0": "item/gem" } }"#;
    let doc = open(import_at(
        model,
        "pack/assets/minecraft/models/item/gem.json",
        &tree,
    ));
    assert_eq!(doc.props.minecraft.gui_light, cuboid_model::GuiLight::Front);
    assert!(!doc.props.minecraft.include_display);
    let ground = slot(&doc, DisplaySlotId::Ground);
    assert!(ground.enabled);
    assert_eq!(ground.translation, Vec3::new(0.0, 2.0, 0.0));
    assert_eq!(ground.scale, Vec3::splat(0.5));
    let fixed = slot(&doc, DisplaySlotId::Fixed);
    assert!(fixed.enabled);
    assert_eq!(fixed.rotation, Vec3::new(0.0, 180.0, 0.0));
    assert!(slot(&doc, DisplaySlotId::Head).enabled);
    assert!(slot(&doc, DisplaySlotId::ThirdRight).enabled);
    assert!(slot(&doc, DisplaySlotId::FirstRight).enabled);
    assert!(!slot(&doc, DisplaySlotId::Gui).enabled);
}

#[test]
fn gui_light_is_read_and_inherited() {
    let own = r#"{ "gui_light": "front", "parent": "block/cube_all", "textures": { "all": "block/stone" } }"#;
    let imported = import_at(own, STONE_PATH, &stone_tree());
    assert!(
        !imported
            .notices
            .iter()
            .any(|n| matches!(n, Notice::Skipped { what, .. } if what == "gui light")),
        "{:?}",
        imported.notices
    );
    let doc = open(imported);
    assert_eq!(doc.props.minecraft.gui_light, cuboid_model::GuiLight::Front);

    // A child of a model in the folder tree takes the parent's value; its own value wins.
    let tree = stone_tree().with(
        "pack/assets/minecraft/models/block/flat.json",
        r#"{ "gui_light": "front", "textures": { "all": "block/stone" } }"#,
    );
    let doc = open(import_at(
        r#"{ "parent": "block/flat" }"#,
        STONE_PATH,
        &tree,
    ));
    assert_eq!(doc.props.minecraft.gui_light, cuboid_model::GuiLight::Front);
    let doc = open(import_at(
        r#"{ "gui_light": "side", "parent": "block/flat" }"#,
        STONE_PATH,
        &tree,
    ));
    assert_eq!(doc.props.minecraft.gui_light, cuboid_model::GuiLight::Side);
}

#[test]
fn the_builtin_parents_offer_their_display_slots_as_presets() {
    use cuboid_fmt_minecraft::{DISPLAY_PRESETS, builtin_display};
    for id in DISPLAY_PRESETS {
        assert!(builtin_display(id).is_some(), "{id}");
    }
    let generated = builtin_display("item/generated").expect("a built-in parent");
    let ground = &generated[DisplaySlotId::Ground];
    assert!(ground.enabled);
    assert_eq!(ground.translation, glam::Vec3::new(0.0, 2.0, 0.0));
    assert_eq!(ground.scale, glam::Vec3::splat(0.5));
    assert!(
        !generated[DisplaySlotId::Gui].enabled,
        "a slot the parent leaves out is off"
    );
    let block = builtin_display("block/block").expect("a built-in parent");
    assert_eq!(
        block[DisplaySlotId::Gui].rotation,
        glam::Vec3::new(30.0, 225.0, 0.0)
    );
    assert!(
        builtin_display("block/cube_all").is_none(),
        "no display settings of its own"
    );
    assert!(builtin_display("mod/unknown").is_none());
}
