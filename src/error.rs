pub type AppResult<T> = Result<T, AppError>;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("I/O error: {context}")]
    Io {
        #[source]
        source: std::io::Error,
        context: String,
    },
    #[error("PDF render failed for page {page}")]
    PdfRender {
        page: usize,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("unimplemented: {0}")]
    Unimplemented(String),
}

impl From<std::io::Error> for AppError {
    fn from(source: std::io::Error) -> Self {
        Self::Io {
            source,
            context: "I/O operation failed".to_string(),
        }
    }
}

impl AppError {
    pub fn io_with_context(source: std::io::Error, context: impl Into<String>) -> Self {
        Self::Io {
            source,
            context: context.into(),
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument(message.into())
    }

    pub fn pdf_render(page: usize, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::PdfRender {
            page,
            source: Box::new(source),
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }

    pub fn unimplemented(message: impl Into<String>) -> Self {
        Self::Unimplemented(message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn pdf_render_error_wraps_page_and_source() {
        let err = AppError::pdf_render(7, AppError::invalid_argument("bad page"));
        assert!(matches!(err, AppError::PdfRender { page: 7, .. }));
        assert_eq!(err.to_string(), "PDF render failed for page 7");
    }
}
