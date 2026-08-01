---
layout: docs.liquid
title: Configuration
description: Every tunable in the crate, what it controls, and the trade-off it makes.
---

Each module owns a `*Config` struct with a `Default` impl, constructed through
`Analyzer::with_config(..)`. `Analyzer::new()` is always the default.

```rust
use image_forensics::analysis::benford_analysis::{BenfordAnalyzer, BenfordConfig};

let analyzer = BenfordAnalyzer::with_config(BenfordConfig {
    block_size: 32,
    ..BenfordConfig::default()
});
```

## The shared vocabulary

A handful of names recur across modules and mean the same thing everywhere.

<div class="table-wrap">

| Name | Meaning |
|------|---------|
| `block_size` | Side of the square analysis tile, in pixels. Smaller localises better and costs more; larger is more stable statistically. |
| `stride` | Step between tiles. Most modules use `block_size / 2`, i.e. 50% overlap. |
| `*_threshold` | The cutoff above which a block is flagged. Lower means more detections and more false positives. |
| `sensitivity` | A multiplier on a statistical threshold, usually in standard deviations. |
| `min_variance` | Blocks flatter than this are skipped: featureless areas match everything. |

</div>

Two block-iteration rules hold across the crate. `full_blocks` yields only
complete tiles and yields *nothing* when the image is smaller than one tile;
`clipped_blocks` tiles the whole image and clips partial tiles at the edges. No
module subtracts the block size from a dimension directly, which is what used
to underflow on small images.

## `AnalysisConfig`

Drives `ForensicsAnalyzer`, the bundled four-module pipeline.

```rust
pub struct AnalysisConfig {
    pub ela_quality: u8,          // default 95
    pub block_size: u32,          // default 16
    pub similarity_threshold: f64,// default 0.95
    pub min_match_distance: u32,  // default 50
}
```

- `ela_quality` — the JPEG quality used for the ELA recompression pass. High
  (90–98) is standard: the point is to recompress *gently* so that regions
  already at a lower quality stand out.
- `block_size`, `similarity_threshold`, `min_match_distance` are forwarded to
  `CopyMoveDetector`.

<div class="note">

`AnalysisConfig` previously carried a `parallel: bool` field that nothing read.
It has been removed. Parallelism is controlled by `rayon`'s thread pool; set
`RAYON_NUM_THREADS` to bound it.

</div>

## Per-module defaults

<div class="table-wrap">

| Module | Config | Notable defaults |
|--------|--------|------------------|
| [Benford](/docs/modules/benford/) | `BenfordConfig` | `block_size: 64`, `chi_square_threshold: 15.0`, `min_samples: 100` |
| [CFA](/docs/modules/cfa/) | `CfaConfig` | `block_size: 32`, `expected_pattern: RGGB`, `mismatch_threshold: 0.3`, `min_variance: 10.0` |
| [Chromatic aberration](/docs/modules/chromatic-aberration/) | `ChromaticAberrationConfig` | `block_size: 64`, `edge_threshold: 30.0`, `max_aberration: 5.0`, `inconsistency_threshold: 1.5` |
| [Copy-move](/docs/modules/copy-move/) | constructor args | `block_size: 4..=64`, `similarity_threshold`, `min_distance` |
| [DCT](/docs/modules/dct/) | `DctConfig` | `block_size: 8`, `histogram_bins: 256`, `anomaly_threshold: 0.3` |
| [ELA](/docs/modules/ela/) | builder methods | `quality`, `amplification: 10.0`, `block_size: 16` |
| [Histogram](/docs/modules/histogram/) | `HistogramConfig` | `block_size: 64`, `gap_threshold: 0`, `clipping_threshold: 0.01` |
| [JPEG](/docs/modules/jpeg/) | builder methods | quality sweep 50–100 step 5, `ghost_prominence: 0.05` |
| [Luminance gradient](/docs/modules/luminance-gradient/) | builder methods | `block_size`, `magnitude_threshold: 30.0`, `angle_tolerance: PI/4` |
| [Noise](/docs/modules/noise/) | builder methods | `block_size: 16`, `sensitivity: 2.0` |
| [PCA](/docs/modules/pca/) | `PcaConfig` | `block_size: 64`, `num_components: 3`, `patch_size: 8`, `patch_stride: 4`, `anomaly_threshold: 2.5` |
| [PRNU](/docs/modules/prnu/) | `PrnuConfig` | `block_size: 64`, `wavelet_levels: 4`, `correlation_threshold: 0.7`, `denoise_sigma: 3.0` |
| [Resampling](/docs/modules/resampling/) | `ResamplingConfig` | `block_size: 64`, `window_size: 16`, `threshold: 0.3`, `min_factor: 0.5`, `max_factor: 2.0` |
| [Shadow](/docs/modules/shadow/) | `ShadowConfig` | `block_size: 32`, `shadow_threshold: 80`, `min_shadow_size: 100`, `angle_tolerance: 20.0` |
| [Splicing](/docs/modules/splicing/) | `SplicingConfig` | `block_size: 16`, sensitivities `0.5`, `min_region_size: 1000`, `ela_quality: 95` |
| [Tampering](/docs/modules/tampering/) | `TamperingConfig` | all detectors on, `block_size: 16`, `sensitivity: 0.5`, `min_confidence: 0.3` |

</div>

## Minimum image sizes

Most modules require the image to be at least twice their block size in both
dimensions and return `ForensicsError::ImageTooSmall(n)` otherwise, where `n`
is the minimum edge length.

<div class="table-wrap">

| Module | Minimum edge |
|--------|--------------|
| DCT | 16 px |
| Benford | 64 px |
| CFA | 64 px |
| Shadow | 64 px |
| Copy-move | 2 × `block_size` |
| Chromatic aberration, PCA, PRNU, Resampling | 128 px |
| ELA, Noise, Histogram, JPEG, Luminance gradient | none — they handle any size |

</div>

## Tuning advice

**Start at the defaults.** They are set for photographic input at a few
megapixels.

**To reduce false positives**, raise thresholds rather than lowering
sensitivity: `similarity_threshold` to `0.97`, `chi_square_threshold` upward,
`min_region_size` upward. Raising `min_variance` also helps, by excluding flat
sky and blank backgrounds that match everything.

**To localise more precisely**, halve `block_size` — and expect roughly four
times the work and noisier output.

**For speed**, raise `block_size`, raise `min_variance` so more blocks are
skipped early, and prefer running two or three targeted modules over the whole
set. The most expensive modules by a wide margin are chromatic aberration
(a shift search per block) and PRNU (an iterated bilateral filter).
