//! Snapshot capture by change category (spec: native 5.2-5.3; tools 15.2).

use std::sync::Arc;

use crate::contract::Document;
use crate::ids::ObjRef;
use crate::list::{ListSnap, ObjList};
use crate::normal_form;
use crate::selection::Selection;
use crate::snapshot::{Categories, ElementsSnap, MeshesSnap, Snapshot};

/// What a capture copies.
///
/// spec: tools 15.2, tools 11.2
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CaptureScope {
    /// The categories to copy.
    pub categories: Categories,
    /// Copy only the selected elements and meshes (clipboard and clone snapshots).
    pub selection_only: bool,
}

/// Copies the parts of the document named by the scope into a snapshot, with the save
/// normalisations of native 5.3. Parts are pointer copies.
///
/// The normalisations that concern the snapshot itself are applied here: `vp.gu` and `vp.gl` with
/// their defaults, the display scale of 0.0001 for a zero component and no ASCII double quotes in
/// the model strings (an undo or clipboard capture gets them too, so a zero display scale comes
/// back as 0.0001 after an undo of the display slots). The others (colour packing, folded UV,
/// `fp` and `cs`, `me` and `sm` of primitives, the blueprint keys) are properties of the record
/// encoding and belong to the file writer; a restore or paste applies the record normal form of
/// `normal_form` itself.
///
/// The caller captures the four cameras into the view properties first and sets the save path
/// `mp.sp` before a save snapshot (native 5.1, 5.2).
///
/// spec: native 5.2-5.3, tools 15.2, arch 9.1
pub fn capture(doc: &Document, sel: &Selection, scope: CaptureScope) -> Snapshot {
    let cats = scope.categories;
    let selection_only = scope.selection_only;
    let has = |flag: Categories| cats.contains(flag);
    Snapshot {
        categories: cats,
        props: has(Categories::MODEL_PROPERTIES)
            .then(|| normal_form::share(&doc.props, normal_form::props(&doc.props))),
        view: has(Categories::VIEW_PROPERTIES)
            .then(|| normal_form::share(&doc.view, normal_form::saved_view(&doc.view))),
        elements: has(Categories::ELEMENTS).then(|| {
            let (list, selected) = list_snap(&doc.elements, sel, selection_only, ObjRef::Element);
            ElementsSnap {
                blueprint: doc.blueprint.clone(),
                list,
                selected,
            }
        }),
        textures: has(Categories::TEXTURES).then(|| Arc::new(doc.textures.clone())),
        voxels: has(Categories::VOXELS).then(|| doc.voxels.as_ref().map(|world| world.snapshot())),
        meshes: has(Categories::MESHES).then(|| {
            let (list, selected) = list_snap(&doc.meshes, sel, selection_only, ObjRef::Mesh);
            MeshesSnap { list, selected }
        }),
        display: has(Categories::MODEL_DISPLAY)
            .then(|| normal_form::share(&doc.display, normal_form::saved_display(&doc.display))),
    }
}

/// The items of a list in list order with their selection flags; with `selection_only` only the
/// selected ones, which then all carry a set flag.
fn list_snap<T: Clone>(
    list: &ObjList<T>,
    sel: &Selection,
    selection_only: bool,
    obj: fn(crate::ids::ObjId) -> ObjRef,
) -> (ListSnap<T>, Vec<bool>) {
    let flags: Vec<bool> = list.ids().iter().map(|&id| sel.contains(obj(id))).collect();
    if !selection_only {
        return (list.snapshot(), flags);
    }
    let mut snap = ListSnap {
        ids: Vec::new(),
        items: Vec::new(),
    };
    for ((&id, item), _) in list
        .ids()
        .iter()
        .zip(list.items())
        .zip(&flags)
        .filter(|(_, selected)| **selected)
    {
        snap.ids.push(id);
        snap.items.push(Arc::clone(item));
    }
    let selected = vec![true; snap.ids.len()];
    (snap, selected)
}

#[cfg(test)]
mod tests {
    use cuboid_voxel::{I3, VoxelWorld, WriteKind, mapping};
    use glam::Vec3;

    use super::*;
    use crate::contract::{DisplaySlotId, ModelProps, ViewProps};
    use crate::objects::{Texture, TextureEntry};
    use crate::setters::fixture::World;

    fn scope(categories: Categories) -> CaptureScope {
        CaptureScope {
            categories,
            selection_only: false,
        }
    }

    fn world_with_objects() -> (World, Vec<crate::ids::ObjId>, Vec<crate::ids::ObjId>) {
        let mut w = World::new();
        let els: Vec<_> = (0..3)
            .map(|i| w.add_element(Vec3::new(i as f32, 0.0, 0.0), Vec3::ONE))
            .collect();
        let meshes: Vec<_> = (0..2)
            .map(|i| w.add_mesh(Vec3::new(0.0, i as f32, 0.0), Vec3::ONE))
            .collect();
        (w, els, meshes)
    }

    #[test]
    fn only_the_named_categories_are_present() {
        let (w, _, _) = world_with_objects();
        let snap = capture(&w.doc, &w.sel, scope(Categories::ELEMENTS_AND_TEXTURES));
        assert_eq!(snap.categories, Categories::ELEMENTS_AND_TEXTURES);
        assert!(snap.elements.is_some() && snap.textures.is_some());
        assert!(snap.props.is_none() && snap.view.is_none() && snap.display.is_none());
        assert!(snap.voxels.is_none() && snap.meshes.is_none());

        let all = capture(&w.doc, &w.sel, scope(Categories::ALL));
        assert!(all.props.is_some() && all.view.is_some() && all.display.is_some());
        assert!(all.elements.is_some() && all.meshes.is_some() && all.textures.is_some());
        assert!(
            matches!(all.voxels, Some(None)),
            "category present, no world"
        );
    }

    #[test]
    fn parts_are_pointer_copies() {
        let (mut w, els, _) = world_with_objects();
        w.doc.textures.entries.push(TextureEntry {
            label: "texture".into(),
            texture: Texture::new(Arc::new(cuboid_image::Image::from_rgba(
                cuboid_image::RgbaBuf::new(2, 2),
            ))),
        });
        let snap = capture(&w.doc, &w.sel, scope(Categories::ALL));
        assert!(Arc::ptr_eq(snap.props.as_ref().unwrap(), &w.doc.props));
        assert!(Arc::ptr_eq(snap.view.as_ref().unwrap(), &w.doc.view));
        assert!(Arc::ptr_eq(snap.display.as_ref().unwrap(), &w.doc.display));
        let elements = snap.elements.as_ref().unwrap();
        assert_eq!(elements.list.ids, w.doc.elements.ids());
        for (copy, live) in elements.list.items.iter().zip(w.doc.elements.items()) {
            assert!(Arc::ptr_eq(copy, live));
        }
        let texture = &snap.textures.as_ref().unwrap().entries[0].texture;
        assert!(Arc::ptr_eq(
            &texture.image,
            &w.doc.textures.entries[0].texture.image
        ));

        // A later edit does not reach the snapshot.
        w.edit_element(els[0], |el| el.base.hidden = true);
        assert!(!elements.list.items[0].base.hidden);
    }

    #[test]
    fn the_selection_flags_follow_the_selection() {
        let (mut w, els, meshes) = world_with_objects();
        w.select(&[ObjRef::Element(els[1]), ObjRef::Mesh(meshes[0])]);
        let snap = capture(&w.doc, &w.sel, scope(Categories::OBJECTS));
        assert_eq!(snap.elements.unwrap().selected, [false, true, false]);
        assert_eq!(snap.meshes.unwrap().selected, [true, false]);
    }

    #[test]
    fn a_selection_only_snapshot_keeps_the_selected_objects_in_list_order() {
        let (mut w, els, meshes) = world_with_objects();
        w.select(&[
            ObjRef::Element(els[2]),
            ObjRef::Element(els[0]),
            ObjRef::Mesh(meshes[1]),
            ObjRef::Blueprint,
        ]);
        let snap = capture(
            &w.doc,
            &w.sel,
            CaptureScope {
                categories: Categories::OBJECTS,
                selection_only: true,
            },
        );
        let elements = snap.elements.unwrap();
        assert_eq!(elements.list.ids, [els[0], els[2]]);
        assert_eq!(elements.selected, [true, true]);
        assert_eq!(
            elements.blueprint, w.doc.blueprint,
            "the blueprint is always in"
        );
        let meshes_snap = snap.meshes.unwrap();
        assert_eq!(meshes_snap.list.ids, [meshes[1]]);
        assert!(Arc::ptr_eq(
            &meshes_snap.list.items[0],
            &w.doc.meshes.items()[1]
        ));
    }

    #[test]
    fn save_normalisations_of_the_snapshot_content() {
        let (mut w, _, _) = world_with_objects();
        let mut props = ModelProps::default();
        props.general.name = "a\"b".to_owned();
        w.doc.props = Arc::new(props);
        w.doc.view = Arc::new(ViewProps {
            uv_step: 0.125,
            large_grid: true,
            key_step: 0.25,
            ..ViewProps::default()
        });
        Arc::make_mut(&mut w.doc.display)[DisplaySlotId::Gui].scale = Vec3::new(0.0, 1.0, 0.0);

        let snap = capture(&w.doc, &w.sel, scope(Categories::ALL));
        let props = snap.props.unwrap();
        assert!(!props.general.name.contains('"'));
        assert_eq!(props.general.name.chars().count(), 3);
        let view = snap.view.unwrap();
        assert_eq!(
            (view.uv_step, view.large_grid, view.key_step),
            (0.5, false, 0.25)
        );
        assert_eq!(
            snap.display.unwrap()[DisplaySlotId::Gui].scale,
            Vec3::new(0.0001, 1.0, 0.0001)
        );
        assert!(w.doc.view.large_grid, "the live document is untouched");
    }

    #[test]
    fn the_voxel_category_holds_the_world_or_its_absence() {
        let (mut w, _, _) = world_with_objects();
        let snap = capture(&w.doc, &w.sel, scope(Categories::VOXELS));
        assert!(matches!(snap.voxels, Some(None)));

        let mut world = VoxelWorld::new(8, true, mapping::grid_box(8));
        world.write_many(
            &mut [(I3::new(1, 2, 3), 0xFF11_2233)].into_iter(),
            WriteKind::Full,
        );
        w.doc.voxels = Some(world);
        let snap = capture(&w.doc, &w.sel, scope(Categories::VOXELS));
        let voxels = snap.voxels.unwrap().expect("a world");
        assert_eq!(voxels.res, 8);
        assert_eq!(
            voxels.slots.len(),
            8,
            "n = ceil(24 / 16) = 2 chunks per axis"
        );
    }
}
