//! Open a standalone AVEVA `.rvm` file into a document plus pre-built scene
//! meshes (`cfg(not(target_arch = "wasm32"))`).
//!
//! A `.rvm` is not a drawing: it is a stream of primitives. The producer turns
//! it into an [`E3dSceneBatch`](super::contract::E3dSceneBatch) and the
//! consumer pours that into a `Document` (proxy entities and centrelines) and
//! a `Scene`'s mesh pools. The file-open worker needs exactly those three
//! pieces, so this module drives the pair through a throwaway `Scene` and hands
//! the results back for the caller to install.
//!
//! It deliberately does **not** run `build_derived_caches`: the document it
//! produces carries no DWG solids, and the meshes already exist, so a cache
//! rebuild would only re-tessellate the centrelines that `ingest_batch` has
//! already added as entities.

use codec::{Document, Handle};

use crate::scene::model::mesh_model::MeshLodSet;
use crate::scene::Scene;

use super::consumer::{ingest_batch, E3dIngestIndex, E3dIngestStats};
use super::producer::produce_from_rvm_file;

/// Mesh pools use `FxHashMap`, which is what `Scene` stores.
type MeshPool = rustc_hash::FxHashMap<Handle, MeshLodSet>;

/// Everything a `.rvm` open contributes to a freshly opened tab.
pub struct RvmOpen {
    /// Proxy entities, centrelines, and the `E3D_*` layers they live on.
    pub document: Document,
    /// Shared mesh pool keyed by its `E3D_MESH_POOL` anchor.
    pub meshes: MeshPool,
    /// Anchor meshes; empty today but kept in step with `Scene::block_meshes`.
    pub block_meshes: MeshPool,
    /// Ingest bookkeeping so a later batch can erase this source cleanly.
    pub index: E3dIngestIndex,
    pub stats: E3dIngestStats,
}

/// Parse `path` and pour it into a `Document` plus mesh pools.
pub fn load_rvm(path: &std::path::Path) -> Result<RvmOpen, String> {
    let batch = produce_from_rvm_file(path)?;
    ingest_into_scene(batch)
}

/// In-memory variant used by tests and by any caller that already holds the
/// bytes.
pub fn load_rvm_bytes(source_label: &str, bytes: &[u8]) -> Result<RvmOpen, String> {
    let batch = super::producer::produce_from_rvm_bytes(source_label, bytes)?;
    ingest_into_scene(batch)
}

fn ingest_into_scene(batch: super::contract::E3dSceneBatch) -> Result<RvmOpen, String> {
    let mut scene = Scene::new();
    let stats = ingest_batch(&mut scene, &batch)?;
    let document = std::mem::replace(&mut scene.document, Document::new());
    Ok(RvmOpen {
        document,
        meshes: std::mem::take(&mut scene.meshes),
        block_meshes: std::mem::take(&mut scene.block_meshes),
        index: std::mem::take(&mut scene.e3d_index),
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_rvm_fixture_opens_into_proxies_and_meshes() {
        // `e3d-model`'s own fixture: a small HVAC run with boxes, cylinders and
        // facet groups. Skipped when the sibling checkout is absent so the test
        // does not depend on a workspace layout.
        let fixture = std::path::Path::new("../e3d-model/tests/fixtures/strt/cpv-hvac6.rvm");
        if !fixture.exists() {
            return;
        }
        let opened = load_rvm(fixture).expect("RVM opens");
        assert!(opened.stats.proxies > 0, "no geometry items ingested");
        assert!(opened.stats.missing_meshes == 0, "mesh pool incomplete");
        assert_eq!(opened.index.by_geometry.len(), opened.stats.proxies);
        assert!(
            !opened.meshes.is_empty(),
            "shared mesh pool must not be empty"
        );
        // Every placed instance must reference one of the pool anchors.
        for set in opened.meshes.values() {
            assert!(set.instance_source.is_some() || set.instance_transform.is_some());
        }
        // Proxies carry the semantic record the properties panel reads.
        let (_, handle) = opened.index.by_geometry.iter().next().expect("a proxy");
        let entity = opened.document.get_entity(*handle).expect("proxy entity");
        assert!(
            crate::io::e3d::consumer::semantics_value(entity, "refno").is_some(),
            "proxy must publish its refno"
        );
    }
}

