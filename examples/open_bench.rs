//! Headless replay of the loader-thread open path, so the phase breakdown the
//! app only prints into its command-line overlay can be captured by a script
//! instead of by burst-screenshotting the fading overlay.
//!
//! Mirrors `io::open_path_with_phase`: parse, purge, xref, caches, prepare.
//! Files are processed in order in one process, so the first file carries the
//! one-time process warm-up exactly as it does in the app.
//!
//! `OCS_BENCH_COLD=1` skips the startup font warm-up to measure what that
//! warm-up is worth.

use std::path::Path;
use std::time::Instant;

use OpenCADStudio::{io, scene};

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: open_bench <file.dwg|file.dxf> [more files...]");
        std::process::exit(2);
    }

    if std::env::var_os("OCS_BENCH_COLD").is_none() {
        let t = Instant::now();
        let t_lff = Instant::now();
        scene::text::lff::warm();
        let lff_ms = t_lff.elapsed().as_millis();
        let t_sys = Instant::now();
        let _ = scene::text::sysfont::families();
        let sys_ms = t_sys.elapsed().as_millis();
        let t_cosmic = Instant::now();
        scene::text::ttf_glyph::warm_font_system();
        let cosmic_ms = t_cosmic.elapsed().as_millis();
        println!(
            "warm-up {}ms (lff {lff_ms}ms, sysfont {sys_ms}ms, cosmic-text {cosmic_ms}ms)",
            t.elapsed().as_millis()
        );
    } else {
        println!("warm-up skipped (OCS_BENCH_COLD)");
    }

    for path in &paths {
        let path = Path::new(path);
        let name = path.file_name().unwrap_or_default().to_string_lossy();

        let t_total = Instant::now();

        let t_parse = Instant::now();
        let mut doc = match io::load_file(path) {
            Ok(doc) => doc,
            Err(error) => {
                eprintln!("{name}: load failed: {error}");
                continue;
            }
        };
        let parse_ms = t_parse.elapsed().as_millis();

        let t_purge = Instant::now();
        let dropped = io::purge_corrupt_entities(&mut doc);
        let purge_ms = t_purge.elapsed().as_millis();

        let t_xref = Instant::now();
        let xref_count = match path.parent() {
            Some(base_dir) => io::xref::resolve_xrefs_with_progress(&mut doc, base_dir, None).0.len(),
            None => 0,
        };
        let xref_ms = t_xref.elapsed().as_millis();

        let t_caches = Instant::now();
        let caches = scene::build_derived_caches_with_progress(&doc, &|_: u16| {}, path.parent());
        let caches_ms = t_caches.elapsed().as_millis();

        let entities = doc.entity_count();
        let t_prepare = Instant::now();
        let (_doc, _geometry, prepare) = scene::prepare_open_geometry(doc, &caches, [0.0; 4]);
        let prepare_ms = t_prepare.elapsed().as_millis();

        println!(
            "{name} — {entities} entities, {dropped} dropped, {xref_count} xrefs\n  \
             parse {parse_ms}ms · purge {purge_ms}ms · xref {xref_ms}ms · caches {caches_ms}ms · \
             prepare {prepare_ms}ms (wires {}ms, index {}ms) · total {}ms",
            prepare.wires_ms,
            prepare.index_ms,
            t_total.elapsed().as_millis()
        );
    }
}
