# Image Forensics

A Rust library for performing digital image forensics. This crate provides a collection of tools and algorithms to detect manipulations, forgeries, and inconsistencies in images, such as tampering, splicing, copy-move forgeries, and more. It leverages various techniques from computer vision and signal processing to analyze image artifacts, metadata, and statistical properties.

## Features

The library includes the following modules:

| Module | Description |
|--------|-------------|
| **Benford's Law Analysis** | Detects anomalies in the distribution of leading digits in image data, which can indicate compression or manipulation. |
| **CFA (Color Filter Array) Analysis** | Examines the color filter array patterns to identify inconsistencies from editing tools. |
| **Chromatic Aberration Analysis** | Analyzes lens distortions and color fringing to spot forged regions. |
| **Copy-Move Detection** | Identifies duplicated regions within an image, a common forgery technique. |
| **DCT (Discrete Cosine Transform) Analysis** | Inspects JPEG compression artifacts in the frequency domain. |
| **ELA (Error Level Analysis)** | Highlights areas with different compression levels, revealing edits. |
| **JPEG Analysis** | General analysis of JPEG-specific artifacts and quantization tables. |
| **Luminance Gradient Analysis** | Checks for lighting inconsistencies via gradient maps. |
| **Noise Analysis** | Examines noise patterns for irregularities caused by manipulation. |
| **PCA (Principal Component Analysis)** | Applies dimensionality reduction to detect patterns in noise or other features. |
| **PRNU (Photo Response Non-Uniformity) Analysis** | Uses sensor noise fingerprints to verify image authenticity. |
| **Shadow Analysis** | Detects inconsistencies in shadows and lighting directions. |
| **Splicing Detection** | Identifies composited elements from different sources. |
| **Tampering Detection** | General-purpose detection of image alterations. |
| **Metadata Analysis** | Extracts and analyzes EXIF and other metadata for tampering clues. |
| **Report Generation** | Tools for visualizing results and generating forensic reports. |
| **Image Utilities** | Helper functions for image loading, processing, and manipulation. |

## Installation

Add this crate to your `Cargo.toml`:

```toml
[dependencies]
image-forensics = { git = "https://github.com/Molcarrus/image-forensics.git" }
```

Note: Since this is a Git dependency, you can also clone the repository and build it locally.

## Usage

### Basic Example

First, import the library in your Rust code:

```rust
use image_forensics::{analysis::copy_move::CopyMoveDetector, error::Result};
```

Load an image and perform copy move analysis:

```rust
fn main() -> Result<()> {
    // Load the image
    let image = image::open("path/to/image.jpg")?;

    let copy_move_detector = CopyMoveDetector::new(
        16,     // block_size
        0.92,   // similarity_threshold
        50      // min_distance
    )?;
    
    // Use the analyze on the detector to get the result 
    let copy_move_result = copy_move_detector.analyze(&image)?;
    
    // Save the output analysis image 
    copy_move_result.visualization.save("path/to/output.png")?;
    
    // Print the analysis however you want 
    println!("Matching regions found: {}", copy_move_result.matches.len());
    println!("Confidence: {:.1}%", copy_move_result.confidence * 100.0);
    
    if !copy_move_result.matches.is_empty() {
        println!("Detected matches: ");
        for (i, match_pair) in copy_move_result.iter().enumerate() {
            println(
                "{}, Source, ({}, {}) -> Target ({}, {}) | Similarity: {:.1}%",
                i + 1,
                match_pair.source.x,
                match_pair.source.y,
                match_pair.target.x,
                match_pair.target.y,
                match_pair.similarity * 100.0
            );
        }
    }

    Ok(())
}
```

Output: 
![](sample_output/copy_move_result.png)

## Dependencies

This crate relies on external crates such as `image` for image processing. Check `Cargo.toml` for the full list.

## Building and Testing

Clone the repository:

```bash
git clone https://github.com/Molcarrus/image-forensics.git
cd image-forensics
cargo build
cargo run --release --example <example>
```

## Contributing

Contributions are welcome! Please open an issue or submit a pull request for bug fixes, new features, or improvements.