//! Scenario DSL skeleton (architecture 19.3): scripted command and input sequences that run
//! against the system under test through a [`Driver`] and produce a trace for the tier P4
//! comparison (`diff::sequence`, or a text golden).
//!
//! Scenario files are RON, `tests/scenarios/*.ron`:
//!
//! ```ron
//! Scenario(
//!     name: "undo-label",
//!     spec: ["tools 15.6"],
//!     steps: [
//!         Do(["add_element", "cube"]),
//!         Expect("undo_label", "Add Element"),
//!         Do(["key", "Ctrl+Z"]),
//!         Record("selection"),
//!     ],
//! )
//! ```
//!
//! `Do` gives the driver a command or an input event (first word: the verb), `Expect` fails the
//! scenario when an observation differs and `Record` appends `what = value` to the trace. The
//! vocabulary of verbs and observations belongs to the driver, which the owner of the system
//! under test writes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::golden;
use crate::oracle::Oracle;

/// The folder of the scenario files, `tests/scenarios`.
pub fn dir() -> PathBuf {
    crate::workspace_root().join("tests").join("scenarios")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    /// Spec sections the scenario covers, for the reader and the ledger (`tools 15.6`).
    #[serde(default)]
    pub spec: Vec<String>,
    /// Oracles whose data the scenario needs, see [`Scenario::runnable`].
    #[serde(default)]
    pub needs: Vec<Oracle>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
pub enum Step {
    /// A command or input event: the verb followed by its arguments.
    Do(Vec<String>),
    /// The observation of the first field must equal the text of the second.
    Expect(String, String),
    /// Appends the observation to the trace.
    Record(String),
}

/// The system under test, seen through text.
pub trait Driver {
    fn act(&mut self, verb: &str, args: &[String]) -> Result<(), String>;

    /// The current value of an observable (undo label, selection, dirty flag, ...).
    fn observe(&self, what: &str) -> Result<String, String>;
}

#[derive(thiserror::Error)]
pub enum ScenarioError {
    #[error("{origin}: {source}")]
    Parse {
        origin: String,
        source: Box<ron::error::SpannedError>,
    },
    #[error("{}: {source}", .path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("scenario `{scenario}`, step {step}: {reason}")]
    Failed {
        scenario: String,
        step: usize,
        reason: String,
    },
}

debug_is_display!(ScenarioError);

impl Scenario {
    /// `origin` names the text in error messages, for example the file name.
    pub fn parse(text: &str, origin: &str) -> Result<Self, ScenarioError> {
        ron::from_str(text).map_err(|source| ScenarioError::Parse {
            origin: origin.into(),
            source: Box::new(source),
        })
    }

    pub fn load(path: &Path) -> Result<Self, ScenarioError> {
        let text = fs::read_to_string(path).map_err(|source| ScenarioError::Io {
            path: path.into(),
            source,
        })?;
        Self::parse(&text, &path.display().to_string())
    }

    /// Whether the data of every needed oracle exists. Otherwise the skip is reported and the
    /// caller leaves the scenario out.
    pub fn runnable(&self) -> bool {
        let absent = self.needs.iter().find(|oracle| oracle.path().is_none());
        if let Some(oracle) = absent {
            crate::notice(&format!(
                "SKIPPED (oracle absent): scenario `{}` needs {} ({})",
                self.name,
                oracle.var(),
                oracle.class()
            ));
        }
        absent.is_none()
    }

    /// Runs the steps in order and returns the recorded trace.
    pub fn run<D: Driver + ?Sized>(&self, driver: &mut D) -> Result<Vec<String>, ScenarioError> {
        let mut trace = Vec::new();
        for (index, step) in self.steps.iter().enumerate() {
            run_step(step, driver, &mut trace).map_err(|reason| ScenarioError::Failed {
                scenario: self.name.clone(),
                step: index + 1,
                reason,
            })?;
        }
        Ok(trace)
    }
}

fn run_step<D: Driver + ?Sized>(
    step: &Step,
    driver: &mut D,
    trace: &mut Vec<String>,
) -> Result<(), String> {
    match step {
        Step::Do(words) => {
            let (verb, args) = words.split_first().ok_or("`Do` needs a verb")?;
            driver
                .act(verb, args)
                .map_err(|err| format!("{} failed: {err}", words.join(" ")))
        }
        Step::Expect(what, expected) => {
            let actual = observe(driver, what)?;
            if actual == *expected {
                return Ok(());
            }
            Err(format!("expected {what} = {expected:?}, actual {actual:?}"))
        }
        Step::Record(what) => {
            let value = observe(driver, what)?;
            trace.push(format!("{what} = {value}"));
            Ok(())
        }
    }
}

fn observe<D: Driver + ?Sized>(driver: &D, what: &str) -> Result<String, String> {
    driver
        .observe(what)
        .map_err(|err| format!("cannot observe `{what}`: {err}"))
}

/// Every `.ron` file of `dir`, loaded in name order.
pub fn load_dir(dir: &Path) -> Result<Vec<Scenario>, ScenarioError> {
    let files = golden::files(dir, Some("ron")).map_err(|source| ScenarioError::Io {
        path: dir.into(),
        source,
    })?;
    files.iter().map(|path| Scenario::load(path)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stack machine standing in for the system under test.
    #[derive(Default)]
    struct Stack(Vec<i32>);

    impl Driver for Stack {
        fn act(&mut self, verb: &str, args: &[String]) -> Result<(), String> {
            match (verb, args) {
                ("push", [n]) => self
                    .0
                    .push(n.parse().map_err(|_| format!("`{n}` is no number"))?),
                ("pop", []) => {
                    self.0.pop().ok_or("the stack is empty")?;
                }
                _ => return Err(format!("unknown command `{verb}`")),
            }
            Ok(())
        }

        fn observe(&self, what: &str) -> Result<String, String> {
            match what {
                "top" => Ok(self.0.last().map_or("none".into(), i32::to_string)),
                "depth" => Ok(self.0.len().to_string()),
                _ => Err("unknown observable".into()),
            }
        }
    }

    const FILE: &str = r#"
        // Comments are RON comments.
        Scenario(
            name: "stack",
            spec: ["arch 19.3"],
            steps: [
                Do(["push", "4"]),
                Do(["push", "7"]),
                Expect("top", "7"),
                Record("depth"),
                Do(["pop"]),
                Record("top"),
            ],
        )
    "#;

    #[test]
    fn a_scenario_file_parses_with_optional_fields_left_out() {
        let scenario = Scenario::parse(FILE, "inline").unwrap();
        assert_eq!(scenario.name, "stack");
        assert_eq!(scenario.spec, ["arch 19.3"]);
        assert!(scenario.needs.is_empty());
        assert_eq!(scenario.steps.len(), 6);
        assert!(matches!(&scenario.steps[3], Step::Record(what) if what == "depth"));
    }

    #[test]
    fn needed_oracles_are_named_in_the_file() {
        let text = r#"Scenario(name: "n", needs: [ParityRefs, UiDump], steps: [])"#;
        let scenario = Scenario::parse(text, "inline").unwrap();
        assert_eq!(scenario.needs, [Oracle::ParityRefs, Oracle::UiDump]);
        assert!(
            Scenario::parse(r#"Scenario(name: "n", steps: [])"#, "inline")
                .unwrap()
                .runnable()
        );
    }

    #[test]
    fn misspelt_fields_and_unknown_steps_are_rejected_with_the_origin() {
        for text in [
            r#"Scenario(name: "n", step: [])"#,
            r#"Scenario(name: "n", steps: [Jump(["x"])])"#,
            r#"Scenario(steps: [])"#,
        ] {
            let msg = Scenario::parse(text, "broken.ron").unwrap_err().to_string();
            assert!(msg.starts_with("broken.ron: "), "{msg}");
        }
    }

    #[test]
    fn running_returns_the_recorded_trace() {
        let scenario = Scenario::parse(FILE, "inline").unwrap();
        let driver: &mut dyn Driver = &mut Stack::default();
        let trace = scenario.run(driver).unwrap();
        assert_eq!(trace, ["depth = 2", "top = 4"]);
    }

    #[test]
    fn a_failed_step_names_the_scenario_and_the_step() {
        let text = r#"Scenario(name: "wrong", steps: [Do(["push", "1"]), Expect("top", "2")])"#;
        let err = Scenario::parse(text, "inline")
            .unwrap()
            .run(&mut Stack::default())
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "scenario `wrong`, step 2: expected top = \"2\", actual \"1\""
        );
    }

    #[test]
    fn driver_errors_unknown_observables_and_empty_steps_fail_the_step() {
        let run = |steps: &str| {
            let text = format!(r#"Scenario(name: "s", steps: [{steps}])"#);
            Scenario::parse(&text, "inline")
                .unwrap()
                .run(&mut Stack::default())
                .unwrap_err()
                .to_string()
        };
        assert!(run(r#"Do(["pop"])"#).contains("step 1: pop failed: the stack is empty"));
        assert!(run(r#"Do(["push", "x"])"#).contains("`x` is no number"));
        assert!(run(r#"Record("height")"#).contains("cannot observe `height`: unknown observable"));
        assert!(run("Do([])").contains("`Do` needs a verb"));
    }

    #[test]
    fn a_folder_loads_its_ron_files_in_name_order() {
        let dir = tempfile::tempdir().unwrap();
        for (file, name) in [
            ("b.ron", "second"),
            ("a.ron", "first"),
            ("c.txt", "ignored"),
        ] {
            fs::write(
                dir.path().join(file),
                format!(r#"Scenario(name: "{name}", steps: [])"#),
            )
            .unwrap();
        }
        let names: Vec<_> = load_dir(dir.path())
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["first", "second"]);

        fs::write(dir.path().join("d.ron"), "Scenario(").unwrap();
        let msg = load_dir(dir.path()).unwrap_err().to_string();
        assert!(msg.contains("d.ron"), "{msg}");
    }
}
