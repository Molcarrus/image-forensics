---
layout: docs.liquid
title: Chromatic Aberration
description: Measures per-channel misalignment at edges and fits a radial lens model, flagging regions that break it.
---

## What it does

A lens refracts wavelengths by slightly different amounts, so the red, green
and blue channels do not land in exactly the same place. The displacement is
radial: near zero at the optical centre, growing outward. It is a property of
the physical lens, and it is hard to reproduce when compositing.

The module finds edge points in each tile, measures the sub-pixel shift that
best aligns red-to-green and blue-to-green, fits a radial model, and flags
tiles whose measured shift departs from what the model predicts there.

## Usage

```rust
use image_forensics::analysis::chromatic_aberration::{
    ChromaticAberrationAnalyzer, ChromaticAberrationConfig,
};

let analyzer = ChromaticAberrationAnalyzer::with_config(ChromaticAberrationConfig {
    block_size: 64,
    edge_threshold: 30.0,
    min_edge_strength: 20.0,
    max_aberration: 5.0,
    inconsistency_threshold: 1.5,
});

let result = analyzer.analyze(&image)?;

if let Some((cx, cy)) = result.optical_center {
    println!("optical centre ({cx:.1}, {cy:.1})");
}
if let Some(model) = &result.radial_model {
    println!("k_red {:.5}  k_blue {:.5}  fit {:.2}",
        model.k_red, model.k_blue, model.fit_quality);
}

result.visualization.save("output/chromatic.png")?;
```

Requires at least `2 * block_size` in both dimensions.

<div class="note">

The config struct was spelled `ChromaticAbberationConfig`. It is now
`ChromaticAberrationConfig`.

</div>

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `block_size` | `64` | Tile size, swept at 50% overlap |
| `edge_threshold` | `30.0` | Sobel magnitude a pixel must exceed to be used as an edge point. A tile needs at least ten |
| `max_aberration` | `5.0` | Search radius in pixels. Measurements beyond it are discarded |
| `inconsistency_threshold` | `1.5` | Scaled by 50 to give the 0–255 cutoff on the inconsistency map |

</div>

## Results

```rust
pub struct ChromaticAberrationResult {
    pub measurements: Vec<AberrationMeasurement>,
    pub aberration_map: GrayImage,
    pub inconsistency_map: GrayImage,
    pub visualization: RgbImage,
    pub inconsistent_regions: Vec<SRegion>,
    pub optical_center: Option<(f64, f64)>,
    pub radial_model: Option<RadialAberrationModel>,
    pub consistency_score: f64,
    pub manipulation_probability: f64,
}

pub struct RadialAberrationModel {
    pub center_x: f64,
    pub center_y: f64,
    pub k_red: f64,
    pub k_blue: f64,
    pub fit_quality: f64, // R-squared, [0, 1]
}
```

The visualization draws each measurement's red and blue shift vectors, scaled
10x, plus a crosshair at the fitted optical centre.

<div class="note">

Three substantive fixes.

**The optical centre was assumed, not found.** It was hard-wired to the middle
of the frame — but a *displaced* optical centre is precisely the signal here,
since splicing breaks the radial symmetry. The module could not detect what it
reported. It now searches a grid of candidate centres and keeps the best fit.

**The alignment measure was not a correlation.** It omitted mean subtraction,
making it a cosine similarity between two all-positive intensity vectors, which
sits near 1.0 for every candidate shift — so the arg-max it fed was effectively
noise. It is now a zero-mean normalised cross-correlation.

**Drawing could hang.** Shift vectors pointing left or up produced a negative
endpoint cast to `u32`, wrapping to about 4 billion; the line loop then marched
towards a target it re-read as negative. Infinite in release, an overflow panic
in debug.

</div>

## Cost

This is the most expensive module in the crate. The shift search is now
coarse-to-fine — whole-pixel candidates, then two refinement passes at 1/3 and
1/9 of a pixel — and parallelised across tiles with `rayon`.

<div class="note">

The previous search evaluated every integer shift crossed with a 3x3 sub-pixel
grid: 1089 full correlations per tile, each over every edge point. On a 12 MP
image that is on the order of 10^10 operations, single-threaded.

</div>

## Interpreting the output

- **`fit_quality`** near 1 means aberration across the frame is well described
  by a single radial model — consistent with one lens, one exposure.
- **`fit_quality` near 0** usually means there was too little edge structure to
  measure, not that the image is composite.
- **`inconsistency_map`** is the output to look at: bright tiles measured a
  shift the model did not predict for their radius and direction.
- **`optical_center` far from the frame centre** is consistent with cropping —
  or with a poor fit. Check `fit_quality` before reading anything into it.

## Limitations

<div class="warning">

- **Needs strong, high-contrast edges.** Soft or low-contrast images produce
  too few usable edge points and the module returns almost nothing.
- **Modern cameras correct aberration in-camera**, and RAW converters apply
  lens profiles. A corrected image has little aberration left to measure, which
  reads as "no signal", not "authentic".
- **JPEG compression and any resize** blur the sub-pixel displacement this
  depends on.
- **Purple fringing is not chromatic aberration** in this sense; blown
  highlights produce colour edges that are not lens dispersion.
- **A well-executed composite from the same lens** carries the right aberration
  and will not be caught.

</div>

## See also

- [Shadow Analysis](/docs/modules/shadow/) — the other physical-consistency check
- [Luminance Gradient](/docs/modules/luminance-gradient/)
- [Splicing Detection](/docs/modules/splicing/)
