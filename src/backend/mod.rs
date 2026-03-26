use std::path::Path;
use std::sync::Arc;

use crate::error::AppResult;

mod hayro;
mod traits;

pub use hayro::{HayroPdfBackend, PdfDoc};
pub use traits::{OutlineNode, PdfBackend, PixelBuffer, PixelBufferPool, RgbaFrame};

pub type SharedPdfBackend = Arc<dyn PdfBackend>;

pub fn open_default_backend(path: impl AsRef<Path>) -> AppResult<SharedPdfBackend> {
    PdfDoc::open(path).map(|doc| Arc::new(doc) as SharedPdfBackend)
}

pub fn load_default_shared_bytes(path: impl AsRef<Path>) -> AppResult<Arc<Vec<u8>>> {
    PdfDoc::load_shared_bytes(path)
}

pub fn open_default_backend_with_shared_bytes(
    path: impl AsRef<Path>,
    bytes: Arc<Vec<u8>>,
) -> AppResult<SharedPdfBackend> {
    PdfDoc::open_with_shared_bytes(path, bytes).map(|doc| Arc::new(doc) as SharedPdfBackend)
}
