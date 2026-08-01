---
layout: docs.liquid
title: Noise Analysis
description: Estimates the sensor noise floor and flags blocks whose local variance departs from it.
---

## What it does

Every camera lays down a roughly uniform noise floor across the frame. Content
from a different source — a different camera, a different ISO, a render, or a
region that has been denoised or blurred — carries a different amount of noise.

The module extracts a noise residual (image minus a Gaussian blur), estimates
the global noise level robustly, then flags blocks whose local standard
deviation is far above or far below that level.

## Usage

```rust
use image_forensics::analysis::noise::NoiseAnalyzer;

let analyzer = NoiseAnalyzer::new()
    .with_block_size(16)   // default 16
    .with_sensitivity(2.0); // default 2.0

let result = analyzer.analyze(&image)?;

println!("noise level  {:.2}", result.estimated_noise_level);
println!("inconsistency {:.2}", result.inconsistency_score);
println!("regions {}", result.anomalous_regions.len());

result.noise_map.save("output/noise.png")?;
```

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `block_size` | `16` | Tile size for both the local-variance window and the anomaly sweep |
| `sensitivity` | `2.0` | A block is flagged above `noise * sensitivity` or below `noise / sensitivity`. Lower is stricter |

</div>

## Results

```rust
pub struct NoiseResult {
    pub noise_map: GrayImage,
    pub local_variance_map: GrayImage,
    pub inconsistency_score: f64,   // fraction of blocks flagged, [0, 1]
    pub estimated_noise_level: f64,
    pub anomalous_regions: Vec<SRegion>,
}
```

`estimated_noise_level` is the median absolute deviation of the residual scaled
by 1.4826 — the MAD-to-sigma factor for a Gaussian. The median is used rather
than the mean precisely so that a large tampered region cannot drag the
baseline toward itself.

`inconsistency_score` is simply the fraction of blocks flagged, so it is a
proportion of the image, not a probability of tampering.

## Interpreting the output

Look at `local_variance_map`. An authentic photograph shows variance tracking
*texture* — high in detailed areas, low in sky. What matters is a region whose
variance breaks that relationship: a detailed object sitting in a low-variance
patch, or a smooth area with unexpectedly high variance.

- **Lower-than-baseline** noise suggests denoising, blurring, or content from a
  cleaner source (a render, a stock image, a lower ISO).
- **Higher-than-baseline** suggests added grain, a higher-ISO source, or heavy
  local sharpening.

## Limitations

<div class="warning">

- **Noise is not uniform in real photographs.** It rises in shadows and falls
  in saturated highlights, because photon noise scales with signal. Dark corners
  and blown skies routinely flag.
- **Modern phones denoise unevenly.** Computational pipelines apply
  face-aware and sky-aware noise reduction, producing large legitimate variance
  differences within one frame.
- **JPEG compression suppresses noise**, more so at low quality and more so in
  smooth blocks. A heavily compressed image has little noise left to compare.
- **The local variance window mixes texture with noise.** This module does not
  separate them; a strongly textured region will read as high-variance
  regardless of its actual noise content.

</div>

## See also

- [PRNU Analysis](/docs/modules/prnu/) — the sensor-fingerprint version of the same idea
- [ELA](/docs/modules/ela/) — compression rather than sensor traces
- [Splicing Detection](/docs/modules/splicing/) — uses this as one of four signals
