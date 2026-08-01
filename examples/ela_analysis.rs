use image_forensics::{analysis::ela::ElaAnalyzer, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/splicing.png")?;

    for quality in [95, 90, 85] {
        println!("Analyzing at quality {quality}...");

        // `with_threshold` is gone: the field it set was never read. The
        // suspicious-region cutoff is derived from the difference distribution
        // (mean + 2 sigma); `with_block_size` controls the granularity.
        let ela_analyzer = ElaAnalyzer::new(quality)
            .with_amplification(15.0)
            .with_block_size(16);

        let ela_result = ela_analyzer.analyze(&image)?;

        let ela_output = format!("output/ela_q{quality}.png");
        ela_result.save(&ela_output)?;

        println!("  Max difference:      {:.2}", ela_result.max_difference);
        println!("  Mean difference:     {:.2}", ela_result.mean_difference);
        println!("  Std deviation:       {:.2}", ela_result.std_deviation);
        println!(
            "  Suspicious regions:  {}",
            ela_result.suspicious_regions.len()
        );
        println!("  Output:              {ela_output}");
        println!();
    }

    Ok(())
}
