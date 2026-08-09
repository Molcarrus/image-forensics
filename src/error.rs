use thiserror::Error;

/// Everything that can go wrong inside this crate.
///
/// [`ImageTooSmall`](Self::ImageTooSmall) is by far the most common in
/// practice: most analyzers need the image to be at least twice their
/// configured block size in both dimensions. Skipping a module that returns
/// it is usually the right response when sweeping the whole set over one
/// image.
#[derive(Error, Debug)]
pub enum ForensicsError {
    /// Decoding failed, or a JPEG round-trip inside ELA or JPEG analysis did.
    #[error("Image loading error: {0}")]
    ImageLoad(#[from] image::ImageError),

    /// The file could not be opened or read.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A constructor argument was outside its accepted range.
    ///
    /// Raised by [`CopyMoveDetector::new`](crate::analysis::copy_move::CopyMoveDetector::new)
    /// for a block size outside `4..=64`.
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// The analysis could not run to completion on this input.
    ///
    /// Raised by PCA when no patches could be extracted, or when there are
    /// fewer samples than requested components.
    #[error("Analysis failed: {0}")]
    AnalysisFailed(String),

    /// The image format is not supported. Reserved; not currently produced.
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    /// EXIF could not be read, or was requested without a file path.
    ///
    /// Distinct from a file that simply carries no EXIF block, which succeeds
    /// with a `"No EXIF data found"` indicator instead. A corrupt file and an
    /// image without metadata are opposite forensic conclusions.
    #[error("Metadata extraction error: {0}")]
    MetadataError(String),

    /// The block size exceeds the image. Reserved; not currently produced.
    #[error("Block size must be smaller than image dimensions")]
    InvalidBlockSize,

    /// The image is below the analyzer's minimum edge length, in pixels.
    #[error("Image too small for analysis (minimum: {0}x{0})")]
    ImageTooSmall(u32),
}

/// `Result` specialised to [`ForensicsError`].
pub type Result<T> = std::result::Result<T, ForensicsError>;
