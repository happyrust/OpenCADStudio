pub mod lff;
pub mod font_face;
pub mod glyph_quads;
pub mod sdf_atlas;
pub mod sysfont;
pub mod ttf_glyph;
pub mod web_font;
pub mod complex_lt;
pub mod shx;

/// Load the process-wide font caches on a background thread at startup.
///
/// These are `OnceLock`s that the text tessellation path initialises on first
/// use, which means the first drawing containing text pays for scanning the
/// installed system fonts — measured at ~290ms of a ~340ms open on Windows,
/// while every subsequent file opens in ~50ms. Doing it while the window and
/// GPU are still coming up takes that cost off the first open entirely.
#[cfg(not(target_arch = "wasm32"))]
pub fn warm_up_fonts() {
    let _ = std::thread::Builder::new()
        .name("ocs-font-warmup".to_string())
        .spawn(|| {
            let t_lff = std::time::Instant::now();
            lff::warm();
            let lff_ms = t_lff.elapsed().as_millis();
            let t_sys = std::time::Instant::now();
            let _ = sysfont::families();
            let sys_ms = t_sys.elapsed().as_millis();
            let t_cosmic = std::time::Instant::now();
            ttf_glyph::warm_font_system();
            if std::env::var_os("RUST_LOG").is_some() {
                eprintln!(
                    "font warm-up: lff {lff_ms}ms, sysfont {sys_ms}ms, cosmic-text {}ms",
                    t_cosmic.elapsed().as_millis()
                );
            }
        });
}
