//! The brief and the suggestions: both are computed from the entries alone, so the same library
//! always gives the same text.
//!
//! Suggestion rules:
//! 1. A name listed in a lore note (see [`crate::meta::lore_meta`]) with no model of the same
//!    normalised name: lower case, letters and digits only, so "Ink Blaster" matches
//!    `ink_blaster.cubik`.
//! 2. A texture outside the palette: fewer than half of its colours are within [`PALETTE_REACH`]
//!    (RGB distance) of a colour of the palette. The palette is the one of the brief when it has
//!    one; otherwise the shared palette (the colours that two or more textures use), which needs
//!    at least [`MIN_PEERS`] textures with colours.
//! 3. A model whose texel density differs from the target by more than [`DENSITY_TOLERANCE`].
//!    The target is the one of the brief when it has one; otherwise the median of the models,
//!    which needs at least [`MIN_PEERS`] models with a density.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use cuboid_assets::mc_versions::{McVersion, RotationRule, mc_versions};
use cuboid_fmt_minecraft::builtin_display;
use cuboid_model::{Document, ModelKind};
use cuboid_validate::{NameCase as ValidateCase, Profile, ProjectRules};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::kind::{Kind, Role};
use crate::meta::{Meta, PALETTE_COLORS, TextureMeta, median};
use crate::store::{Entry, clean_licence, clean_tags};

/// Colours closer than this (RGB distance) count as the same colour of the palette.
pub const PALETTE_REACH: u32 = 48;
/// Allowed distance of a model's texel density from the median, as a fraction of the median.
pub const DENSITY_TOLERANCE: f32 = 0.25;
/// Models or textures needed before a model or texture can be called an outlier.
pub const MIN_PEERS: usize = 3;
/// Suggestions kept.
pub const MAX_SUGGESTIONS: usize = 30;
/// The texture sides a brief can allow.
pub const TEXTURE_SIZES: [u32; 6] = [16, 32, 64, 128, 256, 512];
const MAX_STYLE_TAGS: usize = 12;
const MAX_NAME_CHARS: usize = 80;
const MAX_GOAL_CHARS: usize = 2000;
const MAX_NAMESPACE_CHARS: usize = 64;
const MAX_PREFIX_CHARS: usize = 32;
const MAX_PALETTE: usize = 64;
const MAX_RULES: usize = 20;
const MAX_RULE_CHARS: usize = 200;
/// Folders of style references a project remembers.
pub const MAX_STYLE_SOURCES: usize = 16;
/// Largest texel density a brief asks for, texels per block.
const MAX_DENSITY: f32 = 4096.0;
/// Largest element budget a brief asks for; the Minecraft importer takes no more.
const MAX_BUDGET: u32 = 1 << 16;

/// Where the models of the project go.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// A plain resource pack.
    #[default]
    Vanilla,
    /// ModelEngine.
    ModelEngine,
    /// ItemsAdder.
    ItemsAdder,
    /// Oraxen.
    Oraxen,
    /// GeckoLib.
    Geckolib,
    /// Anything else; also what an unknown name reads as.
    #[serde(other)]
    Other,
}

impl Platform {
    /// Every platform, in menu order.
    pub const ALL: [Platform; 6] = [
        Platform::Vanilla,
        Platform::ModelEngine,
        Platform::ItemsAdder,
        Platform::Oraxen,
        Platform::Geckolib,
        Platform::Other,
    ];

    /// The name the index and the JSON use.
    pub fn id(self) -> &'static str {
        match self {
            Platform::Vanilla => "vanilla",
            Platform::ModelEngine => "model_engine",
            Platform::ItemsAdder => "items_adder",
            Platform::Oraxen => "oraxen",
            Platform::Geckolib => "geckolib",
            Platform::Other => "other",
        }
    }

    /// The platform with this name; `Other` for an unknown one.
    pub fn from_id(id: &str) -> Platform {
        Platform::ALL
            .into_iter()
            .find(|platform| platform.id() == id)
            .unwrap_or(Platform::Other)
    }
}

/// How the names of model files, textures and elements are cased.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameCase {
    /// `ink_blaster`.
    Snake,
    /// `ink-blaster`.
    Kebab,
    /// No rule; also what an unknown name reads as.
    #[default]
    #[serde(other)]
    Any,
}

impl NameCase {
    /// The name the index and the JSON use.
    pub fn id(self) -> &'static str {
        match self {
            NameCase::Snake => "snake",
            NameCase::Kebab => "kebab",
            NameCase::Any => "any",
        }
    }

    /// The case with this name; `Any` for an unknown one.
    pub fn from_id(id: &str) -> NameCase {
        [NameCase::Snake, NameCase::Kebab]
            .into_iter()
            .find(|case| case.id() == id)
            .unwrap_or(NameCase::Any)
    }
}

/// The parent model whose display settings new models start from.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum DisplayPreset {
    /// `item/generated`.
    #[serde(rename = "item/generated")]
    ItemGenerated,
    /// `item/handheld`.
    #[serde(rename = "item/handheld")]
    ItemHandheld,
    /// `block/block`.
    #[serde(rename = "block/block")]
    BlockBlock,
}

impl DisplayPreset {
    /// Every preset, in menu order.
    pub const ALL: [DisplayPreset; 3] = [
        DisplayPreset::ItemGenerated,
        DisplayPreset::ItemHandheld,
        DisplayPreset::BlockBlock,
    ];

    /// The parent model id, which is also the name the index uses.
    pub fn id(self) -> &'static str {
        match self {
            DisplayPreset::ItemGenerated => "item/generated",
            DisplayPreset::ItemHandheld => "item/handheld",
            DisplayPreset::BlockBlock => "block/block",
        }
    }

    /// The preset with this parent model id.
    pub fn from_id(id: &str) -> Option<DisplayPreset> {
        DisplayPreset::ALL
            .into_iter()
            .find(|preset| preset.id() == id)
    }
}

/// Whether a game version restricts rotations to one axis in steps (the document flag "Restrict
/// rotations"): every version but the ones with free rotations on all axes. A version the table
/// does not know gets the strictest rule.
pub fn restrict_rotations(version: McVersion) -> bool {
    let rule = mc_versions().tier_for(version).map(|tier| tier.rotation);
    rule != Some(RotationRule::MultiAxisFree)
}

/// Brings the document of a new Minecraft model to its template: the rotation rule of the game
/// version, the model kind (`item`, else block) and, when the brief names a display preset, its
/// display slots.
pub fn minecraft_template(
    doc: &mut Document,
    version: McVersion,
    item: bool,
    display: Option<DisplayPreset>,
) {
    let props = Arc::make_mut(&mut doc.props);
    props.general.restrict_rotations = restrict_rotations(version);
    if item {
        props.minecraft.kind = ModelKind::Item;
    }
    if let Some(parent) = display.map(DisplayPreset::id)
        && let Some(slots) = builtin_display(parent)
    {
        props.minecraft.include_display = slots.slots.iter().any(|slot| slot.enabled);
        let shown = Arc::make_mut(&mut doc.display);
        *shown = slots;
        for slot in shown.slots.iter_mut().filter(|slot| slot.enabled) {
            slot.preset = parent.to_owned();
        }
    }
}

/// An unknown or mistyped preset reads as none, so a hand-edited index still opens.
fn display_preset_or_none<'de, D: Deserializer<'de>>(
    reader: D,
) -> Result<Option<DisplayPreset>, D::Error> {
    let value = Value::deserialize(reader)?;
    Ok(value.as_str().and_then(DisplayPreset::from_id))
}

/// How the project names its files, textures and elements.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Naming {
    /// The case of the names.
    pub case: NameCase,
    /// What model files and textures start with; empty means nothing.
    pub prefix: String,
}

/// A folder of the project that holds style references, remembered so that Rescan can read it
/// again.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StyleSource {
    /// The folder, relative to the project folder, with `/` separators.
    pub path: String,
    /// The licence of its references.
    pub licence: String,
    /// The tags of its references.
    pub tags: Vec<String>,
    /// When it was last read, seconds since 1970-01-01 UTC.
    pub scanned: i64,
}

/// What the user tells the library about the project; stored in the index (schema 3).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Brief {
    /// Name of the project; empty means "not named".
    pub project_name: String,
    /// Target Minecraft version such as `1.21.6`; empty means the newest the program knows.
    pub mc_version: String,
    /// Style words such as `pixel`, `low-poly`, `cartoon`.
    pub style_tags: Vec<String>,
    /// What the project is for, in the user's words; at most 2000 characters.
    pub goal: String,
    /// Where the models go.
    pub platform: Platform,
    /// Resource id namespace such as `splatcraft`: at most 64 characters of `[a-z0-9_.-]`;
    /// empty means none.
    pub namespace: String,
    /// Target texels per block.
    pub texel_density: Option<f32>,
    /// Largest texture side the project allows; one of [`TEXTURE_SIZES`].
    pub texture_size: Option<u32>,
    /// The project palette, at most 64 colours; empty means none.
    pub palette: Vec<[u8; 3]>,
    /// Texture colours outside the palette are errors instead of warnings.
    pub palette_strict: bool,
    /// Elements allowed per model.
    pub max_elements: Option<u32>,
    /// How names are cased and prefixed.
    pub naming: Naming,
    /// The display settings new models start from.
    #[serde(deserialize_with = "display_preset_or_none")]
    pub display_preset: Option<DisplayPreset>,
    /// Do and don't rules in the user's words: at most 20 lines of at most 200 characters.
    pub rules: Vec<String>,
    /// The folders of style references, at most 16.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub style_sources: Vec<StyleSource>,
}

/// A folder of the project as the sources keep it: the names joined with `/`. `None` for a path
/// that is empty, leaves the project or names a drive or a stream.
pub(crate) fn source_path(text: &str) -> Option<String> {
    let text = text.replace('\\', "/");
    let mut parts = Vec::new();
    for part in text
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
    {
        if part == ".." || part.contains(':') {
            return None;
        }
        parts.push(part);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

impl Brief {
    /// The inputs with trimmed, bounded text and numbers; values that cannot be used read as none.
    pub fn cleaned(self) -> Brief {
        let bound = |text: &str, max: usize| -> String { text.trim().chars().take(max).collect() };
        let mut style_tags = clean_tags(&self.style_tags);
        style_tags.truncate(MAX_STYLE_TAGS);
        let mut palette: Vec<[u8; 3]> = Vec::new();
        for color in self.palette {
            if !palette.contains(&color) {
                palette.push(color);
            }
            if palette.len() == MAX_PALETTE {
                break;
            }
        }
        let rules = self
            .rules
            .iter()
            .flat_map(|rule| rule.lines())
            .map(|line| bound(line, MAX_RULE_CHARS))
            .filter(|line| !line.is_empty())
            .take(MAX_RULES)
            .collect();
        let namespace = self
            .namespace
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.')
            })
            .take(MAX_NAMESPACE_CHARS)
            .collect();
        let mut style_sources: Vec<StyleSource> = Vec::new();
        for source in self.style_sources {
            let Some(path) = source_path(&source.path) else {
                continue;
            };
            if style_sources.iter().any(|known| known.path == path) {
                continue;
            }
            style_sources.push(StyleSource {
                path,
                licence: clean_licence(&source.licence),
                tags: clean_tags(&source.tags),
                scanned: source.scanned,
            });
            if style_sources.len() == MAX_STYLE_SOURCES {
                break;
            }
        }
        Brief {
            project_name: bound(&self.project_name, MAX_NAME_CHARS),
            mc_version: bound(&self.mc_version, MAX_NAME_CHARS),
            style_tags,
            goal: bound(&self.goal.replace('\r', ""), MAX_GOAL_CHARS),
            platform: self.platform,
            namespace,
            texel_density: self
                .texel_density
                .filter(|d| d.is_finite() && *d > 0.0 && *d <= MAX_DENSITY),
            texture_size: self.texture_size.filter(|s| TEXTURE_SIZES.contains(s)),
            palette,
            palette_strict: self.palette_strict,
            max_elements: self.max_elements.filter(|n| (1..=MAX_BUDGET).contains(n)),
            naming: Naming {
                case: self.naming.case,
                prefix: bound(&self.naming.prefix, MAX_PREFIX_CHARS),
            },
            display_preset: self.display_preset,
            rules,
            style_sources,
        }
    }
}

impl Brief {
    /// Puts what the brief asks of a model into a validation profile: the element budget and the
    /// project rules. `model_name` is the file name of the model without its extension, empty
    /// for a model that was never saved. The target stays the caller's.
    pub fn fill_profile(&self, profile: &mut Profile, model_name: &str) {
        if self.max_elements.is_some() {
            profile.max_elements = self.max_elements;
        }
        profile.project = ProjectRules {
            texel_density: self.texel_density,
            texture_size: self.texture_size,
            palette: self.palette.clone(),
            palette_strict: self.palette_strict,
            palette_reach: PALETTE_REACH,
            namespace: self.namespace.clone(),
            case: match self.naming.case {
                NameCase::Any => ValidateCase::Any,
                NameCase::Snake => ValidateCase::Snake,
                NameCase::Kebab => ValidateCase::Kebab,
            },
            prefix: self.naming.prefix.clone(),
            model_name: model_name.to_owned(),
        };
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

/// The entries that are the project's own work: the observed values and the suggestions leave the
/// style references out, which have a summary of their own.
fn own(entries: &[Entry]) -> impl Iterator<Item = &Entry> {
    entries.iter().filter(|entry| entry.role == Role::Work)
}

/// Builds the brief of a library.
pub fn brief_view(brief: &Brief, entries: &[Entry]) -> BriefView {
    let mut brief = brief.clone();
    if brief.mc_version.is_empty() {
        brief.mc_version = newest_version();
    }
    let textures: Vec<&TextureMeta> = own(entries).filter_map(texture_of).collect();
    BriefView {
        brief,
        palette: shared_palette(textures.iter().map(|texture| texture.palette.as_slice())),
        density: median_density(entries),
        models: own(entries).filter(|e| e.kind == Kind::Model).count(),
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
    let mut densities: Vec<f32> = own(entries).filter_map(density_of).collect();
    median(&mut densities)
}

/// Key of the 5-bit bucket of a colour.
fn bucket(color: [u8; 3]) -> u16 {
    (u16::from(color[0] >> 3) << 10) | (u16::from(color[1] >> 3) << 5) | u16::from(color[2] >> 3)
}

/// Merges the palettes: a colour at rank `r` of a texture weighs `16 - r`; colours in the same
/// 5-bit bucket are added up and reported as their weighted mean. Only colours that two or more
/// textures use are shared; when no colour is, every colour counts. Ties go to the lower bucket.
pub fn shared_palette<'a>(palettes: impl IntoIterator<Item = &'a [[u8; 3]]>) -> Vec<[u8; 3]> {
    struct Slot {
        weight: u64,
        sum: [u64; 3],
        textures: usize,
    }
    let mut buckets: BTreeMap<u16, Slot> = BTreeMap::new();
    for palette in palettes {
        let mut seen: Vec<u16> = Vec::new();
        for (rank, color) in palette.iter().enumerate() {
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

/// How many of the colours are not within [`PALETTE_REACH`] of a colour of the palette.
pub fn colours_outside(palette: &[[u8; 3]], colours: &[[u8; 3]]) -> usize {
    colours
        .iter()
        .filter(|color| !palette.iter().any(|wanted| near(*wanted, **color)))
        .count()
}

/// Whether colours are outside the palette: fewer than half of them are near a colour of it.
pub fn outside_palette(palette: &[[u8; 3]], colours: &[[u8; 3]]) -> bool {
    colours_outside(palette, colours) * 2 > colours.len()
}

/// What a suggestion is about.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum SuggestionKind {
    /// A name from the lore without a model.
    LoreWithoutModel,
    /// A model whose texel density is off.
    DensityMismatch,
    /// A texture whose colours do not fit the palette.
    TextureOutsidePalette,
}

impl SuggestionKind {
    /// The name the JSON uses.
    pub fn name(self) -> &'static str {
        match self {
            SuggestionKind::LoreWithoutModel => "lore_without_model",
            SuggestionKind::DensityMismatch => "density_mismatch",
            SuggestionKind::TextureOutsidePalette => "texture_outside_palette",
        }
    }
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
/// each group by name; at most [`MAX_SUGGESTIONS`]. The targets of the brief replace the median
/// density and the shared palette of the library where they are set.
pub fn suggestions(brief: &Brief, entries: &[Entry]) -> Vec<Suggestion> {
    let mut out = lore_without_model(entries);
    out.extend(density_mismatches(brief, entries));
    out.extend(textures_outside_palette(brief, entries));
    out.truncate(MAX_SUGGESTIONS);
    out
}

fn lore_without_model(entries: &[Entry]) -> Vec<Suggestion> {
    let models: HashSet<String> = own(entries)
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

fn density_mismatches(brief: &Brief, entries: &[Entry]) -> Vec<Suggestion> {
    let measured: Vec<(&Entry, f32)> = own(entries)
        .filter_map(|e| density_of(e).map(|d| (e, d)))
        .collect();
    let (middle, detail) = match brief.texel_density.filter(|target| *target > 0.0) {
        Some(target) => (
            target,
            format!("The brief asks for {target} texels per block. Match the texel density."),
        ),
        None => {
            if measured.len() < MIN_PEERS {
                return Vec::new();
            }
            let Some(middle) = median_density(entries).filter(|m| *m > 0.0) else {
                return Vec::new();
            };
            (
                middle,
                format!("The models in this library have {middle:.0}. Match the texel density."),
            )
        }
    };
    let mut out: Vec<Suggestion> = measured
        .into_iter()
        .filter(|(_, d)| ((d - middle) / middle).abs() > DENSITY_TOLERANCE)
        .map(|(entry, d)| Suggestion {
            kind: SuggestionKind::DensityMismatch,
            entry: entry.id.clone(),
            title: format!("{} has {d:.0} texels per block", entry.name),
            detail: detail.clone(),
        })
        .collect();
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

fn textures_outside_palette(brief: &Brief, entries: &[Entry]) -> Vec<Suggestion> {
    let textures: Vec<(&Entry, &TextureMeta)> = own(entries)
        .filter_map(|e| texture_of(e).map(|t| (e, t)))
        .filter(|(_, t)| !t.palette.is_empty())
        .collect();
    let (palette, name, detail) = if brief.palette.is_empty() {
        if textures.len() < MIN_PEERS {
            return Vec::new();
        }
        (
            shared_palette(textures.iter().map(|(_, t)| t.palette.as_slice())),
            "the shared palette",
            "Most of its colours are not used by the other textures. Recolour it or accept it.",
        )
    } else {
        (
            brief.palette.clone(),
            "the project palette",
            "Most of its colours are not in the palette of the brief. Recolour it or change the palette.",
        )
    };
    let mut out: Vec<Suggestion> = textures
        .into_iter()
        .filter(|(_, texture)| outside_palette(&palette, &texture.palette))
        .map(|(entry, _)| Suggestion {
            kind: SuggestionKind::TextureOutsidePalette,
            entry: entry.id.clone(),
            title: format!("{} is outside {name}", entry.name),
            detail: detail.to_owned(),
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
            role: Role::Work,
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
        let found = suggestions(&Brief::default(), &entries);
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
        let found = suggestions(&Brief::default(), &entries);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, SuggestionKind::TextureOutsidePalette);
        assert_eq!(found[0].entry, "t4");
        // Two textures are too few to call one an outlier.
        assert!(suggestions(&Brief::default(), &entries[2..]).is_empty());
    }

    #[test]
    fn colours_are_outside_a_palette_when_fewer_than_half_are_near() {
        let palette = [[200, 20, 20]];
        let mixed = [[205, 25, 20], [0, 0, 255]];
        assert_eq!(colours_outside(&palette, &mixed), 1);
        assert!(!outside_palette(&palette, &mixed), "half is near enough");
        assert!(outside_palette(&palette, &[[0, 0, 255], [0, 255, 0]]));
        assert!(!outside_palette(&palette, &[]));
    }

    #[test]
    fn a_template_takes_the_rotation_rule_the_kind_and_the_display_of_the_brief() {
        let mut block = Document::new(cuboid_model::TextureList::default());
        minecraft_template(&mut block, McVersion::new(1, 20, 4), false, None);
        assert!(block.props.general.restrict_rotations);
        assert_eq!(block.props.minecraft.kind, ModelKind::Block);
        assert!(!block.props.minecraft.include_display);

        let mut item = Document::new(cuboid_model::TextureList::default());
        let handheld = Some(DisplayPreset::ItemHandheld);
        minecraft_template(&mut item, McVersion::new(1, 21, 11), true, handheld);
        assert!(!item.props.general.restrict_rotations, "free rotations");
        assert_eq!(item.props.minecraft.kind, ModelKind::Item);
        assert!(item.props.minecraft.include_display);
        let held = &item.display[cuboid_model::DisplaySlotId::ThirdRight];
        assert!(held.enabled && held.preset == "item/handheld");
        // A version the table does not know gets the strictest rule.
        assert!(restrict_rotations(McVersion::new(0, 5, 0)));
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
        let found = suggestions(&Brief::default(), &entries);
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
        assert!(suggestions(&Brief::default(), &calm).is_empty());
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
            ..Brief::default()
        }
        .cleaned();
        assert_eq!(brief.project_name.chars().count(), MAX_NAME_CHARS);
        assert_eq!(brief.mc_version, "1.21.6");
        assert_eq!(brief.style_tags.len(), MAX_STYLE_TAGS);
    }

    #[test]
    fn the_new_fields_are_trimmed_bounded_and_cleaned() {
        let brief = Brief {
            goal: format!("  line one\r\nline two{}  ", "g".repeat(3000)),
            namespace: "  Splat Craft_2!.x-y ".to_owned(),
            texel_density: Some(16.5),
            texture_size: Some(64),
            palette: (0..100u8).map(|i| [i % 80, 0, 0]).collect(),
            max_elements: Some(128),
            naming: Naming {
                case: NameCase::Snake,
                prefix: format!(" {} ", "p".repeat(50)),
            },
            display_preset: Some(DisplayPreset::ItemHandheld),
            rules: [" Do this ", "", "two\nlines", &"r".repeat(300)]
                .into_iter()
                .map(String::from)
                .chain((0..40).map(|i| format!("rule {i}")))
                .collect(),
            ..Brief::default()
        }
        .cleaned();
        assert!(
            brief.goal.starts_with("line one\nline two"),
            "{:?}",
            brief.goal
        );
        assert_eq!(brief.goal.chars().count(), MAX_GOAL_CHARS);
        assert_eq!(brief.namespace, "splatcraft_2.x-y");
        assert_eq!(brief.texel_density, Some(16.5));
        assert_eq!(brief.texture_size, Some(64));
        assert_eq!(brief.palette.len(), MAX_PALETTE);
        let twice = Brief {
            palette: vec![[1, 2, 3], [1, 2, 3]],
            ..Brief::default()
        };
        assert_eq!(twice.cleaned().palette, [[1, 2, 3]]);
        assert_eq!(brief.max_elements, Some(128));
        assert_eq!(brief.naming.prefix.chars().count(), MAX_PREFIX_CHARS);
        assert_eq!(brief.display_preset, Some(DisplayPreset::ItemHandheld));
        assert_eq!(brief.rules.len(), MAX_RULES);
        assert_eq!(&brief.rules[..3], ["Do this", "two", "lines"]);
        assert_eq!(brief.rules[3].chars().count(), MAX_RULE_CHARS);
    }

    #[test]
    fn numbers_that_cannot_be_used_read_as_none() {
        let wild = |density: f32, size: u32, budget: u32| {
            Brief {
                texel_density: Some(density),
                texture_size: Some(size),
                max_elements: Some(budget),
                ..Brief::default()
            }
            .cleaned()
        };
        for brief in [
            wild(f32::NAN, 24, 0),
            wild(-1.0, 1024, u32::MAX),
            wild(0.0, 0, 1 << 17),
            wild(1e9, 8, 0),
        ] {
            assert_eq!(
                (brief.texel_density, brief.texture_size, brief.max_elements),
                (None, None, None)
            );
        }
        let edge = wild(MAX_DENSITY, 512, MAX_BUDGET);
        assert_eq!(
            (edge.texel_density, edge.texture_size, edge.max_elements),
            (Some(MAX_DENSITY), Some(512), Some(MAX_BUDGET))
        );
    }

    #[test]
    fn unknown_names_read_as_the_fallback_and_a_schema_1_brief_gets_defaults() {
        let text = r#"{"project_name": "Ink", "platform": "nether", "naming": {"case": "title"},
            "display_preset": "item/sword"}"#;
        let brief: Brief = serde_json::from_str(text).expect("reads");
        assert_eq!(brief.platform, Platform::Other);
        assert_eq!(brief.naming.case, NameCase::Any);
        assert_eq!(brief.display_preset, None);
        let old: Brief = serde_json::from_str(
            r#"{"project_name": "Ink", "mc_version": "1.21.6", "style_tags": ["pixel"]}"#,
        )
        .expect("reads");
        assert_eq!(
            old,
            Brief {
                project_name: "Ink".to_owned(),
                mc_version: "1.21.6".to_owned(),
                style_tags: vec!["pixel".to_owned()],
                ..Brief::default()
            }
        );
        assert_eq!(old.platform, Platform::Vanilla);
        let id_round_trip = Platform::ALL.map(|p| Platform::from_id(p.id()));
        assert_eq!(id_round_trip, Platform::ALL);
    }

    #[test]
    fn the_targets_of_the_brief_replace_the_observed_values() {
        // One model is too few to be an outlier of the median, but not to miss a target.
        let entries = [model("m1", "a.cubik", Some(32.0))];
        assert!(suggestions(&Brief::default(), &entries).is_empty());
        let brief = Brief {
            texel_density: Some(16.0),
            ..Brief::default()
        };
        let found = suggestions(&brief, &entries);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].title, "a.cubik has 32 texels per block");
        assert!(found[0].detail.contains("brief asks for 16"));
        // The models agree with each other and still miss the target.
        let many = [
            model("m1", "a.cubik", Some(32.0)),
            model("m2", "b.cubik", Some(32.0)),
            model("m3", "c.cubik", Some(32.0)),
        ];
        assert!(suggestions(&Brief::default(), &many).is_empty());
        assert_eq!(suggestions(&brief, &many).len(), 3);

        // Two reds and a green: the brief palette is red, so the green is out, and the reds
        // that are alone in the library are not judged against each other.
        let reds: &[[u8; 3]] = &[[200, 20, 20], [180, 10, 10]];
        let textures = [
            texture("t1", "red.png", reds),
            texture("t2", "green.png", &[[10, 200, 20], [20, 240, 30]]),
        ];
        assert!(suggestions(&Brief::default(), &textures).is_empty());
        let painted = Brief {
            palette: vec![[200, 20, 20]],
            ..Brief::default()
        };
        let found = suggestions(&painted, &textures);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].entry, "t2");
        assert_eq!(found[0].title, "green.png is outside the project palette");
    }

    #[test]
    fn the_brief_fills_a_validation_profile() {
        let brief = Brief {
            texel_density: Some(16.0),
            texture_size: Some(32),
            palette: vec![[1, 2, 3]],
            palette_strict: true,
            max_elements: Some(64),
            namespace: "splat".to_owned(),
            naming: Naming {
                case: NameCase::Kebab,
                prefix: "ink-".to_owned(),
            },
            ..Brief::default()
        };
        let mut profile = Profile::new(cuboid_validate::Target::Cubik);
        profile.max_elements = Some(7);
        brief.fill_profile(&mut profile, "ink-gun");
        assert_eq!(profile.max_elements, Some(64));
        let rules = &profile.project;
        assert_eq!(rules.texel_density, Some(16.0));
        assert_eq!(rules.texture_size, Some(32));
        assert_eq!(
            (rules.palette.clone(), rules.palette_strict),
            (vec![[1, 2, 3]], true)
        );
        assert_eq!(rules.palette_reach, PALETTE_REACH);
        assert_eq!(rules.namespace, "splat");
        assert_eq!(rules.case, cuboid_validate::NameCase::Kebab);
        assert_eq!(
            (rules.prefix.as_str(), rules.model_name.as_str()),
            ("ink-", "ink-gun")
        );
        // A brief without a budget leaves the caller's.
        Brief::default().fill_profile(&mut profile, "");
        assert_eq!(profile.max_elements, Some(64));
        assert_eq!(profile.project.palette, Vec::<[u8; 3]>::new());
        assert_eq!(profile.project.case, cuboid_validate::NameCase::Any);
    }

    #[test]
    fn style_sources_are_relative_unique_and_at_most_16() {
        let source = |path: &str| StyleSource {
            path: path.to_owned(),
            licence: " ".to_owned(),
            tags: vec!["a".to_owned(), "A".to_owned()],
            scanned: 5,
        };
        let mut sources: Vec<StyleSource> = ["refs\\lamps", "refs/lamps/", "../out", "C:/x", ""]
            .map(source)
            .into();
        sources.extend((0..30).map(|n| source(&format!("refs/{n}"))));
        let brief = Brief {
            style_sources: sources,
            ..Brief::default()
        }
        .cleaned();
        assert_eq!(brief.style_sources.len(), MAX_STYLE_SOURCES);
        let first = &brief.style_sources[0];
        assert_eq!(first.path, "refs/lamps");
        assert_eq!((first.licence.as_str(), first.scanned), ("unknown", 5));
        assert_eq!(first.tags, ["a"]);
        assert_eq!(brief.style_sources[1].path, "refs/0");
    }

    #[test]
    fn style_references_stay_out_of_the_observed_values_and_the_suggestions() {
        let style = |mut entry: Entry| {
            entry.role = Role::Style;
            entry
        };
        let entries = [
            model("m1", "a.cubik", Some(16.0)),
            model("m2", "b.cubik", Some(16.0)),
            model("m3", "c.cubik", Some(16.0)),
            style(model("m4", "far.cubik", Some(64.0))),
            style(model("m5", "lantern.cubik", Some(16.0))),
            style(texture("t1", "odd.png", &[[10, 200, 20]])),
            lore(&["Lantern"]),
        ];
        let view = brief_view(&Brief::default(), &entries);
        assert_eq!((view.models, view.textures), (3, 0));
        assert_eq!(view.density, Some(16.0));
        assert!(view.palette.is_empty());
        // The reference called lantern does not make the lore's lantern; the far one is no outlier.
        let titles: Vec<String> = suggestions(&Brief::default(), &entries)
            .into_iter()
            .map(|s| s.title)
            .collect();
        assert_eq!(titles, ["Lantern has no model"]);
    }

    #[test]
    fn the_same_library_gives_the_same_suggestions() {
        let entries = [lore(&["B", "A", "C"]), model("m1", "x.cubik", Some(16.0))];
        let brief = Brief::default();
        assert_eq!(suggestions(&brief, &entries), suggestions(&brief, &entries));
        let titles: Vec<String> = suggestions(&brief, &entries)
            .into_iter()
            .map(|s| s.title)
            .collect();
        assert_eq!(
            titles,
            ["A has no model", "B has no model", "C has no model"]
        );
    }
}
