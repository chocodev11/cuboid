//! Guides, markers, hidden-element boxes, selection wireframes and gizmos (spec: app 6.5,
//! 8.8-8.10).
//!
//! Every overlay is drawn as coloured lines. The scene crate places each item with a transform
//! from the local geometry below to the world:
//!
//! - `Blueprint`, `HiddenBox` and `SelectionBox` use the unit box of an element frame: x in
//!   [0, 1], y in [0, 1], z in [-1, 0] (tools 1.3); `ModelBox` and `VoxelPointer` use the unit
//!   cube [0, 1] on all three axes, scaled onto a box of the world;
//! - `MainGrid` is in world space (the transform is applied as given): 1-pixel lines every 16
//!   units over the 48-unit square of the view's plane, chosen by the view index (app 6.4);
//! - `CenterMarker` is three unit squares through the origin, `ModelOrigin` an octahedron of
//!   radius 0.4 with axis stubs, `NorthPointer` an open pyramid with the apex at the origin and
//!   the mouth 0.5 along local +Z;
//! - `Gizmo(Move)` and `Gizmo(Rotate)` use the gizmo-local units of tools 9.7, `Gizmo(Scale)` the
//!   unit box with a pyramid on each face centre.
//!
//! `Guides` and `SubGrid` carry no geometry here: the guides have no data in the draw list, and
//! the sub-grid planes are drawn by the grid shader.

use cuboid_scene::{GizmoKind, OverlayItem, OverlayKind};
use glam::{Mat4, Vec3};

use crate::look;

/// One line vertex: position bits and an RGBA8 colour.
pub(crate) type LineVertex = [u32; 4];

/// Bytes of one line vertex.
pub(crate) const LINE_BYTES: u64 = 16;

/// The lines of one view: depth-tested ones first, then the ones that ignore depth.
#[derive(Debug, Default)]
pub(crate) struct Lines {
    pub tested: Vec<LineVertex>,
    pub over: Vec<LineVertex>,
}

fn pack(color: [f32; 4]) -> u32 {
    color
        .iter()
        .enumerate()
        .map(|(i, c)| crate::util::round_u32(c.clamp(0.0, 1.0) * 255.0) << (8 * i))
        .sum()
}

/// A line sink for one overlay item: applies the transform and picks the depth class.
struct Sink<'a> {
    to: &'a mut Vec<LineVertex>,
    m: Mat4,
}

impl Sink<'_> {
    fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 4]) {
        let c = pack(color);
        for p in [a, b] {
            let w = self.m.transform_point3(p);
            self.to
                .push([w.x.to_bits(), w.y.to_bits(), w.z.to_bits(), c]);
        }
    }

    /// A closed loop through the points.
    fn ring(&mut self, points: &[Vec3], color: [f32; 4]) {
        for (i, &p) in points.iter().enumerate() {
            self.line(p, points[(i + 1) % points.len()], color);
        }
    }

    /// The 12 edges of a unit box, plus one diagonal per face when asked. `z_up` is the cube
    /// [0, 1] in z, otherwise the element box [-1, 0].
    fn unit_box(&mut self, z_up: bool, inset: f32, diagonals: bool, color: [f32; 4]) {
        let (lo, hi) = (inset, 1.0 - inset);
        let corner = |i: usize| {
            let z = if i & 4 == 0 { lo } else { hi };
            Vec3::new(
                if i & 1 == 0 { lo } else { hi },
                if i & 2 == 0 { lo } else { hi },
                if z_up { z } else { -z },
            )
        };
        for i in 0..8usize {
            for bit in [1usize, 2, 4] {
                if i & bit == 0 {
                    self.line(corner(i), corner(i | bit), color);
                }
            }
        }
        if diagonals {
            // One diagonal on each of the six faces.
            for (a, b) in [(0, 3), (4, 7), (0, 5), (2, 7), (0, 6), (1, 7)] {
                self.line(corner(a), corner(b), color);
            }
        }
    }

    /// A square pyramid: apex at `apex`, a mouth of half size `half` at `mouth` perpendicular to
    /// the axis `axis` (a unit vector).
    fn pyramid(&mut self, apex: Vec3, mouth: Vec3, axis: Vec3, half: f32, color: [f32; 4]) {
        let (u, v) = perpendicular(axis);
        let corners = [
            mouth + (u + v) * half,
            mouth + (u - v) * half,
            mouth + (-u - v) * half,
            mouth + (-u + v) * half,
        ];
        self.ring(&corners, color);
        for c in corners {
            self.line(apex, c, color);
        }
    }
}

/// Two unit vectors perpendicular to `axis` and to each other.
fn perpendicular(axis: Vec3) -> (Vec3, Vec3) {
    let helper = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let u = axis.cross(helper).normalize_or_zero();
    (u, axis.cross(u))
}

/// The gizmo axes in tools 9.7 order: X to +X, Y to +Y, Z to -Z (internal), with their colours.
fn axes() -> [(Vec3, [f32; 4]); 3] {
    [
        (Vec3::X, look::AXIS[0]),
        (Vec3::Y, look::AXIS[1]),
        (Vec3::NEG_Z, look::AXIS[2]),
    ]
}

/// Appends the lines of one overlay item.
pub(crate) fn add_item(lines: &mut Lines, item: &OverlayItem, view: u8) {
    let white = |a: f32| [1.0, 1.0, 1.0, a];
    let m = item.transform;
    let mut tested = Sink {
        to: &mut lines.tested,
        m,
    };
    let mut over = Sink {
        to: &mut lines.over,
        m,
    };
    match item.kind {
        OverlayKind::MainGrid => {
            // The plane of the view: Top and Main y = 0, Side x = 16, Front z = 0.
            let (u, v, fixed) = match view {
                1 => (1, 2, (0, 16.0)),
                2 => (0, 1, (2, 0.0)),
                _ => (0, 2, (1, 0.0)),
            };
            let point = |a: f32, b: f32| {
                let mut p = Vec3::ZERO;
                p[u] = a;
                p[v] = b;
                p[fixed.0] = fixed.1;
                p
            };
            // The square spans [-16, 32] on x and y and [-32, 16] on z.
            let span = |axis: usize| {
                if axis == 2 {
                    (-32.0, 16.0)
                } else {
                    (-16.0, 32.0)
                }
            };
            let ((u0, u1), (v0, v1)) = (span(u), span(v));
            for i in 0..4u8 {
                let t = f32::from(i) * 16.0;
                tested.line(point(u0 + t, v0), point(u0 + t, v1), look::MAIN_GRID);
                tested.line(point(u0, v0 + t), point(u1, v0 + t), look::MAIN_GRID);
            }
        }
        OverlayKind::Blueprint => tested.unit_box(false, 0.0, false, look::MARKER),
        OverlayKind::HiddenBox => tested.unit_box(false, 0.0, false, white(look::FAINT)),
        OverlayKind::SelectionBox => tested.unit_box(false, 0.0, true, white(look::FAINT)),
        OverlayKind::ModelBox => tested.unit_box(true, 0.0, false, look::MARKER),
        OverlayKind::VoxelPointer => tested.unit_box(true, 0.01, false, look::VOXEL_POINTER),
        OverlayKind::CenterMarker => {
            let s = &mut over;
            let h = 0.5;
            s.ring(
                &[
                    Vec3::new(-h, -h, 0.0),
                    Vec3::new(h, -h, 0.0),
                    Vec3::new(h, h, 0.0),
                    Vec3::new(-h, h, 0.0),
                ],
                look::MARKER,
            );
            s.ring(
                &[
                    Vec3::new(-h, 0.0, -h),
                    Vec3::new(h, 0.0, -h),
                    Vec3::new(h, 0.0, h),
                    Vec3::new(-h, 0.0, h),
                ],
                look::MARKER,
            );
            s.ring(
                &[
                    Vec3::new(0.0, -h, -h),
                    Vec3::new(0.0, h, -h),
                    Vec3::new(0.0, h, h),
                    Vec3::new(0.0, -h, h),
                ],
                look::MARKER,
            );
        }
        OverlayKind::ModelOrigin => {
            let s = &mut tested;
            let r = 0.4;
            let ring = |s: &mut Sink<'_>, a: Vec3, b: Vec3| {
                s.ring(&[a * r, b * r, -a * r, -b * r], look::MARKER);
            };
            ring(s, Vec3::X, Vec3::Y);
            ring(s, Vec3::X, Vec3::Z);
            ring(s, Vec3::Y, Vec3::Z);
            for (axis, color) in axes() {
                s.line(Vec3::ZERO, axis, color);
            }
        }
        OverlayKind::NorthPointer => {
            // Flat, pointing north (+Z) with the mouth toward the block (app 8.10).
            let apex = Vec3::new(0.0, 0.0, 1.0);
            let mouth = [
                Vec3::new(-0.5, 0.0, 0.0),
                Vec3::new(0.0, 0.1, 0.0),
                Vec3::new(0.5, 0.0, 0.0),
                Vec3::new(0.0, -0.1, 0.0),
            ];
            for m in mouth {
                tested.line(apex, m, look::POINTER);
            }
            tested.ring(&mouth, look::POINTER);
        }
        OverlayKind::Gizmo(kind) => add_gizmo(&mut over, kind),
        OverlayKind::SubGrid(_) | OverlayKind::Guides => {}
    }
}

/// The wire shapes of a gizmo (tools 9.7).
fn add_gizmo(s: &mut Sink<'_>, kind: GizmoKind) {
    match kind {
        GizmoKind::Move => {
            for (axis, color) in axes() {
                s.line(Vec3::ZERO, axis, color);
                s.pyramid(axis * 1.3, axis, axis, 0.15, color);
            }
            // Plane squares: xz in (+X, -Z), yz in (+Y, -Z), xy in (+X, +Y), inset 0.01.
            let (a, b, c) = (Vec3::X, Vec3::Y, Vec3::NEG_Z);
            for (u, v, normal) in [(a, c, 1usize), (b, c, 0), (a, b, 2)] {
                let (lo, hi) = (0.01, 0.5);
                s.ring(
                    &[
                        u * lo + v * lo,
                        u * hi + v * lo,
                        u * hi + v * hi,
                        u * lo + v * hi,
                    ],
                    axes()[normal].1,
                );
            }
        }
        GizmoKind::Rotate => {
            for (axis, color) in axes() {
                let (u, v) = perpendicular(axis);
                let ring: Vec<Vec3> = (0..64u8)
                    .map(|i| {
                        let t = f32::from(i) / 64.0 * std::f32::consts::TAU;
                        (u * t.cos() + v * t.sin()) * 1.9
                    })
                    .collect();
                s.ring(&ring, color);
                s.line(Vec3::ZERO, axis, color);
            }
        }
        GizmoKind::Scale => {
            let faces = [
                (Vec3::new(0.0, 0.5, -0.5), Vec3::NEG_X, 0usize),
                (Vec3::new(1.0, 0.5, -0.5), Vec3::X, 0),
                (Vec3::new(0.5, 0.0, -0.5), Vec3::NEG_Y, 1),
                (Vec3::new(0.5, 1.0, -0.5), Vec3::Y, 1),
                (Vec3::new(0.5, 0.5, 0.0), Vec3::Z, 2),
                (Vec3::new(0.5, 0.5, -1.0), Vec3::NEG_Z, 2),
            ];
            for (pivot, dir, axis) in faces {
                s.pyramid(pivot, pivot + dir * 0.5, dir, 0.25, look::AXIS[axis]);
            }
        }
    }
}

/// The lines of a draw list's overlays for the view with the given index.
pub(crate) fn build(items: &[OverlayItem], view: u8) -> Lines {
    let mut lines = Lines::default();
    for item in items {
        add_item(&mut lines, item, view);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: OverlayKind) -> OverlayItem {
        OverlayItem {
            kind,
            transform: Mat4::IDENTITY,
        }
    }

    #[test]
    fn a_selection_box_has_twelve_edges_and_six_diagonals() {
        let lines = build(&[item(OverlayKind::SelectionBox)], 3);
        assert_eq!(lines.tested.len(), 2 * (12 + 6));
        assert!(lines.over.is_empty());
    }

    #[test]
    fn gizmos_ignore_depth_and_boxes_do_not() {
        let lines = build(
            &[
                item(OverlayKind::Gizmo(GizmoKind::Rotate)),
                item(OverlayKind::ModelBox),
            ],
            3,
        );
        assert_eq!(lines.tested.len(), 24);
        assert_eq!(lines.over.len(), 2 * (3 * 64 + 3));
    }

    #[test]
    fn the_main_grid_has_four_lines_each_way() {
        let lines = build(&[item(OverlayKind::MainGrid)], 3);
        assert_eq!(lines.tested.len(), 16);
        assert_eq!(lines.tested[0][3], pack(look::MAIN_GRID));
    }
}
