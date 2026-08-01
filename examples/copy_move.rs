use image_forensics::{analysis::copy_move::CopyMoveDetector, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/copy_move.png")?;

    let copy_move_detector = CopyMoveDetector::new(16, 0.92, 50)?;

    let copy_move_result = copy_move_detector.detect(&image)?;

    copy_move_result
        .visualization
        .save("output/copy_move_result.png")?;

    println!("Matching regions found: {}", copy_move_result.matches.len());
    println!("Confidence: {:.1}%", copy_move_result.confidence * 100.0);

    if !copy_move_result.matches.is_empty() {
        println!("Detected matches:");
        for (i, match_pair) in copy_move_result.matches.iter().take(5).enumerate() {
            println!(
                "  {}, Source: ({}, {}) -> Target ({}, {}) | Similarity: {:.1}%",
                i + 1,
                match_pair.source.x,
                match_pair.source.y,
                match_pair.target.x,
                match_pair.target.y,
                match_pair.similarity * 100.0
            );
        }

        if copy_move_result.matches.len() > 5 {
            println!(
                "  ... and {} more matches",
                copy_move_result.matches.len() - 5
            );
        }
    }

    Ok(())
}
