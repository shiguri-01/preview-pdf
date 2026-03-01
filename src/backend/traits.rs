use std::path::Path;
use std::sync::Arc;

use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u8]>,
}

impl RgbaFrame {
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }

    pub fn pixels_to_vec(&self) -> Vec<u8> {
        self.pixels.as_ref().to_vec()
    }
}

pub trait PdfBackend: Send {
    fn path(&self) -> &Path;
    fn doc_id(&self) -> u64;
    fn page_count(&self) -> usize;
    fn page_dimensions(&self, page: usize) -> AppResult<(f32, f32)>;
    fn render_page(&self, page: usize, scale: f32) -> AppResult<RgbaFrame>;
    fn extract_text(&self, page: usize) -> AppResult<String>;
}
