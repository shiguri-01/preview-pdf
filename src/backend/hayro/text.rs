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

pub(super) fn extract_text_with_device(page: &Page<'_>) -> String {
    let cache = InterpreterCache::new();
    let mut context = Context::new(
        page.initial_transform(true).to_kurbo(),
        page.intersected_crop_box().to_kurbo(),
        &cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut device = PlainTextExtractDevice::default();
    interpret_page(page, &mut context, &mut device);
    device.finish()
}

#[derive(Default)]
pub(super) struct PlainTextExtractDevice {
    text: String,
    last_point: Option<Point>,
    last_glyph: Option<(String, i32, i32)>,
}

impl PlainTextExtractDevice {
    pub(super) fn finish(self) -> String {
        self.text
    }

    fn push_char(&mut self, ch: char, x: f64, y: f64) {
        if ch == '\n' || ch == '\r' {
            push_plain_newline(&mut self.text);
            self.last_point = Some(Point::new(x, y));
            return;
        }
        if ch.is_whitespace() {
            push_plain_space(&mut self.text);
            self.last_point = Some(Point::new(x, y));
            return;
        }

        if let Some(last) = self.last_point
            && (y - last.y).abs() > PLAIN_TEXT_LINE_BREAK_THRESHOLD
        {
            push_plain_newline(&mut self.text);
        }

        self.text.push(ch);
        self.last_point = Some(Point::new(x, y));
    }

    pub(super) fn push_glyph_text(&mut self, text: String, x: f64, y: f64) {
        if self.is_duplicate_glyph(&text, x, y) {
            return;
        }

        for ch in text.chars() {
            self.push_char(ch, x, y);
        }
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

impl<'a> Device<'a> for PlainTextExtractDevice {
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
        self.push_glyph_text(bf_string_text(ch), position.x, position.y);
    }

    fn draw_image(&mut self, _image: Image<'a, '_>, _transform: Affine) {}

    fn pop_clip_path(&mut self) {}

    fn pop_transparency_group(&mut self) {}
}

pub(super) fn extract_positioned_text_with_device(page: &Page<'_>) -> TextPage {
    let cache = InterpreterCache::new();
    let mut context = Context::new(
        page.initial_transform(true).to_kurbo(),
        page.intersected_crop_box().to_kurbo(),
        &cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let (width_pt, height_pt) = page.render_dimensions();
    let mut device = PositionedTextExtractDevice::default();
    interpret_page(page, &mut context, &mut device);
    device.finish(width_pt, height_pt)
}

#[derive(Default)]
pub(super) struct PositionedTextExtractDevice {
    last_glyph: Option<(String, i32, i32)>,
    pub(super) glyphs: Vec<TextGlyph>,
    dropped_glyphs: usize,
}

impl PositionedTextExtractDevice {
    pub(super) fn finish(self, width_pt: f32, height_pt: f32) -> TextPage {
        TextPage {
            width_pt,
            height_pt,
            glyphs: self.glyphs,
            dropped_glyphs: self.dropped_glyphs,
        }
    }

    pub(super) fn push_glyph_text(&mut self, text: String, bbox: Option<PdfRect>, x: f64, y: f64) {
        if self.is_duplicate_glyph(&text, x, y) {
            return;
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

impl<'a> Device<'a> for PositionedTextExtractDevice {
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
        if bbox.is_none() {
            self.dropped_glyphs += 1;
        }
        self.push_glyph_text(bf_string_text(ch), bbox, position.x, position.y);
    }

    fn draw_image(&mut self, _image: Image<'a, '_>, _transform: Affine) {}

    fn pop_clip_path(&mut self) {}

    fn pop_transparency_group(&mut self) {}
}

fn quantize_coord(value: f64) -> i32 {
    (value * 100.0).round() as i32
}

const PLAIN_TEXT_LINE_BREAK_THRESHOLD: f64 = 6.0;

fn push_plain_newline(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn push_plain_space(out: &mut String) {
    if !out.ends_with([' ', '\n']) {
        out.push(' ');
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
