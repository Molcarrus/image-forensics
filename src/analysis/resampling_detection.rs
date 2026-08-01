use std::f64::consts::PI;

use image::{DynamicImage, GrayImage, Luma};

use crate::{
    SRegion,
    error::Result,
    image_utils::{clipped_blocks, ensure_min_dimensions, full_blocks, rgb_to_gray},
};

#[derive(Debug, Clone)]
pub struct ResamplingConfig {
    pub block_size: u32,
    pub window_size: u32,
    pub threshold: f64,
    pub min_factor: f64,
    pub max_factor: f64,
}

impl Default for ResamplingConfig {
    fn default() -> Self {
        Self {
            block_size: 64,
            window_size: 16,
            threshold: 0.3,
            min_factor: 0.5,
            max_factor: 2.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResamplingResult {
    pub probability_map: GrayImage,
    pub periodic_patterns: Vec<PeriodicPattern>,
    pub estimated_factor: Option<f64>,
    pub resampling_probability: f64,
    pub resampled_regions: Vec<SRegion>,
    pub p_map: GrayImage,
}

#[derive(Debug, Clone)]
pub struct PeriodicPattern {
    pub period: f64,
    pub strength: f64,
    pub direction: f64, // 0 = horizontal, PI/2 = vertical
}

pub struct ResamplingDetector {
    config: ResamplingConfig,
}

impl ResamplingDetector {
    pub fn new() -> Self {
        Self::with_config(ResamplingConfig::default())
    }

    pub fn with_config(config: ResamplingConfig) -> Self {
        Self { config }
    }

    pub fn detect(&self, image: &DynamicImage) -> Result<ResamplingResult> {
        let gray = rgb_to_gray(&image.to_rgb8());
        let (width, height) = gray.dimensions();

        ensure_min_dimensions(width, height, self.config.block_size * 2)?;

        let p_map = self.compute_p_map(&gray);

        let periodic_patterns = self.detect_periodic_patterns(&p_map);

        let estimated_factor = self.estimate_resampling_factor(&periodic_patterns);

        let probability_map = self.create_probability_map(&p_map);

        let resampled_regions = self.find_resampled_regions(&probability_map);

        let resampling_probability = self.calculate_resampling_probability(
            &periodic_patterns,
            &resampled_regions,
            width,
            height,
        );

        Ok(ResamplingResult {
            probability_map,
            periodic_patterns,
            estimated_factor,
            resampling_probability,
            resampled_regions,
            p_map,
        })
    }

    fn compute_p_map(&self, gray: &GrayImage) -> GrayImage {
        let (width, height) = gray.dimensions();
        let mut p_map = GrayImage::new(width, height);

        if width < 3 || height < 3 {
            return p_map;
        }

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let d2x = gray.get_pixel(x - 1, y)[0] as f64 - 2.0 * gray.get_pixel(x, y)[0] as f64
                    + gray.get_pixel(x + 1, y)[0] as f64;

                let d2y = gray.get_pixel(x, y - 1)[0] as f64 - 2.0 * gray.get_pixel(x, y)[0] as f64
                    + gray.get_pixel(x, y + 1)[0] as f64;

                let magnitude = (d2x.abs() + d2y.abs()) / 2.0;
                let value = (magnitude.min(255.0)) as u8;

                p_map.put_pixel(x, y, Luma([value]));
            }
        }

        p_map
    }

    fn detect_periodic_patterns(&self, p_map: &GrayImage) -> Vec<PeriodicPattern> {
        let (_width, _height) = p_map.dimensions();
        let mut patterns = Vec::new();

        let h_autocorr = self.compute_autocorrelation(p_map, true);
        if let Some((period, strength)) = self.find_period(&h_autocorr)
            && strength > self.config.threshold {
                patterns.push(PeriodicPattern {
                    period,
                    strength,
                    direction: 0.0,
                });
            }

        let v_autocorr = self.compute_autocorrelation(p_map, false);
        if let Some((period, strength)) = self.find_period(&v_autocorr)
            && strength > self.config.threshold {
                patterns.push(PeriodicPattern {
                    period,
                    strength,
                    direction: PI / 2.0,
                });
            }

        patterns
    }

    fn compute_autocorrelation(&self, p_map: &GrayImage, horizontal: bool) -> Vec<f64> {
        let (width, height) = p_map.dimensions();
        let max_lag = self.config.window_size as usize;
        let mut autocorr = vec![0.0; max_lag];

        let step = 4;
        let mut count = 0;

        if horizontal {
            for y in (0..height).step_by(step) {
                let line = (0..width)
                    .map(|x| p_map.get_pixel(x, y)[0] as f64)
                    .collect::<Vec<_>>();

                let line_autocorr = self.line_autocorrelation(&line, max_lag);
                for i in 0..max_lag {
                    autocorr[i] += line_autocorr[i];
                }
                count += 1;
            }
        } else {
            for x in (0..width).step_by(step) {
                let line = (0..height)
                    .map(|y| p_map.get_pixel(x, y)[0] as f64)
                    .collect::<Vec<_>>();

                let line_autocorr = self.line_autocorrelation(&line, max_lag);
                for i in 0..max_lag {
                    autocorr[i] += line_autocorr[i];
                }
                count += 1;
            }
        }

        if count > 0 {
            for val in &mut autocorr {
                *val /= count as f64;
            }
        }

        autocorr
    }

    fn line_autocorrelation(&self, line: &[f64], max_lag: usize) -> Vec<f64> {
        let n = line.len();
        let mean = line.iter().sum::<f64>() / n as f64;
        let variance = line.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;

        let mut autocorr = vec![0.0; max_lag];

        if variance < 1e-10 {
            return autocorr;
        }

        for lag in 1..max_lag.min(n) {
            let mut sum = 0.0;
            for i in 0..n - lag {
                sum += (line[i] - mean) * (line[i + lag] - mean);
            }
            autocorr[lag] = sum / ((n - lag) as f64 * variance);
        }

        autocorr
    }

    fn find_period(&self, autocorr: &[f64]) -> Option<(f64, f64)> {
        if autocorr.len() < 4 {
            return None;
        }

        let mut best_peak = 0.0;
        let mut best_period = 0.0;

        for i in 2..autocorr.len() - 1 {
            if autocorr[i] > autocorr[i - 1] && autocorr[i] > autocorr[i + 1]
                && autocorr[i] > best_peak {
                    best_peak = autocorr[i];
                    best_period = i as f64;
                }
        }

        if best_peak > 0.1 {
            Some((best_period, best_peak))
        } else {
            None
        }
    }

    /// Recover the scaling factor implied by the strongest periodic peak.
    ///
    /// For a resampling by `p/q` the interpolation residual repeats every `q`
    /// output samples, so a period of `q` corresponds to a factor of
    /// `q / (q - 1)` for upsampling and its reciprocal for downsampling. The
    /// previous version returned the raw autocorrelation lag as if it were the
    /// factor, and never consulted `min_factor`/`max_factor` at all.
    fn estimate_resampling_factor(&self, patterns: &[PeriodicPattern]) -> Option<f64> {
        let best = patterns.iter().max_by(|a, b| {
            a.strength
                .partial_cmp(&b.strength)
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;

        if best.period < 2.0 {
            return None;
        }

        let upsample = best.period / (best.period - 1.0);
        let downsample = 1.0 / upsample;

        // Report whichever interpretation lands inside the configured range.
        for candidate in [upsample, downsample] {
            if (self.config.min_factor..=self.config.max_factor).contains(&candidate) {
                return Some(candidate);
            }
        }

        None
    }

    fn create_probability_map(&self, p_map: &GrayImage) -> GrayImage {
        let (width, height) = p_map.dimensions();
        let block_size = self.config.block_size;
        let mut prob_map = GrayImage::new(width, height);

        for region in full_blocks(width, height, block_size, (block_size / 2).max(1)) {
            let local_prob = self.analyze_local_periodicity(p_map, region.x, region.y, block_size);
            let value = (local_prob * 255.0) as u8;

            for (x, y) in region.clamp_to(width, height).pixels() {
                let existing = prob_map.get_pixel(x, y)[0];
                prob_map.put_pixel(x, y, Luma([existing.max(value)]));
            }
        }

        prob_map
    }

    fn analyze_local_periodicity(&self, p_map: &GrayImage, bx: u32, by: u32, size: u32) -> f64 {
        let (width, height) = p_map.dimensions();

        let mid_y = by + size / 2;
        if mid_y >= height {
            return 0.0;
        }

        let line = (bx..(bx + size).min(width))
            .map(|x| p_map.get_pixel(x, mid_y)[0] as f64)
            .collect::<Vec<_>>();

        if line.len() < 4 {
            return 0.0;
        }

        let max_lag = (line.len() / 2).min(self.config.window_size as usize);
        let autocorr = self.line_autocorrelation(&line, max_lag);

        let max_peak = autocorr.iter().skip(2).cloned().fold(0.0_f64, f64::max);

        max_peak.clamp(0.0, 1.0)
    }

    fn find_resampled_regions(&self, prob_map: &GrayImage) -> Vec<SRegion> {
        let (width, height) = prob_map.dimensions();
        let block_size = self.config.block_size;
        let threshold = (self.config.threshold * 255.0) as u8;

        clipped_blocks(width, height, block_size, block_size)
            .filter(|block| {
                let sum: u64 = block
                    .pixels()
                    .map(|(x, y)| prob_map.get_pixel(x, y)[0] as u64)
                    .sum();

                (sum / block.area()) as u8 > threshold
            })
            .collect()
    }

    fn calculate_resampling_probability(
        &self,
        patterns: &[PeriodicPattern],
        regions: &[SRegion],
        width: u32,
        height: u32,
    ) -> f64 {
        let mut probability = 0.0;

        if !patterns.is_empty() {
            let max_strength = patterns.iter().map(|p| p.strength).fold(0.0, f64::max);
            probability += max_strength * 0.5;
        }

        let total_pixels = width as f64 * height as f64;
        let region_pixels: u64 = regions.iter().map(|r| r.area()).sum();

        if total_pixels > 0.0 {
            probability += (region_pixels as f64 / total_pixels) * 0.5;
        }

        probability.clamp(0.0, 1.0)
    }
}

impl Default for ResamplingDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    use super::*;

    fn checkerboard(width: u32, height: u32, cell: u32) -> RgbImage {
        let mut image = RgbImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let on = ((x / cell) + (y / cell)).is_multiple_of(2);
            let v = if on { 220 } else { 40 };
            *pixel = Rgb([v, v, v]);
        }
        image
    }

    #[test]
    fn undersized_images_error() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(64, 64));
        assert!(ResamplingDetector::new().detect(&image).is_err());
    }

    #[test]
    fn estimated_factor_respects_the_configured_range() {
        let detector = ResamplingDetector::new();
        let image = DynamicImage::ImageRgb8(checkerboard(256, 256, 3));
        let result = detector.detect(&image).unwrap();

        if let Some(factor) = result.estimated_factor {
            assert!(
                (detector.config.min_factor..=detector.config.max_factor).contains(&factor),
                "factor {factor} outside the configured bounds"
            );
        }
    }

    #[test]
    fn probability_is_bounded() {
        let image = DynamicImage::ImageRgb8(checkerboard(256, 192, 5));
        let result = ResamplingDetector::new().detect(&image).unwrap();

        assert!((0.0..=1.0).contains(&result.resampling_probability));
    }

    #[test]
    fn regions_stay_within_the_image() {
        let image = DynamicImage::ImageRgb8(checkerboard(200, 150, 4));
        let result = ResamplingDetector::new().detect(&image).unwrap();

        for region in &result.resampled_regions {
            assert!(region.right() <= 200, "{region:?}");
            assert!(region.bottom() <= 150, "{region:?}");
        }
    }
}
