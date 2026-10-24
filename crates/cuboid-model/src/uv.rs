//! UV rectangle with raw and folded corners, rotation and UV snap (spec: tools 4.5, 4.7; native
//! 4.4.3).

use cuboid_num::{clamp_uv, fold_uv, uv_rotation};
use glam::{Vec2, Vec3};

/// The UV rectangle of a face, in normalised coordinates of the face's own texture with the
/// origin at the bottom left.
///
/// The rectangle keeps the raw corners (clamped to `[-1, 2]` on write) and the rotation
/// separate from the folded corners every consumer reads: a rectangle dragged outside `[0, 1]`
/// folds back in per axis, and files, copies and mesh UVs all use the folded values.
///
/// spec: tools 4.5, native 4.4.3, arch 6 (cuboid-model)
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct UvRect {
    rot: i32,
    tl_raw: Vec2,
    br_raw: Vec2,
}

impl Default for UvRect {
    /// The full texture, `(0, 0)` to `(1, 1)`, rotation 0: the UV of a new face.
    fn default() -> Self {
        Self {
            rot: 0,
            tl_raw: Vec2::ZERO,
            br_raw: Vec2::ONE,
        }
    }
}

impl UvRect {
    /// A rectangle from stored values: the rotation is normalised and the corners are clamped
    /// like the record setters do.
    pub fn new(rot: i32, tl: Vec2, br: Vec2) -> Self {
        Self {
            rot: uv_rotation(rot),
            tl_raw: clamp_uv(tl),
            br_raw: clamp_uv(br),
        }
    }

    /// The rotation in degrees: 0, 90, 180 or 270 (a negative multiple of 90 for stored values
    /// below -405, tools 4.5).
    pub fn rot(&self) -> i32 {
        self.rot
    }

    /// The folded first corner.
    pub fn tl(&self) -> Vec2 {
        fold_uv(self.tl_raw, self.br_raw).0
    }

    /// The folded second corner.
    pub fn br(&self) -> Vec2 {
        fold_uv(self.tl_raw, self.br_raw).1
    }

    /// The raw first corner, as stored: the UV view edits these.
    pub fn tl_raw(&self) -> Vec2 {
        self.tl_raw
    }

    /// The raw second corner, as stored.
    pub fn br_raw(&self) -> Vec2 {
        self.br_raw
    }

    /// The same rectangle with the folded corners stored as raw values: what a face copy keeps
    /// (tools 4.4, 4.5), since every consumer reads the folded values. The rotation is kept as is.
    pub fn folded(&self) -> Self {
        Self {
            rot: self.rot,
            tl_raw: self.tl(),
            br_raw: self.br(),
        }
    }

    /// Sets both corners, clamped to `[-1, 2]` per component.
    pub fn set(&mut self, tl: Vec2, br: Vec2) {
        self.tl_raw = clamp_uv(tl);
        self.br_raw = clamp_uv(br);
    }

    /// Sets both corners without the clamp, as the legacy `.cub` and the Minecraft JSON
    /// importers do.
    ///
    /// spec: native 4.4.3, native 9.5
    pub fn set_unclamped(&mut self, tl: Vec2, br: Vec2) {
        self.tl_raw = tl;
        self.br_raw = br;
    }

    /// Sets the rotation, normalised to a multiple of 90 degrees.
    pub fn set_rot(&mut self, r: i32) {
        self.rot = uv_rotation(r);
    }

    /// The four corners after the rotation, `C0` to `C3` of tools 4.5, from the folded values.
    pub fn rotated_corners(&self) -> [Vec2; 4] {
        let (tl, br) = fold_uv(self.tl_raw, self.br_raw);
        let cycle = [tl, Vec2::new(br.x, tl.y), br, Vec2::new(tl.x, br.y)];
        let k = (self.rot / 90).rem_euclid(4) as usize;
        std::array::from_fn(|i| cycle[(k + i) % 4])
    }
}

/// The automatic default mapping of the six faces in slot order (down, up, north, south, west,
/// east) for an element at position `p` with size `s`, all rotations 0. `p` is the stored
/// position, which for a rotated element is the rotated corner, not the unrotated one.
///
/// spec: tools 4.7, tools 1.4
pub fn default_uv_rects(p: Vec3, s: Vec3, mirror: bool) -> [UvRect; 6] {
    let (x, y, z) = (p.x / 16.0, p.y / 16.0, -p.z / 16.0);
    let (w, h, d) = (s.x / 16.0, s.y / 16.0, s.z / 16.0);
    let rect = |tl: Vec2, br: Vec2| UvRect::new(0, tl, br);
    let (north, east) = if mirror {
        (
            rect(Vec2::new(x + w, y), Vec2::new(x, y + h)),
            rect(Vec2::new(z + d, y), Vec2::new(z, y + h)),
        )
    } else {
        (
            rect(Vec2::new(1.0 - x - w, y), Vec2::new(1.0 - x, y + h)),
            rect(Vec2::new(1.0 - z - d, y), Vec2::new(1.0 - z, y + h)),
        )
    };
    [
        rect(Vec2::new(x, z), Vec2::new(x + w, z + d)),
        rect(Vec2::new(x, 1.0 - z - d), Vec2::new(x + w, 1.0 - z)),
        north,
        rect(Vec2::new(x, y), Vec2::new(x + w, y + h)),
        rect(Vec2::new(z, y), Vec2::new(z + d, y + h)),
        east,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }

    #[test]
    fn a_new_rectangle_normalises_the_rotation_and_clamps_the_corners() {
        let uv = UvRect::new(450, v(-3.0, 0.25), v(5.0, 1.0));
        assert_eq!(uv.rot(), 90);
        assert_eq!(uv.tl_raw(), v(-1.0, 0.25));
        assert_eq!(uv.br_raw(), v(2.0, 1.0));
        assert_eq!(UvRect::new(-90, Vec2::ZERO, Vec2::ONE).rot(), 270);
        assert_eq!(UvRect::new(-450, Vec2::ZERO, Vec2::ONE).rot(), -90);
    }

    #[test]
    fn the_default_rectangle_is_the_full_texture() {
        let uv = UvRect::default();
        assert_eq!((uv.rot(), uv.tl(), uv.br()), (0, v(0.0, 0.0), v(1.0, 1.0)));
    }

    #[test]
    fn setters_clamp_and_the_unclamped_setter_does_not() {
        let mut uv = UvRect::default();
        uv.set(v(-2.0, 0.5), v(3.0, 9.0));
        assert_eq!((uv.tl_raw(), uv.br_raw()), (v(-1.0, 0.5), v(2.0, 2.0)));
        uv.set_unclamped(v(-2.0, 0.5), v(3.0, 9.0));
        assert_eq!((uv.tl_raw(), uv.br_raw()), (v(-2.0, 0.5), v(3.0, 9.0)));
        uv.set_rot(-180);
        assert_eq!(uv.rot(), 180);
        uv.set_rot(359);
        assert_eq!(uv.rot(), 0);
    }

    /// Tools 17 item 7: rectangles in [-1, 0], in [1, 2] and straddling 0 and 1.
    #[test]
    fn the_getters_fold_dragged_rectangles_back_into_the_unit_square() {
        let cases = [
            // (raw tl, raw br) -> (folded tl, folded br)
            (
                (v(-0.75, -1.0), v(-0.25, -0.5)),
                (v(0.25, 0.0), v(0.75, 0.5)),
            ),
            ((v(-0.5, -0.5), v(0.0, 0.0)), (v(0.5, 0.5), v(1.0, 1.0))),
            ((v(1.25, 1.0), v(1.75, 1.5)), (v(0.25, 0.0), v(0.75, 0.5))),
            ((v(1.5, 1.5), v(2.0, 2.0)), (v(0.5, 0.5), v(1.0, 1.0))),
            ((v(-0.25, -0.25), v(0.5, 0.5)), (v(0.0, 0.0), v(0.5, 0.5))),
            ((v(0.5, 0.5), v(1.5, 1.5)), (v(0.5, 0.5), v(1.0, 1.0))),
            (
                (v(0.25, 0.25), v(0.75, 0.75)),
                (v(0.25, 0.25), v(0.75, 0.75)),
            ),
            // Mirrored rectangles keep their corner order.
            ((v(0.75, 0.0), v(0.25, 1.0)), (v(0.75, 0.0), v(0.25, 1.0))),
        ];
        for ((tl, br), (folded_tl, folded_br)) in cases {
            let uv = UvRect::new(0, tl, br);
            assert_eq!((uv.tl(), uv.br()), (folded_tl, folded_br), "{tl} {br}");
            assert_eq!((uv.tl_raw(), uv.br_raw()), (tl, br), "raw values are kept");
        }
    }

    #[test]
    fn a_folded_copy_stores_the_folded_corners_and_reads_the_same() {
        let uv = UvRect::new(-450, v(-0.25, 1.25), v(0.5, 1.75));
        let folded = uv.folded();
        assert_eq!((folded.tl_raw(), folded.br_raw()), (uv.tl(), uv.br()));
        assert_eq!(
            (folded.tl(), folded.br(), folded.rot()),
            (uv.tl(), uv.br(), -90)
        );
        assert_eq!(folded.rotated_corners(), uv.rotated_corners());
    }

    #[test]
    fn rotated_corners_cycle_through_the_four_corners() {
        let corners = |rot| UvRect::new(rot, v(0.25, 0.5), v(0.75, 1.0)).rotated_corners();
        let (p00, p10, p11, p01) = (v(0.25, 0.5), v(0.75, 0.5), v(0.75, 1.0), v(0.25, 1.0));
        assert_eq!(corners(0), [p00, p10, p11, p01]);
        assert_eq!(corners(90), [p10, p11, p01, p00]);
        assert_eq!(corners(180), [p11, p01, p00, p10]);
        assert_eq!(corners(270), [p01, p00, p10, p11]);
        // A stored negative rotation (tools 4.5 quirk) cycles like its positive twin.
        assert_eq!(corners(-450), corners(270));
    }

    #[test]
    fn rotated_corners_use_the_folded_values() {
        let uv = UvRect::new(0, v(1.25, -0.5), v(1.75, -0.25));
        assert_eq!(
            uv.rotated_corners(),
            [v(0.25, 0.5), v(0.75, 0.5), v(0.75, 0.75), v(0.25, 0.75)]
        );
    }

    /// Tools 17 item 6: the table of tools 4.7, mirrored and unmirrored.
    #[test]
    fn the_default_mapping_of_an_element_inside_the_first_block() {
        // bpos = (0.25, 0.5, 0.125), bsize = (0.25, 0.125, 0.5).
        let p = Vec3::new(4.0, 8.0, -2.0);
        let s = Vec3::new(4.0, 2.0, 8.0);
        let off = default_uv_rects(p, s, false);
        let on = default_uv_rects(p, s, true);
        let expect = |rects: &[UvRect; 6], i: usize, tl: Vec2, br: Vec2| {
            assert_eq!((rects[i].tl(), rects[i].br()), (tl, br), "slot {i}");
            assert_eq!(rects[i].rot(), 0);
        };
        // down
        expect(&off, 0, v(0.25, 0.125), v(0.5, 0.625));
        // up: (x, 1 - z - d) .. (x + w, 1 - z)
        expect(&off, 1, v(0.25, 0.375), v(0.5, 0.875));
        // north: (1 - x - w, y) .. (1 - x, y + h) and the mirrored variant
        expect(&off, 2, v(0.5, 0.5), v(0.75, 0.625));
        expect(&on, 2, v(0.5, 0.5), v(0.25, 0.625));
        // south
        expect(&off, 3, v(0.25, 0.5), v(0.5, 0.625));
        // west: (z, y) .. (z + d, y + h)
        expect(&off, 4, v(0.125, 0.5), v(0.625, 0.625));
        // east: (1 - z - d, y) .. (1 - z, y + h) and the mirrored variant
        expect(&off, 5, v(0.375, 0.5), v(0.875, 0.625));
        expect(&on, 5, v(0.625, 0.5), v(0.125, 0.625));
        // Faces without a mirror variant are the same in both modes.
        for i in [0, 1, 3, 4] {
            assert_eq!(off[i], on[i]);
        }
    }

    #[test]
    fn the_default_mapping_reads_the_stored_position_of_a_rotated_element() {
        // The same size at two stored positions maps differently: the rotated corner counts.
        let s = Vec3::new(4.0, 4.0, 4.0);
        let a = default_uv_rects(Vec3::new(0.0, 0.0, 0.0), s, false);
        let b = default_uv_rects(Vec3::new(8.0, 0.0, 0.0), s, false);
        assert_eq!(a[3].tl(), v(0.0, 0.0));
        assert_eq!(b[3].tl(), v(0.5, 0.0));
    }

    #[test]
    fn the_default_mapping_clamps_corners_like_the_record_setter() {
        // bpos = (3, 0, 0): x + w = 3.25 clamps to 2.
        let rects = default_uv_rects(Vec3::new(48.0, 0.0, 0.0), Vec3::new(4.0, 4.0, 4.0), false);
        assert_eq!(rects[3].br_raw(), v(2.0, 0.25));
    }
}
