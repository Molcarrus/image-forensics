use image::{GrayImage, Luma, Rgb, RgbImage};

use crate::{CopyMoveResult, ElaResult, FullAnalysisReport, NoiseResult, SRegion, detection::{DetectionResult, ManipulationType}, error::Result};

#[derive(Debug, Clone, Copy)]
pub enum ColorScheme {
    HeatMap,
    Diverging,
    Viridis,
    Grayscale,
    SingleColor(Rgb<u8>),
}

#[derive(Debug, Clone)]
pub struct VisualizationConfig {
    pub color_scheme: ColorScheme,
    pub overlay_opacity: f32,
    pub border_thickness: u32,
    pub show_labels: bool,
    pub label_scale: f32,
    pub show_legend: bool,
}

impl Default for VisualizationConfig {
    fn default() -> Self {
        Self { 
            color_scheme: ColorScheme::HeatMap, 
            overlay_opacity: 0.5, 
            border_thickness: 2, 
            show_labels: true, 
            label_scale: 1.0, 
            show_legend: true 
        }
    }
}

pub struct Visualizer {
    config: VisualizationConfig
}

impl Visualizer {
    pub fn new() -> Self {
        Self { config: VisualizationConfig::default() }
    }
    
    pub fn with_config(config: VisualizationConfig) -> Self {
        Self { config }
    }
    
    pub fn create_heatmap(&self, gray: &GrayImage) -> RgbImage {
        let (width, height) = gray.dimensions();
        let mut heatmap = RgbImage::new(width, height);
        
        for (x, y, pixel) in gray.enumerate_pixels() {
            let intensity = pixel[0] as f32 / 255.0;
           let color = self .intensity_to_color(intensity);
           heatmap.put_pixel(x, y, color);
        }
        
        heatmap
    }
    
    fn intensity_to_color(&self, intensity: f32) -> Rgb<u8> {
        let intensity = intensity.clamp(0.0, 1.0);
        
        match self.config.color_scheme {
            ColorScheme::HeatMap => {
                let (r, g, b) = if intensity < 0.25 {
                    let t = intensity / 0.25;
                    (0.0, t, 1.0)
                } else if intensity < 0.5 {
                    let t = (intensity - 0.25) / 0.25;
                    (0.0, 1.0, 1.0 - t)
                } else if intensity < 0.75 {
                    let t = (intensity - 0.5) / 0.25;
                    (t, 1.0, 0.0)
                } else {
                    let t = (intensity - 0.75) / 0.25;
                    (1.0, 1.0 - t, 0.0)
                };
                Rgb([(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8])
            }
            ColorScheme::Diverging => {
                let (r, g, b) = if intensity < 0.5 {
                    let t = intensity / 0.5;
                    (t, t, 1.0)
                } else {
                    let t = (intensity - 0.5) / 0.5;
                    (1.0, 1.0 - t, 1.0 - t)
                };
                Rgb([(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8])
            }
            ColorScheme::Viridis => {
                let (r, g, b) = Self::viridis_color(intensity);
                Rgb([(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8])
            }
            ColorScheme::Grayscale => {
                let v = (intensity * 255.0) as u8;
                Rgb([v, v, v])
            }
            ColorScheme::SingleColor(base) => {
                Rgb([
                    (base[0] as f32 * intensity) as u8,
                    (base[1] as f32 * intensity) as u8,
                    (base[2] as f32 * intensity) as u8,
                ])
            }
        }
    }
    
    fn viridis_color(t: f32) -> (f32, f32, f32) {
        let r = 0.267004 + t * (0.993248 - 0.267004);
        let g = 0.004874 + t * (0.906157 - 0.004874);
        let b = 0.329415 + t * (0.143936 - 0.329415) * (1.0 - t) + t * (0.143936);
        (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0))
    }
    
    pub fn overlay_heatmap(&self, original: &RgbImage, heatmap: &RgbImage) -> RgbImage {
        let (width, height) = original.dimensions();
        let mut result = RgbImage::new(width, height);
        let alpha = self.config.overlay_opacity;
        
        for y in 0..height {
            for x in 0..width {
                let orig = original.get_pixel(x, y);
                let heat = heatmap.get_pixel(
                    x.min(heatmap.width() - 1),
                    y.min(heatmap.height() - 1)
                );
                
                let r = ((1.0 - alpha) * orig[0] as f32 + alpha * heat[0] as f32) as u8;
                let g = ((1.0 - alpha) * orig[1] as f32 + alpha * heat[1] as f32) as u8;
                let b = ((1.0 - alpha) * orig[2] as f32 + alpha * heat[2] as f32) as u8;
                
                result.put_pixel(x, y, Rgb([r, g, b]));
            }
        }
        
        result
    }
    
    pub fn visualize_ela(&self, original: &RgbImage, ela_result: &ElaResult) -> RgbImage {
        let heatmap = self.create_heatmap(&ela_result.difference_map);
        let mut vis = self.overlay_heatmap(original, &heatmap);
        
        for region in &ela_result.suspicious_regions {
            self.draw_region_border(&mut vis, region, Rgb([255, 0, 0]));
            
            if self.config.show_legend {
                self.draw_legend(&mut vis, "ELA Analysis", &[
                    ("Low difference", Rgb([0, 0, 255])),
                    ("Medium", Rgb([0, 255, 0])),
                    ("High difference", Rgb([255, 0, 0])),
                ]);
            }
        }
        
        vis
    }
    
    pub fn visualize_copy_move(&self, original: &RgbImage, result: &CopyMoveResult) -> RgbImage {
        let mut vis = original.clone();
        
        for (i, match_pair) in result.matches.iter().enumerate() {
            let hue = (i as f32 * 137.5) % 360.0;
            let color = self.hsv_to_rgb(hue, 1.0, 1.0);
            
            self.draw_region_filled(&mut vis, &match_pair.source, color, 0.3);
            self.draw_region_border(&mut vis, &match_pair.source, color);
            
            self.draw_region_filled(&mut vis, &match_pair.target, color, 0.3);
            self.draw_region_border(&mut vis, &match_pair.target, color);
            
            self.draw_line(
                &mut vis, 
                match_pair.source.x + match_pair.source.width / 2, 
                match_pair.source.y + match_pair.source.height / 2, 
                match_pair.target.x + match_pair.target.width / 2, 
                match_pair.target.y + match_pair.target.height / 2,
               color, 
            );
        }
        
        vis
    }    
    
    pub fn visualize_noise(&self, original: &RgbImage, result: &NoiseResult) -> RgbImage {
        let heatmap = self.create_heatmap(&result.local_variance_map);
        let mut vis = self.overlay_heatmap(original, &heatmap);
        
        for region in &result.anomalous_regions {
            self.draw_region_border(&mut vis, region, Rgb([255, 255, 0]));
        }
        
        vis 
    }
    
    pub fn visualize_detections(&self, original: &RgbImage, result: &DetectionResult) -> RgbImage {
        let mut vis = original.clone();
        
        for manipulation in &result.manipulations {
            let color = self.mainpulation_type_color(&manipulation.manipulation_type);
            let opacity = manipulation.confidence as f32 * 0.4;
            
            self.draw_region_filled(&mut vis, &manipulation.region, color, opacity);
            self.draw_region_border(&mut vis, &manipulation.region, color);
            
            if self.config.show_labels {
                self.draw_label(
                    &mut vis, 
                    manipulation.region.x, 
                    manipulation.region.y, 
                    &format!("{:.0}%", manipulation.confidence * 100.0),
                   color, 
                );
            }
        }
        
        if self.config.show_legend {
            self.draw_detection_legend(&mut vis);
        }
        
        vis
    }
    
    pub fn visulaize_full_analysis(&self, original: &RgbImage, report: &FullAnalysisReport) -> ComprehensiveVisualization {
        let ela_vis = self.visualize_ela(original, &report.ela);
        let copy_move_vis = self.visualize_copy_move(original, &report.copy_move);
        let noise_vis = self.visualize_noise(original, &report.noise);
        
        let combined = self.create_combined_overview(
            original, 
            &report.ela, 
            &report.copy_move, 
            &report.noise
        );
        
        ComprehensiveVisualization { 
            original: original.clone(), 
            ela: ela_vis, 
            copy_move: copy_move_vis, 
            noise: noise_vis, 
            combined, 
            tampering_probability: report.tampering_ability 
        }
    }
    
    fn create_combined_overview(
        &self,
        original: &RgbImage,
        ela: &ElaResult,
        copy_move: &CopyMoveResult,
        noise: &NoiseResult,
    ) -> RgbImage {
        let mut vis = original.clone();
        let (width, height) = vis.dimensions();
        
        let mut suspicion_map = GrayImage::new(width, height);
        
        for (x, y, pixel) in ela.difference_map.enumerate_pixels() {
            if x < width && y < height {
                let current = suspicion_map.get_pixel(x, y)[0];
                let new_val = current.saturating_add(pixel[0] / 3);
                suspicion_map.put_pixel(x, y, Luma([new_val]));
            }
        }
        
        for (x, y, pixel) in noise.local_variance_map.enumerate_pixels() {
            if x < width && y < height {
                let current = suspicion_map.get_pixel(x, y)[0];
                let new_val = current.saturating_add(pixel[0] / 3);
                suspicion_map.put_pixel(x, y, Luma([new_val]));
            }
        }
        
        for match_pair in &copy_move.matches {
            self.fill_region_in_gray(&mut suspicion_map, &match_pair.source, 200);
            self.fill_region_in_gray(&mut suspicion_map, &match_pair.target, 200);
        }
        
        let heatmap = self.create_heatmap(&suspicion_map);
        vis = self.overlay_heatmap(&vis, &heatmap);
        
        for match_pair in &copy_move.matches {
            self.draw_line(
                &mut vis, 
                match_pair.source.x + match_pair.source.width / 2, 
                match_pair.source.y + match_pair.source.height / 2, 
                match_pair.target.x + match_pair.target.width / 2,
                match_pair.target.y + match_pair.target.height / 2,
                Rgb([255, 0, 255])
            );
        }
        
        vis
    }
    
    fn fill_region_in_gray(&self, image: &mut GrayImage, region: &SRegion, value: u8) {
        let (width, height) = image.dimensions();
        for y in region.y..(region.y + region.height).min(height) {
            for x in region.x..(region.x + region.width).min(width) {
                image.put_pixel(x, y, Luma([value]));
            }
        }
    }
    
    fn mainpulation_type_color(&self, manipulation_type: &ManipulationType) -> Rgb<u8> {
        match manipulation_type {
            ManipulationType::CopyMove => Rgb([255, 0, 0]),
            ManipulationType::Splicing => Rgb([255, 165, 0]),
            ManipulationType::Retouching => Rgb([255, 255, 0]),
            ManipulationType::Removal => Rgb([255, 0, 255]),
            ManipulationType::Resizing => Rgb([0, 255, 255]),
            ManipulationType::Rotation => Rgb([0, 255, 0]),
            ManipulationType::ColorManipulation => Rgb([128, 0, 255]),
            ManipulationType::AIGenerated => Rgb([255, 128, 128]),
            ManipulationType::Unknown => Rgb([128, 128, 128])
        }
    }
    
    fn draw_region_border(&self, image: &mut RgbImage, region: &SRegion, color: Rgb<u8>) {
        let (width, height) = image.dimensions();
        let thickness = self.config.border_thickness;
        
        for t in 0..thickness {
            for x in region.x.saturating_sub(t)..(region.x + region.width + t).min(width) {
                if region.y >= t {
                    image.put_pixel(x, region.y - t, color);
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
                    image.put_pixel(region.x - t, y, color);
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
    
    fn draw_region_filled(&self, image: &mut RgbImage, region: &SRegion, color: Rgb<u8>, opacity: f32) {
        let (width, height) = image.dimensions();
        
        for y in region.y..(region.y + region.height).min(height) {
            for x in region.x..(region.x + region.width).min(width) {
                let original = image.get_pixel(x, y);
                let blended = Rgb([
                    ((1.0 - opacity) * original[0] as f32 + opacity * color[0] as f32) as u8,
                    ((1.0 - opacity) * original[1] as f32 + opacity * color[1] as f32) as u8,
                    ((1.0 - opacity) * original[2] as f32 + opacity * color[2] as f32) as u8,
                ]);
                image.put_pixel(x, y, blended);
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
        let sx = if x0 < x1 { 1i32 } else { -1i32 };
        let sy = if y0 < y1 { 1i32 } else { -1i32 };
        let mut err = dx * dy;
        
        let mut x = x0 as i32;
        let mut y = y0 as i32;
        
        loop {
            if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let px = (x + dx).max(0).min(width as i32 - 1) as u32;
                        let py = (y + dy).max(0).min(height as i32 - 1) as u32;
                        image.put_pixel(px, py, color);
                    }
                }
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
    
    fn draw_label(
        &self, 
        image: &mut RgbImage, 
        x: u32, 
        y: u32, 
        text: &str, 
        color: Rgb<u8>
    ) {
        let (width, height) = image.dimensions();
        let char_width = 6;
        let char_height = 8;
        
        let bg_width = (text.len() * char_width + 4) as u32;
        let bg_height = (char_height + 4) as u32;
        
        for dy in 0..bg_height {
            for dx in 0..bg_width {
                let px = x+ dx;
                let py = y + dy;
                if px < width && py < height {
                    image.put_pixel(px, py, Rgb([0, 0, 0]));
                }
            }
        }
        
        for (i, _c) in text.chars().enumerate() {
            let cx = x + 2 + (i * char_width) as u32;
            let cy = y + 2;
            
            for dy in 0..(char_height as u32) {
                for dx in 0..((char_width - 1) as u32) {
                    let px = cx + dx;
                    let py = cy + dy;
                    if px < width && py < height {
                        image.put_pixel(px, py, color);
                    }
                }
            }
        }
    }
    
    fn draw_legend(&self, image: &mut RgbImage, title: &str, items: &[(&str, Rgb<u8>)]) {
        let (width, height) = image.dimensions();
        let legend_width = 150u32;
        let legend_height = (items.len() * 20 + 30) as u32;
        let legend_x = width.saturating_sub(legend_width + 10);
        let legend_y = 10u32;
        
        for y in legend_y..(legend_y + legend_height).min(height) {
            for x in legend_x..(legend_x + legend_width).min(width) {
                let original = image.get_pixel(x, y);
                let blended = Rgb([
                    (original[0] as f32 * 0.3) as u8,
                    (original[1] as f32 * 0.3) as u8,
                    (original[2] as f32 * 0.3) as u8,
                ]);
                image.put_pixel(x, y, blended);
            }
        }
        
        self.draw_label(image, legend_x + 5, legend_y + 5, title, Rgb([255, 255, 255]));
        
        for (i, (label, color)) in items.iter().enumerate() {
            let item_y = legend_y + 25 + (i * 20) as u32;
            
            for dy in 0..12u32 {
                for dx in 0..20u32 {
                    let (px, py) = (legend_x + 5 + dx, item_y + dy);
                    if px < width && py < height {
                        image.put_pixel(px, py, *color);
                    }
                }
            }
            
            self.draw_label(image, legend_x + 30, item_y + 2, label, Rgb([255, 255, 255]));
        }
    }
    
    fn draw_detection_legend(&self, image: &mut RgbImage) {
        self.draw_legend(image, "Detection Type", &[
            ("Copy-Move", Rgb([255, 0, 0])),
            ("Splicing", Rgb([255, 165, 0])),
            ("Retouching", Rgb([255, 255, 0])),
            ("Removal", Rgb([255, 0, 255])),
            ("Other", Rgb([128, 128, 128])),
        ]);
    }
    
    fn hsv_to_rgb(&self, h: f32, s: f32, v: f32) -> Rgb<u8> {
        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;
        
        let (r, g, b) = if h < 60.0 {
            (c, x, 0.0)
        } else if h < 120.0 {
            (x, c, 0.0)
        } else if h < 180.0 {
            (0.0, c, x)
        } else if h < 240.0 {
            (0.0, x, c)
        } else if h < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };
        
        Rgb([
            ((r + m) * 255.0) as u8,
            ((g + m) * 255.0) as u8,
            ((b + m) * 255.0) as u8,
        ])
    }
    
    pub fn create_comparison(&self, images: &[(&str, &RgbImage)]) -> RgbImage {
        if images.is_empty() {
            return RgbImage::new(1, 1);
        }
        
        let padding = 10u32;
        let label_height = 20u32;
        
        let max_height = images
            .iter()
            .map(|(_, img)| img.height())
            .max().unwrap_or(0);
        
        let total_width = images
            .iter()
            .map(|(_, img)| img.width())
            .sum::<u32>() + padding * (images.len() as u32 + 1);
        let toal_height = max_height + label_height + padding * 2;
        
        let mut result = RgbImage::from_pixel(total_width, toal_height, Rgb([40, 40, 40]));
        
        let mut x_offset = padding;
        for (label, img) in images {
            self.draw_label(&mut result, x_offset, padding / 2, label, Rgb([255, 255, 255]));
            
            for y in 0..img.height() {
                for x in 0..img.width() {
                    let px = x_offset + x;
                    let py = label_height + padding + y;
                    if px < total_width && py < toal_height {
                        result.put_pixel(px, py, *img.get_pixel(x, y));
                    }
                }
            }
            
            x_offset += img.width() + padding;
        }
        
        result
    }
    
    fn copy_image_to(
        &self,
        dest: &mut RgbImage,
        src: &RgbImage,
        offset_x: u32,
        offset_y: u32 
    ) {
        let (dest_w, dest_h) = dest.dimensions();
        let (src_w, src_h) = src.dimensions();
        
        for y in 0..src_h {
            for x in 0..src_w {
                let dx = offset_x + x;
                let dy = offset_y + y;
                if dx < dest_w && dy < dest_h {
                    dest.put_pixel(dx, dy, *src.get_pixel(x, y));
                }
            }
        }
    }
}

impl Default for Visualizer {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ComprehensiveVisualization {
    pub original: RgbImage,
    pub ela: RgbImage,
    pub copy_move: RgbImage,
    pub noise: RgbImage,
    pub combined: RgbImage,
    pub tampering_probability: f64,
}

impl ComprehensiveVisualization {
    pub fn save_all(&self, directory: &str) -> Result<()> {
        std::fs::create_dir_all(directory)?;
        
        self.original.save(format!("{}/original.png", directory))?;
        self.ela.save(format!("{}/ela.png", directory))?;
        self.copy_move.save(format!("{}/copy_move.png", directory))?;
        self.noise.save(format!("{}/noise.png", directory))?;
        self.combined.save(format!("{}/combined.png", directory))?;
        
        Ok(())
    }
    
    pub fn create_report_image(&self) -> RgbImage {
        let visualizer = Visualizer::new();
        visualizer.create_comparison(&[
            ("Original", &self.original),
            ("ELA", &self.ela),
            ("Copy-Move", &self.copy_move),
            ("Combined", &self.combined)
        ])
    }
}