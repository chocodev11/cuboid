//! Wavefront OBJ, MTL and texture export (spec: interop 16, modern profile of 31.9).
//!
//! The file holds all positions, normals and texture coordinates first, then one `g` group per
//! part with its `usemtl` and `f` lines. Indices are 1-based and absolute. Normals and texture
//! coordinates that are equal in text share one record. Voxel vertices carry their colour as the
//! extra `r g b` of the `v` line. Line endings are LF, numbers are the shortest text that reads
//! back to the same `f32` and never use an exponent, and the output is a pure function of the
//! document and the options.
//!
//! The materials go to `<base>.mtl` (`Kd`, `d`/`Tr`, `map_Kd` and `map_d` with a path relative
//! to the OBJ, `illum 2`) and each texture to `<base>_<texture>.png`. The frame is described in
//! the crate documentation.

use std::collections::HashMap;
use std::fmt::Write;

use cuboid_model::Document;

use crate::MeshError;
use crate::files::{ExportFile, MeshExport, png};
use crate::gather::{AlphaMode, MeshOptions, Part, Scene, gather};
use crate::names::{self, Unique};

/// Exports the document as `<stem>.obj`, `<base>.mtl` and the texture PNGs, where `base` is the
/// stem made safe for file names.
pub fn export_obj(doc: &Document, stem: &str, opts: &MeshOptions) -> Result<MeshExport, MeshError> {
    let scene = gather(doc, opts)?;
    let base = names::token(stem, "model");

    let mut unique = Unique::default();
    let tex_files: Vec<String> = scene
        .textures
        .iter()
        .map(|t| format!("{}.png", unique.make(format!("{base}_{}", t.name))))
        .collect();

    let mut files = vec![
        ExportFile {
            name: format!("{stem}.obj"),
            bytes: write_obj(&scene, &base).into_bytes(),
        },
        ExportFile {
            name: format!("{base}.mtl"),
            bytes: write_mtl(&scene, &tex_files).into_bytes(),
        },
    ];
    for (tex, name) in scene.textures.iter().zip(tex_files) {
        files.push(ExportFile {
            name,
            bytes: png(tex)?,
        });
    }
    Ok(MeshExport {
        files,
        notices: scene.notices,
    })
}

/// The shortest text of a number that reads back to the same `f32`. A cleaned value is never
/// `-0`.
fn num(out: &mut String, v: f32) {
    let _ = write!(out, "{v}");
}

fn nums<const N: usize>(out: &mut String, tag: &str, values: [f32; N]) {
    out.push_str(tag);
    for v in values {
        out.push(' ');
        num(out, v);
    }
    out.push('\n');
}

/// A header line: control characters become spaces so that the text stays on one line.
fn comment(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// The `vn` and `vt` record of each vertex of a part, for the face lines.
struct Indices {
    position: u32,
    normal: Vec<u32>,
    uv: Vec<u32>,
}

fn write_obj(scene: &Scene, base: &str) -> String {
    let vertices: usize = scene.parts.iter().map(|p| p.positions.len()).sum();
    let mut out = String::with_capacity(vertices * 48 + 256);
    let _ = writeln!(out, "# Export of Cuboid model: {}", comment(&scene.name));
    for line in scene.description.lines() {
        let _ = writeln!(out, "# {}", comment(line));
    }
    out.push_str("# from the Cuboid OBJ exporter\n");
    let _ = writeln!(out, "mtllib {base}.mtl");

    let mut normals: HashMap<[u32; 3], u32> = HashMap::new();
    let mut uvs: HashMap<[u32; 2], u32> = HashMap::new();
    let mut normal_count = 0_u32;
    let mut uv_count = 0_u32;
    let mut next_position = 1_u32;
    let mut tables = Vec::with_capacity(scene.parts.len());
    // Positions, normals and texture coordinates of all parts come first.
    for part in &scene.parts {
        let table = write_vertices(
            &mut out,
            part,
            next_position,
            (&mut normals, &mut normal_count),
            (&mut uvs, &mut uv_count),
        );
        next_position += table.len;
        tables.push(table.indices);
    }

    for (part, indices) in scene.parts.iter().zip(&tables) {
        let _ = writeln!(out, "g {}", part.name);
        for prim in &part.prims {
            let name = scene
                .materials
                .get(prim.material)
                .map_or("default", |m| m.name.as_str());
            let _ = writeln!(out, "usemtl {name}");
            for tri in prim.indices.chunks_exact(3) {
                out.push('f');
                for &i in tri {
                    let i = i as usize;
                    let position = indices.position + u32::try_from(i).unwrap_or(0);
                    let normal = indices.normal.get(i).copied().unwrap_or(0);
                    match indices.uv.get(i) {
                        Some(uv) => {
                            let _ = write!(out, " {position}/{uv}/{normal}");
                        }
                        None => {
                            let _ = write!(out, " {position}//{normal}");
                        }
                    }
                }
                out.push('\n');
            }
        }
    }
    out
}

struct Written {
    len: u32,
    indices: Indices,
}

/// Writes the `v`, `vn` and `vt` records of a part and returns the indices its faces use.
fn write_vertices(
    out: &mut String,
    part: &Part,
    first_position: u32,
    (normals, normal_count): (&mut HashMap<[u32; 3], u32>, &mut u32),
    (uvs, uv_count): (&mut HashMap<[u32; 2], u32>, &mut u32),
) -> Written {
    let has_color = !part.colors.is_empty();
    let has_uv = !part.uvs.is_empty();
    let mut normal_idx = Vec::with_capacity(part.normals.len());
    let mut uv_idx = Vec::with_capacity(if has_uv { part.uvs.len() } else { 0 });

    for (i, &p) in part.positions.iter().enumerate() {
        match part.colors.get(i) {
            Some(c) if has_color => nums(out, "v", [p[0], p[1], p[2], c[0], c[1], c[2]]),
            _ => nums(out, "v", p),
        }
    }
    for &n in &part.normals {
        let key = n.map(f32::to_bits);
        let index = *normals.entry(key).or_insert_with(|| {
            *normal_count += 1;
            nums(out, "vn", n);
            *normal_count
        });
        normal_idx.push(index);
    }
    if has_uv {
        for &uv in &part.uvs {
            let key = uv.map(f32::to_bits);
            let index = *uvs.entry(key).or_insert_with(|| {
                *uv_count += 1;
                nums(out, "vt", uv);
                *uv_count
            });
            uv_idx.push(index);
        }
    }
    Written {
        len: u32::try_from(part.positions.len()).unwrap_or(0),
        indices: Indices {
            position: first_position,
            normal: normal_idx,
            uv: uv_idx,
        },
    }
}

fn write_mtl(scene: &Scene, tex_files: &[String]) -> String {
    let mut out = String::new();
    out.push_str("# Materials of the Cuboid OBJ exporter\n");
    for material in &scene.materials {
        let [r, g, b, a] = material.color;
        let _ = writeln!(out, "newmtl {}", material.name);
        nums(&mut out, "Kd", [r, g, b]);
        if a < 1.0 {
            nums(&mut out, "Tr", [1.0 - a]);
            nums(&mut out, "d", [a]);
        }
        if let Some(file) = material.texture.and_then(|i| tex_files.get(i)) {
            let _ = writeln!(out, "map_Kd {file}");
            if material.alpha != AlphaMode::Opaque {
                let _ = writeln!(out, "map_d {file}");
            }
        }
        out.push_str("illum 2\n\n");
    }
    out
}
