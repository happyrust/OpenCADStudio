//! Temporary probe: what does the `.pid` importer actually hand the scene?

use std::collections::BTreeMap;

use acadrust::EntityType;
use OpenCADStudio::io;

fn main() {
    for arg in std::env::args().skip(1) {
        let path = std::path::PathBuf::from(&arg);
        let doc = match io::load_file(&path) {
            Ok(doc) => doc,
            Err(error) => {
                println!("{arg}: FAILED {error}");
                continue;
            }
        };
        println!("{arg}");
        println!("  entities   = {}", doc.entity_count());
        println!(
            "  ext_min    = ({:.2}, {:.2})  ext_max = ({:.2}, {:.2})",
            doc.header.model_space_extents_min.x,
            doc.header.model_space_extents_min.y,
            doc.header.model_space_extents_max.x,
            doc.header.model_space_extents_max.y
        );
        println!("  ms_block   = {:?}", doc.header.model_space_block_handle);
        let vports: Vec<_> = doc.vports.iter().collect();
        println!("  vports     = {}", vports.len());
        for vp in vports {
            println!(
                "    name={:?} handle={:?} view_height={:.3} target=({:.2}, {:.2}) center=({:.2}, {:.2}) dir=({:.1},{:.1},{:.1}) ll=({:.2},{:.2}) ur=({:.2},{:.2})",
                vp.name, vp.handle, vp.view_height,
                vp.view_target.x, vp.view_target.y,
                vp.view_center.x, vp.view_center.y,
                vp.view_direction.x, vp.view_direction.y, vp.view_direction.z,
                vp.lower_left.x, vp.lower_left.y,
                vp.upper_right.x, vp.upper_right.y
            );
        }
        let mut owned = 0usize;
        let mut per_layer: BTreeMap<String, usize> = BTreeMap::new();
        let mut labels: BTreeMap<String, usize> = BTreeMap::new();
        let mut heights: BTreeMap<String, usize> = BTreeMap::new();
        for e in doc.entities() {
            if e.common().owner_handle == doc.header.model_space_block_handle {
                owned += 1;
            }
            *per_layer.entry(e.common().layer.clone()).or_default() += 1;
            if let EntityType::Text(t) = e {
                if t.common.layer == "PID-SYMBOL-LABEL" {
                    *labels.entry(t.value.clone()).or_default() += 1;
                } else {
                    *heights
                        .entry(format!("{:.2}mm rot={:.0}", t.height, t.rotation))
                        .or_default() += 1;
                }
            }
        }
        println!("  owned_by_model_space = {owned}");
        println!("  layers:");
        for (layer, count) in &per_layer {
            println!("    {layer:<18} {count}");
        }
        println!("  symbol labels ({} distinct):", labels.len());
        for (name, count) in &labels {
            println!("    {count:>3} x {name}");
        }
        println!("  text height/rotation ({} distinct):", heights.len());
        for (key, count) in &heights {
            println!("    {count:>3} x {key}");
        }

        // A sheet is at most ~1189mm (A0) wide; anything reaching past 900 or
        // behind 0 either is the border or is the reason the view is wrong.
        println!("  entities reaching x>900 or x<0:");
        let mut outliers = 0usize;
        for e in doc.entities() {
            let pts: Vec<(f64, f64)> = match e {
                EntityType::Line(l) => vec![(l.start.x, l.start.y), (l.end.x, l.end.y)],
                EntityType::LwPolyline(p) => p
                    .vertices
                    .iter()
                    .map(|v| (v.location.x, v.location.y))
                    .collect(),
                EntityType::Circle(c) => vec![(c.center.x, c.center.y)],
                EntityType::Arc(a) => vec![(a.center.x, a.center.y)],
                EntityType::Text(t) => vec![(t.insertion_point.x, t.insertion_point.y)],
                EntityType::Point(p) => vec![(p.location.x, p.location.y)],
                _ => Vec::new(),
            };
            if pts.iter().any(|(x, _)| *x > 900.0 || *x < 0.0) {
                outliers += 1;
                if outliers <= 12 {
                    let kind = match e {
                        EntityType::Line(_) => "Line",
                        EntityType::LwPolyline(_) => "LwPolyline",
                        EntityType::Circle(_) => "Circle",
                        EntityType::Arc(_) => "Arc",
                        EntityType::Text(_) => "Text",
                        EntityType::Point(_) => "Point",
                        _ => "other",
                    };
                    let shown: Vec<String> = pts
                        .iter()
                        .take(4)
                        .map(|(x, y)| format!("({x:.1},{y:.1})"))
                        .collect();
                    println!("    {kind:<12} {}", shown.join(" "));
                }
            }
        }
        println!("    total outliers = {outliers}");
    }
}