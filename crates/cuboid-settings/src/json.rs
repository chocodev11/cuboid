//! Reading one settings value from JSON with the leniency of app 10.1.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// A settings value readable from JSON. `None` means the value is unusable for the type.
pub(crate) trait Json: DeserializeOwned + Clone {
    fn from_json(value: &Value) -> Option<Self> {
        Self::deserialize(value).ok()
    }
}

macro_rules! strict {
    ($($ty:ty),* $(,)?) => { $(impl Json for $ty {})* };
}

strict!(
    u32,
    String,
    Option<String>,
    Option<f32>,
    BTreeMap<String, String>,
    BTreeMap<String, bool>,
);

/// A boolean, or the text `true` or `false`.
///
/// spec: app 10.1 (conversion of strings is UNVERIFIED for booleans)
impl Json for bool {
    fn from_json(value: &Value) -> Option<Self> {
        match value {
            Value::Bool(flag) => Some(*flag),
            Value::String(text) if text.eq_ignore_ascii_case("true") => Some(true),
            Value::String(text) if text.eq_ignore_ascii_case("false") => Some(false),
            _ => None,
        }
    }
}

/// An integer in the i32 range, or a number written as text ("numbers written as strings are
/// accepted"). A fraction is unusable.
///
/// spec: app 10.1
impl Json for i32 {
    fn from_json(value: &Value) -> Option<Self> {
        match value {
            Value::Number(number) => number.as_i64().and_then(|n| i32::try_from(n).ok()),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        }
    }
}

/// Moves `key` out of `map` into `slot`. A value that is unusable for the type takes `fallback`
/// and is logged; the value itself is not, because it may be a token.
///
/// spec: app 0.2 D10
pub(crate) fn take<T: Json>(map: &mut Map<String, Value>, key: &str, slot: &mut T, fallback: &T) {
    let Some(value) = map.shift_remove(key) else {
        return;
    };
    *slot = T::from_json(&value).unwrap_or_else(|| {
        log::warn!("settings: unusable value for `{key}`, using the default");
        fallback.clone()
    });
}
