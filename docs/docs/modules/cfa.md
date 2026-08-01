---
layout: docs.liquid
title: CFA Analysis
description: Looks for the periodic traces that colour filter array demosaicing leaves, and for regions where they break.
---

## What it does

A single-sensor camera captures one colour per photosite through a Bayer colour
filter array, then interpolates the other two. That demosaicing leaves a
periodic 2×2 correlation structure across the whole frame. Content that was
generated, heavily edited, or pasted from a differently-processed source does
not carry the same structure.

The module scores each tile against the four Bayer arrangements (RGGB, BGGR,
GRBG, GBRG), determines the dominant one, and flags tiles that confidently
disagree with it. It also measures "zipper" interpolation artifacts.

## Usage

```rust
use image_forensics::analysis::cfa_analysis::{CfaAnalyzer, CfaConfig, CfaPattern};

let analyzer = CfaAnalyzer::with_config(CfaConfig {
    block_size: 32,
    expected_pattern: CfaPattern::RGGB,
    mismatch_threshold: 0.3,
    min_variance: 10.0,
    detect_interpolation: true,
});

let result = analyzer.analyze(&image)?;

println!("dominant   {:?}", result.dominant_pattern);
println!("confidence {:.2}", result.pattern_confidence);
println!("consistency {:.2}", result.consistency_score);

result.artifact_map.save("output/cfa_artifacts.png")?;
result.consistency_map.save("output/cfa_consistency.png")?;
```

Requires at least `2 * block_size` in both dimensions.

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `block_size` | `32` | Tile size, swept at 50% overlap |
| `expected_pattern` | `RGGB` | Sets `matches_expected` on each measurement. Does not affect the dominant-pattern search |
| `mismatch_threshold` | `0.3` | A tile must disagree with the dominant pattern *and* exceed this confidence to be flagged |
| `min_variance` | `10.0` | Flat tiles are skipped: there is no interpolation structure to read |
| `detect_interpolation` | `true` | Whether to measure zipper artifacts |

</div>

## Results

```rust
pub struct CfaAnalysisResult {
    pub measurements: Vec<CfaMeasurement>,
    pub dominant_pattern: CfaPattern,
    pub pattern_confidence: f64,
    pub artifact_map: GrayImage,
    pub consistency_map: GrayImage,
    pub inconsistent_regions: Vec<SRegion>,
    pub consistency_score: f64,
    pub manipulation_probability: f64,
    pub pattern_stats: CfaPatternStats,
}
```

`pattern_stats` counts how many tiles voted for each arrangement.
`pattern_confidence` is the winning share of the total.

<div class="note">

The GRBG scorer passed the weight `[0, 2, 0]` for its blue site, which matches
no arm of the scoring function and therefore contributed a constant zero. GRBG
was systematically under-scored on every tile.

</div>

## Interpreting the output

- **`pattern_confidence` near 1** means the tiles agree strongly on one
  arrangement — expected for an unmodified camera original.
- **A near-even split across all four** usually means the image no longer
  carries a CFA trace at all: it was resized, heavily compressed, or rendered.
  Read that as "no signal", not as evidence.
- **`consistency_map`** is the useful output. Bright tiles disagree with the
  dominant pattern; a contiguous bright region is what a paste looks like.
- **`artifact_map`** shows interpolation zippering, strongest along fine
  high-contrast detail.

## Limitations

<div class="warning">

**The strongest caveat in this crate.** The CFA trace is recovered from an
already-demosaiced RGB image using colour-ratio heuristics, not from raw sensor
data. That is a substantially weaker inference than CFA analysis on RAW.

- **Any resize destroys the trace.** So does rotation, and so does most JPEG
  compression at ordinary quality.
- **Multi-frame computational photography destroys it too** — HDR merging,
  night modes and pixel binning are the norm on phones.
- **Cameras without a Bayer CFA** (Foveon, monochrome, some medium-format
  backs) have no pattern to find.
- **A confident dominant pattern does not confirm the arrangement is real.**
  The scoring is a ratio heuristic over interpolated pixels and can settle on a
  pattern in content that never had one.

Treat a positive as a hint to look at that region with other modules, and treat
a negative as uninformative.

</div>

## See also

- [PRNU Analysis](/docs/modules/prnu/) — the other sensor-level trace
- [Resampling](/docs/modules/resampling/) — detects the scaling that erases CFA
- [Noise Analysis](/docs/modules/noise/)
