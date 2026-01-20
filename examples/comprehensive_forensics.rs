//! Comprehensive Forensics Analysis Example
//!
//! This example demonstrates full forensic analysis workflow including:
//! - All analysis methods combined
//! - Detection modules (splicing, tampering)
//! - Visualization generation
//! - JSON report generation
//! - HTML report generation
//!
//! Run with: cargo run --example comprehensive_forensics -- <image_path>

use image::GenericImageView;
use image_forensics::{
    AnalysisConfig, ForensicsAnalyzer, FullAnalysisReport,
    detection::{
        ConfidenceLevel, DetectionResult, Detector, ManipulationType,
        splicing::{SplicingConfig, SplicingDetector},
        tampering::{TamperingConfig, TamperingDetector},
    },
    error::Result,
    report::{
        JsonReport,
        visualization::{ColorScheme, VisualizationConfig, Visualizer},
    },
};
use std::env;
use std::fs;
use std::path::Path;
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage(&args[0]);
        return Ok(());
    }

    let image_path = &args[1];
    let output_dir = args
        .get(2)
        .map(|s| s.as_str())
        .unwrap_or("./forensics_report");

    // Verify input
    if !Path::new(image_path).exists() {
        eprintln!("❌ Error: Image file '{}' not found", image_path);
        std::process::exit(1);
    }

    // Create output directory structure
    fs::create_dir_all(format!("{}/visualizations", output_dir))?;
    fs::create_dir_all(format!("{}/analysis", output_dir))?;

    print_header();
    println!("📁 Analyzing: {}", image_path);
    println!("📂 Output:    {}/", output_dir);
    println!();

    let total_start = Instant::now();

    // ═══════════════════════════════════════════════════════════════════
    // PHASE 1: LOAD AND PREPARE
    // ═══════════════════════════════════════════════════════════════════
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ PHASE 1: Loading and Preparation                           │");
    println!("└─────────────────────────────────────────────────────────────┘");

    let start = Instant::now();

    // Configure the analyzer
    let config = AnalysisConfig {
        ela_quality: 95,
        block_size: 16,
        similarity_threshold: 0.90,
        parallel: true,
        min_match_distance: 50,
    };

    let analyzer = ForensicsAnalyzer::new(image_path)?.with_config(config.clone());

    let image = image::open(image_path)?;
    let (width, height) = image.dimensions();

    println!(
        "  ✓ Image loaded: {}x{} pixels ({:.2} MP)",
        width,
        height,
        (width as f64 * height as f64) / 1_000_000.0
    );
    println!("  ✓ Configuration applied");
    println!("  ⏱ Time: {:?}", start.elapsed());
    println!();

    // ═══════════════════════════════════════════════════════════════════
    // PHASE 2: CORE ANALYSIS
    // ═══════════════════════════════════════════════════════════════════
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ PHASE 2: Core Forensic Analysis                            │");
    println!("└─────────────────────────────────────────────────────────────┘");

    let start = Instant::now();

    print!("  Running full analysis suite...");
    let full_report = analyzer.full_analysis()?;
    println!(" ✓");

    // Save individual analysis results
    println!("  Saving analysis outputs:");

    // ELA
    full_report
        .ela
        .save(format!("{}/analysis/ela.png", output_dir))?;
    full_report
        .ela
        .difference_map
        .save(format!("{}/analysis/ela_difference.png", output_dir))?;
    println!("    ✓ ELA analysis");

    // Copy-Move
    full_report
        .copy_move
        .visualization
        .save(format!("{}/analysis/copy_move.png", output_dir))?;
    println!("    ✓ Copy-move detection");

    // Noise
    full_report
        .noise
        .noise_map
        .save(format!("{}/analysis/noise_map.png", output_dir))?;
    full_report
        .noise
        .local_variance_map
        .save(format!("{}/analysis/noise_variance.png", output_dir))?;
    println!("    ✓ Noise analysis");

    // JPEG
    full_report
        .jpeg
        .blocking_artifact_map
        .save(format!("{}/analysis/jpeg_blocking.png", output_dir))?;
    if let Some(ref ghost_map) = full_report.jpeg.ghost_map {
        ghost_map.save(format!("{}/analysis/jpeg_ghost.png", output_dir))?;
    }
    println!("    ✓ JPEG analysis");

    println!("  ⏱ Time: {:?}", start.elapsed());
    println!();

    // Print analysis summary
    print_analysis_summary(&full_report);

    // ═══════════════════════════════════════════════════════════════════
    // PHASE 3: ADVANCED DETECTION
    // ═══════════════════════════════════════════════════════════════════
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ PHASE 3: Advanced Tampering Detection                      │");
    println!("└─────────────────────────────────────────────────────────────┘");

    let start = Instant::now();

    // Splicing Detection
    print!("  Running splicing detection...");
    let splicing_config = SplicingConfig {
        block_size: 16,
        color_sensitivity: 0.5,
        noise_sensitivity: 0.5,
        edge_sensitivity: 0.5,
        min_region_size: 100,
        ela_quality: 95,
    };
    let splicing_detector = SplicingDetector::with_config(splicing_config);
    let splicing_result = splicing_detector.detect(&image)?;
    splicing_result
        .visualization
        .save(format!("{}/analysis/splicing_detection.png", output_dir))?;
    println!(" ✓ ({} regions)", splicing_result.manipulations.len());

    // Comprehensive Tampering Detection
    print!("  Running comprehensive tampering detection...");
    let tampering_config = TamperingConfig {
        detect_copy_move: true,
        detect_splicing: true,
        detect_retouching: true,
        block_size: 16,
        sensitivity: 0.5,
        min_confidence: 0.3,
    };
    let tampering_detector = TamperingDetector::with_config(tampering_config);
    let tampering_result = tampering_detector.detect(&image)?;
    tampering_result
        .visualization
        .save(format!("{}/analysis/tampering_detection.png", output_dir))?;
    println!(" ✓ ({} detections)", tampering_result.manipulations.len());

    println!("  ⏱ Time: {:?}", start.elapsed());
    println!();

    // Print detection summary
    print_detection_summary(&splicing_result, &tampering_result);

    // ═══════════════════════════════════════════════════════════════════
    // PHASE 4: VISUALIZATION
    // ═══════════════════════════════════════════════════════════════════
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ PHASE 4: Visualization Generation                          │");
    println!("└─────────────────────────────────────────────────────────────┘");

    let start = Instant::now();

    let rgb_image = image.to_rgb8();

    // Create visualizer with custom config
    let vis_config = VisualizationConfig {
        color_scheme: ColorScheme::HeatMap,
        overlay_opacity: 0.5,
        border_thickness: 3,
        show_labels: true,
        label_scale: 1.0,
        show_legend: true,
    };
    let visualizer = Visualizer::with_config(vis_config);

    // Generate comprehensive visualization
    print!("  Generating comprehensive visualization...");
    let comprehensive_vis = visualizer.visulaize_full_analysis(&rgb_image, &full_report);
    comprehensive_vis.save_all(&format!("{}/visualizations", output_dir))?;
    println!(" ✓");

    // Generate analysis grid
    print!("  Generating analysis grid...");
    let grid = visualizer.create_analysis_grid(&rgb_image, &full_report);
    grid.save(format!("{}/visualizations/analysis_grid.png", output_dir))?;
    println!(" ✓");

    // Generate comparison view
    print!("  Generating comparison view...");
    let comparison = visualizer.create_comparison(&[
        ("Original", &rgb_image),
        ("ELA", &comprehensive_vis.ela),
        ("Detections", &tampering_result.visualization),
    ]);
    comparison.save(format!("{}/visualizations/comparison.png", output_dir))?;
    println!(" ✓");

    // Generate report image
    print!("  Generating report image...");
    let report_image = comprehensive_vis.create_report_image();
    report_image.save(format!("{}/report_overview.png", output_dir))?;
    println!(" ✓");

    println!("  ⏱ Time: {:?}", start.elapsed());
    println!();

    // ═══════════════════════════════════════════════════════════════════
    // PHASE 5: REPORT GENERATION
    // ═══════════════════════════════════════════════════════════════════
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ PHASE 5: Report Generation                                 │");
    println!("└─────────────────────────────────────────────────────────────┘");

    let start = Instant::now();

    // Generate JSON report
    print!("  Generating JSON report...");
    let json_report = JsonReport::from(&full_report);
    let json_str = json_report
        .to_json()
        .map_err(|e| image_forensics::error::ForensicsError::AnalysisFailed(e.to_string()))?;
    fs::write(format!("{}/report.json", output_dir), &json_str)?;
    println!(" ✓");

    // Generate detailed JSON with all detections
    print!("  Generating detailed detection report...");
    let detailed_report = generate_detailed_report(
        image_path,
        &full_report,
        &splicing_result,
        &tampering_result,
    );
    fs::write(
        format!("{}/detailed_report.json", output_dir),
        detailed_report,
    )?;
    println!(" ✓");

    // Generate HTML report
    print!("  Generating HTML report...");
    let html_report = generate_html_report(image_path, &full_report, &tampering_result, output_dir);
    fs::write(format!("{}/report.html", output_dir), html_report)?;
    println!(" ✓");

    // Generate text summary
    print!("  Generating text summary...");
    let text_summary = generate_text_summary(image_path, &full_report, &tampering_result);
    fs::write(format!("{}/summary.txt", output_dir), text_summary)?;
    println!(" ✓");

    println!("  ⏱ Time: {:?}", start.elapsed());
    println!();

    // ═══════════════════════════════════════════════════════════════════
    // FINAL SUMMARY
    // ═══════════════════════════════════════════════════════════════════
    print_final_summary(
        &full_report,
        &tampering_result,
        output_dir,
        total_start.elapsed(),
    );

    Ok(())
}

fn print_usage(program: &str) {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║       Image Forensics - Comprehensive Analysis Tool          ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("Usage: {} <image_path> [output_dir]", program);
    println!();
    println!("Arguments:");
    println!("  image_path  - Path to the image to analyze (JPEG, PNG, etc.)");
    println!("  output_dir  - Output directory (default: ./forensics_report)");
    println!();
    println!("Output:");
    println!("  The tool generates a complete forensics report including:");
    println!("  • Error Level Analysis (ELA)");
    println!("  • Copy-Move Forgery Detection");
    println!("  • Noise Pattern Analysis");
    println!("  • JPEG Artifact Analysis");
    println!("  • Splicing Detection");
    println!("  • Comprehensive Tampering Detection");
    println!("  • Visualizations and Heatmaps");
    println!("  • JSON and HTML Reports");
    println!();
    println!("Example:");
    println!("  {} suspicious_image.jpg ./my_report", program);
}

fn print_header() {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     🔍 IMAGE FORENSICS - COMPREHENSIVE ANALYSIS 🔍          ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
}

fn print_analysis_summary(report: &FullAnalysisReport) {
    println!("  ┌─────────────────────────────────────────────────────────┐");
    println!("  │ Core Analysis Results                                   │");
    println!("  ├─────────────────────────────────────────────────────────┤");
    println!("  │ ELA Analysis:                                           │");
    println!(
        "  │   Max Difference:      {:>8.2}                         │",
        report.ela.max_difference
    );
    println!(
        "  │   Mean Difference:     {:>8.2}                         │",
        report.ela.mean_difference
    );
    println!(
        "  │   Suspicious Regions:  {:>8}                         │",
        report.ela.suspicious_regions.len()
    );
    println!("  ├─────────────────────────────────────────────────────────┤");
    println!("  │ Copy-Move Detection:                                    │");
    println!(
        "  │   Matches Found:       {:>8}                         │",
        report.copy_move.matches.len()
    );
    println!(
        "  │   Confidence:          {:>7.1}%                         │",
        report.copy_move.confidence * 100.0
    );
    println!("  ├─────────────────────────────────────────────────────────┤");
    println!("  │ Noise Analysis:                                         │");
    println!(
        "  │   Noise Level:         {:>8.2}                         │",
        report.noise.estimated_noise_level
    );
    println!(
        "  │   Inconsistency:       {:>7.1}%                         │",
        report.noise.inconsistency_score * 100.0
    );
    println!("  ├─────────────────────────────────────────────────────────┤");
    println!("  │ JPEG Analysis:                                          │");
    println!(
        "  │   Quality Estimate:    {:>8}                         │",
        report.jpeg.quality_estimate
    );
    println!(
        "  │   Ghost Detected:      {:>8}                         │",
        if report.jpeg.ghost_detected {
            "Yes"
        } else {
            "No"
        }
    );
    println!(
        "  │   Double Compression:  {:>7.1}%                         │",
        report.jpeg.double_compression_likelihood * 100.0
    );
    println!("  └─────────────────────────────────────────────────────────┘");
    println!();
}

fn print_detection_summary(splicing: &DetectionResult, tampering: &DetectionResult) {
    println!("  ┌─────────────────────────────────────────────────────────┐");
    println!("  │ Detection Results                                       │");
    println!("  ├─────────────────────────────────────────────────────────┤");
    println!("  │ Splicing Detection:                                     │");
    println!(
        "  │   Regions Detected:    {:>8}                         │",
        splicing.manipulations.len()
    );
    println!(
        "  │   Overall Score:       {:>7.1}%                         │",
        splicing.overall_score * 100.0
    );
    println!(
        "  │   Is Manipulated:      {:>8}                         │",
        if splicing.is_manipulated { "Yes" } else { "No" }
    );
    println!("  ├─────────────────────────────────────────────────────────┤");
    println!("  │ Tampering Detection:                                    │");
    println!(
        "  │   Total Detections:    {:>8}                         │",
        tampering.manipulations.len()
    );
    println!(
        "  │   Overall Score:       {:>7.1}%                         │",
        tampering.overall_score * 100.0
    );
    println!(
        "  │   Is Manipulated:      {:>8}                         │",
        if tampering.is_manipulated {
            "Yes"
        } else {
            "No"
        }
    );
    println!("  └─────────────────────────────────────────────────────────┘");

    if !tampering.manipulations.is_empty() {
        println!();
        println!("  Detected Manipulations:");

        let mut by_type: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for m in &tampering.manipulations {
            let type_name = format!("{:?}", m.manipulation_type);
            *by_type.entry(type_name).or_insert(0) += 1;
        }

        for (type_name, count) in by_type {
            println!("    • {}: {} instance(s)", type_name, count);
        }
    }
    println!();
}

fn generate_detailed_report(
    image_path: &str,
    full_report: &FullAnalysisReport,
    splicing: &DetectionResult,
    tampering: &DetectionResult,
) -> String {
    let report = serde_json::json!({
        "image_path": image_path,
        "analysis_timestamp": chrono_lite_timestamp(),
        "tampering_probability": full_report.tampering_ability,
        "ela": {
            "max_difference": full_report.ela.max_difference,
            "mean_difference": full_report.ela.mean_difference,
            "std_deviation": full_report.ela.std_deviation,
            "suspicious_region_count": full_report.ela.suspicious_regions.len(),
        },
        "copy_move": {
            "matches_found": full_report.copy_move.matches.len(),
            "confidence": full_report.copy_move.confidence,
        },
        "noise": {
            "estimated_level": full_report.noise.estimated_noise_level,
            "inconsistency_score": full_report.noise.inconsistency_score,
            "anomalous_regions": full_report.noise.anomalous_regions.len(),
        },
        "jpeg": {
            "quality_estimate": full_report.jpeg.quality_estimate,
            "ghost_detected": full_report.jpeg.ghost_detected,
            "double_compression_likelihood": full_report.jpeg.double_compression_likelihood,
        },
        "splicing_detection": {
            "regions_detected": splicing.manipulations.len(),
            "overall_score": splicing.overall_score,
            "is_manipulated": splicing.is_manipulated,
        },
        "tampering_detection": {
            "total_detections": tampering.manipulations.len(),
            "overall_score": tampering.overall_score,
            "is_manipulated": tampering.is_manipulated,
            "confidence_level": format!("{:?}", tampering.overall_confidence),
            "manipulations": tampering.manipulations.iter().map(|m| {
                serde_json::json!({
                    "type": format!("{:?}", m.manipulation_type),
                    "region": {
                        "x": m.region.x,
                        "y": m.region.y,
                        "width": m.region.width,
                        "height": m.region.height,
                    },
                    "confidence": m.confidence,
                    "confidence_level": format!("{:?}", m.confidence_level),
                    "description": m.description,
                    "evidence": m.evidence,
                })
            }).collect::<Vec<_>>(),
        },
        "metadata": full_report.metadata.as_ref().map(|m| {
            serde_json::json!({
                "camera_make": m.camera_make,
                "camera_model": m.camera_model,
                "software": m.software,
                "datetime": m.date_time,
                "gps_coordinates": m.gps_coordinates,
                "suspicious_indicators": m.suspicious_indicators,
            })
        }),
    });

    serde_json::to_string_pretty(&report).unwrap_or_default()
}

fn generate_html_report(
    image_path: &str,
    full_report: &FullAnalysisReport,
    tampering: &DetectionResult,
    output_dir: &str,
) -> String {
    let tampering_class = if full_report.tampering_ability > 0.7 {
        "high"
    } else if full_report.tampering_ability > 0.4 {
        "medium"
    } else {
        "low"
    };

    let manipulations_html: String = tampering
        .manipulations
        .iter()
        .map(|m| {
            format!(
                r#"
        <div class="manipulation">
            <h4>{:?}</h4>
            <p><strong>Region:</strong> ({}, {}) - {}x{}</p>
            <p><strong>Confidence:</strong> {:.1}% ({:?})</p>
            <p><strong>Description:</strong> {}</p>
            <p><strong>Evidence:</strong></p>
            <ul>
                {}
            </ul>
        </div>
        "#,
                m.manipulation_type,
                m.region.x,
                m.region.y,
                m.region.width,
                m.region.height,
                m.confidence * 100.0,
                m.confidence_level,
                m.description,
                m.evidence
                    .iter()
                    .map(|e| format!("<li>{}</li>", e))
                    .collect::<Vec<_>>()
                    .join("")
            )
        })
        .collect();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Image Forensics Report</title>
    <style>
        * {{ box-sizing: border-box; margin: 0; padding: 0; }}
        body {{ 
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
            background: #1a1a2e; color: #eee; line-height: 1.6; padding: 20px;
        }}
        .container {{ max-width: 1400px; margin: 0 auto; }}
        h1 {{ color: #00d4ff; text-align: center; margin-bottom: 30px; font-size: 2.5em; }}
        h2 {{ color: #00d4ff; border-bottom: 2px solid #00d4ff; padding-bottom: 10px; margin: 30px 0 20px; }}
        h3 {{ color: #ffd700; margin: 20px 0 10px; }}
        .summary-box {{
            background: linear-gradient(135deg, #16213e, #1a1a2e);
            border: 2px solid #00d4ff; border-radius: 15px;
            padding: 30px; margin: 20px 0; text-align: center;
        }}
        .score {{ font-size: 4em; font-weight: bold; }}
        .score.high {{ color: #ff4757; }}
        .score.medium {{ color: #ffa502; }}
        .score.low {{ color: #2ed573; }}
        .grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 20px; }}
        .card {{
            background: #16213e; border-radius: 10px; padding: 20px;
            border: 1px solid #333; transition: transform 0.3s;
        }}
        .card:hover {{ transform: translateY(-5px); border-color: #00d4ff; }}
        .card h3 {{ margin-top: 0; }}
        .stat {{ display: flex; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid #333; }}
        .stat:last-child {{ border-bottom: none; }}
        .stat-value {{ color: #00d4ff; font-weight: bold; }}
        .image-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(400px, 1fr)); gap: 20px; }}
        .image-card {{ background: #16213e; border-radius: 10px; overflow: hidden; }}
        .image-card img {{ width: 100%; height: auto; display: block; }}
        .image-card .caption {{ padding: 15px; text-align: center; color: #888; }}
        .manipulation {{
            background: #0f3460; border-left: 4px solid #ffd700;
            padding: 15px; margin: 10px 0; border-radius: 0 10px 10px 0;
        }}
        .manipulation h4 {{ color: #ffd700; margin-bottom: 10px; }}
        .manipulation ul {{ margin-left: 20px; color: #aaa; }}
        .badge {{
            display: inline-block; padding: 5px 15px; border-radius: 20px;
            font-size: 0.9em; font-weight: bold;
        }}
        .badge.detected {{ background: #ff4757; }}
        .badge.clean {{ background: #2ed573; }}
        footer {{ text-align: center; margin-top: 50px; padding: 20px; color: #666; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>🔍 Image Forensics Report</h1>
        
        <div class="summary-box">
            <h2 style="border: none; margin: 0;">Tampering Probability</h2>
            <div class="score {tampering_class}">{:.1}%</div>
            <p style="margin-top: 20px; font-size: 1.2em;">
                {}
            </p>
            <div style="margin-top: 20px;">
                <span class="badge {}">
                    {}
                </span>
            </div>
        </div>
        
        <h2>📊 Analysis Results</h2>
        <div class="grid">
            <div class="card">
                <h3>Error Level Analysis</h3>
                <div class="stat"><span>Max Difference</span><span class="stat-value">{:.2}</span></div>
                <div class="stat"><span>Mean Difference</span><span class="stat-value">{:.2}</span></div>
                <div class="stat"><span>Std Deviation</span><span class="stat-value">{:.2}</span></div>
                <div class="stat"><span>Suspicious Regions</span><span class="stat-value">{}</span></div>
            </div>
            
            <div class="card">
                <h3>Copy-Move Detection</h3>
                <div class="stat"><span>Matches Found</span><span class="stat-value">{}</span></div>
                <div class="stat"><span>Confidence</span><span class="stat-value">{:.1}%</span></div>
            </div>
            
            <div class="card">
                <h3>Noise Analysis</h3>
                <div class="stat"><span>Noise Level</span><span class="stat-value">{:.2}</span></div>
                <div class="stat"><span>Inconsistency</span><span class="stat-value">{:.1}%</span></div>
                <div class="stat"><span>Anomalous Regions</span><span class="stat-value">{}</span></div>
            </div>
            
            <div class="card">
                <h3>JPEG Analysis</h3>
                <div class="stat"><span>Quality Estimate</span><span class="stat-value">{}</span></div>
                <div class="stat"><span>Ghost Detected</span><span class="stat-value">{}</span></div>
                <div class="stat"><span>Double Compression</span><span class="stat-value">{:.1}%</span></div>
            </div>
        </div>
        
        <h2>🖼️ Visualizations</h2>
        <div class="image-grid">
            <div class="image-card">
                <img src="visualizations/ela.png" alt="ELA Analysis">
                <div class="caption">Error Level Analysis</div>
            </div>
            <div class="image-card">
                <img src="visualizations/copy_move.png" alt="Copy-Move Detection">
                <div class="caption">Copy-Move Detection</div>
            </div>
            <div class="image-card">
                <img src="visualizations/noise.png" alt="Noise Analysis">
                <div class="caption">Noise Pattern Analysis</div>
            </div>
            <div class="image-card">
                <img src="visualizations/combined.png" alt="Combined Analysis">
                <div class="caption">Combined Analysis</div>
            </div>
        </div>
        
        <h2>⚠️ Detected Manipulations</h2>
        {}
        {}
        
        <footer>
            <p>Generated by Image Forensics Library</p>
            <p>Image analyzed: {}</p>
        </footer>
    </div>
</body>
</html>"#,
        full_report.tampering_ability * 100.0,
        if full_report.tampering_ability > 0.7 {
            "High probability of image manipulation detected"
        } else if full_report.tampering_ability > 0.4 {
            "Some signs of potential manipulation detected"
        } else {
            "No significant signs of manipulation detected"
        },
        if tampering.is_manipulated {
            "detected"
        } else {
            "clean"
        },
        if tampering.is_manipulated {
            "Manipulation Detected"
        } else {
            "Appears Authentic"
        },
        full_report.ela.max_difference,
        full_report.ela.mean_difference,
        full_report.ela.std_deviation,
        full_report.ela.suspicious_regions.len(),
        full_report.copy_move.matches.len(),
        full_report.copy_move.confidence * 100.0,
        full_report.noise.estimated_noise_level,
        full_report.noise.inconsistency_score * 100.0,
        full_report.noise.anomalous_regions.len(),
        full_report.jpeg.quality_estimate,
        if full_report.jpeg.ghost_detected {
            "Yes"
        } else {
            "No"
        },
        full_report.jpeg.double_compression_likelihood * 100.0,
        if tampering.manipulations.is_empty() {
            "<p style='color: #2ed573; padding: 20px;'>No manipulations detected.</p>".to_string()
        } else {
            manipulations_html
        },
        "",
        image_path,
    )
}

fn generate_text_summary(
    image_path: &str,
    full_report: &FullAnalysisReport,
    tampering: &DetectionResult,
) -> String {
    let mut summary = String::new();

    summary.push_str("═══════════════════════════════════════════════════════════════\n");
    summary.push_str("                    IMAGE FORENSICS REPORT                      \n");
    summary.push_str("═══════════════════════════════════════════════════════════════\n\n");

    summary.push_str(&format!("Image: {}\n", image_path));
    summary.push_str(&format!("Analysis Date: {}\n\n", chrono_lite_timestamp()));

    summary.push_str("───────────────────────────────────────────────────────────────\n");
    summary.push_str("                         VERDICT                               \n");
    summary.push_str("───────────────────────────────────────────────────────────────\n\n");

    summary.push_str(&format!(
        "Tampering Probability: {:.1}%\n",
        full_report.tampering_ability * 100.0
    ));
    summary.push_str(&format!(
        "Status: {}\n\n",
        if tampering.is_manipulated {
            "⚠️  MANIPULATION DETECTED"
        } else {
            "✅ APPEARS AUTHENTIC"
        }
    ));

    summary.push_str("───────────────────────────────────────────────────────────────\n");
    summary.push_str("                    ANALYSIS DETAILS                           \n");
    summary.push_str("───────────────────────────────────────────────────────────────\n\n");

    summary.push_str("ERROR LEVEL ANALYSIS:\n");
    summary.push_str(&format!(
        "  • Max Difference:      {:.2}\n",
        full_report.ela.max_difference
    ));
    summary.push_str(&format!(
        "  • Mean Difference:     {:.2}\n",
        full_report.ela.mean_difference
    ));
    summary.push_str(&format!(
        "  • Suspicious Regions:  {}\n\n",
        full_report.ela.suspicious_regions.len()
    ));

    summary.push_str("COPY-MOVE DETECTION:\n");
    summary.push_str(&format!(
        "  • Matches Found:       {}\n",
        full_report.copy_move.matches.len()
    ));
    summary.push_str(&format!(
        "  • Confidence:          {:.1}%\n\n",
        full_report.copy_move.confidence * 100.0
    ));

    summary.push_str("NOISE ANALYSIS:\n");
    summary.push_str(&format!(
        "  • Noise Level:         {:.2}\n",
        full_report.noise.estimated_noise_level
    ));
    summary.push_str(&format!(
        "  • Inconsistency:       {:.1}%\n\n",
        full_report.noise.inconsistency_score * 100.0
    ));

    summary.push_str("JPEG ANALYSIS:\n");
    summary.push_str(&format!(
        "  • Quality Estimate:    {}\n",
        full_report.jpeg.quality_estimate
    ));
    summary.push_str(&format!(
        "  • Ghost Detected:      {}\n",
        if full_report.jpeg.ghost_detected {
            "Yes"
        } else {
            "No"
        }
    ));
    summary.push_str(&format!(
        "  • Double Compression:  {:.1}%\n\n",
        full_report.jpeg.double_compression_likelihood * 100.0
    ));

    if !tampering.manipulations.is_empty() {
        summary.push_str("───────────────────────────────────────────────────────────────\n");
        summary.push_str("                  DETECTED MANIPULATIONS                       \n");
        summary.push_str("───────────────────────────────────────────────────────────────\n\n");

        for (i, m) in tampering.manipulations.iter().enumerate() {
            summary.push_str(&format!("{}. {:?}\n", i + 1, m.manipulation_type));
            summary.push_str(&format!(
                "   Region: ({}, {}) - {}x{}\n",
                m.region.x, m.region.y, m.region.width, m.region.height
            ));
            summary.push_str(&format!("   Confidence: {:.1}%\n", m.confidence * 100.0));
            summary.push_str(&format!("   {}\n", m.description));
            if !m.evidence.is_empty() {
                summary.push_str("   Evidence:\n");
                for e in &m.evidence {
                    summary.push_str(&format!("     • {}\n", e));
                }
            }
            summary.push_str("\n");
        }
    }

    summary.push_str("═══════════════════════════════════════════════════════════════\n");
    summary.push_str("                    END OF REPORT                              \n");
    summary.push_str("═══════════════════════════════════════════════════════════════\n");

    summary
}

fn print_final_summary(
    full_report: &FullAnalysisReport,
    tampering: &DetectionResult,
    output_dir: &str,
    elapsed: std::time::Duration,
) {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                    ANALYSIS COMPLETE                         ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Verdict
    let verdict_symbol = if full_report.tampering_ability > 0.7 {
        "🔴"
    } else if full_report.tampering_ability > 0.4 {
        "🟡"
    } else {
        "🟢"
    };

    println!(
        "  {} TAMPERING PROBABILITY: {:.1}%",
        verdict_symbol,
        full_report.tampering_ability * 100.0
    );
    println!();

    if tampering.is_manipulated {
        println!("  ⚠️  This image shows signs of manipulation!");
        println!(
            "  📊 {} manipulation(s) detected",
            tampering.manipulations.len()
        );
    } else {
        println!("  ✅ No significant signs of manipulation detected.");
        println!("  📊 Image appears to be authentic");
    }

    println!();
    println!("  ⏱️  Total analysis time: {:?}", elapsed);
    println!();
    println!("  📁 Output files:");
    println!(
        "     • {}/report.html          - Interactive HTML report",
        output_dir
    );
    println!(
        "     • {}/report.json          - JSON report data",
        output_dir
    );
    println!(
        "     • {}/detailed_report.json - Detailed analysis data",
        output_dir
    );
    println!("     • {}/summary.txt          - Text summary", output_dir);
    println!(
        "     • {}/report_overview.png  - Visual report overview",
        output_dir
    );
    println!(
        "     • {}/visualizations/      - All visualization images",
        output_dir
    );
    println!(
        "     • {}/analysis/            - Individual analysis outputs",
        output_dir
    );
    println!();
    println!("══════════════════════════════════════════════════════════════");
}

// Simple timestamp function (avoids chrono dependency)
fn chrono_lite_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    let secs = duration.as_secs();
    let days = secs / 86400;
    let years = 1970 + days / 365;
    let remaining_days = days % 365;
    let months = remaining_days / 30 + 1;
    let day = remaining_days % 30 + 1;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        years, months, day, hours, minutes, seconds
    )
}
