---
layout: docs.liquid
title: Getting Started
description: Install the crate, run one detector, then run the combined analysis.
---

## 1. Add the dependency

The crate is not published to crates.io, so depend on the Git repository:

```toml
[dependencies]
image-forensics = { git = "{{ site.data.repo.url }}" }
image = "0.25"
```

You will want `image` alongside it: every analyzer takes an `image::DynamicImage`,
and you need `image::open` to produce one.

## 2. Run a single detector

Start with copy-move, which is the easiest to verify by eye — the result carries
a visualization with the matched regions boxed and joined.

```rust
use image_forensics::{analysis::copy_move::CopyMoveDetector, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/copy_move.png")?;

    // block_size, similarity_threshold, min_distance
    let detector = CopyMoveDetector::new(16, 0.95, 50)?;
    let result = detector.detect(&image)?;

    println!("Matching regions found: {}", result.matches.len());
    println!("Confidence: {:.1}%", result.confidence * 100.0);

    for (i, pair) in result.matches.iter().enumerate() {
        println!(
            "{}. source ({}, {}) -> target ({}, {}) | similarity {:.1}%",
            i + 1,
            pair.source.x,
            pair.source.y,
            pair.target.x,
            pair.target.y,
            pair.similarity * 100.0,
        );
    }

    result.visualization.save("output/copy_move.png")?;
    Ok(())
}
```

Three parameters, and all three matter:

- `block_size` (4–64) is the side of the compared square. Smaller finds smaller
  copies but costs more and produces more false pairs.
- `similarity_threshold` (0–1) is the minimum correlation between two blocks'
  DCT features. `0.95` is strict; `0.90` is a reasonable floor.
- `min_distance` is how far apart two blocks must be before a match counts, in
  pixels. This exists to suppress the trivial self-matches that any textured
  region produces with its own neighbours.

`CopyMoveDetector::new` returns `Err(ForensicsError::InvalidParameter)` for a
block size outside 4–64, so it is fallible — note the `?`.

## 3. Run the combined analysis

`ForensicsAnalyzer` bundles ELA, copy-move, noise and JPEG analysis, plus
metadata when it was constructed from a path:

```rust
use image_forensics::{AnalysisConfig, ForensicsAnalyzer, error::Result};

fn main() -> Result<()> {
    let analyzer = ForensicsAnalyzer::new("evidences/splicing.png")?
        .with_config(AnalysisConfig {
            ela_quality: 95,
            block_size: 16,
            similarity_threshold: 0.92,
            min_match_distance: 50,
        });

    let report = analyzer.full_analysis()?;

    println!("Tampering probability: {:.1}%", report.tampering_probability * 100.0);
    println!("ELA max difference:    {:.2}", report.ela.max_difference);
    println!("Copy-move matches:     {}", report.copy_move.matches.len());
    println!("Noise inconsistency:   {:.2}", report.noise.inconsistency_score);
    println!("JPEG ghost detected:   {}", report.jpeg.ghost_detected);

    if let Some(metadata) = &report.metadata {
        for indicator in &metadata.suspicious_indicators {
            println!("  metadata: {indicator}");
        }
    }

    Ok(())
}
```

`ForensicsAnalyzer::from_image` accepts a `DynamicImage` directly. Metadata
extraction then fails with `ForensicsError::MetadataError`, because EXIF lives
in the file container rather than the decoded pixels.

## 4. Emit a report

```rust
use image_forensics::report::JsonReport;

let json = JsonReport::from(&report).to_json().unwrap();
std::fs::write("output/report.json", json)?;
```

## 5. Run the bundled examples

Every module has a runnable example in the repository:

```bash
cargo run --release --example copy_move
```

Use `--release`. These are numerically heavy loops and a debug build is
roughly an order of magnitude slower. Outputs land in `output/`, which must
already exist.

Available examples: `benford_analysis`, `cfa_analysis`, `chromatic_aberration`,
`copy_move`, `dct_analysis`, `ela_analysis`, `histogram_analysis`,
`pca_analysis`, `prnu_analysis`, `resampling_analysis`, `shadow_analysis`,
`splicing_detection`.

## Reading the output

<div class="warning">

A single high score is not evidence. Every module in this crate reports a
number derived from thresholds chosen by hand, and ordinary processing —
saving a JPEG twice, resizing for the web, a screenshot, a phone's own noise
reduction — trips most of them. Treat a result as a pointer to a region worth
examining, and give weight to *agreement across independent modules*, not to
any one score.

</div>

Next: [Configuration](/docs/configuration/) explains what each tunable does,
and [Analysis Modules](/docs/modules/) covers what each method can and cannot
detect.
