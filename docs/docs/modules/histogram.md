---
layout: docs.liquid
title: Histogram Analysis
description: Looks for the comb patterns, gaps and clipping that levels, curves and gamma adjustments leave in the tonal distribution.
---

## What it does

Global tonal edits are destructive in a visible way. Stretching contrast spreads
the existing levels apart and leaves empty bins between them — a comb. Pulling
levels together stacks counts into spikes. Clipping pins mass at 0 or 255.
None of that is visible in the image, but all of it is plain in the histogram.

The module builds luminance and per-channel histograms, then tests for gaps,
comb periodicity, shadow and highlight clipping, unusual spikes, and a
truncated range. It also builds a per-tile gap map, and can render the
histograms as images.

## Usage

```rust
use image_forensics::analysis::histogram_analysis::{HistogramAnalyzer, HistogramConfig};

let analyzer = HistogramAnalyzer::with_config(HistogramConfig {
    block_size: 64,
    gap_threshold: 0,
    peak_threshold: 0.1,
    clipping_threshold: 0.01,
});

let result = analyzer.analyze(&image)?;

println!("probability {:.2}", result.manipulation_probability);
println!("gamma       {:?}", result.estimated_gamma);
println!("stretched   {}", result.contrast_stretched);

for anomaly in &result.anomalies {
    println!("{anomaly:?}");
}

analyzer.render_rgb_histograms(&result).save("output/hist_stacked.png")?;
analyzer.render_rgb_histograms_overlaid(&result).save("output/hist_overlaid.png")?;
result.gaps_map.save("output/hist_gaps.png")?;
```

No minimum image size.

<div class="note">

This module previously had **no size guard at all**, and its gap-map sweep ran
`0..height - 64`. On any image under 64 px that subtraction underflowed: a
panic in debug, and a loop of roughly 4 billion iterations in release. It now
iterates through `full_blocks`, which yields nothing when the image is smaller
than one tile.

</div>

## Anomalies

```rust
pub enum HistogramAnomaly {
    Gap { count: usize, positions: Vec<u8> },
    CombPattern { period: f64, strength: f64 },
    ShadowClipping { percentage: f64 },
    HighlightClipping { percentage: f64 },
    UnusualPeak { position: u8, height: f64 },
    TruncatedRange { min: u8, max: u8 },
}
```

<div class="table-wrap">

| Anomaly | Typical cause |
|---------|---------------|
| `Gap` | Contrast stretch or levels adjustment spreading values apart |
| `CombPattern` | A strong curve or repeated tonal edits |
| `ShadowClipping` | Black point raised, or underexposure |
| `HighlightClipping` | White point lowered, or blown highlights |
| `UnusualPeak` | Levels compressed, or a large flat region such as a synthetic background |
| `TruncatedRange` | Contrast reduced, or a screenshot of a limited-range display |

</div>

## Results

```rust
pub struct HistogramAnalysisResult {
    pub luminance_histogram: [u32; 256],
    pub red_histogram: [u32; 256],
    pub green_histogram: [u32; 256],
    pub blue_histogram: [u32; 256],
    pub anomalies: Vec<HistogramAnomaly>,
    pub gaps_map: GrayImage,
    pub manipulation_probability: f64,
    pub estimated_gamma: Option<f64>,
    pub contrast_stretched: bool,
    pub levels_adjusted: bool,
}
```

`gaps_map` shows where the *local* histogram is combed, which can localise an
adjustment applied to only part of the frame.

<div class="note">

**`estimated_gamma` was derived from the mean luminance** and returned
unconditionally, which produced a plausible-looking number for essentially
every image. It is now derived from the *median* tone — inverting
`median = 0.5^(1/gamma)` — and returned only when it departs from linear by
more than 15%, which is beyond ordinary exposure variation. `None` now means
"no gamma adjustment indicated" rather than "the formula happened to fall out
of range".

</div>

## Interpreting the output

Look at the rendered histograms directly; the comb is unmistakable once seen.

- **A regular comb** across the whole range means a global tonal edit. Common
  and often innocuous — it is what any levels adjustment in any editor does.
- **Gaps in one channel but not the others** suggests a per-channel colour
  adjustment.
- **Clipping** at either end tells you tonal information was discarded and is
  not recoverable.
- **`gaps_map` bright in one region only** is the interesting case: a local
  adjustment.

## Limitations

<div class="warning">

- **Tonal editing is not forgery.** Every raw conversion, every "auto contrast",
  every Instagram filter combs the histogram. This module detects *adjustment*,
  which says nothing about whether content was altered.
- **JPEG compression fills gaps in.** Quantisation noise repopulates empty bins,
  so a resaved image may show a clean histogram despite heavy editing.
- **Small images have sparse histograms** and produce gaps for purely
  statistical reasons.
- **The comb detector counts local extrema**; it does not estimate the period,
  and reports a fixed `period: 2.0`.
- **Screenshots and limited-range video frames** trigger `TruncatedRange`
  legitimately.

</div>

## See also

- [Benford Analysis](/docs/modules/benford/) — frequency-domain statistics
- [ELA](/docs/modules/ela/)
- [Metadata](/docs/modules/metadata/) — editing software often names itself
