#![forbid(unsafe_code)]
//! Layer L6. Binary `cuboid-cli`: dump and roundtrip of model files, and the project brief and
//! model check for agents, with no UI and no GPU.
//!
//! Exit codes: 0 ok (roundtrip: identical), 1 usage, 2 error, 3 roundtrip differs, 4 the check
//! found errors.

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

mod args;
mod brief;
mod check;
mod doc;
mod dump;
mod roundtrip;

use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: cuboid-cli <command> [args]

commands:
  dump [--max-lines N] <file>   print the canonical dump of a .cubik or .cub file
                                (N lines at most, default 10000, 0 for all)
  roundtrip [--out FILE] <file> load and save a .cubik file in memory and report
                                whether the saved payload equals the input payload
                                (exit 0 identical, 3 different); --out writes the
                                saved file atomically
  brief [--json | --md | --write] [<folder>]
                                what the project wants: its brief, targets, rules and
                                library as JSON (default) or Markdown; --write writes
                                brief.md next to library.json and prints its path.
                                <folder> is a project folder or a library root
                                (default: the current folder)
  check [--brief <folder>] [--target block|item|pack] [--json] <model>
                                validate a .cubik, .cub, .bbmodel or Minecraft model
                                JSON against the target and the rules of the project
                                brief (exit 0 without errors, 4 with errors); nothing
                                is written

options:
  -h, --help     print this help
  -V, --version  print the version

exit codes: 0 ok, 1 usage, 2 error, 3 roundtrip differs, 4 check found errors";

/// Why a command stops.
#[derive(Debug)]
pub enum Fail {
    /// Bad command line: exit 1.
    Usage(String),
    /// The file cannot be read, parsed or written: exit 2.
    Error(String),
}

impl fmt::Display for Fail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Self::Usage(text) | Self::Error(text)) = self;
        f.write_str(text)
    }
}

/// The options and the one file of a command line.
struct Parsed {
    file: PathBuf,
    /// The value of the option the command accepts.
    option: Option<String>,
}

/// Parses `<file>` and one option with a value (`--name V` or `--name=V`).
fn parse(args: &[String], name: &str) -> Result<Parsed, Fail> {
    let mut file = None;
    let mut option = None;
    let mut rest = args.iter();
    let mut options_end = false;
    while let Some(arg) = rest.next() {
        let value = if options_end || !arg.starts_with('-') {
            if file.replace(PathBuf::from(arg)).is_some() {
                return Err(Fail::Usage("only one file is accepted".to_owned()));
            }
            continue;
        } else if arg == "--" {
            options_end = true;
            continue;
        } else if arg == name {
            rest.next()
                .cloned()
                .ok_or_else(|| Fail::Usage(format!("{name} needs a value")))?
        } else if let Some(value) = arg
            .strip_prefix(name)
            .and_then(|tail| tail.strip_prefix('='))
        {
            value.to_owned()
        } else {
            return Err(Fail::Usage(format!("unknown option `{arg}`")));
        };
        if option.replace(value).is_some() {
            return Err(Fail::Usage(format!("{name} is given twice")));
        }
    }
    let file = file.ok_or_else(|| Fail::Usage("no file given".to_owned()))?;
    Ok(Parsed { file, option })
}

fn run(args: &[String]) -> Result<ExitCode, Fail> {
    let Some((command, rest)) = args.split_first() else {
        println!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    };
    match command.as_str() {
        "-h" | "--help" => println!("{USAGE}"),
        "-V" | "--version" => println!("cuboid-cli {}", env!("CARGO_PKG_VERSION")),
        "dump" => {
            let parsed = parse(rest, "--max-lines")?;
            let max_lines = match parsed.option {
                None => dump::DEFAULT_MAX_LINES,
                Some(text) => text
                    .parse()
                    .map_err(|_| Fail::Usage(format!("`{text}` is not a line count")))?,
            };
            dump::run(&parsed.file, max_lines)?;
        }
        "roundtrip" => {
            let parsed = parse(rest, "--out")?;
            let out = parsed.option.map(PathBuf::from);
            return Ok(if roundtrip::run(&parsed.file, out.as_deref())? {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(3)
            });
        }
        "brief" => brief::run(rest)?,
        "check" => return check::run(rest),
        other => return Err(Fail::Usage(format!("unknown command `{other}`"))),
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(Fail::Usage(text)) => {
            eprintln!("cuboid-cli: {text}\n\n{USAGE}");
            ExitCode::from(1)
        }
        Err(Fail::Error(text)) => {
            eprintln!("cuboid-cli: {text}");
            ExitCode::from(2)
        }
    }
}
