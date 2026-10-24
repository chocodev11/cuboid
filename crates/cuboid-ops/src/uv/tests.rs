use cuboid_model::{
    Document, EditEnv, ElementType, ObjId, RenderPrefs, Selection, TextureList, uv::UvRect,
};
use cuboid_num::EngineRng;
use glam::Vec3;

use super::*;

struct Rig {
    doc: Document,
    sel: Selection,
    env: EditEnv,
    rng: EngineRng,
}

impl Rig {
    fn new() -> Self {
        Rig {
            doc: Document::new(TextureList::default()),
            sel: Selection::default(),
            env: EditEnv {
                restrict_rotations: true,
                render: RenderPrefs::default(),
                code_page: 1252,
            },
            rng: EngineRng::seeded(5),
        }
    }

    fn cx(&mut self) -> EditCtx<'_> {
        EditCtx {
            doc: &mut self.doc,
            sel: &mut self.sel,
            env: &self.env,
            rng: &mut self.rng,
        }
    }

    /// A cube of size 4 at (4, 8, -2) with its default mapping applied.
    fn cube(&mut self) -> (ObjId, [ObjRef; 1]) {
        let id = crate::primitives::add_element(&mut self.cx(), ElementType::Cube);
        let doc = &mut self.doc;
        let el = doc
            .elements
            .edit(id, &mut doc.journal)
            .expect("the element");
        el.base.p = Vec3::new(4.0, 8.0, -2.0);
        el.base.s = Vec3::splat(4.0);
        (id, [ObjRef::Element(id)])
    }

    fn uv(&self, id: ObjId, face: FaceName) -> UvRect {
        self.doc.elements.get(id).expect("the element").faces[face as usize].uv
    }

    fn flags(&self, id: ObjId) -> ElementFlags {
        self.doc.elements.get(id).expect("the element").flags
    }
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

#[test]
fn box_style_maps_the_faces_by_their_place_in_the_block() {
    let mut rig = Rig::new();
    let (id, objs) = rig.cube();
    assert_eq!(auto_uv(&mut rig.cx(), &objs, &[], AutoUvStyle::Box), 1);
    let rects = default_uv_rects(Vec3::new(4.0, 8.0, -2.0), Vec3::splat(4.0), false);
    for name in FaceName::ALL {
        assert_eq!(rig.uv(id, name), rects[name as usize], "{name:?}");
    }
    assert!(rig.flags(id).contains(ElementFlags::UV_SNAP), "snap stays");
    // Nothing left to do.
    let rev = rig.doc.revision;
    assert_eq!(auto_uv(&mut rig.cx(), &objs, &[], AutoUvStyle::Box), 0);
    assert_eq!(rig.doc.revision, rev);
}

#[test]
fn box_style_can_take_single_faces() {
    let mut rig = Rig::new();
    let (id, objs) = rig.cube();
    let full = UvRect::default();
    let n = auto_uv(&mut rig.cx(), &objs, &[FaceName::Up], AutoUvStyle::Box);
    assert_eq!(n, 1);
    assert_ne!(rig.uv(id, FaceName::Up), full);
    assert_eq!(rig.uv(id, FaceName::Down), full, "other faces stay");
}

#[test]
fn per_face_style_covers_the_whole_texture_and_turns_snap_off() {
    let mut rig = Rig::new();
    let (id, objs) = rig.cube();
    auto_uv(&mut rig.cx(), &objs, &[], AutoUvStyle::Box);
    rotate(&mut rig.cx(), &objs, &[FaceName::North], 90);
    assert_eq!(auto_uv(&mut rig.cx(), &objs, &[], AutoUvStyle::PerFace), 1);
    for name in FaceName::ALL {
        let uv = rig.uv(id, name);
        assert_eq!((uv.tl(), uv.br()), (v(0.0, 0.0), v(1.0, 1.0)), "{name:?}");
    }
    assert_eq!(rig.uv(id, FaceName::North).rot(), 90, "rotation is kept");
    assert!(!rig.flags(id).contains(ElementFlags::UV_SNAP));
}

#[test]
fn rotate_turns_in_quarter_turns_and_wraps() {
    let mut rig = Rig::new();
    let (id, objs) = rig.cube();
    let rot = |rig: &Rig| rig.uv(id, FaceName::Up).rot();
    rotate(&mut rig.cx(), &objs, &[FaceName::Up], 90);
    assert_eq!(rot(&rig), 90);
    rotate(&mut rig.cx(), &objs, &[FaceName::Up], 270);
    assert_eq!(rot(&rig), 0);
    rotate(&mut rig.cx(), &objs, &[FaceName::Up], -90);
    assert_eq!(rot(&rig), 270);
    assert_eq!(rig.uv(id, FaceName::Down).rot(), 0);
    assert!(rig.flags(id).contains(ElementFlags::UV_SNAP), "snap stays");
    assert_eq!(rotate(&mut rig.cx(), &objs, &[], 0), 0);
    assert_eq!(rotate(&mut rig.cx(), &objs, &[], 360), 0);
    // No overflow on silly input.
    rotate(&mut rig.cx(), &objs, &[], i32::MAX);
    rotate(&mut rig.cx(), &objs, &[], i32::MIN);
}

#[test]
fn flip_swaps_the_corner_components_and_twice_is_the_identity() {
    let mut rig = Rig::new();
    let (id, objs) = rig.cube();
    set_rect(
        &mut rig.cx(),
        &objs,
        &[FaceName::South],
        v(0.25, 0.5),
        v(0.75, 1.0),
        None,
    );
    let start = rig.uv(id, FaceName::South);
    flip(
        &mut rig.cx(),
        &objs,
        &[FaceName::South],
        FlipAxis::Horizontal,
    );
    let uv = rig.uv(id, FaceName::South);
    assert_eq!((uv.tl(), uv.br()), (v(0.75, 0.5), v(0.25, 1.0)));
    flip(
        &mut rig.cx(),
        &objs,
        &[FaceName::South],
        FlipAxis::Horizontal,
    );
    assert_eq!(rig.uv(id, FaceName::South), start);
    flip(&mut rig.cx(), &objs, &[FaceName::South], FlipAxis::Vertical);
    let uv = rig.uv(id, FaceName::South);
    assert_eq!((uv.tl(), uv.br()), (v(0.25, 1.0), v(0.75, 0.5)));
}

#[test]
fn a_numeric_rectangle_is_clamped_and_turns_snap_off() {
    let mut rig = Rig::new();
    let (id, objs) = rig.cube();
    let n = set_rect(
        &mut rig.cx(),
        &objs,
        &[FaceName::East],
        v(-5.0, 0.25),
        v(0.5, 9.0),
        Some(180),
    );
    assert_eq!(n, 1);
    let uv = rig.uv(id, FaceName::East);
    assert_eq!(
        (uv.tl_raw(), uv.br_raw(), uv.rot()),
        (v(-1.0, 0.25), v(0.5, 2.0), 180)
    );
    assert!(!rig.flags(id).contains(ElementFlags::UV_SNAP));
    // Without a rotation the old one stays.
    set_rect(
        &mut rig.cx(),
        &objs,
        &[FaceName::East],
        v(0.0, 0.0),
        v(0.5, 0.5),
        None,
    );
    assert_eq!(rig.uv(id, FaceName::East).rot(), 180);
}

#[test]
fn snap_on_maps_the_faces_at_once_and_off_leaves_them() {
    let mut rig = Rig::new();
    let (id, objs) = rig.cube();
    set_snap(&mut rig.cx(), &objs, SnapOption::Snap, false);
    assert!(!rig.flags(id).contains(ElementFlags::UV_SNAP));
    assert_eq!(rig.uv(id, FaceName::Up), UvRect::default());
    set_snap(&mut rig.cx(), &objs, SnapOption::Snap, true);
    assert!(rig.flags(id).contains(ElementFlags::UV_SNAP));
    let rects = default_uv_rects(Vec3::new(4.0, 8.0, -2.0), Vec3::splat(4.0), false);
    assert_eq!(rig.uv(id, FaceName::Up), rects[1]);
    // The mirror option changes north and east.
    set_snap(&mut rig.cx(), &objs, SnapOption::Mirror, true);
    let mirrored = default_uv_rects(Vec3::new(4.0, 8.0, -2.0), Vec3::splat(4.0), true);
    assert_eq!(rig.uv(id, FaceName::North), mirrored[2]);
    assert_ne!(rig.uv(id, FaceName::North), rects[2]);
    assert_eq!(set_snap(&mut rig.cx(), &objs, SnapOption::Mirror, true), 0);
}

#[test]
fn only_elements_are_touched() {
    let mut rig = Rig::new();
    let rev = rig.doc.revision;
    let objs = [
        ObjRef::Blueprint,
        ObjRef::ModelOrigin,
        ObjRef::Mesh(ObjId::next()),
    ];
    assert_eq!(auto_uv(&mut rig.cx(), &objs, &[], AutoUvStyle::PerFace), 0);
    assert_eq!(flip(&mut rig.cx(), &objs, &[], FlipAxis::Vertical), 0);
    assert_eq!(rig.doc.revision, rev);
}
