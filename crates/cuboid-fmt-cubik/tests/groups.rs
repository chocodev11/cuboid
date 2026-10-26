//! The group tree in the `cuboid.json` entry: it survives a save, the payload stays what it was
//! without it, and a payload that another program changed keeps the links that still match.

mod common;

use std::io::Cursor;

use common::*;
use cuboid_fmt_cubik::container;
use cuboid_fmt_cubik::{Limits, SaveOptions, load_bytes, save_bytes};
use cuboid_model::{Document, Group, ObjRef, Selection};
use glam::Vec3;

fn payload_and_entry(file: Vec<u8>) -> (Vec<u8>, Option<Vec<u8>>) {
    let c = container::read(Cursor::new(file), &Limits::default()).unwrap();
    (c.payload, c.extension)
}

/// The sample document with its first two elements in a group "Arm", and that group with the
/// third element in a group "Body".
fn grouped() -> (Document, Selection) {
    let (mut doc, sel) = open(common::load_bytes(&full_payload()));
    assert!(doc.elements.len() >= 3, "the sample has elements to group");
    let ids = doc.elements.ids().to_vec();
    let arm = doc
        .insert_group(
            Group::new("Arm", Vec3::new(1.0, 2.0, 3.0)),
            &[ObjRef::Element(ids[0]), ObjRef::Element(ids[1])],
        )
        .unwrap();
    let body = doc
        .insert_group(
            Group::new("Body", Vec3::ZERO),
            &[ObjRef::Group(arm), ObjRef::Element(ids[2])],
        )
        .unwrap();
    let record = doc
        .groups
        .edit(arm, cuboid_model::ChangeFlags::all(), &mut doc.journal)
        .unwrap();
    record.rotation = Vec3::new(0.0, 90.0, 0.0);
    record.locked = true;
    record.uuid = Some("bone-arm".to_owned());
    doc.groups
        .edit(body, cuboid_model::ChangeFlags::all(), &mut doc.journal)
        .unwrap()
        .visible = false;
    (doc, sel)
}

fn save(doc: &Document, sel: &Selection, options: &SaveOptions) -> Vec<u8> {
    save_bytes(&snapshot(doc, sel), doc.retained.as_deref(), &[], options).unwrap()
}

#[test]
fn the_groups_round_trip_and_the_payload_is_the_one_without_them() {
    let (doc, sel) = grouped();
    let file = save(&doc, &sel, &SaveOptions::default());
    let loaded = load_bytes(&file, &Limits::default()).unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let (back, _) = open(loaded);

    assert_eq!(back.groups.len(), 2);
    let outline = |d: &Document| -> Vec<String> {
        d.outline()
            .into_iter()
            .map(|(node, parent)| match node {
                ObjRef::Group(id) => format!(
                    "group {} under {:?}",
                    d.groups.get(id).unwrap().name,
                    parent
                        .and_then(|p| d.groups.get(p))
                        .map(|g| g.name.to_string())
                ),
                ObjRef::Element(id) => format!(
                    "element {} under {:?}",
                    d.elements.position(id).unwrap(),
                    parent
                        .and_then(|p| d.groups.get(p))
                        .map(|g| g.name.to_string())
                ),
                ObjRef::Mesh(id) => format!(
                    "mesh {} under {:?}",
                    d.meshes.position(id).unwrap(),
                    parent
                        .and_then(|p| d.groups.get(p))
                        .map(|g| g.name.to_string())
                ),
                _ => String::new(),
            })
            .collect()
    };
    assert_eq!(outline(&back), outline(&doc));
    let arm = back
        .groups
        .items()
        .iter()
        .find(|g| &*g.name == "Arm")
        .unwrap();
    assert_eq!(arm.origin, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(arm.rotation, Vec3::new(0.0, 90.0, 0.0));
    assert!(arm.locked && arm.visible && arm.export);
    assert_eq!(arm.uuid.as_deref(), Some("bone-arm"));
    let body = back
        .groups
        .items()
        .iter()
        .find(|g| &*g.name == "Body")
        .unwrap();
    assert!(!body.visible);

    // The payload does not know the groups: a compatible save holds the same bytes.
    let compatible = SaveOptions {
        extension: false,
        ..SaveOptions::default()
    };
    let plain = save(&doc, &sel, &compatible);
    assert_eq!(payload_and_entry(file).0, payload_and_entry(plain).0);
}

#[test]
fn a_model_without_groups_writes_no_entry_for_them() {
    let (doc, sel) = open(common::load_bytes(&full_payload()));
    let file = save(&doc, &sel, &SaveOptions::default());
    assert_eq!(payload_and_entry(file).1, None);
}

#[test]
fn a_compatible_save_leaves_the_groups_out() {
    let (doc, sel) = grouped();
    let compatible = SaveOptions {
        extension: false,
        ..SaveOptions::default()
    };
    let file = save(&doc, &sel, &compatible);
    let (back, _) = open(load_bytes(&file, &Limits::default()).unwrap());
    assert!(back.groups.is_empty());
}

#[test]
fn a_payload_changed_by_another_program_keeps_the_links_that_still_match() {
    let (doc, sel) = grouped();
    let file = save(&doc, &sel, &SaveOptions::default());
    let (_, entry) = payload_and_entry(file);

    // Another program moves the first element and appends a new one.
    let (mut changed, sel2) = open(common::load_bytes(&full_payload()));
    let first = changed.elements.ids()[0];
    changed
        .elements
        .edit(first, &mut changed.journal)
        .unwrap()
        .base
        .p
        .y += 5.0;
    let extra = changed.elements.items()[0].as_ref().clone();
    changed.elements.push(extra);
    let payload = save_payload(&changed, &sel2, &without_retention());
    let file = container::write(&payload, entry.as_deref()).unwrap();

    let loaded = load_bytes(&file, &Limits::default()).unwrap();
    assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
    assert!(
        loaded.warnings[0].contains("changed outside"),
        "{:?}",
        loaded.warnings
    );
    let (back, _) = open(loaded);
    let arm = back
        .groups
        .items()
        .iter()
        .find(|g| &*g.name == "Arm")
        .unwrap();
    // The moved element lost its link; the other one is still in the group.
    assert_eq!(arm.children().len(), 1);
    let kept = match arm.children()[0] {
        ObjRef::Element(id) => back.elements.position(id).unwrap(),
        other => panic!("{other:?}"),
    };
    assert_eq!(kept, 1);
    let moved = ObjRef::Element(back.elements.ids()[0]);
    assert_eq!(back.groups.parent_of(moved), None);
    assert!(back.check_groups().is_ok());
}

#[test]
fn a_damaged_tree_member_is_reported_and_ignored() {
    let entry = br#"{"version":1,"groups":{"records":7}}"#;
    let file = container::write(&full_payload(), Some(entry)).unwrap();
    let loaded = load_bytes(&file, &Limits::default()).unwrap();
    assert_eq!(loaded.warnings.len(), 1);
    assert!(loaded.warnings[0].contains("damaged"));
    let (doc, _) = open(loaded);
    assert!(doc.groups.is_empty());
}
