use std::collections::BTreeSet;

use cuboid_model::{MeshGeometry, MeshKind};
use glam::{Vec2, Vec3};

use super::*;

const S: f32 = FRAC_1_SQRT_2;

const KINDS: [MeshKind; 10] = [
    MeshKind::Cube,
    MeshKind::Sphere,
    MeshKind::Cylinder,
    MeshKind::Cone,
    MeshKind::Pyramid,
    MeshKind::Slope,
    MeshKind::CornerIn,
    MeshKind::CornerOut,
    MeshKind::Torus,
    MeshKind::Sprite,
];

fn near(a: Vec3, b: [f32; 3]) -> bool {
    a.abs_diff_eq(Vec3::from_array(b), 1e-4)
}

fn near_uv(a: Vec2, b: [f32; 2]) -> bool {
    a.abs_diff_eq(Vec2::from_array(b), 1e-3)
}

fn unique_positions(g: &MeshGeometry) -> usize {
    g.vertices
        .iter()
        .map(|v| v.to_array().map(|c| ((c * 1e4).round() + 0.0).to_bits()))
        .collect::<BTreeSet<_>>()
        .len()
}

fn normals(g: &MeshGeometry) -> &[Vec3] {
    g.normals.as_deref().unwrap_or_default()
}

fn check_sane(g: &MeshGeometry) {
    let n = g.vertices.len();
    assert!(n <= MAX_VERTICES);
    assert_eq!(normals(g).len(), n);
    assert_eq!(g.uvs.len(), n);
    assert_eq!(g.triangles.len() % 3, 0);
    assert!(g.triangles.iter().all(|&i| (i as usize) < n));
}

#[test]
fn counts_follow_the_conventions_table() {
    // (kind, vertices, unique positions, triangles); the sphere and torus unique counts are
    // those of the table.
    let table = [
        (MeshKind::Cube, 24, 8, 12),
        (MeshKind::Sphere, 1734, 1538, 3072),
        (MeshKind::Cylinder, 132, 66, 128),
        (MeshKind::Cone, 194, 66, 128),
        (MeshKind::Pyramid, 17, 6, 8),
        (MeshKind::Slope, 18, 6, 8),
        (MeshKind::CornerIn, 24, 7, 10),
        (MeshKind::CornerOut, 16, 5, 6),
        (MeshKind::Torus, 2145, 2048, 4096),
        (MeshKind::Sprite, 8, 4, 4),
    ];
    for (kind, vertices, unique, triangles) in table {
        let g = geometry(kind).expect("a primitive");
        check_sane(&g);
        assert_eq!(g.vertices.len(), vertices, "{kind:?} vertices");
        assert_eq!(g.triangles.len(), triangles * 3, "{kind:?} triangles");
        if !matches!(kind, MeshKind::Sphere) {
            assert_eq!(unique_positions(&g), unique, "{kind:?} unique positions");
        }
    }
}

#[test]
fn an_imported_mesh_has_no_generator() {
    assert!(geometry(MeshKind::Custom).is_none());
}

#[test]
fn a_kind_shares_one_geometry() {
    let a = geometry(MeshKind::Torus).expect("a primitive");
    let b = geometry(MeshKind::Torus).expect("a primitive");
    assert!(Arc::ptr_eq(&a, &b));
}

#[test]
fn every_triangle_faces_along_its_stored_normal() {
    for kind in KINDS {
        let g = geometry(kind).expect("a primitive");
        let normals = normals(&g);
        for tri in g.triangles.chunks_exact(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| g.vertices[i as usize]);
            let face = (b - a).cross(c - a);
            // The poles of the torus section and degenerate sprite-like cells have no area.
            if face.length() < 1e-9 {
                continue;
            }
            let stored = tri.iter().map(|&i| normals[i as usize]).sum::<Vec3>();
            assert!(face.dot(stored) > 0.0, "{kind:?} triangle {tri:?}");
        }
    }
}

#[test]
fn stored_normals_are_unit_length() {
    for kind in KINDS {
        let g = geometry(kind).expect("a primitive");
        for n in normals(&g) {
            assert!((n.length() - 1.0).abs() < 1e-3, "{kind:?} {n}");
        }
    }
}

#[test]
fn positions_stay_in_the_unit_box_and_uvs_in_the_texture() {
    for kind in KINDS {
        let g = geometry(kind).expect("a primitive");
        for v in &g.vertices {
            assert!((-1e-5..=1.0 + 1e-5).contains(&v.x), "{kind:?} {v}");
            assert!((-1e-5..=1.0 + 1e-5).contains(&v.y), "{kind:?} {v}");
            assert!((-1.0 - 1e-5..=1e-5).contains(&v.z), "{kind:?} {v}");
        }
        for uv in &g.uvs {
            assert!((0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y));
        }
    }
}

#[test]
fn generation_is_deterministic() {
    assert_eq!(cylinder(32), cylinder(32));
    assert_eq!(sphere(16), sphere(16));
    assert_eq!(torus(32, 64), torus(32, 64));
}

#[test]
fn the_cube_follows_the_face_table() {
    let g = cube();
    // Faces in the order down, up, north, west, south, east, four corners each.
    assert!(near(g.vertices[0], [1.0, 0.0, 0.0]));
    assert!(near_uv(g.uvs[0], [0.9364, 0.0637]));
    assert!(near(g.vertices[5], [0.0, 1.0, -1.0]));
    assert!(near_uv(g.uvs[5], [0.3117, 0.9996]));
    assert!(near(normals(&g)[8], [0.0, 0.0, 1.0]));
    assert!(near(g.vertices[23], [1.0, 1.0, -1.0]));
    assert!(near_uv(g.uvs[23], [0.0005, 0.3746]));
    // A quad uses (0,1,2) and (1,0,3) of its own corners.
    assert_eq!(&g.triangles[..6], &[0, 1, 2, 1, 0, 3]);
    assert_eq!(&g.triangles[6..12], &[4, 5, 6, 5, 4, 7]);
}

#[test]
fn the_pyramid_base_is_a_fan_around_the_centre() {
    let g = pyramid();
    // Twelve side vertices, then the five of the base.
    assert!(near(g.vertices[0], [0.5, 1.0, -0.5]));
    assert!(near(g.vertices[14], [0.5, 0.0, -0.5]));
    assert!(near_uv(g.uvs[14], [0.7820, 0.5308]));
    assert_eq!(
        &g.triangles[12..],
        &[12, 13, 14, 13, 15, 14, 15, 16, 14, 16, 12, 14]
    );
    assert!(near(normals(&g)[0], [0.0, 0.4472, 0.8944]));
}

#[test]
fn the_corners_and_the_slope_keep_their_mesh_order() {
    let corner_in = corner_in();
    // down quad, two sloped triangles, north and west quads, south and east triangles
    assert!(near(normals(&corner_in)[4], [S, S, 0.0]));
    assert!(near(normals(&corner_in)[7], [0.0, S, -S]));
    assert!(near(normals(&corner_in)[10], [0.0, 0.0, 1.0]));
    assert!(near(normals(&corner_in)[18], [0.0, 0.0, -1.0]));
    assert!(near(normals(&corner_in)[21], [1.0, 0.0, 0.0]));
    let slope = slope();
    assert!(near(normals(&slope)[4], [S, S, 0.0]));
    assert!(near(normals(&slope)[8], [0.0, 0.0, 1.0]));
    assert!(near(normals(&slope)[11], [-1.0, 0.0, 0.0]));
    assert!(near(normals(&slope)[15], [0.0, 0.0, -1.0]));
}

#[test]
fn a_three_segment_cylinder_is_hand_computable() {
    let g = cylinder(3);
    check_sane(&g);
    // 3 + 1 bottom, 2 * 4 side, 3 + 1 top.
    assert_eq!(g.vertices.len(), 16);
    assert_eq!(g.triangles.len(), 12 * 3);
    assert!(near(g.vertices[0], [0.5, 0.0, -0.5]));
    // Ring point 0 at theta = pi is (0, y, -0.5); point 1 at 300 degrees is (0.75, y, -0.9330).
    assert!(near(g.vertices[1], [0.0, 0.0, -0.5]));
    assert!(near(g.vertices[2], [0.75, 0.0, -0.9330]));
    // a = 230.51 - 300 degrees = -69.49 degrees: (0.1871, 0.1874) + 0.1862 (cos a, sin a).
    assert!(near_uv(g.uvs[2], [0.2523, 0.0130]));
    // The side pairs start at 4: bottom then top of the same angle, normal (cos, 0, sin).
    assert!(near(g.vertices[4], [0.0, 0.0, -0.5]));
    assert!(near(g.vertices[5], [0.0, 1.0, -0.5]));
    assert!(near(normals(&g)[4], [-1.0, 0.0, 0.0]));
    assert!(near_uv(g.uvs[4], [0.749, 0.37653]));
    assert!(near_uv(g.uvs[5], [0.749, 0.99795]));
    // u = frac(0.749 + 0.99683 / 3) at pair 1.
    assert!(near_uv(g.uvs[6], [0.0813, 0.37653]));
    // The seam pair repeats the first angle.
    assert!(near(g.vertices[10], [0.0, 0.0, -0.5]));
    assert!(near(g.vertices[11], [0.0, 1.0, -0.5]));
    // Top centre is 12, the top ring follows.
    assert!(near(g.vertices[12], [0.5, 1.0, -0.5]));
    assert!(near_uv(g.uvs[12], [0.8122, 0.1885]));
    assert_eq!(&g.triangles[..3], &[0, 1, 2]);
    assert_eq!(&g.triangles[6..9], &[0, 3, 1]);
    assert_eq!(&g.triangles[9..15], &[4, 5, 6, 6, 5, 7]);
    assert_eq!(&g.triangles[27..30], &[12, 14, 13]);
    assert_eq!(&g.triangles[33..36], &[12, 13, 15]);
}

#[test]
fn a_four_segment_cone_is_hand_computable() {
    let g = cone(4);
    check_sane(&g);
    // 1 + 4 base, 5 ring points and 4 apex copies on the side.
    assert_eq!(g.vertices.len(), 14);
    assert_eq!(g.triangles.len(), 8 * 3);
    assert!(near(g.vertices[0], [0.5, 0.0, -0.5]));
    assert!(near_uv(g.uvs[0], [0.7816, 0.2180]));
    // Ring point 1 at 270 degrees is (0.5, 0, -1).
    assert!(near(g.vertices[2], [0.5, 0.0, -1.0]));
    // Side: ring 0 at index 5, apex 0 at 6, ring 1 at 7.
    assert!(near(g.vertices[5], [0.0, 0.0, -0.5]));
    assert!(near_uv(g.uvs[5], [0.7651, 0.4385]));
    assert!(near(g.vertices[6], [0.5, 1.0, -0.5]));
    assert!(near_uv(g.uvs[6], [0.0151, 0.9991]));
    assert!(near_uv(g.uvs[7], [0.0151, 0.4385]));
    // The seam ring repeats the first point and the first u.
    assert!(near(g.vertices[13], [0.0, 0.0, -0.5]));
    assert!(near_uv(g.uvs[13], [0.7651, 0.4385]));
    // Ring 0 normal at theta = pi: (-0.894, 0.447, 0).
    assert!(near(normals(&g)[5], [-0.8944, 0.4472, 0.0]));
    assert_eq!(&g.triangles[..3], &[0, 1, 2]);
    assert_eq!(&g.triangles[9..12], &[0, 4, 1]);
    assert_eq!(&g.triangles[12..15], &[5, 6, 7]);
}

#[test]
fn a_small_torus_is_hand_computable() {
    let g = torus(4, 8);
    check_sane(&g);
    assert_eq!(g.vertices.len(), 5 * 9);
    assert_eq!(g.triangles.len(), 4 * 8 * 2 * 3);
    // i = 0, j = 0: phi = 0, theta = -90 degrees, R = 0.5 -> (0.5, 0.5, -1).
    assert!(near(g.vertices[0], [0.5, 0.5, -1.0]));
    assert!(near(normals(&g)[0], [0.0, 0.0, -1.0]));
    assert!(near_uv(g.uvs[0], [0.0, 0.0]));
    // i = 0, j = 2: theta = -180 degrees -> (0, 0.5, -0.5).
    assert!(near(g.vertices[2], [0.0, 0.5, -0.5]));
    // i = 1: phi = 90 degrees, R = 0.35, y = 1 -> (0.5, 1, -0.85), normal (0, 1, 0).
    assert!(near(g.vertices[9], [0.5, 1.0, -0.85]));
    assert!(near(normals(&g)[9], [0.0, 1.0, 0.0]));
    assert!(near_uv(g.uvs[9], [0.25, 0.0]));
    // i = 2: the inner equator, R = 0.2.
    assert!(near(g.vertices[18], [0.5, 0.5, -0.7]));
    assert!(near(normals(&g)[18], [0.0, 0.0, 1.0]));
    assert!(near_uv(g.uvs[26], [0.5, 1.0]));
}

#[test]
fn the_default_torus_has_the_table_section() {
    let g = geometry(MeshKind::Torus).expect("a primitive");
    // Grid index i * 65 + j.
    assert!(near(g.vertices[0], [0.5, 0.5, -1.0]));
    // Quarter of the tube: phi = 90 degrees.
    assert!(near(g.vertices[8 * 65], [0.5, 1.0, -0.85]));
}

#[test]
fn a_one_cell_sphere_has_the_cube_corners_on_the_sphere() {
    let g = sphere(1);
    check_sane(&g);
    assert_eq!(g.vertices.len(), 24);
    assert_eq!(g.triangles.len(), 12 * 3);
    // First patch (+X), corner (-1, -1): the direction (1, -1, -1) / sqrt 3.
    let r = SPHERE_RADIUS / 3.0_f32.sqrt();
    assert!(near(g.vertices[0], [0.5 + r, 0.5 - r, -0.5 - r]));
    assert!(near(normals(&g)[0], [0.5774, -0.5774, -0.5774]));
    assert!(near_uv(g.uvs[0], [0.0004, 0.0631]));
    // Last vertex of the last patch (-Z), corner (1, 1) along (Y, X): the direction (1, 1, -1).
    assert!(near(g.vertices[23], [0.5 + r, 0.5 + r, -0.5 - r]));
    assert!(near_uv(g.uvs[23], [0.9369, 0.9995]));
}

#[test]
fn the_default_sphere_lies_on_its_radius() {
    let g = geometry(MeshKind::Sphere).expect("a primitive");
    for v in &g.vertices {
        assert!(((*v - CENTER).length() - SPHERE_RADIUS).abs() < 1e-4);
    }
    // The middle of the +X patch faces +X.
    let middle = 8 * 17 + 8;
    assert!(near(g.vertices[middle], [0.5 + SPHERE_RADIUS, 0.5, -0.5]));
}

#[test]
fn huge_segment_counts_are_capped() {
    for g in [
        cylinder(u32::MAX),
        cone(u32::MAX),
        torus(u32::MAX, u32::MAX),
        torus(3, u32::MAX),
        torus(u32::MAX, 3),
        sphere(u32::MAX),
        cylinder(0),
        cone(0),
        torus(0, 0),
        sphere(0),
    ] {
        check_sane(&g);
        assert!(!g.triangles.is_empty());
    }
    assert!(cylinder(u32::MAX).vertices.len() > 60_000);
    assert!(torus(u32::MAX, u32::MAX).vertices.len() <= MAX_VERTICES);
}
