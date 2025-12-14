use std::io::Cursor;

use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};

use crate::{ElaResult, SRegion, error::Result};

pub struct ElaAnalyzer {
    quality: u8,
    amplification: f64,
    threshold: f64,
}

impl ElaAnalyzer {
    pub fn new(quality: u8) -> Self {
        Self { 
            quality, 
            amplification: 10.0, 
            threshold: 30.0, 
        }
    }
    
    pub fn with_amplification(mut self, amp: f64) -> Self {
        self.amplification = amp;
        self 
    }
    
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold;
        self 
    }
    
    pub fn analyze(&self, image: &DynamicImage) -> Result<ElaResult> {
        let rgb_image = image.to_rgb8();
        let (width, height) = rgb_image.dimensions();
        
        let recompressed = self.recompress_jpeg(image)?;
        let recompressed_rgb = recompressed.to_rgb8();
        
        let mut ela_image = RgbImage::new(width, height);
        let mut difference_map = GrayImage::new(width, height);
        let mut differences = Vec::new();
        
        for y in 0..height {
            for x in 0..width {
                let orig = rgb_image.get_pixel(x, y);
                let recomp = recompressed_rgb.get_pixel(x, y);
                
                let diff_r = (orig[0] as i32 - recomp[0] as i32).abs() as f64;
                let diff_g = (orig[1] as i32 - recomp[1] as i32).abs() as f64;
                let diff_b = (orig[2] as i32 - recomp[2] as i32).abs() as f64;
                
                let ela_r = (diff_r * self.amplification).min(255.0) as u8;
                let ela_g = (diff_g * self.amplification).min(255.0) as u8;
                let ela_b = (diff_b * self.amplification).min(255.0) as u8;
                
                ela_image.put_pixel(x, y, Rgb([ela_r, ela_g, ela_b]));
                
                let gray_diff = (diff_r + diff_g + diff_b) / 3.0;
                differences.push(gray_diff);
                difference_map.put_pixel(x, y, Luma([(gray_diff * self.amplification).min(255.0) as u8]));
            }
        }
        
        let max_difference = differences.iter().cloned().fold(0.0f64, f64::max);
        let mean_difference = difference_map
            .iter()
            .map(|&x| x as f64)
            .sum::<f64>() / differences.len() as f64;
        let variance = differences
            .iter()
            .map(|d| (d - mean_difference).powi(2))
            .sum::<f64>() / differences.len() as f64;
        let std_deviation = variance.sqrt();
        
        let suspicious_regions = self.find_suspicious_regions(&difference_map, mean_difference + 2.0 * std_deviation);
        
        Ok(ElaResult { 
            image: ela_image, 
            difference_map, 
            max_difference, 
            mean_difference, 
            std_deviation, 
            suspicious_regions,
        })
    }
    
    fn recompress_jpeg(&self, image: &DynamicImage) -> Result<DynamicImage> {
        let mut buffer = Cursor::new(Vec::new());
        
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, self.quality);
        image.write_with_encoder(encoder)?;
        
        buffer.set_position(0);
        let recompressed = image::load_from_memory(&buffer.into_inner())?;
        
        Ok(recompressed)
    }
    
    fn find_suspicious_regions(&self, diff_map: &GrayImage, threshold: f64) -> Vec<SRegion> {
        let (width, height) = diff_map.dimensions();
        let block_size = 16u32;
        let mut regions = Vec::new();
        
        for by in (0..height).step_by(block_size as usize) {
            for bx in (0..width).step_by(block_size as usize) {
                let mut block_sum = 0.0;
                let mut count = 0;
                
                for y in by..((by + block_size).min(height)) {
                    for x in bx..((bx + block_size).min(width)) {
                        block_sum += diff_map.get_pixel(x, y)[0] as f64;
                        count += 1;
                    }
                }
                
                let block_mean = block_sum / count as f64;
                
                if block_mean > threshold {
                    regions.push(SRegion {
                        x: bx,
                        y: by,
                        width: block_size.min(width - bx),
                        height: block_size.min(height - by)
                    });
                }
            }
        }
        
        self.merge_adjacent_regions(regions)
    }
    
    fn merge_adjacent_regions(&self, regions: Vec<SRegion>) -> Vec<SRegion> {
        if regions.is_empty() {
            return regions;
        }
        
        let mut merged = Vec::new();
        let mut used = vec![false; regions.len()];
        
        for i in 0..regions.len() {
            if used[i] {
                continue;
            }
            
            let mut current = regions[i];
            used[i] = true;
            
            loop {
                let mut found = false;
                for j in 0..regions.len() {
                    if used[j] {
                        continue;
                    }
                    
                    if self.regions_adjacent(&current, &regions[j]) {
                        current = self.merge_two_regions(&current, &regions[j]);
                        used[j] = true;
                        found = true;
                    }
                }
                
                if !found {
                    break;
                }
            }
            
            merged.push(current);
        }
        
        merged
    }
    
    fn regions_adjacent(&self, a: &SRegion, b: &SRegion) -> bool {
        let gap = 8;
        
        !(a.x + a.width + gap < b.x ||
            b.x + b.width + gap < a.x ||
            a.y + a.height + gap < b.y ||
            b.y + b.height + gap < a.y)
    }
    
    fn merge_two_regions(&self, a: &SRegion, b: &SRegion) -> SRegion {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        let x2 = (a.x + a.width).max(b.x + b.width);
        let y2 = (a.y + a.height).max(b.y + b.height);
        
        SRegion {
            x,
            y,
            width: x2 - x,
            height: y2 - y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ela_analyzer_creation() {
        let analyzer = ElaAnalyzer::new(95);
        assert_eq!(analyzer.quality, 95);
    }
}