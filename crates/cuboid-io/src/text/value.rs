//! The value tree the JSON readers build, with the typed lookups of native 9.4.

use indexmap::IndexMap;

/// A parsed JSON value (32 bytes: an object is boxed, as a model file can hold millions of
/// small arrays). Objects keep the document order of their keys; a repeated key keeps
/// its first position and takes the last value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// Every number is a double; integer readers truncate.
    Number(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Value>),
    /// An object.
    Object(Box<IndexMap<String, Value>>),
}

/// The typed lookups of the importers: a value of another type reads as 0 (numbers), false
/// (booleans) or nothing (strings and containers).
///
/// spec: native 9.4
impl Value {
    /// The member of an object, if this is an object that has the key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(members) => members.get(key),
            _ => None,
        }
    }

    /// A number converted to f32, or 0 for any other type.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "float readers convert the double to f32 (native 9.4)"
    )]
    pub fn as_f32(&self) -> f32 {
        match self {
            Value::Number(n) => *n as f32,
            _ => 0.0,
        }
    }

    /// A number truncated toward zero, or 0 for any other type. Out-of-range values saturate
    /// (the original's cast is unspecified there).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "integer readers truncate the double toward zero (native 9.4)"
    )]
    pub fn as_i32(&self) -> i32 {
        match self {
            Value::Number(n) => *n as i32,
            _ => 0,
        }
    }

    /// A boolean, or false for any other type.
    pub fn as_bool(&self) -> bool {
        matches!(self, Value::Bool(true))
    }

    /// A string; `None` for any other type, which the importers treat as a null string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The items of an array.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The members of an object, in document order.
    pub fn as_object(&self) -> Option<&IndexMap<String, Value>> {
        match self {
            Value::Object(members) => Some(members),
            _ => None,
        }
    }

    /// A Vector3: an array of at least three items, read as numbers. Anything else gives
    /// `None`, and the importer takes the documented default.
    pub fn vec3(&self) -> Option<[f32; 3]> {
        match self.as_array()? {
            [x, y, z, ..] => Some([x.as_f32(), y.as_f32(), z.as_f32()]),
            _ => None,
        }
    }

    /// A Vector4 (a colour): an array of at least four items, read as numbers.
    pub fn vec4(&self) -> Option<[f32; 4]> {
        match self.as_array()? {
            [x, y, z, w, ..] => Some([x.as_f32(), y.as_f32(), z.as_f32(), w.as_f32()]),
            _ => None,
        }
    }
}
