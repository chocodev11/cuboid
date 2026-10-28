//! The protocol in both eras: transcripts of request and reply lines, compared exactly, and the
//! messages that are refused.

mod common;

use std::fs;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use common::{modern, project, run, transcript};
use cuboid_mcp::Options;
use cuboid_mcp::protocol::MAX_LINE;
use cuboid_mcp::tools::{INSTRUCTIONS, definitions};
use serde_json::{Value, json};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn server_info() -> String {
    format!(r#"{{"name":"cuboid","version":"{VERSION}"}}"#)
}

fn quoted(text: &str) -> String {
    Value::String(text.to_owned()).to_string()
}

#[test]
fn the_modern_era_discovers_lists_and_answers_without_a_handshake() {
    let dir = tempfile::tempdir().unwrap();
    let out = transcript(
        dir.path(),
        &[
            &modern(1, "server/discover", json!({})),
            &modern(2, "tools/list", json!({})),
        ],
    );
    let discover = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"complete","supportedVersions":["2026-07-28","2025-11-25","2025-06-18"],"capabilities":{{"tools":{{}}}},"instructions":{},"ttlMs":3600000,"cacheScope":"public","_meta":{{"io.modelcontextprotocol/serverInfo":{}}}}}}}"#,
        quoted(INSTRUCTIONS),
        server_info()
    );
    assert_eq!(out[0], discover);
    let list = json!({"tools": definitions()}).to_string();
    let list = list
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap();
    let list_line = format!(
        r#"{{"jsonrpc":"2.0","id":2,"result":{{"resultType":"complete",{list},"ttlMs":3600000,"cacheScope":"public","_meta":{{"io.modelcontextprotocol/serverInfo":{}}}}}}}"#,
        server_info()
    );
    assert_eq!(out[1], list_line);
    assert_eq!(out.len(), 2);
}

#[test]
fn the_legacy_era_opens_with_initialize_and_has_no_result_type() {
    let dir = tempfile::tempdir().unwrap();
    let initialize = |id: u64, version: &str| {
        json!({"jsonrpc": "2.0", "id": id, "method": "initialize", "params": {
            "protocolVersion": version, "capabilities": {}, "clientInfo": {"name": "old", "version": "9"}}})
        .to_string()
    };
    let out = transcript(
        dir.path(),
        &[
            &initialize(1, "2025-11-25"),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}"#,
            &initialize(4, "2025-06-18"),
            &initialize(5, "2024-11-05"),
        ],
    );
    let ready = |id: u64, version: &str| {
        format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocolVersion":"{version}","capabilities":{{"tools":{{}}}},"serverInfo":{},"instructions":{}}}}}"#,
            server_info(),
            quoted(INSTRUCTIONS)
        )
    };
    assert_eq!(out[0], ready(1, "2025-11-25"));
    assert_eq!(out[1], r#"{"jsonrpc":"2.0","id":2,"result":{}}"#);
    assert_eq!(
        out[2],
        json!({"jsonrpc": "2.0", "id": 3, "result": {"tools": definitions()}}).to_string()
    );
    assert_eq!(out[3], ready(4, "2025-06-18"));
    assert_eq!(out[4], ready(5, "2025-11-25"));
    assert_eq!(out.len(), 5, "notifications get no reply");
}

#[test]
fn a_legacy_tool_call_answers_with_structured_content_and_the_same_text() {
    let dir = project();
    let out = transcript(
        dir.path(),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"project_list","arguments":{}}}"#,
        ],
    );
    let reply: Value = serde_json::from_str(&out[1]).unwrap();
    let result = &reply["result"];
    assert!(result.get("resultType").is_none() && result.get("isError").is_none());
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, result["structuredContent"]);
    assert_eq!(text["files"][0]["path"], "lamp.bbmodel");
}

#[test]
fn a_modern_tool_call_names_the_server_and_a_picture_is_png_image_content() {
    let dir = project();
    let out = transcript(
        dir.path(),
        &[&modern(
            7,
            "tools/call",
            json!({"name": "render_view", "arguments": {"model": "lamp.bbmodel", "camera": "front", "size": 64}}),
        )],
    );
    let reply: Value = serde_json::from_str(&out[0]).unwrap();
    let result = &reply["result"];
    assert_eq!(result["resultType"], "complete");
    assert_eq!(
        result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "cuboid"
    );
    assert_eq!(result["content"][1]["type"], "image");
    assert_eq!(result["content"][1]["mimeType"], "image/png");
    let png = STANDARD
        .decode(result["content"][1]["data"].as_str().unwrap())
        .unwrap();
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    assert_eq!(result["structuredContent"]["camera"], "front");
    let path = result["structuredContent"]["path"].as_str().unwrap();
    assert!(
        path.starts_with("renders/lamp-front-r") && path.ends_with(".png"),
        "{path}"
    );
    assert!(dir.path().join(path).is_file());
}

#[test]
fn a_tool_that_fails_is_a_result_with_is_error_and_a_code() {
    let dir = project();
    let out = transcript(
        dir.path(),
        &[&modern(
            1,
            "tools/call",
            json!({"name": "model_outline", "arguments": {"model": "../outside.cubik"}}),
        )],
    );
    let reply: Value = serde_json::from_str(&out[0]).unwrap();
    assert_eq!(reply["result"]["isError"], true);
    assert_eq!(reply["result"]["resultType"], "complete");
    let error: Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(error["code"], "E_PATH");
    assert!(error["message"].is_string() && error["hint"].is_string());
}

#[test]
fn a_request_without_the_modern_meta_or_an_initialize_is_invalid_params() {
    let dir = tempfile::tempdir().unwrap();
    let out = transcript(
        dir.path(),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"brief_get"}}"#,
        ],
    );
    let both =
        "io.modelcontextprotocol/protocolVersion and io.modelcontextprotocol/clientCapabilities";
    assert_eq!(
        out[0],
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":-32602,"message":"Invalid params: _meta needs {both}, or the session must open with initialize"}}}}"#
        )
    );
    assert_eq!(
        out[1],
        format!(
            r#"{{"jsonrpc":"2.0","id":2,"error":{{"code":-32602,"message":"Invalid params: _meta needs {both}"}}}}"#
        )
    );
    assert!(out[2].contains("-32602"));
}

#[test]
fn an_unknown_version_is_32022_with_the_versions_the_server_has() {
    let dir = tempfile::tempdir().unwrap();
    let line = r#"{"jsonrpc":"2.0","id":9,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01","io.modelcontextprotocol/clientCapabilities":{}}}}"#;
    let out = transcript(dir.path(), &[line]);
    assert_eq!(
        out[0],
        r#"{"jsonrpc":"2.0","id":9,"error":{"code":-32022,"message":"Unsupported protocol version","data":{"supported":["2026-07-28","2025-11-25","2025-06-18"],"requested":"1900-01-01"}}}"#
    );
}

#[test]
fn a_version_that_is_not_a_string_or_capabilities_that_are_not_an_object_are_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let out = transcript(
        dir.path(),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":20260728,"io.modelcontextprotocol/clientCapabilities":{}}}}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":[]}}}"#,
        ],
    );
    for line in &out {
        assert!(line.contains(r#""code":-32602"#), "{line}");
    }
}

#[test]
fn a_json_array_a_bad_line_and_a_bad_envelope_get_their_codes() {
    let dir = tempfile::tempdir().unwrap();
    let out = transcript(
        dir.path(),
        &[
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"ping""#,
            r#"42"#,
            r#"{"jsonrpc":"1.0","id":3,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":{"a":1},"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":4}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/list","params":[]}"#,
            r#"{"jsonrpc":"2.0","id":6,"method":"resources/list"}"#,
            r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#,
            "",
        ],
    );
    let expected = [
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request: a message is one JSON object, not a batch"}}"#,
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}"#,
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request: a message is one JSON object, not a batch"}}"#,
        r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32600,"message":"Invalid Request: jsonrpc must be \"2.0\""}}"#,
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request: the id is a string or an integer"}}"#,
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request: the id is a string or an integer"}}"#,
        r#"{"jsonrpc":"2.0","id":4,"error":{"code":-32600,"message":"Invalid Request: no method"}}"#,
        r#"{"jsonrpc":"2.0","id":5,"error":{"code":-32602,"message":"Invalid params: params is an object"}}"#,
        r#"{"jsonrpc":"2.0","id":6,"error":{"code":-32601,"message":"Method not found: resources/list"}}"#,
        r#"{"jsonrpc":"2.0","id":7,"result":{}}"#,
    ];
    assert_eq!(out, expected);
}

#[test]
fn ping_is_a_legacy_message_and_the_modern_era_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let out = transcript(
        dir.path(),
        &[
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            &modern(2, "ping", json!({})),
        ],
    );
    assert_eq!(out[0], r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
    assert_eq!(
        out[1],
        r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"Method not found: ping"}}"#
    );
}

#[test]
fn an_unknown_tool_and_bad_call_params_are_protocol_errors() {
    let dir = tempfile::tempdir().unwrap();
    let out = transcript(
        dir.path(),
        &[
            &modern(1, "tools/call", json!({"name": "eval"})),
            &modern(2, "tools/call", json!({"arguments": {}})),
            &modern(
                3,
                "tools/call",
                json!({"name": "brief_get", "arguments": [1]}),
            ),
            &modern(4, "tools/list", json!({"cursor": "x"})),
        ],
    );
    assert_eq!(
        out[0],
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"Unknown tool: eval"}}"#
    );
    for line in &out[1..] {
        assert!(line.contains(r#""code":-32602"#), "{line}");
    }
}

#[test]
fn a_line_over_16_mib_is_refused_and_skipped_and_the_next_one_is_served() {
    let dir = tempfile::tempdir().unwrap();
    let mut input = Vec::new();
    input.extend_from_slice(br#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"pad":""#);
    input.resize(MAX_LINE + 10, b'x');
    input.extend_from_slice(b"\"}}\n");
    input.extend_from_slice(br#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#);
    input.push(b'\n');
    let out = run(dir.path(), &input, Options::default());
    assert_eq!(
        out,
        [
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request: a message is at most 16 MiB"}}"#,
            r#"{"jsonrpc":"2.0","id":2,"result":{}}"#,
        ]
    );
}

#[test]
fn the_last_line_needs_no_newline_and_crlf_is_fine() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\r\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}",
        Options::default(),
    );
    assert_eq!(out.len(), 2);
}

#[test]
fn a_byte_order_mark_is_skipped_on_the_first_line_only() {
    // Windows PowerShell 5.1 starts the text it pipes with one.
    let dir = tempfile::tempdir().unwrap();
    let mut input = Vec::new();
    for id in 1..=2 {
        input.extend_from_slice(b"\xEF\xBB\xBF");
        input.extend_from_slice(
            format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#).as_bytes(),
        );
        input.push(b'\n');
    }
    let out = run(dir.path(), &input, Options::default());
    assert_eq!(
        out,
        [
            r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}"#,
        ]
    );
}

#[test]
fn every_line_the_server_writes_is_one_json_rpc_message() {
    let dir = project();
    let out = transcript(
        dir.path(),
        &[
            &modern(1, "server/discover", json!({})),
            &modern(
                2,
                "tools/call",
                json!({"name": "model_outline", "arguments": {"model": "lamp.bbmodel"}}),
            ),
            "not json",
            &modern(
                3,
                "tools/call",
                json!({"name": "model_edit", "arguments": {"model": "lamp.bbmodel", "ops": [{"op": "delete", "select": "nothing"}]}}),
            ),
        ],
    );
    assert_eq!(out.len(), 4);
    for line in &out {
        let message: Value = serde_json::from_str(line).expect("json");
        assert_eq!(message["jsonrpc"], "2.0");
        assert!(message.get("result").is_some() || message.get("error").is_some());
        assert!(!line.contains('\n'));
    }
}

#[test]
fn a_modern_request_may_name_a_legacy_version_the_server_lists() {
    let dir = tempfile::tempdir().unwrap();
    let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2025-11-25","io.modelcontextprotocol/clientCapabilities":{}}}}"#;
    let out = transcript(dir.path(), &[line]);
    assert!(out[0].contains(r#""resultType":"complete""#));
}

#[test]
fn the_journal_names_the_client_of_each_request_and_takes_nothing_from_an_earlier_one() {
    let create = |name: &str| json!({"name": "project_open", "arguments": {"model": name, "create": "item"}});
    let no_name = |id: u64, name: &str| {
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {
        "name": "project_open", "arguments": {"model": name, "create": "item"},
        "_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
        }}})
        .to_string()
    };
    let dir = tempfile::tempdir().unwrap();
    let out = transcript(
        dir.path(),
        &[
            &modern(1, "tools/call", create("a.cubik")),
            &no_name(2, "b.cubik"),
            &modern(3, "tools/call", create("c.cubik")),
        ],
    );
    assert!(out.iter().all(|line| !line.contains("isError")), "{out:?}");
    let clients: Vec<String> = fs::read_to_string(dir.path().join("journal.jsonl"))
        .unwrap()
        .lines()
        .map(|line| {
            let entry: Value = serde_json::from_str(line).unwrap();
            entry["client"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(clients, ["test", "unknown", "test"]);
    // A legacy session without a client name is unknown too.
    let again = tempfile::tempdir().unwrap();
    let initialize = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{}}}"#;
    let call =
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": create("d.cubik")})
            .to_string();
    transcript(again.path(), &[initialize, &call]);
    let line = fs::read_to_string(again.path().join("journal.jsonl")).unwrap();
    assert!(line.contains(r#""client":"unknown""#), "{line}");
}

#[test]
fn serve_returns_the_moment_the_input_ends() {
    // Claude Code on Windows ends the process 300 ms after it closes the input.
    let dir = tempfile::tempdir().unwrap();
    let mut fake = common::Fake::default();
    let input = format!(
        "{}\n{}",
        modern(1, "tools/list", json!({})),
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#
    );
    let mut output = Vec::new();
    let start = std::time::Instant::now();
    cuboid_mcp::serve(
        dir.path(),
        Options::default(),
        &mut fake,
        std::io::Cursor::new(input.into_bytes()),
        &mut output,
    )
    .expect("serves");
    assert!(start.elapsed() < std::time::Duration::from_millis(100));
    assert_eq!(output.iter().filter(|&&byte| byte == b'\n').count(), 2);
}

#[test]
fn a_project_folder_that_is_not_usable_ends_serve_with_a_reason() {
    let dir = project();
    let mut fake = common::Fake::default();
    let missing = dir.path().join("nope");
    let error = cuboid_mcp::serve(
        &missing,
        Options::default(),
        &mut fake,
        std::io::empty(),
        Vec::new(),
    )
    .expect_err("a missing folder");
    assert!(error.to_string().contains("not a folder"), "{error}");
    let file = dir.path().join("lamp.bbmodel");
    assert!(fs::metadata(&file).is_ok());
    assert!(
        cuboid_mcp::serve(
            &file,
            Options::default(),
            &mut fake,
            std::io::empty(),
            Vec::new()
        )
        .is_err()
    );
}
