//! `history`: the undo steps of a model, and named checkpoints in memory.

use cuboid_core::CmdOutcome;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::ToolError;
use crate::schema::{array_of, boolean, choice, integer, model, object, set, string};
use crate::server::{MAX_CHECKPOINTS, Output, Server};

/// Steps of one undo or redo call.
const MAX_STEPS: usize = 50;
/// Labels a list shows.
const LISTED: usize = 30;
/// The longest name of a checkpoint.
const MAX_NAME: usize = 40;

/// What `history` does.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    List,
    Undo,
    Redo,
    Checkpoint,
    Restore,
}

/// Arguments of `history`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryCall {
    model: String,
    action: Action,
    name: Option<String>,
    steps: Option<usize>,
}

pub(crate) fn history_input() -> Value {
    let mut schema = object(
        json!({
            "model": model(),
            "action": choice(
                &["list", "undo", "redo", "checkpoint", "restore"],
                "list the steps and checkpoints; undo or redo steps; checkpoint the model under a name; restore a checkpoint as one step.",
            ),
            "name": string("Name of the checkpoint, for checkpoint and restore."),
            "steps": integer("Steps to undo or redo (default 1, at most 50)."),
        }),
        &["model", "action"],
    );
    let examples = json!([
        {"model": "lamp.cubik", "action": "list"},
        {"model": "lamp.cubik", "action": "undo", "steps": 2},
        {"model": "lamp.cubik", "action": "checkpoint", "name": "blockout"},
        {"model": "lamp.cubik", "action": "restore", "name": "blockout"}
    ]);
    set(&mut schema, "examples", examples);
    schema
}

pub(crate) fn history_output() -> Value {
    object(
        json!({
            "model": string("The model."),
            "revision": string("The revision after the action."),
            "changed": boolean("The model changed."),
            "steps": integer("Steps undone or redone."),
            "undo": array_of(json!({"type": "string"})),
            "undo_total": integer("Steps that can be undone; the labels are the newest first."),
            "redo": array_of(json!({"type": "string"})),
            "redo_total": integer("Steps that can be redone."),
            "checkpoints": array_of(json!({"type": "string"})),
        }),
        &[
            "model",
            "revision",
            "changed",
            "steps",
            "undo",
            "undo_total",
            "redo",
            "redo_total",
            "checkpoints",
        ],
    )
}

impl Server<'_> {
    pub(crate) fn history(&mut self, args: HistoryCall) -> Result<Output, ToolError> {
        let index = self.model(&args.model)?;
        if args.action != Action::List {
            self.writable(index)?;
        }
        let name = |args: &HistoryCall| match args.name.as_deref() {
            Some(name) if !name.is_empty() && name.chars().count() <= MAX_NAME => {
                Ok(name.to_owned())
            }
            _ => Err(ToolError::input(format!(
                "name is 1 to {MAX_NAME} characters"
            ))),
        };
        let (mut changed, mut steps) = (false, 0);
        match args.action {
            Action::List => {}
            Action::Undo | Action::Redo => {
                let wanted = args.steps.unwrap_or(1).clamp(1, MAX_STEPS);
                let core = &mut self.open_mut(index)?.core;
                for _ in 0..wanted {
                    let outcome = if args.action == Action::Undo {
                        core.undo()
                    } else {
                        core.redo()
                    };
                    match outcome {
                        CmdOutcome::Done(_) => steps += 1,
                        // Fewer steps than asked for is not an error, none at all is.
                        refused if steps == 0 => {
                            return Err(ToolError::of(&refused)
                                .unwrap_or_else(|| ToolError::io("nothing was undone")));
                        }
                        _ => break,
                    }
                }
                changed = steps > 0;
            }
            Action::Checkpoint => {
                let name = name(&args)?;
                let model = self.open_mut(index)?;
                let snapshot = model.core.checkpoint();
                if let Some((_, kept)) = model
                    .checkpoints
                    .iter_mut()
                    .find(|(known, _)| *known == name)
                {
                    *kept = snapshot;
                } else if model.checkpoints.len() >= MAX_CHECKPOINTS {
                    return Err(ToolError::limit(format!(
                        "a model keeps at most {MAX_CHECKPOINTS} checkpoints; reuse a name"
                    )));
                } else {
                    model.checkpoints.push((name, snapshot));
                }
            }
            Action::Restore => {
                let name = name(&args)?;
                let model = self.open_mut(index)?;
                let snapshot = model
                    .checkpoints
                    .iter()
                    .find(|(known, _)| *known == name)
                    .map(|(_, snapshot)| snapshot.clone())
                    .ok_or_else(|| ToolError::no_match(format!("there is no checkpoint {name}")))?;
                match model
                    .core
                    .restore_checkpoint(&format!("Restore {name}"), &snapshot)
                {
                    CmdOutcome::Done(_) => changed = true,
                    CmdOutcome::Unchanged => {}
                    refused => {
                        return Err(ToolError::of(&refused)
                            .unwrap_or_else(|| ToolError::io("the checkpoint was not restored")));
                    }
                }
            }
        }
        let model = self.open(index)?;
        let history = model.core.history();
        // The core lists the oldest step first; the next undo comes first here, as the next redo.
        let mut undo: Vec<&str> = history.undo_labels().collect();
        undo.reverse();
        undo.truncate(LISTED);
        Ok(Output::json(json!({
            "model": model.name,
            "revision": self.token(model.core.doc()),
            "changed": changed,
            "steps": steps,
            "undo": undo,
            "undo_total": history.undo_len(),
            "redo": history.redo_labels().take(LISTED).collect::<Vec<_>>(),
            "redo_total": history.redo_len(),
            "checkpoints": model.checkpoints.iter().map(|(name, _)| name).collect::<Vec<_>>(),
        })))
    }
}
