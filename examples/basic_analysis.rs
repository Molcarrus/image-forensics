//! Basic Analysis Example
//!
//! This example demonstrates how to use individual analysis methods
//! from the image-forensics library.
//!
//! Run with: cargo run --example basic_analysis -- <image_path>

use image::GenericImageView;
use image_forensics::{
    ForensicsAnalyzer, AnalysisConfig, error::Result,
    analysis::ela::ElaAnalyzer,
    analysis::copy_move::CopyMoveDetector,
    analysis::noise::NoiseAnalyzer,
    analysis::jpeg_analysis::JpegAnalyzer,
    metadata::exif::ExifExtractor,
};
use std::env;
use std::fs;
use std::path::Path;

fn main() -> Result<()> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("Image Forensics - Basic Analysis Example");
        println!("=========================================");
        println!();
        println!("Usage: {} <image_path> [output_dir]", args[0]);
        println!();
        println!("Arguments:");
        println!("  image_path  - Path to the image to analyze");
        println!("  output_dir  - Optional output directory (default: ./output)");
        println!();
        println!("Example:");
        println!("  {} suspicious_photo.jpg ./results", args[0]);
        return Ok(());
    }
    
    let image_path = &args[1];
    let output_dir = args.get(2).map(|s| s.as_str()).unwrap_or("./output");
    
    // Verify input file exists
    if !Path::new(image_path).exists() {
        eprintln!("Error: Image file '{}' not found", image_path);
        std::process::exit(1);
    }
    
    // Create output directory
    fs::create_dir_all(output_dir)?;
    
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           Image Forensics - Basic Analysis                   ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("📁 Input:  {}", image_path);
    println!("📂 Output: {}", output_dir);
    println!();
    
    // Load the image
    println!("Loading image...");
    let image = image::open(image_path)?;
    let (width, height) = image.dimensions();
    println!("  ✓ Image loaded: {}x{} pixels", width, height);
    println!();
    
    // ═══════════════════════════════════════════════════════════════════
    // 1. ERROR LEVEL ANALYSIS (ELA)
    // ═══════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("1️⃣  ERROR LEVEL ANALYSIS (ELA)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("ELA detects differences in JPEG compression levels across an image.");
    println!("Edited regions often show different error levels than original areas.");
    println!();
    
    // Run ELA with different quality settings
    for quality in [95, 90, 85] {
        print!("  Analyzing at quality {}... ", quality);
        
        let ela_analyzer = ElaAnalyzer::new(quality)
            .with_amplification(15.0)  // Amplify differences for visibility
            .with_threshold(25.0);     // Threshold for suspicious regions
        
        let ela_result = ela_analyzer.analyze(&image)?;
        
        // Save ELA visualization
        let ela_output = format!("{}/ela_q{}.png", output_dir, quality);
        ela_result.save(&ela_output)?;
        
        println!("✓");
        println!("     Max difference:     {:.2}", ela_result.max_difference);
        println!("     Mean difference:    {:.2}", ela_result.mean_difference);
        println!("     Std deviation:      {:.2}", ela_result.std_deviation);
        println!("     Suspicious regions: {}", ela_result.suspicious_regions.len());
        println!("     Output: {}", ela_output);
        println!();
    }
    
    // Interpretation guide
    println!("  📊 Interpretation:");
    println!("     • High error levels in specific areas may indicate editing");
    println!("     • Uniform error levels suggest the image hasn't been modified");
    println!("     • Look for regions that stand out from the background");
    println!();
    
    // ═══════════════════════════════════════════════════════════════════
    // 2. COPY-MOVE DETECTION
    // ═══════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("2️⃣  COPY-MOVE FORGERY DETECTION");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Detects regions that have been copied and pasted within the image.");
    println!("Uses DCT-based block matching with locality-sensitive hashing.");
    println!();
    
    print!("  Detecting copy-move forgery... ");
    
    let copy_move_detector = CopyMoveDetector::new(
        16,    // Block size (16x16 pixels)
        0.92,  // Similarity threshold (92%)
        50,    // Minimum distance between matches
    )?;
    
    let copy_move_result = copy_move_detector.detect(&image)?;
    
    // Save visualization
    let copy_move_output = format!("{}/copy_move.png", output_dir);
    copy_move_result.visualization.save(&copy_move_output)?;
    
    println!("✓");
    println!();
    println!("  Results:");
    println!("     Matching regions found: {}", copy_move_result.matches.len());
    println!("     Overall confidence:     {:.1}%", copy_move_result.confidence * 100.0);
    println!("     Output: {}", copy_move_output);
    println!();
    
    if !copy_move_result.matches.is_empty() {
        println!("  Detected matches:");
        for (i, match_pair) in copy_move_result.matches.iter().take(5).enumerate() {
            println!(
                "     {}. Source: ({}, {}) → Target: ({}, {}) | Similarity: {:.1}%",
                i + 1,
                match_pair.source.x, match_pair.source.y,
                match_pair.target.x, match_pair.target.y,
                match_pair.similarity * 100.0
            );
        }
        if copy_move_result.matches.len() > 5 {
            println!("     ... and {} more matches", copy_move_result.matches.len() - 5);
        }
        println!();
    }
    
    // ═══════════════════════════════════════════════════════════════════
    // 3. NOISE ANALYSIS
    // ═══════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("3️⃣  NOISE PATTERN ANALYSIS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Analyzes noise patterns to detect inconsistencies.");
    println!("Spliced regions often have different noise characteristics.");
    println!();
    
    print!("  Analyzing noise patterns... ");
    
    let noise_analyzer = NoiseAnalyzer::new()
        .with_block_size(16);
    
    let noise_result = noise_analyzer.analyze(&image)?;
    
    // Save visualizations
    let noise_map_output = format!("{}/noise_map.png", output_dir);
    let variance_map_output = format!("{}/noise_variance.png", output_dir);
    
    noise_result.noise_map.save(&noise_map_output)?;
    noise_result.local_variance_map.save(&variance_map_output)?;
    
    println!("✓");
    println!();
    println!("  Results:");
    println!("     Estimated noise level:  {:.2}", noise_result.estimated_noise_level);
    println!("     Inconsistency score:    {:.2}%", noise_result.inconsistency_score * 100.0);
    println!("     Anomalous regions:      {}", noise_result.anomalous_regions.len());
    println!("     Noise map: {}", noise_map_output);
    println!("     Variance map: {}", variance_map_output);
    println!();
    
    // Interpretation
    let noise_interpretation = if noise_result.inconsistency_score < 0.1 {
        "Low - noise appears consistent across the image"
    } else if noise_result.inconsistency_score < 0.3 {
        "Moderate - some noise variations detected"
    } else {
        "High - significant noise inconsistencies found"
    };
    println!("  📊 Inconsistency level: {}", noise_interpretation);
    println!();
    
    // ═══════════════════════════════════════════════════════════════════
    // 4. JPEG ARTIFACT ANALYSIS
    // ═══════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("4️⃣  JPEG ARTIFACT ANALYSIS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Analyzes JPEG compression artifacts for signs of manipulation.");
    println!("Detects double compression, JPEG ghosts, and blocking artifacts.");
    println!();
    
    print!("  Analyzing JPEG artifacts... ");
    
    let jpeg_analyzer = JpegAnalyzer::new();
    let jpeg_result = jpeg_analyzer.analyze(&image)?;
    
    // Save blocking artifact map
    let blocking_output = format!("{}/jpeg_blocking.png", output_dir);
    jpeg_result.blocking_artifact_map.save(&blocking_output)?;
    
    // Save ghost map if detected
    if let Some(ref ghost_map) = jpeg_result.ghost_map {
        let ghost_output = format!("{}/jpeg_ghost.png", output_dir);
        ghost_map.save(&ghost_output)?;
        println!("✓");
        println!("     Ghost map: {}", ghost_output);
    } else {
        println!("✓");
    }
    
    println!();
    println!("  Results:");
    println!("     Estimated quality:           {}", jpeg_result.quality_estimate);
    println!("     JPEG ghost detected:         {}", if jpeg_result.ghost_detected { "Yes ⚠️" } else { "No" });
    println!("     Double compression likelihood: {:.1}%", jpeg_result.double_compression_likelihood * 100.0);
    println!("     Blocking artifact map: {}", blocking_output);
    println!();
    
    // ═══════════════════════════════════════════════════════════════════
    // 5. METADATA EXTRACTION
    // ═══════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("5️⃣  METADATA EXTRACTION");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Extracts and analyzes EXIF metadata for suspicious indicators.");
    println!();
    
    print!("  Extracting metadata... ");
    
    match ExifExtractor::extract(image_path) {
        Ok(metadata) => {
            println!("✓");
            println!();
            println!("  Camera Information:");
            if let Some(ref make) = metadata.camera_make {
                println!("     Make:     {}", make);
            }
            if let Some(ref model) = metadata.camera_model {
                println!("     Model:    {}", model);
            }
            if let Some(ref software) = metadata.software {
                println!("     Software: {}", software);
            }
            if let Some(ref datetime) = metadata.date_time {
                println!("     DateTime: {}", datetime);
            }
            if let Some((lat, lon)) = metadata.gps_coordinates {
                println!("     GPS:      {:.6}, {:.6}", lat, lon);
            }
            
            println!();
            println!("  Total metadata tags found: {}", metadata.all_tags.len());
            
            if !metadata.suspicious_indicators.is_empty() {
                println!();
                println!("  ⚠️  Suspicious Indicators:");
                for indicator in &metadata.suspicious_indicators {
                    println!("     • {}", indicator);
                }
            }
            
            // Save metadata to JSON
            let metadata_output = format!("{}/metadata.json", output_dir);
            let metadata_json = serde_json::to_string_pretty(&metadata.all_tags).unwrap_or_default();
            fs::write(&metadata_output, metadata_json)?;
            println!();
            println!("  Metadata saved to: {}", metadata_output);
        }
        Err(e) => {
            println!("⚠️");
            println!("     Could not extract metadata: {}", e);
        }
    }
    println!();
    
    // ═══════════════════════════════════════════════════════════════════
    // SUMMARY
    // ═══════════════════════════════════════════════════════════════════
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📋 ANALYSIS SUMMARY");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    // Calculate overall suspicion score
    let mut suspicion_factors: Vec<(&str, f64)> = Vec::new();
    
    // ELA-based suspicion (using quality 95 result)
    let ela_analyzer = ElaAnalyzer::new(95);
    let ela_result = ela_analyzer.analyze(&image)?;
    if ela_result.max_difference > 50.0 {
        suspicion_factors.push(("High ELA differences", (ela_result.max_difference / 255.0).min(1.0)));
    }
    
    // Copy-move based suspicion
    if !copy_move_result.matches.is_empty() {
        suspicion_factors.push(("Copy-move regions detected", copy_move_result.confidence));
    }
    
    // Noise-based suspicion
    if noise_result.inconsistency_score > 0.2 {
        suspicion_factors.push(("Noise inconsistencies", noise_result.inconsistency_score));
    }
    
    // JPEG-based suspicion
    if jpeg_result.ghost_detected {
        suspicion_factors.push(("JPEG ghost detected", 0.7));
    }
    if jpeg_result.double_compression_likelihood > 0.5 {
        suspicion_factors.push(("Double compression likely", jpeg_result.double_compression_likelihood));
    }
    
    if suspicion_factors.is_empty() {
        println!("  ✅ No significant signs of manipulation detected.");
        println!();
        println!("  The image appears to be authentic based on automated analysis.");
        println!("  However, sophisticated manipulations may not be detected.");
    } else {
        println!("  ⚠️  Potential signs of manipulation detected:");
        println!();
        
        let mut total_score = 0.0;
        for (factor, score) in &suspicion_factors {
            let bar_length = (score * 20.0) as usize;
            let bar: String = "█".repeat(bar_length) + &"░".repeat(20 - bar_length);
            println!("     {} [{:.0}%]", factor, score * 100.0);
            println!("     [{}]", bar);
            println!();
            total_score += score;
        }
        
        let avg_score = total_score / suspicion_factors.len() as f64;
        println!("  Overall suspicion level: {:.0}%", avg_score * 100.0);
    }
    
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("All outputs saved to: {}/", output_dir);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    Ok(())
}