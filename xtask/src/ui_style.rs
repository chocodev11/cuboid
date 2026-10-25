//! `cargo xtask ui-style`: the UI v2 style rules (ui_v2_spec section 12).
//!
//! Screens in `crates/cuboid-ui/src` build from ui-kit components and set no font, colour, margin,
//! radius or spacing literal of their own. The lint reports
//!
//! - `font`: `FontId::`
//! - `text-size`: `.size(<value>)`, as on `RichText`
//! - `colour`: colour constructors (`Color32::from_*`, `hex_color!`)
//! - `colour-const`: `Color32` constants such as `Color32::WHITE`
//! - `space`: `add_space(<number>)`
//! - `margin`: `Margin::` and `inner_margin(<number>)`, `outer_margin(<number>)`
//! - `radius`: `CornerRadius::` and `corner_radius(<number>)`
//!
//! Comments and string contents are ignored, and so is everything after a `#[cfg(test)]` line. A
//! line can opt out with `// ui-style: allow` at its end or on the line before, for the few places
//! that paint by hand (the view gizmo, the app mark).
//!
//! Every file under the roots is checked and any finding is an error: the command exits 1.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Source folders under the lint, relative to the workspace root.
const ROOTS: &[&str] = &["crates/cuboid-ui/src"];

const ALLOW_MARKER: &str = "ui-style: allow";

/// One rule hit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    /// The rule name, such as `font`.
    pub rule: &'static str,
    /// 1-based line.
    pub line: usize,
    /// The trimmed source line.
    pub text: String,
}

pub fn run(args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    if let Some(other) = args.first() {
        return Err(format!("unknown argument `{other}`").into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut errors = 0;
    for source_root in ROOTS {
        let mut files = Vec::new();
        collect(&root.join(source_root), &mut files)?;
        files.sort();
        for file in files {
            let source = std::fs::read_to_string(&file)?;
            let shown = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .display()
                .to_string()
                .replace('\\', "/");
            for finding in scan(&source) {
                errors += 1;
                eprintln!(
                    "error: {shown}:{}: {}: {}",
                    finding.line, finding.rule, finding.text
                );
            }
        }
    }
    if errors == 0 {
        println!("ui-style: ok");
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("ui-style: {errors} error(s)");
    Ok(ExitCode::FAILURE)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// The line without comments, with the inside of string and character literals blanked.
fn code_only(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            match c {
                '\\' => {
                    chars.next();
                    out.push_str("  ");
                }
                '"' => {
                    in_string = false;
                    out.push('"');
                }
                _ => out.push(' '),
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push('"');
            }
            '/' if chars.peek() == Some(&'/') => break,
            _ => out.push(c),
        }
    }
    out
}

/// Positions just after each occurrence of `needle`.
fn after_each<'a>(haystack: &'a str, needle: &'a str) -> impl Iterator<Item = &'a str> {
    haystack
        .match_indices(needle)
        .map(move |(at, _)| &haystack[at + needle.len()..])
}

fn starts_numeric(rest: &str) -> bool {
    rest.trim_start()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || c == '-')
}

/// The rules a code line breaks.
fn rules_of(code: &str) -> Vec<&'static str> {
    let mut hits = Vec::new();
    if code.contains("FontId::") {
        hits.push("font");
    }
    if after_each(code, ".size(").any(|rest| !rest.trim_start().starts_with(')')) {
        hits.push("text-size");
    }
    if code.contains("Color32::from_")
        || code.contains("Rgba::from_")
        || code.contains("hex_color!")
    {
        hits.push("colour");
    }
    if after_each(code, "Color32::").any(|rest| rest.starts_with(|c: char| c.is_ascii_uppercase()))
    {
        hits.push("colour-const");
    }
    if after_each(code, "add_space(").any(starts_numeric) {
        hits.push("space");
    }
    if code.contains("Margin::")
        || after_each(code, "inner_margin(").any(starts_numeric)
        || after_each(code, "outer_margin(").any(starts_numeric)
    {
        hits.push("margin");
    }
    if code.contains("CornerRadius::")
        || after_each(code, "corner_radius(").any(starts_numeric)
        || after_each(code, "rounding(").any(starts_numeric)
    {
        hits.push("radius");
    }
    hits
}

/// All findings of one source file.
pub fn scan(source: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut allow_next = false;
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == "#[cfg(test)]" {
            break;
        }
        let allowed = allow_next || line.contains(ALLOW_MARKER);
        allow_next = trimmed.starts_with("//") && trimmed.contains(ALLOW_MARKER);
        if allowed {
            continue;
        }
        for rule in rules_of(&code_only(line)) {
            findings.push(Finding {
                rule,
                line: index + 1,
                text: trimmed.to_owned(),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAN: &str = include_str!("../fixtures/ui_style/clean.rs.txt");
    const DIRTY: &str = include_str!("../fixtures/ui_style/dirty.rs.txt");

    fn rules(source: &str) -> Vec<&'static str> {
        scan(source).into_iter().map(|f| f.rule).collect()
    }

    #[test]
    fn a_screen_built_from_the_kit_is_clean() {
        assert_eq!(scan(CLEAN), Vec::new());
    }

    #[test]
    fn every_rule_fires_on_the_dirty_fixture() {
        let found = rules(DIRTY);
        for rule in [
            "font",
            "text-size",
            "colour",
            "colour-const",
            "space",
            "margin",
            "radius",
        ] {
            assert!(found.contains(&rule), "{rule} did not fire: {found:?}");
        }
    }

    #[test]
    fn findings_name_the_line() {
        let findings = scan("fn a() {}\nlet f = FontId::proportional(12.0);\n");
        assert_eq!(findings.len(), 1);
        assert_eq!((findings[0].rule, findings[0].line), ("font", 2));
        assert_eq!(findings[0].text, "let f = FontId::proportional(12.0);");
    }

    #[test]
    fn comments_strings_and_the_test_module_are_skipped() {
        let source = "// FontId::proportional(1.0)\n\
                      let s = \"Margin::same(4) and Color32::WHITE\";\n\
                      let t = \"escaped \\\" FontId::x\";\n\
                      #[cfg(test)]\n\
                      mod tests { let f = FontId::monospace(1.0); }\n";
        assert_eq!(scan(source), Vec::new());
    }

    #[test]
    fn the_allow_marker_covers_its_line_and_the_next() {
        let same = "let c = Color32::WHITE; // ui-style: allow\n";
        assert_eq!(scan(same), Vec::new());
        let before = "// ui-style: allow the gizmo is painted by hand\nlet c = Color32::from_rgb(1, 2, 3);\nlet d = Color32::BLACK;\n";
        let found = rules(before);
        assert_eq!(found, vec!["colour-const"]);
    }

    #[test]
    fn only_literals_count_for_spaces_margins_and_sizes() {
        let source = "ui.add_space(space::MD);\n\
                      ui.add_space(8.0);\n\
                      let r = rect.size();\n\
                      let w = text.size(11.0);\n\
                      frame.inner_margin(margin);\n\
                      frame.inner_margin(8);\n\
                      frame.corner_radius(radius::CARD);\n\
                      frame.corner_radius(4);\n\
                      let c = Color32::from_rgb(1, 2, 3);\n\
                      let d = Color32::TRANSPARENT;\n";
        let lines: Vec<(usize, &str)> =
            scan(source).into_iter().map(|f| (f.line, f.rule)).collect();
        assert_eq!(
            lines,
            vec![
                (2, "space"),
                (4, "text-size"),
                (6, "margin"),
                (8, "radius"),
                (9, "colour"),
                (10, "colour-const"),
            ]
        );
    }

    #[test]
    fn every_screen_file_is_clean() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for source_root in ROOTS {
            let mut files = Vec::new();
            collect(&root.join(source_root), &mut files).unwrap_or_else(|err| panic!("{err}"));
            assert!(!files.is_empty(), "{source_root} has no sources");
            for file in files {
                let source = std::fs::read_to_string(&file)
                    .unwrap_or_else(|err| panic!("{}: {err}", file.display()));
                assert_eq!(scan(&source), Vec::new(), "{}", file.display());
            }
        }
    }
}
