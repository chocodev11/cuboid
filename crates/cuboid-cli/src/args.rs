//! The command lines of the commands that take flags and several options.

use crate::Fail;

/// The words after a command, sorted into flags, options with a value and plain words.
#[derive(Debug, Default)]
pub struct Args {
    flags: Vec<String>,
    options: Vec<(String, String)>,
    words: Vec<String>,
}

impl Args {
    /// Splits `args`. `flags` are the options without a value, `options` the ones with a value
    /// (`--name V` or `--name=V`); `--` ends the options. Anything else that starts with `-` is
    /// refused, and so is an option that is given twice.
    pub fn split(args: &[String], flags: &[&str], options: &[&str]) -> Result<Args, Fail> {
        let mut found = Args::default();
        let mut rest = args.iter();
        let mut options_end = false;
        while let Some(arg) = rest.next().map(String::as_str) {
            if options_end || !arg.starts_with('-') {
                found.words.push(arg.to_owned());
            } else if arg == "--" {
                options_end = true;
            } else if let Some(name) = flags.iter().find(|name| **name == arg) {
                found.flags.push((*name).to_owned());
            } else if let Some((name, value)) = option_of(arg, options, &mut rest)? {
                if found.options.iter().any(|(known, _)| *known == name) {
                    return Err(Fail::Usage(format!("{name} is given twice")));
                }
                found.options.push((name, value));
            } else {
                return Err(Fail::Usage(format!("unknown option `{arg}`")));
            }
        }
        Ok(found)
    }

    /// Whether the flag was given.
    pub fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|flag| flag == name)
    }

    /// The value of the option, if it was given.
    pub fn option(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value.as_str())
    }

    /// The plain word, if there is one; more than one is a usage error.
    pub fn word(&self, what: &str) -> Result<Option<&str>, Fail> {
        match self.words.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some(one)),
            _ => Err(Fail::Usage(format!("only one {what} is accepted"))),
        }
    }
}

/// The name and value of `arg` when it is one of `options`, taking the value from the next
/// argument for `--name V`.
fn option_of<'a>(
    arg: &str,
    options: &[&str],
    rest: &mut impl Iterator<Item = &'a String>,
) -> Result<Option<(String, String)>, Fail> {
    for name in options {
        if arg == *name {
            let value = rest
                .next()
                .ok_or_else(|| Fail::Usage(format!("{name} needs a value")))?;
            return Ok(Some(((*name).to_owned(), value.clone())));
        }
        if let Some(value) = arg
            .strip_prefix(name)
            .and_then(|tail| tail.strip_prefix('='))
        {
            return Ok(Some(((*name).to_owned(), value.to_owned())));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn flags_options_and_words_are_told_apart() {
        let args = Args::split(
            &words(&["--json", "--target=item", "--brief", "p", "m.cubik"]),
            &["--json", "--md"],
            &["--target", "--brief"],
        )
        .expect("splits");
        assert!(args.flag("--json") && !args.flag("--md"));
        assert_eq!(args.option("--target"), Some("item"));
        assert_eq!(args.option("--brief"), Some("p"));
        assert_eq!(args.word("model").expect("one"), Some("m.cubik"));
    }

    #[test]
    fn mistakes_are_usage_errors() {
        let split = |list: &[&str]| Args::split(&words(list), &["--json"], &["--target"]);
        assert!(matches!(split(&["--nope"]), Err(Fail::Usage(_))));
        assert!(matches!(split(&["--target"]), Err(Fail::Usage(_))));
        assert!(matches!(
            split(&["--target=a", "--target", "b"]),
            Err(Fail::Usage(_))
        ));
        let two = split(&["a", "b"]).expect("splits");
        assert!(matches!(two.word("folder"), Err(Fail::Usage(_))));
        // After `--` a dash is part of a word.
        let dashed = split(&["--", "-x"]).expect("splits");
        assert_eq!(dashed.word("folder").expect("one"), Some("-x"));
    }
}
