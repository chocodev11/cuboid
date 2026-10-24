//! Reads everything a document offers and checks what must hold for any input. Shared by the
//! robustness tests and the fuzz target, which includes this file by path: it depends on
//! `cuboid_bson` only.

use cuboid_bson::{BsonError, DocWriter, Key, Limits, RawDoc, Value, dump};

/// A key of a document that was read; it has no NUL.
struct Name<'a>(&'a [u8]);

impl Key for Name<'_> {
    fn put(self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.0);
        out.push(0);
    }
}

/// A document around `body`, as if its elements were valid.
pub fn wrap_body(body: &[u8]) -> Vec<u8> {
    let mut doc = (body.len() as i32 + 5).to_le_bytes().to_vec();
    doc.extend_from_slice(body);
    doc.push(0);
    doc
}

/// Walks a document and everything below it, checks that the spans tile the document and calls
/// every getter on every element. A broken element ends the walk of its document with the error.
pub fn walk(doc: RawDoc<'_>, bytes: &[u8]) -> Result<(), BsonError> {
    let mut cursor = doc.span().start as usize + 4;
    for elem in doc.iter() {
        let elem = elem?;
        assert_eq!(elem.whole.start as usize, cursor);
        cursor = elem.whole.end as usize;
        assert_eq!(elem.value.end, elem.whole.end);
        assert_eq!(elem.value_bytes(), &bytes[elem.value.range()]);
        let key = &bytes[elem.whole.start as usize + 1..][..elem.key.len()];
        assert_eq!(key, elem.key);

        let _ = (elem.f32(), elem.i32(), elem.u8(), elem.bool());
        let _ = (
            elem.enum_i32(&[("a", 1), ("B", 2)]),
            elem.str_opt(),
            elem.bytes_opt(),
        );
        let _ = (elem.doc(), elem.array());
        if let Ok(Value::Doc(child) | Value::Array(child)) = elem.value() {
            walk(child, bytes)?;
        }
    }
    assert_eq!(cursor, doc.span().end as usize - 1);
    Ok(())
}

/// Writes the elements of `doc` again, with the typed writer where the subset has one and by
/// copying the span otherwise. Stops at the first element that does not read.
fn rewrite(w: &mut DocWriter, doc: RawDoc<'_>, bytes: &[u8]) -> Result<(), BsonError> {
    for elem in doc.iter() {
        let elem = elem?;
        let key = Name(elem.key);
        match elem.value()? {
            Value::Double(v) => w.f64(key, v),
            Value::String(v) => w.str(key, v),
            Value::Doc(child) => {
                let mark = w.begin_doc(key);
                rewrite(w, child, bytes)?;
                w.end(mark);
            }
            Value::Array(child) => {
                let mark = w.begin_array(key);
                rewrite(w, child, bytes)?;
                w.end(mark);
            }
            Value::Bool(v) => w.bool(key, v),
            Value::Null => w.null(key),
            Value::Int32(v) => w.i32(key, v),
            Value::Int64(v) => w.i64(key, v),
            Value::Binary { subtype: 0, bytes } => w.bin(key, bytes),
            Value::Binary { .. } | Value::Other(_) => w.raw(&bytes[elem.whole.range()]),
        }
    }
    Ok(())
}

/// Parses `bytes`, walks and dumps whatever is there, and checks that a document which reads
/// completely is written back byte for byte. Panics only on a bug.
pub fn read_everything(bytes: &[u8], limits: &Limits) {
    let Ok(doc) = RawDoc::parse(bytes, limits) else {
        return;
    };
    let walked = walk(doc, bytes);
    let _ = dump(&doc);
    let _ = doc.last(b"a");

    let mut w = DocWriter::new();
    let root = w.begin_root();
    if rewrite(&mut w, doc, bytes).is_ok() {
        w.end(root);
        assert_eq!(w.finish().unwrap(), doc.as_bytes());
        assert!(walked.is_ok());
    }
}
