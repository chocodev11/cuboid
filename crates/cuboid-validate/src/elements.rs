//! The per-element pass: numbers, bounds, rotation, faces, texture references, UV, cull and
//! semi-transparent pixels. It also collects the faces that the z-fighting and texel-density
//! rules need.

use cuboid_assets::mc_versions::RotationRule;
use cuboid_model::image::Image;
use cuboid_model::mc;
use cuboid_model::{Document, FaceFlags, FaceName, Label, ObjId, UvRect};
use glam::{Vec2, Vec3};

use crate::codes::*;
use crate::ctx::{Ctx, Exported, Scan};
use crate::issue::{FixOp, Issue, Severity};
use crate::profile::{Flavor, Profile, RenderType};
use crate::zfight::{Quad, add_quads};

/// The Java model JSON range of `from` and `to` (interop 7.5 uses the same editor box).
const BOUNDS: (f32, f32) = (-16.0, 32.0);
const EPS: f32 = 1e-3;

/// One face for the texel-density rule.
pub(crate) struct Density {
    pub elem: u32,
    pub face: FaceName,
    pub texels_per_unit: f32,
}

#[derive(Default)]
pub(crate) struct Collected {
    pub quads: Vec<Quad>,
    pub densities: Vec<Density>,
}

fn face_names(faces: &[FaceName]) -> String {
    let names: Vec<&str> = faces.iter().map(|&face| mc::face_key(face)).collect();
    names.join(", ")
}

/// The rotation as signed degrees, as the JSON names it (interop 2.2.2).
fn signed(e: Vec3) -> [f32; 3] {
    e.to_array().map(|a| if a > 180.0 { a - 360.0 } else { a })
}

/// Why a rotation is not accepted, or `None` when it is.
fn rotation_problem(e: Vec3, rule: RotationRule, max: f32) -> Option<&'static str> {
    let angles = signed(e);
    let turned = angles.iter().filter(|a| a.abs() > 1e-4).count();
    match rule {
        RotationRule::Classic if mc::rotation_fits_json(e) => None,
        RotationRule::Classic if turned > 1 => Some("it turns on more than one axis"),
        RotationRule::Classic if angles.iter().any(|a| a.abs() > 45.0) => {
            Some("an angle is beyond 45 degrees")
        }
        RotationRule::Classic => Some("an angle is not a multiple of 22.5 degrees"),
        RotationRule::SingleAxisFree if turned > 1 => Some("it turns on more than one axis"),
        RotationRule::SingleAxisFree if angles.iter().any(|a| a.abs() > max + 1e-4) => {
            Some("an angle is beyond the limit")
        }
        RotationRule::SingleAxisFree | RotationRule::MultiAxisFree => None,
    }
}

/// The nearest rotation the rule accepts: the strongest axis, snapped, the others zero.
fn rotation_fix(e: Vec3, rule: RotationRule, max: f32) -> Option<Vec3> {
    let angles = signed(e);
    let (axis, value) = angles
        .into_iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))?;
    let snapped = match rule {
        RotationRule::Classic => ((value / 22.5).round_ties_even() * 22.5).clamp(-45.0, 45.0),
        RotationRule::SingleAxisFree => value.clamp(-max, max),
        RotationRule::MultiAxisFree => return None,
    };
    let stored = if snapped < 0.0 {
        snapped + 360.0
    } else {
        snapped
    };
    Some(match axis {
        0 => Vec3::new(stored, 0.0, 0.0),
        1 => Vec3::new(0.0, stored, 0.0),
        _ => Vec3::new(0.0, 0.0, stored),
    })
}

/// The width and height of a face in model units.
fn face_size(name: FaceName, s: Vec3) -> (f32, f32) {
    let s = s.abs();
    match name {
        FaceName::Down | FaceName::Up => (s.x, s.z),
        FaceName::North | FaceName::South => (s.x, s.y),
        FaceName::West | FaceName::East => (s.z, s.y),
    }
}

/// The texel rectangle `[x0, x1) x [y0, y1)` of a face, rows from the bottom as the image has them.
fn texel_crop(uv: &UvRect, image: &Image) -> (u32, u32, u32, u32) {
    let (tl, br) = (uv.tl(), uv.br());
    let (lo, hi) = (tl.min(br), tl.max(br));
    let (w, h) = (image.w as f32, image.h as f32);
    let edge = |v: f32, size: f32, up: bool| {
        let t = v * size;
        (if up { t.ceil() } else { t.floor() }).clamp(0.0, size) as u32
    };
    (
        edge(lo.x, w, false),
        edge(lo.y, h, false),
        edge(hi.x, w, true),
        edge(hi.y, h, true),
    )
}

/// Whether a crop holds a fully transparent texel, which marks a face as cutout.
fn has_clear_texel(image: &Image, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> bool {
    let rgba = image.rgba();
    let w = image.w as usize;
    (y0 as usize..y1.min(image.h) as usize).any(|y| {
        (x0 as usize..x1.min(image.w) as usize).any(|x| rgba.get((y * w + x) * 4 + 3) == Some(&0))
    })
}

pub(crate) fn run(
    ctx: &mut Ctx,
    doc: &Document,
    scan: &mut Scan<'_>,
    profile: &Profile,
) -> Collected {
    let mut found = Collected::default();
    let Scan {
        elements,
        tex_index,
        used,
    } = scan;
    let want_density = ctx.on(UV_DENSITY);
    let want_quads = ctx.on(GEO_ZFIGHT);
    let want_alpha = ctx.on(TEX_ALPHA_CUTOUT);
    let params = ctx.params.clone();
    for (n, ex) in elements.iter().enumerate() {
        let b = &ex.el.base;
        if !(b.p.is_finite() && b.s.is_finite() && b.e.is_finite() && b.o.is_finite()) {
            // Its textures still count as used.
            for face in ex.el.faces.iter().filter(|face| face.active) {
                let at = tex_index.get(&*face.texture).copied();
                if let Some(flag) = at.and_then(|at| used.get_mut(at)) {
                    *flag = true;
                }
            }
            ctx.emit(NUM_NONFINITE, || {
                Issue::new(
                    NUM_NONFINITE,
                    Severity::Error,
                    vec![ex.id],
                    format!(
                        "{} has a position, size, rotation or pivot that is not finite",
                        ex.name()
                    ),
                    "Set the field to a number",
                )
            });
            continue;
        }
        match params.flavor {
            Flavor::Free => {}
            Flavor::Cubik => cubik_element(ctx, ex),
            Flavor::Java => {
                geometry(ctx, ex, &params, profile);
                let n = n as u32;
                if want_quads {
                    add_quads(n, ex.el, &mut found.quads);
                }
                faces(
                    ctx,
                    doc,
                    ex,
                    n,
                    tex_index,
                    used,
                    profile,
                    (want_density, want_alpha),
                    &mut found.densities,
                );
            }
        }
    }
    found
}

fn cubik_element(ctx: &mut Ctx, ex: &Exported<'_>) {
    let folded: Vec<FaceName> = ex
        .el
        .faces
        .iter()
        .filter(|face| face.uv.tl_raw() != face.uv.tl() || face.uv.br_raw() != face.uv.br())
        .map(|face| face.name)
        .collect();
    if folded.is_empty() {
        return;
    }
    ctx.emit(CUBIK_UV_FOLDED, || {
        Issue::new(
            CUBIK_UV_FOLDED,
            Severity::Info,
            vec![ex.id],
            format!(
                "{}: the UV of {} reaches outside 0..1 and is stored folded into it",
                ex.name(),
                face_names(&folded)
            ),
            "The file keeps the folded corners, not the raw ones",
        )
    });
}

fn geometry(ctx: &mut Ctx, ex: &Exported<'_>, params: &crate::profile::Params, profile: &Profile) {
    let b = &ex.el.base;
    if ctx.on(GEO_BOUNDS) {
        let mc_box = mc::unplace(b.p, b.s, b.o, b.e, false);
        let lo = mc_box.from.min(mc_box.to).min_element();
        let hi = mc_box.from.max(mc_box.to).max_element();
        let inflate = profile.inflate.max(0.0);
        if lo - inflate < BOUNDS.0 - EPS || hi + inflate > BOUNDS.1 + EPS {
            ctx.emit(GEO_BOUNDS, || {
                Issue::new(
                    GEO_BOUNDS,
                    Severity::Error,
                    vec![ex.id],
                    format!(
                        "{} spans {:?} to {:?}, outside {} to {}",
                        ex.name(),
                        mc_box.from.to_array(),
                        mc_box.to.to_array(),
                        BOUNDS.0 + inflate,
                        BOUNDS.1 - inflate
                    ),
                    "Keep every corner of the box between -16 and 32",
                )
                .fixed_by(vec![FixOp::ClampIntoBounds {
                    id: ex.id,
                    min: BOUNDS.0 + inflate,
                    max: BOUNDS.1 - inflate,
                }])
            });
        }
    }
    if ctx.on(GEO_ROTATION)
        && let Some(why) = rotation_problem(b.e, params.rotation, params.rotation_max)
    {
        let severity = if params.rotation_verified {
            Severity::Error
        } else {
            Severity::Warning
        };
        ctx.emit(GEO_ROTATION, || {
            let tier = params.tier.map_or("this target", |tier| tier.id.as_str());
            let mut issue = Issue::new(
                GEO_ROTATION,
                severity,
                vec![ex.id],
                format!(
                    "{} rotates {:?}, which {tier} does not accept: {why}",
                    ex.name(),
                    signed(b.e)
                ),
                if params.rotation_verified {
                    "Rotate on one axis by 0, 22.5 or 45 degrees either way"
                } else {
                    "The version table does not confirm this rule (unverified); check it in game"
                },
            );
            if let Some(rotation) = rotation_fix(b.e, params.rotation, params.rotation_max) {
                issue = issue.fixed_by(vec![FixOp::SetRotation {
                    id: ex.id,
                    rotation,
                }]);
            }
            issue
        });
    }
    if b.s.min_element() < 0.0 {
        ctx.emit(GEO_INVERTED, || {
            Issue::new(
                GEO_INVERTED,
                Severity::Info,
                vec![ex.id],
                format!("{} has a negative size and is drawn inside out", ex.name()),
                "Java accepts this; ignore it unless it is a mistake",
            )
        });
    }
}

/// Points the faces at one label; no edit when the texture list is empty.
fn assign_fixes(
    id: ObjId,
    label: Option<&Label>,
    faces: impl Iterator<Item = FaceName>,
) -> Vec<FixOp> {
    let Some(label) = label else {
        return Vec::new();
    };
    faces
        .map(|face| FixOp::AssignTexture {
            id,
            face,
            label: label.clone(),
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn faces(
    ctx: &mut Ctx,
    doc: &Document,
    ex: &Exported<'_>,
    n: u32,
    tex_index: &rustc_hash::FxHashMap<&str, usize>,
    used: &mut [bool],
    profile: &Profile,
    (want_density, want_alpha): (bool, bool),
    densities: &mut Vec<Density>,
) {
    let mut none: Vec<FaceName> = Vec::new();
    let mut unresolved: Vec<(FaceName, &str)> = Vec::new();
    let mut uv_out: Vec<(FaceName, Vec2, Vec2)> = Vec::new();
    let mut cull_bad: Vec<FaceName> = Vec::new();
    let mut alpha_bad: Vec<FaceName> = Vec::new();
    let mut last: Option<(&str, Option<usize>)> = None;

    for face in ex.el.faces.iter().filter(|face| face.active) {
        let label: &str = &face.texture;
        let found = if label.is_empty() {
            none.push(face.name);
            None
        } else {
            match last {
                Some((seen, at)) if seen == label => at,
                _ => {
                    let at = tex_index.get(label).copied();
                    last = Some((label, at));
                    at
                }
            }
        };
        if let Some(at) = found {
            if let Some(flag) = used.get_mut(at) {
                *flag = true;
            }
        } else if !label.is_empty() {
            unresolved.push((face.name, label));
        }

        let (tl, br) = (face.uv.tl(), face.uv.br());
        let (lo, hi) = (tl.min(br), tl.max(br));
        let inside = lo.x >= -EPS && lo.y >= -EPS && hi.x <= 1.0 + EPS && hi.y <= 1.0 + EPS;
        if !inside {
            uv_out.push((
                face.name,
                tl.clamp(Vec2::ZERO, Vec2::ONE),
                br.clamp(Vec2::ZERO, Vec2::ONE),
            ));
        }
        if face.cull.bits().count_ones() > 1 {
            cull_bad.push(face.name);
        }

        let Some(image) = found
            .and_then(|at| doc.textures.entries.get(at))
            .map(|entry| &*entry.texture.image)
        else {
            continue;
        };
        if want_density {
            let (fw, fh) = face_size(face.name, ex.el.base.s);
            let texels = (br - tl).abs() * Vec2::new(image.w as f32, image.h as f32);
            let area = fw * fh;
            if area > 1e-4 && texels.x * texels.y > 1e-6 {
                densities.push(Density {
                    elem: n,
                    face: face.name,
                    texels_per_unit: (texels.x * texels.y / area).sqrt(),
                });
            }
        }
        if want_alpha && profile.render_type != RenderType::Translucent {
            let crop = texel_crop(&face.uv, image);
            if image.alpha().any_partial(crop.0, crop.1, crop.2, crop.3)
                && (profile.render_type == RenderType::Cutout || has_clear_texel(image, crop))
            {
                alpha_bad.push(face.name);
            }
        }
    }

    if !none.is_empty() || !unresolved.is_empty() {
        let fill = (!doc.textures.entries.is_empty()).then(|| doc.textures.default_face_label());
        if !none.is_empty() {
            ctx.emit(TEX_FACE_NONE, || {
                Issue::new(
                    TEX_FACE_NONE,
                    Severity::Error,
                    vec![ex.id],
                    format!("{}: {} has no texture", ex.name(), face_names(&none)),
                    "Assign a texture, or disable the face if it cannot be seen",
                )
                .fixed_by(assign_fixes(ex.id, fill.as_ref(), none.iter().copied()))
            });
        }
        if !unresolved.is_empty() {
            ctx.emit(TEX_UNRESOLVED, || {
                let named: Vec<String> = unresolved
                    .iter()
                    .map(|(face, label)| format!("{} -> #{label}", mc::face_key(*face)))
                    .collect();
                Issue::new(
                    TEX_UNRESOLVED,
                    Severity::Error,
                    vec![ex.id],
                    format!("{}: no texture entry for {}", ex.name(), named.join(", ")),
                    "Add the texture with that label, or point the face at an existing one",
                )
                .fixed_by(assign_fixes(
                    ex.id,
                    fill.as_ref(),
                    unresolved.iter().map(|&(face, _)| face),
                ))
            });
        }
    }
    if !uv_out.is_empty() {
        ctx.emit(UV_RANGE, || {
            let faces: Vec<FaceName> = uv_out.iter().map(|&(face, ..)| face).collect();
            Issue::new(
                UV_RANGE,
                Severity::Error,
                vec![ex.id],
                format!(
                    "{}: the UV of {} reaches outside the texture (0..16)",
                    ex.name(),
                    face_names(&faces)
                ),
                "Keep the face rectangle within the texture",
            )
            .fixed_by(
                uv_out
                    .iter()
                    .map(|&(face, tl, br)| FixOp::SetUv {
                        id: ex.id,
                        face,
                        tl,
                        br,
                    })
                    .collect(),
            )
        });
    }
    if !cull_bad.is_empty() {
        ctx.emit(FACE_CULL, || {
            Issue::new(
                FACE_CULL,
                Severity::Warning,
                vec![ex.id],
                format!(
                    "{}: the cull direction of {} is not a single side",
                    ex.name(),
                    face_names(&cull_bad)
                ),
                "A cullface names one side: down, up, north, south, west or east",
            )
            .fixed_by(
                cull_bad
                    .iter()
                    .map(|&face| FixOp::SetCull {
                        id: ex.id,
                        face,
                        cull: FaceFlags::empty(),
                    })
                    .collect(),
            )
        });
    }
    if !alpha_bad.is_empty() {
        ctx.emit(TEX_ALPHA_CUTOUT, || {
            Issue::new(
                TEX_ALPHA_CUTOUT,
                Severity::Warning,
                vec![ex.id],
                format!(
                    "{}: {} shows semi-transparent pixels on a cutout face",
                    ex.name(),
                    face_names(&alpha_bad)
                ),
                "A cutout face draws each pixel fully or not at all; make the alpha 0 or 255, or use a translucent block",
            )
        });
    }
}
