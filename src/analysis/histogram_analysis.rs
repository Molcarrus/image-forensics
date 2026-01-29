use image::{DynamicImage, GrayImage, Luma, RgbImage};

use crate::{error::Result, image_utils::rgb_to_gray};

#[derive(Debug, Clone)]
pub struct HistogramConfig {
    pub block_size: u32,
    pub gap_threshold: u32,
    pub peak_threshold: f64,
    pub clipping_threshold: f64,
}

impl Default for HistogramConfig {
    fn default() -> Self {
        Self { 
            block_size: 64, 
            gap_threshold: 0, 
            peak_threshold: 0.1, 
            clipping_threshold: 0.01 
        }
    }
}

#[derive(Debug, Clone)]
pub enum HistogramAnomaly {
    Gap { count: usize, positions: Vec<u8> },
    CombPattern { period: f64, strength: f64 },
    ShadowClipping { percentage: f64 },
    HighlightClipping { percentage: f64 },
    UnusualPeak { position: u8, height: f64 },
    TruncatedRange { min: u8, max: u8 },
}

#[derive(Debug, Clone)]
pub struct HistogramAnalysisResult {
    pub luminance_histogram: [u32; 256],
    pub red_histogram: [u32; 256],
    pub green_histogram: [u32; 256],
    pub anomalies: Vec<HistogramAnomaly>,
    pub gaps_map: GrayImage,
    pub manipulation_probability: f64,
    pub estimated_gamma: Option<f64>,
    pub contrast_stretched: bool,
    pub levels_adjusted: bool,
}

pub struct HistogramAnalyzer {
    config: HistogramConfig,
}

impl HistogramAnalyzer {
    pub fn new() -> Self {
        Self::with_config(HistogramConfig::default())
    }
    
    pub fn with_config(config: HistogramConfig) -> Self {
        Self { config }
    }
    
    pub fn analyze(&self, image: &DynamicImage) -> Result<HistogramAnalysisResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);
        let (width, height) = gray.dimensions();
        
        let luminance_histogram = self.compute_histogram(&gray);
        let (red_histogram, green_histogram, blue_histogram) = self.compute_rgb_histograms(&rgb);
        
        let mut anomalies = Vec::new();
        
        let gaps = self.detect_gaps(&luminance_histogram);
        if !gaps.is_empty() {
            anomalies.push(HistogramAnomaly::Gap { 
                count: gaps.len(), 
                positions: gaps 
            });
        }
        
        if let Some((period, strength)) = self.detect_comb_pattern(&luminance_histogram) {
            anomalies.push(HistogramAnomaly::CombPattern { period, strength });
        }
        
        let total_pixels = (width * height) as f64;
        let shadow_clip = luminance_histogram[0] as f64 / total_pixels;
        let highlight_clip = luminance_histogram[255] as f64 / total_pixels;
        
        if shadow_clip > self.config.clipping_threshold {
            anomalies.push(HistogramAnomaly::ShadowClipping { percentage: shadow_clip });
        }
        
        if highlight_clip > self.config.clipping_threshold {
            anomalies.push(HistogramAnomaly::HighlightClipping { percentage: highlight_clip });
        }
        
        let peaks = self.detect_unusual_peaks(&luminance_histogram, total_pixels);
        anomalies.extend(peaks);
        
        if let Some((min, max)) = self.detect_truncated_range(&luminance_histogram) {
            anomalies.push(HistogramAnomaly::TruncatedRange { min, max });
        }
        
        let gaps_map = self.create_gaps_map(&gray);
        let estimated_gamma = self.estimate_gamma(&luminance_histogram);
        let contrast_stretched = self.detect_contrast_stretch(&luminance_histogram);
        let levels_adjusted = !self.detect_gaps(&luminance_histogram).is_empty();
        let manipulation_probability = self.calculate_manipulation_probability(&anomalies);
        
        Ok(HistogramAnalysisResult { 
            luminance_histogram, 
            red_histogram, 
            green_histogram, 
            anomalies, 
            gaps_map, 
            manipulation_probability, 
            estimated_gamma, 
            contrast_stretched, 
            levels_adjusted 
        })
    }
    
    fn compute_histogram(&self, gray: &GrayImage) -> [u32; 256] {
        let mut histogram = [0u32; 256];
        
        for pixel in gray.pixels() {
            histogram[pixel[0] as usize] += 1;
        }
        
        histogram
    }
    
    fn compute_rgb_histograms(&self, rgb: &RgbImage) -> ([u32; 256], [u32; 256], [u32; 256]) {
        let mut red = [0u32; 256];
        let mut green = [0u32; 256];
        let mut blue = [0u32; 256];
        
        for pixel in rgb.pixels() {
            red[pixel[0] as usize] += 1;
            green[pixel[0] as usize] += 1;
            blue[pixel[0] as usize] += 1;
        }
        
        (red, green, blue)
    }
    
    fn detect_gaps(&self, histogram: &[u32; 256]) -> Vec<u8> {
        let mut gaps = Vec::new();
        
        let first_nonzero = histogram.iter().position(|&x| x > 0).unwrap_or(0);
        let last_nonzero = histogram.iter().rposition(|&x| x > 0).unwrap_or(255);
        
        for i in first_nonzero..=last_nonzero {
            if histogram[i] <= self.config.gap_threshold {
                gaps.push(i as u8);
            }
        }
        
        gaps
    }
    
    fn detect_comb_pattern(&self, histogram: &[u32; 256]) -> Option<(f64, f64)> {
        let mut alterations = 0;
        let mut total_checks = 0;
        
        let mean = histogram.iter().sum::<u32>() as f64 / 256.0;
        
        for i in 1..255 {
            let prev = histogram[i-1] as f64;
            let curr = histogram[i] as f64;
            let next = histogram[i+1] as f64;
            
            if (curr > prev && curr > next) || (curr < prev && curr < next) {
                if curr.abs() > mean * 0.1 {
                    alterations += 1;
                }
            }
            total_checks += 1;
        }
        
        let alteration_rate = alterations as f64 / total_checks as f64;
        
        if alteration_rate > 0.3 {
            Some((2.0, alteration_rate))
        } else {
            None 
        }
    }
    
    fn detect_unusual_peaks(&self, histogram: &[u32; 256], total: f64) -> Vec<HistogramAnomaly> {
        let mut peaks = Vec::new();
        
        let mean = total / 256.0;
        
        for i in 1..255 {
            let curr = histogram[i] as f64;
            let prev = histogram[i-1] as f64;
            let next = histogram[i+1] as f64;
            
            if curr > prev * 3.0 && curr > next * 3.0 && curr > mean * 5.0 {
                peaks.push(HistogramAnomaly::UnusualPeak { 
                    position: i as u8, 
                    height: curr / total 
                });
            }
        }
        
        peaks
    }
    
    fn detect_truncated_range(&self, histogram: &[u32; 256]) -> Option<(u8, u8)> {
        let first_nonzero = histogram.iter().position(|&x| x > 0)?;
        let last_nonzero = histogram.iter().rposition(|&x| x > 0)?;
        
        if first_nonzero > 20 || last_nonzero < 235 {
            Some((first_nonzero as u8, last_nonzero as u8))
        } else {
            None 
        }
    } 
    
    fn create_gaps_map(&self, gray: &GrayImage) -> GrayImage {
        let (width, height) = gray.dimensions();
        let block_size = self.config.block_size;
        let mut gaps_map = GrayImage::new(width, height);
        
        for by in (0..height-block_size).step_by(block_size as usize / 2) {
            for bx in (0..width-block_size).step_by(block_size as usize / 2) {
                let mut local_hist = [0u32; 256];
                
                for y in by..(by + block_size).min(height) {
                    for x in bx..(bx + block_size).min(width) {
                        local_hist[gray.get_pixel(x, y)[0] as usize] += 1;
                    }
                }
                
                let gaps = self.detect_gaps(&local_hist);
                let gap_ratio = gaps.len() as f64 / 256.0;
                let value = (gap_ratio * 255.0 * 4.0).min(255.0) as u8;
                
                for y in by..(by + block_size).min(height) {
                    for x in bx..(bx + block_size).min(width) {
                        gaps_map.put_pixel(x, y, Luma([value]));
                    }
                }
            }
        }
        
        gaps_map
    }
    
    fn estimate_gamma(&self, histogram: &[u32; 256]) -> Option<f64> {
        let total = histogram.iter().map(|&x| x as u64).sum::<u64>();
        if total == 0 {
            return None;
        }
        
        let mut sum = 0;
        for (i, &count) in histogram.iter().enumerate() {
            sum += i as u64 * count as u64;
        }
        let mean = sum as f64 / total as f64 / 255.0;
        
        if mean > 0.01 && mean < 0.99 {
            let gamma = 0.5_f64.ln() / mean.ln();
            if gamma > 0.2 && gamma < 5.0 {
                return Some(gamma);
            }
        }
        
        None 
    }
    
    fn detect_contrast_stretch(&self, histogram: &[u32; 256]) -> bool {
        let gaps = self.detect_gaps(histogram);
        
        if gaps.len() > 10 {
            let mut diffs = Vec::new();
            for i in 1..gaps.len() {
                diffs.push(gaps[i] - gaps[i-1]);
            }
            
            if !diffs.is_empty() {
                let mean_diff = diffs
                    .iter()
                    .map(|&d| d as f64)
                    .sum::<f64>() / diffs.len() as f64;
                let variance = diffs
                    .iter()
                    .map(|&d| (d as f64 - mean_diff).powi(2))
                    .sum::<f64>() / diffs.len() as f64;
                
                if variance < 1.0 {
                    return true;
                }
            }
        }
        
        false 
    }
    
    fn calculate_manipulation_probability(&self, anomalies: &[HistogramAnomaly]) -> f64 {
        if anomalies.is_empty() {
            return 0.0;
        }
        
        let mut probability = 0.0;
        
        for anomaly in anomalies {
            match anomaly {
                HistogramAnomaly::Gap { count, .. } => {
                    probability += (*count as f64 / 50.0).min(0.3);
                }
                HistogramAnomaly::CombPattern { strength, .. } => {
                    probability += strength * 0.4;
                }
                HistogramAnomaly::ShadowClipping { percentage } => {
                    probability += (percentage * 10.0).min(0.2);
                }
                HistogramAnomaly::HighlightClipping { percentage } => {
                    probability += (percentage * 10.0).min(0.2);
                }
                HistogramAnomaly::UnusualPeak { height, .. } => {
                    probability += (height * 5.0).min(0.2);
                }
                HistogramAnomaly::TruncatedRange { min, max } => {
                    let range = *max as i32 - *min as i32;
                    probability += ((255 - range) as f64 / 255.0 * 0.3);
                }
            }
        }
        
        probability.min(1.0)
    }
}

impl Default for HistogramAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}