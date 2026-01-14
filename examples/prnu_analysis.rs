use image_forensics::{analysis::prnu_analysis::PrnuAnalyzer, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/shadow.png")?;
    
    let prnu_analyzer = PrnuAnalyzer::new();
    let prnu_result = prnu_analyzer.analyze(&image)?;
    
    println!("PRNU consistency score: {:.1}%", prnu_result.consistency_score * 100.0);
    println!("Manipulation probability: {:.1}%", prnu_result.manipulation_probability * 100.0);
    println!("Inconsistent regions: {}", prnu_result.inconsistent_regions.len());
    println!("PRNU Statistics:");
    println!("  Mean: {:.2}", prnu_result.prnu_statistics.mean);
    println!("  Standard Deviation: {:.2}", prnu_result.prnu_statistics.std_dev);
    println!(" Energy: {:.2}", prnu_result.prnu_statistics.energy);
    
    prnu_result.prnu_pattern.save("output/prnu_pattern.png")?;
    prnu_result.correlation_map.save("output/prnu_correlation.png")?;
    
    Ok(())
}