# 0002. Lightweight CAD proxy entities with MeshInstanceSource sharing and BevyBridge M2 chunk-rebased mirroring

Date: 2026-10-14
Status: Accepted

## Context

Once an `E3dSceneBatch` arrives in `OpenCADStudio` (either from an in-process native worker or over HTTP in WASM), its **Geometry Items** (`Element` and `ImpliedTube`), shared meshes, and **Centreline Representations** must be represented in the studio's document (`CadDocument`), scene (`Scene`), and 3D viewport pipelines (both the in-house `wgpu` mesh pipeline and the embedded Bevy PBR pipeline `BevyBridge`).

Plant models present three scale and precision challenges:
1. **Entity count and triangle volume**: A single zone or pipe rack can contain tens of thousands of **Geometry Items** and millions of triangles. Converting each item into a full `AcDb3dSolid` or `AcDbPolyFaceMesh` inside `CadDocument` would duplicate vertices in memory and make document operations sluggish.
2. **Large plant coordinates (millimeters)**: E3D plant coordinates are authored in millimeters and routinely sit at \(10^5\text{–}10^7\text{ mm}\) from the project origin. Direct conversion to single-precision `f32` world vertices causes severe vertex jitter during close-up orbit and inspection.
3. **CAD workflow integration**: Users still expect every **Geometry Item** to participate in selection picking, layer/discipline visibility (`PIPE`, `EQUI`, `STRU`, `HVAC`), the properties inspector (`RefNo`, `noun`, `name`, parent path), section plane clipping, and export, while preventing accidental destructive overwrite of read-only source files (`.rvm` / Dabacon).

## Options Considered

1. **Convert every E3D mesh into native `AcDbPolyFaceMesh` / `AcDbMesh` DWG entities** — Materialize all vertices and faces inside `CadDocument`.
   - *Pros*: Zero changes to existing entity exporters.
   - *Cons*: Loses `PrimitiveInstance` / catalogue mesh sharing in `CadDocument`, explodes memory usage, and cannot cleanly represent `GeometryId::ImpliedTube` incremental replacement.
2. **Render E3D models only inside an isolated Bevy world, bypassing `CadDocument` and `Scene` entirely** — Keep `Scene` unaware of E3D items.
   - *Pros*: Fast initial prototype for Bevy rendering.
   - *Cons*: Breaks CPU ray picking, selection highlighting, properties panel inspection, CAD layer filtering, section plane clipping, and causes the default non-`bevy3d` build to show nothing.
3. **Register lightweight read-only CAD proxy entities in `CadDocument` + shared `MeshInstanceSource` in `Scene::meshes` + `BevyBridge` M2 incremental mirror with spatial chunk origin rebasing** — Split semantic ownership from heavy geometry storage.
   - *Pros*: Preserves 100% of CAD interactions (picking, layers, properties, section clipping, export) in both the in-house renderer and `BevyBridge`, shares canonical/catalogue GPU buffers via `MeshInstanceSource`, and eliminates `f32` coordinate jitter.
   - *Cons*: Requires implementing `BevyBridge` Milestone 2 (M2 Mirror Layer) and maintaining a `GeometryIdKey <-> Handle` index in `Scene`.

## Decision

We choose **Option 3**:

1. **Read-Only Source & Lightweight Proxy Entities**:
   - Like `.pid` (`src/io/pid/`), E3D project/RVM sources are marked read-only in `is_read_only_source_path` so "Save" prompts "Save As (.dwg/.dxf)" rather than overwriting source files.
   - Each **Geometry Item** (`GeometryIdKey`) is registered in `CadDocument` as a lightweight proxy entity (carrying its world bounding box, discipline/noun layer, and `E3D_SEMANTICS` XDATA with `refno`, `noun`, `name`, `geometry_id`, and `owner_refno`) without duplicating triangle arrays inside `CadDocument`.
   - **Centreline Representations** (`centreline_only` structural members, `SNOD` markers, and dashed `is_invalid_tubi` diagnostic segments) are emitted as line/polyline entities on dedicated sublayers (`E3D_CENTRELINE`, `E3D_INVALID_TUBI`).
2. **In-House `Scene::meshes` with `MeshInstanceSource` Sharing & Dual-Float Precision**:
   - Shared meshes (`MeshKey`: `BoxV1`, `CylinderV3`, catalogue parts, and repeated boolean meshes) are stored once as block/canonical entries in `Scene::meshes` (`MeshLodSet` with 3 LOD levels and crease feature edges `edge_verts`), and individual **Geometry Items** reference them via `MeshInstanceSource` with their `DMat4` world transform.
   - Unique boolean meshes use `OpenCADStudio`'s existing dual-float (`verts` + `verts_low`) vertex representation for jitter-free in-house rendering.
3. **`BevyBridge` M2 Mirror Layer with Spatial Chunk Origin Rebasing**:
   - When the `bevy3d` feature is active, `BevyBridge` incrementally syncs `Scene::meshes` into Bevy ECS using a monotonic generation watermark and `GeometryIdKey <-> Entity` mapping.
   - Unique and shared `MeshKey` assets map to shared `bevy::asset::Handle<Mesh>` instances (avoiding vertex duplication in Bevy).
   - World transforms in millimeters (`DMat4`) are grouped into \(100\text{ m}\) (\(10^5\text{ mm}\)) spatial chunks (`ChunkOrigin`), subtracting the chunk origin in `f64` before converting to `f32` `Transform` relative to a camera-relative chunk root entity, preventing large-coordinate precision loss in Bevy's `f32` PBR pipeline.
   - CAD wireframes, selection highlights, and **Centreline Representations** continue to render in the native `OpenCADStudio` overlay pass, depth-tested against Bevy's shared Reversed-Z (`Depth32Float`) buffer.

## Consequences

- Both the default `wgpu` build and `--features bevy3d` render E3D models with full instancing, picking, layer control, and property inspection.
- Incremental updates (`increment_update` returning `Regenerate` / `Remove` by `GeometryId`) execute in \(O(\Delta)\) time by looking up the `GeometryIdKey <-> Handle` index in `Scene` and dirty-marking only affected handles for `BevyBridge` M2.
- Completing `BevyBridge` M2 directly advances the roadmap already established in `docs/bevy-viewport-integration.md`.
