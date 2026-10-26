//! The model as a list of triangle parts, in the frame of the exported files (see the crate
//! documentation). The OBJ and the glTF writer both start from a [`Scene`].
//!
//! - An **element** becomes one part with up to 24 vertices: each active face has its own four
//!   vertices, with the face normal and the UV corners the renderer uses, and the triangles of
//!   the renderer's corner tables. The textures of the faces and the colour of the element
//!   become materials.
//! - A **mesh** becomes one part with the vertices it stores (welded further when asked), one
//!   primitive per sub-mesh.
//! - The **voxel world** becomes one part named `Voxels` with vertex colours, built from the
//!   face records of the canonical chunk mesher. Coplanar faces of one colour merge into
//!   rectangles, except faces that carry ambient-occlusion shading.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cuboid_model::image::Image;
use cuboid_model::voxel::{
    ChunkSlot, Dir, ExportVertex, VoxelFace, VoxelWorld, mapping, mesh as voxel_mesh,
};
use cuboid_model::{Document, Element, ElementFlags, FaceName, Mesh, ObjRef};
use cuboid_num::euler_to_quat;
use glam::{Mat3, Quat, Vec2, Vec3};

use crate::names::{self, Unique};
use crate::{MeshError, Notice};

/// The unit of the exported coordinates.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UnitScale {
    /// One unit is one block: the model's 16 units per block divide by 16. In glTF one unit is
    /// one metre, so a block is a metre.
    #[default]
    Block,
    /// The model's own units, sixteen per block.
    Pixel,
}

/// Sizes that stop an export before it allocates more than it should.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Vertices of all parts together.
    pub max_vertices: u32,
    /// Triangles of all parts together.
    pub max_triangles: u32,
    /// Texels of all textures together.
    pub max_texels: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_vertices: 1 << 23,
            max_triangles: 1 << 24,
            max_texels: 1 << 28,
        }
    }
}

/// What is exported and how.
#[derive(Clone, Debug)]
pub struct MeshOptions {
    /// Leave out hidden elements and meshes. On by default: the files hold what is drawn.
    pub skip_hidden: bool,
    /// The unit of the coordinates.
    pub unit_scale: UnitScale,
    /// Share vertices that are equal in position, normal, UV and colour. Meshes keep the sharing
    /// of their own vertices either way.
    pub weld: bool,
    /// Write everything as one part named after the model.
    pub merge_objects: bool,
    /// Merge coplanar voxel faces of one colour into rectangles.
    pub merge_voxel_faces: bool,
    /// The size limits.
    pub limits: Limits,
}

impl Default for MeshOptions {
    fn default() -> Self {
        Self {
            skip_hidden: true,
            unit_scale: UnitScale::Block,
            weld: true,
            merge_objects: false,
            merge_voxel_faces: true,
            limits: Limits::default(),
        }
    }
}

impl MeshOptions {
    /// The defaults with the weld and merge choices stored in the model's mesh export settings.
    /// The stored weld flag is inverted in the reference; here it means what its label says
    /// (interop 16.4, 31.9).
    pub fn for_document(doc: &Document) -> Self {
        let settings = &doc.props.mesh_export;
        Self {
            weld: settings.weld_vertices,
            merge_objects: settings.merge_objects,
            ..Self::default()
        }
    }
}

/// How a material blends with what is behind it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlphaMode {
    /// No transparency.
    Opaque,
    /// Texels are fully opaque or fully transparent (cutout).
    Mask,
    /// Partial alpha.
    Blend,
}

/// A texture used by a material.
#[derive(Clone, Debug)]
pub struct Tex {
    /// A unique name, safe in file names.
    pub name: String,
    /// The pixels, rows bottom to top.
    pub image: Arc<Image>,
}

/// A material: a colour factor, an optional texture, vertex colours.
#[derive(Clone, Debug)]
pub struct Material {
    /// A unique name, safe in file names.
    pub name: String,
    /// RGBA in 0..=1 as displayed (sRGB); multiplies the texture.
    pub color: [f32; 4],
    /// Index into [`Scene::textures`].
    pub texture: Option<usize>,
    /// The colours of the vertices multiply the material.
    pub vertex_colors: bool,
    /// The blend mode.
    pub alpha: AlphaMode,
}

/// The triangles of one material in a part.
#[derive(Clone, Debug)]
pub struct Prim {
    /// Index into [`Scene::materials`].
    pub material: usize,
    /// Three vertex indices per triangle, counter-clockwise seen from outside.
    pub indices: Vec<u32>,
}

/// One object of the export.
#[derive(Clone, Debug, Default)]
pub struct Part {
    /// The name of the object in the model.
    pub name: String,
    /// Positions in the exported frame.
    pub positions: Vec<[f32; 3]>,
    /// Unit normals, one per position.
    pub normals: Vec<[f32; 3]>,
    /// Texture coordinates (V up), one per position, or empty when no vertex has any.
    pub uvs: Vec<[f32; 2]>,
    /// Vertex colours as displayed (sRGB), one per position, or empty when no vertex has any.
    pub colors: Vec<[f32; 4]>,
    /// The triangles by material.
    pub prims: Vec<Prim>,
}

/// The exported model.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    /// The model name.
    pub name: String,
    /// The model description; may be empty.
    pub description: String,
    /// The parts in export order: elements, meshes, voxels.
    pub parts: Vec<Part>,
    /// The materials.
    pub materials: Vec<Material>,
    /// The textures.
    pub textures: Vec<Tex>,
    /// What was left out.
    pub notices: Vec<Notice>,
}

/// The corners of the unit box: `x` and `y` in 0..1, `z` in -1..0.
const CORNERS: [Vec3; 8] = [
    Vec3::new(0.0, 0.0, 0.0),
    Vec3::new(1.0, 0.0, 0.0),
    Vec3::new(1.0, 0.0, -1.0),
    Vec3::new(0.0, 0.0, -1.0),
    Vec3::new(0.0, 1.0, 0.0),
    Vec3::new(1.0, 1.0, 0.0),
    Vec3::new(1.0, 1.0, -1.0),
    Vec3::new(0.0, 1.0, -1.0),
];

/// The four corners of each face slot (down, up, north, south, west, east) for outward normals.
const OUTER: [[usize; 4]; 6] = [
    [2, 3, 0, 1],
    [7, 6, 5, 4],
    [4, 5, 1, 0],
    [6, 7, 3, 2],
    [7, 4, 0, 3],
    [5, 6, 2, 1],
];

/// As [`OUTER`] for inverted normals: each slot sits on the opposite plane and keeps its normal.
const INNER: [[usize; 4]; 6] = [
    [6, 7, 4, 5],
    [3, 2, 1, 0],
    [7, 6, 2, 3],
    [5, 4, 0, 1],
    [6, 5, 1, 2],
    [4, 7, 3, 0],
];

/// The normal of each face slot.
const NORMALS: [Vec3; 6] = [
    Vec3::new(0.0, -1.0, 0.0),
    Vec3::new(0.0, 1.0, 0.0),
    Vec3::new(0.0, 0.0, 1.0),
    Vec3::new(0.0, 0.0, -1.0),
    Vec3::new(-1.0, 0.0, 0.0),
    Vec3::new(1.0, 0.0, 0.0),
];

/// Rounds to six decimals so that the noise of rotations stays out of the files, and turns -0
/// into 0. The narrowing is the point: the rounded value is the nearest `f32` to a short decimal.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn clean(v: f32) -> f32 {
    ((f64::from(v) * 1e6).round() / 1e6) as f32 + 0.0
}

fn clean3(v: [f32; 3]) -> [f32; 3] {
    v.map(clean)
}

/// A colour component in 0..=1 as a byte.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn unit_byte(v: f32) -> u8 {
    // The clamp keeps the rounded value in 0..=255.
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The map from the model's frame to the exported one.
struct Convert {
    origin: Vec3,
    scale: f32,
}

impl Convert {
    fn pos(&self, p: Vec3) -> [f32; 3] {
        let q = (p - self.origin) * self.scale;
        clean3([-q.x, q.y, q.z])
    }

    fn dir(&self, n: Vec3) -> [f32; 3] {
        let n = n.normalize_or_zero();
        clean3([-n.x, n.y, n.z])
    }
}

/// A vertex before it is added to a part.
struct Vtx {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: Option<[f32; 2]>,
    color: Option<[f32; 4]>,
}

impl Vtx {
    /// The bit patterns of every attribute, to find an equal vertex.
    fn key(&self) -> [u32; 13] {
        let uv = self.uv.unwrap_or([0.0; 2]);
        let color = self.color.unwrap_or([0.0; 4]);
        let mut key = [0_u32; 13];
        let floats = self
            .pos
            .into_iter()
            .chain(self.normal)
            .chain(uv)
            .chain(color);
        for (slot, v) in key.iter_mut().zip(floats) {
            *slot = v.to_bits();
        }
        if let Some(last) = key.last_mut() {
            *last = u32::from(self.uv.is_some()) | u32::from(self.color.is_some()) << 1;
        }
        key
    }
}

/// A part under construction.
#[derive(Default)]
struct Builder {
    part: Part,
    any_uv: bool,
    any_color: bool,
    weld: HashMap<[u32; 13], u32>,
    prim_of: HashMap<usize, usize>,
}

impl Builder {
    fn finish(mut self) -> Option<Part> {
        if self.part.prims.iter().all(|p| p.indices.is_empty()) {
            return None;
        }
        if !self.any_uv {
            self.part.uvs.clear();
        }
        if !self.any_color {
            self.part.colors.clear();
        }
        self.part.prims.retain(|p| !p.indices.is_empty());
        Some(self.part)
    }
}

/// The face-to-material and texture state of one export.
struct Gather<'a> {
    doc: &'a Document,
    opts: &'a MeshOptions,
    conv: Convert,
    scene: Scene,
    label_index: HashMap<&'a str, usize>,
    tex_by_image: HashMap<usize, usize>,
    mat_by_key: HashMap<(Option<usize>, [u8; 4], bool), usize>,
    tex_names: Unique,
    mat_names: Unique,
    missing: HashSet<String>,
    cur: Builder,
    open: bool,
    vertices: u32,
    triangles: u32,
    texels: u64,
    bad_elements: usize,
    bad_meshes: usize,
}

/// Collects the model as a [`Scene`]. Hidden objects and the kinds the model excludes from
/// exports (`ee`, `em`, `ev`) are left out.
pub fn gather(doc: &Document, opts: &MeshOptions) -> Result<Scene, MeshError> {
    let general = &doc.props.general;
    let scale = match opts.unit_scale {
        UnitScale::Block => 1.0 / 16.0,
        UnitScale::Pixel => 1.0,
    };
    let origin = if general.origin.is_finite() {
        general.origin
    } else {
        Vec3::ZERO
    };
    let mut g = Gather {
        doc,
        opts,
        conv: Convert { origin, scale },
        scene: Scene {
            name: general.name.clone(),
            description: general.description.clone(),
            ..Scene::default()
        },
        label_index: doc
            .textures
            .entries
            .iter()
            .enumerate()
            .rev()
            .map(|(i, e)| (&*e.label, i))
            .collect(),
        tex_by_image: HashMap::new(),
        mat_by_key: HashMap::new(),
        tex_names: Unique::default(),
        mat_names: Unique::default(),
        missing: HashSet::new(),
        cur: Builder::default(),
        open: false,
        vertices: 0,
        triangles: 0,
        texels: 0,
        bad_elements: 0,
        bad_meshes: 0,
    };

    if !general.exclude_elements {
        for (&id, el) in doc.elements.ids().iter().zip(doc.elements.items()) {
            if opts.skip_hidden && doc.is_hidden(ObjRef::Element(id)) {
                continue;
            }
            g.element(el)?;
        }
    }
    if !general.exclude_meshes {
        for (&id, mesh) in doc.meshes.ids().iter().zip(doc.meshes.items()) {
            if opts.skip_hidden && doc.is_hidden(ObjRef::Mesh(id)) {
                continue;
            }
            g.mesh(mesh)?;
        }
    }
    if !general.exclude_voxels
        && let Some(world) = &doc.voxels
    {
        g.voxels(world)?;
    }
    g.close();

    if g.bad_elements > 0 {
        g.scene.notices.push(Notice::Skipped {
            what: "elements with invalid numbers",
            count: g.bad_elements,
        });
    }
    if g.bad_meshes > 0 {
        g.scene.notices.push(Notice::Skipped {
            what: "meshes without usable geometry",
            count: g.bad_meshes,
        });
    }
    if g.scene.parts.is_empty() {
        return Err(MeshError::Empty);
    }
    Ok(g.scene)
}

impl<'a> Gather<'a> {
    /// Starts a part. With merged objects the first call opens the one part and later calls
    /// continue it.
    fn begin(&mut self, name: &str, fallback: &str) {
        if self.open && self.opts.merge_objects {
            return;
        }
        self.close();
        let name = if self.opts.merge_objects {
            names::group(&self.scene.name, "Model")
        } else {
            names::group(name, fallback)
        };
        self.cur.part.name = name;
        self.open = true;
    }

    /// Ends the open part and keeps it if it has triangles.
    fn close(&mut self) {
        if !self.open {
            return;
        }
        self.open = false;
        if let Some(part) = std::mem::take(&mut self.cur).finish() {
            self.scene.parts.push(part);
        }
    }

    fn vertex(&mut self, v: Vtx) -> Result<u32, MeshError> {
        let key = v.key();
        if self.opts.weld
            && let Some(&index) = self.cur.weld.get(&key)
        {
            return Ok(index);
        }
        if self.vertices >= self.opts.limits.max_vertices {
            return Err(MeshError::Limit("vertices"));
        }
        let cur = &mut self.cur;
        let index = u32::try_from(cur.part.positions.len()).map_err(|_| MeshError::TooLarge)?;
        cur.part.positions.push(v.pos);
        cur.part.normals.push(v.normal);
        cur.part.uvs.push(v.uv.unwrap_or([0.0; 2]));
        cur.part.colors.push(v.color.unwrap_or([1.0; 4]));
        cur.any_uv |= v.uv.is_some();
        cur.any_color |= v.color.is_some();
        if self.opts.weld {
            cur.weld.insert(key, index);
        }
        self.vertices += 1;
        Ok(index)
    }

    /// Adds a triangle that is counter-clockwise in the renderer's frame, written reversed so
    /// that it stays counter-clockwise after the reflection of X.
    fn triangle(&mut self, material: usize, [a, b, c]: [u32; 3]) -> Result<(), MeshError> {
        if self.triangles >= self.opts.limits.max_triangles {
            return Err(MeshError::Limit("triangles"));
        }
        self.triangles += 1;
        let cur = &mut self.cur;
        let slot = match cur.prim_of.get(&material) {
            Some(&slot) => slot,
            None => {
                cur.part.prims.push(Prim {
                    material,
                    indices: Vec::new(),
                });
                cur.prim_of.insert(material, cur.part.prims.len() - 1);
                cur.part.prims.len() - 1
            }
        };
        if let Some(prim) = cur.part.prims.get_mut(slot) {
            prim.indices.extend_from_slice(&[c, b, a]);
        }
        Ok(())
    }

    /// The index of a texture, added on first use. A picture without texels has none.
    fn texture(&mut self, image: &Arc<Image>, name: &str) -> Result<Option<usize>, MeshError> {
        if image.w == 0 || image.h == 0 {
            return Ok(None);
        }
        let id = Arc::as_ptr(image) as usize;
        if let Some(&index) = self.tex_by_image.get(&id) {
            return Ok(Some(index));
        }
        self.texels = self
            .texels
            .saturating_add(u64::from(image.w) * u64::from(image.h));
        if self.texels > self.opts.limits.max_texels {
            return Err(MeshError::Limit("texels"));
        }
        let index = self.scene.textures.len();
        self.scene.textures.push(Tex {
            name: self.tex_names.make(names::token(name, "texture")),
            image: Arc::clone(image),
        });
        self.tex_by_image.insert(id, index);
        Ok(Some(index))
    }

    /// The material of a texture and a colour, added on first use. `hint` names a material
    /// without texture.
    fn material(
        &mut self,
        texture: Option<usize>,
        color: [f32; 4],
        vertex_colors: bool,
        hint: &str,
    ) -> usize {
        let bytes = color.map(unit_byte);
        let key = (texture, bytes, vertex_colors);
        if let Some(&index) = self.mat_by_key.get(&key) {
            return index;
        }
        let tex = texture.and_then(|i| self.scene.textures.get(i));
        let mut name = match tex {
            Some(t) => t.name.clone(),
            None => names::token(hint, "color"),
        };
        if bytes != [255; 4] && !vertex_colors {
            name = format!("{name}_{:02x}{:02x}{:02x}", bytes[0], bytes[1], bytes[2]);
            if bytes[3] != 255 {
                name.push_str(&format!("{:02x}", bytes[3]));
            }
        }
        let alpha = if bytes[3] != 255 {
            AlphaMode::Blend
        } else {
            tex.map_or(AlphaMode::Opaque, |t| texture_alpha(&t.image))
        };
        let index = self.scene.materials.len();
        self.scene.materials.push(Material {
            name: self.mat_names.make(name),
            color: bytes.map(|b| f32::from(b) / 255.0),
            texture,
            vertex_colors,
            alpha,
        });
        self.mat_by_key.insert(key, index);
        index
    }

    fn element(&mut self, el: &Element) -> Result<(), MeshError> {
        let doc = self.doc;
        let base = &el.base;
        if !(base.p.is_finite() && base.s.is_finite() && base.e.is_finite()) {
            self.bad_elements += 1;
            return Ok(());
        }
        self.begin(&base.label, "Element");
        let rot = if base.e == Vec3::ZERO {
            Quat::IDENTITY
        } else {
            euler_to_quat(base.e)
        };
        // A negative size mirrors the box, which reverses the winding of the faces that span the
        // mirrored axis, exactly as the renderer draws it: the side a face shows turns with it.
        // The normal follows the shown side, so a face slot keeps its direction on its own axis
        // and flips once for each mirrored axis among the other two.
        let size = base.s;
        let signs = size.map(|c| if c < 0.0 { -1.0 } else { 1.0 });
        let shown = |normal: Vec3| {
            Vec3::new(
                normal.x * signs.y * signs.z,
                normal.y * signs.z * signs.x,
                normal.z * signs.x * signs.y,
            )
        };
        let table = if el.flags.contains(ElementFlags::INVERT_NORMALS) {
            &INNER
        } else {
            &OUTER
        };
        let color = el.color;
        for (slot, (face, (corners, normal))) in
            el.faces.iter().zip(table.iter().zip(NORMALS)).enumerate()
        {
            if !face.active {
                continue;
            }
            let entry = self
                .label_index
                .get(&*face.texture)
                .and_then(|&i| doc.textures.entries.get(i));
            let texture = match entry {
                Some(entry) => self.texture(&entry.texture.image, &entry.label)?,
                None => {
                    if self.missing.insert(face.texture.to_string()) {
                        self.scene.notices.push(Notice::TextureMissing {
                            label: face.texture.to_string(),
                        });
                    }
                    None
                }
            };
            let material = self.material(texture, color, false, &base.label);

            // The renderer gives vertex `t` of the up face the corner `t` of the rotated UV
            // rectangle and vertex `t` of the other faces the corner `t + 2`.
            let uv = face.uv.rotated_corners();
            let n = self.conv.dir(rot * shown(normal));
            let mut quad = [0_u32; 4];
            for (t, (index, &corner)) in quad.iter_mut().zip(corners).enumerate() {
                let unit = CORNERS.get(corner).copied().unwrap_or(Vec3::ZERO);
                let at = base.p + rot * (unit * size);
                let k = if slot == FaceName::Up as usize {
                    t
                } else {
                    (t + 2) % 4
                };
                let c = uv.get(k).copied().unwrap_or(Vec2::ZERO);
                *index = self.vertex(Vtx {
                    pos: self.conv.pos(at),
                    normal: n,
                    uv: Some(clean2([c.x.clamp(0.0, 1.0), c.y.clamp(0.0, 1.0)])),
                    color: None,
                })?;
            }
            // The renderer draws (3, 1, 0) and (3, 2, 1) of the four corners.
            let [c0, c1, c2, c3] = quad;
            self.triangle(material, [c3, c1, c0])?;
            self.triangle(material, [c3, c2, c1])?;
        }
        Ok(())
    }

    fn mesh(&mut self, mesh: &Mesh) -> Result<(), MeshError> {
        let base = &mesh.base;
        let geometry = &*mesh.geometry;
        let count = geometry.vertices.len();
        let finite = base.p.is_finite() && base.s.is_finite() && base.e.is_finite();
        if !finite
            || count == 0
            || geometry.triangles.len() < 3
            || geometry.vertices.iter().any(|v| !v.is_finite())
        {
            self.bad_meshes += 1;
            return Ok(());
        }
        let count_u32 = u32::try_from(count).map_err(|_| MeshError::TooLarge)?;
        if count_u32 > self.opts.limits.max_vertices {
            return Err(MeshError::Limit("vertices"));
        }
        self.begin(&base.label, "Mesh");

        // Local to world: scale by the size, rotate, move, as the renderer's model matrix.
        let rot = if base.e == Vec3::ZERO {
            Quat::IDENTITY
        } else {
            euler_to_quat(base.e)
        };
        let size = base.s.abs();
        let linear = Mat3::from_quat(rot) * Mat3::from_diagonal(size);
        let normal_matrix = if linear.determinant().abs() > 1e-12 {
            linear.inverse().transpose()
        } else {
            linear
        };
        let normals = mesh_normals(geometry);

        let whole;
        let groups: &[cuboid_model::Submesh] = if mesh.submeshes.is_empty() {
            whole = [cuboid_model::Submesh {
                triangles: geometry.triangles.clone(),
                color: [1.0; 4],
                texture: None,
            }];
            &whole
        } else {
            &mesh.submeshes
        };

        let mut remap: Vec<u32> = vec![u32::MAX; count];
        for (i, sub) in groups.iter().enumerate() {
            let tex = sub.texture.as_ref().or(mesh.texture.as_ref());
            let texture = match tex {
                Some(t) => {
                    let name = if t.name.is_empty() {
                        base.label.to_string()
                    } else {
                        t.name.to_string()
                    };
                    self.texture(&t.image, &name)?
                }
                None => None,
            };
            let color = std::array::from_fn(|k| {
                sub.color.get(k).copied().unwrap_or(1.0) * mesh.tint.get(k).copied().unwrap_or(1.0)
            });
            let hint = if i == 0 {
                base.label.to_string()
            } else {
                format!("{}_{i}", base.label)
            };
            let material = self.material(texture, color, false, &hint);
            for tri in sub.triangles.chunks_exact(3) {
                let mut out = [0_u32; 3];
                let mut valid = true;
                for (slot, &vi) in out.iter_mut().zip(tri) {
                    let vi = vi as usize;
                    let Some(cached) = remap.get(vi).copied() else {
                        valid = false;
                        break;
                    };
                    if cached != u32::MAX {
                        *slot = cached;
                        continue;
                    }
                    let v = geometry.vertices.get(vi).copied().unwrap_or(Vec3::ZERO);
                    let at = base.p + rot * (v * size);
                    let n = normals.get(vi).copied().unwrap_or(Vec3::ZERO);
                    let uv = geometry.uvs.get(vi).copied().unwrap_or(Vec2::ZERO);
                    let index = self.vertex(Vtx {
                        pos: self.conv.pos(at),
                        normal: self.conv.dir(normal_matrix * n),
                        uv: Some(clean2([uv.x, uv.y])),
                        color: None,
                    })?;
                    if let Some(cell) = remap.get_mut(vi) {
                        *cell = index;
                    }
                    *slot = index;
                }
                if valid {
                    self.triangle(material, out)?;
                }
            }
        }
        Ok(())
    }

    fn voxels(&mut self, world: &VoxelWorld) -> Result<(), MeshError> {
        let cell = mapping::cell_size(world.res);
        let mut material = 0;
        let mut merged: [HashMap<[i32; 3], u32>; 6] = Default::default();
        let mut pending = 0_u64;
        let mut records: [Vec<VoxelFace>; 6] = Default::default();
        let mut opened = false;

        for slot in 0..world.slots().len() {
            if !matches!(world.slots().get(slot), Some(ChunkSlot::Data(_))) {
                continue;
            }
            voxel_mesh::mesh_chunk(world, slot, &mut records);
            for (dir, faces) in Dir::ALL.into_iter().zip(&mut records) {
                for face in faces.drain(..) {
                    if !opened {
                        self.begin("Voxels", "Voxels");
                        material = self.material(None, [1.0; 4], true, "voxel");
                        opened = true;
                    }
                    if self.opts.merge_voxel_faces && face.ao() == [0; 4] {
                        pending += 1;
                        if pending > u64::from(self.opts.limits.max_vertices) {
                            return Err(MeshError::Limit("voxel faces"));
                        }
                        let [au, av, ap] = plane_axes(dir);
                        let index = global_index(face);
                        let at = |axis: usize| index.get(axis).copied().unwrap_or(0);
                        let key = [at(ap), at(av), at(au)];
                        if let Some(map) = merged.get_mut(dir as usize) {
                            map.insert(key, face.rgb());
                        }
                    } else {
                        let quad = voxel_mesh::expand_face(face, dir, cell);
                        self.quad(material, &quad)?;
                    }
                }
            }
        }

        for (dir, cells) in Dir::ALL.into_iter().zip(merged) {
            let mut keys: Vec<[i32; 3]> = cells.keys().copied().collect();
            keys.sort_unstable();
            let mut cells = cells;
            for key in keys {
                let [plane, v, u] = key;
                let Some(&rgb) = cells.get(&key) else {
                    continue;
                };
                let at = |u: i32, v: i32| [plane, v, u];
                let mut w = 1;
                while cells.get(&at(u + w, v)) == Some(&rgb) {
                    w += 1;
                }
                let mut h = 1;
                'rows: loop {
                    for du in 0..w {
                        if cells.get(&at(u + du, v + h)) != Some(&rgb) {
                            break 'rows;
                        }
                    }
                    h += 1;
                }
                for dv in 0..h {
                    for du in 0..w {
                        cells.remove(&at(u + du, v + dv));
                    }
                }
                let quad = rectangle(dir, [plane, u, v], [w, h], rgb, cell);
                self.quad(material, &quad)?;
            }
        }
        Ok(())
    }

    /// Adds the quad `(v0, v1, v2, v3)` of the mesher as two triangles.
    fn quad(&mut self, material: usize, quad: &[ExportVertex; 4]) -> Result<(), MeshError> {
        let mut index = [0_u32; 4];
        for (slot, v) in index.iter_mut().zip(quad) {
            let [r, g, b, _] = v.color;
            *slot = self.vertex(Vtx {
                pos: self.conv.pos(v.pos),
                normal: self.conv.dir(v.normal),
                uv: None,
                color: Some(clean4([r, g, b, 1.0])),
            })?;
        }
        let [a, b, c, d] = index;
        self.triangle(material, [a, b, c])?;
        self.triangle(material, [a, c, d])
    }
}

fn clean2(v: [f32; 2]) -> [f32; 2] {
    v.map(clean)
}

fn clean4(v: [f32; 4]) -> [f32; 4] {
    v.map(clean)
}

/// Whether a texture has no transparency, cutout transparency or partial alpha.
fn texture_alpha(image: &Image) -> AlphaMode {
    let mut cutout = false;
    for texel in image.rgba().chunks_exact(4) {
        match texel.get(3).copied().unwrap_or(255) {
            255 => {}
            0 => cutout = true,
            _ => return AlphaMode::Blend,
        }
    }
    if cutout {
        AlphaMode::Mask
    } else {
        AlphaMode::Opaque
    }
}

/// The normals of a mesh: the stored ones, or the area-weighted average of the adjoining
/// triangles, as the renderer computes them.
fn mesh_normals(geometry: &cuboid_model::MeshGeometry) -> Vec<Vec3> {
    let count = geometry.vertices.len();
    let mut normals = match &geometry.normals {
        Some(n) if n.len() == count => n.clone(),
        _ => {
            let mut sums = vec![Vec3::ZERO; count];
            for tri in geometry.triangles.chunks_exact(3) {
                let at = |i: u32| geometry.vertices.get(i as usize).copied();
                let [i0, i1, i2] = tri else { continue };
                if let [Some(a), Some(b), Some(c)] = [at(*i0), at(*i1), at(*i2)] {
                    let n = (b - a).cross(c - a);
                    for &i in tri {
                        if let Some(sum) = sums.get_mut(i as usize) {
                            *sum += n;
                        }
                    }
                }
            }
            sums
        }
    };
    for n in &mut normals {
        *n = n.normalize_or_zero();
    }
    normals
}

/// The axes `[u, v, plane]` of a direction as indices into `(i, j, k)`: the in-plane axis that
/// is the fast one when merging, the other in-plane axis, and the axis of the normal.
fn plane_axes(dir: Dir) -> [usize; 3] {
    match dir {
        Dir::NegY | Dir::PosY => [0, 2, 1],
        Dir::NegZ | Dir::PosZ => [0, 1, 2],
        Dir::NegX | Dir::PosX => [1, 2, 0],
    }
}

/// The voxel index `(i, j, k)` of a face record in the whole grid.
fn global_index(face: VoxelFace) -> [i32; 3] {
    let (local, chunk) = (face.local(), face.chunk());
    std::array::from_fn(|a| {
        let at = |v: [u32; 3]| v.get(a).copied().unwrap_or(0);
        i32::try_from(at(chunk) * 16 + at(local)).unwrap_or(0)
    })
}

/// The quad of a rectangle of `size` cells `(w, h)` along `(u, v)` whose first cell is at
/// `[plane, u, v]`, from the quads of its first and last cell: a vertex on the high side of an
/// in-plane axis takes the position of the last cell.
fn rectangle(
    dir: Dir,
    [plane, u, v]: [i32; 3],
    [w, h]: [i32; 2],
    rgb: u32,
    cell: f32,
) -> [ExportVertex; 4] {
    let [au, av, ap] = plane_axes(dir);
    let cell_at = |du: i32, dv: i32| {
        let mut index = [0_i32; 3];
        for (axis, value) in [(au, u + du), (av, v + dv), (ap, plane)] {
            if let Some(slot) = index.get_mut(axis) {
                *slot = value;
            }
        }
        let [i, j, k] = index.map(|c| u32::try_from(c).unwrap_or(0));
        let record = VoxelFace::new(
            [i % 16, j % 16, k % 16],
            [i / 16, j / 16, k / 16],
            rgb,
            [0; 4],
        );
        voxel_mesh::expand_face(record, dir, cell)
    };
    let first = cell_at(0, 0);
    let last = cell_at(w - 1, h - 1);
    let centre = first.iter().fold(Vec3::ZERO, |sum, v| sum + v.pos) * 0.25;
    let mut out = first;
    for (vertex, far) in out.iter_mut().zip(&last) {
        let shift = far.pos - vertex.pos;
        for axis in 0..3 {
            let d = shift[axis];
            if d.abs() > 1e-6 && (vertex.pos[axis] - centre[axis]) * d > 0.0 {
                vertex.pos[axis] += d;
            }
        }
    }
    out
}
