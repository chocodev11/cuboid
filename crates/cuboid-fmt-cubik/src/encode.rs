//! Records to BSON payload (native 3, 4, 8.2).
//!
//! Keys are written in the order of the schema tables (native 3.3), floats as exact widenings of
//! the f32 values, enumerations and flags as int32. A document reserves its size field and
//! patches it when it closes. The chunks of the voxel world, which make up most of a large
//! payload, are encoded in parallel into separate buffers that are joined in file order.
//!
//! spec: native 3.1-3.3, 8.2, 8.5, arch 12.3

use cuboid_bson::{DocWriter, Key};
use cuboid_model::{DisplaySlots, ModelProps};
use glam::{Vec2, Vec3};
use rayon::prelude::*;

use crate::CubikError;
use crate::records::{
    BoundsRecord, CameraRecord, ChunkRecord, ElementRecord, FaceRecord, MeshDataRecord, MeshRecord,
    QuatRecord, RawTexture, Root, SubmeshRecord, TextureRecord, ViewRecord, VoxelData, VoxelRecord,
};

/// Writes a whole payload from records. A section that is `None` is left out; `vx` is written
/// as null without a voxel model.
///
/// spec: native 4.1
pub fn encode(root: &Root) -> Result<Vec<u8>, CubikError> {
    let mut w = DocWriter::new();
    let top = w.begin_root();
    w.i32(b"v\0", root.v);
    if let Some(mp) = &root.mp {
        props(&mut w, b"mp\0", mp);
    }
    if let Some(vp) = &root.vp {
        view(&mut w, b"vp\0", vp);
    }
    if let Some(bp) = &root.bp {
        element(&mut w, b"bp\0", bp);
    }
    if let Some(list) = &root.el {
        let arr = w.begin_array(b"el\0");
        for (i, rec) in list.iter().enumerate() {
            element(&mut w, i, rec);
        }
        w.end(arr);
    }
    if let Some(list) = &root.tx {
        let arr = w.begin_array(b"tx\0");
        for (i, rec) in list.iter().enumerate() {
            texture(&mut w, i, rec);
        }
        w.end(arr);
    }
    voxels(&mut w, root.vx.as_ref());
    if let Some(list) = &root.me {
        let arr = w.begin_array(b"me\0");
        for (i, rec) in list.iter().enumerate() {
            mesh(&mut w, i, rec);
        }
        w.end(arr);
    }
    if let Some(md) = &root.md {
        display(&mut w, b"md\0", md);
    }
    w.end(top);
    w.finish()
        .map_err(|err| CubikError::Invalid(err.to_string()))
}

pub(crate) fn vec3(w: &mut DocWriter, key: impl Key, v: Vec3) {
    let mark = w.begin_doc(key);
    w.f32(b"x\0", v.x);
    w.f32(b"y\0", v.y);
    w.f32(b"z\0", v.z);
    w.end(mark);
}

pub(crate) fn vec2(w: &mut DocWriter, key: impl Key, v: Vec2) {
    let mark = w.begin_doc(key);
    w.f32(b"x\0", v.x);
    w.f32(b"y\0", v.y);
    w.end(mark);
}

fn opt_str(w: &mut DocWriter, key: impl Key, value: Option<&str>) {
    match value {
        Some(text) => w.str(key, text),
        None => w.null(key),
    }
}

pub(crate) fn quaternion(w: &mut DocWriter, key: impl Key, q: &QuatRecord) {
    let mark = w.begin_doc(key);
    w.f32(b"x\0", q.x);
    w.f32(b"y\0", q.y);
    w.f32(b"z\0", q.z);
    w.f32(b"w\0", q.w);
    if let Some(euler) = q.euler {
        vec3(w, b"eulerAngles\0", euler);
    }
    w.end(mark);
}

pub(crate) fn props(w: &mut DocWriter, key: impl Key, p: &ModelProps) {
    let mp = w.begin_doc(key);
    w.str(b"sp\0", &p.save_path);
    w.str(b"ep\0", &p.export_path);

    let g = w.begin_doc(b"g\0");
    vec3(w, b"o\0", p.general.origin);
    match &p.general.model_box {
        Some(b) => {
            let mb = w.begin_doc(b"mb\0");
            vec3(w, b"min\0", b.min);
            vec3(w, b"max\0", b.max);
            w.end(mb);
        }
        None => w.null(b"mb\0"),
    }
    let general = &p.general;
    w.bool(b"ee\0", general.exclude_elements);
    w.bool(b"ev\0", general.exclude_voxels);
    w.bool(b"em\0", general.exclude_meshes);
    w.bool(b"me\0", general.minify);
    w.bool(b"rer\0", general.restrict_rotations);
    w.bool(b"ehe\0", general.exclude_hidden);
    w.bool(b"pbt\0", general.prevent_bleeding);
    w.bool(b"pzf\0", general.prevent_zfighting);
    w.str(b"d\0", &general.description);
    w.str(b"n\0", &general.name);
    w.end(g);

    let mc = w.begin_doc(b"mc\0");
    let m = &p.minecraft;
    w.i32(b"t\0", m.kind as i32);
    w.bool(b"ao\0", m.ambient_occlusion);
    w.bool(b"uen\0", m.element_names);
    w.bool(b"uvds\0", m.auto_vanilla_parent);
    w.bool(b"uds\0", m.include_display);
    w.bool(b"uos\0", m.keep_overrides);
    w.str(b"eab\0", &m.entity_attach_bone);
    w.i32(b"etr\0", m.entity_resolution);
    w.i32(b"etx\0", m.entity_offset_x);
    w.i32(b"ety\0", m.entity_offset_y);
    w.i32(b"etw\0", m.entity_min_width);
    w.i32(b"eth\0", m.entity_min_height);
    w.bool(b"esb\0", m.entity_same_bone);
    w.bool(b"est\0", m.entity_reuse_texture);
    w.bool(b"ecn\0", m.entity_cube_names);
    w.str(b"mt\0", &m.block_table);
    w.str(b"p\0", &m.parent);
    w.end(mc);

    let ht = w.begin_doc(b"ht\0");
    w.end(ht);

    let sh = w.begin_doc(b"sh\0");
    let s = &p.stonehearth;
    w.bool(b"pog\0", s.on_ground);
    w.bool(b"pow\0", s.on_walls);
    w.i32(b"v\0", s.gold);
    w.str(b"r\0", &s.rarity);
    w.bool(b"b\0", s.buyable);
    w.bool(b"s\0", s.sellable);
    w.end(sh);

    let st = w.begin_doc(b"st\0");
    w.i32(b"t\0", p.staxel.kind as i32);
    w.end(st);

    let tr = w.begin_doc(b"tr\0");
    w.end(tr);

    let qb = w.begin_doc(b"qb\0");
    w.bool(b"rgb\0", p.qubicle.rgb);
    w.bool(b"aiz\0", p.qubicle.inverted_z);
    w.end(qb);

    let obj = w.begin_doc(b"obj\0");
    let o = &p.mesh_export;
    w.bool(b"gmv\0", o.voxel_colored);
    w.bool(b"gmt\0", o.voxel_palette_texture);
    w.bool(b"omt\0", o.voxel_atlas_texture);
    w.bool(b"wv\0", o.weld_vertices);
    w.bool(b"mo\0", o.merge_objects);
    w.bool(b"pt\0", o.atlas_padding);
    w.bool(b"ao\0", o.ambient_occlusion);
    w.bool(b"sv\0", o.smooth_vertices);
    w.end(obj);

    let png = w.begin_doc(b"png\0");
    w.bool(b"gp\0", p.png.gui_preview);
    w.bool(b"mv\0", p.png.main_view);
    w.bool(b"aa\0", p.png.antialias);
    w.end(png);

    w.end(mp);
}

fn camera(w: &mut DocWriter, key: impl Key, cam: Option<&CameraRecord>) {
    let Some(cam) = cam else {
        w.null(key);
        return;
    };
    let mark = w.begin_doc(key);
    vec3(w, b"p\0", cam.p);
    quaternion(w, b"r\0", &cam.r);
    w.bool(b"o\0", cam.o);
    w.f32(b"os\0", cam.os);
    w.f32(b"fv\0", cam.fv);
    w.end(mark);
}

pub(crate) fn view(w: &mut DocWriter, key: impl Key, v: &ViewRecord) {
    let mark = w.begin_doc(key);
    camera(w, b"ctl\0", v.cameras[0].as_ref());
    camera(w, b"ctr\0", v.cameras[1].as_ref());
    camera(w, b"cbl\0", v.cameras[2].as_ref());
    camera(w, b"cbr\0", v.cameras[3].as_ref());
    w.f32(b"gm\0", v.gm);
    w.f32(b"gk\0", v.gk);
    w.f32(b"gu\0", v.gu);
    w.f32(b"gd\0", v.gd);
    w.bool(b"gl\0", v.gl);
    w.end(mark);
}

pub(crate) fn face(w: &mut DocWriter, key: impl Key, f: &FaceRecord) {
    let mark = w.begin_doc(key);
    opt_str(w, b"n\0", f.n.as_deref());
    w.bool(b"a\0", f.a);
    w.i32(b"ti\0", f.ti);
    w.i32(b"c\0", f.c);
    opt_str(w, b"t\0", f.t.as_deref());
    let uv = w.begin_doc(b"uv\0");
    w.i32(b"r\0", f.uv.r);
    vec2(w, b"tl\0", f.uv.tl);
    vec2(w, b"br\0", f.uv.br);
    w.end(uv);
    w.end(mark);
}

pub(crate) fn element(w: &mut DocWriter, key: impl Key, e: &ElementRecord) {
    let mark = w.begin_doc(key);
    element_members(w, e);
    w.end(mark);
}

/// The members of an element record, for callers that open the document themselves.
pub(crate) fn element_members(w: &mut DocWriter, e: &ElementRecord) {
    vec3(w, b"p\0", e.p);
    vec3(w, b"s\0", e.s);
    vec3(w, b"r\0", e.r);
    vec3(w, b"o\0", e.o);
    w.i32(b"c\0", e.c);
    w.bool(b"is\0", e.is);
    w.bool(b"ih\0", e.ih);
    w.bool(b"il\0", e.il);
    opt_str(w, b"l\0", e.l.as_deref());
    match &e.f {
        Some(set) => {
            let f = w.begin_doc(b"f\0");
            for (&key, rec) in FACE_KEYS_Z.iter().zip(&set.faces) {
                face(w, key, rec);
            }
            w.end(f);
        }
        None => w.null(b"f\0"),
    }
    w.bool(b"rs\0", e.rs);
    w.bool(b"suv\0", e.suv);
    w.bool(b"suvm\0", e.suvm);
    w.bool(b"sh\0", e.sh);
    w.bool(b"in\0", e.inv);
    w.i32(b"t\0", e.t);
}

const FACE_KEYS_Z: [&[u8]; 6] = [b"d\0", b"u\0", b"n\0", b"s\0", b"w\0", b"e\0"];

const DISPLAY_KEYS_Z: [&[u8]; 8] = [
    b"tr\0", b"tl\0", b"fr\0", b"fl\0", b"u\0", b"h\0", b"g\0", b"i\0",
];

pub(crate) fn raw_texture(w: &mut DocWriter, key: impl Key, t: &RawTexture) {
    raw_texture_parts(w, key, &t.n, t.w, t.h, t.f, &t.r);
}

/// A RawTexture from its parts, for writers that hold the pixel bytes elsewhere.
pub(crate) fn raw_texture_parts(
    w: &mut DocWriter,
    key: impl Key,
    name: &str,
    width: i32,
    height: i32,
    format: i32,
    pixels: &[u8],
) {
    let mark = w.begin_doc(key);
    w.str(b"n\0", name);
    w.i32(b"w\0", width);
    w.i32(b"h\0", height);
    w.i32(b"f\0", format);
    w.bin(b"r\0", pixels);
    w.end(mark);
}

fn opt_raw_texture(w: &mut DocWriter, key: impl Key, t: Option<&RawTexture>) {
    match t {
        Some(t) => raw_texture(w, key, t),
        None => w.null(key),
    }
}

pub(crate) fn texture(w: &mut DocWriter, key: impl Key, t: &TextureRecord) {
    let mark = w.begin_doc(key);
    w.str(b"lb\0", &t.lb);
    opt_str(w, b"fp\0", t.fp.as_deref());
    opt_str(w, b"fn\0", t.fname.as_deref());
    w.str(b"fm\0", &t.fm);
    raw_texture(w, b"tx\0", &t.tx);
    opt_str(w, b"cs\0", t.cs.as_deref());
    w.end(mark);
}

fn vec3_array(w: &mut DocWriter, key: impl Key, items: &[Vec3]) {
    let arr = w.begin_array(key);
    for (i, &v) in items.iter().enumerate() {
        vec3(w, i, v);
    }
    w.end(arr);
}

fn i32_array(w: &mut DocWriter, key: impl Key, items: &[i32]) {
    let arr = w.begin_array(key);
    for (i, &v) in items.iter().enumerate() {
        w.i32(i, v);
    }
    w.end(arr);
}

fn mesh_data(w: &mut DocWriter, key: impl Key, d: &MeshDataRecord) {
    let mark = w.begin_doc(key);
    vec3_array(w, b"v\0", &d.v);
    match &d.n {
        Some(n) => vec3_array(w, b"n\0", n),
        None => w.null(b"n\0"),
    }
    i32_array(w, b"t\0", &d.t);
    match &d.u {
        Some(u) => {
            let arr = w.begin_array(b"u\0");
            for (i, &v) in u.iter().enumerate() {
                vec2(w, i, v);
            }
            w.end(arr);
        }
        None => w.null(b"u\0"),
    }
    w.end(mark);
}

fn bounds(w: &mut DocWriter, key: impl Key, b: &BoundsRecord) {
    let mark = w.begin_doc(key);
    vec3(w, b"c\0", b.c);
    vec3(w, b"s\0", b.s);
    w.end(mark);
}

fn submesh(w: &mut DocWriter, key: impl Key, s: &SubmeshRecord) {
    let mark = w.begin_doc(key);
    i32_array(w, b"t\0", &s.t);
    w.i32(b"c\0", s.c);
    opt_raw_texture(w, b"tx\0", s.tx.as_ref());
    w.end(mark);
}

pub(crate) fn mesh(w: &mut DocWriter, key: impl Key, m: &MeshRecord) {
    let mark = w.begin_doc(key);
    vec3(w, b"p\0", m.p);
    vec3(w, b"s\0", m.s);
    vec3(w, b"r\0", m.r);
    vec3(w, b"o\0", m.o);
    w.bool(b"is\0", m.is);
    w.bool(b"ih\0", m.ih);
    w.bool(b"il\0", m.il);
    opt_str(w, b"l\0", m.l.as_deref());
    w.i32(b"tp\0", m.tp);
    opt_raw_texture(w, b"tx\0", m.tx.as_ref());
    match &m.mb {
        Some(b) => bounds(w, b"mb\0", b),
        None => w.null(b"mb\0"),
    }
    match &m.me {
        Some(d) => mesh_data(w, b"me\0", d),
        None => w.null(b"me\0"),
    }
    match &m.sm {
        Some(list) => {
            let arr = w.begin_array(b"sm\0");
            for (i, s) in list.iter().enumerate() {
                submesh(w, i, s);
            }
            w.end(arr);
        }
        None => w.null(b"sm\0"),
    }
    w.end(mark);
}

pub(crate) fn display(w: &mut DocWriter, key: impl Key, md: &DisplaySlots) {
    let mark = w.begin_doc(key);
    for (&key, slot) in DISPLAY_KEYS_Z.iter().zip(&md.slots) {
        let s = w.begin_doc(key);
        w.str(b"n\0", &slot.preset);
        w.bool(b"c\0", slot.removable);
        w.bool(b"e\0", slot.enabled);
        vec3(w, b"r\0", slot.rotation);
        vec3(w, b"t\0", slot.translation);
        w.f32(b"a\0", 0.0);
        vec3(w, b"s\0", slot.scale);
        w.end(s);
    }
    w.end(mark);
}

/// `vx`: the voxel world or null.
pub(crate) fn voxels(w: &mut DocWriter, vx: Option<&VoxelData>) {
    let Some(vx) = vx else {
        w.null(b"vx\0");
        return;
    };
    let mark = w.begin_doc(b"vx\0");
    let worlds = w.begin_array(b"w\0");
    for (i, world) in vx.w.iter().enumerate() {
        let wm = w.begin_doc(i);
        w.i32(b"r\0", world.r);
        let chunks = w.begin_array(b"c\0");
        let fragments: Vec<Vec<u8>> = world
            .c
            .par_iter()
            .enumerate()
            .map(|(index, chunk)| chunk_fragment(index, chunk))
            .collect();
        for fragment in &fragments {
            w.raw(fragment);
        }
        w.end(chunks);
        w.end(wm);
    }
    w.end(worlds);
    w.end(mark);
}

/// The array element of one chunk, as a buffer of its own.
pub(crate) fn chunk_fragment(index: usize, chunk: &ChunkRecord) -> Vec<u8> {
    let mut w = DocWriter::with_capacity(chunk.v.len() * 40 + 96);
    let mark = w.begin_doc(index);
    w.i32(b"cx\0", chunk.cx);
    w.i32(b"cy\0", chunk.cy);
    w.i32(b"cz\0", chunk.cz);
    let voxels = w.begin_array(b"v\0");
    for (i, v) in chunk.v.iter().enumerate() {
        voxel(&mut w, i, v);
    }
    w.end(voxels);
    w.end(mark);
    w.finish().unwrap_or_default()
}

pub(crate) fn voxel(w: &mut DocWriter, key: usize, v: &VoxelRecord) {
    let mark = w.begin_doc(key);
    w.i32(b"x\0", i32::from(v.x));
    w.i32(b"y\0", i32::from(v.y));
    w.i32(b"z\0", i32::from(v.z));
    w.i32(b"c\0", v.c);
    w.end(mark);
}
