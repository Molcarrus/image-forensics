use image::{DynamicImage, GrayImage, Luma};

use crate::{SRegion, error::Result, image_utils::rgb_to_gray};

pub struct LuminanceGradientAnalyzer {
    block_size: u32 
}

pub struct LuminanceGradientResult {
    pub gradient_map: GrayImage,
    pub direction_map: GrayImage,
    pub inconsistent_regions: Vec<SRegion>,
    pub dominant_direction: f64,
}

impl LuminanceGradientAnalyzer {
    pub fn new(block_size: u32) -> Self {
        Self { block_size }
    }
    
    pub fn analyze(&self, image: &DynamicImage) -> Result<LuminanceGradientResult> {
        let gray = rgb_to_gray(&image.to_rgb8());
        let (width, height) = gray.dimensions();
        
        let mut gradient_map = GrayImage::new(width, height);
        let mut direction_map = GrayImage::new(width, height);
        let mut directions = Vec::new();
        
        for y in 1..height-1 {
            for x in 1..width-1 {
                let gx = self.sobel_x(&gray, x, y);
                let gy = self.sobel_y(&gray, x, y);
                
                let magnitude = (gx * gx + gy * gy).sqrt();
                let direction = gy.atan2(gx);
                
                gradient_map.put_pixel(x, y, Luma([(magnitude.min(255.0)) as u8]));
                
                let dir_normalized = ((direction + std::f64::consts::PI) / (2.0 * std::f64::consts::PI) * 255.0) as u8;
                direction_map.put_pixel(x, y, Luma([dir_normalized]));
                
                if magnitude > 30.0 {
                    directions.push(direction);
                }
            }
        }
        
        let dominant_direction = self.find_dominant_direction(&directions);
        
        let inconsistent_regions = self.find_incosistent_regions(&direction_map, &gradient_map, dominant_direction);
        
        Ok(LuminanceGradientResult { 
            gradient_map, 
            direction_map, 
            inconsistent_regions, 
            dominant_direction 
        })
    }
    
    fn sobel_x(&self, gray: &GrayImage, x: u32, y: u32) -> f64 {
        let p = |dx: i32, dy: i32| -> f64 {
            gray.get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)[0] as f64
        };
        
        -p(-1, -1) - 2.0 * p(-1, 0) - p(-1, 1) + p(1, -1) - 2.0 * p(1, 0) + p(1, 1)
    }
    
    fn sobel_y(&self, gray: &GrayImage, x: u32, y: u32) -> f64 {
        let p = |dx: i32, dy: i32| -> f64 {
            gray.get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)[0] as f64
        };
        
        -p(-1, -1) - 2.0 * p(0, -1) - p(1, -1) + p(-1, 1) + 2.0 * p(0, 1) + p(1, 1)
    }
    
    fn find_dominant_direction(&self, directions: &[f64]) -> f64 {
        if directions.is_empty() {
            return 0.0;
        }
        
        let bins = 36;
        let mut histogram = vec![0u32; bins];
        
        for &dir in directions {
            let bin = ((dir + std::f64::consts::PI) / (2.0 * std::f64::consts::PI) * bins as f64) as usize;
            histogram[bin.min(bins - 1)] += 1;
        }
        
        let max_bin = histogram
            .iter()
            .enumerate()
            .max_by_key(|&(_, count)| count)
            .map(|(i, _)| i)
            .unwrap_or(0);
        
        (max_bin as f64 / bins as f64) + 2.0 * std::f64::consts::PI - std::f64::consts::PI
    }
    
    fn find_incosistent_regions(
        &self,
        direction_map: &GrayImage,
        gradient_map: &GrayImage,
        dominant: f64 
    ) -> Vec<SRegion> {
        let (width, height) = direction_map.dimensions();
        let mut regions = Vec::new();
        
        let dominant_normalized = ((dominant + std::f64::consts::PI) / (2.0 * std::f64::consts::PI) * 255.0) as i32;
        
        for by in (0..height).step_by(self.block_size as usize) {
            for bx in (0..width).step_by(self.block_size as usize) {
                let mut dir_sum = 0i32;
                let mut grad_sum = 0.0;
                let mut count = 0;
                
                for y in by..(by + self.block_size).min(height) {
                    for x in bx..(bx + self.block_size).min(width) {
                        let grad = gradient_map.get_pixel(x, y)[0] as f64;
                        if grad > 20.0 {
                            dir_sum += direction_map.get_pixel(x, y)[0] as i32;
                            grad_sum += grad;
                            count += 1;
                        }
                    }
                }
                
                if count > (self.block_size * self.block_size / 4) as i32 {
                    let avg_dir = dir_sum / count;
                    let dir_diff = (avg_dir - dominant_normalized).abs();
                    
                    if dir_diff > 32 && dir_diff < 224 {
                        regions.push(SRegion { 
                            x: bx, 
                            y: by, 
                            width: self.block_size.min(width - bx), 
                            height: self.block_size.min(height - by), 
                        });
                    }
                }
            }
        }
        
        regions
    }
}