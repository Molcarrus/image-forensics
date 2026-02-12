use image_forensics::{
    detection::{Detector, splicing::SplicingDetector},
    error::Result,
};

fn main() -> Result<()> {
    let image = image::open("evidences/splicing.png")?;

    let splicing_detector = SplicingDetector::new();
    let splicing_result = splicing_detector.detect(&image)?;

    println!("{:?}", splicing_detector.description());
    println!("{:?}", splicing_detector.name());

    println!(
        "Manipulation Possibility: {:?}",
        splicing_result.is_manipulated
    );
    println!(
        "Number of manipulated regions: {:?}",
        splicing_result.manipulations.len()
    );
    println!(
        "Confidence: {:.2}%",
        splicing_result.overall_confidence.to_score() * 100.0
    );
    println!("Score: {:.2}%", splicing_result.overall_score * 100.0);
    println!("Summary: {:?}", splicing_result.summary);

    splicing_result
        .visualization
        .save("output/splicing_analysis.png")?;

    Ok(())
}
