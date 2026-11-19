//! AVEVA E3D / PDMS 3D model integration (ADR-0001, ADR-0002): the
//! platform-neutral half.
//!
//! - [`contract`] defines the **Scene Batch Manifest** (`E3dBatchManifest`),
//!   content-addressed mesh keys (`MeshKey`), **Geometry Item** identities
//!   (`GeometryIdKey`), and compact triangle meshes (`CompactTriMesh`).
//! - [`consumer`] pours an [`E3dSceneBatch`] into `Document` proxies plus
//!   `Scene` meshes and centreline wires.
//!
//! Both compile on every target and depend on nothing outside this
//! repository. Nothing here produces a batch: the native producer and the
//! `.rvm` file-open entry built on it are not part of this tree at present,
//! because the model generator they wrap is not publicly available (see the
//! status note in ADR-0001).

pub mod consumer;
pub mod contract;

pub use consumer::{apply_incremental_batch, ingest_batch, E3dIngestIndex, E3dIngestStats};
pub use contract::{
    CanonicalShapeKey, CentrelineKind, CompactTriMesh, E3dBatchManifest, E3dCentrelineRecord,
    E3dInstanceRecord, E3dLedgerSummary, E3dSceneBatch, GeometryIdKey, MeshKey,
};
