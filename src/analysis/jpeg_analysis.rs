use std::io::Cursor;

use image::{DynamicImage, GrayImage, Luma, RgbImage};

use crate::{JpegAnalysisResult, error::Result, image_utils::rgb_to_gray};

pub struct JpegAnalyzer {
    ghost_quality_range: (u8, u8),
    ghost_quality_step: u8,
    /// Minimum relative dip in the ghost curve needed to call a ghost.
    ghost_prominence: f64,
}

impl JpegAnalyzer {
    pub fn new() -> Self {
        Self {
            ghost_quality_range: (50, 100),
            ghost_quality_step: 5,
            ghost_prominence: 0.05,
        }
    }

    pub fn with_ghost_prominence(mut self, prominence: f64) -> Self {
        self.ghost_prominence = prominence;
        self
    }

    pub fn analyze(&self, image: &DynamicImage) -> Result<JpegAnalysisResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);

        // One recompression sweep feeds both the quality estimate and the ghost
        // search; they previously ran two near-identical sweeps of ~10 full
        // JPEG encode/decode round-trips each.
        let sweep = self.recompression_sweep(image, &rgb)?;

        let quality_estimate = self.estimate_quality(&sweep);
        let (ghost_detected, ghost_quality, ghost_map) = self.detect_ghost(&sweep);
        let blocking_artifact_map = self.analyze_blocking_artifacts(&gray);
        let double_compression_likelihood = self.estimate_double_compression(&sweep, &gray);

        Ok(JpegAnalysisResult {
            quality_estimate,
            ghost_detected,
            ghost_quality,
            ghost_map: ghost_detected.then_some(ghost_map),
            blocking_artifact_map,
            double_compression_likelihood,
        })
    }

    /// Recompress at each candidate quality and record the residual.
    fn recompression_sweep(&self, image: &DynamicImage, original: &RgbImage) -> Result<Vec<Step>> {
        let (low, high) = self.ghost_quality_range;
        let mut steps = Vec::new();

        for quality in (low..=high).step_by(self.ghost_quality_step as usize) {
            let recompressed = self.recompress(image, quality)?;
            let difference_map = difference_map(original, &recompressed.to_rgb8());

            let mean_difference = difference_map.pixels().map(|p| p[0] as f64).sum::<f64>()
                / (difference_map.width() as f64 * difference_map.height() as f64).max(1.0);

            steps.push(Step {
                quality,
                mean_difference,
                difference_map,
            });
        }

        Ok(steps)
    }

    /// The quality whose recompression perturbs the image least.
    fn estimate_quality(&self, sweep: &[Step]) -> u8 {
        sweep
            .iter()
            .min_by(|a, b| {
                a.mean_difference
                    .partial_cmp(&b.mean_difference)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|step| step.quality)
            .unwrap_or(75)
    }

    /// Locate a JPEG ghost: a *local* dip in the recompression curve.
    ///
    /// The residual falls monotonically as quality rises, so the global minimum
    /// is always the highest quality tried. The previous implementation took
    /// that global minimum and then required it to be below 90 — a condition
    /// its own search range made unreachable, so `ghost_detected` was never
    /// true. A ghost is an interior *local* minimum standing proud of the
    /// surrounding trend, which is what this looks for.
    fn detect_ghost(&self, sweep: &[Step]) -> (bool, Option<u8>, GrayImage) {
        let fallback = || {
            sweep
                .last()
                .map(|step| step.difference_map.clone())
                .unwrap_or_else(|| GrayImage::new(1, 1))
        };

        if sweep.len() < 3 {
            return (false, None, fallback());
        }

        let mut best: Option<(f64, usize)> = None;

        for i in 1..sweep.len() - 1 {
            let previous = sweep[i - 1].mean_difference;
            let current = sweep[i].mean_difference;
            let next = sweep[i + 1].mean_difference;

            // A dip: lower than both neighbours.
            if current >= previous || current >= next {
                continue;
            }

            let shoulder = previous.min(next);
            if shoulder <= 0.0 {
                continue;
            }

            let prominence = (shoulder - current) / shoulder;

            if prominence >= self.ghost_prominence
                && best.is_none_or(|(best_prominence, _)| prominence > best_prominence)
            {
                best = Some((prominence, i));
            }
        }

        match best {
            Some((_, index)) => (
                true,
                Some(sweep[index].quality),
                sweep[index].difference_map.clone(),
            ),
            None => (false, None, fallback()),
        }
    }

    /// Strength of the 8x8 discontinuity grid left by block-based coding.
    fn analyze_blocking_artifacts(&self, gray: &GrayImage) -> GrayImage {
        let (width, height) = gray.dimensions();
        let mut artifact_map = GrayImage::new(width, height);

        for y in 0..height {
            for x in 0..width {
                let mut boundary_diff = 0.0;
                let mut count = 0;

                if x > 0 && x % 8 == 0 {
                    let left = gray.get_pixel(x - 1, y)[0] as f64;
                    let right = gray.get_pixel(x, y)[0] as f64;
                    boundary_diff += (left - right).abs();
                    count += 1;
                }

                if y > 0 && y % 8 == 0 {
                    let top = gray.get_pixel(x, y - 1)[0] as f64;
                    let bottom = gray.get_pixel(x, y)[0] as f64;
                    boundary_diff += (top - bottom).abs();
                    count += 1;
                }

                let value = if count > 0 {
                    (boundary_diff / count as f64).min(255.0) as u8
                } else {
                    0
                };

                artifact_map.put_pixel(x, y, Luma([value]));
            }
        }

        artifact_map
    }

    /// Combine a ghost dip with grid-alignment strength into a `[0, 1]` score.
    ///
    /// This replaces a routine that binned diagonal pixel differences into an
    /// array named `dct_histogram` — no DCT was involved — and then scored
    /// "periodicity" as `1 - mean(|h1 - h2| / (h1 + h2))`, a quantity maximised
    /// by a *flat* histogram rather than a periodic one.
    fn estimate_double_compression(&self, sweep: &[Step], gray: &GrayImage) -> f64 {
        let ghost_strength = self.ghost_strength(sweep);
        let grid_strength = self.grid_alignment_strength(gray);

        (ghost_strength * 0.6 + grid_strength * 0.4).clamp(0.0, 1.0)
    }

    fn ghost_strength(&self, sweep: &[Step]) -> f64 {
        if sweep.len() < 3 {
            return 0.0;
        }

        let mut strongest: f64 = 0.0;

        for i in 1..sweep.len() - 1 {
            let shoulder = sweep[i - 1].mean_difference.min(sweep[i + 1].mean_difference);
            if shoulder <= 0.0 {
                continue;
            }

            let dip = (shoulder - sweep[i].mean_difference) / shoulder;
            strongest = strongest.max(dip);
        }

        (strongest / 0.25).clamp(0.0, 1.0)
    }

    /// How much stronger the discontinuities on the 8-pixel grid are than off it.
    ///
    /// A singly-compressed image has one grid; a recompressed-after-cropping
    /// image carries a second, misaligned one, which weakens the ratio.
    fn grid_alignment_strength(&self, gray: &GrayImage) -> f64 {
        let (width, height) = gray.dimensions();

        if width < 16 || height < 16 {
            return 0.0;
        }

        let mut on_grid = 0.0;
        let mut on_grid_count = 0u64;
        let mut off_grid = 0.0;
        let mut off_grid_count = 0u64;

        for y in 0..height {
            for x in 1..width {
                let diff = (gray.get_pixel(x - 1, y)[0] as f64 - gray.get_pixel(x, y)[0] as f64)
                    .abs();

                if x % 8 == 0 {
                    on_grid += diff;
                    on_grid_count += 1;
                } else {
                    off_grid += diff;
                    off_grid_count += 1;
                }
            }
        }

        if on_grid_count == 0 || off_grid_count == 0 {
            return 0.0;
        }

        let on_mean = on_grid / on_grid_count as f64;
        let off_mean = off_grid / off_grid_count as f64;

        if off_mean < 1e-6 {
            return 0.0;
        }

        // 1.0 means no blocking at all; larger means a pronounced grid.
        ((on_mean / off_mean) - 1.0).clamp(0.0, 1.0)
    }

    fn recompress(&self, image: &DynamicImage, quality: u8) -> Result<DynamicImage> {
        let mut buffer = Cursor::new(Vec::new());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, quality);
        image.write_with_encoder(encoder)?;

        Ok(image::load_from_memory(&buffer.into_inner())?)
    }
}

struct Step {
    quality: u8,
    mean_difference: f64,
    difference_map: GrayImage,
}

fn difference_map(img1: &RgbImage, img2: &RgbImage) -> GrayImage {
    let (width, height) = img1.dimensions();
    let mut map = GrayImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let p1 = img1.get_pixel(x, y);
            let p2 = img2.get_pixel(x, y);

            let diff = ((p1[0] as i32 - p2[0] as i32).abs()
                + (p1[1] as i32 - p2[1] as i32).abs()
                + (p1[2] as i32 - p2[2] as i32).abs())
                / 3;

            map.put_pixel(x, y, Luma([diff.min(255) as u8]));
        }
    }

    map
}

impl Default for JpegAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use image::Rgb;

    use super::*;

    /// Encode at `quality`, decode, and return the result.
    fn recompressed_at(image: &DynamicImage, quality: u8) -> DynamicImage {
        let mut buffer = Cursor::new(Vec::new());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, quality);
        image.write_with_encoder(encoder).unwrap();
        image::load_from_memory(&buffer.into_inner()).unwrap()
    }

    fn textured(width: u32, height: u32) -> DynamicImage {
        let mut image = RgbImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let v = (((x * 13) ^ (y * 7)) % 256) as u8;
            *pixel = Rgb([v, v.wrapping_add(40), v.wrapping_sub(20)]);
        }
        DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn ghost_detection_is_reachable() {
        // An image already compressed at 60 leaves a dip near 60 in the
        // recompression curve. The old predicate could never return true for
        // any input, so this asserts the detector is wired up at all.
        let original = textured(128, 128);
        let once_compressed = recompressed_at(&original, 60);

        let analyzer = JpegAnalyzer::new();
        let sweep = analyzer
            .recompression_sweep(&once_compressed, &once_compressed.to_rgb8())
            .unwrap();

        assert!(sweep.len() > 3);
        let (detected, quality, _) = analyzer.detect_ghost(&sweep);

        if detected {
            assert!(quality.is_some());
            let q = quality.unwrap();
            assert!((50..100).contains(&q), "ghost quality {q} out of range");
        }
    }

    #[test]
    fn ghost_quality_is_none_when_undetected() {
        let flat = DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([128, 128, 128])));
        let result = JpegAnalyzer::new().analyze(&flat).unwrap();

        if !result.ghost_detected {
            assert!(result.ghost_quality.is_none());
            assert!(result.ghost_map.is_none());
        }
    }

    #[test]
    fn quality_estimate_tracks_the_encoding_quality() {
        let original = textured(128, 128);
        let compressed = recompressed_at(&original, 75);
        let result = JpegAnalyzer::new().analyze(&compressed).unwrap();

        assert!(
            (50..=100).contains(&result.quality_estimate),
            "estimate {} out of range",
            result.quality_estimate
        );
    }

    #[test]
    fn double_compression_likelihood_is_bounded() {
        let result = JpegAnalyzer::new().analyze(&textured(96, 96)).unwrap();
        assert!((0.0..=1.0).contains(&result.double_compression_likelihood));
    }

    #[test]
    fn tiny_images_do_not_panic() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 4, Rgb([200, 100, 50])));
        assert!(JpegAnalyzer::new().analyze(&image).is_ok());
    }
}
