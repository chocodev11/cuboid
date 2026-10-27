//! Import end to end on small generated folders: classification, metadata, dedupe, limits,
//! refusals, links, cancel and the summary view.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use cuboid_image::{Image, RgbaBuf, encode_png};
use cuboid_library::{
    Control, ImportOptions, Kind, Library, LibraryError, Limits, Meta, PROJECT_DIR, Query, Quiet,
    Role, Scope, SuggestionKind,
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

/// Makes `at` a link to the folder `target`; false without the permission to.
fn make_link(target: &Path, at: &Path) -> bool {
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, at).is_ok()
        // A junction needs no privilege and is a link to the file system walk as well.
        || std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(at)
            .arg(target)
            .output()
            .is_ok_and(|out| out.status.success());
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, at).is_ok();
    made
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
    let link = |target: &Path, name: &str| make_link(target, &src.join(name));
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

#[test]
fn an_import_with_the_role_style_takes_models_and_textures_only() {
    let dir = tempfile::tempdir().expect("temp dir");
    let src = dir.path().join("src");
    sample(&src);
    let mut library = library(dir.path());
    let options = ImportOptions {
        role: Role::Style,
        ..ImportOptions::default()
    };
    let report = library.import(&[src], &options, &Quiet);
    assert_eq!(report.added.len(), 2, "{report:?}");
    assert!(library.entries().iter().all(|e| e.role == Role::Style));
    assert_eq!(library.counts(), [1, 1, 0, 0, 0]);
    let notices = report.skipped.notices();
    assert!(
        notices.contains(&"Skipped 1 file: .md (not a model or a texture)".to_owned()),
        "{notices:?}"
    );
    // Only the project's own models and textures count as observed.
    let view = library.view(10);
    assert_eq!((view.brief.models, view.brief.textures), (0, 0));
}

/// A model with one element of the given height.
fn lamp(height: u32) -> Vec<u8> {
    format!(r#"{{"elements": [{{"from": [0, 0, 0], "to": [4, {height}, 4]}}]}}"#).into_bytes()
}

/// An empty project library in `dir/project` and the project folder.
fn project(dir: &Path) -> (Library, PathBuf) {
    let project = dir.join("project");
    fs::create_dir_all(&project).expect("dir");
    (
        Library::new(project.join(PROJECT_DIR), Scope::Project),
        project,
    )
}

fn cc0() -> ImportOptions {
    ImportOptions {
        tags: vec!["lamp".to_owned()],
        licence: "CC0".to_owned(),
        ..ImportOptions::default()
    }
}

#[test]
fn a_style_folder_is_imported_remembered_and_read_back() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    let refs = project.join("refs").join("lamps");
    write(&refs, "post.json", &lamp(12));
    write(&refs, "wood.png", &png(16, 16, [120, 80, 40, 255]));
    write(&refs, "notes.md", b"# notes\n");

    let report = library
        .import_style(&refs, &cc0(), &Quiet)
        .expect("imports");
    assert_eq!((report.added.len(), report.dropped), (2, 0));
    assert_eq!(library.counts(), [1, 1, 0, 0, 0], "the note is not taken");
    for entry in library.entries() {
        assert_eq!(entry.role, Role::Style);
        assert_eq!(
            (entry.licence.as_str(), entry.tags.as_slice()),
            ("CC0", ["lamp".to_owned()].as_slice())
        );
        assert!(
            entry.source.starts_with("refs/lamps/"),
            "relative to the project, not {:?}",
            entry.source
        );
    }
    let sources = &library.brief().style_sources;
    assert_eq!(sources.len(), 1);
    let source = &sources[0];
    assert_eq!(
        (source.path.as_str(), source.licence.as_str()),
        ("refs/lamps", "CC0")
    );
    assert_eq!(source.tags, ["lamp"]);
    assert!(source.scanned > 0);

    // The index on disk says the same, as schema 3.
    let (again, _) = Library::open(library.root(), Scope::Project).expect("opens");
    assert_eq!(again.entries(), library.entries());
    assert_eq!(again.brief().style_sources, library.brief().style_sources);
    let text = fs::read_to_string(library.root().join("library.json")).expect("index");
    assert!(text.contains("\"version\": 3") && text.contains("\"role\": \"style\""));
    assert!(
        library
            .style_summary()
            .is_some_and(|s| (s.models, s.textures) == (1, 1))
    );
}

#[test]
fn a_rescan_reads_the_folder_again_and_drops_what_changed_or_went() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    let refs = project.join("refs").join("lamps");
    write(&refs, "post.json", &lamp(12));
    write(&refs, "torch.json", &lamp(8));
    write(&refs, "keep.json", &lamp(6));
    library
        .import_style(&refs, &cc0(), &Quiet)
        .expect("imports");
    let old = library
        .entries()
        .iter()
        .find(|e| e.name == "post.json")
        .expect("post")
        .clone();
    let old_copy = library.file_path(&old).expect("plain");
    assert!(old_copy.is_file());

    write(&refs, "post.json", &lamp(14));
    fs::remove_file(refs.join("torch.json")).expect("remove");
    write(&refs, "lantern.json", &lamp(20));
    let report = library.rescan_style("refs/lamps", &Quiet).expect("rescans");
    assert_eq!(
        (report.added.len(), report.duplicates, report.dropped),
        (2, 1, 2)
    );
    assert!(report.summary().contains("2 no longer in the folder"));

    let mut names: Vec<&str> = library.entries().iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["keep.json", "lantern.json", "post.json"]);
    assert!(library.entry(&old.id).is_none(), "the old content is gone");
    assert!(!old_copy.exists(), "and so is its stored copy");
    for entry in library.entries() {
        assert_eq!((entry.licence.as_str(), entry.role), ("CC0", Role::Style));
        assert_eq!(
            entry.tags,
            ["lamp"],
            "licence and tags come from the source"
        );
    }
    assert_eq!(library.brief().style_sources.len(), 1);
}

#[test]
fn a_project_that_moves_keeps_its_style_references_and_can_rescan() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    let refs = project.join("refs");
    write(&refs, "post.json", &lamp(12));
    library
        .import_style(&refs, &cc0(), &Quiet)
        .expect("imports");
    drop(library);

    let moved = dir.path().join("moved");
    fs::rename(&project, &moved).expect("rename");
    let (mut library, _) = Library::open(&moved.join(PROJECT_DIR), Scope::Project).expect("opens");
    let report = library.rescan_style("refs", &Quiet).expect("rescans");
    assert_eq!(
        (report.added.len(), report.duplicates, report.dropped),
        (0, 1, 0)
    );
    assert_eq!(library.entries().len(), 1);
}

#[test]
fn removing_a_style_source_removes_its_references_and_copies_but_not_the_folder() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    let refs = project.join("refs");
    write(&refs, "post.json", &lamp(12));
    write(&refs, "wood.png", &png(16, 16, [120, 80, 40, 255]));
    let own = write(&project, "own.json", &lamp(30));
    library
        .import_style(&refs, &cc0(), &Quiet)
        .expect("imports");
    let report = library.import(&[own], &ImportOptions::default(), &Quiet);
    assert_eq!(report.added.len(), 1);
    assert_eq!(
        fs::read_dir(library.root().join("files"))
            .expect("files")
            .count(),
        3
    );

    assert_eq!(library.remove_style_source("refs").expect("removes"), 2);
    assert!(library.brief().style_sources.is_empty());
    assert_eq!(library.entries().len(), 1, "the own model stays");
    assert_eq!(library.entries()[0].role, Role::Work);
    assert_eq!(
        fs::read_dir(library.root().join("files"))
            .expect("files")
            .count(),
        1
    );
    assert!(refs.join("post.json").is_file() && refs.join("wood.png").is_file());
    let (again, _) = Library::open(library.root(), Scope::Project).expect("opens");
    assert_eq!(again.entries(), library.entries());
    assert!(again.brief().style_sources.is_empty());
    assert!(matches!(
        library.remove_style_source("refs"),
        Err(LibraryError::Source(_))
    ));
}

#[test]
fn a_style_folder_must_be_inside_the_project_and_outside_the_library() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    write(&project, "refs/lamps/post.json", &lamp(12));
    write(dir.path(), "elsewhere/post.json", &lamp(13));
    library
        .import_style(&project.join("refs").join("lamps"), &cc0(), &Quiet)
        .expect("imports");
    let refused = |library: &mut Library, folder: &Path| {
        matches!(
            library.import_style(folder, &cc0(), &Quiet),
            Err(LibraryError::Source(_))
        )
    };
    assert!(refused(&mut library, &dir.path().join("elsewhere")));
    assert!(refused(&mut library, &project), "the project itself");
    assert!(
        refused(&mut library, &project.join("refs")),
        "it holds a source"
    );
    assert!(
        refused(&mut library, &project.join("refs/lamps/inner")),
        "it is no folder"
    );
    let files = library.root().join("files");
    assert!(refused(&mut library, &files), "the library");
    assert!(refused(
        &mut library,
        &project.join("refs").join("..").join("..")
    ));
    // A global library has no project.
    let mut global = Library::new(dir.path().join("global"), Scope::Global);
    assert!(refused(&mut global, &project.join("refs")));
    assert_eq!(library.brief().style_sources.len(), 1);
}

#[test]
fn a_project_keeps_at_most_16_style_folders() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    for n in 0..17 {
        write(&project, &format!("refs{n}/post.json"), &lamp(10 + n));
    }
    for n in 0..16 {
        let folder = project.join(format!("refs{n}"));
        library
            .import_style(&folder, &cc0(), &Quiet)
            .expect("imports");
    }
    let folder = project.join("refs16");
    assert!(matches!(
        library.import_style(&folder, &cc0(), &Quiet),
        Err(LibraryError::Source(_))
    ));
    assert_eq!(library.brief().style_sources.len(), 16);
    assert_eq!(library.entries().len(), 16);
    // A known folder can still be read again.
    assert!(library.rescan_style("refs3", &Quiet).is_ok());
}

#[test]
fn content_the_project_already_has_as_its_own_work_stays_work() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    let own = write(&project, "own/post.json", &lamp(12));
    library.import(&[own], &ImportOptions::default(), &Quiet);
    write(&project, "refs/same.json", &lamp(12));
    let report = library
        .import_style(&project.join("refs"), &cc0(), &Quiet)
        .expect("imports");
    assert_eq!((report.added.len(), report.duplicates), (0, 1));
    assert_eq!(library.entries()[0].role, Role::Work);
}

#[test]
fn a_file_renamed_or_swapped_inside_a_style_folder_survives_a_rescan() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    let refs = project.join("refs");
    write(&refs, "a.json", &lamp(12));
    write(&refs, "b.json", &lamp(8));
    write(&refs, "c.json", &lamp(6));
    library
        .import_style(&refs, &cc0(), &Quiet)
        .expect("imports");

    fs::rename(refs.join("c.json"), refs.join("moved.json")).expect("rename");
    fs::rename(refs.join("a.json"), refs.join("tmp.json")).expect("rename");
    fs::rename(refs.join("b.json"), refs.join("a.json")).expect("rename");
    fs::rename(refs.join("tmp.json"), refs.join("b.json")).expect("rename");
    let report = library.rescan_style("refs", &Quiet).expect("rescans");
    assert_eq!(report.added.len(), 3, "{report:?}");

    assert_eq!(library.entries().len(), 3);
    let held = |source: &str| {
        let entry = library
            .entries()
            .iter()
            .find(|entry| entry.source == source)
            .unwrap_or_else(|| panic!("no entry for {source}"));
        fs::read(library.file_path(entry).expect("plain")).expect("the stored copy")
    };
    assert_eq!(held("refs/a.json"), lamp(8));
    assert_eq!(held("refs/b.json"), lamp(12));
    assert_eq!(held("refs/moved.json"), lamp(6));
}

#[cfg(windows)]
#[test]
fn a_rescan_through_a_path_form_the_import_refuses_deletes_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    write(&project, "refs/post.json", &lamp(12));
    library
        .import_style(&project.join("refs"), &cc0(), &Quiet)
        .expect("imports");
    let before = library.entries().to_vec();
    drop(library);

    // The canonical path of a folder is a verbatim `\\?\` path on Windows.
    let verbatim = fs::canonicalize(&project).expect("canonical");
    let (mut library, _) =
        Library::open(&verbatim.join(PROJECT_DIR), Scope::Project).expect("opens");
    assert!(matches!(
        library.rescan_style("refs", &Quiet),
        Err(LibraryError::Source(_))
    ));
    assert_eq!(library.entries(), before);
    assert!(
        library
            .file_path(&before[0])
            .is_some_and(|copy| copy.is_file())
    );
}

#[cfg(windows)]
#[test]
fn style_folders_are_compared_as_the_system_resolves_them() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    write(&project, "refs/post.json", &lamp(12));
    fs::create_dir_all(library.root().join("thumbs")).expect("dir");
    // The library in another case is still the library.
    assert!(matches!(
        library.import_style(
            &project.join("CUBOID-LIBRARY").join("thumbs"),
            &cc0(),
            &Quiet
        ),
        Err(LibraryError::Source(_))
    ));
    // The project in another case is still the project.
    let shouty = dir.path().join("PROJECT").join("refs");
    let report = library
        .import_style(&shouty, &cc0(), &Quiet)
        .expect("imports");
    assert_eq!(report.added.len(), 1);
    assert_eq!(library.brief().style_sources[0].path, "refs");
}

#[test]
fn a_style_folder_that_links_out_of_the_project_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    write(dir.path(), "outside/post.json", &lamp(12));
    fs::create_dir_all(&project).expect("dir");
    // Creating links needs a privilege on Windows; without it there is nothing to check.
    if !make_link(&dir.path().join("outside"), &project.join("refs")) {
        eprintln!("no permission to create links: nothing to check");
        return;
    }
    assert!(matches!(
        library.import_style(&project.join("refs"), &cc0(), &Quiet),
        Err(LibraryError::Source(_))
    ));
    assert!(library.entries().is_empty() && library.brief().style_sources.is_empty());
}

#[test]
fn a_reference_that_the_project_imports_as_its_own_work_becomes_work() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    write(&project, "refs/post.json", &lamp(12));
    library
        .import_style(&project.join("refs"), &cc0(), &Quiet)
        .expect("imports");
    assert_eq!(library.entries()[0].role, Role::Style);

    let own = write(&project, "own/post_copy.json", &lamp(12));
    let report = library.import(&[own], &ImportOptions::default(), &Quiet);
    assert_eq!((report.added.len(), report.duplicates), (0, 1));
    assert_eq!(library.entries()[0].role, Role::Work);
    assert!(library.style_summary().is_none());
    assert_eq!(library.remove_style_source("refs").expect("removes"), 0);
    assert_eq!(
        library.entries().len(),
        1,
        "the own work outlives its source"
    );
}

#[test]
fn a_file_that_two_style_folders_hold_is_named_in_the_report() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    write(&project, "one/post.json", &lamp(12));
    write(&project, "two/post.json", &lamp(12));
    let first = library
        .import_style(&project.join("one"), &cc0(), &Quiet)
        .expect("imports");
    assert!(first.shared.is_empty());
    let second = library
        .import_style(&project.join("two"), &cc0(), &Quiet)
        .expect("imports");
    assert_eq!((second.added.len(), second.duplicates), (0, 1));
    assert_eq!(
        second.shared,
        [("two/post.json".to_owned(), "one/post.json".to_owned())]
    );
    assert!(
        second
            .notices()
            .contains(&"two/post.json has the content of the reference one/post.json".to_owned())
    );
}

#[test]
fn a_model_becomes_a_reference_and_back() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    let own = write(&project, "own/post.json", &lamp(12));
    let notes = write(&project, "own/notes.md", b"# notes\n");
    library.import(&[own, notes], &ImportOptions::default(), &Quiet);
    let id_of = |library: &Library, kind| {
        library
            .entries()
            .iter()
            .find(|entry| entry.kind == kind)
            .map(|entry| entry.id.clone())
            .expect("an entry")
    };
    let (model, note) = (id_of(&library, Kind::Model), id_of(&library, Kind::Lore));
    assert!(library.style_summary().is_none());

    assert!(library.set_role(&model, Role::Style));
    assert_eq!(library.style_summary().map(|s| s.models), Some(1));
    assert!(library.set_role(&model, Role::Work));
    assert!(library.style_summary().is_none());
    assert!(!library.set_role("nothing", Role::Style));
    assert!(
        !library.set_role(&note, Role::Style),
        "a note is no reference"
    );
    assert_eq!(library.entry(&note).map(|e| e.role), Some(Role::Work));
}

#[test]
fn a_model_of_an_older_index_gets_its_style_from_its_copy_when_it_becomes_a_reference() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().join("lib");
    write(&root, "files/a1-post.json", &lamp(12));
    let index = r#"{"version": 2, "entries": [{"id": "a1", "kind": "model", "name": "post.json",
        "file": "a1-post.json", "source": "C:/src/post.json", "sha256": "00", "size": 10,
        "added": 100, "meta": {"type": "model", "format": "json", "elements": 1, "meshes": 0,
        "textures": 0, "voxels": false, "bounds": null, "density": null,
        "texture_size": null}}]}"#;
    write(&root, "library.json", index.as_bytes());
    let (mut library, _) = Library::open(&root, Scope::Project).expect("opens");
    let style_of = |library: &Library| match &library.entries()[0].meta {
        Some(Meta::Model(meta)) => meta.style.clone(),
        other => panic!("{other:?}"),
    };
    assert!(style_of(&library).is_none());

    assert!(library.set_role("a1", Role::Style));
    let style = style_of(&library).expect("read from the stored copy");
    assert_eq!(style.sides, Some([12.0; 5]));
    let summary = library.style_summary().expect("a summary");
    assert_eq!(summary.element_size.map(|size| size.median), Some(12.0));
    // Its source is not inside a style folder, so none of them removes it.
    assert_eq!(library.entries()[0].source, "C:/src/post.json");
}

#[test]
fn a_known_folder_added_again_with_another_licence_changes_its_references() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    let refs = project.join("refs");
    write(&refs, "post.json", &lamp(12));
    write(&refs, "torch.json", &lamp(8));
    library
        .import_style(&refs, &cc0(), &Quiet)
        .expect("imports");
    let again = ImportOptions {
        tags: vec!["torch".to_owned()],
        licence: "MIT".to_owned(),
        ..ImportOptions::default()
    };
    library
        .import_style(&refs, &again, &Quiet)
        .expect("imports");
    for entry in library.entries() {
        assert_eq!(
            (entry.licence.as_str(), entry.tags.as_slice()),
            ("MIT", ["torch".to_owned()].as_slice())
        );
    }
    let source = &library.brief().style_sources[0];
    assert_eq!(source.licence, "MIT");

    // A rescan with the licence of the source leaves what was edited by hand.
    let id = library.entries()[0].id.clone();
    assert!(library.set_licence(&id, "CC-BY"));
    library.rescan_style("refs", &Quiet).expect("rescans");
    assert_eq!(
        library.entry(&id).map(|e| e.licence.as_str()),
        Some("CC-BY")
    );
}

#[test]
fn a_cancelled_import_does_not_move_the_time_of_the_scan() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    let refs = project.join("refs");
    for n in 0..6 {
        write(&refs, &format!("m{n}.json"), &lamp(10 + n));
    }
    library
        .import_style(&refs, &cc0(), &Quiet)
        .expect("imports");
    let mut brief = library.brief().clone();
    brief.style_sources[0].scanned = 5;
    library.set_brief(brief);

    let control = CancelAfter {
        reports: Cell::new(0),
        last: Cell::new(0.0),
        stop_at: 2,
    };
    let report = library.rescan_style("refs", &control).expect("rescans");
    assert!(report.cancelled);
    assert_eq!(library.brief().style_sources[0].scanned, 5);
    library.rescan_style("refs", &Quiet).expect("rescans");
    assert!(library.brief().style_sources[0].scanned > 5);
}

#[test]
fn a_style_folder_is_found_by_any_form_of_its_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (mut library, project) = project(dir.path());
    write(&project, "refs/lamps/post.json", &lamp(12));
    library
        .import_style(&project.join("refs").join("lamps"), &cc0(), &Quiet)
        .expect("imports");
    for form in ["refs\\lamps", "./refs//lamps/", "refs/lamps"] {
        let report = library.rescan_style(form, &Quiet).expect(form);
        assert_eq!((report.added.len(), report.duplicates), (0, 1), "{form}");
    }
    assert!(library.rescan_style("refs", &Quiet).is_err());
    assert_eq!(
        library
            .remove_style_source("refs\\lamps/")
            .expect("removes"),
        1
    );
}
