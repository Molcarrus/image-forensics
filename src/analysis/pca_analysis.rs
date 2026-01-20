use image::{DynamicImage, GrayImage, Luma};

use crate::{SRegion, error::Result, image_utils::rgb_to_gray};

#[derive(Debug, Clone)]
pub struct PcaConfig {
    pub block_size: u32,
    pub num_components: usize,
    pub patch_size: u32,
    pub patch_stride: u32,
    pub anomaly_threshold: f64,
    pub min_variance_ratio: f64,
}

impl Default for PcaConfig {
    fn default() -> Self {
        Self { 
            block_size: 64, 
            num_components: 3, 
            patch_size: 8, 
            patch_stride: 4, 
            anomaly_threshold: 2.5, 
            min_variance_ratio: 0.01 
        }
    }
}

#[derive(Debug, Clone)]
pub struct PcaAnalysisResult {
    pub anomaly_map: GrayImage,
    pub pc1_map: GrayImage,
    pub pc2_map: GrayImage,
    pub pc3_map: GrayImage,
    pub anomalous_regions: Vec<SRegion>,
    pub variance_ratios: Vec<f64>,
    pub overall_anomaly_score: f64,
    pub manipulation_probability: f64,
}

pub struct PcaAnalyzer {
    config: PcaConfig,
}

impl PcaAnalyzer {
    pub fn new() -> Self {
        Self::with_config(PcaConfig::default())
    }
    
    pub fn with_config(config: PcaConfig) -> Self {
        Self { config }
    }
    
    pub fn analyze(&self, image: &DynamicImage) -> Result<PcaAnalysisResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);
        let (width, height) = gray.dimensions();
        
        if width < self.config.block_size * 2 || height < self.config.block_size * 2 {
            return Err(crate::error::ForensicsError::ImageTooSmall(self.config.block_size * 2));
        }
        
        let (patches, patch_positions) = self.extract_patches(&gray);
        
        if patches.is_empty() {
            return Err(crate::error::ForensicsError::AnalysisFailed(
                "No patches could be extracted".into()
            ));
        }
        
        let (prinicpal_components, eigenvalues, mean) = self.compute_pca(&patches)?;
        
        let total_variance = eigenvalues.iter().sum::<f64>();
        let variance_ratios = eigenvalues
            .iter()
            .map(|&ev| if total_variance > 0.0 { ev / total_variance } else { 0.0 })
            .collect::<Vec<_>>();
        
        let projections = self.project_patches(&patches, &prinicpal_components, &mean);
        
        let pc1_map = self.create_component_map(
            width, height, &patch_positions, &projections, 0
        );
        let pc2_map = self.create_component_map(
            width, height, &patch_positions, &projections, 1
        );
        let pc3_map = self.create_component_map(
            width, height, &patch_positions, &projections, 2
        );
        
        let reconstruction_errors = self.compute_reconstruction_errors(
            &patches, &prinicpal_components, &mean, &projections
        );
        
        let anomaly_map = self.create_anomaly_map(
            width, height, &patch_positions, &reconstruction_errors
        );
        
        let anomalous_regions = self.find_anomalous_regions(
            &anomaly_map, &reconstruction_errors, &patch_positions
        );
        
        let overall_anomaly_score = self.calculate_overall_anomaly_score(&reconstruction_errors);
        let manipulation_probability = self.calculate_manipulation_probability(&anomalous_regions, overall_anomaly_score, width, height);
        
        Ok(PcaAnalysisResult { 
            anomaly_map, 
            pc1_map, 
            pc2_map, 
            pc3_map, 
            anomalous_regions, 
            variance_ratios, 
            overall_anomaly_score, 
            manipulation_probability 
        })
    }
    
    fn extract_patches(&self, gray: &GrayImage) -> (Vec<Vec<f64>>, Vec<(u32, u32)>) {
        let (width, height) = gray.dimensions();
        let patch_size = self.config.patch_size;
        let stride = self.config.patch_stride;
        
        let mut patches = Vec::new();
        let mut positions = Vec::new();
        
        for y in (0..height - patch_size).step_by(stride as usize) {
            for x in (0..width - patch_size).step_by(stride as usize) {
                let patch = self.extract_single_patch(gray, x, y);
                patches.push(patch);
                positions.push((x, y));
            }
        }
        
        (patches, positions)
    }
    
    fn extract_single_patch(&self, gray: &GrayImage, x: u32, y: u32) -> Vec<f64> {
        let patch_size = self.config.patch_size;
        let mut patch = Vec::with_capacity((patch_size * patch_size) as usize);
        
        for dy in 0..patch_size {
            for dx in 0..patch_size {
                let pixel = gray.get_pixel(x + dx, y + dy)[0] as f64;
                patch.push(pixel);
            }
        }
        
        patch
    }
    
    fn compute_pca(&self, patches: &[Vec<f64>]) -> Result<(Vec<Vec<f64>>, Vec<f64>, Vec<f64>)> {
        let n_samples = patches.len();
        let n_features = patches[0].len();
        
        if n_samples < self.config.num_components {
            return Err(crate::error::ForensicsError::AnalysisFailed(
                "Not enough samples for PCA".into()
            ));
        }
        
        let mut mean = vec![0.0; n_features];
        for patch in patches {
            for (i, &val) in patch.iter().enumerate() {
                mean[i] += val;
            }
        }
        for m in &mut mean {
            *m /= n_samples as f64;
        }
        
        let centered = patches
            .iter()
            .map(|patch| {
                patch
                    .iter()
                    .zip(mean.iter())
                    .map(|(&p, &m)| p - m)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        
        let max_samples = 5000.min(n_samples);
        let step = n_samples / max_samples;
        
        let mut covariance = vec![vec![0.0; n_features]; n_features];
        let mut sample_count = 0;
        
        for (idx, patch) in centered.iter().enumerate() {
            if idx % step != 0 {
                continue;
            }
            sample_count += 1;
            
            for i in 0..n_features {
                for j in i..n_features {
                    let val = patch[i] * patch[j];
                    covariance[i][j] += val;
                    if i != j {
                        covariance[j][i] += val;
                    }
                }
            }
        }
        
        for i in 0..n_features {
            for j in 0..n_features {
                covariance[i][j] /= sample_count as f64;
            }
        }
        
        let (eigenvectors, eigenvalues) = self.power_iteration(
            &covariance, 
            self.config.num_components.min(n_features)
        );
        
        Ok((eigenvectors, eigenvalues, mean))
    }
    
    fn power_iteration(
        &self,
        matrix: &[Vec<f64>],
        num_components: usize,
    ) -> (Vec<Vec<f64>>, Vec<f64>) {
        let n = matrix.len();
        let mut eigenvectors = Vec::new();
        let mut eigenvalues = Vec::new();
        let mut deflated_matrix = matrix.to_vec();
        
        for _ in 0..num_components {
            let mut v = (0..n).map(|i| (i as f64 * 0.1).sin()).collect::<Vec<_>>();
            let mut eigenvalue = 0.0;
            
            for _ in 0..100 {
                let mut new_v = vec![0.0; n];
                for i in 0..n {
                    for j in 0..n {
                        new_v[i] += deflated_matrix[i][j] * v[j];
                    }
                }
                
                eigenvalue = 0.0;
                for i in 0..n {
                    eigenvalue += new_v[i] * v[i];
                }
                
                let norm = new_v.iter().map(|x| x * x).sum::<f64>().sqrt();
                if norm > 1e-10 {
                    for x in &mut new_v {
                        *x /= norm;
                    }
                }
                
                let diff = v
                    .iter()
                    .zip(new_v.iter())
                    .map(|(a, b)| (a-b).abs())
                    .sum::<f64>();
                
                v = new_v;
                
                if diff < 1e-8 {
                    break;
                }
            }
            
            for i in 0..n {
                for j in 0..n {
                    deflated_matrix[i][j] -= eigenvalue * v[i] * v[j];
                }
            }
            
            eigenvectors.push(v);
            eigenvalues.push(eigenvalue.max(0.0));
        }
        
        (eigenvectors, eigenvalues)
    }
    
    fn project_patches(
        &self,
        patches: &[Vec<f64>],
        components: &[Vec<f64>],
        mean: &[f64]
    ) -> Vec<Vec<f64>> {
        patches
            .iter()
            .map(|patch| {
                let centered = patch
                    .iter()
                    .zip(mean.iter())
                    .map(|(&p, &m)| p - m)
                    .collect::<Vec<_>>();
                
                components
                    .iter()
                    .map(|component| {
                        centered
                            .iter()
                            .zip(component.iter())
                            .map(|(&c, &v)| c * v)
                            .sum()
                    })
                    .collect()
            })
            .collect()
    }
    
    fn compute_reconstruction_errors(
        &self,
        patches: &[Vec<f64>],
        components: &[Vec<f64>],
        mean: &[f64],
        projections: &[Vec<f64>]
    ) -> Vec<f64> {
        patches
            .iter()
            .zip(projections.iter())
            .map(|(patch, proj)| {
                let mut reconstructed = mean.to_vec();
                for (i, component) in components.iter().enumerate() {
                    if i < proj.len() {
                        for (j, &c) in component.iter().enumerate() {
                            reconstructed[j] += proj[i] * c;
                        }
                    }
                }
                
                let error = patch
                    .iter()
                    .zip(reconstructed.iter())
                    .map(|(&p, &r)| (p - r).powi(2))
                    .sum::<f64>();
                
                error.sqrt() / patch.len() as f64 
            })
            .collect()
    }
    
    fn create_component_map(
        &self,
        width: u32,
        height: u32,
        positions: &[(u32, u32)],
        projections: &[Vec<f64>],
        component_idx: usize,
    ) -> GrayImage{
        let mut map = GrayImage::new(width, height);
        let mut value_map = vec![vec![Vec::new(); width as usize]; height as usize];
        let patch_size = self.config.patch_size;
        
        for (i, &(x, y)) in positions.iter().enumerate() {
            if component_idx < projections[i].len() {
                let value = projections[i][component_idx];
                
                for dy in 0..patch_size {
                    for dx in 0..patch_size {
                        let px = (x + dx) as usize;
                        let py = (y + dy) as usize;
                        if px < width as usize && py < height as usize {
                            value_map[py][px].push(value);
                        }
                    }
                }
            }
        }
        
        let mut all_values = Vec::new();
        for row in &value_map {
            for cell in row {
                if !cell.is_empty() {
                    let avg = cell.iter().sum::<f64>() / cell.len() as f64;
                    all_values.push(avg);
                }
            }
        }
        
        if all_values.is_empty() {
            return map;
        }
        
        all_values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let min_val = all_values[all_values.len() / 20];
        let max_val = all_values[all_values.len() * 19 / 20];
        let range = (max_val - min_val).max(1e-10);
        
        for y in 0..height {
            for x in 0..width {
                let cell = &value_map[y as usize][x as usize];
                if !cell.is_empty() {
                    let avg = cell.iter().sum::<f64>() / cell.len() as f64;
                    let normalized = ((avg - min_val) / range).clamp(0.0, 1.0);
                    map.put_pixel(x, y, Luma([(normalized * 255.0) as u8]));
                }
            }
        }
        
        map 
    }
    
    fn create_anomaly_map(
        &self,
        width: u32,
        height: u32,
        positions: &[(u32, u32)],
        errors: &[f64],
    ) -> GrayImage {
        let mut map = GrayImage::new(width, height);
        let mut error_map = vec![vec![Vec::new(); width as usize]; height as usize];
        let patch_size = self.config.patch_size;
        
        for (i, &(x, y)) in positions.iter().enumerate() {
            for dy in 0..patch_size {
                for dx in 0..patch_size {
                    let px = (x + dx) as usize;
                    let py = (y + dy) as usize;
                    if px < width as usize && py < height as usize {
                        error_map[py][px].push(errors[i]);
                    }
                }
            }
        }
        
        let mean_error = errors.iter().sum::<f64>() / errors.len() as f64;
        let variance = errors
            .iter()
            .map(|e| (e - mean_error).powi(2))
            .sum::<f64>() / errors.len() as f64;
        let std_dev = variance.sqrt();
        
        for y in 0..height {
            for x in 0..width {
                let cell = &error_map[y as usize][x as usize];
                if !cell.is_empty() {
                    let avg = cell.iter().sum::<f64>() / cell.len() as f64;
                    
                    let z_score = if std_dev > 0.0 {
                        (avg - mean_error) / std_dev
                    } else {
                        0.0
                    };
                    
                    let normalized = (z_score / 5.0 + 0.5).clamp(0.0, 1.0);
                    map.put_pixel(x, y, Luma([(normalized * 255.0) as u8]));
                }
            }
        }
        
        map 
    }
    
    fn find_anomalous_regions(
        &self,
        anomaly_map: &GrayImage,
        errors: &[f64],
        postions: &[(u32, u32)]
    ) -> Vec<SRegion> {
        let (width, height) = anomaly_map.dimensions();
        let block_size = self.config.block_size;
        
        let mean_error = errors.iter().sum::<f64>() / errors.len() as f64;
        let variance = errors
            .iter()
            .map(|e| (e - mean_error).powi(2))
            .sum::<f64>() / errors.len() as f64;
        let std_dev = variance.sqrt();
        let threshold = mean_error + self.config.anomaly_threshold * std_dev;
        
        let mut regions = Vec::new();
        
        for by in (0..height).step_by(block_size as usize) {
            for bx in (0..width).step_by(block_size as usize) {
                let block_w = block_size.min(width - bx);
                let block_h = block_size.min(height - by);
                
                let mut block_sum = 0.0;
                let mut count = 0;
                
                for y in by..(by + block_h) {
                    for x in bx..(bx + block_w) {
                        block_sum += anomaly_map.get_pixel(x, y)[0] as f64;
                        count += 1;
                    }
                }
                
                let block_avg = block_sum / count as f64;
                
                if block_avg > 128.0 + self.config.anomaly_threshold * 30.0 {
                    regions.push(SRegion {
                        x: bx,
                        y: by,
                        width: block_w,
                        height: block_h,
                    });
                }
            }
        }
        
        self.merge_regions(regions)
    }
    
    fn merge_regions(&self, regions: Vec<SRegion>) -> Vec<SRegion> {
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
        let gap = self.config.block_size / 2;
        
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
            height: y2 - y 
        }
    }
    
    fn calculate_overall_anomaly_score(&self, errors: &[f64]) -> f64 {
        if errors.is_empty() {
            return 0.0;
        }
        
        let mean = errors.iter().sum::<f64>() / errors.len() as f64;
        let variance = errors
            .iter()
            .map(|e| (e - mean).powi(2))
            .sum::<f64>() / errors.len() as f64;
        let std_dev = variance.sqrt();
        
        let threshold = mean + self.config.anomaly_threshold * std_dev;
        let anomaly_count = errors.iter().filter(|&&e| e > threshold).count();
        let anomaly_ratio = anomaly_count as f64 / errors.len() as f64;
        
        let spread_score = (std_dev / mean.max(1.0)).min(1.0);
        
        (anomaly_ratio * 0.6 + spread_score * 0.4).min(1.0)
    }
    
    fn calculate_manipulation_probability(
        &self,
        regions: &[SRegion],
        anomaly_score: f64,
        width: u32,
        height: u32,
    ) -> f64 {
        let total_pixels = width * height;
        
        let anomalous_pixels = regions
            .iter()
            .map(|r| r.width * r.height)
            .sum::<u32>();
        
        let coverage = anomalous_pixels as f64 / total_pixels as f64;
        
        let manipulation_prob = if coverage > 0.5 {
            anomaly_score * 0.3 
        } else if coverage > 0.01 {
            anomaly_score * 0.8 + coverage * 0.2
        } else {
            anomaly_score * 0.5 
        };
        
        manipulation_prob.min(1.0)
    }
}

impl Default for PcaAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}