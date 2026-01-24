use image_forensics::{
    analysis::cfa_analysis::{CfaAnalyzer, CfaConfig, CfaPattern},
    error::Result,
};

fn main() -> Result<()> {
    let image = image::open("evidences/copy_move.png")?;

    let cfa_config = CfaConfig {
        expected_pattern: CfaPattern::RGGB,
        ..Default::default()
    };
    let cfa_analyzer = CfaAnalyzer::with_config(cfa_config);
    let cfa_result = cfa_analyzer.analyze(&image)?;

    println!("Dominant pattern: {:?}", cfa_result.dominant_pattern);
    println!(
        "Pattern confidence: {:.1}%",
        cfa_result.pattern_confidence * 100.0
    );
    println!(
        "Consistency score: {:.1}%",
        cfa_result.consistency_score * 100.0
    );
    println!(
        "Inconsistent regions: {}",
        cfa_result.inconsistent_regions.len()
    );
    println!(
        "Manipulation probability: {:.1}%",
        cfa_result.manipulation_probability * 100.0
    );
    println!("Pattern stats: ");
    println!("  RGGB: {}", cfa_result.pattern_stats.rggb_count);
    println!("  BGGR: {}", cfa_result.pattern_stats.bggr_count);
    println!("  GRBG: {}", cfa_result.pattern_stats.grbg_count);
    println!("  GBRG: {}", cfa_result.pattern_stats.gbrg_count);

    cfa_result.artifact_map.save("output/cfa_artifacts.png")?;
    cfa_result
        .consistency_map
        .save("output/cfa_consistency.png")?;

    Ok(())
}
