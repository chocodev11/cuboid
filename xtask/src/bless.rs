//! `cargo xtask bless [cargo test arguments]`: reruns the tests with `CUBOID_BLESS=1` and
//! `INSTA_UPDATE=always`, so golden files (`cuboid-testkit`) and insta snapshots that fail are
//! rewritten (architecture 19.3). The arguments go to `cargo test`, for example
//! `cargo xtask bless -p cuboid-fmt-cubik fresh`; without them the whole workspace runs. The
//! harness refuses to bless when `CI` is set.

use std::error::Error;
use std::process::{Command, ExitCode};

pub fn run(args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut test = Command::new(cargo);
    test.arg("test").args(args);
    if args.is_empty() {
        test.arg("--workspace");
    }
    let status = test
        .env("CUBOID_BLESS", "1")
        .env("INSTA_UPDATE", "always")
        .status()?;
    if !status.success() {
        return Ok(ExitCode::FAILURE);
    }
    println!("bless: review the rewritten files (git diff, git status) before committing them");
    Ok(ExitCode::SUCCESS)
}
