use image::{GrayImage, Luma, Rgb, RgbImage};

use crate::{
    SRegion,
    analysis::{ela::ElaAnalyzer, noise::NoiseAnalyzer},
    detection::{ConfidenceLevel, DetectedManipulation, DetectionResult, Detector},
    draw,
    error::Result,
    image_utils::{clipped_blocks, ensure_min_dimensions, rgb_to_gray, sobel},
};

/// Settings for [`SplicingDetector`].
#[derive(Debug, Clone)]
pub struct SplicingConfig {
    /// Tile size for the colour and edge sweeps. Default 16.
    pub block_size: u32,
    /// Scales both the inconsistency map and the colour flagging cutoff.
    pub color_sensitivity: f64,
    /// Carried but not currently wired into the internal noise analyzer.
    pub noise_sensitivity: f64,
    /// Higher lowers the edge-regularity cutoff, flagging more.
    pub edge_sensitivity: f64,
    /// Merged regions smaller than this many pixels are dropped. The default of
    /// 1000 will hide a small pasted face or licence plate.
    pub min_region_size: u32,
    /// Quality passed to the internal ELA analyzer.
    pub ela_quality: u8,
}

impl Default for SplicingConfig {
    fn default() -> Self {
        Self {
            block_size: 16,
            color_sensitivity: 0.5,
            noise_sensitivity: 0.5,
            edge_sensitivity: 0.5,
            min_region_size: 1000,
            ela_quality: 95,
        }
    }
}

/// Detects content composited in from a different image.
///
/// Runs four checks — colour histogram, edge regularity, noise and ELA — and
/// reports a region only where **at least two independently flag it**. That
/// corroboration requirement is the whole design: the individual signals are
/// weak, and each contributes 0.25 to the confidence, so the score is
/// effectively a count of agreeing methods.
///
/// # Limitations
///
/// Requiring two signals suppresses genuine single-signal splices along with
/// false positives. A resave at uniform quality erases the ELA and much of the
/// noise evidence, typically dropping a real splice below the bar.
pub struct SplicingDetector {
    config: SplicingConfig,
}

impl SplicingDetector {
    /// Detector with the default configuration.
    pub fn new() -> Self {
        Self {
            config: SplicingConfig::default(),
        }
    }

    /// Detector with custom settings.
    pub fn with_config(config: SplicingConfig) -> Self {
        Self { config }
    }

    fn analyze_color_consistency(&self, image: &RgbImage) -> (GrayImage, Vec<SRegion>) {
        let (width, height) = image.dimensions();
        let block_size = self.config.block_size;
        let mut inconsistency_map = GrayImage::new(width, height);
        let mut suspicious_regions = Vec::new();

        let global_histogram = self.calculate_color_histogram(image, 0, 0, width, height);

        for block in clipped_blocks(width, height, block_size, block_size) {
            let block_histogram =
                self.calculate_color_histogram(image, block.x, block.y, block.width, block.height);

            let diff = self.histogram_difference(&global_histogram, &block_histogram);
            let inconsistency = (diff * 255.0 * self.config.color_sensitivity).min(255.0) as u8;

            for (x, y) in block.pixels() {
                inconsistency_map.put_pixel(x, y, Luma([inconsistency]));
            }

            if diff > 0.3 * self.config.color_sensitivity {
                suspicious_regions.push(block);
            }
        }

        (inconsistency_map, suspicious_regions)
    }

    fn calculate_color_histogram(
        &self,
        image: &RgbImage,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> [[[u32; 8]; 8]; 8] {
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

        diff_sum / 2.0
    }

    fn detect_edge_inconsistencies(&self, image: &RgbImage) -> (GrayImage, Vec<SRegion>) {
        let gray = rgb_to_gray(image);
        let (width, height) = gray.dimensions();
        let mut edge_map = GrayImage::new(width, height);
        let mut suspicious_regions = Vec::new();

        if width < 3 || height < 3 {
            return (edge_map, suspicious_regions);
        }

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let (gx, gy) = sobel(&gray, x, y);
                let magnitude = (gx * gx + gy * gy).sqrt();
                edge_map.put_pixel(x, y, Luma([(magnitude.min(255.0)) as u8]));
            }
        }

        let suspicious = self.find_unnatural_edges(&edge_map);
        suspicious_regions.extend(suspicious);

        (edge_map, suspicious_regions)
    }

    fn find_unnatural_edges(&self, edge_map: &GrayImage) -> Vec<SRegion> {
        let (width, height) = edge_map.dimensions();
        let mut regions = Vec::new();
        let block_size = self.config.block_size;

        let edge_threshold = 0.9 - (0.4 * self.config.edge_sensitivity);

        for block in clipped_blocks(width, height, block_size, block_size) {
            let (horizontal_score, vertical_score) =
                self.analyze_edge_regularity(edge_map, block.x, block.y, block.width, block.height);

            if horizontal_score > edge_threshold || vertical_score > edge_threshold {
                regions.push(block);
            }
        }

        regions
    }

    fn analyze_edge_regularity(
        &self,
        edge_map: &GrayImage,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> (f64, f64) {
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
        let peaks = values
            .iter()
            .enumerate()
            .filter(|&(_, v)| *v > threshold)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();

        if peaks.len() < 2 {
            return 0.0;
        }

        let mut intervals = Vec::new();
        for i in 1..peaks.len() {
            intervals.push(peaks[i] - peaks[i - 1]);
        }

        if intervals.is_empty() {
            return 0.0;
        }

        let mean_interval =
            intervals.iter().map(|&i| i as f64).sum::<f64>() / intervals.len() as f64;
        let variance = intervals
            .iter()
            .map(|&i| (i as f64 - mean_interval).powi(2))
            .sum::<f64>()
            / intervals.len() as f64;

        1.0 / (1.0 + variance.sqrt())
    }

    fn combine_analyses(
        &self,
        color_regions: &[SRegion],
        edge_regions: &[SRegion],
        noise_regions: &[SRegion],
        ela_regions: &[SRegion],
    ) -> Vec<(SRegion, f64)> {
        let mut all_regions = Vec::new();
        all_regions.extend_from_slice(color_regions);
        all_regions.extend_from_slice(edge_regions);
        all_regions.extend_from_slice(noise_regions);
        all_regions.extend_from_slice(ela_regions);

        let mut seen = std::collections::HashSet::new();
        let mut combined = Vec::new();

        for region in &all_regions {
            // `SRegion` derives `Eq`/`Hash`, so identity dedup is a set lookup
            // rather than the linear field-by-field scan this replaces.
            if !seen.insert(*region) {
                continue;
            }

            let mut score = 0.0;
            let mut evidence_count = 0;

            if color_regions
                .iter()
                .any(|r| self.regions_overlap(r, region))
            {
                score += 0.25;
                evidence_count += 1;
            }

            if edge_regions.iter().any(|r| self.regions_overlap(r, region)) {
                score += 0.25;
                evidence_count += 1;
            }

            if noise_regions
                .iter()
                .any(|r| self.regions_overlap(r, region))
            {
                score += 0.25;
                evidence_count += 1;
            }

            if ela_regions.iter().any(|r| self.regions_overlap(r, region)) {
                score += 0.25;
                evidence_count += 1;
            }

            if evidence_count >= 2 {
                combined.push((*region, score));
            }
        }

        self.merge_overlapping_detections(combined)
    }

    fn regions_overlap(&self, a: &SRegion, b: &SRegion) -> bool {
        a.overlaps(b)
    }

    fn merge_overlapping_detections(
        &self,
        mut detections: Vec<(SRegion, f64)>,
    ) -> Vec<(SRegion, f64)> {
        if detections.is_empty() {
            return detections;
        }

        detections.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let mut merged = Vec::new();
        let mut used = vec![false; detections.len()];

        let max_merge_iterations = 10;

        for i in 0..detections.len() {
            if used[i] {
                continue;
            }

            let mut current = detections[i].0;
            let mut max_score = detections[i].1;
            used[i] = true;

            let mut iterations = 0;
            loop {
                let mut found = false;
                for j in 0..detections.len() {
                    if used[j] {
                        continue;
                    }

                    if self.regions_overlap(&current, &detections[j].0) {
                        current = current.union(&detections[j].0);
                        max_score = max_score.max(detections[j].1);
                        used[j] = true;
                        found = true;
                    }
                }

                iterations += 1;
                if !found || iterations >= max_merge_iterations {
                    break;
                }
            }

            if current.area() >= self.config.min_region_size as u64 {
                merged.push((current, max_score));
            }
        }

        merged
    }

    fn create_visualization(&self, original: &RgbImage, detections: &[(SRegion, f64)]) -> RgbImage {
        let mut vis = original.clone();

        for (region, score) in detections {
            let intensity = (*score * 255.0).min(255.0) as u8;
            let color = Rgb([intensity, (255u8.saturating_sub(intensity)), 0]);

            draw::rect(&mut vis, region, color, 2);
        }

        vis
    }
}

impl Detector for SplicingDetector {
    fn detect(&self, image: &image::DynamicImage) -> Result<DetectionResult> {
        let rgb = image.to_rgb8();
        let (width, height) = rgb.dimensions();

        ensure_min_dimensions(width, height, self.config.block_size * 2)?;

        let mut result = DetectionResult::new(&rgb);

        let (_, color_regions) = self.analyze_color_consistency(&rgb);
        let (_, edge_regions) = self.detect_edge_inconsistencies(&rgb);

        let noise_analyzer = NoiseAnalyzer::new();
        let noise_result = noise_analyzer.analyze(image)?;
        let noise_regions = noise_result.anomalous_regions;

        let ela_analyzer = ElaAnalyzer::new(self.config.ela_quality);
        let ela_result = ela_analyzer.analyze(image)?;
        let ela_regions = ela_result.suspicious_regions;

        let combined =
            self.combine_analyses(&color_regions, &edge_regions, &noise_regions, &ela_regions);

        for (region, score) in &combined {
            let mut evidence = Vec::new();

            if color_regions
                .iter()
                .any(|r| self.regions_overlap(r, region))
            {
                evidence.push("Color histogram inconsistency".into());
            }

            if edge_regions.iter().any(|r| self.regions_overlap(r, region)) {
                evidence.push("Unnatural edge patterns".into());
            }

            if noise_regions
                .iter()
                .any(|r| self.regions_overlap(r, region))
            {
                evidence.push("Noise pattern mismatch".into());
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
                evidence,
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

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    use super::*;

    /// Two visually distinct halves, i.e. a crude composite.
    fn composite(width: u32, height: u32) -> image::DynamicImage {
        let mut image = RgbImage::new(width, height);

        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = if x < width / 2 {
                let v = (((x * 13) ^ (y * 3)) % 200) as u8;
                Rgb([v, v / 2, 30])
            } else {
                let v = (((x * 5) ^ (y * 17)) % 120 + 130) as u8;
                Rgb([30, v, v])
            };
        }

        image::DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn detected_regions_stay_within_the_image() {
        let result = SplicingDetector::new()
            .detect(&composite(150, 130))
            .unwrap();

        for manipulation in &result.manipulations {
            assert!(
                manipulation.region.right() <= 150,
                "{:?}",
                manipulation.region
            );
            assert!(
                manipulation.region.bottom() <= 130,
                "{:?}",
                manipulation.region
            );
        }
    }

    #[test]
    fn undersized_images_error() {
        let image = image::DynamicImage::ImageRgb8(RgbImage::new(16, 16));
        assert!(SplicingDetector::new().detect(&image).is_err());
    }

    #[test]
    fn scores_are_bounded() {
        let result = SplicingDetector::new()
            .detect(&composite(128, 128))
            .unwrap();

        assert!((0.0..=1.0).contains(&result.overall_score));
        for manipulation in &result.manipulations {
            assert!((0.0..=1.0).contains(&manipulation.confidence));
        }
    }
}
