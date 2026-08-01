---
layout: docs.liquid
title: Resampling Detection
description: Finds the periodic interpolation residue left behind when an image or a region is scaled or rotated.
---

## What it does

Scaling or rotating an image means interpolating: every output pixel becomes a
weighted combination of its neighbours. That introduces a *periodic* linear
dependence between neighbouring pixels, with a period set by the resampling
factor. Detecting it is one of the few ways to spot a region that was resized
to fit before being pasted in.

The module computes a second-derivative map, autocorrelates it along rows and
columns, looks for periodic peaks, and estimates the scaling factor those peaks
imply.

## Usage

```rust
use image_forensics::analysis::resampling_detection::{
    ResamplingConfig, ResamplingDetector,
};

let detector = ResamplingDetector::with_config(ResamplingConfig {
    block_size: 64,
    window_size: 16,
    threshold: 0.3,
    min_factor: 0.5,
    max_factor: 2.0,
});

let result = detector.detect(&image)?;

println!("probability {:.2}", result.resampling_probability);
println!("factor      {:?}", result.estimated_factor);

for pattern in &result.periodic_patterns {
    println!(
        "period {:.1} strength {:.2} direction {:.2} rad",
        pattern.period, pattern.strength, pattern.direction,
    );
}

result.probability_map.save("output/resampling.png")?;
```

Requires at least `2 * block_size` in both dimensions.

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `block_size` | `64` | Tile for the local periodicity sweep |
| `window_size` | `16` | Maximum autocorrelation lag. Bounds the longest period detectable |
| `threshold` | `0.3` | Minimum peak strength for a pattern; also scaled to 0–255 as the region cutoff |
| `min_factor` / `max_factor` | `0.5` / `2.0` | Range of scaling factors considered plausible |

</div>

## Results

```rust
pub struct ResamplingResult {
    pub probability_map: GrayImage,
    pub periodic_patterns: Vec<PeriodicPattern>,
    pub estimated_factor: Option<f64>,
    pub resampling_probability: f64,
    pub resampled_regions: Vec<SRegion>,
    pub p_map: GrayImage,
}

pub struct PeriodicPattern {
    pub period: f64,
    pub strength: f64,
    pub direction: f64, // 0 = horizontal, PI/2 = vertical
}
```

<div class="note">

**`estimated_factor` was the raw autocorrelation lag**, returned as if it were
a scaling factor — they are not the same quantity, and a lag of 7 was reported
as "scaled 7×". For a resampling by `p/q` the interpolation residual repeats
every `q` output samples, so a period of `q` implies a factor of `q / (q − 1)`
upsampling, or its reciprocal downsampling. Both interpretations are now
computed and the one falling inside `min_factor..=max_factor` is reported.
Those two config fields were previously never read at all.

</div>

## Interpreting the output

- **`periodic_patterns` empty** means no periodic residue was found. That is the
  expected result for an unmodified camera original.
- **A horizontal pattern without a vertical one** (or vice versa) suggests
  non-uniform scaling — stretching in one axis.
- **`probability_map`** localises: bright regions have locally periodic residue.
  A bright rectangle in an otherwise dark map is the interesting case.
- **`estimated_factor` near 1** means very slight resizing; far from 1 means
  aggressive scaling.

Whole-image resampling is unremarkable — nearly every image on the web has been
resized. **A region that is resampled while the rest of the frame is not** is
the finding worth pursuing.

## Limitations

<div class="warning">

- **JPEG compression masks the residue.** The 8×8 blocking grid is itself
  periodic and interferes; at low quality the interpolation trace is largely
  gone.
- **Resizing is ubiquitous.** A positive on the whole image tells you it was
  resized, which is true of almost everything.
- **Downsampling is harder to detect than upsampling**, because it discards
  rather than interpolates information.
- **Naturally periodic content** — fabric, brickwork, fences, halftone
  screening — produces the same autocorrelation peaks.
- **The autocorrelation is computed along rows and columns only**, so rotation
  by an angle far from a multiple of 90° is detected weakly if at all.

</div>

## See also

- [Copy-move](/docs/modules/copy-move/) — for duplicates that were *not* scaled
- [DCT Analysis](/docs/modules/dct/) — the interfering compression grid
- [CFA Analysis](/docs/modules/cfa/) — a trace that resampling destroys
