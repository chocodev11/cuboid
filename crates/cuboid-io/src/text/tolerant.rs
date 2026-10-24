//! The lenient JSON of the `.mcpe` importer: the standard tolerance of the original's JSON
//! serializer. Comments, single-quoted strings, unquoted property names and trailing commas are
//! accepted; the standard escapes are decoded. Unknown keys are the caller's business: the
//! reader keeps every member.
//!
//! spec: interop 10.1

use indexmap::IndexMap;
use thiserror::Error;

use super::{MAX_DEPTH, Value, hex4, parse_double};

/// Why a text is not tolerant JSON. The reference message comes from the runtime, so the
/// wording is the port's own.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ParseError {
    /// The text ended inside a value, a string or a comment.
    #[error("unexpected end of text")]
    End,
    /// A character that cannot start or continue the construct at this byte offset.
    #[error("unexpected character at offset {0}")]
    Unexpected(usize),
    /// An escape sequence the dialect does not know.
    #[error("bad escape sequence at offset {0}")]
    Escape(usize),
    /// A number token that is not a number.
    #[error("invalid number at offset {0}")]
    Number(usize),
    /// Arrays or objects nested deeper than the reader follows.
    #[error("nesting deeper than {} levels", MAX_DEPTH)]
    Depth,
}

/// Parses a document. Any value may be the root; anything after it is ignored, as the
/// serializer does by default. An empty text is an error.
///
/// spec: interop 10.1
pub fn parse(text: &str) -> Result<Value, ParseError> {
    let mut parser = Parser {
        text,
        bytes: text.as_bytes(),
        pos: 0,
    };
    parser.value(0)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    /// Always on a character boundary: the parser steps over ASCII bytes, white space, whole
    /// characters of strings and comments.
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn rest(&self) -> &str {
        self.text.get(self.pos..).unwrap_or_default()
    }

    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        self.pos += usize::from(found);
        found
    }

    fn expect(&mut self, byte: u8) -> Result<(), ParseError> {
        if self.eat(byte) {
            return Ok(());
        }
        Err(self.unexpected())
    }

    fn unexpected(&self) -> ParseError {
        if self.pos >= self.bytes.len() {
            ParseError::End
        } else {
            ParseError::Unexpected(self.pos)
        }
    }

    /// Skips white space and comments (`// ...` to the end of the line, `/* ... */`).
    fn skip_trivia(&mut self) -> Result<(), ParseError> {
        loop {
            let rest = self.rest();
            self.pos += rest.len() - rest.trim_start().len();
            let rest = self.rest();
            if rest.starts_with("//") {
                let line = rest.find(['\n', '\r']).unwrap_or(rest.len());
                self.pos += line;
            } else if let Some(comment) = rest.strip_prefix("/*") {
                let end = comment.find("*/").ok_or(ParseError::End)?;
                self.pos += 2 + end + 2;
            } else {
                return Ok(());
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, ParseError> {
        self.skip_trivia()?;
        match self.peek().ok_or(ParseError::End)? {
            b'{' => {
                self.pos += 1;
                self.object(depth + 1)
            }
            b'[' => {
                self.pos += 1;
                self.array(depth + 1)
            }
            quote @ (b'"' | b'\'') => self.string(quote).map(Value::String),
            b'-' | b'.' | b'0'..=b'9' => self.number(),
            b't' => self.word("true", Value::Bool(true)),
            b'f' => self.word("false", Value::Bool(false)),
            b'n' => self.word("null", Value::Null),
            _ => Err(self.unexpected()),
        }
    }

    /// The members after the opening brace.
    fn object(&mut self, depth: usize) -> Result<Value, ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError::Depth);
        }
        let mut members = IndexMap::new();
        loop {
            self.skip_trivia()?;
            if self.eat(b'}') {
                break;
            }
            let key = self.name()?;
            self.skip_trivia()?;
            self.expect(b':')?;
            let value = self.value(depth)?;
            members.insert(key, value);
            self.skip_trivia()?;
            if !self.eat(b',') {
                self.expect(b'}')?;
                break;
            }
        }
        Ok(Value::Object(Box::new(members)))
    }

    /// The items after the opening bracket.
    fn array(&mut self, depth: usize) -> Result<Value, ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError::Depth);
        }
        let mut items = Vec::new();
        loop {
            self.skip_trivia()?;
            if self.eat(b']') {
                break;
            }
            items.push(self.value(depth)?);
            self.skip_trivia()?;
            if !self.eat(b',') {
                self.expect(b']')?;
                break;
            }
        }
        Ok(Value::Array(items))
    }

    /// A property name: a quoted string, or letters, digits, `_` and `$` without quotes.
    fn name(&mut self) -> Result<String, ParseError> {
        if let Some(quote @ (b'"' | b'\'')) = self.peek() {
            return self.string(quote);
        }
        let rest = self.rest();
        let len = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
            .unwrap_or(rest.len());
        if len == 0 {
            return Err(self.unexpected());
        }
        let name = rest.get(..len).unwrap_or_default().to_owned();
        self.pos += len;
        Ok(name)
    }

    /// A string from its opening quote; the standard JSON escapes and `\'` are decoded.
    fn string(&mut self, quote: u8) -> Result<String, ParseError> {
        let mut units: Vec<u16> = Vec::new();
        let mut at = self.pos + 1;
        loop {
            let byte = *self.bytes.get(at).ok_or(ParseError::End)?;
            if byte == quote {
                break;
            }
            if byte == b'\\' {
                let escape = *self.bytes.get(at + 1).ok_or(ParseError::End)?;
                let plain = match escape {
                    b'"' | b'\'' | b'\\' | b'/' => escape,
                    b'b' => 0x08,
                    b'f' => 0x0C,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'u' => {
                        let hex = self.text.get(at + 2..at + 6).and_then(hex4);
                        units.push(hex.ok_or(ParseError::Escape(at))?);
                        at += 6;
                        continue;
                    }
                    _ => return Err(ParseError::Escape(at)),
                };
                units.push(u16::from(plain));
                at += 2;
                continue;
            }
            let c = self
                .text
                .get(at..)
                .and_then(|rest| rest.chars().next())
                .ok_or(ParseError::End)?;
            units.extend(c.encode_utf16(&mut [0; 2]).iter());
            at += c.len_utf8();
        }
        self.pos = at + 1;
        Ok(String::from_utf16_lossy(&units))
    }

    /// A number token: up to white space, `,`, `]`, `}` or the start of a comment.
    fn number(&mut self) -> Result<Value, ParseError> {
        let rest = self.rest();
        let len = rest
            .find(|c: char| c.is_whitespace() || matches!(c, ',' | ']' | '}' | '/'))
            .unwrap_or(rest.len());
        let token = rest.get(..len).unwrap_or_default();
        let number = parse_double(token).ok_or(ParseError::Number(self.pos))?;
        self.pos += len;
        Ok(Value::Number(number))
    }

    fn word(&mut self, word: &str, value: Value) -> Result<Value, ParseError> {
        if !self.rest().starts_with(word) {
            return Err(self.unexpected());
        }
        self.pos += word.len();
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
