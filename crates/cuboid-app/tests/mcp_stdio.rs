//! `cuboid mcp` as a client runs it: the built binary on piped stdio, in both eras of the
//! protocol, with models made in the test. The picture tests need a graphics adapter (a software
//! one will do); without one they are skipped with a notice, and under `CI` they fail instead.
//! Every process gets a folder of its own through `CUBOID_HOME`, so that no test writes into the
//! user's log or leaves a crash report there.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cuboid_mcp::Picture;
use cuboid_model::image::{Image, RgbaBuf, encode_png};
use serde_json::{Value, json};

/// Claude Code on Windows ends a server 300 ms after it closes the input. The exit takes 60 ms
/// without a picture and 110 to 160 ms after one on the development machine (opengl32.dll, which
/// wgpu links, costs about 55 ms of that); the limit leaves room for a busy machine.
const EXIT_LIMIT: Duration = Duration::from_millis(250);
/// The longest a reply may take; the first picture opens the graphics device.
const REPLY_LIMIT: Duration = Duration::from_secs(60);

const BIN: &str = env!("CARGO_BIN_EXE_cuboid");

/// The binary with the arguments, keeping its log and crash reports below `home`.
fn command(args: &[&str], home: &Path) -> Command {
    let mut command = Command::new(BIN);
    command.args(args).env("CUBOID_HOME", home);
    command
}

/// The crash reports a process left below its `CUBOID_HOME`.
fn crash_reports(home: &Path) -> Vec<String> {
    let logs = fs::read_dir(home.join("data").join("logs"));
    logs.into_iter()
        .flatten()
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with("crash_"))
        .collect()
}

/// A server process and its three streams.
struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    stderr: JoinHandle<String>,
    home: tempfile::TempDir,
}

/// What is left when the input has ended.
struct Finished {
    status: ExitStatus,
    pid: u32,
    /// From the close of the input to the end of the process.
    exit_after: Duration,
    /// Output lines nobody asked for.
    rest: Vec<String>,
    stderr: String,
    home: tempfile::TempDir,
}

impl Session {
    fn start(args: &[&str]) -> Session {
        let home = tempfile::tempdir().expect("temp dir");
        let mut child = command(args, home.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary starts");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr = thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            text
        });
        Session {
            child,
            stdin,
            lines,
            stderr,
            home,
        }
    }

    fn serve(root: &Path, extra: &[&str]) -> Session {
        let mut args = vec!["mcp", "--root", root.to_str().expect("UTF-8 path")];
        args.extend_from_slice(extra);
        Session::start(&args)
    }

    /// Sends a message and returns the reply to it. Every line of stdout has to be a JSON-RPC
    /// message, which is the point of the test.
    fn ask(&mut self, message: &Value) -> Value {
        self.send(message);
        let line = self
            .lines
            .recv_timeout(REPLY_LIMIT)
            .unwrap_or_else(|_| panic!("no reply to {message}\n{}", self.child_state()));
        let reply: Value = serde_json::from_str(&line).unwrap_or_else(|error| {
            panic!("stdout holds a line that is no JSON ({error}): {line}")
        });
        assert_eq!(reply["jsonrpc"], "2.0", "{line}");
        assert_eq!(reply["id"], message["id"], "{line}");
        reply
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("the input is open");
        writeln!(stdin, "{message}").expect("writes");
        stdin.flush().expect("flushes");
    }

    fn child_state(&mut self) -> String {
        format!("the process: {:?}", self.child.try_wait())
    }

    /// Closes the input and waits for the process to end.
    fn finish(mut self) -> Finished {
        drop(self.stdin.take());
        let closed = Instant::now();
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("waits") {
                break status;
            }
            if closed.elapsed() > Duration::from_secs(10) {
                let _ = self.child.kill();
                panic!("the server did not end after its input did");
            }
            thread::sleep(Duration::from_millis(1));
        };
        let exit_after = closed.elapsed();
        Finished {
            status,
            pid: self.child.id(),
            exit_after,
            rest: self.lines.iter().collect(),
            stderr: self.stderr.join().expect("the stderr reader"),
            home: self.home,
        }
    }
}

impl Finished {
    /// The process ended well, at once, said nothing else on stdout and left no crash report.
    fn assert_clean(&self) {
        assert!(self.status.success(), "{:?}\n{}", self.status, self.stderr);
        assert!(self.rest.is_empty(), "{:?}", self.rest);
        assert!(
            self.exit_after < EXIT_LIMIT,
            "the exit took {:?} after the input ended",
            self.exit_after
        );
        assert_eq!(crash_reports(self.home.path()), Vec::<String>::new());
    }
}

fn run(args: &[&str]) -> Output {
    let home = tempfile::tempdir().expect("temp dir");
    command(args, home.path())
        .stdin(Stdio::null())
        .output()
        .expect("the binary starts")
}

fn legacy(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn modern(id: u64, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {"name": "stdio-test", "version": "1"},
    });
    legacy(id, method, params)
}

fn call(id: u64, tool: &str, arguments: Value) -> Value {
    legacy(
        id,
        "tools/call",
        json!({"name": tool, "arguments": arguments}),
    )
}

/// The faces of the block of the test model in the order of the texture's columns, with the
/// colour of each.
const FACES: [(&str, [u8; 3]); 6] = [
    ("north", [200, 100, 50]),
    ("east", [50, 200, 100]),
    ("south", [100, 50, 200]),
    ("west", [200, 200, 50]),
    ("up", [50, 200, 200]),
    ("down", [200, 50, 200]),
];
/// The marker's colour: the seventh column of the texture.
const MARKER: [u8; 3] = [240, 240, 240];

/// The faces of an element, each showing the whole height of one column of the texture.
fn faces(column: impl Fn(usize) -> usize) -> Value {
    let mut faces = serde_json::Map::new();
    for (at, (name, _)) in FACES.iter().enumerate() {
        let column = column(at);
        faces.insert(
            (*name).to_owned(),
            json!({"uv": [column, 0, column + 1, 8], "texture": 0}),
        );
    }
    Value::Object(faces)
}

/// A block of 16 units with a differently coloured texel on each face, and a small marker on its
/// east side at the bottom and the north: a model that tells every direction.
fn model() -> String {
    let mut texture = RgbaBuf::new(8, 8);
    let colours = FACES.iter().map(|(_, rgb)| *rgb).chain([MARKER]);
    for (column, [r, g, b]) in colours.enumerate() {
        for row in 0..8 {
            texture.set(i32::try_from(column).unwrap(), row, [r, g, b, 255]);
        }
    }
    let png = encode_png(&Image::from_rgba(texture)).expect("encodes");
    let url = cuboid_fmt_bbmodel::data_url::png_url(&png);
    json!({
        "meta": {"format_version": "4.10", "model_format": "java_block", "box_uv": false},
        "name": "block",
        "resolution": {"width": 8, "height": 8},
        "elements": [
            {"name": "block", "from": [0, 0, 0], "to": [16, 16, 16], "origin": [8, 8, 8],
             "uuid": "e1", "faces": faces(|at| at)},
            {"name": "marker", "from": [16, 0, 0], "to": [20, 4, 4], "origin": [18, 2, 2],
             "uuid": "e2", "faces": faces(|_| 6)},
        ],
        "outliner": ["e1", "e2"],
        "textures": [{"name": "colours", "id": "0", "source": url}],
    })
    .to_string()
}

/// A project folder with the test model in it.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::write(dir.path().join("block.bbmodel"), model()).expect("writes the model");
    dir
}

#[test]
fn a_legacy_session_initializes_lists_calls_and_ends_with_its_input() {
    let dir = project();
    let mut session = Session::serve(dir.path(), &[]);
    let init = session.ask(&legacy(
        1,
        "initialize",
        json!({"protocolVersion": "2025-11-25", "capabilities": {},
               "clientInfo": {"name": "legacy-test", "version": "1"}}),
    ));
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(init["result"]["serverInfo"]["name"], "cuboid");
    session.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    let list = session.ask(&legacy(2, "tools/list", json!({})));
    let tools = list["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 18);
    assert!(tools.iter().any(|tool| tool["name"] == "render_sheet"));
    let listed = session.ask(&call(3, "project_list", json!({})));
    let result = &listed["result"];
    assert_eq!(
        result["structuredContent"]["files"][0]["path"],
        "block.bbmodel"
    );
    assert!(result.get("isError").is_none(), "{result}");
    let finished = session.finish();
    finished.assert_clean();
    eprintln!("legacy exit after {:?}", finished.exit_after);
    // The server logs into the folder of its own, and each of its lines says which process it is.
    let log = fs::read_to_string(finished.home.path().join("data/logs/cuboid.log"))
        .expect("the server writes its log below CUBOID_HOME");
    let pid = finished.pid;
    for words in ["cuboid::mcp serving", "cuboid::mcp the input ended"] {
        assert!(log.contains(&format!("[{pid}] {words}")), "{log}");
    }
    assert!(
        log.lines().all(|line| line.contains(&format!("[{pid}] "))),
        "{log}"
    );
}

#[test]
fn a_modern_session_discovers_and_calls_with_meta_alone() {
    let dir = project();
    let mut session = Session::serve(dir.path(), &[]);
    let discover = session.ask(&modern(1, "server/discover", json!({})));
    let result = &discover["result"];
    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["supportedVersions"][0], "2026-07-28");
    // A request without the metadata is refused, and the session goes on.
    let bare = session.ask(&legacy(2, "tools/list", json!({})));
    assert_eq!(bare["error"]["code"], -32602);
    let opened = session.ask(&modern(
        3,
        "tools/call",
        json!({"name": "project_open", "arguments": {"model": "block.bbmodel"}}),
    ));
    let result = &opened["result"];
    assert_eq!(result["resultType"], "complete", "{result}");
    assert!(
        result["structuredContent"]["revision"].is_string(),
        "{result}"
    );
    let finished = session.finish();
    finished.assert_clean();
    eprintln!("modern exit after {:?}", finished.exit_after);
}

#[test]
fn read_only_reaches_the_server_and_a_save_is_refused_before_it_writes() {
    let dir = project();
    let mut session = Session::serve(dir.path(), &["--read-only"]);
    let save = json!({"name": "project_save", "arguments": {"model": "block.bbmodel", "path": "copy.cubik"}});
    let saved = session.ask(&modern(1, "tools/call", save));
    assert_eq!(saved["result"]["isError"], true, "{saved}");
    let text = saved["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("E_READ_ONLY"), "{text}");
    assert!(!dir.path().join("copy.cubik").exists());
    session.finish().assert_clean();
}

#[test]
fn a_missing_relative_or_drive_root_is_refused_with_exit_2_and_a_message_on_stderr() {
    let dir = project();
    let file = dir.path().join("block.bbmodel");
    let drive = std::env::temp_dir()
        .ancestors()
        .last()
        .expect("a root")
        .to_owned();
    let root = dir.path().to_str().unwrap();
    let cases: [(Vec<&str>, &str); 8] = [
        (vec!["mcp"], "--root"),
        (vec!["mcp", "--read-only"], "--root"),
        (vec!["mcp", "--root"], "needs a folder"),
        (vec!["mcp", "--root", "models/lamp"], "absolute"),
        (vec!["mcp", "--root", drive.to_str().unwrap()], "drive root"),
        (
            vec!["mcp", "--root", file.to_str().unwrap()],
            "not a folder",
        ),
        (vec!["mcp", "--root", root, "--serve"], "--serve"),
        (vec!["mcp", "--root", root, "--root", root], "twice"),
    ];
    for (args, message) in cases {
        let output = run(&args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{args:?}\n{stderr}");
        assert!(stderr.contains(message), "{args:?}\n{stderr}");
        assert!(output.stdout.is_empty(), "{args:?} wrote to stdout");
    }
    // The same refusal for --check, which serves nothing either.
    let output = run(&["mcp", "--check", "--root", "models/lamp"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

/// Whether a picture can be made here; a notice and `false` when there is no adapter.
fn graphics(error: &Value) -> bool {
    let text = error["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    if !text.contains("E_NO_GPU") {
        return true;
    }
    assert!(
        std::env::var_os("CI").is_none(),
        "no graphics adapter under CI: {text}"
    );
    eprintln!("SKIPPED (no graphics adapter): {text}");
    false
}

#[test]
fn check_serves_nothing_and_prints_the_report_and_the_lines_for_each_client() {
    let dir = project();
    let root = dir.path().to_str().expect("UTF-8 path");
    let output = run(&["mcp", "--check", "--root", root]);
    let text = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("ok   brief"), "{text}\n{stderr}");
    assert!(text.contains("validate: one cube"), "{text}");
    if output.status.code() == Some(2) && text.contains("FAIL render: there is no graphics adapter")
    {
        assert!(std::env::var_os("CI").is_none(), "no adapter under CI");
        eprintln!("SKIPPED (no graphics adapter)");
    } else {
        assert_eq!(output.status.code(), Some(0), "{text}\n{stderr}");
        assert!(text.contains("ok   render: a cube drawn"), "{text}");
    }
    let exe = Path::new(BIN).file_name().unwrap().to_str().unwrap();
    assert!(
        text.contains("claude mcp add --transport stdio cuboid \"--\" \""),
        "{text}"
    );
    assert!(
        text.contains(&format!("{exe}\" mcp --root \"{root}\"")),
        "{text}"
    );
    assert!(text.contains("codex mcp add cuboid \"--\" "), "{text}");
    assert!(text.contains("[mcp_servers.cuboid]"), "{text}");
    assert!(
        text.contains("\"cuboid-"),
        "the Claude Desktop entry: {text}"
    );
    let desktop = if cfg!(windows) {
        r"%APPDATA%\Claude\claude_desktop_config.json"
    } else if cfg!(target_os = "macos") {
        "~/Library/Application Support/Claude/claude_desktop_config.json"
    } else {
        "~/.config/Claude/claude_desktop_config.json"
    };
    assert!(text.contains(desktop), "{text}");
    // Nothing was written into the project.
    let mut names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["block.bbmodel"]);
}

#[test]
fn help_and_check_into_a_closed_pipe_exit_1_without_a_crash_report() {
    let dir = project();
    let root = dir.path().to_str().expect("UTF-8 path");
    let cases: [&[&str]; 2] = [&["mcp", "--help"], &["mcp", "--check", "--root", root]];
    for args in cases {
        let home = tempfile::tempdir().expect("temp dir");
        // The reader goes before the process starts, as the reader of `| head` goes.
        let (reader, writer) = io::pipe().expect("a pipe");
        drop(reader);
        let output = command(args, home.path())
            .stdin(Stdio::null())
            .stdout(writer)
            .output()
            .expect("the binary starts");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{args:?}\n{stderr}");
        assert!(!stderr.contains("panicked"), "{args:?}\n{stderr}");
        assert_eq!(crash_reports(home.path()), Vec::<String>::new(), "{args:?}");
    }
}

/// The picture a render call saved under `renders/`.
fn saved_picture(root: &Path, result: &Value) -> Picture {
    let path = result["structuredContent"]["path"]
        .as_str()
        .expect("a path");
    let bytes = fs::read(root.join(path)).expect("the file is there");
    Picture::from_png(&bytes).expect("a PNG")
}

/// `shade` of each channel, as the lighting of a block model darkens a face.
fn lit(rgb: [u8; 3], shade: f32) -> [u8; 3] {
    rgb.map(|c| (f32::from(c) * shade).round() as u8)
}

fn assert_pixel(picture: &Picture, (x, y): (u32, u32), rgb: [u8; 3], what: &str) {
    let [r, g, b, a] = picture.get(x, y);
    let near = |got: u8, want: u8| got.abs_diff(want) <= 4;
    assert!(
        a == 255 && near(r, rgb[0]) && near(g, rgb[1]) && near(b, rgb[2]),
        "{what} at {x},{y}: {:?}, wanted {rgb:?}",
        [r, g, b, a]
    );
}

/// The pixel is `rgb` under some shade, for a light whose shades are not the test's business.
fn assert_hue(picture: &Picture, (x, y): (u32, u32), rgb: [u8; 3], what: &str) {
    let [r, g, b, a] = picture.get(x, y);
    let peak = |rgb: [u8; 3]| f32::from(rgb.into_iter().max().unwrap());
    let shade = peak([r, g, b]) / peak(rgb);
    let want = lit(rgb, shade);
    assert!(
        a == 255
            && shade > 0.3
            && [r, g, b]
                .iter()
                .zip(want)
                .all(|(got, want)| got.abs_diff(want) <= 4),
        "{what} at {x},{y}: {:?}, wanted {rgb:?} under some shade",
        [r, g, b, a]
    );
}

fn assert_empty(picture: &Picture, (x, y): (u32, u32), what: &str) {
    assert_eq!(picture.get(x, y)[3], 0, "{what} at {x},{y} is not empty");
}

#[test]
fn a_known_model_renders_the_expected_pixels_from_each_side() {
    let dir = project();
    let mut session = Session::serve(dir.path(), &[]);
    session.ask(&legacy(
        1,
        "initialize",
        json!({"protocolVersion": "2025-11-25"}),
    ));
    let mut id = 1;
    let mut view = |session: &mut Session, camera: &str| {
        id += 1;
        let reply = session.ask(&call(
            id,
            "render_view",
            json!({"model": "block.bbmodel", "camera": camera, "size": 64, "background": "transparent"}),
        ));
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        if reply["result"]["isError"] == true && !graphics(&reply) {
            return None;
        }
        assert!(reply["result"].get("isError").is_none(), "{text}");
        let content = reply["result"]["content"].as_array().unwrap();
        assert_eq!(content[1]["type"], "image", "{text}");
        assert_eq!(content[1]["mimeType"], "image/png");
        let picture = saved_picture(dir.path(), &reply["result"]);
        assert_eq!((picture.width, picture.height), (64, 64));
        let bounds = reply["result"]["structuredContent"]["bounds"]
            .as_array()
            .map(|b| {
                b.iter()
                    .map(|n| u32::try_from(n.as_u64().unwrap()).unwrap())
                    .collect::<Vec<_>>()
            });
        Some((picture, bounds.expect("the model covers pixels")))
    };

    // From the north the east is on the left: the marker sits at the bottom left of the block,
    // whose north face shows.
    let Some((front, b)) = view(&mut session, "front") else {
        return;
    };
    let north = FACES[0].1;
    assert_pixel(
        &front,
        (b[0] + 2, b[3] - 3),
        lit(MARKER, 0.8),
        "the marker, front",
    );
    assert_empty(&front, (b[0] + 2, b[1] + 3), "above the marker, front");
    assert_pixel(
        &front,
        (b[2] - 4, b[3] - 4),
        lit(north, 0.8),
        "the north face",
    );
    assert_pixel(
        &front,
        (b[2] - 4, b[1] + 4),
        lit(north, 0.8),
        "the north face, up",
    );
    assert_empty(&front, (0, 0), "the corner, front");

    // From above the north is up and the east is on the right: the marker is at the top right.
    let (top, b) = view(&mut session, "top").unwrap();
    assert_pixel(
        &top,
        (b[2] - 3, b[1] + 3),
        lit(MARKER, 1.0),
        "the marker, top",
    );
    assert_empty(&top, (b[2] - 3, b[3] - 3), "south of the marker, top");
    assert_pixel(
        &top,
        (b[0] + 4, b[3] - 4),
        lit(FACES[4].1, 1.0),
        "the up face",
    );

    // From the east the north is on the right: the marker is at the bottom right.
    let (right, b) = view(&mut session, "right").unwrap();
    assert_pixel(
        &right,
        (b[2] - 3, b[3] - 3),
        lit(MARKER, 0.6),
        "the marker, right",
    );
    assert_pixel(
        &right,
        (b[0] + 4, b[1] + 4),
        lit(FACES[1].1, 0.6),
        "the east face",
    );
    assert_empty(&right, (b[0] + 3, b[1] - 3), "above the block, right");

    // The back, bottom and left show the south, down and west faces.
    let (back, b) = view(&mut session, "back").unwrap();
    assert_pixel(
        &back,
        (b[0] + 4, b[1] + 4),
        lit(FACES[2].1, 0.8),
        "the south face",
    );
    let (bottom, b) = view(&mut session, "bottom").unwrap();
    assert_pixel(
        &bottom,
        (b[0] + 4, b[1] + 4),
        lit(FACES[5].1, 0.5),
        "the down face",
    );
    let (left, b) = view(&mut session, "left").unwrap();
    assert_pixel(
        &left,
        (b[0] + 4, b[1] + 4),
        lit(FACES[3].1, 0.6),
        "the west face",
    );

    // The model is framed on its box, not on each view: the four sides show its 16 units of
    // height at one scale.
    let heights: Vec<u32> = [&front, &back, &left, &right]
        .iter()
        .map(|picture| {
            let rows = picture.coverage().0.unwrap();
            rows[3] - rows[1]
        })
        .collect();
    assert!(
        heights
            .windows(2)
            .all(|pair| pair[0].abs_diff(pair[1]) <= 1),
        "{heights:?}"
    );

    // The corner view looks from above at the corner of the north and east faces: the north face
    // is right of the middle, the east face left of it and the top face above.
    let (iso, b) = view(&mut session, "iso").unwrap();
    let (middle, top) = ((b[0] + b[2]) / 2, b[1]);
    assert_pixel(
        &iso,
        (middle + 9, top + 26),
        lit(FACES[0].1, 0.8),
        "the north face, iso",
    );
    assert_pixel(
        &iso,
        (middle - 9, top + 26),
        lit(FACES[1].1, 0.6),
        "the east face, iso",
    );
    assert_pixel(
        &iso,
        (middle, top + 10),
        lit(FACES[4].1, 1.0),
        "the up face, iso",
    );

    // The inventory view is something posed by the slot.
    let gui = session.ask(&call(
        99,
        "render_view",
        json!({"model": "block.bbmodel", "camera": "gui", "size": 64}),
    ));
    assert_eq!(
        gui["result"]["structuredContent"]["emulated"], true,
        "{gui}"
    );
    assert!(
        gui["result"]["structuredContent"]["coverage"]
            .as_f64()
            .unwrap()
            > 0.1
    );
    // With the pose the game gives a block (rotation 30, 225, 0, scale 0.625) the slot shows the
    // same corner: the slot is a fixed frame, so the block is centred and the faces keep their
    // places. The GUI light shades the faces on its own.
    let pose = json!({"op": "set_display", "context": "gui", "rotation": [30, 225, 0], "scale": [0.625, 0.625, 0.625]});
    let posed = session.ask(&call(
        100,
        "model_edit",
        json!({"model": "block.bbmodel", "ops": [pose]}),
    ));
    assert!(posed["result"].get("isError").is_none(), "{posed}");
    let (gui, _) = view(&mut session, "gui").unwrap();
    assert_hue(&gui, (46, 38), FACES[0].1, "the north face, gui");
    assert_hue(&gui, (18, 38), FACES[1].1, "the east face, gui");
    assert_hue(&gui, (32, 14), FACES[4].1, "the up face, gui");

    let finished = session.finish();
    finished.assert_clean();
    eprintln!("exit after a picture {:?}", finished.exit_after);
}
