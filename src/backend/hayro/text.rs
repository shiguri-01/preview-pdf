use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::util::{RectExt, TransformExt};
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, SoftMask, interpret_page,
};
use hayro::hayro_syntax::page::Page;
use kurbo::{Affine, BezPath, Shape};

use crate::backend::{PdfRect, TextGlyph, TextPage};

pub(super) fn extract_text_page_with_device(page: &Page<'_>) -> TextPage {
    let cache = InterpreterCache::new();
    let mut context = Context::new(
        page.initial_transform(true).to_kurbo(),
        page.intersected_crop_box().to_kurbo(),
        &cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let (width_pt, height_pt) = page.render_dimensions();
    let mut device = TextPageExtractDevice::default();
    interpret_page(page, &mut context, &mut device);
    device.finish(width_pt, height_pt)
}

#[derive(Default)]
struct TextPageExtractDevice {
    pending_fill_run: Option<GlyphRunSignature>,
    glyphs: Vec<TextGlyph>,
    dropped_glyphs: usize,
}

#[derive(PartialEq)]
struct GlyphRunSignature {
    transform: Affine,
    glyphs: Vec<GlyphSignature>,
}

#[derive(PartialEq)]
struct GlyphSignature {
    char_code: u32,
    text: Option<String>,
    transform: Affine,
    bbox: Option<PdfRect>,
}

impl TextPageExtractDevice {
    fn finish(self, width_pt: f32, height_pt: f32) -> TextPage {
        TextPage {
            width_pt,
            height_pt,
            glyphs: self.glyphs,
            dropped_glyphs: self.dropped_glyphs,
        }
    }

    fn push_glyph_text(&mut self, text: &str, bbox: Option<PdfRect>) {
        if bbox.is_none() {
            self.dropped_glyphs += text.chars().count();
        }
        self.glyphs
            .extend(text.chars().map(|ch| TextGlyph { ch, bbox }));
    }
}

impl<'a> Device<'a> for TextPageExtractDevice {
    fn draw_path(&mut self, _path: &BezPath, _props: DrawProps<'a>, _draw_mode: &DrawMode) {
        self.pending_fill_run = None;
    }

    fn push_clip_path(&mut self, _clip_path: &ClipPath) {
        self.pending_fill_run = None;
    }

    fn push_transparency_group(
        &mut self,
        _opacity: f32,
        _mask: Option<SoftMask<'a>>,
        _blend_mode: BlendMode,
    ) {
        self.pending_fill_run = None;
    }

    fn draw_glyph_run(
        &mut self,
        glyph_run: &GlyphRun<'_, 'a>,
        props: DrawProps<'a>,
        draw_mode: &DrawMode,
    ) {
        let run = GlyphRunSignature {
            transform: props.transform,
            glyphs: glyph_run
                .glyphs()
                .iter()
                .map(|glyph| GlyphSignature {
                    char_code: glyph.char_code(),
                    text: glyph.as_unicode().map(bf_string_text),
                    transform: glyph.transform(),
                    bbox: glyph_bbox(glyph, props.transform, glyph.transform()),
                })
                .collect(),
        };
        // Hayro paints fill-and-stroke text with adjacent callbacks for the same run.
        // Consume only an immediately preceding matching fill, not repeated text in general.
        // The API cannot distinguish an independent, exactly overlaid fill -> stroke pair,
        // even if intervening PDF operations occur without emitting device callbacks.
        let previous_fill = self.pending_fill_run.take();
        if matches!(draw_mode, DrawMode::Stroke(_)) && previous_fill.as_ref() == Some(&run) {
            return;
        }

        for glyph in &run.glyphs {
            if let Some(text) = &glyph.text {
                self.push_glyph_text(text, glyph.bbox);
            }
        }
        if matches!(draw_mode, DrawMode::Fill(_)) {
            self.pending_fill_run = Some(run);
        }
    }

    fn draw_image(&mut self, _image: Image<'a, '_>, _props: ImageDrawProps<'a>) {
        self.pending_fill_run = None;
    }

    fn pop_clip(&mut self) {
        self.pending_fill_run = None;
    }

    fn pop_transparency_group(&mut self) {
        self.pending_fill_run = None;
    }
}

fn glyph_bbox(glyph: &Glyph<'_>, transform: Affine, glyph_transform: Affine) -> Option<PdfRect> {
    let outline = match glyph {
        Glyph::Outline(outline) => outline.outline(),
        Glyph::Type3(_) => return None,
    };
    let bbox = (transform * glyph_transform * outline).bounding_box();
    if bbox.is_zero_area() {
        return None;
    }

    Some(PdfRect {
        x0: bbox.x0 as f32,
        y0: bbox.y0 as f32,
        x1: bbox.x1 as f32,
        y1: bbox.y1 as f32,
    })
}

fn bf_string_text(value: BfString) -> String {
    match value {
        BfString::Char(ch) => ch.to_string(),
        BfString::String(text) => text,
    }
}

#[cfg(test)]
mod tests {
    use hayro::hayro_syntax::Pdf;

    use crate::backend::TextPage;
    use crate::backend::test_support::{build_pdf_from_objects, build_pdf_with_raw_streams};

    use super::extract_text_page_with_device;

    fn extract_stream(stream: &str) -> TextPage {
        extract_pdf(build_pdf_with_raw_streams(&[stream]))
    }

    fn extract_pdf(bytes: Vec<u8>) -> TextPage {
        let pdf = Pdf::new(bytes).expect("fixture PDF should parse");
        extract_text_page_with_device(&pdf.pages()[0])
    }

    fn assert_single_text_run_with_original_bounds(mode: u8) {
        let fill = extract_stream("BT /F1 14 Tf 36 260 Td (hello) Tj ET");
        assert_eq!(fill.plain_text(), "hello");

        let page = extract_stream(&format!("BT /F1 14 Tf {mode} Tr 36 260 Td (hello) Tj ET"));
        assert_eq!(page.plain_text(), "hello", "rendering mode {mode}");
        assert_eq!(page.glyphs, fill.glyphs, "rendering mode {mode}");
        assert_eq!(page.dropped_glyphs, fill.dropped_glyphs);
    }

    #[test]
    fn fill_and_stroke_text_is_extracted_once_with_original_bounds() {
        assert_single_text_run_with_original_bounds(2);
    }

    #[test]
    fn clipped_fill_and_stroke_text_is_extracted_once_with_original_bounds() {
        assert_single_text_run_with_original_bounds(6);
    }

    #[test]
    fn text_is_preserved_in_each_rendering_mode() {
        for mode in 0..=7 {
            let page = extract_stream(&format!(
                "BT /F1 14 Tf {mode} Tr 36 260 Td (hello ff) Tj ET"
            ));
            assert_eq!(page.plain_text(), "hello ff", "rendering mode {mode}");
        }
    }

    #[test]
    fn repeated_text_runs_at_the_same_position_are_preserved() {
        for mode in [0, 1, 2, 3, 6] {
            for text in ["l", "hello"] {
                let page = extract_stream(&format!(
                    "BT /F1 14 Tf {mode} Tr 1 0 0 1 36 260 Tm ({text}) Tj \
                     1 0 0 1 36 260 Tm ({text}) Tj ET"
                ));
                assert_eq!(page.plain_text(), text.repeat(2), "rendering mode {mode}");
            }
        }
    }

    #[test]
    fn independently_positioned_fill_and_stroke_text_is_preserved() {
        for stream in [
            "BT /F1 14 Tf 36 260 Td (hello) Tj 1 Tr (hello) Tj ET",
            "BT /F1 14 Tf 36 260 Td (hello) Tj 1 Tr 1 0 0 1 36.001 260 Tm (hello) Tj ET",
            "BT /F1 14 Tf 36 260 Td (hello) Tj ET 1 0 0 1 0.001 0 cm \
             BT /F1 14 Tf 1 Tr 36 260 Td (hello) Tj ET",
        ] {
            let page = extract_stream(stream);
            assert_eq!(page.plain_text(), "hellohello");
            assert_ne!(page.glyphs[0].bbox, page.glyphs[5].bbox);
        }
    }

    #[test]
    fn stroke_text_after_a_fill_and_stroke_pair_is_preserved() {
        let page = extract_stream(
            "BT /F1 14 Tf 2 Tr 36 260 Td (hello) Tj \
             1 Tr 1 0 0 1 36 260 Tm (hello) Tj ET",
        );
        assert_eq!(page.plain_text(), "hellohello");
    }

    #[test]
    fn intervening_paint_or_clip_keeps_independent_fill_and_stroke_text() {
        for drawing in ["0 0 m 10 10 l S", "q 0 0 100 100 re W n Q"] {
            let page = extract_stream(&format!(
                "BT /F1 14 Tf 36 260 Td (hello) Tj ET {drawing} \
                 BT /F1 14 Tf 1 Tr 36 260 Td (hello) Tj ET"
            ));
            assert_eq!(page.plain_text(), "hellohello");
        }
    }

    #[test]
    fn fill_and_stroke_preserves_ligatures_and_repeated_characters() {
        let cmap = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap \
                    /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def \
                    /CMapName /Test def /CMapType 2 def \
                    1 begincodespacerange <00> <FF> endcodespacerange \
                    1 beginbfchar <41> <006600660069> endbfchar \
                    endcmap CMapName currentdict /CMap defineresource pop end end";
        for mode in [0, 1, 2, 3, 6] {
            let stream = format!("BT /F1 14 Tf {mode} Tr 36 260 Td (AA) Tj ET");
            let page = extract_pdf(build_pdf_from_objects(&[
                "<< /Type /Catalog /Pages 2 0 R >>".into(),
                "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".into(),
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>".into(),
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] \
                 /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>"
                    .into(),
                format!(
                    "<< /Length {} >>\nstream\n{stream}\nendstream",
                    stream.len()
                ),
                format!("<< /Length {} >>\nstream\n{cmap}\nendstream", cmap.len()),
            ]));
            assert_eq!(page.plain_text(), "ffiffi", "rendering mode {mode}");
            assert_eq!(page.glyphs.len(), 6);
        }
    }

    #[test]
    fn fill_and_stroke_counts_unbounded_glyphs_once() {
        let char_proc = "500 0 d0";
        for mode in [0, 1, 2, 3, 6] {
            let stream = format!("BT /F1 14 Tf {mode} Tr 36 260 Td (AA) Tj ET");
            let page = extract_pdf(build_pdf_from_objects(&[
                "<< /Type /Catalog /Pages 2 0 R >>".into(),
                "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".into(),
                "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 500 700] \
                 /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /A 6 0 R >> \
                 /Encoding << /Type /Encoding /Differences [65 /A] >> \
                 /FirstChar 65 /LastChar 65 /Widths [500] /Resources << >> >>"
                    .into(),
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] \
                 /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>"
                    .into(),
                format!(
                    "<< /Length {} >>\nstream\n{stream}\nendstream",
                    stream.len()
                ),
                format!(
                    "<< /Length {} >>\nstream\n{char_proc}\nendstream",
                    char_proc.len()
                ),
            ]));
            assert_eq!(page.plain_text(), "AA", "rendering mode {mode}");
            assert!(page.glyphs.iter().all(|glyph| glyph.bbox.is_none()));
            assert_eq!(page.dropped_glyphs, 2, "rendering mode {mode}");
        }
    }
}
