use image::{DynamicImage, GrayImage, Rgb, RgbImage};

use crate::{
    SRegion,
    analysis::{copy_move::CopyMoveDetector, jpeg_analysis::JpegAnalyzer},
    detection::{
        ConfidenceLevel, DetectedManipulation, DetectionResult, Detector, ManipulationType,
        splicing::SplicingDetector,
    },
    draw,
    error::Result,
    image_utils::{clipped_blocks, mean_and_variance, rgb_to_gray},
};

/// Settings for [`TamperingDetector`].
#[derive(Debug, Clone)]
pub struct TamperingConfig {
    /// Run copy-move detection.
    pub detect_copy_move: bool,
    /// Run the full splicing detector, which itself runs ELA and noise.
    pub detect_splicing: bool,
    /// Run the texture and blur consistency checks.
    pub detect_retouching: bool,
    /// Tile size, forwarded to copy-move.
    pub block_size: u32,
    /// Multiplies the retouching z-score cutoffs, so a **lower** value produces
    /// **more** detections — the opposite of what the name suggests.
    pub sensitivity: f64,
    /// Retouching findings below this confidence are dropped.
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

/// The broadest composite detector: copy-move, splicing, retouching and
/// double compression in one pass.
///
/// Use it for a first look at an unfamiliar image, then reach for the
/// individual modules once you know what you are chasing.
///
/// # Limitations
///
/// The slowest path in the crate — it runs copy-move, the splicing detector
/// (which runs ELA and noise internally) and a full JPEG sweep. `overall_score`
/// averages, so many weak retouching findings will dilute one strong copy-move
/// detection.
pub struct TamperingDetector {
    config: TamperingConfig,
}

impl TamperingDetector {
    /// Detector with the default configuration.
    pub fn new() -> Self {
        Self {
            config: TamperingConfig::default(),
        }
    }

    /// Detector with custom settings.
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

        let blocks: Vec<SRegion> = clipped_blocks(width, height, block_size, block_size).collect();

        if blocks.is_empty() {
            return Vec::new();
        }

        let textures: Vec<f64> = blocks
            .iter()
            .map(|block| self.calculate_texture_measure(&gray, block.x, block.y, block_size))
            .collect();

        let (mean_texture, variance) = mean_and_variance(&textures);
        let std_dev = variance.sqrt();

        blocks
            .into_iter()
            .zip(textures)
            .filter_map(|(block, texture)| {
                let z_score = if std_dev > 0.0 {
                    (texture - mean_texture).abs() / std_dev
                } else {
                    0.0
                };

                (z_score > 2.0 * self.config.sensitivity).then(|| (block, (z_score / 5.0).min(1.0)))
            })
            .collect()
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

        // The region built here previously used `block_size.midpoint(height - by)`
        // for its height -- a typo for `.min(...)` that averaged the block size
        // with the remaining rows, so flagged regions ran past the image edge.
        // `clipped_blocks` now does the clamping.
        let blocks: Vec<SRegion> = clipped_blocks(width, height, block_size, block_size).collect();

        if blocks.is_empty() {
            return Vec::new();
        }

        let sharpness: Vec<f64> = blocks
            .iter()
            .map(|block| self.calculate_laplacian_variance(&gray, block.x, block.y, block_size))
            .collect();

        let (mean_sharpness, variance) = mean_and_variance(&sharpness);
        let std_dev = variance.sqrt();

        blocks
            .into_iter()
            .zip(sharpness)
            .filter_map(|(block, value)| {
                let z_score = if std_dev > 0.0 {
                    (value - mean_sharpness).abs() / std_dev
                } else {
                    0.0
                };

                (z_score > 2.5 * self.config.sensitivity).then(|| (block, (z_score / 5.0).min(1.0)))
            })
            .collect()
    }

    fn calculate_laplacian_variance(&self, gray: &GrayImage, x: u32, y: u32, size: u32) -> f64 {
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

        mean_and_variance(&laplacian_values).1
    }

    fn analyze_double_compression(
        &self,
        image: &DynamicImage,
    ) -> Result<Option<DetectedManipulation>> {
        let result = JpegAnalyzer::new().analyze(image)?;

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
                manipulation.confidence,
            );
        }

        vis
    }

    fn draw_detection(
        &self,
        image: &mut RgbImage,
        region: &SRegion,
        color: Rgb<u8>,
        confidence: f64,
    ) {
        let thickness = (confidence * 4.0) as u32 + 1;

        draw::fill(image, region, color, (confidence * 0.3) as f32);
        draw::rect(image, region, color, thickness);
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
        "Combines multiple detection methods to identify copy-move forgery, splicing, retouching, and other forms of image manipulation"
    }
}

impl Default for TamperingDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    use super::*;

    fn textured(width: u32, height: u32) -> DynamicImage {
        let mut image = RgbImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let v = (((x * 19) ^ (y * 7)) % 256) as u8;
            *pixel = Rgb([v, v, v]);
        }
        DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn detected_regions_stay_within_the_image() {
        // 100 is not a multiple of the 16 px block size, so the trailing blocks
        // are clipped; the blur sweep used to report over-tall regions here.
        let result = TamperingDetector::new()
            .detect(&textured(100, 100))
            .unwrap();

        for manipulation in &result.manipulations {
            assert!(
                manipulation.region.right() <= 100,
                "{:?}",
                manipulation.region
            );
            assert!(
                manipulation.region.bottom() <= 100,
                "{:?}",
                manipulation.region
            );
        }
    }

    #[test]
    fn overall_score_is_bounded() {
        let result = TamperingDetector::new().detect(&textured(96, 128)).unwrap();
        assert!((0.0..=1.0).contains(&result.overall_score));
    }

    #[test]
    fn visualization_matches_the_input_size() {
        let result = TamperingDetector::new().detect(&textured(96, 64)).unwrap();
        assert_eq!(result.visualization.dimensions(), (96, 64));
    }
}
