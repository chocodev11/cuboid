//! The read-only description of the document that the UI gets each frame (`cuboid_ui::DocView`),
//! built from the core. The parts that are costly to build are kept between frames and rebuilt
//! when the revision they depend on changes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use cuboid_core::{AppQuery, ObjectProps, outliner};
use cuboid_model::{Document, ElementFlags, FaceName, ObjId, ObjRef, Rev};
use cuboid_num::pack::{from_byte, unpack_opaque};
use cuboid_ui::{
    Face, FaceView, HistoryView, ImageData, NodeId, NodeKind, NodeView, PaletteView, PropsView,
    TextureView,
};
use glam::Vec3;

use crate::state::State;

/// Longest edge of a texture preview; larger textures are shown reduced.
const PREVIEW_EDGE: u32 = 512;

/// The faces in the order of the UI, with the slot of the document each one is.
const FACES: [(Face, FaceName); 6] = [
    (Face::Up, FaceName::Up),
    (Face::Down, FaceName::Down),
    (Face::North, FaceName::North),
    (Face::South, FaceName::South),
    (Face::West, FaceName::West),
    (Face::East, FaceName::East),
];

/// The outliner id of an object; the blueprint and the model origin have none.
pub fn node_id(obj: ObjRef) -> Option<NodeId> {
    match obj {
        ObjRef::Element(id) => Some(NodeId(id.get() << 1)),
        ObjRef::Mesh(id) => Some(NodeId(id.get() << 1 | 1)),
        ObjRef::Blueprint | ObjRef::ModelOrigin => None,
    }
}

/// The position in its list of the object an outliner id names.
pub fn target_of(doc: &Document, node: NodeId) -> Option<cuboid_core::Target> {
    let raw = node.0 >> 1;
    let same = |id: &ObjId| id.get() == raw;
    if node.0 & 1 == 0 {
        doc.elements
            .ids()
            .iter()
            .position(same)
            .map(cuboid_core::Target::Element)
    } else {
        doc.meshes
            .ids()
            .iter()
            .position(same)
            .map(cuboid_core::Target::Mesh)
    }
}

/// Everything the document view borrows from.
#[derive(Default)]
pub struct Model {
    pub title: String,
    pub path: Option<PathBuf>,
    pub nodes: Vec<NodeView>,
    pub nodes_rev: u64,
    nodes_key: Option<(Rev, Rev)>,
    pub selection: Vec<NodeId>,
    pub primary: Option<NodeId>,
    pub props: Option<PropsView>,
    pub textures: Vec<TextureView>,
    previews: HashMap<String, ImageData>,
    pub active_texture: usize,
    pub palette: PaletteView,
    palette_key: Option<u64>,
    pub history: HistoryView,
    history_key: Option<(u64, usize, usize)>,
    pub status: String,
}

impl Model {
    pub fn update(&mut self, state: &mut State, query: &AppQuery) {
        self.title = state.editor.name();
        self.path = state.editor.path();
        let colors = *state.editor.core.colors_mut();
        let core = &state.editor.core;
        let doc = core.doc();
        self.update_nodes(doc, core.selection());
        self.selection = core
            .selection()
            .list
            .iter()
            .copied()
            .filter_map(node_id)
            .collect();
        self.primary = core.selection().primary.and_then(node_id);
        self.props = query
            .primary_props
            .as_ref()
            .map(|props| build_props(doc, props, query.selection_count));
        self.update_textures(doc);
        self.update_palette(doc, colors.left, colors.right);
        self.update_history(core, doc.revision.get());
        self.status.clone_from(&query.status);
    }

    fn update_nodes(&mut self, doc: &Document, sel: &cuboid_model::Selection) {
        let key = (doc.elements.rev(), doc.meshes.rev());
        if self.nodes_key == Some(key) {
            return;
        }
        self.nodes_key = Some(key);
        self.nodes_rev += 1;
        self.nodes = outliner(doc, sel)
            .into_iter()
            .filter_map(|row| {
                let id = node_id(row.obj)?;
                let kind = match row.obj {
                    ObjRef::Mesh(_) => NodeKind::Mesh,
                    _ => NodeKind::Element,
                };
                Some(NodeView {
                    id,
                    parent: None,
                    name: Arc::from(row.label.as_str()),
                    kind,
                    visible: !row.hidden,
                    locked: row.locked,
                })
            })
            .collect();
    }

    fn update_textures(&mut self, doc: &Document) {
        let entries = &doc.textures.entries;
        let mut views = Vec::with_capacity(entries.len());
        let mut keep = HashMap::with_capacity(entries.len());
        for entry in entries {
            let image = &entry.texture.image;
            let rev = image.rev.get();
            let key = entry.label.to_string();
            let preview = match self.previews.remove(&key) {
                Some(old) if old.rev == rev => old,
                _ => preview_of(image),
            };
            views.push(TextureView {
                label: Arc::from(&*entry.label),
                size: [image.w, image.h],
                image: Some(preview.clone()),
            });
            keep.insert(key, preview);
        }
        self.previews = keep;
        self.textures = views;
        self.active_texture = self
            .active_texture
            .min(self.textures.len().saturating_sub(1));
    }

    fn update_palette(&mut self, doc: &Document, left: [f32; 4], right: [f32; 4]) {
        self.palette.left = left;
        self.palette.right = right;
        let key = doc.voxels.as_ref().map_or(0, |world| world.rev.get());
        if self.palette_key == Some(key) {
            return;
        }
        self.palette_key = Some(key);
        let colors: Vec<[f32; 4]> = doc.voxels.as_ref().map_or_else(Vec::new, |world| {
            world
                .palette
                .values()
                .iter()
                .skip(1)
                .map(|&v| unpack_opaque(v).map(from_byte))
                .collect()
        });
        self.palette.name = Arc::from("Voxel palette");
        self.palette.colors = Arc::from(colors);
    }

    fn update_history(&mut self, core: &cuboid_core::Core, revision: u64) {
        let hist = core.history();
        let key = (revision, hist.undo_len(), hist.redo_len());
        if self.history_key == Some(key) {
            return;
        }
        self.history_key = Some(key);
        let labels: Vec<Arc<str>> = hist
            .undo_labels()
            .chain(hist.redo_labels())
            .map(Arc::from)
            .collect();
        self.history = HistoryView {
            labels: Arc::from(labels),
            cursor: hist.undo_len(),
        };
    }
}

/// The picture of a texture for the panels: rows top to bottom, reduced when large.
fn preview_of(image: &cuboid_model::image::Image) -> ImageData {
    let (w, h) = (image.w as usize, image.h as usize);
    let src = image.rgba();
    let step = (image.w.max(image.h) / PREVIEW_EDGE + 1) as usize;
    let (pw, ph) = (w.div_ceil(step).max(1), h.div_ceil(step).max(1));
    let mut rgba = vec![0_u8; pw * ph * 4];
    if src.len() >= w * h * 4 {
        for y in 0..ph {
            // The image stores rows bottom to top.
            let sy = h - 1 - (y * step).min(h - 1);
            for x in 0..pw {
                let sx = (x * step).min(w - 1);
                let from = (sy * w + sx) * 4;
                let to = (y * pw + x) * 4;
                rgba[to..to + 4].copy_from_slice(&src[from..from + 4]);
            }
        }
    }
    ImageData {
        size: [pw, ph],
        rgba: Arc::from(rgba),
        rev: image.rev.get(),
    }
}

/// The size of the texture a label names; 16 x 16 when it is not in the list.
pub fn texture_size(doc: &Document, label: &str) -> [f32; 2] {
    doc.textures
        .entries
        .iter()
        .find(|entry| &*entry.label == label)
        .map_or([16.0, 16.0], |entry| {
            [entry.texture.image.w as f32, entry.texture.image.h as f32]
        })
}

fn build_props(doc: &Document, props: &ObjectProps, selected: usize) -> PropsView {
    let (kind, element) = match props.obj {
        ObjRef::Element(id) => (NodeKind::Element, doc.elements.get(id)),
        ObjRef::Mesh(_) => (NodeKind::Mesh, None),
        ObjRef::Blueprint | ObjRef::ModelOrigin => (NodeKind::Group, None),
    };
    let faces = element.map(|el| {
        FACES.map(|(_, name)| {
            let face = &el.faces[name as usize];
            let [w, h] = texture_size(doc, &face.texture);
            let (a, b) = (face.uv.tl(), face.uv.br());
            FaceView {
                enabled: face.active,
                texture: Arc::from(&*face.texture),
                uv: [a.x * w, (1.0 - a.y) * h, b.x * w, (1.0 - b.y) * h],
                tint: face.tint,
            }
        })
    });
    PropsView {
        kind,
        name: Arc::from(props.label.as_str()),
        selected,
        position: props.position,
        size: props.size,
        rotation: props.rotation,
        pivot: props.pivot,
        color: props.color.unwrap_or([1.0; 4]),
        shade: element.is_some_and(|el| el.flags.contains(ElementFlags::SHADE)),
        uv_snap: props.uv_snap.unwrap_or(false),
        faces,
    }
}

/// The vector of the properties panel as the commands take it.
pub fn components(value: Vec3) -> [Option<f32>; 3] {
    [Some(value.x), Some(value.y), Some(value.z)]
}

#[cfg(test)]
mod tests {
    use cuboid_core::{Command, ElementKind};

    use super::*;
    use crate::editor::Editor;

    #[test]
    fn node_ids_find_their_objects_again() {
        let mut editor = Editor::new(1252);
        for _ in 0..2 {
            assert!(
                editor
                    .core
                    .execute(Command::AddElement(ElementKind::Cube))
                    .is_done()
            );
        }
        let doc = editor.doc();
        for (at, id) in doc.elements.ids().iter().enumerate() {
            let node = node_id(ObjRef::Element(*id)).unwrap();
            assert_eq!(target_of(doc, node), Some(cuboid_core::Target::Element(at)));
        }
        assert!(node_id(ObjRef::Blueprint).is_none());
    }

    #[test]
    fn the_preview_has_its_rows_top_to_bottom_and_stays_small() {
        let mut buf = cuboid_model::image::RgbaBuf::new(2, 2);
        // Row 0 is the bottom row.
        buf.set(0, 0, [1, 0, 0, 255]);
        buf.set(0, 1, [2, 0, 0, 255]);
        let image = cuboid_model::image::Image::from_rgba(buf);
        let preview = preview_of(&image);
        assert_eq!(preview.size, [2, 2]);
        assert_eq!(preview.rgba[0], 2, "the top row comes first");
        let big =
            cuboid_model::image::Image::from_rgba(cuboid_model::image::RgbaBuf::new(2048, 1024));
        let preview = preview_of(&big);
        assert!(preview.size[0] <= 1024 && preview.size[1] <= 512);
        assert_eq!(preview.rgba.len(), preview.size[0] * preview.size[1] * 4);
    }

    #[test]
    fn a_new_model_has_textures_and_no_properties() {
        let dir = tempfile::tempdir().unwrap();
        let settings = cuboid_settings::Settings::first_run(&cuboid_settings::Environment {
            desktop_dir: String::new(),
            minecraft_dir: String::new(),
        });
        let mut state = State::new(
            crate::platform::Dirs::under(dir.path()),
            settings,
            crate::io_thread::IoThread::spawn().unwrap(),
            1252,
            std::time::Instant::now(),
        );
        let query = state.editor.core.query();
        let mut model = Model::default();
        model.update(&mut state, &query);
        assert!(model.props.is_none());
        assert!(!model.textures.is_empty());
        assert_eq!(model.title, "Untitled");
    }
}
