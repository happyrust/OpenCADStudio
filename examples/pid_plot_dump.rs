//! Temporary probe: dump an imported drawing as flat CSV so it can be
//! plotted and eyeballed.
//!
//! Row shape matches `pid-parse`'s `dump_symbol_geometry`, so the same
//! plotting script draws either a single symbol body or a whole sheet:
//!
//! ```text
//! line,x1,y1,x2,y2
//! circle,cx,cy,r
//! poly,x1,y1,x2,y2,...
//! text,x,y,height,rotation_deg,"value"
//! ```
//!
//! Arcs are emitted as sampled polylines rather than as their own row, which
//! keeps the plotter from having to know this crate's angle convention.
//!
//! Usage: `pid_plot_dump <file> [layer,layer,...]`
//!
//! The layer list is exact names, not a prefix: `PID-SYMBOL` and
//! `PID-SYMBOL-LABEL` are different things and the latter ships hidden.

use acadrust::EntityType;
use OpenCADStudio::io;

const ARC_STEPS: usize = 48;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: pid_plot_dump <file> [layer,layer,...]");
        return;
    };
    let wanted: Option<Vec<String>> = args
        .next()
        .map(|list| list.split(',').map(str::to_owned).collect());
    let doc = match io::load_file(&std::path::PathBuf::from(&path)) {
        Ok(doc) => doc,
        Err(error) => {
            eprintln!("{path}: FAILED {error}");
            return;
        }
    };

    for entity in doc.entities() {
        if let Some(layers) = &wanted {
            if !layer_of(entity).is_some_and(|l| layers.iter().any(|w| w == l)) {
                continue;
            }
        }
        match entity {
            EntityType::Line(l) => {
                println!("line,{},{},{},{}", l.start.x, l.start.y, l.end.x, l.end.y);
            }
            EntityType::Circle(c) => {
                println!("circle,{},{},{}", c.center.x, c.center.y, c.radius);
            }
            EntityType::Arc(a) => {
                let (from, to) = (a.start_angle.to_radians(), a.end_angle.to_radians());
                let sweep = {
                    let raw = to - from;
                    if raw <= 0.0 {
                        raw + std::f64::consts::TAU
                    } else {
                        raw
                    }
                };
                let points: Vec<String> = (0..=ARC_STEPS)
                    .flat_map(|i| {
                        let angle = from + sweep * (i as f64 / ARC_STEPS as f64);
                        let (sin, cos) = angle.sin_cos();
                        [
                            (a.center.x + a.radius * cos).to_string(),
                            (a.center.y + a.radius * sin).to_string(),
                        ]
                    })
                    .collect();
                println!("poly,{}", points.join(","));
            }
            EntityType::LwPolyline(p) => {
                if p.vertices.len() < 2 {
                    continue;
                }
                let points: Vec<String> = p
                    .vertices
                    .iter()
                    .flat_map(|v| [v.location.x.to_string(), v.location.y.to_string()])
                    .collect();
                println!("poly,{}", points.join(","));
            }
            EntityType::Text(t) => {
                if t.value.trim().is_empty() {
                    continue;
                }
                println!(
                    "text,{},{},{},{},{:?}",
                    t.insertion_point.x, t.insertion_point.y, t.height, t.rotation, t.value
                );
            }
            _ => {}
        }
    }
}

fn layer_of(entity: &EntityType) -> Option<&str> {
    match entity {
        EntityType::Line(l) => Some(l.common.layer.as_str()),
        EntityType::Circle(c) => Some(c.common.layer.as_str()),
        EntityType::Arc(a) => Some(a.common.layer.as_str()),
        EntityType::LwPolyline(p) => Some(p.common.layer.as_str()),
        EntityType::Text(t) => Some(t.common.layer.as_str()),
        _ => None,
    }
}
