use image::{DynamicImage, GrayImage, Luma, RgbImage};

use crate::{SRegion, error::Result, image_utils::rgb_to_gray};

#[derive(Debug, Clone)]
pub struct PrnuConfig {
    pub block_size: u32,
    pub wavelet_levels: usize,
    pub correlation_threshold: f64,
    pub min_variance: f64,
    pub denoise_sigma: f64,
}

impl Default for PrnuConfig {
    fn default() -> Self {
        Self {
            block_size: 64,
            wavelet_levels: 4,
            correlation_threshold: 0.7,
            min_variance: 10.0,
            denoise_sigma: 3.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrnuAnalysisResult {
    pub prnu_pattern: GrayImage,
    pub correlation_map: GrayImage,
    pub inconsistent_regions: Vec<SRegion>,
    pub consistency_score: f64,
    pub manipulation_probability: f64,
    pub block_correlations: Vec<f64>,
    pub prnu_statistics: PrnuStatistics,
}

#[derive(Debug, Clone)]
pub struct PrnuStatistics {
    pub mean: f64,
    pub std_dev: f64,
    pub skewness: f64,
    pub kurtosis: f64,
    pub energy: f64,
}

pub struct PrnuAnalyzer {
    config: PrnuConfig,
}

impl PrnuAnalyzer {
    pub fn new() -> Self {
        Self::with_config(PrnuConfig::default())
    }

    pub fn with_config(config: PrnuConfig) -> Self {
        Self { config }
    }

    pub fn analyze(&self, image: &DynamicImage) -> Result<PrnuAnalysisResult> {
        let rgb = image.to_rgb8();
        let (width, height) = rgb.dimensions();

        if width < self.config.block_size * 2 || height < self.config.block_size * 2 {
            return Err(crate::error::ForensicsError::ImageTooSmall(
                self.config.block_size * 2,
            ));
        }

        let prnu_pattern = self.extract_prnu(&rgb)?;

        let prnu_statistics = self.calculate_prnu_statistics(&prnu_pattern);

        let (correlation_map, block_correlations) = self.analyze_local_consistency(&prnu_pattern);

        let inconsistent_regions =
            self.find_incosistent_regions(&correlation_map, &block_correlations);

        let consistency_score = self.calculate_consistency_score(&block_correlations);

        let manipulation_probability = 1.0 - consistency_score;

        Ok(PrnuAnalysisResult {
            prnu_pattern,
            correlation_map,
            inconsistent_regions,
            consistency_score,
            manipulation_probability,
            block_correlations,
            prnu_statistics,
        })
    }

    fn extract_prnu(&self, rgb: &RgbImage) -> Result<GrayImage> {
        let (width, height) = rgb.dimensions();

        let gray = rgb_to_gray(rgb);

        let denoised = self.denoise_image(&gray);

        let mut prnu = GrayImage::new(width, height);

        for y in 0..height {
            for x in 0..width {
                let original = gray.get_pixel(x, y)[0] as f64;
                let clean = denoised.get_pixel(x, y)[0] as f64;

                let noise = original - clean;

                let normalized = (noise + 128.0).clamp(0.0, 255.0) as u8;
                prnu.put_pixel(x, y, Luma([normalized]));
            }
        }

        let enhanced = self.weiner_filter(&prnu, &gray);

        Ok(enhanced)
    }

    fn denoise_image(&self, gray: &GrayImage) -> GrayImage {
        let (width, height) = gray.dimensions();
        let mut result = gray.clone();

        for _ in 0..self.config.wavelet_levels {
            result = self.bilateral_filter(&result);
        }

        result
    }

    fn bilateral_filter(&self, image: &GrayImage) -> GrayImage {
        let (width, height) = image.dimensions();
        let mut result = GrayImage::new(width, height);
        let radius = 2i32;
        let sigma_space = 2.0;
        let sigma_color = 30.0;

        for y in 0..height {
            for x in 0..width {
                let center = image.get_pixel(x, y)[0] as f64;
                let mut sum = 0.0;
                let mut weight_sum = 0.0;

                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;

                        if nx >= 0 && nx < width as i32 && ny >= 0 && ny < height as i32 {
                            let neighbor = image.get_pixel(nx as u32, ny as u32)[0] as f64;

                            let space_weight = (-(dx * dx + dy * dy) as f64
                                / (2.0 * sigma_space * sigma_space))
                                .exp();

                            let color_diff = center - neighbor;
                            let color_weight = (-color_diff * color_diff
                                / (2.0 * sigma_color * sigma_color))
                                .exp();

                            let weight = space_weight * color_weight;
                            sum += neighbor * weight;
                            weight_sum += weight;
                        }
                    }
                }

                let filtered = if weight_sum > 0.0 {
                    sum / weight_sum
                } else {
                    center
                };
                result.put_pixel(x, y, Luma([filtered.clamp(0.0, 250.0) as u8]));
            }
        }

        result
    }

    fn weiner_filter(&self, noise: &GrayImage, original: &GrayImage) -> GrayImage {
        let (width, height) = noise.dimensions();
        let mut result = GrayImage::new(width, height);

        let block_size = 8;

        for y in 0..height {
            for x in 0..width {
                let (local_mean, local_var) =
                    self.calculate_local_stats(original, x, y, block_size);

                let noise_val = noise.get_pixel(x, y)[0] as f64 - 128.0;
                let orig_val = original.get_pixel(x, y)[0] as f64;

                let noise_var = self.config.denoise_sigma * self.config.denoise_sigma;
                let signal_var = (local_var - noise_var).max(0.0);

                let weiner_weight = if local_var > 0.0 {
                    signal_var / local_var
                } else {
                    0.0
                };

                let intensity_weight = if orig_val > 10.0 && orig_val < 245.0 {
                    1.0
                } else {
                    0.5
                };

                let filtered = noise_val * weiner_weight * intensity_weight;
                let normalized = (filtered + 128.0).clamp(0.0, 255.0) as u8;

                result.put_pixel(x, y, Luma([normalized]));
            }
        }

        result
    }

    fn calculate_local_stats(&self, image: &GrayImage, cx: u32, cy: u32, size: u32) -> (f64, f64) {
        let (width, height) = image.dimensions();
        let half = size / 2;

        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        let mut count = 0;

        for dy in 0..size {
            for dx in 0..size {
                let x = (cx + dx).saturating_sub(half);
                let y = (cy + dy).saturating_sub(half);

                if x < width && y < height {
                    let val = image.get_pixel(x, y)[0] as f64;
                    sum += val;
                    sum_sq += val * val;
                    count += 1;
                }
            }
        }

        if count == 0 {
            return (0.0, 0.0);
        }

        let mean = sum / count as f64;
        let variance = (sum_sq / count as f64) - (mean * mean);

        (mean, variance.max(0.0))
    }

    fn calculate_prnu_statistics(&self, prnu: &GrayImage) -> PrnuStatistics {
        let values = prnu
            .pixels()
            .map(|p| p[0] as f64 - 128.0)
            .collect::<Vec<_>>();

        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;

        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        let skewness = if std_dev > 0.0 {
            values
                .iter()
                .map(|v| ((v - mean) / std_dev).powi(3))
                .sum::<f64>()
                / n
        } else {
            0.0
        };

        let kurtosis = if std_dev > 0.0 {
            values
                .iter()
                .map(|v| ((v - mean) / std_dev).powi(4))
                .sum::<f64>()
                / n
                - 3.0
        } else {
            0.0
        };

        let energy = values.iter().map(|v| v * v).sum::<f64>() / n;

        PrnuStatistics {
            mean,
            std_dev,
            skewness,
            kurtosis,
            energy,
        }
    }

    fn analyze_local_consistency(&self, prnu: &GrayImage) -> (GrayImage, Vec<f64>) {
        let (width, height) = prnu.dimensions();
        let block_size = self.config.block_size;

        let mut correlation_map = GrayImage::new(width, height);
        let mut block_correlations = Vec::new();

        let global_mean =
            prnu.pixels().map(|p| p[0] as f64 - 128.0).sum::<f64>() / (width * height) as f64;

        let global_var = prnu
            .pixels()
            .map(|p| {
                let v = p[0] as f64 - 128.0 - global_mean;
                v * v
            })
            .sum::<f64>()
            / (width * height) as f64;

        for by in (0..height).step_by(block_size as usize) {
            for bx in (0..width).step_by(block_size as usize) {
                let block_w = block_size.min(width - bx);
                let block_h = block_size.min(height - by);

                let mut block_values = Vec::new();
                for dy in 0..block_h {
                    for dx in 0..block_w {
                        let val = prnu.get_pixel(bx + dx, by + dy)[0] as f64 - 128.0;
                        block_values.push(val);
                    }
                }

                if block_values.is_empty() {
                    continue;
                }

                let block_mean = block_values.iter().sum::<f64>() / block_values.len() as f64;
                let block_var = block_values
                    .iter()
                    .map(|v| (v - block_mean).powi(2))
                    .sum::<f64>()
                    / block_values.len() as f64;

                let correlation = if global_var > 0.0 && block_var > self.config.min_variance {
                    let covariance = block_values
                        .iter()
                        .map(|v| (v - block_mean) * (v - global_mean))
                        .sum::<f64>()
                        / block_values.len() as f64;

                    (covariance / (global_var.sqrt() * block_var.sqrt())).abs()
                } else {
                    0.5
                };

                block_correlations.push(correlation);

                let corr_value = (correlation * 255.0) as u8;
                for dy in 0..block_h {
                    for dx in 0..block_w {
                        correlation_map.put_pixel(bx + dx, by + dy, Luma([corr_value]));
                    }
                }
            }
        }

        (correlation_map, block_correlations)
    }

    fn find_incosistent_regions(
        &self,
        correlation_map: &GrayImage,
        correlations: &[f64],
    ) -> Vec<SRegion> {
        let (width, height) = correlation_map.dimensions();
        let block_size = self.config.block_size;
        let blocks_x = (width + block_size - 1) / block_size;

        let mut regions = Vec::new();

        if correlations.is_empty() {
            return regions;
        }

        let mean_corr = correlations.iter().sum::<f64>() / correlations.len() as f64;
        let var_corr = correlations
            .iter()
            .map(|c| (c - mean_corr).powi(2))
            .sum::<f64>()
            / correlations.len() as f64;
        let std_corr = var_corr.sqrt();

        let threshold = (mean_corr - 2.0 * std_corr).max(self.config.correlation_threshold);

        for (idx, &corr) in correlations.iter().enumerate() {
            if corr < threshold {
                let bx = (idx as u32 % blocks_x) * block_size;
                let by = (idx as u32 / blocks_x) * block_size;

                regions.push(SRegion {
                    x: bx,
                    y: by,
                    width: block_size.min(width - bx),
                    height: block_size.min(height - by),
                });
            }
        }

        self.merge_adjacent_regions(regions)
    }

    fn merge_adjacent_regions(&self, regions: Vec<SRegion>) -> Vec<SRegion> {
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

    fn calculate_consistency_score(&self, correlations: &[f64]) -> f64 {
        if correlations.is_empty() {
            return 1.0;
        }

        let valid_correlations = correlations
            .iter()
            .filter(|&&c| c > 0.1)
            .copied()
            .collect::<Vec<_>>();

        if valid_correlations.is_empty() {
            return 1.0;
        }

        let mean = valid_correlations.iter().sum::<f64>() / valid_correlations.len() as f64;

        let low_count = valid_correlations
            .iter()
            .filter(|&&c| c < self.config.correlation_threshold)
            .count();

        let low_ratio = low_count as f64 / valid_correlations.len() as f64;

        (mean * (1.0 - low_ratio * 0.5)).max(0.0).min(1.0)
    }

    pub fn compare_patterns(&self, pattern1: &GrayImage, pattern2: &GrayImage) -> f64 {
        let (w1, h1) = pattern1.dimensions();
        let (w2, h2) = pattern2.dimensions();

        let width = w1.min(w2);
        let height = h1.min(h2);

        let mut sum1 = 0.0;
        let mut sum2 = 0.0;
        let n = (width * height) as f64;

        for y in 0..height {
            for x in 0..height {
                sum1 += pattern1.get_pixel(x, y)[0] as f64 - 128.0;
                sum2 += pattern2.get_pixel(x, y)[0] as f64 - 128.0;
            }
        }

        let mean1 = sum1 / n;
        let mean2 = sum2 / n;

        let mut numerator = 0.0;
        let mut denom1 = 0.0;
        let mut denom2 = 0.0;

        for y in 0..height {
            for x in 0..width {
                let v1 = pattern1.get_pixel(x, y)[0] as f64 - 128.0 - mean1;
                let v2 = pattern2.get_pixel(x, y)[0] as f64 - 128.0 - mean2;

                numerator += v1 * v2;
                denom1 += v1 * v1;
                denom2 += v2 * v2;
            }
        }

        let denom = (denom1 * denom2).sqrt();

        if denom > 0.0 {
            (numerator / denom).max(-1.0).min(1.0)
        } else {
            0.0
        }
    }
}

impl Default for PrnuAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
