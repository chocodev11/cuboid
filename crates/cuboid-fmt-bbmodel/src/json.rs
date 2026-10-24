//! Typed reads of the project's JSON tree. A member of another type reads as missing, like the
//! merge helpers of the editor that wrote the file.

use glam::{Vec2, Vec3};
use serde_json::Value;

/// Numbers beyond this magnitude are not geometry: they read as missing.
const LIMIT: f64 = 1.0e9;

/// A finite number of sane size.
#[expect(
    clippy::cast_possible_truncation,
    reason = "coordinates are stored as f32"
)]
pub fn num(value: &Value) -> Option<f32> {
    value
        .as_f64()
        .filter(|n| n.abs() <= LIMIT)
        .map(|n| n as f32)
}

pub fn num_at(value: &Value, key: &str) -> Option<f32> {
    value.get(key).and_then(num)
}

pub fn flag(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

pub fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

pub fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// An integer member (a rotation, an index), truncated toward zero.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is clamped to the range of i32 first"
)]
pub fn int_at(value: &Value, key: &str) -> Option<i32> {
    num_at(value, key).map(|n| n.clamp(-2.0e9, 2.0e9) as i32)
}

/// A vector whose members that are not numbers keep the value of `default`.
pub fn vec3_at(value: &Value, key: &str, default: Vec3) -> Vec3 {
    let Some(items) = value.get(key).and_then(Value::as_array) else {
        return default;
    };
    let part = |i: usize, d: f32| items.get(i).and_then(num).unwrap_or(d);
    Vec3::new(part(0, default.x), part(1, default.y), part(2, default.z))
}

pub fn vec2_at(value: &Value, key: &str, default: Vec2) -> Vec2 {
    let Some(items) = value.get(key).and_then(Value::as_array) else {
        return default;
    };
    let part = |i: usize, d: f32| items.get(i).and_then(num).unwrap_or(d);
    Vec2::new(part(0, default.x), part(1, default.y))
}

/// A number as JSON text that reads back as the same f32: the shortest decimal, not the digits of
/// the widened double.
pub fn number(v: f32) -> Value {
    if !v.is_finite() {
        return Value::from(0);
    }
    v.to_string()
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map_or(Value::from(0), Value::Number)
}

pub fn array3(v: Vec3) -> Value {
    Value::Array(vec![number(v.x), number(v.y), number(v.z)])
}
