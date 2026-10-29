//! The user guide `docs/mcp.md` says what the code does: the lines that connect a client, the
//! sample of the check, the tools in the order of the tool list, and the error codes.

mod common;

use std::path::Path;

use common::{Fake, project};
use cuboid_core::{CmdOutcome, Refusal};
use cuboid_mcp::tools::TOOLS;
use cuboid_mcp::{ToolError, self_check, snippets};
use cuboid_model::{AnimationError, GroupError};

const GUIDE: &str = include_str!("../../../docs/mcp.md");

/// The program, its console twin and the project folder of the examples in the guide.
const EXE: &str = r"C:\Program Files\cuboid\cuboid.exe";
const TWIN: &str = r"C:\Program Files\cuboid\cuboid.com";
const ROOT: &str = r"D:\models\lamp";

/// The lines of the guide, trimmed.
fn guide_lines() -> Vec<&'static str> {
    GUIDE.lines().map(str::trim).collect()
}

#[test]
fn the_lines_that_connect_a_client_are_what_snippets_gives_for_the_example_paths() {
    // The guide is written for the release build on Windows: the clients start `cuboid.exe`, the
    // check runs `cuboid.com`.
    let given = snippets(Path::new(EXE), Path::new(ROOT), Some(Path::new(TWIN)));
    // The guide shows the line of PowerShell, which snippets gives on Windows only.
    let check = format!("& {}", given.check.trim_start_matches("& "));
    let guide = guide_lines();
    let mut known = Vec::new();
    for (client, text) in [
        ("Claude Code", &given.claude_code),
        ("Claude Desktop", &given.claude_desktop),
        ("Codex command", &given.codex_command),
        ("Codex settings", &given.codex_toml),
        ("Try it", &check),
    ] {
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        assert!(
            guide.windows(lines.len()).any(|found| found == lines),
            "docs/mcp.md has no {client} line like this:\n{text}"
        );
        known.extend(lines);
    }
    // No other line of the guide may start like one of them.
    let starts = [
        "claude mcp add",
        "codex mcp add",
        "command = ",
        "args = ",
        "\"cuboid-",
        "& \"",
    ];
    for line in guide {
        if starts.iter().any(|start| line.starts_with(start)) {
            assert!(
                known.contains(&line),
                "docs/mcp.md has a connect line that snippets does not give: {line}"
            );
        }
    }
}

/// A line with each run of digits as one `#`, so that a count may change without the guide.
fn masked(line: &str) -> String {
    let mut out = String::new();
    for c in line.chars() {
        if !c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('#') {
            out.push('#');
        }
    }
    out
}

#[test]
fn the_sample_of_the_check_has_the_steps_and_the_wording_of_the_self_check() {
    let dir = project();
    let report = self_check(dir.path(), &mut Fake::default());
    assert!(report.ok(), "{}", report.text());
    let text = report.text();
    let sample: Vec<&str> = guide_lines()
        .into_iter()
        .filter(|line| line.starts_with("ok   "))
        .collect();
    assert_eq!(sample.len(), report.steps.len(), "{sample:?}");
    for ((shown, step), line) in sample.iter().zip(&report.steps).zip(text.lines()) {
        assert!(
            shown.starts_with(&format!("ok   {}: ", step.name)),
            "{shown}"
        );
        // The brief line says what the library holds, which the sample has other words for.
        if step.name != "brief" {
            assert_eq!(masked(shown), masked(line));
        }
    }
}

#[test]
fn the_tool_table_lists_the_tools_in_the_order_of_the_tool_list() {
    let section = GUIDE
        .split("\n## ")
        .find(|part| part.starts_with("4. "))
        .expect("the guide has a section 4");
    let listed: Vec<&str> = section
        .lines()
        .filter_map(|line| line.strip_prefix("| `")?.split_once("` |"))
        .map(|(name, _)| name)
        .collect();
    let tools: Vec<&str> = TOOLS.iter().map(|tool| tool.name).collect();
    assert_eq!(listed, tools);
}

#[test]
fn every_error_code_in_the_guide_is_a_code_the_server_gives() {
    // The 21 codes of `Refusal::code()` and `E_NOT_YET` and `E_FAILED` (spec 5.1).
    let refusals = [
        Refusal::NothingSelected,
        Refusal::NoElements,
        Refusal::BadTarget,
        Refusal::NothingToUndo,
        Refusal::NothingToRedo,
        Refusal::ClipboardEmpty,
        Refusal::NotDeletable,
        Refusal::NotFinite,
        Refusal::OutOfRange,
        Refusal::RotationRestricted,
        Refusal::NoGroups,
        Refusal::Group(GroupError::TooMany),
        Refusal::Group(GroupError::TooDeep),
        Refusal::Group(GroupError::Cycle),
        Refusal::Group(GroupError::Missing),
        Refusal::Group(GroupError::Attached),
        Refusal::Animation(AnimationError::TooMany),
        Refusal::Animation(AnimationError::TooManyKeyframes),
        Refusal::Animation(AnimationError::Missing),
        Refusal::Animation(AnimationError::OutOfRange),
        Refusal::Op(String::new()),
    ];
    assert_eq!(refusals.len(), 21);
    let mut known: Vec<&str> = refusals.iter().map(Refusal::code).collect();
    known.extend(
        [CmdOutcome::NotYet(""), CmdOutcome::Failed(String::new())]
            .iter()
            .filter_map(CmdOutcome::code),
    );
    known.extend(
        [
            ToolError::input(""),
            ToolError::path(""),
            ToolError::read_only(""),
            ToolError::revision("", ""),
            ToolError::session("", ""),
            ToolError::no_match(""),
            ToolError::ambiguous(""),
            ToolError::file_changed(""),
            ToolError::limit(""),
            ToolError::no_gpu(""),
            ToolError::io(""),
            ToolError::internal(""),
            ToolError::export(""),
        ]
        .iter()
        .map(|error| error.code),
    );
    let mentioned: Vec<&str> = GUIDE
        .split(|c: char| !(c.is_ascii_uppercase() || c == '_'))
        .filter(|word| word.starts_with("E_") && word.len() > 2)
        .collect();
    assert!(mentioned.contains(&"E_NO_GPU"), "{mentioned:?}");
    for code in mentioned {
        assert!(known.contains(&code), "docs/mcp.md names {code}");
    }
}
