use thiserror::Error;

#[derive(Error, Debug)]
pub enum ForensicsError {
    #[error("Image loading error: {0}")]
    ImageLoad(#[from] image::ImageError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Analysis failed: {0}")]
    AnalysisFailed(String),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("Metadata extraction error: {0}")]
    MetadataError(String),

    #[error("Block size must be smaller than image dimensions")]
    InvalidBlockSize,

    #[error("Image too small for analysis (minimum: {0}x{0})")]
    ImageTooSmall(u32),
}

pub type Result<T> = std::result::Result<T, ForensicsError>;
