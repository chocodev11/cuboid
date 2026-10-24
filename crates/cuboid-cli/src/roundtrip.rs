//! `roundtrip`: load a `.cubik` file, open and save it in memory, compare the payloads.

use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;

use cuboid_bson::{Limits as BsonLimits, RawDoc};
use cuboid_fmt_cubik::{Limits, SaveOptions, container};

use crate::{Fail, doc};

/// The top-level keys of the payload (native 4.1) and what they hold.
const SECTIONS: &[(&[u8], &str)] = &[
    (b"v", "version"),
    (b"mp", "model properties"),
    (b"vp", "view"),
    (b"bp", "blueprint"),
    (b"el", "elements"),
    (b"tx", "textures"),
    (b"vx", "voxels"),
    (b"me", "meshes"),
    (b"md", "display slots"),
];

/// Reports on stdout whether the saved payload equals the input payload; true when it does.
/// `out` receives the saved file, written atomically.
pub fn run(path: &Path, out: Option<&Path>) -> Result<bool, Fail> {
    let failed = |err: &dyn std::fmt::Display| Fail::Error(format!("{}: {err}", path.display()));
    if let Some(out) = out
        && same_file(path, out)
    {
        return Err(Fail::Usage("--out must not be the input file".to_owned()));
    }
    let limits = Limits::default();
    let input = container::read(
        BufReader::new(File::open(path).map_err(|err| failed(&err))?),
        &limits,
    )
    .map_err(|err| failed(&err))?
    .payload;
    let loaded = cuboid_fmt_cubik::load_path(path, &limits).map_err(|err| failed(&err))?;
    for warning in &loaded.warnings {
        eprintln!("cuboid-cli: warning: {warning}");
    }
    let (document, selection) = doc::open(loaded.doc)?;
    let file = cuboid_fmt_cubik::save_bytes(
        &doc::snapshot(&document, &selection),
        document.retained.as_deref(),
        &document.overrides,
        &SaveOptions::default(),
    )
    .map_err(|err| failed(&err))?;
    let saved = container::read(std::io::Cursor::new(&file), &limits)
        .map_err(|err| failed(&err))?
        .payload;
    if let Some(out) = out {
        cuboid_io::write_atomic(out, &file)
            .map_err(|err| Fail::Error(format!("{}: {err}", out.display())))?;
    }

    let Some(offset) = first_difference(&input, &saved) else {
        println!("identical: payload of {} bytes", input.len());
        return Ok(true);
    };
    let mut place = section_at(&input, offset);
    // A changed size shows at offset 0 and says nothing about where the content differs.
    if offset < 4
        && let Some((at, section)) = first_section_difference(&input, &saved)
    {
        place = format!("{place}; the first differing section starts at offset {at}: {section}");
    }
    println!(
        "different: first differing offset {offset} in {place}; input payload {} bytes, saved payload {} bytes",
        input.len(),
        saved.len()
    );
    Ok(false)
}

/// The offset of the first byte that differs; the shorter length when one is a prefix of the
/// other.
fn first_difference(a: &[u8], b: &[u8]) -> Option<usize> {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .or((a.len() != b.len()).then_some(a.len().min(b.len())))
}

fn parse_limits(payload: &[u8]) -> BsonLimits {
    BsonLimits {
        max_doc: u32::try_from(payload.len()).unwrap_or(u32::MAX),
        ..BsonLimits::DEFAULT
    }
}

fn section_name(key: &[u8]) -> String {
    match SECTIONS.iter().find(|(known, _)| *known == key) {
        Some((known, name)) => format!("section `{}` ({name})", String::from_utf8_lossy(known)),
        None => format!("section `{}`", String::from_utf8_lossy(key)),
    }
}

/// Names the top-level section of `payload` that holds `offset`.
fn section_at(payload: &[u8], offset: usize) -> String {
    let Ok(root) = RawDoc::parse(payload, &parse_limits(payload)) else {
        return "an unreadable payload".to_owned();
    };
    if offset >= root.as_bytes().len() {
        return "the end of the input payload".to_owned();
    }
    for elem in root.iter() {
        let Ok(elem) = elem else {
            break;
        };
        if elem.whole.range().contains(&offset) {
            return section_name(elem.key);
        }
    }
    "the document header or its end marker".to_owned()
}

/// The offset in `a` and the name of the first top-level section that differs in key or bytes.
/// Sections are compared in file order, so a missing one shifts the rest.
fn first_section_difference(a: &[u8], b: &[u8]) -> Option<(usize, String)> {
    let a_doc = RawDoc::parse(a, &parse_limits(a)).ok()?;
    let b_doc = RawDoc::parse(b, &parse_limits(b)).ok()?;
    let (mut a_iter, mut b_iter) = (a_doc.iter(), b_doc.iter());
    loop {
        let (a_elem, b_elem) = match (a_iter.next(), b_iter.next()) {
            (Some(Ok(x)), Some(Ok(y))) => (x, y),
            (Some(Ok(x)), _) => return Some((x.whole.range().start, section_name(x.key))),
            _ => return None,
        };
        if a_elem.key != b_elem.key {
            let (at, name) = (a_elem.whole.range().start, section_name(a_elem.key));
            let other = section_name(b_elem.key);
            return Some((
                at,
                format!("{name} in the input, {other} in the saved payload"),
            ));
        }
        let (x, y) = (a.get(a_elem.whole.range())?, b.get(b_elem.whole.range())?);
        if let Some(inner) = first_difference(x, y) {
            return Some((a_elem.whole.range().start + inner, section_name(a_elem.key)));
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
