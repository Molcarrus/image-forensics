use image_forensics::{analysis::pca_analysis::PcaAnalyzer, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/copy_move.png")?;

    let pca_analyzer = PcaAnalyzer::new();
    let pca_result = pca_analyzer.analyze(&image)?;

    println!(
        "Overall anomaly score: {:.2}",
        pca_result.overall_anomaly_score
    );
    println!(
        "Manipulation probability: {:.1}%",
        pca_result.manipulation_probability * 100.0
    );
    println!("Anomalous regions: {}", pca_result.anomalous_regions.len());
    println!("Variance ratios: {:?}", pca_result.variance_ratios);

    pca_result.anomaly_map.save("output/pca_anomaly.png")?;
    pca_result.pc1_map.save("output/pca_pc1.png")?;
    pca_result.pc2_map.save("output/pca_pc2.png")?;
    pca_result.pc3_map.save("output/pca_pc3.png")?;

    Ok(())
}
