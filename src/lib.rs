//! Digital image forensics: sixteen independent analyses plus EXIF metadata,
//! for finding the traces that editing leaves behind.
//!
//! Each module takes an [`image::DynamicImage`], returns its own result type,
//! and knows nothing about the others. That independence is deliberate — the
//! useful signal is agreement between methods that fail for unrelated reasons,
//! not a high score from any single one.
//!
//! # Warning
//!
//! Everything here reports a heuristic score derived from hand-chosen
//! thresholds. **These are investigative signals, not proof.** Ordinary
//! processing — resaving a JPEG, resizing for the web, a phone's own noise
//! reduction — trips most of these detectors. Treat a result as a pointer to a
//! region worth examining by hand.
//!
//! # Getting started
//!
//! A single detector:
//!
//! ```no_run
//! use image_forensics::{analysis::copy_move::CopyMoveDetector, error::Result};
//!
//! # fn main() -> Result<()> {
//! let image = image::open("photo.jpg")?;
//!
//! // block_size, similarity_threshold, min_distance
//! let detector = CopyMoveDetector::new(16, 0.95, 50)?;
//! let result = detector.detect(&image)?;
//!
//! println!("{} duplicated regions", result.matches.len());
//! # Ok(())
//! # }
//! ```
//!
//! Or the bundled pipeline over ELA, copy-move, noise and JPEG analysis:
//!
//! ```no_run
//! use image_forensics::{ForensicsAnalyzer, error::Result};
//!
//! # fn main() -> Result<()> {
//! let report = ForensicsAnalyzer::new("photo.jpg")?.full_analysis()?;
//! println!("{:.1}%", report.tampering_probability * 100.0);
//! # Ok(())
//! # }
//! ```
//!
//! # Module map
//!
//! - [`analysis`] — the sixteen individual detectors.
//! - [`detection`] — composite detectors that combine several analyses.
//! - [`metadata`] — EXIF extraction.
//! - [`region`] — [`SRegion`], the rectangle every module reports locations as.
//! - [`image_utils`] — shared pixel primitives: block iteration, Sobel, statistics.
//! - [`draw`] — overlay primitives for visualizations.
//! - [`report`] — JSON summaries and heatmap rendering.
//! - [`error`] — [`ForensicsError`](error::ForensicsError) and the crate `Result` alias.

#![warn(missing_docs)]

use std::path::Path;

use image::{DynamicImage, GrayImage, RgbImage};

use crate::{
    analysis::{
        copy_move::CopyMoveDetector, ela::ElaAnalyzer, jpeg_analysis::JpegAnalyzer,
        noise::NoiseAnalyzer,
    },
    error::{ForensicsError, Result},
    metadata::exif::ExifExtractor,
};

/// The sixteen individual analysis modules.
pub mod analysis;
/// Composite detectors that combine several analyses.
pub mod detection;
/// Overlay primitives for visualizations: lines, rectangles, arrows.
pub mod draw;
/// [`ForensicsError`](error::ForensicsError) and the crate `Result` alias.
pub mod error;
/// Pixel primitives shared by every module.
pub mod image_utils;
/// EXIF metadata extraction.
pub mod metadata;
/// [`SRegion`] and region clustering.
pub mod region;
/// JSON summaries and heatmap rendering.
pub mod report;

pub use region::{SRegion, merge_regions};

/// Settings for [`ForensicsAnalyzer`], the bundled four-module pipeline.
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    /// JPEG quality for the ELA recompression pass. Default 95.
    pub ela_quality: u8,
    /// Copy-move block size, in pixels. Default 16.
    pub block_size: u32,
    /// Minimum feature correlation for a copy-move match. Default 0.95.
    pub similarity_threshold: f64,
    /// Minimum separation between the two halves of a match. Default 50.
    pub min_match_distance: u32,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            ela_quality: 95,
            block_size: 16,
            similarity_threshold: 0.95,
            min_match_distance: 50,
        }
    }
}

/// Runs ELA, copy-move, noise and JPEG analysis over one image.
///
/// A convenience over calling the four modules individually. Construct from a
/// path to also get EXIF metadata; [`from_image`](Self::from_image) skips it,
/// since metadata lives in the file container rather than the decoded pixels.
///
/// ```no_run
/// use image_forensics::{ForensicsAnalyzer, error::Result};
///
/// # fn main() -> Result<()> {
/// let report = ForensicsAnalyzer::new("photo.jpg")?.full_analysis()?;
/// println!("{:.1}%", report.tampering_probability * 100.0);
/// # Ok(())
/// # }
/// ```
pub struct ForensicsAnalyzer {
    original: DynamicImage,
    config: AnalysisConfig,
    path: Option<String>,
}

impl ForensicsAnalyzer {
    /// Load an image from disk, retaining the path so metadata works.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        let original = image::open(&path)?;

        Ok(Self {
            original,
            config: AnalysisConfig::default(),
            path: Some(path_str),
        })
    }

    /// Wrap an already-decoded image.
    ///
    /// [`extract_metadata`](Self::extract_metadata) will fail on the result:
    /// EXIF is not present in decoded pixels.
    pub fn from_image(image: DynamicImage) -> Self {
        Self {
            original: image,
            config: AnalysisConfig::default(),
            path: None,
        }
    }

    /// Replace the default configuration.
    pub fn with_config(mut self, config: AnalysisConfig) -> Self {
        self.config = config;
        self
    }

    /// Error Level Analysis at the given recompression quality.
    pub fn ela(&self, quality: u8) -> Result<ElaResult> {
        ElaAnalyzer::new(quality).analyze(&self.original)
    }

    /// Find regions duplicated within the image.
    pub fn detect_copy_move(&self) -> Result<CopyMoveResult> {
        let detector = CopyMoveDetector::new(
            self.config.block_size,
            self.config.similarity_threshold,
            self.config.min_match_distance,
        )?;
        detector.detect(&self.original)
    }

    /// Check the sensor noise floor for local inconsistency.
    pub fn analyze_noise(&self) -> Result<NoiseResult> {
        NoiseAnalyzer::new().analyze(&self.original)
    }

    /// Estimate JPEG quality and look for compression ghosts.
    pub fn analyze_jpeg(&self) -> Result<JpegAnalysisResult> {
        JpegAnalyzer::new().analyze(&self.original)
    }

    /// Read EXIF from the source file.
    ///
    /// # Errors
    ///
    /// [`ForensicsError::MetadataError`] when this analyzer was built with
    /// [`from_image`](Self::from_image) and so has no path.
    pub fn extract_metadata(&self) -> Result<MetadataResult> {
        match self.path {
            Some(ref path) => ExifExtractor::extract(path),
            None => Err(ForensicsError::MetadataError(
                "no file path available for metadata extraction".into(),
            )),
        }
    }

    /// Run all four analyses and combine them into one report.
    ///
    /// Metadata is included when available and omitted otherwise, rather than
    /// failing the whole analysis.
    pub fn full_analysis(&self) -> Result<FullAnalysisReport> {
        let ela = self.ela(self.config.ela_quality)?;
        let copy_move = self.detect_copy_move()?;
        let noise = self.analyze_noise()?;
        let jpeg = self.analyze_jpeg()?;
        let metadata = self.extract_metadata().ok();

        let tampering_probability =
            Self::calculate_tampering_probability(&ela, &copy_move, &noise, &jpeg);

        Ok(FullAnalysisReport {
            ela,
            copy_move,
            noise,
            jpeg,
            metadata,
            tampering_probability,
        })
    }

    /// Combine the four core signals into a single `[0, 1]` score.
    ///
    /// Each signal contributes its own weight out of a fixed total. Dividing by
    /// the *triggered* weight instead — as this previously did — meant a single
    /// weak signal produced a maximal score: an image whose only positive was
    /// `ghost_detected` scored `0.1 / 0.1 = 1.0`, i.e. certain tampering.
    fn calculate_tampering_probability(
        ela: &ElaResult,
        copy_move: &CopyMoveResult,
        noise: &NoiseResult,
        jpeg: &JpegAnalysisResult,
    ) -> f64 {
        const W_ELA: f64 = 0.3;
        const W_COPY_MOVE: f64 = 0.4;
        const W_NOISE: f64 = 0.2;
        const W_JPEG: f64 = 0.1;
        const TOTAL_WEIGHT: f64 = W_ELA + W_COPY_MOVE + W_NOISE + W_JPEG;

        let mut score = 0.0;

        if ela.max_difference > 50.0 {
            score += W_ELA * (ela.max_difference / 255.0).min(1.0);
        }

        if !copy_move.matches.is_empty() {
            score += W_COPY_MOVE * (copy_move.matches.len() as f64 / 100.0).min(1.0);
        }

        if noise.inconsistency_score > 0.3 {
            score += W_NOISE * noise.inconsistency_score.min(1.0);
        }

        if jpeg.ghost_detected {
            score += W_JPEG;
        }

        (score / TOTAL_WEIGHT).clamp(0.0, 1.0)
    }
}

/// Output of [`ElaAnalyzer`](analysis::ela::ElaAnalyzer).
///
/// The three scalars are all in *raw* difference units, so they are directly
/// comparable with one another.
#[derive(Debug, Clone)]
pub struct ElaResult {
    /// Amplified per-channel difference, for viewing. The image to look at.
    pub image: RgbImage,
    /// Amplified single-channel difference.
    pub difference_map: GrayImage,
    /// Largest raw per-pixel difference.
    pub max_difference: f64,
    /// Mean raw difference across the image.
    pub mean_difference: f64,
    /// Standard deviation of the raw differences.
    pub std_deviation: f64,
    /// Blocks whose mean difference exceeded `mean + 2 * std_deviation`, merged.
    pub suspicious_regions: Vec<SRegion>,
}

impl ElaResult {
    /// Write [`image`](Self::image) to disk. Format inferred from the extension.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        self.image.save(path)?;
        Ok(())
    }
}

/// Output of [`CopyMoveDetector`](analysis::copy_move::CopyMoveDetector).
///
/// A genuine copy-move usually appears as *many* matches sharing one offset
/// vector. Scattered matches with no common offset are more likely coincidence
/// in repetitive texture.
#[derive(Debug, Clone)]
pub struct CopyMoveResult {
    /// Non-overlapping matched region pairs, strongest first.
    pub matches: Vec<MatchPair>,
    /// The original with each pair boxed and joined by a line.
    pub visualization: RgbImage,
    /// Mean similarity across the retained matches. Zero when there are none.
    pub confidence: f64,
}

/// Two regions of one image that resemble each other closely.
#[derive(Debug, Clone)]
pub struct MatchPair {
    /// The earlier of the two regions in scan order.
    pub source: SRegion,
    /// The later of the two regions in scan order.
    pub target: SRegion,
    /// Pearson correlation of the two DCT feature vectors, floored at zero.
    pub similarity: f64,
}

/// Output of [`NoiseAnalyzer`](analysis::noise::NoiseAnalyzer).
#[derive(Debug, Clone)]
pub struct NoiseResult {
    /// High-frequency residual: the image minus a Gaussian blur.
    pub noise_map: GrayImage,
    /// Local standard deviation around each pixel.
    pub local_variance_map: GrayImage,
    /// Fraction of blocks flagged, in `[0, 1]`. A proportion of the image, not
    /// a probability of tampering.
    pub inconsistency_score: f64,
    /// Global noise floor: median absolute deviation scaled by 1.4826.
    pub estimated_noise_level: f64,
    /// Blocks whose local variance departs from the global floor.
    pub anomalous_regions: Vec<SRegion>,
}

/// Output of [`JpegAnalyzer`](analysis::jpeg_analysis::JpegAnalyzer).
#[derive(Debug, Clone)]
pub struct JpegAnalysisResult {
    /// Quality whose recompression perturbs the image least.
    pub quality_estimate: u8,
    /// Whether a local dip was found in the recompression curve.
    pub ghost_detected: bool,
    /// Quality at which the ghost sits. `Some` exactly when `ghost_detected`.
    pub ghost_quality: Option<u8>,
    /// Difference map at the ghost quality. `Some` exactly when `ghost_detected`.
    pub ghost_map: Option<GrayImage>,
    /// Strength of the discontinuities on the 8-pixel coding grid.
    pub blocking_artifact_map: GrayImage,
    /// Combined ghost and grid-alignment score, in `[0, 1]`. Double compression
    /// is routine and is not by itself evidence of editing.
    pub double_compression_likelihood: f64,
}

/// Output of [`ExifExtractor`](metadata::exif::ExifExtractor).
///
/// EXIF is trivially forged, so consistent metadata is not evidence of
/// authenticity, and its absence is the norm for anything downloaded from a
/// platform that strips it.
#[derive(Debug, Clone)]
pub struct MetadataResult {
    /// Camera manufacturer, as plain text.
    pub camera_make: Option<String>,
    /// Camera model, as plain text.
    pub camera_model: Option<String>,
    /// Software that last wrote the file.
    pub software: Option<String>,
    /// File datetime, as recorded in EXIF.
    pub date_time: Option<String>,
    /// Decimal degrees, `(latitude, longitude)`. Negative is south and west.
    pub gps_coordinates: Option<(f64, f64)>,
    /// Every field found. Keys are the tag name for the primary IFD and
    /// `Thumbnail.<tag>` for the thumbnail IFD.
    pub all_tags: std::collections::HashMap<String, String>,
    /// Human-readable notes on anything that did not add up.
    pub suspicious_indicators: Vec<String>,
}

/// Combined output of [`ForensicsAnalyzer::full_analysis`].
#[derive(Debug)]
pub struct FullAnalysisReport {
    /// Error Level Analysis.
    pub ela: ElaResult,
    /// Duplicated regions.
    pub copy_move: CopyMoveResult,
    /// Sensor noise consistency.
    pub noise: NoiseResult,
    /// JPEG quality, ghosts and blocking.
    pub jpeg: JpegAnalysisResult,
    /// EXIF, when the analyzer was built from a path and the file carried any.
    pub metadata: Option<MetadataResult>,
    /// Weighted combination of the four signals, in `[0, 1]`.
    ///
    /// Each signal contributes its own share of a fixed total weight, so one
    /// weak positive cannot drive the score to 1.
    pub tampering_probability: f64,
}
