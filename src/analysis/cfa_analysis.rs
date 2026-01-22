use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};

use crate::{SRegion, error::Result};

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

        if width < self.config.block_size * 2 || height < self.config.block_size * 2 {
            return Err(crate::error::ForensicsError::ImageTooSmall(
                self.config.block_size * 2,
            ));
        }

        let measurements = self.analyze_cfa_patterns(&rgb);

        let pattern_stats = self.calculate_pattern_stats(&measurements);

        let (dominant_pattern, pattern_confidence) =
            self.determine_dominant_pattern(&pattern_stats, &measurements);

        let artifact_map = self.create_artifact_map(&rgb);

        let consistency_map =
            self.create_consistency_map(width, height, &measurements, dominant_pattern);

        let inconsistent_regions = self.find_inconsistent_regions(&measurements, dominant_pattern);

        let consistency_score = self.calculate_consistency_score(&measurements, dominant_pattern);

        let manipulation_probability = self.calculate_mainpulation_probability(
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
        let mut measurements = Vec::new();

        for by in (0..height - block_size).step_by(block_size as usize / 2) {
            for bx in (0..width - block_size).step_by(block_size as usize / 2) {
                if let Some(measurement) = self.analyze_block(rgb, bx, by, block_size) {
                    measurements.push(measurement);
                }
            }
        }

        measurements
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

                scores[0] += self.pattern_match_score(
                    p00,
                    p10,
                    p01,
                    p11,
                    [2, 0, 0],
                    [0, 1, 0],
                    [0, 1, 0],
                    [0, 0, 2],
                );

                scores[1] += self.pattern_match_score(
                    p00,
                    p10,
                    p01,
                    p11,
                    [0, 0, 2],
                    [0, 1, 0],
                    [0, 1, 0],
                    [2, 0, 0],
                );

                scores[2] += self.pattern_match_score(
                    p00,
                    p10,
                    p01,
                    p11,
                    [0, 1, 0],
                    [2, 0, 0],
                    [0, 2, 0],
                    [0, 1, 0],
                );

                scores[3] += self.pattern_match_score(
                    p00,
                    p10,
                    p01,
                    p11,
                    [0, 1, 0],
                    [0, 0, 2],
                    [2, 0, 0],
                    [0, 1, 0],
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

    fn pattern_match_score(
        &self,
        p00: &Rgb<u8>,
        p10: &Rgb<u8>,
        p01: &Rgb<u8>,
        p11: &Rgb<u8>,
        w00: [u8; 3],
        w10: [u8; 3],
        w01: [u8; 3],
        w11: [u8; 3],
    ) -> f64 {
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

        score(p00, w00) + score(p10, w10) + score(p01, w01) + score(p11, w11)
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
        measurements: &[CfaMeasurement],
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

        self.merge_regions(inconsistent)
    }

    fn merge_regions(&self, regions: Vec<SRegion>) -> Vec<SRegion> {
        if regions.is_empty() {
            return regions;
        }

        let mut merged = Vec::new();
        let mut used = vec![false; regions.len()];

        for i in 0..regions.len() {
            if used[i] {
                continue;
            }

            let mut current = regions[i];
            used[i] = true;

            loop {
                let mut found = false;
                for j in 0..regions.len() {
                    if used[j] {
                        continue;
                    }

                    if self.regions_adjacent(&current, &regions[j]) {
                        current = self.merge_two_regions(&current, &regions[j]);
                        used[j] = true;
                        found = true;
                    }
                }

                if !found {
                    break;
                }
            }

            merged.push(current);
        }

        merged
    }

    fn regions_adjacent(&self, a: &SRegion, b: &SRegion) -> bool {
        let gap = self.config.block_size / 2;

        !(a.x + a.width + gap < b.x
            || b.x + b.width + gap < a.x
            || a.y + a.height + gap < b.y
            || b.y + b.height + gap < a.y)
    }

    fn merge_two_regions(&self, a: &SRegion, b: &SRegion) -> SRegion {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        let x2 = (a.x + a.width).max(b.x + b.width);
        let y2 = (a.y + a.height).max(b.y + b.height);

        SRegion {
            x,
            y,
            width: x2 - x,
            height: y2 - y,
        }
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

    fn calculate_mainpulation_probability(
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

        let total_patterns = pattern_stats.rggb_count
            + pattern_stats.bggr_count
            + pattern_stats.grbg_count
            + pattern_stats.gbrg_count;

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
