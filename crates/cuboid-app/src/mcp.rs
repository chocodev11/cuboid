//! `cuboid mcp`: the stdio server of the agent toolkit (spec: agent toolkit R1, 6). It runs
//! before any window or event loop exists. In server mode stdout carries protocol messages and
//! nothing else; the logger, the panic hook and the usage text go to stderr, and the log file
//! keeps the rest.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cuboid_mcp::connect::DESKTOP_CONFIG;
use cuboid_mcp::{Options, Root, ServeError, Snippets, self_check, serve, snippets};

use crate::mcp_render::GpuRenderer;

const USAGE: &str = "usage: cuboid mcp --root <folder> [--read-only] [--check]\n  \
    --root       the project folder, an absolute local folder (required)\n  \
    --read-only  refuse every write to a file\n  \
    --check      serve nothing: read the brief, validate and render one cube, print the result \
    and the lines that add this server to a client";

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
struct McpArgs {
    root: PathBuf,
    read_only: bool,
    check: bool,
}

impl McpArgs {
    /// Reads the arguments after `mcp`.
    fn parse(args: &[String]) -> Result<McpArgs, String> {
        let (mut root, mut read_only, mut check) = (None, false, false);
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--root" if root.is_some() => return Err("--root is given twice".to_owned()),
                "--root" => root = Some(args.next().ok_or("--root needs a folder")?),
                "--read-only" => read_only = true,
                "--check" => check = true,
                other => return Err(format!("unknown argument {other}")),
            }
        }
        let root = root.ok_or("--root <folder> is required")?;
        Ok(McpArgs {
            root: PathBuf::from(root),
            read_only,
            check,
        })
    }
}

/// Runs `cuboid mcp` with the arguments after the word. Exit codes: 0 after the input ended or a
/// check that passed, 2 for a command line or a folder that cannot be used or a check that failed,
/// 1 when the streams broke.
pub fn run(args: &[String]) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return write_stdout(&format!("{USAGE}\n"), ExitCode::SUCCESS);
    }
    let args = match McpArgs::parse(args) {
        Ok(args) => args,
        Err(why) => return refuse(&why),
    };
    if let Err(why) = Root::new(&args.root) {
        return refuse(&why);
    }
    let mut renderer = GpuRenderer::default();
    let code = if args.check {
        check(&args.root, &mut renderer)
    } else {
        serve_stdio(&args, &mut renderer)
    };
    // The client ends the process 300 ms after it closes the input. The device is not torn down:
    // that took about 15 ms more on Vulkan, and the system frees it when the process ends.
    std::mem::forget(renderer);
    code
}

fn refuse(why: &str) -> ExitCode {
    let _ = writeln!(io::stderr(), "cuboid mcp: {why}\n{USAGE}");
    ExitCode::from(2)
}

/// Writes the text to stdout in one call and gives `done` when it went out. A closed pipe, as
/// `| head` makes, is exit code 1 where `print!` would panic.
pub fn write_stdout(text: &str, done: ExitCode) -> ExitCode {
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => done,
        Err(error) => {
            log::error!("cannot write the output: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Serves the folder on stdin and stdout until the input ends.
fn serve_stdio(args: &McpArgs, renderer: &mut GpuRenderer) -> ExitCode {
    log::info!(
        "serving {}{}",
        args.root.display(),
        if args.read_only { ", read only" } else { "" }
    );
    let options = Options {
        read_only: args.read_only,
    };
    match serve(
        &args.root,
        options,
        renderer,
        io::stdin().lock(),
        io::stdout().lock(),
    ) {
        Ok(()) => {
            log::info!("the input ended");
            ExitCode::SUCCESS
        }
        Err(ServeError::Root(why)) => refuse(&why),
        Err(error) => {
            log::error!("{error}");
            let _ = writeln!(io::stderr(), "cuboid mcp: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The lines that connect a client to the folder `root` with the running program. A release build
/// on Windows is a GUI program, so the check runs its console twin `cuboid.com` next to it.
pub fn client_lines(root: &Path) -> Snippets {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("cuboid"));
    let console =
        cfg!(all(windows, not(debug_assertions))).then(|| exe.with_file_name("cuboid.com"));
    snippets(&exe, root, console.as_deref())
}

/// `--check`: the steps of the self-check, the adapter, and the lines for each client.
fn check(root: &Path, renderer: &mut GpuRenderer) -> ExitCode {
    let report = self_check(root, renderer);
    let lines = client_lines(root);
    let text = format!(
        "{}graphics: {}\n\n\
         Claude Code, run in the project:\n{}\n\n\
         Claude Desktop, an entry in mcpServers of {DESKTOP_CONFIG}, then restart it:\n{}\n\n\
         Codex, run in a terminal:\n{}\n\n\
         Codex, or add to ~/.codex/config.toml:\n{}\n",
        report.text(),
        renderer.describe(),
        lines.claude_code,
        lines.claude_desktop,
        lines.codex_command,
        lines.codex_toml,
    );
    let done = if report.ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    };
    write_stdout(&text, done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<McpArgs, String> {
        McpArgs::parse(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn the_options_are_read_and_the_folder_is_required() {
        let args = parse(&["--read-only", "--root", "D:\\lamp", "--check"]).unwrap();
        assert_eq!(
            args,
            McpArgs {
                root: PathBuf::from("D:\\lamp"),
                read_only: true,
                check: true,
            }
        );
        assert!(parse(&["--root", "D:\\lamp"]).is_ok_and(|a| !a.read_only && !a.check));
        assert!(parse(&[]).unwrap_err().contains("--root"));
        assert!(parse(&["--read-only"]).unwrap_err().contains("required"));
        assert!(parse(&["--root"]).unwrap_err().contains("needs a folder"));
        assert!(
            parse(&["--root", "x", "--serve"])
                .unwrap_err()
                .contains("--serve")
        );
        assert!(
            parse(&["--root", "D:\\lamp", "--root", "D:\\other"])
                .unwrap_err()
                .contains("twice")
        );
    }

    #[test]
    fn a_folder_that_went_away_after_the_check_is_refused_with_exit_2() {
        let dir = tempfile::tempdir().unwrap();
        let args = McpArgs {
            root: dir.path().join("gone"),
            read_only: false,
            check: false,
        };
        let code = serve_stdio(&args, &mut GpuRenderer::default());
        assert_eq!(code, ExitCode::from(2));
    }
}
