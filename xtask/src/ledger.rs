//! `cargo xtask ledger`: validates the parity ledger and prints the coverage per specification
//! document (architecture 19.2). The file format is described in `parity/README.md`.
//!
//! ```text
//! cargo xtask ledger [--no-tests] [--wp <id>]...
//! ```
//!
//! Every `parity/ledger/<wp>.toml` is checked for: valid TOML with known keys, tiers and statuses;
//! ids that start with their `spec` reference and are unique over all files; references to an
//! existing document and section; tests of known packages for `tested` and `verified-o1`; a note
//! for `unverified-spec`. Unless `--no-tests` is given, the tests named by `tested` and
//! `verified-o1` entries must also be listed by `cargo test`; `--wp` limits that part to the
//! entries of the named work packages (it builds only the packages they name).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use serde::Deserialize;

/// Short name of a document in `spec` references, and its path from the repository root.
const DOCS: &[(&str, &str)] = &[
    ("arch", "docs/design/architecture.md"),
    ("native", "docs/spec/formats_native.md"),
    ("interop", "docs/spec/formats_interop.md"),
    ("tools", "docs/spec/model_and_tools.md"),
    ("ui", "docs/spec/ui_layout_theme.md"),
    ("menus", "docs/spec/ui_dialogs_menus.md"),
    ("app", "docs/spec/app_behavior.md"),
];

/// Section numbers of every document, by short name.
type Docs = HashMap<&'static str, HashSet<String>>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerFile {
    /// Work package id, `W1.10`; the file is named after it in lower case.
    wp: String,
    #[serde(default)]
    entry: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    /// `<document>.<section>.<slug>`, for example `native.4.4.3.fold`.
    id: String,
    /// `<document> <section>`, for example `native 4.4.3`.
    spec: String,
    tier: Tier,
    status: Status,
    summary: String,
    /// `<package>::<test path as listed by cargo test>`.
    #[serde(default)]
    tests: Vec<String>,
    note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
enum Tier {
    P0,
    P1,
    P2,
    P3,
    P4,
}

const TIERS: [Tier; 5] = [Tier::P0, Tier::P1, Tier::P2, Tier::P3, Tier::P4];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Todo,
    Implemented,
    Tested,
    VerifiedO1,
    UnverifiedSpec,
}

const STATUSES: [(Status, &str); 5] = [
    (Status::Todo, "todo"),
    (Status::Implemented, "implemented"),
    (Status::Tested, "tested"),
    (Status::VerifiedO1, "verified-o1"),
    (Status::UnverifiedSpec, "unverified-spec"),
];

impl Status {
    /// Entries at these statuses name tests that must exist.
    fn needs_tests(self) -> bool {
        matches!(self, Self::Tested | Self::VerifiedO1)
    }
}

/// A ledger file with the name it is reported under.
struct Loaded {
    /// `parity/ledger/w0.5.toml`
    file: String,
    /// `w0.5`
    stem: String,
    ledger: LedgerFile,
}

#[derive(Default)]
struct Options {
    skip_tests: bool,
    wps: Vec<String>,
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    manifest_path: PathBuf,
}

pub fn run(args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    let options = parse_args(args)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("xtask has no parent folder")?;
    let (ledgers, mut problems) = load_ledgers(&root.join("parity").join("ledger"))?;
    let docs = load_docs(root)?;
    let packages = workspace_packages(root)?;
    let names: HashSet<&str> = packages.iter().map(|pkg| pkg.name.as_str()).collect();
    problems.extend(check(&ledgers, &docs, &names));
    problems.extend(unknown_wps(&options, &ledgers));
    if problems.is_empty() && !options.skip_tests {
        problems.extend(check_tests(root, &packages, &ledgers, &options)?);
    }
    if problems.is_empty() {
        print!("{}", render(&summarize(&ledgers)));
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("ledger: {} problem(s)", problems.len());
    for problem in &problems {
        eprintln!("  {problem}");
    }
    Ok(ExitCode::FAILURE)
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-tests" => options.skip_tests = true,
            "--wp" => {
                let wp = args.next().ok_or("--wp needs a work package id")?;
                options.wps.push(wp.to_uppercase());
            }
            other => {
                return Err(format!(
                    "unknown argument `{other}`; usage: \
                     cargo xtask ledger [--no-tests] [--wp <id>]..."
                ));
            }
        }
    }
    Ok(options)
}

/// Reads every `*.toml` file of the ledger folder in name order. Files that cannot be read or
/// parsed are problems, not errors, so one broken file does not hide the others.
fn load_ledgers(dir: &Path) -> Result<(Vec<Loaded>, Vec<String>), Box<dyn Error>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|err| format!("{}: {err}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    paths.retain(|path| path.extension().is_some_and(|ext| ext == "toml"));
    paths.sort();
    let (mut loaded, mut problems) = (Vec::new(), Vec::new());
    for path in paths {
        let stem = path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let file = format!("parity/ledger/{stem}.toml");
        match fs::read_to_string(&path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(ledger) => loaded.push(Loaded { file, stem, ledger }),
                Err(err) => problems.push(format!("{file}: {err}")),
            },
            Err(err) => problems.push(format!("{file}: {err}")),
        }
    }
    Ok((loaded, problems))
}

fn load_docs(root: &Path) -> Result<Docs, Box<dyn Error>> {
    DOCS.iter()
        .map(|&(name, path)| {
            let text =
                fs::read_to_string(root.join(path)).map_err(|err| format!("{path}: {err}"))?;
            Ok((name, section_numbers(&text)))
        })
        .collect()
}

/// The numbers of the numbered headings (`## 4.`, `### 4.4.3 Title`), outside code blocks.
fn section_numbers(markdown: &str) -> HashSet<String> {
    let mut in_code = false;
    let mut numbers = HashSet::new();
    for line in markdown.lines() {
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let Some(rest) = line.strip_prefix('#') else {
            continue;
        };
        let Some(title) = rest.trim_start_matches('#').strip_prefix(' ') else {
            continue;
        };
        let number = title
            .split_whitespace()
            .next()
            .map(|word| word.trim_end_matches('.'));
        if let Some(number) = number.filter(|number| is_section_number(number)) {
            numbers.insert(number.to_owned());
        }
    }
    numbers
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// `4`, `4.4.3`
fn is_section_number(text: &str) -> bool {
    text.split('.').all(is_digits)
}

/// `W1.10`
fn is_wp_id(text: &str) -> bool {
    text.strip_prefix('W')
        .and_then(|rest| rest.split_once('.'))
        .is_some_and(|(major, minor)| is_digits(major) && is_digits(minor))
}

/// `("native", "4.4.3")` from `native 4.4.3`.
fn parse_spec(spec: &str) -> Option<(&str, &str)> {
    let (doc, section) = spec.split_once(' ')?;
    is_section_number(section).then_some((doc, section))
}

/// The static rules over all files.
fn check(ledgers: &[Loaded], docs: &Docs, packages: &HashSet<&str>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut first_use: HashMap<&str, &str> = HashMap::new();
    for Loaded { file, stem, ledger } in ledgers {
        if !is_wp_id(&ledger.wp) {
            problems.push(format!(
                "{file}: wp `{}` is not a work package id such as W1.10",
                ledger.wp
            ));
        } else if ledger.wp.to_lowercase() != *stem {
            problems.push(format!(
                "{file}: wp `{}` does not match the file name; expected {}.toml",
                ledger.wp,
                ledger.wp.to_lowercase()
            ));
        }
        for entry in &ledger.entry {
            let at = format!("{file}: {}", entry.id);
            let found = check_entry(entry, docs, packages);
            problems.extend(found.into_iter().map(|problem| format!("{at}: {problem}")));
            if let Some(other) = first_use.insert(&entry.id, file) {
                problems.push(format!("{at}: id is already used in {other}"));
            }
        }
    }
    problems
}

fn check_entry(entry: &Entry, docs: &Docs, packages: &HashSet<&str>) -> Vec<String> {
    let mut problems = Vec::new();
    match parse_spec(&entry.spec) {
        None => problems.push(format!(
            "spec `{}` is not `<document> <section>`, for example `native 4.4.3`",
            entry.spec
        )),
        Some((doc, section)) => {
            match docs.get(doc) {
                None => {
                    let known: Vec<&str> = DOCS.iter().map(|&(name, _)| name).collect();
                    problems.push(format!(
                        "unknown document `{doc}` in spec; known documents: {}",
                        known.join(", ")
                    ));
                }
                Some(sections) if !sections.contains(section) => {
                    problems.push(format!("document `{doc}` has no section {section}"));
                }
                Some(_) => {}
            }
            let slug = entry.id.strip_prefix(&format!("{doc}.{section}."));
            if !slug.is_some_and(is_slug) {
                problems.push(format!(
                    "id must be `{doc}.{section}.<slug>` with a slug of lower-case letters, \
                     digits and hyphens"
                ));
            }
        }
    }
    if entry.summary.trim().is_empty() {
        problems.push("summary is empty".into());
    }
    if entry.status.needs_tests() && entry.tests.is_empty() {
        problems.push("status needs at least one test".into());
    }
    if entry.status == Status::UnverifiedSpec
        && entry.note.as_deref().is_none_or(|n| n.trim().is_empty())
    {
        problems.push("`unverified-spec` needs a note that names the gap".into());
    }
    for test in &entry.tests {
        match test.split_once("::") {
            Some((package, name)) if !name.is_empty() => {
                if !packages.contains(package) {
                    problems.push(format!(
                        "test `{test}`: `{package}` is not a workspace package"
                    ));
                }
            }
            _ => problems.push(format!("test `{test}` is not `<package>::<test path>`")),
        }
    }
    problems
}

fn is_slug(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `--wp` ids without a ledger file.
fn unknown_wps(options: &Options, ledgers: &[Loaded]) -> Vec<String> {
    options
        .wps
        .iter()
        .filter(|wp| {
            !ledgers
                .iter()
                .any(|loaded| loaded.ledger.wp.eq_ignore_ascii_case(wp))
        })
        .map(|wp| format!("--wp {wp}: no ledger file for this work package"))
        .collect()
}

fn workspace_packages(root: &Path) -> Result<Vec<Package>, Box<dyn Error>> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo metadata failed:\n{stderr}").into());
    }
    Ok(serde_json::from_slice::<Metadata>(&output.stdout)?.packages)
}

/// Verifies that the tests of `tested` and `verified-o1` entries exist: builds the test binaries
/// of the packages involved and asks each for its test list. The build runs with `--locked`, so
/// it never rewrites `Cargo.lock`.
fn check_tests(
    root: &Path,
    packages: &[Package],
    ledgers: &[Loaded],
    options: &Options,
) -> Result<Vec<String>, Box<dyn Error>> {
    let entries = entries_to_verify(ledgers, options);
    let involved: BTreeSet<&str> = entries
        .iter()
        .flat_map(|(_, entry)| &entry.tests)
        .filter_map(|test| test.split_once("::").map(|(package, _)| package))
        .collect();
    if involved.is_empty() {
        return Ok(Vec::new());
    }
    let listed = list_workspace_tests(root, packages, &involved)?;
    Ok(missing_tests(&entries, &listed))
}

/// The entries whose tests must exist, limited to the selected work packages.
fn entries_to_verify<'a>(ledgers: &'a [Loaded], options: &Options) -> Vec<(&'a Loaded, &'a Entry)> {
    let selected = |loaded: &&Loaded| {
        options.wps.is_empty()
            || options
                .wps
                .iter()
                .any(|wp| loaded.ledger.wp.eq_ignore_ascii_case(wp))
    };
    ledgers
        .iter()
        .filter(selected)
        .flat_map(|loaded| loaded.ledger.entry.iter().map(move |entry| (loaded, entry)))
        .filter(|(_, entry)| entry.status.needs_tests())
        .collect()
}

/// One problem per named test that `listed` lacks.
fn missing_tests(entries: &[(&Loaded, &Entry)], listed: &HashSet<String>) -> Vec<String> {
    entries
        .iter()
        .flat_map(|&(loaded, entry)| {
            entry
                .tests
                .iter()
                .filter(|test| !listed.contains(*test))
                .map(move |test| {
                    format!(
                        "{}: {}: test `{test}` is not listed by `cargo test`",
                        loaded.file, entry.id
                    )
                })
        })
        .collect()
}

/// `<package>::<test>` for every test of the unit and integration test binaries of `involved`.
/// Each binary runs with `--list`, so a test target with `harness = false` has to answer it the
/// way libtest does.
fn list_workspace_tests(
    root: &Path,
    packages: &[Package],
    involved: &BTreeSet<&str>,
) -> Result<HashSet<String>, Box<dyn Error>> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut build = Command::new(cargo);
    build
        .current_dir(root)
        .args(["test", "--no-run", "--locked", "--message-format=json"])
        .stderr(Stdio::inherit());
    for package in involved {
        build.arg("-p").arg(package);
    }
    let output = build.output()?;
    if !output.status.success() {
        return Err("cargo test --no-run failed".into());
    }
    let mut listed = HashSet::new();
    for (manifest, exe) in test_binaries(&String::from_utf8_lossy(&output.stdout))? {
        let Some(package) = packages.iter().find(|pkg| pkg.manifest_path == manifest) else {
            continue;
        };
        let list = Command::new(&exe).arg("--list").output()?;
        if !list.status.success() {
            return Err(format!("{} --list failed", exe.display()).into());
        }
        let output = String::from_utf8_lossy(&list.stdout);
        listed.extend(test_names(&output).map(|name| format!("{}::{name}", package.name)));
    }
    Ok(listed)
}

#[derive(Deserialize)]
struct BuildMessage {
    reason: String,
    manifest_path: Option<PathBuf>,
    executable: Option<PathBuf>,
    profile: Option<BuildProfile>,
}

#[derive(Deserialize)]
struct BuildProfile {
    test: bool,
}

/// Manifest path and executable of every test binary in cargo's JSON build messages.
fn test_binaries(json_lines: &str) -> Result<Vec<(PathBuf, PathBuf)>, serde_json::Error> {
    let mut found = Vec::new();
    for line in json_lines.lines() {
        let message: BuildMessage = serde_json::from_str(line)?;
        if message.reason != "compiler-artifact" {
            continue;
        }
        if let (Some(manifest), Some(exe), Some(BuildProfile { test: true })) =
            (message.manifest_path, message.executable, message.profile)
        {
            found.push((manifest, exe));
        }
    }
    Ok(found)
}

/// The tests of a libtest `--list` output (`name: test`).
fn test_names(list: &str) -> impl Iterator<Item = &str> {
    list.lines().filter_map(|line| line.strip_suffix(": test"))
}

struct Summary {
    files: usize,
    entries: usize,
    /// Counts per document in `DOCS` order, then per status in `STATUSES` order.
    by_doc: Vec<[usize; 5]>,
    by_tier: [usize; 5],
}

fn summarize(ledgers: &[Loaded]) -> Summary {
    let mut summary = Summary {
        files: ledgers.len(),
        entries: 0,
        by_doc: vec![[0; 5]; DOCS.len()],
        by_tier: [0; 5],
    };
    for entry in ledgers.iter().flat_map(|loaded| &loaded.ledger.entry) {
        summary.entries += 1;
        let doc = parse_spec(&entry.spec)
            .and_then(|(doc, _)| DOCS.iter().position(|&(name, _)| name == doc));
        let status = STATUSES
            .iter()
            .position(|&(status, _)| status == entry.status);
        if let (Some(doc), Some(status)) = (doc, status) {
            summary.by_doc[doc][status] += 1;
        }
        if let Some(tier) = TIERS.iter().position(|&tier| tier == entry.tier) {
            summary.by_tier[tier] += 1;
        }
    }
    summary
}

/// A table of the status counts per document. `done` is the share of `tested` and `verified-o1`.
fn render(summary: &Summary) -> String {
    let mut head = vec!["document".to_owned()];
    head.extend(STATUSES.iter().map(|&(_, name)| name.to_owned()));
    head.extend(["total".to_owned(), "done".to_owned()]);
    let mut rows = vec![head];
    for (counts, &(name, _)) in summary.by_doc.iter().zip(DOCS) {
        let total: usize = counts.iter().sum();
        let done: usize = STATUSES
            .iter()
            .zip(counts)
            .filter(|((status, _), _)| status.needs_tests())
            .map(|(_, count)| count)
            .sum();
        let share = (done * 100)
            .checked_div(total)
            .map_or_else(|| "-".to_owned(), |percent| format!("{percent}%"));
        let mut row = vec![name.to_owned()];
        row.extend(counts.iter().map(usize::to_string));
        row.extend([total.to_string(), share]);
        rows.push(row);
    }
    let widths: Vec<usize> = (0..rows[0].len())
        .map(|col| rows.iter().map(|row| row[col].len()).max().unwrap_or(0))
        .collect();
    let mut out = format!(
        "ledger: {} file(s), {} entries\n\n",
        summary.files, summary.entries
    );
    for row in &rows {
        let cells: Vec<String> = row
            .iter()
            .zip(&widths)
            .enumerate()
            .map(|(col, (cell, &width))| {
                if col == 0 {
                    format!("{cell:<width$}")
                } else {
                    format!("{cell:>width$}")
                }
            })
            .collect();
        let _ = writeln!(out, "{}", cells.join("  ").trim_end());
    }
    let tiers: Vec<String> = TIERS
        .iter()
        .zip(summary.by_tier)
        .map(|(tier, count)| format!("{tier:?} {count}"))
        .collect();
    let _ = writeln!(out, "\ntiers: {}", tiers.join(", "));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs() -> Docs {
        let sections = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect();
        HashMap::from([
            ("arch", sections(&["19", "19.1", "19.2"])),
            ("native", sections(&["4", "4.4.3"])),
            ("tools", sections(&["3.4"])),
        ])
    }

    fn packages() -> HashSet<&'static str> {
        HashSet::from(["cuboid-testkit", "cuboid-num"])
    }

    fn file(stem: &str, text: &str) -> Loaded {
        Loaded {
            file: format!("parity/ledger/{stem}.toml"),
            stem: stem.into(),
            ledger: toml::from_str(text).unwrap_or_else(|err| panic!("{err}")),
        }
    }

    const GOOD: &str = r#"
wp = "W0.5"

[[entry]]
id = "native.4.4.3.fold"
spec = "native 4.4.3"
tier = "P0"
status = "tested"
summary = "UV corners fold into [-1, 2]"
tests = ["cuboid-num::uv::tests::fold"]

[[entry]]
id = "tools.3.4.restricted-reject"
spec = "tools 3.4"
tier = "P1"
status = "unverified-spec"
summary = "Restricted rotation rejects off-grid angles"
note = "S4: rotation restriction of primitive meshes is pending"

[[entry]]
id = "arch.19.1.verified"
spec = "arch 19.1"
tier = "P4"
status = "verified-o1"
summary = "Checked against the original"
tests = ["cuboid-testkit::oracle::tests::gated"]
"#;

    fn problems_of(stem: &str, text: &str) -> Vec<String> {
        check(&[file(stem, text)], &docs(), &packages())
    }

    #[test]
    fn a_well_formed_file_has_no_problems() {
        assert_eq!(problems_of("w0.5", GOOD), Vec::<String>::new());
    }

    #[test]
    fn statuses_and_tiers_use_the_documented_spellings() {
        let loaded = file("w0.5", GOOD);
        let statuses: Vec<Status> = loaded.ledger.entry.iter().map(|e| e.status).collect();
        assert_eq!(
            statuses,
            [Status::Tested, Status::UnverifiedSpec, Status::VerifiedO1]
        );
        for (status, name) in STATUSES {
            let text = format!(
                "wp = \"W1.1\"\n[[entry]]\nid = \"a\"\nspec = \"a 1\"\ntier = \"P2\"\n\
                 status = \"{name}\"\nsummary = \"s\"\n"
            );
            let parsed: LedgerFile = toml::from_str(&text).unwrap();
            assert_eq!(parsed.entry[0].status, status);
        }
        let wrong = GOOD.replace("\"verified-o1\"", "\"done\"");
        let err = toml::from_str::<LedgerFile>(&wrong)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown variant `done`"), "{err}");
    }

    #[test]
    fn misspelt_keys_are_rejected() {
        let wrong = GOOD.replace("summary = \"Checked", "sumary = \"Checked");
        assert!(toml::from_str::<LedgerFile>(&wrong).is_err());
        assert!(toml::from_str::<LedgerFile>("wp = \"W1.1\"\nentries = []").is_err());
    }

    #[test]
    fn ids_start_with_their_spec_reference() {
        let wrong = GOOD.replace("native.4.4.3.fold", "native.4.4.fold");
        let found = problems_of("w0.5", &wrong);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains("id must be `native.4.4.3.<slug>`"),
            "{found:?}"
        );
        let bad_slug = GOOD.replace("native.4.4.3.fold", "native.4.4.3.Fold_It");
        assert!(problems_of("w0.5", &bad_slug)[0].contains("slug"));
    }

    #[test]
    fn ids_are_unique_over_all_files() {
        let other = GOOD.replace("W0.5", "W0.6");
        let found = check(
            &[file("w0.5", GOOD), file("w0.6", &other)],
            &docs(),
            &packages(),
        );
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(
            found
                .iter()
                .all(|p| p.contains("id is already used in parity/ledger/w0.5.toml"))
        );
    }

    #[test]
    fn spec_references_need_a_known_document_and_an_existing_section() {
        let missing_section = GOOD.replace("spec = \"native 4.4.3\"", "spec = \"native 4.9\"");
        let found = problems_of("w0.5", &missing_section).join("\n");
        assert!(
            found.contains("document `native` has no section 4.9"),
            "{found}"
        );

        let unknown_doc = GOOD.replace("spec = \"tools 3.4\"", "spec = \"handbook 3.4\"");
        let found = problems_of("w0.5", &unknown_doc).join("\n");
        assert!(
            found.contains("unknown document `handbook`") && found.contains("native"),
            "{found}"
        );

        let no_section = GOOD.replace("spec = \"arch 19.1\"", "spec = \"arch\"");
        assert!(
            problems_of("w0.5", &no_section)
                .join("\n")
                .contains("is not `<document> <section>`")
        );
    }

    #[test]
    fn tested_entries_need_tests_of_workspace_packages() {
        let none = GOOD.replace("tests = [\"cuboid-num::uv::tests::fold\"]", "");
        assert!(
            problems_of("w0.5", &none)
                .join("\n")
                .contains("status needs at least one test")
        );

        let unknown = GOOD.replace("cuboid-num::uv", "cuboid-nope::uv");
        assert!(
            problems_of("w0.5", &unknown)
                .join("\n")
                .contains("`cuboid-nope` is not a workspace package")
        );

        let shape = GOOD.replace("cuboid-num::uv::tests::fold", "fold");
        assert!(
            problems_of("w0.5", &shape)
                .join("\n")
                .contains("is not `<package>::<test path>`")
        );
    }

    #[test]
    fn unverified_spec_names_the_gap() {
        let wrong = GOOD.replace(
            "note = \"S4: rotation restriction of primitive meshes is pending\"",
            "note = \"  \"",
        );
        assert!(
            problems_of("w0.5", &wrong)
                .join("\n")
                .contains("`unverified-spec` needs a note")
        );
    }

    #[test]
    fn the_work_package_matches_the_file_name() {
        assert!(
            problems_of("w0.6", GOOD)
                .join("\n")
                .contains("does not match the file name; expected w0.5.toml")
        );
        let wrong = GOOD.replace("W0.5", "WP5");
        assert!(
            problems_of("wp5", &wrong)
                .join("\n")
                .contains("not a work package id")
        );
    }

    #[test]
    fn numbered_headings_are_the_sections_outside_code_blocks() {
        let markdown = "\
# Title
## Contents
## 0. Ground rules
### 4.4.3 FaceRecord
#### 21.3 Milestones
```text
## 9.9 not a heading
```
#hashtag 7.7
##10.1 no space
### 5 Five
";
        let mut found: Vec<String> = section_numbers(markdown).into_iter().collect();
        found.sort();
        assert_eq!(found, ["0", "21.3", "4.4.3", "5"]);
    }

    #[test]
    fn summary_counts_per_document_and_status() {
        let summary = summarize(&[file("w0.5", GOOD)]);
        assert_eq!((summary.files, summary.entries), (1, 3));
        assert_eq!(summary.by_doc[0], [0, 0, 0, 1, 0], "arch: one verified-o1");
        assert_eq!(summary.by_doc[1], [0, 0, 1, 0, 0], "native: one tested");
        assert_eq!(
            summary.by_doc[3],
            [0, 0, 0, 0, 1],
            "tools: one unverified-spec"
        );
        assert_eq!(summary.by_tier, [1, 1, 0, 0, 1]);

        let table = render(&summary);
        let expected = "\
ledger: 1 file(s), 3 entries

document  todo  implemented  tested  verified-o1  unverified-spec  total  done
arch         0            0       0            1                0      1  100%
native       0            0       1            0                0      1  100%
interop      0            0       0            0                0      0     -
tools        0            0       0            0                1      1    0%
ui           0            0       0            0                0      0     -
menus        0            0       0            0                0      0     -
app          0            0       0            0                0      0     -

tiers: P0 1, P1 1, P2 0, P3 0, P4 1
";
        assert_eq!(table, expected);
    }

    #[test]
    fn only_tested_entries_of_the_selected_work_packages_are_verified() {
        let other = GOOD
            .replace("W0.5", "W0.6")
            .replace("native.4.4.3.fold", "native.4.4.3.other")
            .replace("tools.3.4.restricted-reject", "tools.3.4.other")
            .replace("arch.19.1.verified", "arch.19.1.other");
        let ledgers = [file("w0.5", GOOD), file("w0.6", &other)];

        let all = Options::default();
        let ids = |options: &Options| -> Vec<&str> {
            entries_to_verify(&ledgers, options)
                .iter()
                .map(|(_, entry)| entry.id.as_str())
                .collect()
        };
        // the unverified-spec entry never names tests that must exist
        assert_eq!(
            ids(&all),
            [
                "native.4.4.3.fold",
                "arch.19.1.verified",
                "native.4.4.3.other",
                "arch.19.1.other"
            ]
        );
        let only = Options {
            wps: vec!["W0.6".into()],
            ..Options::default()
        };
        assert_eq!(ids(&only), ["native.4.4.3.other", "arch.19.1.other"]);
    }

    #[test]
    fn every_test_the_build_does_not_list_is_a_problem_with_file_and_id() {
        let ledgers = [file("w0.5", GOOD)];
        let entries = entries_to_verify(&ledgers, &Options::default());
        let listed: HashSet<String> = ["cuboid-num::uv::tests::fold".to_owned()].into();
        assert_eq!(
            missing_tests(&entries, &listed),
            ["parity/ledger/w0.5.toml: arch.19.1.verified: test \
                 `cuboid-testkit::oracle::tests::gated` is not listed by `cargo test`"]
        );
        let everything: HashSet<String> = entries
            .iter()
            .flat_map(|(_, entry)| entry.tests.clone())
            .collect();
        assert!(missing_tests(&entries, &everything).is_empty());
    }

    #[test]
    fn test_binaries_come_from_test_profile_artifacts_only() {
        let artifact = |manifest: &str, exe: &str, test: bool| {
            format!(
                concat!(
                    r#"{{"reason":"compiler-artifact","manifest_path":"{}","#,
                    r#""executable":{},"profile":{{"test":{}}}}}"#
                ),
                manifest, exe, test
            )
        };
        let json = [
            artifact("/w/a/Cargo.toml", r#""/w/t/a-1""#, true),
            artifact("/w/a/Cargo.toml", r#""/w/t/a-example""#, false),
            artifact("/w/b/Cargo.toml", "null", true),
            r#"{"reason":"compiler-message","message":{}}"#.to_owned(),
            r#"{"reason":"build-finished","success":true}"#.to_owned(),
        ]
        .join("\n");
        let found = test_binaries(&json).unwrap();
        assert_eq!(
            found,
            [(PathBuf::from("/w/a/Cargo.toml"), PathBuf::from("/w/t/a-1"))]
        );
        assert!(test_binaries("not json").is_err());
    }

    #[test]
    fn test_lists_keep_tests_and_drop_benchmarks_and_totals() {
        let list = "diff::tests::bytes: test\nbench::speed: benchmark\n\
                    raster::tests::ssim: test\n\n2 tests, 1 benchmarks\n";
        assert_eq!(
            test_names(list).collect::<Vec<_>>(),
            ["diff::tests::bytes", "raster::tests::ssim"]
        );
    }

    #[test]
    fn arguments_select_the_test_check_and_the_work_packages() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let options = parse_args(&args(&["--no-tests", "--wp", "w0.5", "--wp", "W1.1"])).unwrap();
        assert!(options.skip_tests);
        assert_eq!(options.wps, ["W0.5", "W1.1"]);
        assert!(parse_args(&args(&["--wp"])).is_err());
        assert!(parse_args(&args(&["--fast"])).is_err());
        let ledgers = [file("w0.5", GOOD)];
        assert_eq!(
            unknown_wps(&options, &ledgers),
            ["--wp W1.1: no ledger file for this work package"]
        );
    }
}
