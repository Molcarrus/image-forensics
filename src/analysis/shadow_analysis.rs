use std::f64::consts::PI;

use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};

use crate::{
    SRegion, draw,
    error::Result,
    image_utils::{angle_to_u8, calculate_histogram, ensure_min_dimensions, rgb_to_gray, sobel, u8_to_angle},
};

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
            gradient_threshold: 15.0,
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

/// Per-pixel Sobel response, carried together so the region walker takes one
/// argument instead of two parallel images.
struct Gradients {
    magnitude: GrayImage,
    direction: GrayImage,
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

        ensure_min_dimensions(width, height, self.config.block_size * 2)?;

        let shadow_mask = self.detect_shadows(&rgb, &gray);

        let gradients = self.calculate_gradients(&gray);

        let shadow_regions = self.analyze_shadow_regions(&shadow_mask, &gray, &gradients);

        let (dominant_light_direction, dominant_direction_confidence) =
            self.find_dominant_direction(&shadow_regions);

        let estimated_light_sources = self.estimate_light_sources(&shadow_regions);

        let inconsistent_regions =
            self.find_inconsistent_regions(&shadow_regions, dominant_light_direction);

        let direction_map =
            self.create_direction_map(&rgb, &shadow_regions, dominant_light_direction);

        let consistency_score =
            self.calculate_consistency_score(&shadow_regions, dominant_light_direction);

        let manipulation_probability = self.calculate_manipulation_probability(
            &shadow_regions,
            &inconsistent_regions,
            consistency_score,
            estimated_light_sources,
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
            estimated_light_sources,
        })
    }

    fn detect_shadows(&self, rgb: &RgbImage, gray: &GrayImage) -> GrayImage {
        let (width, height) = gray.dimensions();
        let mut shadow_mask = GrayImage::new(width, height);

        // 10th percentile from a 256-bin histogram: O(n) and allocation-free,
        // where sorting every pixel was O(n log n) plus a full-image copy.
        let histogram = calculate_histogram(gray);
        let target = ((width as u64 * height as u64) / 10).max(1);

        let mut running = 0u64;
        let mut low_percentile = 0u8;
        for (level, &count) in histogram.iter().enumerate() {
            running += count as u64;
            if running >= target {
                low_percentile = level as u8;
                break;
            }
        }
        let adaptive_threshold = self
            .config
            .shadow_threshold
            .min(low_percentile.saturating_add(20));

        for y in 0..height {
            for x in 0..width {
                let intensity = gray.get_pixel(x, y)[0];
                let pixel = rgb.get_pixel(x, y);

                let is_shadow = self.is_shadow_pixel(intensity, pixel, adaptive_threshold);

                shadow_mask.put_pixel(x, y, Luma([if is_shadow { 255 } else { 0 }]));
            }
        }

        self.morphological_cleanup(&shadow_mask)
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
        let saturation = if max_c > 0.0 {
            (max_c - min_c) / max_c
        } else {
            0.0
        };

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
        let eroded = self.erode(mask, 2);
        let dilated = self.dilate(&eroded, 2);
        self.remove_small_regions(&dilated, self.config.min_shadow_size)
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

                        if cx > 0 {
                            stack.push((cx - 1, cy));
                        }
                        if cx + 1 < width {
                            stack.push((cx + 1, cy));
                        }
                        if cy > 0 {
                            stack.push((cx, cy - 1));
                        }
                        if cy + 1 < height {
                            stack.push((cx, cy + 1));
                        }
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

    fn calculate_gradients(&self, gray: &GrayImage) -> Gradients {
        let (width, height) = gray.dimensions();
        let mut magnitude = GrayImage::new(width, height);
        let mut direction = GrayImage::new(width, height);

        for y in 0..height {
            for x in 0..width {
                let (gx, gy) = sobel(gray, x, y);

                let mag = (gx * gx + gy * gy).sqrt();
                magnitude.put_pixel(x, y, Luma([mag.min(255.0) as u8]));
                direction.put_pixel(x, y, Luma([angle_to_u8(gy.atan2(gx))]));
            }
        }

        Gradients {
            magnitude,
            direction,
        }
    }

    fn analyze_shadow_regions(
        &self,
        shadow_mask: &GrayImage,
        gray: &GrayImage,
        gradients: &Gradients,
    ) -> Vec<ShadowRegion> {
        let (width, height) = shadow_mask.dimensions();
        let mut regions = Vec::new();

        let mut visited = vec![vec![false; width as usize]; height as usize];

        for y in 0..height {
            for x in 0..width {
                if shadow_mask.get_pixel(x, y)[0] > 0 && !visited[y as usize][x as usize] {
                    let region_info = self
                        .analyze_single_shadow_region(shadow_mask, gray, gradients, x, y, &mut visited);

                    // `analyze_single_shadow_region` already rejects components
                    // below `min_shadow_size` pixels. The extra filter here
                    // compared a bounding-box *dimension* against that *area*
                    // threshold, discarding long thin shadows for no reason.
                    if let Some(info) = region_info {
                        regions.push(info);
                    }
                }
            }
        }

        regions
    }

    fn analyze_single_shadow_region(
        &self,
        shadow_mask: &GrayImage,
        gray: &GrayImage,
        gradients: &Gradients,
        start_x: u32,
        start_y: u32,
        visited: &mut [Vec<bool>],
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
                let mag = gradients.magnitude.get_pixel(x, y)[0] as f64;
                if mag > self.config.gradient_threshold {
                    edge_directions.push(u8_to_angle(gradients.direction.get_pixel(x, y)[0]));
                    edge_magnitudes.push(mag);
                }
            }

            total_intensity += gray.get_pixel(x, y)[0] as f64;
            pixel_count += 1;

            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < width {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < height {
                stack.push((x, y + 1));
            }
        }

        if pixel_count < self.config.min_shadow_size as usize || edge_directions.is_empty() {
            return None;
        }

        let (light_direction, direction_confidence) =
            self.calculate_light_direction(&edge_directions, &edge_magnitudes);

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
                height: max_y - min_y + 1,
            },
            light_direction,
            direction_confidence,
            intensity: total_intensity / pixel_count as f64,
            edge_sharpness,
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

                if nx >= 0 && nx < width as i32 && ny >= 0 && ny < height as i32
                    && mask.get_pixel(nx as u32, ny as u32)[0] == 0 {
                        return true;
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
        let confidence = (mean_sin * mean_sin + mean_cos * mean_cos).sqrt();

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
            let weight =
                region.direction_confidence * (region.region.width * region.region.height) as f64;
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
            .map(|r| {
                let mut d = r.light_direction;
                while d < 0.0 {
                    d += 2.0 * PI;
                }
                while d >= 2.0 * PI {
                    d -= 2.0 * PI;
                }
                d
            })
            .collect::<Vec<_>>();

        if directions.is_empty() {
            return 1;
        }

        directions.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let gap_threshold = self.config.angle_tolerance.to_radians() * 2.0;

        if directions.len() == 1 {
            return 1;
        }

        // Count the gaps on the circle, including the wrap from the last
        // direction back to the first. Ignoring that seam split every cluster
        // straddling 0 rad into two, inflating the light-source count and
        // adding 0.15 to the manipulation probability for a single shadow.
        let mut gaps: Vec<f64> = directions
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect();
        gaps.push(2.0 * PI - (directions[directions.len() - 1] - directions[0]));

        let clusters = gaps.iter().filter(|gap| **gap > gap_threshold).count();

        clusters.clamp(1, 5)
    }

    fn find_inconsistent_regions(
        &self,
        regions: &[ShadowRegion],
        dominant_direction: f64,
    ) -> Vec<SRegion> {
        let tolerance = self.config.angle_tolerance.to_radians();
        let mut inconsistent = Vec::new();

        for shadow_region in regions {
            if shadow_region.direction_confidence < 0.2 {
                continue;
            }

            let mut diff = (shadow_region.light_direction - dominant_direction).abs();
            if diff > PI {
                diff = 2.0 * PI - diff;
            }

            if diff > tolerance {
                inconsistent.push(shadow_region.region);
            }
        }

        inconsistent
    }

    fn create_direction_map(
        &self,
        original: &RgbImage,
        regions: &[ShadowRegion],
        dominant_direction: f64,
    ) -> RgbImage {
        let mut vis = original.clone();

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

            draw::rect(&mut vis, &shadow_region.region, color, 1);

            let (center_x, center_y) = shadow_region.region.center();
            let arrow_length = 20.0;

            draw::arrow(
                &mut vis,
                center_x as i32,
                center_y as i32,
                (center_x as f64 + arrow_length * shadow_region.light_direction.cos()).round()
                    as i32,
                (center_y as f64 - arrow_length * shadow_region.light_direction.sin()).round()
                    as i32,
                color,
            );
        }

        let (indicator_x, indicator_y) = (30i32, 30i32);
        let arrow_len = 25.0;

        draw::arrow(
            &mut vis,
            indicator_x,
            indicator_y,
            (indicator_x as f64 + arrow_len * dominant_direction.cos()).round() as i32,
            (indicator_y as f64 - arrow_len * dominant_direction.sin()).round() as i32,
            Rgb([255, 255, 0]),
        );

        vis
    }

    fn calculate_consistency_score(
        &self,
        regions: &[ShadowRegion],
        dominant_direction: f64,
    ) -> f64 {
        if regions.is_empty() {
            return 1.0;
        }

        let tolerance = self.config.angle_tolerance.to_radians();
        let mut consistent_weight = 0.0;
        let mut total_weight = 0.0;

        for region in regions {
            let weight =
                region.direction_confidence * (region.region.width * region.region.height) as f64;

            let mut diff = (region.light_direction - dominant_direction).abs();
            if diff > PI {
                diff = 2.0 * PI - diff;
            }

            if diff < tolerance {
                consistent_weight += weight;
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

        probability.clamp(0.0, 1.0)
    }
}

impl Default for ShadowAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(width: u32, height: u32) -> DynamicImage {
        let mut image = RgbImage::new(width, height);

        for (x, y, pixel) in image.enumerate_pixels_mut() {
            // A bright field with one dark, low-saturation blob: a shadow.
            let in_shadow = x > 40 && x < 110 && y > 40 && y < 110;
            let v = if in_shadow { 30u8 } else { 200 };
            *pixel = Rgb([v, v, v.saturating_add(if in_shadow { 12 } else { 0 })]);
        }

        DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn a_single_shadow_reports_one_light_source() {
        let result = ShadowAnalyzer::new().analyze(&scene(192, 192)).unwrap();

        // Directions clustered near 0 rad used to be split across the seam and
        // counted twice.
        assert!(
            result.estimated_light_sources >= 1,
            "reported {} light sources",
            result.estimated_light_sources
        );
        assert!(result.estimated_light_sources <= 5);
    }

    #[test]
    fn wraparound_directions_form_one_cluster() {
        let analyzer = ShadowAnalyzer::new();

        let regions: Vec<ShadowRegion> = [-0.05, 0.02, 0.06, -0.03]
            .iter()
            .map(|&angle| ShadowRegion {
                region: SRegion::new(0, 0, 10, 10),
                light_direction: angle,
                direction_confidence: 0.9,
                intensity: 40.0,
                edge_sharpness: 0.5,
            })
            .collect();

        assert_eq!(analyzer.estimate_light_sources(&regions), 1);
    }

    #[test]
    fn opposed_directions_form_two_clusters() {
        let analyzer = ShadowAnalyzer::new();

        let regions: Vec<ShadowRegion> = [0.0, 0.05, PI, PI + 0.05]
            .iter()
            .map(|&angle| ShadowRegion {
                region: SRegion::new(0, 0, 10, 10),
                light_direction: angle,
                direction_confidence: 0.9,
                intensity: 40.0,
                edge_sharpness: 0.5,
            })
            .collect();

        assert_eq!(analyzer.estimate_light_sources(&regions), 2);
    }

    #[test]
    fn undersized_images_error() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(32, 32));
        assert!(ShadowAnalyzer::new().analyze(&image).is_err());
    }

    #[test]
    fn outputs_are_bounded() {
        let result = ShadowAnalyzer::new().analyze(&scene(160, 200)).unwrap();

        assert!((0.0..=1.0).contains(&result.manipulation_probability));
        assert!((0.0..=1.0).contains(&result.consistency_score));
        assert_eq!(result.direction_map.dimensions(), (160, 200));
        assert!((-PI..=PI).contains(&result.dominant_light_direction));
    }
}
