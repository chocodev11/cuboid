//! OBJ and glTF export of small documents, read back with a small OBJ reader and the `gltf`
//! crate.

use std::collections::BTreeSet;
use std::sync::Arc;

use cuboid_fmt_mesh::{
    GltfFormat, MeshError, MeshExport, MeshOptions, UnitScale, export_for_path, export_gltf,
    export_obj,
};
use cuboid_model::image::{Image, ImageFileKind, RgbaBuf, decode_file};
use cuboid_model::voxel::{I3, VoxelWorld, mapping};
use cuboid_model::{
    Blueprint, Document, Element, ElementFlags, ElementType, Label, Mesh, MeshBounds, MeshGeometry,
    MeshKind, ObjectBase, Perms, Texture, TextureEntry, TextureList,
};
use glam::{Vec2, Vec3};

fn picture(w: u32, h: u32, alpha: u8) -> Arc<Image> {
    let mut buf = RgbaBuf::new(w, h);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            buf.set(x, y, [(x * 40) as u8, (y * 40) as u8, 128, alpha]);
        }
    }
    Arc::new(Image::from_rgba(buf))
}

fn textures() -> TextureList {
    TextureList {
        entries: vec![TextureEntry {
            label: Label::from("texture"),
            texture: Texture::new(picture(4, 4, 255)),
        }],
    }
}

fn doc() -> Document {
    Document::new(textures())
}

/// A box with the corner `p`, size `s` and the white colour, in a 16-unit block grid.
fn cube(doc: &mut Document, name: &str, p: Vec3, s: Vec3) -> Element {
    let mut el = Element::new(
        ElementType::Cube,
        &Blueprint::default(),
        1,
        [1.0; 4],
        &Label::from("texture"),
    );
    el.base.label = Label::from(name);
    el.base.p = p;
    el.base.o = p;
    el.base.s = s;
    let _ = doc;
    el
}

fn world(cells: &[(I3, u32)]) -> VoxelWorld {
    let res = 16;
    let mut world = VoxelWorld::new(res, false, mapping::grid_box(res));
    for &(i, v) in cells {
        world.write_full(i, v);
    }
    world
}

const RED: u32 = 0xFFFF_0000;
const BLUE: u32 = 0xFF00_00FF;

type P = [f32; 3];

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: P, b: P) -> P {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: P, b: P) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: P) -> P {
    let l = dot(a, a).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A triangle with the position and the normal of each corner.
struct Tri {
    pos: [P; 3],
    normal: [P; 3],
}

impl Tri {
    /// The normal that the counter-clockwise winding gives.
    fn geometric(&self) -> P {
        unit(cross(
            sub(self.pos[1], self.pos[0]),
            sub(self.pos[2], self.pos[0]),
        ))
    }

    fn centroid(&self) -> P {
        let mut c = [0.0; 3];
        for p in self.pos {
            for (c, p) in c.iter_mut().zip(p) {
                *c += p / 3.0;
            }
        }
        c
    }
}

fn bounds(points: impl IntoIterator<Item = P>) -> (P, P) {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for p in points {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    (lo, hi)
}

fn near(a: P, b: P) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
}

/// A face corner: 0-based indices of the position, the texture coordinate and the normal.
type Corner = (usize, Option<usize>, usize);

/// What the small OBJ reader found.
#[derive(Default)]
struct Obj {
    positions: Vec<P>,
    colors: Vec<Option<P>>,
    normals: Vec<P>,
    uvs: Vec<[f32; 2]>,
    groups: Vec<String>,
    mtllib: Option<String>,
    /// Per face line: the material and the corners `(v, vt, vn)` with 0-based indices.
    faces: Vec<(String, Vec<Corner>)>,
}

impl Obj {
    fn parse(text: &str) -> Obj {
        assert!(!text.contains('\r'), "LF only");
        let mut obj = Obj::default();
        let mut material = String::new();
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let Some(tag) = words.next() else { continue };
            let rest: Vec<&str> = words.collect();
            let f = |s: &str| s.parse::<f32>().unwrap();
            match tag {
                "v" => {
                    obj.positions.push([f(rest[0]), f(rest[1]), f(rest[2])]);
                    obj.colors
                        .push((rest.len() == 6).then(|| [f(rest[3]), f(rest[4]), f(rest[5])]));
                }
                "vn" => obj.normals.push([f(rest[0]), f(rest[1]), f(rest[2])]),
                "vt" => obj.uvs.push([f(rest[0]), f(rest[1])]),
                "g" => obj.groups.push(rest[0].to_owned()),
                "mtllib" => obj.mtllib = Some(rest[0].to_owned()),
                "usemtl" => material = rest[0].to_owned(),
                "f" => {
                    let corners = rest
                        .iter()
                        .map(|c| {
                            let mut it = c.split('/');
                            let v: usize = it.next().unwrap().parse().unwrap();
                            let t = it.next().unwrap();
                            let n: usize = it.next().unwrap().parse().unwrap();
                            (v - 1, t.parse::<usize>().ok().map(|t| t - 1), n - 1)
                        })
                        .collect();
                    obj.faces.push((material.clone(), corners));
                }
                _ => {}
            }
        }
        obj
    }

    fn triangles(&self) -> Vec<Tri> {
        self.faces
            .iter()
            .map(|(_, c)| Tri {
                pos: std::array::from_fn(|i| self.positions[c[i].0]),
                normal: std::array::from_fn(|i| self.normals[c[i].2]),
            })
            .collect()
    }
}

fn text(export: &MeshExport, name: &str) -> String {
    let file = export.files.iter().find(|f| f.name == name).unwrap();
    String::from_utf8(file.bytes.clone()).unwrap()
}

fn obj(export: &MeshExport) -> Obj {
    Obj::parse(&text(export, &export.files[0].name))
}

fn names(export: &MeshExport) -> Vec<&str> {
    export.files.iter().map(|f| f.name.as_str()).collect()
}

/// Every triangle faces the way its vertex normals say.
fn assert_winding_matches_normals(tris: &[Tri]) {
    for t in tris {
        let g = t.geometric();
        for n in t.normal {
            assert!(dot(g, n) > 0.999, "winding {g:?} against normal {n:?}");
        }
    }
}

#[test]
fn a_cube_is_24_vertices_and_12_triangles_in_blocks() {
    let mut d = doc();
    let el = cube(&mut d, "Crate", Vec3::ZERO, Vec3::splat(16.0));
    d.elements.push(el);
    let export = export_obj(&d, "my model", &MeshOptions::default()).unwrap();
    assert_eq!(
        names(&export),
        ["my model.obj", "my_model.mtl", "my_model_texture.png"]
    );

    let o = obj(&export);
    assert_eq!(o.mtllib.as_deref(), Some("my_model.mtl"));
    assert_eq!(o.groups, ["Crate"]);
    assert_eq!(o.positions.len(), 24);
    assert_eq!(o.faces.len(), 12);
    // The unit box x 0..1, y 0..1, z -1..0 of the model, with X negated.
    let (lo, hi) = bounds(o.positions.iter().copied());
    assert!(
        near(lo, [-1.0, 0.0, -1.0]) && near(hi, [0.0, 1.0, 0.0]),
        "{lo:?} {hi:?}"
    );
    assert_eq!(o.normals.len(), 6, "equal normals share a record");

    let tris = o.triangles();
    assert_winding_matches_normals(&tris);
    for t in &tris {
        let outward = sub(t.centroid(), [-0.5, 0.5, -0.5]);
        assert!(
            dot(outward, t.normal[0]) > 0.4,
            "faces point away from the middle"
        );
    }
}

#[test]
fn units_can_stay_in_pixels() {
    let mut d = doc();
    let el = cube(&mut d, "Crate", Vec3::ZERO, Vec3::splat(16.0));
    d.elements.push(el);
    let opts = MeshOptions {
        unit_scale: UnitScale::Pixel,
        ..MeshOptions::default()
    };
    let o = obj(&export_obj(&d, "m", &opts).unwrap());
    let (lo, hi) = bounds(o.positions.iter().copied());
    assert!(near(lo, [-16.0, 0.0, -16.0]) && near(hi, [0.0, 16.0, 0.0]));
}

#[test]
fn the_model_origin_moves_the_export() {
    let mut d = doc();
    let el = cube(&mut d, "Crate", Vec3::ZERO, Vec3::splat(16.0));
    d.elements.push(el);
    std::sync::Arc::make_mut(&mut d.props).general.origin = Vec3::new(8.0, 0.0, -8.0);
    let o = obj(&export_obj(&d, "m", &MeshOptions::default()).unwrap());
    let (lo, hi) = bounds(o.positions.iter().copied());
    assert!(
        near(lo, [-0.5, 0.0, -0.5]) && near(hi, [0.5, 1.0, 0.5]),
        "{lo:?} {hi:?}"
    );
}

#[test]
fn an_inverted_cube_keeps_the_renderer_winding_and_looks_inward() {
    let mut d = doc();
    let mut el = cube(&mut d, "Room", Vec3::ZERO, Vec3::splat(16.0));
    el.flags |= ElementFlags::INVERT_NORMALS;
    d.elements.push(el);
    let o = obj(&export_obj(&d, "m", &MeshOptions::default()).unwrap());
    assert_eq!(o.faces.len(), 12);
    let tris = o.triangles();
    assert_winding_matches_normals(&tris);
    for t in &tris {
        let outward = sub(t.centroid(), [-0.5, 0.5, -0.5]);
        assert!(
            dot(outward, t.normal[0]) < -0.4,
            "faces point to the middle"
        );
    }
}

/// The export of the box `[0, 16]` x `[0, 16]` x `[-16, 0]` given by a position and a (possibly
/// negative) size, and whether its faces look at the middle of the box.
fn faces_look_inward(p: Vec3, s: Vec3, invert_normals: bool) -> bool {
    let mut d = doc();
    let mut el = cube(&mut d, "Room", p, s);
    el.flags.set(ElementFlags::INVERT_NORMALS, invert_normals);
    d.elements.push(el);
    let o = obj(&export_obj(&d, "m", &MeshOptions::default()).unwrap());
    let tris = o.triangles();
    assert_eq!(tris.len(), 12);
    assert_winding_matches_normals(&tris);
    let (lo, hi) = bounds(o.positions.iter().copied());
    assert!(
        near(lo, [-1.0, 0.0, -1.0]) && near(hi, [0.0, 1.0, 0.0]),
        "{lo:?} {hi:?}"
    );
    let inward = tris
        .iter()
        .filter(|t| dot(sub(t.centroid(), [-0.5, 0.5, -0.5]), t.normal[0]) < -0.4)
        .count();
    let outward = tris
        .iter()
        .filter(|t| dot(sub(t.centroid(), [-0.5, 0.5, -0.5]), t.normal[0]) > 0.4)
        .count();
    assert_eq!(
        inward + outward,
        12,
        "every face looks one way or the other"
    );
    assert!(inward == 12 || outward == 12, "all faces agree");
    inward == 12
}

#[test]
fn a_negative_size_exports_the_winding_the_renderer_draws() {
    // The renderer mirrors the box with the sign, which reverses the winding of every face: one
    // negative component shows the inside of the box.
    let one = (Vec3::new(16.0, 0.0, 0.0), Vec3::new(-16.0, 16.0, 16.0));
    assert!(faces_look_inward(one.0, one.1, false));
    // Each of the three axes alike.
    assert!(faces_look_inward(
        Vec3::new(0.0, 16.0, 0.0),
        Vec3::new(16.0, -16.0, 16.0),
        false
    ));
    assert!(faces_look_inward(
        Vec3::new(0.0, 0.0, -16.0),
        Vec3::new(16.0, 16.0, -16.0),
        false
    ));
    // Two mirrors cancel and the box looks outward again.
    assert!(!faces_look_inward(
        Vec3::new(16.0, 0.0, -16.0),
        Vec3::new(-16.0, 16.0, -16.0),
        false
    ));
    // Three mirrors invert once more.
    assert!(faces_look_inward(
        Vec3::new(16.0, 16.0, -16.0),
        Vec3::new(-16.0, -16.0, -16.0),
        false
    ));
    // Inverted normals reverse the winding too: together with one mirror they cancel.
    assert!(!faces_look_inward(one.0, one.1, true));
    assert!(faces_look_inward(Vec3::ZERO, Vec3::splat(16.0), true));
}

#[test]
fn a_rotated_box_turns_about_its_corner() {
    let mut d = doc();
    let mut el = cube(&mut d, "Turned", Vec3::ZERO, Vec3::splat(16.0));
    el.base.e = Vec3::new(0.0, 90.0, 0.0);
    d.elements.push(el);
    let o = obj(&export_obj(&d, "m", &MeshOptions::default()).unwrap());
    let (lo, hi) = bounds(o.positions.iter().copied());
    // The model's +X edge lies on -Z and its -Z edge on -X; the export negates X.
    assert!(
        near(lo, [0.0, 0.0, -1.0]) && near(hi, [1.0, 1.0, 0.0]),
        "{lo:?} {hi:?}"
    );
    assert_winding_matches_normals(&o.triangles());
}

#[test]
fn faces_carry_their_uv_rectangle_and_the_texture_becomes_a_material() {
    let mut d = doc();
    let mut el = cube(&mut d, "Crate", Vec3::ZERO, Vec3::splat(16.0));
    el.color = [1.0, 0.5, 0.0, 1.0];
    for (i, face) in el.faces.iter_mut().enumerate() {
        face.active = i == 1;
        face.uv.set(Vec2::new(0.25, 0.25), Vec2::new(0.75, 0.5));
    }
    d.elements.push(el);
    let export = export_obj(&d, "m", &MeshOptions::default()).unwrap();
    let o = obj(&export);
    assert_eq!(o.positions.len(), 4);
    let corners: BTreeSet<String> = o.uvs.iter().map(|uv| format!("{uv:?}")).collect();
    let expect: BTreeSet<String> = [[0.25, 0.25], [0.75, 0.25], [0.75, 0.5], [0.25, 0.5]]
        .iter()
        .map(|uv| format!("{uv:?}"))
        .collect();
    assert_eq!(corners, expect);

    let mtl = text(&export, "m.mtl");
    assert!(mtl.contains("newmtl texture_ff8000\n"), "{mtl}");
    assert!(mtl.contains("Kd 1 0.5019608 0\n"), "{mtl}");
    assert!(mtl.contains("map_Kd m_texture.png\n"), "{mtl}");
    assert!(!mtl.contains("map_d"), "an opaque texture has no alpha map");
    assert_eq!(o.faces[0].0, "texture_ff8000");

    let png = &export.files[2].bytes;
    let image = decode_file(png, ImageFileKind::Other).unwrap();
    assert_eq!((image.w, image.h), (4, 4));
    assert_eq!(
        image.rgba(),
        picture(4, 4, 255).rgba(),
        "pixels survive the PNG"
    );
}

#[test]
fn hidden_objects_are_skipped_unless_asked() {
    let mut d = doc();
    let a = cube(&mut d, "Seen", Vec3::ZERO, Vec3::splat(16.0));
    let mut b = cube(
        &mut d,
        "Hidden",
        Vec3::new(32.0, 0.0, 0.0),
        Vec3::splat(16.0),
    );
    b.base.hidden = true;
    d.elements.push(a);
    d.elements.push(b);
    let o = obj(&export_obj(&d, "m", &MeshOptions::default()).unwrap());
    assert_eq!(o.groups, ["Seen"]);
    let opts = MeshOptions {
        skip_hidden: false,
        ..MeshOptions::default()
    };
    let o = obj(&export_obj(&d, "m", &opts).unwrap());
    assert_eq!(o.groups, ["Seen", "Hidden"]);

    // All hidden: nothing to write.
    let mut d = doc();
    let mut b = cube(&mut d, "Hidden", Vec3::ZERO, Vec3::splat(16.0));
    b.base.hidden = true;
    d.elements.push(b);
    assert!(matches!(
        export_obj(&d, "m", &MeshOptions::default()),
        Err(MeshError::Empty)
    ));
}

#[test]
fn merged_objects_are_one_group_and_names_stay_one_token() {
    let mut d = doc();
    let a = cube(&mut d, "Left arm", Vec3::ZERO, Vec3::splat(16.0));
    let b = cube(
        &mut d,
        "Right",
        Vec3::new(32.0, 0.0, 0.0),
        Vec3::splat(16.0),
    );
    d.elements.push(a);
    d.elements.push(b);
    let o = obj(&export_obj(&d, "m", &MeshOptions::default()).unwrap());
    assert_eq!(o.groups, ["Left_arm", "Right"]);
    let opts = MeshOptions {
        merge_objects: true,
        ..MeshOptions::default()
    };
    let o = obj(&export_obj(&d, "m", &opts).unwrap());
    assert_eq!(o.groups, ["Cubik_Model"]);
    assert_eq!(o.faces.len(), 24);
}

#[test]
fn a_voxel_world_becomes_coloured_quads() {
    let mut d = doc();
    let mut cells = Vec::new();
    for i in 0..2 {
        for j in 0..2 {
            for k in 0..2 {
                cells.push((I3::new(16 + i, 16 + j, 16 + k), RED));
            }
        }
    }
    cells.push((I3::new(30, 16, 16), BLUE));
    d.voxels = Some(world(&cells));

    let merged = export_obj(&d, "m", &MeshOptions::default()).unwrap();
    let o = obj(&merged);
    assert_eq!(o.groups, ["Voxels"]);
    // A 2 x 2 x 2 block is six rectangles, the lone voxel six faces.
    assert_eq!(o.faces.len(), 2 * (6 + 6));
    assert_eq!(o.positions.len(), 12 * 4);
    assert!(o.uvs.is_empty() && o.faces.iter().all(|(_, c)| c.iter().all(|c| c.1.is_none())));
    assert!(o.colors.iter().all(|c| c.is_some()));
    let reds = o
        .colors
        .iter()
        .flatten()
        .filter(|c| **c == [1.0, 0.0, 0.0])
        .count();
    assert_eq!(reds, 24, "{:?}", o.colors);
    assert_winding_matches_normals(&o.triangles());

    // The block spans two cells of one unit: 1/8 block, starting at the grid cell 16.
    let red = o
        .positions
        .iter()
        .zip(&o.colors)
        .filter(|(_, c)| **c == Some([1.0, 0.0, 0.0]))
        .map(|(p, _)| *p);
    let (lo, hi) = bounds(red);
    let size = sub(hi, lo);
    assert!(near(size, [0.125, 0.125, 0.125]), "{size:?}");

    let opts = MeshOptions {
        merge_voxel_faces: false,
        ..MeshOptions::default()
    };
    let plain = obj(&export_obj(&d, "m", &opts).unwrap());
    assert_eq!(plain.faces.len(), 2 * (24 + 6));
    assert_winding_matches_normals(&plain.triangles());
    let (plo, phi) = bounds(plain.positions.iter().copied());
    let (mlo, mhi) = bounds(o.positions.iter().copied());
    assert!(
        near(plo, mlo) && near(phi, mhi),
        "merging keeps the outline"
    );
}

#[test]
fn voxels_with_ambient_occlusion_keep_their_shading() {
    let mut d = doc();
    let res = 16;
    let mut w = VoxelWorld::new(res, true, mapping::grid_box(res));
    for cell in [(16, 16, 16), (17, 16, 16), (16, 17, 16)] {
        w.write_full(I3::new(cell.0, cell.1, cell.2), RED);
    }
    d.voxels = Some(w);
    let o = obj(&export_obj(&d, "m", &MeshOptions::default()).unwrap());
    assert!(
        o.colors.iter().flatten().any(|c| c[0] < 1.0),
        "some vertex is darkened"
    );
    assert_winding_matches_normals(&o.triangles());
}

#[test]
fn a_mesh_is_exported_as_it_is() {
    let mut d = doc();
    let geometry = MeshGeometry {
        vertices: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
        ],
        normals: None,
        uvs: vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(1.0, 1.0),
        ],
        triangles: vec![0, 1, 2, 2, 1, 3, 0, 1, 99],
    };
    let mut base = ObjectBase::new(
        "Sheet",
        Vec3::new(16.0, 0.0, 0.0),
        Vec3::splat(16.0),
        Perms::IMPORTED_MESH,
    );
    base.e = Vec3::ZERO;
    d.meshes.push(Mesh {
        base,
        kind: MeshKind::Custom,
        geometry: Arc::new(geometry),
        texture: Some(Texture::new(picture(2, 2, 255))),
        submeshes: Arc::from([]),
        bounds: MeshBounds::default(),
        norm_factor: 1.0,
        import_offset: Vec3::ZERO,
        tint: [1.0; 4],
    });
    let export = export_obj(&d, "m", &MeshOptions::default()).unwrap();
    let o = obj(&export);
    assert_eq!(o.groups, ["Sheet"]);
    assert_eq!(o.positions.len(), 4, "the source vertices are kept");
    assert_eq!(o.faces.len(), 2, "the triangle with a bad index is dropped");
    let (lo, hi) = bounds(o.positions.iter().copied());
    assert!(
        near(lo, [-2.0, 0.0, 0.0]) && near(hi, [-1.0, 1.0, 0.0]),
        "{lo:?} {hi:?}"
    );
    assert_winding_matches_normals(&o.triangles());
    for t in o.triangles() {
        assert!(near(t.geometric(), [0.0, 0.0, 1.0]));
    }
    let mtl = text(&export, "m.mtl");
    assert!(
        mtl.contains("newmtl Sheet\n") && mtl.contains("map_Kd m_Sheet.png"),
        "{mtl}"
    );
}

#[test]
fn output_is_the_same_bytes_every_time() {
    let mut d = doc();
    let el = cube(
        &mut d,
        "Crate",
        Vec3::new(3.0, 1.0, -2.0),
        Vec3::new(5.0, 6.0, 7.0),
    );
    d.elements.push(el);
    d.voxels = Some(world(&[
        (I3::new(20, 20, 20), RED),
        (I3::new(21, 20, 20), RED),
        (I3::new(20, 21, 20), BLUE),
    ]));
    let opts = MeshOptions::default();
    assert_eq!(
        export_obj(&d, "m", &opts).unwrap(),
        export_obj(&d, "m", &opts).unwrap()
    );
    for format in [GltfFormat::Glb, GltfFormat::Separate] {
        assert_eq!(
            export_gltf(&d, "m", &opts, format).unwrap(),
            export_gltf(&d, "m", &opts, format).unwrap()
        );
    }
}

/// Reads a GLB back with the `gltf` crate.
fn read_glb(export: &MeshExport) -> (gltf::Gltf, Vec<u8>) {
    let gltf = gltf::Gltf::from_slice(&export.files[0].bytes).unwrap();
    let blob = gltf.blob.clone().unwrap();
    (gltf, blob)
}

#[test]
fn a_glb_holds_nodes_accessors_with_bounds_and_embedded_textures() {
    let mut d = doc();
    let el = cube(&mut d, "Crate", Vec3::ZERO, Vec3::splat(16.0));
    d.elements.push(el);
    d.voxels = Some(world(&[
        (I3::new(16, 16, 16), RED),
        (I3::new(17, 16, 16), RED),
    ]));
    let export = export_gltf(&d, "model", &MeshOptions::default(), GltfFormat::Glb).unwrap();
    assert_eq!(names(&export), ["model.glb"]);
    let (gltf, blob) = read_glb(&export);

    let nodes: Vec<_> = gltf.nodes().map(|n| n.name().unwrap().to_owned()).collect();
    assert_eq!(nodes, ["Crate", "Voxels"]);
    assert_eq!(gltf.meshes().count(), 2);

    let sampler = gltf.samplers().next().unwrap();
    assert_eq!(
        sampler.mag_filter(),
        Some(gltf::texture::MagFilter::Nearest)
    );
    assert_eq!(
        sampler.min_filter(),
        Some(gltf::texture::MinFilter::Nearest)
    );
    assert_eq!(gltf.textures().count(), 1);
    let image = gltf.images().next().unwrap();
    match image.source() {
        gltf::image::Source::View { view, mime_type } => {
            assert_eq!(mime_type, "image/png");
            let bytes = &blob[view.offset()..view.offset() + view.length()];
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        }
        gltf::image::Source::Uri { .. } => panic!("the texture is embedded"),
    }

    let read = |i: usize| {
        let mesh = gltf.meshes().nth(i).unwrap();
        let prim = mesh.primitives().next().unwrap();
        let reader = prim.reader(|b| (b.index() == 0).then_some(blob.as_slice()));
        let positions: Vec<P> = reader.read_positions().unwrap().collect();
        let normals: Vec<P> = reader.read_normals().unwrap().collect();
        let indices: Vec<u32> = reader.read_indices().unwrap().into_u32().collect();
        (prim, positions, normals, indices)
    };

    // The cube: bounds are the accessor's min and max, in blocks, a metre each.
    let (prim, positions, normals, indices) = read(0);
    let accessor = prim.get(&gltf::Semantic::Positions).unwrap();
    let (lo, hi) = bounds(positions.iter().copied());
    let min = accessor.min().unwrap();
    let max = accessor.max().unwrap();
    assert_eq!(min, serde_json::json!([lo[0], lo[1], lo[2]]));
    assert_eq!(max, serde_json::json!([hi[0], hi[1], hi[2]]));
    assert!(near(lo, [-1.0, 0.0, -1.0]) && near(hi, [0.0, 1.0, 0.0]));
    assert_eq!(positions.len(), 24);
    assert_eq!(indices.len(), 36);
    assert!(prim.get(&gltf::Semantic::TexCoords(0)).is_some());
    assert!(prim.get(&gltf::Semantic::Colors(0)).is_none());
    assert_eq!(
        prim.material()
            .pbr_metallic_roughness()
            .base_color_texture()
            .unwrap()
            .texture()
            .index(),
        0
    );
    let tris: Vec<Tri> = indices
        .chunks(3)
        .map(|t| Tri {
            pos: std::array::from_fn(|i| positions[t[i] as usize]),
            normal: std::array::from_fn(|i| normals[t[i] as usize]),
        })
        .collect();
    assert_winding_matches_normals(&tris);

    // Voxels: vertex colours in linear space, a white material without texture.
    let (prim, positions, ..) = read(1);
    assert_eq!(positions.len(), 24);
    assert!(prim.get(&gltf::Semantic::Colors(0)).is_some());
    assert!(prim.get(&gltf::Semantic::TexCoords(0)).is_none());
    assert!(
        prim.material()
            .pbr_metallic_roughness()
            .base_color_texture()
            .is_none()
    );
}

#[test]
fn texcoords_are_flipped_for_gltf() {
    let mut d = doc();
    let mut el = cube(&mut d, "Crate", Vec3::ZERO, Vec3::splat(16.0));
    for (i, face) in el.faces.iter_mut().enumerate() {
        face.active = i == 1;
        face.uv.set(Vec2::new(0.0, 0.0), Vec2::new(0.5, 0.25));
    }
    d.elements.push(el);
    let export = export_gltf(&d, "m", &MeshOptions::default(), GltfFormat::Glb).unwrap();
    let (gltf, blob) = read_glb(&export);
    let prim = gltf.meshes().next().unwrap().primitives().next().unwrap();
    let reader = prim.reader(|_| Some(blob.as_slice()));
    let uvs: BTreeSet<String> = reader
        .read_tex_coords(0)
        .unwrap()
        .into_f32()
        .map(|uv| format!("{uv:?}"))
        .collect();
    let expect: BTreeSet<String> = [[0.0, 1.0], [0.5, 1.0], [0.5, 0.75], [0.0, 0.75]]
        .iter()
        .map(|uv| format!("{uv:?}"))
        .collect();
    assert_eq!(uvs, expect);
}

#[test]
fn a_separate_gltf_points_at_its_files() {
    let mut d = doc();
    let el = cube(&mut d, "Crate", Vec3::ZERO, Vec3::splat(16.0));
    d.elements.push(el);
    let export = export_gltf(
        &d,
        "my model",
        &MeshOptions::default(),
        GltfFormat::Separate,
    )
    .unwrap();
    assert_eq!(
        names(&export),
        ["my model.gltf", "my_model.bin", "my_model_texture.png"]
    );
    let gltf = gltf::Gltf::from_slice(&export.files[0].bytes).unwrap();
    assert!(gltf.blob.is_none());
    let buffer = gltf.buffers().next().unwrap();
    match buffer.source() {
        gltf::buffer::Source::Uri(uri) => assert_eq!(uri, "my_model.bin"),
        gltf::buffer::Source::Bin => panic!("the buffer is a file"),
    }
    assert_eq!(buffer.length(), export.files[1].bytes.len());
    match gltf.images().next().unwrap().source() {
        gltf::image::Source::Uri { uri, .. } => assert_eq!(uri, "my_model_texture.png"),
        gltf::image::Source::View { .. } => panic!("the texture is a file"),
    }
}

#[test]
fn translucent_textures_set_the_blend_mode() {
    let mut d = Document::new(TextureList {
        entries: vec![
            TextureEntry {
                label: Label::from("cutout"),
                texture: Texture::new(picture(2, 2, 0)),
            },
            TextureEntry {
                label: Label::from("glass"),
                texture: Texture::new(picture(2, 2, 100)),
            },
        ],
    });
    for label in ["cutout", "glass"] {
        let mut el = cube(&mut d, label, Vec3::ZERO, Vec3::splat(16.0));
        for face in &mut el.faces {
            face.texture = Label::from(label);
        }
        d.elements.push(el);
    }
    let export = export_gltf(&d, "m", &MeshOptions::default(), GltfFormat::Glb).unwrap();
    let (gltf, _) = read_glb(&export);
    let modes: Vec<_> = gltf.materials().map(|m| m.alpha_mode()).collect();
    assert_eq!(
        modes,
        [
            gltf::material::AlphaMode::Mask,
            gltf::material::AlphaMode::Blend
        ]
    );
    let mtl = text(
        &export_obj(&d, "m", &MeshOptions::default()).unwrap(),
        "m.mtl",
    );
    assert_eq!(mtl.matches("map_d ").count(), 2);
}

#[test]
fn obj_and_gltf_hold_the_same_geometry() {
    let mut d = doc();
    let mut el = cube(
        &mut d,
        "Crate",
        Vec3::new(3.0, 1.0, -2.0),
        Vec3::new(5.0, 6.0, 7.0),
    );
    el.base.e = Vec3::new(10.0, 30.0, 50.0);
    d.elements.push(el);
    d.voxels = Some(world(&[(I3::new(20, 20, 20), RED)]));
    let opts = MeshOptions::default();
    let o = obj(&export_obj(&d, "m", &opts).unwrap());
    let (gltf, blob) = read_glb(&export_gltf(&d, "m", &opts, GltfFormat::Glb).unwrap());
    let key = |p: P| format!("{p:?}");
    let from_obj: BTreeSet<String> = o.positions.iter().copied().map(key).collect();
    let mut from_gltf = BTreeSet::new();
    for mesh in gltf.meshes() {
        for prim in mesh.primitives() {
            let reader = prim.reader(|_| Some(blob.as_slice()));
            from_gltf.extend(reader.read_positions().unwrap().map(key));
        }
    }
    assert_eq!(from_obj, from_gltf);
}

#[test]
fn the_extension_picks_the_format() {
    let mut d = doc();
    let el = cube(&mut d, "Crate", Vec3::ZERO, Vec3::splat(16.0));
    d.elements.push(el);
    let opts = MeshOptions::default();
    for (target, main) in [
        ("a/b.OBJ", "b.obj"),
        ("c.glb", "c.glb"),
        ("d.gltf", "d.gltf"),
    ] {
        let export = export_for_path(&d, std::path::Path::new(target), &opts).unwrap();
        assert!(export.files[0].name.eq_ignore_ascii_case(main), "{target}");
    }
    assert!(matches!(
        export_for_path(&d, std::path::Path::new("e.stl"), &opts),
        Err(MeshError::Extension(_))
    ));
    assert!(matches!(
        export_for_path(
            &Document::new(textures()),
            std::path::Path::new("e.glb"),
            &opts
        ),
        Err(MeshError::Empty)
    ));
}

#[test]
fn the_files_are_written_next_to_the_target() {
    let mut d = doc();
    let el = cube(&mut d, "Crate", Vec3::ZERO, Vec3::splat(16.0));
    d.elements.push(el);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("box.obj");
    export_for_path(&d, &target, &MeshOptions::default())
        .unwrap()
        .write(&target)
        .unwrap();
    for name in ["box.obj", "box.mtl", "box_texture.png"] {
        assert!(dir.path().join(name).is_file(), "{name}");
    }
}

#[test]
fn limits_stop_a_large_export() {
    let mut d = doc();
    let el = cube(&mut d, "Crate", Vec3::ZERO, Vec3::splat(16.0));
    d.elements.push(el);
    let mut opts = MeshOptions::default();
    opts.limits.max_vertices = 10;
    assert!(matches!(
        export_obj(&d, "m", &opts),
        Err(MeshError::Limit(_))
    ));
}

#[test]
fn bad_numbers_do_not_reach_the_files() {
    let mut d = doc();
    let good = cube(&mut d, "Good", Vec3::ZERO, Vec3::splat(16.0));
    let mut bad = cube(&mut d, "Bad", Vec3::ZERO, Vec3::splat(16.0));
    bad.base.p.x = f32::NAN;
    d.elements.push(good);
    d.elements.push(bad);
    let export = export_obj(&d, "m", &MeshOptions::default()).unwrap();
    assert_eq!(obj(&export).groups, ["Good"]);
    assert_eq!(export.notices.len(), 1);
}
