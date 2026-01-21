use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};

use crate::{SRegion, error::Result};

#[derive(Debug, Clone)]
pub struct ChromaticAbberationConfig {
    pub block_size: u32,
    pub edge_threshold: f64,
    pub min_edge_strength: f64,
    pub max_aberration: f64,
    pub inconsistency_threshold: f64,
}

impl Default for ChromaticAbberationConfig {
    fn default() -> Self {
        Self { 
            block_size: 64, 
            edge_threshold: 30.0, 
            min_edge_strength: 20.0, 
            max_aberration: 5.0, 
            inconsistency_threshold: 1.5 
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AberrationMeasurement {
    pub x: u32,
    pub y: u32,
    pub rg_shift_x: f64,
    pub rg_shift_y: f64,
    pub bg_shift_x: f64,
    pub bg_shift_y: f64,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct ChromaticAberrationResult {
    pub measurements: Vec<AberrationMeasurement>,
    pub aberration_map: GrayImage,
    pub inconsistency_map: GrayImage,
    pub visualization: RgbImage,
    pub inconsistent_regions: Vec<SRegion>,
    pub optical_center: Option<(f64, f64)>,
    pub radial_model: Option<RadialAberrationModel>,
    pub consistency_score: f64,
    pub manipulation_probability: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct RadialAberrationModel {
    pub center_x: f64,
    pub center_y: f64,
    pub k_red: f64,
    pub k_blue: f64,
    pub fit_quality: f64,
}

pub struct ChromaticAberrationAnalyzer {
    config: ChromaticAbberationConfig,
}

impl ChromaticAberrationAnalyzer {
    pub fn new() -> Self {
        Self::with_config(ChromaticAbberationConfig::default())
    }
    
    pub fn with_config(config: ChromaticAbberationConfig) -> Self {
        Self { config }
    }
    
    pub fn analyze(&self, image: &DynamicImage) -> Result<ChromaticAberrationResult> {
        let rgb = image.to_rgb8();
        let (width, height) = rgb.dimensions();
        
        if width < self.config.block_size * 2 || height < self.config.block_size * 2 {
            return Err(crate::error::ForensicsError::ImageTooSmall(self.config.block_size * 2));
        }
        
        let (red, green, blue) = self.split_channels(&rgb);
        
        let measurements = self.measure_aberration(&red, &green, &blue);
        
        let aberration_map = self.create_abberation_map(width, height, &measurements);
        
        let radial_model = self.fit_radial_model(&measurements, width, height);
        
        let expected_aberrations = radial_model
            .map(|model| self.calculate_expected_aberrations(&measurements, &model));
        
        let inconsistency_map = self.create_inconsistency_map(width, height, &measurements, expected_aberrations.as_ref());
        
        let inconsistent_regions = self.find_inconsistent_regions(&inconsistency_map);
        
        let visualization = self.create_visualization(&rgb, &measurements, radial_model.as_ref());
        
        let consistency_score = self.calculate_consistency_score(&measurements, expected_aberrations.as_ref());
        
        let manipulation_probability = self.calculate_manipulation_probability( 
            &inconsistent_regions, 
            consistency_score, 
            width, 
            height
        );
        
        let optical_center = radial_model.map(|m| (m.center_x, m.center_y));
        
        Ok(ChromaticAberrationResult { 
            measurements, 
            aberration_map, 
            inconsistency_map, 
            visualization, 
            inconsistent_regions, 
            optical_center, 
            radial_model, 
            consistency_score, 
            manipulation_probability 
        })
    }
    
    fn split_channels(&self, rgb: &RgbImage) -> (GrayImage, GrayImage, GrayImage) {
        let (width, height) = rgb.dimensions();
        let mut red = GrayImage::new(width, height);
        let mut green = GrayImage::new(width, height);
        let mut blue = GrayImage::new(width, height);
        
        for (x, y, pixel) in rgb.enumerate_pixels() {
            red.put_pixel(x, y, Luma([pixel[0]]));
            green.put_pixel(x, y, Luma([pixel[1]]));
            blue.put_pixel(x, y, Luma([pixel[2]]));
        }
        
        (red, green, blue)
    }
    
    fn measure_aberration(
        &self,
        red: &GrayImage,
        green: &GrayImage,
        blue: &GrayImage
    ) -> Vec<AberrationMeasurement> {
        let (width, height) = green.dimensions();
        let block_size = self.config.block_size;
        let mut measurements = Vec::new();
        
        for by in (0..height - block_size).step_by(block_size as usize / 2) {
            for bx in (0..width - block_size).step_by(block_size as usize / 2) {
                if let Some(measurement) = self.measure_block_aberration(
                    red, green, blue, bx, by, block_size
                ) {
                    measurements.push(measurement);
                }
            }
        }
        
        measurements
    }
    
    fn measure_block_aberration(
        &self,
        red: &GrayImage,
        green: &GrayImage,
        blue: &GrayImage,
        bx: u32,
        by: u32,
        size: u32
    ) -> Option<AberrationMeasurement> {
        let edge_points = self.find_edge_points(green, bx, by, size);
        
        if edge_points.len() < 10 {
            return None;
        }
        
        let (rg_shift_x, rg_shift_y, rg_confidence) = self.measure_channel_shift(red, green, &edge_points);
        let (bg_shift_x, bg_shift_y, bg_confidence) = self.measure_channel_shift(blue, green, &edge_points);
        
        let confidence = (rg_confidence + bg_confidence) / 2.0;
        
        if confidence < 0.3 {
            return None;
        }
        
        let max_shift = self.config.max_aberration;
        if rg_shift_x.abs() > max_shift || rg_shift_y.abs() > max_shift || bg_shift_x.abs() > max_shift || bg_shift_y.abs() > max_shift {
            return None;
        }
        
        Some(AberrationMeasurement { 
            x: bx + size / 2, 
            y: by + size / 2, 
            rg_shift_x, 
            rg_shift_y, 
            bg_shift_x, 
            bg_shift_y, 
            confidence 
        })
    }
    
    fn find_edge_points(
        &self,
        gray: &GrayImage,
        bx: u32,
        by: u32,
        size: u32,
    ) -> Vec<(u32, u32, f64, f64)> {
        let mut edges = Vec::new();
        let (width, height) = gray.dimensions();
        
        for y in (by + 1)..(by + size - 1).min(height - 1) {
            for x in (bx + 1)..(bx + size - 1).min(width - 1) {
                let gx = self.sobel_x(gray, x, y);
                let gy = self.sobel_y(gray, x, y);
                let magnitude = (gx * gx + gy * gy).sqrt();
                
                if magnitude > self.config.edge_threshold {
                    edges.push((x, y, gx, gy));
                }
            }
        }
        
        edges
    }
    
    fn sobel_x(&self, gray: &GrayImage, x: u32, y: u32) -> f64 {
        let get = |dx: i32, dy: i32| -> f64 {
            let px = (x as i32 + dx).max(0) as u32;
            let py = (y as i32 + dy).max(0) as u32;
            let (w, h) = gray.dimensions();
            gray.get_pixel(px.min(w - 1), py.min(h - 1))[0] as f64
        };
            
        -get(-1, -1) - 2.0 * get(-1, 0) - get(-1, 1)
        + get(1, -1) + 2.0 * get(1, 0) + get(1, 1)
    }
        
    fn sobel_y(&self, gray: &GrayImage, x: u32, y: u32) -> f64 {
        let get = |dx: i32, dy: i32| -> f64 {
            let px = (x as i32 + dx).max(0) as u32;
            let py = (y as i32 + dy).max(0) as u32;
            let (w, h) = gray.dimensions();
            gray.get_pixel(px.min(w - 1), py.min(h - 1))[0] as f64
        };
            
        -get(-1, -1) - 2.0 * get(0, -1) - get(1, -1)
        + get(-1, 1) + 2.0 * get(0, 1) + get(1, 1)
    }
    
    fn measure_channel_shift(
        &self,
        channel: &GrayImage,
        reference: &GrayImage,
        edge_points: &[(u32, u32, f64, f64)],
    ) -> (f64, f64, f64) {
        let (width, height) = reference.dimensions();
        let search_radius = self.config.max_aberration.ceil() as i32;
        
        let mut best_shift_x = 0.0;
        let mut best_shift_y = 0.0;
        let mut best_correlation = 0.0;
        
        for sy in -search_radius..=search_radius {
            for sx in -search_radius..=search_radius {
                for sub_y in 0..3 {
                    for sub_x in 0..3 {
                        let shift_x = sx as f64 + (sub_x as f64 - 1.0) / 3.0;
                        let shift_y = sy as f64 + (sub_y as f64 - 1.0) / 3.0;
                        
                        let correlation = self.calculate_edge_correlation(
                            channel, reference, edge_points, shift_x, shift_y
                        );
                        
                        if correlation > best_correlation {
                            best_correlation = correlation;
                            best_shift_x = shift_x;
                            best_shift_y = shift_y;
                        }
                    }
                }
            }
        }
        
        (best_shift_x, best_shift_y, best_correlation)
    }
    
    fn calculate_edge_correlation(
        &self,
        channel: &GrayImage,
        reference: &GrayImage,
        edge_points: &[(u32, u32, f64, f64)],
        shift_x: f64,
        shift_y: f64,
    ) -> f64 {
        let (width, height) = reference.dimensions();
        let mut sum_product = 0.0;
        let mut sum_ref_sq = 0.0;
        let mut sum_ch_sq = 0.0;
        let mut count = 0;
        
        for &(x, y, _, _) in edge_points {
            let ref_val = reference.get_pixel(x, y)[0] as f64;
            
            let shifted_x = x as f64 + shift_x;
            let shifted_y = y as f64 + shift_y;
            
            if shifted_x >= 0.0 && shifted_y < (width - 1) as f64 &&
            shifted_y >= 0.0 && shifted_y < (height - 1) as f64 {
                let ch_val = self.bilinear_sample(channel, shifted_x, shifted_y);
                
                sum_product += ref_val * ch_val;
                sum_ref_sq += ref_val * ref_val;
                sum_ch_sq += ch_val * ch_val;
                count += 1;
            }
        }
        
        if count == 0 || sum_ref_sq == 0.0 || sum_ch_sq == 0.0 {
            return 0.0;
        }
        
        sum_product / (sum_ref_sq.sqrt() * sum_ch_sq.sqrt())
    }
    
    fn bilinear_sample(&self, image: &GrayImage, x: f64, y: f64) -> f64 {
        let x0 = x.floor() as u32;
        let y0 = y.floor() as u32;
        let x1 = x0 + 1;
        let y1 = y0 + 1;
        
        let fx = x - x0 as f64;
        let fy = y - y0 as f64;
        
        let (width, height) = image.dimensions();
        
        let v00 = image.get_pixel(x0.min(width-1), y0.min(height-1))[0] as f64;
        let v10 = image.get_pixel(x1.min(width-1), y0.min(height-1))[0] as f64;
        let v01 = image.get_pixel(x0.min(width-1), y1.min(height-1))[0] as f64;
        let v11 = image.get_pixel(x1.min(width-1), y1.min(height-1))[0] as f64;
        
        v00 * (1.0 - fx) * (1.0 - fy) +
        v10 * fx * (1.0 - fy) +
        v01 * (1.0 - fx) * fy +
        v11 * fx * fy
    }
    
    fn create_abberation_map(
        &self,
        width: u32,
        height: u32,
        measurements: &[AberrationMeasurement]
    ) -> GrayImage {
        let mut map = GrayImage::new(width, height);
        let block_size = self.config.block_size;
        
        let max_aberration = measurements
            .iter()
            .map(|m| {
                let rg = (m.rg_shift_x.powi(2) + m.rg_shift_y.powi(2)).sqrt();
                let bg = (m.bg_shift_x.powi(2) + m.bg_shift_y.powi(2)).sqrt();
                rg.max(bg)
            })
            .fold(0.0, f64::max)
            .max(0.1);
        
        for measurement in measurements {
            let rg = (measurement.rg_shift_x.powi(2) + measurement.rg_shift_y.powi(2)).sqrt();
            let bg = (measurement.bg_shift_x.powi(2) + measurement.bg_shift_y.powi(2)).sqrt();
            let magnitude = rg.max(bg);
            let normalized = ((magnitude / max_aberration) * 255.0) as u8;
            
            let bx = measurement.x.saturating_sub(block_size / 2);
            let by = measurement.y.saturating_sub(block_size / 2);
            
            for y in by..(by + block_size).min(height) {
                for x in bx..(bx + block_size).min(width) {
                    let current = map.get_pixel(x, y)[0];
                    map.put_pixel(x, y, Luma([current.max(normalized)]));
                }
            }
        }
        
        map 
    }
    
    fn fit_radial_model(
        &self,
        measurements: &[AberrationMeasurement],
        width: u32,
        height: u32,
    ) -> Option<RadialAberrationModel> {
        if measurements.len() < 10 {
            return None;
        }
        
        let center_x = width as f64 / 2.0;
        let center_y = height as f64 / 2.0;
        
        let mut sum_r_sq = 0.0;
        let mut sum_r_shift_red = 0.0;
        let mut sum_r_shift_blue = 0.0;
        let mut count = 0.0;
        
        for m in measurements {
            let dx = m.x as f64 - center_x;
            let dy = m.y as f64 - center_y;
            let r = (dx * dx + dy * dy).sqrt();
            
            if r < 10.0 {
                continue;
            }
            
            let radial_dir_x = dx / r;
            let radial_dir_y = dy / r;
            
            let rg_radial = m.rg_shift_x * radial_dir_x + m.rg_shift_y * radial_dir_y;
            let bg_radial = m.bg_shift_x * radial_dir_x + m.bg_shift_y * radial_dir_y;
            
            sum_r_sq += r * r * m.confidence;
            sum_r_shift_red += r * rg_radial * m.confidence;
            sum_r_shift_blue += r * bg_radial * m.confidence;
            count += m.confidence;
        }
        
        if sum_r_sq < 1e-10 {
            return None;
        }
        
        let k_red = sum_r_shift_red / sum_r_sq;
        let k_blue = sum_r_shift_blue / sum_r_sq;
        
        let fit_quality = self.calculate_model_fit(measurements, center_x, center_y, k_red, k_blue);
        
        Some(RadialAberrationModel { 
            center_x, 
            center_y, 
            k_red, 
            k_blue, 
            fit_quality 
        })
    }
    
    fn calculate_model_fit(
        &self,
        measurements: &[AberrationMeasurement],
        center_x: f64,
        center_y: f64,
        k_red: f64,
        k_blue: f64 
    ) -> f64 {
        if measurements.is_empty() {
            return 0.0;
        }
        
        let mut ss_res = 0.0;
        let mut ss_tot = 0.0;
        
        let mean_rg = measurements
            .iter()
            .map(|m| (m.rg_shift_x.powi(2) + m.rg_shift_y.powi(2)).sqrt())
            .sum::<f64>() / measurements.len() as f64;
        
        for m in measurements {
            let dx = m.x as f64 - center_x;
            let dy = m.y as f64 - center_y;
            let r = (dx * dx + dy * dy).sqrt();
            
            if r < 10.0 {
                continue;
            }
            
            let radial_dir_x = dx / r;
            let radial_dir_y = dy / r;
            
            let expected_rg_x = k_red * r * radial_dir_x;
            let expected_rg_y = k_red * r * radial_dir_y;
            
            let res_x = m.rg_shift_x - expected_rg_x;
            let res_y = m.rg_shift_y - expected_rg_y;
            ss_res += res_x * res_x + res_y * res_y;
            
            let actual_mag = (m.rg_shift_x.powi(2) + m.rg_shift_y.powi(2)).sqrt();
            ss_tot += (actual_mag - mean_rg).powi(2);
        }
        
        if ss_tot < 1e-10 {
            return 0.0;
        }
        
        (1.0 - ss_res / ss_tot).max(0.0)
    }
    
    fn calculate_expected_aberrations(
        &self,
        measurements: &[AberrationMeasurement],
        model: &RadialAberrationModel
    ) -> Vec<(f64, f64, f64, f64)> {
        measurements
            .iter()
            .map(|m| {
                let dx = m.x as f64 - model.center_x;
                let dy = m.y as f64 - model.center_y;
                let r = (dx * dx + dy * dy).sqrt();
                
                let radial_dir_x = if r > 0.0 { dx / r } else { 0.0 };
                let radial_dir_y = if r > 0.0 { dy / r } else { 0.0 };
                
                let expected_rg_x = model.k_red * r * radial_dir_x;
                let expected_rg_y = model.k_red * r * radial_dir_y;
                let expected_bg_x = model.k_blue * r * radial_dir_x;
                let expected_bg_y = model.k_blue * r* radial_dir_y;
                
                (expected_rg_x, expected_rg_y, expected_bg_x, expected_bg_y)
            })
            .collect::<Vec<_>>()
    }
    
    fn create_inconsistency_map(
        &self,
        width: u32,
        height: u32,
        measurements: &[AberrationMeasurement],
        expected: Option<&Vec<(f64, f64, f64, f64)>>
    ) -> GrayImage {
        let mut map = GrayImage::new(width, height);
        let block_size = self.config.block_size;
        
        for (i, m) in measurements.iter().enumerate() {
            let inconsistency = if let Some(exp) = expected {
                let (exp_rg_x, exp_rg_y, exp_bg_x, exp_bg_y) = exp[i];
                
                let rg_error = ((m.rg_shift_x - exp_rg_x).powi(2) + (m.rg_shift_y - exp_rg_y).powi(2)).sqrt();
                let bg_error = ((m.bg_shift_x - exp_bg_x).powi(2) + (m.bg_shift_y - exp_bg_y).powi(2)).sqrt();
                
                (rg_error + bg_error) / 2.0 
            } else {
                0.0 
            };
            
            let normalized = ((inconsistency / self.config.max_aberration) * 255.0).min(255.0) as u8;
            
            let bx = m.x.saturating_sub(block_size / 2);
            let by = m.y.saturating_sub(block_size / 2);
            
            for y in by..(by + block_size).min(height) {
                for x in bx..(bx + block_size).min(width) {
                    let current = map.get_pixel(x, y)[0];
                    map.put_pixel(x, y, Luma([current.max(normalized)]));
                }
            }
        }
        
        map
    }
    
    fn find_inconsistent_regions(&self, inconsistency_map: &GrayImage) -> Vec<SRegion> {
        let (width, height) = inconsistency_map.dimensions();
        let block_size = self.config.block_size;
        let threshold = (self.config.inconsistency_threshold * 50.0) as u8;
        
        let mut regions = Vec::new();
        
        for by in (0..height).step_by(block_size as usize) {
            for bx in (0..width).step_by(block_size as usize) {
                let block_w = block_size.min(width - bx);
                let block_h = block_size.min(height - by);
                
                let mut sum = 0u32;
                let mut count = 0;
                
                for y in by..(by + block_h) {
                    for x in bx..(bx + block_w) {
                        sum += inconsistency_map.get_pixel(x, y)[0] as u32;
                        count += 1;
                    }
                }
                
                let avg = (sum / count) as u8;
                
                if avg > threshold {
                    regions.push(SRegion { 
                        x: bx, 
                        y: by, 
                        width: block_w, 
                        height: block_h 
                    });
                }
            }
        }
        
        regions 
    }
    
    fn create_visualization(
        &self,
        original: &RgbImage,
        measurements: &[AberrationMeasurement],
        model: Option<&RadialAberrationModel>,
    ) -> RgbImage {
        let mut vis = original.clone();
        let scale = 10.0;
        
        for m in measurements {
            let end_x = m.x as i32 + (m.rg_shift_x * scale) as i32;
            let end_y = m.y as i32 + (m.rg_shift_y * scale) as i32;
            self.draw_line(&mut vis, m.x, m.y, end_x as u32, end_y as u32, Rgb([255, 0, 0]));
            
            let end_x = m.x as i32 + (m.bg_shift_x * scale) as i32;
            let end_y = m.y as i32 + (m.bg_shift_y * scale) as i32;
            self.draw_line(&mut vis, m.x, m.y, end_x as u32, end_y as u32, Rgb([0, 0, 255]));
        }
        
        if let Some(model) = model {
            let cx = model.center_x as u32;
            let cy = model.center_y as u32;
            let (width, height) = vis.dimensions();
            
            for d in 0..20 {
                if cx + d < width { vis.put_pixel(cx + d, cy, Rgb([255, 255, 0])); }
                if cx >= d { vis.put_pixel(cx - d, cy, Rgb([255, 255, 0])); }
                if cy + d < height { vis.put_pixel(cx, cy + d, Rgb([255, 255, 0])); }
                if cy >= d { vis.put_pixel(cx, cy - d, Rgb([255, 255, 0])); }
            }
        }
        
        vis 
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
    
    fn calculate_consistency_score(
        &self,
        measurements: &[AberrationMeasurement],
        expected: Option<&Vec<(f64, f64, f64, f64)>>,
    ) -> f64 {
        if measurements.is_empty() {
            return 1.0;
        }
        
        if let Some(exp) = expected {
            let mut total_error = 0.0;
            let mut total_weight = 0.0;
            
            for (i, m) in measurements.iter().enumerate() {
                let (exp_rg_x, exp_rg_y, exp_bg_x, exp_bg_y) = exp[i];
                
                let rg_error = ((m.rg_shift_x - exp_rg_x).powi(2) + (m.rg_shift_y - exp_rg_y).powi(2)).sqrt();
                let bg_error = ((m.bg_shift_x - exp_bg_x).powi(2) + (m.bg_shift_y - exp_bg_y).powi(2)).sqrt();
                
                total_error += (rg_error + bg_error) * m.confidence;
                total_weight += m.confidence;
            }
            
            if total_weight > 0.0 {
                let avg_error = total_error / total_weight;
                (1.0 - avg_error / self.config.max_aberration).max(0.0).min(1.0)
            } else {
                1.0 
            }
        } else {
            let shifts = measurements
                .iter()
                .map(|m| (m.rg_shift_x.powi(2) + m.rg_shift_y.powi(2)).sqrt())
                .collect::<Vec<_>>();
            let mean = shifts.iter().sum::<f64>() / shifts.len() as f64;
            let variance = shifts
                .iter()
                .map(|s| (s - mean).powi(2))
                .sum::<f64>() / shifts.len() as f64;
            let std_dev = variance.sqrt();
            
            (1.0 - std_dev / self.config.max_aberration).max(0.0).min(1.0)
        }
    }
    
    fn calculate_manipulation_probability(
        &self,
        inconsistent_regions: &[SRegion],
        consistency_score: f64,
        width: u32,
        height: u32,
    ) -> f64 {
        let total_pixels = (width * height) as f64;
        
        let inconsistent_pixels = inconsistent_regions
            .iter()
            .map(|r| r.width * r.height)
            .sum::<u32>();
        
        let coverage = inconsistent_pixels as f64 / total_pixels;
        
        let prob = coverage * 0.4 + (1.0 - consistency_score) * 0.6;
        
        prob.min(1.0)
    }
}

impl Default for ChromaticAberrationAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
