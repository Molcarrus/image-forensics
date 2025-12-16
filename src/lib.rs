use std::path::Path;

use image::{DynamicImage, GrayImage, RgbImage};

use crate::{analysis::{copy_move::CopyMoveDetector, ela::ElaAnalyzer, jpeg_analysis::JpegAnalyzer, noise::NoiseAnalyzer}, error::{ForensicsError, Result}, metadata::exif::ExifExtractor};

pub mod error;
pub mod image_utils;
pub mod analysis;
pub mod metadata;
pub mod report;

#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    pub ela_quality: u8,
    pub block_size: u32,
    pub similarity_threshold: f64,
    pub parallel: bool,
    pub min_match_distance: u32,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            ela_quality: 95,
            block_size: 16,
            similarity_threshold: 0.95,
            parallel: true,
            min_match_distance: 50,
        }
    }
}

pub struct ForensicsAnalyzer {
    original: DynamicImage,
    config: AnalysisConfig,
    path: Option<String>,
}

impl ForensicsAnalyzer {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_str = path
            .as_ref()
            .to_string_lossy()
            .to_string();
        let original = image::open(&path)?;
        
        Ok(Self {
            original,
            config: AnalysisConfig::default(),
            path: Some(path_str)
        })
    }
    
    pub fn from_image(image: DynamicImage) -> Self {
        Self { 
            original: image, 
            config: AnalysisConfig::default(), 
            path: None 
        }
    }
    
    pub fn with_config(mut self, config: AnalysisConfig) -> Self {
        self.config = config;
        self 
    }
    
    pub fn ela(&self, quality: u8) -> Result<ElaResult> {
        let analyzer = ElaAnalyzer::new(quality);
        analyzer.analyze(&self.original)
    }
    
    pub fn detect_cop_move(&self) -> Result<CopyMoveResult> {
        let detector = CopyMoveDetector::new(self.config.block_size, self.config.similarity_threshold, self.config.min_match_distance)?;
        detector.detect(&self.original)
    }
    
    pub fn analyze_noise(&self) -> Result<NoiseResult> {
        let analyzer = NoiseAnalyzer::new();
        analyzer.analyze(&self.original)
    }
    
    pub fn analyze_jpeg(&self) -> Result<JpegAnalysisResult> {
        let analyzer = JpegAnalyzer::new();
        analyzer.analyze(&self.original)
    }
    
    pub fn extract_metadata(&self) -> Result<MetadataResult> {
        if let Some(ref path) = self.path {
            ExifExtractor::extract(path)
        } else {
            Err(ForensicsError::MetadataError(
                "No file patha available for metasata extraction".into()
            ))
        }
    }
    
    pub fn full_analysis(&self) -> Result<FullAnalysisReport> {
        let ela = self.ela(self.config.ela_quality)?;
        let copy_move = self.detect_cop_move()?;
        let noise = self.analyze_noise()?;
        let jpeg = self.analyze_jpeg()?;
        let metadata = self.extract_metadata().ok();
        
        Ok(FullAnalysisReport { 
            ela: ela.clone(), 
            copy_move: copy_move.clone(), 
            noise: noise.clone(), 
            jpeg: jpeg.clone(), 
            metadata, 
            tampering_ability: Self::calculate_tampering_probability(
                &ela, &copy_move, &noise, &jpeg
            ) 
        })
    }
    
    fn calculate_tampering_probability(
        ela: &ElaResult,
        copy_move: &CopyMoveResult,
        noise: &NoiseResult, 
        jpeg: &JpegAnalysisResult
    ) -> f64 {
        let mut score = 0.0;
        let mut weight_sum = 0.0;
        
        if ela.max_difference > 50.0 {
            score += 0.3 * (ela.max_difference / 255.0).min(1.0);
            weight_sum += 0.3;
        }
        
        if !copy_move.matches.is_empty() {
            score += 0.4 * (copy_move.matches.len() as f64 / 100.0).min(1.0);
            weight_sum += 0.4;
        }
        
        if noise.inconsistency_score > 0.3 {
            score += 0.2 * noise.inconsistency_score;
            weight_sum += 0.2;
        }
        
        if jpeg.ghost_detected {
            score += 0.1;
            weight_sum += 0.1;
        }
        
        if weight_sum > 0.0 {
            score / weight_sum
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct ElaResult {
    pub image: RgbImage,
    pub difference_map: GrayImage,
    pub max_difference: f64,
    pub mean_difference: f64,
    pub std_deviation: f64,
    pub suspicious_regions: Vec<SRegion>
}

impl ElaResult {
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        self.image.save(path)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CopyMoveResult {
    pub matches: Vec<MatchPair>,
    pub visualization: RgbImage,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct MatchPair {
    pub source: SRegion,
    pub target: SRegion,
    pub similarity: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct SRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32
}

#[derive(Debug, Clone)]
pub struct NoiseResult {
    pub noise_map: GrayImage,
    pub local_variance_map: GrayImage,
    pub inconsistency_score: f64,
    pub estimated_noise_level: f64,
    pub anomalous_regions: Vec<SRegion>
}

#[derive(Debug, Clone)]
pub struct JpegAnalysisResult {
    pub quality_estimate: u8,
    pub ghost_detected: bool,
    pub ghost_map: Option<GrayImage>,
    pub blocking_artifact_map: GrayImage,
    pub double_compression_likelihood: f64,
} 

#[derive(Debug, Clone)]
pub struct MetadataResult {
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub software: Option<String>,
    pub date_time: Option<String>,
    pub gps_coordinates: Option<(f64, f64)>,
    pub all_tags: std::collections::HashMap<String, String>,
    pub suspicious_indicators: Vec<String>,
}

#[derive(Debug)]
pub struct FullAnalysisReport {
    pub ela: ElaResult,
    pub copy_move: CopyMoveResult,
    pub noise: NoiseResult,
    pub jpeg: JpegAnalysisResult,
    pub metadata: Option<MetadataResult>,
    pub tampering_ability: f64 
}