use image_forensics::{
    analysis::histogram_analysis::{HistogramAnalyzer, HistogramAnomaly},
    error::Result,
};

fn main() -> Result<()> {
    let image = image::open("evidences/histogram.webp")?;

    let histogram_analyzer = HistogramAnalyzer::new();
    let histogram_result = histogram_analyzer.analyze(&image)?;

    println!("Anomalies found: {:?}", histogram_result.anomalies.len());
    println!(
        "Manipulation probability: {:.1}%",
        histogram_result.manipulation_probability * 100.0
    );

    for anomaly in &histogram_result.anomalies {
        match anomaly {
            HistogramAnomaly::Gap { count, .. } => {
                println!("{} hisogram gaps detected", count);
            }
            HistogramAnomaly::CombPattern { strength, .. } => {
                println!("Comb pattern (strength:  {:.3}", strength);
            }
            _ => println!("{:?}", anomaly),
        }
    }

    let stacked = histogram_analyzer.render_rgb_histograms(&histogram_result);
    stacked.save("output/histogram_rgb_stacked.png")?;

    let overlaid = histogram_analyzer.render_rgb_histograms_overlaid(&histogram_result);
    overlaid.save("output/histogram_rgb_overlaid.png")?;

    histogram_result
        .gaps_map
        .save("output/histogram_gaps_map.png")?;

    Ok(())
}
