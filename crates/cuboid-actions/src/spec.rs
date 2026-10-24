//! Test support: reads the tables of `docs/spec/ui_dialogs_menus.md`, so that the menu and
//! binding tests compare the code with the specification text instead of a second copy.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

static MENUS_SPEC: LazyLock<String> = LazyLock::new(|| {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/ui_dialogs_menus.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
});

/// A table row by column header.
pub type Row = BTreeMap<String, String>;

/// The part of the specification from the heading `### {number} ...` up to the next heading.
pub fn section(number: &str) -> &'static str {
    let text = MENUS_SPEC.as_str();
    let heading = format!("\n### {number} ");
    let start = text
        .find(&heading)
        .unwrap_or_else(|| panic!("no section {number}"))
        + 1;
    let end = text[start..]
        .find("\n#")
        .map_or(text.len(), |len| start + len + 1);
    &text[start..end]
}

/// The first table of a section: one map per body row, keyed by the header cells.
pub fn table(section: &str) -> Vec<Row> {
    // A table inside a list item is indented.
    let mut lines = section
        .lines()
        .map(str::trim_start)
        .skip_while(|line| !line.starts_with('|'));
    let header = cells(lines.next().expect("a table"));
    lines.next(); // the |---| line
    lines
        .take_while(|line| line.starts_with('|'))
        .map(|line| header.iter().cloned().zip(cells(line)).collect())
        .collect()
}

fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_owned())
        .collect()
}

/// The text after the first `after` on the first line of `section` that contains `marker`.
pub fn line_after(section: &'static str, marker: &str, after: &str) -> &'static str {
    let line = section
        .lines()
        .find(|line| line.contains(marker))
        .unwrap_or_else(|| panic!("no line with {marker:?}"));
    line.split_once(after)
        .unwrap_or_else(|| panic!("no {after:?} on the line with {marker:?}"))
        .1
}

/// Splits a list at the commas outside parentheses and double quotes.
pub fn items(list: &str) -> Vec<&str> {
    let (mut depth, mut quoted, mut from) = (0_u32, false, 0);
    let mut out = Vec::new();
    for (i, c) in list.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            ',' if !quoted && depth == 0 => {
                out.push(list[from..i].trim());
                from = i + 1;
            }
            _ => {}
        }
    }
    out.push(list[from..].trim());
    out
}
