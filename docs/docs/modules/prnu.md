---
layout: docs.liquid
title: PRNU Analysis
description: Extracts the sensor's photo-response non-uniformity pattern and tests whether it is consistent across the frame.
---

## What it does

No two photosites on a sensor respond identically to light. That fixed pattern
of gain variation — photo-response non-uniformity — is stable across every
photo a given sensor takes, which makes it the closest thing to a camera
fingerprint. Content pasted from another camera carries a different pattern, or
none.

The module denoises the image, takes the residual as the PRNU estimate,
sharpens it with a Wiener-style filter weighted by local variance, then
measures how consistent that pattern is block by block.

## Usage

```rust
use image_forensics::analysis::prnu_analysis::{PrnuAnalyzer, PrnuConfig};

let analyzer = PrnuAnalyzer::with_config(PrnuConfig {
    block_size: 64,
    wavelet_levels: 4,
    correlation_threshold: 0.7,
    min_variance: 10.0,
    denoise_sigma: 3.0,
});

let result = analyzer.analyze(&image)?;

println!("consistency {:.2}", result.consistency_score);
println!("skewness {:.2}", result.prnu_statistics.skewness);
println!("kurtosis {:.2}", result.prnu_statistics.kurtosis);

result.prnu_pattern.save("output/prnu.png")?;
```

Requires at least `2 * block_size` in both dimensions.

## Comparing two images

The forensically meaningful use of PRNU is comparing a questioned image against
a reference pattern from a known camera:

```rust
let analyzer = PrnuAnalyzer::new();

let reference = analyzer.analyze(&known_camera_image)?.prnu_pattern;
let questioned = analyzer.analyze(&unknown_image)?.prnu_pattern;

let correlation = analyzer.compare_patterns(&reference, &questioned);
println!("correlation {correlation:.4}");
```

`compare_patterns` returns a zero-mean normalised correlation in `[-1, 1]` over
the overlapping area.

<div class="note">

`compare_patterns` indexed columns by the *row* count — `for x in 0..height` —
so it computed the wrong mean for any non-square overlap and panicked outright
whenever the patterns were taller than wide.

</div>

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `block_size` | `64` | Tile for the consistency sweep |
| `wavelet_levels` | `4` | Bilateral-filter passes in the denoiser. More removes more content, and costs proportionally |
| `correlation_threshold` | `0.7` | Floor below which a block reads as inconsistent |
| `min_variance` | `10.0` | Blocks flatter than this get a neutral 0.5 rather than a meaningless correlation |
| `denoise_sigma` | `3.0` | Assumed noise standard deviation in the Wiener weighting |

</div>

## Results

```rust
pub struct PrnuAnalysisResult {
    pub prnu_pattern: GrayImage,
    pub correlation_map: GrayImage,
    pub inconsistent_regions: Vec<SRegion>,
    pub consistency_score: f64,
    pub manipulation_probability: f64, // 1 - consistency_score
    pub block_correlations: Vec<f64>,
    pub prnu_statistics: PrnuStatistics,
}

pub struct PrnuStatistics {
    pub mean: f64,
    pub std_dev: f64,
    pub skewness: f64,
    pub kurtosis: f64,
    pub energy: f64,
}
```

A clean PRNU residual should look roughly Gaussian: mean near zero, skewness
near zero, kurtosis near zero (excess). Large departures usually mean content
leaked through the denoiser rather than anything about the sensor.

## Limitations

<div class="warning">

**This is a much weaker instrument than single-image PRNU is usually presented
as being.**

- **Real PRNU needs many reference images.** Practical sensor identification
  averages residuals over dozens of flat-field frames from the known camera to
  suppress content. A pattern estimated from one image is dominated by scene
  content, not sensor gain.
- **The internal consistency check is not a true correlation.** Blocks are
  compared against a single global mean, so the measure largely tracks the
  ratio of local to global standard deviation.
- **Denoising leaks content.** Strong edges survive the bilateral filter and
  appear in the "PRNU" pattern.
- **JPEG compression attenuates PRNU severely**, and resizing destroys it —
  the pattern is tied to physical photosite positions.
- **It is the slowest module here** aside from chromatic aberration: an
  iterated bilateral filter over every pixel.

</div>

## See also

- [Noise Analysis](/docs/modules/noise/) — cheaper, no fingerprint claim
- [CFA Analysis](/docs/modules/cfa/) — the other sensor-level trace
- [Metadata](/docs/modules/metadata/) — what the file claims about the camera
