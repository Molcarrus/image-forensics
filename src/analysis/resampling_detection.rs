use std::f64::consts::PI;

use image::{DynamicImage, GrayImage, Luma};

use crate::{SRegion, error::Result, image_utils::rgb_to_gray};

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

        if width < self.config.block_size * 2 || height < self.config.block_size * 2 {
            return Err(crate::error::ForensicsError::ImageTooSmall(
                self.config.block_size * 2,
            ));
        }

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

        for y in 2..height - 2 {
            for x in 2..width - 2 {
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
        let (width, height) = p_map.dimensions();
        let mut patterns = Vec::new();

        let h_autocorr = self.compute_autocorrelation(p_map, true);
        if let Some((period, strength)) = self.find_period(&h_autocorr) {
            if strength > self.config.threshold {
                patterns.push(PeriodicPattern {
                    period,
                    strength,
                    direction: 0.0,
                });
            }
        }

        let v_autocorr = self.compute_autocorrelation(p_map, false);
        if let Some((period, strength)) = self.find_period(&v_autocorr) {
            if strength > self.config.threshold {
                patterns.push(PeriodicPattern {
                    period,
                    strength,
                    direction: PI / 2.0,
                });
            }
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
        if autocorr.len() < 3 {
            return None;
        }

        let mut best_peak = 0.0;
        let mut best_period = 0.0;

        for i in 2..autocorr.len() - 1 {
            if autocorr[i] > autocorr[i - 1] && autocorr[i] > autocorr[i + 1] {
                if autocorr[i] > best_peak {
                    best_peak = autocorr[i];
                    best_period = i as f64;
                }
            }
        }

        if best_peak > 0.1 {
            Some((best_period, best_peak))
        } else {
            None
        }
    }

    fn estimate_resampling_factor(&self, patterns: &[PeriodicPattern]) -> Option<f64> {
        if patterns.is_empty() {
            return None;
        }

        let best_pattern = patterns
            .iter()
            .max_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap())?;

        if best_pattern.period >= self.config.min_factor
            && best_pattern.period <= self.config.max_factor
        {
            Some(best_pattern.period)
        } else {
            None
        }
    }

    fn create_probability_map(&self, p_map: &GrayImage) -> GrayImage {
        let (width, height) = p_map.dimensions();
        let block_size = self.config.block_size;
        let mut prob_map = GrayImage::new(width, height);

        for by in (0..height - block_size).step_by(block_size as usize / 2) {
            for bx in (0..width - block_size).step_by(block_size as usize / 2) {
                let local_prob = self.analyze_local_periodicity(p_map, bx, by, block_size);
                let value = (local_prob * 255.0) as u8;

                for y in by..(by + block_size).min(height) {
                    for x in bx..(bx + block_size).min(width) {
                        prob_map.put_pixel(x, y, Luma([value]));
                    }
                }
            }
        }

        prob_map
    }

    fn analyze_local_periodicity(&self, p_map: &GrayImage, bx: u32, by: u32, size: u32) -> f64 {
        let (width, height) = p_map.dimensions();

        let mut values = Vec::new();
        for y in by..(by + size).min(height) {
            for x in bx..(bx + size).min(width) {
                values.push(p_map.get_pixel(x, y)[0] as f64);
            }
        }

        if values.is_empty() {
            return 0.0;
        }

        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;

        (variance / 1000.0).min(1.0)
    }

    fn find_resampled_regions(&self, prob_map: &GrayImage) -> Vec<SRegion> {
        let (width, height) = prob_map.dimensions();
        let block_size = self.config.block_size;
        let threshold = (self.config.threshold * 255.0) as u8;

        let mut regions = Vec::new();

        for by in (0..height).step_by(block_size as usize) {
            for bx in (0..width).step_by(block_size as usize) {
                let block_w = block_size.min(width - bx);
                let block_h = block_size.min(height - by);

                let mut sum = 0u32;
                let mut count = 0u32;

                for y in by..(by + block_h) {
                    for x in bx..(bx + block_w) {
                        sum += prob_map.get_pixel(x, y)[0] as u32;
                        count += 1;
                    }
                }

                let avg = (sum / count) as u8;

                if avg > threshold {
                    regions.push(SRegion {
                        x: bx,
                        y: by,
                        width: block_w,
                        height: block_h,
                    });
                }
            }
        }

        regions
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

        let total_pixels = (width * height) as f64;
        let region_pixels = regions.iter().map(|r| r.width * r.height).sum::<u32>();
        let coverage = region_pixels as f64 / total_pixels;
        probability += coverage * 0.5;

        probability.min(1.0)
    }
}

impl Default for ResamplingDetector {
    fn default() -> Self {
        Self::new()
    }
}
