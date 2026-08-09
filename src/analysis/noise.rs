use image::{DynamicImage, GrayImage, Luma};

use crate::{
    NoiseResult, SRegion,
    error::Result,
    image_utils::{clipped_blocks, gaussian_blur_3x3, median, rgb_to_gray},
};

/// Sensor noise consistency.
///
/// Extracts a noise residual, estimates the global noise floor robustly (median
/// absolute deviation, so a large tampered region cannot drag the baseline
/// towards itself), then flags blocks whose local variance sits far above or
/// below it.
///
/// # Limitations
///
/// Noise is not uniform in real photographs: it rises in shadows and falls in
/// saturated highlights. Modern phones also denoise different parts of a frame
/// differently, and JPEG compression suppresses noise unevenly.
pub struct NoiseAnalyzer {
    block_size: u32,
    sensitivity: f64,
}

impl NoiseAnalyzer {
    /// Analyzer with the default 16px block and sensitivity of 2.
    pub fn new() -> Self {
        Self {
            block_size: 16,
            sensitivity: 2.0,
        }
    }

    /// Window size for local variance, and tile size for the anomaly sweep.
    pub fn with_block_size(mut self, size: u32) -> Self {
        self.block_size = size.max(1);
        self
    }

    /// A block is flagged above `noise * sensitivity` or below
    /// `noise / sensitivity`. Lower is stricter. Clamped to at least 1.
    pub fn with_sensitivity(mut self, sensitivity: f64) -> Self {
        self.sensitivity = sensitivity.max(1.0);
        self
    }

    /// Run the analysis. Accepts any image size.
    pub fn analyze(&self, image: &DynamicImage) -> Result<NoiseResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);

        let noise_map = self.extract_noise(&gray);
        let local_variance_map = self.calculate_local_variance(&gray);
        let estimated_noise_level = self.estimate_noise_level(&noise_map);

        let (anomalous_regions, inconsistency_score) =
            self.find_anomalies(&local_variance_map, estimated_noise_level);

        Ok(NoiseResult {
            noise_map,
            local_variance_map,
            inconsistency_score,
            estimated_noise_level,
            anomalous_regions,
        })
    }

    fn extract_noise(&self, gray: &GrayImage) -> GrayImage {
        let blurred = gaussian_blur_3x3(gray);
        let (width, height) = gray.dimensions();
        let mut noise = GrayImage::new(width, height);

        for y in 0..height {
            for x in 0..width {
                let orig = gray.get_pixel(x, y)[0] as i32;
                let blur = blurred.get_pixel(x, y)[0] as i32;
                noise.put_pixel(x, y, Luma([(orig - blur).unsigned_abs().min(255) as u8]));
            }
        }

        noise
    }

    /// Local standard deviation over a window centred on each pixel.
    fn calculate_local_variance(&self, gray: &GrayImage) -> GrayImage {
        let (width, height) = gray.dimensions();
        let mut variance_map = GrayImage::new(width, height);
        let half_block = (self.block_size / 2) as i32;

        for y in 0..height {
            for x in 0..width {
                let mut sum = 0.0;
                let mut sum_sq = 0.0;
                let mut count = 0u32;

                // Iterate signed offsets so the window stays centred at the
                // borders; `x.saturating_sub(half) + dx` biased it rightwards
                // and downwards along the top and left edges.
                for dy in -half_block..=half_block {
                    for dx in -half_block..=half_block {
                        let px = x as i32 + dx;
                        let py = y as i32 + dy;

                        if px >= 0 && px < width as i32 && py >= 0 && py < height as i32 {
                            let val = gray.get_pixel(px as u32, py as u32)[0] as f64;
                            sum += val;
                            sum_sq += val * val;
                            count += 1;
                        }
                    }
                }

                if count > 0 {
                    let mean = sum / count as f64;
                    let variance = (sum_sq / count as f64 - mean * mean).max(0.0);
                    let std_dev = variance.sqrt().min(255.0);
                    variance_map.put_pixel(x, y, Luma([std_dev as u8]));
                }
            }
        }

        variance_map
    }

    /// Robust noise estimate: median absolute deviation scaled to a Gaussian sigma.
    fn estimate_noise_level(&self, noise_map: &GrayImage) -> f64 {
        let values: Vec<f64> = noise_map.pixels().map(|p| p[0] as f64).collect();

        if values.is_empty() {
            return 0.0;
        }

        let centre = median(&values);
        let deviations: Vec<f64> = values.iter().map(|&v| (v - centre).abs()).collect();

        median(&deviations) * 1.4826
    }

    fn find_anomalies(&self, variance_map: &GrayImage, global_noise: f64) -> (Vec<SRegion>, f64) {
        let (width, height) = variance_map.dimensions();

        let threshold_high = global_noise * self.sensitivity;
        let threshold_low = global_noise / self.sensitivity;

        let mut regions = Vec::new();
        let mut total_blocks = 0u32;

        for block in clipped_blocks(width, height, self.block_size, self.block_size) {
            total_blocks += 1;

            let sum: f64 = block
                .pixels()
                .map(|(x, y)| variance_map.get_pixel(x, y)[0] as f64)
                .sum();
            let block_mean = sum / block.area() as f64;

            if block_mean > threshold_high || block_mean < threshold_low {
                regions.push(block);
            }
        }

        let inconsistency_score = if total_blocks == 0 {
            0.0
        } else {
            regions.len() as f64 / total_blocks as f64
        };

        (regions, inconsistency_score)
    }
}

impl Default for NoiseAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    use super::*;

    #[test]
    fn anomalous_regions_stay_inside_the_image() {
        // 100 is not a multiple of the 16px block size, so the trailing blocks
        // are clipped. A `midpoint` typo used to average the block size with
        // the remaining height and report regions running off the bottom edge.
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(100, 100, Rgb([10, 10, 10])));
        let result = NoiseAnalyzer::new().analyze(&image).unwrap();

        for region in &result.anomalous_regions {
            assert!(region.right() <= 100, "{region:?} overruns the width");
            assert!(region.bottom() <= 100, "{region:?} overruns the height");
        }
    }

    #[test]
    fn inconsistency_score_is_a_ratio() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([128, 128, 128])));
        let result = NoiseAnalyzer::new().analyze(&image).unwrap();

        assert!((0.0..=1.0).contains(&result.inconsistency_score));
    }
}
