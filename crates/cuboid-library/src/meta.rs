//! Metadata of the entries, read by the import worker with limits: models through the format
//! crates, textures through the image crate, lore with a small heading and bullet reader.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use cuboid_fmt_cubik::Limits;
use cuboid_fmt_cubik::cub::{self, TextureFiles};
use cuboid_fmt_minecraft::{DISPLAY_PRESETS, builtin_display};
use cuboid_image::{ImageFileKind, decode_file};
use cuboid_io::texture_files::Refusal;
use cuboid_model::{
    CaptureScope, Categories, DisplaySlots, Document, FaceName, Groups, LoadedDoc, Selection,
    Snapshot, TextureList, capture,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::brief::NameCase;
use crate::kind::{JsonClass, extension};

/// Texels above which a texture is not decoded for its palette.
const MAX_DECODE_TEXELS: u64 = 1 << 24;
/// Texels looked at for the palette; larger pictures are sampled on a grid.
const PALETTE_SAMPLE: u64 = 1 << 20;
/// Colours kept per texture.
pub const PALETTE_COLORS: usize = 16;
/// Name tokens kept per model.
const MAX_TOKENS: usize = 8;
/// Characters kept of a name token.
const MAX_WORD: usize = 24;
/// Rotation angles kept per model.
const MAX_ANGLES: usize = 8;
/// The name the Minecraft importer gives an element that has none.
const DEFAULT_LABEL: &str = "Element";
/// Sides of this many px or less make an element thin.
const THIN_SIDE: f32 = 1.0;
/// Bytes of a lore file read for its metadata.
const LORE_READ: usize = 1 << 20;
const MAX_HEADINGS: usize = 64;
const MAX_NAMES: usize = 200;
const MAX_NAME_LEN: usize = 60;
/// Largest frame count taken from an animation.
const MAX_FRAMES: u32 = 4096;

/// What the library knows about an entry beyond its file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Meta {
    /// A model.
    Model(ModelMeta),
    /// A texture.
    Texture(TextureMeta),
    /// A JSON file.
    Json {
        /// What it is.
        class: JsonClass,
    },
    /// A lore note.
    Lore(LoreMeta),
}

/// Counts, bounds and texel density of a model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelMeta {
    /// `cubik`, `cub`, `bbmodel` or `json`.
    pub format: String,
    /// Cuboid and plane elements.
    pub elements: u32,
    /// Meshes.
    pub meshes: u32,
    /// Textures held by the file.
    pub textures: u32,
    /// The model has a voxel world.
    pub voxels: bool,
    /// `[min x, min y, min z, max x, max y, max z]` in model units (16 per block) of the element
    /// and mesh boxes; rotations are ignored. In the space of the document, where z is the
    /// mirror of the z of a Java model, so the box of a Java element from z 0 to 16 reads -16 to
    /// 0. A Minecraft JSON entry of an index written before schema 3 keeps the z of the file.
    pub bounds: Option<[f32; 6]>,
    /// Median texels per block over the active faces that have a texture; `None` when the
    /// format does not give the texture size.
    pub density: Option<f32>,
    /// The largest side of the textures the file holds, in texels; `None` without textures or
    /// when the format does not give the texture size.
    pub texture_size: Option<u32>,
    /// What the model looks like; `None` in an index of schema 2 or older, until the entry
    /// becomes a style reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<StyleMeta>,
}

/// What a model looks like, a few hundred bytes of it, for the style summary of the brief.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleMeta {
    /// Groups of the model.
    pub groups: u32,
    /// Levels of the deepest group; 0 without groups.
    pub depth: u32,
    /// The longest side of an element in px: smallest, 25th percentile, median, 75th percentile
    /// and largest; `None` without elements.
    pub sides: Option<[f32; 5]>,
    /// Elements with a side of 1 px or less.
    pub thin: u32,
    /// Elements with a rotation.
    pub rotated: u32,
    /// The rotation angles in use in degrees, up to 8, smallest first.
    pub angles: Vec<f32>,
    /// Smallest, median and largest texel density of the faces; `None` without densities.
    pub densities: Option<[f32; 3]>,
    /// Up to 16 colours of the textures the file holds, most used first.
    pub palette: Vec<[u8; 3]>,
    /// Up to 8 words that the element and group names are made of, most used first.
    pub tokens: Vec<String>,
    /// How the names are cased: the one that more names use.
    pub case: NameCase,
    /// The display preset that the eight slots match, `custom` when none does; `None` when no slot
    /// is enabled.
    pub display: Option<String>,
    /// The parent model of a Minecraft JSON.
    pub parent: Option<String>,
}

/// How a texture uses alpha.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaUse {
    /// Every texel is opaque.
    Opaque,
    /// Texels are opaque or fully transparent.
    Cutout,
    /// Some texel is partly transparent.
    Translucent,
}

impl AlphaUse {
    /// Name for lists.
    pub fn name(self) -> &'static str {
        match self {
            AlphaUse::Opaque => "opaque",
            AlphaUse::Cutout => "cutout",
            AlphaUse::Translucent => "translucent",
        }
    }
}

/// Size, animation frames, palette and alpha use of a texture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureMeta {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Animation frames from the `.png.mcmeta`; 1 for a still texture.
    pub frames: u32,
    /// Alpha use; `None` for a picture too large to read.
    pub alpha: Option<AlphaUse>,
    /// Up to 16 colours, most used first.
    pub palette: Vec<[u8; 3]>,
}

/// Title, size and the names a lore note lists.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LoreMeta {
    /// The first heading.
    pub title: Option<String>,
    /// Words in the text.
    pub words: u32,
    /// Headings in order.
    pub headings: Vec<String>,
    /// Names listed as bullets under headings such as Items, Weapons or Characters.
    pub names: Vec<String>,
}

/// Texture files that do not exist: metadata never reads files next to the model.
struct NoFiles;

impl TextureFiles for NoFiles {
    fn read(&self, _path: &Path) -> Result<Option<Vec<u8>>, Refusal> {
        Ok(None)
    }
}

/// Metadata of a model file. `name` gives the format by extension.
pub fn model_meta(name: &str, bytes: &[u8]) -> Result<ModelMeta, String> {
    let ext = extension(name);
    let path = Path::new(name);
    match ext.as_str() {
        "cubik" => {
            let limits = Limits {
                max_payload: 256 << 20,
                ..Limits::default()
            };
            let loaded = cuboid_fmt_cubik::load_bytes(bytes, &limits).map_err(|e| e.to_string())?;
            Ok(describe("cubik", &loaded.doc))
        }
        "cub" => {
            let imported = cub::import(bytes, path, &NoFiles).map_err(|e| e.to_string())?;
            Ok(describe("cub", &imported.doc))
        }
        "bbmodel" => {
            let imported =
                cuboid_fmt_bbmodel::import(bytes, path, &NoFiles).map_err(|e| e.to_string())?;
            Ok(describe("bbmodel", &imported.doc))
        }
        _ => {
            let mut imported =
                cuboid_fmt_minecraft::import(bytes, path, &NoFiles).map_err(|e| e.to_string())?;
            // The pictures are not read, so the list holds checkerboards: they count as
            // textures but give no texture size, density or colours.
            let textures = imported
                .doc
                .sections
                .textures
                .take()
                .map_or(0, |list| list.entries.len());
            let mut meta = describe("json", &imported.doc);
            meta.textures = u32::try_from(textures).unwrap_or(u32::MAX);
            Ok(meta)
        }
    }
}

/// Metadata of a document that is open, as the index keeps it for the same model saved as a
/// file of the format `format` (`cubik`, `cub`, `bbmodel` or `json`).
pub fn document_meta(format: &str, doc: &Document) -> ModelMeta {
    let sections = capture(
        doc,
        &Selection::default(),
        CaptureScope {
            categories: Categories::ALL,
            selection_only: false,
        },
    );
    describe(
        format,
        &LoadedDoc {
            sections,
            voxel_replay: None,
            overrides: None,
            retained: None,
            keep_exact_rotations: false,
        },
    )
}

/// A running box over corners.
#[derive(Default)]
struct Bounds(Option<([f32; 3], [f32; 3])>);

impl Bounds {
    fn add(&mut self, a: [f32; 3], b: [f32; 3]) {
        if !(a.iter().chain(&b).all(|v| v.is_finite())) {
            return;
        }
        let (lo, hi) = self.0.get_or_insert((a, a));
        for axis in 0..3 {
            lo[axis] = lo[axis].min(a[axis]).min(b[axis]);
            hi[axis] = hi[axis].max(a[axis]).max(b[axis]);
        }
    }

    fn finish(self) -> Option<[f32; 6]> {
        self.0
            .map(|(lo, hi)| [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]])
    }
}

/// Counts, bounds, density and style of a loaded document; `format` names the file format.
fn describe(format: &str, doc: &LoadedDoc) -> ModelMeta {
    let sections = &doc.sections;
    let textures: Option<&TextureList> = sections.textures.as_deref();
    let mut bounds = Bounds::default();
    let mut densities = Vec::new();
    let mut elements = 0_usize;
    if let Some(snap) = &sections.elements {
        elements = snap.list.items.len();
        for element in &snap.list.items {
            let (p, s) = (element.base.p, element.base.s);
            // `p` is the corner with the largest Z.
            bounds.add([p.x, p.y, p.z - s.z], [p.x + s.x, p.y + s.y, p.z]);
            for face in element.faces.iter().filter(|face| face.active) {
                if let Some(density) = textures.and_then(|t| face_density(t, element, face)) {
                    densities.push(density);
                }
            }
        }
    }
    let meshes = sections
        .meshes
        .as_ref()
        .map_or(0, |snap| snap.list.items.len());
    if let Some(snap) = &sections.meshes {
        for mesh in &snap.list.items {
            let (p, s) = (mesh.base.p, mesh.base.s);
            bounds.add([p.x, p.y, p.z - s.z], [p.x + s.x, p.y + s.y, p.z]);
        }
    }
    let density = median(&mut densities);
    ModelMeta {
        format: format.to_owned(),
        elements: u32::try_from(elements).unwrap_or(u32::MAX),
        meshes: u32::try_from(meshes).unwrap_or(u32::MAX),
        textures: u32::try_from(textures.map_or(0, |t| t.entries.len())).unwrap_or(u32::MAX),
        voxels: matches!(sections.voxels, Some(Some(_))),
        bounds: bounds.finish(),
        density,
        texture_size: textures.and_then(|t| {
            t.entries
                .iter()
                .map(|e| {
                    let (w, h) = e.frame_size();
                    w.max(h)
                })
                .max()
        }),
        style: Some(style_of(sections, &densities)),
    }
}

/// The style of a loaded document. `densities` are the sorted face densities of the model.
fn style_of(sections: &Snapshot, densities: &[f32]) -> StyleMeta {
    let mut style = StyleMeta::default();
    let mut sides = Vec::new();
    // Rotation angles in hundredths of a degree with the number of uses.
    let mut angles: BTreeMap<u32, u32> = BTreeMap::new();
    let mut words_used: HashMap<String, u32> = HashMap::new();
    let (mut snake, mut kebab, mut plain) = (0_u32, 0_u32, 0_u32);
    let mut name = |label: &str| {
        // The label the Minecraft importer gives an element without a name says nothing.
        if label == DEFAULT_LABEL {
            return;
        }
        snake += u32::from(label.contains('_'));
        kebab += u32::from(label.contains('-'));
        plain += u32::from(!label.contains(['_', '-']));
        for word in words(label) {
            *words_used.entry(word).or_insert(0) += 1;
        }
    };
    if let Some(snap) = &sections.elements {
        for element in &snap.list.items {
            let base = &element.base;
            let size = base.s.abs();
            sides.push(size.max_element());
            style.thin += u32::from(size.min_element() <= THIN_SIDE);
            let mut turned = false;
            for angle in base.e.to_array().map(fold_angle) {
                let hundredths = (angle * 100.0).round() as u32;
                if hundredths > 0 {
                    turned = true;
                    *angles.entry(hundredths).or_insert(0) += 1;
                }
            }
            style.rotated += u32::from(turned);
            name(&base.label);
        }
    }
    if let Some(snap) = &sections.groups {
        let groups = Groups::from_snap(snap);
        style.groups = u32::try_from(groups.len()).unwrap_or(u32::MAX);
        let deepest = groups.ids().iter().map(|id| groups.depth(*id)).max();
        style.depth = u32::try_from(deepest.unwrap_or(0)).unwrap_or(u32::MAX);
        for group in groups.items() {
            name(&group.name);
        }
    }
    sides.sort_by(f32::total_cmp);
    if !sides.is_empty() {
        style.sides = Some([0.0, 0.25, 0.5, 0.75, 1.0].map(|q| round3(quantile(&sides, q))));
    }
    let mut common: Vec<(u32, u32)> = angles.into_iter().collect();
    common.sort_by_key(|(_, uses)| Reverse(*uses));
    common.truncate(MAX_ANGLES);
    common.sort_unstable();
    style.angles = common
        .into_iter()
        .map(|(hundredths, _)| hundredths as f32 / 100.0)
        .collect();
    if let (Some(low), Some(high)) = (densities.first(), densities.last()) {
        style.densities = Some([*low, quantile(densities, 0.5), *high].map(round3));
    }
    if let Some(list) = sections.textures.as_deref() {
        let mut colours = Colours::default();
        for entry in &list.entries {
            let image = &entry.texture.image;
            if u64::from(image.w) * u64::from(image.h) <= MAX_DECODE_TEXELS {
                colours.add(image.rgba(), image.w, image.h);
            }
        }
        style.palette = colours.top();
    }
    let mut tokens: Vec<(String, u32)> = words_used.into_iter().collect();
    tokens.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    tokens.truncate(MAX_TOKENS);
    style.tokens = tokens.into_iter().map(|(word, _)| word).collect();
    style.case = plurality(snake, kebab, plain);
    style.parent = sections
        .props
        .as_ref()
        .map(|props| props.minecraft.parent.clone())
        .filter(|parent| !parent.is_empty());
    style.display = sections
        .display
        .as_deref()
        .and_then(|display| display_preset(display, style.parent.as_deref().unwrap_or_default()));
    style
}

/// An angle as its distance from zero turns: 337.5 degrees is 22.5.
fn fold_angle(degrees: f32) -> f32 {
    let turn = degrees.rem_euclid(360.0);
    turn.min(360.0 - turn)
}

/// The lower-case words of a name, split at everything that is no letter and at a capital after
/// a small letter; words of one letter and numbers are dropped, and long ones cut to
/// [`MAX_WORD`] characters.
fn words(label: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut after_small = false;
    for c in label.chars() {
        if !c.is_alphabetic() || (c.is_uppercase() && after_small) {
            out.push(std::mem::take(&mut word));
        }
        if c.is_alphabetic() {
            word.extend(c.to_lowercase());
        }
        after_small = c.is_lowercase();
    }
    out.push(word);
    out.retain(|word| word.chars().count() > 1);
    for word in &mut out {
        if let Some((end, _)) = word.char_indices().nth(MAX_WORD) {
            word.truncate(end);
        }
    }
    out
}

/// The case that most names use: names with an underscore, with a hyphen and with neither. No
/// case with more names than both others, a tie included, is `any`.
pub(crate) fn plurality(snake: u32, kebab: u32, plain: u32) -> NameCase {
    if snake > kebab && snake > plain {
        NameCase::Snake
    } else if kebab > snake && kebab > plain {
        NameCase::Kebab
    } else {
        NameCase::Any
    }
}

/// The preset whose eight slots the display settings match, `custom` when none does; `None` when
/// no slot is enabled. Disabled slots are not compared. Presets with the same slots are told
/// apart by the parent of the model.
fn display_preset(display: &DisplaySlots, parent: &str) -> Option<String> {
    if !display.slots.iter().any(|slot| slot.enabled) {
        return None;
    }
    let same = |preset: &DisplaySlots| {
        display.slots.iter().zip(&preset.slots).all(|(a, b)| {
            a.enabled == b.enabled
                && (!a.enabled
                    || (a.rotation, a.translation, a.scale) == (b.rotation, b.translation, b.scale))
        })
    };
    let parent = parent.strip_prefix("minecraft:").unwrap_or(parent);
    let named = DISPLAY_PRESETS.into_iter().find(|id| *id == parent);
    let found = named
        .into_iter()
        .chain(DISPLAY_PRESETS)
        .find(|id| builtin_display(id).is_some_and(|preset| same(&preset)));
    Some(found.unwrap_or("custom").to_owned())
}

/// Rounded to a thousandth, the precision of the stored numbers.
pub(crate) fn round3(value: f32) -> f32 {
    (value * 1000.0).round() / 1000.0 + 0.0
}

/// The value at `q` (0 to 1) of sorted values, between the two nearest; 0 without values.
pub(crate) fn quantile(sorted: &[f32], q: f32) -> f32 {
    let Some(last) = sorted.len().checked_sub(1) else {
        return 0.0;
    };
    let at = q * last as f32;
    let (low, high) = (at.floor() as usize, at.ceil() as usize);
    sorted[low] + (sorted[high] - sorted[low]) * (at - low as f32)
}

/// Texels per block on one face: the square root of texel area over face area, times 16 units.
/// The UV rotation does not change the area, so it is not looked at.
fn face_density(
    textures: &TextureList,
    element: &cuboid_model::Element,
    face: &cuboid_model::Face,
) -> Option<f32> {
    let entry = textures
        .entries
        .iter()
        .find(|e| *e.label == *face.texture)?;
    let s = element.base.s;
    let (a, b) = match face.name {
        FaceName::Up | FaceName::Down => (s.x, s.z),
        FaceName::North | FaceName::South => (s.x, s.y),
        FaceName::West | FaceName::East => (s.z, s.y),
    };
    let (tl, br) = (face.uv.tl(), face.uv.br());
    // A face is relative to one frame of an animated texture.
    let (w, h) = entry.frame_size();
    let texels = (br.x - tl.x).abs() * w as f32 * (br.y - tl.y).abs() * h as f32;
    let area = a.abs() * b.abs();
    if texels <= 0.0 || area < 1e-4 || !texels.is_finite() {
        return None;
    }
    Some((texels / area).sqrt() * 16.0)
}

/// The median of the values; the mean of the middle two for an even count. Sorts in place.
pub fn median(values: &mut Vec<f32>) -> Option<f32> {
    values.retain(|v| v.is_finite());
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    })
}

/// Metadata of a PNG, with the bytes of its `.png.mcmeta` when it has one.
pub fn texture_meta(bytes: &[u8], mcmeta: Option<&[u8]>) -> Result<TextureMeta, String> {
    let (width, height) = png_size(bytes).ok_or("not a PNG file")?;
    let frames = mcmeta.map_or(1, |text| frame_count(text, width, height));
    let texels = u64::from(width) * u64::from(height);
    if texels > MAX_DECODE_TEXELS {
        return Ok(TextureMeta {
            width,
            height,
            frames,
            alpha: None,
            palette: Vec::new(),
        });
    }
    let image = decode_file(bytes, ImageFileKind::Other).map_err(|e| e.to_string())?;
    let mut colours = Colours::default();
    colours.add(image.rgba(), width, height);
    Ok(TextureMeta {
        width,
        height,
        frames,
        alpha: Some(colours.alpha()),
        palette: colours.top(),
    })
}

/// Width and height from the IHDR chunk.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let header = bytes.get(16..24)?;
    let width = u32::from_be_bytes(header[0..4].try_into().ok()?);
    let height = u32::from_be_bytes(header[4..8].try_into().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

/// Frames of an animated texture: the `frames` list, else the number of square frames stacked in
/// the picture. A picture without an `animation` entry is still.
fn frame_count(mcmeta: &[u8], width: u32, height: u32) -> u32 {
    let Ok(Value::Object(root)) = serde_json::from_slice::<Value>(mcmeta) else {
        return 1;
    };
    let Some(animation) = root.get("animation") else {
        return 1;
    };
    if let Some(list) = animation.get("frames").and_then(Value::as_array) {
        return u32::try_from(list.len())
            .unwrap_or(MAX_FRAMES)
            .clamp(1, MAX_FRAMES);
    }
    let frame_w = animation
        .get("width")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v > 0)
        .unwrap_or(width);
    let frame_h = animation
        .get("height")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v > 0)
        .unwrap_or(frame_w);
    ((width / frame_w.max(1)) * (height / frame_h.max(1))).clamp(1, MAX_FRAMES)
}

/// Colours of straight RGBA texels, counted in buckets of 5 bits per channel and reported as the
/// mean of each bucket, so a texture of exact colours reports them exactly. Texels with an alpha
/// below 128 do not count.
#[derive(Default)]
struct Colours {
    buckets: HashMap<u16, ([u64; 3], u64)>,
    partly: bool,
    clear: bool,
}

impl Colours {
    /// Counts the texels of one picture.
    fn add(&mut self, rgba: &[u8], width: u32, height: u32) {
        let texels = u64::from(width) * u64::from(height);
        let over = texels / PALETTE_SAMPLE;
        let step = if over == 0 {
            1
        } else {
            usize::try_from(over.isqrt() + 1).unwrap_or(1)
        };
        let width = width as usize;
        for (index, texel) in rgba.chunks_exact(4).enumerate() {
            let alpha = texel[3];
            self.partly |= alpha != 0 && alpha != 255;
            self.clear |= alpha == 0;
            let (x, y) = (index % width, index / width);
            if alpha < 128 || (step > 1 && (x % step != 0 || y % step != 0)) {
                continue;
            }
            let key = (u16::from(texel[0] >> 3) << 10)
                | (u16::from(texel[1] >> 3) << 5)
                | u16::from(texel[2] >> 3);
            let slot = self.buckets.entry(key).or_default();
            for (sum, value) in slot.0.iter_mut().zip(&texel[..3]) {
                *sum += u64::from(*value);
            }
            slot.1 += 1;
        }
    }

    fn alpha(&self) -> AlphaUse {
        if self.partly {
            AlphaUse::Translucent
        } else if self.clear {
            AlphaUse::Cutout
        } else {
            AlphaUse::Opaque
        }
    }

    /// The most used colours, up to [`PALETTE_COLORS`], most used first.
    fn top(self) -> Vec<[u8; 3]> {
        let mut list: Vec<(u16, [u64; 3], u64)> = self
            .buckets
            .into_iter()
            .map(|(k, (s, n))| (k, s, n))
            .collect();
        // The bucket key makes the order stable.
        list.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        list.truncate(PALETTE_COLORS);
        list.into_iter()
            .map(|(_, sum, count)| {
                sum.map(|v| u8::try_from((v + count / 2) / count).unwrap_or(255))
            })
            .collect()
    }
}

/// Words that make a heading a list of names.
const NAME_SECTIONS: [&str; 22] = [
    "item",
    "items",
    "weapon",
    "weapons",
    "character",
    "characters",
    "armor",
    "armour",
    "tool",
    "tools",
    "mob",
    "mobs",
    "creature",
    "creatures",
    "enemy",
    "enemies",
    "boss",
    "bosses",
    "npc",
    "npcs",
    "block",
    "blocks",
];

/// Metadata of a lore note: the first heading is the title, headings are lines starting with
/// `#`, and the names are the bullets (`-`, `*`, `+` or `1.`) under a heading that has one of the
/// words in [`NAME_SECTIONS`]; the section ends at the next heading of the same or a higher level.
/// A name is the bullet text without emphasis marks, links, and anything after a colon, a spaced
/// dash or a bracket.
pub fn lore_meta(bytes: &[u8]) -> LoreMeta {
    let bytes = &bytes[..bytes.len().min(LORE_READ)];
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let text = String::from_utf8_lossy(bytes);
    let mut meta = LoreMeta {
        words: u32::try_from(text.split_whitespace().count()).unwrap_or(u32::MAX),
        ..LoreMeta::default()
    };
    let mut section_level: Option<usize> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some((level, heading)) = heading_of(trimmed) {
            if meta.title.is_none() {
                meta.title = Some(heading.to_owned());
            }
            if meta.headings.len() < MAX_HEADINGS {
                meta.headings.push(heading.to_owned());
            }
            if section_level.is_none_or(|open| level <= open) {
                section_level = is_name_section(heading).then_some(level);
            }
            continue;
        }
        if section_level.is_none() || meta.names.len() >= MAX_NAMES {
            continue;
        }
        if let Some(name) = bullet_name(trimmed)
            && !meta.names.contains(&name)
        {
            meta.names.push(name);
        }
    }
    meta
}

fn heading_of(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let text = line[level..]
        .strip_prefix(' ')?
        .trim()
        .trim_end_matches('#')
        .trim();
    (!text.is_empty()).then_some((level, text))
}

fn is_name_section(heading: &str) -> bool {
    heading
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| NAME_SECTIONS.contains(&word))
}

fn bullet_name(line: &str) -> Option<String> {
    let rest = if let Some(rest) = line.strip_prefix(['-', '*', '+']) {
        rest.strip_prefix(' ')?
    } else {
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        line[digits..].strip_prefix(['.', ')'])?.strip_prefix(' ')?
    };
    let plain = strip_markup(rest);
    let end = [":", " - ", " \u{2013} ", " \u{2014} ", " ("]
        .iter()
        .filter_map(|cut| plain.find(cut))
        .min()
        .unwrap_or(plain.len());
    let name = plain[..end].trim();
    (!name.is_empty() && name.chars().count() <= MAX_NAME_LEN).then(|| name.to_owned())
}

/// Removes emphasis marks and backticks and turns `[text](url)` into `text`.
fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '['
            && let Some((label, after)) = rest[1..].split_once("](")
            && let Some((_, tail)) = after.split_once(')')
        {
            out.push_str(label);
            rest = tail;
            continue;
        }
        if !matches!(c, '*' | '_' | '`') {
            out.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use cuboid_image::{Image, RgbaBuf, encode_png};

    use super::*;

    fn png(w: u32, h: u32, texel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut buf = RgbaBuf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let at = (y * w + x) as usize * 4;
                buf.pixels[at..at + 4].copy_from_slice(&texel(x, y));
            }
        }
        encode_png(&Image::from_rgba(buf)).expect("encodes")
    }

    #[test]
    fn a_texture_reports_size_alpha_and_palette() {
        let bytes = png(4, 2, |x, _| {
            if x < 2 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        });
        let meta = texture_meta(&bytes, None).expect("meta");
        assert_eq!((meta.width, meta.height, meta.frames), (4, 2, 1));
        assert_eq!(meta.alpha, Some(AlphaUse::Opaque));
        // Equal counts: the bucket key orders them, blue (low key) first.
        assert_eq!(meta.palette, [[0, 0, 255], [255, 0, 0]]);
    }

    #[test]
    fn alpha_use_tells_cutout_from_translucent() {
        let cutout = png(
            2,
            2,
            |x, _| if x == 0 { [9, 9, 9, 0] } else { [9, 9, 9, 255] },
        );
        assert_eq!(
            texture_meta(&cutout, None).expect("meta").alpha,
            Some(AlphaUse::Cutout)
        );
        let glass = png(2, 2, |_, _| [9, 9, 9, 100]);
        let meta = texture_meta(&glass, None).expect("meta");
        assert_eq!(meta.alpha, Some(AlphaUse::Translucent));
        assert!(
            meta.palette.is_empty(),
            "texels below alpha 128 do not count"
        );
    }

    #[test]
    fn the_palette_has_at_most_16_colours_most_used_first() {
        // 40 distinct colours; colour 0 covers most of the picture.
        let bytes = png(8, 8, |x, y| {
            let i = (x + y * 8) as u8;
            if i < 24 {
                [200, 10, 10, 255]
            } else {
                [(i % 8) * 32, (i / 8) * 32, 100, 255]
            }
        });
        let meta = texture_meta(&bytes, None).expect("meta");
        assert_eq!(meta.palette.len(), PALETTE_COLORS);
        assert_eq!(meta.palette[0], [200, 10, 10]);
    }

    #[test]
    fn frames_come_from_the_mcmeta() {
        let strip = png(2, 8, |_, _| [1, 2, 3, 255]);
        assert_eq!(texture_meta(&strip, None).expect("meta").frames, 1);
        let animated = br#"{"animation": {"frametime": 2}}"#;
        assert_eq!(
            texture_meta(&strip, Some(animated)).expect("meta").frames,
            4
        );
        let listed = br#"{"animation": {"frames": [0, 1, 2]}}"#;
        assert_eq!(texture_meta(&strip, Some(listed)).expect("meta").frames, 3);
        assert_eq!(
            texture_meta(&strip, Some(b"{broken")).expect("meta").frames,
            1
        );
        assert_eq!(
            texture_meta(&strip, Some(br#"{"texture": {}}"#))
                .expect("meta")
                .frames,
            1
        );
    }

    #[test]
    fn a_broken_png_is_an_error() {
        assert!(texture_meta(b"hello", None).is_err());
        let mut bytes = png(2, 2, |_, _| [1, 2, 3, 255]);
        bytes.truncate(40);
        assert!(texture_meta(&bytes, None).is_err());
    }

    #[test]
    fn minecraft_model_json_gives_counts_and_bounds() {
        let json = br#"{"textures": {"0": "block/a", "particle": "block/a"},
            "elements": [{"from": [0, 0, 0], "to": [16, 8, 16]}, {"from": [2, 8, 2], "to": [14, 20, 14]}]}"#;
        let meta = model_meta("slab.json", json).expect("meta");
        assert_eq!(
            (meta.format.as_str(), meta.elements, meta.textures),
            ("json", 2, 2)
        );
        // The boxes are in the space of the document, where z is mirrored.
        assert_eq!(meta.bounds, Some([0.0, 0.0, -16.0, 16.0, 20.0, 0.0]));
        assert_eq!(meta.density, None);
        assert_eq!(meta.texture_size, None);
        assert!(model_meta("bad.json", b"{").is_err());
    }

    #[test]
    fn a_minecraft_json_gives_its_parent_display_and_style_through_the_importer() {
        let json = br#"{"parent": "item/handheld", "textures": {"layer0": "item/lamp"},
            "elements": [
                {"from": [0, 0, 0], "to": [3, 12, 3]},
                {"from": [4, 4, 4], "to": [12, 6, 12],
                 "rotation": {"origin": [8, 4, 8], "axis": "y", "angle": 45}}]}"#;
        let meta = model_meta("lamp.json", json).expect("meta");
        assert_eq!((meta.elements, meta.textures), (2, 1));
        assert_eq!((meta.density, meta.texture_size), (None, None));
        let style = meta.style.expect("style");
        assert_eq!(style.parent.as_deref(), Some("item/handheld"));
        assert_eq!(style.display.as_deref(), Some("item/handheld"));
        assert_eq!(style.sides, Some([8.0, 9.0, 10.0, 11.0, 12.0]));
        assert_eq!((style.thin, style.rotated), (0, 1));
        assert_eq!(style.angles, [45.0]);
        assert_eq!((style.groups, style.depth), (0, 0));
        assert!(style.palette.is_empty() && style.densities.is_none());
        assert!(style.tokens.is_empty(), "the default label is no word");
    }

    /// A bbmodel with one cube for each name.
    fn named(names: &[&str]) -> Vec<u8> {
        let cubes: Vec<String> = names
            .iter()
            .map(|name| {
                format!(r#"{{"name": "{name}", "from": [0, 0, 0], "to": [1, 1, 1], "origin": [0, 0, 0]}}"#)
            })
            .collect();
        format!(
            r#"{{"meta": {{"format_version": "4.10", "model_format": "free"}}, "name": "m",
            "elements": [{}]}}"#,
            cubes.join(",")
        )
        .into_bytes()
    }

    #[test]
    fn the_case_of_a_model_is_the_plurality_of_snake_kebab_and_neither() {
        let case = |names: &[&str]| {
            let meta = model_meta("m.bbmodel", &named(names)).expect("meta");
            meta.style.expect("style").case
        };
        assert_eq!(
            case(&["left_arm", "right_arm", "head_top", "body"]),
            NameCase::Snake
        );
        assert_eq!(case(&["left-arm", "right-arm", "Cube"]), NameCase::Kebab);
        let mostly_plain = ["left_arm", "Cube", "Cube", "Cube"];
        assert_eq!(
            case(&mostly_plain),
            NameCase::Any,
            "one snake name among plain ones"
        );
        assert_eq!(case(&["left_arm", "right-arm"]), NameCase::Any, "a tie");
    }

    #[test]
    fn a_name_token_is_cut_at_24_characters() {
        let long = "ab".repeat(40);
        assert_eq!(words(&long), [&long[..MAX_WORD]]);
        let cjk = "\u{4e2d}".repeat(100);
        assert_eq!(words(&cjk), ["\u{4e2d}".repeat(MAX_WORD)]);
        assert_eq!(words("short_name"), ["short", "name"]);
    }

    /// A bbmodel with a group, a rotated element, a thin one and a 16 x 16 texture of two colours.
    fn arms() -> Vec<u8> {
        let texture = png(16, 16, |x, _| {
            if x < 8 {
                [200, 20, 20, 255]
            } else {
                [20, 20, 200, 255]
            }
        });
        let url = cuboid_fmt_bbmodel::data_url::png_url(&texture);
        let face = |uv: &str| format!(r#"{{"north": {{"uv": {uv}, "texture": 0}}}}"#);
        format!(
            r#"{{"meta": {{"format_version": "4.10", "model_format": "free"}}, "name": "arms",
            "resolution": {{"width": 16, "height": 16}},
            "elements": [
                {{"name": "left_arm", "from": [0, 0, 0], "to": [2, 10, 2], "origin": [0, 0, 0],
                  "uuid": "e1", "faces": {}}},
                {{"name": "right_arm", "from": [4, 0, 0], "to": [6, 10, 2], "origin": [4, 0, 0],
                  "rotation": [0, 0, 45], "uuid": "e2", "faces": {}}},
                {{"name": "trim", "from": [0, 10, 0], "to": [6, 11, 2], "origin": [0, 10, 0],
                  "uuid": "e3", "faces": {}}}],
            "outliner": [{{"name": "arms", "origin": [0, 0, 0], "uuid": "g1",
                "children": ["e1", "e2"]}}, "e3"],
            "textures": [{{"name": "arms.png", "id": "0", "source": "{url}"}}]}}"#,
            face("[0, 0, 2, 10]"),
            face("[0, 0, 2, 10]"),
            face("[0, 0, 6, 1]"),
        )
        .into_bytes()
    }

    #[test]
    fn a_model_keeps_a_few_hundred_bytes_of_style() {
        let meta = model_meta("arms.bbmodel", &arms()).expect("meta");
        let style = meta.style.clone().expect("style");
        assert_eq!((style.groups, style.depth), (1, 1));
        assert_eq!(style.sides, Some([6.0, 8.0, 10.0, 10.0, 10.0]));
        assert_eq!((style.thin, style.rotated), (1, 1));
        assert_eq!(style.angles, [45.0]);
        assert_eq!(style.densities, Some([16.0, 16.0, 16.0]));
        assert_eq!(style.palette, [[20, 20, 200], [200, 20, 20]]);
        assert_eq!(style.tokens, ["arm", "arms", "left", "right", "trim"]);
        assert_eq!(style.case, NameCase::Any, "two snake names, two without");
        assert_eq!((style.display, style.parent), (None, None));
        let text = serde_json::to_string(&meta).expect("encodes");
        assert!(text.len() < 700, "{} bytes: {text}", text.len());
        let again: ModelMeta = serde_json::from_str(&text).expect("reads");
        assert_eq!(again, meta);
    }

    #[test]
    fn names_split_at_capitals_and_everything_that_is_no_letter() {
        assert_eq!(words("headTrim_L2"), ["head", "trim"]);
        assert_eq!(words("Box12"), ["box"]);
        assert_eq!(words("left-arm 2"), ["left", "arm"]);
        assert_eq!(words("A"), Vec::<String>::new());
        let numbers = [
            fold_angle(337.5),
            fold_angle(90.0),
            quantile(&[1.0, 3.0], 0.5),
            quantile(&[], 0.5),
        ];
        let wanted = [22.5, 90.0, 2.0, 0.0];
        assert!(
            numbers
                .iter()
                .zip(wanted)
                .all(|(a, b)| (a - b).abs() < 1e-4),
            "{numbers:?}"
        );
    }

    #[test]
    fn a_bbmodel_gives_counts_bounds_and_density() {
        let url = cuboid_fmt_bbmodel::data_url::png_url(&png(16, 16, |_, _| [9, 9, 9, 255]));
        let project = format!(
            r#"{{"meta": {{"format_version": "4.10", "model_format": "free"}}, "name": "m",
            "resolution": {{"width": 16, "height": 16}},
            "elements": [{{"name": "a", "from": [0, 0, 0], "to": [16, 16, 16], "origin": [8, 8, 8],
                "faces": {{"north": {{"uv": [0, 0, 16, 16], "texture": 0}}}}}}],
            "textures": [{{"name": "t.png", "id": "0", "source": "{url}"}}]}}"#
        );
        let meta = model_meta("m.bbmodel", project.as_bytes()).expect("meta");
        assert_eq!(meta.format, "bbmodel");
        assert_eq!(meta.elements, 1);
        assert_eq!(meta.textures, 1);
        let bounds = meta.bounds.expect("bounds");
        assert!((bounds[3] - bounds[0] - 16.0).abs() < 0.01, "{bounds:?}");
        // A 16 x 16 texture over a 16 unit face: 16 texels per block.
        let density = meta.density.expect("density");
        assert!((density - 16.0).abs() < 0.5, "{density}");
    }

    #[test]
    fn lore_gives_title_words_headings_and_listed_names() {
        let text = "# The Ink Wars\n\nA long story.\n\n## Weapons\n\n- **Ink Blaster**: a gun\n\
            - [Roller](http://x) - heavy\n* Charger (rare)\n1. Splat Brush\n\n### Notes\n- Not a name\n\n\
            ## History\n- Founding\n\n## Key Characters\n- Marie\n";
        let meta = lore_meta(text.as_bytes());
        assert_eq!(meta.title.as_deref(), Some("The Ink Wars"));
        assert_eq!(
            meta.headings,
            [
                "The Ink Wars",
                "Weapons",
                "Notes",
                "History",
                "Key Characters"
            ]
        );
        // "Notes" is a deeper heading inside the Weapons section, so its bullets count too.
        assert_eq!(
            meta.names,
            [
                "Ink Blaster",
                "Roller",
                "Charger",
                "Splat Brush",
                "Not a name",
                "Marie"
            ]
        );
        assert!(meta.words > 15);
    }

    #[test]
    fn lore_without_headings_has_no_title_and_no_names() {
        let meta = lore_meta(b"- an item\n- another\n");
        assert_eq!(meta.title, None);
        assert!(meta.names.is_empty());
        assert_eq!(meta.words, 5);
    }

    #[test]
    fn the_median_of_an_even_count_is_the_mean_of_the_middle_two() {
        assert_eq!(median(&mut vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&mut vec![4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(&mut vec![f32::NAN]), None);
        assert_eq!(median(&mut Vec::new()), None);
    }
}
