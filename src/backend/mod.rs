use std::path::Path;
use std::sync::Arc;

use crate::error::AppResult;

mod hayro;
#[cfg(test)]
pub(crate) mod test_support;
mod traits;

pub use hayro::HayroPdfBackend;
pub use traits::{
    OutlineNode, PdfBackend, PdfRect, PdfRenderContext, PixelBuffer, PixelBufferPool, RgbaFrame,
    TextGlyph, TextPage,
};

pub type SharedPdfBackend = Arc<dyn PdfBackend>;

pub fn open_default_backend(path: impl AsRef<Path>) -> AppResult<SharedPdfBackend> {
    HayroPdfBackend::open(path).map(|backend| Arc::new(backend) as SharedPdfBackend)
}
