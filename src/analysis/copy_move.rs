use std::collections::HashMap;

use image::{DynamicImage, GrayImage, Rgb, RgbImage};
use num_complex::Complex;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use rustfft::FftPlanner;

use crate::{CopyMoveResult, MatchPair, SRegion, error::{ForensicsError, Result}, image_utils::{block_variance, extract_block, rgb_to_gray}};

pub struct CopyMoveDetector {
    block_size: u32,
    similarity_threshold: f64,
    min_distance: u32,
    variance_threshold: f64,
}

#[derive(Clone)]
struct BlockFeature {
    x: u32,
    y: u32,
    dct_coeffs: Vec<f64>,
    hash: u64,
}

impl CopyMoveDetector {
    pub fn new(block_size: u32, similarity_threshold: f64, min_distance: u32) -> Result<Self> {
        if block_size < 4 || block_size > 64 {
            return Err(ForensicsError::InvalidParameter(
                "Block size must be between 4 and 64".into()
            ));
        }
        
        Ok(Self {
            block_size,
            similarity_threshold,
            min_distance,
            variance_threshold: 100.0
        })
    }
    
    pub fn detect(&self, image: &DynamicImage) -> Result<CopyMoveResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);
        let (width, height) = gray.dimensions();
        
        if width < self.block_size * 2 || height < self.block_size * 2 {
            return Err(ForensicsError::ImageTooSmall(self.block_size * 2));
        }
        
        let features = self.extract_features(&gray)?;
        
        let matches = self.find_matches(&features)?;
        
        let visualization = self.create_visualization(&rgb, &matches);
        
        let confidence = if matches.is_empty() {
            0.0
        } else {
            matches.iter().map(|m| m.similarity).sum::<f64>() / matches.len() as f64 
        };
        
        Ok(CopyMoveResult { 
            matches, 
            visualization, 
            confidence 
        })
    }
    
    fn extract_features(&self, gray: &GrayImage) -> Result<Vec<BlockFeature>> {
        let (width, height) = gray.dimensions();
        let step = (self.block_size / 2).max(1);
        
        let mut positions = Vec::new();
        
        for y in (0..height - self.block_size).step_by(step as usize) {
            for x in (0..width - self.block_size).step_by(step as usize) {
                positions.push((x, y));
            }
        }
        
        let features = positions
            .par_iter()
            .filter_map(|&(x, y)| self.extract_block_feature(gray, x, y))
            .collect();
        
        Ok(features)
    }
    
    fn extract_block_feature(&self, gray: &GrayImage, x: u32, y: u32) -> Option<BlockFeature> {
        let block = extract_block(gray, x, y, self.block_size);
        
        if block_variance(&block) < self.variance_threshold {
            return None;
        }
        
        let dct_coeffs = self.compute_dct(&block);
        
        let hash = self.compute_hash(&dct_coeffs);
        
        Some(BlockFeature { 
            x, 
            y, 
            dct_coeffs, 
            hash 
        })
    }
    
    fn compute_dct(&self, block: &[u8]) -> Vec<f64> {
        let n = self.block_size as usize;
        let mut input = block
            .iter()
            .map(|&v| Complex::new(v as f64, 0.0))
            .collect::<Vec<_>>();
        
        while input.len() < n * n {
            input.push(Complex::new(0.0, 0.0));
        }
        
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(n * n);
        fft.process(&mut input);
        
        input
            .iter()
            .take(16)
            .map(|c| c.norm())
            .collect()
    }
    
    fn compute_hash(&self, coeffs: &[f64]) -> u64 {
        let mean = coeffs.iter().sum::<f64>() / coeffs.len() as f64;
        let mut hash = 0u64;
        
        for (i, &c) in coeffs.iter().enumerate().take(64) {
            if c > mean {
                hash |= 1 << i;
            }
        }
        
        hash
    }
    
    fn find_matches(&self, features: &[BlockFeature]) -> Result<Vec<MatchPair>> {
        let mut matches = Vec::new();
        
        let mut hash_groups: HashMap<u64, Vec<usize>> = HashMap::new();
        
        for (i, feature) in features.iter().enumerate() {
            for offset in 0..4u64 {
                let h = feature.hash ^ offset;
                hash_groups.entry(h).or_default().push(i);
            }
        }
        
        for indices in hash_groups.values() {
            if indices.len() < 2 {
                continue;
            }
            
            for i in 0..indices.len() {
                for j in (i + 1)..indices.len() {
                    let f1 = &features[indices[i]];
                    let f2 = &features[indices[j]];
                    
                    let dx = (f1.x as i32 - f2.x as i32).abs() as u32;
                    let dy = (f1.y as i32 - f2.y as i32).abs() as u32;
                    let distance = ((dx * dx + dy * dy) as f64).sqrt() as u32;
                    
                    if distance < self.min_distance {
                        continue;
                    }
                    
                    let similarity = self.calculate_similarity(&f1.dct_coeffs, &f2.dct_coeffs);
                    
                    if similarity >= self.similarity_threshold {
                        matches.push(MatchPair {
                            source: SRegion { 
                                x: f1.x, 
                                y: f1.y, 
                                width: self.block_size, 
                                height: self.block_size    
                            },
                            target: SRegion { 
                                x: f2.x, 
                                y: f2.y, 
                                width: self.block_size, 
                                height: self.block_size 
                            },
                            similarity
                        });
                    }
                }
            }
        }
        
        self.filter_matches(matches)
    }
    
    fn calculate_similarity(&self, coeffs1: &[f64], coeffs2: &[f64]) -> f64 {
        if coeffs1.len() != coeffs2.len() || coeffs1.is_empty() {
            return 0.0;
        }
        
        let mean1 = coeffs1.iter().sum::<f64>() / coeffs1.len() as f64;
        let mean2 = coeffs2.iter().sum::<f64>() / coeffs2.len() as f64; 
        
        let mut numerator = 0.0;
        let mut denom1 = 0.0;
        let mut denom2 = 0.0;
        
        for (&c1, &c2) in coeffs1.iter().zip(coeffs2.iter()) {
            let d1 = c1 - mean1;
            let d2 = c2 - mean2;
            numerator += d1 * d2;
            denom1 += d1 * d1;
            denom2 += d2 * d2;
        }
        
        let denom = (denom1 * denom2).sqrt();
        if denom < 1e-10 {
            0.0
        } else {
            (numerator / denom).max(0.0)
        }
    }
    
    fn filter_matches(&self, mut matches: Vec<MatchPair>) -> Result<Vec<MatchPair>> {
        matches.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        
        let mut filtered = Vec::new();
        
        for m in matches {
            let overlaps = filtered.iter().any(|existing: &MatchPair| {
                self.regions_overlap(&m.source, &existing.source) ||
                self.regions_overlap(&m.target, &existing.source) || 
                self.regions_overlap(&m.source, &existing.target) || 
                self.regions_overlap(&m.target, &existing.target)
            });
            
            if !overlaps {
                filtered.push(m);
            }
        }
        
        Ok(filtered)
    }
    
    fn regions_overlap(&self, a: &SRegion, b: &SRegion) -> bool {
        let overlap_x = a.x < b.x + b.width && a.x + a.width > b.x;
        let overlap_y = a.y < b.y + b.height && a.y + a.height > b.y;
        
        overlap_x && overlap_y
    }
    
    fn create_visualization(&self, original: &RgbImage, matches: &[MatchPair]) -> RgbImage {
        let mut vis = original.clone();
        
        for (i, match_pair) in matches.iter().enumerate() {
            let color = Rgb([
                ((i * 50) % 255) as u8,
                ((i * 80 + 100) % 255) as u8,
                ((i * 120 + 50) % 255) as u8,
            ]);
            
            self.draw_rectangle(&mut vis, &match_pair.source, color);
            self.draw_rectangle(&mut vis, &match_pair.target, color);
            
            self.draw_line(
                &mut vis, 
                match_pair.source.x + match_pair.source.width / 2, 
                match_pair.source.y + match_pair.source.height / 2, 
                match_pair.target.x + match_pair.target.width / 2, 
                match_pair.target.y + match_pair.target.height / 2, 
                color
            );
        }
        
        vis
    }
    
    fn draw_rectangle(
        &self,
        image: &mut RgbImage,
        region: &SRegion,
        color: Rgb<u8>
    ) {
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
    
    fn draw_line(
        &self, 
        image: &mut RgbImage, 
        x0: u32, 
        y0: u32, 
        x1: u32, 
        y1: u32, 
        color: Rgb<u8>
    ) {
        let (width, height) = image.dimensions();
        
        let dx = (x1 as i32 - x0 as i32).abs();
        let dy = -(y1 as i32 - y0 as i32).abs();
        let sz = if x0 < x1 { 1i32 } else { -1i32 };
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
                x += dx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }
}