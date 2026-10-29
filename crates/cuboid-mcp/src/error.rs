//! The failure of a tool: a stable code, a message and a hint, which the protocol layer turns
//! into a result with `isError` set (spec: agent toolkit R1, 5.1).

use cuboid_core::CmdOutcome;
use cuboid_library::LibraryError;
use cuboid_library::doc::LoadError;
use serde_json::{Value, json};

use crate::schema::set;

/// A tool that did not do what was asked. The codes are stable; the text may be reworded.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolError {
    /// `E_PATH`, `E_REVISION` and the others of the spec, or the code of a refused command.
    pub code: &'static str,
    /// What went wrong.
    pub message: String,
    /// What to do about it.
    pub hint: String,
    /// The position in `ops` of the op that failed.
    pub op: Option<usize>,
}

impl ToolError {
    fn new(code: &'static str, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: hint.into(),
            op: None,
        }
    }

    /// The arguments do not fit the schema of the tool.
    pub fn input(message: impl Into<String>) -> Self {
        Self::new(
            "E_INPUT",
            message,
            "Check the arguments against the inputSchema of the tool.",
        )
    }

    /// A path that leaves the project folder or has a form that is never read.
    pub fn path(message: impl Into<String>) -> Self {
        Self::new(
            "E_PATH",
            message,
            "Use a path relative to the project folder, with no `..`.",
        )
    }

    /// A write in a server that was started with `--read-only`, or to a read-only file.
    pub fn read_only(message: impl Into<String>) -> Self {
        Self::new(
            "E_READ_ONLY",
            message,
            "Nothing was written. Restart the server without --read-only, or use another path.",
        )
    }

    /// The model was edited since the revision the caller read.
    pub fn revision(expected: &str, current: &str) -> Self {
        Self::new(
            "E_REVISION",
            format!("the model is at revision {current}, not {expected}"),
            "Read the model again (model_outline, model_get) and repeat the edit.",
        )
    }

    /// The revision comes from another run of the server.
    pub fn session(expected: &str, current: &str) -> Self {
        Self::new(
            "E_SESSION",
            format!(
                "revision {expected} is from another run of the server (this one is {current})"
            ),
            "The server restarted: unsaved edits are lost. Open the model again.",
        )
    }

    /// A selector or a name that matches nothing.
    pub fn no_match(message: impl Into<String>) -> Self {
        Self::new(
            "E_NO_MATCH",
            message,
            "List what exists with model_outline or model_find.",
        )
    }

    /// A path that names more than one object.
    pub fn ambiguous(message: impl Into<String>) -> Self {
        Self::new(
            "E_AMBIGUOUS",
            message,
            "Name the object by its id (e12, g3) instead of its path.",
        )
    }

    /// The file on disk is not the one this server read or wrote last, or an export would replace
    /// files that are there.
    pub fn file_changed(message: impl Into<String>) -> Self {
        Self::new(
            "E_FILE_CHANGED",
            message,
            "Something else wrote the file. Pass overwrite: true to replace it, or save to another path.",
        )
    }

    /// A limit of the server.
    pub fn limit(message: impl Into<String>) -> Self {
        Self::new("E_LIMIT", message, "Split the work into smaller calls.")
    }

    /// No graphics adapter for the render tools.
    pub fn no_gpu(message: impl Into<String>) -> Self {
        Self::new(
            "E_NO_GPU",
            message,
            "The render tools need a graphics adapter; everything else works without one.",
        )
    }

    /// A file that cannot be read or written, a library or an export that failed.
    pub fn io(message: impl Into<String>) -> Self {
        Self::new("E_IO", message, "Check the file and the project folder.")
    }

    /// A bug of the server: a tool panicked, or state that cannot be wrong was.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            "E_INTERNAL",
            message,
            "This is a bug of the server, not of the call. Open the model again.",
        )
    }

    /// A model that the target of an export does not take.
    pub fn export(message: impl Into<String>) -> Self {
        Self::new(
            "E_EXPORT",
            message,
            "Run validate for the issues of the target and fix them, apply_fixes does some.",
        )
    }

    /// The error of a command that was not done. `None` for `Done` and `Unchanged`.
    pub fn of(outcome: &CmdOutcome) -> Option<Self> {
        let code = outcome.code()?;
        let message = match outcome {
            CmdOutcome::Refused(refusal) => refusal.to_string(),
            CmdOutcome::NotYet(why) => format!("not available yet: {why}"),
            CmdOutcome::Failed(why) => why.clone(),
            CmdOutcome::Done(_) | CmdOutcome::Unchanged => return None,
        };
        Some(Self::new(
            code,
            message,
            "The batch was rolled back; nothing changed.",
        ))
    }

    /// The same error, for the op at `index` of the batch; an error that names an op keeps it.
    pub fn at(mut self, index: usize) -> Self {
        self.op.get_or_insert(index);
        self
    }

    /// The same error with what to do about this case.
    pub fn hinted(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    /// The error as the JSON the client reads.
    pub fn to_json(&self) -> Value {
        let mut error = json!({"code": self.code, "message": self.message, "hint": self.hint});
        if let Some(op) = self.op {
            set(&mut error, "op", json!(op));
        }
        error
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(error: serde_json::Error) -> Self {
        Self::input(error.to_string())
    }
}

impl From<std::io::Error> for ToolError {
    fn from(error: std::io::Error) -> Self {
        Self::io(error.to_string())
    }
}

impl From<LibraryError> for ToolError {
    fn from(error: LibraryError) -> Self {
        match error {
            LibraryError::Source(text) => Self::path(text),
            other => Self::io(other.to_string()),
        }
    }
}

impl From<LoadError> for ToolError {
    fn from(error: LoadError) -> Self {
        match error {
            LoadError::Unsupported => Self::path(error.to_string()),
            LoadError::Failed(text) => Self::io(text),
        }
    }
}
