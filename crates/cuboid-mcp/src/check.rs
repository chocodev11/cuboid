//! `validate`: the model check of `cuboid-cli check`, on the model as it is open, with the ids and
//! paths an edit takes.

use cuboid_library::check::{Want, check_document};
use cuboid_model::ObjRef;
use cuboid_validate::validate_self_test;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::brief::offset_of;
use crate::error::ToolError;
use crate::fixes::fixable;
use crate::schema::{array_of, boolean, choice, integer, model, object, paging, string};
use crate::select::{Tree, id_of};
use crate::server::{Output, Server};

/// Issues on a page; 40 are about 10 KB.
const PAGE: usize = 40;
const MAX_PAGE: usize = 60;

/// What a check is for.
#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetWord {
    /// A Java block model.
    Block,
    /// A Java item model.
    Item,
    /// A resource pack.
    Pack,
}

impl From<TargetWord> for Want {
    fn from(word: TargetWord) -> Self {
        match word {
            TargetWord::Block => Want::Block,
            TargetWord::Item => Want::Item,
            TargetWord::Pack => Want::Pack,
        }
    }
}

/// Arguments of `validate`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Validate {
    model: String,
    target: Option<TargetWord>,
    self_test: Option<bool>,
    cursor: Option<String>,
    limit: Option<usize>,
}

pub(crate) fn validate_input() -> Value {
    let mut properties = json!({
        "model": model(),
        "target": choice(
            &["block", "item", "pack"],
            "What to check for; default what the model is (Cubik file, Java block or item).",
        ),
        "self_test": boolean("Also prove on this model that every rule still fires on its own defect."),
    });
    paging(&mut properties, "Issues", PAGE);
    let mut schema = object(properties, &["model"]);
    schema["examples"] = json!([
        {"model": "lamp.cubik"},
        {"model": "lamp.cubik", "target": "pack", "self_test": true, "cursor": "40"}
    ]);
    schema
}

pub(crate) fn validate_output() -> Value {
    object(
        json!({
            "model": string("The model."),
            "revision": string("The revision that was checked."),
            "target": string("What it was checked against."),
            "errors": integer("Issues of the severity error."),
            "warnings": integer("Issues of the severity warning."),
            "infos": integer("Issues of the severity info."),
            "fixable": integer("Issues that apply_fixes can fix."),
            "offset": integer("Position of the first issue listed."),
            "next": string("Cursor of the next page; left out on the last."),
            "notes": array_of(json!({"type": "string"})),
            "issues": array_of(object(
                json!({
                    "code": string("Stable code of the rule."),
                    "severity": choice(&["error", "warning", "info"], "How bad."),
                    "ids": array_of(json!({"type": "string"})),
                    "paths": array_of(json!({"type": "string"})),
                    "message": string("What is wrong."),
                    "hint": string("What to do about it."),
                    "fix": boolean("apply_fixes can fix it."),
                }),
                &["code", "severity", "ids", "paths", "message", "hint", "fix"],
            )),
            "self_test": object(
                json!({
                    "ok": boolean("No rule stayed silent."),
                    "fired": integer("Rules that reported their defect."),
                    "silent": array_of(json!({"type": "string"})),
                }),
                &["ok", "fired", "silent"],
            ),
        }),
        &[
            "model", "revision", "target", "errors", "warnings", "infos", "fixable", "offset",
            "issues",
        ],
    )
}

impl Server<'_> {
    pub(crate) fn validate(&mut self, args: Validate) -> Result<Output, ToolError> {
        let offset = offset_of(args.cursor.as_deref())?;
        let library = self.read_library()?;
        let index = self.model(&args.model)?;
        let model = &self.models[index];
        let doc = model.core.doc();
        let checked = check_document(
            &model.path,
            doc,
            model.project.as_ref(),
            model.notes.clone(),
            Some(&library),
            args.target.map(Into::into),
        );
        let tree = Tree::of(doc);
        let total = checked.issues.len();
        let offset = offset.min(total);
        let limit = args.limit.unwrap_or(PAGE).clamp(1, MAX_PAGE);
        let end = (offset + limit).min(total);
        let issues: Vec<Value> = checked
            .issues
            .get(offset..end)
            .unwrap_or_default()
            .iter()
            .zip(checked.report.issues.get(offset..end).unwrap_or_default())
            .map(|(issue, found)| {
                let objects = issue.objects.iter().map(|&id| ObjRef::Element(id));
                json!({
                    "code": found.code,
                    "severity": found.severity,
                    "ids": objects.clone().filter_map(id_of).collect::<Vec<_>>(),
                    "paths": objects.filter_map(|obj| tree.path_of(obj)).collect::<Vec<_>>(),
                    "message": found.message,
                    "hint": found.hint,
                    "fix": fixable(issue),
                })
            })
            .collect();
        let report = &checked.report;
        let mut value = json!({
            "model": model.name,
            "revision": self.token(doc),
            "target": report.target,
            "errors": report.errors,
            "warnings": report.warnings,
            "infos": report.infos,
            "fixable": checked.issues.iter().filter(|issue| fixable(issue)).count(),
            "offset": offset,
            "notes": report.notes,
            "issues": issues,
        });
        if end < total {
            value["next"] = json!(end.to_string());
        }
        if args.self_test == Some(true) {
            let test = validate_self_test(doc, &checked.profile);
            value["self_test"] = json!({
                "ok": test.ok(),
                "fired": test.fired(),
                "silent": test.silent(),
            });
        }
        Ok(Output::json(value))
    }
}
