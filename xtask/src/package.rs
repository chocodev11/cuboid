//! `cargo xtask package [--no-build] [--debug] [--no-installer]`: the Windows release
//! (docs/design/l6_release.md 4).
//!
//! 1. The logo files, `brand/cuboid.ico` among them, are rendered again from the masters
//!    (`cargo xtask brand`; a file is written only when it changed).
//! 2. Release builds of `cuboid-app` and `cuboid-term`; `--no-build` packages the binaries that exist.
//! 3. `target/package/cuboid-<version>/` gets `cuboid.exe`, `cuboid.com` (the console twin
//!    `cuboid-term.exe`), the guide, the licences and the third-party notice.
//! 4. `target/package/cuboid-<version>-windows-x64.zip` is the portable archive, and
//!    `cuboid-<version>-windows-x64-symbols.zip` holds the PDBs of the two programs, which the
//!    package does not ship: a crash report from a user's machine is symbolicated with them.
//! 5. cargo-packager turns the same folder into the per-user NSIS installer,
//!    `cuboid-<version>-windows-x64-setup.exe`. It must be installed, unless `--no-installer`.
//!
//! `--debug` takes the binaries from `target/debug`, names the outputs `cuboid-<version>-debug...`
//! so that a release package stays, and needs `--no-installer`; both flags are for trying the
//! command out without a release build.

use std::env;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::brand;

/// The workspace version: xtask inherits it.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The notices of the third-party code and assets in the package, from the root.
const NOTICES: &str = "packaging/THIRD-PARTY-NOTICES.txt";

/// Files of the package that come from the repository: the path from the root and the path in the package.
const FILES: [(&str, &str); 4] = [
    ("docs/mcp.md", "docs/mcp.md"),
    ("LICENSE-MIT", "LICENSE-MIT"),
    ("LICENSE-APACHE", "LICENSE-APACHE"),
    (NOTICES, "THIRD-PARTY-NOTICES.txt"),
];

/// The debug information of `cuboid.exe` and `cuboid-term.exe`, which the profile writes next to them
/// (`split-debuginfo = "packed"`).
const SYMBOLS: [&str; 2] = ["cuboid.pdb", "cuboid_term.pdb"];

/// A file next to the executable makes it keep every folder next to it (`platform.rs`). Only the
/// portable archive has it; the installed app keeps its folders in the user profile.
const PORTABLE_MARKER: &str = "cuboid.portable";

/// The version `packaging/installer.nsi` was copied from.
const PACKAGER_VERSION: &str = "0.11.8";

#[derive(Default)]
struct Options {
    no_build: bool,
    debug: bool,
    no_installer: bool,
}

pub fn run(args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    let options = parse_args(args)?;
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("package builds the Windows x64 release; run it on Windows x64".into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no parent folder")?;

    // Before the build, so that the build scripts embed the icon just rendered.
    let folder = brand::folder(root);
    match brand::write(&folder)? {
        0 => println!(
            "package: the logo files in {} are up to date",
            folder.display()
        ),
        written => println!(
            "package: wrote {written} logo file(s) in {}",
            folder.display()
        ),
    }

    if !options.no_build {
        build(root, options.debug)?;
    }
    let out_dir = root.join("target").join("package");
    let profile = if options.debug { "debug" } else { "release" };
    let bins = root.join("target").join(profile);
    let name = if options.debug {
        format!("cuboid-{VERSION}-debug")
    } else {
        format!("cuboid-{VERSION}")
    };
    let folder = out_dir.join(&name);
    stage(root, &bins, &folder)?;

    let zip = out_dir.join(format!("{name}-windows-x64.zip"));
    write_zip(&zip, &name, &staged_files(&folder)?, Some(PORTABLE_MARKER))?;
    println!("package: wrote {}", zip.display());

    let symbols = out_dir.join(format!("{name}-windows-x64-symbols.zip"));
    let pdbs = SYMBOLS.map(|pdb| (pdb.to_owned(), bins.join(pdb)));
    write_zip(&symbols, &format!("{name}-symbols"), &pdbs, None)?;
    println!("package: wrote {}", symbols.display());

    if !options.no_installer {
        let setup = installer(root, &out_dir)?;
        println!("package: wrote {}", setup.display());
    }
    if fs::read_to_string(root.join(NOTICES)).is_ok_and(|text| text.contains("placeholder")) {
        eprintln!(
            "package: warning: {NOTICES} is a placeholder; this package is not for publication"
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    for arg in args {
        match arg.as_str() {
            "--no-build" => options.no_build = true,
            "--debug" => options.debug = true,
            "--no-installer" => options.no_installer = true,
            other => {
                return Err(format!(
                    "unknown argument `{other}`; usage: \
                     cargo xtask package [--no-build] [--debug] [--no-installer]"
                ));
            }
        }
    }
    // The installer takes the folder of the release package.
    if options.debug && !options.no_installer {
        return Err("--debug packages the debug binaries; add --no-installer".to_owned());
    }
    Ok(options)
}

fn build(root: &Path, debug: bool) -> Result<(), Box<dyn Error>> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .current_dir(root)
        .args(["build", "--locked", "-p", "cuboid-app", "-p", "cuboid-term"]);
    if !debug {
        command.arg("--release");
    }
    if !command.status()?.success() {
        return Err("cargo build failed".into());
    }
    Ok(())
}

/// Copies the package files into a fresh `dir` (the programs come from `bins`).
fn stage(root: &Path, bins: &Path, dir: &Path) -> Result<(), Box<dyn Error>> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::create_dir_all(dir.join("docs"))?;
    let binaries = [
        (bins.join("cuboid.exe"), "cuboid.exe"),
        (bins.join("cuboid-term.exe"), "cuboid.com"),
    ];
    let files = FILES.map(|(from, to)| (root.join(from), to));
    for (from, to) in binaries.into_iter().chain(files) {
        fs::copy(&from, dir.join(to)).map_err(|err| format!("{}: {err}", from.display()))?;
    }
    Ok(())
}

/// The files of the staged folder in name order, with their paths inside it.
fn staged_files(stage: &Path) -> Result<Vec<(String, PathBuf)>, Box<dyn Error>> {
    let mut files = Vec::new();
    collect(stage, &mut files)?;
    files.sort();
    files
        .into_iter()
        .map(|file| {
            let name = file
                .strip_prefix(stage)?
                .to_string_lossy()
                .replace('\\', "/");
            Ok((name, file))
        })
        .collect()
}

/// Zips `files` (path in the archive, path on disk) under `top/` with fixed timestamps, so the same
/// files give the same archive. `marker` is an empty file added last.
fn write_zip(
    zip: &Path,
    top: &str,
    files: &[(String, PathBuf)],
    marker: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default());
    let mut writer = ZipWriter::new(fs::File::create(zip)?);
    for (name, file) in files {
        let bytes = fs::read(file).map_err(|err| format!("{}: {err}", file.display()))?;
        writer.start_file(format!("{top}/{name}"), options)?;
        writer.write_all(&bytes)?;
    }
    if let Some(marker) = marker {
        writer.start_file(format!("{top}/{marker}"), options)?;
    }
    writer.finish()?;
    Ok(())
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// Runs cargo-packager on `packaging/Packager.toml` with the root and the version filled in, names
/// the setup like the zip and returns it. cargo-packager is taken from `target/tools/bin` (the
/// `--root` of the hint) first, then from the path.
fn installer(root: &Path, out_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let local = root.join("target/tools/bin/cargo-packager.exe");
    let program = if local.is_file() {
        local
    } else {
        PathBuf::from("cargo-packager")
    };
    let output = Command::new(&program)
        .arg("--version")
        .stderr(Stdio::null())
        .output()
        .map_err(|err| {
            format!(
                "cargo-packager {PACKAGER_VERSION} is not installed ({err}); install it with \
                 `cargo install cargo-packager --locked --version {PACKAGER_VERSION} --root target/tools`, \
                 or pass --no-installer"
            )
        })?;
    if !String::from_utf8_lossy(&output.stdout).contains(PACKAGER_VERSION) {
        return Err(format!(
            "{} is not cargo-packager {PACKAGER_VERSION}, the version packaging/installer.nsi was copied from",
            program.display()
        )
        .into());
    }

    let root_text = root.to_string_lossy().replace('\\', "/");
    let config = fs::read_to_string(root.join("packaging/Packager.toml"))?
        .replace("@ROOT@", &root_text)
        .replace("@VERSION@", VERSION);
    let config_path = out_dir.join("Packager.toml");
    fs::write(&config_path, config)?;
    let status = Command::new(&program)
        .current_dir(root)
        .arg("--config")
        .arg(&config_path)
        .status()?;
    if !status.success() {
        return Err("cargo-packager failed".into());
    }
    let setup = out_dir.join(format!("cuboid_{VERSION}_x64-setup.exe"));
    if !setup.is_file() {
        return Err(format!("cargo-packager did not write {}", setup.display()).into());
    }
    let named = out_dir.join(format!("cuboid-{VERSION}-windows-x64-setup.exe"));
    fs::rename(&setup, &named)?;
    Ok(named)
}
