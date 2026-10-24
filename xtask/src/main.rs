#![forbid(unsafe_code)]
//! Workspace automation, run as `cargo xtask <command>` (alias in `.cargo/config.toml`).

#![deny(clippy::cast_possible_truncation, clippy::float_cmp)]

mod bench;
mod bless;
mod layering;
mod ledger;
mod licences;
mod package;

use std::error::Error;
use std::process::ExitCode;

type Command = fn(&[String]) -> Result<ExitCode, Box<dyn Error>>;

const COMMANDS: &[(&str, Command)] = &[
    ("layering", layering::run),
    ("ledger", ledger::run),
    ("bless", bless::run),
    ("licences", licences::run),
    ("package", package::run),
    ("bench-compare", bench::run),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((name, rest)) = args.split_first() else {
        return usage("missing command");
    };
    let Some((_, run)) = COMMANDS.iter().find(|(known, _)| *known == name.as_str()) else {
        return usage(&format!("unknown command `{name}`"));
    };
    match run(rest) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("xtask {name}: {err}");
            ExitCode::from(2)
        }
    }
}

fn usage(problem: &str) -> ExitCode {
    let names: Vec<&str> = COMMANDS.iter().map(|(name, _)| *name).collect();
    eprintln!("xtask: {problem}\nusage: cargo xtask <{}>", names.join("|"));
    ExitCode::from(2)
}
