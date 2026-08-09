use std::f64::consts::PI;

use image::{DynamicImage, GrayImage, Luma};

use crate::{
    SRegion,
    error::Result,
    image_utils::{ensure_min_dimensions, full_blocks, rgb_to_gray},
    region::merge_regions,
};

/// Settings for [`BenfordAnalyzer`].
#[derive(Debug, Clone)]
pub struct BenfordConfig {
    /// Tile over which each local chi-square is computed. Default 64.
    pub block_size: u32,
    /// Tiles scoring above this are flagged. Default 15.
    pub chi_square_threshold: f64,
    /// Tiles with fewer usable coefficients score 0 rather than a noisy statistic.
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

/// Output of [`BenfordAnalyzer`].
#[derive(Debug, Clone)]
pub struct BenfordAnalysisResult {
    /// Observed first-digit frequencies across the whole image.
    pub global_distribution: [f64; 9],
    /// Benford's Law frequencies, `log10(1 + 1/d)` for `d` in 1..=9.
    pub expected_distribution: [f64; 9],
    /// Goodness of fit between the two distributions. Lower is closer.
    pub global_chi_square: f64,
    /// Per-tile chi-square, normalised for display.
    pub deviation_map: GrayImage,
    /// Tiles exceeding `chi_square_threshold`, merged.
    pub anomalous_regions: Vec<SRegion>,
    /// How closely the image follows Benford, in `[0, 1]`.
    pub conformity_score: f64,
    /// Combined global and local score, in `[0, 1]`.
    pub manipulation_probability: f64,
}

/// Tests the leading digits of DCT coefficients against Benford's Law.
///
/// The AC coefficients of a JPEG follow `P(d) = log10(1 + 1/d)` closely.
/// Editing, requantisation and synthetic content perturb that distribution.
///
/// # Limitations
///
/// Benford applies to lossy-compressed natural images. It is weak on lossless
/// input, on graphic content, and on images with large flat areas. A departure
/// indicates requantisation, which is not the same as editing.
pub struct BenfordAnalyzer {
    config: BenfordConfig,
    expected: [f64; 9],
}

impl BenfordAnalyzer {
    /// Analyzer with the default configuration.
    pub fn new() -> Self {
        Self::with_config(BenfordConfig::default())
    }

    /// Analyzer with custom settings.
    pub fn with_config(config: BenfordConfig) -> Self {
        // Benford's Law: P(d) = log10(1 + 1/d)
        let expected = std::array::from_fn(|i| (1.0 + 1.0 / (i + 1) as f64).log10());

        Self { config, expected }
    }

    /// Run the analysis.
    ///
    /// # Errors
    ///
    /// [`ImageTooSmall`](crate::error::ForensicsError::ImageTooSmall) below
    /// `block_size` in either dimension.
    pub fn analyze(&self, image: &DynamicImage) -> Result<BenfordAnalysisResult> {
        let gray = rgb_to_gray(&image.to_rgb8());
        let (width, height) = gray.dimensions();

        ensure_min_dimensions(width, height, self.config.block_size)?;

        // Every 8x8 DCT block is computed once here and reused for both the
        // global distribution and the per-block sweep; the two passes
        // previously recomputed overlapping blocks from scratch.
        let dct_blocks = self.compute_dct_grid(&gray);

        let global_coefficients: Vec<f64> = dct_blocks
            .iter()
            .flat_map(|block| block.coefficients.iter().copied())
            .collect();

        let global_distribution = self.first_digit_distribution(&global_coefficients);
        let global_chi_square = self.chi_square(&global_distribution);

        let (deviation_map, block_chi_squares) = self.analyze_blocks(width, height, &dct_blocks);
        let anomalous_regions = self.find_anomalous_regions(width, height, &block_chi_squares);
        let conformity_score = self.conformity_score(global_chi_square);
        let manipulation_probability =
            self.manipulation_probability(global_chi_square, &anomalous_regions, width, height);

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

    fn compute_dct_grid(&self, gray: &GrayImage) -> Vec<DctBlock> {
        let (width, height) = gray.dimensions();

        full_blocks(width, height, 8, 8)
            .map(|region| {
                let coeffs = block_dct_8x8(gray, region.x, region.y);

                DctBlock {
                    x: region.x,
                    y: region.y,
                    // The DC term carries block brightness, not compression
                    // structure, so Benford is applied to the AC terms only.
                    coefficients: coeffs
                        .into_iter()
                        .skip(1)
                        .filter(|c| c.abs() >= 1.0)
                        .collect(),
                }
            })
            .collect()
    }

    fn first_digit_distribution(&self, coefficients: &[f64]) -> [f64; 9] {
        let mut counts = [0u32; 9];
        let mut total = 0u32;

        for &coeff in coefficients {
            if let Some(digit) = first_digit(coeff.abs()) {
                counts[digit as usize - 1] += 1;
                total += 1;
            }
        }

        let mut distribution = [0.0f64; 9];
        if total > 0 {
            for (slot, count) in distribution.iter_mut().zip(counts.iter()) {
                *slot = *count as f64 / total as f64;
            }
        }

        distribution
    }

    fn chi_square(&self, observed: &[f64; 9]) -> f64 {
        observed
            .iter()
            .zip(self.expected.iter())
            .filter(|(_, expected)| **expected > 0.0)
            .map(|(&observed, &expected)| (observed - expected).powi(2) / expected)
            .sum()
    }

    fn analyze_blocks(
        &self,
        width: u32,
        height: u32,
        dct_blocks: &[DctBlock],
    ) -> (GrayImage, Vec<(SRegion, f64)>) {
        let block_size = self.config.block_size;
        let stride = (block_size / 2).max(1);

        let mut deviation_map = GrayImage::new(width, height);
        let mut block_chi_squares = Vec::new();

        for region in full_blocks(width, height, block_size, stride) {
            let coefficients: Vec<f64> = dct_blocks
                .iter()
                .filter(|block| {
                    block.x >= region.x
                        && block.x < region.right()
                        && block.y >= region.y
                        && block.y < region.bottom()
                })
                .flat_map(|block| block.coefficients.iter().copied())
                .collect();

            let chi_square = if coefficients.len() < self.config.min_samples {
                0.0
            } else {
                self.chi_square(&self.first_digit_distribution(&coefficients))
            };

            block_chi_squares.push((region, chi_square));

            let normalized = ((chi_square / 50.0).min(1.0) * 255.0) as u8;
            for (x, y) in region.clamp_to(width, height).pixels() {
                let current = deviation_map.get_pixel(x, y)[0];
                deviation_map.put_pixel(x, y, Luma([current.max(normalized)]));
            }
        }

        (deviation_map, block_chi_squares)
    }

    fn find_anomalous_regions(
        &self,
        width: u32,
        height: u32,
        block_chi_squares: &[(SRegion, f64)],
    ) -> Vec<SRegion> {
        let regions = block_chi_squares
            .iter()
            .filter(|(_, chi)| *chi > self.config.chi_square_threshold)
            .map(|(region, _)| region.clamp_to(width, height))
            .collect();

        merge_regions(regions, self.config.block_size / 2)
    }

    fn conformity_score(&self, chi_square: f64) -> f64 {
        (1.0 - chi_square / 30.0).clamp(0.0, 1.0)
    }

    fn manipulation_probability(
        &self,
        global_chi_square: f64,
        anomalous_regions: &[SRegion],
        width: u32,
        height: u32,
    ) -> f64 {
        let total_pixels = (width as f64) * (height as f64);
        let anomalous_pixels: u64 = anomalous_regions.iter().map(|r| r.area()).sum();

        let coverage = if total_pixels > 0.0 {
            anomalous_pixels as f64 / total_pixels
        } else {
            0.0
        };

        let global_factor = (global_chi_square / 30.0).min(1.0);
        let local_factor = (coverage * 2.0).min(1.0);

        (global_factor * 0.5 + local_factor * 0.5).clamp(0.0, 1.0)
    }
}

struct DctBlock {
    x: u32,
    y: u32,
    coefficients: Vec<f64>,
}

/// Leading significant digit of a magnitude, or `None` below 1.0.
fn first_digit(value: f64) -> Option<u8> {
    if !value.is_finite() || value < 1.0 {
        return None;
    }

    let mut v = value;
    while v >= 10.0 {
        v /= 10.0;
    }

    let digit = v as u8;
    (1..=9).contains(&digit).then_some(digit)
}

/// Naive 8x8 DCT-II with the JPEG level shift applied.
fn block_dct_8x8(gray: &GrayImage, bx: u32, by: u32) -> Vec<f64> {
    let mut block = [[0.0f64; 8]; 8];

    for (y, row) in block.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            *cell = gray.get_pixel(bx + x as u32, by + y as u32)[0] as f64 - 128.0;
        }
    }

    let mut coeffs = Vec::with_capacity(64);

    for u in 0..8 {
        for v in 0..8 {
            let cu = if u == 0 { 1.0 / 2.0_f64.sqrt() } else { 1.0 };
            let cv = if v == 0 { 1.0 / 2.0_f64.sqrt() } else { 1.0 };

            let mut sum = 0.0;
            for (y, row) in block.iter().enumerate() {
                for (x, &cell) in row.iter().enumerate() {
                    sum += cell
                        * (PI * (2.0 * x as f64 + 1.0) * u as f64 / 16.0).cos()
                        * (PI * (2.0 * y as f64 + 1.0) * v as f64 / 16.0).cos();
                }
            }

            coeffs.push(0.25 * cu * cv * sum);
        }
    }

    coeffs
}

impl Default for BenfordAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    use super::*;

    fn gradient(width: u32, height: u32) -> DynamicImage {
        let mut image = RgbImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let v = ((x * 7 + y * 11) % 256) as u8;
            *pixel = Rgb([v, v, v]);
        }
        DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn expected_distribution_sums_to_one() {
        let analyzer = BenfordAnalyzer::new();
        let total: f64 = analyzer.expected.iter().sum();

        assert!((total - 1.0).abs() < 1e-9, "sums to {total}");
        // P(1) = log10(2)
        assert!((analyzer.expected[0] - std::f64::consts::LOG10_2).abs() < 1e-9);
    }

    #[test]
    fn first_digit_extracts_the_leading_significant_digit() {
        assert_eq!(first_digit(1.0), Some(1));
        assert_eq!(first_digit(9.99), Some(9));
        assert_eq!(first_digit(4321.0), Some(4));
        assert_eq!(first_digit(0.5), None);
        assert_eq!(first_digit(f64::NAN), None);
    }

    #[test]
    fn regions_stay_within_the_image() {
        // 150 is not a multiple of the 64px block size. The union helper used
        // to build merged heights from `a.y + a.width`, producing boxes that
        // ran off the bottom of non-square images.
        let result = BenfordAnalyzer::new().analyze(&gradient(150, 200)).unwrap();

        for region in &result.anomalous_regions {
            assert!(region.right() <= 150, "{region:?} overruns the width");
            assert!(region.bottom() <= 200, "{region:?} overruns the height");
        }
    }

    #[test]
    fn undersized_images_error_rather_than_panic() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(32, 32));
        assert!(BenfordAnalyzer::new().analyze(&image).is_err());
    }

    #[test]
    fn probabilities_are_bounded() {
        let result = BenfordAnalyzer::new().analyze(&gradient(128, 128)).unwrap();

        assert!((0.0..=1.0).contains(&result.manipulation_probability));
        assert!((0.0..=1.0).contains(&result.conformity_score));
    }
}
