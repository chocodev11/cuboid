//! Generators of the built-in primitive meshes (spec: tools 6.5, 6.6, 6.8). The meshes are never
//! stored in a file; open, undo and paste rebuild them from the kind. They are deterministic and
//! shared: every mesh of a kind holds the same `Arc`.
//!
//! Mesh-local frame = internal space: the unit box x in [0,1], y in [0,1], z in [-1,0]. Every
//! triangle is front facing by `(v1 - v0) x (v2 - v0)` along its stored outward normal. The
//! torus tangent of the original is not kept (`MeshGeometry` has none).

use std::f32::consts::{FRAC_1_SQRT_2, PI, TAU};
use std::sync::{Arc, OnceLock};

use cuboid_model::{MeshGeometry, MeshKind};
use glam::{Vec2, Vec3};

/// The most vertices a generated mesh may have (the engine of Cubik limits a mesh to 65 000).
pub const MAX_VERTICES: usize = 65_000;

/// Segments of the cylinder (tools 6.6).
pub const CYLINDER_SEGMENTS: u32 = 32;
/// Segments of the cone (tools 6.6).
pub const CONE_SEGMENTS: u32 = 64;
/// Grid cells around the tube of the torus (tools 6.6).
pub const TORUS_TUBE: u32 = 32;
/// Grid cells around the ring of the torus (tools 6.6).
pub const TORUS_RING: u32 = 64;
/// Quads along one edge of a sphere patch (tools 6.8).
pub const SPHERE_CELLS: u32 = 16;

/// The radius of the sphere (tools 6.5).
const SPHERE_RADIUS: f32 = 0.498_416;
const CENTER: Vec3 = Vec3::new(0.5, 0.5, -0.5);

/// A corner as `[x, y, z, u, v]`.
type Corner = [f32; 5];

const DOWN: Vec3 = Vec3::new(0.0, -1.0, 0.0);
const NORTH: Vec3 = Vec3::new(0.0, 0.0, 1.0);
const SOUTH: Vec3 = Vec3::new(0.0, 0.0, -1.0);
const WEST: Vec3 = Vec3::new(-1.0, 0.0, 0.0);
const EAST: Vec3 = Vec3::new(1.0, 0.0, 0.0);
const UP: Vec3 = Vec3::new(0.0, 1.0, 0.0);
/// The sloped faces of the corners and the slope.
const SLOPE_X: Vec3 = Vec3::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2, 0.0);
const SLOPE_Z: Vec3 = Vec3::new(0.0, FRAC_1_SQRT_2, -FRAC_1_SQRT_2);
/// The sides of the pyramid and the cone: slope 1 over 0.5 gives `(2, 1) / sqrt(5)`.
const SIDE_LONG: f32 = 0.894_427_2;
const SIDE_SHORT: f32 = 0.447_213_6;

// The UV cells of the boxy primitives (tools 6.6, cube table).
const CUBE_DOWN: [Corner; 4] = [
    [1.0, 0.0, 0.0, 0.9364, 0.0637],
    [0.0, 0.0, -1.0, 0.6260, 0.3739],
    [1.0, 0.0, -1.0, 0.9364, 0.3739],
    [0.0, 0.0, 0.0, 0.6260, 0.0637],
];
const CUBE_UP: [Corner; 4] = [
    [1.0, 1.0, 0.0, 0.0005, 0.6883],
    [0.0, 1.0, -1.0, 0.3117, 0.9996],
    [0.0, 1.0, 0.0, 0.3117, 0.6883],
    [1.0, 1.0, -1.0, 0.0005, 0.9996],
];
const CUBE_NORTH: [Corner; 4] = [
    [1.0, 0.0, 0.0, 0.0008, 0.3757],
    [0.0, 1.0, 0.0, 0.3117, 0.6870],
    [0.0, 0.0, 0.0, 0.3117, 0.3757],
    [1.0, 1.0, 0.0, 0.0008, 0.6870],
];
const CUBE_WEST: [Corner; 4] = [
    [0.0, 0.0, 0.0, 0.3132, 0.3754],
    [0.0, 1.0, -1.0, 0.6245, 0.6870],
    [0.0, 0.0, -1.0, 0.6245, 0.3754],
    [0.0, 1.0, 0.0, 0.3132, 0.6870],
];
const CUBE_SOUTH: [Corner; 4] = [
    [0.0, 0.0, -1.0, 0.6257, 0.3758],
    [1.0, 1.0, -1.0, 0.9367, 0.6870],
    [1.0, 0.0, -1.0, 0.9367, 0.3758],
    [0.0, 1.0, -1.0, 0.6257, 0.6870],
];
const CUBE_EAST: [Corner; 4] = [
    [1.0, 0.0, -1.0, 0.0005, 0.0634],
    [1.0, 1.0, 0.0, 0.3114, 0.3746],
    [1.0, 0.0, 0.0, 0.3114, 0.0634],
    [1.0, 1.0, -1.0, 0.0005, 0.3746],
];

// Triangles that reuse the cube cells.
const NORTH_TRI: [Corner; 3] = [CUBE_NORTH[0], CUBE_NORTH[1], CUBE_NORTH[2]];
const SOUTH_TRI: [Corner; 3] = [
    [1.0, 0.0, -1.0, 0.9367, 0.3758],
    [0.0, 0.0, -1.0, 0.6257, 0.3758],
    [0.0, 1.0, -1.0, 0.6257, 0.6870],
];
const EAST_TRI: [Corner; 3] = [CUBE_EAST[0], CUBE_EAST[1], CUBE_EAST[2]];

/// The mesh under construction. Quads use the triangles `(0,1,2), (1,0,3)` of their own
/// corners, triangles `(0,1,2)`.
#[derive(Default)]
struct Builder {
    vertices: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    triangles: Vec<u32>,
}

impl Builder {
    fn vertex(&mut self, pos: Vec3, normal: Vec3, uv: Vec2) -> u32 {
        let at = u32::try_from(self.vertices.len()).unwrap_or(u32::MAX);
        self.vertices.push(pos);
        self.normals.push(normal);
        self.uvs.push(uv);
        at
    }

    fn corner(&mut self, normal: Vec3, c: &Corner) -> u32 {
        self.vertex(Vec3::new(c[0], c[1], c[2]), normal, Vec2::new(c[3], c[4]))
    }

    fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.triangles.extend([a, b, c]);
    }

    fn quad(&mut self, normal: Vec3, corners: &[Corner; 4]) {
        let [a, b, c, d] = corners.map(|corner| self.corner(normal, &corner));
        self.tri(a, b, c);
        self.tri(b, a, d);
    }

    fn triangle(&mut self, normal: Vec3, corners: &[Corner; 3]) {
        let [a, b, c] = corners.map(|corner| self.corner(normal, &corner));
        self.tri(a, b, c);
    }

    fn finish(self) -> MeshGeometry {
        MeshGeometry {
            vertices: self.vertices,
            normals: Some(self.normals),
            uvs: self.uvs,
            triangles: self.triangles,
        }
    }
}

/// The part of `x` after the decimal point.
fn frac(x: f32) -> f32 {
    x - x.floor()
}

fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// The label of a primitive kind, which is also the name of its paint texture.
///
/// spec: tools 6.2
pub fn label(kind: MeshKind) -> &'static str {
    match kind {
        MeshKind::Custom => "Mesh",
        MeshKind::Cube => "Cube",
        MeshKind::Sphere => "Sphere",
        MeshKind::Cylinder => "Cylinder",
        MeshKind::Cone => "Cone",
        MeshKind::Pyramid => "Pyramid",
        MeshKind::Slope => "Slope",
        MeshKind::CornerIn => "Corner-in",
        MeshKind::CornerOut => "Corner-out",
        MeshKind::Torus => "Torus",
        MeshKind::Sprite => "Sprite",
    }
}

/// The shared geometry of a primitive kind; `None` for an imported mesh.
///
/// spec: tools 6.5, tools 6.6
pub fn geometry(kind: MeshKind) -> Option<Arc<MeshGeometry>> {
    static CACHE: [OnceLock<Arc<MeshGeometry>>; 10] = [const { OnceLock::new() }; 10];
    let at = (kind as usize).checked_sub(1)?;
    let build = || {
        Arc::new(match kind {
            MeshKind::Custom | MeshKind::Cube => cube(),
            MeshKind::Sphere => sphere(SPHERE_CELLS),
            MeshKind::Cylinder => cylinder(CYLINDER_SEGMENTS),
            MeshKind::Cone => cone(CONE_SEGMENTS),
            MeshKind::Pyramid => pyramid(),
            MeshKind::Slope => slope(),
            MeshKind::CornerIn => corner_in(),
            MeshKind::CornerOut => corner_out(),
            MeshKind::Torus => torus(TORUS_TUBE, TORUS_RING),
            MeshKind::Sprite => sprite(),
        })
    };
    Some(Arc::clone(CACHE.get(at)?.get_or_init(build)))
}

fn cube() -> MeshGeometry {
    let mut b = Builder::default();
    b.quad(DOWN, &CUBE_DOWN);
    b.quad(UP, &CUBE_UP);
    b.quad(NORTH, &CUBE_NORTH);
    b.quad(WEST, &CUBE_WEST);
    b.quad(SOUTH, &CUBE_SOUTH);
    b.quad(EAST, &CUBE_EAST);
    b.finish()
}

fn corner_in() -> MeshGeometry {
    let mut b = Builder::default();
    b.quad(DOWN, &CUBE_DOWN);
    b.triangle(
        SLOPE_X,
        &[
            [1.0, 0.0, -1.0, 0.0005, 0.9996],
            [0.0, 1.0, -1.0, 0.3117, 0.9996],
            [0.0, 1.0, 0.0, 0.3117, 0.6883],
        ],
    );
    b.triangle(
        SLOPE_Z,
        &[
            [0.0, 1.0, 0.0, 0.3117, 0.6883],
            [1.0, 1.0, 0.0, 0.0005, 0.6883],
            [1.0, 0.0, -1.0, 0.0005, 0.9996],
        ],
    );
    b.quad(NORTH, &CUBE_NORTH);
    b.quad(WEST, &CUBE_WEST);
    b.triangle(SOUTH, &SOUTH_TRI);
    b.triangle(EAST, &EAST_TRI);
    b.finish()
}

fn corner_out() -> MeshGeometry {
    let mut b = Builder::default();
    b.quad(DOWN, &CUBE_DOWN);
    b.triangle(
        SLOPE_X,
        &[
            [0.0, 1.0, 0.0, 0.3117, 0.6883],
            [1.0, 0.0, 0.0, 0.0005, 0.6883],
            [1.0, 0.0, -1.0, 0.0005, 0.9996],
        ],
    );
    b.triangle(NORTH, &NORTH_TRI);
    b.triangle(
        WEST,
        &[
            [0.0, 0.0, -1.0, 0.6245, 0.3754],
            [0.0, 0.0, 0.0, 0.3132, 0.3754],
            [0.0, 1.0, 0.0, 0.3132, 0.6870],
        ],
    );
    b.triangle(
        SLOPE_Z,
        &[
            [1.0, 0.0, -1.0, 0.9367, 0.3758],
            [0.0, 0.0, -1.0, 0.6257, 0.3758],
            [0.0, 1.0, 0.0, 0.6257, 0.6870],
        ],
    );
    b.finish()
}

fn slope() -> MeshGeometry {
    let mut b = Builder::default();
    b.quad(DOWN, &CUBE_DOWN);
    b.quad(
        SLOPE_X,
        &[
            [1.0, 0.0, 0.0, 0.0005, 0.6883],
            [0.0, 1.0, -1.0, 0.3117, 0.9996],
            [0.0, 1.0, 0.0, 0.3117, 0.6883],
            [1.0, 0.0, -1.0, 0.0005, 0.9996],
        ],
    );
    b.triangle(NORTH, &NORTH_TRI);
    b.quad(WEST, &CUBE_WEST);
    b.triangle(SOUTH, &SOUTH_TRI);
    b.finish()
}

fn pyramid() -> MeshGeometry {
    let apex = |u, v| [0.5, 1.0, -0.5, u, v];
    let sides: [(Vec3, [Corner; 3]); 4] = [
        (
            Vec3::new(0.0, SIDE_SHORT, SIDE_LONG),
            [
                apex(0.4684, 0.9982),
                [0.0, 0.0, 0.0, 0.6221, 0.6895],
                [1.0, 0.0, 0.0, 0.3146, 0.6895],
            ],
        ),
        (
            Vec3::new(-SIDE_LONG, SIDE_SHORT, 0.0),
            [
                apex(0.7817, 1.0),
                [0.0, 0.0, -1.0, 0.9367, 0.6887],
                [0.0, 0.0, 0.0, 0.6267, 0.6887],
            ],
        ),
        (
            Vec3::new(0.0, SIDE_SHORT, -SIDE_LONG),
            [
                apex(0.1563, 0.6852),
                [1.0, 0.0, -1.0, 0.3100, 0.3765],
                [0.0, 0.0, -1.0, 0.0025, 0.3765],
            ],
        ),
        (
            Vec3::new(SIDE_LONG, SIDE_SHORT, 0.0),
            [
                apex(0.1551, 1.0),
                [1.0, 0.0, 0.0, 0.3103, 0.6896],
                [1.0, 0.0, -1.0, 0.0, 0.6896],
            ],
        ),
    ];
    let mut b = Builder::default();
    for (normal, corners) in &sides {
        b.triangle(*normal, corners);
    }
    let base: [Corner; 5] = [
        [1.0, 0.0, 0.0, 0.6284, 0.6853],
        [0.0, 0.0, 0.0, 0.9356, 0.6853],
        [0.5, 0.0, -0.5, 0.7820, 0.5308],
        [0.0, 0.0, -1.0, 0.9356, 0.3764],
        [1.0, 0.0, -1.0, 0.6284, 0.3764],
    ];
    let v = base.map(|corner| b.corner(DOWN, &corner));
    for [i, j, k] in [[0, 1, 2], [1, 3, 2], [3, 4, 2], [4, 0, 2]] {
        b.tri(v[i], v[j], v[k]);
    }
    b.finish()
}

fn sprite() -> MeshGeometry {
    let mut b = Builder::default();
    b.quad(
        DOWN,
        &[
            [0.0, 0.0, -1.0, 0.0003, 0.0003],
            [1.0, 0.0, 0.0, 0.9995, 0.9998],
            [0.0, 0.0, 0.0, 0.0003, 0.9998],
            [1.0, 0.0, -1.0, 0.9995, 0.0003],
        ],
    );
    b.quad(
        UP,
        &[
            [0.0, 0.0, -1.0, 0.0003, 0.0003],
            [1.0, 0.0, 0.0, 0.9995, 0.9998],
            [1.0, 0.0, -1.0, 0.9995, 0.0003],
            [0.0, 0.0, 0.0, 0.0003, 0.9998],
        ],
    );
    b.finish()
}

/// The ring angle of point `k` of `n`: `pi + 2 pi k / n`.
fn ring_angle(k: f32, n: f32) -> f32 {
    PI + TAU * k / n
}

/// A point of a ring of the given radius around the axis through `(0.5, y, -0.5)`.
fn ring_point(y: f32, radius: f32, theta: f32) -> Vec3 {
    Vec3::new(0.5 + radius * theta.cos(), y, -0.5 + radius * theta.sin())
}

/// A UV on a round cap: `centre + radius (cos a, sin a)`.
fn cap_uv(centre: Vec2, radius: f32, angle: f32) -> Vec2 {
    centre + radius * Vec2::new(angle.cos(), angle.sin())
}

/// The cylinder along y with `segments` segments (32 by default), radius 0.5, height 1. The
/// segment count is limited to 3 up to 16 000, which keeps the vertex count under the cap.
///
/// spec: tools 6.6 (Cylinder)
pub fn cylinder(segments: u32) -> MeshGeometry {
    const BOTTOM: Vec2 = Vec2::new(0.1871, 0.1874);
    const TOP: Vec2 = Vec2::new(0.8122, 0.1885);
    let n = segments.clamp(3, 16_000);
    let nf = n as f32;
    let tilt = 230.51_f32.to_radians();
    let mut b = Builder::default();

    b.vertex(Vec3::new(0.5, 0.0, -0.5), DOWN, BOTTOM);
    for k in 0..n {
        let theta = ring_angle(k as f32, nf);
        let uv = cap_uv(BOTTOM, 0.1862, tilt - theta);
        b.vertex(ring_point(0.0, 0.5, theta), DOWN, uv);
    }
    let side = b.vertices.len();
    for i in 0..=n {
        let theta = ring_angle(i as f32, nf);
        let normal = Vec3::new(theta.cos(), 0.0, theta.sin());
        let u = frac(0.749 + 0.996_83 * i as f32 / nf);
        b.vertex(ring_point(0.0, 0.5, theta), normal, Vec2::new(u, 0.376_53));
        b.vertex(ring_point(1.0, 0.5, theta), normal, Vec2::new(u, 0.997_95));
    }
    let top_centre = b.vertices.len();
    b.vertex(Vec3::new(0.5, 1.0, -0.5), UP, TOP);
    for k in 0..n {
        let theta = ring_angle(k as f32, nf);
        let uv = cap_uv(TOP, 0.1849, tilt + theta);
        b.vertex(ring_point(1.0, 0.5, theta), UP, uv);
    }

    let n = n as usize;
    for k in 0..n {
        b.tri(0, index(1 + k), index(1 + (k + 1) % n));
    }
    for i in 0..n {
        let (lo, hi) = (index(side + 2 * i), index(side + 2 * i + 1));
        let (next_lo, next_hi) = (index(side + 2 * i + 2), index(side + 2 * i + 3));
        b.tri(lo, hi, next_lo);
        b.tri(next_lo, hi, next_hi);
    }
    for k in 0..n {
        let ring = top_centre + 1;
        b.tri(
            index(top_centre),
            index(ring + (k + 1) % n),
            index(ring + k),
        );
    }
    b.finish()
}

/// The cone on y = 0 with `segments` segments (64 by default), radius 0.5, apex at y = 1. The
/// segment count is limited to 3 up to 21 600.
///
/// spec: tools 6.6 (Cone)
pub fn cone(segments: u32) -> MeshGeometry {
    const BASE: Vec2 = Vec2::new(0.7816, 0.2180);
    let n = segments.clamp(3, 21_600);
    let nf = n as f32;
    let tilt = 109.80_f32.to_radians();
    let slope =
        |theta: f32| Vec3::new(SIDE_LONG * theta.cos(), SIDE_SHORT, SIDE_LONG * theta.sin());
    let mut b = Builder::default();

    b.vertex(Vec3::new(0.5, 0.0, -0.5), DOWN, BASE);
    for k in 0..n {
        let theta = ring_angle(k as f32, nf);
        let uv = cap_uv(BASE, 0.2174, tilt - theta);
        b.vertex(ring_point(0.0, 0.5, theta), DOWN, uv);
    }
    // The side: a ring point and the apex copy of its segment, one after the other.
    let side = b.vertices.len();
    for i in 0..=n {
        let theta = ring_angle(i as f32, nf);
        let u = frac(0.7651 + i as f32 / nf);
        b.vertex(
            ring_point(0.0, 0.5, theta),
            slope(theta),
            Vec2::new(u, 0.4385),
        );
        if i < n {
            let mid = ring_angle(i as f32 + 0.5, nf);
            let u = frac(0.7651 + (i + 1) as f32 / nf);
            b.vertex(Vec3::new(0.5, 1.0, -0.5), slope(mid), Vec2::new(u, 0.9991));
        }
    }

    let n = n as usize;
    for k in 0..n {
        b.tri(0, index(1 + k), index(1 + (k + 1) % n));
    }
    for i in 0..n {
        let ring = side + 2 * i;
        b.tri(index(ring), index(ring + 1), index(ring + 2));
    }
    b.finish()
}

/// The torus around the vertical axis through the centre of the unit box: `tube` grid cells
/// around the tube section (32 by default) and `ring` around the ring (64). The section is an
/// ellipse of radial half-width 0.15 and vertical half-height 0.5 around the major radius 0.35.
/// Both counts are limited to 3 and the vertex count to [`MAX_VERTICES`].
///
/// spec: tools 6.6 (Torus)
pub fn torus(tube: u32, ring: u32) -> MeshGeometry {
    let tube = tube.clamp(3, 250);
    let ring_max = u32::try_from(MAX_VERTICES / (tube as usize + 1) - 1).unwrap_or(3);
    let ring = ring.clamp(3, ring_max);
    let (tf, rf) = (tube as f32, ring as f32);
    let mut b = Builder::default();
    for i in 0..=tube {
        let phi = TAU * i as f32 / tf;
        let (sin_phi, cos_phi) = phi.sin_cos();
        let radius = 0.35 + 0.15 * cos_phi;
        for j in 0..=ring {
            let theta = -PI / 2.0 - TAU * j as f32 / rf;
            let (sin_t, cos_t) = theta.sin_cos();
            let pos = Vec3::new(
                0.5 + radius * cos_t,
                0.5 + 0.5 * sin_phi,
                -0.5 + radius * sin_t,
            );
            let normal = Vec3::new(0.5 * cos_phi * cos_t, 0.15 * sin_phi, 0.5 * cos_phi * sin_t)
                .normalize_or_zero();
            b.vertex(pos, normal, Vec2::new(i as f32 / tf, j as f32 / rf));
        }
    }
    let at = |i: u32, j: u32| i * (ring + 1) + j;
    for i in 0..tube {
        for j in 0..ring {
            let (p00, p10) = (at(i, j), at(i + 1, j));
            let (p01, p11) = (at(i, j + 1), at(i + 1, j + 1));
            b.tri(p00, p01, p10);
            b.tri(p10, p01, p11);
        }
    }
    b.finish()
}

/// A cube sphere: six patches of `cells` x `cells` quads (16 by default, limited to 1 up to
/// 100), the vertices of the patch edges duplicated, on a sphere of radius 0.498416 around the
/// centre of the unit box. The points are spread evenly by angle along each patch, and the
/// patches are laid out as a 3 x 2 grid in the UV square. This is the own layout of the port,
/// not the one of Cubik (spec: tools 6.8, point 3).
pub fn sphere(cells: u32) -> MeshGeometry {
    const U: [f32; 2] = [0.0004, 0.9369];
    const V: [f32; 2] = [0.0631, 0.9995];
    // The outward normal and the two edge directions of each patch, with `u x v = normal`.
    const PATCHES: [[Vec3; 3]; 6] = [
        [Vec3::X, Vec3::Y, Vec3::Z],
        [Vec3::NEG_X, Vec3::Z, Vec3::Y],
        [Vec3::Y, Vec3::Z, Vec3::X],
        [Vec3::NEG_Y, Vec3::X, Vec3::Z],
        [Vec3::Z, Vec3::X, Vec3::Y],
        [Vec3::NEG_Z, Vec3::Y, Vec3::X],
    ];
    let n = cells.clamp(1, 100);
    let nf = n as f32;
    let row = n + 1;
    let mut b = Builder::default();
    for (patch, [normal, du, dv]) in PATCHES.iter().enumerate() {
        let (col, line) = ((patch % 3) as f32, (patch / 3) as f32);
        for a in 0..=n {
            for c in 0..=n {
                let s = (a as f32 / nf * 2.0 - 1.0) * (PI / 4.0);
                let t = (c as f32 / nf * 2.0 - 1.0) * (PI / 4.0);
                let dir = (*normal + s.tan() * *du + t.tan() * *dv).normalize_or_zero();
                let uv = Vec2::new(
                    U[0] + (col + a as f32 / nf) * (U[1] - U[0]) / 3.0,
                    V[0] + (line + c as f32 / nf) * (V[1] - V[0]) / 2.0,
                );
                b.vertex(CENTER + SPHERE_RADIUS * dir, dir, uv);
            }
        }
        let first = index(patch) * row * row;
        for a in 0..n {
            for c in 0..n {
                let p00 = first + a * row + c;
                let (p10, p01, p11) = (p00 + row, p00 + 1, p00 + row + 1);
                b.tri(p00, p10, p01);
                b.tri(p10, p11, p01);
            }
        }
    }
    b.finish()
}

#[cfg(test)]
mod tests;
