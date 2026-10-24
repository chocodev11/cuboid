use cuboid_model::{DocFlags, Document, EditCtx, EditEnv, RenderPrefs, Selection, TextureList};
use cuboid_num::EngineRng;
use cuboid_voxel::{ChunkSlot, I3, IBox3};
use glam::Vec3;

use super::*;

const RED: u32 = 0xFFFF_0000;
const GREEN: u32 = 0xFF00_FF00;

struct Rig {
    doc: Document,
    sel: Selection,
    env: EditEnv,
    rng: EngineRng,
}

impl Rig {
    /// A document with a voxel world of resolution 16 (a 48 cell grid).
    fn new() -> Self {
        let mut rig = Rig {
            doc: Document::new(TextureList::default()),
            sel: Selection::default(),
            env: EditEnv {
                restrict_rotations: true,
                render: RenderPrefs::default(),
                code_page: 1252,
            },
            rng: EngineRng::seeded(1),
        };
        setup(&mut rig.cx(), 16).expect("setup");
        rig
    }

    fn cx(&mut self) -> EditCtx<'_> {
        EditCtx {
            doc: &mut self.doc,
            sel: &mut self.sel,
            env: &self.env,
            rng: &mut self.rng,
        }
    }

    fn world(&self) -> &cuboid_voxel::VoxelWorld {
        self.doc.voxels.as_ref().expect("a world")
    }

    fn read(&self, x: i32, y: i32, z: i32) -> u32 {
        self.world().read(I3::new(x, y, z))
    }

    fn voxels(&self) -> usize {
        self.world()
            .slots()
            .iter()
            .map(|slot| match slot {
                ChunkSlot::Data(chunk) => usize::from(chunk.nonzero()),
                _ => 0,
            })
            .sum()
    }

    fn put(&mut self, cells: &[I3], brush: Brush, value: u32) -> usize {
        let put = Put {
            value,
            existing_only: false,
        };
        write_cells(&mut self.cx(), cells, brush, put, &mut Vec::new()).expect("write")
    }
}

fn cell(x: i32, y: i32, z: i32) -> I3 {
    I3::new(x, y, z)
}

#[test]
fn setup_makes_a_world_over_the_model_box() {
    let mut doc = Document::new(TextureList::default());
    let (mut sel, mut rng) = (Selection::default(), EngineRng::seeded(1));
    let env = EditEnv {
        restrict_rotations: true,
        render: RenderPrefs::default(),
        code_page: 1252,
    };
    let mut cx = EditCtx {
        doc: &mut doc,
        sel: &mut sel,
        env: &env,
        rng: &mut rng,
    };
    for bad in [0, 1, 129, u32::MAX] {
        assert!(matches!(
            setup(&mut cx, bad),
            Err(VoxelError::Resolution(_))
        ));
    }
    assert!(cx.doc.voxels.is_none(), "a refused setup changes nothing");
    setup(&mut cx, 32).expect("setup");
    let world = cx.doc.voxels.as_ref().expect("a world");
    assert_eq!(world.res, 32);
    assert_eq!(
        world.active_box,
        IBox3 {
            min: I3::ZERO,
            max: I3::splat(96)
        }
    );
    assert!(matches!(setup(&mut cx, 16), Err(VoxelError::AlreadySetUp)));
}

#[test]
fn writes_need_a_world() {
    let mut doc = Document::new(TextureList::default());
    let (mut sel, mut rng) = (Selection::default(), EngineRng::seeded(1));
    let env = EditEnv {
        restrict_rotations: true,
        render: RenderPrefs::default(),
        code_page: 1252,
    };
    let mut cx = EditCtx {
        doc: &mut doc,
        sel: &mut sel,
        env: &env,
        rng: &mut rng,
    };
    let put = Put {
        value: RED,
        existing_only: false,
    };
    assert!(matches!(
        write_cells(&mut cx, &[I3::ZERO], Brush::DOT, put, &mut Vec::new()),
        Err(VoxelError::NoWorld)
    ));
    assert!(matches!(
        palette_add(cx.doc, [1, 2, 3, 255]),
        Err(VoxelError::NoWorld)
    ));
}

#[test]
fn a_write_changes_the_cell_and_the_palette_and_marks_the_chunks() {
    let mut rig = Rig::new();
    assert_eq!(rig.put(&[cell(20, 16, 16)], Brush::DOT, RED), 1);
    assert_eq!(rig.read(20, 16, 16), RED);
    assert_eq!(rig.world().palette.values(), [0, RED]);
    assert!(rig.doc.journal.doc().contains(DocFlags::VOXEL_CHUNKS));
    // Writing the same value again changes nothing, and the revision stays.
    let rev = rig.doc.revision;
    assert_eq!(rig.put(&[cell(20, 16, 16)], Brush::DOT, RED), 0);
    assert_eq!(rig.doc.revision, rev);
}

#[test]
fn writes_outside_the_active_box_are_ignored() {
    let mut rig = Rig::new();
    assert_eq!(
        rig.put(&[cell(-1, 0, 0), cell(48, 0, 0)], Brush::DOT, RED),
        0
    );
    assert_eq!(rig.voxels(), 0);
}

#[test]
fn the_cube_brush_stamps_its_edge() {
    let mut rig = Rig::new();
    let brush = Brush {
        size: 3,
        mirror: [false; 3],
    };
    assert_eq!(rig.put(&[cell(10, 10, 10)], brush, RED), 27);
    assert_eq!(rig.read(9, 9, 9), RED);
    assert_eq!(rig.read(11, 11, 11), RED);
    assert_eq!(rig.read(12, 10, 10), 0);
    // The brush is clamped.
    let huge = Brush {
        size: 1000,
        mirror: [false; 3],
    };
    assert_eq!(rig.put(&[cell(30, 30, 30)], huge, GREEN), 16 * 16 * 16);
}

#[test]
fn mirror_writes_the_images_across_the_middle_of_the_box() {
    let mut rig = Rig::new();
    let brush = Brush {
        size: 1,
        mirror: [true, false, true],
    };
    assert_eq!(rig.put(&[cell(2, 5, 7)], brush, RED), 4);
    for at in [(2, 5, 7), (45, 5, 7), (2, 5, 40), (45, 5, 40)] {
        assert_eq!(rig.read(at.0, at.1, at.2), RED, "{at:?}");
    }
    assert_eq!(rig.voxels(), 4);
    // The 2-wide brush around 23 covers 23 and 24, which are each other's mirror images.
    let mut middle = Rig::new();
    let x_only = Brush {
        size: 2,
        mirror: [true, false, false],
    };
    assert_eq!(middle.put(&[cell(23, 5, 5)], x_only, RED), 8);
}

#[test]
fn a_write_can_leave_empty_cells_alone() {
    let mut rig = Rig::new();
    rig.put(&[cell(5, 5, 5)], Brush::DOT, RED);
    let put = Put {
        value: GREEN,
        existing_only: true,
    };
    let cells = [cell(5, 5, 5), cell(6, 5, 5)];
    let mut log = Vec::new();
    let n = write_cells(&mut rig.cx(), &cells, Brush::DOT, put, &mut log).expect("write");
    assert_eq!(n, 1);
    assert_eq!((rig.read(5, 5, 5), rig.read(6, 5, 5)), (GREEN, 0));
    assert_eq!(log, [(cell(5, 5, 5), RED)]);
}

#[test]
fn rollback_restores_the_logged_cells() {
    let mut rig = Rig::new();
    rig.put(&[cell(5, 5, 5)], Brush::DOT, RED);
    let mut log = Vec::new();
    let put = Put {
        value: GREEN,
        existing_only: false,
    };
    let cells = [cell(5, 5, 5), cell(6, 5, 5), cell(7, 5, 5)];
    write_cells(&mut rig.cx(), &cells, Brush::DOT, put, &mut log).expect("write");
    assert_eq!(rig.voxels(), 3);
    rollback(&mut rig.cx(), &mut log).expect("rollback");
    assert!(log.is_empty());
    assert_eq!(rig.voxels(), 1);
    assert_eq!(rig.read(5, 5, 5), RED);
}

#[test]
fn a_huge_write_is_refused_before_it_touches_anything() {
    let mut rig = Rig::new();
    let cells = vec![cell(1, 1, 1); MAX_STROKE_CELLS + 1];
    let put = Put {
        value: RED,
        existing_only: false,
    };
    let result = write_cells(&mut rig.cx(), &cells, Brush::DOT, put, &mut Vec::new());
    assert!(matches!(result, Err(VoxelError::TooLarge(_))));
    assert_eq!(rig.voxels(), 0);
}

fn slab(rig: &mut Rig, edge: i32) {
    let mut cells = Vec::new();
    box_cells(
        cell(0, 0, 0),
        cell(edge - 1, edge - 1, edge - 1),
        rig.world().active_box,
        &mut cells,
    )
    .expect("box");
    rig.put(&cells, Brush::DOT, RED);
}

#[test]
fn a_fill_recolours_the_connected_voxels_of_the_seed_colour() {
    let mut rig = Rig::new();
    slab(&mut rig, 4);
    // A green voxel inside and a red island apart from the block.
    rig.put(&[cell(1, 1, 1)], Brush::DOT, GREEN);
    rig.put(&[cell(10, 10, 10)], Brush::DOT, RED);
    let mut log = Vec::new();
    let n = fill_region(&mut rig.cx(), cell(0, 0, 0), 0xFF00_00FF, 1000, &mut log).expect("fill");
    assert_eq!(n, 64 - 1, "the whole block but the green voxel");
    assert_eq!(log.len(), n);
    assert_eq!(rig.read(0, 0, 0), 0xFF00_00FF);
    assert_eq!(rig.read(1, 1, 1), GREEN);
    assert_eq!(rig.read(10, 10, 10), RED, "not connected");
}

#[test]
fn a_fill_over_the_limit_changes_nothing() {
    let mut rig = Rig::new();
    slab(&mut rig, 4);
    let before = rig.world().rev;
    let mut log = Vec::new();
    let result = fill_region(&mut rig.cx(), cell(0, 0, 0), GREEN, 63, &mut log);
    assert!(matches!(result, Err(VoxelError::TooLarge(63))));
    assert!(log.is_empty());
    assert_eq!(rig.world().rev, before);
    assert_eq!(rig.read(3, 3, 3), RED);
    // A limit equal to the region size is fine.
    let n = fill_region(&mut rig.cx(), cell(0, 0, 0), GREEN, 64, &mut log).expect("fill");
    assert_eq!(n, 64);
}

#[test]
fn a_fill_of_empty_space_is_bounded_by_the_box_and_the_limit() {
    let mut rig = Rig::new();
    // The empty grid has 48^3 = 110592 cells.
    let mut log = Vec::new();
    let result = fill_region(&mut rig.cx(), cell(0, 0, 0), RED, 1000, &mut log);
    assert!(matches!(result, Err(VoxelError::TooLarge(1000))));
    // Walls around a pocket keep the region small.
    let mut walls = Vec::new();
    box_cells(
        cell(10, 10, 10),
        cell(14, 14, 14),
        rig.world().active_box,
        &mut walls,
    )
    .expect("box");
    rig.put(&walls, Brush::DOT, GREEN);
    let inner: Vec<_> = {
        let mut v = Vec::new();
        box_cells(
            cell(11, 11, 11),
            cell(13, 13, 13),
            rig.world().active_box,
            &mut v,
        )
        .expect("box");
        v
    };
    rig.put(&inner, Brush::DOT, 0);
    let n = fill_region(&mut rig.cx(), cell(12, 12, 12), RED, 1000, &mut log).expect("fill");
    assert_eq!(n, 27);
}

#[test]
fn a_fill_with_the_same_colour_or_outside_the_box_does_nothing() {
    let mut rig = Rig::new();
    slab(&mut rig, 2);
    let mut log = Vec::new();
    assert_eq!(
        fill_region(&mut rig.cx(), cell(0, 0, 0), RED, 100, &mut log).expect("fill"),
        0
    );
    assert_eq!(
        fill_region(&mut rig.cx(), cell(-3, 0, 0), GREEN, 100, &mut log).expect("fill"),
        0
    );
    assert_eq!(rig.voxels(), 8);
}

#[test]
fn replacing_a_colour_recolours_everywhere_and_renames_the_palette_entry() {
    let mut rig = Rig::new();
    rig.put(
        &[cell(1, 1, 1), cell(40, 40, 40), cell(17, 2, 33)],
        Brush::DOT,
        RED,
    );
    rig.put(&[cell(2, 2, 2)], Brush::DOT, GREEN);
    let n = replace_color(&mut rig.cx(), [255, 0, 0, 255], [0, 0, 255, 255]).expect("replace");
    assert_eq!(n, 3);
    let blue = 0xFF00_00FF;
    assert_eq!(
        (rig.read(1, 1, 1), rig.read(40, 40, 40), rig.read(17, 2, 33)),
        (blue, blue, blue)
    );
    assert_eq!(rig.read(2, 2, 2), GREEN);
    assert_eq!(rig.world().palette.values(), [0, blue, GREEN]);
    // Nothing of the colour is left, and transparent colours are refused.
    assert_eq!(
        replace_color(&mut rig.cx(), [255, 0, 0, 255], [1, 1, 1, 255]).expect("replace"),
        0
    );
    assert!(matches!(
        replace_color(&mut rig.cx(), [255, 0, 0, 255], [1, 1, 1, 0]),
        Err(VoxelError::Transparent)
    ));
}

#[test]
fn replacing_reaches_cells_outside_the_active_box() {
    let mut rig = Rig::new();
    rig.put(&[cell(5, 5, 5)], Brush::DOT, RED);
    let shrunk = IBox3 {
        min: cell(10, 10, 10),
        max: cell(20, 20, 20),
    };
    rig.doc
        .voxels
        .as_mut()
        .expect("world")
        .set_active_box(shrunk);
    assert_eq!(rig.read(5, 5, 5), 0, "unreadable now");
    let n = replace_color(&mut rig.cx(), [255, 0, 0, 255], [0, 255, 0, 255]).expect("replace");
    assert_eq!(n, 1);
    let world = rig.world();
    assert_eq!(world.palette.values(), [0, GREEN]);
}

#[test]
fn the_palette_takes_new_colours_once() {
    let mut rig = Rig::new();
    assert!(palette_add(&mut rig.doc, [10, 20, 30, 255]).expect("add"));
    assert!(!palette_add(&mut rig.doc, [10, 20, 30, 255]).expect("add"));
    assert_eq!(rig.world().palette.values(), [0, 0xFF0A_141E]);
    assert!(matches!(
        palette_add(&mut rig.doc, [1, 2, 3, 0]),
        Err(VoxelError::Transparent)
    ));
}

#[test]
fn cells_follow_the_world_corner_convention() {
    // A hit on the top of the block origin voxel selects the voxel below and the one above.
    let hit = hit_cells(Vec3::new(0.5, 1.0, -0.5), Vec3::Y, 16).expect("hit");
    assert_eq!(
        (hit.inward, hit.outward),
        (cell(16, 16, 16), cell(16, 17, 16))
    );
}
