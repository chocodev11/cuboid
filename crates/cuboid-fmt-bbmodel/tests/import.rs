//! What a project imports as, with hand-written projects.

mod common;

use std::path::Path;

use common::{center_m, data_url, elements_of, import_json, near, open};
use cuboid_fmt_bbmodel::{Notice, import};
use cuboid_model::{DisplaySlotId, FaceFlags, FaceName, ObjRef};
use glam::{Mat3, Vec2, Vec3};
use serde_json::{Value, json};

/// A `java_block` project with the given elements and one 16 x 16 texture.
fn java(elements: Value) -> Value {
    json!({
        "meta": {"format_version": "4.10", "model_format": "java_block", "box_uv": false},
        "name": "block",
        "resolution": {"width": 16, "height": 16},
        "elements": elements,
        "textures": [{
            "name": "stone.png", "folder": "block", "id": "0",
            "uv_width": 16, "uv_height": 16,
            "source": data_url(16, 16, [10, 20, 30, 255]),
            "uuid": "t0"
        }],
    })
}

fn face(uv: [f32; 4], texture: Value) -> Value {
    json!({"uv": uv, "texture": texture})
}

fn all_faces(texture: Value) -> Value {
    let mut faces = serde_json::Map::new();
    for key in ["north", "east", "south", "west", "up", "down"] {
        faces.insert(
            key.to_owned(),
            face([0.0, 0.0, 16.0, 16.0], texture.clone()),
        );
    }
    Value::Object(faces)
}

#[test]
fn a_java_block_cube_maps_like_the_minecraft_json() {
    let mut faces = all_faces(json!(0));
    faces["north"] = json!({"uv": [2, 4, 10, 8], "texture": 0, "rotation": 90, "tint": 1,
        "cullface": "north"});
    faces["down"] = json!({"uv": [0, 0, 0, 0], "texture": null});
    let project = java(json!([{
        "name": "Box", "from": [0, 0, 0], "to": [16, 8, 16], "origin": [8, 8, 8],
        "faces": faces, "type": "cube", "uuid": "e0", "color": 2
    }]));
    let imported = import_json(&project);
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    let doc = open(imported);
    assert_eq!(doc.elements.len(), 1);
    let el = &*doc.elements.items()[0];
    assert_eq!(&*el.base.label, "Box");
    assert_eq!(el.base.p, Vec3::new(0.0, 0.0, 0.0));
    assert_eq!(el.base.s, Vec3::new(16.0, 8.0, 16.0));
    assert_eq!(el.base.o, Vec3::new(8.0, 8.0, -8.0));
    assert_eq!(el.base.e, Vec3::ZERO);

    let north = &el.faces[FaceName::North as usize];
    assert!(north.active);
    assert_eq!(&*north.texture, "0");
    assert_eq!(north.uv.rot(), 90);
    assert_eq!(north.tint, 1);
    assert_eq!(north.cull, FaceFlags::NORTH);
    // interop 8.5: tl = (u0, 1 - v1), br = (u1, 1 - v0), the units being 16.
    assert_eq!(north.uv.tl(), Vec2::new(0.125, 0.5));
    assert_eq!(north.uv.br(), Vec2::new(0.625, 0.75));
    assert!(!el.faces[FaceName::Down as usize].active);
    assert!(el.faces[FaceName::Up as usize].active);

    let texture = &doc.textures.entries[0];
    assert_eq!(&*texture.label, "0");
    assert_eq!(texture.texture.source.mc_path, "block/stone");
    assert_eq!((texture.texture.image.w, texture.texture.image.h), (16, 16));
}

#[test]
fn a_marker_colour_labels_the_cube_never_tints_it_and_is_written_back() {
    let project = java(json!([{
        "name": "Red", "from": [0, 0, 0], "to": [4, 4, 4], "faces": all_faces(json!(0)),
        "type": "cube", "uuid": "e0", "color": 3
    }]));
    let doc = open(import_json(&project));
    // Blockbench draws the texture as it is: the marker only labels the cube in the editor.
    assert_eq!(doc.elements.items()[0].color, [1.0; 4]);
    let written: Value =
        serde_json::from_slice(&cuboid_fmt_bbmodel::export(&doc).expect("exports").bytes)
            .expect("json");
    assert_eq!(written["elements"][0]["color"], json!(3));
}

#[test]
fn a_rotation_turns_the_box_about_its_pivot() {
    let project = java(json!([{
        "name": "Turned", "from": [4, 0, 4], "to": [12, 16, 12], "origin": [8, 8, 8],
        "rotation": [0, 22.5, 0], "faces": all_faces(json!(0)), "type": "cube"
    }]));
    let imported = import_json(&project);
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    let doc = open(imported);
    let el = &*doc.elements.items()[0];
    // interop 8.5: the Euler form of a rotation by -angle about +Y.
    assert!(
        near(el.base.e, Vec3::new(0.0, 337.5, 0.0)),
        "{:?}",
        el.base.e
    );
    // The stored corner is the rotated corner (tools 1.3): (4, 4) turned about (8, 8).
    assert!(
        near(el.base.p, Vec3::new(2.7738, 0.0, -5.8352)),
        "{:?}",
        el.base.p
    );
    assert!(near(el.base.o, Vec3::new(8.0, 8.0, -8.0)));
    assert!(doc.props.general.restrict_rotations);
}

#[test]
fn free_rotations_on_several_axes_are_kept_exactly_and_the_model_stops_restricting() {
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "free", "box_uv": false},
        "elements": [{
            "name": "Tilt", "from": [-4, 0, -4], "to": [4, 8, 4], "origin": [0, 4, 0],
            "rotation": [10, 20, 30], "faces": all_faces(json!(null)), "type": "cube"
        }],
        "outliner": ["x"],
    });
    let imported = import_json(&project);
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    let before = elements_of(&imported);
    // The rotation in the project is Rz * Ry * Rx; the model's Euler form is Ry * Rx * Rz in
    // its mirrored space.
    let project_rotation = Mat3::from_rotation_z(30f32.to_radians())
        * Mat3::from_rotation_y(20f32.to_radians())
        * Mat3::from_rotation_x(10f32.to_radians());
    let mirror = Mat3::from_diagonal(Vec3::new(1.0, 1.0, -1.0));
    let expected = mirror * project_rotation * mirror;
    let got = Mat3::from_quat(cuboid_num::euler_to_quat(before[0].base.e));
    assert!(got.abs_diff_eq(expected, 1e-3), "{:?}", before[0].base.e);
    let doc = open(imported);
    assert!(!doc.props.general.restrict_rotations);
    // Applying the file does not requantise the angles.
    assert_eq!(doc.elements.items()[0].base.e, before[0].base.e);
}

#[test]
fn a_rotation_off_the_grid_is_kept() {
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "free"},
        "elements": [{
            "name": "Odd", "from": [0, 0, 0], "to": [8, 8, 8], "origin": [0, 0, 0],
            "rotation": [12, 0, 0], "faces": {}, "type": "cube"
        }],
    });
    let imported = import_json(&project);
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    let doc = open(imported);
    // The Euler form of a rotation by -12 degrees about X, neither on the 5 nor on the 22.5
    // degree grid.
    assert_eq!(doc.elements.items()[0].base.e, Vec3::new(348.0, 0.0, 0.0));
    assert!(!doc.props.general.restrict_rotations);
}

#[test]
fn groups_become_real_groups_and_their_transforms_are_baked_into_the_elements() {
    // Free format: the origin is the middle of the floor, so the model's coordinates are the
    // project's plus (8, 0, 8).
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "free"},
        "elements": [
            {"name": "hand", "from": [1, 2, 3], "to": [3, 4, 5], "origin": [2, 3, 4],
             "faces": {}, "type": "cube", "uuid": "c1"},
            {"name": "loose", "from": [0, 0, 0], "to": [1, 1, 1], "faces": {},
             "type": "cube", "uuid": "c2"}
        ],
        "groups": [
            {"name": "arm", "origin": [0, 10, 0], "rotation": [0, 0, 90], "uuid": "g1",
             "children": []},
            {"name": "forearm", "origin": [4, 10, 0], "rotation": [0, 45, 0], "uuid": "g2",
             "children": []}
        ],
        "outliner": [
            {"uuid": "g1", "isOpen": true, "children": [
                {"uuid": "g2", "children": ["c1"]}
            ]},
            "c2"
        ],
    });
    let imported = import_json(&project);
    assert!(
        !imported
            .notices
            .iter()
            .any(|n| matches!(n, Notice::Skipped { .. }))
    );
    let els = elements_of(&imported);
    assert_eq!(els.len(), 2);
    assert_eq!(&*els[0].base.label, "hand");
    assert_eq!(&*els[1].base.label, "loose");

    // The tree: arm holds forearm, which holds hand; the loose cube follows arm.
    let doc = open(import_json(&project));
    let [arm, forearm] = doc.groups.ids() else {
        panic!("two groups");
    };
    assert_eq!(&*doc.groups.get(*arm).unwrap().name, "arm");
    assert_eq!(doc.groups.parent_of(ObjRef::Group(*forearm)), Some(*arm));
    let hand = ObjRef::Element(doc.elements.ids()[0]);
    assert_eq!(doc.groups.parent_of(hand), Some(*forearm));
    assert_eq!(
        doc.groups.parent_of(ObjRef::Element(doc.elements.ids()[1])),
        None
    );
    assert_eq!(
        doc.root_nodes(),
        [ObjRef::Group(*arm), ObjRef::Element(doc.elements.ids()[1])]
    );
    // The pivots are in world space (the model's, Z mirrored) and the rotations are the
    // local ones of the groups, in the Euler form of an element.
    let pivot_of = |id| doc.groups.get(id).unwrap().origin;
    assert!(
        near(pivot_of(*arm), Vec3::new(8.0, 10.0, -8.0)),
        "{:?}",
        pivot_of(*arm)
    );
    let forearm_pivot = Mat3::from_rotation_z(90f32.to_radians()) * Vec3::new(4.0, 0.0, 0.0)
        + Vec3::new(0.0, 10.0, 0.0);
    let world = Vec3::new(
        forearm_pivot.x + 8.0,
        forearm_pivot.y,
        forearm_pivot.z + 8.0,
    );
    assert!(
        near(pivot_of(*forearm), Vec3::new(world.x, world.y, -world.z)),
        "{:?} vs {world:?}",
        pivot_of(*forearm)
    );
    assert!(near(
        doc.groups.get(*arm).unwrap().rotation,
        Vec3::new(0.0, 0.0, 90.0)
    ));
    assert!(near(
        doc.groups.get(*forearm).unwrap().rotation,
        Vec3::new(0.0, 315.0, 0.0)
    ));

    // The centre of "hand" in the project: forearm turns about (4, 10, 0) by 45 degrees about Y,
    // then arm turns about (0, 10, 0) by 90 degrees about Z.
    let centre = Vec3::new(2.0, 3.0, 4.0);
    let step1 = Mat3::from_rotation_y(45f32.to_radians()) * (centre - Vec3::new(4.0, 10.0, 0.0))
        + Vec3::new(4.0, 10.0, 0.0);
    let step2 = Mat3::from_rotation_z(90f32.to_radians()) * (step1 - Vec3::new(0.0, 10.0, 0.0))
        + Vec3::new(0.0, 10.0, 0.0);
    let expected = step2 + Vec3::new(8.0, 0.0, 8.0);
    assert!(
        near(center_m(&els[0]), expected),
        "{:?} vs {expected:?}",
        center_m(&els[0])
    );
    // The loose element is only shifted.
    assert!(near(center_m(&els[1]), Vec3::new(8.5, 0.5, 8.5)));
}

#[test]
fn a_java_block_group_does_not_move_what_it_holds() {
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "java_block"},
        "elements": [{"name": "c", "from": [0, 0, 0], "to": [2, 2, 2], "faces": {},
            "type": "cube", "uuid": "c1"}],
        "groups": [{"name": "g", "origin": [8, 8, 8], "rotation": [0, 90, 0], "uuid": "g1",
            "children": []}],
        "outliner": [{"uuid": "g1", "children": ["c1"]}],
    });
    let imported = import_json(&project);
    let els = elements_of(&imported);
    assert_eq!(&*els[0].base.label, "c");
    assert!(near(center_m(&els[0]), Vec3::new(1.0, 1.0, 1.0)));
    assert_eq!(els[0].base.e, Vec3::ZERO);
    // The group is a folder: its rotation is not kept.
    let doc = open(imported);
    let group = doc.groups.get(doc.groups.ids()[0]).unwrap();
    assert_eq!(group.rotation, Vec3::ZERO);
    assert_eq!(group.origin, Vec3::new(8.0, 8.0, -8.0));
}

#[test]
fn a_hidden_group_hides_what_it_holds_by_its_own_flag() {
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "java_block"},
        "elements": [
            {"name": "in", "from": [0, 0, 0], "to": [1, 1, 1], "faces": {}, "uuid": "c1"},
            {"name": "own", "from": [0, 0, 0], "to": [1, 1, 1], "faces": {}, "uuid": "c2",
             "visibility": false, "locked": true},
            {"name": "out", "from": [0, 0, 0], "to": [1, 1, 1], "faces": {}, "uuid": "c3"}
        ],
        "groups": [{"name": "g", "visibility": false, "uuid": "g1", "children": []}],
        "outliner": [{"uuid": "g1", "children": ["c1"]}, "c2"],
    });
    let imported = import_json(&project);
    let els = elements_of(&imported);
    // The element no group lists comes last. The flag of the group is the group's own.
    let hidden: Vec<bool> = els.iter().map(|e| e.base.hidden).collect();
    assert_eq!(hidden, [false, true, false]);
    assert!(els[1].base.locked);
    let doc = open(imported);
    assert!(!doc.groups.get(doc.groups.ids()[0]).unwrap().visible);
    let ids = doc.elements.ids();
    assert!(
        doc.is_hidden(ObjRef::Element(ids[0])),
        "hidden through its group"
    );
    assert!(
        doc.is_hidden(ObjRef::Element(ids[1])),
        "hidden by its own flag"
    );
    assert!(!doc.is_hidden(ObjRef::Element(ids[2])));
}

#[test]
fn box_uv_lays_the_faces_out_in_the_texture() {
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "bedrock", "box_uv": true},
        "resolution": {"width": 64, "height": 32},
        "elements": [{
            "name": "head", "box_uv": true, "from": [-4, 24, -4], "to": [4, 32, 4],
            "uv_offset": [0, 0], "origin": [0, 24, 0],
            "faces": all_faces(json!(0)), "type": "cube"
        }],
        "textures": [{"name": "skin.png", "id": "0",
            "source": data_url(64, 32, [255, 255, 255, 255])}],
    });
    let imported = import_json(&project);
    let els = elements_of(&imported);
    // An 8 x 8 x 8 box: north is the square at (8, 8), east the one at (0, 8).
    let north = &els[0].faces[FaceName::North as usize].uv;
    assert_eq!(north.tl(), Vec2::new(8.0 / 64.0, 1.0 - 16.0 / 32.0));
    assert_eq!(north.br(), Vec2::new(16.0 / 64.0, 1.0 - 8.0 / 32.0));
    let east = &els[0].faces[FaceName::East as usize].uv;
    assert_eq!(east.tl(), Vec2::new(0.0, 0.5));
    assert_eq!(east.br(), Vec2::new(8.0 / 64.0, 0.75));
    // Up is above north and read backwards in u.
    let up = &els[0].faces[FaceName::Up as usize].uv;
    assert_eq!(up.tl(), Vec2::new(16.0 / 64.0, 1.0));
    assert_eq!(up.br(), Vec2::new(8.0 / 64.0, 1.0 - 8.0 / 32.0));
}

#[test]
fn per_texture_uv_sizes_are_used_by_the_free_format_only() {
    let texture = |uv: u32| {
        json!({"name": "a.png", "id": "a", "uv_width": uv, "uv_height": uv,
            "source": data_url(16, 16, [0, 0, 0, 255])})
    };
    let cube = json!({"name": "c", "from": [0, 0, 0], "to": [1, 1, 1], "type": "cube",
        "faces": {"north": {"uv": [0, 0, 32, 32], "texture": 0}}});
    let free = json!({
        "meta": {"format_version": "4.10", "model_format": "free"},
        "resolution": {"width": 16, "height": 16},
        "elements": [cube.clone()], "textures": [texture(32)],
    });
    let els = elements_of(&import_json(&free));
    let north = &els[0].faces[FaceName::North as usize].uv;
    assert_eq!((north.tl(), north.br()), (Vec2::ZERO, Vec2::ONE));

    // A Java block measures every face by the project, whatever the texture says.
    let mut block = free.clone();
    block["meta"]["model_format"] = json!("java_block");
    let els = elements_of(&import_json(&block));
    let north = &els[0].faces[FaceName::North as usize].uv;
    assert_eq!(north.tl_raw(), Vec2::new(0.0, -1.0));
    assert_eq!(north.br_raw(), Vec2::new(2.0, 1.0));
}

#[test]
fn the_project_resolution_sets_the_uv_unit() {
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "java_block"},
        "resolution": {"width": 64, "height": 32},
        "elements": [{"name": "c", "from": [0, 0, 0], "to": [1, 1, 1], "type": "cube",
            "faces": {"up": {"uv": [16, 8, 32, 16], "texture": null}}}],
    });
    let els = elements_of(&import_json(&project));
    let up = &els[0].faces[FaceName::Up as usize];
    assert_eq!(up.uv.tl(), Vec2::new(0.25, 0.5));
    assert_eq!(up.uv.br(), Vec2::new(0.5, 0.75));
}

#[test]
fn display_settings_become_slots() {
    let mut project = java(json!([]));
    project["display"] = json!({
        "gui": {"rotation": [30, 225, 0], "translation": [0, 3, 0], "scale": [0.5, 0.5, 0.5]},
        "fixed": {"scale": [2, 2, 2]},
        "on_shelf": {"rotation": [1, 2, 3]},
    });
    let imported = import_json(&project);
    assert!(
        imported.notices.contains(&Notice::Skipped {
            what: "display slots".to_owned(),
            count: 1
        }),
        "{:?}",
        imported.notices
    );
    let doc = open(imported);
    assert!(doc.props.minecraft.include_display);
    let gui = &doc.display[DisplaySlotId::Gui];
    assert!(gui.enabled);
    assert_eq!(gui.rotation, Vec3::new(30.0, 225.0, 0.0));
    assert_eq!(gui.translation, Vec3::new(0.0, 3.0, 0.0));
    assert_eq!(gui.scale, Vec3::splat(0.5));
    assert_eq!(doc.display[DisplaySlotId::Fixed].scale, Vec3::splat(2.0));
    assert!(!doc.display[DisplaySlotId::Head].enabled);
}

#[test]
fn other_element_types_are_skipped_with_one_notice_per_kind_and_controllers_are_kept() {
    let mut project = java(json!([
        {"name": "ok", "from": [0, 0, 0], "to": [1, 1, 1], "type": "cube", "faces": {}},
        {"name": "a", "type": "locator", "position": [0, 0, 0]},
        {"name": "b", "type": "locator", "position": [1, 0, 0]},
        {"name": "n", "type": "null_object", "position": [0, 0, 0]},
        {"name": "x", "type": "billboard"},
    ]));
    project["animation_controllers"] = json!([{"name": "walk"}, {"name": "idle"}]);
    let imported = import_json(&project);
    let skipped = |what: &str| {
        imported.notices.iter().find_map(|n| match n {
            Notice::Skipped { what: w, count } if w == what => Some(*count),
            _ => None,
        })
    };
    assert_eq!(skipped("locator"), Some(2));
    assert_eq!(skipped("null object"), Some(1));
    assert_eq!(skipped("billboard"), Some(1));
    assert_eq!(
        skipped("animation controllers"),
        None,
        "they are kept, not skipped"
    );
    assert!(imported.notices.contains(&Notice::Kept {
        what: "animation controllers".to_owned(),
        count: 2
    }));
    let doc = open(imported);
    assert_eq!(doc.elements.len(), 1);
    assert_eq!(
        cuboid_fmt_bbmodel::kept_animations(doc.retained.as_deref()),
        [("animation controllers".to_owned(), 2)]
    );
}

#[test]
fn the_model_takes_its_properties_from_the_project() {
    let mut project = java(json!([]));
    project["parent"] = json!("item/handheld");
    project["ambientocclusion"] = json!(false);
    project["front_gui_light"] = json!(true);
    project["credit"] = json!("by me");
    let doc = open(import_json(&project));
    assert_eq!(doc.props.general.name, "block");
    assert_eq!(doc.props.general.description, "by me");
    assert_eq!(doc.props.minecraft.parent, "item/handheld");
    assert_eq!(doc.props.minecraft.kind, cuboid_model::ModelKind::Item);
    assert!(!doc.props.minecraft.ambient_occlusion);
    assert_eq!(doc.props.minecraft.gui_light, cuboid_model::GuiLight::Front);

    // Without a name the file name is used.
    let mut nameless = java(json!([]));
    nameless.as_object_mut().unwrap().remove("name");
    assert_eq!(open(import_json(&nameless)).props.general.name, "Model");
}

#[test]
fn newer_and_older_format_versions() {
    let mut project = java(json!([]));
    project["meta"]["format_version"] = json!("5.1");
    let imported = import_json(&project);
    assert_eq!(imported.notices, [Notice::NewerVersion("5.1".to_owned())]);
    project["meta"]["format_version"] = json!("5.0");
    assert!(import_json(&project).notices.is_empty());

    // Before 4.5 a box UV cube that does not shade is mirrored.
    let cube = |shade: bool| {
        json!({"name": "c", "from": [0, 0, 0], "to": [2, 2, 2], "box_uv": true,
            "uv_offset": [0, 0], "shade": shade, "type": "cube", "faces": all_faces(json!(null))})
    };
    let old = json!({
        "meta": {"format_version": "4.0", "model_format": "bedrock", "box_uv": true},
        "resolution": {"width": 16, "height": 16},
        "elements": [cube(false)],
    });
    let mut new = old.clone();
    new["meta"]["format_version"] = json!("4.5");
    let north_old = elements_of(&import_json(&old))[0].faces[FaceName::North as usize].uv;
    let north_new = elements_of(&import_json(&new))[0].faces[FaceName::North as usize].uv;
    assert_ne!(north_old.tl(), north_new.tl());
}

#[test]
fn an_inflated_cube_grows_on_every_side() {
    let project = java(
        json!([{"name": "c", "from": [4, 4, 4], "to": [8, 8, 8], "inflate": 1,
        "type": "cube", "faces": {}}]),
    );
    let els = elements_of(&import_json(&project));
    assert_eq!(els[0].base.s, Vec3::splat(6.0));
    assert_eq!(els[0].base.p, Vec3::new(3.0, 3.0, -3.0));
}

#[test]
fn an_inverted_box_keeps_its_negative_size_through_the_apply_path() {
    let project = java(
        json!([{"name": "c", "from": [8, 0, 0], "to": [0, 8, 8], "type": "cube",
        "faces": {}}]),
    );
    let imported = import_json(&project);
    let els = elements_of(&imported);
    assert_eq!(els[0].base.s, Vec3::new(-8.0, 8.0, 8.0));
    assert_eq!(els[0].base.p, Vec3::new(8.0, 0.0, 0.0));
    // `open` applies the file and runs `check_loaded_invariants` on the result.
    let doc = open(imported);
    assert_eq!(doc.elements.items()[0].base.s, Vec3::new(-8.0, 8.0, 8.0));
}

#[test]
fn the_box_uv_of_an_inverted_cube_follows_its_negative_size() {
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "bedrock", "box_uv": true},
        "resolution": {"width": 64, "height": 32},
        "elements": [{
            "name": "in", "box_uv": true, "from": [4, 8, 4], "to": [-4, 0, -4],
            "uv_offset": [0, 0], "origin": [0, 0, 0], "faces": all_faces(json!(null)),
            "type": "cube"
        }],
    });
    let els = elements_of(&import_json(&project));
    // Sizes of -8: north is the rectangle from (-8, -8) to (-16, -16).
    let north = &els[0].faces[FaceName::North as usize].uv;
    assert_eq!(north.tl_raw(), Vec2::new(-8.0 / 64.0, 1.0 + 16.0 / 32.0));
    assert_eq!(north.br_raw(), Vec2::new(-16.0 / 64.0, 1.0 + 8.0 / 32.0));
}

#[test]
fn faces_may_name_a_texture_by_uuid_and_unknown_indices_leave_it_open() {
    let mut project = java(json!([{
        "name": "c", "from": [0, 0, 0], "to": [1, 1, 1], "type": "cube",
        "faces": {"north": {"uv": [0, 0, 1, 1], "texture": "t0"},
                  "south": {"uv": [0, 0, 1, 1], "texture": 99},
                  "up": {"uv": [0, 0, 1, 1]}}
    }]));
    project["textures"][0]["id"] = json!("side");
    let els = elements_of(&import_json(&project));
    assert_eq!(&*els[0].faces[FaceName::North as usize].texture, "side");
    // A face without a texture gets the default label of the list.
    assert_eq!(&*els[0].faces[FaceName::South as usize].texture, "side");
    assert!(els[0].faces[FaceName::South as usize].active);
}

#[test]
fn texture_labels_are_cleaned_and_unique() {
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "java_block"},
        "textures": [
            {"name": "a b.png", "id": "same!", "source": data_url(2, 2, [1, 2, 3, 255])},
            {"name": "c.png", "id": "same", "source": data_url(2, 2, [1, 2, 3, 255])},
            {"name": "", "id": "", "source": data_url(2, 2, [1, 2, 3, 255])}
        ],
    });
    let doc = open(import_json(&project));
    let labels: Vec<&str> = doc.textures.entries.iter().map(|t| &*t.label).collect();
    assert_eq!(labels, ["same", "same_1", "texture2"]);
}

#[test]
fn a_file_next_to_the_project_is_read_when_nothing_is_embedded() {
    use std::cell::RefCell;
    struct Files(RefCell<Vec<String>>);
    impl cuboid_fmt_bbmodel::TextureFiles for Files {
        fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, cuboid_io::texture_files::Refusal> {
            self.0
                .borrow_mut()
                .push(path.to_string_lossy().replace('\\', "/"));
            if path.ends_with("tex/a.png") {
                Ok(Some(common::png_of(&common::picture(4, 4, [9, 9, 9, 255]))))
            } else {
                Ok(None)
            }
        }
    }
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "java_block"},
        "textures": [{"name": "a.png", "id": "a", "relative_path": "tex/a.png",
            "path": "C:/gone/a.png"}],
    });
    let files = Files(RefCell::new(Vec::new()));
    let imported = import(
        &common::text_of(&project),
        Path::new("work/m.bbmodel"),
        &files,
    )
    .unwrap();
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    assert_eq!(
        files.0.borrow().first().map(String::as_str),
        Some("work/tex/a.png")
    );
    let doc = open(imported);
    assert_eq!(doc.textures.entries[0].texture.image.w, 4);
    assert_eq!(
        doc.textures.entries[0].texture.source.path.as_deref(),
        Some("work/tex/a.png")
    );
}

#[test]
fn a_quad_and_a_triangle_make_a_mesh() {
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "free"},
        "textures": [{"name": "a.png", "id": "a", "uv_width": 16, "uv_height": 16,
            "source": data_url(16, 16, [1, 2, 3, 255])}],
        "elements": [{
            "name": "tent", "type": "mesh", "origin": [0, 2, 0], "rotation": [0, 90, 0],
            "vertices": {
                "a": [0, 0, 0], "b": [4, 0, 0], "c": [4, 0, 4], "d": [0, 0, 4], "e": [2, 4, 2]
            },
            "faces": {
                "f1": {"vertices": ["a", "b", "c", "d"], "texture": 0,
                       "uv": {"a": [0, 0], "b": [8, 0], "c": [8, 8], "d": [0, 8]}},
                "f2": {"vertices": ["a", "b", "e"], "texture": 0,
                       "uv": {"a": [0, 0], "b": [16, 0], "e": [8, 16]}}
            }
        }],
    });
    let imported = import_json(&project);
    assert!(imported.notices.is_empty(), "{:?}", imported.notices);
    let doc = open(imported);
    assert_eq!(doc.meshes.len(), 1);
    let mesh = &*doc.meshes.items()[0];
    assert_eq!(&*mesh.base.label, "tent");
    // 4 + 3 vertices, 2 + 1 triangles.
    assert_eq!(mesh.geometry.vertices.len(), 7);
    assert_eq!(mesh.geometry.triangles.len(), 9);
    assert_eq!(mesh.geometry.uvs.len(), 7);
    // The uv of the second vertex of the first face is (8/16, 1 - 0).
    assert_eq!(mesh.geometry.uvs[1], Vec2::new(0.5, 1.0));
    // Mirrored space: z is negated, so "c" = (4, 0, 4) is stored at z = -4.
    assert_eq!(mesh.geometry.vertices[2], Vec3::new(4.0, 0.0, -4.0));
    // The first triangle's winding is swapped by the mirror.
    assert_eq!(&mesh.geometry.triangles[..3], &[0, 2, 1]);
    assert!(mesh.texture.is_some() && mesh.submeshes.is_empty());
    // The pivot sits at the mesh origin, shifted to the model's space.
    assert!(near(mesh.base.p, Vec3::new(8.0, 2.0, -8.0)));
    assert!(near(mesh.base.e, Vec3::new(0.0, 270.0, 0.0)));
    assert_eq!(mesh.base.s, Vec3::ONE);
}

#[test]
fn a_mesh_with_two_textures_gets_a_sub_mesh_for_each() {
    let tex = |name: &str| {
        json!({"name": name, "id": name, "uv_width": 16, "uv_height": 16,
            "source": data_url(2, 2, [5, 5, 5, 255])})
    };
    let project = json!({
        "meta": {"format_version": "4.10", "model_format": "free"},
        "textures": [tex("a"), tex("b")],
        "elements": [{
            "name": "m", "type": "mesh", "origin": [0, 0, 0],
            "vertices": {"a": [0, 0, 0], "b": [1, 0, 0], "c": [0, 1, 0]},
            "faces": {
                "f1": {"vertices": ["a", "b", "c"], "texture": 0, "uv": {}},
                "f2": {"vertices": ["a", "b", "c"], "texture": 1, "uv": {}},
                "f3": {"vertices": ["a", "b", "c"], "texture": 1, "uv": {}}
            }
        }],
    });
    let doc = open(import_json(&project));
    let mesh = &*doc.meshes.items()[0];
    assert_eq!(mesh.submeshes.len(), 2);
    assert_eq!(mesh.submeshes[0].triangles.len(), 3);
    assert_eq!(mesh.submeshes[1].triangles.len(), 6);
    assert_eq!(mesh.geometry.triangles.len(), 9);
}
