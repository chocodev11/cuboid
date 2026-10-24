//! Model to project to model.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::{NoFiles, gradient, near, open, picture};
use cuboid_fmt_bbmodel::{Notice, export, import};
use cuboid_model::mc::{self, McBox, Rigid};
use cuboid_model::uv::UvRect;
use cuboid_model::{
    Blueprint, Document, Element, ElementFlags, ElementType, FaceFlags, FaceName, Label, Mesh,
    MeshBounds, MeshGeometry, MeshKind, ObjectBase, Perms, Submesh, Texture, TextureEntry,
    TextureList,
};
use cuboid_num::euler_to_quat;
use glam::{Vec2, Vec3};
use serde_json::Value;

fn entry(label: &str, mc_path: &str, image: Arc<cuboid_model::image::Image>) -> TextureEntry {
    let mut texture = Texture::new(image);
    texture.source.mc_path = mc_path.to_owned();
    TextureEntry {
        label: Label::from(label),
        texture,
    }
}

fn new_document() -> Document {
    Document::new(TextureList {
        entries: vec![
            entry("a", "block/a", gradient(4, 4)),
            entry("b", "item/b", picture(2, 2, [200, 100, 50, 255])),
        ],
    })
}

/// An element placed by the rules of the Minecraft mapping.
fn placed(label: &str, b: McBox) -> Element {
    let at = mc::place(&b, Rigid::IDENTITY);
    let mut el = Element::new(
        ElementType::Cube,
        &Blueprint::default(),
        0,
        [1.0, 0.55, 0.55, 1.0],
        &Label::from("a"),
    );
    el.base = ObjectBase::new(label, at.p, at.s, Perms::ELEMENT);
    el.base.o = at.o;
    el.base.e = at.e;
    el.flags = ElementFlags::SHADE;
    el
}

fn boxed(from: [f32; 3], to: [f32; 3], pivot: [f32; 3], rotation: [f32; 3]) -> McBox {
    McBox {
        from: Vec3::from_array(from),
        to: Vec3::from_array(to),
        pivot: Vec3::from_array(pivot),
        rotation: Vec3::from_array(rotation),
    }
}

/// Gives every face its own rectangle, texture and options.
fn dress(el: &mut Element) {
    for (i, face) in el.faces.iter_mut().enumerate() {
        let f = i as f32;
        face.active = true;
        face.texture = Label::from(if i % 2 == 0 { "a" } else { "b" });
        face.uv = UvRect::new(
            0,
            Vec2::new(0.0625 * f, 0.125),
            Vec2::new(0.0625 * f + 0.25, 0.625),
        );
    }
    el.faces[FaceName::North as usize].uv.set_rot(90);
    el.faces[FaceName::North as usize].tint = 2;
    el.faces[FaceName::East as usize].cull = FaceFlags::EAST;
    el.faces[FaceName::Down as usize].active = false;
}

fn export_project(doc: &Document) -> (Vec<u8>, Value) {
    let out = export(doc).expect("exports");
    let value: Value = serde_json::from_slice(&out.bytes).expect("the output is JSON");
    (out.bytes, value)
}

fn reopen(bytes: &[u8]) -> Document {
    let imported = import(bytes, Path::new("out/Model.bbmodel"), &NoFiles).expect("imports");
    open(imported)
}

fn same_element(a: &Element, b: &Element) {
    assert_eq!(a.base.label, b.base.label);
    assert!(near(a.base.p, b.base.p), "{:?} {:?}", a.base.p, b.base.p);
    assert!(near(a.base.s, b.base.s), "{:?} {:?}", a.base.s, b.base.s);
    assert!(near(a.base.o, b.base.o), "{:?} {:?}", a.base.o, b.base.o);
    assert!(near(a.base.e, b.base.e), "{:?} {:?}", a.base.e, b.base.e);
    assert_eq!(a.base.hidden, b.base.hidden);
    assert_eq!(a.base.locked, b.base.locked);
    assert_eq!(a.flags, b.flags);
    for (fa, fb) in a.faces.iter().zip(&b.faces) {
        assert_eq!(fa.name, fb.name);
        assert_eq!(fa.active, fb.active, "{:?}", fa.name);
        if !fa.active {
            continue;
        }
        assert_eq!(fa.texture, fb.texture, "{:?}", fa.name);
        assert_eq!(fa.tint, fb.tint);
        assert_eq!(fa.cull, fb.cull);
        assert_eq!(fa.uv.rot(), fb.uv.rot());
        assert!(
            fa.uv.tl().abs_diff_eq(fb.uv.tl(), 1e-5) && fa.uv.br().abs_diff_eq(fb.uv.br(), 1e-5),
            "{:?}: {:?} {:?} / {:?} {:?}",
            fa.name,
            fa.uv.tl(),
            fa.uv.br(),
            fb.uv.tl(),
            fb.uv.br()
        );
    }
}

fn same_textures(a: &Document, b: &Document) {
    assert_eq!(a.textures.entries.len(), b.textures.entries.len());
    for (ta, tb) in a.textures.entries.iter().zip(&b.textures.entries) {
        assert_eq!(ta.label, tb.label);
        assert_eq!(ta.texture.source.mc_path, tb.texture.source.mc_path);
        assert_eq!(
            (ta.texture.image.w, ta.texture.image.h),
            (tb.texture.image.w, tb.texture.image.h)
        );
        assert_eq!(ta.texture.image.rgba(), tb.texture.image.rgba());
    }
}

#[test]
fn a_model_that_follows_the_minecraft_rules_is_a_java_block_project() {
    let mut doc = new_document();
    let mut plain = placed(
        "plain",
        boxed([1.0, 2.0, 3.0], [5.0, 7.0, 9.0], [1.0, 2.0, 3.0], [0.0; 3]),
    );
    dress(&mut plain);
    plain.base.hidden = true;
    plain.flags.remove(ElementFlags::SHADE);
    let mut turned = placed(
        "turned",
        boxed(
            [2.0, 0.0, 2.0],
            [6.0, 4.0, 6.0],
            [4.0, 2.0, 4.0],
            [0.0, 22.5, 0.0],
        ),
    );
    dress(&mut turned);
    turned.flags.insert(ElementFlags::RESCALE);
    let mut tilted = placed(
        "tilted",
        boxed(
            [0.0, 0.0, 0.0],
            [16.0, 1.0, 16.0],
            [8.0, 0.0, 8.0],
            [-45.0, 0.0, 0.0],
        ),
    );
    dress(&mut tilted);
    tilted.base.locked = true;
    for el in [&plain, &turned, &tilted] {
        doc.elements.push(el.clone());
    }
    {
        let props = Arc::make_mut(&mut doc.props);
        props.general.name = "My block".to_owned();
        props.general.description = "A test".to_owned();
        props.minecraft.parent = "block/cube".to_owned();
        props.minecraft.ambient_occlusion = false;
        props.minecraft.gui_light = cuboid_model::GuiLight::Front;
    }

    let (bytes, project) = export_project(&doc);
    assert_eq!(project["meta"]["model_format"], "java_block");
    // Blockbench 4 reads at most 4.10.
    assert_eq!(project["meta"]["format_version"], "4.10");
    assert_eq!(project["elements"].as_array().unwrap().len(), 3);
    assert_eq!(project["outliner"].as_array().unwrap().len(), 3);
    assert_eq!(
        project["elements"][1]["rotation"],
        serde_json::json!([0.0, 22.5, 0.0])
    );
    assert_eq!(
        project["elements"][2]["rotation"],
        serde_json::json!([-45.0, 0.0, 0.0])
    );
    assert_eq!(
        project["elements"][0]["from"],
        serde_json::json!([1.0, 2.0, 3.0])
    );

    let back = reopen(&bytes);
    assert_eq!(back.elements.len(), 3);
    for (a, b) in doc.elements.items().iter().zip(back.elements.items()) {
        same_element(a, b);
    }
    same_textures(&doc, &back);
    assert_eq!(back.props.general.name, "My block");
    assert_eq!(back.props.general.description, "A test");
    assert_eq!(back.props.minecraft.parent, "block/cube");
    assert!(!back.props.minecraft.ambient_occlusion);
    assert_eq!(
        back.props.minecraft.gui_light,
        cuboid_model::GuiLight::Front
    );
    assert_eq!(project["front_gui_light"], true);
    assert!(back.props.general.restrict_rotations);
}

#[test]
fn free_rotations_and_far_elements_make_a_free_project() {
    let mut doc = new_document();
    let mut tilted = placed(
        "tilted",
        boxed(
            [-3.0, 0.0, -2.0],
            [3.0, 5.0, 2.0],
            [0.0, 2.0, 0.0],
            [10.0, 20.0, 30.0],
        ),
    );
    dress(&mut tilted);
    let mut far = placed(
        "far",
        boxed(
            [100.0, 0.0, 0.0],
            [104.0, 4.0, 4.0],
            [100.0, 0.0, 0.0],
            [0.0; 3],
        ),
    );
    dress(&mut far);
    doc.elements.push(tilted.clone());
    doc.elements.push(far.clone());

    let (bytes, project) = export_project(&doc);
    assert_eq!(project["meta"]["model_format"], "free");
    // The free format has its origin in the middle of the floor.
    assert_eq!(
        project["elements"][1]["from"],
        serde_json::json!([92.0, 0.0, -8.0])
    );

    let imported = import(&bytes, Path::new("m.bbmodel"), &NoFiles).unwrap();
    let back = common::elements_of(&imported);
    same_element(&tilted, &back[0]);
    same_element(&far, &back[1]);
}

#[test]
fn arbitrary_multi_axis_rotations_come_back_unchanged() {
    let mut doc = new_document();
    let angles = [
        [13.7, -48.2, 71.9],
        [53.0, 0.0, 0.0],
        [10.0, 20.0, 30.0],
        [-27.3, 81.5, 8.8],
        [0.0, 44.4, -63.1],
    ];
    for (i, rotation) in angles.into_iter().enumerate() {
        let mut el = placed(
            &format!("tilted {i}"),
            boxed(
                [-3.0, 0.0, -2.0 + i as f32],
                [3.0, 5.0, 2.0 + i as f32],
                [0.5, 2.0, 1.0],
                rotation,
            ),
        );
        dress(&mut el);
        doc.elements.push(el);
    }
    let (bytes, _) = export_project(&doc);
    let back = reopen(&bytes);
    assert_eq!(back.elements.len(), angles.len());
    for (a, b) in doc.elements.items().iter().zip(back.elements.items()) {
        same_element(a, b);
        // The angles are not on the 5 degree grid and the apply path leaves them alone: the
        // second trip changes nothing, bit for bit.
        assert!(near(a.base.e, b.base.e), "{:?} {:?}", a.base.e, b.base.e);
    }
    assert!(!back.props.general.restrict_rotations);
    let (again, _) = export_project(&back);
    let twice = reopen(&again);
    for (a, b) in back.elements.items().iter().zip(twice.elements.items()) {
        assert_eq!(a.base.e, b.base.e);
        assert_eq!(a.base.p, b.base.p);
    }
}

#[test]
fn a_project_exports_to_the_same_bytes_again() {
    let mut doc = new_document();
    let mut turned = placed(
        "turned",
        boxed(
            [2.0, 0.0, 2.0],
            [6.0, 4.0, 6.0],
            [4.0, 2.0, 4.0],
            [0.0, 0.0, 22.5],
        ),
    );
    dress(&mut turned);
    doc.elements.push(turned);
    let (first, _) = export_project(&doc);
    let (again, _) = export_project(&doc);
    assert_eq!(first, again);
    let back = reopen(&first);
    let (second, _) = export_project(&back);
    assert_eq!(
        String::from_utf8_lossy(&first),
        String::from_utf8_lossy(&second)
    );
}

#[test]
fn display_slots_survive() {
    let mut doc = new_document();
    {
        let display = Arc::make_mut(&mut doc.display);
        display.slots[4].enabled = true;
        display.slots[4].rotation = Vec3::new(30.0, 225.0, 0.0);
        display.slots[4].scale = Vec3::splat(0.625);
        display.slots[7].enabled = true;
        display.slots[7].translation = Vec3::new(1.0, 2.0, -3.0);
        // Enabled with the default values: not written, comes back disabled (interop 7.7).
        display.slots[1].enabled = true;
    }
    let (bytes, project) = export_project(&doc);
    assert_eq!(project["display"].as_object().unwrap().len(), 2);
    let back = reopen(&bytes);
    for (a, b) in doc
        .display
        .slots
        .iter()
        .zip(&back.display.slots)
        .enumerate()
        .filter(|(i, _)| *i != 1)
        .map(|(_, p)| p)
    {
        assert_eq!(a.enabled, b.enabled);
        assert_eq!(
            (a.rotation, a.translation, a.scale),
            (b.rotation, b.translation, b.scale)
        );
    }
    assert!(!back.display.slots[1].enabled);
    assert!(back.props.minecraft.include_display);
}

fn world_triangles(mesh: &Mesh) -> Vec<[[f32; 5]; 3]> {
    let rot = euler_to_quat(mesh.base.e);
    let at = |i: u32| {
        let v = mesh.geometry.vertices[i as usize] * mesh.base.s;
        let w = mesh.base.p + rot * v;
        let uv = mesh
            .geometry
            .uvs
            .get(i as usize)
            .copied()
            .unwrap_or(Vec2::ZERO);
        [w.x, w.y, w.z, uv.x, uv.y]
    };
    let mut out = Vec::new();
    let runs: Vec<&[u32]> = if mesh.submeshes.is_empty() {
        vec![&mesh.geometry.triangles]
    } else {
        mesh.submeshes
            .iter()
            .map(|s| s.triangles.as_slice())
            .collect()
    };
    for run in runs {
        for t in run.chunks_exact(3) {
            out.push([at(t[0]), at(t[1]), at(t[2])]);
        }
    }
    out
}

#[test]
fn a_mesh_keeps_its_triangles_and_textures() {
    let mut doc = new_document();
    let geometry = MeshGeometry {
        vertices: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(2.0, 2.0, -1.0),
            Vec3::new(0.0, 2.0, -1.0),
        ],
        normals: None,
        uvs: vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ],
        triangles: vec![0, 1, 2, 0, 2, 3, 3, 2, 1],
    };
    let mut base = ObjectBase::new(
        "roof",
        Vec3::new(5.0, 1.0, -2.0),
        Vec3::new(2.0, 1.5, 2.0),
        Perms::IMPORTED_MESH,
    );
    base.e = Vec3::new(0.0, 90.0, 0.0);
    base.o = base.p;
    let main = doc.textures.entries[0].texture.clone();
    let second = doc.textures.entries[1].texture.clone();
    let mesh = Mesh {
        base,
        kind: MeshKind::Custom,
        geometry: Arc::new(geometry),
        texture: Some(main),
        submeshes: vec![
            Submesh {
                triangles: vec![0, 1, 2, 0, 2, 3],
                color: [1.0; 4],
                texture: None,
            },
            Submesh {
                triangles: vec![3, 2, 1],
                color: [1.0; 4],
                texture: Some(second),
            },
        ]
        .into(),
        bounds: MeshBounds::default(),
        norm_factor: 1.0,
        import_offset: Vec3::ZERO,
        tint: [1.0; 4],
    };
    doc.meshes.push(mesh.clone());

    let (bytes, project) = export_project(&doc);
    assert_eq!(project["meta"]["model_format"], "free");
    assert_eq!(project["elements"][0]["type"], "mesh");
    let back = reopen(&bytes);
    assert_eq!(back.meshes.len(), 1);
    let imported = &*back.meshes.items()[0];
    assert_eq!(&*imported.base.label, "roof");
    assert_eq!(imported.submeshes.len(), 2);
    assert!(imported.submeshes[0].texture.is_some() && imported.submeshes[1].texture.is_some());

    let want = world_triangles(&mesh);
    let got = world_triangles(imported);
    assert_eq!(want.len(), got.len());
    for (w, g) in want.iter().zip(&got) {
        for (pw, pg) in w.iter().zip(g) {
            for (a, b) in pw.iter().zip(pg) {
                assert!((a - b).abs() < 2e-3, "{w:?} vs {g:?}");
            }
        }
    }
    same_textures(&doc, &back);
}

#[test]
fn a_texture_that_only_a_mesh_uses_is_added_to_the_list() {
    let mut doc = new_document();
    let geometry = MeshGeometry {
        vertices: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        normals: None,
        uvs: vec![Vec2::ZERO, Vec2::X, Vec2::Y],
        triangles: vec![0, 1, 2],
    };
    let mut mesh = Mesh {
        base: ObjectBase::new("m", Vec3::ZERO, Vec3::ONE, Perms::IMPORTED_MESH),
        kind: MeshKind::Custom,
        geometry: Arc::new(geometry),
        texture: Some(Texture::new(picture(8, 8, [1, 2, 3, 255]))),
        submeshes: Arc::from([]),
        bounds: MeshBounds::default(),
        norm_factor: 1.0,
        import_offset: Vec3::ZERO,
        tint: [1.0; 4],
    };
    mesh.texture.as_mut().unwrap().name = Label::from("Mesh Paint");
    doc.meshes.push(mesh);
    let (bytes, project) = export_project(&doc);
    assert_eq!(project["textures"].as_array().unwrap().len(), 3);
    let back = reopen(&bytes);
    assert_eq!(back.textures.entries.len(), 3);
    assert_eq!(back.textures.entries[2].texture.image.w, 8);
}

#[test]
fn what_the_project_cannot_hold_is_reported() {
    let mut doc = new_document();
    // A mesh without triangles.
    doc.meshes.push(Mesh {
        base: ObjectBase::new("empty", Vec3::ZERO, Vec3::ONE, Perms::PRIMITIVE),
        kind: MeshKind::Sphere,
        geometry: Arc::new(MeshGeometry::default()),
        texture: None,
        submeshes: Arc::from([]),
        bounds: MeshBounds::default(),
        norm_factor: 1.0,
        import_offset: Vec3::ZERO,
        tint: [1.0; 4],
    });
    let out = export(&doc).unwrap();
    assert_eq!(
        out.notices,
        [Notice::Skipped {
            what: "meshes without geometry".to_owned(),
            count: 1
        }]
    );
}

#[test]
fn inverted_cubes_and_mirrored_uvs_round_trip_exactly() {
    let mut doc = new_document();
    // One inverted axis, two inverted axes, and one inverted and turned.
    let mut one = placed(
        "one",
        boxed([8.0, 0.0, 0.0], [0.0, 8.0, 8.0], [8.0, 0.0, 0.0], [0.0; 3]),
    );
    let mut two = placed(
        "two",
        boxed([8.0, 8.0, 2.0], [2.0, 2.0, 6.0], [4.0, 4.0, 4.0], [0.0; 3]),
    );
    let mut turned = placed(
        "turned",
        boxed(
            [10.0, 4.0, 10.0],
            [4.0, 0.0, 14.0],
            [8.0, 2.0, 12.0],
            [0.0, 45.0, 0.0],
        ),
    );
    for el in [&mut one, &mut two, &mut turned] {
        dress(el);
        // A mirrored rectangle: the second corner lies before the first on both axes.
        el.faces[FaceName::South as usize].uv =
            UvRect::new(0, Vec2::new(0.75, 0.875), Vec2::new(0.25, 0.125));
        el.faces[FaceName::West as usize].uv =
            UvRect::new(90, Vec2::new(0.5, 0.25), Vec2::new(0.0, 0.75));
        doc.elements.push(el.clone());
    }
    assert!(one.base.s.x < 0.0 && two.base.s.x < 0.0 && two.base.s.y < 0.0);

    let (bytes, project) = export_project(&doc);
    assert_eq!(project["meta"]["model_format"], "java_block");
    assert_eq!(
        project["elements"][0]["from"],
        serde_json::json!([8.0, 0.0, 0.0])
    );
    assert_eq!(
        project["elements"][0]["to"],
        serde_json::json!([0.0, 8.0, 8.0])
    );
    assert_eq!(
        project["elements"][1]["from"],
        serde_json::json!([8.0, 8.0, 2.0])
    );
    assert_eq!(
        project["elements"][1]["to"],
        serde_json::json!([2.0, 2.0, 6.0])
    );
    // The mirrored face keeps its corners in the order they were given.
    let south = &project["elements"][0]["faces"]["south"]["uv"];
    assert_eq!(south, &serde_json::json!([12.0, 14.0, 4.0, 2.0]));

    let back = reopen(&bytes);
    for (a, b) in doc.elements.items().iter().zip(back.elements.items()) {
        same_element(a, b);
        assert_eq!(a.base.s.signum(), b.base.s.signum());
    }
    let south = &back.elements.items()[0].faces[FaceName::South as usize].uv;
    assert!(south.tl().x > south.br().x && south.tl().y > south.br().y);

    // The free format writes the same boxes.
    let mut free = doc.clone();
    let mut tilted = placed(
        "tilted",
        boxed(
            [8.0, 0.0, 0.0],
            [0.0, 8.0, 8.0],
            [4.0, 4.0, 4.0],
            [10.0, 20.0, 30.0],
        ),
    );
    dress(&mut tilted);
    free.elements.push(tilted);
    let (bytes, project) = export_project(&free);
    assert_eq!(project["meta"]["model_format"], "free");
    // Before the apply path rounds the angles to the grid of the model.
    let imported = import(&bytes, Path::new("m.bbmodel"), &NoFiles).unwrap();
    let back = common::elements_of(&imported);
    for (a, b) in free.elements.items().iter().zip(&back) {
        same_element(a, b);
    }
}
