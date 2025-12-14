use std::{f64, io::Cursor};

use image::{DynamicImage, GrayImage, Luma, RgbImage};

use crate::{JpegAnalysisResult, error::Result, image_utils::rgb_to_gray};

pub struct JpegAnalyzer {
    ghost_quality_range: (u8, u8),
    ghost_quality_step: u8, 
}

impl JpegAnalyzer {
    pub fn new() -> Self {
        Self {
            ghost_quality_range: (60, 100),
            ghost_quality_step: 5,
        }
    }
    
    pub fn analyze(&self, image: &DynamicImage) -> Result<JpegAnalysisResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);
        
        let quality_estimate = self.estimate_quality(image)?;
        
        let (ghost_detected, ghost_map) = self.detect_ghost(image)?;
        
        let blocking_artifact_map = self.analyze_blocking_artifacts(&gray);
        
        let double_compression_likelihood = self.detect_double_compression(image)?;
        
        Ok(JpegAnalysisResult { 
            quality_estimate, 
            ghost_detected, 
            ghost_map: if ghost_detected { Some(ghost_map) } else { None }, 
            blocking_artifact_map, 
            double_compression_likelihood 
        })
    }
    
    fn estimate_quality(&self, image: &DynamicImage) -> Result<u8> {
        let mut min_diff = f64::MAX;
        let mut best_quality = 75u8;
        
        let original_rgb = image.to_rgb8();
        
        for quality in (50..100).step_by(5) {
            let recompressed = self.recompress(image, quality)?;
            let recompressed_rgb = recompressed.to_rgb8();
            
            let diff = self.calculate_image_difference(&original_rgb, &recompressed_rgb);
            
            if diff < min_diff {
                min_diff = diff;
                best_quality = quality;
            }
        }
        
        Ok(best_quality)
    }
    
    fn detect_ghost(&self, image: &DynamicImage) -> Result<(bool, GrayImage)> {
        let original_rgb = image.to_rgb8();
        let (width, height) = original_rgb.dimensions();
        let mut min_ghost_map = GrayImage::new(width, height);
        let mut min_diff = f64::MAX;
        let mut ghost_quality = 0u8;
        
        for quality in (self.ghost_quality_range.0..self.ghost_quality_range.1).step_by(self.ghost_quality_step as usize) {
            let recompressed = self.recompress(image, quality)?;
            let recompressed_rgb = recompressed.to_rgb8();
            
            let ghost_map = self.create_difference_map(&original_rgb, &recompressed_rgb);
            let avg_diff = self.average_difference(&ghost_map);
            
            if avg_diff < min_diff && quality < 95 {
                min_diff = avg_diff;
                min_ghost_map = ghost_map;
                ghost_quality = quality;
            }
        }
        
        let ghost_detected = ghost_quality > 0 && ghost_quality < 90 && min_diff < 5.0;
        
        Ok((ghost_detected, min_ghost_map))
    }
    
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
                
                let artifact_value = if count > 0 {
                    (boundary_diff / count as f64).min(255.0) as u8 
                } else {
                    0
                };
                
                artifact_map.put_pixel(x, y, Luma([artifact_value]));
            }
        }
        
        artifact_map
    }
    
    fn detect_double_compression(&self, image: &DynamicImage) -> Result<f64> {
        let gray = rgb_to_gray(&image.to_rgb8());
        let (width, height) = gray.dimensions();
        
        let mut dct_histogram = [0u32; 256];
        
        for y in (0..height - 8).step_by(8) {
            for x in (0..width - 8).step_by(8) {
                let mut block_energy = 0.0;
                for dy in 0..8 {
                    for dx in 0..8 {
                        if x + dx + 1 < width && y + dy + 1 < height {
                            let p1 = gray.get_pixel(x + dx, y + dy)[0] as f64;
                            let p2 = gray.get_pixel(x+ dx + 1, y + dy + 1)[0] as f64;
                            block_energy += (p1 - p2).abs();
                        }
                    }
                }
                
                let energy_idx = (block_energy / 64.0).min(255.0) as usize;
                dct_histogram[energy_idx] += 1;
            }
        }
        
        let likelihood = self.detect_histogram_periodicity(&dct_histogram);
        
        Ok(likelihood)
    }
    
    fn detect_histogram_periodicity(&self, histogram: &[u32; 256]) -> f64 {
        let mut max_periodicity = 0.0;
        
        for period in 2..20 {
            let mut periodicity_score = 0.0;
            let mut count = 0;
            
            for i in period..256 {
                let h1 = histogram[i] as f64;
                let h2 = histogram[i - period] as f64;
                
                if h1 > 0.0 && h2 > 0.0 {
                    periodicity_score += (h1 - h2).abs() / (h1 + h2);
                    count += 1;
                }
            }
            
            if count > 0 {
                periodicity_score = 1.0 - (periodicity_score / count as f64);
                if periodicity_score > max_periodicity {
                    max_periodicity = periodicity_score;
                }
            }
        }
        
        max_periodicity
    }
    
    fn recompress(&self, image: &DynamicImage, quality: u8) -> Result<DynamicImage> {
        let mut buffer = Cursor::new(Vec::new());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, quality);
        image.write_with_encoder(encoder)?;
        
        buffer.set_position(0);
        let recompressed = image::load_from_memory(&buffer.into_inner())?;
        
        Ok(recompressed)
    }
    
    fn calculate_image_difference(&self, img1: &RgbImage, img2: &RgbImage) -> f64 {
        let mut total_diff = 0.0;
        let mut count = 0;
        
        for (p1, p2) in img1.pixels().zip(img2.pixels()) {
            for c in 0..3 {
                total_diff += (p1[c] as f64 - p2[c] as f64).abs();
                count += 1;
            }
        }
        
        total_diff / count as f64 
    }
    
    fn create_difference_map(&self, img1: &RgbImage, img2: &RgbImage) -> GrayImage {
        let (width, height) = img1.dimensions();
        let mut diff_map = GrayImage::new(width, height);
        
        for y in 0..height {
            for x in 0..width {
                let p1 = img1.get_pixel(x, y);
                let p2 = img2.get_pixel(x, y);
                
                let diff = (
                    (p1[0] as i32 - p2[0] as i32).abs() +
                    (p1[1] as i32 - p2[1] as i32).abs() + 
                    (p1[2] as i32 - p2[2] as i32).abs()
                ) / 3;
                
                diff_map.put_pixel(x, y, Luma([diff.min(255) as u8]));
            }
        }
        
        diff_map
    }
    
    fn average_difference(&self, map: &GrayImage) -> f64 {
        let sum = map.pixels().map(|p| p[0] as f64).sum::<f64>();
        sum / (map.width() * map.height()) as f64 
    }
}

impl Default for JpegAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}