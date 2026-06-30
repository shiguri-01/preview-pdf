use hayro::hayro_interpret::font::Glyph;
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::util::{RectExt, TransformExt};
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache,
    InterpreterSettings, Paint, PathDrawMode, SoftMask, interpret_page,
};
use hayro::hayro_syntax::page::Page;
use kurbo::{Affine, BezPath, Point, Shape};

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
    last_glyph: Option<(String, i32, i32)>,
    glyphs: Vec<TextGlyph>,
    dropped_glyphs: usize,
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

    fn push_glyph_text(&mut self, text: String, bbox: Option<PdfRect>, x: f64, y: f64) {
        if self.is_duplicate_glyph(&text, x, y) {
            return;
        }

        if bbox.is_none() {
            self.dropped_glyphs += text.chars().count();
        }
        self.glyphs
            .extend(text.chars().map(|ch| TextGlyph { ch, bbox }));
        self.set_last_glyph(text, x, y);
    }

    fn is_duplicate_glyph(&self, text: &str, x: f64, y: f64) -> bool {
        self.last_glyph
            .as_ref()
            .is_some_and(|(last, last_x, last_y)| {
                last == text && *last_x == quantize_coord(x) && *last_y == quantize_coord(y)
            })
    }

    fn set_last_glyph(&mut self, text: String, x: f64, y: f64) {
        self.last_glyph = Some((text, quantize_coord(x), quantize_coord(y)));
    }
}

impl<'a> Device<'a> for TextPageExtractDevice {
    fn set_soft_mask(&mut self, _mask: Option<SoftMask<'a>>) {}

    fn set_blend_mode(&mut self, _blend_mode: BlendMode) {}

    fn draw_path(
        &mut self,
        _path: &BezPath,
        _transform: Affine,
        _paint: &Paint<'a>,
        _draw_mode: &PathDrawMode,
    ) {
    }

    fn push_clip_path(&mut self, _clip_path: &ClipPath) {}

    fn push_transparency_group(
        &mut self,
        _opacity: f32,
        _mask: Option<SoftMask<'a>>,
        _blend_mode: BlendMode,
    ) {
    }

    fn draw_glyph(
        &mut self,
        glyph: &Glyph<'a>,
        transform: Affine,
        glyph_transform: Affine,
        _paint: &Paint<'a>,
        _draw_mode: &GlyphDrawMode,
    ) {
        let Some(ch) = glyph.as_unicode() else {
            return;
        };

        let position = (transform * glyph_transform) * Point::ORIGIN;
        let bbox = glyph_bbox(glyph, transform, glyph_transform);
        self.push_glyph_text(bf_string_text(ch), bbox, position.x, position.y);
    }

    fn draw_image(&mut self, _image: Image<'a, '_>, _transform: Affine) {}

    fn pop_clip_path(&mut self) {}

    fn pop_transparency_group(&mut self) {}
}

fn quantize_coord(value: f64) -> i32 {
    (value * 100.0).round() as i32
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
    use super::{PdfRect, TextPageExtractDevice};

    #[test]
    fn duplicate_filter_preserves_repeated_chars_in_same_glyph_token() {
        let mut device = TextPageExtractDevice::default();
        let bbox = Some(PdfRect {
            x0: 1.0,
            y0: 2.0,
            x1: 3.0,
            y1: 4.0,
        });

        device.push_glyph_text("ff".to_owned(), bbox, 10.0, 20.0);
        device.push_glyph_text("ff".to_owned(), bbox, 10.0, 20.0);

        let text: String = device.glyphs.iter().map(|glyph| glyph.ch).collect();
        assert_eq!(text, "ff");
        assert_eq!(device.glyphs.len(), 2);
    }

    #[test]
    fn dropped_glyph_count_matches_emitted_unbounded_glyphs() {
        let mut device = TextPageExtractDevice::default();

        device.push_glyph_text("ffi".to_owned(), None, 10.0, 20.0);
        device.push_glyph_text("ffi".to_owned(), None, 10.0, 20.0);

        let page = device.finish(100.0, 100.0);
        assert_eq!(page.glyphs.len(), 3);
        assert_eq!(page.dropped_glyphs, 3);
    }
}
