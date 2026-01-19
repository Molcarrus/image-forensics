use image_forensics::{analysis::ela::ElaAnalyzer, error::Result};
use imageproc::drawing::Canvas;

fn main() -> Result<()> {
    let image = image::open("evidences/copy_move.png")?;
    let (width, height) = image.dimensions();
    
    for quality in [95, 90, 85] {
        println!("Analyzing at quality {}... ", quality);
        
        let ela_analyzer = ElaAnalyzer::new(quality)
            .with_amplification(15.0)
            .with_threshold(25.0);
        
        let ela_result = ela_analyzer.analyze(&image)?;
        
        let ela_output = format!("output/ela_q{}.png", quality);
        ela_result.save(&ela_output)?;
        
        println!("  Max difference: {:.2}", ela_result.max_difference);
        println!("  Mean difference: {:2}", ela_result.mean_difference);
        println!("  Std deviation: {:.2}", ela_result.std_deviation);
        println!("  Suspicious regions: {}", ela_result.suspicious_regions.len());
        println!("  Output: {}", ela_output);
        println!();
    }
    
    Ok(())
}