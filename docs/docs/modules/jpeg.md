---
layout: docs.liquid
title: JPEG Analysis
description: Estimates the encoding quality, searches for JPEG ghosts, and measures the 8x8 blocking grid.
---

## What it does

Four things, from one sweep of recompressions at qualities 50 to 100:

- **Quality estimate** — the quality whose recompression perturbs the image
  least is, approximately, the quality it already carries.
- **Ghost detection** — a *local* dip in the recompression curve at a quality
  below the current one betrays an earlier compression at that quality.
- **Blocking artifacts** — the strength of discontinuities on the 8-pixel grid.
- **Double-compression likelihood** — combines the ghost dip with how well the
  blocking grid is aligned.

## Usage

```rust
use image_forensics::analysis::jpeg_analysis::JpegAnalyzer;

let analyzer = JpegAnalyzer::new()
    .with_ghost_prominence(0.05); // default 0.05

let result = analyzer.analyze(&image)?;

println!("quality      {}", result.quality_estimate);
println!("ghost        {}", result.ghost_detected);
println!("ghost qual   {:?}", result.ghost_quality);
println!("double comp  {:.2}", result.double_compression_likelihood);

if let Some(map) = &result.ghost_map {
    map.save("output/ghost.png")?;
}
```

## Results

```rust
pub struct JpegAnalysisResult {
    pub quality_estimate: u8,
    pub ghost_detected: bool,
    pub ghost_quality: Option<u8>,
    pub ghost_map: Option<GrayImage>,
    pub blocking_artifact_map: GrayImage,
    pub double_compression_likelihood: f64,
}
```

`ghost_quality` and `ghost_map` are `Some` exactly when `ghost_detected`.

## How ghost detection works

The residual between an image and its recompression falls *monotonically* as
the recompression quality rises — so the global minimum is always the highest
quality tried, and finding it tells you nothing. A JPEG ghost is an **interior
local minimum**: the curve dips at the quality of a previous compression, then
rises again before resuming its downward trend.

The module looks for such a dip and measures its prominence relative to the
surrounding shoulder. `ghost_prominence` is the minimum relative dip that
counts; raise it to require a more pronounced ghost.

<div class="note">

This previously took the *global* minimum and then required it to be below
quality 90 — a condition its own search range (60 to 95, with 95 excluded) made
unreachable. `ghost_detected` could never be true for any input.

The `double_compression_likelihood` was equally broken: it binned diagonal
pixel differences into an array named `dct_histogram`, with no DCT involved,
and then scored "periodicity" as `1 - mean(|h₁-h₂| / (h₁+h₂))` — a quantity
*maximised by a flat histogram*, i.e. the opposite of periodic.

</div>

## Grid alignment

`grid_alignment_strength` compares the mean horizontal discontinuity *on* the
8-pixel grid against the mean *off* it. A singly-compressed JPEG has one clean
grid, so the ratio is high. An image that was cropped and resaved carries a
second, misaligned grid, which weakens it.

## Interpreting the output

- **`quality_estimate` around 95–100** on a file you believe is a JPEG often
  means the pixels came from a lossless source, or were upscaled after
  compression.
- **A ghost at a quality well below the current one** is the most useful single
  result here: it means the image was saved at that quality and then resaved
  higher. That is consistent with an edit-and-resave cycle.
- **`double_compression_likelihood` above ~0.6** is worth pursuing with
  [DCT analysis](/docs/modules/dct/).

## Limitations

<div class="warning">

- **Double compression is not tampering.** Every image that has passed through
  a messaging app, a CMS, or a social platform has been recompressed, usually
  several times.
- **Quality estimation is approximate.** It compares reconstruction error, not
  quantisation tables, so it cannot distinguish encoders that use different
  tables at the same nominal quality.
- **The sweep is expensive**: a full encode and decode per step.
- **Chroma subsampling is ignored.** A change in subsampling between saves is a
  strong signal this module does not look at.
- **Resizing destroys the grid**, so grid alignment says nothing about a resized
  image.

</div>

## See also

- [ELA](/docs/modules/ela/) — the spatial view of the same recompression idea
- [DCT Analysis](/docs/modules/dct/) — quantisation tables and coefficient histograms
- [Benford Analysis](/docs/modules/benford/) — first-digit statistics of DCT coefficients
