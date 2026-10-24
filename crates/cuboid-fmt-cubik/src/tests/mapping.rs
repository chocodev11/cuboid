//! Retention of keys that the model does not know (native 3.6, 8.3).

use cuboid_bson::DocWriter;
use cuboid_model::{
    ApplyMode, ApplySource, CaptureScope, Categories, Document, EditCtx, EditEnv, Label,
    RenderPrefs, Selection, TextureList, apply_document, capture,
};
use cuboid_num::EngineRng;
use glam::Vec3;

use super::root;
use crate::records::ElementRecord;
use crate::{Limits, SaveOptions, encode, load, save};

/// A file whose elements and root carry keys that Cubik does not write.
fn payload_with_unknown_keys() -> Vec<u8> {
    root(|w: &mut DocWriter| {
        w.i32(b"v\x00", 1);
        w.str(b"extra\x00", "root-level");
        encode::element(w, b"bp\x00", &ElementRecord::default());
        let list = w.begin_array(b"el\x00");
        for (i, name) in ["a", "b"].into_iter().enumerate() {
            let item = w.begin_doc(i);
            let rec = ElementRecord {
                l: Some(name.to_owned()),
                ..ElementRecord::default()
            };
            encode::element_members(w, &rec);
            w.i32(b"unknown_number\x00", 12_345 + i32::try_from(i).unwrap());
            w.end(item);
        }
        w.end(list);
    })
}

fn open(payload: Vec<u8>) -> (Document, Selection) {
    let loaded = load::load_payload(payload, &Limits::default()).unwrap();
    let mut doc = Document::new(TextureList::default());
    let mut sel = Selection::default();
    let env = EditEnv {
        restrict_rotations: true,
        render: RenderPrefs::default(),
        code_page: 1252,
        allow_negative_size: false,
    };
    let mut rng = EngineRng::seeded(1);
    apply_document(
        &mut EditCtx {
            doc: &mut doc,
            sel: &mut sel,
            env: &env,
            rng: &mut rng,
        },
        ApplySource::Loaded(Box::new(loaded.doc)),
        ApplyMode::Open,
    )
    .unwrap();
    (doc, sel)
}

fn save(doc: &Document, sel: &Selection) -> Vec<u8> {
    let snap = capture(
        doc,
        sel,
        CaptureScope {
            categories: Categories::ALL,
            selection_only: false,
        },
    );
    save::save_payload(&snap, doc.retained.as_deref(), &SaveOptions::default()).unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn unknown_keys_survive_an_unchanged_save_and_the_save_of_another_record() {
    let payload = payload_with_unknown_keys();
    let (mut doc, sel) = open(payload.clone());
    // Sections that the file lacks are written; the records that it has are copied.
    let saved = save(&doc, &sel);
    assert!(contains(&saved, &12_345_i32.to_le_bytes()));
    assert!(contains(&saved, &12_346_i32.to_le_bytes()));

    let second = doc.elements.ids()[1];
    let mut journal = std::mem::take(&mut doc.journal);
    doc.elements.edit(second, &mut journal).unwrap().base.p = Vec3::new(5.0, 0.0, 0.0);
    doc.journal = journal;
    let saved = save(&doc, &sel);
    assert!(
        contains(&saved, &12_345_i32.to_le_bytes()),
        "the untouched record"
    );
    assert!(
        !contains(&saved, &12_346_i32.to_le_bytes()),
        "the edited record is encoded again"
    );
    assert_eq!(&*doc.elements.get(second).unwrap().base.label, "b");
    let _ = Label::default();
}
