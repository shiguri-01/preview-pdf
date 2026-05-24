use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use bytemuck::allocation::cast_vec;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, color::PremulRgba8};
use hayro::{RenderCache, RenderSettings, render};

use crate::backend::{OutlineNode, RgbaFrame, TextPage};
use crate::error::{AppError, AppResult};

use super::PdfDoc;
use super::outline::extract_outline_nodes;
use super::text::{extract_positioned_text_with_device, extract_text_with_device};

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
        let render_cache = RenderCache::new();
        self.render_page_with_cache(page, scale, &render_cache)
    }

    pub(super) fn render_page_with_cache<'a>(
        &'a self,
        page: usize,
        scale: f32,
        render_cache: &RenderCache<'a>,
    ) -> AppResult<RgbaFrame> {
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
        let pixmap = render(
            page_ref,
            render_cache,
            &interpreter_settings,
            &render_settings,
        );

        Ok(RgbaFrame {
            width: pixmap.width() as u32,
            height: pixmap.height() as u32,
            pixels: pixel_buffer_from_pixmap(pixmap).into(),
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

    pub fn extract_positioned_text(&self, page: usize) -> AppResult<TextPage> {
        if page >= self.page_count() {
            return Err(AppError::invalid_argument("page index is out of range"));
        }

        let page_ref = self
            .pdf
            .pages()
            .get(page)
            .ok_or(AppError::invalid_argument("page index is out of range"))?;

        Ok(extract_positioned_text_with_device(page_ref))
    }

    pub fn extract_outline(&self) -> AppResult<Vec<OutlineNode>> {
        extract_outline_nodes(&self.pdf)
    }
}
fn calculate_doc_id(path: &Path, byte_len: usize) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    byte_len.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn pixel_buffer_from_pixmap(pixmap: Pixmap) -> Vec<u8> {
    let width = pixmap.width() as usize;
    let height = pixmap.height() as usize;
    let pixels: Vec<PremulRgba8> = pixmap.take();
    let bytes = cast_vec(pixels);
    debug_assert_eq!(bytes.len(), width * height * 4);
    bytes
}
