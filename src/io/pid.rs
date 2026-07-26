// SmartPlant / Smart P&ID `.pid` import.
//
// `pid-parse` decodes the CFB container's Sheet streams into a normalized
// geometry projection; this module maps the source-backed part of that
// projection onto acadrust entities so a `.pid` opens like any other drawing.
//
// Only `Decoded` entities are imported. `Inferred` coordinate hints are raw
// i32 pairs that land in the +/-900k range on real drawings -- plotting them
// would scatter the sheet across a region a thousand times its own size --
// and `ProbeOnly` evidence has no position at all.

use std::path::Path;

use acadrust::entities::{Circle, Line, LwPolyline, Point, Text};
use acadrust::types::{Vector2, Vector3};
use acadrust::{CadDocument, EntityType};
use pid_parse::{
    build_normalized_geometry, PidGeometryConfidence, PidGraphicKind, PidParser, PidPoint,
};

// `pid-parse` reports Sheet units as undecoded, but the decoded extents of
// every fixture land in 0..1 and match the drawing's own ISO template once
// multiplied by 1000 (an A2 sheet measures 0.584 x 0.410, an A1 0.827 x 0.551),
// so the source unit is the metre.
const MM_PER_SOURCE_UNIT: f64 = 1000.0;

// SmartPlant carries text height in the style record, which is not decoded
// yet; 2.5mm is the ISO 3098 body-text size a P&ID annotation normally uses.
const TEXT_HEIGHT_MM: f64 = 2.5;

// A symbol's body lives in an external `.sym` library that the fixtures
// reference over UNC and `pid-parse` does not decode, so a placement can be
// marked but not drawn. The marker sits on its own layer to be switched off.
const SYMBOL_MARKER_RADIUS_MM: f64 = 1.5;

const LAYER_GEOMETRY: &str = "PID-GEOMETRY";
const LAYER_TEXT: &str = "PID-TEXT";
const LAYER_SYMBOL: &str = "PID-SYMBOL";
const LAYER_POINT: &str = "PID-POINT";

/// Parse a `.pid` file and project its decoded Sheet geometry into a document.
pub fn load_pid(path: &Path) -> Result<CadDocument, String> {
    let parsed = PidParser::new()
        .parse_file(path)
        .map_err(|error| error.to_string())?;
    let geometry = build_normalized_geometry(&parsed);

    let mut doc = CadDocument::new();
    crate::io::linetypes::populate_document(&mut doc);
    for layer in [LAYER_GEOMETRY, LAYER_TEXT, LAYER_SYMBOL, LAYER_POINT] {
        ensure_layer(&mut doc, layer);
    }

    let mut bounds = Bounds::default();
    for entity in &geometry.entities {
        if entity.confidence != PidGeometryConfidence::Decoded {
            continue;
        }
        let built = build_entities(&entity.kind);
        if built.is_empty() {
            continue;
        }
        accumulate_bounds(&entity.kind, &mut bounds);
        for one in built {
            let _ = doc.add_entity(one);
        }
    }

    if doc.entity_count() == 0 {
        return Err(format!(
            "No decoded geometry in {}: pid-parse produced {} evidence item(s), none of them source-backed",
            path.display(),
            geometry.entities.len()
        ));
    }

    frame_drawing(&mut doc, &bounds);
    doc.source_path = Some(path.to_string_lossy().into_owned());
    Ok(doc)
}

fn build_entities(kind: &PidGraphicKind) -> Vec<EntityType> {
    match kind {
        PidGraphicKind::Line { start, end } => {
            let mut line = Line::from_points(point3(start), point3(end));
            line.common.layer = LAYER_GEOMETRY.to_string();
            vec![EntityType::Line(line)]
        }
        PidGraphicKind::Polyline { points, closed } => {
            if points.len() < 2 {
                return Vec::new();
            }
            let vertices: Vec<Vector2> = points
                .iter()
                .map(|p| Vector2::new(to_mm(p.x), to_mm(p.y)))
                .collect();
            let mut polyline = LwPolyline::from_points(vertices);
            polyline.is_closed = *closed;
            polyline.common.layer = LAYER_GEOMETRY.to_string();
            vec![EntityType::LwPolyline(polyline)]
        }
        PidGraphicKind::Circle { center, radius } => {
            let mut circle = Circle::new();
            circle.center = point3(center);
            circle.radius = to_mm(*radius);
            circle.common.layer = LAYER_GEOMETRY.to_string();
            vec![EntityType::Circle(circle)]
        }
        PidGraphicKind::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let mut arc = acadrust::entities::Arc::new();
            arc.center = point3(center);
            arc.radius = to_mm(*radius);
            arc.start_angle = start_angle.to_degrees();
            arc.end_angle = end_angle.to_degrees();
            arc.common.layer = LAYER_GEOMETRY.to_string();
            vec![EntityType::Arc(arc)]
        }
        PidGraphicKind::Text {
            insertion,
            value,
            height,
            rotation,
        } => {
            if value.trim().is_empty() {
                return Vec::new();
            }
            let mut text = Text::new();
            text.value = value.clone();
            text.insertion_point = point3(insertion);
            text.height = if *height > 0.0 {
                to_mm(*height)
            } else {
                TEXT_HEIGHT_MM
            };
            text.rotation = rotation.to_degrees();
            text.common.layer = LAYER_TEXT.to_string();
            vec![EntityType::Text(text)]
        }
        PidGraphicKind::SymbolInstance { insertion, .. } => {
            let mut marker = Circle::new();
            marker.center = point3(insertion);
            marker.radius = SYMBOL_MARKER_RADIUS_MM;
            marker.common.layer = LAYER_SYMBOL.to_string();
            vec![EntityType::Circle(marker)]
        }
        PidGraphicKind::Point { position } => {
            let mut point = Point::new();
            point.location = point3(position);
            point.common.layer = LAYER_POINT.to_string();
            vec![EntityType::Point(point)]
        }
        PidGraphicKind::Annotation { .. } | PidGraphicKind::Unknown { .. } => Vec::new(),
    }
}

/// State the opening view, the way a DWG does.
///
/// `Scene::restore_saved_camera` reads the `*Active` VPORT and only falls back
/// to `fit_all` when there is none. Neither default is usable here: a fresh
/// `CadDocument` ships an `*Active` entry parked at the origin with a 10-unit
/// height, and `fit_all` fits every wire including the off-sheet strays that
/// `on_sheet` keeps out of the framing box. So the importer states the view
/// itself, over the filtered bounds.
fn frame_drawing(doc: &mut CadDocument, bounds: &Bounds) {
    if bounds.is_empty() {
        return;
    }
    let width = bounds.max_x - bounds.min_x;
    let height = bounds.max_y - bounds.min_y;

    doc.header.model_space_extents_min = Vector3::new(bounds.min_x, bounds.min_y, 0.0);
    doc.header.model_space_extents_max = Vector3::new(bounds.max_x, bounds.max_y, 0.0);

    let mut vport = acadrust::tables::VPort::new("*Active");
    vport.lower_left = Vector2::new(0.0, 0.0);
    vport.upper_right = Vector2::new(1.0, 1.0);
    vport.view_direction = Vector3::new(0.0, 0.0, 1.0);
    vport.view_target = Vector3::new(
        (bounds.min_x + bounds.max_x) / 2.0,
        (bounds.min_y + bounds.max_y) / 2.0,
        0.0,
    );
    vport.view_center = Vector2::ZERO;
    // `view_height` alone decides the zoom, so a landscape sheet in a window
    // narrower than 4:3 would spill sideways; widen it to cover that case.
    vport.view_height = (height.max(width * 0.75) * 1.05).max(1.0);
    vport.handle = doc.allocate_handle();
    // `add` refuses a duplicate name, and the default entry above is one.
    doc.vports.add_or_replace(vport);
}

struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            min_x: f64::MAX,
            min_y: f64::MAX,
            max_x: f64::MIN,
            max_y: f64::MIN,
        }
    }
}

impl Bounds {
    fn add(&mut self, point: &PidPoint) {
        let (x, y) = (to_mm(point.x), to_mm(point.y));
        if !on_sheet(x) || !on_sheet(y) {
            return;
        }
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    fn is_empty(&self) -> bool {
        self.min_x > self.max_x || self.min_y > self.max_y
    }
}

fn accumulate_bounds(kind: &PidGraphicKind, bounds: &mut Bounds) {
    match kind {
        PidGraphicKind::Line { start, end } => {
            bounds.add(start);
            bounds.add(end);
        }
        PidGraphicKind::Polyline { points, .. } => {
            for point in points {
                bounds.add(point);
            }
        }
        PidGraphicKind::Circle { center, .. } | PidGraphicKind::Arc { center, .. } => {
            bounds.add(center);
        }
        PidGraphicKind::Text { insertion, .. }
        | PidGraphicKind::SymbolInstance { insertion, .. } => bounds.add(insertion),
        PidGraphicKind::Point { position } => bounds.add(position),
        PidGraphicKind::Annotation { .. } | PidGraphicKind::Unknown { .. } => {}
    }
}

fn to_mm(value: f64) -> f64 {
    value * MM_PER_SOURCE_UNIT
}

/// Whether a converted coordinate could plausibly sit on a drawing sheet.
///
/// A misparsed record still yields the occasional coordinate off the page --
/// one fixture places a symbol 126mm below it -- and framing to those leaves
/// the drawing small and off-centre. Only framing is filtered; the entity
/// itself is still imported, so zooming out still finds it.
fn on_sheet(value: f64) -> bool {
    // The largest ISO sheet, A0, is 1189 x 841mm; the bound is loose enough
    // for an oversized custom sheet and still rejects a metres-off outlier.
    value.is_finite() && (-100.0..=2000.0).contains(&value)
}

fn point3(point: &PidPoint) -> Vector3 {
    Vector3::new(to_mm(point.x), to_mm(point.y), 0.0)
}

fn ensure_layer(doc: &mut CadDocument, name: &str) {
    if doc.layers.contains(name) {
        return;
    }
    let mut layer = acadrust::tables::Layer::new(name);
    layer.handle = doc.allocate_handle();
    let _ = doc.layers.add(layer);
}
