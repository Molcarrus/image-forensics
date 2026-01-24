use std::f64::consts::PI;

use image::{DynamicImage, GrayImage, Luma};

use crate::{SRegion, error::Result, image_utils::rgb_to_gray};

#[derive(Debug, Clone)]
pub struct BenfordConfig {
    pub block_size: u32,
    pub chi_square_threshold: f64,
    pub min_samples: usize,
}

impl Default for BenfordConfig {
    fn default() -> Self {
        Self {
            block_size: 64,
            chi_square_threshold: 15.0,
            min_samples: 100,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BenfordAnalysisResult {
    pub global_distribution: [f64; 9],
    pub expected_distribution: [f64; 9],
    pub global_chi_square: f64,
    pub deviation_map: GrayImage,
    pub anomalous_regions: Vec<SRegion>,
    pub conformity_score: f64,
    pub manipulation_probability: f64,
}

pub struct BenfordAnalyzer {
    config: BenfordConfig,
    expected: [f64; 9],
}

impl BenfordAnalyzer {
    pub fn new() -> Self {
        Self::with_config(BenfordConfig::default())
    }

    pub fn with_config(config: BenfordConfig) -> Self {
        // Benford's Law: P(d) = log10(1 + 1/d)
        let expected = std::array::from_fn(|i| (1.0 + 1.0 / (i + 1) as f64).log10());

        Self { config, expected }
    }

    pub fn analyze(&self, image: &DynamicImage) -> Result<BenfordAnalysisResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);
        let (width, height) = gray.dimensions();

        if width < self.config.block_size || height < self.config.block_size {
            return Err(crate::error::ForensicsError::ImageTooSmall(
                self.config.block_size,
            ));
        }

        let global_coefficients = self.extract_dct_coefficients(&gray);
        let global_distribution = self.compute_first_digit_distribution(&global_coefficients);
        let global_chi_square = self.compute_chi_square(&global_distribution);

        let (deviation_map, block_chi_squares) = self.analyze_blocks(&gray);

        let anomalous_regions = self.find_anomalous_regions(width, height, &block_chi_squares);

        let conformity_score = self.calculate_conformity_score(global_chi_square);

        let manipulation_probability = self.calculate_manipulation_probability(
            global_chi_square,
            &anomalous_regions,
            width,
            height,
        );

        Ok(BenfordAnalysisResult {
            global_distribution,
            expected_distribution: self.expected,
            global_chi_square,
            deviation_map,
            anomalous_regions,
            conformity_score,
            manipulation_probability,
        })
    }

    fn extract_dct_coefficients(&self, gray: &GrayImage) -> Vec<f64> {
        let (width, height) = gray.dimensions();
        let mut coefficients = Vec::new();

        for by in (0..height - 7).step_by(8) {
            for bx in (0..width - 7).step_by(8) {
                let block_coeffs = self.compute_block_dct(gray, bx, by);

                for coeff in block_coeffs.iter().skip(1) {
                    if coeff.abs() >= 1.0 {
                        coefficients.push(*coeff);
                    }
                }
            }
        }

        coefficients
    }

    fn compute_block_dct(&self, gray: &GrayImage, bx: u32, by: u32) -> Vec<f64> {
        let mut block = [[0.0f64; 8]; 8];

        for y in 0..8 {
            for x in 0..8 {
                block[y][x] = gray.get_pixel(bx + x as u32, by + y as u32)[0] as f64 - 128.0;
            }
        }

        let mut coeffs = Vec::with_capacity(64);

        for u in 0..8 {
            for v in 0..8 {
                let cu = if u == 0 { 1.0 / 2.0_f64.sqrt() } else { 1.0 };
                let cv = if v == 0 { 1.0 / 2.0_f64.sqrt() } else { 1.0 };

                let mut sum = 0.0;
                for y in 0..8 {
                    for x in 0..8 {
                        sum += block[y][x]
                            * (PI * (2.0 * x as f64 + 1.0) * u as f64 / 16.0).cos()
                            * (PI * (2.0 * y as f64 + 1.0) * v as f64 / 16.0).cos();
                    }
                }

                coeffs.push(0.25 * cu * cv * sum);
            }
        }

        coeffs
    }

    fn compute_first_digit_distribution(&self, coefficients: &[f64]) -> [f64; 9] {
        let mut counts = [0u32; 9];
        let mut total = 0u32;

        for &coeff in coefficients {
            if let Some(first_digit) = self.get_first_digit(coeff.abs()) {
                if first_digit >= 1 && first_digit <= 9 {
                    counts[first_digit as usize - 1] += 1;
                    total += 1;
                }
            }
        }

        let mut distribution = [0.0f64; 9];
        if total > 0 {
            for i in 0..9 {
                distribution[i] = counts[i] as f64 / total as f64;
            }
        }

        distribution
    }

    fn get_first_digit(&self, value: f64) -> Option<u8> {
        if value < 1.0 {
            return None;
        }

        let mut v = value;
        while v >= 10.0 {
            v /= 10.0;
        }

        Some(v as u8)
    }

    fn compute_chi_square(&self, observed: &[f64; 9]) -> f64 {
        let mut chi_square = 0.0;

        for i in 0..9 {
            let expected = self.expected[i];
            let observed_val = observed[i];

            if expected > 0.0 {
                chi_square += (observed_val - expected).powi(2) / expected;
            }
        }

        chi_square
    }

    fn analyze_blocks(&self, gray: &GrayImage) -> (GrayImage, Vec<(u32, u32, f64)>) {
        let (width, height) = gray.dimensions();
        let block_size = self.config.block_size;
        let mut deviation_map = GrayImage::new(width, height);
        let mut block_chi_squares = Vec::new();

        for by in (0..height - block_size).step_by(block_size as usize / 2) {
            for bx in (0..width - block_size).step_by(block_size as usize / 2) {
                let chi_square = self.analyze_single_block(gray, bx, by, block_size);
                block_chi_squares.push((bx, by, chi_square));

                let normalized = ((chi_square / 50.0).min(1.0) * 255.0) as u8;

                for y in by..(by + block_size).min(height) {
                    for x in bx..(bx + block_size).min(width) {
                        let current = deviation_map.get_pixel(x, y)[0];
                        deviation_map.put_pixel(x, y, Luma([current.max(normalized)]));
                    }
                }
            }
        }

        (deviation_map, block_chi_squares)
    }

    fn analyze_single_block(&self, gray: &GrayImage, bx: u32, by: u32, size: u32) -> f64 {
        let mut coefficients = Vec::new();

        for y in (by..by + size - 7).step_by(8) {
            for x in (bx..bx + size - 7).step_by(8) {
                let block_coeffs = self.compute_block_dct(gray, x, y);
                for coeff in block_coeffs.iter().skip(1) {
                    if coeff.abs() >= 1.0 {
                        coefficients.push(*coeff);
                    }
                }
            }
        }

        if coefficients.len() < self.config.min_samples {
            return 0.0;
        }

        let distribution = self.compute_first_digit_distribution(&coefficients);

        self.compute_chi_square(&distribution)
    }

    fn find_anomalous_regions(
        &self,
        width: u32,
        height: u32,
        block_chi_squares: &[(u32, u32, f64)],
    ) -> Vec<SRegion> {
        let block_size = self.config.block_size;

        let regions = block_chi_squares
            .iter()
            .filter(|(_, _, chi)| *chi > self.config.chi_square_threshold)
            .map(|(x, y, _)| SRegion {
                x: *x,
                y: *y,
                width: block_size.min(width - x),
                height: block_size.min(height - y),
            })
            .collect::<Vec<_>>();

        self.merge_regions(regions)
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
        let y2 = (a.y + a.width).max(b.y + b.height);

        SRegion {
            x,
            y,
            width: x2 - x,
            height: y2 - y,
        }
    }

    fn calculate_conformity_score(&self, chi_square: f64) -> f64 {
        (1.0 - chi_square / 30.0).max(0.0).min(1.0)
    }

    fn calculate_manipulation_probability(
        &self,
        global_chi_square: f64,
        anomalous_regions: &[SRegion],
        width: u32,
        height: u32,
    ) -> f64 {
        let total_pixels = (width * height) as f64;

        let anomalous_pixels = anomalous_regions
            .iter()
            .map(|r| r.width * r.height)
            .sum::<u32>();

        let coverage = anomalous_pixels as f64 / total_pixels;

        let global_factor = (global_chi_square / 30.0).min(1.0);
        let local_factor = coverage * 2.0;

        (global_factor * 0.5 + local_factor * 0.5).min(1.0)
    }
}

impl Default for BenfordAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
