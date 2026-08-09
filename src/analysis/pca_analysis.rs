use image::{DynamicImage, GrayImage, Luma};

use crate::{
    SRegion,
    error::Result,
    image_utils::{
        clipped_blocks, ensure_min_dimensions, full_blocks, mean_and_variance, rgb_to_gray,
    },
    region::merge_regions,
};

/// Settings for [`PcaAnalyzer`].
#[derive(Debug, Clone)]
pub struct PcaConfig {
    /// Tile size for aggregating patch anomalies into regions.
    pub block_size: u32,
    /// Eigenvectors retained. More captures more variance and leaves less residual.
    pub num_components: usize,
    /// Side of each patch, so `patch_size^2` features.
    pub patch_size: u32,
    /// Step between patches. Half `patch_size` gives 50% overlap.
    pub patch_stride: u32,
    /// Standard deviations of reconstruction error above the mean before a patch counts as anomalous.
    pub anomaly_threshold: f64,
    /// Reserved for future component pruning.
    pub min_variance_ratio: f64,
}

impl Default for PcaConfig {
    fn default() -> Self {
        Self {
            block_size: 64,
            num_components: 3,
            patch_size: 8,
            patch_stride: 4,
            anomaly_threshold: 2.5,
            min_variance_ratio: 0.01,
        }
    }
}

/// Output of [`PcaAnalyzer`].
#[derive(Debug, Clone)]
pub struct PcaAnalysisResult {
    /// Z-scored reconstruction error. Mid-grey is average.
    pub anomaly_map: GrayImage,
    /// First principal component projection.
    pub pc1_map: GrayImage,
    /// Second principal component projection.
    pub pc2_map: GrayImage,
    /// Third principal component projection.
    pub pc3_map: GrayImage,
    /// Tiles where anomalous patches concentrate, merged.
    pub anomalous_regions: Vec<SRegion>,
    /// Share of total variance each component explains. Sums to less than 1.
    pub variance_ratios: Vec<f64>,
    /// Combined anomaly rate and error spread, in `[0, 1]`.
    pub overall_anomaly_score: f64,
    /// Coverage-weighted anomaly score, in `[0, 1]`.
    pub manipulation_probability: f64,
}

/// Flags patches that reconstruct poorly from the image's own subspace.
///
/// Overlapping patches from one photograph mostly live in a low-dimensional
/// subspace. Content from a different source reconstructs from it badly.
///
/// # Limitations
///
/// It flags *unusual*, not *foreign* — the single most distinctive genuine
/// object in a photograph is exactly what this reports. A large tampered
/// region also contaminates the basis it is measured against.
pub struct PcaAnalyzer {
    config: PcaConfig,
}

#[allow(clippy::needless_range_loop)]
impl PcaAnalyzer {
    /// Analyzer with the default configuration.
    pub fn new() -> Self {
        Self::with_config(PcaConfig::default())
    }

    /// Analyzer with custom settings.
    pub fn with_config(config: PcaConfig) -> Self {
        Self { config }
    }

    /// Run the analysis.
    ///
    /// # Errors
    ///
    /// [`ImageTooSmall`](crate::error::ForensicsError::ImageTooSmall) below
    /// twice `block_size`, or
    /// [`AnalysisFailed`](crate::error::ForensicsError::AnalysisFailed) when
    /// too few patches could be extracted.
    pub fn analyze(&self, image: &DynamicImage) -> Result<PcaAnalysisResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);
        let (width, height) = gray.dimensions();

        ensure_min_dimensions(width, height, self.config.block_size * 2)?;

        let (patches, patch_positions) = self.extract_patches(&gray);

        if patches.is_empty() {
            return Err(crate::error::ForensicsError::AnalysisFailed(
                "No patches could be extracted".into(),
            ));
        }

        let pca = self.compute_pca(&patches)?;

        // Fraction of the *total* variance each component explains, so the
        // ratios legitimately sum to less than 1.
        let variance_ratios = pca
            .eigenvalues
            .iter()
            .map(|&ev| {
                if pca.total_variance > 0.0 {
                    (ev / pca.total_variance).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            })
            .collect::<Vec<_>>();

        let projections = self.project_patches(&patches, &pca.components, &pca.mean);

        let pc1_map = self.create_component_map(width, height, &patch_positions, &projections, 0);
        let pc2_map = self.create_component_map(width, height, &patch_positions, &projections, 1);
        let pc3_map = self.create_component_map(width, height, &patch_positions, &projections, 2);

        let reconstruction_errors =
            self.compute_reconstruction_errors(&patches, &pca.components, &pca.mean, &projections);

        let anomaly_map =
            self.create_anomaly_map(width, height, &patch_positions, &reconstruction_errors);

        let anomalous_regions =
            self.find_anomalous_regions(width, height, &reconstruction_errors, &patch_positions);

        let overall_anomaly_score = self.calculate_overall_anomaly_score(&reconstruction_errors);
        let manipulation_probability = self.calculate_manipulation_probability(
            &anomalous_regions,
            overall_anomaly_score,
            width,
            height,
        );

        Ok(PcaAnalysisResult {
            anomaly_map,
            pc1_map,
            pc2_map,
            pc3_map,
            anomalous_regions,
            variance_ratios,
            overall_anomaly_score,
            manipulation_probability,
        })
    }

    fn extract_patches(&self, gray: &GrayImage) -> (Vec<Vec<f64>>, Vec<(u32, u32)>) {
        let (width, height) = gray.dimensions();
        let patch_size = self.config.patch_size;
        let stride = self.config.patch_stride;

        let mut patches = Vec::new();
        let mut positions = Vec::new();

        for region in full_blocks(width, height, patch_size, stride) {
            patches.push(self.extract_single_patch(gray, region.x, region.y));
            positions.push((region.x, region.y));
        }

        (patches, positions)
    }

    fn extract_single_patch(&self, gray: &GrayImage, x: u32, y: u32) -> Vec<f64> {
        let patch_size = self.config.patch_size;
        let mut patch = Vec::with_capacity((patch_size * patch_size) as usize);

        for dy in 0..patch_size {
            for dx in 0..patch_size {
                let pixel = gray.get_pixel(x + dx, y + dy)[0] as f64;
                patch.push(pixel);
            }
        }

        patch
    }

    fn compute_pca(&self, patches: &[Vec<f64>]) -> Result<Pca> {
        let n_samples = patches.len();
        let n_features = patches[0].len();

        if n_samples < self.config.num_components {
            return Err(crate::error::ForensicsError::AnalysisFailed(
                "Not enough samples for PCA".into(),
            ));
        }

        let mut mean = vec![0.0; n_features];
        for patch in patches {
            for (i, &val) in patch.iter().enumerate() {
                mean[i] += val;
            }
        }
        for m in &mut mean {
            *m /= n_samples as f64;
        }

        let centered = patches
            .iter()
            .map(|patch| {
                patch
                    .iter()
                    .zip(mean.iter())
                    .map(|(&p, &m)| p - m)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let max_samples = 5000.min(n_samples);
        let step = n_samples / max_samples;

        let mut covariance = vec![vec![0.0; n_features]; n_features];
        let mut sample_count = 0;

        for (idx, patch) in centered.iter().enumerate() {
            if idx % step != 0 {
                continue;
            }
            sample_count += 1;

            for i in 0..n_features {
                for j in i..n_features {
                    let val = patch[i] * patch[j];
                    covariance[i][j] += val;
                    if i != j {
                        covariance[j][i] += val;
                    }
                }
            }
        }

        for i in 0..n_features {
            for j in 0..n_features {
                covariance[i][j] /= sample_count as f64;
            }
        }

        // The trace is the total variance across all features. Summing only the
        // extracted eigenvalues, as the caller previously did, forced the
        // reported ratios to sum to 1.0 and made "explained variance"
        // meaningless.
        let total_variance: f64 = (0..n_features).map(|i| covariance[i][i]).sum();

        let (eigenvectors, eigenvalues) =
            self.power_iteration(&covariance, self.config.num_components.min(n_features));

        Ok(Pca {
            components: eigenvectors,
            eigenvalues,
            mean,
            total_variance,
        })
    }

    fn power_iteration(
        &self,
        matrix: &[Vec<f64>],
        num_components: usize,
    ) -> (Vec<Vec<f64>>, Vec<f64>) {
        let n = matrix.len();
        let mut eigenvectors = Vec::new();
        let mut eigenvalues = Vec::new();
        let mut deflated_matrix = matrix.to_vec();

        for _ in 0..num_components {
            let mut v = (0..n).map(|i| (i as f64 * 0.1).sin()).collect::<Vec<_>>();
            let mut eigenvalue = 0.0;

            for _ in 0..100 {
                let mut new_v = vec![0.0; n];
                for i in 0..n {
                    for j in 0..n {
                        new_v[i] += deflated_matrix[i][j] * v[j];
                    }
                }

                eigenvalue = 0.0;
                for i in 0..n {
                    eigenvalue += new_v[i] * v[i];
                }

                let norm = new_v.iter().map(|x| x * x).sum::<f64>().sqrt();
                if norm > 1e-10 {
                    for x in &mut new_v {
                        *x /= norm;
                    }
                }

                let diff = v
                    .iter()
                    .zip(new_v.iter())
                    .map(|(a, b)| (a - b).abs())
                    .sum::<f64>();

                v = new_v;

                if diff < 1e-8 {
                    break;
                }
            }

            for i in 0..n {
                for j in 0..n {
                    deflated_matrix[i][j] -= eigenvalue * v[i] * v[j];
                }
            }

            eigenvectors.push(v);
            eigenvalues.push(eigenvalue.max(0.0));
        }

        (eigenvectors, eigenvalues)
    }

    fn project_patches(
        &self,
        patches: &[Vec<f64>],
        components: &[Vec<f64>],
        mean: &[f64],
    ) -> Vec<Vec<f64>> {
        patches
            .iter()
            .map(|patch| {
                let centered = patch
                    .iter()
                    .zip(mean.iter())
                    .map(|(&p, &m)| p - m)
                    .collect::<Vec<_>>();

                components
                    .iter()
                    .map(|component| {
                        centered
                            .iter()
                            .zip(component.iter())
                            .map(|(&c, &v)| c * v)
                            .sum()
                    })
                    .collect()
            })
            .collect()
    }

    fn compute_reconstruction_errors(
        &self,
        patches: &[Vec<f64>],
        components: &[Vec<f64>],
        mean: &[f64],
        projections: &[Vec<f64>],
    ) -> Vec<f64> {
        patches
            .iter()
            .zip(projections.iter())
            .map(|(patch, proj)| {
                let mut reconstructed = mean.to_vec();
                for (i, component) in components.iter().enumerate() {
                    if i < proj.len() {
                        for (j, &c) in component.iter().enumerate() {
                            reconstructed[j] += proj[i] * c;
                        }
                    }
                }

                let error = patch
                    .iter()
                    .zip(reconstructed.iter())
                    .map(|(&p, &r)| (p - r).powi(2))
                    .sum::<f64>();

                error.sqrt() / patch.len() as f64
            })
            .collect()
    }

    /// Average one scalar per patch over the pixels each patch covers.
    ///
    /// Returns `(sums, counts)` as flat row-major buffers. Overlapping patches
    /// used to be accumulated into a `Vec<Vec<Vec<f64>>>`, i.e. one heap
    /// allocation per pixel — twelve million of them on a 12 MP image, repeated
    /// for each of the three component maps and again for the anomaly map. A
    /// running sum and count needs two flat buffers and no per-pixel allocation.
    fn accumulate_per_pixel(
        &self,
        width: u32,
        height: u32,
        positions: &[(u32, u32)],
        values: impl Fn(usize) -> Option<f64>,
    ) -> (Vec<f64>, Vec<u32>) {
        let len = (width as usize) * (height as usize);
        let mut sums = vec![0.0f64; len];
        let mut counts = vec![0u32; len];
        let patch_size = self.config.patch_size;

        for (i, &(x, y)) in positions.iter().enumerate() {
            let Some(value) = values(i) else {
                continue;
            };

            let patch = SRegion::new(x, y, patch_size, patch_size).clamp_to(width, height);

            for (px, py) in patch.pixels() {
                let index = (py as usize) * (width as usize) + px as usize;
                sums[index] += value;
                counts[index] += 1;
            }
        }

        (sums, counts)
    }

    fn create_component_map(
        &self,
        width: u32,
        height: u32,
        positions: &[(u32, u32)],
        projections: &[Vec<f64>],
        component_idx: usize,
    ) -> GrayImage {
        let mut map = GrayImage::new(width, height);

        let (sums, counts) = self.accumulate_per_pixel(width, height, positions, |i| {
            projections[i].get(component_idx).copied()
        });

        let mut averages: Vec<f64> = sums
            .iter()
            .zip(counts.iter())
            .filter(|(_, count)| **count > 0)
            .map(|(&sum, &count)| sum / count as f64)
            .collect();

        if averages.is_empty() {
            return map;
        }

        // Stretch between the 5th and 95th percentiles so a few outliers do not
        // flatten the whole map.
        averages.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let min_val = averages[averages.len() / 20];
        let max_val = averages[averages.len() * 19 / 20];
        let range = (max_val - min_val).max(1e-10);

        for y in 0..height {
            for x in 0..width {
                let index = (y as usize) * (width as usize) + x as usize;
                if counts[index] > 0 {
                    let avg = sums[index] / counts[index] as f64;
                    let normalized = ((avg - min_val) / range).clamp(0.0, 1.0);
                    map.put_pixel(x, y, Luma([(normalized * 255.0) as u8]));
                }
            }
        }

        map
    }

    fn create_anomaly_map(
        &self,
        width: u32,
        height: u32,
        positions: &[(u32, u32)],
        errors: &[f64],
    ) -> GrayImage {
        let mut map = GrayImage::new(width, height);

        let (sums, counts) =
            self.accumulate_per_pixel(width, height, positions, |i| errors.get(i).copied());

        let (mean_error, variance) = mean_and_variance(errors);
        let std_dev = variance.sqrt();

        for y in 0..height {
            for x in 0..width {
                let index = (y as usize) * (width as usize) + x as usize;
                if counts[index] == 0 {
                    continue;
                }

                let avg = sums[index] / counts[index] as f64;
                let z_score = if std_dev > 0.0 {
                    (avg - mean_error) / std_dev
                } else {
                    0.0
                };

                let normalized = (z_score / 5.0 + 0.5).clamp(0.0, 1.0);
                map.put_pixel(x, y, Luma([(normalized * 255.0) as u8]));
            }
        }

        map
    }

    /// Flag blocks where reconstruction error is concentrated.
    ///
    /// The threshold is derived from the error distribution and then actually
    /// applied. Previously it was computed and discarded in favour of the magic
    /// constant `128.0 + anomaly_threshold * 30.0` tested against the rendered
    /// map, so `anomaly_threshold` bore no relationship to the data and the
    /// `errors`/`positions` arguments went unused.
    fn find_anomalous_regions(
        &self,
        width: u32,
        height: u32,
        errors: &[f64],
        positions: &[(u32, u32)],
    ) -> Vec<SRegion> {
        if errors.is_empty() {
            return Vec::new();
        }

        let (mean_error, variance) = mean_and_variance(errors);
        let threshold = mean_error + self.config.anomaly_threshold * variance.sqrt();

        let patch_size = self.config.patch_size;
        let block_size = self.config.block_size;

        let regions = clipped_blocks(width, height, block_size, block_size)
            .filter(|block| {
                let mut anomalous = 0usize;
                let mut total = 0usize;

                for (&(px, py), &error) in positions.iter().zip(errors.iter()) {
                    let patch = SRegion::new(px, py, patch_size, patch_size);
                    if !patch.overlaps(block) {
                        continue;
                    }

                    total += 1;
                    if error > threshold {
                        anomalous += 1;
                    }
                }

                // A block is anomalous when a fifth of the patches covering it
                // exceed the threshold.
                total > 0 && anomalous * 5 >= total
            })
            .collect();

        merge_regions(regions, block_size / 2)
    }

    fn calculate_overall_anomaly_score(&self, errors: &[f64]) -> f64 {
        if errors.is_empty() {
            return 0.0;
        }

        let (mean, variance) = mean_and_variance(errors);
        let std_dev = variance.sqrt();

        let threshold = mean + self.config.anomaly_threshold * std_dev;
        let anomaly_count = errors.iter().filter(|&&e| e > threshold).count();
        let anomaly_ratio = anomaly_count as f64 / errors.len() as f64;

        let spread_score = (std_dev / mean.max(1.0)).min(1.0);

        (anomaly_ratio * 0.6 + spread_score * 0.4).min(1.0)
    }

    fn calculate_manipulation_probability(
        &self,
        regions: &[SRegion],
        anomaly_score: f64,
        width: u32,
        height: u32,
    ) -> f64 {
        let total_pixels = width as f64 * height as f64;

        let anomalous_pixels: u64 = regions.iter().map(|r| r.area()).sum();

        let coverage = if total_pixels > 0.0 {
            anomalous_pixels as f64 / total_pixels
        } else {
            0.0
        };

        let manipulation_prob = if coverage > 0.5 {
            anomaly_score * 0.3
        } else if coverage > 0.01 {
            anomaly_score * 0.8 + coverage * 0.2
        } else {
            anomaly_score * 0.5
        };

        manipulation_prob.min(1.0)
    }
}

/// Outcome of the covariance decomposition over the extracted patches.
struct Pca {
    components: Vec<Vec<f64>>,
    eigenvalues: Vec<f64>,
    mean: Vec<f64>,
    /// Trace of the covariance matrix: variance across all features.
    total_variance: f64,
}

impl Default for PcaAnalyzer {
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
            let v = (((x * 11) ^ (y * 23)) % 256) as u8;
            *pixel = Rgb([v, v, v]);
        }
        DynamicImage::ImageRgb8(image)
    }

    fn small_config() -> PcaConfig {
        PcaConfig {
            block_size: 16,
            ..PcaConfig::default()
        }
    }

    #[test]
    fn variance_ratios_do_not_sum_to_one_by_construction() {
        let analyzer = PcaAnalyzer::with_config(small_config());
        let result = analyzer.analyze(&textured(128, 128)).unwrap();

        let total: f64 = result.variance_ratios.iter().sum();

        // Three components out of 64 features cannot explain everything.
        // Normalising by the extracted eigenvalues alone forced this to 1.0.
        assert!(total <= 1.0 + 1e-9, "ratios sum to {total}");
        assert!(
            result
                .variance_ratios
                .iter()
                .all(|r| (0.0..=1.0).contains(r)),
            "ratios out of range: {:?}",
            result.variance_ratios
        );
    }

    #[test]
    fn anomalous_regions_stay_in_bounds() {
        let analyzer = PcaAnalyzer::with_config(small_config());
        let result = analyzer.analyze(&textured(100, 140)).unwrap();

        for region in &result.anomalous_regions {
            assert!(region.right() <= 100, "{region:?}");
            assert!(region.bottom() <= 140, "{region:?}");
        }
    }

    #[test]
    fn component_maps_match_the_image_size() {
        let analyzer = PcaAnalyzer::with_config(small_config());
        let result = analyzer.analyze(&textured(96, 64)).unwrap();

        assert_eq!(result.pc1_map.dimensions(), (96, 64));
        assert_eq!(result.anomaly_map.dimensions(), (96, 64));
    }

    #[test]
    fn undersized_images_error() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(64, 64));
        assert!(PcaAnalyzer::new().analyze(&image).is_err());
    }
}
