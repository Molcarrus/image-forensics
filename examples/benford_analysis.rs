use image_forensics::{analysis::benford_analysis::BenfordAnalyzer, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/shadow.png")?;

    let benford_analyzer = BenfordAnalyzer::new();
    let benford_result = benford_analyzer.analyze(&image)?;

    println!(
        "Anomalous regions: {}",
        benford_result.anomalous_regions.len()
    );
    println!(
        "Expected distribution: {:?}",
        benford_result.expected_distribution
    );
    println!(
        "Global distribution: {:?}",
        benford_result.global_distribution
    );
    println!("Global chi square: {:.2}", benford_result.global_chi_square);
    println!(
        "Conformity score: {:.2}",
        benford_result.conformity_score * 100.0
    );
    println!(
        "Manipulation probability: {:.2}",
        benford_result.manipulation_probability * 100.0
    );

    benford_result
        .deviation_map
        .save("output/benford_deviation.png")?;

    Ok(())
}
