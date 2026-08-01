use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};

use crate::{
    SRegion,
    error::Result,
    image_utils::{ensure_min_dimensions, full_blocks},
    region::merge_regions,
};

#[derive(Debug, Clone)]
pub struct CfaConfig {
    pub block_size: u32,
    pub expected_pattern: CfaPattern,
    pub mismatch_threshold: f64,
    pub min_variance: f64,
    pub detect_interpolation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CfaPattern {
    RGGB,
    BGGR,
    GRBG,
    GBRG,
    Unknown,
}

impl Default for CfaConfig {
    fn default() -> Self {
        Self {
            block_size: 32,
            expected_pattern: CfaPattern::RGGB,
            mismatch_threshold: 0.3,
            min_variance: 10.0,
            detect_interpolation: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CfaMeasurement {
    pub x: u32,
    pub y: u32,
    pub detected_pattern: CfaPattern,
    pub confidence: f64,
    pub interpolation_strength: f64,
    pub matches_expected: bool,
}

#[derive(Debug, Clone)]
pub struct CfaAnalysisResult {
    pub measurements: Vec<CfaMeasurement>,
    pub dominant_pattern: CfaPattern,
    pub pattern_confidence: f64,
    pub artifact_map: GrayImage,
    pub consistency_map: GrayImage,
    pub inconsistent_regions: Vec<SRegion>,
    pub consistency_score: f64,
    pub manipulation_probability: f64,
    pub pattern_stats: CfaPatternStats,
}

#[derive(Debug, Clone, Default)]
pub struct CfaPatternStats {
    pub rggb_count: usize,
    pub bggr_count: usize,
    pub grbg_count: usize,
    pub gbrg_count: usize,
    pub unknown_count: usize,
}

pub struct CfaAnalyzer {
    config: CfaConfig,
}

impl CfaAnalyzer {
    pub fn new() -> Self {
        Self::with_config(CfaConfig::default())
    }

    pub fn with_config(config: CfaConfig) -> Self {
        Self { config }
    }

    pub fn analyze(&self, image: &DynamicImage) -> Result<CfaAnalysisResult> {
        let rgb = image.to_rgb8();
        let (width, height) = rgb.dimensions();

        ensure_min_dimensions(width, height, self.config.block_size * 2)?;

        let measurements = self.analyze_cfa_patterns(&rgb);

        let pattern_stats = self.calculate_pattern_stats(&measurements);

        let (dominant_pattern, pattern_confidence) =
            self.determine_dominant_pattern(&pattern_stats, &measurements);

        let artifact_map = self.create_artifact_map(&rgb);

        let consistency_map =
            self.create_consistency_map(width, height, &measurements, dominant_pattern);

        let inconsistent_regions = self.find_inconsistent_regions(&measurements, dominant_pattern);

        let consistency_score = self.calculate_consistency_score(&measurements, dominant_pattern);

        let manipulation_probability = self.calculate_manipulation_probability(
            &inconsistent_regions,
            consistency_score,
            &pattern_stats,
            width,
            height,
        );

        Ok(CfaAnalysisResult {
            measurements,
            dominant_pattern,
            pattern_confidence,
            artifact_map,
            consistency_map,
            inconsistent_regions,
            consistency_score,
            manipulation_probability,
            pattern_stats,
        })
    }

    fn analyze_cfa_patterns(&self, rgb: &RgbImage) -> Vec<CfaMeasurement> {
        let (width, height) = rgb.dimensions();
        let block_size = self.config.block_size;

        full_blocks(width, height, block_size, (block_size / 2).max(1))
            .filter_map(|region| self.analyze_block(rgb, region.x, region.y, block_size))
            .collect()
    }

    fn analyze_block(&self, rgb: &RgbImage, bx: u32, by: u32, size: u32) -> Option<CfaMeasurement> {
        let variance = self.calculate_block_variance(rgb, bx, by, size);
        if variance < self.config.min_variance {
            return None;
        }

        let pattern_scores = self.detect_cfa_pattern(rgb, bx, by, size);

        let (detected_pattern, confidence) = self.best_pattern(&pattern_scores);

        let interpolation_strength = if self.config.detect_interpolation {
            self.measure_interpolation_artifacts(rgb, bx, by, size)
        } else {
            0.0
        };

        let matches_expected = detected_pattern == self.config.expected_pattern;

        Some(CfaMeasurement {
            x: bx,
            y: by,
            detected_pattern,
            confidence,
            interpolation_strength,
            matches_expected,
        })
    }

    fn calculate_block_variance(&self, rgb: &RgbImage, bx: u32, by: u32, size: u32) -> f64 {
        let (width, height) = rgb.dimensions();
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        let mut count = 0;

        for y in by..(by + size).min(height) {
            for x in bx..(bx + size).min(width) {
                let pixel = rgb.get_pixel(x, y);
                let gray =
                    0.299 * pixel[0] as f64 + 0.587 * pixel[1] as f64 + 0.114 * pixel[2] as f64;
                sum += gray;
                sum_sq += gray * gray;
                count += 1;
            }
        }

        if count == 0 {
            return 0.0;
        }

        let mean = sum / count as f64;
        (sum_sq / count as f64 - mean * mean).max(0.0)
    }

    fn detect_cfa_pattern(&self, rgb: &RgbImage, bx: u32, by: u32, size: u32) -> [f64; 4] {
        let (width, height) = rgb.dimensions();
        let mut scores = [0.0f64; 4];

        for y in (by..by + size - 1).step_by(2) {
            for x in (bx..bx + size - 1).step_by(2) {
                if x + 1 >= width || y + 1 >= height {
                    continue;
                }

                let p00 = rgb.get_pixel(x, y);
                let p10 = rgb.get_pixel(x + 1, y);
                let p01 = rgb.get_pixel(x, y + 1);
                let p11 = rgb.get_pixel(x + 1, y + 1);

                // RGGB
                scores[0] += self.pattern_match_score(
                    [p00, p10, p01, p11],
                    [[2, 0, 0], [0, 1, 0], [0, 1, 0], [0, 0, 2]],
                );

                // BGGR
                scores[1] += self.pattern_match_score(
                    [p00, p10, p01, p11],
                    [[0, 0, 2], [0, 1, 0], [0, 1, 0], [2, 0, 0]],
                );

                // GRBG. The blue site previously carried the weight
                // `[0, 2, 0]`, which matches no arm of `pattern_match_score`
                // and so scored a constant zero, biasing this pattern down on
                // every block.
                scores[2] += self.pattern_match_score(
                    [p00, p10, p01, p11],
                    [[0, 1, 0], [2, 0, 0], [0, 0, 2], [0, 1, 0]],
                );

                // GBRG
                scores[3] += self.pattern_match_score(
                    [p00, p10, p01, p11],
                    [[0, 1, 0], [0, 0, 2], [2, 0, 0], [0, 1, 0]],
                );
            }
        }

        let max_score = scores.iter().cloned().fold(0.0, f64::max);
        if max_score > 0.0 {
            for score in &mut scores {
                *score /= max_score;
            }
        }

        scores
    }

    /// Score a 2x2 quad against one Bayer arrangement.
    ///
    /// `quad` is the sampled pixels in (00, 10, 01, 11) order and `weights` the
    /// expected filter at each of those sites, same order.
    fn pattern_match_score(&self, quad: [&Rgb<u8>; 4], weights: [[u8; 3]; 4]) -> f64 {
        let score = |pixel: &Rgb<u8>, weight: [u8; 3]| -> f64 {
            let r = pixel[0] as f64;
            let g = pixel[1] as f64;
            let b = pixel[2] as f64;

            match (weight[0], weight[1], weight[2]) {
                (2, 0, 0) => r / (g + b + 1.0),
                (0, 1, 0) => g / (r + b + 1.0),
                (0, 0, 2) => b / (r + g + 1.0),
                _ => 0.0,
            }
        };

        quad.iter()
            .zip(weights.iter())
            .map(|(&pixel, &weight)| score(pixel, weight))
            .sum()
    }

    fn best_pattern(&self, scores: &[f64; 4]) -> (CfaPattern, f64) {
        let mut best_idx = 0;
        let mut best_score = scores[0];

        for (i, &score) in scores.iter().enumerate().skip(1) {
            if score > best_score {
                best_score = score;
                best_idx = i;
            }
        }

        let pattern = match best_idx {
            0 => CfaPattern::RGGB,
            1 => CfaPattern::BGGR,
            2 => CfaPattern::GRBG,
            3 => CfaPattern::GBRG,
            _ => CfaPattern::Unknown,
        };

        let second_best = scores
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != best_idx)
            .map(|(_, &s)| s)
            .fold(0.0, f64::max);

        let confidence = if best_score > 0.0 {
            (best_score - second_best) / best_score
        } else {
            0.0
        };

        (pattern, confidence)
    }

    fn measure_interpolation_artifacts(&self, rgb: &RgbImage, bx: u32, by: u32, size: u32) -> f64 {
        let (width, height) = rgb.dimensions();
        let mut artifact_sum = 0.0;
        let mut count = 0;

        for y in (by + 1)..(by + size - 1).min(height - 1) {
            for x in (bx + 1)..(bx + size - 1).min(width - 1) {
                let artifact = self.detect_zipper_artifact(rgb, x, y);
                artifact_sum += artifact;
                count += 1;
            }
        }

        if count > 0 {
            artifact_sum / count as f64
        } else {
            0.0
        }
    }

    fn detect_zipper_artifact(&self, rgb: &RgbImage, x: u32, y: u32) -> f64 {
        let center = rgb.get_pixel(x, y);
        let left = rgb.get_pixel(x - 1, y);
        let right = rgb.get_pixel(x + 1, y);
        let top = rgb.get_pixel(x, y - 1);
        let bottom = rgb.get_pixel(x, y + 1);

        let mut artifact = 0.0;

        for c in 0..3 {
            let h_diff = (left[c] as i32 - 2 * center[c] as i32 + right[c] as i32).abs();
            let v_diff = (top[c] as i32 - 2 * center[c] as i32 + bottom[c] as i32).abs();
            artifact += (h_diff + v_diff) as f64;
        }

        artifact / (6.0 * 255.0)
    }

    fn calculate_pattern_stats(&self, measurements: &[CfaMeasurement]) -> CfaPatternStats {
        let mut stats = CfaPatternStats::default();

        for m in measurements {
            match m.detected_pattern {
                CfaPattern::RGGB => stats.rggb_count += 1,
                CfaPattern::BGGR => stats.bggr_count += 1,
                CfaPattern::GRBG => stats.grbg_count += 1,
                CfaPattern::GBRG => stats.gbrg_count += 1,
                CfaPattern::Unknown => stats.unknown_count += 1,
            }
        }

        stats
    }

    fn determine_dominant_pattern(
        &self,
        stats: &CfaPatternStats,
        _measurements: &[CfaMeasurement],
    ) -> (CfaPattern, f64) {
        let total = stats.rggb_count
            + stats.bggr_count
            + stats.grbg_count
            + stats.gbrg_count
            + stats.unknown_count;

        if total == 0 {
            return (CfaPattern::Unknown, 0.0);
        }

        let counts = [
            (CfaPattern::RGGB, stats.rggb_count),
            (CfaPattern::BGGR, stats.bggr_count),
            (CfaPattern::GRBG, stats.grbg_count),
            (CfaPattern::GBRG, stats.gbrg_count),
        ];

        let (pattern, count) = counts.iter().max_by_key(|(_, c)| *c).unwrap();

        let confidence = *count as f64 / total as f64;

        (*pattern, confidence)
    }

    fn create_artifact_map(&self, rgb: &RgbImage) -> GrayImage {
        let (width, height) = rgb.dimensions();
        let mut map = GrayImage::new(width, height);

        if width < 3 || height < 3 {
            return map;
        }

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let artifact = self.detect_zipper_artifact(rgb, x, y);
                let value = (artifact * 255.0).min(255.0) as u8;
                map.put_pixel(x, y, Luma([value]));
            }
        }

        map
    }

    fn create_consistency_map(
        &self,
        width: u32,
        height: u32,
        measurements: &[CfaMeasurement],
        dominant: CfaPattern,
    ) -> GrayImage {
        let mut map = GrayImage::new(width, height);
        let block_size = self.config.block_size;

        for m in measurements {
            let value = if m.detected_pattern == dominant {
                ((1.0 - m.confidence) * 127.0) as u8
            } else {
                (128.0 + m.confidence * 127.0) as u8
            };

            for y in m.y..(m.y + block_size).min(height) {
                for x in m.x..(m.x + block_size).min(width) {
                    let current = map.get_pixel(x, y)[0];
                    map.put_pixel(x, y, Luma([current.max(value)]));
                }
            }
        }

        map
    }

    fn find_inconsistent_regions(
        &self,
        measurements: &[CfaMeasurement],
        dominant: CfaPattern,
    ) -> Vec<SRegion> {
        let block_size = self.config.block_size;

        let inconsistent = measurements
            .iter()
            .filter(|m| {
                m.detected_pattern != dominant && m.confidence > self.config.mismatch_threshold
            })
            .map(|m| SRegion {
                x: m.x,
                y: m.y,
                width: block_size,
                height: block_size,
            })
            .collect::<Vec<_>>();

        merge_regions(inconsistent, self.config.block_size / 2)
    }

    fn calculate_consistency_score(
        &self,
        measurements: &[CfaMeasurement],
        dominant: CfaPattern,
    ) -> f64 {
        if measurements.is_empty() {
            return 1.0;
        }

        let matching = measurements
            .iter()
            .filter(|m| m.detected_pattern == dominant)
            .count();

        matching as f64 / measurements.len() as f64
    }

    fn calculate_manipulation_probability(
        &self,
        inconsistent_regions: &[SRegion],
        consistency_score: f64,
        pattern_stats: &CfaPatternStats,
        width: u32,
        height: u32,
    ) -> f64 {
        let total_pixels = (width * height) as f64;

        let inconsistent_pixels = inconsistent_regions
            .iter()
            .map(|r| r.width * r.height)
            .sum::<u32>();

        let coverage = inconsistent_pixels as f64 / total_pixels;

        let non_zero_patterns = [
            pattern_stats.rggb_count,
            pattern_stats.bggr_count,
            pattern_stats.grbg_count,
            pattern_stats.gbrg_count,
        ]
        .iter()
        .filter(|&&c| c > 0)
        .count();

        let diversity_penalty = if non_zero_patterns > 1 {
            (non_zero_patterns - 1) as f64 * 0.2
        } else {
            0.0
        };

        let probability =
            coverage * 0.3 + (1.0 - consistency_score) * 0.4 + diversity_penalty * 0.3;

        probability.min(1.0)
    }
}

impl Default for CfaAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
