use image::{GrayImage, Luma, RgbImage};
use ndarray::Array2;

use crate::{
    error::{ForensicsError, Result},
    region::SRegion,
};

/// ITU-R BT.601 luma weights, matching the JPEG colour transform.
const LUMA_R: f64 = 0.299;
const LUMA_G: f64 = 0.587;
const LUMA_B: f64 = 0.114;

/// Convert an RGB image to 8-bit luminance.
pub fn rgb_to_gray(image: &RgbImage) -> GrayImage {
    let (width, height) = image.dimensions();
    let mut gray = GrayImage::new(width, height);

    for (x, y, pixel) in image.enumerate_pixels() {
        gray.put_pixel(x, y, Luma([luma(pixel[0], pixel[1], pixel[2])]));
    }

    gray
}

/// Luminance of a single RGB triple, rounded rather than truncated.
pub fn luma(r: u8, g: u8, b: u8) -> u8 {
    (LUMA_R * r as f64 + LUMA_G * g as f64 + LUMA_B * b as f64).round() as u8
}

/// Return an error when either dimension is below `min`.
///
/// Every analyzer calls this before touching block arithmetic, so a small image
/// produces a typed error instead of an underflowing subtraction.
pub fn ensure_min_dimensions(width: u32, height: u32, min: u32) -> Result<()> {
    if width < min || height < min {
        Err(ForensicsError::ImageTooSmall(min))
    } else {
        Ok(())
    }
}

/// Iterate complete `size` x `size` blocks at the given stride.
///
/// Yields nothing when the image is smaller than one block, which is what makes
/// this safe to call without a prior size check. The final aligned block is
/// included (the hand-rolled `0..height - size` loops this replaces dropped it).
pub fn full_blocks(
    width: u32,
    height: u32,
    size: u32,
    stride: u32,
) -> impl Iterator<Item = SRegion> {
    let stride = stride.max(1) as usize;
    let last_x = width.checked_sub(size);
    let last_y = height.checked_sub(size);

    last_y
        .into_iter()
        .flat_map(move |max_y| (0..=max_y).step_by(stride))
        .flat_map(move |y| {
            last_x
                .into_iter()
                .flat_map(move |max_x| (0..=max_x).step_by(stride))
                .map(move |x| SRegion::new(x, y, size, size))
        })
}

/// Iterate blocks tiling the whole image, clipping partial blocks at the edges.
pub fn clipped_blocks(
    width: u32,
    height: u32,
    size: u32,
    stride: u32,
) -> impl Iterator<Item = SRegion> {
    let stride = stride.max(1) as usize;

    (0..height)
        .step_by(stride)
        .flat_map(move |y| {
            (0..width)
                .step_by(stride)
                .map(move |x| SRegion::clipped(x, y, size, width, height))
        })
        .filter(|region| !region.is_empty())
}

/// Sample a pixel with edge replication, so kernels need no border special case.
pub fn sample_clamped(image: &GrayImage, x: i32, y: i32) -> f64 {
    let (width, height) = image.dimensions();
    let px = x.clamp(0, width as i32 - 1) as u32;
    let py = y.clamp(0, height as i32 - 1) as u32;

    image.get_pixel(px, py)[0] as f64
}

/// 3x3 Sobel gradient at (`x`, `y`), returning `(gx, gy)`.
///
/// Borders are handled by edge replication. This is the single definition used
/// by every module; four separate copies previously existed and one of them had
/// an inverted sign in the X kernel.
pub fn sobel(image: &GrayImage, x: u32, y: u32) -> (f64, f64) {
    let (x, y) = (x as i32, y as i32);
    let p = |dx: i32, dy: i32| sample_clamped(image, x + dx, y + dy);

    let gx = -p(-1, -1) - 2.0 * p(-1, 0) - p(-1, 1) + p(1, -1) + 2.0 * p(1, 0) + p(1, 1);
    let gy = -p(-1, -1) - 2.0 * p(0, -1) - p(1, -1) + p(-1, 1) + 2.0 * p(0, 1) + p(1, 1);

    (gx, gy)
}

/// Sobel gradient magnitude and orientation in radians, `(magnitude, angle)`.
pub fn sobel_polar(image: &GrayImage, x: u32, y: u32) -> (f64, f64) {
    let (gx, gy) = sobel(image, x, y);
    ((gx * gx + gy * gy).sqrt(), gy.atan2(gx))
}

/// Map an angle in `[-PI, PI]` onto the full `u8` range for storage in a map.
pub fn angle_to_u8(angle: f64) -> u8 {
    use std::f64::consts::PI;
    (((angle + PI) / (2.0 * PI)) * 255.0).clamp(0.0, 255.0) as u8
}

/// Inverse of [`angle_to_u8`].
pub fn u8_to_angle(value: u8) -> f64 {
    use std::f64::consts::PI;
    (value as f64 / 255.0) * 2.0 * PI - PI
}

pub fn gray_to_array(image: &GrayImage) -> Array2<f64> {
    let (width, height) = image.dimensions();
    let mut arr = Array2::zeros((height as usize, width as usize));

    for (x, y, pixel) in image.enumerate_pixels() {
        arr[[y as usize, x as usize]] = pixel[0] as f64;
    }

    arr
}

pub fn array_to_gray(arr: &Array2<f64>) -> GrayImage {
    let (height, width) = arr.dim();
    let mut image = GrayImage::new(width as u32, height as u32);

    for y in 0..height {
        for x in 0..width {
            let value = arr[[y, x]].clamp(0.0, 255.0) as u8;
            image.put_pixel(x as u32, y as u32, Luma([value]));
        }
    }

    image
}

pub fn gaussian_blur_3x3(image: &GrayImage) -> GrayImage {
    let kernel = [
        [1.0 / 16.0, 2.0 / 16.0, 1.0 / 16.0],
        [2.0 / 16.0, 4.0 / 16.0, 2.0 / 16.0],
        [1.0 / 16.0, 2.0 / 16.0, 1.0 / 16.0],
    ];

    convolve_gray(image, &kernel)
}

/// Convolve with a 3x3 kernel using edge replication.
///
/// The border is filled from clamped samples rather than left black, and the
/// loop no longer underflows on images narrower or shorter than two pixels.
pub fn convolve_gray(image: &GrayImage, kernel: &[[f64; 3]; 3]) -> GrayImage {
    let (width, height) = image.dimensions();
    let mut result = GrayImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;

            for (ky, row) in kernel.iter().enumerate() {
                for (kx, weight) in row.iter().enumerate() {
                    let px = x as i32 + kx as i32 - 1;
                    let py = y as i32 + ky as i32 - 1;
                    sum += sample_clamped(image, px, py) * weight;
                }
            }

            result.put_pixel(x, y, Luma([sum.clamp(0.0, 255.0) as u8]));
        }
    }

    result
}

pub fn calculate_histogram(image: &GrayImage) -> [u32; 256] {
    let mut histogram = [0u32; 256];

    for pixel in image.pixels() {
        histogram[pixel[0] as usize] += 1;
    }

    histogram
}

pub fn normalize_to_u8(arr: &Array2<f64>) -> Array2<f64> {
    let min = arr.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = arr.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;

    if range < 1e-10 {
        Array2::zeros(arr.dim())
    } else {
        arr.mapv(|v| ((v - min) / range) * 255.0)
    }
}

/// Extract a `size` x `size` block, replicating edge pixels when the block
/// overhangs the image.
///
/// The returned vector is always `size * size` long. It previously came back
/// short at the borders, which left callers silently zero-padding their
/// feature vectors.
pub fn extract_block(image: &GrayImage, x: u32, y: u32, size: u32) -> Vec<u8> {
    let (width, height) = image.dimensions();
    let mut block = Vec::with_capacity((size * size) as usize);

    if width == 0 || height == 0 {
        return vec![0; (size * size) as usize];
    }

    for dy in 0..size {
        let py = (y + dy).min(height - 1);
        for dx in 0..size {
            let px = (x + dx).min(width - 1);
            block.push(image.get_pixel(px, py)[0]);
        }
    }

    block
}

pub fn block_mean(block: &[u8]) -> f64 {
    if block.is_empty() {
        return 0.0;
    }
    block.iter().map(|&v| v as f64).sum::<f64>() / block.len() as f64
}

pub fn block_variance(block: &[u8]) -> f64 {
    if block.is_empty() {
        return 0.0;
    }

    let mean = block_mean(block);

    block
        .iter()
        .map(|&v| {
            let diff = v as f64 - mean;
            diff * diff
        })
        .sum::<f64>()
        / block.len() as f64
}

/// Mean and population variance of a slice, computed in one pass each.
pub fn mean_and_variance(values: &[f64]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0);
    }

    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;

    (mean, variance.max(0.0))
}

/// Median of a slice, without mutating the caller's data.
pub fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_blocks_yields_nothing_for_undersized_images() {
        // The `0..height - size` loops this replaces panicked here.
        assert_eq!(full_blocks(32, 32, 64, 32).count(), 0);
        assert_eq!(full_blocks(0, 0, 8, 4).count(), 0);
    }

    #[test]
    fn full_blocks_includes_the_last_aligned_block() {
        let blocks: Vec<_> = full_blocks(128, 64, 64, 64).collect();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1], SRegion::new(64, 0, 64, 64));
    }

    #[test]
    fn clipped_blocks_cover_the_whole_image() {
        let covered: u64 = clipped_blocks(100, 70, 32, 32).map(|r| r.area()).sum();
        assert_eq!(covered, 100 * 70);
    }

    #[test]
    fn sobel_x_responds_to_a_vertical_edge() {
        let mut image = GrayImage::new(5, 5);
        for y in 0..5 {
            for x in 0..5 {
                image.put_pixel(x, y, Luma([if x < 2 { 0 } else { 255 }]));
            }
        }

        let (gx, gy) = sobel(&image, 2, 2);
        // A dark-to-light edge going right must give a positive gx. The old
        // luminance_gradient copy had `-2.0 * p(1, 0)` and got this backwards.
        assert!(gx > 0.0, "gx = {gx}");
        assert!(gy.abs() < 1e-9, "gy = {gy}");
    }

    #[test]
    fn sobel_y_responds_to_a_horizontal_edge() {
        let mut image = GrayImage::new(5, 5);
        for y in 0..5 {
            for x in 0..5 {
                image.put_pixel(x, y, Luma([if y < 2 { 0 } else { 255 }]));
            }
        }

        let (gx, gy) = sobel(&image, 2, 2);
        assert!(gy > 0.0, "gy = {gy}");
        assert!(gx.abs() < 1e-9, "gx = {gx}");
    }

    #[test]
    fn angle_round_trips_through_u8() {
        for angle in [-3.0, -1.0, 0.0, 1.0, 3.0] {
            let recovered = u8_to_angle(angle_to_u8(angle));
            assert!((recovered - angle).abs() < 0.03, "{angle} -> {recovered}");
        }
    }

    #[test]
    fn extract_block_is_always_full_size() {
        let image = GrayImage::new(10, 10);
        assert_eq!(extract_block(&image, 8, 8, 16).len(), 256);
    }

    #[test]
    fn convolve_handles_tiny_images() {
        let image = GrayImage::new(1, 1);
        assert_eq!(gaussian_blur_3x3(&image).dimensions(), (1, 1));
    }
}
