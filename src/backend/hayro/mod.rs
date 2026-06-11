mod document;
mod encoding;
mod outline;
mod text;

use std::path::{Path, PathBuf};

use hayro::RenderCache;
use hayro::hayro_syntax::Pdf;

use crate::error::AppResult;

use super::traits::{OutlineNode, PdfBackend, PdfRenderContext, RgbaFrame, TextPage};

pub struct PdfDoc {
    path: PathBuf,
    doc_id: u64,
    pdf: Pdf,
}

pub type HayroPdfBackend = PdfDoc;

struct HayroRenderContext<'a> {
    doc: &'a PdfDoc,
    render_cache: RenderCache<'a>,
}

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

    fn render_context(&self) -> Box<dyn PdfRenderContext + '_> {
        Box::new(HayroRenderContext {
            doc: self,
            render_cache: RenderCache::new(),
        })
    }

    fn extract_text(&self, page: usize) -> AppResult<String> {
        PdfDoc::extract_text(self, page)
    }

    fn extract_positioned_text(&self, page: usize) -> AppResult<TextPage> {
        PdfDoc::extract_positioned_text(self, page)
    }

    fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
        PdfDoc::extract_outline(self)
    }
}

impl PdfRenderContext for HayroRenderContext<'_> {
    fn render_page(&mut self, page: usize, scale: f32) -> AppResult<RgbaFrame> {
        self.doc
            .render_page_with_cache(page, scale, &self.render_cache)
    }
}
#[cfg(test)]
mod tests {
    use std::fs;

    use hayro::vello_cpu::Pixmap;

    use crate::backend::test_support::{
        build_pdf, build_pdf_from_objects, build_pdf_with_raw_streams, unique_temp_path,
    };
    use crate::error::AppError;

    use crate::backend::{PdfBackend, PdfRect};

    use super::{
        PdfDoc,
        document::pixel_buffer_from_pixmap,
        encoding::decode_pdf_text_string,
        text::{PlainTextExtractDevice, PositionedTextExtractDevice},
    };

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
    fn doc_id_changes_when_same_path_same_size_pdf_content_changes() {
        let file = unique_temp_path("doc_id_same_size.pdf");
        let first = build_pdf(&["alpha"]);
        let second = build_pdf(&["bravo"]);
        assert_eq!(
            first.len(),
            second.len(),
            "fixture PDFs must be the same byte length"
        );

        fs::write(&file, first).expect("first test pdf should be created");
        let first_doc = PdfDoc::open(&file).expect("first pdf should open");
        let first_doc_id = first_doc.doc_id();

        fs::write(&file, second).expect("second test pdf should replace first");
        let second_doc = PdfDoc::open(&file).expect("second pdf should open");

        assert_ne!(first_doc_id, second_doc.doc_id());
        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn doc_id_is_stable_for_same_path_and_content() {
        let file = unique_temp_path("doc_id_stable.pdf");
        fs::write(&file, build_pdf(&["stable"])).expect("test pdf should be created");

        let first = PdfDoc::open(&file).expect("first pdf open should succeed");
        let second = PdfDoc::open(&file).expect("second pdf open should succeed");

        assert_eq!(first.doc_id(), second.doc_id());
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

    #[test]
    fn render_context_renders_multiple_pages_with_reused_cache() {
        let file = unique_temp_path("render_context.pdf");
        fs::write(&file, build_pdf(&["first", "second"])).expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let mut render_context = doc.render_context();
        let first = render_context
            .render_page(0, 1.0)
            .expect("first page should render");
        let second = render_context
            .render_page(1, 1.0)
            .expect("second page should render");

        assert_eq!(
            first.pixels.len(),
            first.width as usize * first.height as usize * 4
        );
        assert_eq!(
            second.pixels.len(),
            second.width as usize * second.height as usize * 4
        );

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn plain_text_duplicate_filter_preserves_repeated_chars_in_same_glyph_token() {
        let mut device = PlainTextExtractDevice::default();

        device.push_glyph_text("ll".to_owned(), 10.0, 20.0);
        device.push_glyph_text("ll".to_owned(), 10.0, 20.0);

        assert_eq!(device.finish(), "ll");
    }

    #[test]
    fn positioned_text_duplicate_filter_preserves_repeated_chars_in_same_glyph_token() {
        let mut device = PositionedTextExtractDevice::default();
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
    fn pixel_buffer_from_pixmap_matches_slice_copy_bytes() {
        let mut pixmap = Pixmap::new(2, 1);
        let expected = {
            let bytes = pixmap.data_as_u8_slice_mut();
            bytes.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
            pixmap.data_as_u8_slice().to_vec()
        };

        assert_eq!(pixel_buffer_from_pixmap(pixmap), expected);
    }

    #[test]
    fn decode_pdf_text_string_decodes_utf16be_bom() {
        let decoded =
            decode_pdf_text_string(&[0xFE, 0xFF, 0x30, 0x42, 0x30, 0x44, 0x30, 0x46, 0x30, 0x48]);
        assert_eq!(decoded, "あいうえ");
    }

    #[test]
    fn decode_pdf_text_string_falls_back_to_utf8_lossy_without_bom() {
        let decoded = decode_pdf_text_string("outline".as_bytes());
        assert_eq!(decoded, "outline");
    }

    #[test]
    fn decode_pdf_text_string_uses_pdfdoc_encoding_without_bom() {
        let decoded = decode_pdf_text_string(&[0x8D, b'A', 0x8E]);
        assert_eq!(decoded, "\u{201C}A\u{201D}");
    }

    #[test]
    fn decode_pdf_text_string_decodes_pdfdoc_encoding_control_byte_0x16() {
        let decoded = decode_pdf_text_string(&[0x16]);
        assert_eq!(decoded, "\u{0016}");
    }

    #[test]
    fn extract_outline_resolves_named_destinations_from_name_tree() {
        let file = unique_temp_path("outline_named_dest.pdf");
        fs::write(&file, build_pdf_with_named_outline()).expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let outline = doc
            .extract_outline()
            .expect("outline extraction should succeed");

        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].title, "Chapter 1");
        assert_eq!(outline[0].page, 0);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn extract_outline_handles_cyclic_named_destination_tree() {
        let file = unique_temp_path("outline_named_dest_cycle.pdf");
        fs::write(&file, build_pdf_with_cyclic_named_outline())
            .expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let outline = doc
            .extract_outline()
            .expect("outline extraction should succeed");

        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].title, "Chapter 1");
        assert_eq!(outline[0].page, 0);

        fs::remove_file(&file).expect("test file should be removed");
    }

    #[test]
    fn extract_outline_handles_named_destination_alias_cycles() {
        let file = unique_temp_path("outline_named_dest_alias_cycle.pdf");
        fs::write(&file, build_pdf_with_cyclic_named_outline_aliases())
            .expect("test file should be created");

        let doc = PdfDoc::open(&file).expect("pdf should open");
        let outline = doc
            .extract_outline()
            .expect("outline extraction should succeed");

        assert!(outline.is_empty());

        fs::remove_file(&file).expect("test file should be removed");
    }

    fn build_pdf_with_named_outline() -> Vec<u8> {
        let objects = vec![
            "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R /Names << /Dests 7 0 R >> >>"
                .to_string(),
            "<< /Type /Pages /Kids [5 0 R] /Count 1 >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            "<< /First 6 0 R /Last 6 0 R /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents 8 0 R >>".to_string(),
            "<< /Title (Chapter 1) /Parent 4 0 R /Dest (chapter-1) >>".to_string(),
            "<< /Names [(chapter-1) [5 0 R /Fit]] >>".to_string(),
            "<< /Length 36 >>\nstream\nBT /F1 14 Tf 36 260 Td (hello) Tj ET\nendstream".to_string(),
        ];

        build_pdf_from_objects(&objects)
    }

    fn build_pdf_with_cyclic_named_outline() -> Vec<u8> {
        let objects = vec![
            "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R /Names << /Dests 7 0 R >> >>"
                .to_string(),
            "<< /Type /Pages /Kids [5 0 R] /Count 1 >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            "<< /First 6 0 R /Last 6 0 R /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents 9 0 R >>".to_string(),
            "<< /Title (Chapter 1) /Parent 4 0 R /Dest (chapter-1) >>".to_string(),
            "<< /Kids [8 0 R] >>".to_string(),
            "<< /Names [(chapter-1) [5 0 R /Fit]] /Kids [7 0 R] >>".to_string(),
            "<< /Length 36 >>\nstream\nBT /F1 14 Tf 36 260 Td (hello) Tj ET\nendstream".to_string(),
        ];

        build_pdf_from_objects(&objects)
    }

    fn build_pdf_with_cyclic_named_outline_aliases() -> Vec<u8> {
        let objects = vec![
            "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R /Names << /Dests 7 0 R >> >>"
                .to_string(),
            "<< /Type /Pages /Kids [5 0 R] /Count 1 >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            "<< /First 6 0 R /Last 6 0 R /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 3 0 R >> >> /Contents 8 0 R >>".to_string(),
            "<< /Title (Loop) /Parent 4 0 R /Dest (A) >>".to_string(),
            "<< /Names [(A) (B) (B) (A)] >>".to_string(),
            "<< /Length 36 >>\nstream\nBT /F1 14 Tf 36 260 Td (hello) Tj ET\nendstream".to_string(),
        ];

        build_pdf_from_objects(&objects)
    }
}
