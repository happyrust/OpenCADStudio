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

use std::path::{Path, PathBuf};

use acadrust::entities::{Circle, Line, LwPolyline, Point, Text};
use acadrust::types::{Vector2, Vector3};
use acadrust::{CadDocument, EntityType};
use pid_parse::symbol_library::{SymbolLibrary, SymbolPrimitive};
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

// What a symbol's template text reads as where the drawing is expected to
// supply the value. See `carries_a_label`.
const SYMBOL_TEXT_PLACEHOLDER: &str = "NULL";

// A symbol's body lives in an external `.sym` library the drawing references
// over UNC. With the library on hand the body is drawn; without it, or for a
// symbol the local copy lacks, the placement falls back to this marker so it
// is still visible as something rather than silently absent.
const SYMBOL_MARKER_RADIUS_MM: f64 = 1.5;

// Environment override for the reference-data shares holding the `Design`,
// `Piping`, `Equipment` ... symbol trees, `;` separated like PATH. Without it
// the importer looks for the library next to the drawing.
const SYMBOL_LIBRARY_ENV: &str = "PID_SYMBOL_LIBRARY";

// The library file name says what the marker stands for ("Flanged Nozzle",
// "Gauge Hatch"), which is the readable half of a symbol until the body can be
// drawn. Smaller than body text so a label never reads as an annotation, and
// on its own layer because a dense sheet carries dozens of them.
const SYMBOL_LABEL_HEIGHT_MM: f64 = 2.0;
const SYMBOL_LABEL_GAP_MM: f64 = 0.8;

const LAYER_GEOMETRY: &str = "PID-GEOMETRY";
const LAYER_TEXT: &str = "PID-TEXT";
const LAYER_SYMBOL: &str = "PID-SYMBOL";
const LAYER_SYMBOL_LABEL: &str = "PID-SYMBOL-LABEL";
const LAYER_POINT: &str = "PID-POINT";

/// Parse a `.pid` file and project its decoded Sheet geometry into a document.
pub fn load_pid(path: &Path) -> Result<CadDocument, String> {
    let parsed = PidParser::new()
        .parse_file(path)
        .map_err(|error| error.to_string())?;
    let geometry = build_normalized_geometry(&parsed);

    let mut doc = CadDocument::new();
    crate::io::linetypes::populate_document(&mut doc);
    for (layer, visible) in [
        (LAYER_GEOMETRY, true),
        (LAYER_TEXT, true),
        (LAYER_SYMBOL, true),
        // "Flanged Nozzle with blind" is wider than the equipment it names, so
        // on a sheet with 58 placements the labels bury the drawing. They ship
        // switched off: the answer is in the file, one layer toggle away.
        (LAYER_SYMBOL_LABEL, false),
        (LAYER_POINT, true),
    ] {
        ensure_layer(&mut doc, layer, visible);
    }

    let mut library = discover_symbol_library(path);

    let mut bounds = Bounds::default();
    for entity in &geometry.entities {
        if entity.confidence != PidGeometryConfidence::Decoded {
            continue;
        }
        let built = build_entities(&entity.kind, library.as_mut());
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

fn build_entities(kind: &PidGraphicKind, library: Option<&mut SymbolLibrary>) -> Vec<EntityType> {
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
        PidGraphicKind::SymbolInstance {
            insertion,
            symbol_path,
            rotation,
            scale,
        } => {
            let placement = Placement {
                insertion,
                rotation: *rotation,
                scale: *scale,
            };
            let body = library
                .zip(symbol_path.as_deref())
                .and_then(|(library, path)| library.resolve(path))
                .filter(|body| !body.primitives.is_empty())
                .map(|body| {
                    body.primitives
                        .iter()
                        .filter_map(|primitive| place_primitive(primitive, &placement))
                        .collect::<Vec<_>>()
                });

            // Only fall back to the marker when the body is genuinely
            // unavailable. A symbol that resolved to real geometry should not
            // also carry a dot -- that reads as a second object.
            let mut built = match body {
                Some(entities) if !entities.is_empty() => entities,
                _ => {
                    let mut marker = Circle::new();
                    marker.center = point3(insertion);
                    marker.radius = SYMBOL_MARKER_RADIUS_MM;
                    marker.common.layer = LAYER_SYMBOL.to_string();
                    vec![EntityType::Circle(marker)]
                }
            };

            if let Some(name) = symbol_path.as_deref().and_then(symbol_name) {
                let mut label = Text::new();
                label.value = name;
                label.height = SYMBOL_LABEL_HEIGHT_MM;
                // Beside the marker, not on it, and horizontal whatever the
                // placement angle is -- a rotated label is the harder read.
                label.insertion_point = Vector3::new(
                    to_mm(insertion.x) + SYMBOL_MARKER_RADIUS_MM + SYMBOL_LABEL_GAP_MM,
                    to_mm(insertion.y) - SYMBOL_LABEL_HEIGHT_MM / 2.0,
                    0.0,
                );
                label.common.layer = LAYER_SYMBOL_LABEL.to_string();
                built.push(EntityType::Text(label));
            }
            built
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
            if unresolved_unit_line(start, end) {
                return;
            }
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

/// Whether a line is the unit segment a `GLine2d` decodes to when its
/// parameter range never resolved.
///
/// The parametric form is `origin + t * direction` with `direction` a unit
/// vector, so an unresolved record comes out as the origin walked one whole
/// source unit along x: `A01` yields `(1e-6, 1e-12) -> (1, 1e-6)` and
/// `DWG-0201` the same shape twice. At 1000mm that is wider than the sheet
/// it sits on, and framing on it shrinks the real drawing to a smudge in the
/// middle of the screen.
///
/// The line is still imported -- the record is in the file, and dropping it
/// would hide a decode gap rather than report it. It just gets no vote on
/// where the camera goes.
fn unresolved_unit_line(start: &PidPoint, end: &PidPoint) -> bool {
    let (start_x, start_y) = (to_mm(start.x), to_mm(start.y));
    let (end_x, end_y) = (to_mm(end.x), to_mm(end.y));
    start_y.abs() < 1.0
        && end_y.abs() < 1.0
        && start_x.abs() < 5.0
        && (end_x - MM_PER_SOURCE_UNIT).abs() < 1.0e-3
}

/// Where a symbol placement puts its library body on the sheet.
struct Placement<'a> {
    insertion: &'a PidPoint,
    rotation: f64,
    scale: [f64; 2],
}

impl Placement<'_> {
    /// Map a point out of the symbol's own space onto the sheet.
    ///
    /// `pid-parse` splits the placement's 2x2 matrix into an angle and a
    /// scale, carrying a reflection as a negative y scale instead of folding
    /// it into the angle. Recomposing gives rows `sx * (cos, sin)` and
    /// `sy * (-sin, cos)`, which reproduces the original matrix for the
    /// rotate / scale / mirror placements a P&ID uses. Symbol bodies are in
    /// the same source unit as the drawing, so the conversion to mm happens
    /// once, at the end.
    fn apply(&self, x: f64, y: f64) -> Vector3 {
        let (sin, cos) = self.rotation.sin_cos();
        let [scale_x, scale_y] = self.scale;
        Vector3::new(
            to_mm(self.insertion.x + x * scale_x * cos - y * scale_y * sin),
            to_mm(self.insertion.y + x * scale_x * sin + y * scale_y * cos),
            0.0,
        )
    }

    /// Scale a radius. A placement with different x and y scales would turn a
    /// circle into an ellipse; P&ID placements mirror and turn but do not
    /// stretch one axis alone, so the mean is exact in practice and degrades
    /// gently if that ever stops being true.
    fn scale_radius(&self, radius: f64) -> f64 {
        to_mm(radius * (self.scale[0].abs() + self.scale[1].abs()) / 2.0)
    }

    /// Whether the placement flips handedness, which reverses the direction
    /// an arc sweeps.
    fn mirrored(&self) -> bool {
        self.scale[1] < 0.0
    }
}

/// Draw one primitive of a symbol body at its placement.
fn place_primitive(primitive: &SymbolPrimitive, at: &Placement<'_>) -> Option<EntityType> {
    match primitive {
        SymbolPrimitive::Line { start, end } => {
            let mut line = Line::from_points(at.apply(start.0, start.1), at.apply(end.0, end.1));
            line.common.layer = LAYER_SYMBOL.to_string();
            Some(EntityType::Line(line))
        }
        SymbolPrimitive::Circle { center, radius } => {
            let radius = at.scale_radius(*radius);
            if !radius.is_finite() || radius <= 0.0 {
                return None;
            }
            let mut circle = Circle::new();
            circle.center = at.apply(center.0, center.1);
            circle.radius = radius;
            circle.common.layer = LAYER_SYMBOL.to_string();
            Some(EntityType::Circle(circle))
        }
        SymbolPrimitive::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let radius = at.scale_radius(*radius);
            if !radius.is_finite() || radius <= 0.0 {
                return None;
            }
            // An arc always runs counter-clockwise from start to end, so a
            // mirrored placement has to swap the ends as well as reflect the
            // angles -- otherwise the arc is drawn as its own complement.
            let (start_angle, end_angle) = if at.mirrored() {
                (at.rotation - end_angle, at.rotation - start_angle)
            } else {
                (at.rotation + start_angle, at.rotation + end_angle)
            };
            let mut arc = acadrust::entities::Arc::new();
            arc.center = at.apply(center.0, center.1);
            arc.radius = radius;
            arc.start_angle = start_angle.to_degrees();
            arc.end_angle = end_angle.to_degrees();
            arc.common.layer = LAYER_SYMBOL.to_string();
            Some(EntityType::Arc(arc))
        }
        SymbolPrimitive::Polyline { vertices } => {
            if vertices.len() < 2 {
                return None;
            }
            let points: Vec<Vector2> = vertices
                .iter()
                .map(|(x, y)| {
                    let placed = at.apply(*x, *y);
                    Vector2::new(placed.x, placed.y)
                })
                .collect();
            let mut polyline = LwPolyline::from_points(points);
            polyline.common.layer = LAYER_SYMBOL.to_string();
            Some(EntityType::LwPolyline(polyline))
        }
        SymbolPrimitive::Text { text, at: origin } => {
            let value = text.trim();
            if !carries_a_label(value) {
                return None;
            }
            let mut label = Text::new();
            label.value = value.to_string();
            label.insertion_point = at.apply(origin.0, origin.1);
            // The record holds no height, so this is the same ISO 3098
            // fallback the sheet's own text gets, scaled with the placement
            // so a half-size symbol does not carry full-size lettering.
            label.height = at.scale_radius(TEXT_HEIGHT_MM / MM_PER_SOURCE_UNIT);
            label.rotation = at.rotation.to_degrees();
            label.common.layer = LAYER_SYMBOL.to_string();
            Some(EntityType::Text(label))
        }
    }
}

/// Whether a symbol's own text run says anything once its unfilled template
/// fields are discounted.
///
/// The library is a set of templates: a run reads `NULL` wherever the drawing
/// is expected to supply a value, and 328 of the 1043 runs in the reference
/// library are nothing but that. Those are not lettering anyone drew, and
/// putting them on the sheet would print `NULL` across every equipment table.
/// A run that still has a word in it after the placeholders are discounted --
/// `HH=NULL`, `设备位号` -- is real lettering and is drawn as it stands,
/// placeholder included, because guessing at the missing value would be worse
/// than showing that it is missing.
fn carries_a_label(text: &str) -> bool {
    text.replace(SYMBOL_TEXT_PLACEHOLDER, "")
        .chars()
        .any(char::is_alphanumeric)
}

/// Find the `SmartPlant` reference-data symbol libraries for a drawing.
///
/// A `.pid` names its symbols by UNC path into the project's reference share,
/// which is normally unreachable from wherever the file is being read. The
/// override says where local copies live; failing that, a SmartPlant project
/// keeps drawings and reference data under one root (`Plant\Drawings\...` next
/// to `Plant\Ref\Symbols\...`), so walking up from the drawing finds it.
///
/// Every root found is kept, searched in the order listed. One drawing can
/// cite several shares, and a machine usually holds a partial copy of each
/// rather than one merged tree, so stopping at the first root leaves whatever
/// it does not cover undrawn.
fn discover_symbol_library(drawing: &Path) -> Option<SymbolLibrary> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(list) = std::env::var_os(SYMBOL_LIBRARY_ENV) {
        roots.extend(std::env::split_paths(&list).filter(|root| root.is_dir()));
    }
    let mut dir = drawing.parent();
    for _ in 0..5 {
        let Some(at) = dir else { break };
        for candidate in ["Symbols", "Ref/Symbols", "symbols"] {
            let path = at.join(candidate);
            if path.is_dir() && !roots.contains(&path) {
                roots.push(path);
            }
        }
        dir = at.parent();
    }
    (!roots.is_empty()).then(|| SymbolLibrary::with_roots(roots))
}

/// The symbol's name, which is the file name of the `.sym` it is placed from.
///
/// `pid-parse` resolves the placement to a library path off the drawing's
/// `JSite` layer, and those are UNC paths into a SmartPlant reference share
/// (`\\WIN-SPID\...\Piping\Valves\Angle\2-Way Angle Globe Valve.sym`), so the
/// leaf is the name the drafter picked the symbol by.
fn symbol_name(path: &str) -> Option<String> {
    let file = path.rsplit(['\\', '/']).next()?.trim();
    let stem = match file.rfind('.') {
        Some(dot) if file[dot..].eq_ignore_ascii_case(".sym") => &file[..dot],
        _ => file,
    };
    let stem = stem.trim();
    (!stem.is_empty()).then(|| stem.to_owned())
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

fn ensure_layer(doc: &mut CadDocument, name: &str, visible: bool) {
    if doc.layers.contains(name) {
        return;
    }
    let mut layer = acadrust::tables::Layer::new(name);
    layer.handle = doc.allocate_handle();
    layer.flags.off = !visible;
    let _ = doc.layers.add(layer);
}
