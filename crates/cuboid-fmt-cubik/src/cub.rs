//! Legacy `.cub`, "Cubik model alpha" (native 9). Read only: no writer exists.
//!
//! The file is text, decoded the way a .NET reader does (byte-order marks, invalid sequences
//! replaced), obfuscated with a double XOR over the UTF-16 code units, and holds a Minecraft
//! model JSON with Cubik keys in the permissive dialect of `cuboid_io::text::jsonlite`.
//!
//! [`import`] builds the same [`LoadedDoc`] as the `.cubik` loader. The dialogs of native 9.5
//! (inherited model, generated model, no texture) are reported as [`Notice`]s for the caller to
//! show. Texture files are found through [`TextureFiles`], so the importer itself does not
//! touch the file system.
//!
//! spec: native 9, interop 8 (shared importer)

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cuboid_io::text::{Value, decode_bom, jsonlite};
pub use cuboid_io::texture_files::{DiskFiles, MAX_TEXTURE_FILE, Refusal, TextureFiles};
use cuboid_io::texture_files::{Form, form};
use cuboid_model::apply::{VoxelReplay, voxel_world_bytes};
use cuboid_model::image::{Image, ImageFileKind, StoredPixels, decode_file, format, signature};
use cuboid_model::uv::default_uv_rects;
use cuboid_model::voxel::I3;
use cuboid_model::{
    Blueprint, Categories, DisplaySlot, DisplaySlots, Element, ElementFlags, ElementType,
    ElementsSnap, Face, FaceFlags, FaceName, Label, ListSnap, LoadedDoc, ModelKind, ModelProps,
    ObjectBase, OverrideCase, Perms, Snapshot, Texture, TextureEntry, TextureList, TextureSource,
    ViewProps, normal_form,
};
use cuboid_num::{euler_to_quat, quat_to_euler};
use glam::{Quat, Vec2, Vec3};
use rustc_hash::FxHashSet;
use thiserror::Error;

use crate::Limits;

/// The two keys of the obfuscation (native 9.3).
const K1: &[u8] = b"oBeeware";
const K2: &[u8] = b"BDcraft Cubik - Encryption System Key";

/// The code page of the texture signatures `cs`.
const CODE_PAGE: u16 = 1252;

/// The file text before the JSON parser: decoded, then XOR-ed with the key stream. The operation
/// is its own inverse.
///
/// spec: native 9.2, 9.3
pub fn deobfuscate(bytes: &[u8]) -> String {
    let text = decode_bom(bytes);
    let units: Vec<u16> = text
        .encode_utf16()
        .enumerate()
        .map(|(i, unit)| {
            let k1 = K1.get(i % K1.len()).copied().unwrap_or(0);
            let k2 = K2.get(i % K2.len()).copied().unwrap_or(0);
            unit ^ u16::from(k1 ^ k2)
        })
        .collect();
    String::from_utf16_lossy(&units)
}

/// Why a `.cub` file is not imported.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CubError {
    /// The text is not in the JSON dialect ("Invalid CUB").
    #[error("Invalid data found")]
    Syntax,
    /// None of `elements`, `textures` and `parent` is present ("Invalid CUB Model").
    #[error("the file is not a valid model")]
    NotAModel,
    /// `parent` is `item/generated` ("Generated Model").
    #[error("generated item models cannot be imported")]
    Generated,
    /// A value that must be a string is not one; the original aborts the import.
    #[error("`{0}` is not a string")]
    NotAString(&'static str),
    /// A face without `texture`.
    #[error("a face has no `texture`")]
    FaceWithoutTexture,
    /// A `voxel` object without `voxels`.
    #[error("the `voxel` object has no `voxels` array")]
    MissingVoxels,
    /// The file is larger than a limit of [`Limits`] allows, named here.
    #[error("the file exceeds the limit on {0}")]
    Limit(&'static str),
}

/// Something the original shows a dialog or message for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// "Inherited Model": the model has a `parent`. With `ask` the user chooses Import or Abort;
    /// otherwise the box is informational (OK or Abort).
    Inherited {
        /// The parent model.
        parent: String,
        /// The model has no elements or a parent other than `block/block` and `item/handheld`.
        ask: bool,
    },
    /// "No texture found": the file has no `textures_path`.
    NoTexture,
    /// A texture file was not read for the reason given; the texture is missing in the model.
    TextureRefused {
        /// The texture key.
        key: String,
        /// The path that was refused.
        path: String,
        /// Why.
        reason: Refusal,
    },
}

/// An imported `.cub` file.
#[derive(Debug)]
pub struct CubImport {
    /// The sections, to apply with `ApplyMode::Open`.
    pub doc: LoadedDoc,
    /// The dialogs the original shows.
    pub notices: Vec<Notice>,
}

/// Imports a `.cub` file with the default [`Limits`]; see [`import_with`].
pub fn import(bytes: &[u8], path: &Path, files: &dyn TextureFiles) -> Result<CubImport, CubError> {
    import_with(bytes, path, files, &Limits::default())
}

/// Imports a `.cub` file. `path` is the location of the file: its name gives the initial model
/// name and its folder is searched for textures. The size of the file, the elements, the texture
/// entries, the texels and the voxel cells are checked against `limits` before they are built.
///
/// spec: native 9.4, 9.5, 8.1
pub fn import_with(
    bytes: &[u8],
    path: &Path,
    files: &dyn TextureFiles,
    limits: &Limits,
) -> Result<CubImport, CubError> {
    if bytes.len() as u64 > limits.max_text {
        return Err(CubError::Limit("the file size"));
    }
    let text = deobfuscate(bytes);
    let root = jsonlite::parse(&text).map_err(|_| CubError::Syntax)?;
    build(&root, path, files, limits)
}

fn string<'a>(root: &'a Value, key: &str) -> Option<&'a str> {
    root.get(key).and_then(Value::as_str)
}

/// A string member: empty when the key is missing, an error when it is something else (the
/// original aborts on a null string).
fn text<'a>(root: &'a Value, key: &'static str) -> Result<&'a str, CubError> {
    match root.get(key) {
        None => Ok(""),
        Some(value) => value.as_str().ok_or(CubError::NotAString(key)),
    }
}

fn flag(root: &Value, key: &str, default: bool) -> bool {
    root.get(key).map_or(default, Value::as_bool)
}

fn number(root: &Value, key: &str, default: f32) -> f32 {
    root.get(key).map_or(default, Value::as_f32)
}

fn vec3(root: &Value, key: &str) -> Option<Vec3> {
    root.get(key)?.vec3().map(Vec3::from_array)
}

fn build(
    root: &Value,
    path: &Path,
    files: &dyn TextureFiles,
    limits: &Limits,
) -> Result<CubImport, CubError> {
    if ["elements", "textures", "parent"]
        .iter()
        .all(|key| root.get(key).is_none())
    {
        return Err(CubError::NotAModel);
    }
    let mut notices = Vec::new();
    let mut props = ModelProps::default();
    let mut view = ViewProps {
        mouse_step: number(root, "gridmousestep", 1.0),
        key_step: number(root, "gridkeystep", 1.0),
        display_step: number(root, "gridstep", 1.0),
        ..ViewProps::default()
    };
    view.cameras = [None; 4];

    if let Some(parent) = root.get("parent") {
        let parent = parent.as_str().ok_or(CubError::NotAString("parent"))?;
        if parent == "item/generated" {
            return Err(CubError::Generated);
        }
        let has_elements = root.get("elements").is_some();
        let known = parent == "block/block" || parent == "item/handheld";
        notices.push(Notice::Inherited {
            parent: parent.to_owned(),
            ask: !has_elements || !known,
        });
        parent.clone_into(&mut props.minecraft.parent);
        props.minecraft.kind = if parent == "item/handheld" {
            ModelKind::Item
        } else {
            ModelKind::Block
        };
    }
    props.general.name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(comment) = root.get("__comment") {
        props.general.description = comment
            .as_str()
            .ok_or(CubError::NotAString("__comment"))?
            .to_owned();
    }
    if let Some(ao) = root.get("ambientocclusion") {
        props.minecraft.ambient_occlusion = ao.as_bool();
    }
    if let Some(origin) = vec3(root, "modelOrigin") {
        props.general.origin = origin;
    }
    // The name key replaces the file name, so a missing key gives an empty name.
    props.general.name = text(root, "name")?.to_owned();
    props.minecraft.kind = match root.get("type").map_or(1, Value::as_i32) {
        0 => ModelKind::None,
        2 => ModelKind::Item,
        3 => ModelKind::Entity,
        4 => ModelKind::Optifine,
        5 => ModelKind::Map,
        _ => ModelKind::Block,
    };
    props.minecraft.entity_resolution = 16;
    props.minecraft.auto_vanilla_parent = flag(root, "usevanilladisplaysettings", true);
    props.minecraft.include_display = root.get("display").is_some();
    props.minecraft.keep_overrides = root.get("overrides").is_some();
    props.general.prevent_bleeding = flag(root, "preventbleedingtextures", false);
    props.general.exclude_hidden = flag(root, "excludehiddenelements", false);
    props.general.minify = flag(root, "minifyexport", false);
    if let Some(model_box) = root.get("modelBox") {
        let n = |key: &str, default: f32| number(model_box, key, default);
        props.general.model_box = Some(cuboid_model::ModelBox {
            min: Vec3::new(n("xmin", -16.0), n("ymin", -16.0), n("zmin", -32.0)),
            max: Vec3::new(n("xmax", 32.0), n("ymax", 32.0), n("zmax", 16.0)),
        });
    }

    let mut textures = texture_table(root, path, files, limits, &mut notices)?;
    let items = root
        .get("elements")
        .and_then(Value::as_array)
        .unwrap_or_default();
    if items.len() > limits.max_objects as usize {
        return Err(CubError::Limit("elements"));
    }
    let mut elements = Vec::with_capacity(items.len());
    for item in items {
        elements.push(element(item, &mut textures, limits)?);
    }
    let list = ListSnap::fresh(elements);
    let selected = vec![false; list.ids.len()];

    let display = display(root);
    let overrides = root.get("overrides").map(overrides);
    let (voxels, replay) = voxels(root, limits)?;

    let textures = textures.entries;
    let has_textures = !textures.is_empty();
    let sections = Snapshot {
        categories: Categories::MODEL_PROPERTIES
            | Categories::VIEW_PROPERTIES
            | Categories::ELEMENTS
            | Categories::VOXELS
            | Categories::MODEL_DISPLAY
            | if has_textures {
                Categories::TEXTURES
            } else {
                Categories::empty()
            },
        props: Some(Arc::new(props)),
        view: Some(Arc::new(view)),
        elements: Some(ElementsSnap {
            blueprint: Blueprint::default(),
            list,
            selected,
        }),
        textures: has_textures.then(|| Arc::new(TextureList { entries: textures })),
        voxels: Some(voxels),
        meshes: None,
        display: Some(Arc::new(display)),
    };
    Ok(CubImport {
        doc: LoadedDoc {
            sections,
            voxel_replay: replay,
            overrides,
            retained: None,
            keep_exact_rotations: false,
        },
        notices,
    })
}

// --- Textures -----------------------------------------------------------------------------------

/// Our own 16 x 16 pictures with the roles of Cubik's built-in ones (native 4.5.2): a frame, and
/// a checkerboard for the missing texture.
fn picture(name: &str, checker: bool) -> Texture {
    let mut raw = Vec::with_capacity(16 * 16 * 3);
    for y in 0..16usize {
        for x in 0..16usize {
            let edge = x == 0 || y == 0 || x == 15 || y == 15;
            let rgb: [u8; 3] = if checker {
                if (x / 4 + y / 4) % 2 == 0 {
                    [255, 0, 255]
                } else {
                    [0, 0, 0]
                }
            } else if edge {
                [0xC1, 0xC1, 0xC1]
            } else {
                [0xFF, 0xFF, 0xFF]
            };
            raw.extend_from_slice(&rgb);
        }
    }
    let image = Image::from_stored(
        16,
        16,
        StoredPixels {
            format: format::RGB24,
            raw: raw.into(),
        },
    );
    let mut texture = Texture::new(Arc::new(image));
    texture.name = Label::from(name);
    texture
}

/// The Minecraft id of a path: the part after `/textures/` without extension, or the bare file
/// name without extension (native 4.5).
fn mc_id(path: &str) -> String {
    let path = path.replace('\\', "/");
    let rest = path
        .split_once("/textures/")
        .map_or_else(|| path.rsplit('/').next().unwrap_or(""), |(_, rest)| rest);
    rest.rsplit_once('.')
        .map_or(rest, |(stem, _)| stem)
        .to_owned()
}

/// The texture entries of the model while it is built, and what the entries are checked against.
struct Table {
    entries: Vec<TextureEntry>,
    /// The labels of `entries`, for the lookup of a face.
    labels: FxHashSet<String>,
    /// Texels of the decoded files so far.
    texels: u64,
}

/// The texture entries of `textures_path`.
fn texture_table(
    root: &Value,
    cub: &Path,
    files: &dyn TextureFiles,
    limits: &Limits,
    notices: &mut Vec<Notice>,
) -> Result<Table, CubError> {
    let mut table = Table {
        entries: Vec::new(),
        labels: FxHashSet::default(),
        texels: 0,
    };
    let Some(paths) = root.get("textures_path") else {
        notices.push(Notice::NoTexture);
        return Ok(table);
    };
    let members = paths.as_object();
    if members.is_some_and(|members| members.len() > limits.max_textures as usize) {
        return Err(CubError::Limit("texture entries"));
    }
    let dir = cub.parent().unwrap_or_else(|| Path::new(""));
    // The files together are bounded like the texels they may decode to.
    let mut read_left = limits.max_texels.saturating_mul(4);
    for (key, value) in members.into_iter().flatten() {
        let given = value
            .as_str()
            .ok_or(CubError::NotAString("textures_path"))?;
        let (candidates, id) = match given.strip_prefix(':') {
            Some(id) => (minecraft_candidates(cub, dir, id), id.to_owned()),
            None => (plain_candidates(dir, key, given), mc_id(given)),
        };
        let mut refused = None;
        let mut texture = None;
        for candidate in candidates {
            let bytes = match files.read(&candidate) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(reason) => {
                    refused.get_or_insert((candidate, reason));
                    continue;
                }
            };
            let len = bytes.len() as u64;
            if len > read_left {
                refused.get_or_insert((candidate, Refusal::TooLarge));
                continue;
            }
            read_left -= len;
            let ext = candidate
                .extension()
                .map(|e| e.to_string_lossy().into_owned());
            let kind = ImageFileKind::from_extension(ext.as_deref().unwrap_or(""));
            // The first file that exists decides, also when it is no picture.
            let Ok(image) = decode_file(&bytes, kind) else {
                break;
            };
            let texels = table
                .texels
                .saturating_add(u64::from(image.w) * u64::from(image.h));
            if texels > limits.max_texels {
                refused.get_or_insert((candidate, Refusal::TooLarge));
                break;
            }
            table.texels = texels;
            let mut tex = Texture::new(Arc::new(image));
            let full = candidate.to_string_lossy().replace('\\', "/");
            tex.source = TextureSource {
                path: Some(full.clone()),
                file_name: Some(mc_id(&full).rsplit('/').next().unwrap_or("").to_owned()),
                mc_path: id.clone(),
                signature: Some(signature(&bytes, CODE_PAGE)),
                file_size: len,
            };
            texture = Some(tex);
            break;
        }
        let texture = texture.unwrap_or_else(|| {
            if let Some((path, reason)) = refused {
                notices.push(Notice::TextureRefused {
                    key: key.clone(),
                    path: path.to_string_lossy().into_owned(),
                    reason,
                });
            }
            // The searched path is not kept; the name is the last segment of the id.
            let mut tex = picture("e_error", true);
            tex.source.file_name = Some(id.rsplit('/').next().unwrap_or("").to_owned());
            tex.source.mc_path = id;
            tex
        });
        table.labels.insert(key.clone());
        table.entries.push(TextureEntry {
            label: Label::from(key.as_str()),
            texture,
        });
    }
    Ok(table)
}

/// Where a texture named by `given` is looked for. A key or an id that is a rooted path is not
/// joined to the folder (the join would drop the folder); one that would be refused is passed on
/// as it is, so that the refusal is reported.
fn plain_candidates(dir: &Path, key: &str, given: &str) -> Vec<PathBuf> {
    let name = given.rsplit(['/', '\\']).next().unwrap_or(given);
    let mut out = vec![dir.join(name)];
    let keyed = format!("{key}.png");
    match form(&keyed) {
        Form::Relative => out.push(dir.join(keyed)),
        Form::Refused(_) => out.push(PathBuf::from(keyed)),
        Form::Local => {}
    }
    out.push(dir.join("textures").join(name));
    out.push(match form(given) {
        Form::Relative => dir.join(given),
        Form::Local | Form::Refused(_) => PathBuf::from(given),
    });
    out
}

fn minecraft_candidates(cub: &Path, dir: &Path, id: &str) -> Vec<PathBuf> {
    let file = format!("{id}.png");
    match form(&file) {
        Form::Relative => {}
        Form::Refused(_) => return vec![PathBuf::from(file)],
        Form::Local => return Vec::new(),
    }
    let mut out = Vec::new();
    if let Some(minecraft) = cub
        .ancestors()
        .find(|p| p.file_name().is_some_and(|name| name == "minecraft"))
    {
        out.push(minecraft.join("textures").join(&file));
    }
    out.push(dir.join("textures").join(&file));
    out.push(dir.join(file));
    out
}

// --- Elements -----------------------------------------------------------------------------------

const FACE_KEYS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

fn cull(value: Option<&Value>) -> FaceFlags {
    let Some(text) = value.and_then(Value::as_str) else {
        return FaceFlags::empty();
    };
    if let Ok(bits) = text.trim().parse::<i32>() {
        return FaceFlags::from_bits_truncate(u8::try_from(bits & 0x3F).unwrap_or(0));
    }
    let mut flags = FaceFlags::empty();
    for name in text.split(',').map(str::trim) {
        flags |= match name {
            "down" => FaceFlags::DOWN,
            "up" => FaceFlags::UP,
            "north" => FaceFlags::NORTH,
            "south" => FaceFlags::SOUTH,
            "west" => FaceFlags::WEST,
            "east" => FaceFlags::EAST,
            "any" | "all" => FaceFlags::ALL,
            "none" => FaceFlags::empty(),
            _ => return FaceFlags::empty(),
        };
    }
    flags
}

fn element(item: &Value, textures: &mut Table, limits: &Limits) -> Result<Element, CubError> {
    let from = vec3(item, "from").unwrap_or(Vec3::ZERO);
    let to = vec3(item, "to").unwrap_or(Vec3::ZERO);
    let p = Vec3::new(from.x, from.y, -from.z);
    let s = (to - from).map(|c| if c == 0.0 { 0.001 } else { c });

    let mut pivot = p;
    let mut euler = Vec3::ZERO;
    let mut rescale = false;
    if let Some(rotation) = item.get("rotation") {
        if let Some(origin) = vec3(rotation, "origin") {
            pivot = Vec3::new(origin.x, origin.y, -origin.z);
        }
        let angle = number(rotation, "angle", 0.0).to_radians();
        let axis = match string(rotation, "axis") {
            Some("x") => Some(Quat::from_axis_angle(Vec3::X, -angle)),
            Some("y") => Some(Quat::from_axis_angle(Vec3::Y, -angle)),
            // About -Z by -angle, which is +Z by +angle.
            Some("z") => Some(Quat::from_axis_angle(Vec3::Z, angle)),
            _ => None,
        };
        if let Some(q) = axis {
            euler = quat_to_euler(q);
        }
        rescale = flag(rotation, "rescale", false);
    } else if let Some(origin) = vec3(item, "origin") {
        pivot = origin;
    }

    // The rotation is stored by the rule of interop 2.2.1: the angles snap to the grid of
    // "Restrict rotations" (on after a reset), then the transform turns about the pivot, so the
    // stored position is the rotated corner.
    let euler = normal_form::rotation(euler, true, Vec3::ZERO);
    let position = pivot + euler_to_quat(euler) * (p - pivot);

    let label = text(item, "label")?;
    let mut base = ObjectBase::new(label, position, s, Perms::ELEMENT);
    base.o = pivot;
    base.e = euler;

    let mut flags = ElementFlags::empty();
    flags.set(ElementFlags::SHADE, flag(item, "shade", true));
    flags.set(ElementFlags::UV_SNAP, flag(item, "uvsnap", false));
    flags.set(
        ElementFlags::UV_SNAP_MIRROR,
        flag(item, "uvsnapmirrored", false),
    );
    flags.set(ElementFlags::RESCALE, rescale);

    let color = item.get("color").and_then(Value::vec4).unwrap_or([0.0; 4]);

    // The automatic unwrap first, then the faces that the file has.
    let auto = default_uv_rects(p, s, flags.contains(ElementFlags::UV_SNAP_MIRROR));
    let first = textures
        .entries
        .first()
        .map_or("texture", |e| &e.label)
        .to_owned();
    let mut faces: [Face; 6] = std::array::from_fn(|slot| {
        let name = FaceName::ALL.get(slot).copied().unwrap_or(FaceName::Down);
        let mut face = Face::new(name, false, Label::from(first.as_str()));
        face.uv = auto.get(slot).copied().unwrap_or_default();
        face
    });
    if let Some(Value::Object(members)) = item.get("faces") {
        for (face, key) in faces.iter_mut().zip(FACE_KEYS) {
            let Some(spec) = members.get(key) else {
                continue;
            };
            face.active = true;
            if let Some([x1, y1, x2, y2]) = spec
                .get("uv")
                .and_then(Value::as_array)
                .and_then(|a| <[Value; 4]>::try_from(a.to_vec()).ok())
                .map(|a| a.each_ref().map(Value::as_f32))
            {
                face.uv.set_unclamped(
                    Vec2::new(x1 / 16.0, 1.0 - y2 / 16.0),
                    Vec2::new(x2 / 16.0, 1.0 - y1 / 16.0),
                );
            }
            let reference = spec
                .get("texture")
                .and_then(Value::as_str)
                .ok_or(CubError::FaceWithoutTexture)?;
            // The first character, normally `#`, is dropped.
            let mut chars = reference.chars();
            chars.next();
            let label = chars.as_str();
            if !textures.labels.contains(label) {
                if textures.entries.len() >= limits.max_textures as usize {
                    return Err(CubError::Limit("texture entries"));
                }
                textures.labels.insert(label.to_owned());
                textures.entries.push(TextureEntry {
                    label: Label::from(label),
                    texture: picture(
                        if label == "particle" {
                            "e_particle"
                        } else {
                            "e_texture"
                        },
                        false,
                    ),
                });
            }
            face.texture = Label::from(label);
            face.cull = cull(spec.get("cullface"));
            face.uv
                .set_rot(spec.get("rotation").map_or(0, Value::as_i32));
            face.tint = spec.get("tintindex").map_or(-1, Value::as_i32);
        }
    }

    Ok(Element {
        base,
        kind: ElementType::Cube,
        color,
        faces,
        flags,
    })
}

// --- Display, overrides, voxels -----------------------------------------------------------------

const SLOT_KEYS: [&str; 8] = [
    "thirdperson_righthand",
    "thirdperson_lefthand",
    "firstperson_righthand",
    "firstperson_lefthand",
    "gui",
    "head",
    "ground",
    "fixed",
];

/// Present slots are enabled with their values, absent ones disabled and reset.
fn display(root: &Value) -> DisplaySlots {
    let mut out = DisplaySlots::default();
    let Some(display) = root.get("display") else {
        return out;
    };
    for (slot, key) in out.slots.iter_mut().zip(SLOT_KEYS) {
        let Some(spec) = display.get(key) else {
            *slot = DisplaySlot::default();
            continue;
        };
        slot.enabled = true;
        slot.rotation = vec3(spec, "rotation").unwrap_or(Vec3::ZERO);
        slot.translation = vec3(spec, "translation").unwrap_or(Vec3::ZERO);
        slot.scale = vec3(spec, "scale").unwrap_or(Vec3::ONE);
    }
    out
}

fn overrides(list: &Value) -> Arc<[OverrideCase]> {
    list.as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            let predicate = item.get("predicate")?;
            let f = |key: &str| predicate.get(key).map(Value::as_f32);
            let i = |key: &str| predicate.get(key).map(Value::as_i32);
            Some(OverrideCase {
                model: string(item, "model").unwrap_or_default().to_owned(),
                angle: f("angle"),
                blocking: i("blocking"),
                broken: i("broken"),
                cast: i("cast"),
                damage: f("damage"),
                damaged: i("damaged"),
                lefthanded: i("lefthanded"),
                pull: f("pull"),
                pulling: i("pulling"),
                time: f("time"),
            })
        })
        .collect()
}

type Voxels = (Option<cuboid_model::voxel::VoxelSnap>, Option<VoxelReplay>);

/// `voxel`: a new grid of `resolution` (default 16) and the cells `[x, y, z, colour]`, global
/// indices; cells outside the grid are ignored.
fn voxels(root: &Value, limits: &Limits) -> Result<Voxels, CubError> {
    let Some(voxel) = root.get("voxel") else {
        return Ok((None, None));
    };
    let res = voxel.get("resolution").map_or(16, Value::as_i32);
    let items = voxel
        .get("voxels")
        .and_then(Value::as_array)
        .ok_or(CubError::MissingVoxels)?;
    if items.len() > limits.max_array as usize {
        return Err(CubError::Limit("voxel cells"));
    }
    // A grid that the apply path would have to refuse is refused here, before the cells.
    if u32::try_from(res).is_ok_and(|res| {
        res > 0 && voxel_world_bytes(res, 0).is_none_or(|bytes| bytes > limits.voxel_budget)
    }) {
        return Err(CubError::Limit("voxel memory"));
    }
    let size = i64::from(res).saturating_mul(3);
    let mut cells = Vec::with_capacity(items.len());
    for item in items {
        let Some([x, y, z, c]) = item.as_array().and_then(|a| a.first_chunk::<4>()) else {
            continue;
        };
        let (x, y, z) = (x.as_i32(), y.as_i32(), z.as_i32());
        if [x, y, z]
            .iter()
            .any(|&v| i64::from(v) < 0 || i64::from(v) >= size)
        {
            continue;
        }
        let colour = colour(c);
        if colour != 0 {
            cells.push((I3::new(x, y, z), colour));
        }
    }
    Ok((
        None,
        Some(VoxelReplay {
            res: u32::try_from(res).unwrap_or(0),
            cells,
        }),
    ))
}

/// A colour above 2^31 - 1 is an unsigned value of the same bits.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn colour(value: &Value) -> u32 {
    match value {
        Value::Number(n) if *n > f64::from(i32::MAX) && *n <= f64::from(u32::MAX) => *n as u32,
        other => other.as_i32().cast_unsigned(),
    }
}
