//! `brief`: what a project wants, for an agent: the JSON view, the Markdown view, or `brief.md`
//! written next to the index. Reads the library without changing it; `--write` is the only
//! thing this command writes.

use std::path::{Path, PathBuf};

use cuboid_library::{BRIEF_FILE, INDEX_FILE, Library, PROJECT_DIR, Scope};

use crate::args::Args;
use crate::{Fail, dump};

/// The folder of the library that `folder` names and the scope it is in: the folder itself when it
/// holds an index, else its project library `cuboid-library`.
pub fn locate(folder: &Path) -> Option<(PathBuf, Scope)> {
    let project = folder.join(PROJECT_DIR);
    if folder.join(INDEX_FILE).is_file() {
        let is_project = folder.file_name().is_some_and(|name| name == PROJECT_DIR);
        Some((
            folder.to_owned(),
            if is_project {
                Scope::Project
            } else {
                Scope::Global
            },
        ))
    } else if project.join(INDEX_FILE).is_file() {
        Some((project, Scope::Project))
    } else {
        None
    }
}

/// Reads the library of `folder`; a folder without an index is an error.
pub fn open(folder: &Path) -> Result<Library, Fail> {
    let (root, scope) = locate(folder).ok_or_else(|| {
        Fail::Error(format!(
            "{}: no {INDEX_FILE} here or in {PROJECT_DIR}/",
            folder.display()
        ))
    })?;
    Library::read(&root, scope).map_err(|err| Fail::Error(format!("{}: {err}", root.display())))
}

/// Prints the brief of the folder in the format asked for, or writes `brief.md` and prints its
/// path.
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
    let library = open(Path::new(folder))?;
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
