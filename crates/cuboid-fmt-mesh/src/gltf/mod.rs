//! glTF 2.0 and GLB export (spec: interop 31.9).
//!
//! One node and one mesh per part, with the node named after the object of the model. A mesh
//! has one primitive per material. Every accessor is tightly packed little-endian data with
//! the `min` and `max` of the positions; indices are 16 bit when the part has fewer than 65535
//! vertices and 32 bit otherwise. One material per texture and colour, with a nearest-neighbour
//! sampler that clamps to the edge and PNG images that are never resized.
//!
//! - [`GltfFormat::Glb`] writes one binary file with the buffer and the PNG textures embedded.
//! - [`GltfFormat::Separate`] writes `<stem>.gltf` (indented JSON), `<base>.bin` and
//!   `<base>_<texture>.png`.
//!
//! Colours (material factors and vertex colours) are stored as displayed in the editor, which
//! is sRGB; glTF wants them linear, so they are converted. The frame is described in the crate
//! documentation; V is flipped because glTF measures it from the top.

use std::collections::HashMap;

use cuboid_model::Document;
use serde::Serialize;

use crate::MeshError;
use crate::files::{ExportFile, MeshExport, png};
use crate::gather::{AlphaMode, MeshOptions, Scene, gather};
use crate::names::{self, Unique, uri};

/// How the glTF is laid out in files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GltfFormat {
    /// One `.glb` file with the textures embedded.
    Glb,
    /// A `.gltf` text file with a `.bin` buffer and PNG textures beside it.
    Separate,
}

const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const NEAREST: u32 = 9728;
const CLAMP_TO_EDGE: u32 = 33071;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Root {
    asset: Asset,
    scene: usize,
    scenes: Vec<SceneJson>,
    nodes: Vec<Node>,
    meshes: Vec<MeshJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    materials: Vec<MaterialJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    textures: Vec<TextureJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    images: Vec<ImageJson>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    samplers: Vec<SamplerJson>,
    buffers: Vec<BufferJson>,
    buffer_views: Vec<BufferView>,
    accessors: Vec<Accessor>,
}

#[derive(Serialize)]
struct Asset {
    version: &'static str,
    generator: &'static str,
}

#[derive(Serialize)]
struct SceneJson {
    name: String,
    nodes: Vec<usize>,
}

#[derive(Serialize)]
struct Node {
    name: String,
    mesh: usize,
}

#[derive(Serialize)]
struct MeshJson {
    name: String,
    primitives: Vec<Primitive>,
}

#[derive(Serialize)]
struct Primitive {
    attributes: Attributes,
    indices: usize,
    material: usize,
}

#[derive(Serialize)]
struct Attributes {
    #[serde(rename = "POSITION")]
    position: usize,
    #[serde(rename = "NORMAL")]
    normal: usize,
    #[serde(rename = "TEXCOORD_0", skip_serializing_if = "Option::is_none")]
    texcoord: Option<usize>,
    #[serde(rename = "COLOR_0", skip_serializing_if = "Option::is_none")]
    color: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MaterialJson {
    name: String,
    pbr_metallic_roughness: Pbr,
    #[serde(skip_serializing_if = "Option::is_none")]
    alpha_mode: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Pbr {
    base_color_factor: [f32; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    base_color_texture: Option<TextureRef>,
    metallic_factor: f32,
    roughness_factor: f32,
}

#[derive(Serialize)]
struct TextureRef {
    index: usize,
}

#[derive(Serialize)]
struct TextureJson {
    name: String,
    sampler: usize,
    source: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImageJson {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    buffer_view: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mime_type: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SamplerJson {
    mag_filter: u32,
    min_filter: u32,
    wrap_s: u32,
    wrap_t: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BufferJson {
    byte_length: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    uri: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BufferView {
    buffer: usize,
    byte_offset: usize,
    byte_length: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Accessor {
    buffer_view: usize,
    component_type: u32,
    count: usize,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<Vec<f32>>,
}

/// The binary buffer with its views and accessors.
#[derive(Default)]
struct Bin {
    data: Vec<u8>,
    views: Vec<BufferView>,
    accessors: Vec<Accessor>,
}

impl Bin {
    /// Appends bytes at a 4-byte boundary as a buffer view.
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        self.data.resize(self.data.len().next_multiple_of(4), 0);
        self.views.push(BufferView {
            buffer: 0,
            byte_offset: self.data.len(),
            byte_length: bytes.len(),
            target,
        });
        self.data.extend_from_slice(bytes);
        self.views.len() - 1
    }

    fn accessor(
        &mut self,
        bytes: &[u8],
        target: u32,
        component_type: u32,
        count: usize,
        kind: &'static str,
        bounds: Option<(Vec<f32>, Vec<f32>)>,
    ) -> usize {
        let buffer_view = self.view(bytes, Some(target));
        let (min, max) = bounds.unzip();
        self.accessors.push(Accessor {
            buffer_view,
            component_type,
            count,
            kind,
            min,
            max,
        });
        self.accessors.len() - 1
    }

    /// An accessor of `f32` vectors with `N` components.
    fn floats<const N: usize>(
        &mut self,
        values: &[[f32; N]],
        kind: &'static str,
        bounds: bool,
    ) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * N * 4);
        let mut lo = [f32::INFINITY; N];
        let mut hi = [f32::NEG_INFINITY; N];
        for v in values {
            for ((x, lo), hi) in v.iter().zip(&mut lo).zip(&mut hi) {
                bytes.extend_from_slice(&x.to_le_bytes());
                *lo = lo.min(*x);
                *hi = hi.max(*x);
            }
        }
        let bounds = bounds.then(|| (lo.to_vec(), hi.to_vec()));
        self.accessor(&bytes, ARRAY_BUFFER, FLOAT, values.len(), kind, bounds)
    }

    fn indices(&mut self, indices: &[u32], short: bool) -> usize {
        let mut bytes = Vec::with_capacity(indices.len() * if short { 2 } else { 4 });
        for &i in indices {
            match u16::try_from(i) {
                Ok(i) if short => bytes.extend_from_slice(&i.to_le_bytes()),
                _ => bytes.extend_from_slice(&i.to_le_bytes()),
            }
        }
        let component = if short { UNSIGNED_SHORT } else { UNSIGNED_INT };
        self.accessor(
            &bytes,
            ELEMENT_ARRAY_BUFFER,
            component,
            indices.len(),
            "SCALAR",
            None,
        )
    }
}

/// sRGB to linear for one channel, remembered per value: a model has few distinct colours.
struct Linear(HashMap<u32, f32>);

impl Linear {
    fn get(&mut self, c: f32) -> f32 {
        *self.0.entry(c.to_bits()).or_insert_with(|| {
            let c = c.clamp(0.0, 1.0);
            let v = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
            crate::gather::clean(v)
        })
    }

    fn rgba(&mut self, [r, g, b, a]: [f32; 4]) -> [f32; 4] {
        [self.get(r), self.get(g), self.get(b), a]
    }
}

/// Exports the document as glTF 2.0 in the layout given. `stem` names the files.
pub fn export_gltf(
    doc: &Document,
    stem: &str,
    opts: &MeshOptions,
    format: GltfFormat,
) -> Result<MeshExport, MeshError> {
    let scene = gather(doc, opts)?;
    let base = names::token(stem, "model");
    let embed = format == GltfFormat::Glb;

    let mut bin = Bin::default();
    let mut linear = Linear(HashMap::new());
    let mut nodes = Vec::with_capacity(scene.parts.len());
    let mut meshes = Vec::with_capacity(scene.parts.len());
    for (index, part) in scene.parts.iter().enumerate() {
        let position = bin.floats(&part.positions, "VEC3", true);
        let normal = bin.floats(&part.normals, "VEC3", false);
        let texcoord = (!part.uvs.is_empty()).then(|| {
            let flipped: Vec<[f32; 2]> = part
                .uvs
                .iter()
                .map(|&[u, v]| [u, crate::gather::clean(1.0 - v)])
                .collect();
            bin.floats(&flipped, "VEC2", false)
        });
        let color = (!part.colors.is_empty()).then(|| {
            let colors: Vec<[f32; 4]> = part.colors.iter().map(|&c| linear.rgba(c)).collect();
            bin.floats(&colors, "VEC4", false)
        });
        let short = part.positions.len() < usize::from(u16::MAX);
        let primitives = part
            .prims
            .iter()
            .map(|prim| Primitive {
                attributes: Attributes {
                    position,
                    normal,
                    texcoord,
                    color,
                },
                indices: bin.indices(&prim.indices, short),
                material: prim.material,
            })
            .collect();
        nodes.push(Node {
            name: part.name.clone(),
            mesh: index,
        });
        meshes.push(MeshJson {
            name: part.name.clone(),
            primitives,
        });
    }

    let mut unique = Unique::default();
    let mut images = Vec::with_capacity(scene.textures.len());
    let mut sidecars = Vec::new();
    for tex in &scene.textures {
        let bytes = png(tex)?;
        if embed {
            let view = bin.view(&bytes, None);
            images.push(ImageJson {
                name: tex.name.clone(),
                uri: None,
                buffer_view: Some(view),
                mime_type: Some("image/png"),
            });
        } else {
            let file = format!("{}.png", unique.make(format!("{base}_{}", tex.name)));
            images.push(ImageJson {
                name: tex.name.clone(),
                uri: Some(uri(&file)),
                buffer_view: None,
                mime_type: None,
            });
            sidecars.push(ExportFile { name: file, bytes });
        }
    }

    let byte_length = bin.data.len().next_multiple_of(4);
    bin.data.resize(byte_length, 0);
    if u32::try_from(byte_length).is_err() {
        return Err(MeshError::TooLarge);
    }

    let bin_name = format!("{base}.bin");
    let root = Root {
        asset: Asset {
            version: "2.0",
            generator: "Cuboid",
        },
        scene: 0,
        scenes: vec![SceneJson {
            name: scene.name.clone(),
            nodes: (0..nodes.len()).collect(),
        }],
        nodes,
        meshes,
        materials: materials(&scene, &mut linear),
        textures: (0..scene.textures.len())
            .map(|i| TextureJson {
                name: scene
                    .textures
                    .get(i)
                    .map(|t| t.name.clone())
                    .unwrap_or_default(),
                sampler: 0,
                source: i,
            })
            .collect(),
        images,
        samplers: if scene.textures.is_empty() {
            Vec::new()
        } else {
            vec![SamplerJson {
                mag_filter: NEAREST,
                min_filter: NEAREST,
                wrap_s: CLAMP_TO_EDGE,
                wrap_t: CLAMP_TO_EDGE,
            }]
        },
        buffers: vec![BufferJson {
            byte_length,
            uri: (!embed).then(|| uri(&bin_name)),
        }],
        buffer_views: bin.views,
        accessors: bin.accessors,
    };

    let mut files = Vec::new();
    if embed {
        let json = serde_json::to_vec(&root).map_err(|_| MeshError::TooLarge)?;
        files.push(ExportFile {
            name: format!("{stem}.glb"),
            bytes: glb(json, &bin.data)?,
        });
    } else {
        let mut json = serde_json::to_vec_pretty(&root).map_err(|_| MeshError::TooLarge)?;
        json.push(b'\n');
        files.push(ExportFile {
            name: format!("{stem}.gltf"),
            bytes: json,
        });
        files.push(ExportFile {
            name: bin_name,
            bytes: bin.data,
        });
        files.extend(sidecars);
    }
    Ok(MeshExport {
        files,
        notices: scene.notices,
    })
}

fn materials(scene: &Scene, linear: &mut Linear) -> Vec<MaterialJson> {
    scene
        .materials
        .iter()
        .map(|m| MaterialJson {
            name: m.name.clone(),
            pbr_metallic_roughness: Pbr {
                base_color_factor: linear.rgba(m.color),
                base_color_texture: m.texture.map(|index| TextureRef { index }),
                metallic_factor: 0.0,
                roughness_factor: 1.0,
            },
            alpha_mode: match m.alpha {
                AlphaMode::Opaque => None,
                AlphaMode::Mask => Some("MASK"),
                AlphaMode::Blend => Some("BLEND"),
            },
        })
        .collect()
}

/// The GLB container: a header, the JSON chunk padded with spaces and the binary chunk padded
/// with zeros.
fn glb(mut json: Vec<u8>, bin: &[u8]) -> Result<Vec<u8>, MeshError> {
    const JSON: u32 = 0x4E4F_534A;
    const BIN: u32 = 0x004E_4942;
    json.resize(json.len().next_multiple_of(4), b' ');
    let json_len = u32::try_from(json.len()).map_err(|_| MeshError::TooLarge)?;
    let bin_len = u32::try_from(bin.len()).map_err(|_| MeshError::TooLarge)?;
    let total = 12_u32
        .checked_add(8)
        .and_then(|n| n.checked_add(json_len))
        .and_then(|n| n.checked_add(8))
        .and_then(|n| n.checked_add(bin_len))
        .ok_or(MeshError::TooLarge)?;
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(&0x4654_6C67_u32.to_le_bytes());
    out.extend_from_slice(&2_u32.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&json_len.to_le_bytes());
    out.extend_from_slice(&JSON.to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&bin_len.to_le_bytes());
    out.extend_from_slice(&BIN.to_le_bytes());
    out.extend_from_slice(bin);
    Ok(out)
}
