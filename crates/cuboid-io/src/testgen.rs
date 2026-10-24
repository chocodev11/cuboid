//! Test helpers: a small deterministic generator, mutated copies of a valid input for the tests
//! that the parsers never panic on hostile input (architecture 2, principle 10), and random
//! documents for the differential tests against `serde_json`.

use crate::text::Value;

/// xorshift64.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number in `0..n`; `n` must not be 0.
    pub(crate) fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }

    /// A number in `0..n`; `n` must not be 0.
    pub(crate) fn below32(&mut self, n: u32) -> u32 {
        u32::try_from(self.next() % u64::from(n)).unwrap()
    }
}

/// `count` copies of `seed` with a few bytes changed, inserted, removed or cut off.
pub(crate) fn mutants(seed: &[u8], count: usize) -> impl Iterator<Item = Vec<u8>> + '_ {
    let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
    (0..count).map(move |_| {
        let mut data = seed.to_vec();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(data.len() + 1);
            let byte = rng.next().to_le_bytes()[0];
            match rng.below(4) {
                0 if at < data.len() => data[at] = byte,
                1 => data.insert(at, byte),
                2 if at < data.len() => {
                    data.remove(at);
                }
                3 => data.truncate(at),
                _ => {}
            }
        }
        data
    })
}

/// A random JSON value. Strings hold structural characters and non-ASCII text; only with
/// `escapes` do they also hold quotes, backslashes and control characters, which the permissive
/// dialect does not read like standard JSON.
pub(crate) fn random_json(rng: &mut Rng, depth: usize, escapes: bool) -> serde_json::Value {
    use serde_json::{Map, Number, Value as Json};
    const PLAIN: [&str; 12] = ["a", "Z", "7", " ", "{", "}", "[", "]", ",", ":", "é", "😀"];
    const TRICKY: [&str; 6] = ["\"", "\\", "\n", "\u{1}", "/", "\u{7f}"];
    let string = |rng: &mut Rng| -> String {
        (0..rng.below(8))
            .map(|_| match rng.below(if escapes { 3 } else { 2 }) {
                0 | 1 => PLAIN[rng.below(PLAIN.len())],
                _ => TRICKY[rng.below(TRICKY.len())],
            })
            .collect()
    };
    match rng.below(if depth >= 4 { 5 } else { 7 }) {
        0 => Json::Null,
        1 => Json::Bool(rng.below(2) == 1),
        2 => Json::from(i64::from(rng.below32(2_000_000_000)) - 1_000_000_000),
        3 => Number::from_f64(f64::from(rng.below32(1_000_000)) / 64.0 - 5000.0)
            .map_or(Json::Null, Json::Number),
        4 => Json::String(string(rng)),
        5 => Json::Array(
            (0..rng.below(5))
                .map(|_| random_json(rng, depth + 1, escapes))
                .collect(),
        ),
        _ => {
            let mut members = Map::new();
            for _ in 0..rng.below(5) {
                members.insert(string(rng), random_json(rng, depth + 1, escapes));
            }
            Json::Object(members)
        }
    }
}

/// The same tree as a reader value; every number is a double.
pub(crate) fn reader_value(json: &serde_json::Value) -> Value {
    use serde_json::Value as Json;
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => Value::Number(n.as_f64().unwrap()),
        Json::String(s) => Value::String(s.clone()),
        Json::Array(items) => Value::Array(items.iter().map(reader_value).collect()),
        Json::Object(members) => Value::Object(Box::new(
            members
                .iter()
                .map(|(key, item)| (key.clone(), reader_value(item)))
                .collect(),
        )),
    }
}

/// A document with `value` as its only member, the shape both readers need at the root.
pub(crate) fn rooted(value: serde_json::Value) -> serde_json::Value {
    let mut root = serde_json::Map::new();
    root.insert("root".to_owned(), value);
    serde_json::Value::Object(root)
}

#[test]
fn mutants_differ_from_the_seed_and_repeat() {
    let seed = b"{\"a\":[1,2,3]}";
    let first: Vec<_> = mutants(seed, 50).collect();
    assert!(first.iter().any(|data| data != seed));
    assert_eq!(first, mutants(seed, 50).collect::<Vec<_>>());
}
