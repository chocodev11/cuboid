//! Plain records that mirror the document schema of native 4, one type per record of the file.
//!
//! The codec turns bytes into these records and back; the mapping to the editor model lives in
//! `load` and `save`. A section that is `None` was absent in the file (decode) or is not
//! written (encode); `vx` is the one exception, it is written as null without a voxel model.
//!
//! The model groups `mp` and `md` already have the shape of their records, so the model types
//! `ModelProps` and `DisplaySlots` stand in for them.
//!
//! spec: native 4, arch 12.1

use std::sync::Arc;

use cuboid_model::{DisplaySlots, ModelProps};
use glam::{Quat, Vec2, Vec3};

/// The root document (native 4.1).
#[derive(Clone, Debug, PartialEq)]
pub struct Root {
    /// Format version `v`, always 1 and never checked.
    pub v: i32,
    /// Model properties.
    pub mp: Option<ModelProps>,
    /// Grid steps and cameras.
    pub vp: Option<ViewRecord>,
    /// The blueprint.
    pub bp: Option<ElementRecord>,
    /// Elements in list order.
    pub el: Option<Vec<ElementRecord>>,
    /// Texture list in list order.
    pub tx: Option<Vec<TextureRecord>>,
    /// The voxel model; `None` is written as null.
    pub vx: Option<VoxelData>,
    /// Meshes in list order.
    pub me: Option<Vec<MeshRecord>>,
    /// The eight display slots.
    pub md: Option<DisplaySlots>,
}

impl Default for Root {
    fn default() -> Self {
        Self {
            v: 1,
            mp: None,
            vp: None,
            bp: None,
            el: None,
            tx: None,
            vx: None,
            me: None,
            md: None,
        }
    }
}

/// A quaternion with the redundant Euler copy (native 3.4, 4.3.2).
#[derive(Clone, Copy, Debug)]
pub struct QuatRecord {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
    /// `eulerAngles` in degrees; `None` when the key is absent.
    pub euler: Option<Vec3>,
    /// What Cubik's reader ends up with: the members applied in document order, where
    /// `eulerAngles` replaces the whole rotation. Not written.
    pub rotation: Quat,
}

impl QuatRecord {
    /// The identity rotation without an Euler copy: what a reader makes of an absent `r`.
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
        euler: None,
        rotation: Quat::IDENTITY,
    };
}

/// Equality of the stored members; the bits count, so `-0.0` differs from `0.0`.
impl PartialEq for QuatRecord {
    fn eq(&self, other: &Self) -> bool {
        let bits = |q: &Self| [q.x, q.y, q.z, q.w].map(f32::to_bits);
        let euler = |q: &Self| q.euler.map(|e| e.to_array().map(f32::to_bits));
        bits(self) == bits(other) && euler(self) == euler(other)
    }
}

/// One view camera (native 4.3.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraRecord {
    pub p: Vec3,
    pub r: QuatRecord,
    pub o: bool,
    pub os: f32,
    pub fv: f32,
}

/// `vp` (native 4.3).
#[derive(Clone, Debug, PartialEq)]
pub struct ViewRecord {
    /// `ctl`, `ctr`, `cbl`, `cbr`.
    pub cameras: [Option<CameraRecord>; 4],
    pub gm: f32,
    pub gk: f32,
    pub gu: f32,
    pub gd: f32,
    pub gl: bool,
}

impl Default for ViewRecord {
    fn default() -> Self {
        Self {
            cameras: [None; 4],
            gm: 0.5,
            gk: 0.125,
            gu: 0.5,
            gd: 1.0,
            gl: false,
        }
    }
}

/// An element record, also the shape of the blueprint (native 4.4).
#[derive(Clone, Debug, PartialEq)]
pub struct ElementRecord {
    pub p: Vec3,
    pub s: Vec3,
    pub r: Vec3,
    pub o: Vec3,
    pub c: i32,
    pub is: bool,
    pub ih: bool,
    pub il: bool,
    pub l: Option<String>,
    /// The six faces; null for the blueprint.
    pub f: Option<FaceSetRecord>,
    pub rs: bool,
    pub suv: bool,
    pub suvm: bool,
    pub sh: bool,
    pub inv: bool,
    pub t: i32,
}

impl Default for ElementRecord {
    /// A record created by the reader without keys (native 4.4 "if absent").
    fn default() -> Self {
        Self {
            p: Vec3::ZERO,
            s: Vec3::ZERO,
            r: Vec3::ZERO,
            o: Vec3::ZERO,
            c: 0,
            is: false,
            ih: false,
            il: false,
            l: None,
            f: None,
            rs: false,
            suv: false,
            suvm: false,
            sh: false,
            inv: false,
            t: 0,
        }
    }
}

/// `f`: the faces in the order `d`, `u`, `n`, `s`, `w`, `e` (native 4.4.1).
#[derive(Clone, Debug, PartialEq)]
pub struct FaceSetRecord {
    pub faces: [FaceRecord; 6],
}

/// The names of the faces in file order.
pub const FACE_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

/// The keys of the faces in file order.
pub const FACE_KEYS: [&[u8]; 6] = [b"d", b"u", b"n", b"s", b"w", b"e"];

/// One face (native 4.4.2).
#[derive(Clone, Debug, PartialEq)]
pub struct FaceRecord {
    pub n: Option<String>,
    pub a: bool,
    pub ti: i32,
    pub c: i32,
    pub t: Option<String>,
    pub uv: FaceUvRecord,
}

impl FaceRecord {
    /// A face created by the reader without keys, in slot `slot` (native 4.4.2 "if absent").
    pub fn absent(slot: usize) -> Self {
        Self {
            n: FACE_NAMES.get(slot).map(|name| (*name).to_owned()),
            a: true,
            ti: -1,
            c: 0,
            t: Some("texture".to_owned()),
            uv: FaceUvRecord::default(),
        }
    }
}

/// `uv` of a face (native 4.4.3).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FaceUvRecord {
    pub r: i32,
    pub tl: Vec2,
    pub br: Vec2,
}

/// The pixels of a texture exactly as stored (native 4.5.1).
#[derive(Clone, Debug, PartialEq)]
pub struct RawTexture {
    pub n: String,
    pub w: i32,
    pub h: i32,
    pub f: i32,
    pub r: Arc<[u8]>,
}

/// One item of `tx` (native 4.5).
#[derive(Clone, Debug, PartialEq)]
pub struct TextureRecord {
    pub lb: String,
    pub fp: Option<String>,
    pub fname: Option<String>,
    pub fm: String,
    pub tx: RawTexture,
    pub cs: Option<String>,
}

/// One item of `me` (native 4.6).
#[derive(Clone, Debug, PartialEq)]
pub struct MeshRecord {
    pub p: Vec3,
    pub s: Vec3,
    pub r: Vec3,
    pub o: Vec3,
    pub is: bool,
    pub ih: bool,
    pub il: bool,
    pub l: Option<String>,
    pub tp: i32,
    pub tx: Option<RawTexture>,
    pub mb: Option<BoundsRecord>,
    pub me: Option<MeshDataRecord>,
    pub sm: Option<Vec<SubmeshRecord>>,
}

impl Default for MeshRecord {
    fn default() -> Self {
        Self {
            p: Vec3::ZERO,
            s: Vec3::ZERO,
            r: Vec3::ZERO,
            o: Vec3::ZERO,
            is: false,
            ih: false,
            il: false,
            l: None,
            tp: 0,
            tx: None,
            mb: None,
            me: None,
            sm: None,
        }
    }
}

/// `mb` (native 4.6.2).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoundsRecord {
    pub c: Vec3,
    pub s: Vec3,
}

/// `me` of a custom mesh (native 4.6.1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshDataRecord {
    pub v: Vec<Vec3>,
    /// `None` is null; Cubik writes an empty array for a mesh without normals.
    pub n: Option<Vec<Vec3>>,
    pub t: Vec<i32>,
    /// `None` is null; Cubik writes an empty array when there are no coordinates.
    pub u: Option<Vec<Vec2>>,
}

/// One item of `sm` (native 4.6.3).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubmeshRecord {
    pub t: Vec<i32>,
    pub c: i32,
    pub tx: Option<RawTexture>,
}

/// `vx` (native 4.7). Only the first world is read; Cubik writes exactly one.
#[derive(Clone, Debug, PartialEq)]
pub struct VoxelData {
    pub w: Vec<WorldRecord>,
}

/// One voxel world: the resolution and the chunks that were written.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldRecord {
    pub r: i32,
    pub c: Vec<ChunkRecord>,
}

/// One chunk of a voxel world. A chunk whose voxels were all erased one by one is still
/// written, with an empty `v`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChunkRecord {
    pub cx: i32,
    pub cy: i32,
    pub cz: i32,
    pub v: Vec<VoxelRecord>,
}

/// One voxel; the coordinates are local to the chunk (0..15). The decoder skips records outside
/// that range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoxelRecord {
    pub x: u8,
    pub y: u8,
    pub z: u8,
    /// The packed colour as stored (native 4.10).
    pub c: i32,
}
