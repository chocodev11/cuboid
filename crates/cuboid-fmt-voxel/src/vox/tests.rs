//! Tests of the `.vox` reader and writer on files built byte by byte.

use std::path::Path;

use cuboid_model::apply::voxel_world_bytes;
use cuboid_model::{
    ApplyMode, ApplySource, Document, EditCtx, EditEnv, RenderPrefs, Selection, TextureList,
    apply_document,
};
use cuboid_num::EngineRng;
use cuboid_voxel::{I3, IBox3, VoxelWorld};

use super::*;

// --- Building files -------------------------------------------------------------------------

fn chunk(id: &[u8; 4], content: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend_from_slice(&u32::try_from(content.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(content);
    out
}

fn file(chunks: &[Vec<u8>]) -> Vec<u8> {
    let rest: Vec<u8> = chunks.concat();
    let mut out = b"VOX ".to_vec();
    out.extend_from_slice(&150_u32.to_le_bytes());
    out.extend_from_slice(b"MAIN");
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&u32::try_from(rest.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&rest);
    out
}

fn size(x: u32, y: u32, z: u32) -> Vec<u8> {
    chunk(
        b"SIZE",
        &[x.to_le_bytes(), y.to_le_bytes(), z.to_le_bytes()].concat(),
    )
}

fn xyzi(voxels: &[[u8; 4]]) -> Vec<u8> {
    let mut content = u32::try_from(voxels.len()).unwrap().to_le_bytes().to_vec();
    content.extend(voxels.iter().flatten());
    chunk(b"XYZI", &content)
}

/// An RGBA chunk with the given first colours and black for the rest.
fn rgba(colours: &[[u8; 4]]) -> Vec<u8> {
    let mut content = Vec::new();
    for slot in 0..256 {
        content.extend_from_slice(colours.get(slot).unwrap_or(&[0, 0, 0, 255]));
    }
    chunk(b"RGBA", &content)
}

fn int(v: i32) -> [u8; 4] {
    v.to_le_bytes()
}

fn text(s: &str) -> Vec<u8> {
    let mut out = u32::try_from(s.len()).unwrap().to_le_bytes().to_vec();
    out.extend_from_slice(s.as_bytes());
    out
}

fn dict(pairs: &[(&str, &str)]) -> Vec<u8> {
    let mut out = u32::try_from(pairs.len()).unwrap().to_le_bytes().to_vec();
    for (k, v) in pairs {
        out.extend(text(k));
        out.extend(text(v));
    }
    out
}

fn transform(id: i32, child: i32, rotation: Option<u8>, t: [i32; 3]) -> Vec<u8> {
    let mut frame = vec![("_t", format!("{} {} {}", t[0], t[1], t[2]))];
    if let Some(r) = rotation {
        frame.push(("_r", r.to_string()));
    }
    let frame: Vec<(&str, &str)> = frame.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut content = int(id).to_vec();
    content.extend(dict(&[]));
    content.extend([int(child), int(-1), int(-1), int(1)].concat());
    content.extend(dict(&frame));
    chunk(b"nTRN", &content)
}

fn group(id: i32, children: &[i32]) -> Vec<u8> {
    let mut content = int(id).to_vec();
    content.extend(dict(&[]));
    content.extend(int(i32::try_from(children.len()).unwrap()));
    for &c in children {
        content.extend(int(c));
    }
    chunk(b"nGRP", &content)
}

fn shape(id: i32, model: i32) -> Vec<u8> {
    let mut content = int(id).to_vec();
    content.extend(dict(&[]));
    content.extend([int(1), int(model)].concat());
    content.extend(dict(&[]));
    chunk(b"nSHP", &content)
}

fn open(imported: VoxImport) -> Document {
    let mut doc = Document::new(TextureList::default());
    let mut sel = Selection::default();
    let env = EditEnv {
        restrict_rotations: true,
        render: RenderPrefs::default(),
        code_page: 1252,
        allow_negative_size: false,
    };
    let mut rng = EngineRng::seeded(1);
    apply_document(
        &mut EditCtx {
            doc: &mut doc,
            sel: &mut sel,
            env: &env,
            rng: &mut rng,
        },
        ApplySource::Loaded(Box::new(imported.doc)),
        ApplyMode::Open,
    )
    .unwrap();
    doc
}

fn load(bytes: &[u8]) -> Document {
    open(import(bytes, Path::new("dir/Thing.vox")).unwrap())
}

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const ARGB_RED: u32 = 0xFFFF_0000;
const ARGB_GREEN: u32 = 0xFF00_FF00;
const ARGB_BLUE: u32 = 0xFF00_00FF;

/// Every voxel of the active box as `(grid cell, value)`, in scan order.
fn contents(world: &VoxelWorld) -> Vec<(I3, u32)> {
    let b = world.active_box;
    let mut found = Vec::new();
    for x in b.min.x..b.max.x {
        for y in b.min.y..b.max.y {
            for z in b.min.z..b.max.z {
                let v = world.read(I3::new(x, y, z));
                if v != 0 {
                    found.push((I3::new(x, y, z), v));
                }
            }
        }
    }
    found
}

// --- Import ---------------------------------------------------------------------------------

#[test]
fn a_small_model_lands_in_the_middle_block_with_z_up() {
    let bytes = file(&[
        size(3, 2, 4),
        xyzi(&[[1, 0, 3, 2], [0, 1, 0, 1], [2, 1, 2, 3]]),
        rgba(&[RED, GREEN, BLUE]),
    ]);
    let imported = import(&bytes, Path::new("dir/Thing.vox")).unwrap();
    assert!(imported.notices.is_empty());
    let doc = open(imported);
    assert_eq!(doc.props.general.name, "Thing");
    let world = doc.voxels.as_ref().unwrap();
    // N = 4: C = 4, x in [4, 7), VOX z (4) is grid y, VOX y (2) is grid z.
    assert_eq!(world.res, 4);
    assert_eq!(
        world.active_box,
        IBox3 {
            min: I3::new(4, 4, 4),
            max: I3::new(7, 8, 6)
        }
    );
    // VOX (x, y, z) is grid (C + x, C + z, C + sy - 1 - y).
    assert_eq!(world.read(I3::new(5, 7, 5)), ARGB_GREEN);
    assert_eq!(world.read(I3::new(4, 4, 4)), ARGB_RED);
    assert_eq!(world.read(I3::new(6, 6, 4)), ARGB_BLUE);
    assert_eq!(contents(world).len(), 3);
    // The palette lists the colours in file order after the empty entry.
    assert_eq!(world.palette.values(), [0, ARGB_GREEN, ARGB_RED, ARGB_BLUE]);
}

#[test]
fn the_active_box_matches_the_size_for_every_axis_length() {
    let lengths = [1_u32, 2, 3, 5, 7, 16, 17, 100, 255, 256, 300];
    for &n in &lengths {
        for (sx, sy, sz) in [(n, 1, 1), (1, n, 1), (1, 1, n), (n, 2, 3)] {
            let doc = load(&file(&[size(sx, sy, sz), xyzi(&[[0, 0, 0, 1]])]));
            let world = doc.voxels.as_ref().unwrap();
            let res = i32::try_from(world.res).unwrap();
            let (sx, sy, sz) = (
                i32::try_from(sx).unwrap(),
                i32::try_from(sy).unwrap(),
                i32::try_from(sz).unwrap(),
            );
            assert_eq!(world.res, sx.max(sy).max(sz).max(2) as u32);
            assert_eq!(
                world.active_box,
                IBox3 {
                    min: I3::splat(res),
                    max: I3::new(res + sx, res + sz, res + sy)
                },
                "size {sx} {sy} {sz}"
            );
        }
    }
}

#[test]
fn the_default_palette_is_used_without_an_rgba_chunk() {
    let doc = load(&file(&[
        size(2, 1, 1),
        xyzi(&[[0, 0, 0, 1], [1, 0, 0, 2], [0, 0, 0, 216], [1, 0, 0, 255]]),
    ]));
    let world = doc.voxels.as_ref().unwrap();
    // Index 1 is white, index 2 white with a lower blue, 216 the first red ramp step.
    assert_eq!(world.palette.values().get(1), Some(&0xFFFF_FFFF));
    assert_eq!(world.palette.values().get(2), Some(&0xFFFF_FFCC));
    assert_eq!(world.palette.values().get(3), Some(&0xFFEE_0000));
    assert_eq!(world.palette.values().get(4), Some(&0xFF11_1111));
}

#[test]
fn the_default_palette_follows_the_published_one() {
    let ours = default_palette();
    assert_eq!(ours.len(), 255);
    let theirs = &*dot_vox::DEFAULT_PALETTE;
    for (k, &colour) in ours.iter().enumerate() {
        let c = theirs.get(k).unwrap();
        let expected = u32::from_be_bytes([c.a, c.r, c.g, c.b]);
        assert_eq!(colour, expected, "entry {k}");
    }
}

#[test]
fn indices_outside_the_palette_and_transparent_colours() {
    let doc = load(&file(&[
        size(3, 1, 1),
        xyzi(&[[0, 0, 0, 1], [1, 0, 0, 2], [2, 0, 0, 9]]),
        // Two colours, the second fully transparent.
        chunk(b"RGBA", &[RED, [1, 2, 3, 0]].concat()),
    ]));
    let world = doc.voxels.as_ref().unwrap();
    let found = contents(world);
    // The transparent colour is empty, index 9 gets the stand-in colour.
    assert_eq!(found.len(), 2);
    assert_eq!(found.first().map(|c| c.1), Some(ARGB_RED));
    assert_eq!(found.last().map(|c| c.1), Some(MISSING_COLOUR));
}

#[test]
fn voxels_outside_the_size_are_left_out_with_a_notice() {
    let imported = import(
        &file(&[
            size(2, 2, 2),
            xyzi(&[[0, 0, 0, 1], [2, 0, 0, 1], [0, 0, 9, 1]]),
        ]),
        Path::new("a.vox"),
    )
    .unwrap();
    assert_eq!(imported.notices, [Notice::VoxelsOutsideModel { count: 2 }]);
    assert_eq!(contents(open(imported).voxels.as_ref().unwrap()).len(), 1);
}

#[test]
fn unknown_chunks_and_children_are_skipped() {
    let mut odd = b"rOBJ".to_vec();
    odd.extend_from_slice(&3_u32.to_le_bytes());
    odd.extend_from_slice(&5_u32.to_le_bytes());
    odd.extend_from_slice(&[1, 2, 3, 9, 9, 9, 9, 9]);
    let doc = load(&file(&[
        chunk(b"PACK", &1_u32.to_le_bytes()),
        size(1, 1, 1),
        odd,
        xyzi(&[[0, 0, 0, 1]]),
        chunk(b"NOTE", &[]),
        rgba(&[BLUE]),
    ]));
    assert_eq!(contents(doc.voxels.as_ref().unwrap())[0].1, ARGB_BLUE);
}

#[test]
fn a_later_xyzi_replaces_an_earlier_one_and_the_repeat_keeps_the_last_value() {
    let doc = load(&file(&[
        size(2, 1, 1),
        xyzi(&[[0, 0, 0, 1]]),
        xyzi(&[[0, 0, 0, 1], [0, 0, 0, 2]]),
        rgba(&[RED, GREEN]),
    ]));
    let world = doc.voxels.as_ref().unwrap();
    assert_eq!(contents(world), [(I3::new(2, 2, 2), ARGB_GREEN)]);
    // The replay adds every value in file order.
    assert_eq!(world.palette.values(), [0, ARGB_RED, ARGB_GREEN]);
}

// --- Several models -------------------------------------------------------------------------

/// Two 2 x 2 x 2 models, one voxel each; the second sits 4 voxels to the right.
fn two_models(second_rotation: Option<u8>) -> Vec<u8> {
    file(&[
        size(2, 2, 2),
        xyzi(&[[0, 0, 0, 1]]),
        size(2, 2, 2),
        xyzi(&[[1, 1, 1, 2]]),
        transform(0, 1, None, [0, 0, 0]),
        group(1, &[2, 4]),
        transform(2, 3, None, [0, 0, 0]),
        shape(3, 0),
        transform(4, 5, second_rotation, [4, 0, 0]),
        shape(5, 1),
        rgba(&[RED, GREEN]),
    ])
}

#[test]
fn models_are_placed_by_their_translations() {
    let imported = import(&two_models(None), Path::new("a.vox")).unwrap();
    assert!(imported.notices.is_empty());
    let doc = open(imported);
    let world = doc.voxels.as_ref().unwrap();
    // x spans -1 to 4 (6 voxels), y and z 2 voxels: C = 6.
    assert_eq!(world.res, 6);
    assert_eq!(
        world.active_box,
        IBox3 {
            min: I3::splat(6),
            max: I3::new(12, 8, 8)
        }
    );
    assert_eq!(world.read(I3::new(6, 6, 7)), ARGB_RED);
    assert_eq!(world.read(I3::new(11, 7, 6)), ARGB_GREEN);
    assert_eq!(contents(world).len(), 2);
}

#[test]
fn a_rotated_model_turns_about_its_centre() {
    // A line of three along x, turned a quarter about Z (byte 17): it runs along y instead.
    let bytes = file(&[
        size(3, 1, 1),
        xyzi(&[[0, 0, 0, 1], [1, 0, 0, 2], [2, 0, 0, 3]]),
        size(1, 1, 1),
        xyzi(&[]),
        transform(0, 1, None, [0, 0, 0]),
        group(1, &[2]),
        transform(2, 3, Some(17), [5, 5, 0]),
        shape(3, 0),
        rgba(&[RED, GREEN, BLUE]),
    ]);
    let doc = load(&bytes);
    let world = doc.voxels.as_ref().unwrap();
    // 1 x 3 x 1: C = 3. The first voxel ends at y = 0 (grid z = C + 2).
    assert_eq!(
        world.active_box,
        IBox3 {
            min: I3::splat(3),
            max: I3::new(4, 4, 6)
        }
    );
    assert_eq!(world.read(I3::new(3, 3, 5)), ARGB_RED);
    assert_eq!(world.read(I3::new(3, 3, 4)), ARGB_GREEN);
    assert_eq!(world.read(I3::new(3, 3, 3)), ARGB_BLUE);
}

#[test]
fn models_that_do_not_fit_leave_the_first_one() {
    let limits = Limits {
        voxel_budget: voxel_world_bytes(2, 1).unwrap(),
        ..Limits::default()
    };
    let imported = import_with(&two_models(None), Path::new("a.vox"), &limits).unwrap();
    assert_eq!(imported.notices, [Notice::ModelsDoNotFit { models: 2 }]);
    let doc = open(imported);
    assert_eq!(contents(doc.voxels.as_ref().unwrap()).len(), 1);
    assert_eq!(doc.voxels.as_ref().unwrap().res, 2);
}

#[test]
fn several_models_without_a_scene_graph_leave_the_first_one() {
    let imported = import(
        &file(&[
            size(1, 1, 1),
            xyzi(&[[0, 0, 0, 1]]),
            size(1, 1, 1),
            xyzi(&[[0, 0, 0, 2]]),
            rgba(&[RED, GREEN]),
        ]),
        Path::new("a.vox"),
    )
    .unwrap();
    assert_eq!(imported.notices, [Notice::NoSceneGraph { models: 2 }]);
    let found = contents(open(imported).voxels.as_ref().unwrap());
    assert_eq!(found.iter().map(|c| c.1).collect::<Vec<_>>(), [ARGB_RED]);
}

#[test]
fn a_looping_scene_graph_is_a_notice_not_a_hang() {
    for graph in [
        // A transform that is its own child.
        vec![transform(0, 0, None, [0, 0, 0])],
        // A group that holds itself twice: the walk would fan out for ever.
        vec![transform(0, 1, None, [0, 0, 0]), group(1, &[1, 1])],
        // A root that is missing.
        vec![transform(7, 3, None, [0, 0, 0])],
    ] {
        let mut chunks = vec![
            size(1, 1, 1),
            xyzi(&[[0, 0, 0, 1]]),
            size(1, 1, 1),
            xyzi(&[[0, 0, 0, 2]]),
        ];
        chunks.extend(graph);
        let imported = import(&file(&chunks), Path::new("a.vox")).unwrap();
        assert_eq!(imported.notices, [Notice::SceneGraphBroken { models: 2 }]);
        assert_eq!(imported.doc.voxel_replay.unwrap().cells.len(), 1);
    }
}

#[test]
fn instances_of_one_model_can_fan_out_only_so_far() {
    // 40 groups of 40 children each point at one shape: 1600 instances of a 2 x 2 x 2 model.
    let mut chunks = vec![
        size(2, 2, 2),
        xyzi(&[[0, 0, 0, 1]]),
        size(1, 1, 1),
        xyzi(&[[0, 0, 0, 1]]),
        transform(0, 1, None, [0, 0, 0]),
        group(1, &(0..40).map(|i| 10 + i).collect::<Vec<_>>()),
        shape(5, 0),
    ];
    for i in 0..40 {
        chunks.push(group(10 + i, &[5; 40]));
    }
    let limits = Limits {
        max_visits: 100,
        ..Limits::default()
    };
    let imported = import_with(&file(&chunks), Path::new("a.vox"), &limits).unwrap();
    assert_eq!(imported.notices, [Notice::SceneGraphBroken { models: 2 }]);
}

// --- Hostile files --------------------------------------------------------------------------

#[test]
fn bad_headers_are_messages() {
    let good = file(&[size(1, 1, 1), xyzi(&[[0, 0, 0, 1]])]);
    assert_eq!(
        import(&[], Path::new("a.vox")).unwrap_err(),
        VoxError::NotVox
    );
    assert_eq!(
        import(b"VOX ", Path::new("a.vox")).unwrap_err(),
        VoxError::NotVox
    );
    let mut wrong = good.clone();
    wrong[..4].copy_from_slice(b"RIFF");
    assert_eq!(
        import(&wrong, Path::new("a.vox")).unwrap_err(),
        VoxError::NotVox
    );
    let mut no_main = good.clone();
    no_main[8..12].copy_from_slice(b"MAIM");
    assert!(matches!(
        import(&no_main, Path::new("a.vox")),
        Err(VoxError::Corrupted(_))
    ));
    let only_main = file(&[]);
    assert_eq!(
        import(&only_main, Path::new("a.vox")).unwrap_err(),
        VoxError::SizeNotFound
    );
    assert_eq!(
        import(&file(&[size(1, 1, 1)]), Path::new("a.vox")).unwrap_err(),
        VoxError::VoxelsNotFound
    );
    assert_eq!(
        import(&file(&[xyzi(&[[0, 0, 0, 1]])]), Path::new("a.vox")).unwrap_err(),
        VoxError::SizeNotFound
    );
}

#[test]
fn huge_and_empty_sizes_are_refused_before_anything_is_allocated() {
    for (x, y, z) in [(u32::MAX, u32::MAX, u32::MAX), (0, 1, 1), (4097, 1, 1)] {
        let bytes = file(&[size(x, y, z), xyzi(&[[0, 0, 0, 1]])]);
        assert_eq!(
            import(&bytes, Path::new("a.vox")).unwrap_err(),
            VoxError::BadSize(x, y, z)
        );
    }
    // Within the dimension limit, but a grid of 3 x 4096 voxels per axis is over the budget.
    let bytes = file(&[size(4096, 1, 1), xyzi(&[[0, 0, 0, 1]])]);
    assert_eq!(
        import(&bytes, Path::new("a.vox")).unwrap_err(),
        VoxError::Limit("voxel memory")
    );
    // A tight budget refuses an ordinary model.
    let limits = Limits {
        voxel_budget: 8,
        ..Limits::default()
    };
    let bytes = file(&[size(20, 1, 1), xyzi(&[[0, 0, 0, 1]])]);
    assert!(import_with(&bytes, Path::new("a.vox"), &limits).is_err());
}

#[test]
fn a_voxel_count_larger_than_its_chunk_is_corrupt() {
    let mut content = u32::MAX.to_le_bytes().to_vec();
    content.extend_from_slice(&[0, 0, 0, 1]);
    let bytes = file(&[size(1, 1, 1), chunk(b"XYZI", &content)]);
    assert!(matches!(
        import(&bytes, Path::new("a.vox")),
        Err(VoxError::Corrupted(_))
    ));
    // The count of a file over the voxel limit is refused too.
    let limits = Limits {
        max_voxels: 1,
        ..Limits::default()
    };
    let bytes = file(&[size(2, 1, 1), xyzi(&[[0, 0, 0, 1], [1, 0, 0, 1]])]);
    assert_eq!(
        import_with(&bytes, Path::new("a.vox"), &limits).unwrap_err(),
        VoxError::Limit("voxel records")
    );
}

#[test]
fn a_file_over_the_size_limit_is_refused() {
    let limits = Limits {
        max_file: 10,
        ..Limits::default()
    };
    let bytes = file(&[size(1, 1, 1), xyzi(&[[0, 0, 0, 1]])]);
    assert_eq!(
        import_with(&bytes, Path::new("a.vox"), &limits).unwrap_err(),
        VoxError::Limit("the file size")
    );
}

#[test]
fn chunk_sizes_past_the_end_of_the_file_are_truncation() {
    let good = file(&[size(1, 1, 1), xyzi(&[[0, 0, 0, 1]])]);
    for cut in 1..good.len() {
        let result = import(&good[..cut], Path::new("a.vox"));
        // A cut inside the file is an error, never a panic; a cut that ends a whole chunk
        // may leave a file without voxels.
        assert!(result.is_err(), "cut at {cut}");
    }
    // A chunk that claims 4 GiB of content.
    let mut huge = b"SIZE".to_vec();
    huge.extend_from_slice(&u32::MAX.to_le_bytes());
    huge.extend_from_slice(&0_u32.to_le_bytes());
    assert!(matches!(
        import(&file(&[huge]), Path::new("a.vox")),
        Err(VoxError::Truncated(_))
    ));
    // The same for the children size of a known chunk.
    let mut children = b"NOTE".to_vec();
    children.extend_from_slice(&0_u32.to_le_bytes());
    children.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        import(&file(&[children]), Path::new("a.vox")),
        Err(VoxError::Truncated(_))
    ));
}

#[test]
fn many_empty_chunks_are_walked_once() {
    let mut chunks = vec![size(1, 1, 1), xyzi(&[[0, 0, 0, 1]])];
    chunks.extend((0..50_000).map(|_| chunk(b"NOTE", &[])));
    assert!(import(&file(&chunks), Path::new("a.vox")).is_ok());
}

#[test]
fn damaged_scene_nodes_are_errors() {
    // A dictionary that claims more pairs than the chunk holds, a group with a huge child count.
    let mut dict_claim = int(0).to_vec();
    dict_claim.extend_from_slice(&u32::MAX.to_le_bytes());
    let mut children_claim = int(0).to_vec();
    children_claim.extend(dict(&[]));
    children_claim.extend(int(i32::MAX));
    for bad in [
        chunk(b"nTRN", &dict_claim),
        chunk(b"nGRP", &children_claim),
        chunk(b"nSHP", &[1, 2]),
    ] {
        let bytes = file(&[size(1, 1, 1), xyzi(&[[0, 0, 0, 1]]), bad]);
        assert!(matches!(
            import(&bytes, Path::new("a.vox")),
            Err(VoxError::Corrupted(_))
        ));
    }
}

#[test]
fn flipped_bytes_never_panic() {
    let seed = file(&[
        size(3, 2, 2),
        xyzi(&[[0, 0, 0, 1], [1, 1, 1, 2], [2, 0, 1, 3]]),
        size(2, 2, 2),
        xyzi(&[[1, 1, 1, 2]]),
        transform(0, 1, None, [0, 0, 0]),
        group(1, &[2, 4]),
        transform(2, 3, None, [0, 0, 0]),
        shape(3, 0),
        transform(4, 5, Some(17), [4, 0, 0]),
        shape(5, 1),
        rgba(&[RED, GREEN, BLUE]),
    ]);
    let limits = Limits {
        max_voxels: 1 << 16,
        voxel_budget: 1 << 22,
        ..Limits::default()
    };
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..3000 {
        let mut bytes = seed.clone();
        for _ in 0..1 + next() % 4 {
            let at = usize::try_from(next() % bytes.len() as u64).unwrap();
            bytes[at] = next().to_le_bytes()[0];
        }
        let _ = import_with(&bytes, Path::new("a.vox"), &limits);
    }
}

// --- Export ---------------------------------------------------------------------------------

fn world_in(res: u32, min: [i32; 3], max: [i32; 3]) -> VoxelWorld {
    VoxelWorld::new(
        res,
        false,
        IBox3 {
            min: I3::from_array(min),
            max: I3::from_array(max),
        },
    )
}

#[test]
fn an_export_reads_back_with_an_independent_reader() {
    let mut world = world_in(16, [10, 20, 30], [14, 22, 33]);
    world.write_full(I3::new(10, 20, 32), ARGB_RED);
    world.write_full(I3::new(13, 21, 30), ARGB_GREEN);
    world.write_full(I3::new(11, 21, 31), 0x80AB_CDEF);
    let out = export(&world, &ExportOptions::default()).unwrap();
    assert!(out.notices.is_empty());

    let data = dot_vox::load_bytes(&out.bytes).unwrap();
    assert_eq!(data.version, 150);
    assert_eq!(data.models.len(), 1);
    let model = data.models.first().unwrap();
    // Region 4 x 2 x 3: SIZE is (dx, dz, dy).
    assert_eq!((model.size.x, model.size.y, model.size.z), (4, 3, 2));
    let voxels: Vec<_> = model.voxels.iter().map(|v| (v.x, v.y, v.z, v.i)).collect();
    // Grid z is mirrored in the region: z 32 is y 0 and z 30 is y 2; grid y is VOX z.
    // The reader counts colour indices from 0, the file from 1.
    assert_eq!(voxels, [(0, 0, 0, 0), (1, 1, 1, 2), (3, 2, 1, 1)]);
    assert_eq!(data.palette.len(), 256);
    let entry = |i: usize| data.palette.get(i).map(|c| (c.r, c.g, c.b, c.a));
    assert_eq!(entry(0), Some((255, 0, 0, 255)));
    assert_eq!(entry(1), Some((0, 255, 0, 255)));
    assert_eq!(entry(2), Some((0xAB, 0xCD, 0xEF, 0x80)));
    assert_eq!(entry(3), Some((0xA0, 0xD2, 0xFF, 255)));
}

#[test]
fn the_palette_follows_the_world_palette_and_the_output_is_deterministic() {
    let mut world = world_in(16, [0, 0, 0], [3, 1, 1]);
    // Written in the order blue, red, green; the palette order is the write order.
    world.write_full(I3::new(0, 0, 0), ARGB_BLUE);
    world.write_full(I3::new(1, 0, 0), ARGB_RED);
    world.write_full(I3::new(2, 0, 0), ARGB_GREEN);
    let first = export(&world, &ExportOptions::default()).unwrap();
    let second = export(&world.clone(), &ExportOptions::default()).unwrap();
    assert_eq!(first.bytes, second.bytes);
    let data = dot_vox::load_bytes(&first.bytes).unwrap();
    let colours: Vec<_> = data
        .palette
        .iter()
        .take(3)
        .map(|c| (c.r, c.g, c.b))
        .collect();
    assert_eq!(colours, [(0, 0, 255), (255, 0, 0), (0, 255, 0)]);
}

#[test]
fn a_world_round_trips() {
    let bytes = file(&[
        size(5, 4, 3),
        xyzi(&[
            [0, 0, 0, 1],
            [4, 3, 2, 2],
            [2, 1, 1, 3],
            [1, 2, 0, 2],
            [3, 0, 2, 1],
        ]),
        rgba(&[RED, GREEN, BLUE]),
    ]);
    let doc = load(&bytes);
    let world = doc.voxels.as_ref().unwrap();
    let out = export(world, &ExportOptions::default()).unwrap();
    // The file is the same model, byte for byte in its voxel records and sizes.
    let again = load(&out.bytes);
    let back = again.voxels.as_ref().unwrap();
    assert_eq!(back.res, world.res);
    assert_eq!(back.active_box, world.active_box);
    assert_eq!(contents(back), contents(world));
    assert_eq!(
        export(back, &ExportOptions::default()).unwrap().bytes,
        out.bytes
    );
    assert_eq!(contents(world).len(), 5);
}

#[test]
fn an_empty_region_or_world_is_a_message() {
    let world = world_in(16, [0, 0, 0], [4, 4, 4]);
    assert_eq!(
        export(&world, &ExportOptions::default()).unwrap_err(),
        VoxError::NoVoxels
    );
    let flat = world_in(16, [3, 0, 0], [3, 4, 4]);
    assert_eq!(
        export(&flat, &ExportOptions::default()).unwrap_err(),
        VoxError::NoVoxels
    );
}

#[test]
fn a_region_over_256_is_refused_and_256_is_written() {
    let mut world = world_in(128, [0, 0, 0], [257, 10, 10]);
    world.write_full(I3::new(0, 0, 0), ARGB_RED);
    assert_eq!(
        export(&world, &ExportOptions::default()).unwrap_err(),
        VoxError::TooLarge(257, 10, 10)
    );
    let mut world = world_in(128, [0, 0, 0], [256, 256, 256]);
    world.write_full(I3::new(255, 255, 255), ARGB_RED);
    let out = export(&world, &ExportOptions::default()).unwrap();
    let data = dot_vox::load_bytes(&out.bytes).unwrap();
    let model = data.models.first().unwrap();
    assert_eq!((model.size.x, model.size.y, model.size.z), (256, 256, 256));
    let v = model.voxels.first().unwrap();
    assert_eq!((v.x, v.y, v.z), (255, 0, 255));
}

/// A world with one voxel per colour, `n` colours in a row.
fn rainbow(n: u32) -> VoxelWorld {
    let width = i32::try_from(n).unwrap();
    let mut world = world_in(128, [0, 0, 0], [width, 1, 1]);
    for i in 0..n {
        let value = 0xFF00_0000 | (i * 0x0001_0203 % 0x00FF_FFFF);
        world.write_full(I3::new(i32::try_from(i).unwrap(), 0, 0), value);
    }
    world
}

#[test]
fn the_palette_limit_is_255_colours() {
    let ok = export(&rainbow(255), &ExportOptions::default()).unwrap();
    let data = dot_vox::load_bytes(&ok.bytes).unwrap();
    assert_eq!(data.models.first().unwrap().voxels.len(), 255);

    assert_eq!(
        export(&rainbow(256), &ExportOptions::default()).unwrap_err(),
        VoxError::TooManyColours(256)
    );
}

#[test]
fn quantizing_keeps_the_most_used_colours_and_maps_the_rest_to_the_nearest() {
    // 255 colours used once each, then a 256th that differs from the first by one blue step.
    let mut world = rainbow(255);
    world.set_active_box(IBox3 {
        min: I3::ZERO,
        max: I3::new(256, 1, 1),
    });
    let first = world.read(I3::ZERO);
    let near_first = first + 1;
    world.write_full(I3::new(255, 0, 0), near_first);
    let options = ExportOptions { quantize: true };
    let out = export(&world, &options).unwrap();
    assert_eq!(out.notices, [Notice::Quantized { from: 256, to: 255 }]);
    let data = dot_vox::load_bytes(&out.bytes).unwrap();
    let model = data.models.first().unwrap();
    assert_eq!(model.voxels.len(), 256);
    // Every colour is either kept or replaced by its nearest neighbour; the 256th voxel and
    // the first share an entry because they differ by one blue step only.
    let index_of = |x: u8| model.voxels.iter().find(|v| v.x == x).map(|v| v.i);
    assert_eq!(index_of(255), index_of(0));
    // The output stays deterministic.
    assert_eq!(export(&world, &options).unwrap().bytes, out.bytes);
}
