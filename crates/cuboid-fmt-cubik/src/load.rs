//! Records to the sections of the model (native 6, 8.1).
//!
//! The mapping builds a [`LoadedDoc`]: the sections in the record shape of a snapshot, with fresh
//! object ids, the permissions of each mesh kind, `fp` null turned into an empty path and the
//! voxel records either as a chunk table (canonical files) or in file order for the full
//! writer. The read-side normalisations (quote replacement, rotation quantisation, colour
//! unpacking, UV clamp) are the apply path's: it applies the record normal form to what is
//! built here.
//!
//! spec: native 6.2, 6.3, 8.1, 4.7, model_and_tools 3.1

use std::sync::Arc;

use cuboid_model::apply::{DEFAULT_VOXEL_BUDGET, VoxelReplay, voxel_world_bytes};
use cuboid_model::image::{Image, StoredPixels, check_size, format};
use cuboid_model::voxel::{CHUNK_EDGE, Chunk, ChunkSlot, I3, VoxelSnap};
use cuboid_model::{
    Blueprint, CameraState, Categories, Element, ElementFlags, ElementType, ElementsSnap, Face,
    FaceFlags, FaceName, Label, ListSnap, LoadedDoc, Mesh, MeshBounds, MeshGeometry, MeshKind,
    MeshesSnap, ObjectBase, Perms, Snapshot, Submesh, Texture, TextureEntry, TextureList,
    TextureSource, UvRect, ViewProps, normal_form,
};
use cuboid_num::pack;
use glam::{Quat, Vec3};
use rayon::prelude::*;

use crate::decode::{self, Decoded};
use crate::records::{
    CameraRecord, ElementRecord, FaceRecord, MeshRecord, RawTexture, Root, TextureRecord,
    ViewRecord, VoxelData, WorldRecord,
};
use crate::retain::{Kept, KeptElement, KeptMesh, KeptTexture, KeptVoxels};
use crate::{CubikError, Limits, Loaded};

/// Adds a warning of a per-record kind; at most twice the warnings of the decoder are kept.
fn warn(warnings: &mut Vec<String>, message: impl FnOnce() -> String) {
    match warnings.len().cmp(&(2 * decode::MAX_WARNINGS)) {
        std::cmp::Ordering::Less => warnings.push(message()),
        std::cmp::Ordering::Equal => warnings.push("more warnings are left out".to_owned()),
        std::cmp::Ordering::Greater => {}
    }
}

/// The default for [`Limits::voxel_budget`].
pub const fn default_voxel_budget() -> u64 {
    DEFAULT_VOXEL_BUDGET
}

/// Reads a decompressed payload into the sections of the model. The payload is kept for the
/// byte-identical re-save when the limits allow it.
///
/// spec: native 8.1, 8.3
pub fn load_payload(payload: Vec<u8>, limits: &Limits) -> Result<Loaded, CubikError> {
    let Decoded {
        root,
        spans,
        mut warnings,
    } = decode::decode(&payload, limits)?;
    let retain = limits.retain_cap > 0 && payload.len() <= limits.retain_cap;
    let (doc, mut kept) = sections(root, limits, &mut warnings)?;
    let doc = if retain {
        kept.payload = payload;
        kept.spans = spans;
        LoadedDoc {
            retained: Some(Arc::new(kept.into_retained())),
            ..doc
        }
    } else {
        doc
    };
    Ok(Loaded { doc, warnings })
}

/// Builds the sections of a root; the retention data comes back without payload and spans.
pub(crate) fn sections(
    root: Root,
    limits: &Limits,
    warnings: &mut Vec<String>,
) -> Result<(LoadedDoc, Kept), CubikError> {
    let Root {
        v,
        mp,
        vp,
        bp,
        el,
        tx,
        vx,
        me,
        md,
    } = root;
    let mut cats = Categories::VOXELS;
    let mut kept = Kept {
        v,
        ..Kept::default()
    };
    let mut texels = 0u64;
    let restricted = mp.as_ref().is_none_or(|p| p.general.restrict_rotations);
    let mut snap = Snapshot {
        categories: Categories::empty(),
        props: None,
        view: None,
        elements: None,
        textures: None,
        voxels: Some(None),
        meshes: None,
        display: None,
    };

    if let Some(props) = mp {
        cats |= Categories::MODEL_PROPERTIES;
        // A section that a save would write differently is not copied from the file.
        kept.props = normal_form::props(&props).is_none().then(|| props.clone());
        snap.props = Some(Arc::new(props));
    }

    if let Some(rec) = &vp {
        cats |= Categories::VIEW_PROPERTIES;
        let (view, cameras) = view(rec, warnings);
        kept.view = normal_form::saved_view(&view)
            .is_none()
            .then(|| view.clone());
        kept.cameras = cameras;
        snap.view = Some(Arc::new(view));
    }

    if bp.is_some() || el.is_some() {
        cats |= Categories::ELEMENTS;
        let blueprint = bp.as_ref().map_or_else(Blueprint::default, blueprint);
        kept.blueprint = bp.as_ref().map(|_| {
            normal_form::blueprint(&blueprint, restricted, Vec3::ZERO)
                .unwrap_or_else(|| blueprint.clone())
        });
        let records = el.unwrap_or_default();
        let selected: Vec<bool> = records.iter().map(|r| r.is).collect();
        let items = records
            .iter()
            .map(|r| element(r, warnings))
            .collect::<Vec<_>>();
        let list = ListSnap::fresh(items);
        for ((&id, item), &is) in list.ids.iter().zip(&list.items).zip(&selected) {
            let value =
                normal_form::element(item, restricted).map_or_else(|| Arc::clone(item), Arc::new);
            kept.element_index.insert(id, kept.elements.len());
            kept.elements.push(KeptElement {
                id,
                value,
                selected: is,
            });
        }
        snap.elements = Some(ElementsSnap {
            blueprint,
            list,
            selected,
        });
    }

    if let Some(records) = tx {
        cats |= Categories::TEXTURES;
        let mut entries = Vec::with_capacity(records.len());
        for rec in &records {
            if rec.lb.is_empty() {
                return Err(CubikError::Invalid(
                    "Invalid Label: a texture record has an empty label".to_owned(),
                ));
            }
            let entry = texture_entry(rec, limits, &mut texels, warnings)?;
            kept.texture_index
                .insert(entry.label.clone(), kept.textures.len());
            kept.textures.push(KeptTexture {
                label: entry.label.clone(),
                value: entry.texture.clone(),
            });
            entries.push(entry);
        }
        snap.textures = Some(Arc::new(TextureList { entries }));
    }

    let mut replay = None;
    if let Some(data) = vx {
        let (voxels, replayed, held) = voxels(&data, limits, warnings)?;
        snap.voxels = Some(voxels);
        replay = replayed;
        kept.voxels = held;
    }
    if let Some(records) = me {
        cats |= Categories::MESHES;
        let selected: Vec<bool> = records.iter().map(|r| r.is).collect();
        let mut items = Vec::with_capacity(records.len());
        for rec in &records {
            items.push(mesh(rec, limits, &mut texels, warnings)?);
        }
        let list = ListSnap::fresh(items);
        for ((&id, item), &is) in list.ids.iter().zip(&list.items).zip(&selected) {
            let value = normal_form::mesh(item).map_or_else(|| Arc::clone(item), Arc::new);
            kept.mesh_index.insert(id, kept.meshes.len());
            kept.meshes.push(KeptMesh {
                id,
                value,
                selected: is,
            });
        }
        snap.meshes = Some(MeshesSnap { list, selected });
    }

    if let Some(display) = md {
        cats |= Categories::MODEL_DISPLAY;
        kept.display = normal_form::saved_display(&display)
            .is_none()
            .then(|| display.clone());
        snap.display = Some(Arc::new(display));
    }

    snap.categories = cats;
    let doc = LoadedDoc {
        sections: snap,
        voxel_replay: replay,
        overrides: None,
        retained: None,
    };
    Ok((doc, kept))
}

// --- View ---------------------------------------------------------------------------------------

type KeptCameras = [Option<(CameraState, CameraRecord)>; 4];

fn view(rec: &ViewRecord, warnings: &mut Vec<String>) -> (ViewProps, KeptCameras) {
    let mut kept: KeptCameras = [None; 4];
    let mut cameras = [None; 4];
    for (i, (src, dst)) in rec.cameras.iter().zip(&mut cameras).enumerate() {
        let Some(cam) = src else { continue };
        let mut rot = cam.r.rotation;
        if !rot.is_finite() || rot.length_squared() < 1e-12 {
            warnings.push(format!(
                "camera {i} has no usable rotation; read as identity"
            ));
            rot = Quat::IDENTITY;
        }
        let state = CameraState {
            pos: cam.p,
            rot,
            ortho: cam.o,
            ortho_size: cam.os,
            fov_deg: cam.fv,
        };
        *dst = Some(state);
        if let Some(slot) = kept.get_mut(i) {
            *slot = Some((state, *cam));
        }
    }
    let view = ViewProps {
        cameras,
        mouse_step: rec.gm,
        key_step: rec.gk,
        uv_step: rec.gu,
        display_step: rec.gd,
        large_grid: rec.gl,
    };
    (view, kept)
}

// --- Elements -----------------------------------------------------------------------------------

fn label(text: Option<&str>) -> Label {
    text.map_or_else(Label::default, Label::from)
}

fn blueprint(rec: &ElementRecord) -> Blueprint {
    let mut bp = Blueprint::default();
    let base = &mut bp.base;
    base.p = rec.p;
    base.s = rec.s;
    base.e = rec.r;
    base.o = rec.p;
    base.hidden = rec.ih;
    base.locked = rec.il;
    if let Some(text) = &rec.l {
        base.label = Label::from(text.as_str());
    }
    bp
}

fn element(rec: &ElementRecord, warnings: &mut Vec<String>) -> Element {
    let kind = match rec.t {
        0 => ElementType::Cube,
        1 => ElementType::PlaneX,
        2 => ElementType::PlaneY,
        3 => ElementType::PlaneZ,
        other => {
            warn(warnings, || {
                format!("element type {other} is unknown; read as cube")
            });
            ElementType::Cube
        }
    };
    let mut base = ObjectBase::new(label(rec.l.as_deref()), rec.p, rec.s, Perms::ELEMENT);
    base.e = rec.r;
    base.o = rec.o;
    base.hidden = rec.ih;
    base.locked = rec.il;

    let absent = std::array::from_fn(FaceRecord::absent);
    let records = rec.f.as_ref().map_or(&absent, |set| &set.faces);
    let faces: [Face; 6] = std::array::from_fn(|slot| {
        let name = FaceName::ALL.get(slot).copied().unwrap_or(FaceName::Down);
        let fallback = FaceRecord::absent(slot);
        let f = records.get(slot).unwrap_or(&fallback);
        Face {
            name,
            active: f.a,
            tint: f.ti,
            cull: FaceFlags::from_bits_truncate(u8::try_from(f.c & 0x3F).unwrap_or(0)),
            texture: Label::from(f.t.as_deref().unwrap_or("texture")),
            uv: UvRect::new(f.uv.r, f.uv.tl, f.uv.br),
        }
    });

    let mut flags = ElementFlags::empty();
    flags.set(ElementFlags::UV_SNAP, rec.suv);
    flags.set(ElementFlags::UV_SNAP_MIRROR, rec.suvm);
    flags.set(ElementFlags::RESCALE, rec.rs);
    flags.set(ElementFlags::SHADE, rec.sh);
    flags.set(ElementFlags::INVERT_NORMALS, rec.inv);
    Element {
        base,
        kind,
        color: pack::unpack_element(rec.c),
        faces,
        flags,
    }
}

// --- Textures -----------------------------------------------------------------------------------

fn image(raw: &RawTexture, limits: &Limits, texels: &mut u64) -> Result<Arc<Image>, CubikError> {
    let size = |v: i32| {
        u32::try_from(v).map_err(|_| {
            CubikError::Invalid(format!(
                "texture `{}` has the size {} x {}",
                raw.n, raw.w, raw.h
            ))
        })
    };
    let (w, h) = (size(raw.w)?, size(raw.h)?);
    check_size(w, h).map_err(|err| CubikError::Invalid(format!("texture `{}`: {err}", raw.n)))?;
    *texels += u64::from(w) * u64::from(h);
    if *texels > limits.max_texels {
        return Err(CubikError::Invalid(format!(
            "the textures hold more than {} texels",
            limits.max_texels
        )));
    }
    // An unknown format code keeps its bytes (native 4.5.1); a known one needs level 0.
    if let Ok(needed) = format::level_len(raw.f, w, h)
        && raw.r.len() < needed
    {
        return Err(CubikError::Invalid(format!(
            "texture `{}` has {} bytes of pixel data where {needed} are needed",
            raw.n,
            raw.r.len()
        )));
    }
    Ok(Arc::new(Image::from_stored(
        w,
        h,
        StoredPixels {
            format: raw.f,
            raw: Arc::clone(&raw.r),
        },
    )))
}

fn texture(raw: &RawTexture, limits: &Limits, texels: &mut u64) -> Result<Texture, CubikError> {
    Ok(Texture {
        name: Label::from(raw.n.as_str()),
        source: TextureSource::default(),
        image: image(raw, limits, texels)?,
    })
}

fn texture_entry(
    rec: &TextureRecord,
    limits: &Limits,
    texels: &mut u64,
    warnings: &mut Vec<String>,
) -> Result<TextureEntry, CubikError> {
    if rec.lb == "error" {
        warn(warnings, || {
            "the texture label `error` is listed as a normal texture".to_owned()
        });
    }
    let mut tex = texture(&rec.tx, limits, texels)?;
    tex.source = TextureSource {
        // The path setter turns a null into an empty path (native 6.3).
        path: Some(rec.fp.clone().unwrap_or_default()),
        file_name: rec.fname.clone(),
        mc_path: rec.fm.clone(),
        signature: rec.cs.clone(),
        file_size: 0,
    };
    Ok(TextureEntry {
        label: Label::from(rec.lb.as_str()),
        texture: tex,
    })
}

// --- Meshes -------------------------------------------------------------------------------------

fn triangles(list: &[i32], vertices: usize, what: &str) -> Result<Vec<u32>, CubikError> {
    list.iter()
        .map(|&i| {
            u32::try_from(i)
                .ok()
                .filter(|&i| (i as usize) < vertices)
                .ok_or_else(|| {
                    CubikError::Invalid(format!("{what} refers to the vertex {i} of {vertices}"))
                })
        })
        .collect()
}

fn mesh(
    rec: &MeshRecord,
    limits: &Limits,
    texels: &mut u64,
    warnings: &mut Vec<String>,
) -> Result<Mesh, CubikError> {
    let kind = match rec.tp {
        0 => MeshKind::Custom,
        1 => MeshKind::Cube,
        2 => MeshKind::Sphere,
        3 => MeshKind::Cylinder,
        4 => MeshKind::Cone,
        5 => MeshKind::Pyramid,
        6 => MeshKind::Slope,
        7 => MeshKind::CornerIn,
        8 => MeshKind::CornerOut,
        9 => MeshKind::Torus,
        10 => MeshKind::Sprite,
        other => {
            warn(warnings, || {
                format!("mesh kind {other} is unknown; read as cube")
            });
            MeshKind::Cube
        }
    };
    let custom = kind == MeshKind::Custom;
    let perms = if custom {
        Perms::IMPORTED_MESH
    } else {
        Perms::PRIMITIVE
    };
    let mut base = ObjectBase::new(label(rec.l.as_deref()), rec.p, rec.s, perms);
    base.e = rec.r;
    base.o = rec.o;
    base.hidden = rec.ih;
    base.locked = rec.il;

    let mut geometry = MeshGeometry::default();
    let mut submeshes = Vec::new();
    if custom {
        match &rec.me {
            Some(data) => {
                let count = data.v.len();
                geometry.vertices.clone_from(&data.v);
                // A null or empty `n` asks for computed normals; so does a count mismatch.
                geometry.normals = data.n.clone().filter(|n| n.len() == count);
                geometry.uvs = data.u.clone().unwrap_or_default();
                geometry.triangles = triangles(&data.t, count, "a mesh triangle")?;
                for sm in rec.sm.iter().flatten() {
                    submeshes.push(Submesh {
                        triangles: triangles(&sm.t, count, "a sub-mesh triangle")?,
                        color: pack::unpack_submesh(sm.c),
                        texture: sm
                            .tx
                            .as_ref()
                            .map(|raw| texture(raw, limits, texels))
                            .transpose()?,
                    });
                }
            }
            None => warn(warnings, || {
                "a custom mesh record has no geometry".to_owned()
            }),
        }
    }
    let bounds = match &rec.mb {
        Some(mb) => MeshBounds {
            center: mb.c,
            size: mb.s,
        },
        None => MeshBounds::default(),
    };
    Ok(Mesh {
        base,
        kind,
        geometry: Arc::new(geometry),
        texture: rec
            .tx
            .as_ref()
            .map(|raw| texture(raw, limits, texels))
            .transpose()?,
        submeshes: submeshes.into(),
        bounds,
        norm_factor: 1.0,
        import_offset: Vec3::ZERO,
        tint: [1.0; 4],
    })
}

// --- Voxels -------------------------------------------------------------------------------------

type VoxelSections = (Option<VoxelSnap>, Option<VoxelReplay>, Option<KeptVoxels>);

/// The voxel section: the chunk table when the records are in slot and scan order without
/// repeats (what Cubik writes), the records in file order otherwise.
///
/// spec: native 4.7, 6.3 (`vx`)
fn voxels(
    data: &VoxelData,
    limits: &Limits,
    warnings: &mut Vec<String>,
) -> Result<VoxelSections, CubikError> {
    let Some(world) = data.w.first() else {
        return Ok((None, None, None));
    };
    let res = u32::try_from(world.r)
        .ok()
        .filter(|&r| r >= 1)
        .ok_or_else(|| CubikError::Invalid(format!("voxel resolution {} is below 1", world.r)))?;
    let chunks = world.c.iter().filter(|c| !c.v.is_empty()).count();
    check_budget(res, chunks, limits)?;
    if canonical(world) {
        let snap = chunk_table(world, res);
        let kept = KeptVoxels {
            res,
            slots: Arc::clone(&snap.slots),
        };
        return Ok((Some(snap), None, Some(kept)));
    }
    warnings.push(
        "the voxel records are not in slot and scan order; they are replayed in file order"
            .to_owned(),
    );
    Ok((None, Some(replay(world, res)), None))
}

fn check_budget(res: u32, chunks: usize, limits: &Limits) -> Result<(), CubikError> {
    match voxel_world_bytes(res, chunks) {
        Some(bytes) if bytes <= limits.voxel_budget => Ok(()),
        Some(bytes) => Err(CubikError::Invalid(format!(
            "a voxel world of resolution {res} needs {bytes} bytes, over the budget of {}",
            limits.voxel_budget
        ))),
        None => Err(CubikError::Invalid(format!(
            "voxel resolution {res} is out of range"
        ))),
    }
}

fn chunks_per_axis(res: u32) -> usize {
    (3 * res).div_ceil(16) as usize
}

fn slot_index(cx: i32, cy: i32, cz: i32, n: usize) -> usize {
    let at = |c: i32| usize::try_from(c).unwrap_or(0);
    (at(cx) * n + at(cy)) * n + at(cz)
}

/// Chunks in strictly increasing slot order, voxels in strictly increasing scan order
/// (`x`, then `y`, then `z`) and every colour with a non-zero alpha.
fn canonical(world: &WorldRecord) -> bool {
    let n = chunks_per_axis(u32::try_from(world.r).unwrap_or(0));
    let mut last = None;
    for chunk in &world.c {
        let at = slot_index(chunk.cx, chunk.cy, chunk.cz, n);
        if last.is_some_and(|last| at <= last) {
            return false;
        }
        last = Some(at);
        let mut key = None;
        for v in &chunk.v {
            let k = (usize::from(v.x) * 16 + usize::from(v.y)) * 16 + usize::from(v.z);
            if key.is_some_and(|key| k <= key) || v.c.cast_unsigned() >> 24 == 0 {
                return false;
            }
            key = Some(k);
        }
    }
    true
}

fn chunk_table(world: &WorldRecord, res: u32) -> VoxelSnap {
    let n = chunks_per_axis(res);
    let mut slots = vec![ChunkSlot::Unallocated; n * n * n];
    let built: Vec<(usize, Arc<Chunk>)> = world
        .c
        .par_iter()
        .filter(|chunk| !chunk.v.is_empty())
        .map(|chunk| {
            let mut cells = Chunk::default();
            for v in &chunk.v {
                let local =
                    Chunk::local_index(usize::from(v.x), usize::from(v.y), usize::from(v.z));
                cells.set(local, v.c.cast_unsigned());
            }
            (slot_index(chunk.cx, chunk.cy, chunk.cz, n), Arc::new(cells))
        })
        .collect();
    for (at, chunk) in built {
        if let Some(slot) = slots.get_mut(at) {
            *slot = ChunkSlot::Data(chunk);
        }
    }
    VoxelSnap {
        res,
        slots: slots.into(),
    }
}

fn replay(world: &WorldRecord, res: u32) -> VoxelReplay {
    let edge = i32::try_from(CHUNK_EDGE).unwrap_or(16);
    let mut cells = Vec::new();
    for chunk in &world.c {
        let base = I3::new(chunk.cx, chunk.cy, chunk.cz) * edge;
        for v in &chunk.v {
            if v.c != 0 {
                let local = I3::new(i32::from(v.x), i32::from(v.y), i32::from(v.z));
                cells.push((base + local, v.c.cast_unsigned()));
            }
        }
    }
    VoxelReplay { res, cells }
}
