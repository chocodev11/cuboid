//! Small files that declare huge counts, and texture paths that must never be read (native 8.1
//! item 4, review of W1.7). The counts are checked against one budget for the whole file, before
//! anything is allocated for the items they count.

use std::cell::RefCell;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cuboid_bson::DocWriter;
use cuboid_fmt_cubik::cub::{
    self, CubError, DiskFiles, MAX_TEXTURE_FILE, Notice, Refusal, TextureFiles,
};
use cuboid_fmt_cubik::{CubikError, Limits, container, decode, load_bytes};

/// A payload around the elements that `build` writes.
fn root(build: impl FnOnce(&mut DocWriter)) -> Vec<u8> {
    let mut w = DocWriter::new();
    let top = w.begin_root();
    build(&mut w);
    w.end(top);
    w.finish().expect("a payload below 2 GiB")
}

/// `meshes` meshes with `per_mesh` empty sub-meshes each. An empty sub-mesh is an 8-byte item that
/// repeats exactly, so the archive of the payload is tiny.
fn sub_mesh_payload(meshes: usize, per_mesh: usize) -> Vec<u8> {
    root(|w| {
        let me = w.begin_array(b"me\x00");
        for i in 0..meshes {
            let mesh = w.begin_doc(i);
            let sm = w.begin_array(b"sm\x00");
            for _ in 0..per_mesh {
                // Cubik ignores the keys of array items, so every item may be called "0".
                let item = w.begin_doc(0usize);
                w.end(item);
            }
            w.end(sm);
            w.end(mesh);
        }
        w.end(me);
    })
}

fn write_list(w: &mut DocWriter, key: &'static [u8], count: usize) {
    let list = w.begin_array(key);
    for i in 0..count {
        let item = w.begin_doc(i);
        w.end(item);
    }
    w.end(list);
}

/// One mesh whose `me.v` has `vertices` empty items and whose `me.t` and `sm[0].t` have
/// `indices` integers each.
fn mesh_data_payload(vertices: usize, indices: usize) -> Vec<u8> {
    root(|w| {
        let me = w.begin_array(b"me\x00");
        let mesh = w.begin_doc(0usize);
        let data = w.begin_doc(b"me\x00");
        write_list(w, b"v\x00", vertices);
        let t = w.begin_array(b"t\x00");
        for i in 0..indices {
            w.i32(i, 0);
        }
        w.end(t);
        w.end(data);
        let sm = w.begin_array(b"sm\x00");
        let item = w.begin_doc(0usize);
        let t = w.begin_array(b"t\x00");
        for i in 0..indices {
            w.i32(i, 0);
        }
        w.end(t);
        w.end(item);
        w.end(sm);
        w.end(mesh);
        w.end(me);
    })
}

fn payload_error(bytes: &[u8], limits: &Limits) -> String {
    match decode::decode(bytes, limits) {
        Ok(_) => String::new(),
        Err(err) => err.to_string(),
    }
}

// --- .cubik --------------------------------------------------------------------------------------

#[test]
fn sub_meshes_are_counted_over_all_meshes() {
    // Each mesh is within the limit; together they are not. Before the shared budget, the
    // archive of this payload asked for far more memory than its size says.
    let limits = Limits::default();
    let per_mesh = limits.max_objects as usize / 4;
    let payload = sub_mesh_payload(6, per_mesh);
    assert!(decode::decode(&sub_mesh_payload(1, per_mesh), &limits).is_ok());

    let archive = container::write(&payload, None).expect("a small archive");
    assert!(
        archive.len() < 100_000,
        "the archive is small: {}",
        archive.len()
    );
    match load_bytes(&archive, &limits) {
        Err(CubikError::Payload(err)) => {
            assert!(err.to_string().contains("sub-meshes"), "{err}");
        }
        other => panic!("expected a limit error, got {other:?}"),
    }
}

#[test]
fn elements_meshes_and_sub_meshes_share_one_budget() {
    let limits = Limits {
        max_objects: 10,
        ..Limits::default()
    };
    assert!(
        payload_error(&sub_mesh_payload(2, 4), &limits).is_empty(),
        "2 + 8 fit"
    );
    assert!(payload_error(&sub_mesh_payload(2, 5), &limits).contains("more than 10"));

    let both = root(|w| {
        write_list(w, b"el\x00", 6);
        write_list(w, b"me\x00", 5);
    });
    assert!(payload_error(&both, &limits).contains("more than 10"));
    let both = root(|w| {
        write_list(w, b"el\x00", 5);
        write_list(w, b"me\x00", 5);
    });
    assert!(payload_error(&both, &limits).is_empty());
}

#[test]
fn textures_are_counted_in_the_file() {
    let limits = Limits {
        max_textures: 3,
        ..Limits::default()
    };
    let textures = |count: usize| {
        root(|w| {
            let tx = w.begin_array(b"tx\x00");
            for _ in 0..count {
                let item = w.begin_doc(0usize);
                let image = w.begin_doc(b"tx\x00");
                w.end(image);
                w.end(item);
            }
            w.end(tx);
        })
    };
    assert!(payload_error(&textures(3), &limits).is_empty());
    let error = payload_error(&textures(4), &limits);
    assert!(error.contains("more than 3 textures"), "{error}");
}

#[test]
fn the_mesh_data_arrays_are_counted_over_the_file() {
    let limits = Limits {
        max_mesh_items: 100,
        ..Limits::default()
    };
    assert!(payload_error(&mesh_data_payload(40, 30), &limits).is_empty());
    let error = payload_error(&mesh_data_payload(40, 31), &limits);
    assert!(error.contains("mesh data arrays"), "{error}");
    let error = payload_error(&mesh_data_payload(101, 0), &limits);
    assert!(error.contains("mesh data arrays"), "{error}");
}

// --- .cub ----------------------------------------------------------------------------------------

/// Counts the reads; no file exists.
#[derive(Default)]
struct Probe(RefCell<Vec<PathBuf>>);

impl TextureFiles for Probe {
    fn read(&self, path: &Path) -> Result<Option<Vec<u8>>, Refusal> {
        self.0.borrow_mut().push(path.to_path_buf());
        Ok(None)
    }
}

fn cub_bytes(text: &str) -> Vec<u8> {
    cub::deobfuscate(text.as_bytes()).into_bytes()
}

fn import(
    text: &str,
    limits: &Limits,
    files: &dyn TextureFiles,
) -> Result<cub::CubImport, CubError> {
    cub::import_with(&cub_bytes(text), Path::new("dir/m.cub"), files, limits)
}

fn limit_of(result: Result<cub::CubImport, CubError>) -> &'static str {
    match result {
        Err(CubError::Limit(what)) => what,
        Err(other) => panic!("expected a limit error, got {other}"),
        Ok(_) => panic!("the file was imported"),
    }
}

#[test]
fn a_cub_with_a_million_elements_is_refused_before_they_are_built() {
    // 3 MB of text; the importer used to build a full element for each object.
    let text = format!(r#"{{"elements":[{}]}}"#, vec!["{}"; 1_000_000].join(","));
    assert!(text.len() < 4_000_000);
    let probe = Probe::default();
    let started = Instant::now();
    assert_eq!(
        limit_of(import(&text, &Limits::default(), &probe)),
        "elements"
    );
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_cub_over_the_text_limit_is_refused_before_it_is_decoded() {
    let limits = Limits {
        max_text: 100,
        ..Limits::default()
    };
    let text = format!(r#"{{"elements":[],"__comment":"{}"}}"#, "x".repeat(200));
    assert_eq!(
        limit_of(import(&text, &limits, &Probe::default())),
        "the file size"
    );
}

#[test]
fn texture_entries_are_counted_before_any_file_is_looked_for() {
    let limits = Limits {
        max_textures: 20,
        ..Limits::default()
    };
    let keys: Vec<String> = (0..21).map(|i| format!(r#""k{i}":"k{i}.png""#)).collect();
    let text = format!(
        r#"{{"textures_path":{{{}}},"elements":[]}}"#,
        keys.join(",")
    );
    let probe = Probe::default();
    assert_eq!(limit_of(import(&text, &limits, &probe)), "texture entries");
    assert!(probe.0.borrow().is_empty(), "no file was looked for");

    // The labels that faces name count as well.
    let element = |i: usize| {
        format!(r##"{{"from":[0,0,0],"to":[1,1,1],"faces":{{"up":{{"texture":"#f{i}"}}}}}}"##)
    };
    let elements: Vec<String> = (0..21).map(element).collect();
    let text = format!(r#"{{"elements":[{}]}}"#, elements.join(","));
    assert_eq!(
        limit_of(import(&text, &limits, &Probe::default())),
        "texture entries"
    );
}

#[test]
fn many_new_face_textures_import_in_linear_time() {
    // Every element names a label of its own. The label lookup was linear in the number of
    // labels, which made 80 000 of them take 23 s.
    let count = 65_000;
    let elements: Vec<String> = (0..count)
        .map(|i| {
            format!(r##"{{"from":[0,0,0],"to":[1,1,1],"faces":{{"up":{{"texture":"#f{i}"}}}}}}"##)
        })
        .collect();
    let text = format!(r#"{{"elements":[{}]}}"#, elements.join(","));
    let started = Instant::now();
    let imported = import(&text, &Limits::default(), &Probe::default()).expect("within the limits");
    let elapsed = started.elapsed();
    let textures = imported.doc.sections.textures.as_ref().expect("textures");
    assert_eq!(textures.entries.len(), count);
    assert!(elapsed < Duration::from_secs(8), "{elapsed:?}");
}

#[test]
fn many_declared_textures_import_in_linear_time() {
    let count = 60_000;
    let keys: Vec<String> = (0..count)
        .map(|i| format!(r#""k{i}":"missing/k{i}.png""#))
        .collect();
    let text = format!(
        r#"{{"textures_path":{{{}}},"elements":[]}}"#,
        keys.join(",")
    );
    let started = Instant::now();
    let imported = import(&text, &Limits::default(), &Probe::default()).expect("within the limits");
    let elapsed = started.elapsed();
    let textures = imported.doc.sections.textures.as_ref().expect("textures");
    assert_eq!(textures.entries.len(), count);
    assert!(elapsed < Duration::from_secs(8), "{elapsed:?}");
}

#[test]
fn voxel_cells_and_the_grid_are_limited() {
    let limits = Limits {
        max_array: 3,
        ..Limits::default()
    };
    let cells = |n: usize| vec!["[0,0,0,1]"; n].join(",");
    let text = |n: usize| {
        format!(
            r#"{{"elements":[],"voxel":{{"resolution":4,"voxels":[{}]}}}}"#,
            cells(n)
        )
    };
    assert!(import(&text(3), &limits, &Probe::default()).is_ok());
    assert_eq!(
        limit_of(import(&text(4), &limits, &Probe::default())),
        "voxel cells"
    );

    let huge = r#"{"elements":[],"voxel":{"resolution":100000,"voxels":[]}}"#;
    assert_eq!(
        limit_of(import(huge, &Limits::default(), &Probe::default())),
        "voxel memory"
    );
}

#[test]
fn a_key_or_an_id_that_is_a_rooted_path_is_not_joined_to_the_folder() {
    let probe = Probe::default();
    let text = r#"{"elements":[],"textures_path":{"/abs/key":"a.png","C:/abs/key":"b.png","u":":/abs/id","w":":C:/abs/id"}}"#;
    import(text, &Limits::default(), &probe).expect("the import goes on");
    for path in probe.0.borrow().iter() {
        assert!(path.starts_with("dir"), "{}", path.display());
    }
}

// --- Texture files that are never read ---------------------------------------------------------

#[test]
fn network_device_and_special_paths_are_refused_in_both_slash_forms() {
    let network = [
        r"\\host\share\t.png",
        "//host/share/t.png",
        r"\\192.0.2.1\share\t.png",
        "//192.0.2.1/share/t.png",
        r"\/host/share/t.png",
        r"/\host\share\t.png",
        r"\\host",
        "//",
        r"\\.host\share\t.png",
    ];
    let device = [
        r"\\.\COM1",
        "//./COM1",
        r"\\.\pipe\name",
        "//./pipe/name",
        r"\\?\C:\dir\t.png",
        "//?/C:/dir/t.png",
        r"\\?\UNC\host\share\t.png",
        "//?/UNC/host/share/t.png",
        r"\??\C:\dir\t.png",
        "/??/C:/dir/t.png",
        r"\\.",
        "//?",
        "CON",
        "con",
        "NUL",
        "Nul.png",
        "AUX.txt",
        "PRN",
        "COM1",
        "com9.png",
        "LPT1",
        "lpt0.png",
        "CONIN$",
        "CONOUT$",
        r"dir\NUL",
        "dir/com1.png",
        r"C:\dir\NUL.png",
        "C:/dir/aux",
        "textures/con/t.png",
        "NUL .png",
        "t.png:stream",
        r"C:\dir\t.png:stream",
        "NUL:",
        "C:t.png",
        "C:",
        "/dev/zero",
        "/dev/null",
        "/dev",
    ];
    for path in network {
        assert_eq!(
            DiskFiles.read(Path::new(path)),
            Err(Refusal::Network),
            "{path}"
        );
    }
    for path in device {
        assert_eq!(
            DiskFiles.read(Path::new(path)),
            Err(Refusal::Device),
            "{path}"
        );
    }
}

#[test]
fn only_regular_files_below_the_cap_are_read() {
    let dir = tempfile::tempdir().expect("a temporary folder");

    let small = dir.path().join("small.png");
    std::fs::write(&small, b"abc").expect("a file");
    assert_eq!(DiskFiles.read(&small), Ok(Some(b"abc".to_vec())));

    // Missing files and folders are not found; neither is an error.
    assert_eq!(DiskFiles.read(&dir.path().join("missing.png")), Ok(None));
    assert_eq!(DiskFiles.read(dir.path()), Ok(None));

    // The size comes from the metadata, so a file over the cap is never opened for reading.
    let big = dir.path().join("big.png");
    File::create(&big)
        .and_then(|file| file.set_len(MAX_TEXTURE_FILE + 1))
        .expect("a sparse file");
    assert_eq!(DiskFiles.read(&big), Err(Refusal::TooLarge));
    File::options()
        .write(true)
        .open(&big)
        .and_then(|file| file.set_len(MAX_TEXTURE_FILE))
        .expect("a file at the cap");
    assert!(
        matches!(DiskFiles.read(&big), Ok(Some(bytes)) if bytes.len() as u64 == MAX_TEXTURE_FILE)
    );
}

#[test]
fn a_refused_texture_is_a_notice_and_the_import_goes_on() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let path = dir.path().join("m.cub");
    let text = r#"{
        "textures_path": {
            "net": "//192.0.2.1/share/t.png",
            "win": "\\host\share\x.png",
            "dev": "\\.\COM1",
            "con": "CON",
            "colon": ":\\host\share\id",
            "gone": "gone.png"
        },
        "elements": [{"from":[0,0,0],"to":[1,1,1]}]
    }"#;
    let imported = cub::import(&cub_bytes(text), &path, &DiskFiles).expect("the import goes on");

    let mut refused: Vec<(String, Refusal)> = imported
        .notices
        .iter()
        .filter_map(|notice| match notice {
            Notice::TextureRefused { key, reason, .. } => Some((key.clone(), *reason)),
            _ => None,
        })
        .collect();
    refused.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        refused,
        [
            ("colon".to_owned(), Refusal::Network),
            ("con".to_owned(), Refusal::Device),
            ("dev".to_owned(), Refusal::Device),
            ("net".to_owned(), Refusal::Network),
            ("win".to_owned(), Refusal::Network),
        ],
        "a texture that is only missing is not reported"
    );
    // All six textures are in the model, with the picture of the missing texture.
    let textures = imported.doc.sections.textures.as_ref().expect("textures");
    assert_eq!(textures.entries.len(), 6);
    assert!(
        textures
            .entries
            .iter()
            .all(|e| &*e.texture.name == "e_error")
    );
}
