use std::path::Path;

use image::{DynamicImage, GrayImage, RgbImage};

use crate::error::Result;

pub mod error;
pub mod image_utils;
pub mod analysis;

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
    pub visulaization: RgbImage,
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