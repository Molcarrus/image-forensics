---
layout: docs.liquid
title: Benford Analysis
description: Tests the leading digits of DCT coefficients against Benford's Law, which natural images follow and manipulated regions often do not.
---

## What it does

Benford's Law says that in many natural datasets the leading significant digit
`d` occurs with probability `log₁₀(1 + 1/d)` — 1 leads about 30% of the time,
9 under 5%. The AC coefficients of a JPEG's DCT follow it closely. Editing,
requantisation and synthetic content perturb the distribution.

The module computes the DCT of every aligned 8×8 block, collects the AC
coefficients, measures the first-digit distribution against the Benford
expectation with a chi-square statistic, and repeats the test per tile to
localise departures.

## Usage

```rust
use image_forensics::analysis::benford_analysis::{BenfordAnalyzer, BenfordConfig};

let analyzer = BenfordAnalyzer::with_config(BenfordConfig {
    block_size: 64,
    chi_square_threshold: 15.0,
    min_samples: 100,
});

let result = analyzer.analyze(&image)?;

println!("expected  {:?}", result.expected_distribution);
println!("observed  {:?}", result.global_distribution);
println!("chi²      {:.2}", result.global_chi_square);
println!("conformity {:.2}", result.conformity_score);

result.deviation_map.save("output/benford.png")?;
```

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `block_size` | `64` | Tile over which each local chi-square is computed. Tiles are swept at 50% overlap |
| `chi_square_threshold` | `15.0` | Tiles above this are flagged |
| `min_samples` | `100` | Tiles with fewer usable coefficients score 0 rather than a noisy statistic |

</div>

Requires the image to be at least `block_size` in both dimensions.

## Results

```rust
pub struct BenfordAnalysisResult {
    pub global_distribution: [f64; 9],
    pub expected_distribution: [f64; 9],
    pub global_chi_square: f64,
    pub deviation_map: GrayImage,
    pub anomalous_regions: Vec<SRegion>,
    pub conformity_score: f64,          // [0, 1], higher = closer to Benford
    pub manipulation_probability: f64,  // [0, 1]
}
```

Only AC coefficients with magnitude at least 1 are used. The DC term is
excluded: it carries block brightness rather than compression structure.

<div class="note">

The 8×8 DCT grid is now computed once and shared between the global
distribution and the per-tile sweep. The two passes previously recomputed
overlapping blocks from scratch, transforming much of the image several times
over.

The region-merging helper also had a transposed field — merged heights were
built from `a.y + a.width` — so on non-square images the reported boxes ran off
the bottom edge.

</div>

## Interpreting the output

- **`conformity_score` near 1** means the coefficients follow Benford closely,
  which is the normal state for a photograph compressed once.
- **`global_chi_square` above ~15** suggests the image as a whole departs from
  the law. Double compression is the most common cause.
- **`deviation_map`** is the useful output: bright tiles are locally
  non-conforming. A contiguous bright block is more interesting than scattered
  bright tiles.

## Limitations

<div class="warning">

- **Benford applies to the DCT coefficients of lossy-compressed natural
  images.** It is a weak test on lossless input, on synthetic or graphic
  content, and on images with large flat areas that produce few coefficients
  above the magnitude cutoff.
- **Double compression breaks conformity by itself.** A departure indicates
  requantisation, which is not the same as editing.
- **Small tiles give noisy statistics.** The `min_samples` guard exists for
  this; lowering it produces confident-looking nonsense.
- **A conforming image is not a clean image.** A well-executed edit followed by
  a single uniform resave restores conformity.

</div>

## See also

- [DCT Analysis](/docs/modules/dct/) — the coefficients this operates on
- [JPEG Analysis](/docs/modules/jpeg/)
- [Histogram Analysis](/docs/modules/histogram/) — pixel-domain statistics
