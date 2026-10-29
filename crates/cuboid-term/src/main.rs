#![forbid(unsafe_code)]
//! Layer L6. Binary `cuboid-term`: the console twin of `cuboid`. The release `cuboid.exe` is a
//! Windows GUI program, so a shell does not wait for it, it shows none of its output, and it is
//! attached to no console. This console program, installed next to it as `cuboid.com`, starts it
//! from the same folder with the same arguments, waits, and exits with its code. The child gets
//! pipes for its standard streams and the twin relays them to its own: the twin is attached to
//! the console, so std writes the child's UTF-8 text to it as UTF-16 and a non-ASCII path shows
//! right, and when the twin dies (Ctrl+C) the pipes close and the child sees a broken pipe or the
//! end of its input, not a console that is gone. Exit code 2 when `cuboid` cannot be started, or
//! when this program is installed under that name itself.

use std::env::{self, consts::EXE_SUFFIX};
use std::io::{self, ErrorKind, Read, Write};
use std::process::{Command, Stdio, exit};
use std::thread;

/// Copies `from` to `to` in chunks, flushing after each so interactive output is not held back.
/// Ends at the end of `from`, or quietly at the first error: a write error means the receiving
/// side is gone, and dropping `from` then closes the child's pipe.
fn relay(mut from: impl Read, mut to: impl Write) {
    let mut chunk = [0; 8192];
    loop {
        let len = match from.read(&mut chunk) {
            Ok(0) => return,
            Ok(len) => len,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return,
        };
        if to
            .write_all(&chunk[..len])
            .and_then(|()| to.flush())
            .is_err()
        {
            return;
        }
    }
}

fn run() -> Result<i32, String> {
    let me = env::current_exe().map_err(|error| format!("cannot find its own folder: {error}"))?;
    let program = me.with_file_name(format!("cuboid{EXE_SUFFIX}"));
    if program == me {
        return Err(format!("{} would start itself", me.display()));
    }
    let mut child = Command::new(&program)
        .args(env::args_os().skip(1))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", program.display()))?;
    let child_in = child.stdin.take().expect("stdin is piped");
    let child_out = child.stdout.take().expect("stdout is piped");
    let child_err = child.stderr.take().expect("stderr is piped");
    let out = thread::spawn(move || relay(child_out, io::stdout()));
    let err = thread::spawn(move || relay(child_err, io::stderr()));
    // Not joined: it may wait on a console read for ever, and `exit` in `main` ends it. Dropping
    // `child_in` when the twin's input ends gives a server its end of input.
    thread::spawn(move || relay(io::stdin(), child_in));
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for {}: {error}", program.display()))?;
    // Both end at the child's end of output, so this only waits for the last chunk to be written.
    let _ = out.join();
    let _ = err.join();
    // No code means a signal ended it, which only Unix has; 1 is the code chosen for that.
    Ok(status.code().unwrap_or(1))
}

fn main() {
    match run() {
        Ok(code) => exit(code),
        Err(why) => {
            eprintln!("cuboid-term: {why}");
            exit(2);
        }
    }
}
