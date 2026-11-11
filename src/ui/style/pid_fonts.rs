//! The two type families of the P&ID interface, carried in the program.
//!
//! The design names two families — `font-ui` (Inter) and `font-mono`
//! (JetBrains Mono), the two [`TextToken`]s — and Windows comes with neither.
//! Without them every label of the P&ID interface is set in whatever face the
//! font system falls back to, and its widths are not the design's. The
//! weights the design uses are in `assets/pid-fonts` (its `README.md` says
//! where each file comes from) and are compiled in here: [`FACES`].
//!
//! [`ensure_loaded`] puts them in the interface's font database. The P&ID
//! shell calls it as it comes up, so a session that stays in the general
//! interface (`--classic`) never loads them and draws what it drew before.
//! Text in a drawing is shaped with a font system of its own
//! (`scene::text::ttf_glyph`) and never sees them.
//!
//! A screen takes its fonts from [`ui`] and [`mono`]:
//!
//! ```ignore
//! text("PID-1101").font(pid_fonts::mono(theme, font::Weight::Semibold))
//! ```
//!
//! Neither family has Chinese: those glyphs still come from the system's
//! fallback for the script, as they did.

use iced::{font, Font};

use super::pid_tokens::{PidTheme, TextToken};

/// One face carried in the program.
pub struct Face {
    /// The family it belongs to, as its own name table spells it.
    pub family: &'static str,
    /// Its weight (`usWeightClass`).
    pub weight: u16,
    /// Its file in `assets/pid-fonts`.
    pub file: &'static str,
    /// The file.
    pub bytes: &'static [u8],
}

macro_rules! face {
    ($family:literal, $weight:literal, $file:literal) => {
        Face {
            family: $family,
            weight: $weight,
            file: $file,
            bytes: include_bytes!(concat!("../../../assets/pid-fonts/", $file)),
        }
    };
}

/// The faces carried: each weight the design sets text in, and no other.
/// (A `static`: a `const` is instantiated where it is used, and the program
/// then carries the font files once for each use.)
pub static FACES: [Face; 6] = [
    face!("Inter", 400, "Inter-Regular.ttf"),
    face!("Inter", 500, "Inter-Medium.ttf"),
    face!("Inter", 600, "Inter-SemiBold.ttf"),
    face!("JetBrains Mono", 400, "JetBrainsMono-Regular.ttf"),
    face!("JetBrains Mono", 600, "JetBrainsMono-SemiBold.ttf"),
    face!("JetBrains Mono", 700, "JetBrainsMono-Bold.ttf"),
];

/// The interface font of the design (`font-ui`) at `weight`.
pub fn ui(theme: PidTheme, weight: font::Weight) -> Font {
    named(TextToken::FontUi, theme, weight)
}

/// The fixed-pitch font of the design (`font-mono`) at `weight`.
pub fn mono(theme: PidTheme, weight: font::Weight) -> Font {
    named(TextToken::FontMono, theme, weight)
}

fn named(token: TextToken, theme: PidTheme, weight: font::Weight) -> Font {
    Font {
        family: font::Family::Name(token.value(theme)),
        weight,
        ..Font::DEFAULT
    }
}

#[cfg(test)]
thread_local! {
    static REQUESTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times this thread — one test — has asked for the faces.
#[cfg(test)]
pub(crate) fn requests() -> usize {
    REQUESTS.with(std::cell::Cell::get)
}

/// Put [`FACES`] in the interface's font database; the first call does it,
/// the others return at once.
pub fn ensure_loaded() {
    #[cfg(test)]
    REQUESTS.with(|requests| requests.set(requests.get() + 1));
    static LOADED: std::sync::Once = std::sync::Once::new();
    LOADED.call_once(load);
}

fn load() {
    use iced::advanced::graphics::text::{cosmic_text::fontdb, font_system};

    let mut fonts = font_system()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A machine may have a copy of a family installed. A face of the same
    // weight would be matched ahead of the one carried here — the database
    // keeps faces in the order they came, the system's first — and it may be
    // another version with other widths. The interface is set in the faces it
    // was designed with wherever it runs, so such a face stands aside; the
    // installed family's other weights and its italics stay.
    let db = fonts.raw().db_mut();
    let stood_for: Vec<fontdb::ID> = db
        .faces()
        .filter(|installed| {
            installed.style == fontdb::Style::Normal
                && installed.stretch == fontdb::Stretch::Normal
                && FACES.iter().any(|face| {
                    installed.weight.0 == face.weight
                        && installed
                            .families
                            .iter()
                            .any(|(name, _)| name == face.family)
                })
        })
        .map(|installed| installed.id)
        .collect();
    if !stood_for.is_empty() {
        log::info!(
            "{} installed face(s) of the P&ID interface's families stand aside for the ones carried in the program",
            stood_for.len()
        );
    }
    for id in stood_for {
        db.remove_face(id);
    }
    for face in &FACES {
        fonts.load_font(std::borrow::Cow::Borrowed(face.bytes));
    }
}

/// The carried face each glyph of `sample` comes from when it is set in
/// `font`, by file name; `None` for a glyph the font system took from any
/// other face.
#[cfg(test)]
pub(crate) fn faces_setting(font: Font, sample: &str) -> Vec<Option<&'static str>> {
    use iced::advanced::graphics::text::{self, cosmic_text};

    let mut fonts = text::font_system()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut buffer = cosmic_text::Buffer::new(fonts.raw(), cosmic_text::Metrics::new(14.0, 18.0));
    buffer.set_size(None, None);
    buffer.set_text(
        sample,
        &text::to_attributes(font),
        cosmic_text::Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(fonts.raw(), false);
    let db = fonts.raw().db();
    buffer
        .layout_runs()
        .flat_map(|run| run.glyphs.iter())
        .map(|glyph| {
            db.with_face_data(glyph.font_id, |data, _| {
                FACES
                    .iter()
                    .find(|face| face.bytes == data)
                    .map(|face| face.file)
            })
            .flatten()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use iced::advanced::graphics::text::cosmic_text::fontdb;
    use serde_json::Value;
    use sha2::{Digest, Sha256};

    use super::*;

    const FOLDER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/pid-fonts");
    const README: &str = include_str!("../../../assets/pid-fonts/README.md");
    const PEN: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/docs/pid-platform/design/pid-platform.pen"
    );

    fn sha256(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    /// The row of the README's table about `file`.
    fn row(file: &str) -> &'static str {
        let cell = format!("| `{file}` |");
        let mut rows = README.lines().filter(|line| line.starts_with(&cell));
        let row = rows
            .next()
            .unwrap_or_else(|| panic!("the README has a row for {file}"));
        assert!(rows.next().is_none(), "one row for {file}");
        row
    }

    /// The README states the files that are there: every font file of the
    /// folder is carried, every carried face is a file of the folder, and
    /// each row gives the size and the SHA-256 of the bytes compiled in.
    #[test]
    fn the_readme_states_the_files_that_are_carried() {
        let in_folder: BTreeSet<String> = std::fs::read_dir(FOLDER)
            .expect("assets/pid-fonts is there")
            .map(|entry| entry.expect("an entry").file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".ttf") || name.ends_with(".otf"))
            .collect();
        let carried: BTreeSet<String> = FACES.iter().map(|face| face.file.to_owned()).collect();
        assert_eq!(in_folder, carried);

        for face in &FACES {
            let row = row(face.file);
            let on_disk = std::fs::read(format!("{FOLDER}/{}", face.file)).expect("the file");
            assert_eq!(on_disk, face.bytes, "{}", face.file);
            for said in [
                format!("| {} |", face.family),
                format!("| {} |", face.weight),
                format!("| {} |", face.bytes.len()),
                format!("| `{}` |", sha256(face.bytes)),
            ] {
                assert!(row.contains(&said), "{}: {said} in {row}", face.file);
            }
        }
        // The licence of each family goes with it.
        for text in [
            "Inter-LICENSE.txt",
            "JetBrainsMono-OFL.txt",
            "JetBrainsMono-AUTHORS.txt",
        ] {
            let bytes = std::fs::read(format!("{FOLDER}/{text}"))
                .unwrap_or_else(|_| panic!("{text} is in the folder"));
            let row = row(text);
            assert!(
                row.contains(&format!("| `{}` |", sha256(&bytes))),
                "{text}: {row}"
            );
        }
        for licence in ["Inter-LICENSE.txt", "JetBrainsMono-OFL.txt"] {
            let text = std::fs::read_to_string(format!("{FOLDER}/{licence}")).expect("the text");
            assert!(
                text.contains("SIL OPEN FONT LICENSE Version 1.1"),
                "{licence}"
            );
        }
    }

    /// Each file is the family and the weight it is listed as — read from
    /// the file's own tables, not from its name — upright and of normal
    /// width, and the fixed-pitch family is fixed-pitch.
    #[test]
    fn each_face_is_the_family_and_weight_it_is_listed_as() {
        for face in &FACES {
            let mut db = fontdb::Database::new();
            db.load_font_data(face.bytes.to_vec());
            let inside: Vec<_> = db.faces().collect();
            assert_eq!(inside.len(), 1, "{}: one face in the file", face.file);
            let info = inside[0];
            let families: Vec<&str> = info.families.iter().map(|(name, _)| &**name).collect();
            assert_eq!(families, [face.family], "{}", face.file);
            assert_eq!(info.weight.0, face.weight, "{}", face.file);
            assert_eq!(info.style, fontdb::Style::Normal, "{}", face.file);
            assert_eq!(info.stretch, fontdb::Stretch::Normal, "{}", face.file);
            assert_eq!(
                info.monospaced,
                face.family == TextToken::FontMono.value(PidTheme::Dark),
                "{}",
                face.file
            );
        }
    }

    /// The font names of the theme are the families carried, in both themes.
    #[test]
    fn the_font_tokens_name_the_families_carried() {
        let carried: BTreeSet<&str> = FACES.iter().map(|face| face.family).collect();
        for theme in [PidTheme::Dark, PidTheme::Light] {
            let named: BTreeSet<&str> = TextToken::ALL
                .iter()
                .map(|token| token.value(theme))
                .collect();
            assert_eq!(named, carried, "{theme:?}");
        }
    }

    /// Every `(font variable, weight)` the design sets text in, read from
    /// the design file itself.
    fn set_in_the_design() -> BTreeSet<(String, u16)> {
        fn walk(node: &Value, into: &mut BTreeSet<(String, u16)>) {
            match node {
                Value::Object(fields) => {
                    if let Some(family) = fields.get("fontFamily") {
                        let family = family.as_str().expect("a font family is a string");
                        let variable = family.strip_prefix('$').unwrap_or_else(|| {
                            panic!("the design names its fonts by variable, not {family:?}")
                        });
                        let style = fields.get("fontStyle").and_then(Value::as_str);
                        assert!(
                            matches!(style, None | Some("normal")),
                            "no carried face is {style:?}: {fields:?}"
                        );
                        let weight = match fields.get("fontWeight") {
                            None => 400,
                            Some(Value::Number(weight)) => {
                                weight.as_u64().expect("a whole weight") as u16
                            }
                            Some(Value::String(weight)) => match weight.as_str() {
                                "normal" => 400,
                                "bold" => 700,
                                number => number
                                    .parse()
                                    .unwrap_or_else(|_| panic!("a weight: {number:?}")),
                            },
                            Some(other) => panic!("a weight: {other:?}"),
                        };
                        into.insert((variable.to_owned(), weight));
                    }
                    fields.values().for_each(|field| walk(field, into));
                }
                Value::Array(items) => items.iter().for_each(|item| walk(item, into)),
                _ => {}
            }
        }
        let design: Value =
            serde_json::from_str(&std::fs::read_to_string(PEN).expect("the design file"))
                .expect("the design file is JSON");
        let mut found = BTreeSet::new();
        walk(&design, &mut found);
        found
    }

    /// The faces carried are the weights the design sets text in: none
    /// missing, none it does not use.
    #[test]
    fn the_faces_are_the_weights_the_design_uses() {
        let used: BTreeSet<(String, u16)> = set_in_the_design()
            .into_iter()
            .map(|(variable, weight)| {
                let token = TextToken::ALL
                    .iter()
                    .find(|token| token.name() == variable)
                    .unwrap_or_else(|| panic!("{variable} is a font variable of the theme"));
                (token.value(PidTheme::Dark).to_owned(), weight)
            })
            .collect();
        let carried: BTreeSet<(String, u16)> = FACES
            .iter()
            .map(|face| (face.family.to_owned(), face.weight))
            .collect();
        assert_eq!(carried, used);
    }
}
