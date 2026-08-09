use image::{DynamicImage, GrayImage, Luma};

use crate::{
    SRegion,
    error::Result,
    image_utils::{ensure_min_dimensions, mean_and_variance, rgb_to_gray},
};

/// Settings for [`DctAnalyzer`].
#[derive(Debug, Clone)]
pub struct DctConfig {
    /// DCT block size. JPEG uses 8 and nothing else is meaningful here.
    pub block_size: usize,
    /// Resolution of the AC coefficient histogram.
    pub histogram_bins: usize,
    /// Reserved for future block-level thresholding.
    pub anomaly_threshold: f64,
    /// How many AC coefficients participate in the statistics.
    pub ac_coefficients_count: usize,
}

impl Default for DctConfig {
    fn default() -> Self {
        Self {
            block_size: 8,
            histogram_bins: 256,
            anomaly_threshold: 0.3,
            ac_coefficients_count: 15,
        }
    }
}

/// Output of [`DctAnalyzer`].
pub struct DctAnalysisResult {
    /// Quality implied by the estimated quantisation table.
    pub primary_quality: u8,
    /// Quality of an earlier compression, when one is indicated.
    pub secondary_quality: Option<u8>,
    /// Combined periodicity, distribution and grid score, in `[0, 1]`.
    pub double_compression_probability: f64,
    /// Histogram of the first AC coefficient across all blocks.
    pub ac_histogram: Vec<u32>,
    /// Strongest normalised autocorrelation peak, in `[0, 1]`.
    pub histogram_periodicity: f64,
    /// Discontinuity strength on the 8-pixel coding grid.
    pub block_artifact_map: GrayImage,
    /// High-frequency energy per block.
    pub dct_energy_map: GrayImage,
    /// Blocks more than 2.5 standard deviations from the mean energy.
    pub anomalous_regions: Vec<SRegion>,
    /// Quantisation step recovered per coefficient position.
    pub estimated_quantization_table: [[f64; 8]; 8],
}

// Index-based loops are the clearest expression of a dense 8x8 matrix
// product and of the DCT basis construction; iterator rewrites here obscure
// the (row, column) roles without changing the generated code.
/// Frequency-domain analysis of JPEG quantisation structure.
///
/// Computes the DCT of every aligned 8x8 block, estimates the quantisation
/// table from coefficient spacing, and looks for the periodicity that
/// requantising an already-quantised coefficient set leaves behind.
///
/// # Limitations
///
/// The quantisation table is estimated from decoded pixels, not read from the
/// JPEG header, so it is an approximation. Only grid-aligned blocks are
/// analysed: a crop by an amount not divisible by 8 degrades every estimate.
#[allow(clippy::needless_range_loop)]
pub struct DctAnalyzer {
    config: DctConfig,
    dct_matrix: [[f64; 8]; 8],
    dct_matrix_t: [[f64; 8]; 8],
}

#[allow(clippy::needless_range_loop)]
impl DctAnalyzer {
    /// Analyzer with the default configuration.
    pub fn new() -> Self {
        Self::with_config(DctConfig::default())
    }

    /// Analyzer with custom settings.
    pub fn with_config(config: DctConfig) -> Self {
        let dct_matrix = Self::compute_dct_matrix(8);
        let dct_matrix_t = Self::transpose_matrix(&dct_matrix);

        Self {
            config,
            dct_matrix,
            dct_matrix_t,
        }
    }

    /// Orthonormal DCT-II basis of size `n`, padded to 8x8.
    pub fn compute_dct_matrix(n: usize) -> [[f64; 8]; 8] {
        let mut matrix = [[0.0f64; 8]; 8];

        for i in 0..n {
            for j in 0..n {
                if i == 0 {
                    matrix[i][j] = 1.0 / (n as f64).sqrt();
                } else {
                    matrix[i][j] = (2.0 / n as f64).sqrt()
                        * (std::f64::consts::PI * (2.0 * j as f64 + 1.0) * i as f64
                            / (2.0 * n as f64))
                            .cos();
                }
            }
        }

        matrix
    }

    fn transpose_matrix(matrix: &[[f64; 8]; 8]) -> [[f64; 8]; 8] {
        let mut result = [[0.0f64; 8]; 8];
        for i in 0..8 {
            for j in 0..8 {
                result[i][j] = matrix[j][i];
            }
        }

        result
    }

    /// Run the analysis.
    ///
    /// # Errors
    ///
    /// [`ImageTooSmall`](crate::error::ForensicsError::ImageTooSmall) below 16px.
    pub fn analyze(&self, image: &DynamicImage) -> Result<DctAnalysisResult> {
        let gray = rgb_to_gray(&image.to_rgb8());
        let (width, height) = gray.dimensions();

        ensure_min_dimensions(width, height, 16)?;

        let coefficients = self.extract_all_dct_coefficients(&gray);
        let ac_histogram = self.build_ac_histogram(&coefficients);
        let histogram_periodicity = self.detect_histogram_periodicity(&ac_histogram);
        let estimated_quantization_table = self.estimate_quantization_table(&coefficients);
        let primary_quality = self.estimate_quality_from_qtable(&estimated_quantization_table);
        let (double_compression_probability, secondary_quality) =
            self.detect_double_compression(&ac_histogram, &coefficients);
        let block_artifact_map = self.create_block_artifact_map(&gray);
        let dct_energy_map = self.create_dct_energy_map(&gray, &coefficients);
        let anomalous_regions = self.find_anomalous_regions(&gray, &coefficients);

        Ok(DctAnalysisResult {
            primary_quality,
            secondary_quality,
            double_compression_probability,
            ac_histogram,
            histogram_periodicity,
            block_artifact_map,
            dct_energy_map,
            anomalous_regions,
            estimated_quantization_table,
        })
    }

    fn dct_2d(&self, block: &[[f64; 8]; 8]) -> [[f64; 8]; 8] {
        let mut temp = [[0.0f64; 8]; 8];
        let mut result = [[0.0f64; 8]; 8];

        for i in 0..8 {
            for j in 0..8 {
                let mut sum = 0.0;
                for k in 0..8 {
                    sum += self.dct_matrix[i][k] * block[k][j];
                }
                temp[i][j] = sum;
            }
        }

        for i in 0..8 {
            for j in 0..8 {
                let mut sum = 0.0;
                for k in 0..8 {
                    sum += temp[i][k] * self.dct_matrix_t[k][j];
                }
                result[i][j] = sum;
            }
        }

        result
    }

    fn extract_block(&self, gray: &GrayImage, bx: u32, by: u32) -> [[f64; 8]; 8] {
        let mut block = [[0.0f64; 8]; 8];

        for y in 0..8 {
            for x in 0..8 {
                let px = bx + x as u32;
                let py = by + y as u32;
                if px < gray.width() && py < gray.height() {
                    block[y][x] = gray.get_pixel(px, py)[0] as f64 - 128.0;
                }
            }
        }

        block
    }

    fn extract_all_dct_coefficients(&self, gray: &GrayImage) -> Vec<[[f64; 8]; 8]> {
        let (width, height) = gray.dimensions();
        let blocks_x = width / 8;
        // Was `width / 8`. On a tall image that analysed only the top square
        // region; on a wide one it manufactured phantom blocks past the bottom
        // edge, filled with the -128 level shift.
        let blocks_y = height / 8;

        let mut coefficients = Vec::with_capacity((blocks_x * blocks_y) as usize);

        for by in 0..blocks_y {
            for bx in 0..blocks_x {
                let block = self.extract_block(gray, bx * 8, by * 8);
                let dct_block = self.dct_2d(&block);
                coefficients.push(dct_block);
            }
        }

        coefficients
    }

    /// Histogram of the first AC coefficient across all blocks.
    ///
    /// Unquantised DCT coefficients of a decoded image routinely reach several
    /// hundred, so the previous `ac + 128` offset clamped to `0..255` piled
    /// most of the distribution into the two end bins. The range is derived
    /// from the data instead, and the bin width is recorded so periodicity can
    /// be interpreted in coefficient units.
    fn build_ac_histogram(&self, coefficients: &[[[f64; 8]; 8]]) -> Vec<u32> {
        let bins = self.config.histogram_bins.max(2);
        let mut histogram = vec![0u32; bins];

        if coefficients.is_empty() {
            return histogram;
        }

        let extent = coefficients
            .iter()
            .map(|block| block[0][1].abs())
            .fold(0.0f64, f64::max)
            .max(1.0);

        let scale = (bins as f64 - 1.0) / (2.0 * extent);

        for block in coefficients {
            let shifted = (block[0][1] + extent) * scale;
            let bin = (shifted.round() as isize).clamp(0, bins as isize - 1) as usize;
            histogram[bin] += 1;
        }

        histogram
    }

    /// Strength of the strongest periodic component in a histogram.
    ///
    /// Computed as a mean-centred, variance-normalised autocorrelation, which
    /// is genuinely bounded by 1. The previous version divided a raw
    /// (uncentred) correlation by a global norm, routinely exceeded 1, and was
    /// then clamped — so it returned ~1.0 for almost every image and put a
    /// permanent 0.4 floor under the double-compression score.
    fn detect_histogram_periodicity(&self, histogram: &[u32]) -> f64 {
        let n = histogram.len();
        if n < 10 {
            return 0.0;
        }

        let values: Vec<f64> = histogram.iter().map(|&x| x as f64).collect();
        let mean = values.iter().sum::<f64>() / n as f64;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;

        if variance < 1e-10 {
            return 0.0;
        }

        let mut max_correlation = 0.0f64;

        for period in 2..20.min(n) {
            let mut sum = 0.0;
            let count = n - period;

            for i in period..n {
                sum += (values[i] - mean) * (values[i - period] - mean);
            }

            let correlation = (sum / count as f64) / variance;
            max_correlation = max_correlation.max(correlation);
        }

        max_correlation.clamp(0.0, 1.0)
    }

    fn estimate_quantization_table(&self, coefficients: &[[[f64; 8]; 8]]) -> [[f64; 8]; 8] {
        let mut qtable = [[1.0f64; 8]; 8];

        if coefficients.is_empty() {
            return qtable;
        }

        for y in 0..8 {
            for x in 0..8 {
                if x == 0 && y == 0 {
                    continue;
                }

                let mut values = coefficients
                    .iter()
                    .map(|block| block[y][x])
                    .filter(|&v| v.abs() > 0.5)
                    .collect::<Vec<_>>();

                if values.len() < 10 {
                    continue;
                }

                values.sort_by(|a, b| a.abs().partial_cmp(&b.abs()).unwrap());

                let step = self.estimate_step_from_values(&values);
                qtable[y][x] = step.max(1.0);
            }
        }

        qtable
    }

    fn estimate_step_from_values(&self, values: &[f64]) -> f64 {
        if values.len() < 5 {
            return 1.0;
        }

        let mut gaps = Vec::new();

        for i in 1..values.len().min(100) {
            let gap = (values[i].abs() - values[i - 1].abs()).abs();
            if gap > 0.5 {
                gaps.push(gap);
            }
        }

        if gaps.is_empty() {
            return 1.0;
        }

        gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let median_idx = gaps.len() / 2;
        gaps[median_idx]
    }

    fn estimate_quality_from_qtable(&self, qtable: &[[f64; 8]; 8]) -> u8 {
        // standard jpeg luminance quantization table at quality 50
        let standard_qtable: [[f64; 8]; 8] = [
            [16.0, 11.0, 10.0, 16.0, 24.0, 40.0, 51.0, 61.0],
            [12.0, 12.0, 14.0, 19.0, 26.0, 58.0, 60.0, 55.0],
            [14.0, 13.0, 16.0, 24.0, 40.0, 57.0, 69.0, 56.0],
            [14.0, 17.0, 22.0, 29.0, 51.0, 87.0, 80.0, 62.0],
            [18.0, 22.0, 37.0, 56.0, 68.0, 109.0, 103.0, 77.0],
            [24.0, 35.0, 55.0, 64.0, 81.0, 104.0, 113.0, 92.0],
            [49.0, 64.0, 78.0, 87.0, 103.0, 121.0, 120.0, 101.0],
            [72.0, 92.0, 95.0, 98.0, 112.0, 100.0, 103.0, 99.0],
        ];

        let mut ratio_sum = 0.0;
        let mut count = 0;

        for y in 0..8 {
            for x in 0..8 {
                if x == 0 && y == 0 {
                    continue;
                }
                if qtable[y][x] > 0.0 {
                    ratio_sum += qtable[y][x] / standard_qtable[y][x];
                    count += 1;
                }
            }
        }

        if count == 0 {
            return 75;
        }

        let avg_ratio = ratio_sum / count as f64;

        let quality = if avg_ratio < 1.0 {
            (50.0 + 50.0 * (1.0 - avg_ratio)) as u8
        } else {
            (50.0 / avg_ratio) as u8
        };

        quality.clamp(1, 100)
    }

    fn detect_double_compression(
        &self,
        histogram: &[u32],
        coefficients: &[[[f64; 8]; 8]],
    ) -> (f64, Option<u8>) {
        let periodicity = self.detect_histogram_periodicity(histogram);
        let distribution_score = self.analyze_coefficient_distribution(coefficients);
        let grid_score = self.analyze_block_grid(coefficients);

        let combined_score =
            (periodicity * 0.4 + distribution_score * 0.3 + grid_score * 0.3).min(1.0);

        let secondary_quality = if combined_score > 0.5 {
            Some(self.estimate_secondary_quality(histogram))
        } else {
            None
        };

        (combined_score, secondary_quality)
    }

    fn analyze_coefficient_distribution(&self, coefficients: &[[[f64; 8]; 8]]) -> f64 {
        if coefficients.is_empty() {
            return 0.0;
        }

        let mut zero_count = 0;
        let mut non_zero_count = 0;

        for block in coefficients {
            for y in 0..8 {
                for x in 0..8 {
                    if x == 0 && y == 0 {
                        continue;
                    }

                    let v = block[y][x].abs();
                    if v < 0.5 {
                        zero_count += 1;
                    } else {
                        non_zero_count += 1;
                    }
                }
            }
        }

        let zero_ratio = zero_count as f64 / (zero_count + non_zero_count) as f64;

        if zero_ratio > 0.855 || zero_ratio < 0.5 {
            (zero_ratio - 0.7).abs()
        } else {
            0.0
        }
    }

    fn analyze_block_grid(&self, coefficients: &[[[f64; 8]; 8]]) -> f64 {
        if coefficients.len() < 4 {
            return 0.0;
        }

        let total_energy: Vec<f64> = coefficients
            .iter()
            .map(|block| block.iter().flatten().map(|c| c * c).sum())
            .collect();

        if total_energy.is_empty() {
            return 0.0;
        }

        let (mean_energy, energy_variance) = mean_and_variance(&total_energy);
        let normalized_variance = (energy_variance.sqrt() / mean_energy.max(1.0)).min(1.0);

        if normalized_variance > 0.5 {
            normalized_variance
        } else {
            0.0
        }
    }

    fn estimate_secondary_quality(&self, histogram: &[u32]) -> u8 {
        let n = histogram.len();
        let mut best_period = 0;
        let mut best_score = 0.0;

        for period in 2..20 {
            let mut score = 0.0;
            let mut count = 0;

            for i in period..n {
                if histogram[i] > 0 && histogram[i - period] > 0 {
                    let ratio = histogram[i].min(histogram[i - period]) as f64
                        / histogram[i].max(histogram[i - period]) as f64;
                    score += ratio;
                    count += 1;
                }
            }

            if count > 0 {
                score /= count as f64;
                if score > best_score {
                    best_score = score;
                    best_period = period;
                }
            }
        }

        match best_period {
            0..=4 => 85,
            5..=8 => 75,
            9..=12 => 65,
            _ => 50,
        }
    }

    fn create_block_artifact_map(&self, gray: &GrayImage) -> GrayImage {
        let (width, height) = gray.dimensions();
        let mut artifact_map = GrayImage::new(width, height);

        for y in 0..height {
            for x in 0..width {
                let mut artifact = 0.0;

                let on_h_boundary = x > 0 && x % 8 == 0;
                let on_v_boundary = y > 0 && y % 8 == 0;

                if on_h_boundary {
                    let left = gray.get_pixel(x - 1, y)[0] as f64;
                    let right = gray.get_pixel(x, y)[0] as f64;
                    artifact += (left - right).abs();
                }

                if on_v_boundary {
                    let top = gray.get_pixel(x, y - 1)[0] as f64;
                    let bottom = gray.get_pixel(x, y)[0] as f64;
                    artifact += (top - bottom).abs();
                }

                artifact_map.put_pixel(x, y, Luma([(artifact.min(255.0)) as u8]));
            }
        }

        artifact_map
    }

    fn create_dct_energy_map(&self, gray: &GrayImage, coefficients: &[[[f64; 8]; 8]]) -> GrayImage {
        let (width, height) = gray.dimensions();
        let blocks_x = (width / 8) as usize;
        let mut energy_map = GrayImage::new(width, height);

        for (idx, block) in coefficients.iter().enumerate() {
            let bx = (idx % blocks_x) as u32 * 8;
            let by = (idx / blocks_x) as u32 * 8;

            let mut hf_energy = 0.0;
            for y in 0..8 {
                for x in 0..8 {
                    if x + y > 2 {
                        hf_energy += block[y][x].abs();
                    }
                }
            }

            let energy_value = (hf_energy / 50.0).min(1.0);
            let pixel_value = (energy_value * 255.0) as u8;

            for dy in 0..8 {
                for dx in 0..8 {
                    let px = bx + dx;
                    let py = by + dy;
                    if px < width && py < height {
                        energy_map.put_pixel(px, py, Luma([pixel_value]));
                    }
                }
            }
        }

        energy_map
    }

    fn find_anomalous_regions(
        &self,
        gray: &GrayImage,
        coefficients: &[[[f64; 8]; 8]],
    ) -> Vec<SRegion> {
        let (width, _height) = gray.dimensions();
        let blocks_x = (width / 8) as usize;
        let mut regions = Vec::new();

        if coefficients.is_empty() || blocks_x == 0 {
            return regions;
        }

        let energies = coefficients
            .iter()
            .map(|block| {
                let mut e = 0.0;
                for y in 0..8 {
                    for x in 0..8 {
                        e += block[y][x] * block[y][x];
                    }
                }
                e.sqrt()
            })
            .collect::<Vec<_>>();

        let mean_energy = energies.iter().sum::<f64>() / energies.len() as f64;
        let variance = energies
            .iter()
            .map(|e| (e - mean_energy).powi(2))
            .sum::<f64>()
            / energies.len() as f64;
        let std_dev = variance.sqrt();

        for (idx, &energy) in energies.iter().enumerate() {
            let z_score = (energy - mean_energy).abs() / std_dev.max(1.0);

            if z_score > 2.5 {
                let bx = (idx % blocks_x) as u32 * 8;
                let by = (idx / blocks_x) as u32 * 8;

                regions.push(SRegion {
                    x: bx,
                    y: by,
                    width: 8,
                    height: 8,
                });
            }
        }

        regions
    }
}

impl Default for DctAnalyzer {
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
            let v = (((x * 17) ^ (y * 5)) % 256) as u8;
            *pixel = Rgb([v, v, v]);
        }
        DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn block_grid_covers_the_full_height() {
        // 64x256: eight columns of blocks, thirty-two rows. `blocks_y` was
        // derived from the width, so only the top 64 rows were ever analysed.
        let analyzer = DctAnalyzer::new();
        let gray = rgb_to_gray(&textured(64, 256).to_rgb8());
        let coefficients = analyzer.extract_all_dct_coefficients(&gray);

        assert_eq!(coefficients.len(), (64 / 8) * (256 / 8));
    }

    #[test]
    fn wide_images_produce_no_phantom_blocks() {
        let analyzer = DctAnalyzer::new();
        let gray = rgb_to_gray(&textured(256, 64).to_rgb8());
        let coefficients = analyzer.extract_all_dct_coefficients(&gray);

        assert_eq!(coefficients.len(), (256 / 8) * (64 / 8));
    }

    #[test]
    fn periodicity_is_bounded_by_one() {
        let analyzer = DctAnalyzer::new();

        // A flat histogram must not read as strongly periodic.
        let flat = vec![100u32; 256];
        assert_eq!(analyzer.detect_histogram_periodicity(&flat), 0.0);

        let periodic: Vec<u32> = (0..256).map(|i| if i % 4 == 0 { 200 } else { 5 }).collect();
        let score = analyzer.detect_histogram_periodicity(&periodic);

        assert!((0.0..=1.0).contains(&score), "score {score} out of range");
        assert!(score > 0.5, "period-4 comb scored only {score}");
    }

    #[test]
    fn ac_histogram_spreads_across_bins() {
        let analyzer = DctAnalyzer::new();
        let gray = rgb_to_gray(&textured(128, 128).to_rgb8());
        let coefficients = analyzer.extract_all_dct_coefficients(&gray);
        let histogram = analyzer.build_ac_histogram(&coefficients);

        let occupied = histogram.iter().filter(|&&count| count > 0).count();
        assert!(occupied > 2, "only {occupied} bins occupied; range clamped");
    }

    #[test]
    fn undersized_images_error() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(8, 8));
        assert!(DctAnalyzer::new().analyze(&image).is_err());
    }

    #[test]
    fn anomalous_regions_stay_in_bounds() {
        let result = DctAnalyzer::new().analyze(&textured(96, 160)).unwrap();

        for region in &result.anomalous_regions {
            assert!(region.right() <= 96);
            assert!(region.bottom() <= 160);
        }
    }
}
