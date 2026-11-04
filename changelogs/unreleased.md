# OpenCADStudio - Unreleased (2026-11-01 to Present)

> **Summary:** Added chained action sequencing to dynamic block parameters and corrected block attribute propagation and visibility-state behavior.

### 🌟 Notable Release Notes

* **Chained Parameter Actions:** A parameter can now chain its actions in a named order, authored from the Actions tab and named by its evaluation graph node. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Block Attribute Propagation:** Inserted references receive the attributes their definitions declare, bare references fill from their definition, and copies keep their own identity. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*
* **Visibility State Handling:** A state hides only the objects it names, objects outside the state set are left alone, and the first state keeps every object visible. *(Contributed by [@KarimJerbi](https://github.com/KarimJerbi))*

### 🚀 New Features

* **AVEVA RVM Import:** Open a standalone `.rvm` file from the file dialog. Geometry items arrive as read-only proxies carrying their `RefNo` / noun / name, shared canonical boxes and cylinders are uploaded once and instanced, and structural centrelines (including dashed broken tubes) come in on the `E3D_CENTRELINE` / `E3D_INVALID_TUBI` layers.
* **Quad Cursor Menu:** A context-aware command menu at the cursor with panels per entity type, hover dwell, right-click moments, keyboard picking (`Enter`, `1-9`, `Tab`), and `QUAD`/`QUADWIDTH`/`QUADCOMMANDLAUNCH` sysvars. Off by default; opted into via `SHIFT+F12`, the status-bar QUAD pill, or Options → Quad Cursor Menu.
* **Plugin Quad Surface (API v15):** Plugins register entity-type-bound quad panels with custom icons; command tiles dispatch host-side, callback tiles notify the owning plugin with the quad target.
* **Chained Actions:** Chain a parameter's actions in the named order from the Actions tab, with the chained action named when it is authored and resolved through its evaluation graph node.
* **Parameter Value Evaluation:** A written parameter value moves the reference it was set on.

### 🐛 Bug Fixes

* **RVM Facets:** Triangulate a facet polygon in its own plane so inner contours (bolt holes, plate openings) are punched out instead of filled in, and wind the triangles to the polygon's own normal.
* **RVM Spheres:** Draw a `Sphere` primitive instead of dropping it silently.
* **RVM Nouns:** Take a named element's engineering noun from its nearest ancestor and fold the long-form nouns (`FTUBE`, `ELBOW`, `VALVE`, …) onto the short ones, so imported items land on the right discipline layer.
* **Attribute Insertion:** Give inserted references the attributes their definitions declare, and fill a bare reference from its definition.
* **Attribute Copies:** Keep a reference's own identity when it is copied.
* **Attributed Marker:** Make the attributed marker follow the definition body.
* **Visibility States:** Keep every object visible in the first state, and leave objects outside the state set alone.
* **Explodable Flag:** Make the explodable flag decide against the explode command.
