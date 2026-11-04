//! Native E3D / RVM producer (`cfg(not(target_arch = "wasm32"))`).
//!
//! Bridges `e3d-model` (`generate_subtree_with_cache`, `primitive_instance`,
//! and `rvm_read::RvmFile`) into the platform-neutral [`E3dSceneBatch`] contract
//! (`docs/adr/0001-e3d-scene-batch-dual-target-architecture.md`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use e3d_io::db_element::DbElement;
use e3d_model::catalogue::CatalogueMeshCache;
use e3d_model::elmodl::{GeneratedElement, GeometryId};
use e3d_model::mesh::TriMesh;
use e3d_model::pipeline::{generate_subtree_with_cache, GenerationOutcome, Report};
use e3d_model::primitive_instance::{canonical_primitive_mesh, PrimitiveMeshKey};
use e3d_model::rvm_read::{Primitive as RvmPrimitive, RvmFile, RvmMember};
use e3d_model::transform::dmat4_to_affine4x3;
use glam::{DMat4, DVec3};

use super::contract::{
    CanonicalShapeKey, CentrelineKind, CompactTriMesh, E3dBatchManifest, E3dCentrelineRecord,
    E3dInstanceRecord, E3dLedgerSummary, E3dSceneBatch, GeometryIdKey, MeshKey,
};

/// Run `e3d_model::generate_subtree_with_cache` on `root` and package the result
/// into a platform-neutral [`E3dSceneBatch`].
pub fn produce_subtree(
    root: DbElement,
    cache: &mut CatalogueMeshCache,
) -> Result<(GenerationOutcome, E3dSceneBatch), String> {
    let label = format!(
        "{} {}",
        root.element_type().unwrap_or_else(|_| "ELEM".to_string()),
        root.refno()
    );
    let outcome = generate_subtree_with_cache(root, cache)
        .map_err(|err| format!("E3D subtree generation failed for {label}: {err}"))?;
    let batch = produce_from_outcome(label, &outcome)?;
    Ok((outcome, batch))
}

/// Convert an already-computed [`GenerationOutcome`] into an [`E3dSceneBatch`].
///
/// - **Canonical Primitives** (`BoxV1`, `CylinderV3`) emit a single shared
///   [`MeshKey::Canonical`] entry in `batch.meshes` and fold
///   `element.world * instance.local_transform` into `mesh_to_world_mm`.
/// - **Boolean-baked and Catalogue items** are transformed back into element-local
///   coordinates (`element.world.inverse()`) before hashing into
///   [`MeshKey::BakedSha256`], so identical components placed at different plant
///   coordinates share a single mesh buffer and avoid large-coordinate `f32`
///   cancellation.
pub fn produce_from_outcome(
    source_label: impl Into<String>,
    outcome: &GenerationOutcome,
) -> Result<E3dSceneBatch, String> {
    let mut meshes: BTreeMap<MeshKey, CompactTriMesh> = BTreeMap::new();
    let mut required_set: BTreeSet<MeshKey> = BTreeSet::new();
    let mut upserted = Vec::with_capacity(outcome.elements.len());
    let mut centrelines = Vec::new();

    for element in &outcome.elements {
        let geometry_id = map_geometry_id(&element.geometry_id);
        let owner_refno = match &element.geometry_id {
            GeometryId::Element { .. } => None,
            GeometryId::ImpliedTube {
                container_refno, ..
            } => Some(container_refno.clone()),
        };

        if element.is_invalid_tubi {
            if let Some((lo, hi)) = element.mesh().aabb() {
                centrelines.push(E3dCentrelineRecord {
                    geometry_id,
                    refno: element.refno.to_string(),
                    noun: element.noun.clone(),
                    name: element.name.clone(),
                    kind: CentrelineKind::InvalidTubi,
                    polylines_mm: vec![vec![lo, hi]],
                });
            }
            continue;
        }

        let Some((mesh_key, mesh_to_world, world_aabb_mm)) =
            extract_element_geometry(element, &mut meshes)?
        else {
            continue;
        };

        required_set.insert(mesh_key.clone());
        upserted.push(E3dInstanceRecord {
            geometry_id,
            refno: element.refno.to_string(),
            owner_refno,
            noun: element.noun.clone(),
            name: element.name.clone(),
            mesh_key,
            mesh_to_world_mm: mesh_to_world.to_cols_array(),
            world_aabb_mm,
        });
    }

    let manifest = E3dBatchManifest {
        source_label: source_label.into(),
        is_incremental: false,
        upserted,
        centrelines,
        removed: Vec::new(),
        required_meshes: required_set.into_iter().collect(),
        ledger: summarize_report(&outcome.report),
    };

    Ok(E3dSceneBatch { manifest, meshes })
}

/// Parse an AVEVA `.rvm` file from disk and convert its `CNTB` / `PRIM` hierarchy
/// into an [`E3dSceneBatch`].
pub fn produce_from_rvm_file(path: &Path) -> Result<E3dSceneBatch, String> {
    let label = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let rvm = RvmFile::read(path).map_err(|err| format!("{err:#}"))?;
    produce_from_rvm(&label, &rvm)
}

/// Parse in-memory `.rvm` bytes into an [`E3dSceneBatch`].
pub fn produce_from_rvm_bytes(
    source_label: impl Into<String>,
    bytes: &[u8],
) -> Result<E3dSceneBatch, String> {
    let label = source_label.into();
    let rvm = RvmFile::parse(bytes).map_err(|err| format!("parse RVM {label}: {err:#}"))?;
    produce_from_rvm(&label, &rvm)
}

fn produce_from_rvm(source_label: &str, rvm: &RvmFile) -> Result<E3dSceneBatch, String> {
    let mut meshes: BTreeMap<MeshKey, CompactTriMesh> = BTreeMap::new();
    let mut required_set: BTreeSet<MeshKey> = BTreeSet::new();
    let mut upserted = Vec::new();
    let mut centrelines = Vec::new();

    for (member_index, member) in rvm.members.iter().enumerate() {
        let noun = infer_noun_from_rvm_name(rvm, member);
        for (geom_index, geom) in member.geometries.iter().enumerate() {
            let geometry_id = GeometryIdKey::RvmPrimitive {
                member_index,
                geom_index,
                path: member.path.clone(),
            };
            let rot = geom.rotation();
            let world_mat = DMat4::from_cols_array(&[
                rot[0],
                rot[1],
                rot[2],
                0.0,
                rot[3],
                rot[4],
                rot[5],
                0.0,
                rot[6],
                rot[7],
                rot[8],
                0.0,
                geom.translation_mm[0],
                geom.translation_mm[1],
                geom.translation_mm[2],
                1.0,
            ]);

            match &geom.primitive {
                RvmPrimitive::Line(params) => {
                    let half = params[0].abs() * 0.5;
                    let p0 = geom.to_world_mm([-half, 0.0, 0.0]);
                    let p1 = geom.to_world_mm([half, 0.0, 0.0]);
                    centrelines.push(E3dCentrelineRecord {
                        geometry_id,
                        refno: member.path.clone(),
                        noun: noun.clone(),
                        name: Some(member.name.clone()),
                        kind: CentrelineKind::RvmLine,
                        polylines_mm: vec![vec![p0, p1]],
                    });
                }
                RvmPrimitive::Box([lx, ly, lz]) if *lx > 0.0 && *ly > 0.0 && *lz > 0.0 => {
                    let canon_key = CanonicalShapeKey::BoxV1;
                    let mesh_key = ensure_canonical_mesh(canon_key, &mut meshes)?;
                    let scale = DMat4::from_scale(DVec3::new(*lx, *ly, *lz));
                    let mesh_to_world = world_mat * scale;
                    if let Some(aabb) = geom.world_aabb_mm() {
                        required_set.insert(mesh_key.clone());
                        upserted.push(E3dInstanceRecord {
                            geometry_id,
                            refno: member.path.clone(),
                            owner_refno: None,
                            noun: noun.clone(),
                            name: Some(member.name.clone()),
                            mesh_key,
                            mesh_to_world_mm: mesh_to_world.to_cols_array(),
                            world_aabb_mm: [
                                [aabb[0], aabb[1], aabb[2]],
                                [aabb[3], aabb[4], aabb[5]],
                            ],
                        });
                    }
                }
                RvmPrimitive::Cylinder([radius, height]) if *radius > 0.0 && *height > 0.0 => {
                    let segments = e3d_model::solid::cylinder_segments(*radius);
                    let canon_key = CanonicalShapeKey::CylinderV3 { segments };
                    let mesh_key = ensure_canonical_mesh(canon_key, &mut meshes)?;
                    // RVM cylinders are centered on Z in [-height/2, +height/2], whereas
                    // Canonical CylinderV3 spans Z in [0, 1].
                    let local_shift = DMat4::from_scale_rotation_translation(
                        DVec3::new(*radius, *radius, *height),
                        glam::DQuat::IDENTITY,
                        DVec3::new(0.0, 0.0, -0.5 * *height),
                    );
                    let mesh_to_world = world_mat * local_shift;
                    if let Some(aabb) = geom.world_aabb_mm() {
                        required_set.insert(mesh_key.clone());
                        upserted.push(E3dInstanceRecord {
                            geometry_id,
                            refno: member.path.clone(),
                            owner_refno: None,
                            noun: noun.clone(),
                            name: Some(member.name.clone()),
                            mesh_key,
                            mesh_to_world_mm: mesh_to_world.to_cols_array(),
                            world_aabb_mm: [
                                [aabb[0], aabb[1], aabb[2]],
                                [aabb[3], aabb[4], aabb[5]],
                            ],
                        });
                    }
                }
                RvmPrimitive::FacetGroup(polygons) => {
                    let (verts, indices) = triangulate_rvm_polygons(polygons);
                    if indices.len() >= 3 {
                        let compact = CompactTriMesh::from_local_f64(&verts, &indices);
                        let mesh_key = compact.content_mesh_key();
                        meshes.entry(mesh_key.clone()).or_insert(compact);
                        if let Some(aabb) = geom.world_aabb_mm() {
                            required_set.insert(mesh_key.clone());
                            upserted.push(E3dInstanceRecord {
                                geometry_id,
                                refno: member.path.clone(),
                                owner_refno: None,
                                noun: noun.clone(),
                                name: Some(member.name.clone()),
                                mesh_key,
                                mesh_to_world_mm: world_mat.to_cols_array(),
                                world_aabb_mm: [
                                    [aabb[0], aabb[1], aabb[2]],
                                    [aabb[3], aabb[4], aabb[5]],
                                ],
                            });
                        }
                    }
                }
                _ => {
                    // Other analytic RVM primitives (Dish, Torus, Pyramid, Snout, Sphere):
                    // build a bounding hull mesh from local_hull_points_mm or tessellate via solid.
                    if let Some(compact) = build_rvm_analytic_mesh(&geom.primitive) {
                        let mesh_key = compact.content_mesh_key();
                        meshes.entry(mesh_key.clone()).or_insert(compact);
                        if let Some(aabb) = geom.world_aabb_mm() {
                            required_set.insert(mesh_key.clone());
                            upserted.push(E3dInstanceRecord {
                                geometry_id,
                                refno: member.path.clone(),
                                owner_refno: None,
                                noun: noun.clone(),
                                name: Some(member.name.clone()),
                                mesh_key,
                                mesh_to_world_mm: world_mat.to_cols_array(),
                                world_aabb_mm: [
                                    [aabb[0], aabb[1], aabb[2]],
                                    [aabb[3], aabb[4], aabb[5]],
                                ],
                            });
                        }
                    }
                }
            }
        }
    }

    let ledger = E3dLedgerSummary {
        visited: rvm.members.len(),
        consumed: 0,
        generated: upserted.len(),
        skipped: 0,
        failed: 0,
        centreline_only: if centrelines.is_empty() {
            BTreeMap::new()
        } else {
            BTreeMap::from([("LINE".to_string(), centrelines.len())])
        },
        implied_tubes_count: 0,
        implied_tubes_built: 0,
        tubes_not_drawn: None,
    };

    Ok(E3dSceneBatch {
        manifest: E3dBatchManifest {
            source_label: source_label.to_string(),
            is_incremental: false,
            upserted,
            centrelines,
            removed: Vec::new(),
            required_meshes: required_set.into_iter().collect(),
            ledger,
        },
        meshes,
    })
}

fn extract_element_geometry(
    element: &GeneratedElement,
    meshes: &mut BTreeMap<MeshKey, CompactTriMesh>,
) -> Result<Option<(MeshKey, DMat4, [[f64; 3]; 2])>, String> {
    let world_mesh = element.mesh();
    let Some((world_lo, world_hi)) = world_mesh.aabb() else {
        return Ok(None);
    };

    let world_ocs = DMat4::from_cols_array(&element.world.to_cols_array());

    if let Some(instance) = element.primitive_instance.as_ref() {
        let canon_key = match instance.key {
            PrimitiveMeshKey::BoxV1 => CanonicalShapeKey::BoxV1,
            PrimitiveMeshKey::CylinderV3 { segments } => CanonicalShapeKey::CylinderV3 { segments },
        };
        let mesh_key = ensure_canonical_mesh(canon_key, meshes)?;
        let local_ocs = DMat4::from_cols_array(&instance.local_transform.to_cols_array());
        let mesh_to_world = world_ocs * local_ocs;
        return Ok(Some((mesh_key, mesh_to_world, [world_lo, world_hi])));
    }

    // Localize world-space boolean/catalogue mesh via `element.world.inverse()`
    // so identical components at different world positions share one MeshKey
    // and vertices stay near (0, 0, 0) in f32.
    let has_valid_world = element.world.determinant().abs() > 1e-12;
    let affine = if has_valid_world {
        dmat4_to_affine4x3(element.world.inverse())
    } else {
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]
    };
    let local_tri_mesh = match element.parts.as_deref() {
        Some(parts) => {
            let transformed: Vec<_> = parts.iter().map(|m| m.transform(&affine)).collect();
            TriMesh::from_manifolds(&transformed)
        }
        None => {
            let local_solid = element.solid.transform(&affine);
            TriMesh::from_manifold(&local_solid)
        }
    };

    if local_tri_mesh.indices.len() < 3 {
        return Ok(None);
    }

    let compact = CompactTriMesh::from_local_f64(&local_tri_mesh.vertices, &local_tri_mesh.indices);
    let mesh_key = compact.content_mesh_key();
    meshes.entry(mesh_key.clone()).or_insert(compact);
    let mesh_to_world = if has_valid_world {
        world_ocs
    } else {
        DMat4::IDENTITY
    };

    Ok(Some((mesh_key, mesh_to_world, [world_lo, world_hi])))
}

fn ensure_canonical_mesh(
    canon_key: CanonicalShapeKey,
    meshes: &mut BTreeMap<MeshKey, CompactTriMesh>,
) -> Result<MeshKey, String> {
    let mesh_key = MeshKey::Canonical(canon_key);
    if !meshes.contains_key(&mesh_key) {
        let prim_key = match canon_key {
            CanonicalShapeKey::BoxV1 => PrimitiveMeshKey::BoxV1,
            CanonicalShapeKey::CylinderV3 { segments } => PrimitiveMeshKey::CylinderV3 { segments },
        };
        let manifold = canonical_primitive_mesh(prim_key)
            .map_err(|err| format!("canonical mesh {canon_key:?}: {err}"))?;
        let tri = TriMesh::from_manifold(&manifold);
        let compact = CompactTriMesh::from_local_f64(&tri.vertices, &tri.indices);
        meshes.insert(mesh_key.clone(), compact);
    }
    Ok(mesh_key)
}

fn map_geometry_id(id: &GeometryId) -> GeometryIdKey {
    match id {
        GeometryId::Element { refno } => GeometryIdKey::Element {
            refno: refno.clone(),
        },
        GeometryId::ImpliedTube {
            container_refno,
            route_ordinal,
            from_refno,
            to_refno,
        } => GeometryIdKey::ImpliedTube {
            container_refno: container_refno.clone(),
            route_ordinal: *route_ordinal,
            from_refno: from_refno.clone(),
            to_refno: to_refno.clone(),
        },
    }
}

pub fn summarize_report(report: &Report) -> E3dLedgerSummary {
    let consumed_total: usize = report.consumed.values().copied().sum();
    let tubes_not_drawn = if report.implied_tubes.not_drawn.is_empty() {
        None
    } else {
        Some(format!(
            "{} implied tube slot(s) not drawn ({:?})",
            report
                .implied_tubes
                .not_drawn_by_reason
                .values()
                .copied()
                .sum::<usize>(),
            report.implied_tubes.not_drawn_by_reason
        ))
    };

    E3dLedgerSummary {
        visited: report.visited,
        consumed: consumed_total,
        generated: report.generated,
        skipped: report.skipped.len(),
        failed: report.failed.len(),
        centreline_only: report.centreline_only.clone(),
        implied_tubes_count: report.implied_tubes.count,
        implied_tubes_built: report.implied_tubes.built,
        tubes_not_drawn,
    }
}

/// Engineering noun for one `CNTB` member.
///
/// A named element (`/C-OR-1R345-C`) carries no noun of its own, so the noun
/// comes from the nearest ancestor that has one; an unnamed element spells its
/// noun out in full (`FTUBE 1 of BRANCH /C-OR-1R345-C`), which
/// [`normalize_rvm_noun`] folds back to the short E3D noun the discipline
/// layers key on.
fn infer_noun_from_rvm_name(rvm: &RvmFile, member: &RvmMember) -> String {
    let mut cursor = Some(member);
    while let Some(current) = cursor {
        let trimmed = current.name.trim();
        if !trimmed.starts_with('/') {
            if let Some(first) = trimmed.split_whitespace().next() {
                return normalize_rvm_noun(first);
            }
        }
        cursor = current.parent.and_then(|index| rvm.members.get(index));
    }
    "RVM".to_string()
}

/// Map the long-form nouns an `.rvm` spells out onto the short E3D nouns that
/// `consumer::discipline_layer` routes to a discipline layer.
fn normalize_rvm_noun(raw: &str) -> String {
    let upper = raw.trim().to_ascii_uppercase();
    let short = match upper.as_str() {
        "BRANCH" | "PIPE" | "PIPING" => "PIPE",
        "TUBE" | "FTUBE" | "PIPE_COMPONENT" => "TUBI",
        "ELBOW" | "BEND" => "ELBO",
        "FLANGE" => "FLAN",
        "VALVE" => "VALV",
        "GASKET" => "GASK",
        "REDUCER" => "REDU",
        "TEE" | "OLET" => "TEE",
        "WELD" => "WELD",
        "ATTACHMENT" => "ATTA",
        "NOZZLE" => "NOZZ",
        "EQUIPMENT" => "EQUI",
        "SUB_EQUIPMENT" => "SUBE",
        "STRUCTURE" | "SECTION" => "SCTN",
        "PANEL" => "PANE",
        "WALL" => "WALL",
        "GENSEC" => "GENSEC",
        "DUCT" | "HVAC" => "DUCT",
        other => other,
    };
    short.to_string()
}

fn triangulate_rvm_polygons(
    polygons: &[e3d_model::rvm_read::Polygon],
) -> (Vec<[f64; 3]>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for poly in polygons {
        let Some(outer) = poly.contours.first() else {
            continue;
        };
        if outer.vertices.len() < 3 {
            continue;
        }
        // RVM facet polygons may carry inner contours (a bolted flange's bolt
        // holes, a plate's openings). Fan-triangulating the outer ring alone
        // fills them in and self-intersects on any concave outline, so project
        // every contour onto the polygon plane and let the 2D even-odd
        // tessellator punch the holes out.
        let (poly_vertices, poly_indices) = triangulate_polygon_contours(&poly.contours);
        if poly_indices.len() < 3 {
            continue;
        }
        let base = vertices.len() as u32;
        vertices.extend(poly_vertices);
        indices.extend(poly_indices.into_iter().map(|index| base + index));
    }
    (vertices, indices)
}

/// Triangulate one RVM polygon (outer ring plus optional holes) in its own
/// plane. Falls back to a fan over the outer ring when the projection is
/// degenerate, so a bad facet still yields something rather than vanishing.
fn triangulate_polygon_contours(
    contours: &[e3d_model::rvm_read::Contour],
) -> (Vec<[f64; 3]>, Vec<u32>) {
    let outer = &contours[0];
    let Some(normal) = polygon_normal(&outer.vertices) else {
        return fan_triangulate(&outer.vertices);
    };
    let origin = outer.vertices[0];
    let (u, v) = plane_basis(normal);

    let mut rings: Vec<Vec<[f64; 2]>> = Vec::with_capacity(contours.len());
    for contour in contours {
        if contour.vertices.len() < 3 {
            continue;
        }
        let ring = contour
            .vertices
            .iter()
            .map(|point| {
                let offset = [point[0] - origin[0], point[1] - origin[1], point[2] - origin[2]];
                [dot3(offset, u), dot3(offset, v)]
            })
            .collect();
        rings.push(ring);
    }
    if rings.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let (points, triangles) = kernel::geom2d::triangulate_rings(&rings);
    if triangles.is_empty() {
        return fan_triangulate(&outer.vertices);
    }
    // The tessellator returns its own (de-duplicated, re-wound) point list in
    // the plane, so rebuild the 3D vertices from the basis rather than assuming
    // a 1:1 correspondence with the input contours.
    let vertices: Vec<[f64; 3]> = points
        .iter()
        .map(|[x, y]| {
            [
                origin[0] + u[0] * x + v[0] * y,
                origin[1] + u[1] * x + v[1] * y,
                origin[2] + u[2] * x + v[2] * y,
            ]
        })
        .collect();
    let mut indices = Vec::with_capacity(triangles.len() * 3);
    for triangle in triangles {
        for index in triangle {
            indices.push(index as u32);
        }
    }
    if winding_is_inverted(&vertices, &indices, normal) {
        for triangle in indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
    }
    (vertices, indices)
}

/// True when the tessellator's triangle winding faces away from `normal`.
fn winding_is_inverted(vertices: &[[f64; 3]], indices: &[u32], normal: [f64; 3]) -> bool {
    let mut accumulated = [0.0f64; 3];
    for triangle in indices.chunks_exact(3) {
        let (Some(a), Some(b), Some(c)) = (
            vertices.get(triangle[0] as usize),
            vertices.get(triangle[1] as usize),
            vertices.get(triangle[2] as usize),
        ) else {
            continue;
        };
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let face = cross3(ab, ac);
        accumulated[0] += face[0];
        accumulated[1] += face[1];
        accumulated[2] += face[2];
    }
    dot3(accumulated, normal) < 0.0
}

fn fan_triangulate(ring: &[[f64; 3]]) -> (Vec<[f64; 3]>, Vec<u32>) {
    if ring.len() < 3 {
        return (Vec::new(), Vec::new());
    }
    let mut indices = Vec::with_capacity((ring.len() - 2) * 3);
    for i in 1..(ring.len() as u32 - 1) {
        indices.extend_from_slice(&[0, i, i + 1]);
    }
    (ring.to_vec(), indices)
}

/// Newell's method: robust polygon normal for a non-planar or slightly bent
/// ring, which facet data frequently is.
fn polygon_normal(points: &[[f64; 3]]) -> Option<[f64; 3]> {
    if points.len() < 3 {
        return None;
    }
    let mut normal = [0.0f64; 3];
    for i in 0..points.len() {
        let current = points[i];
        let next = points[(i + 1) % points.len()];
        normal[0] += (current[1] - next[1]) * (current[2] + next[2]);
        normal[1] += (current[2] - next[2]) * (current[0] + next[0]);
        normal[2] += (current[0] - next[0]) * (current[1] + next[1]);
    }
    let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if !length.is_finite() || length < 1e-12 {
        return None;
    }
    Some([normal[0] / length, normal[1] / length, normal[2] / length])
}

/// Two unit vectors spanning the plane whose normal is `normal`.
fn plane_basis(normal: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let seed = if normal[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalize3(cross3(seed, normal));
    let v = cross3(normal, u);
    (u, v)
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize3(v: [f64; 3]) -> [f64; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if !length.is_finite() || length < 1e-12 {
        [1.0, 0.0, 0.0]
    } else {
        [v[0] / length, v[1] / length, v[2] / length]
    }
}

fn build_rvm_analytic_mesh(prim: &RvmPrimitive) -> Option<CompactTriMesh> {
    let solid = match prim {
        RvmPrimitive::Pyramid([bx, by, tx, ty, ox, oy, h]) => {
            e3d_model::solid::pyramid_solid(*bx, *by, *tx, *ty, *ox, *oy, *h).ok()?
        }
        RvmPrimitive::RectangularTorus([r_in, r_out, h, angle_rad]) => {
            e3d_model::solid::rect_torus_solid(*r_in, *r_out, *h, angle_rad.to_degrees()).ok()?
        }
        RvmPrimitive::CircularTorus([offset, radius, angle_rad]) => {
            e3d_model::solid::circ_torus_solid(*offset, *radius, angle_rad.to_degrees()).ok()?
        }
        RvmPrimitive::EllipticalDish([radius, height]) => {
            e3d_model::solid::elliptical_dish_solid(*radius, *height, 0.0).ok()?
        }
        RvmPrimitive::SphericalDish([radius, height]) => {
            e3d_model::solid::spherical_dish_solid(*radius, *height).ok()?
        }
        RvmPrimitive::Snout([rb, rt, h, ox, oy, _bsx, _bsy, _tsx, _tsy]) => {
            e3d_model::solid::snout_solid(*rb * 2.0, *rt * 2.0, *h, *ox, *oy).ok()?
        }
        RvmPrimitive::Sphere([radius]) if *radius > 0.0 => {
            // `unit_sphere_solid` is a unit-radius body; the caller's radius is a
            // uniform scale, matching how catalogue spheres place themselves.
            let segments = e3d_model::solid::cylinder_segments(*radius);
            e3d_model::solid::unit_sphere_solid(segments)
                .ok()?
                .scale(*radius, *radius, *radius)
        }
        _ => return None,
    };
    let tri = TriMesh::from_manifold(&solid);
    if tri.indices.len() < 3 {
        None
    } else {
        Some(CompactTriMesh::from_local_f64(&tri.vertices, &tri.indices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use e3d_io::refno::RefNo;
    use e3d_model::primitive_instance::PrimitiveInstance;

    #[test]
    fn canonical_boxes_and_localized_solids_deduplicate_in_mesh_pool() {
        // Construct an `e3d-model` glam::DMat4 via `e3d_model::tumat` and overwrite its 16 f64 slots
        let e3d_base = e3d_model::tumat(
            [0.0, 0.0, 0.0].into(),
            [0.0, 0.0, 1.0].into(),
            [0.0, 0.0, 0.0].into(),
        )
        .expect("tumat should return a DMat4");
        let e3d_identity = e3d_base.inverse() * e3d_base;

        let make_e3d_mat = |sx: f64, sy: f64, sz: f64, tx: f64, ty: f64, tz: f64| {
            let mut m = e3d_identity;
            let arr: &mut [f64; 16] = m.as_mut();
            arr[0] = sx;
            arr[5] = sy;
            arr[10] = sz;
            arr[12] = tx;
            arr[13] = ty;
            arr[14] = tz;
            m
        };

        let box_inst_1 = PrimitiveInstance {
            key: PrimitiveMeshKey::BoxV1,
            local_transform: make_e3d_mat(200.0, 300.0, 400.0, 0.0, 0.0, 0.0),
        };
        let box_inst_2 = PrimitiveInstance {
            key: PrimitiveMeshKey::BoxV1,
            local_transform: make_e3d_mat(500.0, 150.0, 800.0, 0.0, 0.0, 0.0),
        };

        let w1 = make_e3d_mat(1.0, 1.0, 1.0, 125_000.0, -48_000.0, 9_500.0);
        let w2 = make_e3d_mat(1.0, 1.0, 1.0, 525_000.0, 112_000.0, 24_000.0);
        let raw_box_1 = e3d_model::solid::box_solid(200.0, 300.0, 400.0).expect("box 1");
        let raw_box_2 = e3d_model::solid::box_solid(500.0, 150.0, 800.0).expect("box 2");
        let solid_1 = e3d_model::solid::placed(&raw_box_1, w1);
        let solid_2 = e3d_model::solid::placed(&raw_box_2, w2);

        // Two boolean-baked solids with identical local geometry at distant plant coordinates.
        let base_box = e3d_model::solid::box_solid(100.0, 100.0, 100.0).expect("base box");
        let hole_cyl = e3d_model::solid::cylinder_solid(25.0, 120.0).expect("hole cyl");
        let local_booled = base_box.difference(&hole_cyl);
        let w3 = make_e3d_mat(1.0, 1.0, 1.0, 800_000.0, -250_000.0, 15_000.0);
        let w4 = make_e3d_mat(1.0, 1.0, 1.0, -950_000.0, 610_000.0, 42_000.0);
        let booled_1 = e3d_model::solid::placed(&local_booled, w3);
        let booled_2 = e3d_model::solid::placed(&local_booled, w4);

        let make_el = |refno: RefNo,
                       noun: &str,
                       solid,
                       world,
                       primitive_instance: Option<PrimitiveInstance>| {
            GeneratedElement {
                geometry_id: GeometryId::Element {
                    refno: refno.to_string(),
                },
                refno,
                noun: noun.to_string(),
                is_invalid_tubi: false,
                name: Some(format!("/{noun}-{refno}")),
                solid,
                parts: None,
                part_catalogue_nouns: Vec::new(),
                part_record_repeats: Vec::new(),
                degenerate_parts: Vec::new(),
                world,
                primitive_instance,
                analytic_primitive: None,
                negatives_applied: 0,
                negatives_skipped: Vec::new(),
                notes: Vec::new(),
                joint_cuts_explained: Default::default(),
                own_ngms_cuts: Default::default(),
            }
        };

        let mut report = Report::default();
        report.visited = 4;
        report.generated = 4;

        let outcome = GenerationOutcome {
            elements: vec![
                make_el(RefNo::new(100, 1), "BOX", solid_1, w1, Some(box_inst_1)),
                make_el(RefNo::new(100, 2), "BOX", solid_2, w2, Some(box_inst_2)),
                make_el(RefNo::new(100, 3), "PANE", booled_1, w3, None),
                make_el(RefNo::new(100, 4), "PANE", booled_2, w4, None),
            ],
            report,
            bodyless: Default::default(),
            marks: Default::default(),
            point_sets: Default::default(),
            visited: vec![
                RefNo::new(100, 1),
                RefNo::new(100, 2),
                RefNo::new(100, 3),
                RefNo::new(100, 4),
            ],
            catalogue_visited: Vec::new(),
        };

        let batch = produce_from_outcome("TEST_SITE", &outcome).expect("produce batch");
        assert_eq!(batch.manifest.upserted.len(), 4);
        // Only 2 unique meshes in the pool: 1 Canonical(BoxV1) + 1 BakedSha256!
        assert_eq!(batch.meshes.len(), 2);
        assert_eq!(batch.manifest.required_meshes.len(), 2);
        assert!(batch.manifest.ledger.accounts_for(4));
        assert_eq!(
            batch.manifest.upserted[0].mesh_key,
            MeshKey::Canonical(CanonicalShapeKey::BoxV1)
        );
        assert_eq!(
            batch.manifest.upserted[0].mesh_key,
            batch.manifest.upserted[1].mesh_key
        );
        assert_eq!(
            batch.manifest.upserted[2].mesh_key,
            batch.manifest.upserted[3].mesh_key
        );
    }

    fn member(name: &str, parent: Option<usize>) -> RvmMember {
        RvmMember {
            name: name.to_string(),
            path: format!("/{name}"),
            parent,
            translation_mm: [0.0; 3],
            material: 0,
            geometries: Vec::new(),
        }
    }

    fn rvm(members: Vec<RvmMember>) -> RvmFile {
        RvmFile {
            header: Default::default(),
            members,
        }
    }

    #[test]
    fn named_rvm_members_inherit_the_nearest_ancestor_noun() {
        // A named element carries no noun of its own (`/C-OR-1R345-C`), so it
        // must take the noun of the nearest ancestor that spells one out.
        let file = rvm(vec![
            member("BRANCH 1 of ZONE /Z1", None),
            member("/C-OR-1R345-C", Some(0)),
            member("/C-OR-1R345-C/1", Some(1)),
        ]);
        assert_eq!(infer_noun_from_rvm_name(&file, &file.members[0]), "PIPE");
        assert_eq!(infer_noun_from_rvm_name(&file, &file.members[1]), "PIPE");
        assert_eq!(infer_noun_from_rvm_name(&file, &file.members[2]), "PIPE");
    }

    #[test]
    fn long_form_rvm_nouns_fold_onto_discipline_short_names() {
        assert_eq!(normalize_rvm_noun("FTUBE"), "TUBI");
        assert_eq!(normalize_rvm_noun("ELBOW"), "ELBO");
        assert_eq!(normalize_rvm_noun("VALVE"), "VALV");
        assert_eq!(normalize_rvm_noun("NOZZLE"), "NOZZ");
        assert_eq!(normalize_rvm_noun("DUCT"), "DUCT");
        // Unknown nouns pass through so the operator still sees what E3D wrote.
        assert_eq!(normalize_rvm_noun("WIDGET"), "WIDGET");
    }

    #[test]
    fn a_member_with_no_noun_anywhere_falls_back_to_rvm() {
        let file = rvm(vec![member("/A", None), member("/A/B", Some(0))]);
        assert_eq!(infer_noun_from_rvm_name(&file, &file.members[1]), "RVM");
    }

    fn contour(points: &[[f64; 3]]) -> e3d_model::rvm_read::Contour {
        e3d_model::rvm_read::Contour {
            vertices: points.to_vec(),
            normals: vec![[0.0, 0.0, 1.0]; points.len()],
        }
    }

    #[test]
    fn a_facet_hole_is_punched_out_instead_of_filled() {
        // 100 x 100 plate in z = 0 with a 40 x 40 hole in the middle. The fan
        // over the outer ring alone would cover the hole with 2 triangles; the
        // contour tessellator must emit more than that and skip the interior.
        let outer = contour(&[
            [0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
            [100.0, 100.0, 0.0],
            [0.0, 100.0, 0.0],
        ]);
        let hole = contour(&[
            [30.0, 30.0, 0.0],
            [70.0, 30.0, 0.0],
            [70.0, 70.0, 0.0],
            [30.0, 70.0, 0.0],
        ]);
        let (vertices, indices) = triangulate_polygon_contours(&[outer, hole]);
        assert!(indices.len() / 3 > 2, "hole must add triangles");

        // Total triangle area must equal the plate minus the hole.
        let mut area = 0.0;
        for triangle in indices.chunks_exact(3) {
            let a = vertices[triangle[0] as usize];
            let b = vertices[triangle[1] as usize];
            let c = vertices[triangle[2] as usize];
            let ab = [b[0] - a[0], b[1] - a[1]];
            let ac = [c[0] - a[0], c[1] - a[1]];
            area += (ab[0] * ac[1] - ab[1] * ac[0]).abs() * 0.5;
        }
        assert!(
            (area - (100.0 * 100.0 - 40.0 * 40.0)).abs() < 1e-6,
            "area = {area}"
        );
    }

    #[test]
    fn facet_winding_follows_the_polygon_normal() {
        let outer = contour(&[
            [0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
            [100.0, 100.0, 0.0],
            [0.0, 100.0, 0.0],
        ]);
        let (vertices, indices) = triangulate_polygon_contours(&[outer]);
        let mut accumulated = [0.0f64; 3];
        for triangle in indices.chunks_exact(3) {
            let a = vertices[triangle[0] as usize];
            let b = vertices[triangle[1] as usize];
            let c = vertices[triangle[2] as usize];
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let face = cross3(ab, ac);
            accumulated[0] += face[0];
            accumulated[1] += face[1];
            accumulated[2] += face[2];
        }
        assert!(
            accumulated[2] > 0.0,
            "winding must face +Z, got {accumulated:?}"
        );
    }

    #[test]
    fn a_degenerate_facet_falls_back_to_a_fan() {
        // Three collinear points have no usable normal; the fallback keeps the
        // facet rather than dropping it.
        let ring = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let (vertices, indices) = fan_triangulate(&ring);
        assert_eq!(vertices.len(), 3);
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn an_rvm_sphere_produces_a_closed_mesh() {
        let mesh = build_rvm_analytic_mesh(&RvmPrimitive::Sphere([50.0])).expect("sphere mesh");
        assert!(mesh.triangle_count() > 0);
        // A unit sphere scaled to r = 50 spans [-50, 50] on every axis.
        for axis in 0..3 {
            assert!((mesh.local_aabb_mm[0][axis] + 50.0).abs() < 1.0, "{axis}");
            assert!((mesh.local_aabb_mm[1][axis] - 50.0).abs() < 1.0, "{axis}");
        }
    }
}
