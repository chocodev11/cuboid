//! The rules of the project: the targets and the naming of the brief that the application hands
//! over in [`ProjectRules`]. They hold for every target; a rule whose value is unset is silent.

use std::cmp::Reverse;

use cuboid_model::image::Image;
use cuboid_model::{Document, Label};
use rustc_hash::FxHashMap;

use crate::codes::*;
use crate::ctx::{Ctx, Scan};
use crate::elements::texels_per_unit;
use crate::issue::{FixOp, Issue, Severity};
use crate::profile::{NameCase, Profile, ProjectRules};
use crate::textures::effective_path;

/// Model units in a block.
const UNITS_PER_BLOCK: f32 = 16.0;
/// Colours of a texture that the message lists.
const LISTED_COLORS: usize = 8;
/// Texels looked at per texture; a larger picture is sampled on a grid.
const PALETTE_SAMPLE: u64 = 1 << 20;
/// Texels with less alpha than this do not count as a colour.
const MIN_ALPHA: u8 = 128;

pub(crate) fn run(ctx: &mut Ctx, doc: &Document, scan: &Scan<'_>, profile: &Profile) {
    let project = &profile.project;
    density(ctx, doc, scan, project, profile.texel_ratio);
    texture_size(ctx, doc, project);
    palette(ctx, doc, project);
    names(ctx, doc, scan, project);
    namespace(ctx, doc, profile);
}

fn density(ctx: &mut Ctx, doc: &Document, scan: &Scan<'_>, project: &ProjectRules, ratio: f32) {
    let Some(target) = project.texel_density.filter(|t| t.is_finite() && *t > 0.0) else {
        return;
    };
    if !ctx.on(PRJ_DENSITY) || ratio.is_nan() || ratio < 1.0 {
        return;
    }
    for ex in &scan.elements {
        let size = ex.el.base.s;
        if !size.is_finite() {
            continue;
        }
        // The face that is farthest from the target, in either direction.
        let worst = ex
            .el
            .faces
            .iter()
            .filter(|face| face.active)
            .filter_map(|face| {
                let at = scan.tex_index.get(&*face.texture)?;
                let image = &doc.textures.entries.get(*at)?.texture.image;
                let per_block = texels_per_unit(face, size, image)? * UNITS_PER_BLOCK;
                let factor = per_block / target;
                (factor > ratio || factor < 1.0 / ratio).then_some((face.name, per_block, factor))
            })
            .max_by(|a, b| a.2.max(1.0 / a.2).total_cmp(&b.2.max(1.0 / b.2)));
        let Some((face, per_block, _)) = worst else {
            continue;
        };
        ctx.emit(PRJ_DENSITY, || {
            Issue::new(
                PRJ_DENSITY,
                Severity::Warning,
                vec![ex.id],
                format!(
                    "{}: the {} face has {per_block:.1} texels per block, the project asks for {target}",
                    ex.name(),
                    cuboid_model::mc::face_key(face)
                ),
                "Resize the face rectangle or the texture so that the face shows the texel density of the brief",
            )
        });
    }
}

fn texture_size(ctx: &mut Ctx, doc: &Document, project: &ProjectRules) {
    let Some(limit) = project.texture_size else {
        return;
    };
    for entry in &doc.textures.entries {
        let image = &entry.texture.image;
        if image.w.max(image.h) <= limit {
            continue;
        }
        let label = &entry.label;
        ctx.emit(PRJ_TEXTURE_SIZE, || {
            Issue::new(
                PRJ_TEXTURE_SIZE,
                Severity::Warning,
                Vec::new(),
                format!(
                    "texture \"{label}\" is {} x {}, over the project limit of {limit}",
                    image.w, image.h
                ),
                "Scale the texture down, or raise the limit in the project brief",
            )
        });
    }
}

/// The colours of the picture that are farther than `reach` from every colour of the palette,
/// with the number of texels each fills, most used first.
fn colors_outside(image: &Image, palette: &[[u8; 3]], reach: u32) -> Vec<([u8; 3], u32)> {
    let texels = u64::from(image.w) * u64::from(image.h);
    let step = if texels <= PALETTE_SAMPLE {
        1
    } else {
        usize::try_from((texels / PALETTE_SAMPLE).isqrt() + 1).unwrap_or(1)
    };
    let width = (image.w as usize).max(1);
    let mut counts: FxHashMap<[u8; 3], u32> = FxHashMap::default();
    for (index, texel) in image.rgba().chunks_exact(4).enumerate() {
        let &[r, g, b, a] = texel else { continue };
        let off_grid = step > 1
            && (!(index % width).is_multiple_of(step) || !(index / width).is_multiple_of(step));
        if a < MIN_ALPHA || off_grid {
            continue;
        }
        *counts.entry([r, g, b]).or_insert(0) += 1;
    }
    let near = |color: &[u8; 3]| {
        palette.iter().any(|known| {
            let distance: u32 = known
                .iter()
                .zip(color)
                .map(|(x, y)| u32::from(x.abs_diff(*y)).pow(2))
                .sum();
            distance <= reach * reach
        })
    };
    let mut far: Vec<([u8; 3], u32)> = counts
        .into_iter()
        .filter(|(color, _)| !near(color))
        .collect();
    far.sort_by_key(|&(color, count)| (Reverse(count), color));
    far
}

fn palette(ctx: &mut Ctx, doc: &Document, project: &ProjectRules) {
    if project.palette.is_empty() || !ctx.on(PRJ_PALETTE) {
        return;
    }
    let severity = if project.palette_strict {
        Severity::Error
    } else {
        Severity::Warning
    };
    for entry in &doc.textures.entries {
        let far = colors_outside(
            &entry.texture.image,
            &project.palette,
            project.palette_reach,
        );
        if far.is_empty() {
            continue;
        }
        let label = &entry.label;
        ctx.emit(PRJ_PALETTE, || {
            let listed: Vec<String> = far
                .iter()
                .take(LISTED_COLORS)
                .map(|([r, g, b], _)| format!("#{r:02x}{g:02x}{b:02x}"))
                .collect();
            let more = match far.len().saturating_sub(LISTED_COLORS) {
                0 => String::new(),
                n => format!(" and {n} more"),
            };
            Issue::new(
                PRJ_PALETTE,
                severity,
                Vec::new(),
                format!(
                    "texture \"{label}\" has {} colour(s) outside the project palette: {}{more}",
                    far.len(),
                    listed.join(", ")
                ),
                "Recolour the texture with the colours of the palette, or add them to the palette in the project brief",
            )
        });
    }
}

/// Whether the name has the case.
fn has_case(name: &str, case: NameCase) -> bool {
    let separator = match case {
        NameCase::Any => return true,
        NameCase::Snake => '_',
        NameCase::Kebab => '-',
    };
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == separator)
}

/// The name in the case: capitals start a new word, anything but letters and digits is a word
/// break, and a break is written once.
fn in_case(name: &str, case: NameCase) -> String {
    let separator = match case {
        NameCase::Any => return name.to_owned(),
        NameCase::Snake => '_',
        NameCase::Kebab => '-',
    };
    let mut out = String::new();
    let mut after_lower = false;
    for c in name.trim().chars() {
        if c.is_ascii_uppercase() {
            if after_lower {
                out.push(separator);
            }
            out.push(c.to_ascii_lowercase());
            after_lower = false;
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            after_lower = true;
        } else {
            if !out.is_empty() && !out.ends_with(separator) {
                out.push(separator);
            }
            after_lower = false;
        }
    }
    out.trim_end_matches(separator).to_owned()
}

/// What is wrong with a name, or `None`. The prefix is checked only for names that take one.
fn name_problem(name: &str, project: &ProjectRules, prefixed: bool) -> Option<String> {
    let mut problems = Vec::new();
    if !has_case(name, project.case) {
        problems.push(match project.case {
            NameCase::Kebab => "is not kebab-case (lower-case letters, digits and -)",
            _ => "is not snake_case (lower-case letters, digits and _)",
        });
    }
    if prefixed && !project.prefix.is_empty() && !name.starts_with(&project.prefix) {
        problems.push("does not start with the prefix");
    }
    (!problems.is_empty()).then(|| problems.join(" and "))
}

fn names(ctx: &mut Ctx, doc: &Document, scan: &Scan<'_>, project: &ProjectRules) {
    if project.case == NameCase::Any && project.prefix.is_empty() {
        return;
    }
    let hint = "Rename it as the naming rule of the project brief says";
    let file = &project.model_name;
    if !file.is_empty()
        && let Some(why) = name_problem(file, project, true)
    {
        ctx.emit(PRJ_NAME, || {
            Issue::new(
                PRJ_NAME,
                Severity::Info,
                Vec::new(),
                format!("the model file name \"{file}\" {why}"),
                hint,
            )
        });
    }
    for entry in &doc.textures.entries {
        let label: &Label = &entry.label;
        if let Some(why) = name_problem(label, project, true) {
            ctx.emit(PRJ_NAME, || {
                Issue::new(
                    PRJ_NAME,
                    Severity::Info,
                    Vec::new(),
                    format!("texture \"{label}\" {why}"),
                    hint,
                )
            });
        }
    }
    // The prefix is for files and textures; the elements only follow the case.
    for ex in &scan.elements {
        let name: &str = &ex.el.base.label;
        if has_case(name, project.case) {
            continue;
        }
        ctx.emit(PRJ_NAME, || {
            let renamed = in_case(name, project.case);
            let issue = Issue::new(
                PRJ_NAME,
                Severity::Info,
                vec![ex.id],
                format!(
                    "{} {}",
                    ex.name(),
                    name_problem(name, project, false).unwrap_or_default()
                ),
                hint,
            );
            if renamed.is_empty() {
                issue
            } else {
                issue.fixed_by(vec![FixOp::RenameElement {
                    id: ex.id,
                    name: renamed,
                }])
            }
        });
    }
}

fn namespace(ctx: &mut Ctx, doc: &Document, profile: &Profile) {
    let wanted = profile.project.namespace.as_str();
    if wanted.is_empty() {
        return;
    }
    for entry in &doc.textures.entries {
        let path = effective_path(&entry.texture.source);
        // A texture without a path or a shortcut to another label has no namespace of its own.
        if path.is_empty() || path.starts_with('#') {
            continue;
        }
        // An explicit `minecraft:` path points at a vanilla texture on purpose. A path without a
        // namespace resolves to `minecraft` too, which in a project with its own namespace is
        // usually a slip, so only that one is reported.
        let (namespace, rest) = match path.split_once(':') {
            Some(("minecraft", _)) => continue,
            Some(split) => split,
            None => ("minecraft", &path[..]),
        };
        if namespace == wanted {
            continue;
        }
        let label = &entry.label;
        ctx.emit(PRJ_NAMESPACE, || {
            let fixed = format!("{wanted}:{rest}");
            Issue::new(
                PRJ_NAMESPACE,
                Severity::Warning,
                Vec::new(),
                format!(
                    "texture \"{label}\", path \"{path}\" is in the namespace {namespace}, the project uses {wanted}"
                ),
                format!("Use \"{fixed}\""),
            )
            .fixed_by(vec![FixOp::SetTexturePath {
                label: label.clone(),
                path: fixed,
            }])
        });
    }
    if let Some(model) = profile.pack.as_ref().and_then(|pack| pack.model.as_ref())
        && model.namespace != wanted
    {
        ctx.emit(PRJ_NAMESPACE, || {
            Issue::new(
                PRJ_NAMESPACE,
                Severity::Warning,
                Vec::new(),
                format!(
                    "the item id {}:{} is in the namespace {}, the project uses {wanted}",
                    model.namespace, model.id, model.namespace
                ),
                "Give the item an id in the namespace of the project",
            )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_converted_into_the_case() {
        assert_eq!(in_case("Ink Blaster", NameCase::Snake), "ink_blaster");
        assert_eq!(in_case("BlockModel", NameCase::Snake), "block_model");
        assert_eq!(in_case("Cube #2", NameCase::Snake), "cube_2");
        assert_eq!(in_case("  top--Plate ", NameCase::Kebab), "top-plate");
        assert_eq!(in_case("already_fine", NameCase::Snake), "already_fine");
        assert_eq!(in_case("###", NameCase::Snake), "");
        assert_eq!(in_case("Keep Me", NameCase::Any), "Keep Me");
        for name in ["Ink Blaster", "BlockModel", "Cube #2", "x__Y"] {
            assert!(
                has_case(&in_case(name, NameCase::Snake), NameCase::Snake),
                "{name}"
            );
        }
        assert!(has_case("a-b2", NameCase::Kebab) && !has_case("a_b", NameCase::Kebab));
        assert!(!has_case("", NameCase::Snake) && has_case("", NameCase::Any));
    }
}
