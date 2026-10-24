//! Public contract types and traits of this crate: capabilities, visibility, profiles, format
//! entries, prompts, answers, artifacts, errors and the codec session traits, with their spec
//! references (W0.2). `Registry` (registry.rs) and `write_atomic` (atomic.rs) are re-exported
//! at the crate root.

use std::path::PathBuf;
use std::sync::Arc;

use bitflags::bitflags;
use cuboid_model::{ApplyError, DocSnapshot, EditCtx, ObjRef, RenderRequest, UserMessage};
use thiserror::Error;

bitflags! {
    /// What a registry entry can do.
    ///
    /// spec: interop 4.1
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub struct Caps: u8 {
        /// Open and merge (native formats).
        const LOAD = 1;
        /// Save as.
        const SAVE = 2;
        /// Import.
        const IMPORT = 4;
        /// Export.
        const EXPORT = 8;
    }
}

/// The visibility rule of a registry entry.
///
/// spec: interop 4.1, interop 4.2
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Visibility {
    /// Always present.
    Always,
    /// Removed from the registry by "Hide Minecraft formats" (M).
    Minecraft,
    /// Removed by "Hide Stonehearth formats" (SH).
    Stonehearth,
    /// Removed by "Hide Staxel formats" (SX).
    Staxel,
    /// Removed by "Hide Voxel formats" (V).
    Voxel,
    /// Hidden from dialog lists by "Hide Mesh formats" but still registered (H).
    Mesh,
    /// Present only while the built-in file browser is in use (L).
    BuiltinBrowserOnly,
}

/// The preferences that decide which entries exist and which are listed.
///
/// spec: interop 2.8, interop 4.2, app 10.2
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct FormatVisibility {
    /// `formatHideMinecraft`.
    pub hide_minecraft: bool,
    /// `formatHideStonehearth`.
    pub hide_stonehearth: bool,
    /// `formatHideStaxel`.
    pub hide_staxel: bool,
    /// `formatHideVoxel`.
    pub hide_voxel: bool,
    /// `formatHideMesh`.
    pub hide_mesh: bool,
    /// The built-in file browser is in use (`useNativeFileBrowser` is off).
    pub builtin_browser: bool,
}

/// The output profile of an export. `Cubik29` reproduces the reference output including its
/// quirks; the modern profiles are opt-in and arrive with M8.
///
/// spec: interop 1.2, arch 12.2
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum Profile {
    /// The reference product's output (default).
    #[default]
    Cubik29,
    /// A modern output for a target game version.
    Modern(Target),
}

/// The import interpretation: the reference one, or the corrected one.
///
/// spec: interop 1.2, interop 4.3
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum ImportMode {
    /// Keeps the reference interpretation, including case-sensitive extension matching.
    #[default]
    Cubik29,
    /// Uses the corrected interpretation.
    Modern,
}

/// The game version a modern profile targets, `major.minor.patch`. Provisional: M8 refines it.
///
/// spec: interop 31
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Target {
    /// Major version, 1 for Java Edition.
    pub major: u16,
    /// Minor version.
    pub minor: u16,
    /// Patch version.
    pub patch: u16,
}

/// The wording of a format in messages: the export and import names differ for some formats.
///
/// spec: interop 4.1
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MessageNames {
    /// The name in import messages, or `None` when the format has no importer.
    pub import: Option<&'static str>,
    /// The name in export messages, or `None` when the format has no exporter.
    pub export: Option<&'static str>,
}

/// One entry of the format registry.
///
/// spec: interop 4.1, arch 12.2
pub struct FormatEntry {
    /// The extension without its dot; `png` appears twice (Model Render and Sprite).
    pub key: &'static str,
    /// The dialog label.
    pub label: &'static str,
    /// The dialog filter such as `*.cubik`; the Stonehearth and Staxel filters have no wildcard.
    pub filter: &'static str,
    /// What the entry can do.
    pub caps: Caps,
    /// The names that appear verbatim in dialogs and statuses.
    pub names: MessageNames,
    /// The visibility rule.
    pub visibility: Visibility,
    /// The importer, if the entry imports.
    pub importer: Option<Arc<dyn Importer>>,
    /// The exporter, if the entry exports.
    pub exporter: Option<Arc<dyn Exporter>>,
}

/// Whether a path is looked up for opening or for saving.
///
/// spec: interop 4.3
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LookupMode {
    /// Open mode: the first matching entry with an importer.
    Open,
    /// Save mode: the first matching entry with an exporter.
    Save,
}

/// The result of an extension lookup.
///
/// spec: interop 4.3, interop 5.2 (step 1)
pub enum Lookup<'a> {
    /// The path has no extension.
    NoExtension,
    /// No entry has this key; carries the extension with its dot, for the message.
    Unknown(String),
    /// The entry to use. It may still lack the capability the dialog needs.
    Entry(&'a FormatEntry),
}

/// The file an importer reads, already read by the I/O thread.
///
/// spec: arch 11.3, arch 12.2
#[derive(Clone, Debug)]
pub struct Source {
    /// Where it came from, for the model folder heuristic and the model name.
    pub path: PathBuf,
    /// The whole file.
    pub bytes: Arc<[u8]>,
}

/// What an importer may read besides the file.
///
/// spec: interop 5.3, interop 2.8
#[derive(Clone, Debug)]
pub struct ImportCx {
    /// The interpretation to use.
    pub mode: ImportMode,
    /// The replace flag of the entry point: whether the import resets the model (its meaning
    /// per importer is interop 5.3).
    pub replace: bool,
    /// `objCenterOnImport`.
    pub center_on_import: bool,
}

/// What an exporter may read besides the snapshot.
///
/// spec: interop 1.2, interop 2.8, interop 6.1
#[derive(Clone, Debug)]
pub struct ExportCx {
    /// The output profile.
    pub profile: Profile,
    /// The chosen target path.
    pub path: PathBuf,
    /// The application identity string that some outputs embed; golden tests set the reference
    /// one.
    pub identity: String,
    /// `minecraftExportTextures`: save textures as PNG beside the JSON.
    pub export_textures: bool,
    /// Seed of the injected generator for jitter ("avoid z-fighting").
    pub rng_seed: u64,
}

/// A question an importer or exporter asks mid-way. The UI thread shows it and resumes the
/// session with the [`Answer`]; no worker waits for the UI.
///
/// spec: arch 11.2, interop 8.2, interop 10.3, interop 6.3
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Prompt {
    /// A message box with one to three buttons.
    Message(UserMessage),
    /// A text input dialog.
    Text {
        /// The dialog title.
        title: String,
        /// The pre-filled text.
        initial: String,
    },
}

/// The answer to a [`Prompt`], or the result of a render the session asked for.
///
/// spec: arch 11.2, menus 8.4
#[derive(Clone, Debug)]
pub enum Answer {
    /// A button of the message box, counted from 0.
    Button(u8),
    /// The box was closed with its X.
    Close,
    /// The text of the input dialog; `None` when cancelled.
    Text(Option<String>),
    /// The PNG bytes of a [`BuildStep::NeedRender`] request.
    Rendered(Arc<[u8]>),
}

/// What a session does after an answer.
///
/// spec: arch 6 (cuboid-io), interop 8.2
#[derive(Clone, Debug)]
pub enum Step {
    /// Go on: ask the next prompt or run the next stage.
    Continue,
    /// Stop after showing this message, whatever the earlier answers were.
    Stop(UserMessage),
}

/// A message box that does not wait for an answer: warnings and information boxes shown while
/// or after an operation.
///
/// spec: interop 6.2, interop 8.3
pub type Notice = UserMessage;

/// A sink for the progress an export reports; the texts and cadence are the original's.
///
/// spec: interop 6.2 (step 3), arch 11.1
pub trait ProgressSink {
    /// Sets the status line.
    fn status(&mut self, text: &str);
    /// Reports progress in 0..=1.
    fn progress(&mut self, fraction: f32);
    /// Whether the user cancelled the operation (cancelling long operations is deviation D-P3).
    fn cancelled(&self) -> bool;
}

/// How an output file is written and reported.
///
/// spec: interop 6.4
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OutKind {
    /// A binary file.
    Binary,
    /// A text file, UTF-8 without BOM.
    Text,
    /// A PNG texture.
    Png,
}

/// One file an export produced; the I/O thread writes it atomically and reports it.
///
/// spec: interop 6.4, arch 12.2
#[derive(Clone, Debug)]
pub struct OutFile {
    /// Target path.
    pub path: PathBuf,
    /// Content.
    pub bytes: Vec<u8>,
    /// Which writer-helper messages apply.
    pub kind: OutKind,
}

/// What an export produced, in write order.
///
/// spec: arch 6 (cuboid-io), interop 6.2 (step 5)
#[derive(Clone, Debug, Default)]
pub struct Artifacts {
    /// The files to write.
    pub files: Vec<OutFile>,
    /// Boxes to show without waiting.
    pub notices: Vec<Notice>,
}

/// The result of one `build` call.
///
/// spec: arch 6 (cuboid-io), arch 11.2
#[derive(Clone, Debug)]
pub enum BuildStep {
    /// The session needs a PNG render. The application answers it on the UI thread through the
    /// renderer, passes the PNG bytes in [`Answer::Rendered`] and calls `build` again.
    NeedRender(RenderRequest),
    /// The output is complete.
    Done(Artifacts),
}

/// What an import changed besides the document.
///
/// spec: interop 5.2 (step 5), tools 4.7
#[derive(Clone, Debug, Default)]
pub struct ImportOutcome {
    /// Objects the import created, for the start queue.
    pub created: Vec<ObjRef>,
    /// Boxes to show without waiting.
    pub notices: Vec<Notice>,
}

/// Why an import or export failed.
///
/// spec: arch 16, interop 5.4
#[derive(Debug, Error)]
pub enum IoError {
    /// A contract stub whose package has not landed; the payload names the package.
    #[error("{0} is not implemented yet")]
    Unimplemented(&'static str),
    /// The input or the model cannot be used; the text is the port's own detail message that
    /// follows the original's lead sentence.
    #[error("{0}")]
    Invalid(String),
    /// Reading or writing failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The model rejected the imported data.
    #[error(transparent)]
    Apply(#[from] ApplyError),
}

/// Reads and parses a file; the first stage of an import, run on a worker.
///
/// spec: arch 6 (cuboid-io), arch 11.2
pub trait Importer: Send + Sync {
    /// Reads and parses the source; no document access.
    fn begin(&self, src: Source, cx: &ImportCx) -> Result<Box<dyn ImportSession>, IoError>;
}

/// A resumable import: a step machine that asks questions and finally applies to a working copy
/// of the document, which the UI thread swaps in only on success and only if the base revision
/// is unchanged.
///
/// spec: arch 6 (cuboid-io), arch 11.2, native 6.4
pub trait ImportSession: Send {
    /// The next question, if any: parent prompts, the entity name with Retry, ...
    fn prompt(&mut self) -> Option<Prompt>;

    /// Takes the answer to the last prompt.
    fn answer(&mut self, a: Answer) -> Step;

    /// Applies the import to the working copy; runs on a worker.
    fn apply(self: Box<Self>, work: &mut EditCtx<'_>) -> Result<ImportOutcome, IoError>;
}

/// Starts an export over an immutable snapshot of the document.
///
/// spec: arch 6 (cuboid-io), arch 11.2
pub trait Exporter: Send + Sync {
    /// Starts a session. The snapshot was taken on the UI thread with the cameras captured.
    fn begin(&self, snap: Arc<DocSnapshot>, cx: &ExportCx) -> Box<dyn ExportSession>;
}

/// A resumable export. The flow runs `prompt`, `answer`, `build` and `post_build_prompt`; no
/// worker ever waits for the UI.
///
/// spec: arch 6 (cuboid-io), arch 11.2, interop 6.2
pub trait ExportSession: Send {
    /// The next pre-check question, in the order of the specification.
    fn prompt(&mut self) -> Option<Prompt>;

    /// Takes the answer to the last prompt.
    fn answer(&mut self, a: Answer) -> Step;

    /// Builds the output in memory on a worker; may ask for a render first.
    fn build(&mut self, p: &mut dyn ProgressSink) -> Result<BuildStep, IoError>;

    /// A question after the build, such as the skipped-elements warning.
    fn post_build_prompt(&mut self) -> Option<Prompt>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flows hold the codecs and their sessions as trait objects.
    fn _codec_traits_are_object_safe(
        _: &dyn Importer,
        _: &dyn ImportSession,
        _: &dyn Exporter,
        _: &dyn ExportSession,
        _: &dyn ProgressSink,
    ) {
    }

    fn assert_send<T: Send>() {}

    #[test]
    fn sessions_move_between_threads() {
        assert_send::<Box<dyn ImportSession>>();
        assert_send::<Box<dyn ExportSession>>();
        assert_send::<Artifacts>();
    }

    #[test]
    fn capability_values_follow_interop_4_1() {
        assert_eq!(Caps::LOAD.bits(), 1);
        assert_eq!(Caps::SAVE.bits(), 2);
        assert_eq!(Caps::IMPORT.bits(), 4);
        assert_eq!(Caps::EXPORT.bits(), 8);
    }
}
