use image_forensics::{analysis::chromatic_aberration::ChromaticAberrationAnalyzer, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/copy_move.png")?;
    
    let ca_analyzer = ChromaticAberrationAnalyzer::new();
    let ca_result = ca_analyzer.analyze(&image)?;
    
    println!("Measurements: {}", ca_result.measurements.len());
    println!("Consistency score: {:.1}%", ca_result.consistency_score * 100.0);
    println!("Manipulation probability: {:.1}%", ca_result.manipulation_probability * 100.0);
    
    if let Some((cx, cy)) = ca_result.optical_center {
        println!("Optical center: ({:.1}, {:.1})", cx, cy);
    }
    
    if let Some(ref model) = ca_result.radial_model {
        println!("Radial model fit: {:.2}", model.fit_quality);
        println!("Red coefficient: {:.4}", model.k_red);
        println!("Blue coefficient: {:.4}", model.k_blue);
    }
    
    ca_result.visualization.save("output/chromatic_aberration.png")?;
    ca_result.aberration_map.save("output/aberration_map.png")?;
    
    Ok(())
}