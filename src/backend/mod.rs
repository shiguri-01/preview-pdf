use std::path::Path;
use std::sync::Arc;

use crate::error::AppResult;

mod hayro;
mod traits;

pub use hayro::{HayroPdfBackend, PdfDoc};
pub use traits::{PdfBackend, RgbaFrame};

pub fn open_default_backend(path: impl AsRef<Path>) -> AppResult<Box<dyn PdfBackend>> {
    PdfDoc::open(path).map(|doc| Box::new(doc) as Box<dyn PdfBackend>)
}

pub fn load_default_shared_bytes(path: impl AsRef<Path>) -> AppResult<Arc<Vec<u8>>> {
    PdfDoc::load_shared_bytes(path)
}

pub fn open_default_backend_with_shared_bytes(
    path: impl AsRef<Path>,
    bytes: Arc<Vec<u8>>,
) -> AppResult<Box<dyn PdfBackend>> {
    PdfDoc::open_with_shared_bytes(path, bytes).map(|doc| Box::new(doc) as Box<dyn PdfBackend>)
}
