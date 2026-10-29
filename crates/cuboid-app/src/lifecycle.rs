//! Start-up order and shut-down (spec: app 1.2, 2.1).

use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use cuboid_settings::{Origin, Settings};
use cuboid_shell::ShellOptions;

use crate::app::App;
use crate::io_thread::IoThread;
use crate::platform::{self, Dirs};
use crate::state::{Level, State};
use crate::{crash, logging, render_glue, ui_setup};

/// The command line.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub file: Option<PathBuf>,
    pub debug: bool,
    pub reset_window: bool,
    pub help: bool,
    pub version: bool,
    /// The arguments after `mcp`, when the command is the agent server (no window opens).
    pub mcp: Option<Vec<String>>,
}

impl Args {
    /// Reads the arguments after the program name. Unknown options are ignored, except after
    /// `mcp`, which owns the rest of the line.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Args {
        let mut parsed = Args::default();
        let mut args = args.into_iter().peekable();
        if args.next_if(|first| first == "mcp").is_some() {
            parsed.mcp = Some(args.collect());
            return parsed;
        }
        for arg in args {
            match arg.as_str() {
                "--debug" => parsed.debug = true,
                "--reset-window" => parsed.reset_window = true,
                "--help" | "-h" => parsed.help = true,
                "--version" | "-V" => parsed.version = true,
                other if other.starts_with("--") => {
                    let _ = writeln!(io::stderr(), "ignoring unknown option {other}");
                }
                _ => {
                    parsed.file = parsed.file.or_else(|| {
                        Some(std::path::absolute(&arg).unwrap_or_else(|_| PathBuf::from(&arg)))
                    });
                }
            }
        }
        parsed
    }
}

const HELP: &str = "usage: cuboid [--debug] [--reset-window] [FILE.cubik|FILE.cub]\n       cuboid mcp --root <folder> [--read-only] [--check]";

/// Runs the program: logging and the crash hook first, then the settings, the I/O thread and the
/// window. Returns when the window has closed and the file thread has finished its queue. The
/// agent server (`cuboid mcp`) runs right after the logging and never reaches the window; stdout
/// belongs to its protocol, so everything else it says goes to stderr and the log file.
pub fn run(args: Args) -> ExitCode {
    let version = env!("CARGO_PKG_VERSION");
    if args.version {
        return crate::mcp::write_stdout(&format!("cuboid {version}\n"), ExitCode::SUCCESS);
    }
    if args.help {
        return crate::mcp::write_stdout(&format!("{HELP}\n"), ExitCode::SUCCESS);
    }
    let dirs = Dirs::detect();
    let server = args.mcp.is_some();
    logging::init(&dirs.logs(), logging::level(args.debug), args.debug, server);
    crash::install_hook(&dirs.logs());
    log::info!(
        "cuboid {version} on {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if let Some(mcp) = &args.mcp {
        return crate::mcp::run(mcp);
    }

    let mut notices = Vec::new();
    let mut settings = load_settings(&dirs, &mut notices);
    let io = match IoThread::spawn() {
        Ok(io) => io,
        Err(error) => {
            log::error!("cannot start the file thread: {error}");
            let _ = writeln!(
                io::stderr(),
                "cuboid: cannot start the file thread: {error}"
            );
            return ExitCode::FAILURE;
        }
    };
    let prefs_path = ui_setup::prefs_file(&dirs);
    let prefs = ui_setup::load_prefs(&prefs_path);
    // The UI preferences hold the autosave interval.
    settings.backup_every_minutes = i32::try_from(prefs.autosave_minutes).unwrap_or(10);
    let cuboid = &settings.cuboid;
    let options = ShellOptions {
        // macOS keeps the system title bar; elsewhere the top bar is the title bar.
        decorations: prefs.system_title_bar || cfg!(target_os = "macos"),
        size: (cuboid.window_width, cuboid.window_height),
        position: settings.saved_window_position(),
        ui_scale_override: cuboid.scale_override,
        reset_window: args.reset_window,
        optional_features: render_glue::OPTIONAL_FEATURES,
        ..ShellOptions::default()
    };

    let mut state = State::new(
        dirs,
        settings,
        io,
        platform::ansi_code_page(),
        Instant::now(),
    );
    state.editor.core.env_mut().render.antialiasing = prefs.antialiasing;
    for notice in notices {
        state.toast(Level::Warn, notice);
    }
    let no_file = args.file.is_none();
    state.startup(args.file);

    let mut app = App::with_ui(state, prefs, ui_setup::fonts(), prefs_path);
    if no_file {
        // Nothing to open: the start screen is the first thing to see.
        app.front.ui.show_start();
    }
    match cuboid_shell::run(app, options) {
        Ok(()) => {
            log::info!("closed");
            ExitCode::SUCCESS
        }
        Err(error) => {
            log::error!("{error}");
            let _ = writeln!(io::stderr(), "cuboid: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Loads the settings; a file that cannot be used gives the first-run values and a notice for the
/// user (spec: app 1.2 step 5).
fn load_settings(dirs: &Dirs, notices: &mut Vec<String>) -> Settings {
    let env = platform::environment();
    match Settings::load(&dirs.settings_file(), &env) {
        Ok(loaded) => {
            if let Origin::Rejected { error, kept_as } = &loaded.origin {
                log::warn!("the settings file is not usable: {error}");
                notices.push(match kept_as {
                    Some(path) => format!(
                        "The settings file was not usable. It was kept as {}.",
                        path.display()
                    ),
                    None => "The settings file was not usable and was ignored.".to_owned(),
                });
            }
            loaded.settings
        }
        Err(error) => {
            log::warn!("{error}");
            notices.push(format!("Cannot read the settings: {error}"));
            Settings::first_run(&env)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn the_command_line_is_read() {
        let args = parse(&["--debug", "model.cubik", "--reset-window", "other.cubik"]);
        assert!(args.debug && args.reset_window);
        assert!(args.file.is_some_and(|file| file.ends_with("model.cubik")));
        assert_eq!(parse(&[]), Args::default());
        assert!(parse(&["--version"]).version);
    }

    #[test]
    fn a_relative_file_is_made_absolute() {
        let file = parse(&["lamp.cubik"]).file.unwrap();
        assert!(file.is_absolute() && file.ends_with("lamp.cubik"));
        let absolute = std::env::current_dir().unwrap().join("lamp.cubik");
        assert_eq!(parse(&[absolute.to_str().unwrap()]).file, Some(absolute));
    }

    #[test]
    fn mcp_takes_the_rest_of_the_line_and_only_at_the_start() {
        let args = parse(&["mcp", "--root", "D:\\lamp", "--debug", "model.cubik"]);
        assert_eq!(
            args.mcp.as_deref(),
            Some(&["--root", "D:\\lamp", "--debug", "model.cubik"].map(String::from)[..])
        );
        assert!(!args.debug && args.file.is_none());
        // A file called mcp is opened by its path, and the word later in the line is a file.
        assert_eq!(parse(&["./mcp"]).mcp, None);
        let args = parse(&["model.cubik", "mcp"]);
        assert_eq!(args.mcp, None);
        assert!(args.file.is_some_and(|file| file.ends_with("model.cubik")));
    }

    #[test]
    fn an_unusable_settings_file_gives_defaults_and_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = Dirs::under(dir.path());
        std::fs::create_dir_all(&dirs.config).unwrap();
        std::fs::write(dirs.settings_file(), b"{ broken").unwrap();
        let mut notices = Vec::new();
        let settings = load_settings(&dirs, &mut notices);
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains(".bad"), "{notices:?}");
        assert!(settings.backup_every_minutes > 0);
        assert!(dirs.config.join("settings.json.bad").exists());
    }
}
