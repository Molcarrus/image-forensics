//! Turning analysis results into something you can save or read.
//!
//! [`JsonReport`] is a serialisable scalar summary; [`visualization`] renders
//! heatmaps and annotated overlays.

/// Heatmap and overlay rendering.
pub mod visualization;

use serde::Serialize;

use crate::FullAnalysisReport;

/// A serialisable summary of a [`FullAnalysisReport`].
///
/// Carries the scalar findings and region counts, not the image buffers, so it
/// can be written to disk or sent over a wire.
///
/// ```no_run
/// use image_forensics::{ForensicsAnalyzer, report::JsonReport};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let report = ForensicsAnalyzer::new("photo.jpg")?.full_analysis()?;
/// let json = JsonReport::from(&report).to_json()?;
/// # Ok(())
/// # }
/// ```
#[derive(Serialize)]
pub struct JsonReport {
    /// Combined tampering score, in `[0, 1]`.
    pub tampering_probability: f64,
    /// Error Level Analysis findings.
    pub ela_analysis: ElaReportSection,
    /// Copy-move findings.
    pub copy_move_analysis: CopyMoveReportSection,
    /// Noise consistency findings.
    pub noise_analysis: NoiseReportSection,
    /// JPEG compression findings.
    pub jpeg_analysis: JpegReportSection,
    /// EXIF summary, when metadata was available.
    pub metadata: Option<MetadataReportSection>,
}

/// Error Level Analysis, reduced to scalars.
#[derive(Serialize)]
pub struct ElaReportSection {
    /// Largest raw per-pixel difference.
    pub max_difference: f64,
    /// Mean raw difference.
    pub mean_difference: f64,
    /// Standard deviation of the raw differences.
    pub std_deviation: f64,
    /// How many regions exceeded the threshold.
    pub suspicious_region_count: usize,
}

/// Copy-move detection, reduced to scalars.
#[derive(Serialize)]
pub struct CopyMoveReportSection {
    /// How many non-overlapping duplicate pairs were kept.
    pub match_count: usize,
    /// Mean similarity across them.
    pub confidence: f64,
}

/// Noise analysis, reduced to scalars.
#[derive(Serialize)]
pub struct NoiseReportSection {
    /// Fraction of blocks flagged, in `[0, 1]`.
    pub inconsistency_score: f64,
    /// Global noise floor.
    pub estimated_noise_level: f64,
    /// How many blocks departed from it.
    pub anomalous_region_count: usize,
}

/// JPEG analysis, reduced to scalars.
#[derive(Serialize)]
pub struct JpegReportSection {
    /// Estimated encoding quality.
    pub quality_estimate: u8,
    /// Whether a compression ghost was found.
    pub ghost_detected: bool,
    /// Double-compression score, in `[0, 1]`.
    pub double_compression_likelihood: f64,
}

/// EXIF metadata, reduced to the fields worth reporting.
#[derive(Serialize)]
pub struct MetadataReportSection {
    /// Camera model, falling back to the make.
    pub camera_info: Option<String>,
    /// Software that last wrote the file.
    pub software: Option<String>,
    /// Notes on anything that did not add up.
    pub suspicious_indicators: Vec<String>,
}

impl From<&FullAnalysisReport> for JsonReport {
    fn from(report: &FullAnalysisReport) -> Self {
        Self {
            tampering_probability: report.tampering_probability,
            ela_analysis: ElaReportSection {
                max_difference: report.ela.max_difference,
                mean_difference: report.ela.mean_difference,
                std_deviation: report.ela.std_deviation,
                suspicious_region_count: report.ela.suspicious_regions.len(),
            },
            copy_move_analysis: CopyMoveReportSection {
                match_count: report.copy_move.matches.len(),
                confidence: report.copy_move.confidence,
            },
            noise_analysis: NoiseReportSection {
                inconsistency_score: report.noise.inconsistency_score,
                estimated_noise_level: report.noise.estimated_noise_level,
                anomalous_region_count: report.noise.anomalous_regions.len(),
            },
            jpeg_analysis: JpegReportSection {
                quality_estimate: report.jpeg.quality_estimate,
                ghost_detected: report.jpeg.ghost_detected,
                double_compression_likelihood: report.jpeg.double_compression_likelihood,
            },
            metadata: report.metadata.as_ref().map(|m| MetadataReportSection {
                camera_info: m.camera_model.clone().or_else(|| m.camera_make.clone()),
                software: m.software.clone(),
                suspicious_indicators: m.suspicious_indicators.clone(),
            }),
        }
    }
}

impl JsonReport {
    /// Serialise to pretty-printed JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
