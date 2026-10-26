//! The sections of the model to a payload (native 5, 8.2, 8.3).
//!
//! The saver takes a snapshot of all categories (`cuboid_model::capture`, which applies the
//! snapshot-side normalisations of native 5.3) and maps it to the records of native 4 with the
//! rules of the record encoding: colour packing, folded UV corners, `fp` and `cs` of the
//! texture source, `me` and `sm` only for custom meshes, the blueprint keys. Where the file the
//! document was loaded from is still retained, every section and record whose value is
//! unchanged is copied from it byte for byte (see [`crate::retain`]).
//!
//! spec: native 5.3, 8.2, 8.3, 4.5.3, arch 12.3

use std::sync::Arc;

use cuboid_bson::{DocWriter, Span};
use cuboid_model::image::format;
use cuboid_model::voxel::{CHUNK_EDGE, ChunkSlot};
use cuboid_model::{
    Blueprint, CameraState, Categories, DisplaySlots, Element, ElementFlags, ElementsSnap, Mesh,
    MeshKind, MeshesSnap, ModelProps, Retained, Snapshot, Texture, TextureEntry, TextureList,
    ViewProps, normal_form,
};
use cuboid_num::{pack, quat_to_euler};
use rayon::prelude::*;

use crate::records::{
    BoundsRecord, CameraRecord, ChunkRecord, ElementRecord, FACE_NAMES, FaceRecord, FaceSetRecord,
    FaceUvRecord, MeshDataRecord, MeshRecord, QuatRecord, RawTexture, SubmeshRecord, ViewRecord,
    VoxelData, VoxelRecord, WorldRecord,
};
use crate::retain::{Kept, same_frame, same_mesh, same_slots, same_texture};
use crate::{CubikError, decode, encode};

/// How the texture records are written (integration backlog item 1, native 4.5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TexturePolicy {
    /// Textures that are unchanged since the file was read keep their format and bytes; created
    /// or edited ones are written as RGBA32 (`f` = 4) without a mip chain.
    #[default]
    Retained,
    /// The strict switch `atlas_recut_argb32`: every texture of the list is written as after
    /// the end of a paint stroke in Cubik (ARGB32, no mip chain, `n` empty, `fp` and `cs`
    /// null), for comparisons with Cubik's payload.
    AtlasRecutArgb32,
}

/// Options of a save.
#[derive(Debug, Clone)]
pub struct SaveOptions {
    /// How textures are written.
    pub textures: TexturePolicy,
    /// Copy unchanged sections and records from the retained file.
    pub use_retention: bool,
    /// Write the `cuboid.json` entry (item overrides, project). Off for a Cubik-compatible save.
    pub extension: bool,
    /// The preset to store in the `cuboid.json` entry; `None` writes no preset.
    pub project: Option<crate::ProjectMeta>,
    /// Read the produced payload again and fail if it is malformed (native 8.2 item 6).
    pub verify: bool,
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self {
            textures: TexturePolicy::Retained,
            use_retention: true,
            extension: true,
            project: None,
            verify: cfg!(debug_assertions),
        }
    }
}

/// Writes the payload of a snapshot that holds all categories.
///
/// spec: native 5.2, 8.2
pub fn save_payload(
    snap: &Snapshot,
    retained: Option<&Retained>,
    options: &SaveOptions,
) -> Result<Vec<u8>, CubikError> {
    let missing = |category: Categories, name: &str| {
        CubikError::Invalid(format!("the snapshot has no {name} ({category:?})"))
    };
    let props = snap
        .props
        .as_deref()
        .ok_or_else(|| missing(Categories::MODEL_PROPERTIES, "model properties"))?;
    let view = snap
        .view
        .as_deref()
        .ok_or_else(|| missing(Categories::VIEW_PROPERTIES, "view properties"))?;
    let elements = snap
        .elements
        .as_ref()
        .ok_or_else(|| missing(Categories::ELEMENTS, "elements"))?;
    let textures = snap
        .textures
        .as_deref()
        .ok_or_else(|| missing(Categories::TEXTURES, "textures"))?;
    let voxels = snap
        .voxels
        .as_ref()
        .ok_or_else(|| missing(Categories::VOXELS, "voxel category"))?;
    let meshes = snap
        .meshes
        .as_ref()
        .ok_or_else(|| missing(Categories::MESHES, "meshes"))?;
    let display = snap
        .display
        .as_deref()
        .ok_or_else(|| missing(Categories::MODEL_DISPLAY, "display slots"))?;

    // The writer never writes what a save normalises away, whatever the snapshot holds.
    let props_n = normal_form::props(props);
    let view_n = normal_form::saved_view(view);
    let display_n = normal_form::saved_display(display);

    let mut saver = Saver {
        w: DocWriter::with_capacity(retained.map_or(1 << 16, |r| r.bytes)),
        kept: options
            .use_retention
            .then(|| retained.and_then(Kept::of))
            .flatten(),
        options,
    };
    let root = saver.w.begin_root();
    saver.w.i32(b"v\0", saver.kept.map_or(1, |k| k.v));
    saver.props(props_n.as_ref().unwrap_or(props));
    saver.view(view_n.as_ref().unwrap_or(view));
    saver.blueprint(&elements.blueprint);
    saver.elements(elements);
    saver.textures(textures);
    saver.voxels(voxels.as_ref());
    saver.meshes(meshes);
    saver.display(display_n.as_ref().unwrap_or(display));
    saver.w.end(root);
    let payload = saver
        .w
        .finish()
        .map_err(|err| CubikError::Invalid(err.to_string()))?;

    if options.verify {
        decode::decode(&payload, &crate::Limits::default())?;
    }
    Ok(payload)
}

struct Saver<'a> {
    w: DocWriter,
    kept: Option<&'a Kept>,
    options: &'a SaveOptions,
}

impl Saver<'_> {
    /// Copies a whole section element of the retained payload.
    fn copy(&mut self, span: Span) {
        if let Some(kept) = self.kept {
            self.w.raw(kept.bytes(span));
        }
    }

    /// Copies a retained record as item `index` of an array.
    fn copy_item(&mut self, index: usize, span: Span) {
        let mark = self.w.begin_doc(index);
        if let Some(kept) = self.kept {
            self.w.raw(kept.bytes(span));
        }
        self.w.end(mark);
    }

    fn props(&mut self, props: &ModelProps) {
        let reuse = self
            .kept
            .and_then(|k| (k.props.as_ref() == Some(props)).then_some(k.spans.mp)?);
        match reuse {
            Some(span) => self.copy(span),
            None => encode::props(&mut self.w, b"mp\0", props),
        }
    }

    fn view(&mut self, view: &ViewProps) {
        let reuse = self
            .kept
            .and_then(|k| (k.view.as_ref() == Some(view)).then_some(k.spans.vp)?);
        if let Some(span) = reuse {
            self.copy(span);
            return;
        }
        let kept = self.kept;
        let camera = |i: usize| {
            let state = view.cameras.get(i).copied().flatten()?;
            Some(camera_record(&state, kept, i))
        };
        let record = ViewRecord {
            cameras: std::array::from_fn(camera),
            gm: view.mouse_step,
            gk: view.key_step,
            gu: view.uv_step,
            gd: view.display_step,
            gl: view.large_grid,
        };
        encode::view(&mut self.w, b"vp\0", &record);
    }

    fn blueprint(&mut self, bp: &Blueprint) {
        let reuse = self
            .kept
            .and_then(|k| (k.blueprint.as_ref() == Some(bp)).then_some(k.spans.bp)?);
        match reuse {
            Some(span) => self.copy(span),
            None => encode::element(&mut self.w, b"bp\0", &blueprint_record(bp)),
        }
    }

    fn elements(&mut self, snap: &ElementsSnap) {
        let kept = self.kept;
        // Per item: the position among the retained records when the record is unchanged.
        let reuse: Vec<Option<(usize, Span)>> = snap
            .list
            .ids
            .iter()
            .zip(&snap.list.items)
            .zip(&snap.selected)
            .map(|((id, item), &selected)| {
                let k = kept?;
                let &at = k.element_index.get(id)?;
                let old = k.elements.get(at)?;
                let same = old.selected == selected
                    && (Arc::ptr_eq(&old.value, item) || *old.value == **item);
                same.then_some(())?;
                Some((at, *k.spans.el_items.get(at)?))
            })
            .collect();
        let whole = kept.and_then(|k| {
            let in_place = reuse
                .iter()
                .enumerate()
                .all(|(i, r)| r.is_some_and(|(at, _)| at == i));
            (in_place && reuse.len() == k.elements.len())
                .then_some(k.spans.el)
                .flatten()
        });
        if let Some(span) = whole {
            self.copy(span);
            return;
        }
        let arr = self.w.begin_array(b"el\0");
        for (i, ((item, &selected), reused)) in snap
            .list
            .items
            .iter()
            .zip(&snap.selected)
            .zip(&reuse)
            .enumerate()
        {
            match reused {
                Some((_, span)) => self.copy_item(i, *span),
                None => encode::element(&mut self.w, i, &element_record(item, selected)),
            }
        }
        self.w.end(arr);
    }

    fn textures(&mut self, list: &TextureList) {
        let recut = self.options.textures == TexturePolicy::AtlasRecutArgb32;
        let kept = self.kept.filter(|_| !recut);
        // Cubik cannot play a strip: the record of an animated texture holds its first frame, and
        // the strip goes into the `cuboid.json` entry.
        let animated: Vec<bool> = list.entries.iter().map(|e| e.animation.is_some()).collect();
        let still;
        let list = if animated.contains(&true) {
            still = TextureList {
                entries: list
                    .entries
                    .iter()
                    .map(|entry| TextureEntry {
                        label: entry.label.clone(),
                        texture: Texture {
                            image: entry.frame(0),
                            ..entry.texture.clone()
                        },
                        animation: None,
                    })
                    .collect(),
            };
            &still
        } else {
            list
        };
        let reuse: Vec<Option<(usize, Span)>> = list
            .entries
            .iter()
            .zip(&animated)
            .map(|(entry, &strip)| {
                let k = kept?;
                let &at = k.texture_index.get(&entry.label)?;
                let old = k.textures.get(at)?;
                // The first frame is a new image each time, so it is the pixels that must match.
                let same = if strip {
                    same_frame(&entry.texture, &old.value)
                } else {
                    same_texture(&entry.texture, &old.value)
                };
                same.then_some(())?;
                Some((at, *k.spans.tx_items.get(at)?))
            })
            .collect();
        let whole = kept.and_then(|k| {
            let in_place = reuse
                .iter()
                .enumerate()
                .all(|(i, r)| r.is_some_and(|(at, _)| at == i));
            (in_place && reuse.len() == k.textures.len())
                .then_some(k.spans.tx)
                .flatten()
        });
        if let Some(span) = whole {
            self.copy(span);
            return;
        }
        let arr = self.w.begin_array(b"tx\0");
        for (i, (entry, reused)) in list.entries.iter().zip(&reuse).enumerate() {
            match reused {
                Some((_, span)) => self.copy_item(i, *span),
                None => self.texture(i, entry, recut),
            }
        }
        self.w.end(arr);
    }

    /// One texture record, written straight from the image so that a large texture is not
    /// copied.
    fn texture(&mut self, index: usize, entry: &TextureEntry, recut: bool) {
        let tex = &entry.texture;
        let src = &tex.source;
        let w = &mut self.w;
        let mark = w.begin_doc(index);
        w.str(b"lb\0", &entry.label);
        if recut {
            w.null(b"fp\0");
        } else {
            match &src.path {
                Some(path) => w.str(b"fp\0", path),
                None => w.null(b"fp\0"),
            }
        }
        match &src.file_name {
            Some(name) => w.str(b"fn\0", name),
            None => w.null(b"fn\0"),
        }
        w.str(b"fm\0", &src.mc_path);
        if recut {
            let argb = format::encode(format::ARGB32, tex.image.w, tex.image.h, tex.image.rgba())
                .unwrap_or_default();
            encode::raw_texture_parts(
                w,
                b"tx\0",
                "",
                to_i32(tex.image.w),
                to_i32(tex.image.h),
                format::ARGB32,
                &argb,
            );
            w.null(b"cs\0");
        } else {
            let (code, bytes) = tex.image.stored_bytes();
            encode::raw_texture_parts(
                w,
                b"tx\0",
                &tex.name,
                to_i32(tex.image.w),
                to_i32(tex.image.h),
                code,
                bytes,
            );
            match &src.signature {
                Some(sig) => w.str(b"cs\0", sig),
                None => w.null(b"cs\0"),
            }
        }
        w.end(mark);
    }

    fn voxels(&mut self, voxels: Option<&cuboid_model::voxel::VoxelSnap>) {
        let Some(snap) = voxels else {
            encode::voxels(&mut self.w, None);
            return;
        };
        let reuse = self.kept.and_then(|k| {
            let held = k.voxels.as_ref()?;
            (held.res == snap.res && same_slots(&held.slots, &snap.slots)).then_some(k.spans.vx)?
        });
        if let Some(span) = reuse {
            self.copy(span);
            return;
        }
        encode::voxels(&mut self.w, Some(&voxel_data(snap)));
    }

    fn meshes(&mut self, snap: &MeshesSnap) {
        let kept = self.kept;
        let reuse: Vec<Option<(usize, Span)>> = snap
            .list
            .ids
            .iter()
            .zip(&snap.list.items)
            .zip(&snap.selected)
            .map(|((id, item), &selected)| {
                let k = kept?;
                let &at = k.mesh_index.get(id)?;
                let old = k.meshes.get(at)?;
                let same = old.selected == selected
                    && (Arc::ptr_eq(&old.value, item) || same_mesh(&old.value, item));
                same.then_some(())?;
                Some((at, *k.spans.me_items.get(at)?))
            })
            .collect();
        let whole = kept.and_then(|k| {
            let in_place = reuse
                .iter()
                .enumerate()
                .all(|(i, r)| r.is_some_and(|(at, _)| at == i));
            (in_place && reuse.len() == k.meshes.len())
                .then_some(k.spans.me)
                .flatten()
        });
        if let Some(span) = whole {
            self.copy(span);
            return;
        }
        let arr = self.w.begin_array(b"me\0");
        for (i, ((item, &selected), reused)) in snap
            .list
            .items
            .iter()
            .zip(&snap.selected)
            .zip(&reuse)
            .enumerate()
        {
            match reused {
                Some((_, span)) => self.copy_item(i, *span),
                None => encode::mesh(&mut self.w, i, &mesh_record(item, selected)),
            }
        }
        self.w.end(arr);
    }

    fn display(&mut self, display: &DisplaySlots) {
        let reuse = self
            .kept
            .and_then(|k| (k.display.as_ref() == Some(display)).then_some(k.spans.md)?);
        match reuse {
            Some(span) => self.copy(span),
            None => encode::display(&mut self.w, b"md\0", display),
        }
    }
}

// --- Records of the objects ---------------------------------------------------------------------

fn camera_record(state: &CameraState, kept: Option<&Kept>, index: usize) -> CameraRecord {
    let held = kept
        .and_then(|k| k.cameras.get(index))
        .and_then(Option::as_ref);
    if let Some((old, record)) = held
        && old == state
    {
        return *record;
    }
    let q = state.rot;
    CameraRecord {
        p: state.pos,
        r: QuatRecord {
            x: q.x,
            y: q.y,
            z: q.z,
            w: q.w,
            euler: Some(quat_to_euler(q)),
            rotation: q,
        },
        o: state.ortho,
        os: state.ortho_size,
        fv: state.fov_deg,
    }
}

/// The blueprint record: its position doubles as the world position `o`, the colour is 0, the
/// faces null and the flags and the type are 0 (native 4.4).
fn blueprint_record(bp: &Blueprint) -> ElementRecord {
    let base = &bp.base;
    ElementRecord {
        p: base.p,
        s: base.s,
        r: base.e,
        o: base.p,
        c: 0,
        is: false,
        ih: base.hidden,
        il: base.locked,
        l: Some(base.label.to_string()),
        f: None,
        rs: false,
        suv: false,
        suvm: false,
        sh: false,
        inv: false,
        t: 0,
    }
}

fn element_record(el: &Element, selected: bool) -> ElementRecord {
    let base = &el.base;
    let faces = std::array::from_fn(|slot| {
        let fallback = FaceRecord::absent(slot);
        let Some(face) = el.faces.get(slot) else {
            return fallback;
        };
        FaceRecord {
            n: FACE_NAMES
                .get(face.name as usize)
                .map(|name| (*name).to_owned()),
            a: face.active,
            ti: face.tint,
            c: i32::from(face.cull.bits()),
            t: Some(face.texture.to_string()),
            uv: FaceUvRecord {
                r: face.uv.rot(),
                tl: face.uv.tl(),
                br: face.uv.br(),
            },
        }
    });
    ElementRecord {
        p: base.p,
        s: base.s,
        r: base.e,
        o: base.o,
        c: pack::element(el.color),
        is: selected,
        ih: base.hidden,
        il: base.locked,
        l: Some(base.label.to_string()),
        f: Some(FaceSetRecord { faces }),
        rs: el.flags.contains(ElementFlags::RESCALE),
        suv: el.flags.contains(ElementFlags::UV_SNAP),
        suvm: el.flags.contains(ElementFlags::UV_SNAP_MIRROR),
        sh: el.flags.contains(ElementFlags::SHADE),
        inv: el.flags.contains(ElementFlags::INVERT_NORMALS),
        t: el.kind as i32,
    }
}

fn raw_texture(tex: &Texture) -> RawTexture {
    let (f, bytes) = tex.image.stored_bytes();
    RawTexture {
        n: tex.name.to_string(),
        w: i32::try_from(tex.image.w).unwrap_or(i32::MAX),
        h: i32::try_from(tex.image.h).unwrap_or(i32::MAX),
        f,
        r: Arc::from(bytes),
    }
}

fn to_i32(i: u32) -> i32 {
    i32::try_from(i).unwrap_or(i32::MAX)
}

/// The mesh record: geometry and sub-meshes only for a custom mesh (native 4.6).
fn mesh_record(mesh: &Mesh, selected: bool) -> MeshRecord {
    let base = &mesh.base;
    let custom = mesh.kind == MeshKind::Custom;
    let geometry = &mesh.geometry;
    let me = custom.then(|| MeshDataRecord {
        v: geometry.vertices.clone(),
        // Cubik writes an empty array for a mesh without normals or coordinates.
        n: Some(geometry.normals.clone().unwrap_or_default()),
        t: geometry.triangles.iter().map(|&t| to_i32(t)).collect(),
        u: Some(geometry.uvs.clone()),
    });
    let sm = (custom && !mesh.submeshes.is_empty()).then(|| {
        mesh.submeshes
            .iter()
            .map(|sub| SubmeshRecord {
                t: sub.triangles.iter().map(|&t| to_i32(t)).collect(),
                c: pack::submesh(sub.color),
                tx: sub.texture.as_ref().map(raw_texture),
            })
            .collect()
    });
    MeshRecord {
        p: base.p,
        s: base.s,
        r: base.e,
        o: base.o,
        is: selected,
        ih: base.hidden,
        il: base.locked,
        l: Some(base.label.to_string()),
        tp: mesh.kind as i32,
        tx: mesh.texture.as_ref().map(raw_texture),
        mb: Some(BoundsRecord {
            c: mesh.bounds.center,
            s: mesh.bounds.size,
        }),
        me,
        sm,
    }
}

// --- Voxels -------------------------------------------------------------------------------------

/// One world with the chunks that are written: allocated chunks and chunks erased again (an
/// empty `v`), in slot order, voxels in scan order (native 4.7).
fn voxel_data(snap: &cuboid_model::voxel::VoxelSnap) -> VoxelData {
    let n = (3 * snap.res).div_ceil(16) as usize;
    let chunks: Vec<ChunkRecord> = snap
        .slots
        .par_iter()
        .enumerate()
        .filter_map(|(at, slot)| {
            let v = match slot {
                ChunkSlot::Unallocated => return None,
                ChunkSlot::EmptyWritten => Vec::new(),
                ChunkSlot::Data(chunk) => chunk
                    .scan()
                    .map(|(local, value)| voxel_record(local, value))
                    .collect(),
            };
            let coord = |c: usize| i32::try_from(c).unwrap_or(i32::MAX);
            Some(ChunkRecord {
                cx: coord(at / (n * n)),
                cy: coord(at / n % n),
                cz: coord(at % n),
                v,
            })
        })
        .collect();
    VoxelData {
        w: vec![WorldRecord {
            r: i32::try_from(snap.res).unwrap_or(i32::MAX),
            c: chunks,
        }],
    }
}

fn voxel_record(local: usize, value: u32) -> VoxelRecord {
    let part = |shift: usize| u8::try_from(local / shift % CHUNK_EDGE).unwrap_or(0);
    VoxelRecord {
        x: part(1),
        y: part(CHUNK_EDGE),
        z: part(CHUNK_EDGE * CHUNK_EDGE),
        c: value.cast_signed(),
    }
}
