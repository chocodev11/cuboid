//! The brief and the suggestions: both are computed from the entries alone, so the same library
//! always gives the same text.
//!
//! Suggestion rules:
//! 1. A name listed in a lore note (see [`crate::meta::lore_meta`]) with no model of the same
//!    normalised name: lower case, letters and digits only, so "Ink Blaster" matches
//!    `ink_blaster.cubik`.
//! 2. A texture outside the shared palette (the colours that two or more textures use): fewer than half of its colours are within
//!    [`PALETTE_REACH`] (RGB distance) of a colour of the shared palette. Needs at least
//!    [`MIN_PEERS`] textures with colours.
//! 3. A model whose texel density differs from the median of the models by more than
//!    [`DENSITY_TOLERANCE`]. Needs at least [`MIN_PEERS`] models with a density.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::kind::Kind;
use crate::meta::{Meta, PALETTE_COLORS, TextureMeta, median};
use crate::store::{Entry, clean_tags};

/// Colours closer than this (RGB distance) count as the same colour of the palette.
pub const PALETTE_REACH: u32 = 48;
/// Allowed distance of a model's texel density from the median, as a fraction of the median.
pub const DENSITY_TOLERANCE: f32 = 0.25;
/// Models or textures needed before a model or texture can be called an outlier.
pub const MIN_PEERS: usize = 3;
/// Suggestions kept.
pub const MAX_SUGGESTIONS: usize = 30;
const MAX_STYLE_TAGS: usize = 12;
const MAX_NAME_CHARS: usize = 80;

/// What the user tells the library about the project; stored in the index.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Brief {
    /// Name of the project; empty means "not named".
    pub project_name: String,
    /// Target Minecraft version such as `1.21.6`; empty means the newest the program knows.
    pub mc_version: String,
    /// Style words such as `pixel`, `low-poly`, `cartoon`.
    pub style_tags: Vec<String>,
}

impl Brief {
    /// The inputs with trimmed, bounded text.
    pub fn cleaned(self) -> Brief {
        let bound = |text: String| -> String { text.trim().chars().take(MAX_NAME_CHARS).collect() };
        let mut style_tags = clean_tags(&self.style_tags);
        style_tags.truncate(MAX_STYLE_TAGS);
        Brief {
            project_name: bound(self.project_name),
            mc_version: bound(self.mc_version),
            style_tags,
        }
    }
}

/// The brief as the agent and the start screen show it.
#[derive(Clone, Debug, PartialEq)]
pub struct BriefView {
    /// The inputs, with the version filled in.
    pub brief: Brief,
    /// Colours of the textures merged: up to 16, most shared first.
    pub palette: Vec<[u8; 3]>,
    /// Median texel density of the models, texels per block.
    pub density: Option<f32>,
    /// Models in the library.
    pub models: usize,
    /// Textures in the library.
    pub textures: usize,
}

/// The newest Minecraft version of the version table of the program.
fn newest_version() -> String {
    cuboid_assets::mc_versions::mc_versions()
        .tiers
        .last()
        .map_or_else(String::new, |tier| tier.from.to_string())
}

/// Builds the brief of a library.
pub fn brief_view(brief: &Brief, entries: &[Entry]) -> BriefView {
    let mut brief = brief.clone();
    if brief.mc_version.is_empty() {
        brief.mc_version = newest_version();
    }
    let textures: Vec<&TextureMeta> = entries.iter().filter_map(texture_of).collect();
    BriefView {
        brief,
        palette: shared_palette(&textures),
        density: median_density(entries),
        models: entries.iter().filter(|e| e.kind == Kind::Model).count(),
        textures: textures.len(),
    }
}

fn texture_of(entry: &Entry) -> Option<&TextureMeta> {
    match &entry.meta {
        Some(Meta::Texture(texture)) => Some(texture),
        _ => None,
    }
}

fn density_of(entry: &Entry) -> Option<f32> {
    match &entry.meta {
        Some(Meta::Model(model)) => model.density,
        _ => None,
    }
}

/// Median of the model densities.
pub fn median_density(entries: &[Entry]) -> Option<f32> {
    let mut densities: Vec<f32> = entries.iter().filter_map(density_of).collect();
    median(&mut densities)
}

/// Key of the 5-bit bucket of a colour.
fn bucket(color: [u8; 3]) -> u16 {
    (u16::from(color[0] >> 3) << 10) | (u16::from(color[1] >> 3) << 5) | u16::from(color[2] >> 3)
}

/// Merges the palettes: a colour at rank `r` of a texture weighs `16 - r`; colours in the same
/// 5-bit bucket are added up and reported as their weighted mean. Only colours that two or more
/// textures use are shared; when no colour is, every colour counts. Ties go to the lower bucket.
pub fn shared_palette(textures: &[&TextureMeta]) -> Vec<[u8; 3]> {
    struct Slot {
        weight: u64,
        sum: [u64; 3],
        textures: usize,
    }
    let mut buckets: BTreeMap<u16, Slot> = BTreeMap::new();
    for texture in textures {
        let mut seen: Vec<u16> = Vec::new();
        for (rank, color) in texture.palette.iter().enumerate() {
            let weight = (PALETTE_COLORS - rank.min(PALETTE_COLORS - 1)) as u64;
            let key = bucket(*color);
            let slot = buckets.entry(key).or_insert(Slot {
                weight: 0,
                sum: [0; 3],
                textures: 0,
            });
            slot.weight += weight;
            for (sum, value) in slot.sum.iter_mut().zip(color) {
                *sum += weight * u64::from(*value);
            }
            if !seen.contains(&key) {
                seen.push(key);
                slot.textures += 1;
            }
        }
    }
    let shared_only = buckets.values().any(|slot| slot.textures >= 2);
    let mut list: Vec<(u16, Slot)> = buckets
        .into_iter()
        .filter(|(_, slot)| !shared_only || slot.textures >= 2)
        .collect();
    list.sort_by(|a, b| b.1.weight.cmp(&a.1.weight).then(a.0.cmp(&b.0)));
    list.truncate(PALETTE_COLORS);
    list.into_iter()
        .map(|(_, slot)| {
            slot.sum
                .map(|v| u8::try_from((v + slot.weight / 2) / slot.weight).unwrap_or(255))
        })
        .collect()
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    let distance: u32 = a
        .iter()
        .zip(&b)
        .map(|(x, y)| u32::from(x.abs_diff(*y)).pow(2))
        .sum();
    distance <= PALETTE_REACH * PALETTE_REACH
}

/// What a suggestion is about.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum SuggestionKind {
    /// A name from the lore without a model.
    LoreWithoutModel,
    /// A model whose texel density is off.
    DensityMismatch,
    /// A texture whose colours do not fit the shared palette.
    TextureOutsidePalette,
}

/// One hint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Suggestion {
    /// The rule.
    pub kind: SuggestionKind,
    /// The entry it is about: the lore note, the model or the texture.
    pub entry: String,
    /// One line.
    pub title: String,
    /// What to do about it.
    pub detail: String,
}

/// Lower case, letters and digits only.
pub fn normalise(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The suggestions of a library, in a fixed order: lore names, then densities, then textures,
/// each group by name; at most [`MAX_SUGGESTIONS`].
pub fn suggestions(entries: &[Entry]) -> Vec<Suggestion> {
    let mut out = lore_without_model(entries);
    out.extend(density_mismatches(entries));
    out.extend(textures_outside_palette(entries));
    out.truncate(MAX_SUGGESTIONS);
    out
}

fn lore_without_model(entries: &[Entry]) -> Vec<Suggestion> {
    let models: HashSet<String> = entries
        .iter()
        .filter(|e| e.kind == Kind::Model)
        .map(|e| normalise(e.stem()))
        .collect();
    let mut found: BTreeMap<String, (String, String)> = BTreeMap::new();
    for entry in entries {
        let Some(Meta::Lore(lore)) = &entry.meta else {
            continue;
        };
        for name in &lore.names {
            let key = normalise(name);
            if key.is_empty() || models.contains(&key) {
                continue;
            }
            found
                .entry(key)
                .or_insert_with(|| (name.clone(), entry.id.clone()));
        }
    }
    found
        .into_values()
        .map(|(name, entry)| Suggestion {
            kind: SuggestionKind::LoreWithoutModel,
            entry,
            title: format!("{name} has no model"),
            detail: "The lore lists it. Make a model with this name.".to_owned(),
        })
        .collect()
}

fn density_mismatches(entries: &[Entry]) -> Vec<Suggestion> {
    let measured: Vec<(&Entry, f32)> = entries
        .iter()
        .filter_map(|e| density_of(e).map(|d| (e, d)))
        .collect();
    if measured.len() < MIN_PEERS {
        return Vec::new();
    }
    let Some(middle) = median_density(entries).filter(|m| *m > 0.0) else {
        return Vec::new();
    };
    let mut out: Vec<Suggestion> = measured
        .into_iter()
        .filter(|(_, d)| ((d - middle) / middle).abs() > DENSITY_TOLERANCE)
        .map(|(entry, d)| Suggestion {
            kind: SuggestionKind::DensityMismatch,
            entry: entry.id.clone(),
            title: format!("{} has {d:.0} texels per block", entry.name),
            detail: format!(
                "The models in this library have {middle:.0}. Match the texel density."
            ),
        })
        .collect();
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

fn textures_outside_palette(entries: &[Entry]) -> Vec<Suggestion> {
    let textures: Vec<(&Entry, &TextureMeta)> = entries
        .iter()
        .filter_map(|e| texture_of(e).map(|t| (e, t)))
        .filter(|(_, t)| !t.palette.is_empty())
        .collect();
    if textures.len() < MIN_PEERS {
        return Vec::new();
    }
    let all: Vec<&TextureMeta> = textures.iter().map(|(_, t)| *t).collect();
    let shared = shared_palette(&all);
    let mut out: Vec<Suggestion> = textures
        .into_iter()
        .filter(|(_, texture)| {
            let covered = texture
                .palette
                .iter()
                .filter(|color| shared.iter().any(|s| near(*s, **color)))
                .count();
            covered * 2 < texture.palette.len()
        })
        .map(|(entry, _)| Suggestion {
            kind: SuggestionKind::TextureOutsidePalette,
            entry: entry.id.clone(),
            title: format!("{} is outside the shared palette", entry.name),
            detail:
                "Most of its colours are not used by the other textures. Recolour it or accept it."
                    .to_owned(),
        })
        .collect();
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::{AlphaUse, LoreMeta, ModelMeta};
    use crate::store::UNKNOWN_LICENCE;

    fn entry(id: &str, kind: Kind, name: &str, meta: Option<Meta>) -> Entry {
        Entry {
            id: id.to_owned(),
            kind,
            name: name.to_owned(),
            file: format!("{id}-{name}"),
            companion: None,
            source: String::new(),
            sha256: id.to_owned(),
            size: 1,
            tags: Vec::new(),
            licence: UNKNOWN_LICENCE.to_owned(),
            added: 0,
            meta,
        }
    }

    fn model(id: &str, name: &str, density: Option<f32>) -> Entry {
        let meta = ModelMeta {
            density,
            ..ModelMeta::default()
        };
        entry(id, Kind::Model, name, Some(Meta::Model(meta)))
    }

    fn texture(id: &str, name: &str, palette: &[[u8; 3]]) -> Entry {
        let meta = TextureMeta {
            width: 16,
            height: 16,
            frames: 1,
            alpha: Some(AlphaUse::Opaque),
            palette: palette.to_vec(),
        };
        entry(id, Kind::Texture, name, Some(Meta::Texture(meta)))
    }

    fn lore(names: &[&str]) -> Entry {
        let meta = LoreMeta {
            names: names.iter().map(|n| (*n).to_owned()).collect(),
            ..LoreMeta::default()
        };
        entry("l1", Kind::Lore, "story.md", Some(Meta::Lore(meta)))
    }

    #[test]
    fn names_are_compared_without_case_spaces_and_punctuation() {
        assert_eq!(normalise("Ink Blaster!"), "inkblaster");
        assert_eq!(normalise("ink_blaster"), "inkblaster");
        assert_eq!(normalise("Splat-Roller 2"), "splatroller2");
    }

    #[test]
    fn lore_names_without_a_model_are_suggested_once() {
        let entries = [
            model("m1", "ink_blaster.cubik", None),
            lore(&["Ink Blaster", "Roller", "ROLLER", "Charger"]),
        ];
        let found = suggestions(&entries);
        let titles: Vec<&str> = found.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Charger has no model", "Roller has no model"]);
        assert!(
            found
                .iter()
                .all(|s| s.kind == SuggestionKind::LoreWithoutModel && s.entry == "l1")
        );
    }

    #[test]
    fn a_texture_with_foreign_colours_is_outside_the_palette() {
        let reds: &[[u8; 3]] = &[[200, 20, 20], [180, 10, 10], [220, 60, 60]];
        let entries = [
            texture("t1", "a.png", reds),
            texture("t2", "b.png", reds),
            texture("t3", "c.png", reds),
            texture(
                "t4",
                "odd.png",
                &[[10, 200, 20], [20, 240, 30], [0, 180, 0]],
            ),
        ];
        let found = suggestions(&entries);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, SuggestionKind::TextureOutsidePalette);
        assert_eq!(found[0].entry, "t4");
        // Two textures are too few to call one an outlier.
        assert!(suggestions(&entries[2..]).is_empty());
    }

    #[test]
    fn a_model_off_the_median_density_is_suggested() {
        let entries = [
            model("m1", "a.cubik", Some(16.0)),
            model("m2", "b.cubik", Some(16.0)),
            model("m3", "c.cubik", Some(17.0)),
            model("m4", "fine.cubik", Some(32.0)),
            model("m5", "none.cubik", None),
        ];
        let found = suggestions(&entries);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, SuggestionKind::DensityMismatch);
        assert_eq!(found[0].entry, "m4");
        assert_eq!(found[0].title, "fine.cubik has 32 texels per block");
        // Within the tolerance of 25 %.
        let calm = [
            model("m1", "a.cubik", Some(16.0)),
            model("m2", "b.cubik", Some(18.0)),
            model("m3", "c.cubik", Some(19.0)),
        ];
        assert!(suggestions(&calm).is_empty());
    }

    #[test]
    fn the_brief_merges_palettes_and_takes_the_median_density() {
        let entries = [
            texture("t1", "a.png", &[[255, 0, 0], [0, 0, 255]]),
            texture("t2", "b.png", &[[250, 4, 2]]),
            model("m1", "a.cubik", Some(16.0)),
            model("m2", "b.cubik", Some(32.0)),
        ];
        let brief = Brief {
            project_name: "Ink".to_owned(),
            ..Brief::default()
        };
        let view = brief_view(&brief, &entries);
        // The two reds share a bucket; the blue is used by one texture only and is not shared.
        assert_eq!(view.palette.len(), 1);
        assert!(
            view.palette[0][0] > 240 && view.palette[0][2] < 8,
            "{:?}",
            view.palette
        );
        assert_eq!(view.density, Some(24.0));
        assert_eq!((view.models, view.textures), (2, 2));
        assert_eq!(view.brief.project_name, "Ink");
        assert!(
            !view.brief.mc_version.is_empty(),
            "the newest version fills in"
        );
    }

    #[test]
    fn brief_inputs_are_bounded() {
        let brief = Brief {
            project_name: format!("  {}  ", "n".repeat(500)),
            mc_version: " 1.21.6 ".to_owned(),
            style_tags: (0..40).map(|i| format!("tag{i}")).collect(),
        }
        .cleaned();
        assert_eq!(brief.project_name.chars().count(), MAX_NAME_CHARS);
        assert_eq!(brief.mc_version, "1.21.6");
        assert_eq!(brief.style_tags.len(), MAX_STYLE_TAGS);
    }

    #[test]
    fn the_same_library_gives_the_same_suggestions() {
        let entries = [lore(&["B", "A", "C"]), model("m1", "x.cubik", Some(16.0))];
        assert_eq!(suggestions(&entries), suggestions(&entries));
        let titles: Vec<String> = suggestions(&entries).into_iter().map(|s| s.title).collect();
        assert_eq!(
            titles,
            ["A has no model", "B has no model", "C has no model"]
        );
    }
}
