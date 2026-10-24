use cuboid_model::{EditEnv, MeshGeometry, RenderPrefs, Selection, TextureList, check_invariants};
use cuboid_num::EngineRng;
use glam::{Vec2, Vec3};

use super::*;

fn env() -> EditEnv {
    EditEnv {
        restrict_rotations: true,
        render: RenderPrefs::default(),
        code_page: 1252,
        allow_negative_size: false,
    }
}

fn bare(kind: MeshKind) -> Mesh {
    Mesh {
        base: ObjectBase::new("Mesh", Vec3::ZERO, Vec3::ONE, Perms::PRIMITIVE),
        kind,
        geometry: Arc::default(),
        texture: None,
        submeshes: Arc::from([]),
        bounds: MeshBounds::default(),
        norm_factor: 1.0,
        import_offset: Vec3::ZERO,
        tint: [1.0; 4],
    }
}

#[test]
fn a_primitive_is_placed_at_the_blueprint_with_its_pivot_offset() {
    let mut doc = Document::new(TextureList::default());
    let mut sel = Selection::default();
    let env = env();
    let mut rng = EngineRng::seeded(1);
    doc.blueprint.base.s = Vec3::new(4.0, 6.0, 8.0);
    let (p, s) = (doc.blueprint.base.p, doc.blueprint.base.s);
    let color = [1.0, 0.0, 0.0, 1.0];
    let mut cx = EditCtx {
        doc: &mut doc,
        sel: &mut sel,
        env: &env,
        rng: &mut rng,
    };
    let kinds = [
        (MeshKind::Cube, Vec3::ZERO),
        (MeshKind::Sphere, Vec3::new(2.0, 3.0, -4.0)),
        (MeshKind::Cylinder, Vec3::new(2.0, 0.0, -4.0)),
        (MeshKind::Cone, Vec3::new(2.0, 0.0, -4.0)),
        (MeshKind::Pyramid, Vec3::ZERO),
        (MeshKind::Slope, Vec3::ZERO),
        (MeshKind::CornerIn, Vec3::ZERO),
        (MeshKind::CornerOut, Vec3::ZERO),
        (MeshKind::Torus, Vec3::new(2.0, 3.0, -4.0)),
    ];
    for (kind, offset) in kinds {
        let id = add_primitive(&mut cx, kind, color).expect("a primitive");
        let mesh = cx.doc.meshes.get(id).expect("created");
        assert_eq!(mesh.kind, kind);
        assert_eq!(&*mesh.base.label, shapes::label(kind));
        assert_eq!((mesh.base.p, mesh.base.s), (p, s));
        assert_eq!(mesh.base.o, p + offset, "{kind:?} pivot");
        assert_eq!(mesh.base.e, Vec3::ZERO);
        assert_eq!(mesh.geometry, shapes::geometry(kind).expect("shared"));
        assert_eq!(cx.sel.primary, Some(ObjRef::Mesh(id)));
        assert_eq!(cx.sel.list.len(), 1);
    }
    assert_eq!(cx.doc.meshes.len(), 9);
    check_invariants(&doc, &sel).expect("valid document");
}

#[test]
fn the_paint_texture_is_128_square_and_named_after_the_kind() {
    let mut doc = Document::new(TextureList::default());
    let mut sel = Selection::default();
    let env = env();
    let mut rng = EngineRng::seeded(1);
    let mut cx = EditCtx {
        doc: &mut doc,
        sel: &mut sel,
        env: &env,
        rng: &mut rng,
    };
    let id = add_primitive(&mut cx, MeshKind::Cone, [0.0, 1.0, 0.0, 1.0]).expect("a primitive");
    let texture = cx
        .doc
        .meshes
        .get(id)
        .and_then(|m| m.texture.as_ref())
        .expect("a paint texture");
    assert_eq!(&*texture.name, "Cone");
    assert_eq!((texture.image.w, texture.image.h), (128, 128));
    assert!(
        texture
            .image
            .rgba()
            .chunks_exact(4)
            .all(|t| t == [0, 255, 0, 255])
    );
}

#[test]
fn a_sprite_is_flat_and_an_imported_mesh_cannot_be_added() {
    let mut doc = Document::new(TextureList::default());
    let mut sel = Selection::default();
    let env = env();
    let mut rng = EngineRng::seeded(1);
    let mut cx = EditCtx {
        doc: &mut doc,
        sel: &mut sel,
        env: &env,
        rng: &mut rng,
    };
    assert!(add_primitive(&mut cx, MeshKind::Custom, [1.0; 4]).is_none());
    assert!(cx.doc.meshes.is_empty());
    let id = add_primitive(&mut cx, MeshKind::Sprite, [1.0; 4]).expect("a primitive");
    let mesh = cx.doc.meshes.get(id).expect("created");
    assert!((mesh.base.s.y - MIN_SIZE).abs() < 1e-9);
    assert_eq!(mesh.geometry.triangles.len(), 12);
}

#[test]
fn ensure_geometry_fills_only_empty_primitives() {
    let mut doc = Document::new(TextureList::default());
    let kept = Arc::new(MeshGeometry {
        vertices: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        normals: None,
        uvs: vec![Vec2::ZERO; 3],
        triangles: vec![0, 1, 2],
    });
    let mut own = bare(MeshKind::Cylinder);
    own.geometry = Arc::clone(&kept);
    let ids = [
        doc.meshes.push(bare(MeshKind::Cube)),
        doc.meshes.push(bare(MeshKind::Sphere)),
        doc.meshes.push(own),
        doc.meshes.push(bare(MeshKind::Custom)),
        doc.meshes.push(bare(MeshKind::Sprite)),
    ];
    let before = doc.revision;
    assert_eq!(ensure_geometry(&mut doc), 3);
    assert_ne!(doc.revision, before);
    let geometry = |at: usize| Arc::clone(&doc.meshes.get(ids[at]).expect("kept").geometry);
    assert_eq!(geometry(0).vertices.len(), 24);
    assert_eq!(geometry(1).vertices.len(), 1734);
    assert!(Arc::ptr_eq(&geometry(2), &kept), "geometry is not replaced");
    assert!(
        geometry(3).vertices.is_empty(),
        "imported meshes stay as loaded"
    );
    assert_eq!(geometry(4).vertices.len(), 8);
    assert!(doc.journal.objects().contains_key(&ids[0]));
    assert!(!doc.journal.objects().contains_key(&ids[2]));

    // A second run has nothing to do and leaves the revision alone.
    let after = doc.revision;
    assert_eq!(ensure_geometry(&mut doc), 0);
    assert_eq!(doc.revision, after);
}

#[test]
fn equal_kinds_share_one_geometry_after_ensure() {
    let mut doc = Document::new(TextureList::default());
    let a = doc.meshes.push(bare(MeshKind::Torus));
    let b = doc.meshes.push(bare(MeshKind::Torus));
    ensure_geometry(&mut doc);
    let (a, b) = (doc.meshes.get(a).expect("a"), doc.meshes.get(b).expect("b"));
    assert!(Arc::ptr_eq(&a.geometry, &b.geometry));
}
