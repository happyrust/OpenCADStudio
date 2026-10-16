//! Platform-neutral wire and in-memory contract for E3D **Scene Batch Manifests**
//! (`docs/adr/0001-e3d-scene-batch-dual-target-architecture.md`).
//!
//! This module is pure Rust with zero C++ FFI or filesystem dependencies so that
//! both the native desktop studio and the `wasm32-unknown-unknown` browser client
//! share the exact same deserialization and scene-ingestion types.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Domain tag for content-addressed boolean-baked meshes (aligned with `e3d-model`
/// tessellation v1).
const BAKED_MESH_DOMAIN: &[u8] = b"ocs/e3d-model/baked-mesh/v1\0";

/// Reusable **Canonical Primitive** archetype (`GLOSSARY.md`).
///
/// The key holds only the shape family and tessellation resolution; real-world
/// dimensions live in the instance's `mesh_to_world_mm` transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "family", rename_all = "snake_case")]
pub enum CanonicalShapeKey {
    /// Unit centered box `[-0.5, 0.5]^3`.
    BoxV1,
    /// Unit cylinder with radius `1.0`, height `1.0` (`z in [0.0, 1.0]`), and
    /// `segments` circumferential divisions.
    CylinderV3 { segments: i32 },
}

/// Content-addressed identity of a surface mesh in the shared mesh pool.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum MeshKey {
    /// Shared **Canonical Primitive** (`BoxV1` or `CylinderV3`).
    Canonical(CanonicalShapeKey),
    /// Element-local boolean or catalogue mesh addressed by its SHA-256 hex digest.
    BakedSha256(String),
}

impl MeshKey {
    /// Stable string identifier suitable for HTTP cache keys and CAD block names.
    pub fn cache_id(&self) -> String {
        match self {
            Self::Canonical(CanonicalShapeKey::BoxV1) => "e3d_canon_box_v1".to_string(),
            Self::Canonical(CanonicalShapeKey::CylinderV3 { segments }) => {
                format!("e3d_canon_cyl_v3_{segments}")
            }
            Self::BakedSha256(digest) => format!("e3d_baked_{digest}"),
        }
    }
}

/// Stable identity of a **Geometry Item** (`GLOSSARY.md`) across full subtree
/// loads and incremental session diffs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GeometryIdKey {
    /// A **Geometry Item** produced directly from a database **Model Element**.
    Element { refno: String },
    /// A synthesized straight pipe segment (**Implied Tube**) on a branch route.
    ImpliedTube {
        container_refno: String,
        route_ordinal: u32,
        from_refno: String,
        to_refno: String,
    },
    /// A primitive or facet group imported from a standalone `.rvm` file.
    RvmPrimitive {
        member_index: usize,
        geom_index: usize,
        path: String,
    },
}

impl std::fmt::Display for GeometryIdKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Element { refno } => write!(f, "element:{refno}"),
            Self::ImpliedTube {
                container_refno,
                route_ordinal,
                from_refno,
                to_refno,
            } => write!(
                f,
                "implied-tube:{container_refno}:{route_ordinal}:{from_refno}:{to_refno}"
            ),
            Self::RvmPrimitive {
                member_index,
                geom_index,
                path,
            } => write!(f, "rvm:{member_index}:{geom_index}:{path}"),
        }
    }
}

/// Compact, element-local triangle mesh with precomputed vertex normals and
/// crease feature edges.
///
/// Coordinates are stored in element-local millimeters (`f32`) so that large
/// plant world coordinates (\(10^5\text{–}10^7\text{ mm}\)) live exclusively in
/// `E3dInstanceRecord::mesh_to_world_mm` (`f64`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompactTriMesh {
    /// Local vertex positions in millimeters (`[x, y, z]`).
    pub positions: Vec<[f32; 3]>,
    /// Unit vertex normals matching `positions` 1-to-1.
    pub normals: Vec<[f32; 3]>,
    /// Triangle indices (`3 * triangle_count`), right-handed outward winding.
    pub indices: Vec<u32>,
    /// Pairs of vertex indices (`[i0, i1]`) forming sharp crease or boundary edges
    /// for wireframe and CAD Shaded-with-Edges rendering.
    pub feature_edges: Vec<[u32; 2]>,
    /// Axis-aligned bounding box `[min_mm, max_mm]` in local mesh coordinates.
    pub local_aabb_mm: [[f32; 3]; 2],
}

impl CompactTriMesh {
    /// Build a `CompactTriMesh` from raw `f64` local vertices (mm) and triangle
    /// indices, zeroing sub-\(10^{-10}\text{ mm}\) cancellation noise before
    /// computing normals, feature edges, and local AABB.
    pub fn from_local_f64(vertices: &[[f64; 3]], indices: &[u32]) -> Self {
        if vertices.is_empty() || indices.len() < 3 {
            return Self::default();
        }

        let clean = |v: f64| -> f32 {
            if v.abs() < 1e-10 {
                0.0
            } else {
                v as f32
            }
        };

        let mut positions = Vec::with_capacity(vertices.len());
        let mut min_mm = [f32::INFINITY; 3];
        let mut max_mm = [f32::NEG_INFINITY; 3];
        for &[x, y, z] in vertices {
            let p = [clean(x), clean(y), clean(z)];
            for axis in 0..3 {
                min_mm[axis] = min_mm[axis].min(p[axis]);
                max_mm[axis] = max_mm[axis].max(p[axis]);
            }
            positions.push(p);
        }

        let normals = compute_vertex_normals(&positions, indices);
        let feature_edges = extract_feature_edges(&positions, indices, 0.866_025_4); // cos(30 deg)

        Self {
            positions,
            normals,
            indices: indices.to_vec(),
            feature_edges,
            local_aabb_mm: [min_mm, max_mm],
        }
    }

    /// Number of triangles in this mesh.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Compute the deterministic SHA-256 `MeshKey::BakedSha256` for this local mesh.
    pub fn content_mesh_key(&self) -> MeshKey {
        let mut hasher = Sha256::new();
        hasher.update(BAKED_MESH_DOMAIN);
        hasher.update((self.positions.len() as u64).to_le_bytes());
        for p in &self.positions {
            for &coord in p {
                // Canonicalize -0.0 to +0.0 before hashing.
                let bits = if coord == 0.0 { 0u32 } else { coord.to_bits() };
                hasher.update(bits.to_le_bytes());
            }
        }
        hasher.update((self.indices.len() as u64).to_le_bytes());
        for &idx in &self.indices {
            hasher.update(idx.to_le_bytes());
        }
        let digest = hasher.finalize();
        MeshKey::BakedSha256(hex_encode(&digest))
    }
}

/// One placed 3D **Geometry Item** in a **Scene Batch Manifest**.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct E3dInstanceRecord {
    /// Stable identity (`Element`, `ImpliedTube`, or `RvmPrimitive`).
    pub geometry_id: GeometryIdKey,
    /// Source database reference number (`a/b`), or container `RefNo` for an
    /// **Implied Tube**.
    pub refno: String,
    /// Optional parent/container reference number (`BRAN`, `ZONE`, `PIPE`, etc.).
    pub owner_refno: Option<String>,
    /// Engineering type (`BOX`, `CYL`, `ELBO`, `VALV`, `SCTN`, `TUBI`, `PANE`, …).
    pub noun: String,
    /// Optional engineering name (e.g. `/100-B-1`).
    pub name: Option<String>,
    /// Reference to the shared or baked surface mesh in the batch's mesh pool.
    pub mesh_key: MeshKey,
    /// Column-major \(4 \times 4\) affine transform (`glam::DMat4::to_cols_array()`)
    /// mapping `CompactTriMesh` local millimeters to world millimeters.
    pub mesh_to_world_mm: [f64; 16],
    /// World-space axis-aligned bounding box `[min_mm, max_mm]` in millimeters.
    pub world_aabb_mm: [[f64; 3]; 2],
}

/// Classification of a **Centreline Representation** (`GLOSSARY.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CentrelineKind {
    /// Structural section (`SCTN`) without a profile specification (`SPRE`).
    ProfileCentreline,
    /// Structural node marker (`SNOD`) represented by cross/axis lines.
    NodeMarker,
    /// Diagnostic broken pipe segment (`is_invalid_tubi == true`), rendered dashed.
    InvalidTubi,
    /// Linear skeletal segment imported from an `.rvm` `Line` primitive.
    RvmLine,
}

/// A linear **Centreline Representation** emitted alongside or instead of a solid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct E3dCentrelineRecord {
    pub geometry_id: GeometryIdKey,
    pub refno: String,
    pub noun: String,
    pub name: Option<String>,
    pub kind: CentrelineKind,
    /// One or more polylines in world millimeters.
    pub polylines_mm: Vec<Vec<[f64; 3]>>,
}

/// Summary of the **Generation Ledger** (`GLOSSARY.md`) for conservation checks
/// and operator diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct E3dLedgerSummary {
    /// Number of **Model Elements** visited by DFS.
    pub visited: usize,
    /// Number of child elements consumed by their owning positive solid (loops,
    /// negative primitives).
    pub consumed: usize,
    /// Number of solid **Geometry Items** produced (including **Implied Tubes**).
    pub generated: usize,
    /// Number of elements intentionally skipped (zero height, no-shape).
    pub skipped: usize,
    /// Number of elements that failed solid construction due to bad data.
    pub failed: usize,
    /// Count of elements recorded as **Centreline Representation** only, by `noun`.
    pub centreline_only: BTreeMap<String, usize>,
    /// Total **Implied Tube** route slots evaluated.
    pub implied_tubes_count: usize,
    /// Number of **Implied Tubes** built as 3D solids.
    pub implied_tubes_built: usize,
    /// Human-readable diagnostic line naming any **Implied Tubes** rejected by
    /// coaxiality or bore-mismatch gates.
    pub tubes_not_drawn: Option<String>,
}

impl E3dLedgerSummary {
    /// Conservation invariant (`e3d_model::Report::accounts_for`): every element
    /// in the subtree must be either visited or consumed by an owner.
    pub fn accounts_for(&self, tree_total: usize) -> bool {
        self.visited + self.consumed == tree_total
    }
}

/// Platform-neutral **Scene Batch Manifest** (`GLOSSARY.md`).
///
/// Over HTTP (`/api/v1/e3d/generate` or `/api/v1/e3d/increment`), the server
/// sends this manifest first; the Web client checks which `required_meshes` are
/// missing from its local cache and fetches only the missing `MeshKey` blobs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct E3dBatchManifest {
    /// Human-readable origin label (e.g. `SITE /100`, `PIPE 26229/1509`, or `.rvm` filename).
    pub source_label: String,
    /// `false` for a full subtree/file load; `true` for a session-window delta.
    pub is_incremental: bool,
    /// Added or updated 3D **Geometry Items**.
    pub upserted: Vec<E3dInstanceRecord>,
    /// Added or updated **Centreline Representations**.
    pub centrelines: Vec<E3dCentrelineRecord>,
    /// **Geometry Items** retired by an incremental session update.
    pub removed: Vec<GeometryIdKey>,
    /// Deduplicated list of every `MeshKey` referenced by `upserted`.
    pub required_meshes: Vec<MeshKey>,
    /// Conservation and diagnostic ledger for this batch.
    pub ledger: E3dLedgerSummary,
}

/// Complete in-memory package pairing an [`E3dBatchManifest`] with its resolved
/// [`CompactTriMesh`] pool.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct E3dSceneBatch {
    pub manifest: E3dBatchManifest,
    pub meshes: BTreeMap<MeshKey, CompactTriMesh>,
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn compute_vertex_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut accum = vec![[0.0f32; 3]; positions.len()];
    for tri in indices.chunks_exact(3) {
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
            continue;
        }
        let p0 = positions[i0];
        let p1 = positions[i1];
        let p2 = positions[i2];
        let ux = p1[0] - p0[0];
        let uy = p1[1] - p0[1];
        let uz = p1[2] - p0[2];
        let vx = p2[0] - p0[0];
        let vy = p2[1] - p0[1];
        let vz = p2[2] - p0[2];
        let nx = uy * vz - uz * vy;
        let ny = uz * vx - ux * vz;
        let nz = ux * vy - uy * vx;
        for &idx in &[i0, i1, i2] {
            accum[idx][0] += nx;
            accum[idx][1] += ny;
            accum[idx][2] += nz;
        }
    }
    for n in &mut accum {
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len > 1e-12 {
            n[0] /= len;
            n[1] /= len;
            n[2] /= len;
        } else {
            *n = [0.0, 0.0, 1.0];
        }
    }
    accum
}

fn extract_feature_edges(
    positions: &[[f32; 3]],
    indices: &[u32],
    cos_threshold: f32,
) -> Vec<[u32; 2]> {
    let face_count = indices.len() / 3;
    let mut face_normals = Vec::with_capacity(face_count);
    // Map undirected edge (min_v, max_v) -> (first_face_idx, count_or_second_face)
    let mut edge_faces: HashMap<(u32, u32), (usize, Option<usize>)> =
        HashMap::with_capacity(indices.len());

    for (f_idx, tri) in indices.chunks_exact(3).enumerate() {
        let (i0, i1, i2) = (tri[0], tri[1], tri[2]);
        let (u0, u1, u2) = (i0 as usize, i1 as usize, i2 as usize);
        if u0 >= positions.len() || u1 >= positions.len() || u2 >= positions.len() {
            face_normals.push([0.0f32, 0.0, 1.0]);
            continue;
        }
        let p0 = positions[u0];
        let p1 = positions[u1];
        let p2 = positions[u2];
        let ux = p1[0] - p0[0];
        let uy = p1[1] - p0[1];
        let uz = p1[2] - p0[2];
        let vx = p2[0] - p0[0];
        let vy = p2[1] - p0[1];
        let vz = p2[2] - p0[2];
        let nx = uy * vz - uz * vy;
        let ny = uz * vx - ux * vz;
        let nz = ux * vy - uy * vx;
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        let normal = if len > 1e-12 {
            [nx / len, ny / len, nz / len]
        } else {
            [0.0, 0.0, 1.0]
        };
        face_normals.push(normal);

        for &(a, b) in &[(i0, i1), (i1, i2), (i2, i0)] {
            let key = if a < b { (a, b) } else { (b, a) };
            edge_faces
                .entry(key)
                .and_modify(|entry| {
                    if entry.1.is_none() {
                        entry.1 = Some(f_idx);
                    }
                })
                .or_insert((f_idx, None));
        }
    }

    let mut feature_edges = Vec::new();
    for (&(a, b), &(f0, f1_opt)) in &edge_faces {
        match f1_opt {
            None => {
                // Boundary edge.
                feature_edges.push([a, b]);
            }
            Some(f1) => {
                let n0 = face_normals[f0];
                let n1 = face_normals[f1];
                let dot = n0[0] * n1[0] + n0[1] * n1[1] + n0[2] * n1[2];
                if dot < cos_threshold {
                    feature_edges.push([a, b]);
                }
            }
        }
    }
    feature_edges.sort_unstable();
    feature_edges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_noise_is_zeroed_in_baked_mesh_key() {
        let verts_a = [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 100.0, 0.0]];
        // Simulate ~1e-14 floating-point cancellation residue after world.inverse()
        let verts_b = [
            [-4.2e-14, 1.1e-15, 0.0],
            [100.0, -2.0e-14, 0.0],
            [0.0, 100.0, 8.0e-15],
        ];
        let indices = [0u32, 1, 2];
        let mesh_a = CompactTriMesh::from_local_f64(&verts_a, &indices);
        let mesh_b = CompactTriMesh::from_local_f64(&verts_b, &indices);
        assert_eq!(mesh_a.content_mesh_key(), mesh_b.content_mesh_key());
    }
}
