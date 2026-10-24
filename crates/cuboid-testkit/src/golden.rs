//! Golden files of `tests/golden`: load, compare by tier, bless (architecture 19.3).
//!
//! A test names a file relative to the corpus and compares its result with it:
//!
//! ```no_run
//! use cuboid_testkit::golden::Golden;
//!
//! Golden::new("cubik/fresh/payload.dump.txt").expect_text("mp: doc len=0\n").unwrap();
//! ```
//!
//! Files the port emits, text or binary, are P0 byte goldens: check them with `expect_bytes`,
//! which assumes nothing about encoding, line endings (`cubik29` writes CRLF, interop 3.3) or a
//! final newline. `expect_text` is the same exact comparison for UTF-8 text, with a report by
//! line; blessing stores the text as it is. Only the port's own canonical dumps and summaries are
//! stored with LF line endings and a final newline, because the port defines them.
//!
//! A golden that is missing or differs fails the test, and the actual output is saved in the
//! `golden-actual` folder of Cargo's target folder for inspection. With `CUBOID_BLESS=1`
//! (`cargo xtask bless`) the actual output is written instead. Goldens that pass their tier check
//! are never rewritten, so blessing does not churn tolerance-based files. Blessing is refused when
//! `CI` is set.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::diff::{self, Mismatch, Tolerance};
use crate::raster::{self, Raster, RasterError, RasterTolerance};

/// The corpus folder `tests/golden`.
pub fn root() -> PathBuf {
    crate::workspace_root().join("tests").join("golden")
}

/// What a golden that is missing or differs leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The test fails.
    Check,
    /// `CUBOID_BLESS=1`: the golden is written.
    Bless,
    /// `CUBOID_BLESS=1` together with `CI`: the test fails and says why.
    BlessRefused,
}

impl Mode {
    /// `CUBOID_BLESS=1` selects blessing unless `CI` is set.
    pub fn from_env() -> Self {
        let bless = std::env::var("CUBOID_BLESS").is_ok_and(|value| value == "1");
        match (bless, std::env::var_os("CI").is_some()) {
            (false, _) => Self::Check,
            (true, false) => Self::Bless,
            (true, true) => Self::BlessRefused,
        }
    }
}

#[derive(thiserror::Error)]
pub enum GoldenError {
    #[error(
        "golden file {} is missing; create it with CUBOID_BLESS=1 (cargo xtask bless){saved}",
        .path.display()
    )]
    Missing { path: PathBuf, saved: String },
    #[error("golden file {} differs: {mismatch}{saved}", .path.display())]
    Differs {
        path: PathBuf,
        mismatch: Mismatch,
        saved: String,
    },
    #[error(
        "refusing to bless {} in CI (CUBOID_BLESS is set while CI is): {why}",
        .path.display()
    )]
    BlessRefused { path: PathBuf, why: String },
    #[error(
        "golden file {} is unusable: {reason}; replace it with CUBOID_BLESS=1 \
         (cargo xtask bless){saved}",
        .path.display()
    )]
    Corrupt {
        path: PathBuf,
        reason: String,
        saved: String,
    },
    #[error("golden file {}: {source}", .path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("golden file {}: {source}", .path.display())]
    Raster { path: PathBuf, source: RasterError },
}

debug_is_display!(GoldenError);

/// Why a golden does not pass its tier check.
enum Failure {
    Missing,
    /// The file exists but cannot be read the way the tier needs, for example a broken PNG.
    Unusable(String),
    Differs(Mismatch),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("the golden is missing"),
            Self::Unusable(reason) => write!(f, "the golden is unusable: {reason}"),
            Self::Differs(mismatch) => write!(f, "the golden differs: {mismatch}"),
        }
    }
}

/// One file of the corpus.
pub struct Golden {
    name: String,
    path: PathBuf,
    mode: Mode,
}

impl Golden {
    /// `name` is relative to `tests/golden` and uses `/` as separator. Whether failing goldens
    /// are blessed follows `CUBOID_BLESS` and `CI`.
    pub fn new(name: &str) -> Self {
        Self::in_dir(&root(), name, Mode::from_env())
    }

    /// A golden in another folder, for corpora outside `tests/golden`.
    pub fn in_dir(dir: &Path, name: &str, mode: Mode) -> Self {
        Self {
            name: name.into(),
            path: dir.join(name.split('/').collect::<PathBuf>()),
            mode,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The golden as input of a test.
    pub fn load(&self) -> Result<Vec<u8>, GoldenError> {
        self.read()?.ok_or_else(|| GoldenError::Missing {
            path: self.path.clone(),
            saved: String::new(),
        })
    }

    pub fn load_text(&self) -> Result<String, GoldenError> {
        parse_text(self.load()?).map_err(|reason| GoldenError::Corrupt {
            path: self.path.clone(),
            reason,
            saved: String::new(),
        })
    }

    /// P0: identical bytes.
    pub fn expect_bytes(&self, actual: &[u8]) -> Result<(), GoldenError> {
        let failure = self.judge(Ok, |golden| diff::bytes(golden, actual))?;
        self.settle(failure, || Ok(actual.to_vec()))
    }

    /// P0: identical UTF-8 text, line endings and final newline included (see [`diff::text`]).
    /// Blessing stores `actual` as it is.
    pub fn expect_text(&self, actual: &str) -> Result<(), GoldenError> {
        let failure = self.judge(parse_text, |golden| diff::text(golden, actual))?;
        self.settle(failure, || Ok(actual.as_bytes().to_vec()))
    }

    /// P1: numbers within `tol`. The file holds whitespace separated numbers, written one per
    /// line.
    pub fn expect_numbers(&self, actual: &[f64], tol: Tolerance) -> Result<(), GoldenError> {
        let failure = self.judge(parse_numbers, |golden| diff::numbers(golden, actual, tol))?;
        self.settle(failure, || {
            Ok(actual
                .iter()
                .map(|number| format!("{number}\n"))
                .collect::<String>()
                .into_bytes())
        })
    }

    /// P2: the golden is a PNG, compared with [`raster::within`].
    pub fn expect_raster(&self, actual: &Raster, tol: &RasterTolerance) -> Result<(), GoldenError> {
        let failure = self.judge(parse_png, |golden| raster::within(golden, actual, tol))?;
        self.settle(failure, || self.encode(actual))
    }

    /// P3: the golden is a PNG, compared with [`raster::ssim`]; `min` is calibrated per scene
    /// class.
    pub fn expect_ssim(&self, actual: &Raster, min: f64) -> Result<(), GoldenError> {
        let failure = self.judge(parse_png, |golden| raster::ssim(golden, actual, min))?;
        self.settle(failure, || self.encode(actual))
    }

    /// Reads the golden, turns it into what the tier compares with `parse` and runs `check`.
    /// `None` means the golden passes.
    fn judge<T>(
        &self,
        parse: impl FnOnce(Vec<u8>) -> Result<T, String>,
        check: impl FnOnce(&T) -> Result<(), Mismatch>,
    ) -> Result<Option<Failure>, GoldenError> {
        let Some(bytes) = self.read()? else {
            return Ok(Some(Failure::Missing));
        };
        Ok(match parse(bytes) {
            Ok(golden) => check(&golden).err().map(Failure::Differs),
            Err(reason) => Some(Failure::Unusable(reason)),
        })
    }

    /// `None` when the file does not exist.
    fn read(&self) -> Result<Option<Vec<u8>>, GoldenError> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(self.io(source)),
        }
    }

    fn encode(&self, picture: &Raster) -> Result<Vec<u8>, GoldenError> {
        picture.to_png().map_err(|source| GoldenError::Raster {
            path: self.path.clone(),
            source,
        })
    }

    /// Passes, blesses or fails according to the mode. `actual` produces the bytes to store and
    /// runs only when they are needed.
    fn settle(
        &self,
        failure: Option<Failure>,
        actual: impl FnOnce() -> Result<Vec<u8>, GoldenError>,
    ) -> Result<(), GoldenError> {
        let Some(failure) = failure else {
            return Ok(());
        };
        match self.mode {
            Mode::Bless => self.write(&actual()?),
            Mode::BlessRefused => Err(GoldenError::BlessRefused {
                path: self.path.clone(),
                why: failure.to_string(),
            }),
            Mode::Check => {
                let saved = self.save_actual(&actual()?);
                let path = self.path.clone();
                Err(match failure {
                    Failure::Missing => GoldenError::Missing { path, saved },
                    Failure::Unusable(reason) => GoldenError::Corrupt {
                        path,
                        reason,
                        saved,
                    },
                    Failure::Differs(mismatch) => GoldenError::Differs {
                        path,
                        mismatch,
                        saved,
                    },
                })
            }
        }
    }

    fn write(&self, bytes: &[u8]) -> Result<(), GoldenError> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|source| self.io(source))?;
        }
        fs::write(&self.path, bytes).map_err(|source| self.io(source))?;
        crate::notice(&format!(
            "blessed {} ({} bytes)",
            self.path.display(),
            bytes.len()
        ));
        Ok(())
    }

    /// Keeps the actual output next to the other build output; returns a sentence for the error.
    fn save_actual(&self, bytes: &[u8]) -> String {
        let path = target_dir()
            .join("golden-actual")
            .join(self.name.split('/').collect::<PathBuf>());
        let saved = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&path, bytes));
        match saved {
            Ok(()) => format!("\nactual output saved in {}", path.display()),
            Err(err) => format!(
                "\nactual output could not be saved in {}: {err}",
                path.display()
            ),
        }
    }

    fn io(&self, source: io::Error) -> GoldenError {
        GoldenError::Io {
            path: self.path.clone(),
            source,
        }
    }
}

/// Cargo's target folder: `CARGO_TARGET_DIR` (relative to the workspace root) or `target`.
fn target_dir() -> PathBuf {
    let root = crate::workspace_root();
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), |dir| root.join(dir))
}

fn parse_text(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| "not valid UTF-8".into())
}

fn parse_numbers(bytes: Vec<u8>) -> Result<Vec<f64>, String> {
    let text = parse_text(bytes)?;
    text.split_whitespace()
        .map(|token| {
            token
                .parse()
                .map_err(|_| format!("`{token}` is not a number"))
        })
        .collect()
}

fn parse_png(bytes: Vec<u8>) -> Result<Raster, String> {
    Raster::from_png(&bytes).map_err(|err| err.to_string())
}

/// The files of `dir` with the extension `ext` (all files when `None`), sorted by path. For
/// replays of a whole corpus and for scenario runs.
pub fn files(dir: &Path, ext: Option<&str>) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let wanted = ext.is_none_or(|ext| path.extension().is_some_and(|have| have == ext));
        if wanted && path.is_file() {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Names under `testkit-selftest` keep the saved actual outputs of these tests apart.
    fn golden(dir: &TempDir, name: &str, mode: Mode) -> Golden {
        Golden::in_dir(dir.path(), &format!("testkit-selftest/{name}"), mode)
    }

    fn seed(dir: &TempDir, name: &str, bytes: &[u8]) -> Golden {
        let golden = golden(dir, name, Mode::Check);
        fs::create_dir_all(golden.path().parent().unwrap()).unwrap();
        fs::write(golden.path(), bytes).unwrap();
        golden
    }

    #[test]
    fn a_missing_golden_fails_with_a_hint_and_keeps_the_actual_output() {
        let dir = TempDir::new().unwrap();
        let golden = golden(&dir, "missing.bin", Mode::Check);
        let err = golden.expect_bytes(b"new").unwrap_err();
        assert!(matches!(err, GoldenError::Missing { .. }));
        let msg = err.to_string();
        assert!(msg.contains("CUBOID_BLESS=1"), "{msg}");
        let (_, saved) = msg
            .split_once("actual output saved in ")
            .expect("the message names the saved file");
        assert_eq!(fs::read(saved).unwrap(), b"new");
        assert!(!golden.path().exists());
    }

    #[test]
    fn a_differing_golden_reports_the_first_difference() {
        let dir = TempDir::new().unwrap();
        let golden = seed(&dir, "differs.bin", &[1, 2, 3]);
        let msg = golden.expect_bytes(&[1, 2, 4]).unwrap_err().to_string();
        assert!(msg.contains("differs") && msg.contains("offset 2"), "{msg}");
        assert_eq!(
            fs::read(golden.path()).unwrap(),
            [1, 2, 3],
            "check mode never writes"
        );
    }

    #[test]
    fn bless_creates_missing_goldens_and_replaces_differing_ones() {
        let dir = TempDir::new().unwrap();
        let golden = golden(&dir, "nested/folder/new.bin", Mode::Bless);
        golden.expect_bytes(b"first").unwrap();
        assert_eq!(fs::read(golden.path()).unwrap(), b"first");
        golden.expect_bytes(b"second").unwrap();
        assert_eq!(fs::read(golden.path()).unwrap(), b"second");
    }

    #[test]
    fn bless_is_refused_in_ci_writes_nothing_and_keeps_the_reason() {
        let dir = TempDir::new().unwrap();
        let refused = golden(&dir, "refused.bin", Mode::BlessRefused);
        let err = refused.expect_bytes(b"x").unwrap_err();
        assert!(matches!(err, GoldenError::BlessRefused { .. }), "{err}");
        assert!(err.to_string().contains("the golden is missing"), "{err}");
        assert!(!refused.path().exists());

        let seeded = seed(&dir, "differs.bin", &[1, 2, 3]);
        let err = golden(&dir, "differs.bin", Mode::BlessRefused)
            .expect_bytes(&[1, 2, 4])
            .unwrap_err();
        assert!(
            err.to_string().contains("the golden differs") && err.to_string().contains("offset 2"),
            "{err}"
        );
        assert_eq!(fs::read(seeded.path()).unwrap(), [1, 2, 3]);
    }

    #[test]
    fn text_goldens_keep_their_bytes_and_compare_them_exactly() {
        let dir = TempDir::new().unwrap();
        // LF and CRLF mixed, no final newline: how some `cubik29` exports end (interop 3.3)
        let emitted = "# header\n{\r\n  \"a\": 1\r\n}";
        let blessing = golden(&dir, "text.json", Mode::Bless);
        blessing.expect_text(emitted).unwrap();
        assert_eq!(fs::read(blessing.path()).unwrap(), emitted.as_bytes());

        let check = golden(&dir, "text.json", Mode::Check);
        check.expect_text(emitted).unwrap();
        assert_eq!(check.load_text().unwrap(), emitted);
        for other in [
            "# header\r\n{\r\n  \"a\": 1\r\n}",
            "# header\n{\n  \"a\": 1\n}",
            "# header\n{\r\n  \"a\": 1\r\n}\n",
        ] {
            let msg = check.expect_text(other).unwrap_err().to_string();
            assert!(msg.contains("only in their ending"), "{other:?}: {msg}");
        }
        // blessing a different ending replaces the file with the new bytes
        let lf = "# header\n{\n  \"a\": 1\n}\n";
        blessing.expect_text(lf).unwrap();
        assert_eq!(fs::read(blessing.path()).unwrap(), lf.as_bytes());
    }

    #[test]
    fn number_goldens_round_trip_exactly_and_honour_the_tolerance() {
        let dir = TempDir::new().unwrap();
        let values = [0.1, 1.0 / 3.0, 1e-7, -0.0, 123_456.789, f64::MAX];
        let blessing = golden(&dir, "numbers.txt", Mode::Bless);
        blessing.expect_numbers(&values, Tolerance::EXACT).unwrap();
        let blessed = fs::read(blessing.path()).unwrap();
        let check = golden(&dir, "numbers.txt", Mode::Check);
        check.expect_numbers(&values, Tolerance::EXACT).unwrap();

        let mut off = values;
        off[0] += 5e-5;
        assert!(check.expect_numbers(&off, Tolerance::EXACT).is_err());
        check.expect_numbers(&off, Tolerance::ANGLE).unwrap();
        // a golden that passes its tier is not rewritten, even when blessing
        blessing.expect_numbers(&off, Tolerance::ANGLE).unwrap();
        assert_eq!(fs::read(blessing.path()).unwrap(), blessed);
    }

    #[test]
    fn an_unusable_golden_is_corrupt_not_different_and_bless_replaces_it() {
        let dir = TempDir::new().unwrap();
        let junk = seed(&dir, "junk.txt", b"1.5\nabc\n");
        let err = junk.expect_numbers(&[1.5], Tolerance::EXACT).unwrap_err();
        assert!(matches!(err, GoldenError::Corrupt { .. }), "{err}");
        assert!(err.to_string().contains("`abc` is not a number"), "{err}");

        let broken = seed(&dir, "broken.png", b"not a png");
        let picture = Raster::from_fn(2, 2, |_, _| [1, 2, 3, 255]);
        let err = broken.expect_ssim(&picture, 0.9).unwrap_err();
        assert!(matches!(err, GoldenError::Corrupt { .. }), "{err}");
        golden(&dir, "broken.png", Mode::Bless)
            .expect_ssim(&picture, 0.9)
            .unwrap();
        broken.expect_ssim(&picture, 0.9).unwrap();
    }

    #[test]
    fn picture_goldens_pass_within_their_tier_and_are_not_rewritten() {
        let dir = TempDir::new().unwrap();
        let tol = RasterTolerance::default();
        let flat = Raster::from_fn(16, 16, |_, _| [100, 100, 100, 255]);
        let noisy = Raster::from_fn(16, 16, |x, _| [100 + u8::from(x % 2 == 1), 100, 100, 255]);
        let blessing = golden(&dir, "flat.png", Mode::Bless);
        blessing.expect_raster(&flat, &tol).unwrap();
        let before = fs::read(blessing.path()).unwrap();
        blessing.expect_raster(&noisy, &tol).unwrap();
        blessing.expect_ssim(&flat, raster::SSIM_MIN).unwrap();
        assert_eq!(fs::read(blessing.path()).unwrap(), before);

        let wrong = Raster::from_fn(16, 16, |_, _| [200, 0, 0, 255]);
        let check = golden(&dir, "flat.png", Mode::Check);
        let msg = check.expect_raster(&wrong, &tol).unwrap_err().to_string();
        assert!(msg.contains("raster differs"), "{msg}");
        assert!(check.expect_ssim(&wrong, raster::SSIM_MIN).is_err());
        blessing.expect_raster(&wrong, &tol).unwrap();
        check.expect_raster(&wrong, &tol).unwrap();
    }

    #[test]
    fn files_are_listed_sorted_and_filtered_by_extension() {
        let dir = TempDir::new().unwrap();
        for name in ["b.ron", "a.ron", "c.txt", "d"] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        fs::create_dir(dir.path().join("e.ron")).unwrap();
        let names = |found: Vec<PathBuf>| -> Vec<String> {
            found
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            names(files(dir.path(), Some("ron")).unwrap()),
            ["a.ron", "b.ron"]
        );
        assert_eq!(
            names(files(dir.path(), None).unwrap()),
            ["a.ron", "b.ron", "c.txt", "d"]
        );
    }
}
