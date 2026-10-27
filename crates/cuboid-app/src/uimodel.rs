//! The read-only description of the document that the UI gets each frame (`cuboid_ui::DocView`),
//! built from the core. The parts that are costly to build are kept between frames and rebuilt
//! when the revision they depend on changes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use cuboid_core::{AppQuery, DisplayContext, ObjectProps, outliner};
use cuboid_model::{
    DisplaySlotId, Document, ElementFlags, FaceFlags, FaceName, ObjId, ObjRef, Rev,
    TextureAnimation, UvRect,
};
use cuboid_num::pack::{from_byte, unpack_opaque};
use cuboid_ui::{
    DisplaySlotView, DisplayView, Face, FaceView, FramesView, GroupView, HistoryView, ImageData,
    ModelView, NodeId, NodeKind, NodeView, PaletteView, PropsView, StepView, TextureView,
    UvOutline,
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

/// The display slots in the order of the Display tab, with the name the game gives each. The
/// index of a slot in the UI is its position here.
pub const DISPLAY_SLOTS: [(DisplayContext, &str); 8] = [
    (DisplayContext::ThirdRight, "thirdperson_righthand"),
    (DisplayContext::ThirdLeft, "thirdperson_lefthand"),
    (DisplayContext::FirstRight, "firstperson_righthand"),
    (DisplayContext::FirstLeft, "firstperson_lefthand"),
    (DisplayContext::Head, "head"),
    (DisplayContext::Gui, "gui"),
    (DisplayContext::Ground, "ground"),
    (DisplayContext::Fixed, "fixed"),
];

/// The face a cull direction names, when it names one.
fn cull_face(flags: FaceFlags) -> Option<Face> {
    [
        (FaceFlags::UP, Face::Up),
        (FaceFlags::DOWN, Face::Down),
        (FaceFlags::NORTH, Face::North),
        (FaceFlags::SOUTH, Face::South),
        (FaceFlags::WEST, Face::West),
        (FaceFlags::EAST, Face::East),
    ]
    .into_iter()
    .find_map(|(flag, face)| flags.contains(flag).then_some(face))
}

/// The outliner id of an object; the blueprint and the model origin have none. The two low bits
/// say the kind: 0 an element, 1 a mesh, 2 a group.
pub fn node_id(obj: ObjRef) -> Option<NodeId> {
    match obj {
        ObjRef::Element(id) => Some(NodeId(id.get() << 2)),
        ObjRef::Mesh(id) => Some(NodeId(id.get() << 2 | 1)),
        ObjRef::Group(id) => Some(NodeId(id.get() << 2 | 2)),
        ObjRef::Blueprint | ObjRef::ModelOrigin => None,
    }
}

/// The object an outliner id names, when the document still has it.
pub fn object_of(doc: &Document, node: NodeId) -> Option<ObjRef> {
    let raw = node.0 >> 2;
    let same = |id: &&ObjId| id.get() == raw;
    match node.0 & 3 {
        0 => doc
            .elements
            .ids()
            .iter()
            .find(same)
            .map(|&id| ObjRef::Element(id)),
        1 => doc
            .meshes
            .ids()
            .iter()
            .find(same)
            .map(|&id| ObjRef::Mesh(id)),
        2 => doc
            .groups
            .ids()
            .iter()
            .find(same)
            .map(|&id| ObjRef::Group(id)),
        _ => None,
    }
}

/// The position in its list of the object an outliner id names.
pub fn target_of(doc: &Document, node: NodeId) -> Option<cuboid_core::Target> {
    let raw = node.0 >> 2;
    let same = |id: &ObjId| id.get() == raw;
    match node.0 & 3 {
        0 => doc
            .elements
            .ids()
            .iter()
            .position(same)
            .map(cuboid_core::Target::Element),
        1 => doc
            .meshes
            .ids()
            .iter()
            .position(same)
            .map(cuboid_core::Target::Mesh),
        2 => doc
            .groups
            .ids()
            .iter()
            .position(same)
            .map(cuboid_core::Target::Group),
        _ => None,
    }
}

/// The Frames section of an animated texture, and what it was made from.
struct FramesCache {
    animation: TextureAnimation,
    tiles: u32,
    view: FramesView,
}

/// The picture of a texture that was made for the panels, and what it was made from.
struct Preview {
    /// The revision of the image.
    rev: u64,
    /// The rows of the image it shows.
    rows: (u32, u32),
    image: ImageData,
}

/// Everything the document view borrows from.
#[derive(Default)]
pub struct Model {
    pub title: String,
    pub path: Option<PathBuf>,
    pub nodes: Vec<NodeView>,
    pub nodes_rev: u64,
    nodes_key: Option<(Rev, Rev, Rev)>,
    pub selection: Vec<NodeId>,
    pub primary: Option<NodeId>,
    pub props: Option<PropsView>,
    pub textures: Vec<TextureView>,
    previews: HashMap<String, Preview>,
    frames: HashMap<String, FramesCache>,
    /// The texture tick the views show, which decides the frame of an animated texture the
    /// panels show.
    pub tick: u32,
    pub active_texture: usize,
    pub palette: PaletteView,
    palette_key: Option<(u64, usize)>,
    pub history: HistoryView,
    history_key: Option<(u64, usize, usize)>,
    pub status: String,
    /// The model settings of the Model tab.
    pub settings: ModelView,
    /// The display slots of the Display tab.
    pub display: DisplayView,
    /// The slot the Display tab edits.
    pub display_active: usize,
    display_key: Option<(Rev, usize)>,
    /// The UV editor shows the faces of the other elements: `uv_outlines` is kept only then.
    pub uv_outlines_wanted: bool,
    /// The faces of the elements other than the primary, for the UV editor's "All UVs".
    pub uv_outlines: Vec<UvOutline>,
    /// What `uv_outlines` was built for; `None` while it is not wanted.
    outlines_key: Option<(Rev, Option<ObjRef>)>,
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
        self.update_settings(doc);
        self.update_display(doc);
        self.update_outlines(doc, core.selection().primary);
        self.status.clone_from(&query.status);
    }

    fn update_outlines(&mut self, doc: &Document, primary: Option<ObjRef>) {
        let key = self.uv_outlines_wanted.then_some((doc.revision, primary));
        if self.outlines_key == key {
            return;
        }
        self.outlines_key = key;
        if key.is_none() {
            self.uv_outlines = Vec::new();
            return;
        }
        let others = doc.elements.ids().iter().zip(doc.elements.items());
        self.uv_outlines = others
            .filter(|(id, _)| primary != Some(ObjRef::Element(**id)))
            .flat_map(|(_, element)| element.faces.iter().filter(|face| face.active))
            .filter_map(|face| {
                let texture = self.textures.iter().find(|t| *t.label == *face.texture)?;
                let [w, h] = texture.size;
                Some(UvOutline {
                    texture: texture.label.clone(),
                    uv: texels(face.uv, [w as f32, h as f32]),
                })
            })
            .collect();
    }

    fn update_nodes(&mut self, doc: &Document, sel: &cuboid_model::Selection) {
        let key = (doc.elements.rev(), doc.meshes.rev(), doc.groups.rev());
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
                    ObjRef::Group(_) => NodeKind::Group,
                    _ => NodeKind::Element,
                };
                Some(NodeView {
                    id,
                    parent: row.parent.and_then(node_id),
                    name: Arc::from(row.label.as_str()),
                    kind,
                    visible: !row.hidden,
                    locked: row.locked,
                    hidden_above: row.hidden_by_group,
                })
            })
            .collect();
    }

    fn update_textures(&mut self, doc: &Document) {
        let entries = &doc.textures.entries;
        let mut views = Vec::with_capacity(entries.len());
        let mut keep = HashMap::with_capacity(entries.len());
        let mut frames = HashMap::new();
        for entry in entries {
            let image = &entry.texture.image;
            let tiles = entry.animation.as_ref().map(|a| a.tiles(image.h));
            // What the views show of an animated texture is the frame the texture tick is at.
            let shown = entry
                .animation
                .as_ref()
                .zip(tiles)
                .map(|(a, tiles)| (a, tiles, a.shown(tiles, self.tick)));
            let rows = shown.map_or((0, image.h), |(a, _, shown)| {
                (a.tile_rows(image.h, shown.tile).start, a.frame_height)
            });
            let rev = image.rev.get();
            let key = entry.label.to_string();
            let preview = match self.previews.remove(&key) {
                Some(old) if old.rev == rev && old.rows == rows => old,
                _ => Preview {
                    rev,
                    rows,
                    image: preview_of(image, rows),
                },
            };
            // The steps are listed again only when the animation or the step shown changes.
            let animation = shown.map(|(a, tiles, shown)| {
                let cache = match self.frames.remove(&key) {
                    Some(c)
                        if c.animation == *a && c.tiles == tiles && c.view.shown == shown.step =>
                    {
                        c
                    }
                    _ => FramesCache {
                        animation: a.clone(),
                        tiles,
                        view: frames_view(a, tiles, shown.step),
                    },
                };
                let view = cache.view.clone();
                frames.insert(key.clone(), cache);
                view
            });
            views.push(TextureView {
                label: Arc::from(&*entry.label),
                size: entry.frame_size().into(),
                image: Some(preview.image.clone()),
                source: entry.texture.source.path.as_deref().map(Arc::from),
                animation,
                animatable: (entry.animation.is_none()
                    && image.w > 0
                    && image.h.is_multiple_of(image.w))
                .then_some(image.w),
            });
            keep.insert(key, preview);
        }
        self.previews = keep;
        self.frames = frames;
        self.textures = views;
        self.active_texture = self
            .active_texture
            .min(self.textures.len().saturating_sub(1));
    }

    fn update_palette(&mut self, doc: &Document, left: [f32; 4], right: [f32; 4]) {
        self.palette.left = left;
        self.palette.right = right;
        // The palette changes without a write when a swatch is added or removed.
        let key = doc.voxels.as_ref().map_or((0, 0), |world| {
            (world.rev.get(), world.palette.values().len())
        });
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

    fn update_settings(&mut self, doc: &Document) {
        let model_box = doc.model_box;
        self.settings = ModelView {
            model_box: [model_box.min, model_box.max],
            origin: doc.props.general.origin,
            blueprint: doc.blueprint.base.p,
            voxel_resolution: doc.voxels.as_ref().map(|world| world.res),
        };
    }

    fn update_display(&mut self, doc: &Document) {
        self.display_active = self.display_active.min(DISPLAY_SLOTS.len() - 1);
        let key = (doc.revision, self.display_active);
        if self.display_key == Some(key) {
            return;
        }
        self.display_key = Some(key);
        let slots: Vec<DisplaySlotView> = DISPLAY_SLOTS
            .iter()
            .map(|(context, name)| {
                let slot = &doc.display[DisplaySlotId::from(*context)];
                DisplaySlotView {
                    name: Arc::from(*name),
                    enabled: slot.enabled,
                    preset: Arc::from(slot.preset.as_str()),
                    rotation: slot.rotation,
                    translation: slot.translation,
                    scale: slot.scale,
                }
            })
            .collect();
        self.display = DisplayView {
            slots: Arc::from(slots),
            active: self.display_active,
        };
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

/// How an animated texture plays, as the Frames section shows it.
fn frames_view(animation: &TextureAnimation, tiles: u32, shown: usize) -> FramesView {
    let own = |step: usize| {
        let frames = animation.frames.as_ref();
        frames
            .and_then(|steps| steps.get(step))
            .and_then(|s| s.time)
    };
    FramesView {
        frame_height: animation.frame_height,
        tiles,
        frametime: animation.frametime,
        interpolate: animation.interpolate,
        steps: (0..animation.steps(tiles))
            .map(|step| StepView {
                tile: animation.step_tile(tiles, step),
                time: own(step),
            })
            .collect(),
        explicit: animation.frames.is_some(),
        shown,
    }
}

/// The picture of the rows `(first, count)` of a texture for the panels: rows top to bottom,
/// reduced when large. The rows are the whole picture, or one frame of a strip.
fn preview_of(image: &cuboid_model::image::Image, (first, rows): (u32, u32)) -> ImageData {
    let (w, h, first) = (image.w as usize, rows as usize, first as usize);
    let src = image.rgba();
    let step = (image.w.max(rows) / PREVIEW_EDGE + 1) as usize;
    let (pw, ph) = (w.div_ceil(step).max(1), h.div_ceil(step).max(1));
    let mut rgba = vec![0_u8; pw * ph * 4];
    if src.len() >= (first + h) * w * 4 {
        for y in 0..ph {
            // The image stores rows bottom to top.
            let sy = first + h - 1 - (y * step).min(h - 1);
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
        rev: Rev::next().get(),
    }
}

/// The size of the texture a label names, of one frame for an animated texture; 16 x 16 when it
/// is not in the list.
pub fn texture_size(doc: &Document, label: &str) -> [f32; 2] {
    doc.textures
        .entries
        .iter()
        .find(|entry| &*entry.label == label)
        .map_or([16.0, 16.0], |entry| {
            let (w, h) = entry.frame_size();
            [w as f32, h as f32]
        })
}

/// A UV rectangle as the UI has it: in texels of a texture of this size, rows from the top.
fn texels(uv: UvRect, [w, h]: [f32; 2]) -> [f32; 4] {
    let (a, b) = (uv.tl(), uv.br());
    [a.x * w, (1.0 - a.y) * h, b.x * w, (1.0 - b.y) * h]
}

fn build_props(doc: &Document, props: &ObjectProps, selected: usize) -> PropsView {
    let (kind, element) = match props.obj {
        ObjRef::Element(id) => (NodeKind::Element, doc.elements.get(id)),
        ObjRef::Mesh(_) => (NodeKind::Mesh, None),
        ObjRef::Group(_) | ObjRef::Blueprint | ObjRef::ModelOrigin => (NodeKind::Group, None),
    };
    let group = match props.obj {
        ObjRef::Group(id) => doc.groups.get(id).map(|g| GroupView {
            visible: g.visible,
            locked: g.locked,
            export: g.export,
            children: g.children().len(),
        }),
        _ => None,
    };
    let faces = element.map(|el| {
        FACES.map(|(_, name)| {
            let face = &el.faces[name as usize];
            FaceView {
                enabled: face.active,
                texture: Arc::from(&*face.texture),
                uv: texels(face.uv, texture_size(doc, &face.texture)),
                tint: face.tint,
                rotation: face.uv.rot(),
                cull: cull_face(face.cull),
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
        group,
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
    fn groups_have_ids_and_parents_in_the_outline() {
        let mut editor = Editor::new(1252);
        for _ in 0..3 {
            assert!(
                editor
                    .core
                    .execute(Command::AddElement(ElementKind::Cube))
                    .is_done()
            );
        }
        let group = Command::Group {
            targets: vec![
                cuboid_core::Target::Element(0),
                cuboid_core::Target::Element(1),
            ],
            name: "arm".to_owned(),
        };
        assert!(editor.core.execute(group).is_done());
        let doc = editor.doc();
        let group_id = doc.groups.ids()[0];
        let group_node = node_id(ObjRef::Group(group_id)).unwrap();
        assert_eq!(object_of(doc, group_node), Some(ObjRef::Group(group_id)));
        assert_eq!(
            target_of(doc, group_node),
            Some(cuboid_core::Target::Group(0))
        );
        // An element and a group of the same raw id are different nodes.
        assert_ne!(
            node_id(ObjRef::Element(doc.elements.ids()[0])),
            Some(group_node)
        );

        let mut model = Model::default();
        model.update_nodes(doc, editor.core.selection());
        let row = |obj| {
            let id = node_id(obj).unwrap();
            model.nodes.iter().find(|node| node.id == id).unwrap()
        };
        assert_eq!(row(ObjRef::Group(group_id)).kind, NodeKind::Group);
        assert_eq!(row(ObjRef::Group(group_id)).parent, None);
        for at in 0..2 {
            let child = row(ObjRef::Element(doc.elements.ids()[at]));
            assert_eq!(child.parent, Some(group_node));
        }
        assert_eq!(row(ObjRef::Element(doc.elements.ids()[2])).parent, None);
    }

    #[test]
    fn the_preview_has_its_rows_top_to_bottom_and_stays_small() {
        let mut buf = cuboid_model::image::RgbaBuf::new(2, 2);
        // Row 0 is the bottom row.
        buf.set(0, 0, [1, 0, 0, 255]);
        buf.set(0, 1, [2, 0, 0, 255]);
        let image = cuboid_model::image::Image::from_rgba(buf);
        let preview = preview_of(&image, (0, 2));
        assert_eq!(preview.size, [2, 2]);
        assert_eq!(preview.rgba[0], 2, "the top row comes first");
        // One row of the picture is a picture of its own.
        let row = preview_of(&image, (0, 1));
        assert_eq!((row.size, row.rgba[0]), ([2, 1], 1));
        let big =
            cuboid_model::image::Image::from_rgba(cuboid_model::image::RgbaBuf::new(2048, 1024));
        let preview = preview_of(&big, (0, 1024));
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
