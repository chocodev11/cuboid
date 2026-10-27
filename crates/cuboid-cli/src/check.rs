//! `check`: validates a model file against a target and the rules of the project brief, as the
//! Validator panel does, without the app. Reads only; nothing is written.

use std::path::Path;
use std::process::ExitCode;

use cuboid_library::check::{Report, Want, check};

use crate::args::Args;
use crate::{Fail, doc, dump};

fn parse_target(text: &str) -> Result<Want, Fail> {
    match text {
        "block" => Ok(Want::Block),
        "item" => Ok(Want::Item),
        "pack" => Ok(Want::Pack),
        other => Err(Fail::Usage(format!(
            "`{other}` is not a target; use block, item or pack"
        ))),
    }
}

fn plural(count: usize, word: &str) -> String {
    format!("{count} {word}{}", if count == 1 { "" } else { "s" })
}

/// One line per issue: severity, code, the elements, the message and whether a fix exists, then a
/// summary line.
fn text(report: &Report) -> String {
    let mut out = format!(
        "{} against {}{}\n",
        report.model,
        report.target,
        report
            .project
            .as_ref()
            .map_or_else(String::new, |root| format!(", project rules from {root}"))
    );
    for issue in &report.issues {
        let names: Vec<&str> = issue.objects.iter().map(|o| o.name.as_str()).collect();
        let objects = if names.is_empty() {
            String::new()
        } else {
            format!(" [{}]", names.join(", "))
        };
        let fix = if issue.fix { " (fix available)" } else { "" };
        out.push_str(&format!(
            "{} {}{objects} {}{fix}\n",
            issue.severity, issue.code, issue.message
        ));
    }
    out.push_str(&format!(
        "{}, {}, {}\n",
        plural(report.errors, "error"),
        plural(report.warnings, "warning"),
        plural(report.infos, "note")
    ));
    out
}

/// Prints the issues in the format asked for. Exit code 0 without errors, 4 with errors.
pub fn run(args: &[String]) -> Result<ExitCode, Fail> {
    let args = Args::split(args, &["--json"], &["--brief", "--target"])?;
    let model = args
        .word("model")?
        .ok_or_else(|| Fail::Usage("no model given".to_owned()))?;
    let want = args.option("--target").map(parse_target).transpose()?;
    let report = check(
        Path::new(model),
        args.option("--brief").map(Path::new),
        want,
    )
    .map_err(|err| doc::fail(err, "checked"))?;
    let text = if args.flag("--json") {
        serde_json::to_string_pretty(&report).map_err(|err| Fail::Error(err.to_string()))?
    } else {
        for note in &report.notes {
            eprintln!("cuboid-cli: note: {note}");
        }
        text(&report)
    };
    dump::print(&text, 0).map_err(|err| Fail::Error(format!("cannot write the output: {err}")))?;
    Ok(if report.errors > 0 {
        ExitCode::from(4)
    } else {
        ExitCode::SUCCESS
    })
}
