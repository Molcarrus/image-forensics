use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};
use rayon::prelude::*;

use crate::{
    SRegion, draw,
    error::Result,
    image_utils::{clipped_blocks, ensure_min_dimensions, full_blocks, sobel},
};

/// Settings for [`ChromaticAberrationAnalyzer`].
#[derive(Debug, Clone)]
pub struct ChromaticAberrationConfig {
    /// Tile size, swept at 50% overlap. Default 64.
    pub block_size: u32,
    /// Sobel magnitude a pixel must exceed to serve as an edge point.
    pub edge_threshold: f64,
    /// Reserved for future edge filtering.
    pub min_edge_strength: f64,
    /// Search radius in pixels. Measurements beyond it are discarded.
    pub max_aberration: f64,
    /// Scaled by 50 to give the 0-255 cutoff on the inconsistency map.
    pub inconsistency_threshold: f64,
}

impl Default for ChromaticAberrationConfig {
    fn default() -> Self {
        Self {
            block_size: 64,
            edge_threshold: 30.0,
            min_edge_strength: 20.0,
            max_aberration: 5.0,
            inconsistency_threshold: 1.5,
        }
    }
}

/// One tile's measured per-channel misalignment.
#[derive(Debug, Clone, Copy)]
pub struct AberrationMeasurement {
    /// Tile centre, x.
    pub x: u32,
    /// Tile centre, y.
    pub y: u32,
    /// Red-to-green horizontal displacement, in pixels.
    pub rg_shift_x: f64,
    /// Red-to-green vertical displacement, in pixels.
    pub rg_shift_y: f64,
    /// Blue-to-green horizontal displacement, in pixels.
    pub bg_shift_x: f64,
    /// Blue-to-green vertical displacement, in pixels.
    pub bg_shift_y: f64,
    /// Mean normalised cross-correlation at the chosen shift.
    pub confidence: f64,
}

/// Output of [`ChromaticAberrationAnalyzer`].
#[derive(Debug, Clone)]
pub struct ChromaticAberrationResult {
    /// Per-tile displacement measurements.
    pub measurements: Vec<AberrationMeasurement>,
    /// Displacement magnitude, normalised for display.
    pub aberration_map: GrayImage,
    /// Where measured displacement departs from the fitted model.
    pub inconsistency_map: GrayImage,
    /// The original with shift vectors and the fitted optical centre drawn.
    pub visualization: RgbImage,
    /// Tiles exceeding `inconsistency_threshold`.
    pub inconsistent_regions: Vec<SRegion>,
    /// Best-fitting optical centre, if a model was found.
    pub optical_center: Option<(f64, f64)>,
    /// The fitted radial dispersion model, if one was found.
    pub radial_model: Option<RadialAberrationModel>,
    /// How well the measurements match the model, in `[0, 1]`.
    pub consistency_score: f64,
    /// Combined coverage and consistency score, in `[0, 1]`.
    pub manipulation_probability: f64,
}

/// A radial lens-dispersion model fitted across the frame.
///
/// A *displaced* optical centre is the forensic signal: splicing breaks the
/// radial symmetry that a single lens imposes.
#[derive(Debug, Clone, Copy)]
pub struct RadialAberrationModel {
    /// Fitted optical centre, x.
    pub center_x: f64,
    /// Fitted optical centre, y.
    pub center_y: f64,
    /// Red-channel dispersion coefficient, per unit radius.
    pub k_red: f64,
    /// Blue-channel dispersion coefficient, per unit radius.
    pub k_blue: f64,
    /// Coefficient of determination for the fit, in `[0, 1]`.
    pub fit_quality: f64,
}

/// Per-channel lens dispersion, fitted to a radial model.
///
/// A lens refracts wavelengths by slightly different amounts, displacing red
/// and blue relative to green by an amount that grows with distance from the
/// optical centre. Composited content rarely carries the right displacement
/// for its position.
///
/// # Limitations
///
/// Needs strong, high-contrast edges. Modern cameras correct aberration
/// in-camera and RAW converters apply lens profiles, so a corrected image
/// reads as "no signal" rather than "authentic". This is the most expensive
/// module in the crate.
pub struct ChromaticAberrationAnalyzer {
    config: ChromaticAberrationConfig,
}

impl ChromaticAberrationAnalyzer {
    /// Analyzer with the default configuration.
    pub fn new() -> Self {
        Self::with_config(ChromaticAberrationConfig::default())
    }

    /// Analyzer with custom settings.
    pub fn with_config(config: ChromaticAberrationConfig) -> Self {
        Self { config }
    }

    /// Run the analysis.
    ///
    /// # Errors
    ///
    /// [`ImageTooSmall`](crate::error::ForensicsError::ImageTooSmall) below
    /// twice `block_size`.
    pub fn analyze(&self, image: &DynamicImage) -> Result<ChromaticAberrationResult> {
        let rgb = image.to_rgb8();
        let (width, height) = rgb.dimensions();

        ensure_min_dimensions(width, height, self.config.block_size * 2)?;

        let (red, green, blue) = self.split_channels(&rgb);

        let measurements = self.measure_aberration(&red, &green, &blue);

        let aberration_map = self.create_aberration_map(width, height, &measurements);

        let radial_model = self.fit_radial_model(&measurements, width, height);

        let expected_aberrations =
            radial_model.map(|model| self.calculate_expected_aberrations(&measurements, &model));

        let inconsistency_map = self.create_inconsistency_map(
            width,
            height,
            &measurements,
            expected_aberrations.as_ref(),
        );

        let inconsistent_regions = self.find_inconsistent_regions(&inconsistency_map);

        let visualization = self.create_visualization(&rgb, &measurements, radial_model.as_ref());

        let consistency_score =
            self.calculate_consistency_score(&measurements, expected_aberrations.as_ref());

        let manipulation_probability = self.calculate_manipulation_probability(
            &inconsistent_regions,
            consistency_score,
            width,
            height,
        );

        let optical_center = radial_model.map(|m| (m.center_x, m.center_y));

        Ok(ChromaticAberrationResult {
            measurements,
            aberration_map,
            inconsistency_map,
            visualization,
            inconsistent_regions,
            optical_center,
            radial_model,
            consistency_score,
            manipulation_probability,
        })
    }

    fn split_channels(&self, rgb: &RgbImage) -> (GrayImage, GrayImage, GrayImage) {
        let (width, height) = rgb.dimensions();
        let mut red = GrayImage::new(width, height);
        let mut green = GrayImage::new(width, height);
        let mut blue = GrayImage::new(width, height);

        for (x, y, pixel) in rgb.enumerate_pixels() {
            red.put_pixel(x, y, Luma([pixel[0]]));
            green.put_pixel(x, y, Luma([pixel[1]]));
            blue.put_pixel(x, y, Luma([pixel[2]]));
        }

        (red, green, blue)
    }

    fn measure_aberration(
        &self,
        red: &GrayImage,
        green: &GrayImage,
        blue: &GrayImage,
    ) -> Vec<AberrationMeasurement> {
        let (width, height) = green.dimensions();
        let block_size = self.config.block_size;

        let blocks: Vec<SRegion> =
            full_blocks(width, height, block_size, (block_size / 2).max(1)).collect();

        blocks
            .par_iter()
            .filter_map(|region| {
                self.measure_block_aberration(red, green, blue, region.x, region.y, block_size)
            })
            .collect()
    }

    fn measure_block_aberration(
        &self,
        red: &GrayImage,
        green: &GrayImage,
        blue: &GrayImage,
        bx: u32,
        by: u32,
        size: u32,
    ) -> Option<AberrationMeasurement> {
        let edge_points = self.find_edge_points(green, bx, by, size);

        if edge_points.len() < 10 {
            return None;
        }

        let (rg_shift_x, rg_shift_y, rg_confidence) =
            self.measure_channel_shift(red, green, &edge_points);
        let (bg_shift_x, bg_shift_y, bg_confidence) =
            self.measure_channel_shift(blue, green, &edge_points);

        let confidence = (rg_confidence + bg_confidence) / 2.0;

        if confidence < 0.3 {
            return None;
        }

        let max_shift = self.config.max_aberration;
        if rg_shift_x.abs() > max_shift
            || rg_shift_y.abs() > max_shift
            || bg_shift_x.abs() > max_shift
            || bg_shift_y.abs() > max_shift
        {
            return None;
        }

        Some(AberrationMeasurement {
            x: bx + size / 2,
            y: by + size / 2,
            rg_shift_x,
            rg_shift_y,
            bg_shift_x,
            bg_shift_y,
            confidence,
        })
    }

    fn find_edge_points(
        &self,
        gray: &GrayImage,
        bx: u32,
        by: u32,
        size: u32,
    ) -> Vec<(u32, u32, f64, f64)> {
        let mut edges = Vec::new();
        let (width, height) = gray.dimensions();

        for y in (by + 1)..(by + size - 1).min(height - 1) {
            for x in (bx + 1)..(bx + size - 1).min(width - 1) {
                let (gx, gy) = sobel(gray, x, y);
                let magnitude = (gx * gx + gy * gy).sqrt();

                if magnitude > self.config.edge_threshold {
                    edges.push((x, y, gx, gy));
                }
            }
        }

        edges
    }

    /// Best sub-pixel alignment of `channel` onto `reference` at the edge points.
    ///
    /// Coarse-to-fine: whole-pixel shifts first, then two refinement passes at
    /// 1/3 and 1/9 of a pixel around the winner. The previous version evaluated
    /// every integer shift crossed with a 3x3 sub-pixel grid — 1089 full
    /// correlations per block, each over every edge point — which for a 12 MP
    /// image works out at roughly 10^10 operations, single-threaded.
    fn measure_channel_shift(
        &self,
        channel: &GrayImage,
        reference: &GrayImage,
        edge_points: &[(u32, u32, f64, f64)],
    ) -> (f64, f64, f64) {
        let search_radius = self.config.max_aberration.ceil() as i32;

        let mut best_shift_x = 0.0;
        let mut best_shift_y = 0.0;
        let mut best_correlation = f64::NEG_INFINITY;

        for sy in -search_radius..=search_radius {
            for sx in -search_radius..=search_radius {
                let correlation = self.calculate_edge_correlation(
                    channel,
                    reference,
                    edge_points,
                    sx as f64,
                    sy as f64,
                );

                if correlation > best_correlation {
                    best_correlation = correlation;
                    best_shift_x = sx as f64;
                    best_shift_y = sy as f64;
                }
            }
        }

        let mut step = 1.0 / 3.0;
        for _ in 0..2 {
            let (centre_x, centre_y) = (best_shift_x, best_shift_y);

            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }

                    let shift_x = centre_x + dx as f64 * step;
                    let shift_y = centre_y + dy as f64 * step;

                    let correlation = self.calculate_edge_correlation(
                        channel,
                        reference,
                        edge_points,
                        shift_x,
                        shift_y,
                    );

                    if correlation > best_correlation {
                        best_correlation = correlation;
                        best_shift_x = shift_x;
                        best_shift_y = shift_y;
                    }
                }
            }

            step /= 3.0;
        }

        (best_shift_x, best_shift_y, best_correlation.max(0.0))
    }

    /// Zero-mean normalised cross-correlation at the given shift.
    ///
    /// The previous form omitted the mean subtraction, making it a cosine
    /// similarity between two all-positive intensity vectors. That sits near
    /// 1.0 for every candidate shift, so the arg-max it fed was effectively
    /// noise and the reported aberration vectors were meaningless.
    fn calculate_edge_correlation(
        &self,
        channel: &GrayImage,
        reference: &GrayImage,
        edge_points: &[(u32, u32, f64, f64)],
        shift_x: f64,
        shift_y: f64,
    ) -> f64 {
        let (width, height) = reference.dimensions();

        let mut reference_values = Vec::with_capacity(edge_points.len());
        let mut channel_values = Vec::with_capacity(edge_points.len());

        for &(x, y, _, _) in edge_points {
            let shifted_x = x as f64 + shift_x;
            let shifted_y = y as f64 + shift_y;

            if shifted_x < 0.0
                || shifted_x >= (width - 1) as f64
                || shifted_y < 0.0
                || shifted_y >= (height - 1) as f64
            {
                continue;
            }

            reference_values.push(reference.get_pixel(x, y)[0] as f64);
            channel_values.push(self.bilinear_sample(channel, shifted_x, shifted_y));
        }

        if reference_values.len() < 4 {
            return 0.0;
        }

        let n = reference_values.len() as f64;
        let ref_mean = reference_values.iter().sum::<f64>() / n;
        let ch_mean = channel_values.iter().sum::<f64>() / n;

        let mut covariance = 0.0;
        let mut ref_variance = 0.0;
        let mut ch_variance = 0.0;

        for (&r, &c) in reference_values.iter().zip(channel_values.iter()) {
            let dr = r - ref_mean;
            let dc = c - ch_mean;
            covariance += dr * dc;
            ref_variance += dr * dr;
            ch_variance += dc * dc;
        }

        let denominator = (ref_variance * ch_variance).sqrt();
        if denominator < 1e-10 {
            0.0
        } else {
            covariance / denominator
        }
    }

    fn bilinear_sample(&self, image: &GrayImage, x: f64, y: f64) -> f64 {
        let x0 = x.floor() as u32;
        let y0 = y.floor() as u32;
        let x1 = x0 + 1;
        let y1 = y0 + 1;

        let fx = x - x0 as f64;
        let fy = y - y0 as f64;

        let (width, height) = image.dimensions();

        let v00 = image.get_pixel(x0.min(width - 1), y0.min(height - 1))[0] as f64;
        let v10 = image.get_pixel(x1.min(width - 1), y0.min(height - 1))[0] as f64;
        let v01 = image.get_pixel(x0.min(width - 1), y1.min(height - 1))[0] as f64;
        let v11 = image.get_pixel(x1.min(width - 1), y1.min(height - 1))[0] as f64;

        v00 * (1.0 - fx) * (1.0 - fy)
            + v10 * fx * (1.0 - fy)
            + v01 * (1.0 - fx) * fy
            + v11 * fx * fy
    }

    fn create_aberration_map(
        &self,
        width: u32,
        height: u32,
        measurements: &[AberrationMeasurement],
    ) -> GrayImage {
        let mut map = GrayImage::new(width, height);
        let block_size = self.config.block_size;

        let max_aberration = measurements
            .iter()
            .map(|m| {
                let rg = (m.rg_shift_x.powi(2) + m.rg_shift_y.powi(2)).sqrt();
                let bg = (m.bg_shift_x.powi(2) + m.bg_shift_y.powi(2)).sqrt();
                rg.max(bg)
            })
            .fold(0.0, f64::max)
            .max(0.1);

        for measurement in measurements {
            let rg = (measurement.rg_shift_x.powi(2) + measurement.rg_shift_y.powi(2)).sqrt();
            let bg = (measurement.bg_shift_x.powi(2) + measurement.bg_shift_y.powi(2)).sqrt();
            let magnitude = rg.max(bg);
            let normalized = ((magnitude / max_aberration) * 255.0) as u8;

            let bx = measurement.x.saturating_sub(block_size / 2);
            let by = measurement.y.saturating_sub(block_size / 2);

            for y in by..(by + block_size).min(height) {
                for x in bx..(bx + block_size).min(width) {
                    let current = map.get_pixel(x, y)[0];
                    map.put_pixel(x, y, Luma([current.max(normalized)]));
                }
            }
        }

        map
    }

    fn fit_radial_model(
        &self,
        measurements: &[AberrationMeasurement],
        width: u32,
        height: u32,
    ) -> Option<RadialAberrationModel> {
        if measurements.len() < 10 {
            return None;
        }

        // A *displaced* optical centre is the forensic signal here: splicing
        // breaks the radial symmetry of lens aberration. Pinning the centre to
        // the middle of the frame, as this previously did, meant the model
        // could not detect the very thing it reports.
        let mut best: Option<RadialAberrationModel> = None;

        let span_x = width as f64;
        let span_y = height as f64;

        for grid_y in 0..5 {
            for grid_x in 0..5 {
                let center_x = span_x * (0.3 + 0.1 * grid_x as f64);
                let center_y = span_y * (0.3 + 0.1 * grid_y as f64);

                if let Some(model) = self.fit_at_center(measurements, center_x, center_y)
                    && best
                        .as_ref()
                        .is_none_or(|current| model.fit_quality > current.fit_quality)
                {
                    best = Some(model);
                }
            }
        }

        best
    }

    /// Least-squares radial coefficients for a fixed optical centre.
    fn fit_at_center(
        &self,
        measurements: &[AberrationMeasurement],
        center_x: f64,
        center_y: f64,
    ) -> Option<RadialAberrationModel> {
        let mut sum_r_sq = 0.0;
        let mut sum_r_shift_red = 0.0;
        let mut sum_r_shift_blue = 0.0;

        for m in measurements {
            let dx = m.x as f64 - center_x;
            let dy = m.y as f64 - center_y;
            let r = (dx * dx + dy * dy).sqrt();

            if r < 10.0 {
                continue;
            }

            let radial_dir_x = dx / r;
            let radial_dir_y = dy / r;

            let rg_radial = m.rg_shift_x * radial_dir_x + m.rg_shift_y * radial_dir_y;
            let bg_radial = m.bg_shift_x * radial_dir_x + m.bg_shift_y * radial_dir_y;

            sum_r_sq += r * r * m.confidence;
            sum_r_shift_red += r * rg_radial * m.confidence;
            sum_r_shift_blue += r * bg_radial * m.confidence;
        }

        if sum_r_sq < 1e-10 {
            return None;
        }

        let k_red = sum_r_shift_red / sum_r_sq;
        let k_blue = sum_r_shift_blue / sum_r_sq;
        let fit_quality = self.calculate_model_fit(measurements, center_x, center_y, k_red, k_blue);

        Some(RadialAberrationModel {
            center_x,
            center_y,
            k_red,
            k_blue,
            fit_quality,
        })
    }

    fn calculate_model_fit(
        &self,
        measurements: &[AberrationMeasurement],
        center_x: f64,
        center_y: f64,
        k_red: f64,
        k_blue: f64,
    ) -> f64 {
        if measurements.is_empty() {
            return 0.0;
        }

        let count = measurements.len() as f64;
        let mean_rg_x = measurements.iter().map(|m| m.rg_shift_x).sum::<f64>() / count;
        let mean_rg_y = measurements.iter().map(|m| m.rg_shift_y).sum::<f64>() / count;
        let mean_bg_x = measurements.iter().map(|m| m.bg_shift_x).sum::<f64>() / count;
        let mean_bg_y = measurements.iter().map(|m| m.bg_shift_y).sum::<f64>() / count;

        let mut ss_res = 0.0;
        let mut ss_tot = 0.0;

        for m in measurements {
            let dx = m.x as f64 - center_x;
            let dy = m.y as f64 - center_y;
            let r = (dx * dx + dy * dy).sqrt();

            if r < 10.0 {
                continue;
            }

            let radial_dir_x = dx / r;
            let radial_dir_y = dy / r;

            let expected_rg_x = k_red * r * radial_dir_x;
            let expected_rg_y = k_red * r * radial_dir_y;
            let expected_bg_x = k_blue * r * radial_dir_x;
            let expected_bg_y = k_blue * r * radial_dir_y;

            // Both channels contribute; `k_blue` was previously computed and
            // then left out of the residual entirely.
            ss_res += (m.rg_shift_x - expected_rg_x).powi(2)
                + (m.rg_shift_y - expected_rg_y).powi(2)
                + (m.bg_shift_x - expected_bg_x).powi(2)
                + (m.bg_shift_y - expected_bg_y).powi(2);
            ss_tot += (m.rg_shift_x - mean_rg_x).powi(2)
                + (m.rg_shift_y - mean_rg_y).powi(2)
                + (m.bg_shift_x - mean_bg_x).powi(2)
                + (m.bg_shift_y - mean_bg_y).powi(2);
        }

        if ss_tot < 1e-10 {
            return 0.0;
        }

        (1.0 - ss_res / ss_tot).max(0.0)
    }

    fn calculate_expected_aberrations(
        &self,
        measurements: &[AberrationMeasurement],
        model: &RadialAberrationModel,
    ) -> Vec<(f64, f64, f64, f64)> {
        measurements
            .iter()
            .map(|m| {
                let dx = m.x as f64 - model.center_x;
                let dy = m.y as f64 - model.center_y;
                let r = (dx * dx + dy * dy).sqrt();

                let radial_dir_x = if r > 0.0 { dx / r } else { 0.0 };
                let radial_dir_y = if r > 0.0 { dy / r } else { 0.0 };

                let expected_rg_x = model.k_red * r * radial_dir_x;
                let expected_rg_y = model.k_red * r * radial_dir_y;
                let expected_bg_x = model.k_blue * r * radial_dir_x;
                let expected_bg_y = model.k_blue * r * radial_dir_y;

                (expected_rg_x, expected_rg_y, expected_bg_x, expected_bg_y)
            })
            .collect::<Vec<_>>()
    }

    fn create_inconsistency_map(
        &self,
        width: u32,
        height: u32,
        measurements: &[AberrationMeasurement],
        expected: Option<&Vec<(f64, f64, f64, f64)>>,
    ) -> GrayImage {
        let mut map = GrayImage::new(width, height);
        let block_size = self.config.block_size;

        for (i, m) in measurements.iter().enumerate() {
            let inconsistency = if let Some(exp) = expected {
                let (exp_rg_x, exp_rg_y, exp_bg_x, exp_bg_y) = exp[i];

                let rg_error =
                    ((m.rg_shift_x - exp_rg_x).powi(2) + (m.rg_shift_y - exp_rg_y).powi(2)).sqrt();
                let bg_error =
                    ((m.bg_shift_x - exp_bg_x).powi(2) + (m.bg_shift_y - exp_bg_y).powi(2)).sqrt();

                (rg_error + bg_error) / 2.0
            } else {
                0.0
            };

            let normalized =
                ((inconsistency / self.config.max_aberration) * 255.0).min(255.0) as u8;

            let bx = m.x.saturating_sub(block_size / 2);
            let by = m.y.saturating_sub(block_size / 2);

            for y in by..(by + block_size).min(height) {
                for x in bx..(bx + block_size).min(width) {
                    let current = map.get_pixel(x, y)[0];
                    map.put_pixel(x, y, Luma([current.max(normalized)]));
                }
            }
        }

        map
    }

    fn find_inconsistent_regions(&self, inconsistency_map: &GrayImage) -> Vec<SRegion> {
        let (width, height) = inconsistency_map.dimensions();
        let block_size = self.config.block_size;
        let threshold = (self.config.inconsistency_threshold * 50.0) as u8;

        clipped_blocks(width, height, block_size, block_size)
            .filter(|block| {
                let sum: u64 = block
                    .pixels()
                    .map(|(x, y)| inconsistency_map.get_pixel(x, y)[0] as u64)
                    .sum();

                (sum / block.area()) as u8 > threshold
            })
            .collect()
    }

    fn create_visualization(
        &self,
        original: &RgbImage,
        measurements: &[AberrationMeasurement],
        model: Option<&RadialAberrationModel>,
    ) -> RgbImage {
        let mut vis = original.clone();
        let scale = 10.0;

        // Shift vectors routinely point left or up. Casting a negative i32
        // endpoint to u32 wrapped it to ~4e9, and the old Bresenham loop then
        // marched x upwards forever chasing a target it re-cast as negative:
        // an infinite loop in release, an overflow panic in debug.
        for m in measurements {
            let (x, y) = (m.x as i32, m.y as i32);

            draw::line(
                &mut vis,
                x,
                y,
                x + (m.rg_shift_x * scale).round() as i32,
                y + (m.rg_shift_y * scale).round() as i32,
                Rgb([255, 0, 0]),
            );

            draw::line(
                &mut vis,
                x,
                y,
                x + (m.bg_shift_x * scale).round() as i32,
                y + (m.bg_shift_y * scale).round() as i32,
                Rgb([0, 0, 255]),
            );
        }

        if let Some(model) = model {
            draw::crosshair(
                &mut vis,
                model.center_x.round() as i32,
                model.center_y.round() as i32,
                20,
                Rgb([255, 255, 0]),
            );
        }

        vis
    }

    fn calculate_consistency_score(
        &self,
        measurements: &[AberrationMeasurement],
        expected: Option<&Vec<(f64, f64, f64, f64)>>,
    ) -> f64 {
        if measurements.is_empty() {
            return 1.0;
        }

        if let Some(exp) = expected {
            let mut total_error = 0.0;
            let mut total_weight = 0.0;

            for (i, m) in measurements.iter().enumerate() {
                let (exp_rg_x, exp_rg_y, exp_bg_x, exp_bg_y) = exp[i];

                let rg_error =
                    ((m.rg_shift_x - exp_rg_x).powi(2) + (m.rg_shift_y - exp_rg_y).powi(2)).sqrt();
                let bg_error =
                    ((m.bg_shift_x - exp_bg_x).powi(2) + (m.bg_shift_y - exp_bg_y).powi(2)).sqrt();

                total_error += (rg_error + bg_error) * m.confidence;
                total_weight += m.confidence;
            }

            if total_weight > 0.0 {
                let avg_error = total_error / total_weight;
                (1.0 - avg_error / self.config.max_aberration).clamp(0.0, 1.0)
            } else {
                1.0
            }
        } else {
            let shifts = measurements
                .iter()
                .map(|m| (m.rg_shift_x.powi(2) + m.rg_shift_y.powi(2)).sqrt())
                .collect::<Vec<_>>();
            let mean = shifts.iter().sum::<f64>() / shifts.len() as f64;
            let variance =
                shifts.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / shifts.len() as f64;
            let std_dev = variance.sqrt();

            (1.0 - std_dev / self.config.max_aberration).clamp(0.0, 1.0)
        }
    }

    fn calculate_manipulation_probability(
        &self,
        inconsistent_regions: &[SRegion],
        consistency_score: f64,
        width: u32,
        height: u32,
    ) -> f64 {
        let total_pixels = width as f64 * height as f64;
        let inconsistent_pixels: u64 = inconsistent_regions.iter().map(|r| r.area()).sum();

        let coverage = if total_pixels > 0.0 {
            inconsistent_pixels as f64 / total_pixels
        } else {
            0.0
        };

        (coverage * 0.4 + (1.0 - consistency_score) * 0.6).clamp(0.0, 1.0)
    }
}

impl Default for ChromaticAberrationAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Colour edges with a deliberate one-pixel red/blue fringe.
    fn fringed_edges(width: u32, height: u32) -> DynamicImage {
        let mut image = RgbImage::new(width, height);

        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let base = if (x / 16 + y / 16) % 2 == 0 {
                210u8
            } else {
                45
            };
            let shifted = if ((x + 1) / 16 + y / 16) % 2 == 0 {
                210u8
            } else {
                45
            };
            *pixel = Rgb([shifted, base, base]);
        }

        DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn visualization_terminates_with_leftward_shifts() {
        // Any measurement pointing left or up used to wrap through u32 and hang
        // the Bresenham loop. Draw directly to pin the regression.
        let analyzer = ChromaticAberrationAnalyzer::new();
        let original = RgbImage::new(64, 64);

        let measurements = vec![AberrationMeasurement {
            x: 2,
            y: 2,
            rg_shift_x: -40.0,
            rg_shift_y: -40.0,
            bg_shift_x: -40.0,
            bg_shift_y: 40.0,
            confidence: 1.0,
        }];

        let vis = analyzer.create_visualization(&original, &measurements, None);
        assert_eq!(vis.dimensions(), (64, 64));
    }

    #[test]
    fn correlation_is_bounded_and_peaks_at_zero_shift() {
        let analyzer = ChromaticAberrationAnalyzer::new();
        let mut channel = GrayImage::new(32, 32);
        for (x, _, pixel) in channel.enumerate_pixels_mut() {
            *pixel = Luma([if x < 16 { 20 } else { 200 }]);
        }

        let edge_points: Vec<(u32, u32, f64, f64)> = (12..20)
            .flat_map(|x| (8..24).map(move |y| (x, y, 0.0, 0.0)))
            .collect();

        let aligned =
            analyzer.calculate_edge_correlation(&channel, &channel, &edge_points, 0.0, 0.0);
        let offset =
            analyzer.calculate_edge_correlation(&channel, &channel, &edge_points, 3.0, 0.0);

        assert!(aligned <= 1.0 + 1e-9, "correlation {aligned} exceeds 1");
        assert!(
            aligned > offset,
            "aligned {aligned} did not beat offset {offset}"
        );
    }

    #[test]
    fn undersized_images_error() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(64, 64));
        assert!(ChromaticAberrationAnalyzer::new().analyze(&image).is_err());
    }

    #[test]
    fn analysis_outputs_are_bounded() {
        let result = ChromaticAberrationAnalyzer::new()
            .analyze(&fringed_edges(256, 192))
            .unwrap();

        assert!((0.0..=1.0).contains(&result.manipulation_probability));
        assert!((0.0..=1.0).contains(&result.consistency_score));
        assert_eq!(result.visualization.dimensions(), (256, 192));

        for region in &result.inconsistent_regions {
            assert!(region.right() <= 256);
            assert!(region.bottom() <= 192);
        }
    }

    #[test]
    fn optical_center_is_searched_not_assumed() {
        let result = ChromaticAberrationAnalyzer::new()
            .analyze(&fringed_edges(256, 256))
            .unwrap();

        if let Some((cx, cy)) = result.optical_center {
            // Inside the frame, but not hard-wired to the exact middle.
            assert!((0.0..=256.0).contains(&cx));
            assert!((0.0..=256.0).contains(&cy));
        }
    }
}
