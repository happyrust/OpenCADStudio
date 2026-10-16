//! Subsetted TrueType faces, so filled glyph quads draw as real text objects.
//!
//! The emitter tags every filled TrueType glyph with the face its outline came
//! from (`GlyphExport::source`). `FontPlans::collect` turns the faces a
//! document actually uses into embedded subsets — one `ParsedFont` per face,
//! registered on the document — so `PdfSink` can draw those glyphs with
//! `SetFont` / `ShowText` instead of hundreds of tessellated triangles: the
//! page carries extractable text and the content stream shrinks by orders of
//! magnitude on text-heavy sheets. A face that fails to subset is simply
//! absent from the plan set and its glyphs keep the outline path.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use allsorts::binary::read::ReadScope;
use allsorts::font_data::FontData;
use allsorts::subset::{subset, CmapTarget, SubsetProfile};
use allsorts::tables::FontTableProvider;
use printpdf::{FontId, ParsedFont, PdfDocument};

use crate::io::plot_emit::GlyphSnapshot;
use crate::io::plot_types::PdfPageInput;
use crate::scene::text::ttf_glyph::FontBlob;

/// One embedded face: its resource id and the renumbering `FontGlyph` ops need.
pub(crate) struct FontPlan {
    /// `SetFont` handle of the registered subset font.
    pub font: FontId,
    /// Units per em — the `Tf` size that makes one text unit one font unit, so
    /// `FontGlyph::tm` maps font units straight onto the page.
    pub upem: f32,
    /// Original-face gid → subset gid. `allsorts::subset` renumbers glyphs by
    /// their position in the id list (.notdef first), which is what this map
    /// records.
    gids: HashMap<u16, u16>,
}

impl FontPlan {
    /// The subset gid for an original-face gid; `None` only if the usage scan
    /// missed a glyph the emitter later drew.
    pub(crate) fn subset_gid(&self, original: u16) -> Option<u16> {
        self.gids.get(&original).copied()
    }
}

/// The faces a document's text needs, subsetted and registered on `doc`.
pub(crate) struct FontPlans {
    plans: HashMap<(usize, u32, u32), FontPlan>,
}

impl FontPlans {
    /// An empty plan set: every glyph draws as outlines.
    pub(crate) fn none() -> Self {
        Self {
            plans: HashMap::new(),
        }
    }

    /// Scan the pages' glyph quads through `snapshot`, subset each face used by
    /// a filled TrueType glyph, and register the results on `doc`.
    pub(crate) fn collect(
        doc: &mut PdfDocument,
        pages: &[PdfPageInput],
        snapshot: &GlyphSnapshot,
    ) -> Self {
        // Face id → (face bytes, original gid → the character it draws).
        let mut usage: HashMap<(usize, u32, u32), (Arc<FontBlob>, BTreeMap<u16, char>)> =
            HashMap::new();
        let solid = snapshot.solid_key();
        for page in pages {
            for wire in page.content.wires.iter() {
                for quad in wire.wire.text_verts.as_chunks::<6>().0 {
                    // Mirrors `emit_text`: an invisible quad is never looked
                    // up, and the decoration bar's solid texel is no glyph.
                    if quad[0].color[3] < 0.01 {
                        continue;
                    }
                    let uv_min = [quad[5].uv[0], quad[5].uv[1]];
                    if crate::scene::text::sdf_atlas::uv_key(uv_min) == solid {
                        continue;
                    }
                    let Some(ge) = snapshot.glyph_at_uv(uv_min) else {
                        continue;
                    };
                    let Some(src) = ge.source.as_ref() else {
                        continue;
                    };
                    if ge.fill_tris.is_empty() {
                        continue;
                    }
                    usage
                        .entry(src.font.id)
                        .or_insert_with(|| (src.font.clone(), BTreeMap::new()))
                        .1
                        .entry(src.gid)
                        .or_insert(ge.ch);
                }
            }
        }

        let mut plans = HashMap::new();
        for (id, (blob, used)) in usage {
            if let Some((parsed, gids, upem)) = subset_face(&blob, &used) {
                let font = doc.add_font(&parsed);
                plans.insert(
                    id,
                    FontPlan {
                        font,
                        upem: f32::from(upem),
                        gids,
                    },
                );
            }
        }
        Self { plans }
    }

    /// The face ids that made it — what `PlotAssets::embedded_fonts` carries.
    pub(crate) fn faces(&self) -> Option<Arc<HashSet<(usize, u32, u32)>>> {
        (!self.plans.is_empty()).then(|| Arc::new(self.plans.keys().copied().collect()))
    }

    /// The plan for a `FontGlyph`'s `font` key.
    pub(crate) fn get(&self, id: &(usize, u32, u32)) -> Option<&FontPlan> {
        self.plans.get(id)
    }
}

/// Subset one face to the glyphs `used` lists and wrap it for `doc.add_font`.
/// `None` when the face cannot be read or subset.
fn subset_face(
    blob: &FontBlob,
    used: &BTreeMap<u16, char>,
) -> Option<(ParsedFont, HashMap<u16, u16>, u16)> {
    let face = ttf_parser::Face::parse(&blob.data, blob.index).ok()?;
    let font_data = ReadScope::new(&blob.data).read::<FontData>().ok()?;
    let provider = font_data.table_provider(blob.index as usize).ok()?;

    // Subset gids are input positions; .notdef must lead and duplicates are
    // rejected — `used` keys are already sorted and unique.
    let ids: Vec<u16> = std::iter::once(0u16)
        .chain(used.keys().copied().filter(|&gid| gid != 0))
        .collect();
    let bytes = subset(&provider, &ids, &SubsetProfile::Pdf, CmapTarget::Unicode).ok()?;
    let gids: HashMap<u16, u16> = ids
        .iter()
        .enumerate()
        .map(|(new_id, &old_id)| (old_id, new_id as u16))
        .collect();

    let units_per_em = face.units_per_em().max(1);
    let codepoint_to_glyph = used
        .iter()
        .map(|(&gid, &ch)| (ch as u32, gids[&gid]))
        .collect();
    let glyph_widths = used
        .keys()
        .map(|&gid| {
            (
                gids[&gid],
                face.glyph_hor_advance(ttf_parser::GlyphId(gid))
                    .unwrap_or(0),
            )
        })
        .collect();
    let name = face
        .names()
        .into_iter()
        .find(|name| name.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
        .or_else(|| {
            face.names()
                .into_iter()
                .find(|name| name.name_id == ttf_parser::name_id::FAMILY)
        })
        .and_then(|name| name.to_string())
        .map(|name| {
            name.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        });

    let mut parsed = ParsedFont::with_glyph_data(
        bytes,
        0,
        name,
        codepoint_to_glyph,
        glyph_widths,
        units_per_em,
        printpdf::FontMetrics {
            ascent: face.ascender(),
            descent: face.descender(),
        },
    );
    let bbox = face.global_bounding_box();
    parsed.pdf_font_metrics = printpdf::PdfFontMetricsStub {
        units_per_em,
        x_min: bbox.x_min,
        y_min: bbox.y_min,
        x_max: bbox.x_max,
        y_max: bbox.y_max,
    };
    parsed.font_type = if provider.has_table(allsorts::tag::CFF) {
        printpdf::FontType::OpenTypeCFF(())
    } else {
        printpdf::FontType::TrueType
    };
    Some((parsed, gids, units_per_em))
}
