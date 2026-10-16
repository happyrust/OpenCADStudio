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
use e3d_model::rvm_read::{Primitive as RvmPrimitive, RvmFile};
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
        let noun = infer_noun_from_rvm_name(&member.name);
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

fn infer_noun_from_rvm_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.starts_with('/') {
        return "E3D".to_string();
    }
    trimmed
        .split_whitespace()
        .next()
        .unwrap_or("RVM")
        .to_ascii_uppercase()
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
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&outer.vertices);
        for i in 1..(outer.vertices.len() as u32 - 1) {
            indices.extend_from_slice(&[base, base + i, base + i + 1]);
        }
    }
    (vertices, indices)
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
}
