//! Regression: an ingested E3D batch must reach the solid-mesh collection.
//!
//! `io::e3d::consumer` parks one invisible `POINT` proxy per **Geometry Item**
//! and hangs the placed [`MeshLodSet`] off the same handle. The proxy is
//! invisible so the 2D point path stays quiet, but the mesh visibility gate
//! (`mesh_entity_visible` → `mesh_definition_entity_visible` →
//! `display_entity_invisible`) reads the same DXF flag, so before
//! `is_e3d_geometry_proxy` existed every ingested solid was filtered out and
//! the batch rendered nothing.

use std::collections::BTreeMap;

use crate::io::e3d::contract::{
    CanonicalShapeKey, CompactTriMesh, E3dBatchManifest, E3dInstanceRecord, E3dSceneBatch,
    GeometryIdKey, MeshKey,
};
use crate::io::e3d::ingest_batch;
use crate::scene::Scene;

fn translation(x: f64, y: f64, z: f64) -> [f64; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0,
    ]
}

fn triangle() -> CompactTriMesh {
    CompactTriMesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
        indices: vec![0, 1, 2],
        feature_edges: vec![[0, 1], [1, 2], [2, 0]],
        local_aabb_mm: [[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
    }
}

fn instance(refno: &str, x: f64) -> E3dInstanceRecord {
    E3dInstanceRecord {
        geometry_id: GeometryIdKey::Element {
            refno: refno.to_string(),
        },
        refno: refno.to_string(),
        owner_refno: Some("26229/1509".to_string()),
        noun: "VALV".to_string(),
        name: Some("/100-B-1".to_string()),
        mesh_key: MeshKey::Canonical(CanonicalShapeKey::BoxV1),
        mesh_to_world_mm: translation(x, 0.0, 0.0),
        world_aabb_mm: [[x - 0.5, -0.5, -0.5], [x + 0.5, 0.5, 0.5]],
    }
}

fn batch(instances: Vec<E3dInstanceRecord>) -> E3dSceneBatch {
    let key = MeshKey::Canonical(CanonicalShapeKey::BoxV1);
    let mut meshes = BTreeMap::new();
    meshes.insert(key.clone(), triangle());
    E3dSceneBatch {
        manifest: E3dBatchManifest {
            source_label: "synthetic".to_string(),
            is_incremental: false,
            upserted: instances,
            centrelines: Vec::new(),
            removed: Vec::new(),
            required_meshes: vec![key],
            ledger: Default::default(),
        },
        meshes,
    }
}

#[test]
fn ingested_geometry_items_reach_the_solid_mesh_collection() {
    let mut scene = Scene::new();
    let full = batch(vec![
        instance("24381/47065", 1000.0),
        instance("24381/47066", 2000.0),
    ]);
    let stats = ingest_batch(&mut scene, &full).expect("full ingest");
    assert_eq!(stats.proxies, 2);
    assert_eq!(stats.missing_meshes, 0);

    // Both proxies carry the DXF invisible flag, which is what used to hide
    // their meshes.
    for &handle in scene.e3d_index.by_geometry.values() {
        let entity = scene.document.get_entity(handle).expect("proxy entity");
        assert!(entity.common().invisible, "proxy stays invisible");
        assert!(crate::scene::is_e3d_geometry_proxy(entity.common()));
    }

    let collected = scene.meshes_arc();
    assert_eq!(
        collected.len(),
        2,
        "both E3D geometry items must survive the mesh visibility gate"
    );
    for set in collected.iter() {
        assert!(
            set.instance_transform.is_some(),
            "collected E3D instance keeps its placement"
        );
        assert!(
            set.lods.is_empty(),
            "collected E3D instance must not copy triangles"
        );
    }
}

#[test]
fn clearing_the_batch_removes_the_collected_meshes() {
    let mut scene = Scene::new();
    ingest_batch(&mut scene, &batch(vec![instance("1/1", 0.0)])).expect("first ingest");
    assert_eq!(scene.meshes_arc().len(), 1);

    // A second full batch erases the first source before pouring again.
    ingest_batch(&mut scene, &batch(vec![instance("2/2", 500.0)])).expect("second ingest");
    let collected = scene.meshes_arc();
    assert_eq!(collected.len(), 1);
    assert_eq!(scene.e3d_index.by_geometry.len(), 1);
}

#[test]
fn an_ordinary_invisible_point_stays_out_of_the_mesh_collection() {
    let mut scene = Scene::new();
    let mut point = codec::entities::Point::at(codec::types::Vector3::new(0.0, 0.0, 0.0));
    point.common.invisible = true;
    let handle = scene.add_entity(codec::EntityType::Point(point));
    let entity = scene.document.get_entity(handle).expect("point");
    assert!(!crate::scene::is_e3d_geometry_proxy(entity.common()));
}
