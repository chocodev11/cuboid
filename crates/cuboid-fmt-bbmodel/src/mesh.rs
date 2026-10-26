//! Meshes of a free-format project: faces of three or more vertices with a UV point per vertex.

use std::collections::HashMap;
use std::sync::Arc;

use cuboid_model::mc::{self, McBox, Placed};
use cuboid_model::{
    Mesh, MeshBounds, MeshGeometry, MeshKind, ObjId, ObjRef, ObjectBase, Perms, Submesh,
};
use glam::{Vec2, Vec3};
use serde_json::Value;

use crate::BbError;
use crate::import::{Ctx, Importer, TexRef, uuid_of};
use crate::json::{flag, list, num, text, vec3_at};

/// The most vertices one mesh of the model gets; a larger mesh is split.
const MAX_VERTICES: usize = 65_534;

/// The texture list entry a triangle uses, if any.
type Slot = Option<usize>;

/// The vertices and triangles of one mesh being built.
#[derive(Default)]
struct Chunk {
    vertices: Vec<Vec3>,
    uvs: Vec<Vec2>,
    triangles: Vec<(Slot, [u32; 3])>,
}

impl Importer<'_> {
    /// Reads a mesh element into one mesh, or several when it has too many vertices.
    pub(crate) fn mesh(&mut self, item: &Value, ctx: &Ctx) -> Result<(), BbError> {
        let label = self.label(text(item, "name"), "mesh")?;

        let mut points: HashMap<&str, Vec3> = HashMap::new();
        if let Some(Value::Object(vertices)) = item.get("vertices") {
            for (key, value) in vertices {
                let Some([x, y, z]) = value
                    .as_array()
                    .and_then(|a| a.first_chunk::<3>())
                    .map(|a| a.each_ref().map(|v| num(v).unwrap_or(0.0)))
                else {
                    continue;
                };
                points.insert(key, Vec3::new(x, y, z));
                self.count_mesh_items(1)?;
            }
        }

        let mut chunks = vec![Chunk::default()];
        if let Some(Value::Object(faces)) = item.get("faces") {
            for face in faces.values() {
                let keys: Vec<&str> = list(face, "vertices")
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|key| points.contains_key(key))
                    .collect();
                let count = keys.len();
                if count < 3 {
                    continue;
                }
                self.count_mesh_items(count + 3 * (count - 2))?;
                let slot = match self.texture_of(face.get("texture")) {
                    TexRef::Index(index) => Some(index),
                    TexRef::Untextured | TexRef::Off => None,
                };
                let units = match slot {
                    Some(index) if self.space.per_texture => self
                        .table
                        .units
                        .get(index)
                        .copied()
                        .unwrap_or(self.space.project),
                    _ => self.space.project,
                };
                if chunks
                    .last()
                    .is_some_and(|c| c.vertices.len() + count > MAX_VERTICES)
                {
                    chunks.push(Chunk::default());
                }
                let Some(chunk) = chunks.last_mut() else {
                    continue;
                };
                let first = u32::try_from(chunk.vertices.len()).unwrap_or(0);
                for key in &keys {
                    let p = points.get(key).copied().unwrap_or(Vec3::ZERO);
                    chunk.vertices.push(Vec3::new(p.x, p.y, -p.z));
                    let uv = face
                        .get("uv")
                        .and_then(|uv| uv.get(*key))
                        .and_then(Value::as_array);
                    let at = |i: usize| uv.and_then(|a| a.get(i)).and_then(num).unwrap_or(0.0);
                    chunk
                        .uvs
                        .push(Vec2::new(at(0) / units.x, 1.0 - at(1) / units.y));
                }
                // A fan from the first vertex; the mirror to the model's space swaps the
                // winding.
                for k in 1..count - 1 {
                    let k = u32::try_from(k).unwrap_or(0);
                    chunk
                        .triangles
                        .push((slot, [first, first + k + 1, first + k]));
                }
            }
        }
        chunks.retain(|c| !c.triangles.is_empty());
        if chunks.is_empty() {
            *self
                .skipped
                .entry("meshes without faces".to_owned())
                .or_default() += 1;
            return Ok(());
        }

        let origin = vec3_at(item, "origin", Vec3::ZERO) + self.shift;
        let placed = mc::place(
            &McBox {
                from: origin,
                to: origin,
                pivot: origin,
                rotation: vec3_at(item, "rotation", Vec3::ZERO),
            },
            ctx.outer,
        );
        let hidden = flag(item, "visibility") == Some(false);
        let several = chunks.len() > 1;
        for (index, chunk) in chunks.into_iter().enumerate() {
            if self.elements.len() + self.meshes.len() >= self.limits.max_objects as usize {
                return Err(BbError::Limit("elements"));
            }
            let label = if several {
                format!("{label}_part{}", index + 1)
            } else {
                label.clone()
            };
            let mesh = self.make_mesh(label, &placed, chunk, hidden);
            let id = ObjId::next();
            self.mesh_ids.push(id);
            self.meshes.push(mesh);
            // The parts of a split mesh have no single uuid to keep.
            let uuid = if several { None } else { uuid_of(item) };
            self.attach(ObjRef::Mesh(id), uuid, ctx)?;
        }
        Ok(())
    }

    fn count_mesh_items(&mut self, more: usize) -> Result<(), BbError> {
        self.mesh_items = self.mesh_items.saturating_add(more as u64);
        if self.mesh_items > u64::from(self.limits.max_mesh_items) {
            return Err(BbError::Limit("mesh data"));
        }
        Ok(())
    }

    fn make_mesh(&self, label: String, placed: &Placed, chunk: Chunk, hidden: bool) -> Mesh {
        // The triangles by texture, in the order the textures first appear.
        let mut groups: Vec<(Slot, Vec<u32>)> = Vec::new();
        for (slot, triangle) in chunk.triangles {
            let at = match groups.iter().position(|(s, _)| *s == slot) {
                Some(at) => at,
                None => {
                    groups.push((slot, Vec::new()));
                    groups.len() - 1
                }
            };
            if let Some((_, indices)) = groups.get_mut(at) {
                indices.extend_from_slice(&triangle);
            }
        }
        let texture_of = |slot: Slot| {
            slot.and_then(|i| self.table.entries.get(i))
                .map(|entry| entry.texture.clone())
        };
        let triangles: Vec<u32> = groups.iter().flat_map(|(_, t)| t.iter().copied()).collect();
        let (texture, submeshes) = match groups.as_slice() {
            [(slot, _)] => (texture_of(*slot), Vec::new()),
            _ => (
                None,
                groups
                    .iter()
                    .map(|(slot, triangles)| Submesh {
                        triangles: triangles.clone(),
                        color: [1.0; 4],
                        texture: texture_of(*slot),
                    })
                    .collect(),
            ),
        };

        let (min, max) = chunk
            .vertices
            .iter()
            .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        let bounds = MeshBounds {
            center: (min + max) * 0.5,
            size: max - min,
        };

        let mut base = ObjectBase::new(label, placed.p, Vec3::ONE, Perms::IMPORTED_MESH);
        base.o = placed.p;
        base.e = placed.e;
        base.hidden = hidden;
        Mesh {
            base,
            kind: MeshKind::Custom,
            geometry: Arc::new(MeshGeometry {
                vertices: chunk.vertices,
                normals: None,
                uvs: chunk.uvs,
                triangles,
            }),
            texture,
            submeshes: submeshes.into(),
            bounds,
            norm_factor: 1.0,
            import_offset: Vec3::ZERO,
            tint: [1.0; 4],
        }
    }
}
