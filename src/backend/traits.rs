use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::error::AppResult;

#[derive(Debug, Default)]
pub struct PixelBufferPool {
    recycled: Mutex<Vec<Vec<u8>>>,
}

impl PixelBufferPool {
    pub fn take(&self, len: usize) -> Vec<u8> {
        let mut recycled = self.recycled.lock().expect("pixel buffer pool lock");
        let Some(index) = recycled.iter().position(|buffer| buffer.capacity() >= len) else {
            return vec![0; len];
        };
        let mut buffer = recycled.swap_remove(index);
        buffer.resize(len, 0);
        buffer
    }

    fn give_back(&self, mut bytes: Vec<u8>) {
        bytes.clear();
        let mut recycled = self.recycled.lock().expect("pixel buffer pool lock");
        recycled.push(bytes);
    }

    #[cfg(test)]
    fn available(&self) -> usize {
        self.recycled.lock().expect("pixel buffer pool lock").len()
    }
}

#[derive(Debug)]
struct PixelStorage {
    bytes: Vec<u8>,
    recycle: Option<&'static PixelBufferPool>,
}

impl Drop for PixelStorage {
    fn drop(&mut self) {
        let Some(pool) = self.recycle.take() else {
            return;
        };
        let bytes = std::mem::take(&mut self.bytes);
        pool.give_back(bytes);
    }
}

#[derive(Debug, Clone)]
pub struct PixelBuffer(Arc<PixelStorage>);

impl PixelBuffer {
    pub fn len(&self) -> usize {
        self.0.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.bytes.is_empty()
    }

    pub fn into_vec(self) -> Vec<u8> {
        match Arc::try_unwrap(self.0) {
            Ok(mut storage) if storage.recycle.is_none() => std::mem::take(&mut storage.bytes),
            Ok(storage) => storage.bytes.clone(),
            Err(shared) => shared.bytes.clone(),
        }
    }

    pub fn with_mut_bytes<T>(self, f: impl FnOnce(&mut [u8]) -> T) -> T {
        match Arc::try_unwrap(self.0) {
            Ok(mut storage) => f(storage.bytes.as_mut_slice()),
            Err(shared) => {
                let mut bytes = shared.bytes.clone();
                f(bytes.as_mut_slice())
            }
        }
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn from_pooled_vec(bytes: Vec<u8>, pool: &'static PixelBufferPool) -> Self {
        Self(Arc::new(PixelStorage {
            bytes,
            recycle: Some(pool),
        }))
    }
}

impl PartialEq for PixelBuffer {
    fn eq(&self, other: &Self) -> bool {
        self[..] == other[..]
    }
}

impl Eq for PixelBuffer {}

impl Deref for PixelBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.0.bytes.as_slice()
    }
}

impl From<Vec<u8>> for PixelBuffer {
    fn from(value: Vec<u8>) -> Self {
        Self(Arc::new(PixelStorage {
            bytes: value,
            recycle: None,
        }))
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

pub trait PdfBackend: Send + Sync {
    fn path(&self) -> &Path;
    fn doc_id(&self) -> u64;
    fn page_count(&self) -> usize;
    fn page_dimensions(&self, page: usize) -> AppResult<(f32, f32)>;
    fn render_page(&self, page: usize, scale: f32) -> AppResult<RgbaFrame>;
    fn extract_text(&self, page: usize) -> AppResult<String>;
}

#[cfg(test)]
mod tests {
    use super::{PdfBackend, PixelBuffer, PixelBufferPool, RgbaFrame};

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

    #[test]
    fn pooled_pixel_buffer_returns_storage_after_drop() {
        let pool = Box::leak(Box::new(PixelBufferPool::default()));
        let pixels = PixelBuffer::from_pooled_vec(vec![1, 2, 3, 4], pool);

        assert_eq!(pool.available(), 0);
        drop(pixels);
        assert_eq!(pool.available(), 1);
    }

    #[test]
    fn into_pixels_vec_clones_pooled_storage_and_recycles_original() {
        let pool = Box::leak(Box::new(PixelBufferPool::default()));
        let expected = vec![1, 2, 3, 4];
        let frame = RgbaFrame {
            width: 1,
            height: 1,
            pixels: PixelBuffer::from_pooled_vec(expected.clone(), pool),
        };

        assert_eq!(pool.available(), 0);
        assert_eq!(frame.into_pixels_vec(), expected);
        assert_eq!(pool.available(), 1);
    }

    #[test]
    fn with_mut_bytes_recycles_pooled_storage_after_use() {
        let pool = Box::leak(Box::new(PixelBufferPool::default()));
        let pixels = PixelBuffer::from_pooled_vec(vec![1, 2, 3, 4], pool);

        assert_eq!(pool.available(), 0);
        let first = pixels.with_mut_bytes(|bytes| {
            bytes[0] = 9;
            bytes[0]
        });
        assert_eq!(first, 9);
        assert_eq!(pool.available(), 1);
    }

    fn _assert_pdf_backend_object_safe(_: &dyn PdfBackend) {}
}
