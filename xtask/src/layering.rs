//! `cargo xtask layering`: the dependency rules of architecture 3.3, checked over `cargo metadata`.
//!
//! Rules 1-3 are checked here. Rule 4 (no I/O and no threads in `cuboid-model`, no views or
//! cameras in `cuboid-ops`) concerns code rather than manifests and stays a review item.

use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::process::{Command, ExitCode};

use serde::Deserialize;

/// Layer of every workspace crate. The test kit is a dev-only crate on the top layer.
const LAYERS: &[(&str, u8)] = &[
    ("cuboid-num", 0),
    ("cuboid-bson", 0),
    ("cuboid-actions", 0),
    ("cuboid-assets", 0),
    ("cuboid-image", 1),
    ("cuboid-voxel", 1),
    ("cuboid-settings", 1),
    ("cuboid-model", 2),
    ("cuboid-ui-kit", 2),
    ("cuboid-ops", 3),
    ("cuboid-scene", 3),
    ("cuboid-io", 3),
    ("cuboid-validate", 3),
    ("cuboid-core", 4),
    ("cuboid-fmt-bbmodel", 4),
    ("cuboid-fmt-cubik", 4),
    ("cuboid-fmt-minecraft", 4),
    ("cuboid-fmt-mesh", 4),
    ("cuboid-fmt-voxel", 4),
    ("cuboid-render", 4),
    ("cuboid-ui", 5),
    ("cuboid-library", 5),
    ("cuboid-shell", 5),
    ("cuboid-app", 6),
    ("cuboid-cli", 6),
    ("cuboid-testkit", 6),
];

/// The tool crate sits outside the layers.
const XTASK: &str = "xtask";

/// Any crate may use the test kit in its tests, so rule 1 skips those dev edges.
const TESTKIT: &str = "cuboid-testkit";

const FORMAT_PREFIX: &str = "cuboid-fmt-";

/// Rule 3: a format crate depends on none of these (core, scene, render and the UI crates).
const FORMAT_FORBIDDEN: &[&str] = &[
    "cuboid-core",
    "cuboid-scene",
    "cuboid-render",
    "cuboid-ui-kit",
    "cuboid-ui",
    "cuboid-shell",
];

const GPU_USERS: &[&str] = &["cuboid-render", "cuboid-shell", "cuboid-app"];
const EGUI_USERS: &[&str] = &["cuboid-ui-kit", "cuboid-ui", "cuboid-shell", "cuboid-app"];
const WINIT_USERS: &[&str] = &["cuboid-shell"];

/// Rule 2: external crates only some workspace crates may depend on. The integration crates
/// follow the library they bind: `egui-wgpu` follows wgpu, `egui-winit` follows winit.
const PLATFORM: &[(&str, &[&str])] = &[
    ("wgpu", GPU_USERS),
    ("egui-wgpu", GPU_USERS),
    ("egui", EGUI_USERS),
    ("winit", WINIT_USERS),
    ("egui-winit", WINIT_USERS),
];

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    /// `None` for normal dependencies, otherwise `"dev"` or `"build"`.
    kind: Option<String>,
}

pub fn run(args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    if !args.is_empty() {
        return Err("layering takes no arguments".into());
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo metadata failed:\n{stderr}").into());
    }
    let metadata: Metadata = serde_json::from_slice(&output.stdout)?;

    let mut problems = check(&metadata.packages);
    problems.extend(stale_entries(&metadata.packages));
    problems.sort();
    // A crate that lists the same dependency as normal and as dev dependency reports it once.
    problems.dedup();
    if problems.is_empty() {
        let checked = metadata
            .packages
            .iter()
            .filter(|pkg| pkg.name != XTASK)
            .count();
        println!("layering: ok ({checked} crates checked)");
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("layering: {} violation(s)", problems.len());
    for problem in &problems {
        eprintln!("  {problem}");
    }
    Ok(ExitCode::FAILURE)
}

/// Rules 1-3 over every workspace package except `xtask`.
fn check(packages: &[Package]) -> Vec<String> {
    let layers: HashMap<&str, u8> = LAYERS.iter().copied().collect();
    let mut problems = Vec::new();

    for pkg in packages.iter().filter(|pkg| pkg.name != XTASK) {
        let name = pkg.name.as_str();
        let Some(&layer) = layers.get(name) else {
            problems.push(format!(
                "{name} has no layer; add it to LAYERS in xtask/src/layering.rs"
            ));
            continue;
        };
        for dep in &pkg.dependencies {
            let dep_name = dep.name.as_str();
            if let Some(&dep_layer) = layers.get(dep_name) {
                let testkit_in_tests = dep_name == TESTKIT && dep.kind.as_deref() == Some("dev");
                if dep_layer >= layer && !testkit_in_tests {
                    problems.push(format!(
                        "rule 1: {name} (L{layer}) depends on {dep_name} (L{dep_layer}); \
                         only lower layers are allowed"
                    ));
                }
                if name.starts_with(FORMAT_PREFIX) && FORMAT_FORBIDDEN.contains(&dep_name) {
                    problems.push(format!(
                        "rule 3: format crate {name} depends on {dep_name}; format crates never \
                         depend on core, scene, render or UI crates"
                    ));
                }
            }
            if let Some((_, users)) = PLATFORM.iter().find(|(platform, _)| *platform == dep_name)
                && !users.contains(&name)
            {
                problems.push(format!(
                    "rule 2: {name} depends on {dep_name}; only {} may",
                    users.join(", ")
                ));
            }
        }
    }
    problems
}

/// Table entries without a workspace member (a renamed or removed crate).
fn stale_entries(packages: &[Package]) -> Vec<String> {
    LAYERS
        .iter()
        .filter(|(name, _)| !packages.iter().any(|pkg| pkg.name == *name))
        .map(|(name, _)| format!("{name} is in LAYERS but is not a workspace member"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, deps: &[(&str, Option<&str>)]) -> Package {
        Package {
            name: name.into(),
            dependencies: deps
                .iter()
                .map(|(dep, kind)| Dependency {
                    name: (*dep).into(),
                    kind: kind.map(Into::into),
                })
                .collect(),
        }
    }

    fn count(problems: &[String], prefix: &str) -> usize {
        problems
            .iter()
            .filter(|problem| problem.starts_with(prefix))
            .count()
    }

    #[test]
    fn graph_of_the_architecture_passes() {
        let packages = [
            package(
                "cuboid-model",
                &[("cuboid-num", None), ("cuboid-image", None)],
            ),
            package(
                "cuboid-fmt-cubik",
                &[("cuboid-io", None), ("cuboid-model", None)],
            ),
            package(
                "cuboid-render",
                &[("cuboid-scene", None), ("wgpu", None), ("egui-wgpu", None)],
            ),
            package(
                "cuboid-core",
                &[("cuboid-model", None), ("cuboid-testkit", Some("dev"))],
            ),
            package("xtask", &[("cuboid-app", None)]),
        ];
        assert!(check(&packages).is_empty());
    }

    #[test]
    fn every_rule_reports_its_violation() {
        let packages = [
            // Same layer and higher layer: rule 1.
            package("cuboid-ops", &[("cuboid-scene", None)]),
            package("cuboid-model", &[("cuboid-ops", None)]),
            // Platform crates outside their users: rule 2.
            package("cuboid-image", &[("wgpu", None)]),
            package("cuboid-ui", &[("winit", None), ("egui", None)]),
            // Scene is a lower layer than a format crate, so only rule 3 objects.
            package("cuboid-fmt-voxel", &[("cuboid-scene", None)]),
            // Testkit outside dev dependencies is an ordinary upward edge.
            package("cuboid-io", &[("cuboid-testkit", None)]),
            package("cuboid-mystery", &[]),
        ];
        let problems = check(&packages);
        assert_eq!(count(&problems, "rule 1"), 3, "{problems:?}");
        assert_eq!(count(&problems, "rule 2"), 2, "{problems:?}");
        assert_eq!(count(&problems, "rule 3"), 1, "{problems:?}");
        assert_eq!(count(&problems, "cuboid-mystery"), 1, "{problems:?}");
    }
}
