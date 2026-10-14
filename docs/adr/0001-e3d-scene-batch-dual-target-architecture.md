# 0001. Dual-target E3D model integration via a content-addressed Scene Batch Manifest

Date: 2026-10-14
Status: Accepted

## Context

`OpenCADStudio` targets both native desktop (`x86_64` / `aarch64`) and browser (`wasm32-unknown-unknown`) runtimes from a single codebase. We want to integrate `../e3d-model` (along with `../e3d-io` and `../e3d-attlib`) so users can load, inspect, and incrementally refresh AVEVA E3D / PDMS Dabacon databases and `.rvm` files.

Two hard technical constraints shape this integration:

1. `e3d-model` depends on `manifold-csg` (a C++17 boolean CSG library via FFI) and `e3d-io` (which performs memory-mapped / random-access reads over multi-file local Dabacon project directories). Neither C++ FFI nor local directory scanning is desirable or directly available in the browser WASM runtime.
2. Plant models contain tens of thousands of repeated **Canonical Primitives** (`BoxV1`, `CylinderV3`) and shared **Catalogue Components**, plus derived **Implied Tubes** (`GeometryId::ImpliedTube`) and **Centreline Representations** (`centreline_only` and `is_invalid_tubi`). Flattening everything into an unstructured triangle soup or a legacy CAD file format would destroy instancing, blow up network transfer size on the web, and break incremental session updates (`e3d_model::increment::increment_update`).

## Options Considered

1. **Compile `e3d-model` into both Native and WASM** — Attempt to compile `manifold-csg` C++ via Emscripten/clang for `wasm32` and stream multi-hundred-megabyte Dabacon `.000` database files into browser memory.
   - *Pros*: Single execution path.
   - *Cons*: Fragile C++ WASM toolchain, prohibitive memory and bandwidth cost in the browser, cannot scan local project directories (`DirectoryDbResolver`).
2. **couple `OpenCADStudio` directly to the external `gen-model` (`aios-database`) stack** — Require every desktop and web user to deploy `gen-model` (SurrealDB / RocksDB / `aios_core`).
   - *Pros*: Reuses existing `plant-ui` HTTP endpoints (`/model/records`, `meshes://`).
   - *Cons*: Pulls heavy database infrastructure into standalone desktop CAD workflows and prevents opening a local E3D directory or `.rvm` file out-of-the-box in `OpenCADStudio`.
3. **Define a platform-neutral `E3dSceneBatch` contract with in-process desktop generation, built-in `OpenCADStudio --http` server endpoints for Web, and an optional adapter for `gen-model`** — Keep `e3d-model` / `e3d-io` / `e3d-attlib` as native-only dependencies (`[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`), while both Native and WASM runtimes consume the exact same pure-Rust `E3dSceneBatch` (Manifest + content-addressed mesh blobs).
   - *Pros*: Desktop works standalone in-process; Web stays 100% pure Rust and fetches compact, content-addressed batches from `OpenCADStudio --http` (or an adapted `gen-model` service); `Canonical Primitive` instancing and `GeometryId` incremental updates (`Regenerate` / `Remove`) work identically on both targets.
   - *Cons*: Requires maintaining a small, versioned wire contract (`src/io/e3d/contract.rs`) shared between native producer and native/wasm consumer.

## Decision

We choose **Option 3**.

We introduce a pure-Rust, platform-neutral `E3dSceneBatch` contract compiled on both Native and WASM targets:
- **Manifest (`E3dBatchManifest`)**: Carries the **Generation Ledger** (`Report`), element hierarchy nodes, incremental tombstones (`removed: Vec<GeometryIdKey>`), **Centreline Representations** (solid-less profiles, `SNOD` markers, and dashed `is_invalid_tubi` diagnostic segments), and **Geometry Item** instance records (`GeometryIdKey`, `RefNo`, `noun`, `name`, `world_transform: [f64; 16]`, `mesh_key: MeshKey`, and bounding box in mm).
- **Content-Addressed Mesh Pool (`MeshKey` → `CompactTriMesh`)**: Separates reusable geometry (`Canonical Primitive` keys like `BoxV1` / `CylinderV3 { segments }`, shared catalogue meshes, and SHA-256 content-addressed boolean-baked meshes) from instance transforms. On the web, the client only requests `MeshKey` blobs not already present in its local cache (`POST /api/v1/e3d/meshes`).
- **Dual Producer**:
  - On **Desktop (Native)**, a background worker calls `e3d_model::generate_subtree_with_cache`, `e3d_model::increment::increment_update`, or `e3d_model::rvm_read::RvmFile` in-process and feeds `E3dSceneBatch` directly into the scene ingestion channel.
  - On **Web (WASM)**, the client calls `OpenCADStudio --http` (`/api/v1/e3d/*` in `src/rest.rs`, or a compatible `gen-model` adapter endpoint) via `fetch`, receives the `E3dBatchManifest` and missing mesh blobs, and feeds the exact same scene ingestion path.

## Consequences

- `Cargo.toml` only enables `e3d-model`, `e3d-io`, and `e3d-attlib` under `cfg(not(target_arch = "wasm32"))`; `cargo check --target wasm32-unknown-unknown` remains free of C++ FFI and filesystem dependencies.
- Canonical boxes and cylinders are transmitted and uploaded to the GPU once per viewport session regardless of whether 100 or 100,000 instances exist.
- Incremental session updates (`increment_update`) work seamlessly over HTTP and in-process by keyed replacement/removal on `GeometryIdKey`.
- An optional adapter layer can translate `gen-model` (`/model/records` + `.mesh`) into `E3dSceneBatch` without changing the studio's scene or rendering code.
