//! Bulk import of files and folders into a library.
//!
//! Rules: the paths the texture-file rules refuse (network, device and verbatim paths) are not
//! read; a symbolic link is followed only when it stays inside the chosen folder; the number of
//! files, their total size and the size of one file are limited; a file is copied into the
//! library once, however often and under whatever name it is offered (SHA-256).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use cuboid_io::texture_files::{DiskFiles, Form, TextureFiles, form};
use sha2::{Digest, Sha256};

use crate::kind::{
    Class, JsonClass, Kind, Role, Skipped, classify, is_candidate, is_companion, skip_label,
};
use crate::meta::{Meta, lore_meta, model_meta, texture_meta};
use crate::store::{
    Entry, Library, UNKNOWN_LICENCE, clean_licence, clean_tags, hex, now_secs, stored_name,
};
use crate::thumb::thumb_png;

/// Entries added between two writes of the index, so a long import survives a crash.
const SAVE_EVERY: usize = 100;

/// What an import may take.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Files imported in one call.
    pub max_files: usize,
    /// Bytes of all files imported in one call.
    pub max_total_bytes: u64,
    /// Bytes of one file.
    pub max_file_bytes: u64,
    /// Directory entries looked at while walking folders.
    pub max_visited: usize,
    /// Folder depth below a chosen folder.
    pub max_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_files: 5_000,
            max_total_bytes: 2 << 30,
            max_file_bytes: cuboid_io::texture_files::MAX_TEXTURE_FILE,
            max_visited: 200_000,
            max_depth: 32,
        }
    }
}

/// What the files get.
#[derive(Clone, Debug)]
pub struct ImportOptions {
    /// The limits.
    pub limits: Limits,
    /// Tags added to every new entry.
    pub tags: Vec<String>,
    /// Licence of every new entry.
    pub licence: String,
    /// The role of every new entry. Style references are models and textures: other files of
    /// the folder are skipped.
    pub role: Role,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            tags: Vec::new(),
            licence: UNKNOWN_LICENCE.to_owned(),
            role: Role::Work,
        }
    }
}

/// Progress and cancellation of the caller.
pub trait Control {
    /// Reports the finished part, 0 to 1.
    fn progress(&self, fraction: f32);
    /// True once the user asked to stop. The import ends after the file it is on.
    fn cancelled(&self) -> bool;
}

/// A [`Control`] that reports nothing and never cancels.
pub struct Quiet;

impl Control for Quiet {
    fn progress(&self, _fraction: f32) {}

    fn cancelled(&self) -> bool {
        false
    }
}

/// What an import did.
#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    /// Ids of the new entries, in order.
    pub added: Vec<String>,
    /// Files whose content the library had already.
    pub duplicates: usize,
    /// Files that are of no kind of the library, by label.
    pub skipped: Skipped,
    /// Style references taken out because their file is gone or holds other content now (a
    /// rescan of a style folder).
    pub dropped: usize,
    /// Style files whose content another reference holds already: the path of the file, as a
    /// reference would have it, and the `source` of that reference. Removing the folder of that
    /// reference leaves the file without one until its own folder is read again.
    pub shared: Vec<(String, String)>,
    /// Paths that were not read, with the reason.
    pub refused: Vec<(PathBuf, String)>,
    /// Files that could not be read or copied, with the reason.
    pub failed: Vec<(String, String)>,
    /// Files imported without metadata because the format crates could not read them.
    pub unreadable: Vec<String>,
    /// Set when a limit stopped the import; says which.
    pub limit: Option<String>,
    /// The user stopped the import.
    pub cancelled: bool,
    /// Bytes copied.
    pub bytes: u64,
}

impl ImportReport {
    /// One line for a toast.
    pub fn summary(&self) -> String {
        let mut text = match self.added.len() {
            0 => "Nothing new was imported".to_owned(),
            1 => "Imported 1 file".to_owned(),
            count => format!("Imported {count} files"),
        };
        let mut notes = Vec::new();
        if self.duplicates > 0 {
            notes.push(format!("{} already in the library", self.duplicates));
        }
        if self.skipped.total() > 0 {
            notes.push(format!("{} skipped", self.skipped.total()));
        }
        if self.dropped > 0 {
            notes.push(format!("{} no longer in the folder", self.dropped));
        }
        if !self.refused.is_empty() {
            notes.push(format!("{} refused", self.refused.len()));
        }
        if !self.failed.is_empty() {
            notes.push(format!("{} failed", self.failed.len()));
        }
        if self.cancelled {
            notes.push("cancelled".to_owned());
        }
        if let Some(limit) = &self.limit {
            notes.push(limit.clone());
        }
        if !notes.is_empty() {
            text.push_str(" (");
            text.push_str(&notes.join(", "));
            text.push(')');
        }
        text
    }

    /// The details: one line per skipped extension, per refusal and per failure.
    pub fn notices(&self) -> Vec<String> {
        let mut lines = self.skipped.notices();
        lines.extend(
            self.shared
                .iter()
                .map(|(path, held)| format!("{path} has the content of the reference {held}")),
        );
        lines.extend(
            self.refused
                .iter()
                .map(|(path, why)| format!("Not read: {} ({why})", path.display())),
        );
        lines.extend(
            self.failed
                .iter()
                .map(|(name, why)| format!("Failed: {name} ({why})")),
        );
        lines.extend(
            self.unreadable
                .iter()
                .map(|name| format!("Imported without details: {name}")),
        );
        lines
    }
}

/// A file picked for import.
struct Found {
    path: PathBuf,
    name: String,
}

impl Library {
    /// Imports files and folders. Folders are walked recursively. Each new file is copied into
    /// `files/`, described and added; the index is written every [`SAVE_EVERY`] entries and at
    /// the end, also after a cancel. Errors on single files go into the report.
    pub fn import(
        &mut self,
        sources: &[PathBuf],
        options: &ImportOptions,
        control: &dyn Control,
    ) -> ImportReport {
        let mut report = ImportReport::default();
        let plan = collect(sources, &options.limits, control, &mut report);
        let tags = clean_tags(&options.tags);
        let licence = clean_licence(&options.licence);
        let total = plan.len().max(1);
        let mut unsaved = 0_usize;
        for (index, found) in plan.iter().enumerate() {
            if control.cancelled() {
                report.cancelled = true;
                break;
            }
            control.progress(index as f32 / total as f32);
            let before = self.entries().len();
            self.import_one(found, &tags, &licence, options.role, &mut report);
            unsaved += self.entries().len() - before;
            if unsaved >= SAVE_EVERY {
                unsaved = 0;
                self.save_logged();
            }
        }
        if !report.added.is_empty()
            && let Err(error) = self.save()
        {
            report.limit = Some(format!("the index could not be written: {error}"));
        }
        control.progress(1.0);
        report
    }

    fn save_logged(&self) {
        if let Err(error) = self.save() {
            log::warn!("cannot write the library index: {error}");
        }
    }

    fn import_one(
        &mut self,
        found: &Found,
        tags: &[String],
        licence: &str,
        role: Role,
        report: &mut ImportReport,
    ) {
        let bytes = match DiskFiles.read(&found.path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                report
                    .failed
                    .push((found.name.clone(), "the file could not be read".to_owned()));
                return;
            }
            Err(refusal) => {
                report
                    .refused
                    .push((found.path.clone(), refusal.to_string()));
                return;
            }
        };
        let (kind, json) = match classify(&found.name, &bytes) {
            Class::Entry { kind, json } => (kind, json),
            Class::Skip(label) => {
                report.skipped.add(label);
                return;
            }
            Class::Companion => return,
        };
        if role == Role::Style && !matches!(kind, Kind::Model | Kind::Texture) {
            report.skipped.add(format!(
                "{} (not a model or a texture)",
                skip_label(&found.name)
            ));
            return;
        }
        let sha256 = hex(&Sha256::digest(&bytes));
        if let Some(known) = self.by_sha(&sha256) {
            report.duplicates += 1;
            if role == Role::Work && known.role == Role::Style {
                // Own work wins: it stays when the folder of the reference is removed.
                let id = known.id.clone();
                self.set_role(&id, Role::Work);
            } else if role == Role::Style && known.role == Role::Style {
                let source = self.source_text(&found.path, role);
                if source != known.source {
                    report.shared.push((source, known.source.clone()));
                }
            }
            return;
        }
        let id = sha256[..16].to_owned();
        let file = stored_name(&id, &found.name);
        if let Err(error) = self.store_file(&file, &bytes) {
            report.failed.push((found.name.clone(), error));
            return;
        }
        report.bytes += bytes.len() as u64;

        let mut companion = None;
        let mut mcmeta = None;
        if kind == Kind::Texture {
            let mut beside = found.path.clone().into_os_string();
            beside.push(".mcmeta");
            if let Ok(Some(text)) = DiskFiles.read(Path::new(&beside)) {
                let name = format!("{file}.mcmeta");
                if self.store_file(&name, &text).is_ok() {
                    companion = Some(name);
                    mcmeta = Some(text);
                }
            }
        }
        let meta = self.describe(
            &id,
            kind,
            json,
            &found.name,
            &bytes,
            mcmeta.as_deref(),
            report,
        );
        self.push(Entry {
            id: id.clone(),
            kind,
            name: found.name.clone(),
            file,
            companion,
            source: self.source_text(&found.path, role),
            sha256,
            size: bytes.len() as u64,
            tags: tags.to_vec(),
            licence: licence.to_owned(),
            role,
            added: now_secs(),
            meta,
        });
        report.added.push(id);
    }

    /// Metadata by kind. A texture also gets its 160 px picture here, a model gets its picture
    /// later from the renderer.
    #[expect(clippy::too_many_arguments)]
    fn describe(
        &self,
        id: &str,
        kind: Kind,
        json: Option<JsonClass>,
        name: &str,
        bytes: &[u8],
        mcmeta: Option<&[u8]>,
        report: &mut ImportReport,
    ) -> Option<Meta> {
        match kind {
            Kind::Model => match model_meta(name, bytes) {
                Ok(meta) => Some(Meta::Model(meta)),
                Err(error) => {
                    log::info!("{name}: {error}");
                    report.unreadable.push(name.to_owned());
                    None
                }
            },
            Kind::Texture => {
                self.write_thumb(id, bytes);
                match texture_meta(bytes, mcmeta) {
                    Ok(meta) => Some(Meta::Texture(meta)),
                    Err(error) => {
                        log::info!("{name}: {error}");
                        report.unreadable.push(name.to_owned());
                        None
                    }
                }
            }
            Kind::Json => json.map(|class| Meta::Json { class }),
            Kind::Lore => Some(Meta::Lore(lore_meta(bytes))),
            Kind::Reference => None,
        }
    }

    fn store_file(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let dir = self.files_dir();
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        cuboid_io::write_atomic(&dir.join(name), bytes).map_err(|e| e.to_string())
    }

    /// Best effort: a texture without a picture shows a placeholder.
    fn write_thumb(&self, id: &str, png: &[u8]) {
        let Some(path) = self.thumb_path(id) else {
            return;
        };
        let written = thumb_png(png).and_then(|small| {
            fs::create_dir_all(self.thumbs_dir()).map_err(|e| e.to_string())?;
            cuboid_io::write_atomic(&path, &small).map_err(|e| e.to_string())
        });
        if let Err(error) = written {
            log::info!("no picture for {id}: {error}");
        }
    }
}

/// Walks the sources and lists the files to import, within the limits.
fn collect(
    sources: &[PathBuf],
    limits: &Limits,
    control: &dyn Control,
    report: &mut ImportReport,
) -> Vec<Found> {
    let mut found = Vec::new();
    let mut bytes = 0_u64;
    let mut visited = 0_usize;
    for source in sources {
        if control.cancelled() {
            report.cancelled = true;
            break;
        }
        if let Some(why) = refused_form(source) {
            report.refused.push((source.clone(), why));
            continue;
        }
        // A file or folder the user chose is followed even when it is a link.
        let Ok(meta) = fs::metadata(source) else {
            report
                .failed
                .push((source.display().to_string(), "it cannot be read".to_owned()));
            continue;
        };
        let mut walk = Walk {
            limits,
            control,
            report,
            found: &mut found,
            bytes: &mut bytes,
            visited: &mut visited,
        };
        if meta.is_dir() {
            walk.folder(source);
        } else if meta.is_file() {
            let name = file_name(source);
            walk.consider(source, name, meta.len());
        }
        if report.limit.is_some() || report.cancelled {
            break;
        }
    }
    found
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The reason a path is not read, from its text alone.
pub(crate) fn refused_form(path: &Path) -> Option<String> {
    match form(&path.to_string_lossy()) {
        Form::Refused(reason) => Some(reason.to_string()),
        _ => None,
    }
}

struct Walk<'a> {
    limits: &'a Limits,
    control: &'a dyn Control,
    report: &'a mut ImportReport,
    found: &'a mut Vec<Found>,
    bytes: &'a mut u64,
    visited: &'a mut usize,
}

impl Walk<'_> {
    fn stop(&self) -> bool {
        self.report.limit.is_some() || self.report.cancelled
    }

    /// Lists the files below `root`, depth first, names in order. Links that lead out of `root`
    /// are refused; a folder is entered once.
    fn folder(&mut self, root: &Path) {
        let Ok(root_real) = fs::canonicalize(root) else {
            self.report.failed.push((
                root.display().to_string(),
                "the folder cannot be read".to_owned(),
            ));
            return;
        };
        let mut seen: HashSet<PathBuf> = HashSet::from([root_real.clone()]);
        let mut stack = vec![(root.to_path_buf(), 0_usize)];
        while let Some((dir, depth)) = stack.pop() {
            if self.control.cancelled() {
                self.report.cancelled = true;
                return;
            }
            let Ok(read) = fs::read_dir(&dir) else {
                self.report.failed.push((
                    dir.display().to_string(),
                    "the folder cannot be read".to_owned(),
                ));
                continue;
            };
            let mut names: Vec<_> = read.filter_map(Result::ok).map(|e| e.file_name()).collect();
            names.sort();
            // The stack is last in, first out; reversed pushes keep the names in order.
            let mut subfolders = Vec::new();
            for name in names {
                *self.visited += 1;
                if *self.visited > self.limits.max_visited {
                    self.report.limit = Some("too many files to look at".to_owned());
                    return;
                }
                let path = dir.join(&name);
                if let Some(why) = refused_form(&path) {
                    self.report.refused.push((path, why));
                    continue;
                }
                let Ok(link_meta) = fs::symlink_metadata(&path) else {
                    continue;
                };
                if link_meta.is_symlink() {
                    match fs::canonicalize(&path) {
                        Ok(real) if real.starts_with(&root_real) => {}
                        Ok(_) => {
                            self.report
                                .refused
                                .push((path, "the link leaves the folder".to_owned()));
                            continue;
                        }
                        Err(_) => continue,
                    }
                }
                let Ok(meta) = fs::metadata(&path) else {
                    continue;
                };
                if meta.is_dir() {
                    let entered = fs::canonicalize(&path).is_ok_and(|real| seen.insert(real));
                    if entered && depth < self.limits.max_depth {
                        subfolders.push((path, depth + 1));
                    }
                } else if meta.is_file() {
                    self.consider(&path, name.to_string_lossy().into_owned(), meta.len());
                    if self.stop() {
                        return;
                    }
                }
            }
            stack.extend(subfolders.into_iter().rev());
        }
    }

    /// Decides by name and size whether a file is listed.
    fn consider(&mut self, path: &Path, name: String, size: u64) {
        if is_companion(&name) {
            return;
        }
        if !is_candidate(&name) {
            self.report.skipped.add(skip_label(&name));
            return;
        }
        if size > self.limits.max_file_bytes {
            self.report
                .skipped
                .add(format!("{} (over the size limit)", skip_label(&name)));
            return;
        }
        if self.found.len() >= self.limits.max_files {
            self.report.limit = Some(format!("stopped at {} files", self.limits.max_files));
            return;
        }
        if *self.bytes + size > self.limits.max_total_bytes {
            self.report.limit = Some("stopped at the size limit".to_owned());
            return;
        }
        *self.bytes += size;
        self.found.push(Found {
            path: path.to_path_buf(),
            name,
        });
    }
}
