//! Style references: the folders of a project that hold reference models and textures (add,
//! rescan, remove) and the summary of what the references have in common.
//!
//! A folder is remembered in the brief as a [`StyleSource`] with its path relative to the project
//! folder. The entries it brought have the role [`Role::Style`] and a `source` relative to the
//! project too, so a project that moves or is shared keeps them and Rescan and removal find them.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use cuboid_io::texture_files::{DiskFiles, Refusal, TextureFiles};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::brief::{
    DENSITY_TOLERANCE, MAX_STYLE_SOURCES, NameCase, StyleSource, shared_palette, source_path,
};
use crate::import::{Control, ImportOptions, ImportReport, refused_form};
use crate::kind::{Kind, Role};
use crate::meta::{Meta, ModelMeta, TextureMeta, plurality, quantile, round3};
use crate::store::{Entry, Library, LibraryError, Scope, clean_licence, clean_tags, hex, now_secs};

/// Reference models nearest to the medians that the summary names.
const TYPICAL: usize = 3;
/// Rotation angles and name tokens the summary lists.
const LISTED: usize = 8;

/// The names of a path joined with `/`, the form the sources and the `source` of a style entry
/// have.
pub(crate) fn slashed(path: &Path) -> String {
    let names: Vec<_> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect();
    names.join("/")
}

/// Whether `path` is `folder` or lies below it; both are relative to the project, joined with
/// `/`. The app counts the references of a folder with it, as removing the folder does.
pub fn within(path: &str, folder: &str) -> bool {
    path.strip_prefix(folder)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

impl Library {
    /// The folder of the project: the one that holds `cuboid-library`. Only a project library has
    /// style folders.
    fn project_dir(&self) -> Result<&Path, LibraryError> {
        self.root()
            .parent()
            .filter(|_| self.scope() == Scope::Project)
            .ok_or_else(|| {
                LibraryError::Source("style folders belong to the library of a project".to_owned())
            })
    }

    /// The `source` text of a new entry: for a style reference inside the project the path
    /// relative to the project, else the path as it is.
    pub(crate) fn source_text(&self, path: &Path, role: Role) -> String {
        let inside = self
            .project_dir()
            .ok()
            .filter(|_| role == Role::Style)
            .and_then(|project| path.strip_prefix(project).ok());
        inside.map_or_else(|| path.display().to_string(), slashed)
    }

    /// Imports a folder of the project as style references and remembers it, or reads a known
    /// one again: the references whose file is gone or holds other content now are dropped
    /// first, so that a file that was renamed or moved is imported again under its new name.
    /// Licence and tags of `options` go on the new entries and into the source; for a known
    /// folder they replace those of its references when they differ from the source's. The
    /// folder must lie inside the project folder, outside the library, and not overlap another
    /// source; links and case do not matter, since the folders are compared as the system
    /// resolves them.
    pub fn import_style(
        &mut self,
        folder: &Path,
        options: &ImportOptions,
        control: &dyn Control,
    ) -> Result<ImportReport, LibraryError> {
        let project = self.project_dir()?.to_owned();
        let refused = |why: &str| LibraryError::Source(format!("{}: {why}", folder.display()));
        // By their text first, so that the system is not asked about a network path.
        if let Some(why) = refused_form(folder).or_else(|| refused_form(&project)) {
            return Err(refused(&why));
        }
        let unreadable = || refused("it is not a folder that can be read");
        let real = fs::canonicalize(folder).map_err(|_| unreadable())?;
        if !real.is_dir() {
            return Err(unreadable());
        }
        let real_project = fs::canonicalize(&project).map_err(|_| unreadable())?;
        let relative = real
            .strip_prefix(&real_project)
            .ok()
            .and_then(|inside| source_path(&slashed(inside)))
            .ok_or_else(|| refused("it is not a folder inside the project"))?;
        if fs::canonicalize(self.root()).is_ok_and(|library| real.starts_with(library)) {
            return Err(refused("it is inside the library"));
        }
        // Canonical paths are verbatim, which the import refuses: the walk uses the project as
        // it was given.
        let folder = project.join(&relative);
        let sources = &self.brief().style_sources;
        let known = sources.iter().position(|known| known.path == relative);
        if known.is_none() && sources.len() >= MAX_STYLE_SOURCES {
            return Err(refused("the project keeps at most 16 style folders"));
        }
        if let Some(other) = sources.iter().find(|other| {
            other.path != relative
                && (within(&other.path, &relative) || within(&relative, &other.path))
        }) {
            return Err(refused(&format!(
                "it overlaps the style folder {}",
                other.path
            )));
        }
        let old = known.map(|at| sources[at].clone());

        let options = ImportOptions {
            role: Role::Style,
            ..options.clone()
        };
        let dropped = self.drop_stale(&relative, &project);
        let mut report = self.import(std::slice::from_ref(&folder), &options, control);
        report.dropped = dropped;
        let source = StyleSource {
            licence: clean_licence(&options.licence),
            tags: clean_tags(&options.tags),
            // A cancelled import has not read the whole folder.
            scanned: if report.cancelled {
                old.as_ref().map_or(0, |old| old.scanned)
            } else {
                now_secs()
            },
            path: relative,
        };
        if old
            .as_ref()
            .is_some_and(|old| old.licence != source.licence || old.tags != source.tags)
        {
            for id in self.style_ids(&source.path) {
                self.set_licence(&id, &source.licence);
                self.set_tags(&id, &source.tags);
            }
        }
        let mut brief = self.brief().clone();
        match known {
            Some(at) => brief.style_sources[at] = source,
            None => brief.style_sources.push(source),
        }
        self.set_brief(brief);
        self.save()?;
        Ok(report)
    }

    /// Reads a remembered style folder again with its licence and tags. `path` is the `path` of
    /// the [`StyleSource`], in any form [`source_path`] cleans.
    pub fn rescan_style(
        &mut self,
        path: &str,
        control: &dyn Control,
    ) -> Result<ImportReport, LibraryError> {
        let project = self.project_dir()?.to_owned();
        let source = self.style_source(path)?;
        let options = ImportOptions {
            tags: source.tags.clone(),
            licence: source.licence.clone(),
            ..ImportOptions::default()
        };
        let folder = project.join(&source.path);
        self.import_style(&folder, &options, control)
    }

    /// Forgets a style folder and removes its references and their stored copies. The folder
    /// itself is not touched. Returns how many references went.
    pub fn remove_style_source(&mut self, path: &str) -> Result<usize, LibraryError> {
        let path = self.style_source(path)?.path.clone();
        let ids = self.style_ids(&path);
        for id in &ids {
            self.remove(id);
        }
        let mut brief = self.brief().clone();
        brief.style_sources.retain(|source| source.path != path);
        self.set_brief(brief);
        self.save()?;
        Ok(ids.len())
    }

    fn style_source(&self, path: &str) -> Result<&StyleSource, LibraryError> {
        source_path(path)
            .and_then(|clean| {
                self.brief()
                    .style_sources
                    .iter()
                    .find(|source| source.path == clean)
            })
            .ok_or_else(|| LibraryError::Source(format!("{path} is not a style folder")))
    }

    /// The ids of the style references that came from the folder.
    fn style_ids(&self, folder: &str) -> Vec<String> {
        self.entries()
            .iter()
            .filter(|entry| entry.role == Role::Style && within(&entry.source, folder))
            .map(|entry| entry.id.clone())
            .collect()
    }

    /// Removes the references of the folder whose file is gone, holds other content now, is too
    /// large or is no regular file, and returns how many. A file that exists but cannot be read
    /// keeps its reference, and so does a path in a form that is never read.
    fn drop_stale(&mut self, folder: &str, project: &Path) -> usize {
        let stale: Vec<String> = self
            .entries()
            .iter()
            .filter(|entry| entry.role == Role::Style && within(&entry.source, folder))
            .filter(|entry| {
                let file = project.join(&entry.source);
                match DiskFiles.read(&file) {
                    Ok(Some(bytes)) => hex(&Sha256::digest(&bytes)) != entry.sha256,
                    Ok(None) => !file.is_file(),
                    Err(Refusal::NotRegular | Refusal::TooLarge) => true,
                    Err(_) => false,
                }
            })
            .map(|entry| entry.id.clone())
            .collect();
        for id in &stale {
            self.remove(id);
        }
        stale.len()
    }

    /// The summary of the style references, `None` when there are none.
    pub fn style_summary(&self) -> Option<StyleSummary> {
        style_summary(self.entries())
    }
}

/// The smallest, median and largest of a count.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Spread {
    /// The smallest.
    pub min: f32,
    /// The median.
    pub median: f32,
    /// The largest.
    pub max: f32,
}

/// How big the elements are, in px.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ElementSize {
    /// The 10th percentile of the longest sides. The sides are pooled as the five values each
    /// model keeps (smallest, quartiles and largest), so a model weighs the same however many
    /// elements it has.
    pub p10: f32,
    /// The median of the pooled sides.
    pub median: f32,
    /// The 90th percentile of the pooled sides.
    pub p90: f32,
    /// The share of the elements with a side of 1 px or less.
    pub thin_share: f32,
}

/// How deep the models are grouped.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GroupSummary {
    /// The median number of groups of a model.
    pub median: f32,
    /// The median depth of the deepest group of a model.
    pub depth: f32,
}

/// Texels per block of the references.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DensitySummary {
    /// The 10th percentile of the models' densities.
    pub p10: f32,
    /// The median.
    pub median: f32,
    /// The 90th percentile.
    pub p90: f32,
    /// The share of the models within [`DENSITY_TOLERANCE`] of the median.
    pub near_median_share: f32,
}

/// How much is rotated, and by what.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RotationSummary {
    /// The share of the elements with a rotation.
    pub share: f32,
    /// The angles that most models use, smallest first.
    pub angles: Vec<f32>,
}

/// How things are named.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NamingSummary {
    /// The case that most models use; `any` when no case has more models than the others.
    pub case: NameCase,
    /// The words that most models use in their names.
    pub tokens: Vec<String>,
}

/// What the style references have in common, computed from the stored metadata. A few hundred
/// bytes of JSON however many references there are. What is about models is left out when
/// there are no models, only textures.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StyleSummary {
    /// Reference models.
    pub models: usize,
    /// Reference textures.
    pub textures: usize,
    /// Elements per model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elements: Option<Spread>,
    /// The sizes of the elements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_size: Option<ElementSize>,
    /// Groups per model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<GroupSummary>,
    /// Texels per block; left out when no model has a texture size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texel_density: Option<DensitySummary>,
    /// Pictures per largest side in texels, models and textures together; the 8 sizes that most
    /// pictures have. A texture that a model holds and the same PNG kept as a reference count
    /// twice.
    pub texture_sizes: BTreeMap<u32, usize>,
    /// The colours that several pictures share, as `#rrggbb`.
    pub palette: Vec<String>,
    /// Rotated elements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<RotationSummary>,
    /// Names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub naming: Option<NamingSummary>,
    /// Models per display preset or `custom`.
    pub display: BTreeMap<String, usize>,
    /// Up to three references nearest the medians, to open first: entry ids.
    pub typical: Vec<String>,
}

fn sorted(mut values: Vec<f32>) -> Vec<f32> {
    values.sort_by(f32::total_cmp);
    values
}

/// The most used keys of a count, up to [`LISTED`]; ties go to the smaller key.
fn most_used<K: Ord + Clone>(counts: &HashMap<K, usize>) -> Vec<K> {
    let mut list: Vec<(&K, &usize)> = counts.iter().collect();
    list.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    list.into_iter()
        .take(LISTED)
        .map(|(key, _)| key.clone())
        .collect()
}

/// The summary of the style references among `entries`, `None` when there are none.
pub fn style_summary(entries: &[Entry]) -> Option<StyleSummary> {
    let style = || entries.iter().filter(|entry| entry.role == Role::Style);
    let models = style().filter(|entry| entry.kind == Kind::Model).count();
    let textures = style().filter(|entry| entry.kind == Kind::Texture).count();
    if models + textures == 0 {
        return None;
    }
    let described: Vec<(&Entry, &ModelMeta)> = style()
        .filter_map(|entry| match &entry.meta {
            Some(Meta::Model(meta)) => Some((entry, meta)),
            _ => None,
        })
        .collect();
    let styles: Vec<_> = described
        .iter()
        .filter_map(|(entry, meta)| meta.style.as_ref().map(|style| (*entry, *meta, style)))
        .collect();
    let pictures: Vec<&TextureMeta> = style()
        .filter_map(|entry| match &entry.meta {
            Some(Meta::Texture(texture)) => Some(texture),
            _ => None,
        })
        .collect();

    let counts = sorted(described.iter().map(|(_, m)| m.elements as f32).collect());
    let spread = |values: &[f32]| Spread {
        min: values.first().copied().unwrap_or(0.0),
        median: round3(quantile(values, 0.5)),
        max: values.last().copied().unwrap_or(0.0),
    };

    let sides = sorted(
        styles
            .iter()
            .flat_map(|(_, _, style)| style.sides.into_iter().flatten())
            .collect(),
    );
    let elements: u32 = styles.iter().map(|(_, m, _)| m.elements).sum();
    let thin: u32 = styles.iter().map(|(_, _, style)| style.thin).sum();
    let rotated: u32 = styles.iter().map(|(_, _, style)| style.rotated).sum();
    let share = |part: u32| round2(part as f32 / elements.max(1) as f32);

    let group_counts = sorted(styles.iter().map(|(_, _, s)| s.groups as f32).collect());
    let depths = sorted(styles.iter().map(|(_, _, s)| s.depth as f32).collect());

    let densities = sorted(described.iter().filter_map(|(_, m)| m.density).collect());
    let texel_density = (!densities.is_empty()).then(|| {
        let median = quantile(&densities, 0.5);
        let near = densities
            .iter()
            .filter(|d| ((*d - median) / median).abs() <= DENSITY_TOLERANCE)
            .count();
        DensitySummary {
            p10: round3(quantile(&densities, 0.1)),
            median: round3(median),
            p90: round3(quantile(&densities, 0.9)),
            near_median_share: round2(near as f32 / densities.len() as f32),
        }
    });

    let mut size_uses: HashMap<u32, usize> = HashMap::new();
    let sizes = described
        .iter()
        .filter_map(|(_, m)| m.texture_size)
        .chain(pictures.iter().map(|t| t.width.max(t.height)));
    for size in sizes {
        *size_uses.entry(size).or_insert(0) += 1;
    }
    let texture_sizes = most_used(&size_uses)
        .into_iter()
        .map(|size| (size, size_uses[&size]))
        .collect();
    let palettes = pictures
        .iter()
        .map(|t| t.palette.as_slice())
        .chain(styles.iter().map(|(_, _, s)| s.palette.as_slice()));
    let palette = shared_palette(palettes)
        .iter()
        .map(|c| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]))
        .collect();

    // Angles and words are counted once per model, so one model full of them does not decide.
    let mut angle_uses: HashMap<u32, usize> = HashMap::new();
    let mut word_uses: HashMap<&str, usize> = HashMap::new();
    let (mut snake, mut kebab, mut plain) = (0, 0, 0);
    let mut display: BTreeMap<String, usize> = BTreeMap::new();
    for (_, _, style) in &styles {
        for angle in &style.angles {
            *angle_uses
                .entry((angle * 100.0).round() as u32)
                .or_insert(0) += 1;
        }
        for word in &style.tokens {
            *word_uses.entry(word).or_insert(0) += 1;
        }
        match style.case {
            NameCase::Snake => snake += 1,
            NameCase::Kebab => kebab += 1,
            NameCase::Any => plain += 1,
        }
        if let Some(preset) = &style.display {
            *display.entry(preset.clone()).or_insert(0) += 1;
        }
    }
    let mut angles = most_used(&angle_uses);
    angles.sort_unstable();

    let with_models = !described.is_empty();
    let summary = StyleSummary {
        models,
        textures,
        elements: with_models.then(|| spread(&counts)),
        element_size: with_models.then(|| ElementSize {
            p10: round3(quantile(&sides, 0.1)),
            median: round3(quantile(&sides, 0.5)),
            p90: round3(quantile(&sides, 0.9)),
            thin_share: share(thin),
        }),
        groups: with_models.then(|| GroupSummary {
            median: round3(quantile(&group_counts, 0.5)),
            depth: round3(quantile(&depths, 0.5)),
        }),
        texel_density,
        texture_sizes,
        palette,
        rotation: with_models.then(|| RotationSummary {
            share: share(rotated),
            angles: angles.iter().map(|a| *a as f32 / 100.0).collect(),
        }),
        naming: with_models.then(|| NamingSummary {
            case: plurality(snake, kebab, plain),
            tokens: most_used(&word_uses)
                .into_iter()
                .map(str::to_owned)
                .collect(),
        }),
        display,
        typical: typical(&described, &counts, &sides, &densities),
    };
    Some(summary)
}

fn round2(value: f32) -> f32 {
    (value * 100.0).round() / 100.0 + 0.0
}

/// The ids of the models nearest to the medians of the element count, the element size and the
/// texel density: the mean of the distances as fractions of the medians, over the values the
/// model has, ties by id.
fn typical(
    models: &[(&Entry, &ModelMeta)],
    counts: &[f32],
    sides: &[f32],
    densities: &[f32],
) -> Vec<String> {
    let far = |value: f32, middle: f32| (value - middle).abs() / middle.max(1.0);
    let (count, side, density) = (
        quantile(counts, 0.5),
        quantile(sides, 0.5),
        quantile(densities, 0.5),
    );
    let mut ranked: Vec<(f32, &str)> = models
        .iter()
        .map(|(entry, meta)| {
            let mut distance = far(meta.elements as f32, count);
            let mut terms = 1.0;
            if let Some(median) = meta.style.as_ref().and_then(|s| s.sides).map(|s| s[2]) {
                distance += far(median, side);
                terms += 1.0;
            }
            if let Some(value) = meta.density {
                distance += far(value, density);
                terms += 1.0;
            }
            (distance / terms, entry.id.as_str())
        })
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    ranked
        .into_iter()
        .take(TYPICAL)
        .map(|(_, id)| id.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::StyleMeta;
    use crate::store::UNKNOWN_LICENCE;

    fn reference(index: usize) -> Entry {
        let id = format!("{index:016x}");
        let n = index as u32;
        let meta = ModelMeta {
            format: "bbmodel".to_owned(),
            elements: 6 + n % 50,
            textures: 1,
            density: Some(if n.is_multiple_of(7) { 32.0 } else { 16.0 }),
            texture_size: Some(16 + n % 40),
            style: Some(StyleMeta {
                groups: n % 5,
                depth: n % 3,
                sides: Some([1.0, 2.0, 3.0 + (n % 4) as f32, 6.0, 12.0]),
                thin: n % 4,
                rotated: n % 3,
                angles: vec![22.5, 45.0, (n % 90) as f32 + 0.5],
                densities: Some([16.0, 16.0, 32.0]),
                palette: vec![[43, 45, 66], [141, 153, 174], [n as u8, 35, 60]],
                tokens: vec!["handle".to_owned(), format!("part{}", n % 30)],
                case: NameCase::Snake,
                display: Some(
                    if n.is_multiple_of(5) {
                        "custom"
                    } else {
                        "item/handheld"
                    }
                    .to_owned(),
                ),
                parent: None,
            }),
            ..ModelMeta::default()
        };
        Entry {
            id: id.clone(),
            kind: Kind::Model,
            name: format!("ref_{index}.json"),
            file: format!("{id}-ref_{index}.json"),
            companion: None,
            source: format!("refs/ref_{index}.json"),
            sha256: format!("{id:0>64}"),
            size: 100,
            tags: Vec::new(),
            licence: UNKNOWN_LICENCE.to_owned(),
            role: Role::Style,
            added: 1,
            meta: Some(Meta::Model(meta)),
        }
    }

    #[test]
    fn a_thousand_references_sum_up_in_under_4_kb() {
        let entries: Vec<Entry> = (0..1000).map(reference).collect();
        let summary = style_summary(&entries).expect("a summary");
        let text = serde_json::to_string(&summary).expect("encodes");
        assert!(text.len() < 4096, "{} bytes: {text}", text.len());
        assert_eq!(summary.models, 1000);
        assert_eq!(summary.typical.len(), 3);
        let naming = summary.naming.as_ref().expect("naming");
        assert!(summary.palette.len() <= 16 && naming.tokens.len() <= 8);
        assert_eq!(
            summary.texture_sizes.len(),
            8,
            "of 40 sizes the 8 most used"
        );
        assert_eq!(naming.case, NameCase::Snake);
        let rotation = summary.rotation.as_ref().expect("rotation");
        assert!(rotation.angles.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(summary.display.values().sum::<usize>(), 1000);
    }

    #[test]
    fn the_brief_json_lists_a_reference_in_a_few_hundred_bytes() {
        let mut library = Library::new(
            std::path::PathBuf::from("lib"),
            crate::store::Scope::Project,
        );
        let empty = library.brief_json().expect("encodes").len();
        for n in 0..300 {
            library.push(reference(n));
        }
        let text = library.brief_json().expect("encodes");
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(json["entries"].as_array().map(Vec::len), Some(300));
        assert!(json["entries"][0]["meta"].get("style").is_none());
        // About 440 bytes, where the style of a model made it 1,140.
        let each = (text.len() - empty) / 300;
        assert!(each < 500, "{each} bytes a reference");
    }

    #[test]
    fn work_entries_and_an_empty_set_have_no_summary() {
        assert_eq!(style_summary(&[]), None);
        let mut work = reference(1);
        work.role = Role::Work;
        assert_eq!(style_summary(&[work]), None);
    }

    #[test]
    fn the_summary_reads_the_medians_of_a_few_models() {
        let entries: Vec<Entry> = (1..=3).map(reference).collect();
        let summary = style_summary(&entries).expect("a summary");
        let elements = summary.elements.as_ref().expect("elements");
        assert_eq!(
            (elements.min, elements.median, elements.max),
            (7.0, 8.0, 9.0)
        );
        let density = summary.texel_density.expect("density");
        assert_eq!((density.median, density.near_median_share), (16.0, 1.0));
        let naming = summary.naming.as_ref().expect("naming");
        assert_eq!(naming.tokens[0], "handle");
        let rotation = summary.rotation.as_ref().expect("rotation");
        assert!(rotation.angles.ends_with(&[22.5, 45.0]));
        assert_eq!(summary.display["item/handheld"], 3);
        assert_eq!(summary.texture_sizes.values().sum::<usize>(), 3);
    }

    #[test]
    fn references_that_are_all_textures_have_no_model_parts() {
        let mut wood = reference(1);
        wood.kind = Kind::Texture;
        wood.meta = Some(Meta::Texture(TextureMeta {
            width: 16,
            height: 16,
            frames: 1,
            alpha: None,
            palette: vec![[1, 2, 3]],
        }));
        let summary = style_summary(&[wood]).expect("a summary");
        assert_eq!((summary.models, summary.textures), (0, 1));
        assert!(summary.elements.is_none() && summary.element_size.is_none());
        assert!(summary.groups.is_none() && summary.rotation.is_none() && summary.naming.is_none());
        let text = serde_json::to_string(&summary).expect("encodes");
        assert_eq!(
            text,
            r##"{"models":0,"textures":1,"texture_sizes":{"16":1},"palette":["#010203"],"display":{},"typical":[]}"##
        );
    }

    #[test]
    fn the_case_is_the_plurality_of_snake_kebab_and_neither() {
        // Three snake models of ten do not make the summary snake.
        let entries: Vec<Entry> = (0..10)
            .map(|n| {
                let mut entry = reference(n);
                if let Some(Meta::Model(meta)) = &mut entry.meta
                    && let Some(style) = &mut meta.style
                {
                    style.case = if n < 3 {
                        NameCase::Snake
                    } else {
                        NameCase::Any
                    };
                }
                entry
            })
            .collect();
        let summary = style_summary(&entries).expect("a summary");
        assert_eq!(
            summary.naming.map(|naming| naming.case),
            Some(NameCase::Any)
        );
    }

    #[test]
    fn a_model_without_a_density_does_not_score_as_if_it_sat_on_the_median() {
        let model = |id: &str, elements, side: f32, density| {
            let mut entry = reference(0);
            entry.id = id.to_owned();
            if let Some(Meta::Model(meta)) = &mut entry.meta {
                meta.elements = elements;
                meta.density = density;
                meta.style.as_mut().expect("style").sides = Some([1.0, 2.0, side, 6.0, 12.0]);
            }
            entry
        };
        let pair = |entry: &Entry| match &entry.meta {
            Some(Meta::Model(meta)) => (entry.clone(), meta.clone()),
            _ => panic!("a model"),
        };
        // A Minecraft JSON has no density: its sum of distances is shorter, its mean is not.
        let (json, json_meta) = pair(&model("json", 12, 4.0, None));
        let (blocky, blocky_meta) = pair(&model("blocky", 10, 4.6, Some(17.6)));
        let ranked = typical(
            &[(&json, &json_meta), (&blocky, &blocky_meta)],
            &[10.0],
            &[4.0],
            &[16.0],
        );
        assert_eq!(ranked, ["blocky", "json"]);
    }

    #[test]
    fn source_paths_are_relative_and_inside() {
        assert_eq!(source_path("refs\\lamps/").as_deref(), Some("refs/lamps"));
        assert_eq!(source_path("./refs//a").as_deref(), Some("refs/a"));
        for bad in ["", ".", "../x", "refs/../../x", "C:/x", "refs/a:b"] {
            assert_eq!(source_path(bad), None, "{bad}");
        }
        assert!(within("refs/a/b.json", "refs/a"));
        assert!(!within("refs/ab/b.json", "refs/a"));
        assert!(within("refs/a", "refs/a"));
    }
}
