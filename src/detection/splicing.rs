use image::{GrayImage, Luma, Rgb, RgbImage};

use crate::{SRegion, analysis::{ela::ElaAnalyzer, noise::NoiseAnalyzer}, detection::{ConfidenceLevel, DetectedManipulation, DetectionResult, Detector}, error::Result, image_utils::rgb_to_gray};

#[derive(Debug, Clone)]
pub struct SplicingConfig {
    pub block_size: u32,
    pub color_sensitivity: f64,
    pub noise_sensitivity: f64,
    pub edge_sensitivity: f64,
    pub min_region_size: u32,
    pub ela_quality: u8,
}

impl Default for SplicingConfig {
    fn default() -> Self {
        Self { 
            block_size: 16, 
            color_sensitivity: 0.5, 
            noise_sensitivity: 0.5, 
            edge_sensitivity: 0.5, 
            min_region_size: 100, 
            ela_quality: 95 
        }
    }
}

pub struct SplicingDetector {
    config: SplicingConfig
}

impl SplicingDetector {
    pub fn new() -> Self {
        Self { config: SplicingConfig::default() }
    }
    
    pub fn with_config(config: SplicingConfig) -> Self {
        Self { config }
    }
    
    fn analyze_color_consistency(&self, image: &RgbImage) -> (GrayImage, Vec<SRegion>) {
        let (width, height) = image.dimensions();
        let block_size = self.config.block_size;
        let mut inconsistency_map = GrayImage::new(width, height);
        let mut suspicious_regions = Vec::new();
            
        let global_histogram = self.calculate_color_histogram(image, 0, 0, width, height);
            
        for by in (0..height).step_by(block_size as usize) {
            for bx in (0..width).step_by(block_size as usize) {
                let block_w = block_size.min(width - bx);
                let block_h = block_size.min(height - by);
                    
                let block_histogram = self.calculate_color_histogram(image, bx, by, block_w, block_h);
                    
                let diff = self.histogram_difference(&global_histogram, &block_histogram);
                let inconsistency = (diff * 255.0 * self.config.color_sensitivity) as u8;
                    
                for y in by..(by + block_h) {
                    for x in bx..(bx + block_w) {
                        inconsistency_map.put_pixel(x, y, Luma([inconsistency]));
                    }
                }
                    
                if diff > 0.3 * self.config.color_sensitivity {
                    suspicious_regions.push(SRegion {
                        x: bx,
                        y: by,
                        width: block_w,
                        height: block_h,
                    });
                }
            }
        }
            
        (inconsistency_map, suspicious_regions)
    }
    
    fn calculate_color_histogram(&self, image: &RgbImage, x: u32, y: u32, w: u32, h: u32) -> [[[u32; 8]; 8]; 8] {
        let mut histogram = [[[0u32; 8]; 8]; 8];
            
        for py in y..(y + h).min(image.height()) {
            for px in x..(x + w).min(image.width()) {
                let pixel = image.get_pixel(px, py);
                let r_bin = (pixel[0] / 32) as usize;
                let g_bin = (pixel[1] / 32) as usize;
                let b_bin = (pixel[2] / 32) as usize;
                histogram[r_bin][g_bin][b_bin] += 1;
            }
        }
            
        histogram
    }
    
    fn histogram_difference(&self, h1: &[[[u32; 8]; 8]; 8], h2: &[[[u32; 8]; 8]; 8]) -> f64 {
        let mut sum1 = 0u32;
        let mut sum2 = 0u32;
        let mut diff_sum = 0.0;
            
        for r in 0..8 {
            for g in 0..8 {
                for b in 0..8 {
                    sum1 += h1[r][g][b];
                    sum2 += h2[r][g][b];
                }
            }
        }
            
        if sum1 == 0 || sum2 == 0 {
            return 0.0;
        }
            
        for r in 0..8 {
            for g in 0..8 {
                for b in 0..8 {
                    let n1 = h1[r][g][b] as f64 / sum1 as f64;
                    let n2 = h2[r][g][b] as f64 / sum2 as f64;
                    diff_sum += (n1 - n2).abs();
                }
            }
        }
            
        diff_sum / 2.0 // Normalize to 0-1
    }
    
    fn detect_edge_inconsistencies(&self, image: &RgbImage) -> (GrayImage, Vec<SRegion>) {
        let gray = rgb_to_gray(image);
        let (width, height) = gray.dimensions();
        let mut edge_map = GrayImage::new(width, height);
        let mut suspicious_regions = Vec::new();
        
        for y in 1..height-1 {
            for x in 1..width-1 {
                let gx = self.sobel_x(&gray, x, y);
                let gy = self.sobel_y(&gray, x, y);
                let magnitude = (gx * gx + gy * gy).sqrt();
                edge_map.put_pixel(x, y, Luma([(magnitude.min(255.0)) as u8]));
            }
        }
        
        let suspicious = self.find_unnatural_edges(&edge_map);
        suspicious_regions.extend(suspicious);
        
        (edge_map, suspicious_regions)
    }
    
    fn sobel_x(&self, gray: &GrayImage, x: u32, y: u32) -> f64 {
        let get_pixel = |dx: i32, dy: i32| -> f64 {
            let px = (x as i32 + dx).max(0) as u32;
            let py = (y as i32 + dy).max(0) as u32;
            gray.get_pixel(px.min(gray.width() - 1), py.min(gray.height() - 1))[0] as f64
        };
            
        -get_pixel(-1, -1) - 2.0 * get_pixel(-1, 0) - get_pixel(-1, 1)
        + get_pixel(1, -1) + 2.0 * get_pixel(1, 0) + get_pixel(1, 1)
    }
        
    fn sobel_y(&self, gray: &GrayImage, x: u32, y: u32) -> f64 {
        let get_pixel = |dx: i32, dy: i32| -> f64 {
            let px = (x as i32 + dx).max(0) as u32;
            let py = (y as i32 + dy).max(0) as u32;
            gray.get_pixel(px.min(gray.width() - 1), py.min(gray.height() - 1))[0] as f64
        };
            
        -get_pixel(-1, -1) - 2.0 * get_pixel(0, -1) - get_pixel(1, -1)
        + get_pixel(-1, 1) + 2.0 * get_pixel(0, 1) + get_pixel(1, 1)
    }
    
    fn find_unnatural_edges(&self, edge_map: &GrayImage) -> Vec<SRegion> {
        let (width, height) = edge_map.dimensions();
        let mut regions = Vec::new();
        let block_size = self.config.block_size;
        
        for by in (0..height).step_by(block_size as usize) {
            for bx in (0..width).step_by(block_size as usize) {
                let block_w = block_size.min(width - bx);
                let block_h = block_size.min(height - by);
                
                let (horizontal_score, vertical_score) = self.analyze_edge_regularity(edge_map, bx, by, block_w, block_h);
                
                if horizontal_score > 0.7 || vertical_score > 0.7 {
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
    
    fn analyze_edge_regularity(&self, edge_map: &GrayImage, x: u32, y: u32, w: u32, h: u32) -> (f64, f64) {
        let mut horizontal_edges = vec![0.0; h as usize];
        let mut vertical_edges = vec![0.0; w as usize];
        
        for dy in 0..h {
            for dx in 0..w {
                let px = x + dx;
                let py = y + dy;
                if px < edge_map.width() && py < edge_map.height() {
                    let edge_val = edge_map.get_pixel(px, py)[0] as f64;
                    horizontal_edges[dy as usize] += edge_val;
                    vertical_edges[dx as usize] += edge_val;
                }
            }
        }
        
        let h_regularity = self.calculate_regularity(&horizontal_edges);
        let v_regularity = self.calculate_regularity(&vertical_edges);
        
        (h_regularity, v_regularity)
    }
    
    fn calculate_regularity(&self, values: &[f64]) -> f64 {
        if values.len() < 3 {
            return 0.0;
        }
        
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        if mean < 10.0 {
            return 0.0;
        }
        
        let threshold = mean * 1.5;
        let peaks = values.iter()
            .enumerate()
            .filter(|&(_, v)| *v > threshold)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        
        if peaks.len() < 2 {
            return 0.0;
        }
        
        let mut intervals = Vec::new();
        for i in 1..peaks.len() {
            intervals.push(peaks[i] - peaks[i-1]);
        }
        
        if intervals.is_empty() {
            return 0.0;
        }
        
        let mean_interval = intervals.iter().map(|&i| i as f64).sum::<f64>() / intervals.len() as f64;
        let variance = intervals
            .iter()
            .map(|&i| (i as f64 - mean_interval).powi(2))
            .sum::<f64>() / intervals.len() as f64;
        
        let regularity = 1.0 / (1.0 + variance.sqrt());
        
        regularity
    }
    
    fn combine_analyses(
        &self,
        color_regions: &[SRegion],
        edge_regions: &[SRegion],
        noise_regions: &[SRegion],
        ela_regions: &[SRegion]
    ) -> Vec<(SRegion, f64)> {
        let mut combined = Vec::new();
        
        for color_region in color_regions {
            let mut score = 0.25;
            let mut evidence_count = 1;
            
            for edge_region in edge_regions {
                if self.regions_overlap(color_region, edge_region) {
                    score += 0.25;
                    evidence_count += 1;
                }
            }
            
            for noise_region in noise_regions {
                if self.regions_overlap(color_region, noise_region) {
                    score += 0.25;
                    evidence_count += 1;
                }
            }
            
            for ela_region in ela_regions {
                if self.regions_overlap(color_region, ela_region) {
                    score += 0.25;
                    evidence_count += 1;
                }
            }
            
            if evidence_count >= 2 {
                combined.push((*color_region, score));
            }
        }
        
        self.merge_overlapping_detections(combined)
    }
    
    fn regions_overlap(&self, a: &SRegion, b: &SRegion) -> bool {
        !(a.x + a.width <= b.x ||
            b.x + b.width <= a.x ||
            a.y + a.height <= b.y ||
            b.y + b.height <= a.y)
    }
    
    fn merge_overlapping_detections(&self, mut detections: Vec<(SRegion, f64)>) -> Vec<(SRegion, f64)> {
        if detections.is_empty() {
            return detections;
        }
        
        detections.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        let mut merged = Vec::new();
        let mut used = vec![false; detections.len()];
        
        for i in 0..detections.len() {
            if used[i] {
                continue;
            }
            
            let mut current = detections[i].0;
            let mut max_score = detections[i].1;
            used[i] = true;
            
            loop {
                let mut found = false;
                for j in 0..detections.len() {
                    if used[j] {
                        continue;
                    }
                    
                    if self.regions_overlap(&current, &detections[j].0) {
                        current = self.merge_regions(&current, &detections[j].0);
                        max_score = max_score.max(detections[j].1);
                        used[j] = true;
                        found = true;
                    }
                }
                
                if !found {
                    break;
                }
            }
            
            if current.width * current.height >= self.config.min_region_size {
                merged.push((current, max_score));
            }
        }
        
        merged
    }
    
    fn merge_regions(&self, a: &SRegion, b: &SRegion) -> SRegion {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        let x2 = (a.x + a.width).max(b.x + b.width);
        let y2 = (a.y + a.height).max(b.y + b.height);
        
        SRegion { 
            x, 
            y, 
            width: x2 - x, 
            height: y2 - y 
        }
    }
    
    fn create_visualization(&self, original: &RgbImage, detections: &[(SRegion, f64)]) -> RgbImage {
        let mut vis = original.clone();
        
        for (region, score) in detections {
            let intensity = (*score + 255.0) as u8;
            let color = Rgb([intensity, (255 - intensity / 2), 0]);
            
            self.draw_rectangle(&mut vis, region, color, 2);
        }
        
        vis
    }
    
    fn draw_rectangle(
        &self,
        image: &mut RgbImage,
        region: &SRegion,
        color: Rgb<u8>,
        thickness: u32
    ) {
        let (width, height) = image.dimensions();
        
        for t in 0..thickness {
            for x in region.x.saturating_sub(t)..(region.x + region.width + t).min(width) {
                if region.y >= t && region.y - t < height {
                    image.put_pixel(x, region.y.saturating_sub(t), color);
                }
            }
            
            for x in region.x.saturating_sub(t)..(region.x + region.width + t).min(width) {
                let y = region.y + region.height + t;
                if y < height {
                    image.put_pixel(x, y, color);
                }
            }
            
            for y in region.y.saturating_sub(t)..(region.y + region.height + t).min(height) {
                if region.x >= t {
                    image.put_pixel(region.x.saturating_sub(t), y, color);
                }
            }
            
            for y in region.y.saturating_sub(t)..(region.y + region.height + t).min(height) {
                let x = region.x + region.width + t;
                if x < width {
                    image.put_pixel(x, y, color);
                }
            }
        }
    }
}

impl Detector for SplicingDetector {
    fn detect(&self, image: &image::DynamicImage) -> Result<DetectionResult> {
        let rgb = image.to_rgb8();
        let mut result = DetectionResult::new(&rgb);
        
        let (_, color_regions) = self.analyze_color_consistency(&rgb);
        let (_, edge_regions) = self.analyze_color_consistency(&rgb);
        
        let noise_analyzer = NoiseAnalyzer::new();
        let noise_result = noise_analyzer.analyze(image)?;
        let noise_regions = noise_result.anomalous_regions;
        
        let ela_analyzer = ElaAnalyzer::new(self.config.ela_quality);
        let ela_result = ela_analyzer.analyze(image)?;
        let ela_regions = ela_result.suspicious_regions;
        
        let combined = self.combine_analyses(
            &color_regions, 
            &edge_regions, 
            &noise_regions, 
            &ela_regions
        );
        
        for (region, score) in &combined {
            let mut evidence = Vec::new();
            
            if color_regions.iter().any(|r| self.regions_overlap(r, region)) {
                evidence.push("Color histogram incosistency".into());
            }
            
            if edge_regions.iter().any(|r| self.regions_overlap(r, region)) {
                evidence.push("Unnatrural edge patterns".into());
            }
            
            if noise_regions.iter().any(|r| self.regions_overlap(r, region)) {
                evidence.push("Noide pattern mismatch".into());
            }
            
            if ela_regions.iter().any(|r| self.regions_overlap(r, region)) {
                evidence.push("ELA inconsistency".into());
            }
            
            result.add_manipulation(DetectedManipulation {
                manipulation_type: super::ManipulationType::Splicing,
                region: *region,
                confidence: *score,
                confidence_level: ConfidenceLevel::from_score(*score),
                description: format!(
                    "Potential spliced region at ({}, {}) with {}x{} size",
                    region.x, region.y, region.width, region.height
                ),
                evidence
            });
        }
        
        result.visualization = self.create_visualization(&rgb, &combined);
        
        Ok(result)
    }
    
    fn name(&self) -> &str {
        "Splicing Detector"
    }
    
    fn description(&self) -> &str {
        "Detects regions that appear to be spliced from another image using color histogram analysis, edge detection, noise analysis, and ELA"
    }
}

impl Default for SplicingDetector {
    fn default() -> Self {
        Self::new()
    }
}