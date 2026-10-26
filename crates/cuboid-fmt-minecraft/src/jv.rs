//! Typed reads of a JSON tree. A member of another type reads as missing.

use glam::Vec3;
use serde_json::Value;

/// Numbers beyond this magnitude are not geometry: they read as missing.
const LIMIT: f64 = 1.0e9;

/// A finite number of sane size.
#[expect(
    clippy::cast_possible_truncation,
    reason = "coordinates are stored as f32"
)]
pub(crate) fn num(value: &Value) -> Option<f32> {
    value
        .as_f64()
        .filter(|n| n.abs() <= LIMIT)
        .map(|n| n as f32)
}

pub(crate) fn num_at(value: &Value, key: &str) -> Option<f32> {
    value.get(key).and_then(num)
}

pub(crate) fn flag(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

pub(crate) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

pub(crate) fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// An integer value, truncated toward zero.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is clamped to the range of i32 first"
)]
pub(crate) fn int(value: &Value) -> Option<i32> {
    num(value).map(|n| n.clamp(-2.0e9, 2.0e9) as i32)
}

/// An integer member, truncated toward zero.
pub(crate) fn int_at(value: &Value, key: &str) -> Option<i32> {
    value.get(key).and_then(int)
}

/// A vector of at least three items; an item that is not a number reads as 0. Anything else is
/// missing and the caller takes its default (interop 8.3, 8.5).
pub(crate) fn vec3_at(value: &Value, key: &str) -> Option<Vec3> {
    let items = value.get(key)?.as_array()?;
    if items.len() < 3 {
        return None;
    }
    let part = |i: usize| items.get(i).and_then(num).unwrap_or(0.0);
    Some(Vec3::new(part(0), part(1), part(2)))
}

/// The label rule of the model (tools 5.9): ASCII letters, digits, `-` and `_`.
pub(crate) fn clean_label(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

/// Whether `path` is a relative resource path that stays inside its folder: no empty or parent
/// segments, no backslash, colon or control character.
pub(crate) fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != ".." && part != ".")
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
}
