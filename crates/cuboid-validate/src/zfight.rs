//! Coplanar overlapping faces (z-fighting), found with a spatial hash.
//!
//! Only unrotated elements take part: the faces of a rotated box are not axis aligned. Two faces
//! fight when they lie in one plane (within 0.0005), look the same way and overlap by more than
//! 0.001 in both directions. Each face is registered in the 4 x 4 cells that it covers, the
//! entries are sorted by (side, plane, cell) and only faces that share an entry are tested, so
//! the cost follows the number of near faces, not the number of elements.

use cuboid_model::{Element, FaceName, ObjId};
use glam::Vec3;

use crate::codes::GEO_ZFIGHT;
use crate::ctx::{Ctx, Exported};
use crate::issue::{Issue, Severity};

const CELL: f32 = 4.0;
const PLANE_STEP: f32 = 1000.0;
const MIN_OVERLAP: f32 = 1e-3;
/// Elements farther out or larger than this are skipped: they leave every Java range anyway, and
/// a face this wide would fill too many cells.
const FAR: f32 = 128.0;
/// Pair tests stop at this many fighting pairs.
const MAX_PAIRS: usize = 100_000;

#[derive(Copy, Clone)]
pub(crate) struct Quad {
    /// Position of the element in the exported list.
    elem: u32,
    face: FaceName,
    /// Axis in the low bits (0 x, 1 y, 2 z) and 1 << 2 when the face looks toward +.
    side: u8,
    plane: i32,
    u: (f32, f32),
    v: (f32, f32),
}

fn span(a: f32, b: f32) -> (f32, f32) {
    (a.min(b), a.max(b))
}

/// Adds the active faces of an unrotated, finite element.
pub(crate) fn add_quads(elem: u32, el: &Element, out: &mut Vec<Quad>) {
    let (p, s) = (el.base.p, el.base.s);
    if el.base.e != Vec3::ZERO || p.abs().max_element() > FAR || s.abs().max_element() > FAR {
        return;
    }
    let (x, y, z) = (
        span(p.x, p.x + s.x),
        span(p.y, p.y + s.y),
        span(p.z - s.z, p.z),
    );
    for face in el.faces.iter().filter(|face| face.active) {
        // Plane coordinate, axis, outward direction for a positive size, then the two spans.
        let (axis, plane, dir, u, v) = match face.name {
            FaceName::Down => (1u8, p.y, -1.0, x, z),
            FaceName::Up => (1, p.y + s.y, 1.0, x, z),
            FaceName::North => (2, p.z, 1.0, x, y),
            FaceName::South => (2, p.z - s.z, -1.0, x, y),
            FaceName::West => (0, p.x, -1.0, y, z),
            FaceName::East => (0, p.x + s.x, 1.0, y, z),
        };
        let size = match axis {
            0 => s.x,
            1 => s.y,
            _ => s.z,
        };
        // A negative size turns the box inside out, so the faces look the other way.
        let outward = dir * size.signum() > 0.0;
        out.push(Quad {
            elem,
            face: face.name,
            side: axis | (u8::from(outward) << 2),
            plane: (plane * PLANE_STEP).round() as i32,
            u,
            v,
        });
    }
}

fn cell(x: f32) -> i32 {
    (x / CELL).floor() as i32
}

pub(crate) fn run(ctx: &mut Ctx, elements: &[Exported<'_>], quads: &[Quad]) {
    if !ctx.on(GEO_ZFIGHT) || quads.len() < 2 {
        return;
    }
    let mut entries: Vec<((u8, i32, i32, i32), u32)> =
        Vec::with_capacity(quads.len() + quads.len() / 4);
    for (at, quad) in quads.iter().enumerate() {
        let at = at as u32;
        for cu in cell(quad.u.0)..=cell(quad.u.1) {
            for cv in cell(quad.v.0)..=cell(quad.v.1) {
                entries.push(((quad.side, quad.plane, cu, cv), at));
            }
        }
    }
    entries.sort_unstable();

    // (element a, element b) with a < b, and the face of the first quad of the pair.
    let mut pairs: Vec<(u32, u32, FaceName)> = Vec::new();
    let mut capped = false;
    'runs: for run in entries.chunk_by(|a, b| a.0 == b.0) {
        let Some(&(key, _)) = run.first() else {
            continue;
        };
        for (i, &(_, ai)) in run.iter().enumerate() {
            let Some(a) = quads.get(ai as usize) else {
                continue;
            };
            for &(_, bi) in run.iter().skip(i + 1) {
                let Some(b) = quads.get(bi as usize) else {
                    continue;
                };
                if a.elem == b.elem {
                    continue;
                }
                let (u0, v0) = (a.u.0.max(b.u.0), a.v.0.max(b.v.0));
                if a.u.1.min(b.u.1) - u0 <= MIN_OVERLAP || a.v.1.min(b.v.1) - v0 <= MIN_OVERLAP {
                    continue;
                }
                // The pair is found in every cell it covers; keep the one that holds the
                // lower corner of the overlap.
                if cell(u0) != key.2 || cell(v0) != key.3 {
                    continue;
                }
                if pairs.len() >= MAX_PAIRS {
                    capped = true;
                    break 'runs;
                }
                pairs.push((a.elem.min(b.elem), a.elem.max(b.elem), a.face));
            }
        }
    }
    if capped {
        ctx.emit(GEO_ZFIGHT, || {
            Issue::new(
                GEO_ZFIGHT,
                Severity::Warning,
                Vec::new() as Vec<ObjId>,
                format!("more than {MAX_PAIRS} coplanar face pairs; the check stopped early"),
                "Remove duplicated elements, such as copies made in place",
            )
        });
    }
    pairs.sort_unstable_by_key(|&(a, b, _)| (a, b));
    for group in pairs.chunk_by(|x, y| (x.0, x.1) == (y.0, y.1)) {
        let Some(&(a, b, face)) = group.first() else {
            continue;
        };
        let (Some(ea), Some(eb)) = (elements.get(a as usize), elements.get(b as usize)) else {
            continue;
        };
        ctx.emit(GEO_ZFIGHT, || {
            Issue::new(
                GEO_ZFIGHT,
                Severity::Warning,
                vec![ea.id, eb.id] as Vec<ObjId>,
                format!(
                    "{} and {} share {} coplanar overlapping face(s), first the {} face",
                    ea.name(),
                    eb.name(),
                    group.len(),
                    cuboid_model::mc::face_key(face),
                ),
                "Move one element by 0.001 or shrink it, or hide the faces that cannot be seen",
            )
        });
    }
}
