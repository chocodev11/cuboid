//! `brief`: what a project wants, for an agent: the JSON view, the Markdown view, or `brief.md`
//! written next to the index. Reads the library without changing it; `--write` is the only
//! thing this command writes.

use std::path::Path;

use cuboid_library::{BRIEF_FILE, Library};

use crate::args::Args;
use crate::{Fail, dump};

/// Prints the brief of the folder in the format asked for, or writes `brief.md` and prints its
/// path. A folder without an index is an error.
pub fn run(args: &[String]) -> Result<(), Fail> {
    let args = Args::split(args, &["--json", "--md", "--write"], &[])?;
    let (json, md, write) = (args.flag("--json"), args.flag("--md"), args.flag("--write"));
    if json && md {
        return Err(Fail::Usage("--json and --md exclude each other".to_owned()));
    }
    if write && (json || md) {
        return Err(Fail::Usage(
            "--write prints the path of brief.md and takes neither --json nor --md".to_owned(),
        ));
    }
    let folder = args.word("folder")?.unwrap_or(".");
    let (root, scope) = Library::locate(Path::new(folder))
        .map_err(|err| Fail::Error(format!("{folder}: {err}")))?;
    let library = Library::read(&root, scope)
        .map_err(|err| Fail::Error(format!("{}: {err}", root.display())))?;
    let text = if write {
        library
            .write_brief_md()
            .map_err(|err| Fail::Error(format!("cannot write brief.md: {err}")))?;
        library.root().join(BRIEF_FILE).display().to_string()
    } else if md {
        library.brief_markdown()
    } else {
        library
            .brief_json()
            .map_err(|err| Fail::Error(err.to_string()))?
    };
    dump::print(&text, 0).map_err(|err| Fail::Error(format!("cannot write the output: {err}")))
}
