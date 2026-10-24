//! The permissive JSON dialect of the Minecraft JSON and `.cub` importers.
//!
//! It differs from standard JSON and the port must accept and reject the same text: only
//! `\uXXXX` escapes are decoded, a string ends at the first `"` not directly preceded by `\`,
//! a number token runs to the next `,`, `]` or `}`, literals are recognised by their first
//! letter, a trailing comma is accepted and the last of repeated keys wins.
//!
//! spec: native 9.4, interop 8.6

use indexmap::IndexMap;
use thiserror::Error;

use super::{MAX_DEPTH, Value, hex4, parse_double};

/// A syntax error. The reference parser runs silently, so its message is always the same.
///
/// spec: native 9.4, interop 8.1
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("Invalid data found")]
pub struct SyntaxError;

/// Parses a document. The root must be an object; anything after it is ignored.
///
/// Characters that are not part of the dialect's syntax, such as comments, single quotes or a
/// leading comma, are errors. An empty text is a syntax error (the reference dereferences null).
///
/// spec: native 9.4, interop 8.6
pub fn parse(text: &str) -> Result<Value, SyntaxError> {
    let mut parser = Parser {
        text,
        bytes: text.as_bytes(),
        pos: 0,
    };
    parser.skip_white();
    parser.expect(b'{')?;
    parser.object(1)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    /// Always on a character boundary: the parser steps over ASCII bytes, white space and
    /// whole strings.
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        self.pos += usize::from(found);
        found
    }

    fn expect(&mut self, byte: u8) -> Result<(), SyntaxError> {
        if self.eat(byte) {
            Ok(())
        } else {
            Err(SyntaxError)
        }
    }

    /// Skips Unicode white space.
    fn skip_white(&mut self) {
        let rest = self.text.get(self.pos..).unwrap_or_default();
        self.pos += rest.len() - rest.trim_start().len();
    }

    fn value(&mut self, depth: usize) -> Result<Value, SyntaxError> {
        self.skip_white();
        match self.peek().ok_or(SyntaxError)? {
            b'{' => {
                self.pos += 1;
                self.object(depth + 1)
            }
            b'[' => {
                self.pos += 1;
                self.array(depth + 1)
            }
            b'"' => self.string().map(Value::String),
            b'-' | b'0'..=b'9' => self.number(),
            b't' => self.word("true", Value::Bool(true)),
            b'f' => self.word("false", Value::Bool(false)),
            b'n' => self.word("null", Value::Null),
            _ => Err(SyntaxError),
        }
    }

    /// The members after the opening brace.
    fn object(&mut self, depth: usize) -> Result<Value, SyntaxError> {
        if depth > MAX_DEPTH {
            return Err(SyntaxError);
        }
        let mut members = IndexMap::new();
        loop {
            self.skip_white();
            if self.eat(b'}') {
                break;
            }
            if self.peek() != Some(b'"') {
                return Err(SyntaxError);
            }
            let key = self.string()?;
            self.skip_white();
            self.expect(b':')?;
            let value = self.value(depth)?;
            members.insert(key, value);
            self.skip_white();
            if !self.eat(b',') {
                self.expect(b'}')?;
                break;
            }
        }
        Ok(Value::Object(Box::new(members)))
    }

    /// The items after the opening bracket.
    fn array(&mut self, depth: usize) -> Result<Value, SyntaxError> {
        if depth > MAX_DEPTH {
            return Err(SyntaxError);
        }
        let mut items = Vec::new();
        loop {
            self.skip_white();
            if self.eat(b']') {
                break;
            }
            items.push(self.value(depth)?);
            self.skip_white();
            if !self.eat(b',') {
                self.expect(b']')?;
                break;
            }
        }
        Ok(Value::Array(items))
    }

    /// A string from its opening quote. It ends at the first `"` whose previous character is
    /// not a backslash, so a content that ends with a backslash swallows the quote.
    fn string(&mut self) -> Result<String, SyntaxError> {
        let start = self.pos + 1;
        let body = self.bytes.get(start..).ok_or(SyntaxError)?;
        let mut prev = b'"';
        let len = body
            .iter()
            .position(|&b| {
                let end = b == b'"' && prev != b'\\';
                prev = b;
                end
            })
            .ok_or(SyntaxError)?;
        let raw = self.text.get(start..start + len).ok_or(SyntaxError)?;
        self.pos = start + len + 1;
        Ok(unescape(raw))
    }

    /// A number token: from the current character to the next `,`, `]` or `}`.
    fn number(&mut self) -> Result<Value, SyntaxError> {
        let body = self.bytes.get(self.pos..).ok_or(SyntaxError)?;
        let len = body
            .iter()
            .position(|b| matches!(b, b',' | b']' | b'}'))
            .ok_or(SyntaxError)?;
        let token = self.text.get(self.pos..self.pos + len).ok_or(SyntaxError)?;
        let number = parse_double(token).ok_or(SyntaxError)?;
        self.pos += len;
        Ok(Value::Number(number))
    }

    /// A literal: its first letter selects it and the full word must follow.
    fn word(&mut self, word: &str, value: Value) -> Result<Value, SyntaxError> {
        let rest = self.text.get(self.pos..).unwrap_or_default();
        if !rest.starts_with(word) {
            return Err(SyntaxError);
        }
        self.pos += word.len();
        Ok(value)
    }
}

/// Replaces every `\uXXXX` of the delimited content by its UTF-16 code unit and keeps every
/// other character, backslashes included. The scan runs from the left, so `\\u0041` is a
/// backslash followed by `A`. A lone surrogate becomes U+FFFD, as a Rust string cannot hold
/// one.
fn unescape(raw: &str) -> String {
    if !raw.contains("\\u") {
        return raw.to_owned();
    }
    let mut units: Vec<u16> = Vec::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find("\\u") {
        let (head, tail) = rest.split_at(at);
        units.extend(head.encode_utf16());
        match tail.get(2..6).and_then(hex4) {
            Some(unit) => {
                units.push(unit);
                rest = tail.get(6..).unwrap_or_default();
            }
            None => {
                units.push(u16::from(b'\\'));
                rest = tail.get(1..).unwrap_or_default();
            }
        }
    }
    units.extend(rest.encode_utf16());
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests;
