//! Import end to end on small generated folders: classification, metadata, dedupe, limits,
//! refusals, links, cancel and the summary view.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use cuboid_image::{Image, RgbaBuf, encode_png};
use cuboid_library::{
    Control, ImportOptions, Kind, Library, Limits, Meta, Query, Quiet, Scope, SuggestionKind,
};

fn png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
    let mut buf = RgbaBuf::new(w, h);
    for texel in buf.pixels.chunks_exact_mut(4) {
        texel.copy_from_slice(&color);
    }
    encode_png(&Image::from_rgba(buf)).expect("encodes")
}

fn write(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    fs::write(&path, bytes).expect("write");
    path
}

const MODEL: &[u8] =
    br#"{"elements": [{"from": [0, 0, 0], "to": [16, 16, 16]}], "textures": {"0": "block/a"}}"#;

/// A folder with one file of every kind and some junk.
fn sample(root: &Path) {
    write(root, "models/gun.json", MODEL);
    write(
        root,
        "models/item.json",
        br#"{"model": {"type": "minecraft:model", "model": "item/gun"}}"#,
    );
    write(root, "tex/stone.png", &png(16, 16, [120, 120, 120, 255]));
    write(
        root,
        "tex/stone.png.mcmeta",
        br#"{"animation": {"frames": [0, 1]}}"#,
    );
    write(root, "refs/photo.jpg", b"jpeg bytes");
    write(
        root,
        "lore/story.md",
        b"# Story\n\n## Weapons\n- Gun\n- Roller\n",
    );
    write(
        root,
        "pack.mcmeta",
        br#"{"pack": {"pack_format": 46, "description": "x"}}"#,
    );
    write(root, "lang/en_us.json", br#"{"item.gun": "Gun"}"#);
    write(root, "junk/a.dll", b"MZ");
    write(root, "junk/b.dll", b"MZ2");
    write(root, "junk/c.exe", b"MZ3");
}

fn library(root: &Path) -> Library {
    Library::new(root.join("lib"), Scope::Global)
}

fn import(library: &mut Library, sources: &[PathBuf]) -> cuboid_library::ImportReport {
    library.import(sources, &ImportOptions::default(), &Quiet)
}

#[test]
fn a_folder_is_imported_recursively_and_classified_by_content() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    sample(&src);
    let mut library = library(dir.path());
    let report = import(&mut library, &[src]);

    assert_eq!(report.added.len(), 7, "{report:?}");
    assert_eq!(
        library.counts(),
        [1, 1, 3, 1, 1],
        "models, textures, JSON, references, lore"
    );
    // One notice per extension, with the count.
    assert_eq!(
        report.skipped.notices(),
        ["Skipped 2 files: .dll", "Skipped 1 file: .exe"]
    );
    assert!(report.refused.is_empty() && report.failed.is_empty() && !report.cancelled);

    let texture = library
        .entries()
        .iter()
        .find(|e| e.kind == Kind::Texture)
        .expect("texture");
    let Some(Meta::Texture(meta)) = &texture.meta else {
        panic!("{texture:?}");
    };
    assert_eq!((meta.width, meta.height, meta.frames), (16, 16, 2));
    assert_eq!(meta.palette, [[120, 120, 120]]);
    assert!(
        texture.companion.is_some(),
        "the .png.mcmeta travels with the PNG"
    );
    let stored = library.file_path(texture).expect("path");
    assert!(stored.is_file());
    assert!(library.companion_path(texture).is_some_and(|p| p.is_file()));
    assert!(library.thumb_path(&texture.id).is_some_and(|p| p.is_file()));

    let model = library
        .entries()
        .iter()
        .find(|e| e.kind == Kind::Model)
        .expect("model");
    let Some(Meta::Model(meta)) = &model.meta else {
        panic!("{model:?}");
    };
    assert_eq!((meta.elements, meta.textures), (1, 1));
    assert_eq!(model.sha256.len(), 64);
    assert!(model.id == model.sha256[..16]);
    assert_eq!(model.licence, "unknown");

    // The index is on disk and reads back the same.
    let (again, warning) = Library::open(library.root(), Scope::Global).expect("opens");
    assert_eq!(warning, None);
    assert_eq!(again.entries(), library.entries());
}

#[test]
fn the_same_content_is_stored_once_whatever_its_name() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    write(&src, "a/model.json", MODEL);
    write(&src, "b/copy_of_model.json", MODEL);
    let mut library = library(dir.path());
    let first = import(&mut library, std::slice::from_ref(&src));
    assert_eq!((first.added.len(), first.duplicates), (1, 1));
    let again = import(&mut library, &[src]);
    assert_eq!((again.added.len(), again.duplicates), (0, 2));
    assert_eq!(library.entries().len(), 1);
    assert_eq!(
        fs::read_dir(library.root().join("files"))
            .expect("files")
            .count(),
        1
    );
}

#[test]
fn tags_and_licence_of_the_batch_go_on_every_new_entry() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = write(dir.path(), "src/gun.json", MODEL);
    let mut library = library(dir.path());
    let options = ImportOptions {
        tags: vec!["Weapon".to_owned(), "weapon".to_owned(), " ".to_owned()],
        licence: "CC0".to_owned(),
        ..ImportOptions::default()
    };
    library.import(&[file], &options, &Quiet);
    let entry = &library.entries()[0];
    assert_eq!(entry.tags, ["Weapon"]);
    assert_eq!(entry.licence, "CC0");
}

#[test]
fn the_limits_stop_the_import_and_say_so() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    for i in 0..5 {
        write(
            &src,
            &format!("f{i}.md"),
            format!("# note {i}\n").as_bytes(),
        );
    }
    write(&src, "big.md", &vec![b'x'; 5000]);
    let run = |limits: Limits| {
        let mut library = Library::new(
            dir.path().join(format!("lib{}", limits.max_files)),
            Scope::Global,
        );
        let options = ImportOptions {
            limits,
            ..ImportOptions::default()
        };
        let report = library.import(std::slice::from_ref(&src), &options, &Quiet);
        (library, report)
    };

    let (library, report) = run(Limits {
        max_files: 3,
        ..Limits::default()
    });
    assert_eq!(library.entries().len(), 3);
    assert_eq!(report.limit.as_deref(), Some("stopped at 3 files"));

    let (library, report) = run(Limits {
        max_file_bytes: 1000,
        ..Limits::default()
    });
    assert_eq!(library.entries().len(), 5, "the big file is left out");
    assert_eq!(
        report.skipped.notices(),
        ["Skipped 1 file: .md (over the size limit)"]
    );

    let (library, report) = run(Limits {
        max_total_bytes: 25,
        max_files: 100,
        ..Limits::default()
    });
    assert!(library.entries().len() < 5);
    assert_eq!(report.limit.as_deref(), Some("stopped at the size limit"));

    let (_, report) = run(Limits {
        max_visited: 2,
        max_files: 101,
        ..Limits::default()
    });
    assert_eq!(report.limit.as_deref(), Some("too many files to look at"));
}

#[test]
fn network_device_and_verbatim_paths_are_refused_without_being_read() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut library = library(dir.path());
    let bad = [
        r"\\host\share\a.png",
        "//host/share/b.png",
        r"\\.\COM1",
        r"\\?\C:\models\c.png",
        "D:nul.png",
    ];
    let sources: Vec<PathBuf> = bad.iter().map(PathBuf::from).collect();
    let report = import(&mut library, &sources);
    assert_eq!(report.refused.len(), bad.len(), "{report:?}");
    assert!(report.added.is_empty() && report.failed.is_empty());
    assert!(!library.root().exists(), "nothing was written");
}

#[test]
fn a_missing_source_is_a_failure_not_a_panic() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut library = library(dir.path());
    let report = import(&mut library, &[dir.path().join("nothing.png")]);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.summary(), "Nothing new was imported (1 failed)");
}

#[test]
fn a_broken_file_is_imported_without_details_and_a_fake_png_is_skipped() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    // A real PNG signature with a cut-off body: kept, no metadata.
    let mut cut = png(8, 8, [1, 2, 3, 255]);
    cut.truncate(30);
    write(&src, "cut.png", &cut);
    write(&src, "fake.png", b"this is not a picture");
    write(&src, "broken.bbmodel", b"{ nope");
    let mut library = library(dir.path());
    let report = import(&mut library, &[src]);
    assert_eq!(report.added.len(), 2, "{report:?}");
    assert_eq!(report.unreadable.len(), 2);
    assert_eq!(
        report.skipped.notices(),
        ["Skipped 1 file: .png (not a PNG file)"]
    );
    assert!(library.entries().iter().all(|e| e.meta.is_none()));
}

/// Cancels after a number of progress reports and records the fractions.
struct CancelAfter {
    reports: Cell<usize>,
    last: Cell<f32>,
    stop_at: usize,
}

impl Control for CancelAfter {
    fn progress(&self, fraction: f32) {
        assert!(fraction >= self.last.get(), "progress never goes back");
        self.last.set(fraction);
        self.reports.set(self.reports.get() + 1);
    }

    fn cancelled(&self) -> bool {
        self.reports.get() >= self.stop_at
    }
}

#[test]
fn cancel_keeps_what_was_imported_and_saves_the_index() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    for i in 0..10 {
        write(
            &src,
            &format!("n{i:02}.md"),
            format!("# note {i}\n").as_bytes(),
        );
    }
    let mut library = library(dir.path());
    let control = CancelAfter {
        reports: Cell::new(0),
        last: Cell::new(0.0),
        stop_at: 4,
    };
    let report = library.import(&[src], &ImportOptions::default(), &control);
    assert!(report.cancelled);
    assert_eq!(report.added.len(), 4, "the files before the stop are in");
    assert!(report.summary().contains("cancelled"));
    let (again, _) = Library::open(library.root(), Scope::Global).expect("opens");
    assert_eq!(again.entries().len(), 4);
}

#[test]
fn a_link_out_of_the_folder_is_not_followed_but_one_inside_is() {
    let dir = tempfile::tempdir().expect("temp dir");
    let outside = dir.path().join("outside");
    write(&outside, "secret.md", b"# secret\n");
    let src = dir.path().join("src");
    write(&src, "real/note.md", b"# note\n");
    let link = |target: &Path, name: &str| -> bool {
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(target, src.join(name)).is_ok()
            // A junction needs no privilege and is a link to the file system walk as well.
            || std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(src.join(name))
                .arg(target)
                .output()
                .is_ok_and(|out| out.status.success());
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, src.join(name)).is_ok();
        made
    };
    // Creating links needs a privilege on Windows; without it there is nothing to check.
    if !link(&outside, "out") {
        eprintln!("no permission to create links: nothing to check");
        return;
    }
    assert!(link(&src.join("real"), "inside"));
    // A link back to the folder itself must not loop.
    assert!(link(&src, "loop"));
    let mut library = library(dir.path());
    let report = import(&mut library, &[src]);
    assert!(
        library.entries().iter().all(|e| e.name != "secret.md"),
        "the link out of the folder was followed"
    );
    assert!(
        report
            .refused
            .iter()
            .any(|(path, why)| path.ends_with("out") && why.contains("leaves"))
    );
    let notes = library
        .entries()
        .iter()
        .filter(|e| e.name == "note.md")
        .count();
    assert_eq!(notes, 1, "same content once, however many paths lead to it");
}

#[test]
fn the_view_counts_and_suggests() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    write(&src, "gun.json", MODEL);
    write(&src, "story.md", b"## Weapons\n- Gun\n- Roller\n");
    write(&src, "stone.png", &png(16, 16, [10, 20, 30, 255]));
    let mut library = library(dir.path());
    import(&mut library, &[src]);
    let view = library.view(10);
    assert_eq!(view.total, 3);
    assert_eq!(view.count(Kind::Model), 1);
    assert_eq!(view.recent.len(), 3);
    assert_eq!(view.brief.textures, 1);
    assert_eq!(view.brief.palette, [[10, 20, 30]]);
    let titles: Vec<&str> = view.suggestions.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, ["Roller has no model"]);
    assert_eq!(view.suggestions[0].kind, SuggestionKind::LoreWithoutModel);
    // Search by text and kind through the same entries.
    let query = Query {
        text: "gun".to_owned(),
        kind: Some(Kind::Model),
        ..Query::default()
    };
    assert_eq!(library.search(&query, 10).len(), 1);
}

#[test]
fn removing_an_entry_frees_its_content_for_a_later_import() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = write(dir.path(), "src/gun.json", MODEL);
    let mut library = library(dir.path());
    import(&mut library, std::slice::from_ref(&file));
    let id = library.entries()[0].id.clone();
    assert!(library.remove(&id).is_some());
    let report = import(&mut library, &[file]);
    assert_eq!((report.added.len(), report.duplicates), (1, 0));
}
