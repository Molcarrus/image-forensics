use image_forensics::{analysis::resampling_detection::ResamplingDetector, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/copy_move.png")?;

    let resampling_detector = ResamplingDetector::new();
    let resampling_result = resampling_detector.detect(&image)?;

    println!(
        "Resampled regions: {}",
        resampling_result.resampled_regions.len()
    );
    println!(
        "Periodic patterns: {}",
        resampling_result.periodic_patterns.len()
    );
    // println!("Estimated factor: {:.2}", resampling_result.estimated_factor);
    println!(
        "Resampling probability: {:.1}%",
        resampling_result.resampling_probability * 100.0
    );

    resampling_result
        .p_map
        .save("output/resampling_p_map.png")?;
    resampling_result
        .probability_map
        .save("output/resampling_prob_map.png")?;

    Ok(())
}
