use std::io::Cursor;

use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};

use crate::{
    ElaResult, SRegion,
    error::Result,
    image_utils::{clipped_blocks, mean_and_variance},
    region::merge_regions,
};

/// Error Level Analysis.
///
/// Recompresses the image at a chosen quality and measures where the result
/// moves. A region pasted in from a source saved at a different quality has not
/// settled onto the same quantisation grid, so it moves more than its
/// surroundings.
///
/// # Reading the output
///
/// Look at [`ElaResult::image`], not the scalars. In a healthy
/// single-compression JPEG the ELA image is close to uniform with brightness
/// concentrated on edges — edges always carry the most quantisation error.
///
/// # Limitations
///
/// ELA shows *compression history*, not editing. Lossless input has no history
/// to show; texture always reads brighter than smooth areas; and one resave of
/// the whole composite at uniform quality erases the difference entirely, so a
/// negative result says very little.
pub struct ElaAnalyzer {
    quality: u8,
    amplification: f64,
    block_size: u32,
    merge_gap: u32,
}

impl ElaAnalyzer {
    /// Analyzer recompressing at `quality`. 90-98 is the useful band: the
    /// point is to recompress gently so lower-quality regions stand out.
    pub fn new(quality: u8) -> Self {
        Self {
            quality,
            amplification: 10.0,
            block_size: 16,
            merge_gap: 8,
        }
    }

    /// Display gain applied to the difference maps. Visual only; the reported
    /// statistics stay in raw difference units.
    pub fn with_amplification(mut self, amp: f64) -> Self {
        self.amplification = amp;
        self
    }

    /// Tile size for suspicious-region detection.
    pub fn with_block_size(mut self, block_size: u32) -> Self {
        self.block_size = block_size.max(1);
        self
    }

    /// Run the analysis. Accepts any image size.
    pub fn analyze(&self, image: &DynamicImage) -> Result<ElaResult> {
        let rgb_image = image.to_rgb8();
        let (width, height) = rgb_image.dimensions();

        let recompressed = self.recompress_jpeg(image)?;
        let recompressed_rgb = recompressed.to_rgb8();

        let mut ela_image = RgbImage::new(width, height);
        let mut difference_map = GrayImage::new(width, height);
        let mut differences = Vec::with_capacity((width as usize) * (height as usize));

        for y in 0..height {
            for x in 0..width {
                let orig = rgb_image.get_pixel(x, y);
                let recomp = recompressed_rgb.get_pixel(x, y);

                let diff_r = (orig[0] as i32 - recomp[0] as i32).unsigned_abs() as f64;
                let diff_g = (orig[1] as i32 - recomp[1] as i32).unsigned_abs() as f64;
                let diff_b = (orig[2] as i32 - recomp[2] as i32).unsigned_abs() as f64;

                ela_image.put_pixel(
                    x,
                    y,
                    Rgb([
                        self.amplify(diff_r),
                        self.amplify(diff_g),
                        self.amplify(diff_b),
                    ]),
                );

                let gray_diff = (diff_r + diff_g + diff_b) / 3.0;
                differences.push(gray_diff);
                difference_map.put_pixel(x, y, Luma([self.amplify(gray_diff)]));
            }
        }

        let max_difference = differences.iter().copied().fold(0.0f64, f64::max);

        // Statistics are reported in raw difference units, matching
        // `max_difference`. They were previously mixed: the mean came from the
        // amplified, u8-saturated map while the variance came from the raw
        // values, so `std_deviation` and the region threshold were computed
        // across two different scales.
        let (mean_difference, variance) = mean_and_variance(&differences);
        let std_deviation = variance.sqrt();

        let threshold = mean_difference + 2.0 * std_deviation;
        let suspicious_regions =
            self.find_suspicious_regions(&differences, width, height, threshold);

        Ok(ElaResult {
            image: ela_image,
            difference_map,
            max_difference,
            mean_difference,
            std_deviation,
            suspicious_regions,
        })
    }

    fn amplify(&self, difference: f64) -> u8 {
        (difference * self.amplification).clamp(0.0, 255.0) as u8
    }

    fn recompress_jpeg(&self, image: &DynamicImage) -> Result<DynamicImage> {
        let mut buffer = Cursor::new(Vec::new());

        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, self.quality);
        image.write_with_encoder(encoder)?;

        Ok(image::load_from_memory(&buffer.into_inner())?)
    }

    /// Flag blocks whose mean raw difference exceeds `threshold`.
    fn find_suspicious_regions(
        &self,
        differences: &[f64],
        width: u32,
        height: u32,
        threshold: f64,
    ) -> Vec<SRegion> {
        let regions = clipped_blocks(width, height, self.block_size, self.block_size)
            .filter(|block| {
                let sum: f64 = block
                    .pixels()
                    .map(|(x, y)| differences[(y as usize) * (width as usize) + x as usize])
                    .sum();

                sum / block.area() as f64 > threshold
            })
            .collect();

        merge_regions(regions, self.merge_gap)
    }
}

#[cfg(test)]
mod tests {
    use image::RgbImage;

    use super::*;

    fn flat_image(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb([120, 120, 120])))
    }

    #[test]
    fn statistics_share_one_scale() {
        let result = ElaAnalyzer::new(95).analyze(&flat_image(64, 64)).unwrap();

        // A uniform image recompresses almost exactly, so every raw statistic
        // must stay small. Reading the mean off the 10x-amplified map used to
        // inflate it well past `max_difference`.
        assert!(
            result.mean_difference <= result.max_difference + 1e-9,
            "mean {} exceeded max {}",
            result.mean_difference,
            result.max_difference
        );
        assert!(result.std_deviation.is_finite());
    }

    #[test]
    fn flat_image_has_no_suspicious_regions() {
        let result = ElaAnalyzer::new(95).analyze(&flat_image(64, 64)).unwrap();
        assert!(result.suspicious_regions.is_empty());
    }

    #[test]
    fn output_matches_input_dimensions() {
        let result = ElaAnalyzer::new(90).analyze(&flat_image(48, 32)).unwrap();
        assert_eq!(result.image.dimensions(), (48, 32));
        assert_eq!(result.difference_map.dimensions(), (48, 32));
    }
}
