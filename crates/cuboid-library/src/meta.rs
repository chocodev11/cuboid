//! Metadata of the entries, read by the import worker with limits: models through the format
//! crates, textures through the image crate, lore with a small heading and bullet reader.

use std::collections::HashMap;
use std::path::Path;

use cuboid_fmt_cubik::Limits;
use cuboid_fmt_cubik::cub::{self, TextureFiles};
use cuboid_image::{ImageFileKind, decode_file};
use cuboid_io::texture_files::Refusal;
use cuboid_model::{FaceName, LoadedDoc, TextureList};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kind::{JsonClass, extension};

/// Texels above which a texture is not decoded for its palette.
const MAX_DECODE_TEXELS: u64 = 1 << 24;
/// Texels looked at for the palette; larger pictures are sampled on a grid.
const PALETTE_SAMPLE: u64 = 1 << 20;
/// Colours kept per texture.
pub const PALETTE_COLORS: usize = 16;
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
    /// and mesh boxes; rotations are ignored.
    pub bounds: Option<[f32; 6]>,
    /// Median texels per block over the active faces that have a texture; `None` when the
    /// format does not give the texture size.
    pub density: Option<f32>,
    /// The largest side of the textures the file holds, in texels; `None` without textures or
    /// when the format does not give the texture size.
    pub texture_size: Option<u32>,
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
        _ => minecraft_model_meta(bytes),
    }
}

/// Counts and bounds of a Minecraft model JSON. The texture size is not in the file, so there is
/// no density.
fn minecraft_model_meta(bytes: &[u8]) -> Result<ModelMeta, String> {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let root: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let elements = root
        .get("elements")
        .and_then(Value::as_array)
        .ok_or("the model has no elements")?;
    let textures = root
        .get("textures")
        .and_then(Value::as_object)
        .map_or(0, |map| map.values().filter(|v| v.is_string()).count());
    let corner = |element: &Value, key: &str| -> Option<[f32; 3]> {
        let values = element.get(key)?.as_array()?;
        let mut out = [0.0_f32; 3];
        for (slot, value) in out.iter_mut().zip(values) {
            *slot = value.as_f64()? as f32;
        }
        (values.len() == 3).then_some(out)
    };
    let mut bounds = Bounds::default();
    for element in elements {
        if let (Some(from), Some(to)) = (corner(element, "from"), corner(element, "to")) {
            bounds.add(from, to);
        }
    }
    Ok(ModelMeta {
        format: "json".to_owned(),
        elements: u32::try_from(elements.len()).unwrap_or(u32::MAX),
        textures: u32::try_from(textures).unwrap_or(u32::MAX),
        bounds: bounds.finish(),
        ..ModelMeta::default()
    })
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

/// Counts, bounds and density of a loaded Cubik document.
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
    ModelMeta {
        format: format.to_owned(),
        elements: u32::try_from(elements).unwrap_or(u32::MAX),
        meshes: u32::try_from(meshes).unwrap_or(u32::MAX),
        textures: u32::try_from(textures.map_or(0, |t| t.entries.len())).unwrap_or(u32::MAX),
        voxels: matches!(sections.voxels, Some(Some(_))),
        bounds: bounds.finish(),
        density: median(&mut densities),
        texture_size: textures.and_then(|t| {
            t.entries
                .iter()
                .map(|e| {
                    let (w, h) = e.frame_size();
                    w.max(h)
                })
                .max()
        }),
    }
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
    let (alpha, palette) = scan(image.rgba(), width, height);
    Ok(TextureMeta {
        width,
        height,
        frames,
        alpha: Some(alpha),
        palette,
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

/// Alpha use and palette of straight RGBA texels. Colours are counted in buckets of 5 bits per
/// channel and reported as the mean of each bucket, so a texture of exact colours reports them
/// exactly. Texels with an alpha below 128 do not count.
fn scan(rgba: &[u8], width: u32, height: u32) -> (AlphaUse, Vec<[u8; 3]>) {
    let (mut partly, mut clear) = (false, false);
    let texels = u64::from(width) * u64::from(height);
    let over = texels / PALETTE_SAMPLE;
    let step = if over == 0 {
        1
    } else {
        usize::try_from(over.isqrt() + 1).unwrap_or(1)
    };
    let mut buckets: HashMap<u16, ([u32; 3], u32)> = HashMap::new();
    let width = width as usize;
    for (index, texel) in rgba.chunks_exact(4).enumerate() {
        let alpha = texel[3];
        partly |= alpha != 0 && alpha != 255;
        clear |= alpha == 0;
        let (x, y) = (index % width, index / width);
        if alpha < 128 || (step > 1 && (x % step != 0 || y % step != 0)) {
            continue;
        }
        let key = (u16::from(texel[0] >> 3) << 10)
            | (u16::from(texel[1] >> 3) << 5)
            | u16::from(texel[2] >> 3);
        let slot = buckets.entry(key).or_default();
        for (sum, value) in slot.0.iter_mut().zip(&texel[..3]) {
            *sum += u32::from(*value);
        }
        slot.1 += 1;
    }
    let mut list: Vec<(u16, [u32; 3], u32)> =
        buckets.into_iter().map(|(k, (s, n))| (k, s, n)).collect();
    // Most used first; the bucket key makes the order stable.
    list.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    list.truncate(PALETTE_COLORS);
    let palette = list
        .into_iter()
        .map(|(_, sum, count)| sum.map(|v| u8::try_from((v + count / 2) / count).unwrap_or(255)))
        .collect();
    let alpha = if partly {
        AlphaUse::Translucent
    } else if clear {
        AlphaUse::Cutout
    } else {
        AlphaUse::Opaque
    };
    (alpha, palette)
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
        assert_eq!(meta.bounds, Some([0.0, 0.0, 0.0, 16.0, 20.0, 16.0]));
        assert_eq!(meta.density, None);
        assert!(model_meta("bad.json", b"{").is_err());
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
