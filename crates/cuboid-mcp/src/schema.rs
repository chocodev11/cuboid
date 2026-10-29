//! Small builders for the JSON Schema (2020-12) of the tools. Only these keywords are used: `type`,
//! `properties`, `required`, `additionalProperties`, `items`, `enum`, `const`, `oneOf`, `anyOf`,
//! `minimum`, `maximum`, `minItems`, `maxItems`, `$ref` to `$defs`, and the annotations
//! `description`, `default` and `examples`.

use serde_json::{Value, json};

/// A string.
pub fn string(description: &str) -> Value {
    json!({"type": "string", "description": description})
}

/// A whole number.
pub fn integer(description: &str) -> Value {
    json!({"type": "integer", "description": description})
}

/// A number.
pub fn number(description: &str) -> Value {
    json!({"type": "number", "description": description})
}

/// A boolean.
pub fn boolean(description: &str) -> Value {
    json!({"type": "boolean", "description": description})
}

/// Three numbers: a point, a size or angles.
pub fn vec3(description: &str) -> Value {
    json!({
        "type": "array",
        "items": {"type": "number"},
        "minItems": 3,
        "maxItems": 3,
        "description": description,
    })
}

/// A list.
pub fn array_of(items: Value) -> Value {
    json!({"type": "array", "items": items})
}

/// One of the listed words.
pub fn choice(words: &[&str], description: &str) -> Value {
    json!({"type": "string", "enum": words, "description": description})
}

/// Sets the member `key` of a JSON object that the caller built; a value that is no object is
/// left as it is.
pub fn set(value: &mut Value, key: &str, member: Value) {
    if let Value::Object(map) = value {
        map.insert(key.to_owned(), member);
    }
}

/// An object that has these properties and no others.
pub fn object(properties: Value, required: &[&str]) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        set(&mut schema, "required", json!(required));
    }
    schema
}

/// The path of a model file, which every model tool takes.
pub fn model() -> Value {
    string(
        "The model file, relative to the project folder, such as lamp.cubik. It is opened on first use.",
    )
}

/// The revision a write expects.
pub fn expected_revision() -> Value {
    string(
        "The revision the edit is based on, from an earlier result. A model edited since is E_REVISION.",
    )
}

/// Whether a write only shows what it would do.
pub fn dry_run() -> Value {
    boolean("Run it on a copy and report what would happen; the model is not changed.")
}

/// One or several selectors.
pub fn select() -> Value {
    json!({
        "description": "An id (e12, g3), a path (head/trim_l), a glob (head/**) or a list of them.",
        "oneOf": [
            {"type": "string"},
            {"type": "array", "items": {"type": "string"}, "minItems": 1},
        ],
    })
}

/// A cursor and a limit, which every list takes.
pub fn paging(properties: &mut Value, what: &str, default: usize) {
    set(
        properties,
        "cursor",
        string("The `next` of the previous page."),
    );
    set(
        properties,
        "limit",
        integer(&format!("{what} on a page (default {default}).")),
    );
}
