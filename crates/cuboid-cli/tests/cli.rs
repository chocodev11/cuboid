//! The two commands on generated documents, and on the private samples (oracle O1).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use cuboid_fmt_cubik::records::{
    ElementRecord, FACE_NAMES, FaceRecord, FaceSetRecord, FaceUvRecord, Root,
};
use cuboid_fmt_cubik::{Limits, SaveOptions, container, decode, encode, save};
use cuboid_model::{CaptureScope, Categories, Document, Selection, TextureList, capture};
use cuboid_testkit::oracle::{self, Oracle};
use glam::{Vec2, Vec3};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cuboid-cli"))
        .args(args)
        .output()
        .expect("the binary starts")
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("UTF-8 temporary path")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exited normally")
}

fn element(label: &str, p: Vec3, s: Vec3) -> ElementRecord {
    let uv = FaceUvRecord {
        r: 0,
        tl: Vec2::new(0.0, 0.25),
        br: Vec2::new(0.5, 0.75),
    };
    ElementRecord {
        p,
        s,
        o: p,
        c: 0xFF33_6699_u32.cast_signed(),
        l: Some(label.to_owned()),
        f: Some(FaceSetRecord {
            faces: std::array::from_fn(|slot| FaceRecord {
                n: Some(FACE_NAMES[slot].to_owned()),
                a: true,
                ti: -1,
                c: 0,
                t: Some("texture".to_owned()),
                uv,
            }),
        }),
        suv: true,
        sh: true,
        ..ElementRecord::default()
    }
}

/// The root of the port's own save of a fresh document, with elements added.
fn root() -> Root {
    let doc = Document::new(TextureList::default());
    let snapshot = capture(
        &doc,
        &Selection::default(),
        CaptureScope {
            categories: Categories::ALL,
            selection_only: false,
        },
    );
    let options = SaveOptions {
        use_retention: false,
        ..SaveOptions::default()
    };
    let payload = save::save_payload(&snapshot, None, &options).expect("a fresh document saves");
    let mut root = decode::decode(&payload, &Limits::default())
        .expect("decodes")
        .root;
    root.el = Some(vec![
        element("first", Vec3::ZERO, Vec3::ONE),
        element(
            "second",
            Vec3::new(4.0, 0.0, -4.0),
            Vec3::new(2.0, 2.0, 2.0),
        ),
    ]);
    root
}

fn write_cubik(dir: &Path, name: &str, root: &Root) -> PathBuf {
    let payload = encode::encode(root).expect("encodes");
    let path = dir.join(name);
    fs::write(&path, container::write(&payload, None).expect("archive")).expect("writes");
    path
}

#[test]
fn dump_prints_the_canonical_lines_of_the_payload() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_cubik(dir.path(), "a.cubik", &root());
    let output = cli(&["dump", path_arg(&file)]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.starts_with("v: i32 = 1\n"), "{text}");
    assert!(text.contains("el[1].l: str = \"second\""), "{text}");
}

#[test]
fn dump_cuts_the_output_at_max_lines() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_cubik(dir.path(), "a.cubik", &root());
    let path = path_arg(&file);
    for args in [
        vec!["dump", "--max-lines", "3", path],
        vec!["dump", "--max-lines=3", path],
    ] {
        let output = cli(&args);
        assert_eq!(code(&output), 0, "{}", stderr(&output));
        assert_eq!(stdout(&output).lines().count(), 3);
        assert!(stderr(&output).contains("cut at 3 lines"));
    }
    let all = cli(&["dump", "--max-lines", "0", path_arg(&file)]);
    assert!(stdout(&all).lines().count() > 3);
    assert!(stderr(&all).is_empty());
}

#[test]
fn dump_reads_a_legacy_cub() {
    // The obfuscation is its own inverse, so it makes the file from the JSON.
    let json = r#"{"textures":{},"elements":[{"from":[0,0,0],"to":[4,4,4]}]}"#;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.cub");
    fs::write(&file, cuboid_fmt_cubik::cub::deobfuscate(json.as_bytes())).unwrap();
    let output = cli(&["dump", path_arg(&file)]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).contains("el[0].p: "), "{}", stdout(&output));
}

#[test]
fn dump_fails_with_2_on_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let garbage = dir.path().join("garbage.cubik");
    fs::write(&garbage, b"not a zip archive").unwrap();
    let bad_cub = dir.path().join("bad.cub");
    fs::write(&bad_cub, b"not a model").unwrap();
    for file in [
        garbage,
        bad_cub,
        dir.path().join("missing.cubik"),
        dir.path().to_path_buf(),
    ] {
        let output = cli(&["dump", path_arg(&file)]);
        assert_eq!(code(&output), 2, "{}", file.display());
        assert!(stdout(&output).is_empty());
        assert!(!stderr(&output).is_empty());
    }
}

#[test]
fn bad_command_lines_exit_with_1() {
    for args in [
        &["bogus"][..],
        &["dump"],
        &["dump", "a", "b"],
        &["dump", "--max-lines"],
        &["dump", "--max-lines", "x", "a"],
        &["dump", "--max-lines", "1", "--max-lines", "2", "a"],
        &["dump", "--nope", "a"],
        &["roundtrip"],
        &["roundtrip", "--out"],
    ] {
        let output = cli(args);
        assert_eq!(code(&output), 1, "{args:?}");
        assert!(stderr(&output).contains("usage:"), "{args:?}");
    }
    assert_eq!(code(&cli(&[])), 0);
    assert_eq!(code(&cli(&["--help"])), 0);
    assert!(stdout(&cli(&["--version"])).starts_with("cuboid-cli "));
}

#[test]
fn roundtrip_reports_an_unchanged_document_as_identical() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_cubik(dir.path(), "a.cubik", &root());
    let before = fs::read(&file).unwrap();
    let output = cli(&["roundtrip", path_arg(&file)]);
    assert_eq!(code(&output), 0, "{}{}", stdout(&output), stderr(&output));
    assert!(stdout(&output).starts_with("identical: "));
    assert_eq!(fs::read(&file).unwrap(), before, "the input is not touched");
}

#[test]
fn roundtrip_names_the_section_of_the_first_difference() {
    // A save writes the view of the document, never the null that this file holds.
    let mut root = root();
    root.vp = None;
    let dir = tempfile::tempdir().unwrap();
    let file = write_cubik(dir.path(), "a.cubik", &root);
    let output = cli(&["roundtrip", path_arg(&file)]);
    assert_eq!(code(&output), 3, "{}{}", stdout(&output), stderr(&output));
    let text = stdout(&output);
    assert!(
        text.starts_with("different: first differing offset "),
        "{text}"
    );
    assert!(text.contains("section `vp` (view)"), "{text}");
}

#[test]
fn roundtrip_out_writes_the_saved_file_and_refuses_the_input() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_cubik(dir.path(), "a.cubik", &root());
    let before = fs::read(&file).unwrap();
    let out = dir.path().join("saved.cubik");
    let output = cli(&["roundtrip", "--out", path_arg(&out), path_arg(&file)]);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert_eq!(fs::read(&file).unwrap(), before);
    let again = cli(&["roundtrip", path_arg(&out)]);
    assert_eq!(code(&again), 0, "{}", stderr(&again));

    let same = cli(&["roundtrip", "--out", path_arg(&file), path_arg(&file)]);
    assert_eq!(code(&same), 1);
    assert_eq!(fs::read(&file).unwrap(), before);
}

#[test]
fn roundtrip_fails_with_2_on_bad_input() {
    let dir = tempfile::tempdir().unwrap();
    let garbage = dir.path().join("garbage.cubik");
    fs::write(&garbage, b"not a zip archive").unwrap();
    for file in [garbage, dir.path().join("missing.cubik")] {
        assert_eq!(code(&cli(&["roundtrip", path_arg(&file)])), 2);
    }
}

#[test]
#[ignore = "oracle O1: CUBOID_PARITY_REFS"]
fn both_commands_accept_every_private_sample() {
    let Some(dir) = oracle::require(Oracle::ParityRefs) else {
        return;
    };
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .expect("readable folder")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "cubik" || ext == "cub")
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "the oracle folder holds no sample");
    for (n, file) in files.iter().enumerate() {
        let dump = cli(&["dump", "--max-lines", "0", path_arg(file)]);
        assert_eq!(code(&dump), 0, "sample {n}: dump");
        if file.extension().is_some_and(|ext| ext == "cubik") {
            let round = cli(&["roundtrip", path_arg(file)]);
            assert_eq!(code(&round), 0, "sample {n}: roundtrip");
        }
    }
}
