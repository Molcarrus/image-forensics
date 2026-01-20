use std::f64;

use image::{GrayImage, Luma, RgbImage};
use ndarray::Array2;

pub fn rgb_to_gray(image: &RgbImage) -> GrayImage {
    let (width, height) = image.dimensions();
    let mut gray = GrayImage::new(width, height);

    for (x, y, pixel) in image.enumerate_pixels() {
        let lum =
            (0.299 * pixel[0] as f64 + 0.587 * pixel[1] as f64 + 0.114 * pixel[2] as f64) as u8;
        gray.put_pixel(x, y, Luma([lum]));
    }

    gray
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

pub fn convolve_gray(image: &GrayImage, kernel: &[[f64; 3]; 3]) -> GrayImage {
    let (width, height) = image.dimensions();
    let mut result = GrayImage::new(width, height);

    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let mut sum = 0.0;

            for ky in 0..3 {
                for kx in 0..3 {
                    let px = image.get_pixel(x + kx - 1, y + ky - 1)[0] as f64;
                    sum += px * kernel[ky as usize][kx as usize];
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

pub fn extract_block(image: &GrayImage, x: u32, y: u32, size: u32) -> Vec<u8> {
    let mut block = Vec::with_capacity((size * size) as usize);

    for dy in 0..size {
        for dx in 0..size {
            if x + dx < image.width() && y + dy < image.height() {
                block.push(image.get_pixel(x + dx, y + dy)[0]);
            }
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
    let variance = block
        .iter()
        .map(|&v| {
            let diff = v as f64 - mean;
            diff * diff
        })
        .sum::<f64>()
        / block.len() as f64;

    variance
}
