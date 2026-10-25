//! `dump`: the canonical text dump of a `.cubik` payload or a `.cub` file (native 10.2).

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::Path;

use cuboid_fmt_cubik::cub::{self, DiskFiles};
use cuboid_fmt_cubik::{Limits, SaveOptions, container, dump, save};

use crate::{Fail, doc};

/// The dump repeats full paths, so a hostile file can make it huge.
pub const DEFAULT_MAX_LINES: usize = 10_000;

/// Prints the dump of the file; `max_lines` of 0 prints all of it.
pub fn run(path: &Path, max_lines: usize) -> Result<(), Fail> {
    let failed = |err: &dyn std::fmt::Display| Fail::Error(format!("{}: {err}", path.display()));
    let text = if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cub"))
    {
        cub_dump(path).map_err(|err| failed(&err))?
    } else {
        let file = File::open(path).map_err(|err| failed(&err))?;
        let container = container::read(BufReader::new(file), &Limits::default())
            .map_err(|err| failed(&err))?;
        dump::payload(&container.payload).map_err(|err| failed(&err))?
    };
    print(&text, max_lines).map_err(|err| Fail::Error(format!("cannot write the output: {err}")))
}

/// A `.cub` file has no payload of its own: it is imported, then dumped as the payload that a
/// save would write.
fn cub_dump(path: &Path) -> Result<String, Fail> {
    let bytes = fs::read(path).map_err(|err| Fail::Error(err.to_string()))?;
    let imported =
        cub::import(&bytes, path, &DiskFiles).map_err(|err| Fail::Error(err.to_string()))?;
    for notice in &imported.notices {
        eprintln!("cuboid-cli: note: {notice:?}");
    }
    let (document, selection) = doc::open(imported.doc)?;
    let options = SaveOptions {
        use_retention: false,
        ..SaveOptions::default()
    };
    let payload = save::save_payload(&doc::snapshot(&document, &selection), None, &options)
        .map_err(|err| Fail::Error(err.to_string()))?;
    dump::payload(&payload).map_err(|err| Fail::Error(err.to_string()))
}

/// Prints the text line by line; a closed pipe is not an error. `max_lines` of 0 prints all.
pub fn print(text: &str, max_lines: usize) -> io::Result<()> {
    let mut out = BufWriter::new(io::stdout().lock());
    let mut truncated = false;
    for (n, line) in text.lines().enumerate() {
        if max_lines != 0 && n >= max_lines {
            truncated = true;
            break;
        }
        if let Err(err) = writeln!(out, "{line}") {
            // A closed pipe (`| head`) is not an error.
            return if err.kind() == io::ErrorKind::BrokenPipe {
                Ok(())
            } else {
                Err(err)
            };
        }
    }
    match out.flush() {
        Err(err) if err.kind() != io::ErrorKind::BrokenPipe => return Err(err),
        _ => {}
    }
    if truncated {
        eprintln!("cuboid-cli: output cut at {max_lines} lines (--max-lines 0 prints all)");
    }
    Ok(())
}
