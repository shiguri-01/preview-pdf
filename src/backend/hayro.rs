use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hayro::hayro_interpret::font::Glyph;
use hayro::hayro_interpret::util::{PageExt, RectExt};
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterSettings, Paint,
    PathDrawMode, SoftMask, interpret_page,
};
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::page::Page;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderSettings, render};
use kurbo::{Affine, BezPath, Point};

use crate::error::{AppError, AppResult};

use super::traits::{PdfBackend, RgbaFrame};

pub struct PdfDoc {
    path: PathBuf,
    doc_id: u64,
    pdf: Pdf,
}

pub type HayroPdfBackend = PdfDoc;

impl PdfBackend for PdfDoc {
    fn path(&self) -> &Path {
        PdfDoc::path(self)
    }

    fn doc_id(&self) -> u64 {
        PdfDoc::doc_id(self)
    }

    fn page_count(&self) -> usize {
        PdfDoc::page_count(self)
    }

    fn page_dimensions(&self, page: usize) -> AppResult<(f32, f32)> {
        PdfDoc::page_render_dimensions(self, page)
    }

    fn render_page(&self, page: usize, scale: f32) -> AppResult<RgbaFrame> {
        PdfDoc::render_page(self, page, scale)
    }

    fn extract_text(&self, page: usize) -> AppResult<String> {
        PdfDoc::extract_text(self, page)
    }
}

impl PdfDoc {
    pub fn open(path: impl AsRef<Path>) -> AppResult<Self> {
        let path = path.as_ref();
        let bytes = Self::load_shared_bytes(path)?;
        Self::open_with_shared_bytes(path, bytes)
    }

    pub fn load_shared_bytes(path: impl AsRef<Path>) -> AppResult<Arc<Vec<u8>>> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(AppError::invalid_argument("pdf path must not be empty"));
        }
        if !path.exists() {
            return Err(AppError::io_with_context(
                std::io::Error::new(std::io::ErrorKind::NotFound, "missing file"),
                format!("pdf file not found: {}", path.display()),
            ));
        }
        if !path.is_file() {
            return Err(AppError::invalid_argument(
                "pdf path must be a regular file",
            ));
        }

        let bytes = Arc::new(std::fs::read(path)?);
        if !bytes.as_slice().starts_with(b"%PDF-") {
            return Err(AppError::invalid_argument(
                "input is not a valid PDF header",
            ));
        }

        Ok(bytes)
    }

    pub fn open_with_shared_bytes(path: impl AsRef<Path>, bytes: Arc<Vec<u8>>) -> AppResult<Self> {
        let path = path.as_ref();
        if !bytes.as_slice().starts_with(b"%PDF-") {
            return Err(AppError::invalid_argument(
                "input is not a valid PDF header",
            ));
        }
        let doc_id = calculate_doc_id(path, bytes.len());
        let pdf = Pdf::new(bytes)
            .map_err(|_| AppError::invalid_argument("failed to parse PDF with hayro"))?;

        Ok(Self {
            path: path.to_path_buf(),
            doc_id,
            pdf,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn doc_id(&self) -> u64 {
        self.doc_id
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    pub fn page_render_dimensions(&self, page: usize) -> AppResult<(f32, f32)> {
        if page >= self.page_count() {
            return Err(AppError::invalid_argument("page index is out of range"));
        }

        let page_ref = self
            .pdf
            .pages()
            .get(page)
            .ok_or(AppError::invalid_argument("page index is out of range"))?;

        Ok(page_ref.render_dimensions())
    }

    pub fn render_page(&self, page: usize, scale: f32) -> AppResult<RgbaFrame> {
        if page >= self.page_count() {
            return Err(AppError::invalid_argument("page index is out of range"));
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err(AppError::invalid_argument(
                "scale must be a positive finite value",
            ));
        }

        let page_ref = self
            .pdf
            .pages()
            .get(page)
            .ok_or(AppError::invalid_argument("page index is out of range"))?;

        let render_settings = RenderSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
            ..Default::default()
        };
        let interpreter_settings = InterpreterSettings::default();
        let pixmap = render(page_ref, &interpreter_settings, &render_settings);

        Ok(RgbaFrame {
            width: pixmap.width() as u32,
            height: pixmap.height() as u32,
            pixels: pixmap.data_as_u8_slice().to_vec().into(),
        })
    }

    pub fn extract_text(&self, page: usize) -> AppResult<String> {
        if page >= self.page_count() {
            return Err(AppError::invalid_argument("page index is out of range"));
        }

        let page_ref = self
            .pdf
            .pages()
            .get(page)
            .ok_or(AppError::invalid_argument("page index is out of range"))?;

        Ok(extract_text_with_device(page_ref).trim().to_owned())
    }
}

fn extract_text_with_device(page: &Page<'_>) -> String {
    let mut context = Context::new(
        page.initial_transform(true),
        page.intersected_crop_box().to_kurbo(),
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut device = TextExtractDevice::default();
    interpret_page(page, &mut context, &mut device);
    device.finish()
}

#[derive(Default)]
struct TextExtractDevice {
    text: String,
    last_point: Option<Point>,
    last_glyph: Option<(char, i32, i32)>,
}

impl TextExtractDevice {
    fn finish(self) -> String {
        self.text
    }

    fn push_char(&mut self, ch: char, x: f64, y: f64) {
        if ch == '\n' || ch == '\r' {
            push_newline(&mut self.text);
            self.last_point = Some(Point::new(x, y));
            return;
        }
        if ch.is_whitespace() {
            push_space(&mut self.text);
            self.last_point = Some(Point::new(x, y));
            return;
        }

        if let Some(last) = self.last_point {
            let y_delta = (y - last.y).abs();
            if y_delta > LINE_BREAK_THRESHOLD {
                push_newline(&mut self.text);
            }
        }

        self.text.push(ch);
        self.last_point = Some(Point::new(x, y));
    }

    fn is_duplicate_glyph(&self, ch: char, x: f64, y: f64) -> bool {
        self.last_glyph == Some((ch, quantize_coord(x), quantize_coord(y)))
    }

    fn set_last_glyph(&mut self, ch: char, x: f64, y: f64) {
        self.last_glyph = Some((ch, quantize_coord(x), quantize_coord(y)));
    }
}

impl<'a> Device<'a> for TextExtractDevice {
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
        if self.is_duplicate_glyph(ch, position.x, position.y) {
            return;
        }

        self.set_last_glyph(ch, position.x, position.y);
        self.push_char(ch, position.x, position.y);
    }

    fn draw_image(&mut self, _image: Image<'a, '_>, _transform: Affine) {}

    fn pop_clip_path(&mut self) {}

    fn pop_transparency_group(&mut self) {}
}

fn quantize_coord(value: f64) -> i32 {
    (value * 100.0).round() as i32
}

const LINE_BREAK_THRESHOLD: f64 = 6.0;

fn push_newline(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn push_space(out: &mut String) {
    if !out.ends_with([' ', '\n']) {
        out.push(' ');
    }
}

fn calculate_doc_id(path: &Path, byte_len: usize) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    byte_len.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::error::AppError;

    use super::PdfDoc;

    fn unique_temp_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();

        let mut path = std::env::temp_dir();
        path.push(format!("pvf_{suffix}_{}_{}", process::id(), nanos));
        path
    }

    #[test]
    fn open_rejects_directory_path() {
        let dir = unique_temp_path("dir");
        fs::create_dir_all(&dir).expect("test directory should be created");

        let result = PdfDoc::open(&dir);
        assert!(matches!(
            result,
            Err(AppError::InvalidArgument(message))
                if message == "pdf path must be a regular file"
        ));

        fs::remove_dir_all(&dir).expect("test directory should be removed");
    }

    #[test]
    fn open_accepts_valid_pdf_with_page_count() {
        let file = unique_temp_path("file.pdf");
        fs::write(&file, build_pdf(&["first page", "second page"]))
            .expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("regular file path should be accepted");
        assert_eq!(doc.path(), file.as_path());
        assert_eq!(doc.page_count(), 2);
        assert_ne!(doc.doc_id(), 0);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn render_page_rejects_out_of_range_page() {
        let file = unique_temp_path("render.pdf");
        fs::write(&file, build_pdf(&["hello"])).expect("test file should be created");
        let doc = PdfDoc::open(&file).expect("pdf should open");

        let err = doc.render_page(8, 1.0).expect_err("page should be invalid");
        assert!(matches!(
            err,
            AppError::InvalidArgument(message) if message == "page index is out of range"
        ));

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn page_render_dimensions_read_page_size() {
        let file = unique_temp_path("dimensions.pdf");
        fs::write(&file, build_pdf(&["hello"])).expect("test file should be created");
        let doc = PdfDoc::open(&file).expect("pdf should open");

        let (width, height) = doc
            .page_render_dimensions(0)
            .expect("dimensions should be available");
        assert!((width - 300.0).abs() < f32::EPSILON);
        assert!((height - 300.0).abs() < f32::EPSILON);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn extract_text_returns_page_bucket_text() {
        let file = unique_temp_path("text.pdf");
        fs::write(&file, build_pdf(&["hello world", "second page"]))
            .expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let text = doc.extract_text(0).expect("extract should succeed");
        let normalized: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
        assert!(normalized.contains("helloworld"));

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn extract_text_does_not_insert_false_space_from_tj_position_gap() {
        let file = unique_temp_path("tj_gap.pdf");
        fs::write(
            &file,
            build_pdf_with_raw_streams(&["BT /F1 14 Tf 36 260 Td [(hello) -220 (world)] TJ ET"]),
        )
        .expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let text = doc.extract_text(0).expect("extract should succeed");
        let normalized: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
        assert!(
            normalized.to_lowercase().contains("helloworld"),
            "expected stable extraction without false splits, got: {text:?}"
        );

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn render_page_uses_hayro_pixmap_output() {
        let file = unique_temp_path("pixmap.pdf");
        fs::write(&file, build_pdf(&["render me"])).expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let frame = doc.render_page(0, 1.0).expect("render should succeed");
        assert!(frame.width > 0);
        assert!(frame.height > 0);
        assert_eq!(
            frame.pixels.len(),
            frame.width as usize * frame.height as usize * 4
        );

        fs::remove_file(&file).expect("test file should be removed");
    }

    fn build_pdf(page_texts: &[&str]) -> Vec<u8> {
        let page_texts = if page_texts.is_empty() {
            vec!["".to_string()]
        } else {
            page_texts
                .iter()
                .map(|text| {
                    let escaped = escape_literal_string(text);
                    format!("BT /F1 14 Tf 36 260 Td ({escaped}) Tj ET")
                })
                .collect()
        };

        build_pdf_from_streams(&page_texts)
    }

    fn build_pdf_with_raw_streams(page_streams: &[&str]) -> Vec<u8> {
        let page_streams = if page_streams.is_empty() {
            vec!["".to_string()]
        } else {
            page_streams
                .iter()
                .map(|stream| (*stream).to_string())
                .collect()
        };

        build_pdf_from_streams(&page_streams)
    }

    fn build_pdf_from_streams(page_streams: &[String]) -> Vec<u8> {
        let page_count = page_streams.len();
        let page_ids: Vec<usize> = (0..page_count).map(|i| 4 + i * 2).collect();

        let mut objects = Vec::new();
        objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());

        let kids = page_ids
            .iter()
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        objects.push(format!(
            "<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"
        ));
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());

        for (index, stream) in page_streams.iter().enumerate() {
            let content_id = 5 + index * 2;

            let page_obj = format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>"
            );
            let content_obj = format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                stream.len(),
                stream
            );

            objects.push(page_obj);
            objects.push(content_obj);
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

        let mut offsets = Vec::new();
        offsets.push(0_usize);
        for (index, object) in objects.iter().enumerate() {
            let object_id = index + 1;
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{object_id} 0 obj\n{object}\nendobj\n").as_bytes());
        }

        let xref_start = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        bytes.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }

        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                xref_start
            )
            .as_bytes(),
        );

        bytes
    }

    fn escape_literal_string(text: &str) -> String {
        let mut out = String::with_capacity(text.len());

        for ch in text.chars() {
            match ch {
                '\\' => out.push_str("\\\\"),
                '(' => out.push_str("\\("),
                ')' => out.push_str("\\)"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                _ => out.push(ch),
            }
        }

        out
    }
}
