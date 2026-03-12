use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelBuffer(Arc<Vec<u8>>);

impl PixelBuffer {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn into_vec(self) -> Vec<u8> {
        Arc::try_unwrap(self.0).unwrap_or_else(|shared| (*shared).clone())
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Deref for PixelBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.0.as_slice()
    }
}

impl From<Vec<u8>> for PixelBuffer {
    fn from(value: Vec<u8>) -> Self {
        Self(Arc::new(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: PixelBuffer,
}

impl RgbaFrame {
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }

    pub fn into_pixels_vec(self) -> Vec<u8> {
        self.pixels.into_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::{PixelBuffer, RgbaFrame};

    #[test]
    fn into_pixels_vec_reuses_unique_allocation() {
        let expected = vec![1, 2, 3, 4];
        let frame = RgbaFrame {
            width: 1,
            height: 1,
            pixels: expected.clone().into(),
        };

        assert_eq!(frame.into_pixels_vec(), expected);
    }

    #[test]
    fn pixel_buffer_ptr_eq_tracks_shared_storage() {
        let pixels: PixelBuffer = vec![7; 4].into();
        let cloned = pixels.clone();

        assert!(pixels.ptr_eq(&cloned));
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
