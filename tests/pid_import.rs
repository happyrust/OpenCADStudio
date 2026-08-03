//! `.pid` import regression cover.
//!
//! The fixtures live in the sibling `pid-parse` checkout rather than in this
//! repository -- they are real SmartPlant drawings, and the parser they
//! exercise is developed against them there. Every test soft-skips when that
//! checkout is absent, the way the ACadSharp sample tests do, so a clone of
//! this repository alone still runs green.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use acadrust::{CadDocument, EntityType};

// An A2 sheet is 594 x 420mm. The decoded content of both fixtures sits
// inside that, so any converted coordinate an order of magnitude past it is a
// stray that reached the drawing rather than the diagnostic layers.
const SHEET_LIMIT_MM: f64 = 2000.0;

fn fixture(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("pid-parse")
        .join("test-file")
        .join(name);
    path.is_file().then_some(path)
}

fn import(name: &str) -> Option<CadDocument> {
    let path = fixture(name)?;
    Some(
        OpenCADStudio::io::load_file(&path)
            .unwrap_or_else(|error| panic!("load {}: {error}", path.display())),
    )
}

fn layer_of(entity: &EntityType) -> &str {
    entity.common().layer.as_str()
}

fn on_layer<'a>(doc: &'a CadDocument, layer: &'a str) -> impl Iterator<Item = &'a EntityType> {
    doc.entities().filter(move |e| layer_of(e) == layer)
}

fn is_hidden(doc: &CadDocument, layer: &str) -> bool {
    doc.layers
        .get(layer)
        .unwrap_or_else(|| panic!("{layer} is not in the layer table"))
        .flags
        .off
}

/// Every layer the importer names exists, and the ones carrying evidence
/// rather than drawing ship switched off.
#[test]
fn import_declares_its_layers_and_hides_the_evidence_ones() {
    let Some(doc) = import("DWG-0201GP06-01.pid") else {
        return;
    };

    for visible in [
        "PID-GEOMETRY",
        "PID-FRAME",
        "PID-TEXT",
        "PID-SYMBOL",
        "PID-POINT",
    ] {
        assert!(!is_hidden(&doc, visible), "{visible} must open visible");
    }
    for hidden in [
        "PID-SYMBOL-LABEL",
        "PID-ANNOTATION",
        "PID-CONNECTIVITY",
        "PID-UNRESOLVED",
    ] {
        assert!(is_hidden(&doc, hidden), "{hidden} must open hidden");
    }
}

/// A `GLine2d` whose parameter range never resolved decodes as the origin
/// walked one whole source unit -- a 1000mm rule straight across a 594mm
/// sheet. It stays in the document, because the record is in the file, but on
/// the hidden diagnostic layer rather than among the drawing's line work.
#[test]
fn unresolved_unit_lines_are_kept_off_the_drawing() {
    let Some(doc) = import("DWG-0201GP06-01.pid") else {
        return;
    };

    let unresolved: Vec<_> = on_layer(&doc, "PID-UNRESOLVED").collect();
    assert!(
        !unresolved.is_empty(),
        "DWG-0201 has two unresolved unit lines; none reached the diagnostic layer"
    );

    for entity in on_layer(&doc, "PID-GEOMETRY") {
        let EntityType::Line(line) = entity else {
            continue;
        };
        let width = (line.end.x - line.start.x).abs();
        assert!(
            width < 900.0,
            "a {width:.0}mm line is on the drawing layer; the unit-line filter missed it"
        );
    }
}

/// Endpoint pairs are the drawing's connectivity graph. Only the ones whose
/// two ends both land on the sheet are drawn, on their own hidden layer.
#[test]
fn connectivity_links_stay_on_the_sheet() {
    let Some(doc) = import("DWG-0201GP06-01.pid") else {
        return;
    };

    let links: Vec<_> = on_layer(&doc, "PID-CONNECTIVITY").collect();
    assert!(
        !links.is_empty(),
        "DWG-0201 has 35 on-sheet endpoint pairs; none were imported"
    );
    for entity in links {
        let EntityType::Line(line) = entity else {
            panic!("PID-CONNECTIVITY carries lines only, found {entity:?}");
        };
        for value in [line.start.x, line.start.y, line.end.x, line.end.y] {
            assert!(
                value.abs() < SHEET_LIMIT_MM,
                "connectivity link reaches {value:.0}mm, which is off the sheet"
            );
        }
        assert!(
            line.start.distance(&line.end) > 0.0,
            "a zero-length link carries no direction to draw"
        );
    }
}

/// The opening view is stated by the importer rather than left to the
/// document default, and it is stated over the sheet.
#[test]
fn import_frames_the_drawing_on_its_sheet() {
    let Some(doc) = import("DWG-0201GP06-01.pid") else {
        return;
    };

    let extents_min = doc.header.model_space_extents_min;
    let extents_max = doc.header.model_space_extents_max;
    assert!(
        extents_max.x > extents_min.x && extents_max.y > extents_min.y,
        "model-space extents are empty: {extents_min:?}..{extents_max:?}"
    );
    assert!(
        extents_max.x < SHEET_LIMIT_MM && extents_max.y < SHEET_LIMIT_MM,
        "extents {extents_max:?} were framed on an off-sheet stray"
    );

    let vport = doc
        .vports
        .get("*Active")
        .expect("importer states the opening viewport");
    // The default `CadDocument` entry is parked at the origin with a 10-unit
    // height; anything sheet-sized means the importer replaced it.
    assert!(
        vport.view_height > 100.0,
        "*Active still has the default {}-unit height",
        vport.view_height
    );
}

/// The sheet's border is drawn, because a `.pid` carries it as an OLE object
/// linked into the drawing rather than as line work: without this the content
/// hangs in an empty background with no edge to read it against.
///
/// Only the rectangle is drawn. Its corners are the page `pid-parse` decoded
/// from the drawing's own `igSmartFrame2d` record, so the border and the
/// opening view agree by construction.
#[test]
fn the_sheet_border_is_drawn_at_the_page_the_drawing_states() {
    let Some(doc) = import("DWG-0201GP06-01.pid") else {
        return;
    };

    let borders: Vec<_> = on_layer(&doc, "PID-FRAME").collect();
    assert_eq!(borders.len(), 1, "one sheet carries one border");
    let EntityType::LwPolyline(border) = borders[0] else {
        panic!("the border is a polyline, found {:?}", borders[0]);
    };
    assert!(
        border.is_closed,
        "an open border does not read as a sheet edge"
    );
    assert_eq!(border.vertices.len(), 4, "a sheet is a rectangle");

    let xs: Vec<f64> = border.vertices.iter().map(|v| v.location.x).collect();
    let ys: Vec<f64> = border.vertices.iter().map(|v| v.location.y).collect();
    let min_x = xs.iter().copied().fold(f64::MAX, f64::min);
    let min_y = ys.iter().copied().fold(f64::MAX, f64::min);
    let width = xs.iter().copied().fold(f64::MIN, f64::max) - min_x;
    let height = ys.iter().copied().fold(f64::MIN, f64::max) - min_y;

    assert!(
        min_x.abs() < 1.0e-9 && min_y.abs() < 1.0e-9,
        "the page starts at the origin, this one at ({min_x}, {min_y})"
    );
    // DWG-0201 is an A2 whose own frame measures 594.3 x 420.3mm.
    assert!(
        (width - 594.3).abs() < 0.1 && (height - 420.3).abs() < 0.1,
        "border is {width:.1} x {height:.1}mm, the drawing states 594.3 x 420.3"
    );

    for entity in on_layer(&doc, "PID-GEOMETRY") {
        for point in geometry_extremes(entity) {
            assert!(
                point.0 > -SHEET_MARGIN_MM && point.0 < width + SHEET_MARGIN_MM,
                "drawing content at x={} is not on the {width:.1}mm sheet",
                point.0
            );
        }
    }
}

/// How far outside the border a drawn coordinate may still sit. A symbol
/// whose insertion point a misparse nudged past the edge is still part of the
/// drawing; a coordinate a page-width away is not.
const SHEET_MARGIN_MM: f64 = 100.0;

fn geometry_extremes(entity: &EntityType) -> Vec<(f64, f64)> {
    match entity {
        EntityType::Line(line) => vec![(line.start.x, line.start.y), (line.end.x, line.end.y)],
        EntityType::LwPolyline(polyline) => polyline
            .vertices
            .iter()
            .map(|v| (v.location.x, v.location.y))
            .collect(),
        EntityType::Circle(circle) => vec![(circle.center.x, circle.center.y)],
        _ => Vec::new(),
    }
}

/// Both fixtures import, and the drawing lands on the layers that open
/// visible rather than only on the diagnostic ones.
#[test]
fn fixtures_import_with_visible_drawing_content() {
    for name in [
        "DWG-0201GP06-01.pid",
        "DWG-0202GP06-01.pid",
        "D06.pid",
        "工艺管道及仪表流程-1.pid",
    ] {
        let Some(doc) = import(name) else {
            continue;
        };
        let visible = ["PID-GEOMETRY", "PID-TEXT", "PID-SYMBOL", "PID-POINT"]
            .iter()
            .map(|layer| on_layer(&doc, layer).count())
            .sum::<usize>();
        assert!(visible > 0, "{name}: nothing reached a visible layer");
        assert_eq!(
            doc.source_path.as_deref().map(|p| p.ends_with(name)),
            Some(true),
            "{name}: import did not record where it came from"
        );
    }
}
