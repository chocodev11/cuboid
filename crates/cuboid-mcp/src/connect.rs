//! Connecting a client (spec: agent toolkit R1, 6): the snippets that add the server to Claude
//! Code, Claude Desktop and Codex, and the self-check that `cuboid mcp --check` runs. cuboid
//! writes no file of another program; the user pastes the snippets.

use std::path::Path;

use cuboid_core::{Command, Compat, Core, ElementKind};
use cuboid_validate::{Profile, Severity, Target, validate};
use serde_json::{Value, json};

use crate::render::{Camera, RenderError, Renderer};
use crate::server::{Options, Server};

/// The side of the picture of the self-check.
const CHECK_SIZE: u32 = 64;

/// The ways to add the server to a client, for one program and one project folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snippets {
    /// A command to run in the project: Claude Code, local scope.
    pub claude_code: String,
    /// An entry of `mcpServers` in `claude_desktop_config.json`; Claude Desktop restarts after.
    pub claude_desktop: String,
    /// A command for Codex.
    pub codex_command: String,
    /// The same server in `~/.codex/config.toml`.
    pub codex_toml: String,
}

/// A TOML string: literal when it can be, else with escapes.
fn toml_string(text: &str) -> String {
    if text.contains(['\'', '\n', '\r']) {
        Value::String(text.to_owned()).to_string()
    } else {
        format!("'{text}'")
    }
}

/// The snippets for the program at `exe` serving the folder `root`.
pub fn snippets(exe: &Path, root: &Path) -> Snippets {
    let (exe, root) = (exe.display().to_string(), root.display().to_string());
    let name: String = root
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or("project")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let start = format!("\"{exe}\" mcp --root \"{root}\"");
    let json_string = |text: &str| Value::String(text.to_owned()).to_string();
    Snippets {
        claude_code: format!("claude mcp add --transport stdio cuboid -- {start}"),
        claude_desktop: format!(
            "\"cuboid-{name}\": {{\"command\": {}, \"args\": [\"mcp\", \"--root\", {}]}}",
            json_string(&exe),
            json_string(&root)
        ),
        codex_command: format!("codex mcp add cuboid -- {start}"),
        codex_toml: format!(
            "[mcp_servers.cuboid]\ncommand = {}\nargs = ['mcp', '--root', {}]\ntool_timeout_sec = 120  # the default is 60\n",
            toml_string(&exe),
            toml_string(&root),
        ),
    }
}

/// One step of the self-check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckStep {
    /// What was tried.
    pub name: &'static str,
    /// Whether it worked.
    pub ok: bool,
    /// What it found.
    pub detail: String,
}

/// The result of the self-check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckReport {
    /// The steps, in the order they ran.
    pub steps: Vec<CheckStep>,
}

impl CheckReport {
    /// Whether every step worked.
    pub fn ok(&self) -> bool {
        self.steps.iter().all(|step| step.ok)
    }

    /// One line a step.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for step in &self.steps {
            let mark = if step.ok { "ok  " } else { "FAIL" };
            out.push_str(&format!("{mark} {}: {}\n", step.name, step.detail));
        }
        out
    }
}

/// Reads the brief of the project folder, validates one cube and renders it, as a client's first
/// calls would. Nothing is written.
pub fn self_check(root: &Path, renderer: &mut dyn Renderer) -> CheckReport {
    let mut steps = Vec::new();
    let mut step = |name: &'static str, result: Result<String, String>| {
        let (ok, detail) = match result {
            Ok(detail) => (true, detail),
            Err(detail) => (false, detail),
        };
        steps.push(CheckStep { name, ok, detail });
    };
    let brief =
        Server::new(root, Options { read_only: true }, &mut *renderer).and_then(|mut server| {
            let output = server
                .call("brief_get", json!({"limit": 1}))
                .map_err(|error| error.message)?;
            let entries = output.value["total_entries"].as_u64().unwrap_or(0);
            let version = output.value["brief"]["mc_version"]
                .as_str()
                .filter(|text| !text.is_empty());
            Ok(format!(
                "{} library entries, game version {}",
                entries,
                version.unwrap_or("not set")
            ))
        });
    let reached = brief.is_ok();
    step("brief", brief);
    if !reached {
        return CheckReport { steps };
    }
    let mut core = Core::new(Compat::port(), 0);
    let made = core.execute(Command::AddElement(ElementKind::Cube));
    let target = Target::JavaBlock {
        version: cuboid_fmt_minecraft::latest_version(),
    };
    let issues = validate(core.doc(), &Profile::new(target));
    let errors = issues
        .iter()
        .filter(|issue| issue.severity == Severity::Error)
        .count();
    step(
        "validate",
        if made.is_done() {
            Ok(format!(
                "one cube, {} issues, {errors} of them errors",
                issues.len()
            ))
        } else {
            Err("the core refused to make a cube".to_owned())
        },
    );
    let rendered = match renderer.render(core.doc(), Camera::Iso, CHECK_SIZE) {
        Ok(picture) if picture.coverage().0.is_some() => Ok(format!(
            "a cube drawn at {} x {} px",
            picture.width, picture.height
        )),
        Ok(_) => Err("the picture is empty".to_owned()),
        Err(RenderError::NoGpu) => {
            Err("there is no graphics adapter: the render tools will answer E_NO_GPU".to_owned())
        }
        Err(RenderError::TooLarge(limit)) => Err(format!(
            "the adapter draws at most {limit} px a side, not {CHECK_SIZE}"
        )),
        Err(RenderError::Failed(text)) => Err(text),
    };
    step("render", rendered);
    CheckReport { steps }
}
