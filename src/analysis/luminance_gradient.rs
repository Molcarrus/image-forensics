use std::f64::consts::PI;

use image::{DynamicImage, GrayImage, Luma};

use crate::{
    SRegion,
    error::Result,
    image_utils::{angle_to_u8, clipped_blocks, rgb_to_gray, sobel_polar, u8_to_angle},
    region::merge_regions,
};

/// Shading direction, and blocks that disagree with the dominant lighting.
///
/// Surfaces lit from one direction shade consistently. An object composited
/// from a differently-lit source shades the wrong way.
///
/// # Limitations
///
/// Edges dominate the measurement: texture and object boundaries produce far
/// stronger gradients than surface shading, so on a detailed scene the
/// "dominant direction" largely reflects edge orientation statistics. Albedo
/// changes are indistinguishable from shading here.
pub struct LuminanceGradientAnalyzer {
    block_size: u32,
    /// Gradients weaker than this are treated as flat and ignored.
    magnitude_threshold: f64,
    /// A block deviating from the dominant direction by more than this is flagged.
    angle_tolerance: f64,
}

/// Output of [`LuminanceGradientAnalyzer`].
pub struct LuminanceGradientResult {
    /// Sobel magnitude at every pixel.
    pub gradient_map: GrayImage,
    /// Sobel orientation, packed into `0..=255` across `[-PI, PI]`.
    pub direction_map: GrayImage,
    /// Blocks whose shading disagrees with the dominant direction.
    pub inconsistent_regions: Vec<SRegion>,
    /// Dominant illumination direction in radians, within `[-PI, PI]`.
    /// Magnitude-weighted circular mean, in `[-PI, PI]`.
    pub dominant_direction: f64,
    /// Circular concentration of the gradient directions, within `[0, 1]`.
    /// Resultant length of that mean, in `[0, 1]`.
    pub direction_confidence: f64,
}

impl LuminanceGradientAnalyzer {
    /// Analyzer with the given tile size and default thresholds.
    pub fn new(block_size: u32) -> Self {
        Self {
            block_size: block_size.max(1),
            magnitude_threshold: 30.0,
            angle_tolerance: PI / 4.0,
        }
    }

    /// Angular deviation from the dominant direction before a block is flagged.
    pub fn with_angle_tolerance(mut self, radians: f64) -> Self {
        self.angle_tolerance = radians;
        self
    }

    /// Set the Sobel magnitude below which a pixel counts as flat.
    ///
    /// The default of 30 suits high-contrast scenes; a gentle luminance ramp
    /// carries only a few levels per pixel and needs a lower cutoff.
    pub fn with_magnitude_threshold(mut self, threshold: f64) -> Self {
        self.magnitude_threshold = threshold.max(0.0);
        self
    }

    /// Run the analysis. Accepts any image size.
    pub fn analyze(&self, image: &DynamicImage) -> Result<LuminanceGradientResult> {
        let gray = rgb_to_gray(&image.to_rgb8());
        let (width, height) = gray.dimensions();

        let mut gradient_map = GrayImage::new(width, height);
        let mut direction_map = GrayImage::new(width, height);
        let mut directions = Vec::new();

        for y in 0..height {
            for x in 0..width {
                let (magnitude, direction) = sobel_polar(&gray, x, y);

                gradient_map.put_pixel(x, y, Luma([magnitude.min(255.0) as u8]));
                direction_map.put_pixel(x, y, Luma([angle_to_u8(direction)]));

                if magnitude > self.magnitude_threshold {
                    directions.push((direction, magnitude));
                }
            }
        }

        let (dominant_direction, direction_confidence) = circular_mean(&directions);

        let inconsistent_regions =
            self.find_inconsistent_regions(&direction_map, &gradient_map, dominant_direction);

        Ok(LuminanceGradientResult {
            gradient_map,
            direction_map,
            inconsistent_regions,
            dominant_direction,
            direction_confidence,
        })
    }

    fn find_inconsistent_regions(
        &self,
        direction_map: &GrayImage,
        gradient_map: &GrayImage,
        dominant: f64,
    ) -> Vec<SRegion> {
        let (width, height) = direction_map.dimensions();

        let regions = clipped_blocks(width, height, self.block_size, self.block_size)
            .filter(|block| {
                // Average the directions as unit vectors. Averaging the raw u8
                // codes wraps incorrectly across the -PI/+PI seam.
                let mut samples = Vec::new();

                for (x, y) in block.pixels() {
                    let magnitude = gradient_map.get_pixel(x, y)[0] as f64;
                    if magnitude > self.magnitude_threshold * 0.65 {
                        samples.push((u8_to_angle(direction_map.get_pixel(x, y)[0]), magnitude));
                    }
                }

                // Require a quarter of the block to carry usable structure.
                if (samples.len() as u64) * 4 < block.area() {
                    return false;
                }

                let (block_direction, confidence) = circular_mean(&samples);
                confidence > 0.3
                    && angular_distance(block_direction, dominant) > self.angle_tolerance
            })
            .collect();

        merge_regions(regions, self.block_size / 2)
    }
}

/// Magnitude-weighted circular mean, returning `(angle, concentration)`.
///
/// The concentration is the resultant length: 1 when every sample agrees, 0
/// when they cancel out.
fn circular_mean(samples: &[(f64, f64)]) -> (f64, f64) {
    let mut sin_sum = 0.0;
    let mut cos_sum = 0.0;
    let mut weight_sum = 0.0;

    for &(angle, weight) in samples {
        sin_sum += angle.sin() * weight;
        cos_sum += angle.cos() * weight;
        weight_sum += weight;
    }

    if weight_sum < 1e-10 {
        return (0.0, 0.0);
    }

    let mean_sin = sin_sum / weight_sum;
    let mean_cos = cos_sum / weight_sum;

    (
        mean_sin.atan2(mean_cos),
        (mean_sin * mean_sin + mean_cos * mean_cos).sqrt(),
    )
}

/// Shortest angular separation between two directions, within `[0, PI]`.
fn angular_distance(a: f64, b: f64) -> f64 {
    let mut diff = (a - b).abs() % (2.0 * PI);
    if diff > PI {
        diff = 2.0 * PI - diff;
    }
    diff
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    use super::*;

    /// Horizontal ramp: brightness increasing to the right.
    fn horizontal_ramp(width: u32, height: u32) -> DynamicImage {
        let mut image = RgbImage::new(width, height);
        for (x, _, pixel) in image.enumerate_pixels_mut() {
            let v = ((x * 255) / width.max(1)) as u8;
            *pixel = Rgb([v, v, v]);
        }
        DynamicImage::ImageRgb8(image)
    }

    /// A gentle ramp spans only ~2 levels per pixel, so the default cutoff of
    /// 30 would discard every sample.
    fn ramp_analyzer() -> LuminanceGradientAnalyzer {
        LuminanceGradientAnalyzer::new(16).with_magnitude_threshold(5.0)
    }

    #[test]
    fn dominant_direction_stays_within_pi() {
        let result = ramp_analyzer().analyze(&horizontal_ramp(128, 128)).unwrap();

        // The old expression was `(bin / bins) + 2*PI - PI`, missing a
        // multiplication, so it always landed in [PI, PI + 1).
        assert!(
            (-PI..=PI).contains(&result.dominant_direction),
            "direction {} out of range",
            result.dominant_direction
        );
    }

    #[test]
    fn horizontal_ramp_points_right() {
        let result = ramp_analyzer().analyze(&horizontal_ramp(128, 128)).unwrap();

        // Brightness rises with x, so the gradient points along +x: angle ~0.
        assert!(
            angular_distance(result.dominant_direction, 0.0) < 0.2,
            "direction was {}",
            result.dominant_direction
        );
        assert!(result.direction_confidence > 0.8);
    }

    #[test]
    fn uniform_lighting_has_no_inconsistent_regions() {
        let result = ramp_analyzer().analyze(&horizontal_ramp(128, 128)).unwrap();

        assert!(result.inconsistent_regions.is_empty());
    }

    #[test]
    fn angular_distance_wraps_across_the_seam() {
        assert!(angular_distance(PI - 0.1, -PI + 0.1) < 0.25);
    }
}
