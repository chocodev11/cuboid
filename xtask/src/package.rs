//! `cargo xtask package`: installers, bundles and portable archives through cargo-packager
//! (architecture 20.1). Implemented by W7.3.

use std::error::Error;
use std::process::ExitCode;

pub fn run(_args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    println!("package: not implemented yet");
    Ok(ExitCode::SUCCESS)
}
