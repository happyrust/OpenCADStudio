# Glossary

Canonical definitions of domain terms for integrating plant 3D engineering models (E3D / PDMS / RVM) into OpenCADStudio. Use these terms consistently in code, documentation, and conversations.

## Canonical Primitive (规范基本体)

A standard parametric shape—such as a unit box or a unit cylinder with a fixed circumferential segmentation—whose surface mesh is generated once and reused across all occurrences that have not undergone boolean cutting, differing only by their spatial placement and scale.

_Avoid_: "template mesh", "instanced primitive", "basic solid" — use **Canonical Primitive** for the reusable geometric archetype, and **Catalogue Component** for multi-primitive standard parts defined in a specification library.

## Centreline Representation (中心线表达)

A linear skeletal path emitted in place of, or alongside, a 3D surface when an engineering member intentionally has no solid representation—such as a structural profile lacking a cross-section specification, a structural node marker, or a diagnostic broken pipe segment.

_Avoid_: "wireframe", "axis line", "fallback line" — "wireframe" refers to a viewport rendering mode for all solids, whereas a **Centreline Representation** is an explicit domain output for non-solid or diagnostic items.

## Drawing Entity (图纸实体)

A CAD element registered in the active document that owns layer assignment, visibility, selection state, and extended engineering metadata, acting as the selectable and inspectable handle for a **Geometry Item** inside the studio.

_Avoid_: "model element", "mesh node", "scene object" — a **Drawing Entity** belongs to the CAD document domain and may reference a **Geometry Item** generated from a **Model Element**.

## Generation Ledger (生成总账)

The complete accounting record of a subtree generation run, partitioning every **Model Element** in the source hierarchy into mutually exclusive outcomes—solid produced, centreline-only, empty container, hidden, suppressed, skipped, failed, or consumed by a parent—so that all visited and consumed elements sum exactly to the subtree total.

_Avoid_: "import log", "build stats", "error report" — the **Generation Ledger** is a strict conservation invariant, not a best-effort log.

## Geometry Item (几何项)

An independently addressable, pickable, and incrementally updatable 3D graphic unit in the viewport. A **Geometry Item** is produced either directly from a single **Model Element** or as a derived **Implied Tube** spanning two adjacent branch components.

_Avoid_: "element", "mesh", "instance" — one **Model Element** (such as a pipe branch) can yield multiple **Geometry Items** (fittings plus multiple **Implied Tubes**), and multiple **Geometry Items** can share a single surface mesh.

## Implied Tube (隐式管身)

A straight pipe segment synthesized between two adjacent components along a pipe branch route. It has no standalone record in the engineering database; its identity is determined by its enclosing branch, its route ordinal, and its upstream and downstream neighbor elements.

_Avoid_: "virtual element", "fake pipe", "TUBI element" — an **Implied Tube** is a first-class **Geometry Item** derived from branch topology, not a database record.

## Model Element (模型单元)

An individual engineering database record identified by a unique reference number (`RefNo`) and classified by an engineering type (`noun`, such as `SITE`, `ZONE`, `PIPE`, `BRAN`, `ELBO`, or `BOX`), organized in a strict parent–child containment hierarchy.

_Avoid_: "CAD entity", "node", "object" — reserve **Model Element** for the source plant database record and **Drawing Entity** for the CAD document counterpart.

## Scene Batch Manifest (场景批次清单)

A platform-neutral delivery package describing a generated or incrementally updated set of **Geometry Items**, pairing their spatial placements, engineering metadata, **Centreline Representations**, and **Generation Ledger** with content-addressed references to shared surface meshes.

_Avoid_: "mesh dump", "scene snapshot", "RVM payload" — a **Scene Batch Manifest** supports both initial subtree loading and session-to-session incremental diffs across desktop and web runtimes.
