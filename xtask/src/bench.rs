//! `cargo xtask bench-compare`: criterion results against the stored baselines, failing on
//! regressions above 10 % (architecture 18). Implemented by W7.1.

use std::error::Error;
use std::process::ExitCode;

pub fn run(_args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    println!("bench-compare: not implemented yet");
    Ok(ExitCode::SUCCESS)
}
