# OpenCADStudio - Unreleased (2026-11-15 to Present)

> **Summary:** Nothing yet.

### 🚀 New Features

* **AVEVA RVM Import:** Open a standalone `.rvm` file from the file dialog. Geometry items arrive as read-only proxies carrying their `RefNo` / noun / name, shared canonical boxes and cylinders are uploaded once and instanced, and structural centrelines (including dashed broken tubes) come in on the `E3D_CENTRELINE` / `E3D_INVALID_TUBI` layers.

### 🐛 Bug Fixes

* **RVM Facets:** Triangulate a facet polygon in its own plane so inner contours (bolt holes, plate openings) are punched out instead of filled in, and wind the triangles to the polygon's own normal.
* **RVM Spheres:** Draw a `Sphere` primitive instead of dropping it silently.
* **RVM Nouns:** Take a named element's engineering noun from its nearest ancestor and fold the long-form nouns (`FTUBE`, `ELBOW`, `VALVE`, …) onto the short ones, so imported items land on the right discipline layer.
