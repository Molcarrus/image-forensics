use image_forensics::{analysis::shadow_analysis::ShadowAnalyzer, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/shadow.jpg")?;

    let shadow_analyzer = ShadowAnalyzer::new();
    let shadow_result = shadow_analyzer.analyze(&image)?;

    println!(
        "Shadow regions detected: {}",
        shadow_result.shadow_regions.len()
    );
    println!(
        "Dominant light direction: {:.1}",
        shadow_result.dominant_light_direction.to_degrees()
    );
    println!(
        "Direction confidence: {:.1}%",
        shadow_result.dominant_direction_confidence * 100.0
    );
    println!(
        "Estimated light sources: {}",
        shadow_result.estimated_light_sources
    );
    println!(
        "Consistency score: {:.1}%",
        shadow_result.consistency_score * 100.0
    );
    println!(
        "Inconsistent shadow regions: {}",
        shadow_result.inconsistent_regions.len()
    );
    println!(
        "Manipulation probability: {:.1}%",
        shadow_result.manipulation_probability * 100.0
    );

    shadow_result.shadow_mask.save("output/shadow_mask.png")?;
    shadow_result
        .direction_map
        .save("output/shadow_directions.png")?;

    Ok(())
}
