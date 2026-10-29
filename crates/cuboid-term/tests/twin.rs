//! The twin starts the `cuboid` next to it with its arguments, relays its standard streams
//! through pipes and gives back its exit code.

use std::env::consts::EXE_SUFFIX;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use tempfile::TempDir;

/// A folder that holds the twin.
fn folder_with_twin() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let twin = dir.path().join(format!("cuboid-term{EXE_SUFFIX}"));
    std::fs::copy(env!("CARGO_BIN_EXE_cuboid-term"), &twin).expect("copy the twin");
    (dir, twin)
}

/// A folder that holds the twin and a shell as its `cuboid`, with the flag that makes the shell
/// run the script given after it.
fn folder_with_shell() -> (TempDir, PathBuf, &'static str) {
    let (dir, twin) = folder_with_twin();
    let (shell, flag) = if cfg!(windows) {
        (
            PathBuf::from(std::env::var_os("ComSpec").expect("ComSpec")),
            "/c",
        )
    } else {
        (PathBuf::from("/bin/sh"), "-c")
    };
    std::fs::copy(shell, dir.path().join(format!("cuboid{EXE_SUFFIX}"))).expect("copy the shell");
    (dir, twin, flag)
}

#[test]
fn without_a_cuboid_next_to_it_the_twin_says_so_in_one_line_and_exits_with_2() {
    let (_dir, twin) = folder_with_twin();
    let output = Command::new(twin).output().expect("run the twin");
    let message = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(message.lines().count(), 1, "{message}");
    assert!(
        message.starts_with("cuboid-term: cannot start "),
        "{message}"
    );
}

#[test]
fn the_arguments_and_the_output_of_cuboid_pass_through_and_so_does_its_exit_code() {
    let (_dir, twin, flag) = folder_with_shell();
    // Windows codes have 32 bits, so 300 shows that the twin does not cut one to a byte.
    let code = if cfg!(windows) { 300 } else { 7 };
    let script = format!("echo forwarded&&echo kept 1>&2&&exit {code}");
    let output = Command::new(twin)
        .args([flag, &script])
        .output()
        .expect("run the twin");
    assert_eq!(output.status.code(), Some(code));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "forwarded");
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "kept");
}

#[test]
fn the_input_of_the_twin_reaches_cuboid_and_its_end_ends_the_input_of_cuboid() {
    let (_dir, twin, flag) = folder_with_shell();
    // `sort` and `cat` copy their input to their output and finish only at the end of it.
    let script = if cfg!(windows) { "sort" } else { "cat" };
    let mut child = Command::new(twin)
        .args([flag, script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("run the twin");
    let mut input = child.stdin.take().expect("stdin of the twin");
    input.write_all(b"hello twin\n").expect("write the input");
    drop(input);
    let output = child.wait_with_output().expect("wait for the twin");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello twin");
}

#[test]
fn non_ascii_utf8_output_of_cuboid_arrives_byte_exact() {
    let (dir, twin, flag) = folder_with_shell();
    // Many lines, so that the relay cuts the text inside multi-byte characters more than once.
    let text = "D:\\mô hình\\đèn \u{1F9CA}\n".repeat(5000);
    std::fs::write(dir.path().join("paths.txt"), &text).expect("write the file");
    let script = if cfg!(windows) {
        "type paths.txt"
    } else {
        "cat paths.txt"
    };
    let output = Command::new(twin)
        .args([flag, script])
        .current_dir(dir.path())
        .output()
        .expect("run the twin");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout == text.as_bytes(), "the bytes differ");
}
