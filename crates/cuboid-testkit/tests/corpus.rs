//! The kit against the real corpus in `tests/golden/testkit`: the goldens are blessed with
//! `cargo xtask bless -p cuboid-testkit` and show how a format crate uses the harness.

use cuboid_testkit::dump::Dump;
use cuboid_testkit::golden::Golden;
use cuboid_testkit::raster::{Raster, RasterTolerance, SSIM_MIN};

#[test]
fn a_canonical_dump_matches_its_golden() {
    let mut dump = Dump::new();
    dump.doc("mp", |d| {
        d.str("n", "Untitled \"model\"");
        d.i32("o", -3);
        d.i64("size", 5_000_000_000);
        d.doc("g", |d| {
            d.bool("rer", true);
            d.f32("grid", 0.125);
            d.f64("ox", -1.5);
        });
    });
    dump.array("tx", |d| {
        d.doc(0, |d| d.bytes("r", &[0, 1, 2, 3, 4, 5, 6, 7]));
        d.doc(1, |d| d.null("r"));
    });
    dump.array("el", |_| {});
    dump.i32("a.b: c", 1);
    Golden::new("testkit/dump.txt")
        .expect_text(&dump.finish())
        .unwrap();
}

#[test]
fn a_synthetic_picture_matches_its_golden_at_tiers_p2_and_p3() {
    let picture = Raster::from_fn(32, 16, |x, y| {
        let channel = |v: u32| u8::try_from(v).expect("below 256");
        [channel(x * 8), channel(y * 16), 128, 255]
    });
    let golden = Golden::new("testkit/gradient.png");
    golden
        .expect_raster(&picture, &RasterTolerance::default())
        .unwrap();
    golden.expect_ssim(&picture, SSIM_MIN).unwrap();
}
