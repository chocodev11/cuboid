//! `cargo xtask brand [--check]`: the PNG and ICO files of the logo and their manifest, rendered from
//! the SVG masters in `crates/cuboid-assets/brand/` (docs/design/logo.md 3).
//!
//! Without a flag the files are rendered again and written where they changed. `--check` renders
//! nothing: it compares the SHA-256 of every master and every output on disk with `manifest.txt`,
//! so it gives the same answer on every platform, whatever a renderer does with the last bit of a
//! pixel. `cargo xtask package` calls [`write`] too, so there is one renderer and one size table.
//!
//! CI runs two steps. `brand --check` runs on every platform and catches an edited master, a
//! hand-edited, missing or unlisted output and a stale manifest line; it cannot see a change of
//! the render itself (`band`, `ICO_SIZES`, `ICO_PNG_FROM`, or a new resvg, tiny-skia, png or ico in
//! `Cargo.lock`). So on Windows, the target the committed files were rendered on, CI also runs
//! `brand` and fails on `git diff --exit-code -- crates/cuboid-assets/brand`: the committed bytes
//! must be what the renderer writes. The other two systems stay with the check, because a renderer
//! may differ there in the last bit of a pixel.

use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;
use sha2::{Digest, Sha256};

/// The masters, one per size band: a small size has its own drawing (logo.md 2).
const MASTERS: [&str; 3] = ["logo-16.svg", "logo-32.svg", "logo.svg"];

/// Sizes of the files in `png/`.
const PNG_SIZES: [u32; 10] = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256];

/// Sizes of the images in `cuboid.ico`.
const ICO_SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

/// From this size on an `.ico` image is a PNG; the smaller ones are BMP frames, which every
/// Windows version loads.
const ICO_PNG_FROM: u32 = 64;

const ICO: &str = "cuboid.ico";
const MANIFEST: &str = "manifest.txt";

/// Files as `(path in the folder, bytes)`.
type Files = Vec<(String, Vec<u8>)>;

/// The folder of the masters and the rendered files, from the workspace root.
pub fn folder(root: &Path) -> PathBuf {
    root.join("crates").join("cuboid-assets").join("brand")
}

pub fn run(args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    if let Some(other) = args.iter().find(|arg| arg.as_str() != "--check") {
        return Err(
            format!("unknown argument `{other}`; usage: cargo xtask brand [--check]").into(),
        );
    }
    let check = !args.is_empty();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no parent folder")?;
    let dir = folder(root);
    if !check {
        let written = write(&dir)?;
        println!("brand: wrote {written} file(s) in {}", dir.display());
        return Ok(ExitCode::SUCCESS);
    }
    let problems = check_files(&dir)?;
    if problems.is_empty() {
        println!("brand: ok");
        return Ok(ExitCode::SUCCESS);
    }
    for problem in &problems {
        eprintln!("error: {problem}");
    }
    eprintln!(
        "brand: {} error(s); run `cargo xtask brand`",
        problems.len()
    );
    Ok(ExitCode::FAILURE)
}

/// Renders the outputs and the manifest into `dir`, writing only the files whose bytes changed.
/// Returns how many it wrote.
pub fn write(dir: &Path) -> Result<usize, Box<dyn Error>> {
    let masters = read_masters(dir)?;
    let mut files = render(&masters)?;
    let manifest = manifest(&masters, &files);
    files.push((MANIFEST.to_owned(), manifest.into_bytes()));
    let mut written = 0;
    for (name, bytes) in files {
        let path = dir.join(&name);
        if fs::read(&path).is_ok_and(|old| old == bytes) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
        written += 1;
    }
    Ok(written)
}

/// The masters as `(name, bytes)`, in [`MASTERS`] order.
fn read_masters(dir: &Path) -> Result<Files, Box<dyn Error>> {
    MASTERS
        .iter()
        .map(|name| {
            let path = dir.join(name);
            let bytes = fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))?;
            Ok(((*name).to_owned(), bytes))
        })
        .collect()
}

/// The index in [`MASTERS`] of the master drawn for `size` pixels (logo.md 2).
fn band(size: u32) -> usize {
    match size {
        ..=20 => 0,
        21..=40 => 1,
        _ => 2,
    }
}

/// The master drawn at `size` pixels. The pixmap is premultiplied; the rounded corners of the
/// tile are partly transparent.
fn draw(svg: &[u8], size: u32) -> Result<Pixmap, Box<dyn Error>> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default())?;
    let mut pixmap = Pixmap::new(size, size).ok_or("cannot allocate the logo bitmap")?;
    let scale = size as f32 / tree.size().width();
    resvg::render(
        &tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

/// Every output as `(path in the folder, bytes)`: the PNG files, then the icon.
fn render(masters: &[(String, Vec<u8>)]) -> Result<Files, Box<dyn Error>> {
    let mut files = Vec::new();
    for size in PNG_SIZES {
        let pixmap = draw(&masters[band(size)].1, size)?;
        files.push((format!("png/logo-{size}.png"), pixmap.encode_png()?));
    }
    let mut dir = IconDir::new(ResourceType::Icon);
    for size in ICO_SIZES {
        let pixmap = draw(&masters[band(size)].1, size)?;
        let rgba = pixmap
            .pixels()
            .iter()
            .flat_map(|pixel| {
                let color = pixel.demultiply();
                [color.red(), color.green(), color.blue(), color.alpha()]
            })
            .collect();
        let image = IconImage::from_rgba_data(size, size, rgba);
        dir.add_entry(if size < ICO_PNG_FROM {
            IconDirEntry::encode_as_bmp(&image)?
        } else {
            IconDirEntry::encode_as_png(&image)?
        });
    }
    let mut bytes = Vec::new();
    dir.write(&mut bytes)?;
    files.push((ICO.to_owned(), bytes));
    Ok(files)
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// One line per master and per output, `sha256sum` style: the hash, two spaces, the path in the
/// folder. The manifest does not list itself.
fn manifest(masters: &[(String, Vec<u8>)], outputs: &[(String, Vec<u8>)]) -> String {
    let mut text = String::new();
    for (name, bytes) in masters.iter().chain(outputs) {
        let _ = writeln!(text, "{}  {name}", sha256(bytes));
    }
    text
}

/// The names `manifest.txt` has to list: the masters, the PNG files and the icon.
fn expected_names() -> Vec<String> {
    let masters = MASTERS.iter().map(|name| (*name).to_owned());
    let pngs = PNG_SIZES.iter().map(|size| format!("png/logo-{size}.png"));
    masters.chain(pngs).chain([ICO.to_owned()]).collect()
}

/// The problems of the files in `dir` against their manifest: a missing manifest, file or line, a
/// file that no longer has its recorded hash, or a line nobody expects. Empty when all is well.
fn check_files(dir: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let path = dir.join(MANIFEST);
    let text = fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(verify(&text, |name| fs::read(dir.join(name)).ok()))
}

/// [`check_files`] over `read`, which gives the bytes of a file of the folder.
fn verify(manifest: &str, read: impl Fn(&str) -> Option<Vec<u8>>) -> Vec<String> {
    let listed: Vec<(&str, &str)> = manifest
        .lines()
        .filter_map(|line| line.split_once("  "))
        .collect();
    let mut problems = Vec::new();
    for name in expected_names() {
        let Some((hash, _)) = listed.iter().find(|(_, listed)| *listed == name) else {
            problems.push(format!("{name}: not in {MANIFEST}"));
            continue;
        };
        match read(&name) {
            None => problems.push(format!("{name}: missing")),
            Some(bytes) if sha256(&bytes) != *hash => {
                problems.push(format!("{name}: changed without a new render"));
            }
            Some(_) => {}
        }
    }
    let expected = expected_names();
    for (_, name) in &listed {
        if !expected.iter().any(|known| known == name) {
            problems.push(format!("{name}: listed in {MANIFEST} but not a brand file"));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn each_size_is_drawn_from_the_master_of_its_band() {
        // logo.md 2: 16 and 20 from the 16 grid, 24 to 40 from the 32 grid, 48 and up from the large one.
        let masters = |sizes: &[u32]| {
            sizes
                .iter()
                .map(|&size| MASTERS[band(size)])
                .collect::<Vec<_>>()
        };
        assert_eq!(masters(&[16, 20]), ["logo-16.svg"; 2]);
        assert_eq!(masters(&[24, 32, 40]), ["logo-32.svg"; 3]);
        assert_eq!(masters(&[48, 64, 96, 128, 256]), ["logo.svg"; 5]);
    }

    #[test]
    fn the_icon_sizes_are_png_sizes_too() {
        for size in ICO_SIZES {
            assert!(PNG_SIZES.contains(&size), "{size}");
        }
    }

    /// A folder as the check reads it: every expected file, with its hash in the manifest.
    fn folder_with_manifest() -> (String, HashMap<String, Vec<u8>>) {
        let files: HashMap<String, Vec<u8>> = expected_names()
            .into_iter()
            .map(|name| {
                let bytes = name.as_bytes().to_vec();
                (name, bytes)
            })
            .collect();
        let manifest = expected_names()
            .iter()
            .map(|name| format!("{}  {name}\n", sha256(&files[name])))
            .collect();
        (manifest, files)
    }

    #[test]
    fn a_folder_that_matches_its_manifest_has_no_problem() {
        let (manifest, files) = folder_with_manifest();
        assert_eq!(
            verify(&manifest, |name| files.get(name).cloned()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_changed_missing_or_unlisted_file_is_reported() {
        let (manifest, mut files) = folder_with_manifest();
        files.insert("logo.svg".to_owned(), b"edited".to_vec());
        files.remove("png/logo-16.png");
        let extra = format!("{manifest}{}  png/logo-7.png\n", sha256(b"x"));
        let without_ico = extra.replace(&format!("{}  {ICO}\n", sha256(&files[ICO])), "");
        let problems = verify(&without_ico, |name| files.get(name).cloned());
        assert_eq!(
            problems,
            [
                "logo.svg: changed without a new render".to_owned(),
                "png/logo-16.png: missing".to_owned(),
                format!("{ICO}: not in {MANIFEST}"),
                format!("png/logo-7.png: listed in {MANIFEST} but not a brand file"),
            ]
        );
    }

    #[test]
    fn the_manifest_lists_the_masters_before_the_outputs() {
        let masters = vec![("logo.svg".to_owned(), b"m".to_vec())];
        let outputs = vec![("png/logo-16.png".to_owned(), b"o".to_vec())];
        let text = manifest(&masters, &outputs);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("  logo.svg") && lines[1].ends_with("  png/logo-16.png"));
        assert_eq!(lines[0].split("  ").next().map(str::len), Some(64));
    }
}
