use image::{DynamicImage, GrayImage, Luma};

use crate::{
    NoiseResult, SRegion,
    error::Result,
    image_utils::{gaussian_blur_3x3, rgb_to_gray},
};

pub struct NoiseAnalyzer {
    block_size: u32,
    sensitivity: f64,
}

impl NoiseAnalyzer {
    pub fn new() -> Self {
        Self {
            block_size: 16,
            sensitivity: 2.0,
        }
    }

    pub fn with_block_size(mut self, size: u32) -> Self {
        self.block_size = size;
        self
    }

    pub fn analyze(&self, image: &DynamicImage) -> Result<NoiseResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);

        let noise_map = self.extract_noise(&gray);
        let local_variance_map = self.calculate_local_variance(&gray);
        let estimated_noise_level = self.estimate_noise_level(&noise_map);

        let (anomalous_regions, inconsistency_score) =
            self.find_anomlaies(&local_variance_map, estimated_noise_level);

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
                let diff = (orig - blur).abs() as u8;
                noise.put_pixel(x, y, Luma([diff]));
            }
        }

        noise
    }

    fn calculate_local_variance(&self, gray: &GrayImage) -> GrayImage {
        let (width, height) = gray.dimensions();
        let mut variance_map = GrayImage::new(width, height);
        let half_block = self.block_size / 2;

        for y in 0..height {
            for x in 0..width {
                let mut sum = 0.0;
                let mut sum_sq = 0.0;
                let mut count = 0;

                for dy in 0..self.block_size {
                    for dx in 0..self.block_size {
                        let px = x.saturating_sub(half_block) + dx;
                        let py = y.saturating_sub(half_block) + dy;

                        if px < width && py < height {
                            let val = gray.get_pixel(px, py)[0] as f64;
                            sum += val;
                            sum_sq += val * val;
                            count += 1;
                        }
                    }
                }

                if count > 0 {
                    let mean = sum / count as f64;
                    let variance = (sum_sq / count as f64) - (mean * mean);
                    let std_dev = variance.sqrt().min(255.0);
                    variance_map.put_pixel(x, y, Luma([std_dev as u8]));
                }
            }
        }

        variance_map
    }

    fn estimate_noise_level(&self, noise_map: &GrayImage) -> f64 {
        let mut values = noise_map.pixels().map(|p| p[0] as f64).collect::<Vec<_>>();

        values.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let median = if values.len() % 2 == 0 {
            (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
        } else {
            values[values.len() / 2]
        };

        let mut deviations = values
            .iter()
            .map(|&v| (v - median).abs())
            .collect::<Vec<_>>();
        deviations.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let mad = if deviations.len() % 2 == 0 {
            (deviations[deviations.len() / 2 - 1] + deviations[deviations.len() / 2]) / 2.0
        } else {
            deviations[deviations.len() / 2]
        };

        mad * 1.4826
    }

    fn find_anomlaies(&self, variance_map: &GrayImage, global_noise: f64) -> (Vec<SRegion>, f64) {
        let (width, height) = variance_map.dimensions();
        let mut regions = Vec::new();
        let mut anomaly_count = 0;
        let mut total_blocks = 0;

        let threshold_high = global_noise * self.sensitivity;
        let threshold_low = global_noise / self.sensitivity;

        for by in (0..height).step_by(self.block_size as usize) {
            for bx in (0..width).step_by(self.block_size as usize) {
                let mut block_sum = 0.0;
                let mut count = 0;

                for y in by..(by + self.block_size).min(height) {
                    for x in bx..(bx + self.block_size).min(width) {
                        block_sum += variance_map.get_pixel(x, y)[0] as f64;
                        count += 1;
                    }
                }

                let block_mean = block_sum / count as f64;
                total_blocks += 1;

                if block_mean > threshold_high || block_mean < threshold_low {
                    anomaly_count += 1;
                    regions.push(SRegion {
                        x: bx,
                        y: by,
                        width: self.block_size.min(width - bx),
                        height: self.block_size.midpoint(height - by),
                    });
                }
            }
        }
        let inconsistency_score = anomaly_count as f64 / total_blocks as f64;

        (regions, inconsistency_score)
    }
}

impl Default for NoiseAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
