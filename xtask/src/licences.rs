//! `cargo xtask licences`: third-party notices through cargo-about (architecture 20.1). Implemented
//! by W7.3.

use std::error::Error;
use std::process::ExitCode;

pub fn run(_args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    println!("licences: not implemented yet");
    Ok(ExitCode::SUCCESS)
}
