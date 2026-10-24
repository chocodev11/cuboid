//! The editing core of the app, the path of the document and the last good state the panic
//! boundary restores. Everything that edits goes through `core` (commands, undo, tick); this
//! type adds what the app owns around it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cuboid_core::{Compat, Core, SwapKind};
use cuboid_fmt_cubik::{CubikError, SaveOptions, save_path};
use cuboid_model::image::Rev;
use cuboid_model::{
    CameraState, CaptureScope, Categories, Document, OverrideCase, Retained, Selection, Snapshot,
    capture,
};

use crate::backup::Stamp;

/// The last good state is refreshed at most this often while the document keeps changing.
const SNAPSHOT_EVERY: Duration = Duration::from_millis(250);

struct Good {
    doc: Document,
    taken: Instant,
}

pub struct Editor {
    pub core: Core,
    /// The file the document is saved to; the file's own copy of it is not trusted.
    path: Option<PathBuf>,
    /// That file as this session last opened or saved it; recorded in the autosaves.
    pub source_stamp: Option<Stamp>,
    /// A recovered document counts as unsaved although the core saw no edit.
    recovered: bool,
    /// Names the document in the recovery folder; new at every [`Editor::replace`].
    doc_id: u64,
    next_doc_id: u64,
    /// The camera poses of the views, stored in the document before a save or an autosave.
    pub cameras: Option<[Option<CameraState>; 4]>,
    /// Counts the documents put in place and the restores, so the scene caches know to rebuild.
    pub generation: u64,
    /// The revision at the last frame.
    seen: Rev,
    good: Good,
    /// The last good state is older than the document.
    good_is_stale: bool,
    /// Makes the restore or the capture of the last good state panic, to test the recovery.
    #[cfg(test)]
    pub fail_restore: bool,
    #[cfg(test)]
    pub fail_capture: bool,
}

/// Everything a worker needs to write the document, taken on the UI thread as pointer copies.
pub struct SaveInput {
    snapshot: Snapshot,
    retained: Option<Arc<Retained>>,
    overrides: Arc<[OverrideCase]>,
    /// The revision the snapshot was taken at.
    pub revision: Rev,
}

impl SaveInput {
    /// Encodes and writes the `.cubik` file atomically. `verify` reads the payload back first;
    /// a save the user relies on asks for it also in release builds.
    pub fn write(&self, path: &Path, verify: bool) -> Result<(), CubikError> {
        let options = SaveOptions {
            verify,
            ..SaveOptions::default()
        };
        self.write_with(path, &options)
    }

    /// A copy any Cubik reads: without the `cuboid.json` entry of the port.
    pub fn write_copy(&self, path: &Path) -> Result<(), CubikError> {
        let options = SaveOptions {
            extension: false,
            verify: true,
            ..SaveOptions::default()
        };
        self.write_with(path, &options)
    }

    fn write_with(&self, path: &Path, options: &SaveOptions) -> Result<(), CubikError> {
        save_path(
            path,
            &self.snapshot,
            self.retained.as_deref(),
            &self.overrides,
            options,
        )
    }
}

/// A fresh model with the default textures.
pub fn empty_document() -> Document {
    Document::new(cuboid_ops::textures::default_list())
}

impl Editor {
    pub fn new(code_page: u16) -> Editor {
        // Production seeds the random generator from the clock.
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|since| u64::try_from(since.as_nanos()).ok())
            .unwrap_or_default();
        let mut core = Core::new(Compat::port(), seed);
        core.env_mut().code_page = code_page;
        let doc = core.doc().clone();
        Editor {
            core,
            path: None,
            source_stamp: None,
            recovered: false,
            doc_id: 1,
            next_doc_id: 2,
            cameras: None,
            generation: 0,
            seen: doc.revision,
            good: Good {
                doc,
                taken: Instant::now(),
            },
            good_is_stale: false,
            #[cfg(test)]
            fail_restore: false,
            #[cfg(test)]
            fail_capture: false,
        }
    }

    pub fn doc(&self) -> &Document {
        self.core.doc()
    }

    pub fn doc_id(&self) -> u64 {
        self.doc_id
    }

    /// The revision of the last good state, the one a restore brings back.
    pub fn good_revision(&self) -> Rev {
        self.good.doc.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.core.is_dirty() || self.recovered
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.path.clone()
    }

    /// The file name without extension, or "Untitled".
    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_stem())
            .map_or_else(
                || "Untitled".to_owned(),
                |stem| stem.to_string_lossy().into_owned(),
            )
    }

    /// Ends a frame that ran to the end, UI included: a change since the last frame refreshes
    /// the last good state (at most every [`SNAPSHOT_EVERY`] while changes go on). Never call it
    /// for a frame that panicked or in safe mode, or the state that caused the panic would
    /// become the good one. The journal is not cleared here: the scene extract of the next
    /// frame reads it first.
    pub fn commit_frame(&mut self, now: Instant) {
        let revision = self.core.doc().revision;
        if revision != self.seen {
            self.seen = revision;
            self.good_is_stale = true;
        }
        if self.good_is_stale && now.duration_since(self.good.taken) >= SNAPSHOT_EVERY {
            self.good = Good {
                doc: self.core.doc().clone(),
                taken: now,
            };
            self.good_is_stale = false;
        }
    }

    /// Takes the last good state back after a panic. The history and the selection start empty,
    /// because their entries may belong to the state that was dropped. The document counts as
    /// unsaved because the restore is a change the user has not saved.
    pub fn restore_last_good(&mut self) {
        #[cfg(test)]
        assert!(!self.fail_restore, "restore failed on purpose");
        let base = self.core.doc().revision;
        if let Err(error) = self
            .core
            .swap_document(self.good.doc.clone(), base, SwapKind::Open)
        {
            log::error!("cannot restore the last good state: {error}");
            return;
        }
        self.recovered = true;
        self.generation += 1;
        self.seen = self.core.doc().revision;
        self.good_is_stale = false;
    }

    /// Puts a prepared document in place (open, new, recovery): history and selection start
    /// empty. `unsaved` marks recovered work.
    pub fn replace(&mut self, doc: Document, path: Option<PathBuf>, unsaved: bool) {
        let base = self.core.doc().revision;
        if let Err(error) = self.core.swap_document(doc, base, SwapKind::Open) {
            log::error!("cannot put the document in place: {error}");
            return;
        }
        self.path = path;
        self.source_stamp = None;
        self.recovered = unsaved;
        self.doc_id = self.next_doc_id;
        self.next_doc_id += 1;
        self.generation += 1;
        // The views adopt the cameras of the new document before the next save.
        self.cameras = None;
        self.seen = self.core.doc().revision;
        self.good = Good {
            doc: self.core.doc().clone(),
            taken: Instant::now(),
        };
        self.good_is_stale = false;
    }

    /// File > New: an empty model that keeps the blueprint and the view settings (tools 2.3).
    pub fn new_document(&self) -> Document {
        let mut doc = empty_document();
        doc.blueprint = self.doc().blueprint.clone();
        doc.view = Arc::clone(&self.doc().view);
        doc
    }

    /// The marking after "No" in the unsaved-changes prompt.
    pub fn mark_clean(&mut self) {
        self.core.mark_saved();
        self.recovered = false;
    }

    /// The copies a worker needs to write the document. With `path` the file records it as its
    /// own path (`mp.sp`); without, the document's current path stays.
    pub fn save_input(&mut self, path: Option<&Path>) -> SaveInput {
        if let Some(cameras) = self.cameras {
            self.core.set_cameras(cameras);
        }
        let doc = Arc::unwrap_or_clone(self.core.export_snapshot());
        self.input_of(doc, self.core.selection(), path)
    }

    /// The same for the last good state, which a panic recovery writes instead of the live
    /// document. Reads nothing from the core, so a damaged live state cannot reach it.
    pub fn good_save_input(&self) -> SaveInput {
        #[cfg(test)]
        assert!(!self.fail_capture, "capture failed on purpose");
        self.input_of(self.good.doc.clone(), &Selection::default(), None)
    }

    fn input_of(&self, mut doc: Document, selection: &Selection, path: Option<&Path>) -> SaveInput {
        let text = path
            .or(self.path.as_deref())
            .map_or_else(String::new, |path| path.display().to_string());
        if doc.props.save_path != text {
            Arc::make_mut(&mut doc.props).save_path = text;
        }
        let snapshot = capture(
            &doc,
            selection,
            CaptureScope {
                categories: Categories::ALL,
                selection_only: false,
            },
        );
        SaveInput {
            snapshot,
            retained: doc.retained.clone(),
            overrides: Arc::clone(&doc.overrides),
            revision: doc.revision,
        }
    }

    /// A save of `revision` to `path` finished: the path becomes the document's, and the
    /// document is clean unless it changed while the worker wrote.
    pub fn mark_saved(&mut self, path: &Path, revision: Rev) {
        self.path = Some(path.to_owned());
        if self.core.doc().revision == revision {
            self.mark_clean();
        }
    }
}

#[cfg(test)]
mod tests {
    use cuboid_core::{Command, ElementKind};

    use super::*;

    fn add_cube(editor: &mut Editor) {
        let outcome = editor.core.execute(Command::AddElement(ElementKind::Cube));
        assert!(outcome.is_done(), "{outcome:?}");
    }

    fn cubes(editor: &Editor) -> usize {
        editor.doc().elements.len()
    }

    #[test]
    fn an_edit_marks_the_document_unsaved() {
        let mut editor = Editor::new(1252);
        editor.commit_frame(Instant::now());
        assert!(!editor.is_dirty());
        add_cube(&mut editor);
        assert!(editor.is_dirty());
        editor.mark_clean();
        assert!(!editor.is_dirty());
    }

    #[test]
    fn restore_brings_back_the_last_committed_state() {
        let mut editor = Editor::new(1252);
        add_cube(&mut editor);
        editor.commit_frame(Instant::now() + SNAPSHOT_EVERY);
        // A change that the frame never committed.
        add_cube(&mut editor);
        assert_eq!(cubes(&editor), 2);
        editor.restore_last_good();
        assert_eq!(cubes(&editor), 1);
        assert!(editor.is_dirty());
    }

    #[test]
    fn the_good_state_is_refreshed_at_a_limited_rate() {
        let mut editor = Editor::new(1252);
        let start = Instant::now();
        add_cube(&mut editor);
        editor.commit_frame(start + SNAPSHOT_EVERY);
        add_cube(&mut editor);
        editor.commit_frame(start + SNAPSHOT_EVERY + Duration::from_millis(10));
        editor.restore_last_good();
        assert_eq!(cubes(&editor), 1);
    }

    #[test]
    fn a_save_during_edits_leaves_the_document_unsaved() {
        let mut editor = Editor::new(1252);
        add_cube(&mut editor);
        let input = editor.save_input(Some(Path::new("a.cubik")));
        add_cube(&mut editor);
        editor.mark_saved(Path::new("a.cubik"), input.revision);
        assert!(editor.is_dirty());
        assert_eq!(editor.name(), "a");

        let input = editor.save_input(None);
        editor.mark_saved(Path::new("a.cubik"), input.revision);
        assert!(!editor.is_dirty());
    }

    #[test]
    fn replace_starts_a_clean_history_and_recovered_work_is_unsaved() {
        let mut editor = Editor::new(1252);
        add_cube(&mut editor);
        editor.replace(empty_document(), None, true);
        assert_eq!(cubes(&editor), 0);
        assert!(editor.is_dirty());
        assert!(!editor.core.undo().is_done(), "the history is empty");
        editor.replace(empty_document(), Some(PathBuf::from("x.cubik")), false);
        assert!(!editor.is_dirty());
        assert_eq!(editor.name(), "x");
    }
}
