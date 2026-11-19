use super::*;

/// A summary stored into a document reads back field for field --
/// counts, the failed flag, several library roots, and both unit
/// readings -- and `take` leaves nothing behind for a second reader: the
/// document is the one it was before `store`, allocator included, which
/// is what keeps a `.pid` saved after its open byte-identical to one
/// saved before the summary rode this way.
#[test]
fn the_summary_round_trips_through_its_properties_and_take_removes_them() {
    let mut summary = ImportSummary::empty();
    summary.drawn = 335;
    summary.decoded = 310;
    summary.missing = 12;
    summary.style_tables_failed = true;
    summary.sheet_layers = 7;
    summary.layered_entities = 300;
    summary.unresolved_sheet_layers = 1;
    summary.sheet_layer_names = 4;
    summary.sheet_layers_off = 1;
    summary.driving_dimensions = 4;
    summary.template_bodies = 2;
    summary.parametric_placements = 2;
    summary.cache_bodies = 20;
    summary.library_bodies = 3;
    summary.hidden_strokes_skipped = 31;
    summary.label_anchors_undrawn = 4;
    summary.lettering_flattened = 10;
    summary.symbol_library = vec![
        PathBuf::from(r"\\server\Plant\Ref\Symbols"),
        PathBuf::from("D:/sym with spaces/=and=equals"),
    ];
    summary.unit = ImportUnit::Stated {
        unit: "m".to_string(),
        mm_per_unit: MM_PER_METRE,
    };

    let mut doc = Document::new();
    doc.summary_info
        .custom_properties
        .push(("Client".to_string(), "kept".to_string()));
    let before = doc.clone();
    assert!(
        ImportSummary::load(&doc).is_none(),
        "a document with only its own properties carries none"
    );
    summary.store(&mut doc);
    assert_eq!(ImportSummary::load(&doc).as_ref(), Some(&summary));
    assert_ne!(doc, before, "stored, it is in the document");
    assert_eq!(ImportSummary::take(&mut doc), Some(summary.clone()));
    assert!(
        ImportSummary::take(&mut doc).is_none(),
        "take drains the properties"
    );
    assert_eq!(
        doc, before,
        "taken, the document is exactly what it was -- other properties kept, no handle spent"
    );

    // Storing again replaces rather than appends, and the assumed unit
    // reads back as itself.
    summary.unit = ImportUnit::AssumedMetre;
    summary.symbol_library.clear();
    summary.store(&mut doc);
    summary.store(&mut doc);
    let read = ImportSummary::load(&doc).expect("stored");
    assert_eq!(read, summary);
    assert!(read.unit.is_assumed());
    assert_eq!(read.unit.mm_per_unit(), MM_PER_METRE);
    assert_eq!(
        doc.summary_info
            .custom_properties
            .iter()
            .filter(|(tag, _)| tag.starts_with(SUMMARY_PROPERTY_PREFIX))
            .count(),
        summary.counts().len() + 2,
        "one property per field: the counts, the failed flag, the unit"
    );
}

/// The unit is read off the first decoded record that states one this
/// importer knows; a drawing whose records state none, or an unknown
/// label, is scaled as metres and says so.
#[test]
fn the_unit_is_read_from_a_decoded_record_or_assumed_to_be_the_metre() {
    let entity =
        |confidence: PidGeometryConfidence, units: PidDrawingUnits| pid_parse::PidGraphicEntity {
            id: "e".to_string(),
            drawing_id: None,
            graphic_oid: None,
            source_layer: None,
            kind: PidGraphicKind::Point {
                position: PidPoint { x: 0.0, y: 0.0 },
            },
            coordinate_context: pid_parse::PidCoordinateContext {
                units,
                ..Default::default()
            },
            source: pid_parse::PidGraphicProvenance {
                stream_path: None,
                byte_range: None,
                record_id: None,
                record_kind: None,
                field_x: None,
                note: None,
            },
            confidence,
        };
    let known = |unit: &str| PidDrawingUnits::Known {
        unit: unit.to_string(),
    };
    let unknown = || PidDrawingUnits::Unknown {
        diagnostic: "no frame".to_string(),
    };
    let path = Path::new("unit-test.pid");
    let geometry = |entities: Vec<pid_parse::PidGraphicEntity>| NormalizedPidGeometry {
        entities,
        ..Default::default()
    };

    // A stated metre, even after an inferred record with no unit.
    let stated = ImportUnit::read(
        &geometry(vec![
            entity(PidGeometryConfidence::Inferred, unknown()),
            entity(PidGeometryConfidence::Decoded, known("m")),
        ]),
        path,
    );
    assert_eq!(
        stated,
        ImportUnit::Stated {
            unit: "m".to_string(),
            mm_per_unit: MM_PER_METRE
        }
    );
    assert!(!stated.is_assumed());
    // Millimetres are the identity.
    assert_eq!(
        ImportUnit::read(
            &geometry(vec![entity(PidGeometryConfidence::Decoded, known("mm"))]),
            path
        )
        .mm_per_unit(),
        1.0
    );
    // Nothing decoded states a unit: assumed, and a metre's worth.
    let assumed = ImportUnit::read(
        &geometry(vec![entity(PidGeometryConfidence::Decoded, unknown())]),
        path,
    );
    assert_eq!(assumed, ImportUnit::AssumedMetre);
    assert_eq!(assumed.mm_per_unit(), MM_PER_METRE);
    // A label this importer has not been shown is not guessed at.
    assert!(ImportUnit::read(
        &geometry(vec![entity(
            PidGeometryConfidence::Decoded,
            known("furlong")
        )]),
        path
    )
    .is_assumed());
    // Only decoded records are asked; a probe's unit is not a reading.
    assert!(ImportUnit::read(
        &geometry(vec![entity(PidGeometryConfidence::ProbeOnly, known("mm"))]),
        path
    )
    .is_assumed());
}

/// The file's display bit decides, and the name only speaks when the
/// file said nothing (plan 2026-09-07, L1): a layer named `Invisible`
/// the sheet displays is drawn, a layer named `Default` the sheet hides
/// is hidden, and a layer without a bit falls back to the three names.
#[test]
fn the_display_bit_outranks_the_name_and_the_name_is_only_the_fallback() {
    let layer = |name: &str, displayed: Option<bool>| PidSourceLayer {
        oid: 8,
        name: Some(name.to_string()),
        storage_path: "/".to_string(),
        displayed,
    };
    assert!(!sheet_layer_is_hidden(&layer("Invisible", Some(true))));
    assert!(sheet_layer_is_hidden(&layer("Default", Some(false))));
    assert!(sheet_layer_is_hidden(&layer("HiddenObjects", Some(false))));
    assert!(sheet_layer_is_hidden(&layer("Hidden", None)));
    assert!(sheet_layer_is_hidden(&layer("Invisible", None)));
    assert!(!sheet_layer_is_hidden(&layer("Labels", None)));
    assert!(!sheet_layer_is_hidden(&PidSourceLayer {
        oid: 8,
        name: None,
        storage_path: "/".to_string(),
        displayed: None,
    }));
}

/// The extent a user reads off the panel follows the arc's sweep rather
/// than its whole circle (plan 2026-09-18, K2): a quarter reaches one
/// corner, a half one side, a sweep across the x axis takes in the point
/// at angle zero, and coincident ends are the full circle.
#[test]
fn an_arcs_extent_is_the_sweep_it_draws_and_not_its_circle() {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    let close = |a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)| {
        (a.0 - b.0).abs() < 1e-9
            && (a.1 - b.1).abs() < 1e-9
            && (a.2 - b.2).abs() < 1e-9
            && (a.3 - b.3).abs() < 1e-9
    };
    let quarter = arc_extent((0.0, 0.0), 1.0, 0.0, FRAC_PI_2);
    assert!(close(quarter, (0.0, 0.0, 1.0, 1.0)), "{quarter:?}");
    let half = arc_extent((0.0, 0.0), 1.0, FRAC_PI_2, 3.0 * FRAC_PI_2);
    assert!(close(half, (-1.0, -1.0, 0.0, 1.0)), "{half:?}");
    let across_zero = arc_extent((0.0, 0.0), 1.0, 7.0 * FRAC_PI_4, FRAC_PI_4);
    let c = FRAC_PI_4.cos();
    assert!(close(across_zero, (c, -c, 1.0, c)), "{across_zero:?}");
    let whole = arc_extent((2.0, 3.0), 1.0, PI, PI);
    assert!(close(whole, (1.0, 2.0, 3.0, 4.0)), "{whole:?}");
    // The Manifold's end caps: two semicircles of r 35.59 facing outward
    // reach exactly the body's top and bottom and one end each.
    let mut arc = document::entities::Arc::new();
    arc.center = Vector3::new(10.0, 10.0, 0.0);
    arc.radius = 35.59;
    arc.start_angle = FRAC_PI_2;
    arc.end_angle = 3.0 * FRAC_PI_2;
    let cap = stroke_extent(&EntityType::Arc(arc)).expect("an arc is a stroke");
    assert!(
        close(cap, (10.0 - 35.59, 10.0 - 35.59, 10.0, 10.0 + 35.59)),
        "{cap:?}"
    );
    let mut label = Text::new();
    label.value = "Parametric Manifold".to_string();
    assert!(stroke_extent(&EntityType::Text(label)).is_none());
}

fn marker_at(x: f64, y: f64) -> EntityType {
    let mut marker = Circle::new();
    marker.center = Vector3::new(x, y, 0.0);
    marker.radius = SYMBOL_MARKER_RADIUS_MM;
    EntityType::Circle(marker)
}

fn line(start: (f64, f64), end: (f64, f64)) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(start.0, start.1, 0.0),
        Vector3::new(end.0, end.1, 0.0),
    ))
}

/// The marker fallback keeps the position the old anchor-relative formula
/// gave it, to the last decimal: `insertion + radius + gap` across, and
/// half a cap-height down from the insertion point.
#[test]
fn a_marker_is_named_where_the_insertion_point_formula_put_it() {
    let insertion = (40.0, 25.0);
    let anchor = symbol_label_anchor(&[marker_at(insertion.0, insertion.1)], insertion);
    assert_eq!(
        anchor.x,
        insertion.0 + SYMBOL_MARKER_RADIUS_MM + SYMBOL_LABEL_GAP_MM
    );
    assert_eq!(anchor.y, insertion.1 - SYMBOL_LABEL_HEIGHT_MM / 2.0);
}

/// A symbol whose library body is drawn away from its own origin is named
/// beside the body. Reverting to the insertion point puts the name a
/// hundred millimetres away from anything the placement drew, which on
/// `DWG-0202` was off the edge of the sheet -- see
/// `docs/analysis/2026-08-24-two-texts-outside-the-frame.md`.
#[test]
fn a_body_drawn_away_from_its_anchor_is_named_beside_the_body() {
    // `ElecTraceLine.sym` to scale: the placement is anchored at x = -25.6
    // and the body it draws sits 104mm right and 155mm up from there.
    let insertion = (-25.6, 349.9);
    let body = [
        line((78.2, 504.1), (82.1, 505.6)),
        line((82.1, 504.1), (78.2, 505.6)),
    ];
    let anchor = symbol_label_anchor(&body, insertion);

    assert_eq!(anchor.x, 82.1 + SYMBOL_LABEL_GAP_MM);
    assert_eq!(
        anchor.y,
        (504.1 + 505.6) / 2.0 - SYMBOL_LABEL_HEIGHT_MM / 2.0
    );
    assert!(
        anchor.x - insertion.0 > 100.0,
        "the name follows the body, not the anchor it was placed from"
    );
}

/// An arc contributes its whole circle and lettering contributes only the
/// point it starts from, so the extent a label and the opening view are
/// built on covers the line work either way.
#[test]
fn an_extent_covers_an_arc_and_stops_at_a_text_insertion_point() {
    let mut arc = document::entities::Arc::new();
    arc.center = Vector3::new(10.0, 20.0, 0.0);
    arc.radius = 2.5;
    arc.start_angle = 0.0;
    arc.end_angle = std::f64::consts::FRAC_PI_2;
    assert_eq!(
        drawn_extent(&EntityType::Arc(arc)),
        Some((7.5, 17.5, 12.5, 22.5))
    );

    let mut text = Text::new();
    text.value = "pump".to_string();
    text.height = 2.5;
    text.insertion_point = Vector3::new(3.0, 4.0, 0.0);
    assert_eq!(
        drawn_extent(&EntityType::Text(text)),
        Some((3.0, 4.0, 3.0, 4.0))
    );
}

/// A cached body draws its strokes on the layers the file displays and
/// none of the others (plan 2026-09-19, P-D2), and the placement is
/// measured over the same strokes. A stroke on a layer the file says
/// nothing about is drawn, and one with no layer entry at all (a body
/// from before the field existed) too.
#[test]
fn a_cached_body_draws_only_the_strokes_on_its_displayed_layers() {
    use pid_parse::{PidSymbolDefinitionRef, PidSymbolSheetLayer};
    let line = |x: f64| SymbolPrimitive::Line {
        start: (x, 0.0),
        end: (x, 0.01),
    };
    let body = PidSymbolDefinition {
        reference: PidSymbolDefinitionRef {
            site: 396,
            sheet: 113,
        },
        layers: vec![8, 9, 10],
        sheet_layers: vec![
            PidSymbolSheetLayer {
                oid: 8,
                name: Some("Default".to_string()),
                displayed: Some(true),
            },
            PidSymbolSheetLayer {
                oid: 9,
                name: Some("Construction".to_string()),
                displayed: Some(false),
            },
            PidSymbolSheetLayer {
                oid: 10,
                name: None,
                displayed: None,
            },
        ],
        primitives: vec![line(0.0), line(0.1), line(0.2), line(0.3), line(0.4)],
        primitive_layers: vec![8, 9, 10, 9],
        primitive_styles: Vec::new(),
        primitive_letterings: Vec::new(),
        dimensions: Vec::new(),
        variables: Vec::new(),
        template: None,
    };
    let x_of = |stroke: &SymbolPrimitive| match stroke {
        SymbolPrimitive::Line { start, .. } => start.0,
        other => panic!("{other:?}"),
    };
    let drawn: Vec<f64> = body.visible_strokes().map(|(p, _)| x_of(p)).collect();
    assert_eq!(
        drawn,
        vec![0.0, 0.2, 0.4],
        "Default on, Construction off, an unlisted layer and a stroke with no layer entry drawn"
    );
    let measured: Vec<f64> = body.visible_primitives().map(x_of).collect();
    assert_eq!(measured, drawn, "the panel measures the strokes on screen");

    let insertion = PidPoint { x: 0.0, y: 0.0 };
    let at = Placement {
        insertion: &insertion,
        rotation: 0.0,
        scale: [1.0, 1.0],
        projection: Projection {
            mm_per_unit: 1000.0,
            band: SheetBand::for_page(None),
        },
    };
    let mut tally = SymbolBodies::default();
    let entities = cached_body_entities(
        Some(&body),
        &at,
        &mut tally,
        &HashMap::new(),
        1.0,
        &HashMap::new(),
    )
    .expect("three strokes draw");
    assert_eq!(entities.len(), 3);
    assert_eq!(
        (tally.cache, tally.hidden_strokes_skipped),
        (1, 2),
        "one body drawn, its two Construction strokes counted as skipped"
    );
}

/// A label symbol is one SmartPlant files under a `Labels` folder -- plain
/// or `Labels - <class>` -- or the one this plant files beside its sheet
/// templates, matched by file name; the folder is read off the directory
/// segments, so a file whose name carries the word is not one, nor a
/// folder that only starts with it, and a bare `Labels` with no file is
/// nothing at all (plan 2026-10-10, label anchors).
#[test]
fn a_label_symbol_is_one_filed_under_a_labels_folder_or_named_as_filed_elsewhere() {
    for path in [
        r"\\WIN-SPID\qsmcqtaz13\Plant\Ref\Symbols\Design\Annotation\Labels\Item Note & Label.sym",
        r"\\SPID\XaLNG\P&ID Reference Data\Symbols\Piping\Labels - Piping Segments\Remarks.sym",
        r"\\WIN-SPID\x\Plant\Ref\Symbols\Equipment\Labels - Equipment\Descriptions\Vessel Description.sym",
        r"\\WIN-SPID\qsmcqtaz13\Plant\Ref\Symbols\Design\Drawing Description.sym",
        "Design/Annotation/Labels/Item Note & Label line1.5.sym",
        r"Design\DRAWING DESCRIPTION.SYM",
        "Drawing Description.sym",
    ] {
        assert!(is_label_symbol(path), "{path} is a label symbol");
    }
    for path in [
        r"\\WIN-SPID\qsmcqtaz13\Plant\Ref\Symbols\Piping\Piping OPC's\Off-Drawing.sym",
        r"\\WIN-SPID\qsmcqtaz13\Plant\Ref\Symbols\Equipment\Other Equipment\Parametric Manifold.sym",
        r"\\WIN-SPID\x\Plant\Ref\Symbols\Piping\Valves\Labels Valve.sym",
        r"\\WIN-SPID\x\Plant\Ref\Symbols\Piping\Labelsmith\Valve.sym",
        r"\\WIN-SPID\x\Plant\Ref\Symbols\Design\ Line2.sym",
        "Labels",
        "",
    ] {
        assert!(!is_label_symbol(path), "{path:?} is not a label symbol");
    }
}

/// A body is nothing but its anchor cross when every stroke the file
/// displays is a line narrower than any ISO pen: the two 0.01 mm lines of
/// an `Item Note & Label` body are, the same cross at 0.13 mm is not, a
/// cross with a hidden `NULL` beside it still is (the lettering is on a
/// switched-off layer), a stroke with no style of its own is not a
/// hairline, and a body with nothing displayed is not a cross (plan
/// 2026-10-10, label anchors).
#[test]
fn a_body_is_an_anchor_cross_when_its_displayed_strokes_are_all_hairline_lines() {
    use pid_parse::{PidSymbolDefinitionRef, PidSymbolSheetLayer};
    let hairline = |width_mm: f64| {
        Some(PrimitiveStyle {
            rgb: [0, 0, 0],
            width_mm,
            dash_mm: Vec::new(),
            paper: false,
        })
    };
    let cross = vec![
        SymbolPrimitive::Line {
            start: (0.10541, 0.13970),
            end: (0.10795, 0.13970),
        },
        SymbolPrimitive::Line {
            start: (0.10668, 0.14097),
            end: (0.10668, 0.13843),
        },
    ];
    let body = |primitives: Vec<SymbolPrimitive>,
                layers: Vec<u32>,
                styles: Vec<Option<PrimitiveStyle>>| PidSymbolDefinition {
        reference: PidSymbolDefinitionRef {
            site: 329,
            sheet: 327,
        },
        layers: vec![334, 332],
        sheet_layers: vec![
            PidSymbolSheetLayer {
                oid: 334,
                name: Some("Default".to_string()),
                displayed: Some(true),
            },
            PidSymbolSheetLayer {
                oid: 332,
                name: Some("Label".to_string()),
                displayed: Some(false),
            },
        ],
        primitives,
        primitive_layers: layers,
        primitive_styles: styles,
        primitive_letterings: Vec::new(),
        dimensions: Vec::new(),
        variables: Vec::new(),
        template: None,
    };

    assert!(
        is_anchor_cross_only(&body(
            cross.clone(),
            vec![334, 334],
            vec![hairline(0.01), hairline(0.01)]
        )),
        "two 0.01 mm lines on the displayed layer"
    );
    let mut with_lettering = cross.clone();
    with_lettering.push(SymbolPrimitive::Text {
        text: "NULL".to_string(),
        at: (0.10668, 0.13970),
    });
    assert!(
        is_anchor_cross_only(&body(
            with_lettering,
            vec![334, 334, 332],
            vec![hairline(0.01), hairline(0.01), None]
        )),
        "the template lettering sits on the switched-off Label layer"
    );
    assert!(
        !is_anchor_cross_only(&body(
            cross.clone(),
            vec![334, 334],
            vec![hairline(0.01), hairline(0.13)]
        )),
        "a 0.13 mm line is drafting"
    );
    assert!(
        !is_anchor_cross_only(&body(
            cross.clone(),
            vec![334, 334],
            vec![hairline(0.01), None]
        )),
        "a stroke with no style of its own cannot be called a hairline"
    );
    assert!(
        !is_anchor_cross_only(&body(
            cross.clone(),
            vec![332, 332],
            vec![hairline(0.01), hairline(0.01)]
        )),
        "a body with nothing displayed is not a cross"
    );
    assert!(
        !is_anchor_cross_only(&body(
            vec![SymbolPrimitive::Circle {
                center: (0.0, 0.0),
                radius: 0.001,
            }],
            vec![334],
            vec![hairline(0.01)]
        )),
        "a hairline circle is not the cross"
    );
}

/// A cached stroke's dash is its own storage's, and its colour and width
/// are its placement's (plan 2026-09-20, P-E1 / P-E8): an off-page
/// connector's dashed circle, authored `#00FEA0` 0.50 with a 3.5 / 1.75
/// dash, is repainted olive 0.35 by the solid placement style and stays
/// dashed; its solid leg stays solid; a stroke with no style of its own
/// takes only the placement's paint. The body's pattern joins the pool
/// after the sheet's own, whose name does not move. A placement style
/// that names a dash of its own wins over the stroke's.
#[test]
fn a_cached_strokes_dash_is_its_own_and_its_colour_its_placements() {
    use pid_parse::style_link::{LineSymbology, StyleHop};
    use pid_parse::{PidSymbolDefinitionRef, PidSymbolSheetLayer};

    let own = |dash_mm: Vec<f64>| PrimitiveStyle {
        rgb: [0x00, 0xFE, 0xA0],
        width_mm: 0.5,
        dash_mm,
        paper: false,
    };
    let body = PidSymbolDefinition {
        reference: PidSymbolDefinitionRef {
            site: 7559,
            sheet: 155,
        },
        layers: vec![8],
        sheet_layers: vec![PidSymbolSheetLayer {
            oid: 8,
            name: Some("Default".to_string()),
            displayed: Some(true),
        }],
        primitives: vec![
            SymbolPrimitive::Circle {
                center: (0.0, 0.0),
                radius: 0.005,
            },
            SymbolPrimitive::Line {
                start: (0.0, 0.0),
                end: (0.01, 0.0),
            },
            SymbolPrimitive::Line {
                start: (0.0, 0.0),
                end: (0.0, 0.01),
            },
        ],
        primitive_layers: vec![8, 8, 8],
        primitive_styles: vec![Some(own(vec![3.5, 1.75])), Some(own(Vec::new())), None],
        primitive_letterings: Vec::new(),
        dimensions: Vec::new(),
        variables: Vec::new(),
        template: None,
    };
    // The placement's style: olive 0.35, `COLORREF` `0x00BBGGRR`.
    let placement_style = |dash: Option<DashPattern>| ResolvedLineStyle {
        style_id: 25,
        symbology: LineSymbology {
            width_m: 0.000_35,
            colour: 0x0000_8080,
            paper: false,
        },
        dash,
        marker: None,
        complex_dash: None,
        hop: StyleHop::Direct,
    };
    let sheet_dash = || DashPattern::from_segments_m(&[0.001, 0.001]);

    let mut styles = LineStyleIndex::new();
    styles.insert(("/".to_string(), 1), placement_style(sheet_dash()));
    let mut doc = Document::new();
    let pool = register_dash_linetypes(&mut doc, &styles, std::slice::from_ref(&body), 1.0);
    assert_eq!(
        pool.get(&vec![1000, 1000]).map(String::as_str),
        Some("PID-DASH-1"),
        "the sheet's own pattern is pooled first and keeps its name"
    );
    assert_eq!(
        pool.get(&vec![3500, 1750]).map(String::as_str),
        Some("PID-DASH-2"),
        "the cached body's pattern joins the pool after it"
    );
    assert!(doc.line_types.contains("PID-DASH-2"));

    let insertion = PidPoint { x: 0.0, y: 0.0 };
    let at = Placement {
        insertion: &insertion,
        rotation: 0.0,
        scale: [1.0, 1.0],
        projection: Projection {
            mm_per_unit: 1000.0,
            band: SheetBand::for_page(None),
        },
    };
    let mut tally = SymbolBodies::default();
    let mut drawn = cached_body_entities(Some(&body), &at, &mut tally, &pool, 1.0, &HashMap::new())
        .expect("three strokes draw");
    assert_eq!(drawn.len(), 3);
    let coat = |entity: &EntityType| {
        let common = entity.common();
        (common.color, common.line_weight, common.linetype.clone())
    };
    let fresh = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).common;
    let as_built = fresh.linetype.clone();
    let olive = Color::from_rgb(0x80, 0x80, 0x00);
    assert_eq!(
        coat(&drawn[0]),
        (
            Color::from_rgb(0x00, 0xFE, 0xA0),
            LineWeight::Value(34),
            "PID-DASH-2".to_string()
        ),
        "the undercoat is the body's own colour, width and dash"
    );
    assert_eq!(
        coat(&drawn[1]).2,
        as_built,
        "a solid stroke names no linetype"
    );
    assert_eq!(
        coat(&drawn[2]),
        (fresh.color, fresh.line_weight, as_built.clone()),
        "a stroke with no style of its own is left as built"
    );

    for entity in &mut drawn {
        apply_symbology(entity, &placement_style(None), &pool, 1.0);
    }
    assert_eq!(
        coat(&drawn[0]),
        (olive, LineWeight::Value(34), "PID-DASH-2".to_string()),
        "a solid placement style repaints colour and width and leaves the dash"
    );
    assert_eq!(
        coat(&drawn[1]),
        (olive, LineWeight::Value(34), as_built.clone())
    );
    assert_eq!(coat(&drawn[2]), (olive, LineWeight::Value(34), as_built));

    for entity in &mut drawn {
        apply_symbology(entity, &placement_style(sheet_dash()), &pool, 1.0);
    }
    assert!(
        drawn
            .iter()
            .all(|entity| entity.common().linetype == "PID-DASH-1"),
        "a placement style that names a dash of its own wins over the stroke's: {:?}",
        drawn.iter().map(coat).collect::<Vec<_>>()
    );
}

/// A symbol's lettering takes its placement's colour and nothing else.
/// Measured on DWG-0201's level-gauge bubbles: `LG-Magnetic Float
/// Gauge.sym` and `LT-Magnetostrictive Level Gauge.sym` author their
/// letters in a `#FF0000` character style, the placements name the
/// instrument class colour `#008000`, and SmartPlant's screenshot
/// letters them green. The rule used to be pinned on the library-first
/// import, the one that still lettered the symbol layer; that import is
/// retired (plan 2026-09-20-retire-the-library-first-symbol-source), and
/// the branch is still a real path -- a library body standing in for a
/// placement the drawing caches nothing drawable for letters through it
/// -- so it is pinned here: a text on `PID-SYMBOL` is repainted and
/// takes no line weight, and a text on the sheet's own layer is left
/// alone, its colour being its character style's.
#[test]
fn a_symbols_lettering_takes_its_placements_colour_and_nothing_else() {
    use pid_parse::style_link::{LineSymbology, StyleHop};
    let green = ResolvedLineStyle {
        style_id: 61,
        symbology: LineSymbology {
            width_m: 0.000_18,
            colour: 0x0000_8000,
            paper: false,
        },
        dash: None,
        marker: None,
        complex_dash: None,
        hop: StyleHop::Direct,
    };
    let letters = |layer: &str| {
        let mut text = Text::new();
        text.value = "LGM".to_string();
        text.height = 2.5;
        text.common.layer = layer.to_string();
        text.common.color = Color::from_rgb(0xFF, 0x00, 0x00);
        EntityType::Text(text)
    };
    let no_dashes = HashMap::new();

    let mut on_symbol = letters(LAYER_SYMBOL);
    apply_symbology(&mut on_symbol, &green, &no_dashes, 1.0);
    assert_eq!(on_symbol.common().color, Color::from_rgb(0x00, 0x80, 0x00));
    assert_eq!(
        on_symbol.common().line_weight,
        Text::new().common.line_weight,
        "no line weight lands on lettering"
    );
    let EntityType::Text(text) = &on_symbol else {
        panic!("still a text");
    };
    assert_eq!((text.value.as_str(), text.height), ("LGM", 2.5));

    let mut on_sheet = letters(LAYER_TEXT);
    apply_symbology(&mut on_sheet, &green, &no_dashes, 1.0);
    assert_eq!(
        on_sheet.common().color,
        Color::from_rgb(0xFF, 0x00, 0x00),
        "the sheet's own lettering is coloured by its character style, not here"
    );
}

/// A body's arc runs clockwise from its start angle to its end angle,
/// and lands on the sheet as the DXF arc that covers the same points:
/// the Manifold's left cap, `270° -> 90°` about a centre on the shell's
/// left edge, is drawn as the counter-clockwise arc `90° -> 270°`, whose
/// midpoint bulges left out of the shell -- and a mirrored placement,
/// which reverses the sense again, keeps the bulge on the outside.
#[test]
fn a_bodys_arc_is_drawn_as_the_counter_clockwise_arc_over_the_same_points() {
    use std::f64::consts::{FRAC_PI_2, PI};
    let insertion = PidPoint { x: 0.0, y: 0.0 };
    let placement = |scale_y: f64| Placement {
        insertion: &insertion,
        rotation: 0.0,
        scale: [1.0, scale_y],
        projection: Projection {
            mm_per_unit: 1000.0,
            band: SheetBand::for_page(None),
        },
    };
    let left_cap = SymbolPrimitive::Arc {
        center: (0.04118, 0.08890),
        radius: 0.03559,
        start_angle: 3.0 * FRAC_PI_2,
        end_angle: FRAC_PI_2,
        clockwise: false,
    };
    let midpoint = |arc: &document::entities::Arc| {
        let sweep = (arc.end_angle - arc.start_angle).rem_euclid(2.0 * PI);
        let angle = arc.start_angle + sweep / 2.0;
        (
            arc.center.x + arc.radius * angle.cos(),
            arc.center.y + arc.radius * angle.sin(),
        )
    };

    let Some(EntityType::Arc(upright)) =
        shape_primitive(&left_cap, None, &placement(1.0), &HashMap::new())
    else {
        panic!("the cap did not build");
    };
    assert!((upright.start_angle - FRAC_PI_2).abs() < 1e-12);
    assert!((upright.end_angle - 3.0 * FRAC_PI_2).abs() < 1e-12);
    let (x, y) = midpoint(&upright);
    assert!(
        (x - (41.18 - 35.59)).abs() < 1e-6 && (y - 88.90).abs() < 1e-6,
        "the cap's midpoint is the shell's left edge less the radius: ({x}, {y})"
    );
    let cap = stroke_extent(&EntityType::Arc(upright)).expect("an arc is a stroke");
    assert!(
        (cap.0 - (41.18 - 35.59)).abs() < 1e-6 && (cap.2 - 41.18).abs() < 1e-6,
        "the cap reaches from the shell's edge out to its apex: {cap:?}"
    );

    // Mirrored about x: the same cap on a body flipped upside down still
    // bulges left, with its ends swapped top for bottom.
    let Some(EntityType::Arc(mirrored)) =
        shape_primitive(&left_cap, None, &placement(-1.0), &HashMap::new())
    else {
        panic!("the mirrored cap did not build");
    };
    let (x, y) = midpoint(&mirrored);
    assert!(
        (x - (41.18 - 35.59)).abs() < 1e-6 && (y + 88.90).abs() < 1e-6,
        "the mirrored cap still bulges out of the shell: ({x}, {y})"
    );
}

/// A half-turn cap whose file angles are one ulp past `f64::consts::PI`
/// is still a half-turn: `Parametric V Drum`'s left head reads `π -> 0`
/// with `π` stored as `3.141592653589794`, and a bare `sweep > PI`
/// comparison flipped it into its complement (the head caved into the
/// shell). With the epsilon, a placement rotated a quarter turn draws
/// the cap bulging left.
#[test]
fn a_half_turn_cap_within_one_ulp_of_pi_keeps_its_clockwise_half() {
    use std::f64::consts::PI;
    let insertion = PidPoint { x: 0.0, y: 0.0 };
    let placement = Placement {
        insertion: &insertion,
        rotation: 1.571,
        scale: [1.0, 1.0],
        projection: Projection {
            mm_per_unit: 1000.0,
            band: SheetBand::for_page(None),
        },
    };
    let head = SymbolPrimitive::Arc {
        center: (0.1016, 0.22959),
        radius: 0.02691,
        start_angle: 3.141592653589794,
        end_angle: 0.0,
        clockwise: false,
    };
    let Some(EntityType::Arc(arc)) = shape_primitive(&head, None, &placement, &HashMap::new())
    else {
        panic!("the head did not build");
    };
    let sweep = (arc.end_angle - arc.start_angle).rem_euclid(2.0 * PI);
    let mid = arc.start_angle + sweep / 2.0;
    let mid_x = arc.center.x + arc.radius * mid.cos();
    assert!(
        mid_x < arc.center.x - 1e-6,
        "the rotated head must bulge left of its chord: mid_x {mid_x} vs chord x {}",
        arc.center.x
    );
}

#[test]
fn symbol_polyline_closure_reaches_the_cad_entity() {
    let insertion = PidPoint { x: 0.0, y: 0.0 };
    let placement = Placement {
        insertion: &insertion,
        rotation: 0.0,
        scale: [1.0, 1.0],
        projection: Projection {
            mm_per_unit: 1000.0,
            band: SheetBand::for_page(None),
        },
    };
    let primitive = SymbolPrimitive::Polyline {
        vertices: vec![(0.0, 0.0), (0.01, 0.0), (0.01, 0.01)],
        is_closed: true,
    };
    let Some(EntityType::LwPolyline(polyline)) =
        shape_primitive(&primitive, None, &placement, &HashMap::new())
    else {
        panic!("symbol polyline did not build");
    };
    assert!(polyline.is_closed);
}

/// A `.sym`'s own lettering: the record's anchor byte picks the hot point,
/// the style's height and typeface letter it, and the placement's rotation
/// and scale carry through. `0x15` is centre-on-middle, `0x01` left-on-top —
/// the two anchors the library's lettering is written at.
#[test]
fn symbol_text_letters_at_its_own_style_and_anchor() {
    let insertion = PidPoint { x: 0.0, y: 0.0 };
    let placement = Placement {
        insertion: &insertion,
        rotation: 0.0,
        scale: [1.0, 1.0],
        projection: Projection {
            mm_per_unit: 1000.0,
            band: SheetBand::for_page(None),
        },
    };
    let primitive = SymbolPrimitive::Text {
        text: "I".to_string(),
        at: (0.001, 0.004),
    };
    let lettering = SymbolLettering {
        anchor_code: 0x15,
        rotation_rad: 0.0,
        letter_height_m: 0.0,
        height_m: Some(0.003_175),
        height_mode: None,
        width_permille: None,
        rgb: Some([0, 0, 0]),
        alignment: Some(TextAlignment::Left),
        line_spacing: Some(1.0),
        run_style_id: Default::default(),
        spans: Default::default(),
        font_name: Some("Arial".to_string()),
    };
    let font_styles: HashMap<String, String> = [("Arial".to_string(), "PID-Arial".to_string())]
        .into_iter()
        .collect();
    let Some(EntityType::Text(label)) =
        shape_primitive(&primitive, Some(&lettering), &placement, &font_styles)
    else {
        panic!("symbol text did not build");
    };
    assert!(
        (label.height - 3.175).abs() < 1e-9,
        "the character style's 3.175 mm: {}",
        label.height
    );
    assert_eq!(label.horizontal_alignment, TextHorizontalAlignment::Center);
    assert_eq!(label.vertical_alignment, TextVerticalAlignment::Middle);
    assert_eq!(
        label.alignment_point,
        Some(label.insertion_point),
        "the anchor moved the origin to alignment_point"
    );
    assert_eq!(label.style, "PID-Arial");

    let left_top = SymbolLettering {
        anchor_code: 0x01,
        ..lettering.clone()
    };
    let Some(EntityType::Text(label)) =
        shape_primitive(&primitive, Some(&left_top), &placement, &font_styles)
    else {
        panic!("symbol text did not build");
    };
    assert_eq!(label.horizontal_alignment, TextHorizontalAlignment::Left);
    assert_eq!(label.vertical_alignment, TextVerticalAlignment::Top);

    // A record that scales its direction pair to the letter height states
    // the size itself, and it beats the style's: `letter_height_m` is what
    // the record letters at.
    let self_sized = SymbolLettering {
        letter_height_m: 0.002,
        height_m: Some(0.003_175),
        ..lettering.clone()
    };
    let Some(EntityType::Text(label)) =
        shape_primitive(&primitive, Some(&self_sized), &placement, &font_styles)
    else {
        panic!("symbol text did not build");
    };
    assert!(
        (label.height - 2.0).abs() < 1e-9,
        "the direction pair's own 2 mm: {}",
        label.height
    );

    // No lettering at all keeps the scaled ISO fallback the record used
    // to draw with.
    let Some(EntityType::Text(label)) =
        shape_primitive(&primitive, None, &placement, &HashMap::new())
    else {
        panic!("symbol text did not build");
    };
    assert!(
        (label.height - 2.5).abs() < 1e-9,
        "the 2.5 mm fallback: {}",
        label.height
    );
}

/// A mirrored placement still prints readable lettering: SmartPlant
/// normalizes the glyph rotation into `[-pi/2, pi/2)` and flips the anchor
/// the way the dropped mirror would have flipped it. The ESD interlock sits
/// at a pi-rotation mirror and its label reads upright in the reference; a
/// quarter-turn mirror keeps right-reading vertical text.
#[test]
fn mirrored_placement_lettering_stays_readable() {
    let insertion = PidPoint { x: 0.0, y: 0.0 };
    let lettering = SymbolLettering {
        anchor_code: 0x01,
        rotation_rad: 0.0,
        letter_height_m: 0.0,
        height_m: Some(0.003_175),
        height_mode: None,
        width_permille: None,
        rgb: Some([0, 0, 0]),
        alignment: Some(TextAlignment::Left),
        line_spacing: Some(1.0),
        run_style_id: Default::default(),
        spans: Default::default(),
        font_name: None,
    };
    let primitive = SymbolPrimitive::Text {
        text: "ESD".to_string(),
        at: (0.0, 0.0),
    };

    // A pi-rotation mirror (scale y negated): the label emits upright, and
    // its left anchor flips right — the mirror the rotation absorbed.
    let pi_mirror = Placement {
        insertion: &insertion,
        rotation: std::f64::consts::PI,
        scale: [1.0, -1.0],
        projection: Projection {
            mm_per_unit: 1000.0,
            band: SheetBand::for_page(None),
        },
    };
    let Some(EntityType::Text(label)) =
        shape_primitive(&primitive, Some(&lettering), &pi_mirror, &HashMap::new())
    else {
        panic!("mirrored symbol text did not build");
    };
    assert!(
        label.rotation.abs() < 1e-9,
        "the pi mirror emits upright lettering: {}",
        label.rotation
    );
    assert_eq!(label.horizontal_alignment, TextHorizontalAlignment::Right);
    assert_eq!(label.vertical_alignment, TextVerticalAlignment::Top);

    // A quarter-turn mirror keeps the right-reading -pi/2 direction and
    // flips the vertical anchor instead.
    let quarter_mirror = Placement {
        insertion: &insertion,
        rotation: -std::f64::consts::FRAC_PI_2,
        scale: [1.0, -1.0],
        projection: Projection {
            mm_per_unit: 1000.0,
            band: SheetBand::for_page(None),
        },
    };
    let Some(EntityType::Text(label)) = shape_primitive(
        &primitive,
        Some(&lettering),
        &quarter_mirror,
        &HashMap::new(),
    ) else {
        panic!("mirrored symbol text did not build");
    };
    assert!(
        (label.rotation + std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "the quarter-turn mirror keeps -pi/2: {}",
        label.rotation
    );
    assert_eq!(label.vertical_alignment, TextVerticalAlignment::Bottom);

    // The same rotation without the mirror is left alone: a pi-rotated
    // placement letters upside down the way the file placed it.
    let unmirrored = Placement {
        insertion: &insertion,
        rotation: std::f64::consts::PI,
        scale: [1.0, 1.0],
        projection: Projection {
            mm_per_unit: 1000.0,
            band: SheetBand::for_page(None),
        },
    };
    let Some(EntityType::Text(label)) =
        shape_primitive(&primitive, Some(&lettering), &unmirrored, &HashMap::new())
    else {
        panic!("symbol text did not build");
    };
    assert!(
        (label.rotation - std::f64::consts::PI).abs() < 1e-9,
        "unmirrored lettering keeps the placement rotation: {}",
        label.rotation
    );
    assert_eq!(label.horizontal_alignment, TextHorizontalAlignment::Left);
    assert_eq!(label.vertical_alignment, TextVerticalAlignment::Top);
}

/// A plug valve's boundary holds concentric circle members, and SmartPlant
/// prints every ring as its own solid disc — so the boundary builds one
/// hatch per circle rather than one compound path the renderer would read
/// as a ring with holes.
#[test]
fn concentric_boundary_circles_build_a_solid_hatch_each() {
    let insertion = PidPoint { x: 0.0, y: 0.0 };
    let placement = Placement {
        insertion: &insertion,
        rotation: 0.0,
        scale: [1.0, 1.0],
        projection: Projection {
            mm_per_unit: 1000.0,
            band: SheetBand::for_page(None),
        },
    };
    let circle = |radius| SymbolPrimitive::Circle {
        center: (0.0, 0.0),
        radius,
    };
    let primitive = SymbolPrimitive::Boundary {
        members: vec![circle(0.00034), circle(0.00065), circle(0.00095)],
        fill: Some(pid_parse::symbol_library::PrimitiveFill {
            rgb: Some([0, 0, 255]),
            type_code: 0x002A,
        }),
    };
    let entities = shape_primitives(&primitive, None, &placement, &HashMap::new());
    assert_eq!(entities.len(), 3, "one hatch per circle member");
    let mut radii: Vec<f64> = entities
        .iter()
        .map(|entity| {
            let EntityType::Hatch(hatch) = entity else {
                panic!("a circle member builds a hatch");
            };
            assert_eq!(hatch.paths.len(), 1, "the disc is a single path");
            let Some(BoundaryEdge::CircularArc(arc)) = hatch.paths[0].edges.first() else {
                panic!("the circle member's path is a circular arc");
            };
            arc.radius
        })
        .collect();
    radii.sort_by(f64::total_cmp);
    let expected = [0.34, 0.65, 0.95];
    for (got, want) in radii.iter().zip(expected) {
        assert!((got - want).abs() < 1e-6, "radius {want}: {got}");
    }
}

/// `paper_style_scale` multiplies style-declared dash lengths, line weights,
/// and point-marker stroke offsets, and leaves lettering at the height the
/// file states. Scale 1 is the unscaled reading; 5/3 is the screen multiple
/// measured on the reference sheet.
#[test]
fn paper_style_scale_multiplies_declared_sizes_and_leaves_lettering() {
    use pid_parse::style_link::{LineSymbology, StyleHop};

    let scale = 5.0 / 3.0;
    assert_eq!(effective_paper_style_scale(scale), scale);
    assert_eq!(effective_paper_style_scale(1.0), 1.0);
    assert_eq!(effective_paper_style_scale(0.0), 1.0);
    assert_eq!(effective_paper_style_scale(-1.0), 1.0);
    assert_eq!(effective_paper_style_scale(f64::NAN), 1.0);
    assert_eq!(effective_paper_style_scale(f64::INFINITY), 1.0);

    let style = |width_mm: f64| ResolvedLineStyle {
        style_id: 1,
        symbology: LineSymbology {
            width_m: width_mm / 1000.0,
            colour: 0,
            paper: false,
        },
        dash: None,
        marker: None,
        complex_dash: None,
        hop: StyleHop::Direct,
    };
    let weight = |width_mm: f64, k: f64| {
        let mut line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
        line.common.layer = LAYER_GEOMETRY.to_string();
        let mut entity = EntityType::Line(line);
        apply_symbology(&mut entity, &style(width_mm), &HashMap::new(), k);
        entity.common().line_weight
    };
    // Declared widths land on the plotter's three pens — 0.169/0.339/0.678mm
    // (17/34/68 hundredths), thin to 0.30mm and medium to 0.55mm — so 0.35mm
    // is the medium pen at 1:1 and heavy once the scale carries it past
    // 0.55mm.
    for (mm, at_one, at_scale) in [
        (0.35, 34_i16, 68),
        (0.13, 17, 17),
        (0.18, 17, 17),
        (0.70, 68, 68),
    ] {
        assert_eq!(
            weight(mm, 1.0),
            LineWeight::Value(at_one),
            "{mm} mm at scale 1"
        );
        assert_eq!(
            weight(mm, scale),
            LineWeight::Value(at_scale),
            "{mm} mm at 5/3"
        );
    }

    let magnitudes = |segments: &[f64], k: f64| {
        build_dash_linetype("PID-DASH", segments, k)
            .elements
            .iter()
            .map(|element| (element.length.abs() * 1000.0).round() as i64)
            .collect::<Vec<_>>()
    };
    assert_eq!(magnitudes(&[3.5, 1.75], 1.0), vec![3500, 1750]);
    assert_eq!(magnitudes(&[3.5, 1.75], scale), vec![5833, 2917]);
    assert_eq!(magnitudes(&[1.905, 1.27], 1.0), vec![1905, 1270]);
    assert_eq!(magnitudes(&[1.905, 1.27], scale), vec![3175, 2117]);

    // The pool is keyed by the unscaled pattern, so the name does not move
    // when the stored segment lengths do.
    let dashed = |id: u32, segments_m: &[f64]| ResolvedLineStyle {
        style_id: id,
        symbology: LineSymbology {
            width_m: 0.000_35,
            colour: 0,
            paper: false,
        },
        dash: DashPattern::from_segments_m(segments_m),
        marker: None,
        complex_dash: None,
        hop: StyleHop::Direct,
    };
    let mut styles = LineStyleIndex::new();
    styles.insert(("/".to_string(), 1), dashed(1, &[0.0035, 0.00175]));
    styles.insert(("/".to_string(), 2), dashed(2, &[0.001_905, 0.001_27]));
    let mut doc = Document::new();
    let pool = register_dash_linetypes(&mut doc, &styles, &[], scale);
    assert_eq!(
        pool.get(&vec![3500, 1750]).map(String::as_str),
        Some("PID-DASH-1"),
        "the 3.5/1.75 key is the unscaled pattern"
    );
    assert_eq!(
        pool.get(&vec![1905, 1270]).map(String::as_str),
        Some("PID-DASH-2"),
        "the 1.905/1.27 key is the unscaled pattern"
    );
    let stored = |name: &str| {
        doc.line_types
            .get(name)
            .unwrap_or_else(|| panic!("{name} missing"))
            .elements
            .iter()
            .map(|element| (element.length.abs() * 1000.0).round() as i64)
            .collect::<Vec<_>>()
    };
    assert_eq!(stored("PID-DASH-1"), vec![5833, 2917]);
    assert_eq!(stored("PID-DASH-2"), vec![3175, 2117]);

    let mut stroke = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
    stroke.common.layer = LAYER_SYMBOL.to_string();
    let mut entity = EntityType::Line(stroke);
    paint_symbol_stroke(
        &mut entity,
        Some(&PrimitiveStyle {
            rgb: [0, 0, 0],
            width_mm: 0.35,
            dash_mm: vec![3.5, 1.75],
            paper: false,
        }),
        &pool,
        scale,
    );
    assert_eq!(entity.common().line_weight, LineWeight::Value(68));
    assert_eq!(entity.common().linetype, "PID-DASH-1");

    let mut text = Text::new();
    text.value = "LIA".to_string();
    text.height = 2.5;
    text.common.layer = LAYER_SYMBOL.to_string();
    let mut lettering = EntityType::Text(text);
    apply_symbology(&mut lettering, &style(0.18), &HashMap::new(), scale);
    let EntityType::Text(text) = &lettering else {
        panic!("lettering is still text");
    };
    assert_eq!(text.height, 2.5, "text height is not multiplied");
    assert_eq!(
        lettering.common().line_weight,
        Text::new().common.line_weight,
        "lettering takes no line weight"
    );

    let projection = Projection {
        mm_per_unit: 1000.0,
        band: SheetBand::for_page(None),
    };
    let origin = Vector3::new(10.0, 20.0, 0.0);
    let length = |k: f64, end: (f64, f64)| {
        let at = point_marker_offset(origin, end, projection, k);
        (at.x - origin.x).hypot(at.y - origin.y)
    };
    // (0, 0) -> (3, 6) mm, the long stroke of the warning slash.
    let long = length(1.0, (0.003, 0.006));
    assert!((long - 6.708_203_932_499_369).abs() < 1e-9, "{long}");
    assert!((length(scale, (0.003, 0.006)) - long * scale).abs() < 1e-9);
    // (-1, -2) -> (-0.7, -1) mm, the stub.
    let stub_at = |k: f64, xy: (f64, f64)| point_marker_offset(origin, xy, projection, k);
    let stub = stub_at(1.0, (-0.0007, -0.001));
    let stub_from = stub_at(1.0, (-0.001, -0.002));
    let stub_len = (stub.x - stub_from.x).hypot(stub.y - stub_from.y);
    let stub_k = stub_at(scale, (-0.0007, -0.001));
    let stub_from_k = stub_at(scale, (-0.001, -0.002));
    let stub_len_k = (stub_k.x - stub_from_k.x).hypot(stub_k.y - stub_from_k.y);
    assert!((stub_len_k - stub_len * scale).abs() < 1e-9);
}

/// Authored paper ink becomes the contrast colour: `RGB(0,0,0)` stored as
/// ACI 7 reads white on the dark canvas (`adapt_to_bg`) and black on the
/// plotted sheet, where the true colour it used to be stayed invisible in
/// the former. `RGB(255,255,255)` is the same foreground pen -- SmartPlant
/// prints the `.igr`'s white-stated frame drafting black. Every other
/// stated colour is the true colour it is.
#[test]
fn authored_black_is_the_paper_foreground_colour() {
    assert_eq!(foreground_color(0, 0, 0), Color::Index(7));
    assert_eq!(
        foreground_color(0xFF, 0x00, 0x00),
        Color::from_rgb(0xFF, 0x00, 0x00)
    );
    assert_eq!(
        foreground_color(0xFF, 0xFF, 0xFF),
        Color::Index(7),
        "stated white is the same foreground pen: the linked .igr letters its \
         frame drafting white and SmartPlant's own PDFs print it black"
    );

    let mut text = Text::new();
    text.value = "TITLE".to_string();
    text.common.layer = LAYER_TEXT.to_string();
    let mut lettering = EntityType::Text(text);
    apply_text_colour(&mut lettering, [0, 0, 0]);
    assert_eq!(
        lettering.common().color,
        Color::Index(7),
        "the sheet's black lettering is the contrast colour"
    );
    let mut text = Text::new();
    text.value = "LGM".to_string();
    text.common.layer = LAYER_SYMBOL.to_string();
    text.common.color = Color::from_rgb(0xFF, 0x00, 0x00);
    let mut symbol_lettering = EntityType::Text(text);
    apply_text_colour(&mut symbol_lettering, [0, 0, 0]);
    assert_eq!(
        symbol_lettering.common().color,
        Color::from_rgb(0xFF, 0x00, 0x00),
        "a symbol's internal lettering keeps what the .sym authored"
    );
}

/// An OLE link names its target several ways -- the absolute path the
/// linking machine saw, a relative tail, a bare name; all of them answer
/// the file name the local copy is found by.
#[test]
fn a_template_link_is_named_by_its_basename() {
    assert_eq!(
        template_basename(r"C:\PID_SQProject\SQPlant\Ref\Template Files\CPE-SQ-A1.igr"),
        "CPE-SQ-A1.igr"
    );
    assert_eq!(
        template_basename(r"\\WIN-SPID\share\Ref\Template Files\CPECCHBA2.igr"),
        "CPECCHBA2.igr"
    );
    assert_eq!(
        template_basename("Ref\\Template Files\\XA-A1.igr"),
        "XA-A1.igr"
    );
    assert_eq!(template_basename("CPE-SQ-A0.igr"), "CPE-SQ-A0.igr");
    assert_eq!(template_basename("  XA-A1.igr  "), "XA-A1.igr");
}

/// `.igr` links come off every `JSite`'s OLE stream: whatever the site's
/// symbol is for, its `\x01Ole` strings are scanned for template targets.
#[test]
fn linked_templates_are_the_igr_links_of_every_jsite() {
    let mut parsed = pid_parse::PidDocument::default();
    let site = |ole_links: Vec<String>| pid_parse::JSite {
        name: "JSite1".to_string(),
        ole_links,
        ..Default::default()
    };
    assert!(linked_templates(&parsed).is_empty());
    parsed.jsites.push(site(vec![
        r"C:\Plant\Ref\Template Files\A1.igr".to_string(),
        "A1.igr".to_string(),
        "Sheet1".to_string(),
    ]));
    parsed.jsites.push(site(vec![
        r"\\share\Ref\Template Files\A0.IGR".to_string(),
        "not-a-template.xls".to_string(),
    ]));
    assert_eq!(
        linked_templates(&parsed),
        vec![
            "A1.igr".to_string(),
            r"C:\Plant\Ref\Template Files\A1.igr".to_string(),
            r"\\share\Ref\Template Files\A0.IGR".to_string()
        ],
        "deduplicated, sorted, only .igr"
    );
}

/// The stated path wins when it exists on this machine; the basename search
/// under the roots is the fallback for a link authored elsewhere.
#[test]
fn a_link_resolves_directly_or_by_basename_under_a_root() {
    let temp = std::env::temp_dir().join(format!(
        "pid-template-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let nested = temp.join("Ref").join("Template Files");
    std::fs::create_dir_all(&nested).expect("temp dirs");
    let local = nested.join("Local-A1.igr");
    std::fs::write(&local, b"stub").expect("write");

    let roots = vec![temp.clone()];
    assert_eq!(
        resolve_template_path(&local.to_string_lossy(), &roots).as_deref(),
        Some(local.as_path()),
        "a path that exists resolves to itself"
    );
    assert_eq!(
        resolve_template_path(r"C:\elsewhere\Ref\Template Files\Local-A1.igr", &roots).as_deref(),
        Some(local.as_path()),
        "a foreign absolute path resolves by basename"
    );
    assert_eq!(
        resolve_template_path(r"C:\elsewhere\Ref\Template Files\Missing.igr", &roots),
        None
    );
    assert!(
        resolve_template_path("", &roots).is_none(),
        "an empty link has no basename"
    );
    let _ = std::fs::remove_dir_all(&temp);
}

/// The linked `.igr` parses through the same pipeline as the drawing and
/// overlays onto `PID-FRAME`: the fixture template draws its closed page
/// frame (so the synthetic border stands down), its title-block and
/// margin-strip lines, and reports the records it cannot draw beside the
/// drawing's own `missing` rather than in it.
#[test]
fn a_linked_igr_overlays_its_drafting_on_pid_frame() {
    let igr = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|dir| {
            dir.join("pid-parse")
                .join("test-file")
                .join("Ref")
                .join("Template Files")
                .join("CPE-SQ-A1.igr")
        })
        .filter(|path| path.is_file());
    let Some(igr) = igr else {
        return; // fixture absent: soft-skip like the corpus tests do
    };
    let mut doc = Document::new();
    // The drawing's own normalized geometry is only consulted for its page:
    // a frame of the template that covers it stands the synthetic border
    // down. Which template a sheet wears is `pick_linked_template`'s call,
    // made on the strokes measured here.
    let drawing = NormalizedPidGeometry {
        page_dimensions_mm: Some((841.0, 594.0)),
        ..Default::default()
    };
    let template = read_template(&igr, false).expect("the fixture parses");
    let (width, height) = template
        .candidate()
        .stroked_mm
        .expect("the fixture has strokes to measure");
    assert!(
        (width - 841.0).abs() < 0.01 && (height - 594.0).abs() < 0.01,
        "the A1 template's strokes span the A1 sheet, not {width} x {height}"
    );
    let overlay = overlay_template(&template, &drawing, &mut doc, 1.0).expect("the fixture draws");
    assert!(overlay.drawn > 50, "a title block is more than a few lines");
    assert!(
        overlay.page_frame_covered,
        "the template draws the page frame the synthetic border doubles"
    );
    let frames: Vec<_> = doc
        .entities()
        .filter(|entity| entity.common().layer == LAYER_FRAME)
        .collect();
    assert_eq!(
        frames.len(),
        overlay.drawn,
        "every template entity files on PID-FRAME"
    );
    // The page rectangle the synthetic border would have drawn is among the
    // template's own drafting -- however the `.igr` files it: a closed
    // polyline or the four lines of the frame.
    let spans_page = frames
        .iter()
        .filter_map(|entity| page::drawn_extent(entity))
        .any(|(min_x, min_y, max_x, max_y)| {
            (max_x - min_x - 841.0).abs() < 2.0 && (max_y - min_y - 594.0).abs() < 2.0
        });
    assert!(spans_page, "the frame covers the A1 page");
    // Provenance: every overlay entity says which template drew it.
    let tagged = frames
        .iter()
        .filter(|entity| {
            entity
                .common()
                .extended_data
                .records()
                .iter()
                .flat_map(|record| record.values.iter())
                .any(|value| {
                    matches!(value, document::xdata::XDataValue::String(s) if s == "template=CPE-SQ-A1.igr")
                })
        })
        .count();
    assert_eq!(
        tagged, overlay.drawn,
        "every template entity names the .igr it came from"
    );
}

/// The importer finds a drawing's symbols by walking up from the drawing to
/// a `Ref` folder. A backup as SmartPlant leaves it has none: its symbol
/// library is a numbered volume. Restored, the volume is back under `Ref`
/// beside the drawings, and the walk finds it.
#[test]
fn a_restored_backup_has_the_symbol_library_its_drawings_look_for() {
    use std::io::Write;
    // The name says nothing of symbols, and the restores lie two folders
    // down: the walk goes five levels up from a drawing and takes every
    // folder with "symbol" in its name for a library, the temp folder's too.
    let root = std::env::temp_dir().join(format!("ocs-pid-restored-refdata-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let deep = root.join("a").join("b");
    let backup = deep.join("backup");
    std::fs::create_dir_all(&backup).unwrap();
    let archive = |name: &str, members: &[(&str, &[u8])]| {
        let mut writer = zip::ZipWriter::new(std::fs::File::create(backup.join(name)).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (member, bytes) in members {
            writer.start_file(*member, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    };
    std::fs::write(
        backup.join("Manifest.txt"),
        "Name<<|>>SQPlant\r\n\
         File<<|>>2<<|>>1<<|>>711<<|>>1<<|>>\\\\host\\P\\Plant<<|>>1<<|>>PlantData~2~711.zip\r\n\
         File<<|>>4<<|>>1<<|>>681<<|>>1<<|>>\\\\host\\P\\Ref\\Symbols<<|>>1<<|>>RefData~4~681.zip\r\n",
    )
    .unwrap();
    archive("PlantData~2~711.zip", &[("00/00/A.pid", b"a")]);
    archive("RefData~4~681.zip", &[("Piping/Valves/gate.sym", b"gate")]);

    // The library roots the walk finds under `folder` (the environment may
    // name others, which are not this test's business).
    let found_under = |drawing: &Path, folder: &Path| -> Vec<PathBuf> {
        super::symbols::discover_symbol_library(drawing)
            .map(|library| {
                library
                    .roots()
                    .iter()
                    .filter(|root| root.starts_with(folder))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    let target = deep.join("out");
    let restored = crate::io::pid_backup::restore(&backup, Some(&target)).unwrap();
    let drawing = restored.drawings.first().expect("the drawing is restored");
    assert_eq!(found_under(drawing, &root), [target.join("Ref").join("Symbols")]);
    assert!(target.join("Ref/Symbols/Piping/Valves/gate.sym").is_file());

    // The same backup with its volume taken away: nothing to find.
    std::fs::remove_file(backup.join("RefData~4~681.zip")).unwrap();
    let bare = deep.join("bare");
    let restored = crate::io::pid_backup::restore(&backup, Some(&bare)).unwrap();
    let drawing = restored.drawings.first().expect("the drawing is restored");
    assert!(found_under(drawing, &root).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// The view the importer states for a drawing that draws over the rectangle
/// `drawn` spans, on `page` where it names one: its centre, its height, and
/// the width it states it takes in (`view_height * aspect_ratio`).
fn opening_view(page: Option<(f64, f64)>, drawn: [(f64, f64); 2]) -> ((f64, f64), f64, f64) {
    let mut bounds = Bounds::new(Projection {
        mm_per_unit: 1.0,
        band: SheetBand::for_page(page),
    });
    for (x, y) in drawn {
        bounds.add_mm(x, y);
    }
    let mut doc = Document::new();
    frame_drawing(&mut doc, &bounds, page);
    let view = doc
        .vports
        .iter()
        .find(|vport| vport.name == "*Active")
        .expect("the importer states the opening view");
    (
        (view.view_target.x, view.view_target.y),
        view.view_height,
        view.view_height * view.aspect_ratio,
    )
}

fn assert_same_view(found: ((f64, f64), f64, f64), wanted: ((f64, f64), f64, f64)) {
    let ((x, y), height, width) = found;
    let ((wanted_x, wanted_y), wanted_height, wanted_width) = wanted;
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * b.abs().max(1.0);
    assert!(
        close(x, wanted_x)
            && close(y, wanted_y)
            && close(height, wanted_height)
            && close(width, wanted_width),
        "found {found:?}, wanted {wanted:?}"
    );
}

/// A landscape sheet -- `D07`'s A2, 594.328 x 420.566 -- opens centred on
/// the sheet and as high as three quarters of its width with a twentieth
/// over, 468.03 as measured, and the view states the width it has to take
/// in: the sheet's with a twentieth over. Restored at its height alone, it
/// showed 81.8% of the sheet's width on the canvas the backup panel leaves
/// (`readings.md` §8).
#[test]
fn a_landscape_sheet_opens_on_a_view_that_states_its_width() {
    let (width, height) = (594.328, 420.566);
    assert_same_view(
        opening_view(Some((width, height)), [(30.0, 40.0), (560.0, 400.0)]),
        (
            (width / 2.0, height / 2.0),
            width * 0.75 * 1.05,
            width * 1.05,
        ),
    );
}

/// A portrait sheet opens as high as the sheet with a twentieth over, and
/// states the sheet's width with a twentieth over: a canvas no narrower
/// than the sheet takes that in.
#[test]
fn a_portrait_sheet_opens_on_a_view_that_states_its_width() {
    let (width, height) = (420.0, 594.0);
    assert_same_view(
        opening_view(Some((width, height)), [(20.0, 30.0), (400.0, 560.0)]),
        ((width / 2.0, height / 2.0), height * 1.05, width * 1.05),
    );
}

/// A drawing that names no sheet opens on what it draws, and states the
/// width of what it draws with a twentieth over. What it draws here is
/// narrower than 4:3, so the height is its own and not three quarters of
/// its width.
#[test]
fn a_drawing_without_a_sheet_opens_on_a_view_that_states_the_width_it_draws_over() {
    assert_same_view(
        opening_view(None, [(10.0, 20.0), (310.0, 270.0)]),
        ((160.0, 145.0), 250.0 * 1.05, 300.0 * 1.05),
    );
}

/// A character style states its character width in thousandths of the
/// face's own (`width_permille`), and only a narrowing or a widening becomes
/// a width factor: `0` -- the face's own width, what nearly every style of a
/// drawing's own lettering states -- reads like `1000` and like no statement
/// at all, the way `pid-plot` reads it. Like the height, the width goes on
/// the sheet's own lettering only.
#[test]
fn pid_text_width_only_a_stated_width_becomes_a_width_factor() {
    assert_eq!(text_width_factor(None), None);
    assert_eq!(text_width_factor(Some(0)), None);
    assert_eq!(text_width_factor(Some(1000)), None);
    assert_eq!(text_width_factor(Some(800)), Some(0.8));
    assert_eq!(text_width_factor(Some(700)), Some(0.7));

    let lettered_on = |layer: &str| {
        let mut text = Text::new();
        text.common.layer = layer.to_string();
        let mut entity = EntityType::Text(text);
        apply_text_width(&mut entity, 0.8);
        match entity {
            EntityType::Text(text) => text.width_factor,
            _ => unreachable!("built as text"),
        }
    };
    assert_eq!(lettered_on(LAYER_TEXT), 0.8);
    assert_eq!(
        lettered_on(LAYER_SYMBOL),
        Text::new().width_factor,
        "a symbol's lettering takes its width from its own record"
    );
}

/// A symbol's own lettering letters at the width its record's style states:
/// the `M` and `S` of the motor and solenoid operators in the vendor's
/// drawings state `800`.
#[test]
fn pid_text_width_symbol_lettering_letters_at_its_styles_width() {
    let insertion = PidPoint { x: 0.0, y: 0.0 };
    let placement = Placement {
        insertion: &insertion,
        rotation: 0.0,
        scale: [1.0, 1.0],
        projection: Projection {
            mm_per_unit: 1000.0,
            band: SheetBand::for_page(None),
        },
    };
    let primitive = SymbolPrimitive::Text {
        text: "M".to_string(),
        at: (0.0, 0.0),
    };
    let width_of = |width_permille: Option<u16>| {
        let lettering = SymbolLettering {
            anchor_code: 0x15,
            rotation_rad: 0.0,
            letter_height_m: 0.0,
            height_m: Some(0.002_54),
            height_mode: None,
            width_permille,
            rgb: Some([0, 0, 0]),
            alignment: Some(TextAlignment::Center),
            line_spacing: Some(1.0),
            run_style_id: Default::default(),
            spans: Default::default(),
            font_name: Some("Arial".to_string()),
        };
        let Some(EntityType::Text(label)) =
            shape_primitive(&primitive, Some(&lettering), &placement, &HashMap::new())
        else {
            panic!("symbol text did not build");
        };
        label.width_factor
    };
    assert_eq!(width_of(Some(800)), 0.8);
    for width_permille in [None, Some(0), Some(1000)] {
        assert_eq!(
            width_of(width_permille),
            Text::new().width_factor,
            "{width_permille:?} keeps the face's own width"
        );
    }
}

/// The `CPE-SQ` title block letters at the width its styles state: every
/// label of the template states `800`, and the vendor's plots narrow them to
/// match. At the face's own width the company name ran over the title
/// block's right-hand line.
#[test]
fn pid_text_width_template_lettering_letters_at_its_styles_width() {
    let igr = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|dir| {
            dir.join("pid-parse")
                .join("test-file")
                .join("Ref")
                .join("Template Files")
                .join("CPE-SQ-A1.igr")
        })
        .filter(|path| path.is_file());
    let Some(igr) = igr else {
        return; // fixture absent: soft-skip like the corpus tests do
    };
    let mut doc = Document::new();
    let drawing = NormalizedPidGeometry {
        page_dimensions_mm: Some((841.0, 594.0)),
        ..Default::default()
    };
    let template = read_template(&igr, false).expect("the fixture parses");
    overlay_template(&template, &drawing, &mut doc, 1.0).expect("the fixture draws");
    let widths: Vec<(String, f64)> = doc
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Text(text) => Some((text.value.clone(), text.width_factor)),
            _ => None,
        })
        .collect();
    assert!(widths.len() >= 30, "a title block's labels: {widths:?}");
    let off: Vec<_> = widths
        .iter()
        .filter(|(_, width)| (width - 0.8).abs() > 1e-12)
        .collect();
    assert!(off.is_empty(), "labels not at the stated 0.8: {off:?}");
}

/// A drawing's own lettering letters at the width its style states, and only
/// where its style states one: in the export fixture `A01`, `项目经理` states
/// `800` and `文件号` beside it in the title block states `0`.
#[test]
fn pid_text_width_sheet_lettering_letters_at_its_styles_width() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|dir| {
            dir.join("pid-parse")
                .join("test-file")
                .join("export-test")
                .join("publish-data")
                .join("A01")
                .join("A01.pid")
        })
        .filter(|path| path.is_file());
    let Some(path) = path else {
        return; // fixture absent: soft-skip like the corpus tests do
    };
    let import = load_pid(&path).expect("the fixture imports");
    // The drawing's own lettering, not a linked template's that may letter
    // the same words.
    let widths_of = |value: &str| -> Vec<f64> {
        import
            .document
            .entities()
            .filter(|entity| {
                !entity
                    .common()
                    .extended_data
                    .records()
                    .iter()
                    .flat_map(|record| record.values.iter())
                    .any(|datum| {
                        matches!(datum, document::xdata::XDataValue::String(s) if s.starts_with("template="))
                    })
            })
            .filter_map(|entity| match entity {
                EntityType::Text(text) if text.value == value => Some(text.width_factor),
                _ => None,
            })
            .collect()
    };
    assert_eq!(widths_of("项目经理"), vec![0.8]);
    assert_eq!(widths_of("文件号"), vec![Text::new().width_factor]);
}

/// Narrowing a label keeps the hot point its record states. The canvas steps
/// back from the anchor by the run's advance measured at the width factor,
/// so a right-aligned label keeps its right end and a centred one its middle.
#[test]
fn pid_text_width_keeps_a_right_or_centred_labels_anchor() {
    use crate::entities::text::text_run_placement_at_scale;
    use crate::entities::text_support::text_local_bounds;

    let doc = Document::new();
    // `0x20` is right-on-bottom and `0x10` centre-on-bottom; beside each, the
    // share of the run's advance that lies left of the anchor.
    for (code, left_of_anchor) in [(0x20_u8, 1.0), (0x10, 0.5)] {
        let laid_out = |factor: Option<f64>| {
            let mut text = Text::new();
            text.value = "华北油田苏桥储气库群".to_string();
            text.height = 4.0;
            text.insertion_point.x = 700.0;
            text.insertion_point.y = 30.0;
            text.common.layer = LAYER_TEXT.to_string();
            let mut entity = EntityType::Text(text);
            apply_text_anchor(&mut entity, code, None);
            if let Some(factor) = factor {
                apply_text_width(&mut entity, factor);
            }
            let EntityType::Text(text) = entity else {
                unreachable!("built as text");
            };
            let placement = text_run_placement_at_scale(&text, &doc, 1.0);
            let advance = text_local_bounds(
                &placement.font,
                &text.value,
                placement.height,
                placement.width_factor,
                placement.oblique_angle,
            )
            .expect("the label lays out")
            .advance;
            let advance = f64::from(advance);
            (placement.origin[0] + left_of_anchor * advance, advance)
        };
        let (anchor, advance) = laid_out(None);
        let (narrow_anchor, narrow_advance) = laid_out(text_width_factor(Some(800)));
        assert!((anchor - 700.0).abs() < 1e-3, "{code:#04x}: {anchor}");
        assert!(
            (narrow_anchor - 700.0).abs() < 1e-3,
            "{code:#04x} narrowed: {narrow_anchor}"
        );
        assert!(
            (narrow_advance / advance - 0.8).abs() < 1e-4,
            "{code:#04x}: {narrow_advance} against {advance}"
        );
    }
}

/// A sheet label placed the way the importer places it -- the anchor code,
/// then the box when the record states one -- and laid out the way the canvas
/// lays it out: the entity, the run's origin and its advance along the
/// baseline, in drawing millimetres.
fn boxed_label(
    value: &str,
    insertion: (f64, f64),
    rotation: f64,
    code: u8,
    box_width_mm: Option<f64>,
    paragraph: Option<TextAlignment>,
) -> (Text, [f64; 2], f64) {
    use crate::entities::text::text_run_placement_at_scale;
    use crate::entities::text_support::text_local_bounds;

    let mut text = Text::new();
    text.value = value.to_string();
    text.height = 2.469;
    text.insertion_point.x = insertion.0;
    text.insertion_point.y = insertion.1;
    text.rotation = rotation;
    text.common.layer = LAYER_TEXT.to_string();
    let mut entity = EntityType::Text(text);
    apply_text_anchor(&mut entity, code, None);
    apply_text_box(&mut entity, code, box_width_mm, paragraph);
    let EntityType::Text(text) = entity else {
        unreachable!("built as text");
    };
    let placement = text_run_placement_at_scale(&text, &Document::new(), 1.0);
    let advance = text_local_bounds(
        &placement.font,
        &text.value,
        placement.height,
        placement.width_factor,
        placement.oblique_angle,
    )
    .expect("the label lays out")
    .advance;
    let origin = [placement.origin[0], placement.origin[1]];
    (text, origin, f64::from(advance))
}

/// A boxed record whose paragraph is left-aligned starts at its box's left
/// edge. The connectors' drawing numbers are the case: a 45.307 mm box with
/// the insertion at its middle (`0x48`), around a number some 28 mm long.
#[test]
fn pid_text_box_starts_a_left_aligned_run_at_the_box_left_edge() {
    let label = |width| {
        boxed_label(
            "DWG-0100集05-37",
            (106.956, 505.482),
            0.0,
            0x48,
            width,
            Some(TextAlignment::Left),
        )
    };
    let (text, origin, advance) = label(Some(45.307));
    let (_, unboxed, _) = label(None);
    assert!(advance < 40.0, "a box wider than its text: {advance}");
    assert_eq!(text.horizontal_alignment, TextHorizontalAlignment::Left);
    assert!(
        (origin[0] - (106.956 - 45.307 / 2.0)).abs() < 1e-6,
        "starts at the box's left edge: {origin:?}"
    );
    assert!(
        (origin[1] - unboxed[1]).abs() < 1e-9,
        "keeps the line the anchor code puts it on: {origin:?} against {unboxed:?}"
    );
}

/// A right-aligned paragraph hangs the run from the box's right edge: the
/// three such numbers of the reference drawings end 0.05 mm inside it.
#[test]
fn pid_text_box_ends_a_right_aligned_run_at_the_box_right_edge() {
    let (text, origin, advance) = boxed_label(
        "DWG-0100集05-42",
        (689.867, 300.0),
        0.0,
        0x48,
        Some(45.307),
        Some(TextAlignment::Right),
    );
    assert!(advance < 40.0, "a box wider than its text: {advance}");
    assert_eq!(text.horizontal_alignment, TextHorizontalAlignment::Right);
    assert!(
        (origin[0] + advance - (689.867 + 45.307 / 2.0)).abs() < 1e-3,
        "ends at the box's right edge: {origin:?} + {advance}"
    );
}

/// A centred paragraph letters the run at the box's middle, and the anchor
/// code still says where the box is: a left anchor (`0x05`) puts the
/// insertion on the box's left edge, so the run centres half a box further
/// along.
#[test]
fn pid_text_box_centres_a_centre_aligned_run_in_the_box() {
    let (text, origin, advance) = boxed_label(
        "323",
        (63.922, 335.647),
        0.0,
        0x05,
        Some(20.0),
        Some(TextAlignment::Center),
    );
    assert!(advance < 20.0, "a box wider than its text: {advance}");
    assert_eq!(text.horizontal_alignment, TextHorizontalAlignment::Center);
    assert!(
        (origin[0] + advance / 2.0 - (63.922 + 10.0)).abs() < 1e-3,
        "centred in the box: {origin:?} + {advance} / 2"
    );
}

/// The box lies along the baseline: a record lettered up the page moves up
/// it, and stays where it was across it.
#[test]
fn pid_text_box_moves_a_rotated_run_along_its_baseline() {
    let label = |width| {
        boxed_label(
            "DWG-0100集05-37",
            (74.005, 283.540),
            std::f64::consts::FRAC_PI_2,
            0x48,
            width,
            Some(TextAlignment::Left),
        )
    };
    let (_, origin, _) = label(Some(42.009));
    let (_, unboxed, _) = label(None);
    assert!(
        (origin[1] - (283.540 - 42.009 / 2.0)).abs() < 1e-6,
        "starts at the box's lower edge: {origin:?}"
    );
    // The canvas turns a run in `f32`: the centred one steps back half its
    // advance along a cosine of about -4e-8, some 7e-7 mm across.
    assert!(
        (origin[0] - unboxed[0]).abs() < 1e-3,
        "stays across the baseline: {origin:?} against {unboxed:?}"
    );
}

/// A record that states no box letters at its insertion by its anchor code,
/// whatever its paragraph says.
#[test]
fn pid_text_box_absent_leaves_the_anchor_code_in_charge() {
    let mut anchored = Text::new();
    anchored.value = "PG-1001".to_string();
    anchored.height = 2.469;
    anchored.insertion_point.x = 100.0;
    anchored.insertion_point.y = 50.0;
    anchored.common.layer = LAYER_TEXT.to_string();
    let mut anchored = EntityType::Text(anchored);
    apply_text_anchor(&mut anchored, 0x15, None);
    let EntityType::Text(anchored) = anchored else {
        unreachable!("built as text");
    };
    for paragraph in [
        None,
        Some(TextAlignment::Left),
        Some(TextAlignment::Center),
        Some(TextAlignment::Right),
    ] {
        let (text, _, _) = boxed_label("PG-1001", (100.0, 50.0), 0.0, 0x15, None, paragraph);
        assert_eq!(
            (
                text.horizontal_alignment,
                text.vertical_alignment,
                text.insertion_point,
                text.alignment_point,
            ),
            (
                anchored.horizontal_alignment,
                anchored.vertical_alignment,
                anchored.insertion_point,
                anchored.alignment_point,
            ),
            "{paragraph:?}"
        );
    }
}

/// Read from a drawing: `DWG-0100集05-38` is lettered twice on the corpus's
/// `一级闪蒸分离器B`, by records that state a 45.307 mm box around
/// insertions at x = 106.956 and 693.814 mm and a left-aligned paragraph. Both
/// start at their box's left edge.
#[test]
fn pid_text_box_sheet_numbers_start_at_their_box_left_edge() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|dir| {
            dir.join("pid-parse")
                .join("test-file")
                .join("集注站 凝析油闪蒸单元 一级闪蒸分离器B工艺及自控流程图.pid")
        })
        .filter(|path| path.is_file());
    let Some(path) = path else {
        return; // fixture absent: soft-skip like the corpus tests do
    };
    let import = load_pid(&path).expect("the fixture imports");
    let mut starts: Vec<(TextHorizontalAlignment, f64)> = import
        .document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Text(text) if text.value == "DWG-0100集05-38" => Some((
                text.horizontal_alignment,
                text.alignment_point.unwrap_or(text.insertion_point).x,
            )),
            _ => None,
        })
        .collect();
    starts.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(starts.len(), 2, "{starts:?}");
    for ((alignment, x), insertion) in starts.iter().zip([106.956, 693.814]) {
        assert_eq!(*alignment, TextHorizontalAlignment::Left, "{starts:?}");
        assert!((x - (insertion - 45.307 / 2.0)).abs() < 0.01, "{starts:?}");
    }
}

/// A sheet record's lettering as its styles state it: 7 pt in `font`, the
/// paragraph's lines spaced by `leading`, the height measured the way
/// `height_mode` says.
fn line_lettering(
    font: &str,
    leading: Option<pid_parse::style_link::LineLeading>,
    height_mode: Option<u8>,
) -> pid_parse::style_link::ResolvedTextHeight {
    pid_parse::style_link::ResolvedTextHeight {
        style_id: 0,
        height_m: 0.002_469,
        height_mode,
        width_permille: None,
        colour: None,
        alignment: None,
        line_spacing: None,
        leading,
        font_name: Some(font.to_string()),
    }
}

/// Where the canvas starts the baseline of a 2.469 mm sheet record inserted
/// at (100, 50) and placed by its anchor code, in drawing millimetres.
fn line_origin(
    code: u8,
    rotation: f64,
    lettering: Option<&pid_parse::style_link::ResolvedTextHeight>,
) -> [f64; 2] {
    use crate::entities::text::text_run_placement_at_scale;

    let mut text = Text::new();
    text.value = "PG-1001".to_string();
    text.height = 2.469;
    text.insertion_point.x = 100.0;
    text.insertion_point.y = 50.0;
    text.rotation = rotation;
    text.common.layer = LAYER_TEXT.to_string();
    let mut entity = EntityType::Text(text);
    apply_text_anchor(&mut entity, code, lettering);
    let EntityType::Text(text) = entity else {
        unreachable!("built as text");
    };
    let placement = text_run_placement_at_scale(&text, &Document::new(), 1.0);
    [placement.origin[0], placement.origin[1]]
}

/// The low nibble of a sheet record's anchor code names the line its
/// insertion point sits on, the way `pid-plot` reads it: `0` the baseline,
/// `1` and `3` the face's ascender line, `2` the top of the body. Arial
/// Narrow's ascender is 58 px of a 72 px body whose capitals take 44.
#[test]
fn pid_text_line_low_nibble_names_the_line_the_insertion_sits_on() {
    use pid_parse::style_link::LineLeading;

    let narrow = line_lettering("Arial Narrow", Some(LineLeading::FractionTile), Some(0x0D));
    for (code, above) in [
        (0x10_u8, 0.0),
        (0x00, 0.0),
        (0x01, 58.0 / 44.0),
        (0x21, 58.0 / 44.0),
        (0x03, 58.0 / 44.0),
        (0x12, 1.0),
    ] {
        let origin = line_origin(code, 0.0, Some(&narrow));
        assert!(
            (origin[1] - (50.0 - above * 2.469)).abs() < 1e-6,
            "{code:#04x}: baseline at {}",
            origin[1]
        );
    }
}

/// Any other low nibble centres the run on the middle of the body, halfway
/// between the face's ascender and descender lines, so where that is depends
/// on the face: half a cap height up in Arial Narrow, 0.539 of one in 仿宋
/// and 0.478 in Arial. A face outside `pid-plot`'s table, and a record with
/// no style at all, are measured as Arial, the way `pid-plot` measures them.
#[test]
fn pid_text_line_middle_sits_where_the_face_puts_it() {
    use pid_parse::style_link::LineLeading;

    for (font, middle) in [
        (Some("Arial Narrow"), 22.0 / 44.0),
        (Some("Arial"), 22.0 / 46.0),
        (Some("宋体"), 26.0 / 49.2),
        (Some("SimSun"), 26.0 / 49.2),
        (Some("仿宋"), 26.0 / 48.2),
        (Some("Braggadocio"), 22.0 / 46.0),
        (None, 22.0 / 46.0),
    ] {
        let lettering =
            font.map(|font| line_lettering(font, Some(LineLeading::FractionTile), Some(0x0D)));
        for code in [0x05_u8, 0x15, 0x25, 0x18, 0x38, 0x48, 0x58] {
            let origin = line_origin(code, 0.0, lettering.as_ref());
            assert!(
                (origin[1] - (50.0 - middle * 2.469)).abs() < 1e-6,
                "{font:?} {code:#04x}: baseline at {}",
                origin[1]
            );
        }
    }
}

/// The device tags lettered in Arial Narrow under a paragraph whose lines are
/// spaced by the face's ascent hang from half its ascender, 29 px against the
/// 44 its capitals take, rather than from the middle of the body: `pid-plot`
/// measured them 0.656 of a body down. Only an `8` does: a `5` under the same
/// paragraph keeps the middle, and so does an `8` under any other spacing.
#[test]
fn pid_text_line_ascent_spaced_tags_hang_from_half_the_ascender() {
    use pid_parse::style_link::LineLeading;

    let ascent_spaced = line_lettering(
        "Arial Narrow",
        Some(LineLeading::FractionAscender),
        Some(0x0D),
    );
    let tile_spaced = line_lettering("Arial Narrow", Some(LineLeading::FractionTile), Some(0x0D));
    for (code, lettering, above) in [
        (0x48_u8, &ascent_spaced, 29.0 / 44.0),
        (0x08, &ascent_spaced, 29.0 / 44.0),
        (0x45, &ascent_spaced, 0.5),
        (0x48, &tile_spaced, 0.5),
    ] {
        let origin = line_origin(code, 0.0, Some(lettering));
        assert!(
            (origin[1] - (50.0 - above * 2.469)).abs() < 1e-6,
            "{code:#04x} {:?}: baseline at {}",
            lettering.leading,
            origin[1]
        );
    }
}

/// A character style that measures its height to the face's ascent rather
/// than its capitals puts the ascender line one height up and the rest in
/// proportion: the top of the body at 44/58 of it and the middle at half
/// that. No sheet or template record of the corpus states it.
#[test]
fn pid_text_line_ascent_measured_height_scales_by_the_ascent() {
    let ascent_measured = line_lettering("Arial Narrow", None, Some(0x0C));
    for (code, above) in [
        (0x10_u8, 0.0),
        (0x01, 1.0),
        (0x12, 44.0 / 58.0),
        (0x15, 22.0 / 58.0),
    ] {
        let origin = line_origin(code, 0.0, Some(&ascent_measured));
        assert!(
            (origin[1] - (50.0 - above * 2.469)).abs() < 1e-6,
            "{code:#04x}: baseline at {}",
            origin[1]
        );
    }
}

/// The baseline drops across the run rather than down the page: a record
/// lettered up the page has its baseline to the right of the insertion.
#[test]
fn pid_text_line_rotated_record_drops_across_its_baseline() {
    use pid_parse::style_link::LineLeading;

    let narrow = line_lettering("Arial Narrow", Some(LineLeading::FractionTile), Some(0x0D));
    let origin = line_origin(0x01, std::f64::consts::FRAC_PI_2, Some(&narrow));
    assert!(
        (origin[0] - (100.0 + 58.0 / 44.0 * 2.469)).abs() < 1e-6,
        "{origin:?}"
    );
    assert!((origin[1] - 50.0).abs() < 1e-6, "{origin:?}");
}
