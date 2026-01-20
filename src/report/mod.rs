pub mod visualization;

use serde::Serialize;

use crate::FullAnalysisReport;

#[derive(Serialize)]
pub struct JsonReport {
    pub tampering_probability: f64,
    pub ela_analysis: ElaReportSection,
    pub copy_move_analysis: CopyMoveReportSection,
    pub noise_analysis: NoiseReportSection,
    pub jpeg_analysis: JpegReportSection,
    pub metadata: Option<MetadataReportSection>,
}

#[derive(Serialize)]
pub struct ElaReportSection {
    pub max_difference: f64,
    pub mean_difference: f64,
    pub std_deviation: f64,
    pub suspicious_region_count: usize,
}

#[derive(Serialize)]
pub struct CopyMoveReportSection {
    pub match_count: usize,
    pub confidence: f64,
}

#[derive(Serialize)]
pub struct NoiseReportSection {
    pub inconsistency_score: f64,
    pub estimated_noise_level: f64,
    pub anomalous_region_count: usize,
}

#[derive(Serialize)]
pub struct JpegReportSection {
    pub quality_estimate: u8,
    pub ghost_detected: bool,
    pub double_compression_likelihood: f64,
}

#[derive(Serialize)]
pub struct MetadataReportSection {
    pub camera_info: Option<String>,
    pub software: Option<String>,
    pub suspicious_indicators: Vec<String>,
}

impl From<&FullAnalysisReport> for JsonReport {
    fn from(report: &FullAnalysisReport) -> Self {
        Self {
            tampering_probability: report.tampering_ability,
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
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
