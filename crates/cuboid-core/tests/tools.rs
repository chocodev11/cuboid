//! The voxel, paint and UV tools and commands: one scenario each (apply, check, undo, redo), the
//! flood-fill limits, the mirror, a property test of random strokes and a timing test.

mod common;

use std::cell::Cell;
use std::time::{Duration, Instant};

use common::{assert_same, run, scene, select, state};
use cuboid_core::{
    AutoUvStyle, Button, ButtonState, CmdOutcome, Command, Compat, Core, FaceId, InputFrame,
    PaintOptions, PaintTool, PaintToolKind, Refusal, Target, UvAxis, ViewInput, VoxelOptions,
    VoxelTool, VoxelToolKind,
};
use cuboid_model::voxel::{ChunkSlot, I3, mapping};
use cuboid_model::{ElementFlags, FaceName, check_invariants};
use cuboid_scene::{GizmoHit, GizmoState, Hit, HitTarget, Ray, Raycaster, Rect, ViewId};
use glam::{Mat4, Vec2, Vec3};
use proptest::prelude::*;

const RED: u32 = 0xFFFF_0000;
const GREEN: u32 = 0xFF00_FF00;
const BLUE: u32 = 0xFF00_00FF;

#[derive(Default)]
struct Mock {
    hit: Cell<Option<Hit>>,
}

impl Raycaster for Mock {
    fn pick_model(&self, _ray: Ray, _max_len: f32) -> Option<Hit> {
        self.hit.get()
    }

    fn pick_gizmo(&self, _ray: Ray, _view: ViewId, _gizmo: &GizmoState) -> Option<GizmoHit> {
        None
    }

    fn drag_plane(&self, _ray: Ray, _through: Vec3, _max_len: f32) -> Option<Vec3> {
        None
    }
}

struct Rig {
    core: Core,
    mock: Mock,
    t0: Instant,
    frames: u64,
    /// The newest status text a tick asked for.
    said: String,
}

impl Rig {
    /// The standard scene with a voxel world of resolution 16 (cell size 1).
    fn voxels() -> Rig {
        let mut rig = Rig::plain(scene());
        run(&mut rig.core, Command::SetupVoxels { resolution: 16 });
        rig
    }

    fn plain(core: Core) -> Rig {
        Rig {
            core,
            mock: Mock::default(),
            t0: Instant::now(),
            frames: 0,
            said: String::new(),
        }
    }

    fn frame(&mut self, hit: Option<Hit>, button: Button, state: ButtonState) {
        self.mock.hit.set(hit);
        self.frames += 1;
        let mut input = InputFrame::idle(self.t0 + Duration::from_millis(16 * self.frames));
        input.pointer = Some(Vec2::new(100.0, 100.0));
        match button {
            Button::Left => input.left = state,
            Button::Right => input.right = state,
        }
        input.view = Some(ViewInput {
            id: ViewId::Main,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
            view_proj: Mat4::IDENTITY,
            ray: Some(Ray {
                origin: Vec3::ZERO,
                dir: Vec3::NEG_Z,
            }),
        });
        let out = self.core.tick(&input, &self.mock);
        if let Some(text) = out.effects.status {
            self.said = text;
        }
    }

    /// Press at the first hit, hold over the others, release.
    fn stroke_with(&mut self, button: Button, hits: &[Hit]) {
        let (first, rest) = hits.split_first().expect("a hit");
        self.frame(Some(*first), button, ButtonState::PRESSED);
        for hit in rest {
            self.frame(Some(*hit), button, ButtonState::HELD);
        }
        self.frame(None, button, ButtonState::RELEASED);
    }

    fn stroke(&mut self, hits: &[Hit]) {
        self.stroke_with(Button::Left, hits);
    }

    fn voxel_tool(&mut self, kind: VoxelToolKind, options: VoxelOptions) {
        self.core
            .set_tool(Some(Box::new(VoxelTool::new(kind, options))));
    }

    fn paint_tool(&mut self, kind: PaintToolKind, options: PaintOptions) {
        self.core
            .set_tool(Some(Box::new(PaintTool::new(kind, options))));
    }

    fn steps(&self) -> usize {
        self.core.history().undo_len()
    }

    fn undo(&mut self) {
        assert!(self.core.undo().is_done());
    }

    fn redo(&mut self) {
        assert!(self.core.redo().is_done());
    }
}

fn cell(x: i32, y: i32, z: i32) -> I3 {
    I3::new(x, y, z)
}

/// A hit on the face of `cell` that looks along `normal` (a world axis direction), R = 16.
fn face_hit(at: I3, normal: Vec3) -> Hit {
    let centre = mapping::index_to_world(at, 16) + Vec3::new(0.5, 0.5, -0.5);
    Hit {
        target: HitTarget::Voxel,
        point: centre + normal * 0.5,
        normal,
        face: None,
        uv: None,
    }
}

/// A hit on top of `cell`: adding there puts a voxel one cell higher.
fn top(at: I3) -> Hit {
    face_hit(at, Vec3::Y)
}

fn world(core: &Core) -> &cuboid_model::voxel::VoxelWorld {
    core.doc().voxels.as_ref().expect("a voxel world")
}

/// Every voxel, sorted by cell.
fn voxels(core: &Core) -> Vec<(I3, u32)> {
    let Some(world) = core.doc().voxels.as_ref() else {
        return Vec::new();
    };
    let n = world.chunks_per_axis() as i32;
    let mut out = Vec::new();
    for (at, slot) in world.slots().iter().enumerate() {
        let ChunkSlot::Data(chunk) = slot else {
            continue;
        };
        let at = at as i32;
        let base = I3::new(at / (n * n), at / n % n, at % n) * 16;
        for (local, value) in chunk.scan() {
            let local = local as i32;
            out.push((
                base + I3::new(local & 15, (local >> 4) & 15, local >> 8),
                value,
            ));
        }
    }
    out.sort_by_key(|(c, _)| (c.x, c.y, c.z));
    out
}

fn read(core: &Core, at: I3) -> u32 {
    world(core).read(at)
}

fn set_colors(core: &mut Core, left: [f32; 4], right: [f32; 4]) {
    let colors = core.colors_mut();
    colors.left = left;
    colors.right = right;
}

const OPAQUE_RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const OPAQUE_GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const OPAQUE_BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const CLEAR: [f32; 4] = [0.0; 4];

fn opts() -> VoxelOptions {
    VoxelOptions::default()
}

// ---- setup and palette commands -------------------------------------------------------------

#[test]
fn setup_voxels_creates_the_world_and_undo_and_redo_move_it() {
    let mut core = scene();
    let steps = core.history().undo_len();
    let done = run(&mut core, Command::SetupVoxels { resolution: 16 });
    assert_eq!(done.affected, 1);
    assert!(core.query().voxels_set_up);
    assert_eq!(world(&core).res, 16);
    assert_eq!(core.history().undo_len(), steps + 1);
    assert_eq!(core.history().undo_label(), Some("Setup voxels"));
    assert_eq!(
        done.effects.colliders,
        [cuboid_core::ColliderAction::Refresh]
    );

    let undone = core.undo();
    assert!(core.doc().voxels.is_none());
    let CmdOutcome::Done(undone) = undone else {
        panic!("{undone:?}");
    };
    assert_eq!(
        undone.effects.colliders,
        [cuboid_core::ColliderAction::Refresh]
    );
    assert!(core.redo().is_done());
    assert_eq!(world(&core).res, 16);
}

#[test]
fn setup_voxels_refuses_bad_resolutions_and_a_second_world() {
    let mut core = scene();
    let steps = core.history().undo_len();
    for bad in [0, 1, 129, 1000] {
        let outcome = core.execute(Command::SetupVoxels { resolution: bad });
        assert!(
            matches!(outcome, CmdOutcome::Refused(Refusal::Op(_))),
            "{bad}: {outcome:?}"
        );
    }
    run(&mut core, Command::SetupVoxels { resolution: 128 });
    let again = core.execute(Command::SetupVoxels { resolution: 16 });
    assert!(
        matches!(again, CmdOutcome::Refused(Refusal::Op(_))),
        "{again:?}"
    );
    assert_eq!(world(&core).res, 128);
    assert_eq!(core.history().undo_len(), steps + 1);
}

#[test]
fn palette_commands_need_a_world() {
    let mut core = scene();
    for cmd in [
        Command::PaletteAdd {
            color: [1, 2, 3, 255],
        },
        Command::PaletteReplace {
            from: [1, 2, 3, 255],
            to: [4, 5, 6, 255],
        },
        Command::PaletteRemove {
            color: [1, 2, 3, 255],
        },
    ] {
        assert!(matches!(
            core.execute(cmd),
            CmdOutcome::Refused(Refusal::Op(_))
        ));
    }
}

#[test]
fn palette_add_registers_a_colour_without_an_undo_step() {
    let mut rig = Rig::voxels();
    let steps = rig.steps();
    run(
        &mut rig.core,
        Command::PaletteAdd {
            color: [10, 20, 30, 255],
        },
    );
    assert_eq!(world(&rig.core).palette.values(), [0, 0xFF0A_141E]);
    assert_eq!(
        rig.steps(),
        steps,
        "the palette is derived, so there is no step"
    );
    assert_eq!(
        rig.core.execute(Command::PaletteAdd {
            color: [10, 20, 30, 255]
        }),
        CmdOutcome::Unchanged
    );
    assert!(matches!(
        rig.core.execute(Command::PaletteAdd {
            color: [1, 2, 3, 0]
        }),
        CmdOutcome::Refused(_)
    ));
}

#[test]
fn palette_remove_takes_a_colour_out_without_an_undo_step() {
    let mut rig = Rig::voxels();
    let add = Command::PaletteAdd {
        color: [10, 20, 30, 255],
    };
    run(&mut rig.core, add);
    let steps = rig.steps();
    let remove = Command::PaletteRemove {
        color: [10, 20, 30, 255],
    };
    run(&mut rig.core, remove.clone());
    assert_eq!(world(&rig.core).palette.values(), [0]);
    assert_eq!(
        rig.steps(),
        steps,
        "the palette is derived, so there is no step"
    );
    assert_eq!(rig.core.execute(remove), CmdOutcome::Unchanged);
    assert!(matches!(
        rig.core.execute(Command::PaletteRemove {
            color: [1, 2, 3, 0]
        }),
        CmdOutcome::Refused(_)
    ));
}

#[test]
fn a_removed_colour_that_voxels_use_comes_back_with_the_next_undo_and_redo() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.voxel_tool(VoxelToolKind::Pencil, opts());
    rig.stroke(&[top(cell(20, 16, 16))]);
    rig.core.set_tool(None);
    assert_eq!(world(&rig.core).palette.values(), [0, RED]);

    let remove = Command::PaletteRemove {
        color: [255, 0, 0, 255],
    };
    run(&mut rig.core, remove);
    assert_eq!(world(&rig.core).palette.values(), [0]);
    assert_eq!(voxels(&rig.core).len(), 1, "the voxels keep their colour");
    assert!(rig.core.undo().is_done());
    assert!(rig.core.redo().is_done());
    assert_eq!(world(&rig.core).palette.values(), [0, RED]);
}

#[test]
fn palette_replace_recolours_the_world_and_undo_restores_it() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.voxel_tool(VoxelToolKind::Pencil, opts());
    rig.stroke(&[top(cell(20, 16, 16)), top(cell(21, 16, 16))]);
    rig.core.set_tool(None);
    let before = voxels(&rig.core);
    assert_eq!(before.len(), 2);

    let done = run(
        &mut rig.core,
        Command::PaletteReplace {
            from: [255, 0, 0, 255],
            to: [0, 0, 255, 255],
        },
    );
    assert_eq!(done.affected, 2);
    assert!(voxels(&rig.core).iter().all(|(_, v)| *v == BLUE));
    assert_eq!(world(&rig.core).palette.values(), [0, BLUE]);
    assert_eq!(
        rig.core.history().undo_label(),
        Some("Replace voxels color")
    );

    rig.undo();
    assert_eq!(voxels(&rig.core), before);
    rig.redo();
    assert!(voxels(&rig.core).iter().all(|(_, v)| *v == BLUE));
    // Nothing left of the old colour: no step.
    let steps = rig.steps();
    assert_eq!(
        rig.core.execute(Command::PaletteReplace {
            from: [255, 0, 0, 255],
            to: [1, 1, 1, 255]
        }),
        CmdOutcome::Unchanged
    );
    assert_eq!(rig.steps(), steps);
}

// ---- voxel tools ------------------------------------------------------------------------------

#[test]
fn the_pencil_adds_voxels_on_the_face_and_a_stroke_is_one_step() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.voxel_tool(VoxelToolKind::Pencil, opts());
    let steps = rig.steps();
    rig.stroke(&[
        top(cell(10, 16, 10)),
        top(cell(11, 16, 10)),
        top(cell(12, 16, 10)),
    ]);
    // The outward cells are one higher.
    assert_eq!(
        voxels(&rig.core),
        [
            (cell(10, 17, 10), RED),
            (cell(11, 17, 10), RED),
            (cell(12, 17, 10), RED)
        ]
    );
    assert_eq!(rig.steps(), steps + 1);
    assert_eq!(rig.core.history().undo_label(), Some("Voxel add"));
    assert!(rig.core.is_dirty());

    rig.undo();
    assert!(voxels(&rig.core).is_empty());
    rig.redo();
    assert_eq!(voxels(&rig.core).len(), 3);
}

#[test]
fn the_pencil_joins_fast_moves_and_stays_on_its_plane() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.voxel_tool(VoxelToolKind::Pencil, opts());
    // The pointer jumps five cells in one frame and over a one cell step: the stroke fills the
    // gap and keeps the plane of the first cell.
    rig.stroke(&[top(cell(10, 16, 10)), top(cell(15, 17, 10))]);
    let cells: Vec<_> = voxels(&rig.core).into_iter().map(|(c, _)| c).collect();
    assert_eq!(cells.len(), 6, "{cells:?}");
    assert!(cells.iter().all(|c| c.y == 17 && c.z == 10));
    // A hit on another face is ignored.
    let mut rig = Rig::voxels();
    rig.voxel_tool(VoxelToolKind::Pencil, opts());
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.stroke(&[top(cell(10, 16, 10)), face_hit(cell(11, 16, 10), Vec3::X)]);
    assert_eq!(voxels(&rig.core).len(), 1);
}

#[test]
fn a_stroke_that_changes_nothing_leaves_no_step() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.voxel_tool(VoxelToolKind::Pencil, opts());
    // Outside the active box: nothing is written.
    let steps = rig.steps();
    rig.stroke(&[top(cell(60, 16, 10))]);
    assert_eq!(rig.steps(), steps);
    // Erasing air.
    rig.voxel_tool(VoxelToolKind::Erase, opts());
    rig.stroke(&[top(cell(10, 16, 10))]);
    assert_eq!(rig.steps(), steps);
}

#[test]
fn the_eraser_removes_voxels_and_the_right_pencil_recolours() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_RED, OPAQUE_GREEN);
    rig.voxel_tool(VoxelToolKind::Box, opts());
    rig.stroke(&[top(cell(4, 3, 4)), top(cell(6, 5, 6))]);
    assert_eq!(voxels(&rig.core).len(), 27);

    // The eraser takes the voxel under the pointer (the inward cell).
    rig.voxel_tool(VoxelToolKind::Erase, opts());
    let steps = rig.steps();
    rig.stroke(&[top(cell(5, 5, 5))]);
    assert_eq!(read(&rig.core, cell(5, 5, 5)), 0);
    assert_eq!(voxels(&rig.core).len(), 26);
    assert_eq!(rig.steps(), steps + 1);
    rig.undo();
    assert_eq!(read(&rig.core, cell(5, 5, 5)), RED);
    rig.redo();
    assert_eq!(read(&rig.core, cell(5, 5, 5)), 0);

    // The right button of the pencil recolours what exists with the right colour.
    rig.voxel_tool(VoxelToolKind::Pencil, opts());
    rig.stroke_with(Button::Right, &[top(cell(4, 4, 4)), top(cell(5, 4, 4))]);
    assert_eq!(
        (
            read(&rig.core, cell(4, 4, 4)),
            read(&rig.core, cell(5, 4, 4))
        ),
        (GREEN, GREEN)
    );
    // A transparent right colour erases.
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.stroke_with(Button::Right, &[top(cell(6, 4, 4))]);
    assert_eq!(read(&rig.core, cell(6, 4, 4)), 0);
    // It leaves empty cells alone.
    let steps = rig.steps();
    set_colors(&mut rig.core, OPAQUE_RED, OPAQUE_GREEN);
    rig.stroke_with(Button::Right, &[top(cell(30, 30, 30))]);
    assert_eq!(rig.steps(), steps);
}

#[test]
fn the_box_tool_drags_a_cuboid_with_a_rubber_band() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_BLUE, CLEAR);
    rig.voxel_tool(VoxelToolKind::Box, opts());
    let steps = rig.steps();
    // Press, drag far away and back: the final box is the one between the first and last cell.
    rig.stroke(&[
        top(cell(10, 10, 10)),
        top(cell(20, 20, 20)),
        top(cell(30, 12, 25)),
        top(cell(12, 10, 11)),
    ]);
    let cells: Vec<_> = voxels(&rig.core).into_iter().map(|(c, _)| c).collect();
    // Outward cells: y one higher. x 10..=12, y 11..=11, z 10..=11.
    assert_eq!(cells.len(), 3 * 2);
    assert!(
        cells
            .iter()
            .all(|c| c.y == 11 && (10..=12).contains(&c.x) && (10..=11).contains(&c.z))
    );
    assert_eq!(rig.steps(), steps + 1);
    assert_eq!(rig.core.history().undo_label(), Some("Voxel add cuboid"));
    rig.undo();
    assert!(voxels(&rig.core).is_empty());
    rig.redo();
    assert_eq!(voxels(&rig.core).len(), 6);
}

#[test]
fn the_right_box_removes_the_voxels_inside() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_BLUE, CLEAR);
    rig.voxel_tool(VoxelToolKind::Box, opts());
    rig.stroke(&[top(cell(2, 2, 2)), top(cell(5, 5, 5))]);
    assert_eq!(voxels(&rig.core).len(), 64);
    rig.stroke_with(Button::Right, &[top(cell(3, 3, 3)), top(cell(4, 4, 4))]);
    // Inward cells 3..=4 cubed are removed.
    assert_eq!(voxels(&rig.core).len(), 64 - 8);
    assert_eq!(rig.core.history().undo_label(), Some("Voxel sub cuboid"));
}

#[test]
fn the_line_tool_draws_a_bresenham_line_with_a_brush() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_GREEN, CLEAR);
    rig.voxel_tool(VoxelToolKind::Line, opts());
    rig.stroke(&[top(cell(5, 5, 5)), top(cell(5, 5, 5)), top(cell(15, 8, 5))]);
    let cells: Vec<_> = voxels(&rig.core).into_iter().map(|(c, _)| c).collect();
    assert_eq!(cells.len(), 11);
    assert_eq!(cells.first(), Some(&cell(5, 6, 5)));
    assert_eq!(cells.last(), Some(&cell(15, 9, 5)));
    rig.undo();
    assert!(voxels(&rig.core).is_empty());
    rig.redo();
    assert_eq!(voxels(&rig.core).len(), 11);

    // A 3 voxel brush thickens it.
    let mut thick = Rig::voxels();
    set_colors(&mut thick.core, OPAQUE_GREEN, CLEAR);
    thick.voxel_tool(VoxelToolKind::Line, VoxelOptions { brush: 3, ..opts() });
    thick.stroke(&[top(cell(10, 10, 10)), top(cell(10, 10, 10))]);
    assert_eq!(voxels(&thick.core).len(), 27);
}

#[test]
fn the_fill_recolours_the_connected_region() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.voxel_tool(VoxelToolKind::Box, opts());
    rig.stroke(&[top(cell(4, 3, 4)), top(cell(6, 5, 6))]);
    // A green centre voxel stays out of the region.
    set_colors(&mut rig.core, OPAQUE_GREEN, CLEAR);
    rig.voxel_tool(VoxelToolKind::Pencil, opts());
    rig.stroke(&[top(cell(5, 3, 5))]);
    assert_eq!(read(&rig.core, cell(5, 4, 5)), GREEN);

    set_colors(&mut rig.core, OPAQUE_BLUE, CLEAR);
    rig.voxel_tool(VoxelToolKind::Fill, opts());
    let steps = rig.steps();
    rig.stroke(&[top(cell(4, 4, 4))]);
    let all = voxels(&rig.core);
    assert_eq!(all.len(), 27);
    assert_eq!(all.iter().filter(|(_, v)| *v == BLUE).count(), 26);
    assert_eq!(read(&rig.core, cell(5, 4, 5)), GREEN);
    assert_eq!(rig.steps(), steps + 1);
    assert_eq!(rig.core.history().undo_label(), Some("Fill voxels"));
    rig.undo();
    assert_eq!(
        voxels(&rig.core).iter().filter(|(_, v)| *v == RED).count(),
        26
    );
    rig.redo();
    assert_eq!(
        voxels(&rig.core).iter().filter(|(_, v)| *v == BLUE).count(),
        26
    );
}

#[test]
fn a_fill_over_its_limit_changes_nothing_and_says_so() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.voxel_tool(VoxelToolKind::Box, opts());
    rig.stroke(&[top(cell(4, 3, 4)), top(cell(6, 5, 6))]);
    set_colors(&mut rig.core, OPAQUE_BLUE, CLEAR);
    rig.voxel_tool(
        VoxelToolKind::Fill,
        VoxelOptions {
            fill_limit: 26,
            ..opts()
        },
    );
    let steps = rig.steps();
    let before = voxels(&rig.core);
    rig.stroke(&[top(cell(4, 4, 4))]);
    assert_eq!(voxels(&rig.core), before);
    assert_eq!(rig.steps(), steps);
    assert!(rig.said.contains("26"), "{}", rig.said);
    // The same fill with a limit that fits.
    rig.voxel_tool(
        VoxelToolKind::Fill,
        VoxelOptions {
            fill_limit: 27,
            ..opts()
        },
    );
    rig.stroke(&[top(cell(4, 4, 4))]);
    assert!(voxels(&rig.core).iter().all(|(_, v)| *v == BLUE));
    assert_eq!(rig.steps(), steps + 1);
}

#[test]
fn a_fill_on_something_that_is_no_voxel_does_nothing() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_BLUE, CLEAR);
    rig.voxel_tool(VoxelToolKind::Fill, opts());
    let steps = rig.steps();
    rig.stroke(&[top(cell(4, 4, 4))]);
    assert!(voxels(&rig.core).is_empty());
    assert_eq!(rig.steps(), steps);
}

#[test]
fn the_picker_takes_the_voxel_colour_and_leaves_no_step() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, [0.2, 0.4, 0.6, 1.0], CLEAR);
    rig.voxel_tool(VoxelToolKind::Pencil, opts());
    rig.stroke(&[top(cell(7, 7, 7))]);
    set_colors(&mut rig.core, OPAQUE_RED, OPAQUE_GREEN);
    let steps = rig.steps();
    let placed = read(&rig.core, cell(7, 8, 7));
    assert_ne!(placed, 0);

    rig.voxel_tool(VoxelToolKind::Picker, opts());
    rig.stroke(&[top(cell(7, 8, 7))]);
    let left = rig.core.colors_mut().left;
    let to_byte = |c: f32| (c * 255.0).round() as u8;
    assert_eq!(
        [
            to_byte(left[0]),
            to_byte(left[1]),
            to_byte(left[2]),
            to_byte(left[3])
        ],
        [0x33, 0x66, 0x99, 255]
    );
    assert_eq!(
        rig.core.colors_mut().right,
        OPAQUE_GREEN,
        "the right colour stays"
    );
    rig.stroke_with(Button::Right, &[top(cell(7, 8, 7))]);
    assert_eq!(rig.core.colors_mut().right, left);
    // An empty cell picks nothing.
    rig.stroke(&[top(cell(30, 30, 30))]);
    assert_eq!(rig.core.colors_mut().left, left);
    assert_eq!(rig.steps(), steps, "picking is no step");
}

#[test]
fn the_mirror_writes_the_images_in_one_step() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.voxel_tool(
        VoxelToolKind::Pencil,
        VoxelOptions {
            mirror: [true, false, true],
            ..opts()
        },
    );
    let steps = rig.steps();
    rig.stroke(&[top(cell(2, 5, 7))]);
    let got: Vec<I3> = voxels(&rig.core).into_iter().map(|(c, _)| c).collect();
    // x -> 47 - x and z -> 47 - z about the middle of the grid.
    assert_eq!(
        got,
        [
            cell(2, 6, 7),
            cell(2, 6, 40),
            cell(45, 6, 7),
            cell(45, 6, 40)
        ]
    );
    assert_eq!(rig.steps(), steps + 1);
    rig.undo();
    assert!(voxels(&rig.core).is_empty());
    rig.redo();
    assert_eq!(voxels(&rig.core).len(), 4);

    // A box mirrored on Y covers both halves.
    let mut boxed = Rig::voxels();
    set_colors(&mut boxed.core, OPAQUE_RED, CLEAR);
    boxed.voxel_tool(
        VoxelToolKind::Box,
        VoxelOptions {
            mirror: [false, true, false],
            ..opts()
        },
    );
    boxed.stroke(&[top(cell(5, 9, 5)), top(cell(6, 10, 5))]);
    let got = voxels(&boxed.core);
    assert_eq!(got.len(), 2 * 2 * 2);
    assert!(got.iter().any(|(c, _)| c.y == 10) && got.iter().any(|(c, _)| c.y == 47 - 10));
}

#[test]
fn a_tool_without_a_world_says_so_and_changes_nothing() {
    let mut rig = Rig::plain(scene());
    rig.voxel_tool(VoxelToolKind::Pencil, opts());
    let steps = rig.steps();
    rig.stroke(&[top(cell(5, 5, 5))]);
    assert_eq!(rig.steps(), steps);
    assert!(
        rig.said.to_lowercase().contains("voxels first"),
        "{}",
        rig.said
    );
}

#[test]
fn a_stroke_freezes_and_refreshes_the_colliders() {
    let mut rig = Rig::voxels();
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.voxel_tool(VoxelToolKind::Pencil, opts());
    let mut seen = Vec::new();
    for (i, state) in [
        ButtonState::PRESSED,
        ButtonState::HELD,
        ButtonState::RELEASED,
    ]
    .into_iter()
    .enumerate()
    {
        rig.mock.hit.set(Some(top(cell(10 + i as i32, 16, 10))));
        rig.frames += 1;
        let mut input = InputFrame::idle(rig.t0 + Duration::from_millis(16 * rig.frames));
        input.pointer = Some(Vec2::ZERO);
        input.left = state;
        input.view = Some(ViewInput {
            id: ViewId::Main,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            view_proj: Mat4::IDENTITY,
            ray: Some(Ray {
                origin: Vec3::ZERO,
                dir: Vec3::NEG_Z,
            }),
        });
        seen.extend(rig.core.tick(&input, &rig.mock).effects.colliders);
    }
    assert_eq!(
        seen,
        [
            cuboid_core::ColliderAction::Freeze,
            cuboid_core::ColliderAction::Refresh
        ]
    );
    // Undo asks for a refresh as well.
    let CmdOutcome::Done(done) = rig.core.undo() else {
        panic!("undo");
    };
    assert_eq!(
        done.effects.colliders,
        [cuboid_core::ColliderAction::Refresh]
    );
}

// ---- paint tools ----------------------------------------------------------------------------------

/// The standard scene's first cube shows `texture` (16 x 16) on its up face.
fn paint_rig() -> Rig {
    let mut rig = Rig::plain(scene());
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig
}

fn up_hit(core: &Core, element: usize, uv: Vec2) -> Hit {
    let id = core.doc().elements.ids()[element];
    Hit {
        target: HitTarget::Element(id),
        point: Vec3::ZERO,
        normal: Vec3::Y,
        face: Some(FaceName::Up),
        uv: Some(uv),
    }
}

/// The 16 x 16 uv at the middle of texel (x, y).
fn texel_uv(x: u32, y: u32) -> Vec2 {
    Vec2::new((x as f32 + 0.5) / 16.0, (y as f32 + 0.5) / 16.0)
}

fn texture(core: &Core, label: &str) -> Vec<u8> {
    let entry = core
        .doc()
        .textures
        .entries
        .iter()
        .find(|e| &*e.label == label)
        .expect("the texture");
    entry.texture.image.rgba().to_vec()
}

fn texel(core: &Core, label: &str, x: usize, y: usize) -> [u8; 4] {
    let rgba = texture(core, label);
    let at = (y * 16 + x) * 4;
    [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
}

#[test]
fn the_pencil_paints_texels_joined_by_a_line_in_one_step() {
    let mut rig = paint_rig();
    let before = texture(&rig.core, "texture");
    rig.paint_tool(PaintToolKind::Pencil, PaintOptions::default());
    let steps = rig.steps();
    let hits = [
        up_hit(&rig.core, 0, texel_uv(2, 3)),
        up_hit(&rig.core, 0, texel_uv(8, 3)),
        up_hit(&rig.core, 0, texel_uv(8, 9)),
    ];
    rig.stroke(&hits);
    for x in 2..=8 {
        assert_eq!(
            texel(&rig.core, "texture", x, 3),
            [255, 0, 0, 255],
            "x = {x}"
        );
    }
    for y in 3..=9 {
        assert_eq!(
            texel(&rig.core, "texture", 8, y),
            [255, 0, 0, 255],
            "y = {y}"
        );
    }
    assert_ne!(texel(&rig.core, "texture", 0, 0), [255, 0, 0, 255]);
    assert_eq!(rig.steps(), steps + 1);
    assert_eq!(rig.core.history().undo_label(), Some("Paint pixel"));
    let painted = texture(&rig.core, "texture");
    assert_ne!(painted, before);
    // Other textures are untouched.
    rig.undo();
    assert_eq!(texture(&rig.core, "texture"), before);
    rig.redo();
    assert_eq!(texture(&rig.core, "texture"), painted);
}

#[test]
fn painting_the_same_colour_again_leaves_no_step() {
    let mut rig = paint_rig();
    rig.paint_tool(PaintToolKind::Pencil, PaintOptions::default());
    rig.stroke(&[up_hit(&rig.core, 0, texel_uv(4, 4))]);
    let steps = rig.steps();
    rig.stroke(&[up_hit(&rig.core, 0, texel_uv(4, 4))]);
    assert_eq!(rig.steps(), steps);
}

#[test]
fn the_brush_size_and_the_alpha_shape_the_stroke() {
    let mut rig = paint_rig();
    rig.paint_tool(
        PaintToolKind::Pencil,
        PaintOptions {
            brush: 5,
            alpha: 128,
            ..PaintOptions::default()
        },
    );
    rig.stroke(&[up_hit(&rig.core, 0, texel_uv(8, 8))]);
    let painted = (0..16)
        .flat_map(|y| (0..16).map(move |x| (x, y)))
        .filter(|&(x, y)| texel(&rig.core, "texture", x, y) == [255, 0, 0, 128])
        .count();
    assert!((9..=25).contains(&painted), "{painted}");
    assert_eq!(texel(&rig.core, "texture", 8, 8), [255, 0, 0, 128]);
}

#[test]
fn the_right_button_paints_the_right_colour_and_the_eraser_clears() {
    let mut rig = paint_rig();
    set_colors(&mut rig.core, OPAQUE_RED, OPAQUE_GREEN);
    rig.paint_tool(PaintToolKind::Pencil, PaintOptions::default());
    rig.stroke_with(Button::Right, &[up_hit(&rig.core, 0, texel_uv(1, 1))]);
    assert_eq!(texel(&rig.core, "texture", 1, 1), [0, 255, 0, 255]);

    rig.paint_tool(PaintToolKind::Eraser, PaintOptions::default());
    let steps = rig.steps();
    rig.stroke(&[
        up_hit(&rig.core, 0, texel_uv(1, 1)),
        up_hit(&rig.core, 0, texel_uv(3, 1)),
    ]);
    for x in 1..=3 {
        assert_eq!(texel(&rig.core, "texture", x, 1), [0, 0, 0, 0]);
    }
    assert_eq!(rig.steps(), steps + 1);
    rig.undo();
    assert_eq!(texel(&rig.core, "texture", 1, 1), [0, 255, 0, 255]);
}

#[test]
fn the_texture_fill_floods_a_region_and_respects_its_limit() {
    let mut rig = paint_rig();
    // The default texture is a checkerboard of 4 texel squares, so a fill takes one square.
    rig.paint_tool(PaintToolKind::Fill, PaintOptions::default());
    let steps = rig.steps();
    let before = texture(&rig.core, "texture");
    rig.stroke(&[up_hit(&rig.core, 0, texel_uv(1, 1))]);
    let red = (0..16)
        .flat_map(|y| (0..16).map(move |x| (x, y)))
        .filter(|&(x, y)| texel(&rig.core, "texture", x, y) == [255, 0, 0, 255])
        .count();
    assert_eq!(red, 16);
    assert_eq!(rig.steps(), steps + 1);
    assert_eq!(rig.core.history().undo_label(), Some("Fill pixels"));
    rig.undo();
    assert_eq!(texture(&rig.core, "texture"), before);
    rig.redo();

    // A limit below the region leaves the texture alone.
    set_colors(&mut rig.core, OPAQUE_BLUE, CLEAR);
    rig.paint_tool(
        PaintToolKind::Fill,
        PaintOptions {
            fill_limit: 15,
            ..PaintOptions::default()
        },
    );
    let steps = rig.steps();
    let painted = texture(&rig.core, "texture");
    rig.stroke(&[up_hit(&rig.core, 0, texel_uv(5, 1))]);
    assert_eq!(texture(&rig.core, "texture"), painted);
    assert_eq!(rig.steps(), steps);
    rig.paint_tool(
        PaintToolKind::Fill,
        PaintOptions {
            fill_limit: 16,
            ..PaintOptions::default()
        },
    );
    rig.stroke(&[up_hit(&rig.core, 0, texel_uv(5, 1))]);
    assert_eq!(texel(&rig.core, "texture", 5, 1), [0, 0, 255, 255]);
    assert_eq!(rig.steps(), steps + 1);
}

#[test]
fn the_texture_picker_takes_the_texel_colour_without_a_step() {
    let mut rig = paint_rig();
    rig.paint_tool(PaintToolKind::Pencil, PaintOptions::default());
    rig.stroke(&[up_hit(&rig.core, 0, texel_uv(6, 6))]);
    set_colors(&mut rig.core, OPAQUE_BLUE, OPAQUE_GREEN);
    let steps = rig.steps();
    rig.paint_tool(PaintToolKind::Picker, PaintOptions::default());
    rig.stroke(&[up_hit(&rig.core, 0, texel_uv(6, 6))]);
    assert_eq!(rig.core.colors_mut().left, OPAQUE_RED);
    rig.stroke_with(Button::Right, &[up_hit(&rig.core, 0, texel_uv(6, 6))]);
    assert_eq!(rig.core.colors_mut().right, OPAQUE_RED);
    assert_eq!(rig.steps(), steps);
}

#[test]
fn paint_tools_ignore_hits_without_a_texture() {
    let mut rig = paint_rig();
    rig.paint_tool(PaintToolKind::Pencil, PaintOptions::default());
    let steps = rig.steps();
    let mut hit = up_hit(&rig.core, 0, texel_uv(2, 2));
    hit.uv = None;
    rig.stroke(&[hit]);
    rig.stroke(&[face_hit(cell(1, 1, 1), Vec3::Y)]);
    assert_eq!(rig.steps(), steps);
}

#[test]
fn a_long_move_paints_a_connected_line_across_the_texture() {
    let mut rig = paint_rig();
    run(
        &mut rig.core,
        Command::CreateTexture {
            label: Some("big".into()),
            width: 512,
            height: 512,
            color: [0, 0, 0, 255],
        },
    );
    run(
        &mut rig.core,
        Command::AssignTexture {
            targets: vec![Target::Element(0)],
            label: "big".into(),
            faces: vec![FaceId::Up],
        },
    );
    rig.paint_tool(PaintToolKind::Pencil, PaintOptions::default());
    let at = |x: f32, y: f32| up_hit(&rig.core, 0, Vec2::new(x, y));
    rig.stroke(&[at(0.01, 0.01), at(0.99, 0.5)]);
    let rgba = texture(&rig.core, "big");
    let row_hits = |x: usize| (0..512).filter(|&y| rgba[(y * 512 + x) * 4] == 255).count();
    // One texel in every column from the start to the end.
    assert!((6..505).step_by(7).all(|x| row_hits(x) >= 1));
}

// ---- UV commands ------------------------------------------------------------------------------------

fn uv_of(core: &Core, face: FaceName) -> (i32, Vec2, Vec2) {
    let uv = core.doc().elements.items()[0].faces[face as usize].uv;
    (uv.rot(), uv.tl_raw(), uv.br_raw())
}

#[test]
fn auto_uv_in_both_styles_has_a_step_each() {
    let mut core = scene();
    select(&mut core, &[Target::Element(1)]);
    let before = state(&core, true);
    let steps = core.history().undo_len();

    run(
        &mut core,
        Command::AutoUv {
            targets: vec![],
            style: AutoUvStyle::PerFace,
            faces: vec![],
        },
    );
    assert_eq!(core.history().undo_label(), Some("Apply full UV mapping"));
    let element = core.doc().elements.items()[1].clone();
    assert!(
        element
            .faces
            .iter()
            .all(|f| f.uv.tl() == Vec2::ZERO && f.uv.br() == Vec2::ONE)
    );
    assert!(!element.flags.contains(ElementFlags::UV_SNAP));
    let full = state(&core, true);

    run(
        &mut core,
        Command::AutoUv {
            targets: vec![Target::Element(1)],
            style: AutoUvStyle::Box,
            faces: vec![FaceId::Up, FaceId::North],
        },
    );
    assert_eq!(core.history().undo_label(), Some("Apply default UV"));
    assert_eq!(core.history().undo_len(), steps + 2);
    let element = core.doc().elements.items()[1].clone();
    assert_eq!(
        element.faces[FaceName::Down as usize].uv.br(),
        Vec2::ONE,
        "other faces stay"
    );
    assert_ne!(element.faces[FaceName::Up as usize].uv.tl(), Vec2::ZERO);
    let boxed = state(&core, true);

    core.undo();
    assert_same(&state(&core, true), &full, "undo box");
    core.undo();
    assert_same(&state(&core, true), &before, "undo full");
    core.redo();
    core.redo();
    assert_same(&state(&core, true), &boxed, "redo");
}

#[test]
fn rotate_and_flip_change_one_face_and_undo() {
    let mut core = scene();
    select(&mut core, &[Target::Element(0)]);
    run(
        &mut core,
        Command::SetUvRect {
            targets: vec![],
            faces: vec![FaceId::South],
            rect: [0.25, 0.5, 0.75, 1.0],
            rotation: Some(0),
        },
    );
    let before = state(&core, true);

    run(
        &mut core,
        Command::RotateUv {
            targets: vec![],
            faces: vec![FaceId::South],
            degrees: 90,
        },
    );
    assert_eq!(uv_of(&core, FaceName::South).0, 90);
    assert_eq!(uv_of(&core, FaceName::North).0, 0);
    run(
        &mut core,
        Command::RotateUv {
            targets: vec![],
            faces: vec![FaceId::South],
            degrees: -90,
        },
    );
    assert_eq!(uv_of(&core, FaceName::South).0, 0);

    run(
        &mut core,
        Command::FlipUv {
            targets: vec![],
            faces: vec![FaceId::South],
            axis: UvAxis::Horizontal,
        },
    );
    let (_, tl, br) = uv_of(&core, FaceName::South);
    assert_eq!((tl, br), (Vec2::new(0.75, 0.5), Vec2::new(0.25, 1.0)));
    run(
        &mut core,
        Command::FlipUv {
            targets: vec![],
            faces: vec![FaceId::South],
            axis: UvAxis::Vertical,
        },
    );
    let (_, tl, br) = uv_of(&core, FaceName::South);
    assert_eq!((tl, br), (Vec2::new(0.75, 1.0), Vec2::new(0.25, 0.5)));

    for _ in 0..4 {
        core.undo();
    }
    assert_same(&state(&core, true), &before, "undo all four");
    for _ in 0..4 {
        core.redo();
    }
    let (_, tl, br) = uv_of(&core, FaceName::South);
    assert_eq!((tl, br), (Vec2::new(0.75, 1.0), Vec2::new(0.25, 0.5)));
}

#[test]
fn a_numeric_rectangle_sets_the_corners_turns_snap_off_and_merges_in_a_gesture() {
    let mut core = scene();
    let steps = core.history().undo_len();
    let before = state(&core, false);
    let set = |u1: f32| Command::SetUvRect {
        targets: vec![Target::Element(2)],
        faces: vec![FaceId::East],
        rect: [0.0, 0.0, u1, 1.0],
        rotation: None,
    };
    for i in 1..=5_u8 {
        assert!(core.execute_merged(set(f32::from(i) / 10.0), 9).is_done());
    }
    assert_eq!(
        core.history().undo_len(),
        steps + 1,
        "one step for the drag"
    );
    let el = core.doc().elements.items()[2].clone();
    assert_eq!(
        el.faces[FaceName::East as usize].uv.br_raw(),
        Vec2::new(0.5, 1.0)
    );
    assert!(!el.flags.contains(ElementFlags::UV_SNAP));
    core.undo();
    assert_same(&state(&core, false), &before, "undo the merged rectangle");

    // Bad numbers and bad rotations are refused.
    let nan = Command::SetUvRect {
        targets: vec![],
        faces: vec![],
        rect: [f32::NAN, 0.0, 1.0, 1.0],
        rotation: None,
    };
    assert_eq!(core.execute(nan), CmdOutcome::Refused(Refusal::NotFinite));
    let odd = Command::RotateUv {
        targets: vec![Target::Element(0)],
        faces: vec![],
        degrees: 45,
    };
    assert_eq!(core.execute(odd), CmdOutcome::Refused(Refusal::OutOfRange));
}

#[test]
fn uv_snap_follows_the_element_and_the_menu_actions_map_to_commands() {
    let mut core = scene();
    select(&mut core, &[Target::Element(1)]);
    run(
        &mut core,
        Command::SetUvSnap {
            targets: vec![],
            enabled: false,
            mirror: false,
        },
    );
    assert!(
        !core.doc().elements.items()[1]
            .flags
            .contains(ElementFlags::UV_SNAP)
    );
    assert_eq!(core.history().undo_label(), Some("Disable UV-snap"));
    let off = state(&core, true);
    run(
        &mut core,
        Command::SetUvSnap {
            targets: vec![],
            enabled: true,
            mirror: false,
        },
    );
    assert!(
        core.doc().elements.items()[1]
            .flags
            .contains(ElementFlags::UV_SNAP)
    );
    assert_eq!(core.history().undo_label(), Some("Enable UV-snap"));
    run(
        &mut core,
        Command::SetUvSnap {
            targets: vec![],
            enabled: true,
            mirror: true,
        },
    );
    assert_eq!(core.history().undo_label(), Some("Enable UV-snap mirror"));
    core.undo();
    core.undo();
    assert_same(&state(&core, true), &off, "undo snap");

    // Nothing selected, nothing to do.
    run(&mut core, Command::SelectNone);
    assert_eq!(
        core.execute(Command::SetUvSnap {
            targets: vec![],
            enabled: true,
            mirror: false
        }),
        CmdOutcome::Refused(Refusal::NothingSelected)
    );
    assert!(matches!(
        Command::from_action(cuboid_actions::ActionId::UvSnapMirrorDisable),
        Some(Command::SetUvSnap {
            enabled: false,
            mirror: true,
            ..
        })
    ));
}

#[test]
fn the_new_commands_round_trip_through_json() {
    let commands = [
        Command::SetupVoxels { resolution: 32 },
        Command::PaletteAdd {
            color: [1, 2, 3, 255],
        },
        Command::PaletteReplace {
            from: [1, 2, 3, 255],
            to: [4, 5, 6, 255],
        },
        Command::AutoUv {
            targets: vec![Target::Element(1)],
            style: AutoUvStyle::PerFace,
            faces: vec![FaceId::Up],
        },
        Command::RotateUv {
            targets: vec![],
            faces: vec![],
            degrees: -90,
        },
        Command::FlipUv {
            targets: vec![],
            faces: vec![FaceId::West],
            axis: UvAxis::Vertical,
        },
        Command::SetUvRect {
            targets: vec![],
            faces: vec![],
            rect: [0.0, 0.25, 0.5, 1.0],
            rotation: Some(90),
        },
        Command::SetUvSnap {
            targets: vec![],
            enabled: true,
            mirror: true,
        },
    ];
    for command in commands {
        let text = serde_json::to_string(&command).expect("serialises");
        let back: Command = serde_json::from_str(&text).expect("deserialises");
        assert_eq!(back, command, "{text}");
    }
    let minimal: Command = serde_json::from_str(r#"{"AutoUv":{}}"#).expect("defaults");
    assert_eq!(
        minimal,
        Command::AutoUv {
            targets: vec![],
            style: AutoUvStyle::Box,
            faces: vec![]
        }
    );
}

// ---- random strokes ---------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct VoxelStrokeSpec {
    kind: u8,
    right: bool,
    mirror: [bool; 3],
    brush: i32,
    color: u8,
    hits: Vec<([i32; 3], u8)>,
}

fn voxel_stroke() -> impl Strategy<Value = VoxelStrokeSpec> {
    (
        0..6_u8,
        any::<bool>(),
        any::<[bool; 3]>(),
        1..4_i32,
        0..4_u8,
        prop::collection::vec(([0..48_i32, 0..48, 0..48], 0..6_u8), 1..6),
    )
        .prop_map(
            |(kind, right, mirror, brush, color, hits)| VoxelStrokeSpec {
                kind,
                right,
                mirror,
                brush,
                color,
                hits,
            },
        )
}

fn axis_hit(at: [i32; 3], face: u8) -> Hit {
    let normal = [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
    ][face as usize];
    face_hit(cell(at[0], at[1], at[2]), normal)
}

const COLOURS: [[f32; 4]; 4] = [OPAQUE_RED, OPAQUE_GREEN, OPAQUE_BLUE, CLEAR];
const VOXEL_KINDS: [VoxelToolKind; 6] = [
    VoxelToolKind::Pencil,
    VoxelToolKind::Erase,
    VoxelToolKind::Fill,
    VoxelToolKind::Box,
    VoxelToolKind::Line,
    VoxelToolKind::Picker,
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn random_voxel_strokes_undo_back_to_the_start(
        strokes in prop::collection::vec(voxel_stroke(), 1..8),
    ) {
        let mut rig = Rig::voxels();
        // Something to erase and fill.
        set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
        rig.voxel_tool(VoxelToolKind::Box, opts());
        rig.stroke(&[top(cell(10, 10, 10)), top(cell(20, 14, 20))]);
        rig.core.set_tool(None);
        let start_steps = rig.steps();
        let start = voxels(&rig.core);

        for spec in &strokes {
            set_colors(&mut rig.core, COLOURS[usize::from(spec.color)], COLOURS[usize::from((spec.color + 1) % 4)]);
            rig.voxel_tool(
                VOXEL_KINDS[usize::from(spec.kind)],
                VoxelOptions { brush: spec.brush, mirror: spec.mirror, ..opts() },
            );
            let hits: Vec<Hit> = spec.hits.iter().map(|(c, f)| axis_hit(*c, *f)).collect();
            let button = if spec.right { Button::Right } else { Button::Left };
            rig.stroke_with(button, &hits);
            prop_assert!(check_invariants(rig.core.doc(), rig.core.selection()).is_ok());
        }
        let end = voxels(&rig.core);
        let pushed = rig.steps() - start_steps;
        prop_assert!(pushed <= strokes.len());

        for _ in 0..pushed {
            prop_assert!(rig.core.undo().is_done());
        }
        prop_assert_eq!(rig.steps(), start_steps);
        prop_assert_eq!(voxels(&rig.core), start);
        for _ in 0..pushed {
            prop_assert!(rig.core.redo().is_done());
        }
        prop_assert_eq!(voxels(&rig.core), end);
    }

    #[test]
    fn random_paint_strokes_undo_back_to_the_start(
        strokes in prop::collection::vec(
            (0..4_u8, any::<bool>(), 1..6_i32, 0..4_u8, prop::collection::vec((0.0..1.0_f32, 0.0..1.0_f32), 1..6)),
            1..8,
        ),
    ) {
        let mut rig = paint_rig();
        let start_steps = rig.steps();
        let start = (texture(&rig.core, "texture"), texture(&rig.core, "particle"));
        let kinds = [PaintToolKind::Pencil, PaintToolKind::Eraser, PaintToolKind::Fill, PaintToolKind::Picker];
        for (kind, right, brush, color, uvs) in &strokes {
            set_colors(&mut rig.core, COLOURS[usize::from(*color)], COLOURS[usize::from((*color + 1) % 4)]);
            rig.paint_tool(
                kinds[usize::from(*kind)],
                PaintOptions { brush: *brush, ..PaintOptions::default() },
            );
            let hits: Vec<Hit> = uvs.iter().map(|(u, v)| up_hit(&rig.core, 0, Vec2::new(*u, *v))).collect();
            rig.stroke_with(if *right { Button::Right } else { Button::Left }, &hits);
        }
        let end = (texture(&rig.core, "texture"), texture(&rig.core, "particle"));
        let pushed = rig.steps() - start_steps;
        prop_assert!(pushed <= strokes.len());
        for _ in 0..pushed {
            prop_assert!(rig.core.undo().is_done());
        }
        prop_assert!(texture(&rig.core, "texture") == start.0 && texture(&rig.core, "particle") == start.1);
        for _ in 0..pushed {
            prop_assert!(rig.core.redo().is_done());
        }
        prop_assert!(texture(&rig.core, "texture") == end.0 && texture(&rig.core, "particle") == end.1);
    }
}

// ---- timing -------------------------------------------------------------------------------------------

/// A stroke of 1000 cells must cost far less than a frame (16 ms) per frame of the stroke. Run
/// with `cargo test -p cuboid-core --release --test tools -- --ignored`.
#[test]
#[ignore = "timing; run by hand"]
fn a_thousand_cell_stroke_stays_far_below_a_frame() {
    let mut rig = Rig::plain(Core::new(Compat::port(), 1));
    run(&mut rig.core, Command::SetupVoxels { resolution: 32 });
    set_colors(&mut rig.core, OPAQUE_RED, CLEAR);
    rig.voxel_tool(
        VoxelToolKind::Pencil,
        VoxelOptions {
            brush: 3,
            mirror: [true, false, true],
            ..opts()
        },
    );
    let steps = rig.steps();
    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    // 1000 distinct cells along a snake over a 32 x 32 floor (res 32 has a cell size of 0.5).
    for i in 0..1000_i32 {
        let (x, z) = (i % 32, (i / 32) * 2);
        let centre =
            mapping::index_to_world(cell(32 + x, 31, 32 + z), 32) + Vec3::new(0.25, 0.25, -0.25);
        let hit = Hit {
            target: HitTarget::Voxel,
            point: centre + Vec3::Y * 0.25,
            normal: Vec3::Y,
            face: None,
            uv: None,
        };
        let state = if i == 0 {
            ButtonState::PRESSED
        } else {
            ButtonState::HELD
        };
        let t = Instant::now();
        rig.frame(Some(hit), Button::Left, state);
        slowest = slowest.max(t.elapsed());
    }
    rig.frame(None, Button::Left, ButtonState::RELEASED);
    let total = started.elapsed();
    eprintln!("1000 frames: {total:?}, slowest {slowest:?}");
    assert!(
        slowest < Duration::from_millis(4),
        "slowest frame {slowest:?}"
    );
    assert!(total < Duration::from_millis(200), "total {total:?}");
    assert_eq!(rig.steps(), steps + 1);
    assert!(voxels(&rig.core).len() >= 1000);
}
