//! The canonical dump with the RawTexture levels (native 10.2).

mod common;

use cuboid_fmt_cubik::dump;
use cuboid_testkit::dump::sha256_hex;

#[test]
fn a_raw_texture_is_dumped_with_its_level_0_and_the_length_of_the_tail() {
    let payload = common::full_payload();
    let text = dump::payload(&payload).unwrap();
    let root = common::full_root();
    let first = &root.tx.as_ref().unwrap()[0].tx;

    // 16 x 16 RGB24 with a mip chain: 768 bytes of level 0 and 255 of tail.
    let level0 = sha256_hex(&first.r[..768]);
    let lines: Vec<&str> = text.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.starts_with("tx[0].tx.r: bytes = len=1023 "))
        .expect("the r line");
    assert_eq!(
        lines[at + 1],
        format!("tx[0].tx.r.level0: bytes = len=768 sha256={level0}")
    );
    assert_eq!(lines[at + 2], "tx[0].tx.r.tail: i32 = 255");

    assert!(text.contains("tx[1].tx.r.tail: i32 = 0"));
    assert!(text.contains("me[0].tx.r.level0: bytes = len=16 "));
    assert!(text.contains("me[1].sm[0].tx.r.level0: bytes = len=4 "));
    assert!(text.starts_with("v: i32 = 1\n"));
}

#[test]
fn a_payload_without_textures_dumps_as_the_plain_lines() {
    let payload =
        cuboid_fmt_cubik::encode::encode(&cuboid_fmt_cubik::records::Root::default()).unwrap();
    let doc = cuboid_bson::RawDoc::parse(&payload, &cuboid_bson::Limits::DEFAULT).unwrap();
    assert_eq!(
        dump::payload(&payload).unwrap(),
        cuboid_bson::dump(&doc).unwrap()
    );
}
