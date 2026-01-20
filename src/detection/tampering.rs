use image::{DynamicImage, GrayImage, Rgb, RgbImage};

use crate::{
    SRegion,
    analysis::{copy_move::CopyMoveDetector, jpeg_analysis::JpegAnalyzer},
    detection::{
        ConfidenceLevel, DetectedManipulation, DetectionResult, Detector, ManipulationType,
        splicing::SplicingDetector,
    },
    error::Result,
    image_utils::rgb_to_gray,
};

#[derive(Debug, Clone)]
pub struct TamperingConfig {
    pub detect_copy_move: bool,
    pub detect_splicing: bool,
    pub detect_retouching: bool,
    pub block_size: u32,
    pub sensitivity: f64,
    pub min_confidence: f64,
}

impl Default for TamperingConfig {
    fn default() -> Self {
        Self {
            detect_copy_move: true,
            detect_splicing: true,
            detect_retouching: true,
            block_size: 16,
            sensitivity: 0.5,
            min_confidence: 0.3,
        }
    }
}

pub struct TamperingDetector {
    config: TamperingConfig,
}

impl TamperingDetector {
    pub fn new() -> Self {
        Self {
            config: TamperingConfig::default(),
        }
    }

    pub fn with_config(config: TamperingConfig) -> Self {
        Self { config }
    }

    fn detect_retouching(&self, image: &DynamicImage) -> Result<Vec<DetectedManipulation>> {
        let rgb = image.to_rgb8();
        let mut manipulations = Vec::new();

        let texture_anomalies = self.analyze_texture_consistency(&rgb);
        let blur_anomalies = self.analyze_blur_consistency(&rgb);

        for (region, score) in texture_anomalies {
            if score >= self.config.min_confidence {
                manipulations.push(DetectedManipulation {
                    manipulation_type: ManipulationType::Retouching,
                    region,
                    confidence: score,
                    confidence_level: ConfidenceLevel::from_score(score),
                    description: "Texture inconsistency suggesting retouching".into(),
                    evidence: vec!["Abnormal texture pattern".into()],
                });
            }
        }

        for (region, score) in blur_anomalies {
            if score >= self.config.min_confidence {
                manipulations.push(DetectedManipulation {
                    manipulation_type: ManipulationType::Retouching,
                    region,
                    confidence: score,
                    confidence_level: ConfidenceLevel::from_score(score),
                    description: "Blur inconsistency suggesting retouching".into(),
                    evidence: vec!["Abnormal blur pattern".into()],
                });
            }
        }

        Ok(manipulations)
    }

    fn analyze_texture_consistency(&self, image: &RgbImage) -> Vec<(SRegion, f64)> {
        let (width, height) = image.dimensions();
        let block_size = self.config.block_size;
        let gray = rgb_to_gray(image);

        let mut results = Vec::new();
        let mut block_textures = Vec::new();

        for by in (0..height).step_by(block_size as usize) {
            for bx in (0..width).step_by(block_size as usize) {
                let texture = self.calculate_texture_measure(&gray, bx, by, block_size);
                block_textures.push((bx, by, texture));
            }
        }

        if block_textures.is_empty() {
            return results;
        }

        let mean_texture =
            block_textures.iter().map(|(_, _, t)| t).sum::<f64>() / block_textures.len() as f64;
        let variance = block_textures
            .iter()
            .map(|(_, _, t)| (t - mean_texture).powi(2))
            .sum::<f64>()
            / block_textures.len() as f64;
        let std_dev = variance.sqrt();

        for (bx, by, texture) in block_textures {
            let z_score = if std_dev > 0.0 {
                (texture - mean_texture).abs() / std_dev
            } else {
                0.0
            };

            if z_score > 2.0 * self.config.sensitivity {
                let score = (z_score / 5.0).min(1.0);
                results.push((
                    SRegion {
                        x: bx,
                        y: by,
                        width: block_size.min(width - bx),
                        height: block_size.min(height - by),
                    },
                    score,
                ));
            }
        }

        results
    }

    fn calculate_texture_measure(&self, gray: &GrayImage, x: u32, y: u32, size: u32) -> f64 {
        let (width, height) = gray.dimensions();
        let mut sum = 0.0;
        let mut count = 0;

        for dy in 0..size {
            for dx in 0..size {
                let px = x + dx;
                let py = y + dy;

                if px + 1 < width && py + 1 < height {
                    let p00 = gray.get_pixel(px, py)[0] as f64;
                    let p10 = gray.get_pixel(px + 1, py)[0] as f64;
                    let p01 = gray.get_pixel(px, py + 1)[0] as f64;

                    let gx = (p10 - p00).abs();
                    let gy = (p01 - p00).abs();
                    sum += (gx * gx + gy * gy).sqrt();
                    count += 1;
                }
            }
        }

        if count > 0 { sum / count as f64 } else { 0.0 }
    }

    fn analyze_blur_consistency(&self, image: &RgbImage) -> Vec<(SRegion, f64)> {
        let (width, height) = image.dimensions();
        let block_size = self.config.block_size;
        let gray = rgb_to_gray(image);

        let mut results = Vec::new();
        let mut block_sharpness = Vec::new();

        for by in (0..height).step_by(block_size as usize) {
            for bx in (0..width).step_by(block_size as usize) {
                let sharpness = self.calculate_laplcaian_variance(&gray, bx, by, block_size);
                block_sharpness.push((bx, by, sharpness));
            }
        }

        if block_sharpness.is_empty() {
            return results;
        }

        let mean_sharpness =
            block_sharpness.iter().map(|(_, _, s)| s).sum::<f64>() / block_sharpness.len() as f64;
        let variance = block_sharpness
            .iter()
            .map(|(_, _, s)| (s - mean_sharpness).powi(2))
            .sum::<f64>()
            / block_sharpness.len() as f64;
        let std_dev = variance.sqrt();

        for (bx, by, sharpness) in block_sharpness {
            let z_score = if std_dev > 0.0 {
                (sharpness - mean_sharpness).abs() / std_dev
            } else {
                0.0
            };

            if z_score > 2.5 * self.config.sensitivity {
                let score = (z_score / 5.0).min(1.0);
                results.push((
                    SRegion {
                        x: bx,
                        y: by,
                        width: block_size.min(width - bx),
                        height: block_size.midpoint(height - by),
                    },
                    score,
                ));
            }
        }

        results
    }

    fn calculate_laplcaian_variance(&self, gray: &GrayImage, x: u32, y: u32, size: u32) -> f64 {
        let (width, height) = gray.dimensions();
        let mut laplacian_values = Vec::new();

        for dy in 1..size.saturating_sub(1) {
            for dx in 1..size.saturating_sub(1) {
                let px = x + dx;
                let py = y + dy;

                if px > 0 && px + 1 < width && py > 0 && py + 1 < height {
                    let center = gray.get_pixel(px, py)[0] as f64;
                    let top = gray.get_pixel(px, py - 1)[0] as f64;
                    let bottom = gray.get_pixel(px, py + 1)[0] as f64;
                    let left = gray.get_pixel(px - 1, py)[0] as f64;
                    let right = gray.get_pixel(px + 1, py)[0] as f64;

                    let laplacian = -4.0 * center + top + bottom + left + right;
                    laplacian_values.push(laplacian);
                }
            }
        }

        if laplacian_values.is_empty() {
            return 0.0;
        }

        let mean = laplacian_values.iter().sum::<f64>() / laplacian_values.len() as f64;
        let variance = laplacian_values
            .iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>()
            / laplacian_values.len() as f64;

        variance
    }

    fn analyze_double_compression(
        &self,
        image: &DynamicImage,
    ) -> Result<Option<DetectedManipulation>> {
        let jpeg_analyzer = JpegAnalyzer::new();
        let result = jpeg_analyzer.analyze(image)?;

        if result.double_compression_likelihood > 0.6 {
            Ok(Some(DetectedManipulation {
                manipulation_type: ManipulationType::Unknown,
                region: SRegion {
                    x: 0,
                    y: 0,
                    width: image.width(),
                    height: image.height(),
                },
                confidence: result.double_compression_likelihood,
                confidence_level: ConfidenceLevel::from_score(result.double_compression_likelihood),
                description: "Image shows signs of double JPEG compression".into(),
                evidence: vec![
                    format!("Estimated quality: {}", result.quality_estimate),
                    format!(
                        "Double compression likelihood: {:.1}%",
                        result.double_compression_likelihood * 100.0
                    ),
                ],
            }))
        } else {
            Ok(None)
        }
    }

    fn create_combined_visualization(
        &self,
        original: &RgbImage,
        manipulations: &[DetectedManipulation],
    ) -> RgbImage {
        let mut vis = original.clone();

        for manipulation in manipulations {
            let color = match manipulation.manipulation_type {
                ManipulationType::CopyMove => Rgb([255, 0, 0]),
                ManipulationType::Splicing => Rgb([255, 165, 0]),
                ManipulationType::Retouching => Rgb([255, 255, 0]),
                ManipulationType::Removal => Rgb([255, 0, 255]),
                _ => Rgb([0, 255, 255]),
            };

            self.draw_detection(
                &mut vis,
                &manipulation.region,
                color,
                &manipulation.confidence,
            );
        }

        vis
    }

    fn draw_detection(
        &self,
        image: &mut RgbImage,
        region: &SRegion,
        color: Rgb<u8>,
        confidence: &f64,
    ) {
        let (width, height) = image.dimensions();
        let thickness = (*confidence * 4.0) as u32 + 1;

        for t in 0..thickness {
            for x in region.x.saturating_sub(t)..(region.x + region.width + t).min(width) {
                if region.y >= t {
                    image.put_pixel(x, region.y - t, color);
                }
                let bottom_y = region.y + region.height + t;
                if bottom_y < height {
                    image.put_pixel(x, bottom_y, color);
                }
            }

            for y in region.y.saturating_sub(t)..(region.y + region.height + t).min(height) {
                if region.x >= t {
                    image.put_pixel(region.x - t, y, color);
                }
                let right_x = region.x + region.width + t;
                if right_x < width {
                    image.put_pixel(right_x, y, color);
                }
            }
        }

        let alpha = (*confidence * 0.3) as f32;
        for y in region.y..(region.y + region.height).min(height) {
            for x in region.x..(region.x + region.width).min(width) {
                let original = image.get_pixel(x, y);
                let blended = Rgb([
                    ((1.0 - alpha) * original[0] as f32 + alpha * color[0] as f32) as u8,
                    ((1.0 - alpha) * original[1] as f32 + alpha * color[1] as f32) as u8,
                    ((1.0 - alpha) * original[2] as f32 + alpha * color[2] as f32) as u8,
                ]);
                image.put_pixel(x, y, blended);
            }
        }
    }
}

impl Detector for TamperingDetector {
    fn detect(&self, image: &DynamicImage) -> Result<DetectionResult> {
        let rgb = image.to_rgb8();
        let mut result = DetectionResult::new(&rgb);

        if self.config.detect_copy_move {
            let copy_move_detector = CopyMoveDetector::new(self.config.block_size, 0.9, 50)?;
            let copy_move_result = copy_move_detector.detect(image)?;

            for match_pair in &copy_move_result.matches {
                result.add_manipulation(DetectedManipulation {
                    manipulation_type: ManipulationType::CopyMove,
                    region: match_pair.source,
                    confidence: match_pair.similarity,
                    confidence_level: ConfidenceLevel::from_score(match_pair.similarity),
                    description: format!(
                        "Copy-move detected: region copied from ({}, {}) to ({}, {})",
                        match_pair.source.x,
                        match_pair.source.y,
                        match_pair.target.x,
                        match_pair.target.y
                    ),
                    evidence: vec![format!("Similarity: {:.1}%", match_pair.similarity * 100.0)],
                });

                result.add_manipulation(DetectedManipulation {
                    manipulation_type: ManipulationType::CopyMove,
                    region: match_pair.target,
                    confidence: match_pair.similarity,
                    confidence_level: ConfidenceLevel::from_score(match_pair.similarity),
                    description: "Copy-move target region".into(),
                    evidence: vec![],
                });
            }
        }

        if self.config.detect_splicing {
            let splicing_detector = SplicingDetector::new();
            let splicing_result = splicing_detector.detect(image)?;

            for manipulation in splicing_result.manipulations {
                result.add_manipulation(manipulation);
            }
        }

        if self.config.detect_retouching {
            let retouching = self.detect_retouching(image)?;
            for manipulation in retouching {
                result.add_manipulation(manipulation);
            }
        }

        if let Some(compression) = self.analyze_double_compression(image)? {
            result.add_manipulation(compression);
        }

        result.visualization = self.create_combined_visualization(&rgb, &result.manipulations);

        Ok(result)
    }

    fn name(&self) -> &str {
        "Comprehensive Tampering Detector"
    }

    fn description(&self) -> &str {
        "Combines multiple detection methods to identify copy-move forgey, splicing, retouching, and other forms of image manipulation"
    }
}

impl Default for TamperingDetector {
    fn default() -> Self {
        Self::new()
    }
}
