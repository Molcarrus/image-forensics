use image_forensics::{analysis::dct_analysis::DctAnalyzer, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/shadow.png")?;
    
    let dct_analyzer = DctAnalyzer::new();
    let dct_result = dct_analyzer.analyze(&image)?;
    
    println!("Primary JPEG quality: {}", dct_result.primary_quality);
    println!("Double compression probability: {:.1}%", dct_result.double_compression_probability * 100.0);
    if let Some(secondary) = dct_result.secondary_quality {
        println!("Secondary quality estimate: {}", secondary);
    }
    println!("Histogram periodicity: {:.3}", dct_result.histogram_periodicity);
    println!("Anomalous DCT regions: {}", dct_result.anomalous_regions.len());
    
    dct_result.dct_energy_map.save("output/dct_energy.png")?;
    dct_result.block_artifact_map.save("output/dct_artifacts.png")?;
    
    Ok(())
}