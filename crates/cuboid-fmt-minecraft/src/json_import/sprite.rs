//! The sprite of an `item/generated` model as elements (interop 8.2, 18.2).
//!
//! The game extrudes the first layer picture into a slab one unit thick, with a quad for every
//! pixel edge. Here the opaque pixels become rectangles of the fewest rows that cover them, each
//! one box in the plane z = 7.5 to 8.5, and the faces that touch another box are switched off.

use cuboid_model::image::Image;
use cuboid_model::mc::{self, McBox, Rigid};
use cuboid_model::{Element, ElementFlags, Face, FaceName, Label, ObjectBase, Perms};
use glam::Vec3;

use super::elements::marker_color;

/// The depth of the slab: the plane of the item model, one unit thick.
const Z_RANGE: (f32, f32) = (7.5, 8.5);

/// A rectangle of pixels: column, row from the top, width, height.
type Rect = (usize, usize, usize, usize);

/// The sprite of one layer. `tint` is the tint index of its faces (the layer number). The
/// result is one slab for the whole picture when it has more than `max_texels` texels in a frame.
/// A picture taller than wide whose height is a multiple of its width is an animation strip;
/// only the first frame is used.
pub(crate) fn extrude(
    image: &Image,
    label: &Label,
    tint: i32,
    max_texels: u32,
    first_index: usize,
) -> (Vec<Element>, bool) {
    let w = image.w as usize;
    let frame = if image.h > image.w && image.w > 0 && image.h.is_multiple_of(image.w) {
        image.w as usize
    } else {
        image.h as usize
    };
    if w == 0 || frame == 0 {
        return (Vec::new(), false);
    }
    let slab = w.saturating_mul(frame) > max_texels as usize;
    let pixels = image.rgba();
    let rows = image.h as usize;
    // Pixel rows run bottom to top in the image; `y` counts from the top.
    let opaque = |x: usize, y: usize| {
        rows.checked_sub(y + 1)
            .and_then(|row| pixels.get((row * w + x) * 4 + 3))
            .is_some_and(|alpha| *alpha != 0)
    };
    let rects = if slab {
        vec![(0, 0, w, frame)]
    } else {
        rectangles(w, frame, &opaque)
    };
    let covered = |x: usize, y: usize| !slab && x < w && y < frame && opaque(x, y);

    let (sx, sy) = (16.0 / w as f32, 16.0 / frame as f32);
    let (tu, tv) = (16.0 / w as f32, 16.0 / image.h as f32);
    let elements = rects
        .into_iter()
        .enumerate()
        .map(|(n, (x, y, rw, rh))| {
            let from = Vec3::new(x as f32 * sx, 16.0 - (y + rh) as f32 * sy, Z_RANGE.0);
            let to = Vec3::new((x + rw) as f32 * sx, 16.0 - y as f32 * sy, Z_RANGE.1);
            let placed = mc::place(
                &McBox {
                    from,
                    to,
                    pivot: from,
                    rotation: Vec3::ZERO,
                },
                Rigid::IDENTITY,
            );
            let mut base = ObjectBase::new(
                format!("{label}_{}", first_index + n),
                placed.p,
                placed.s,
                Perms::ELEMENT,
            );
            base.o = placed.o;
            let (u0, u1) = (x as f32 * tu, (x + rw) as f32 * tu);
            let (v0, v1) = (y as f32 * tv, (y + rh) as f32 * tv);
            let (left, right) = (x as f32 * tu, (x + rw) as f32 * tu);
            let hidden_right = (y..y + rh).all(|row| covered(x + rw, row));
            let hidden_left = x > 0 && (y..y + rh).all(|row| covered(x - 1, row));
            let hidden_top = y > 0 && (x..x + rw).all(|col| covered(col, y - 1));
            let hidden_bottom = (x..x + rw).all(|col| covered(col, y + rh));
            let faces: [Face; 6] = FaceName::ALL.map(|name| {
                // `[u0, v0, u1, v1]` in the 16-unit space of the model file.
                let (active, rect) = match name {
                    FaceName::South => (true, [u0, v0, u1, v1]),
                    FaceName::North => (true, [u1, v0, u0, v1]),
                    FaceName::East => (!hidden_right, [right - tu, v0, right, v1]),
                    FaceName::West => (!hidden_left, [left, v0, left + tu, v1]),
                    FaceName::Up => (!hidden_top, [u0, v0, u1, v0 + tv]),
                    FaceName::Down => (!hidden_bottom, [u0, v1 - tv, u1, v1]),
                };
                let mut face = Face::new(name, active, label.clone());
                let (tl, br) = mc::uv_from_units(rect, mc::JSON_UV_UNITS);
                face.uv.set_unclamped(tl, br);
                face.tint = tint;
                face
            });
            Element {
                base,
                kind: cuboid_model::ElementType::Cube,
                color: marker_color(first_index + n),
                faces,
                flags: ElementFlags::SHADE,
                marker: None,
            }
        })
        .collect();
    (elements, slab)
}

/// Rectangles that cover the opaque pixels of a `w` x `h` frame exactly once: each row's runs
/// extend the rectangle above them when the columns match.
fn rectangles(w: usize, h: usize, opaque: &dyn Fn(usize, usize) -> bool) -> Vec<Rect> {
    let mut done: Vec<Rect> = Vec::new();
    // Open rectangles: first column, end column, first row.
    let mut open: Vec<(usize, usize, usize)> = Vec::new();
    for y in 0..h {
        let mut now = Vec::new();
        let mut x = 0;
        while x < w {
            if !opaque(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < w && opaque(x, y) {
                x += 1;
            }
            match open.iter().position(|o| o.0 == start && o.1 == x) {
                Some(at) => now.push(open.swap_remove(at)),
                None => now.push((start, x, y)),
            }
        }
        done.extend(open.drain(..).map(|(a, b, top)| (a, top, b - a, y - top)));
        open = now;
    }
    done.extend(open.into_iter().map(|(a, b, top)| (a, top, b - a, h - top)));
    done.sort_unstable_by_key(|r| (r.1, r.0));
    done
}

/// The tint index and the picture position of a layer key such as `layer2`.
pub(crate) fn layer_index(key: &str) -> Option<i32> {
    key.strip_prefix("layer")?.parse().ok()
}
