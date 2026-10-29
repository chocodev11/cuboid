//! The protocol: one JSON-RPC message per line over a pair of streams, in both eras of MCP.
//!
//! The current revision (2026-07-28) has no handshake: every request carries its protocol version
//! and the capabilities of the client in `_meta`, `server/discover` tells what the server is, and
//! results say `resultType`. Up to 2025-11-25 a session opens with `initialize`. A message that
//! carries the modern `_meta` is served the modern way; after an `initialize` the others are
//! served the legacy way. Only what the tools need is built: `ping` (legacy only), `tools/list`,
//! `tools/call`, the notifications that are accepted, and no progress, resources, prompts,
//! sampling, subscriptions or logging.

use std::fmt;
use std::io::{self, BufRead, Write};
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value, json};

use crate::render::Renderer;
use crate::schema::set;
use crate::server::{Options, Output, Server};
use crate::tools::{self, INSTRUCTIONS};

/// The newest revision; the one whose requests carry `_meta`.
pub const MODERN: &str = "2026-07-28";
/// The revisions the server answers to, newest first. The modern one is served statelessly, the
/// two before it after `initialize`.
pub const SUPPORTED: [&str; 3] = [MODERN, "2025-11-25", "2025-06-18"];
/// The version a legacy client gets when it asks for one the server does not know.
const LEGACY_DEFAULT: &str = "2025-11-25";
/// The longest message a line may hold.
pub const MAX_LINE: usize = 16 << 20;
/// What Windows PowerShell 5.1 puts at the start of the text it pipes.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
/// How long a client may keep the list of tools, which never changes while the server runs.
const LIST_TTL_MS: u64 = 3_600_000;

const KEY_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const KEY_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const KEY_CLIENT: &str = "io.modelcontextprotocol/clientInfo";
const KEY_SERVER: &str = "io.modelcontextprotocol/serverInfo";

/// Why [`serve`] ended in an error.
#[derive(Debug)]
pub enum ServeError {
    /// The project folder cannot be used; the text says why.
    Root(String),
    /// Reading the input or writing the output failed.
    Io(io::Error),
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServeError::Root(text) => f.write_str(text),
            ServeError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ServeError {}

impl From<io::Error> for ServeError {
    fn from(error: io::Error) -> Self {
        ServeError::Io(error)
    }
}

/// Serves the project folder `root` until the input ends: reads a message per line from `input`,
/// writes a reply per line to `output` and nothing else to it. It returns the moment the input
/// ends and waits for nothing, since on Windows Claude Code ends the process tree 300 ms after it
/// closes the input.
pub fn serve(
    root: &Path,
    options: Options,
    renderer: &mut dyn Renderer,
    mut input: impl BufRead,
    mut output: impl Write,
) -> Result<(), ServeError> {
    let mut server = Server::new(root, options, renderer).map_err(ServeError::Root)?;
    let mut connection = Connection { legacy: false };
    let mut line = Vec::new();
    let mut at_start = true;
    loop {
        let found = read_line(&mut input, &mut line)?;
        // RFC 8259 8.1 lets a parser skip a byte order mark at the start of its input.
        if std::mem::take(&mut at_start) && line.starts_with(UTF8_BOM) {
            line.drain(..UTF8_BOM.len());
        }
        let reply = match found {
            Line::End => return Ok(()),
            Line::TooLong => Some(failure(
                &Value::Null,
                -32600,
                "Invalid Request: a message is at most 16 MiB",
                None,
            )),
            Line::Text => connection.handle(&line, &mut server),
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
}

/// What a read of the input found.
enum Line {
    /// The input ended.
    End,
    /// A message, without its newline, is in the buffer.
    Text,
    /// A line over [`MAX_LINE`]; its end was skipped.
    TooLong,
}

/// Reads up to the next newline into `line`. A line over [`MAX_LINE`] is read to its end without
/// being kept.
fn read_line(input: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<Line> {
    line.clear();
    let (mut too_long, mut any) = (false, false);
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok(match (too_long, any) {
                (true, _) => Line::TooLong,
                (false, true) => Line::Text,
                (false, false) => Line::End,
            });
        }
        any = true;
        let newline = chunk.iter().position(|&byte| byte == b'\n');
        let (part, _) = chunk.split_at(newline.unwrap_or(chunk.len()));
        if !too_long && line.len() + part.len() > MAX_LINE {
            too_long = true;
            line.clear();
        }
        if !too_long {
            line.extend_from_slice(part);
        }
        let used = newline.map_or(part.len(), |at| at + 1);
        input.consume(used);
        if newline.is_some() {
            return Ok(if too_long { Line::TooLong } else { Line::Text });
        }
    }
}

fn reply(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn failure(id: &Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        set(&mut error, "data", data);
    }
    json!({"jsonrpc": "2.0", "id": id, "error": error})
}

/// The era a request is served in.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Era {
    Modern,
    Legacy,
}

/// What the server knows about the client at the other end of the streams.
struct Connection {
    /// An `initialize` was answered.
    legacy: bool,
}

/// The `_meta` of a request, when it is an object.
fn meta_of(params: &Value) -> Option<&Map<String, Value>> {
    params.get("_meta").and_then(Value::as_object)
}

impl Connection {
    /// The reply to one line, `None` for what gets none.
    fn handle(&mut self, line: &[u8], server: &mut Server<'_>) -> Option<Value> {
        // A blank line is no message.
        if line.trim_ascii().is_empty() {
            return None;
        }
        let Ok(message) = serde_json::from_slice::<Value>(line) else {
            return Some(failure(&Value::Null, -32700, "Parse error", None));
        };
        let Value::Object(message) = message else {
            return Some(failure(
                &Value::Null,
                -32600,
                "Invalid Request: a message is one JSON object, not a batch",
                None,
            ));
        };
        let id = message.get("id");
        let valid_id = id.filter(|id| id.is_string() || id.is_i64() || id.is_u64());
        let invalid =
            |why: &str| Some(failure(valid_id.unwrap_or(&Value::Null), -32600, why, None));
        if message.get("jsonrpc") != Some(&json!("2.0")) {
            return invalid("Invalid Request: jsonrpc must be \"2.0\"");
        }
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response: clients send none, and there is nobody to answer.
            let answer = message.contains_key("result") || message.contains_key("error");
            return if answer {
                None
            } else {
                invalid("Invalid Request: no method")
            };
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        // A notification has no id and gets no reply. `notifications/initialized` ends the
        // handshake of the legacy era; `notifications/cancelled` names a request that is over by
        // the time it is read, since the server answers one message before it reads the next.
        id?;
        let Some(id) = valid_id else {
            return invalid("Invalid Request: the id is a string or an integer");
        };
        if !(params.is_null() || params.is_object()) {
            return Some(failure(
                id,
                -32602,
                "Invalid params: params is an object",
                None,
            ));
        }
        Some(self.request(id, method, &params, server))
    }

    fn request(
        &mut self,
        id: &Value,
        method: &str,
        params: &Value,
        server: &mut Server<'_>,
    ) -> Value {
        let meta = meta_of(params);
        let modern = meta.is_some_and(|meta| {
            meta.contains_key(KEY_VERSION) || meta.contains_key(KEY_CAPABILITIES)
        });
        match method {
            "initialize" => return self.initialize(id, params, server),
            "ping" if !modern => return reply(id, json!({})),
            "server/discover" | "tools/list" | "tools/call" => {}
            _ => return failure(id, -32601, &format!("Method not found: {method}"), None),
        }
        let era = if modern {
            if let Err(error) = check_meta(id, meta) {
                return error;
            }
            // Nothing is taken from an earlier request: one without a name is `unknown`.
            let name = meta
                .and_then(|meta| meta.get(KEY_CLIENT))
                .and_then(|client| client.get("name"))
                .and_then(Value::as_str);
            name.unwrap_or("unknown").clone_into(&mut server.client);
            Era::Modern
        } else if self.legacy {
            Era::Legacy
        } else {
            return failure(
                id,
                -32602,
                &format!(
                    "Invalid params: _meta needs {KEY_VERSION} and {KEY_CAPABILITIES}, \
                     or the session must open with initialize"
                ),
                None,
            );
        };
        match method {
            "server/discover" => reply(id, complete(era, discover())),
            "tools/list" => {
                if params.get("cursor").is_some_and(|cursor| !cursor.is_null()) {
                    return failure(id, -32602, "Invalid params: the list has one page", None);
                }
                let mut list = json!({"tools": tools::definitions()});
                if era == Era::Modern {
                    set(&mut list, "ttlMs", json!(LIST_TTL_MS));
                    set(&mut list, "cacheScope", json!("public"));
                }
                reply(id, complete(era, list))
            }
            _ => self.call(id, params, era, server),
        }
    }

    /// The legacy handshake: the version is the client's when the server knows it.
    fn initialize(&mut self, id: &Value, params: &Value, server: &mut Server<'_>) -> Value {
        let Some(asked) = params.get("protocolVersion").and_then(Value::as_str) else {
            return failure(
                id,
                -32602,
                "Invalid params: protocolVersion is a string",
                None,
            );
        };
        let name = params
            .get("clientInfo")
            .and_then(|client| client.get("name"))
            .and_then(Value::as_str);
        name.unwrap_or("unknown").clone_into(&mut server.client);
        self.legacy = true;
        let version = SUPPORTED
            .iter()
            .skip(1)
            .find(|known| **known == asked)
            .unwrap_or(&LEGACY_DEFAULT);
        reply(
            id,
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {}},
                "serverInfo": server_info(),
                "instructions": INSTRUCTIONS,
            }),
        )
    }

    fn call(&mut self, id: &Value, params: &Value, era: Era, server: &mut Server<'_>) -> Value {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return failure(
                id,
                -32602,
                "Invalid params: name is the name of a tool",
                None,
            );
        };
        let arguments = match params.get("arguments") {
            None | Some(Value::Null) => json!({}),
            Some(arguments @ Value::Object(_)) => arguments.clone(),
            Some(_) => return failure(id, -32602, "Invalid params: arguments is an object", None),
        };
        if !tools::exists(name) {
            return failure(id, -32602, &format!("Unknown tool: {name}"), None);
        }
        // A panic in a tool is an error result and the server keeps serving; the model that a
        // changing call may have left half applied is closed.
        let model = arguments
            .get("model")
            .and_then(Value::as_str)
            .filter(|_| tools::changes_model(name, &arguments))
            .map(str::to_owned);
        let called = panic::catch_unwind(AssertUnwindSafe(|| server.call(name, arguments)));
        let outcome = called.unwrap_or_else(|payload| {
            Err(server.panicked(name, model.as_deref(), payload.as_ref()))
        });
        let result = match outcome {
            Ok(output) => tool_result(&output),
            Err(error) => json!({
                "content": [{"type": "text", "text": error.to_json().to_string()}],
                "isError": true,
            }),
        };
        reply(id, complete(era, result))
    }
}

/// Checks the fields every modern request carries.
fn check_meta(id: &Value, meta: Option<&Map<String, Value>>) -> Result<(), Value> {
    let get = |key: &str| meta.and_then(|meta| meta.get(key));
    let (Some(version), Some(capabilities)) = (get(KEY_VERSION), get(KEY_CAPABILITIES)) else {
        return Err(failure(
            id,
            -32602,
            &format!("Invalid params: _meta needs {KEY_VERSION} and {KEY_CAPABILITIES}"),
            None,
        ));
    };
    let (Some(version), true) = (version.as_str(), capabilities.is_object()) else {
        return Err(failure(
            id,
            -32602,
            "Invalid params: the version is a string and the capabilities an object",
            None,
        ));
    };
    if !SUPPORTED.contains(&version) {
        return Err(failure(
            id,
            -32022,
            "Unsupported protocol version",
            Some(json!({"supported": SUPPORTED, "requested": version})),
        ));
    }
    Ok(())
}

fn server_info() -> Value {
    json!({"name": "cuboid", "version": env!("CARGO_PKG_VERSION")})
}

/// The result of `server/discover`.
fn discover() -> Value {
    json!({
        "supportedVersions": SUPPORTED,
        "capabilities": {"tools": {}},
        "instructions": INSTRUCTIONS,
        "ttlMs": LIST_TTL_MS,
        "cacheScope": "public",
    })
}

/// A result in the form of the era: the modern one says `resultType` first and names the server
/// in `_meta`.
fn complete(era: Era, fields: Value) -> Value {
    if era == Era::Legacy {
        return fields;
    }
    let mut result = Map::new();
    result.insert("resultType".to_owned(), json!("complete"));
    if let Value::Object(fields) = fields {
        result.extend(fields);
    }
    result.insert("_meta".to_owned(), json!({KEY_SERVER: server_info()}));
    Value::Object(result)
}

/// A successful tool call: the result as structured content and as text for clients that read
/// only that, then the pictures.
fn tool_result(output: &Output) -> Value {
    let mut content = vec![json!({"type": "text", "text": output.value.to_string()})];
    content.extend(output.images.iter().map(
        |png| json!({"type": "image", "data": STANDARD.encode(png), "mimeType": "image/png"}),
    ));
    json!({"content": content, "structuredContent": output.value})
}
