//! Oracle gating (architecture 19.1). Tests that need data of the original program or private
//! research data return early where it is absent and print "SKIPPED (oracle absent)" on stderr.
//!
//! ```
//! use cuboid_testkit::oracle::{self, Oracle};
//!
//! fn layout_matches_the_private_dump() {
//!     let Some(_dump) = oracle::require(Oracle::UiDump) else { return };
//!     // compare rectangles with the dump
//! }
//! layout_matches_the_private_dump(); // prints the notice where the variable is unset
//! ```
//!
//! libtest cannot mark a running test as skipped: a test that returns early counts as passed. The
//! notice goes to the real stderr, so `cargo test` shows it for passing tests as well, but
//! `cargo nextest` hides the output of passing tests unless it runs with
//! `--success-output immediate`. A test that has to show up as skipped in the report adds
//! `#[ignore = "oracle O1: CUBOID_PARITY_REFS"]` to its `#[test]`; maintainers who have the data
//! run it with `--include-ignored` (nextest: `--run-ignored all`).

use std::ffi::OsString;
use std::panic::Location;
use std::path::PathBuf;

use serde::Deserialize;

/// A source of reference data that is not part of the repository.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Oracle {
    /// O2: the private UI hierarchy dump.
    UiDump,
    /// O2: the private sprites.
    Sprites,
    /// O1: files, exports and screenshots captured from the original program. Architecture 19.1
    /// lists this variable together with the two O2 ones; the class is this crate's reading.
    ParityRefs,
}

impl Oracle {
    /// The environment variable that points at the data.
    pub const fn var(self) -> &'static str {
        match self {
            Self::UiDump => "CUBOID_PRIVATE_UI_DUMP",
            Self::Sprites => "CUBOID_PRIVATE_SPRITES",
            Self::ParityRefs => "CUBOID_PARITY_REFS",
        }
    }

    /// The oracle class of architecture 19.1, `O1` or `O2`.
    pub const fn class(self) -> &'static str {
        match self {
            Self::UiDump | Self::Sprites => "O2",
            Self::ParityRefs => "O1",
        }
    }

    /// The oracle's data; `None` when the variable is unset or points nowhere.
    pub fn path(self) -> Option<PathBuf> {
        locate(std::env::var_os(self.var()), None)
    }
}

/// [`Oracle::path`], reporting the skip when the data is absent.
#[track_caller]
pub fn require(oracle: Oracle) -> Option<PathBuf> {
    require_in(oracle, None)
}

/// The file `rel` inside the oracle's data, reporting the skip when it is absent.
#[track_caller]
pub fn require_file(oracle: Oracle, rel: &str) -> Option<PathBuf> {
    require_in(oracle, Some(rel))
}

#[track_caller]
fn require_in(oracle: Oracle, rel: Option<&str>) -> Option<PathBuf> {
    let found = locate(std::env::var_os(oracle.var()), rel);
    if found.is_none() {
        crate::notice(&skip_notice(oracle, rel, Location::caller()));
    }
    found
}

/// `value` is the content of the oracle's variable.
fn locate(value: Option<OsString>, rel: Option<&str>) -> Option<PathBuf> {
    let base = PathBuf::from(value.filter(|value| !value.is_empty())?);
    let path = match rel {
        Some(rel) => base.join(rel),
        None => base,
    };
    path.exists().then_some(path)
}

fn skip_notice(oracle: Oracle, rel: Option<&str>, at: &Location<'_>) -> String {
    let what = rel.map_or_else(String::new, |rel| format!(" with {rel}"));
    format!(
        "SKIPPED (oracle absent): {} needs {}{what} [{at}]",
        oracle.class(),
        oracle.var()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_needs_a_set_variable_pointing_at_an_existing_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("s01.cubik"), b"x").unwrap();
        let base = Some(dir.path().as_os_str().to_owned());
        assert_eq!(locate(base.clone(), None), Some(dir.path().to_path_buf()));
        assert_eq!(
            locate(base.clone(), Some("s01.cubik")),
            Some(dir.path().join("s01.cubik"))
        );
        assert_eq!(locate(base, Some("s02.cubik")), None);
        assert_eq!(locate(None, None), None);
        assert_eq!(locate(Some(OsString::new()), None), None);
        assert_eq!(locate(Some("/no/such/folder/anywhere".into()), None), None);
    }

    #[test]
    fn the_notice_names_the_class_the_variable_and_the_call_site() {
        let notice = skip_notice(Oracle::ParityRefs, Some("s01.cubik"), Location::caller());
        assert!(
            notice.starts_with(
                "SKIPPED (oracle absent): O1 needs CUBOID_PARITY_REFS with s01.cubik ["
            ),
            "{notice}"
        );
        assert!(notice.contains("oracle.rs"), "{notice}");
        let plain = skip_notice(Oracle::UiDump, None, Location::caller());
        assert!(
            plain.contains("O2 needs CUBOID_PRIVATE_UI_DUMP ["),
            "{plain}"
        );
    }

    #[test]
    fn an_absent_sample_yields_none_so_the_test_returns_early() {
        // Nobody has this file, whether or not the oracle itself is configured.
        assert_eq!(
            require_file(Oracle::ParityRefs, "no-such-sample.cubik"),
            None
        );
    }

    #[test]
    fn each_oracle_has_its_variable_and_class() {
        let table = [
            (Oracle::UiDump, "CUBOID_PRIVATE_UI_DUMP", "O2"),
            (Oracle::Sprites, "CUBOID_PRIVATE_SPRITES", "O2"),
            (Oracle::ParityRefs, "CUBOID_PARITY_REFS", "O1"),
        ];
        for (oracle, var, class) in table {
            assert_eq!((oracle.var(), oracle.class()), (var, class));
        }
    }
}
