//! The raster primitives of the paint tools on a texture: the exact texel footprints of pen,
//! line, rectangle, round and square brush, flood fill, noise fill and colour replacement.
//!
//! Coordinates are texels with rows from the bottom, so `(0, 0)` is the lower-left texel.
//! Every primitive writes into the buffer only; texels outside it are ignored, as the original's
//! `SetPixel` ignores them. A stroke keeps its own state: the pen's last-texel memory, the
//! preview copy that line and rectangle restore, the undo entry.
//!
//! Sizes are the integer diameters of the tool size fields (tools 8.10); the radius is
//! `diameter / 2` as a float, so 7 gives 3.5. The pen is [`RgbaBuf::set`].
//!
//! spec: tools 13.2

use cuboid_num::{EngineRng, round_he, trunc_i32};

use crate::contract::RgbaBuf;

/// A rectangle of texels `[x0, x1) x [y0, y1)`: the clip rectangle of a stroke, the atlas
/// entry that contained the first hit, or the whole texture.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TexelRect {
    /// First column.
    pub x0: i32,
    /// First row.
    pub y0: i32,
    /// One past the last column.
    pub x1: i32,
    /// One past the last row.
    pub y1: i32,
}

impl TexelRect {
    /// The whole buffer.
    pub fn of(buf: &RgbaBuf) -> Self {
        Self {
            x0: 0,
            y0: 0,
            x1: buf.w as i32,
            y1: buf.h as i32,
        }
    }

    fn contains(self, x: i32, y: i32) -> bool {
        (self.x0..self.x1).contains(&x) && (self.y0..self.y1).contains(&y)
    }

    /// The part of the rectangle inside the buffer.
    fn within(self, buf: &RgbaBuf) -> Self {
        let bounds = Self::of(buf);
        Self {
            x0: self.x0.max(bounds.x0),
            y0: self.y0.max(bounds.y0),
            x1: self.x1.min(bounds.x1),
            y1: self.y1.min(bounds.y1),
        }
    }
}

/// The columns (or rows) of a round brush around `c`: from `trunc(c - r)` while the index is
/// below `max(1, c + r)`.
fn disc_span(c: i32, r: f32) -> (i32, i32) {
    let c = c as f32;
    (trunc_i32(c - r), trunc_i32((c + r).max(1.0).ceil()))
}

/// Round brush and sprayer: the disc stamp of `diameter` texels around `(cx, cy)`, inside `clip`.
///
/// The stamp spans `[trunc(c - r), max(1, c + r))` per axis and drops the texels with
/// `dx^2 + dy^2 >= r^2 - 1` when `r^2 - 1 > 1`, so the footprint of a tiny brush is a plain
/// block (diameters 1 and 2 give 2 x 2 texels) and a diameter of 7 gives the familiar disc of
/// seven texels across. The sprayer's "first clicked colour" sampling does not exist on
/// textures: it is this brush.
///
/// spec: tools 13.2 (Brush / Sprayer)
pub fn disc(buf: &mut RgbaBuf, cx: i32, cy: i32, diameter: i32, color: [u8; 4], clip: TexelRect) {
    let r = diameter as f32 / 2.0;
    let area = clip.within(buf);
    let (xs, xe) = disc_span(cx, r);
    let (ys, ye) = disc_span(cy, r);
    let limit = r * r - 1.0;
    let ring = limit > 1.0;
    for y in ys.max(area.y0)..ye.min(area.y1) {
        for x in xs.max(area.x0)..xe.min(area.x1) {
            let d2 = (x - cx) * (x - cx) + (y - cy) * (y - cy);
            if ring && d2 as f32 >= limit {
                continue;
            }
            buf.set(x, y, color);
        }
    }
}

/// Square brush: the block of `diameter` texels around `(cx, cy)`, from `round(c + 0.5 - r)`
/// to below `round(c + 0.5 + r)` per axis with half-to-even rounding. The original computes a
/// clip rectangle and does not apply it (QUIRK: a square can paint across atlas entries), so
/// only the buffer limits it.
///
/// spec: tools 13.2 (Square)
pub fn square(buf: &mut RgbaBuf, cx: i32, cy: i32, diameter: i32, color: [u8; 4]) {
    let r = diameter as f32 / 2.0;
    let span = |c: i32| {
        let mid = c as f32 + 0.5;
        (trunc_i32(round_he(mid - r)), trunc_i32(round_he(mid + r)))
    };
    let ((xs, xe), (ys, ye)) = (span(cx), span(cy));
    let bounds = TexelRect::of(buf);
    for y in ys.max(bounds.y0)..ye.min(bounds.y1) {
        for x in xs.max(bounds.x0)..xe.min(bounds.x1) {
            buf.set(x, y, color);
        }
    }
}

/// Rectangle: every texel of the inclusive box between two corners, given in any order. The
/// original does not clip it to the atlas entry (QUIRK), so only the buffer limits it.
///
/// spec: tools 13.2 (Rectangle)
pub fn rect(buf: &mut RgbaBuf, a: (i32, i32), b: (i32, i32), color: [u8; 4]) {
    let bounds = TexelRect::of(buf);
    let (x0, x1) = (
        a.0.min(b.0).max(bounds.x0),
        (a.0.max(b.0) + 1).min(bounds.x1),
    );
    let (y0, y1) = (
        a.1.min(b.1).max(bounds.y0),
        (a.1.max(b.1) + 1).min(bounds.y1),
    );
    for y in y0..y1 {
        for x in x0..x1 {
            buf.set(x, y, color);
        }
    }
}

/// Line: an integer Bresenham line from `from` to `to` (the `err = dx + dy` form, which takes
/// the diagonal step at ties), clipped to `clip`. A `thickness` of 1 or less sets single
/// texels; a larger one stamps the [`disc`] of that diameter at every point.
///
/// spec: tools 13.2 (Line)
pub fn line(
    buf: &mut RgbaBuf,
    from: (i32, i32),
    to: (i32, i32),
    thickness: i32,
    color: [u8; 4],
    clip: TexelRect,
) {
    let (mut x, mut y) = from;
    let (dx, dy) = ((to.0 - x).abs(), -(to.1 - y).abs());
    let (sx, sy) = (if x < to.0 { 1 } else { -1 }, if y < to.1 { 1 } else { -1 });
    let mut err = dx + dy;
    loop {
        if thickness > 1 {
            disc(buf, x, y, thickness, color, clip);
        } else if clip.contains(x, y) {
            buf.set(x, y, color);
        }
        if (x, y) == to {
            return;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Whether two colours match: alpha equal and every RGB channel within `limit / 2` levels
/// (`limit` is doubled so that half levels stay integers).
fn within(a: [u8; 4], b: [u8; 4], limit: u32) -> bool {
    let ([ar, ag, ab, aa], [br, bg, bb, ba]) = (a, b);
    let near = |x: u8, y: u8| 2 * u32::from(x.abs_diff(y)) <= limit;
    aa == ba && near(ar, br) && near(ag, bg) && near(ab, bb)
}

/// A horizontal run of texels, `x0..=x1` in row `y`.
struct Span {
    y: i32,
    x0: i32,
    x1: i32,
}

/// The texels of `area` connected (four ways) to `seed` that `joins` accepts, as runs.
fn flood(
    buf: &RgbaBuf,
    seed: (i32, i32),
    area: TexelRect,
    joins: impl Fn([u8; 4]) -> bool,
) -> Vec<Span> {
    let width = (area.x1 - area.x0) as usize;
    let index = |x: i32, y: i32| (y - area.y0) as usize * width + (x - area.x0) as usize;
    let mut seen = vec![false; width * (area.y1 - area.y0) as usize];
    let open = |seen: &[bool], x: i32, y: i32| {
        area.contains(x, y)
            && !seen.get(index(x, y)).copied().unwrap_or(true)
            && buf.get(x, y).is_some_and(&joins)
    };
    let mut spans = Vec::new();
    let mut stack = vec![seed];
    while let Some((sx, sy)) = stack.pop() {
        if !open(&seen, sx, sy) {
            continue;
        }
        let (mut left, mut right) = (sx, sx);
        while open(&seen, left - 1, sy) {
            left -= 1;
        }
        while open(&seen, right + 1, sy) {
            right += 1;
        }
        for x in left..=right {
            if let Some(cell) = seen.get_mut(index(x, sy)) {
                *cell = true;
            }
        }
        spans.push(Span {
            y: sy,
            x0: left,
            x1: right,
        });
        // One seed per run of open texels in the rows above and below.
        for y in [sy - 1, sy + 1] {
            let mut in_run = false;
            for x in left..=right {
                let ok = open(&seen, x, y);
                if ok && !in_run {
                    stack.push((x, y));
                }
                in_run = ok;
            }
        }
    }
    spans
}

/// The fill colour with an independent uniform offset of at most `half` levels on each RGB
/// channel, truncated and clamped.
fn jitter([r, g, b, a]: [u8; 4], half: f32, rng: &mut EngineRng) -> [u8; 4] {
    let mut shade = |v: u8| {
        let level = f32::from(v) + rng.range_f32(-half, half);
        u8::try_from(trunc_i32(level).clamp(0, 255)).unwrap_or(v)
    };
    [shade(r), shade(g), shade(b), a]
}

/// Flood fill (`tolerance` 0) and noise fill: a scanline fill from `(x, y)` inside `clip`.
///
/// A texel joins when it is similar to the clicked colour (alpha equal, every RGB channel
/// within `tolerance` levels) and not already similar to `color` (within `tolerance / 2`), which
/// also ends a fill with the colour that is already there. With a tolerance above 0 each painted
/// texel gets `color` plus an independent uniform offset of `+-tolerance / 2` levels per RGB
/// channel from `rng`, texels in rows from the bottom and left to right.
///
/// The region is found on the unmodified texture and painted afterwards, so every texel is
/// painted once. Connectivity is four-way, and the offset is truncated to a level
/// (**UNVERIFIED**: the specification names neither).
///
/// spec: tools 13.2 (Fill / Noise)
pub fn fill(
    buf: &mut RgbaBuf,
    x: i32,
    y: i32,
    color: [u8; 4],
    tolerance: u8,
    clip: TexelRect,
    rng: &mut EngineRng,
) {
    let area = clip.within(buf);
    let Some(target) = buf.get(x, y).filter(|_| area.contains(x, y)) else {
        return;
    };
    let tol = u32::from(tolerance);
    let joins = |texel| within(texel, target, 2 * tol) && !within(texel, color, tol);
    let mut spans = flood(buf, (x, y), area, joins);
    spans.sort_unstable_by_key(|s| (s.y, s.x0));
    let half = f32::from(tolerance) / 2.0;
    for span in spans {
        for x in span.x0..=span.x1 {
            let texel = if tolerance == 0 {
                color
            } else {
                jitter(color, half, rng)
            };
            buf.set(x, span.y, texel);
        }
    }
}

/// Magic: every texel inside `clip` that equals the colour of `(x, y)` becomes `color`.
///
/// spec: tools 13.2 (Magic)
pub fn replace(buf: &mut RgbaBuf, x: i32, y: i32, color: [u8; 4], clip: TexelRect) {
    let Some(target) = buf.get(x, y) else {
        return;
    };
    let area = clip.within(buf);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            if buf.get(x, y) == Some(target) {
                buf.set(x, y, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INK: [u8; 4] = [200, 30, 40, 255];
    const BG: [u8; 4] = [10, 10, 10, 255];
    const WALL: [u8; 4] = [250, 250, 250, 255];
    const OTHER: [u8; 4] = [100, 50, 0, 255];

    fn int(v: usize) -> i32 {
        i32::try_from(v).unwrap()
    }

    /// Picture of the INK texels of a buffer, top row first.
    fn art(buf: &RgbaBuf) -> Vec<String> {
        (0..buf.h as i32)
            .rev()
            .map(|y| {
                (0..buf.w as i32)
                    .map(|x| if buf.get(x, y) == Some(INK) { '#' } else { '.' })
                    .collect()
            })
            .collect()
    }

    /// A buffer from a picture, top row first: `.` background, `#` wall, `o` another colour.
    fn grid(rows: &[&str]) -> RgbaBuf {
        let mut buf = RgbaBuf::new(
            u32::try_from(rows[0].len()).unwrap(),
            u32::try_from(rows.len()).unwrap(),
        );
        for (y, text) in rows.iter().rev().enumerate() {
            for (x, c) in text.chars().enumerate() {
                let color = match c {
                    '.' => BG,
                    '#' => WALL,
                    _ => OTHER,
                };
                buf.set(int(x), int(y), color);
            }
        }
        buf
    }

    fn uniform(w: u32, h: u32, color: [u8; 4]) -> RgbaBuf {
        let mut buf = RgbaBuf::new(w, h);
        buf.pixels.as_chunks_mut::<4>().0.fill(color);
        buf
    }

    #[test]
    fn round_brush_of_diameter_7_is_a_disc_of_seven_texels_across() {
        let mut buf = RgbaBuf::new(11, 11);
        let clip = TexelRect::of(&buf);
        disc(&mut buf, 5, 5, 7, INK, clip);
        assert_eq!(
            art(&buf),
            [
                "...........",
                "...........",
                "....###....",
                "...#####...",
                "..#######..",
                "..#######..",
                "..#######..",
                "...#####...",
                "....###....",
                "...........",
                "...........",
            ]
        );
    }

    #[test]
    fn small_round_brushes_follow_the_loop_bounds_of_the_formula() {
        // Diameters 1 and 2 skip the ring test and span [trunc(c - r), c + r], two texels.
        let two_by_two = [".......", ".......", "..##...", "..##...", "......."];
        // Diameter 3: r^2 - 1 = 1.25 keeps dx^2 + dy^2 <= 1, a plus sign.
        let plus = [".......", "...#...", "..###..", "...#...", "......."];
        // Diameter 4: r^2 - 1 = 3 keeps dx^2 + dy^2 <= 2, a block of 3 x 3.
        let three_by_three = [".......", "..###..", "..###..", "..###..", "......."];
        for (diameter, want) in [
            (1, two_by_two),
            (2, two_by_two),
            (3, plus),
            (4, three_by_three),
        ] {
            let mut buf = RgbaBuf::new(7, 5);
            let clip = TexelRect::of(&buf);
            disc(&mut buf, 3, 2, diameter, INK, clip);
            assert_eq!(art(&buf), want, "diameter {diameter}");
        }
    }

    #[test]
    fn the_brush_is_clipped_to_the_rectangle_and_the_buffer() {
        let mut buf = RgbaBuf::new(11, 11);
        let clip = TexelRect {
            x0: 5,
            y0: 0,
            x1: 8,
            y1: 6,
        };
        disc(&mut buf, 5, 5, 7, INK, clip);
        assert_eq!(
            art(&buf),
            [
                "...........",
                "...........",
                "...........",
                "...........",
                "...........",
                ".....###...",
                ".....###...",
                ".....###...",
                ".....##....",
                "...........",
                "...........",
            ]
        );
        let mut corner = RgbaBuf::new(4, 4);
        let clip = TexelRect::of(&corner);
        disc(&mut corner, 0, 0, 7, INK, clip);
        assert_eq!(art(&corner), ["##..", "###.", "####", "####"]);
    }

    #[test]
    fn square_brush_spans_round_half_even_and_ignores_the_clip() {
        let mut buf = RgbaBuf::new(11, 11);
        square(&mut buf, 5, 5, 7, INK);
        assert_eq!(
            art(&buf),
            [
                "...........",
                "...........",
                "..#######..",
                "..#######..",
                "..#######..",
                "..#######..",
                "..#######..",
                "..#######..",
                "..#######..",
                "...........",
                "...........",
            ]
        );
        // Even sizes: round(c + 0.5 - r) rounds the half to the even integer, so the centres 5
        // and 6 both give the four texels 4 to 7.
        let (mut at5, mut at6) = (RgbaBuf::new(10, 10), RgbaBuf::new(10, 10));
        square(&mut at5, 5, 5, 4, INK);
        square(&mut at6, 6, 6, 4, INK);
        assert_eq!(art(&at5), art(&at6));
        assert_eq!(art(&at5)[3], "....####..");
        assert_eq!(art(&at5).iter().filter(|r| r.contains('#')).count(), 4);
        let mut odd = RgbaBuf::new(10, 10);
        square(&mut odd, 5, 5, 3, INK);
        assert_eq!(art(&odd)[4], "....###...");
        assert_eq!(art(&odd).iter().filter(|r| r.contains('#')).count(), 3);
        let mut corner = RgbaBuf::new(4, 4);
        square(&mut corner, 0, 0, 7, INK);
        assert_eq!(art(&corner), ["####"; 4]);
    }

    #[test]
    fn rectangle_covers_both_corners_in_any_order_and_stays_in_the_buffer() {
        let mut buf = RgbaBuf::new(8, 6);
        rect(&mut buf, (5, 4), (2, 1), INK);
        assert_eq!(
            art(&buf),
            [
                "........", "..####..", "..####..", "..####..", "..####..", "........"
            ]
        );
        let mut other = RgbaBuf::new(8, 6);
        rect(&mut other, (2, 4), (5, 1), INK);
        assert_eq!(art(&other), art(&buf));
        let mut edge = RgbaBuf::new(3, 3);
        rect(&mut edge, (-3, -3), (1, 0), INK);
        assert_eq!(art(&edge), ["...", "...", "##."]);
    }

    #[test]
    fn bresenham_takes_the_diagonal_step_at_ties_and_depends_on_the_direction() {
        let mut forward = RgbaBuf::new(6, 3);
        let clip = TexelRect::of(&forward);
        line(&mut forward, (0, 0), (4, 2), 1, INK, clip);
        assert_eq!(art(&forward), ["...##.", ".##...", "#....."]);
        let mut backward = RgbaBuf::new(6, 3);
        line(&mut backward, (4, 2), (0, 0), 1, INK, clip);
        assert_eq!(art(&backward), ["....#.", "..##..", "##...."]);
        let mut steep = RgbaBuf::new(3, 5);
        let clip = TexelRect::of(&steep);
        line(&mut steep, (0, 0), (2, 4), 1, INK, clip);
        assert_eq!(art(&steep), ["..#", "..#", ".#.", ".#.", "#.."]);
    }

    #[test]
    fn every_line_is_a_connected_path_with_one_texel_per_major_step() {
        let from = (16, 16);
        for (dx, dy) in (-9..=9).flat_map(|x| (-9..=9).map(move |y| (x, y))) {
            let mut buf = RgbaBuf::new(32, 32);
            let clip = TexelRect::of(&buf);
            let to = (16 + dx, 16 + dy);
            line(&mut buf, from, to, 1, INK, clip);
            let major = |&(x, y): &(i32, i32)| (x - from.0).abs().max((y - from.1).abs());
            let mut painted: Vec<(i32, i32)> = (0..32)
                .flat_map(|y| (0..32).map(move |x| (x, y)))
                .filter(|&(x, y)| buf.get(x, y) == Some(INK))
                .collect();
            painted.sort_by_key(major);
            let steps: Vec<i32> = painted.iter().map(major).collect();
            assert_eq!(
                steps,
                (0..=dx.abs().max(dy.abs())).collect::<Vec<_>>(),
                "to {to:?}"
            );
            assert_eq!(painted.first(), Some(&from));
            assert_eq!(painted.last(), Some(&to));
            assert!(
                painted
                    .windows(2)
                    .all(|w| (w[0].0 - w[1].0).abs() <= 1 && (w[0].1 - w[1].1).abs() <= 1),
                "to {to:?}"
            );
        }
    }

    #[test]
    fn thick_lines_stamp_a_disc_at_every_point_and_are_clipped_to_the_start_rectangle() {
        let mut thick = RgbaBuf::new(11, 6);
        let clip = TexelRect::of(&thick);
        line(&mut thick, (3, 3), (7, 3), 3, INK, clip);
        assert_eq!(
            art(&thick),
            [
                "...........",
                "...#####...",
                "..#######..",
                "...#####...",
                "...........",
                "...........",
            ]
        );
        let mut clipped = RgbaBuf::new(9, 3);
        let clip = TexelRect {
            x0: 2,
            y0: 0,
            x1: 5,
            y1: 3,
        };
        line(&mut clipped, (0, 2), (8, 2), 1, INK, clip);
        assert_eq!(art(&clipped), ["..###....", ".........", "........."]);
    }

    #[test]
    fn fill_stops_at_walls_and_leaks_through_gaps_but_not_diagonally() {
        let mut rng = EngineRng::seeded(1);
        let mut walled = grid(&["..#...", "..#...", "..#..."]);
        let clip = TexelRect::of(&walled);
        fill(&mut walled, 0, 0, INK, 0, clip, &mut rng);
        assert_eq!(art(&walled), ["##....", "##....", "##...."]);

        let mut gap = grid(&["..#...", "..#...", "......", "..#..."]);
        let clip = TexelRect::of(&gap);
        fill(&mut gap, 0, 0, INK, 0, clip, &mut rng);
        assert_eq!(art(&gap), ["##.###", "##.###", "######", "##.###"]);

        // Texels that touch only at a corner are not connected.
        let mut diagonal = grid(&["#.", ".#"]);
        let clip = TexelRect::of(&diagonal);
        fill(&mut diagonal, 0, 0, INK, 0, clip, &mut rng);
        assert_eq!(art(&diagonal), ["..", "#."]);
    }

    #[test]
    fn fill_is_limited_by_the_clip_rectangle_and_ignores_seeds_outside_it() {
        let mut rng = EngineRng::seeded(1);
        let mut buf = uniform(6, 4, BG);
        let clip = TexelRect {
            x0: 2,
            y0: 1,
            x1: 5,
            y1: 3,
        };
        fill(&mut buf, 3, 2, INK, 0, clip, &mut rng);
        assert_eq!(art(&buf), ["......", "..###.", "..###.", "......"]);
        let before = buf.clone();
        fill(&mut buf, 0, 0, [1, 2, 3, 255], 0, clip, &mut rng);
        assert_eq!(buf, before);
    }

    #[test]
    fn fill_tolerance_joins_close_colours_and_keeps_alpha_exact() {
        let mut rng = EngineRng::seeded(1);
        let near = [BG[0] + 8, BG[1], BG[2], 255];
        let far = [BG[0] + 30, BG[1], BG[2], 255];
        let translucent = [BG[0], BG[1], BG[2], 254];
        let mut buf = RgbaBuf::new(5, 1);
        for (x, c) in [BG, near, far, translucent, BG].into_iter().enumerate() {
            buf.set(int(x), 0, c);
        }
        let clip = TexelRect::of(&buf);
        let mut exact = buf.clone();
        fill(&mut exact, 0, 0, INK, 0, clip, &mut rng);
        assert_eq!(art(&exact), ["#...."]);
        // A tolerance of 10 joins the texel 8 levels away and stops at the one 30 away; the
        // translucent texel differs in alpha. Painted texels are jittered, so compare with the
        // original instead of with ink.
        let mut loose = buf.clone();
        fill(&mut loose, 0, 0, [200, 200, 200, 255], 10, clip, &mut rng);
        let changed: Vec<bool> = (0..5).map(|x| loose.get(x, 0) != buf.get(x, 0)).collect();
        assert_eq!(changed, [true, true, false, false, false]);
    }

    #[test]
    fn a_fill_that_changes_nothing_ends() {
        let mut rng = EngineRng::seeded(1);
        let mut buf = uniform(4, 4, [100, 100, 100, 255]);
        let clip = TexelRect::of(&buf);
        let before = buf.clone();
        // The colour that is already there, and a colour within tolerance / 2 of it.
        fill(&mut buf, 1, 1, [100, 100, 100, 255], 0, clip, &mut rng);
        assert_eq!(buf, before);
        fill(&mut buf, 1, 1, [110, 100, 100, 255], 32, clip, &mut rng);
        assert_eq!(buf, before);
        // 40 levels away is beyond tolerance / 2 = 16, so it paints.
        fill(&mut buf, 1, 1, [140, 100, 100, 255], 32, clip, &mut rng);
        assert_ne!(buf, before);
    }

    #[test]
    fn noise_fill_jitters_each_channel_within_half_the_amount_in_scan_order() {
        let mut buf = uniform(8, 8, [100, 100, 100, 255]);
        let clip = TexelRect::of(&buf);
        let fill_colour = [128, 128, 128, 255];
        fill(
            &mut buf,
            3,
            3,
            fill_colour,
            32,
            clip,
            &mut EngineRng::seeded(7),
        );
        let mut distinct = std::collections::HashSet::new();
        for texel in buf.pixels.as_chunks::<4>().0 {
            assert_eq!(texel[3], 255);
            assert!(
                texel[..3].iter().all(|c| (112..=143).contains(c)),
                "{texel:?}"
            );
            distinct.insert(*texel);
        }
        assert!(distinct.len() > 20);
        // The lower-left texel is painted first, with the first three draws: red, green, blue.
        let first = jitter(fill_colour, 16.0, &mut EngineRng::seeded(7));
        assert_eq!(buf.get(0, 0), Some(first));
        // The same seed gives the same texture, wherever the fill starts.
        let mut again = uniform(8, 8, [100, 100, 100, 255]);
        fill(
            &mut again,
            0,
            7,
            fill_colour,
            32,
            clip,
            &mut EngineRng::seeded(7),
        );
        assert_eq!(again, buf);
    }

    #[test]
    fn jitter_clamps_to_the_byte_range() {
        let mut rng = EngineRng::seeded(5);
        for _ in 0..200 {
            let [r, g, b, a] = jitter([0, 255, 3, 77], 127.5, &mut rng);
            assert!(r <= 127 && g >= 127 && b <= 130 && a == 77);
        }
    }

    #[test]
    fn magic_replaces_the_clicked_colour_inside_the_clip_only() {
        let mut buf = grid(&["o..o", ".oo.", "o..o"]);
        let clip = TexelRect {
            x0: 1,
            y0: 0,
            x1: 4,
            y1: 2,
        };
        replace(&mut buf, 1, 1, INK, clip);
        // The clicked texel (1, 1) is "o": the ones inside the clip become ink, the ones at
        // (0, 0) and in the top row stay.
        let ink: Vec<(i32, i32)> = (0..3)
            .flat_map(|y| (0..4).map(move |x| (x, y)))
            .filter(|&(x, y)| buf.get(x, y) == Some(INK))
            .collect();
        assert_eq!(ink, [(3, 0), (1, 1), (2, 1)]);
        assert_eq!(buf.get(0, 0), Some(OTHER));
        assert_eq!(buf.get(0, 2), Some(OTHER));
    }
}
