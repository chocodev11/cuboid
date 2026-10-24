//! The canonical text dump of a payload (native 10.2).
//!
//! The lines are those of `cuboid_bson::dump` (key path, type, value; doubles as hex bit
//! patterns; binary values as length and SHA-256). This module adds what the golden files need
//! for a RawTexture: below its `r` line come the level-0 part of the pixel data as length and
//! SHA-256 and the length of the tail behind it (a mip chain).
//!
//! ```text
//! tx[0].tx.r: bytes = len=1023 sha256=...
//! tx[0].tx.r.level0: bytes = len=768 sha256=...
//! tx[0].tx.r.tail: i32 = 255
//! ```
//!
//! spec: native 10.2

use std::collections::HashMap;
use std::fmt::Write as _;

use cuboid_bson::{BsonError, Limits, RawDoc, RawElem, Value};
use cuboid_model::image::format;
use sha2::{Digest, Sha256};

/// Dumps a payload.
pub fn payload(bytes: &[u8]) -> Result<String, BsonError> {
    let doc = RawDoc::parse(bytes, &Limits::DEFAULT)?;
    let base = cuboid_bson::dump(&doc)?;
    let mut extras: HashMap<String, String> = HashMap::new();
    for elem in doc.iter() {
        let elem = elem?;
        match elem.key {
            b"tx" => {
                for (i, item) in items(&elem)?.into_iter().enumerate() {
                    if let Some(tx) = member(&item, b"tx")? {
                        texture_lines(&tx, &format!("tx[{i}].tx"), &mut extras)?;
                    }
                }
            }
            b"me" => {
                for (i, item) in items(&elem)?.into_iter().enumerate() {
                    let path = format!("me[{i}]");
                    if let Some(tx) = member(&item, b"tx")? {
                        texture_lines(&tx, &format!("{path}.tx"), &mut extras)?;
                    }
                    if let Some(sm) = member(&item, b"sm")? {
                        for (j, sub) in items(&sm)?.into_iter().enumerate() {
                            if let Some(tx) = member(&sub, b"tx")? {
                                texture_lines(&tx, &format!("{path}.sm[{j}].tx"), &mut extras)?;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if extras.is_empty() {
        return Ok(base);
    }
    let mut out = String::with_capacity(base.len() + extras.len() * 160);
    for line in base.lines() {
        out.push_str(line);
        out.push('\n');
        if let Some((path, _)) = line.split_once(": bytes = ")
            && let Some(extra) = extras.get(path)
        {
            out.push_str(extra);
        }
    }
    Ok(out)
}

/// The elements of an array as elements.
fn items<'a>(elem: &RawElem<'a>) -> Result<Vec<RawElem<'a>>, BsonError> {
    let Some(arr) = elem.array()? else {
        return Ok(Vec::new());
    };
    arr.iter().collect()
}

/// The last member `key` of the document `elem`, when `elem` is a document.
fn member<'a>(elem: &RawElem<'a>, key: &[u8]) -> Result<Option<RawElem<'a>>, BsonError> {
    match elem.doc()? {
        Some(doc) => doc.last(key),
        None => Ok(None),
    }
}

/// Adds the lines for the RawTexture `tx` under `path` to `extras`, keyed by the path of its `r`
/// line.
fn texture_lines(
    tx: &RawElem<'_>,
    path: &str,
    extras: &mut HashMap<String, String>,
) -> Result<(), BsonError> {
    let Some(doc) = tx.doc()? else {
        return Ok(());
    };
    let number =
        |key: &[u8]| -> Result<i32, BsonError> { doc.last(key)?.map_or(Ok(0), |elem| elem.i32()) };
    let (w, h, f) = (number(b"w")?, number(b"h")?, number(b"f")?);
    let Some(r) = doc.last(b"r")? else {
        return Ok(());
    };
    let Value::Binary { bytes, .. } = r.value()? else {
        return Ok(());
    };
    let (Ok(w), Ok(h)) = (u32::try_from(w), u32::try_from(h)) else {
        return Ok(());
    };
    let Ok(level0) = format::level_len(f, w, h) else {
        return Ok(());
    };
    let level0 = level0.min(bytes.len());
    let (head, tail) = bytes.split_at(level0);
    let mut text = String::new();
    let _ = writeln!(
        text,
        "{path}.r.level0: bytes = len={} sha256={}",
        head.len(),
        sha256(head)
    );
    let _ = writeln!(text, "{path}.r.tail: i32 = {}", tail.len());
    extras.insert(format!("{path}.r"), text);
    Ok(())
}

fn sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}
