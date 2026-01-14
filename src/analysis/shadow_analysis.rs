use std::f64::consts::PI;

use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};

use crate::{SRegion, error::Result, image_utils::rgb_to_gray};

#[derive(Debug, Clone)]
pub struct ShadowConfig {
    pub block_size: u32,
    pub edge_threshold: f64,
    pub shadow_threshold: u8,
    pub min_shadow_size: u32,
    pub angle_tolerance: f64,
    pub gradient_threshold: f64,
}

impl Default for ShadowConfig {
    fn default() -> Self {
        Self { 
            block_size: 32, 
            edge_threshold: 30.0, 
            shadow_threshold: 80, 
            min_shadow_size: 100, 
            angle_tolerance: 20.0, 
            gradient_threshold: 15.0 
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShadowRegion {
    pub region: SRegion,
    pub light_direction: f64,
    pub direction_confidence: f64,
    pub intensity: f64,
    pub edge_sharpness: f64,
}

#[derive(Debug, Clone)]
pub struct ShadowAnalysisResult {
    pub shadow_regions: Vec<ShadowRegion>,
    pub dominant_light_direction: f64,
    pub dominant_direction_confidence: f64,
    pub inconsistent_regions: Vec<SRegion>,
    pub direction_map: RgbImage,
    pub shadow_mask: GrayImage,
    pub consistency_score: f64,
    pub manipulation_probability: f64,
    pub estimated_light_sources: usize,
}

pub struct ShadowAnalyzer {
    config: ShadowConfig,
}

impl ShadowAnalyzer {
    pub fn new() -> Self {
        Self::with_config(ShadowConfig::default())
    }
    
    pub fn with_config(config: ShadowConfig) -> Self {
        Self { config }
    }
    
    pub fn analyze(&self, image: &DynamicImage) -> Result<ShadowAnalysisResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);
        let (width, height) = gray.dimensions();
        
        if width < self.config.block_size * 2 || height < self.config.block_size * 2 {
            return Err(crate::error::ForensicsError::ImageTooSmall(self.config.block_size * 2));
        }
        
        let shadow_mask = self.detect_shadows(&rgb, &gray);
        
        let (gradient_magnitude, gradient_direction) = self.calculate_gradients(&gray);
        
        let shadow_regions = self.analyze_shadow_regions(
            &shadow_mask, 
            &gradient_magnitude, 
            &gradient_direction
        );
        
        let (dominant_light_direction, dominant_direction_confidence) = self.find_dominant_direction(&shadow_regions);
        
        let estimated_light_sources = self.estimate_light_sources(&shadow_regions);
        
        let inconsistent_regions = self.find_incosistent_regions(
            &shadow_regions, dominant_light_direction
        );
        
        let direction_map = self.create_direction_map(
            &rgb, 
            &shadow_regions, 
            dominant_light_direction
        );
        
        let consistency_score = self.calculate_consistency_score(
            &shadow_regions, 
            dominant_light_direction
        );
        
        let manipulation_probability = self.calculate_manipulation_probability(
            &shadow_regions, 
            &inconsistent_regions, 
            consistency_score, 
            estimated_light_sources
        );
        
        Ok(ShadowAnalysisResult { 
            shadow_regions, 
            dominant_light_direction, 
            dominant_direction_confidence, 
            inconsistent_regions, 
            direction_map, 
            shadow_mask, 
            consistency_score, 
            manipulation_probability, 
            estimated_light_sources 
        })
    }
    
    fn detect_shadows(&self, rgb: &RgbImage, gray: &GrayImage) -> GrayImage {
        let (width, height) = gray.dimensions();
        let mut shadow_mask = GrayImage::new(width, height);
        
        let mut intensities = gray
            .pixels()
            .map(|p| p[0])
            .collect::<Vec<_>>();
        
        let low_percentile = intensities[intensities.len() / 10];
        let adaptive_threshold = self.config.shadow_threshold.min(low_percentile + 20);
        
        for y in 0..height {
            for x in 0..width {
                let intensity = gray.get_pixel(x, y)[0];
                let pixel = rgb.get_pixel(x, y);
                
                let is_shadow = self.is_shadow_pixel(intensity, pixel, adaptive_threshold);
                
                shadow_mask.put_pixel(x, y, Luma([if is_shadow { 255 } else { 0 }]));
            }
        }
        
        let cleaned = self.morphological_cleanup(&shadow_mask);
        
        cleaned
    }
    
    fn is_shadow_pixel(&self, intensity: u8, pixel: &Rgb<u8>, threshold: u8) -> bool {
        if intensity > threshold {
            return false;
        }
        
        let r = pixel[0] as f64;
        let g = pixel[1] as f64;
        let b = pixel[2] as f64;
        
        let max_c = r.max(g).max(b);
        let min_c = r.min(g).min(b);
        let saturation = if max_c > 0.0 { (max_c - min_c) / max_c } else { 0.0 };
        
        if saturation > 0.5 {
            return false;
        }
        
        let total = r + g + b;
        if total > 0.0 {
            let blue_ratio = b / total;
            let red_ratio = r / total;
            
            if blue_ratio > 0.4 && red_ratio < 0.35 {
                return true;
            }
        }
        
        intensity < threshold && saturation < 0.3
    }
    
    fn morphological_cleanup(&self, mask: &GrayImage) -> GrayImage {
        let (width, height) = mask.dimensions();
        let mut result = mask.clone();
        
        let eroded = self.erode(&result, 2);
        result = self.dilate(&eroded, 2);
        
        result = self.remove_small_regions(&result, self.config.min_shadow_size);
        
        result
    }
    
    fn erode(&self, image: &GrayImage, radius: i32) -> GrayImage {
        let (width, height) = image.dimensions();
        let mut result = GrayImage::new(width, height);
        
        for y in 0..height {
            for x in 0..width {
                let mut min_val = 255u8;
                
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        
                        if nx >= 0 && nx < width as i32 && ny >= 0 && ny < height as i32 {
                            min_val = min_val.min(image.get_pixel(nx as u32, ny as u32)[0]);
                        }
                    }
                }
                
                result.put_pixel(x, y, Luma([min_val]));
            }
        }
        
        result
    }
    
    fn dilate(&self, image: &GrayImage, radius: i32) -> GrayImage {
        let (width, height) = image.dimensions();
        let mut result = GrayImage::new(width, height);
        
        for y in 0..height {
            for x in 0..width {
                let mut max_val = 0u8;
                
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        
                        if nx >= 0 && nx < width as i32 && ny >= 0 && ny < height as i32 {
                            max_val = max_val.max(image.get_pixel(nx as u32, ny as u32)[0]);
                        }
                    }
                }
                
                result.put_pixel(x, y, Luma([max_val]));
            }
        }
        
        result
    }
    
    fn remove_small_regions(&self, mask: &GrayImage, min_size: u32) -> GrayImage {
        let (width, height) = mask.dimensions();
        let mut result = mask.clone();
        let mut visited = vec![vec![false; width as usize]; height as usize];
        
        for y in 0..height {
            for x in 0..width {
                if mask.get_pixel(x, y)[0] > 0 && !visited[y as usize][x as usize] {
                    let mut component = Vec::new();
                    let mut stack = vec![(x, y)];
                    
                    while let Some((cx, cy)) = stack.pop() {
                        if cx >= width || cy >= height {
                            continue;
                        }
                        if visited[cy as usize][cx as usize] {
                            continue;
                        }
                        if mask.get_pixel(cx, cy)[0] == 0 {
                            continue;
                        }
                        
                        visited[cy as usize][cx as usize] = true;
                        component.push((cx, cy));
                        
                        if cx > 0 { stack.push((cx - 1, cy)); }
                        if cx + 1 < width { stack.push((cx + 1, cy)); }
                        if cy > 0 { stack.push((cx, cy - 1)); }
                        if cy + 1 < height { stack.push((cx, cy + 1)); }
                    }
                    
                    if (component.len() as u32) < min_size {
                        for (px, py) in component {
                            result.put_pixel(px, py, Luma([0]));
                        }
                    }
                }
            }
        }
        
        result
    }
    
    fn calculate_gradients(&self, gray: &GrayImage) -> (GrayImage, GrayImage) {
        let (width, height) = gray.dimensions();
        let mut magnitude = GrayImage::new(width, height);
        let mut direction = GrayImage::new(width, height);
        
        for y in 1..height-1 {
            for x in 1..width-1 {
                let gx = self.sobel_x(gray, x, y);
                let gy = self.sobel_y(gray, x, y);
                
                let mag = (gx * gx + gy * gy).sqrt();
                let dir = gy.atan2(gx);
                
                let dir_normalized = ((dir + PI) / (2.0 * PI) * 255.0) as u8;
                
                magnitude.put_pixel(x, y, Luma([(mag.min(255.0)) as u8]));
                direction.put_pixel(x, y, Luma([dir_normalized]));
            }
        }
        
        (magnitude, direction)
    }
    
    fn sobel_x(&self, gray: &GrayImage, x: u32, y: u32) -> f64 {
        let get = |dx: i32, dy: i32| -> f64 {
            let px = (x as i32 + dx).max(0) as u32;
            let py = (y as i32 + dy).max(0) as u32;
            gray.get_pixel(
                px.min(gray.width() - 1), 
                py.min(gray.height() - 1)
            )[0] as f64 
        };
        
        -get(-1, -1) - 2.0 * get(-1, 0) - get(-1, 1) + get(1, -1) + 2.0 * get(1, 0) + get(1, 1)
    }
    
    fn sobel_y(&self, gray: &GrayImage, x: u32, y: u32) -> f64 {
        let get = |dx: i32, dy: i32| -> f64 {
            let px = (x as i32 + dx).max(0) as u32;
            let py = (y as i32 + dy).max(0) as u32;
            gray.get_pixel(
                px.min(gray.width() - 1), 
                py.min(gray.height() - 1)
            )[0] as f64 
        };
        
        -get(-1, -1) - 2.0 * get(0, -1) - get(1, -1) + get(-1, 1) + 2.0 * get(0, 1) + get(1, 1)
    }
    
    fn analyze_shadow_regions(
        &self,
        shadow_mask: &GrayImage,
        gradient_magnitude: &GrayImage,
        gradient_direction: &GrayImage
    ) -> Vec<ShadowRegion> {
        let (width, height) = shadow_mask.dimensions();
        let block_size = self.config.block_size;
        let mut regions = Vec::new();
        
        let mut visited = vec![vec![false; width as usize]; height as usize];
        
        for y in (0..height).step_by(block_size as usize / 2) {
            for x in (0..width).step_by(block_size as usize / 2) {
                if shadow_mask.get_pixel(x, y)[0] > 0 && !visited[y as usize][x as usize] {
                    let region_info = self.analyze_single_shadow_region(
                        shadow_mask, 
                        gradient_magnitude, 
                        gradient_direction, 
                        x, 
                        y, 
                        &mut visited,
                    );
                    
                    if let Some(info) = region_info {
                        if info.region.width >= self.config.min_shadow_size / 2 || info.region.height >= self.config.min_shadow_size / 2 {
                            regions.push(info);
                        }
                    }
                }
            }
        }
        
        regions
    }
    
    fn analyze_single_shadow_region(
        &self,
        shadow_mask: &GrayImage,
        gradient_magnitude: &GrayImage,
        gradient_direction: &GrayImage,
        start_x: u32,
        start_y: u32,
        visited: &mut Vec<Vec<bool>>
    ) -> Option<ShadowRegion> {
        let (width, height) = shadow_mask.dimensions();
        
        let mut min_x = start_x;
        let mut max_x = start_x;
        let mut min_y = start_y;
        let mut max_y = start_y;
        
        let mut edge_directions = Vec::new();
        let mut edge_magnitudes = Vec::new();
        let mut total_intensity = 0.0;
        let mut pixel_count = 0;
        
        let mut stack = vec![(start_x, start_y)];
        
        while let Some((x, y)) = stack.pop() {
            if x >= width || y >= height {
                continue;
            }
            if visited[y as usize][x as usize] {
                continue;
            }
            if shadow_mask.get_pixel(x, y)[0] == 0 {
                continue;
            }
            
            visited[y as usize][x as usize] = true;
            
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            
            let is_edge = self.is_shadow_edge(shadow_mask, x, y);
            
            if is_edge {
                let mag = gradient_magnitude.get_pixel(x, y)[0] as f64;
                if mag > self.config.gradient_threshold {
                    let dir_normalized = gradient_direction.get_pixel(x, y)[0] as f64;
                    let dir = (dir_normalized / 255.0) * 2.0 * PI - PI;
                    
                    edge_directions.push(dir);
                    edge_magnitudes.push(mag);
                }
            }
            
            total_intensity += shadow_mask.get_pixel(x, y)[0] as f64;
            pixel_count += 1;
            
            if x > 0 { stack.push((x-1, y)); }
            if x + 1 < width { stack.push((x+1, y)); }
            if y > 0 { stack.push((x, y-1)); }
            if y + 1 < height { stack.push((x, y+1)); }
        }
        
        if pixel_count < self.config.min_shadow_size as usize || edge_directions.is_empty() {
            return None;
        }
        
        let (light_direction, direction_confidence) = self.calculate_light_direction(&edge_directions, &edge_magnitudes);
        
        let edge_sharpness = if !edge_magnitudes.is_empty() {
            edge_magnitudes.iter().sum::<f64>() / edge_magnitudes.len() as f64 / 255.0
        } else {
            0.0 
        };
        
        Some(ShadowRegion { 
            region: SRegion { 
                x: min_x, 
                y: min_y, 
                width: max_x - min_x + 1, 
                height: max_y - min_y + 1 
            }, 
            light_direction, 
            direction_confidence, 
            intensity: total_intensity / pixel_count as f64, 
            edge_sharpness 
        })
    }
    
    fn is_shadow_edge(&self, mask: &GrayImage, x: u32, y: u32) -> bool {
        let (width, height) = mask.dimensions();
        
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                
                if nx >= 0 && nx < width as i32 && ny >= 0 && ny < height as i32 {
                    if mask.get_pixel(nx as u32, ny as u32)[0] == 0 {
                        return true;
                    }
                }
            }
        }
        
        false
    }
    
    fn calculate_light_direction(&self, directions: &[f64], magnitudes: &[f64]) -> (f64, f64) {
        if directions.is_empty() {
            return (0.0, 0.0);
        }
        
        let mut sin_sum = 0.0;
        let mut cos_sum = 0.0;
        let mut weight_sum = 0.0;
        
        for (dir, mag) in directions.iter().zip(magnitudes.iter()) {
            let light_dir = dir + PI;
            sin_sum += light_dir.sin() * mag;
            cos_sum += light_dir.cos() * mag;
            weight_sum += mag;
        }
        
        if weight_sum < 1e-10 {
            return (0.0, 0.0);
        }
        
        let mean_sin = sin_sum / weight_sum;
        let mean_cos = cos_sum / weight_sum;
        
        let mean_direction = mean_sin.atan2(mean_cos);
        
        let r = (mean_sin * mean_sin + mean_cos * mean_cos).sqrt();
        let confidence = r;
        
        (mean_direction, confidence)
    }
    
    fn find_dominant_direction(&self, regions: &[ShadowRegion]) -> (f64, f64) {
        if regions.is_empty() {
            return (0.0, 0.0);
        }
        
        let mut sin_sum = 0.0;
        let mut cos_sum = 0.0;
        let mut weight_sum = 0.0;
        
        for region in regions {
            let weight = region.direction_confidence * (region.region.width * region.region.height) as f64;
            sin_sum += region.light_direction.sin() * weight;
            cos_sum += region.light_direction.cos() * weight;
            weight_sum += weight;
        }
        
        if weight_sum < 1e-10 {
            return (0.0, 0.0);
        }
        
        let mean_direction = (sin_sum / weight_sum).atan2(cos_sum / weight_sum);
        let r = ((sin_sum / weight_sum).powi(2) + (cos_sum / weight_sum).powi(2)).sqrt();
        
        (mean_direction, r)
    }
    
    fn estimate_light_sources(&self, regions: &[ShadowRegion]) -> usize {
        if regions.len() < 2 {
            return 1;
        }
        
        let mut directions = regions
            .iter()
            .filter(|r| r.direction_confidence > 0.3)
            .map(|r| r.light_direction)
            .collect::<Vec<_>>();
        
        if directions.is_empty() {
            return 1;
        }
        
        directions.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let gap_threshold = self.config.angle_tolerance.to_radians() * 2.0;
        let mut clusters = 1;
        
        for i in 1..directions.len() {
            let mut gap = directions[i] - directions[i-1];
            
            if gap < 0.0 {
                gap += 2.0 * PI;
            }
            
            if gap > gap_threshold {
                clusters += 1;
            }
        }
        
        let wrap_gap = (directions[0] + 2.0 * PI) - directions[directions.len() - 1];
        if wrap_gap > gap_threshold && clusters > 1 {
            clusters = clusters;
        }
        
        clusters.min(5)
    }
    
    fn find_incosistent_regions(&self, regions: &[ShadowRegion], dominant_direction: f64) -> Vec<SRegion> {
        let tolerance = self.config.angle_tolerance.to_radians();
        let mut incosistent = Vec::new();
        
        for shadow_region in regions {
            if shadow_region.direction_confidence < 0.2 {
                continue;
            }
            
            let mut diff = (shadow_region.light_direction - dominant_direction).abs();
            if diff > PI {
                diff = 2.0 * PI - diff;
            }
            
            if diff > tolerance {
                incosistent.push(shadow_region.region);
            }
        }
        
        incosistent
    }
    
    fn create_direction_map(
        &self,
        original: &RgbImage, 
        regions: &[ShadowRegion],
        dominant_direction: f64,
    ) -> RgbImage {
        let mut vis = original.clone();
        let (width, height) = vis.dimensions();
        
        for shadow_region in regions {
            let mut diff = (shadow_region.light_direction - dominant_direction).abs();
            if diff > PI {
                diff = 2.0 * PI - diff;
            }
            
            let is_consistent = diff < self.config.angle_tolerance.to_radians();
            let color = if is_consistent {
                Rgb([0, 255, 0])
            } else {
                Rgb([255, 0, 0])
            };
            
            self.draw_region_border(&mut vis, &shadow_region.region, color);
            
            let center_x = shadow_region.region.x + shadow_region.region.width / 2;
            let center_y = shadow_region.region.y + shadow_region.region.height / 2;
            let arrow_length = 20.0;
            
            let end_x = center_x as f64 + arrow_length * shadow_region.light_direction.cos();
            let end_y = center_y as f64 - arrow_length * shadow_region.light_direction.sin();
            
            self.draw_arrow(&mut vis, center_x, center_y, end_x as u32, end_y as u32, color);
        }
        
        let indicator_x = 30u32;
        let indicator_y = 30u32;
        let arrow_len = 25.0;
        
        let end_x = indicator_x as f64 + arrow_len * dominant_direction.cos();
        let end_y = indicator_y as f64 - arrow_len * dominant_direction.sin();
        
        self.draw_arrow(&mut vis, indicator_x, indicator_y, end_x as u32, end_y as u32, Rgb([255, 255, 0]));
        
        vis 
    }
    
    fn draw_region_border(&self, image: &mut RgbImage, region: &SRegion, color: Rgb<u8>) {
        let (width, height) = image.dimensions();
        
        for x in region.x..(region.x + region.width).min(width) {
            if region.y < height {
                image.put_pixel(x, region.y, color);
            }
            if region.y + region.height - 1 < height {
                image.put_pixel(x, region.y + region.height - 1, color);
            }
        }
        
        for y in region.y..(region.y + region.height).min(height) {
            if region.x < width {
                image.put_pixel(region.x, y, color);
            }
            if region.x + region.width - 1 < width {
                image.put_pixel(region.x + region.width - 1, y, color);
            }
        }
    }
    
    fn draw_arrow(&self, image: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, color: Rgb<u8>) {
        let (width, height) = image.dimensions();
        
        let dx = (x1 as i32 - x0 as i32).abs();
        let dy = -(y1 as i32 - y0 as i32).abs();
        let sx = if x0 < x1 { 1i32 } else { -1i32 };
        let sy = if y0 < y1 { 1i32 } else { -1i32 };
        let mut err = dx + dy;
        
        let mut x = x0 as i32;
        let mut y = y0 as i32;
        
        loop {
            if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
                image.put_pixel(x as u32, y as u32, color);
            }
            
            if x == x1 as i32 && y == y1 as i32 {
                break;
            }
            
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
        
        let angle = (y1 as f64 - y0 as f64).atan2(x1 as f64 - x0 as f64);
    }
    
    fn draw_line(&self, image: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, color: Rgb<u8>) {
        let (width, height) = image.dimensions();
        
        let dx = (x1 as i32 - x0 as i32).abs();
        let dy = -(y1 as i32 - y0 as i32).abs();
        let sx = if x0 < x1 { 1i32 } else { -1i32 };
        let sy = if y0 < y1 { 1i32 } else { -1i32 };
        let mut err = dx + dy;
        
        let mut x = x0 as i32;
        let mut y = y0 as i32;
        
        loop {
            if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
                image.put_pixel(x as u32, y as u32, color);
            }
            
            if x == x1 as i32 && y == y1 as i32 {
                break;
            }
            
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }
    
    fn calculate_consistency_score(&self, regions: &[ShadowRegion], dominant_direction: f64) -> f64 {
        if regions.is_empty() {
            return 1.0;
        }
        
        let tolerance = self.config.angle_tolerance.to_radians();
        let mut consistent_weight = 0.0;
        let mut total_weight = 0.0;
        
        for region in regions {
            let weight = region.direction_confidence * (region.region.width * region.region.height) as f64;
            
            let mut diff = (region.light_direction - dominant_direction).abs();
            if diff > PI {
                diff = 2.0 * PI - diff;
            } else if diff < tolerance * 2.0 {
                consistent_weight += weight * 0.5;
            }
            
            total_weight += weight;
        }
        
        if total_weight > 0.0 {
            consistent_weight / total_weight
        } else {
            1.0
        }
    }
    
    fn calculate_manipulation_probability(
        &self,
        regions: &[ShadowRegion],
        inconsistent: &[SRegion],
        consistency_score: f64,
        light_sources: usize,
    ) -> f64 {
        let mut probability = 0.0;
        
        if !regions.is_empty() {
            let inconsistent_ratio = inconsistent.len() as f64 / regions.len() as f64;
            probability += inconsistent_ratio * 0.4;
        }
        
        probability += (1.0 - consistency_score) * 0.3;
        
        if light_sources > 2 {
            probability += (light_sources - 2) as f64 * 0.15;
        }
        
        probability.min(1.0)
    }
}

impl Default for ShadowAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}