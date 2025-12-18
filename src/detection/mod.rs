pub mod splicing;

use image::RgbImage;
use serde::{Deserialize, Serialize};

use crate::{SRegion, error::Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfidenceLevel {
    None,
    Low,
    Medium,
    High,
    VeryHigh
}

impl ConfidenceLevel {
    pub fn from_score(score: f64) -> Self {
        match score {
            s if s < 0.2 => ConfidenceLevel::None,
            s if s < 0.4 => ConfidenceLevel::Low,
            s if s < 0.6 => ConfidenceLevel::Medium,
            s if s < 0.8 => ConfidenceLevel::High,
            _ => ConfidenceLevel::VeryHigh
        }
    }
    
    pub fn to_score(&self) -> f64 {
        match self {
            ConfidenceLevel::None => 0.0,
            ConfidenceLevel::Low => 0.3,
            ConfidenceLevel::Medium => 0.5,
            ConfidenceLevel::High => 0.7,
            ConfidenceLevel::VeryHigh => 0.9
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManipulationType {
    CopyMove,
    Splicing,
    Retouching,
    Removal,
    Resizing,
    Rotation,
    ColorManipulation,
    AIGenerated,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedManipulation {
    pub manipulation_type: ManipulationType,
    pub region: SRegion,
    pub confidence: f64,
    pub confidence_level: ConfidenceLevel,
    pub description: String,
    pub evidence: Vec<String>
}

#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub manipulations: Vec<DetectedManipulation>,
    pub overall_score: f64,
    pub overall_confidence: ConfidenceLevel,
    pub is_manipulated: bool,
    pub visualization: RgbImage,
    pub summary: String
}

impl DetectionResult {
    pub fn new(image: &RgbImage) -> Self {
        Self { 
            manipulations: Vec::new(), 
            overall_score: 0.0, 
            overall_confidence: ConfidenceLevel::None, 
            is_manipulated: false, 
            visualization: image.clone(), 
            summary: String::new() 
        }
    }
    
    pub fn add_manipulation(&mut self, manipulation: DetectedManipulation) {
        self.manipulations.push(manipulation);
        self.recalculate_overall();
    }
    
    fn recalculate_overall(&mut self) {
        if self.manipulations.is_empty() {
            self.overall_score = 0.0;
            self.overall_confidence = ConfidenceLevel::None;
            self.is_manipulated = false;
            return;
        }
        
        let total_confidence = self 
            .manipulations
            .iter()
            .map(|m| m.confidence)
            .sum::<f64>();
        
        self.overall_score = total_confidence / self.manipulations.len() as f64;
        self.overall_confidence = ConfidenceLevel::from_score(self.overall_score);
        self.is_manipulated = self.overall_score > 0.3;
        
        self.summary = format!(
            "Detected {} potential manipulation(s) with {:.1}% overall confidence",
            self.manipulations.len(),
            self.overall_score * 100.0 
        );
    }
}

pub trait Detector {
    fn detect(&self, image: &image::DynamicImage) -> Result<DetectionResult>;
    
    fn name(&self) -> &str;
    
    fn description(&self) -> &str;
}