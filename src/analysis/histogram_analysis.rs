use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};

use crate::{
    error::Result,
    image_utils::{full_blocks, rgb_to_gray},
};

/// Settings for [`HistogramAnalyzer`].
#[derive(Debug, Clone)]
pub struct HistogramConfig {
    /// Tile for the local gap map. Default 64.
    pub block_size: u32,
    /// A level with at most this many pixels counts as a gap.
    pub gap_threshold: u32,
    /// Reserved for future peak filtering.
    pub peak_threshold: f64,
    /// Share of pixels at 0 or 255 before clipping is reported.
    pub clipping_threshold: f64,
}

impl Default for HistogramConfig {
    fn default() -> Self {
        Self {
            block_size: 64,
            gap_threshold: 0,
            peak_threshold: 0.1,
            clipping_threshold: 0.01,
        }
    }
}

/// A departure from the tonal distribution of an unedited photograph.
#[derive(Debug, Clone)]
pub enum HistogramAnomaly {
    /// Empty levels between the darkest and brightest occupied bins, as a
    /// contrast stretch or levels adjustment leaves behind.
    Gap {
        /// How many levels are empty.
        count: usize,
        /// Which levels.
        positions: Vec<u8>,
    },
    /// Regular alternation between full and empty bins, from a strong curve or
    /// repeated tonal edits.
    CombPattern {
        /// Spacing between teeth, in levels.
        period: f64,
        /// Share of bins participating, in `[0, 1]`.
        strength: f64,
    },
    /// Mass piled at level 0: the black point was raised, or the shot was
    /// underexposed.
    ShadowClipping {
        /// Share of all pixels sitting at zero.
        percentage: f64,
    },
    /// Mass piled at level 255: the white point was lowered, or highlights blew.
    HighlightClipping {
        /// Share of all pixels sitting at 255.
        percentage: f64,
    },
    /// A spike far above its neighbours, from compressed levels or a large flat
    /// synthetic region.
    UnusualPeak {
        /// Level the spike sits at.
        position: u8,
        /// Share of all pixels in that one bin.
        height: f64,
    },
    /// The image never reaches full black or full white.
    TruncatedRange {
        /// Darkest occupied level.
        min: u8,
        /// Brightest occupied level.
        max: u8,
    },
}

/// Output of [`HistogramAnalyzer`].
#[derive(Debug, Clone)]
pub struct HistogramAnalysisResult {
    /// 256-bin luminance distribution.
    pub luminance_histogram: [u32; 256],
    /// 256-bin red channel distribution.
    pub red_histogram: [u32; 256],
    /// 256-bin green channel distribution.
    pub green_histogram: [u32; 256],
    /// 256-bin blue channel distribution.
    pub blue_histogram: [u32; 256],
    /// Everything unusual found in the distribution.
    pub anomalies: Vec<HistogramAnomaly>,
    /// Where the *local* histogram is combed, localising a partial adjustment.
    pub gaps_map: GrayImage,
    /// Weighted sum over the anomalies, in `[0, 1]`.
    pub manipulation_probability: f64,
    /// Gamma implied by the median tone, when it departs from linear by more than 15%.
    pub estimated_gamma: Option<f64>,
    /// Whether the gaps are evenly spaced, as a contrast stretch leaves them.
    pub contrast_stretched: bool,
    /// Whether any gaps were found at all.
    pub levels_adjusted: bool,
}

/// Placement of a single chart inside a larger canvas.
#[derive(Debug, Clone, Copy)]
struct PlotArea {
    x_offset: u32,
    y_offset: u32,
    plot_width: u32,
    plot_height: u32,
}

/// Combs, gaps and clipping left by levels, curves and gamma adjustments.
///
/// Stretching contrast spreads existing levels apart and leaves empty bins;
/// compressing them stacks counts into spikes. None of that is visible in the
/// image, but all of it is plain in the histogram.
///
/// # Limitations
///
/// Tonal editing is not forgery. Every raw conversion and every filter combs
/// the histogram; this module detects *adjustment*, which says nothing about
/// whether content was altered.
pub struct HistogramAnalyzer {
    config: HistogramConfig,
}

impl HistogramAnalyzer {
    /// Analyzer with the default configuration.
    pub fn new() -> Self {
        Self::with_config(HistogramConfig::default())
    }

    /// Analyzer with custom settings.
    pub fn with_config(config: HistogramConfig) -> Self {
        Self { config }
    }

    /// Run the analysis. Accepts any image size.
    pub fn analyze(&self, image: &DynamicImage) -> Result<HistogramAnalysisResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);
        let (width, height) = gray.dimensions();

        let luminance_histogram = self.compute_histogram(&gray);
        let (red_histogram, green_histogram, blue_histogram) = self.compute_rgb_histograms(&rgb);

        let mut anomalies = Vec::new();

        let gaps = self.detect_gaps(&luminance_histogram);
        if !gaps.is_empty() {
            anomalies.push(HistogramAnomaly::Gap {
                count: gaps.len(),
                positions: gaps,
            });
        }

        if let Some((period, strength)) = self.detect_comb_pattern(&luminance_histogram) {
            anomalies.push(HistogramAnomaly::CombPattern { period, strength });
        }

        let total_pixels = (width * height) as f64;
        let shadow_clip = luminance_histogram[0] as f64 / total_pixels;
        let highlight_clip = luminance_histogram[255] as f64 / total_pixels;

        if shadow_clip > self.config.clipping_threshold {
            anomalies.push(HistogramAnomaly::ShadowClipping {
                percentage: shadow_clip,
            });
        }

        if highlight_clip > self.config.clipping_threshold {
            anomalies.push(HistogramAnomaly::HighlightClipping {
                percentage: highlight_clip,
            });
        }

        let peaks = self.detect_unusual_peaks(&luminance_histogram, total_pixels);
        anomalies.extend(peaks);

        if let Some((min, max)) = self.detect_truncated_range(&luminance_histogram) {
            anomalies.push(HistogramAnomaly::TruncatedRange { min, max });
        }

        let gaps_map = self.create_gaps_map(&gray);
        let estimated_gamma = self.estimate_gamma(&luminance_histogram);
        let contrast_stretched = self.detect_contrast_stretch(&luminance_histogram);
        let levels_adjusted = !self.detect_gaps(&luminance_histogram).is_empty();
        let manipulation_probability = self.calculate_manipulation_probability(&anomalies);

        Ok(HistogramAnalysisResult {
            luminance_histogram,
            red_histogram,
            green_histogram,
            blue_histogram,
            anomalies,
            gaps_map,
            manipulation_probability,
            estimated_gamma,
            contrast_stretched,
            levels_adjusted,
        })
    }

    fn compute_histogram(&self, gray: &GrayImage) -> [u32; 256] {
        let mut histogram = [0u32; 256];

        for pixel in gray.pixels() {
            histogram[pixel[0] as usize] += 1;
        }

        histogram
    }

    fn compute_rgb_histograms(&self, rgb: &RgbImage) -> ([u32; 256], [u32; 256], [u32; 256]) {
        let mut red = [0u32; 256];
        let mut green = [0u32; 256];
        let mut blue = [0u32; 256];

        for pixel in rgb.pixels() {
            red[pixel[0] as usize] += 1;
            green[pixel[1] as usize] += 1;
            blue[pixel[2] as usize] += 1;
        }

        (red, green, blue)
    }

    /// Empty levels between the darkest and brightest occupied bins.
    ///
    /// Gaps outside the occupied range are just unused headroom, not evidence.
    fn detect_gaps(&self, histogram: &[u32; 256]) -> Vec<u8> {
        let first_nonzero = histogram.iter().position(|&x| x > 0).unwrap_or(0);
        let last_nonzero = histogram.iter().rposition(|&x| x > 0).unwrap_or(255);

        histogram[first_nonzero..=last_nonzero]
            .iter()
            .enumerate()
            .filter(|&(_, &count)| count <= self.config.gap_threshold)
            .map(|(offset, _)| (first_nonzero + offset) as u8)
            .collect()
    }

    fn detect_comb_pattern(&self, histogram: &[u32; 256]) -> Option<(f64, f64)> {
        let mut alterations = 0;
        let mut total_checks = 0;

        let mean = histogram.iter().sum::<u32>() as f64 / 256.0;

        for window in histogram.windows(3) {
            let (prev, curr, next) = (window[0] as f64, window[1] as f64, window[2] as f64);

            let is_local_extreme = (curr > prev && curr > next) || (curr < prev && curr < next);

            if is_local_extreme && curr > mean * 0.1 {
                alterations += 1;
            }
            total_checks += 1;
        }

        let alteration_rate = alterations as f64 / total_checks as f64;

        if alteration_rate > 0.3 {
            Some((2.0, alteration_rate))
        } else {
            None
        }
    }

    fn detect_unusual_peaks(&self, histogram: &[u32; 256], total: f64) -> Vec<HistogramAnomaly> {
        let mean = total / 256.0;

        histogram
            .windows(3)
            .enumerate()
            .filter_map(|(i, window)| {
                let (prev, curr, next) = (window[0] as f64, window[1] as f64, window[2] as f64);

                (curr > prev * 3.0 && curr > next * 3.0 && curr > mean * 5.0).then(|| {
                    HistogramAnomaly::UnusualPeak {
                        // `windows` is offset by one from the bin index.
                        position: (i + 1) as u8,
                        height: curr / total,
                    }
                })
            })
            .collect()
    }

    fn detect_truncated_range(&self, histogram: &[u32; 256]) -> Option<(u8, u8)> {
        let first_nonzero = histogram.iter().position(|&x| x > 0)?;
        let last_nonzero = histogram.iter().rposition(|&x| x > 0)?;

        if first_nonzero > 20 || last_nonzero < 235 {
            Some((first_nonzero as u8, last_nonzero as u8))
        } else {
            None
        }
    }

    fn create_gaps_map(&self, gray: &GrayImage) -> GrayImage {
        let (width, height) = gray.dimensions();
        let block_size = self.config.block_size;
        let mut gaps_map = GrayImage::new(width, height);

        // `full_blocks` yields nothing when the image is smaller than one
        // block. The `0..height - block_size` loop this replaces underflowed
        // instead, panicking in debug and spinning through ~4e9 iterations in
        // release for any image under 64 px. This module has no minimum-size
        // guard, so undersized input reached it routinely.
        for region in full_blocks(width, height, block_size, (block_size / 2).max(1)) {
            let mut local_hist = [0u32; 256];

            for (x, y) in region.pixels() {
                local_hist[gray.get_pixel(x, y)[0] as usize] += 1;
            }

            let gaps = self.detect_gaps(&local_hist);
            let gap_ratio = gaps.len() as f64 / 256.0;
            let value = (gap_ratio * 255.0 * 4.0).min(255.0) as u8;

            for (x, y) in region.pixels() {
                gaps_map.put_pixel(x, y, Luma([value]));
            }
        }

        gaps_map
    }

    /// Gamma implied by the median tone, when it departs from a neutral image.
    ///
    /// This inverts `median = 0.5^(1/gamma)`, so a mid-grey median gives
    /// gamma 1. It is an indication, not a measurement: only a value far enough
    /// from 1 to be unlikely under normal exposure is reported. Deriving it
    /// from the *mean* and returning it unconditionally, as before, produced a
    /// plausible-looking number for essentially every image.
    fn estimate_gamma(&self, histogram: &[u32; 256]) -> Option<f64> {
        let total = histogram.iter().map(|&x| x as u64).sum::<u64>();
        if total == 0 {
            return None;
        }

        let half = total / 2;
        let mut running = 0u64;
        let mut median_level = 0usize;

        for (level, &count) in histogram.iter().enumerate() {
            running += count as u64;
            if running >= half {
                median_level = level;
                break;
            }
        }

        let median = median_level as f64 / 255.0;
        if !(0.02..=0.98).contains(&median) {
            return None;
        }

        let gamma = 0.5_f64.ln() / median.ln();

        // Within 15% of linear is ordinary exposure variation, not correction.
        if (0.2..=5.0).contains(&gamma) && (gamma - 1.0).abs() > 0.15 {
            Some(gamma)
        } else {
            None
        }
    }

    fn detect_contrast_stretch(&self, histogram: &[u32; 256]) -> bool {
        let gaps = self.detect_gaps(histogram);

        if gaps.len() > 10 {
            let diffs: Vec<u8> = gaps.windows(2).map(|pair| pair[1] - pair[0]).collect();

            if !diffs.is_empty() {
                let mean_diff = diffs.iter().map(|&d| d as f64).sum::<f64>() / diffs.len() as f64;
                let variance = diffs
                    .iter()
                    .map(|&d| (d as f64 - mean_diff).powi(2))
                    .sum::<f64>()
                    / diffs.len() as f64;

                if variance < 1.0 {
                    return true;
                }
            }
        }

        false
    }

    fn calculate_manipulation_probability(&self, anomalies: &[HistogramAnomaly]) -> f64 {
        if anomalies.is_empty() {
            return 0.0;
        }

        let mut probability = 0.0;

        for anomaly in anomalies {
            match anomaly {
                HistogramAnomaly::Gap { count, .. } => {
                    probability += (*count as f64 / 50.0).min(0.3);
                }
                HistogramAnomaly::CombPattern { strength, .. } => {
                    probability += strength * 0.4;
                }
                HistogramAnomaly::ShadowClipping { percentage } => {
                    probability += (percentage * 10.0).min(0.2);
                }
                HistogramAnomaly::HighlightClipping { percentage } => {
                    probability += (percentage * 10.0).min(0.2);
                }
                HistogramAnomaly::UnusualPeak { height, .. } => {
                    probability += (height * 5.0).min(0.2);
                }
                HistogramAnomaly::TruncatedRange { min, max } => {
                    let range = *max as i32 - *min as i32;
                    probability += (255 - range) as f64 / 255.0 * 0.3;
                }
            }
        }

        probability.min(1.0)
    }

    /// Render one histogram into a sub-rectangle of `canvas`.
    ///
    /// All writes are clipped, so a caller passing a plot larger than the
    /// canvas gets a truncated chart rather than a panic.
    fn draw_histogram(
        &self,
        canvas: &mut RgbImage,
        histogram: &[u32; 256],
        color: [u8; 3],
        plot: PlotArea,
    ) {
        let PlotArea {
            x_offset,
            y_offset,
            plot_width,
            plot_height,
        } = plot;

        let (canvas_width, canvas_height) = canvas.dimensions();
        let max_val = *histogram.iter().max().unwrap_or(&1).max(&1);

        for y in y_offset..(y_offset + plot_height).min(canvas_height) {
            for x in x_offset..(x_offset + plot_width).min(canvas_width) {
                canvas.put_pixel(x, y, Rgb([20, 20, 20]));
            }
        }

        let bar_width = (plot_width / 256).max(1);

        for (i, &count) in histogram.iter().enumerate() {
            let bar_height = (count as f64 / max_val as f64 * plot_height as f64) as u32;
            let x_start = x_offset + i as u32 * bar_width;

            for bw in 0..bar_width {
                let x = x_start + bw;
                if x >= x_offset + plot_width || x >= canvas_width {
                    break;
                }

                for h in 0..bar_height.min(plot_height) {
                    let y = y_offset + plot_height - 1 - h;
                    if y < canvas_height {
                        canvas.put_pixel(x, y, Rgb(color));
                    }
                }
            }
        }
    }

    /// Render the three channel histograms stacked vertically.
    pub fn render_rgb_histograms(&self, result: &HistogramAnalysisResult) -> RgbImage {
        let plot_width = 512u32;
        let plot_height = 200u32;
        let padding = 10u32;
        let label_height = 20u32;

        let total_width = plot_width + padding * 2;
        let total_height = (plot_height + padding + label_height) * 3 + padding;

        let mut canvas = RgbImage::from_pixel(total_width, total_height, Rgb([30, 30, 30]));

        let channels = [
            (&result.red_histogram, [255, 60, 60], "Red"),
            (&result.green_histogram, [60, 255, 60], "Green"),
            (&result.blue_histogram, [60, 60, 255], "Blue"),
        ];

        for (idx, (histogram, color, _label)) in channels.iter().enumerate() {
            let y_offset =
                padding + (idx as u32) * (plot_height + padding + label_height) + label_height;

            self.draw_histogram(
                &mut canvas,
                histogram,
                *color,
                PlotArea {
                    x_offset: padding,
                    y_offset,
                    plot_width,
                    plot_height,
                },
            );
        }

        canvas
    }

    /// Render the three channel histograms superimposed, additively blended.
    pub fn render_rgb_histograms_overlaid(&self, result: &HistogramAnalysisResult) -> RgbImage {
        let plot_width = 512u32;
        let plot_height = 300u32;
        let padding = 20u32;

        let total_width = plot_width + padding * 2;
        let total_height = plot_height + padding * 2;

        let mut canvas = RgbImage::from_pixel(total_width, total_height, Rgb([10, 10, 10]));

        let max_val = [
            &result.red_histogram,
            &result.green_histogram,
            &result.blue_histogram,
        ]
        .iter()
        .flat_map(|h| h.iter())
        .max()
        .copied()
        .unwrap_or(1)
        .max(1);

        let bar_width = (plot_width / 256).max(1);

        for i in 0..256u32 {
            let r_height = (result.red_histogram[i as usize] as f64 / max_val as f64
                * plot_height as f64) as u32;
            let g_height = (result.green_histogram[i as usize] as f64 / max_val as f64
                * plot_height as f64) as u32;
            let b_height = (result.blue_histogram[i as usize] as f64 / max_val as f64
                * plot_height as f64) as u32;

            let x_start = padding + i * bar_width;

            for bw in 0..bar_width {
                let x = x_start + bw;
                if x >= padding + plot_width {
                    break;
                }

                let max_h = r_height.max(g_height).max(b_height);
                for h in 0..max_h {
                    let y = padding + plot_height - 1 - h;

                    let existing = canvas.get_pixel(x, y).0;

                    let r = if h < r_height { 180 } else { 0 };
                    let g = if h < g_height { 180 } else { 0 };
                    let b = if h < b_height { 180 } else { 0 };

                    let new_r = (existing[0] as u16 + r as u16).min(255) as u8;
                    let new_g = (existing[1] as u16 + g as u16).min(255) as u8;
                    let new_b = (existing[2] as u16 + b as u16).min(255) as u8;

                    canvas.put_pixel(x, y, Rgb([new_r, new_g, new_b]));
                }
            }
        }

        canvas
    }
}

impl Default for HistogramAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    use super::*;

    fn solid(width: u32, height: u32, value: u8) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(
            width,
            height,
            Rgb([value, value, value]),
        ))
    }

    fn ramp(width: u32, height: u32) -> DynamicImage {
        let mut image = RgbImage::new(width, height);
        for (x, _, pixel) in image.enumerate_pixels_mut() {
            let v = ((x * 255) / width.max(1)) as u8;
            *pixel = Rgb([v, v, v]);
        }
        DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn tiny_images_do_not_panic() {
        // 32 x 32 is smaller than the 64 px default block: the previous
        // `0..height - block_size` underflowed here.
        for size in [1u32, 4, 17, 32, 63] {
            let result = HistogramAnalyzer::new().analyze(&solid(size, size, 128));
            assert!(result.is_ok(), "panicked or errored at {size}x{size}");
        }
    }

    #[test]
    fn gaps_map_matches_the_image_size() {
        let result = HistogramAnalyzer::new().analyze(&ramp(200, 120)).unwrap();
        assert_eq!(result.gaps_map.dimensions(), (200, 120));
    }

    #[test]
    fn neutral_image_reports_no_gamma_correction() {
        // A linear ramp has a mid-grey median, so no correction is implied.
        let result = HistogramAnalyzer::new().analyze(&ramp(256, 64)).unwrap();
        assert!(
            result.estimated_gamma.is_none(),
            "reported gamma {:?} for a neutral ramp",
            result.estimated_gamma
        );
    }

    #[test]
    fn dark_image_reports_a_gamma() {
        let result = HistogramAnalyzer::new()
            .analyze(&solid(64, 64, 20))
            .unwrap();
        assert!(result.estimated_gamma.is_some());
    }

    #[test]
    fn probability_is_bounded() {
        let result = HistogramAnalyzer::new().analyze(&ramp(128, 128)).unwrap();
        assert!((0.0..=1.0).contains(&result.manipulation_probability));
    }

    #[test]
    fn renderers_produce_non_empty_canvases() {
        let analyzer = HistogramAnalyzer::new();
        let result = analyzer.analyze(&ramp(128, 128)).unwrap();

        assert!(analyzer.render_rgb_histograms(&result).width() > 0);
        assert!(analyzer.render_rgb_histograms_overlaid(&result).width() > 0);
    }
}
