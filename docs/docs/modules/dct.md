---
layout: docs.liquid
title: DCT Analysis
description: "Works in the frequency domain: estimates the quantisation table, histograms AC coefficients, and looks for double-compression periodicity."
---

## What it does

JPEG divides the image into 8×8 blocks, transforms each with a DCT, and divides
the coefficients by a quantisation table. That division leaves the surviving
coefficients clustered on multiples of the quantisation step — a structure this
module recovers from the decoded pixels.

It computes the DCT of every aligned 8×8 block, estimates the quantisation
table from coefficient spacing, derives a quality figure, histograms the first
AC coefficient, and looks for periodicity in that histogram.

## Usage

```rust
use image_forensics::analysis::dct_analysis::{DctAnalyzer, DctConfig};

let analyzer = DctAnalyzer::with_config(DctConfig {
    block_size: 8,
    histogram_bins: 256,
    anomaly_threshold: 0.3,
    ac_coefficients_count: 15,
});

let result = analyzer.analyze(&image)?;

println!("primary quality   {}", result.primary_quality);
println!("secondary quality {:?}", result.secondary_quality);
println!("double compression {:.2}", result.double_compression_probability);
println!("periodicity        {:.3}", result.histogram_periodicity);

result.block_artifact_map.save("output/dct_blocks.png")?;
result.dct_energy_map.save("output/dct_energy.png")?;
```

## Results

```rust
pub struct DctAnalysisResult {
    pub primary_quality: u8,
    pub secondary_quality: Option<u8>,
    pub double_compression_probability: f64,
    pub ac_histogram: Vec<u32>,
    pub histogram_periodicity: f64,
    pub block_artifact_map: GrayImage,
    pub dct_energy_map: GrayImage,
    pub anomalous_regions: Vec<SRegion>,
    pub estimated_quantization_table: [[f64; 8]; 8],
}
```

`estimated_quantization_table` is compared against the standard JPEG luminance
table at quality 50 to produce `primary_quality`.

<div class="note">

Three defects were fixed here.

**The block grid was derived from the wrong dimension.** The row count came
from `width / 8` instead of `height / 8`, so a tall image was analysed only
down to its top square, and a wide image produced phantom blocks past the
bottom edge filled with the −128 level shift. Every result for a non-square
image was wrong.

**The AC histogram was clamped to nothing.** Coefficients were binned as
`ac + 128` clipped to `0..255`, but unquantised DCT coefficients of a decoded
image routinely reach several hundred, so most of the distribution piled into
the two end bins. The range is now derived from the data.

**Periodicity exceeded 1 and was clamped.** The correlation was neither
mean-centred nor variance-normalised, so it usually came out above 1, got
clamped to 1.0 for nearly every image, and put a permanent 0.4 floor under
`double_compression_probability`. It is now a proper normalised
autocorrelation.

</div>

## Interpreting the output

- **`histogram_periodicity`** near zero means a flat, featureless coefficient
  histogram. A clear peak at some period is the classic double-compression
  signature: requantising an already-quantised coefficient set leaves periodic
  gaps.
- **`block_artifact_map`** shows the 8-pixel grid directly. A visible grid means
  block-based compression is present and aligned; a second faint grid at an
  offset suggests cropping between saves.
- **`dct_energy_map`** shows high-frequency energy per block. A region of
  markedly lower energy has been smoothed or came from a more heavily
  compressed source.
- **`anomalous_regions`** are blocks whose total coefficient energy is more than
  2.5 standard deviations from the image mean.

## Limitations

<div class="warning">

- **The quantisation table is estimated from decoded pixels**, not read from
  the file. It is an approximation; reading the actual table from the JPEG
  header would be strictly better and this module does not do it.
- **Only aligned 8×8 blocks are analysed.** If the image was cropped by an
  amount not divisible by 8, the grid no longer aligns and the estimates
  degrade.
- **Non-JPEG input produces meaningless quantisation output.** There is no
  quantisation to recover from a PNG.
- **`secondary_quality` is coarse**, mapped from the detected period through a
  small lookup rather than derived analytically.
- **Double compression is routine**, not evidence of editing.

</div>

## See also

- [JPEG Analysis](/docs/modules/jpeg/) — the pixel-domain view
- [Benford Analysis](/docs/modules/benford/) — first-digit statistics over these coefficients
- [ELA](/docs/modules/ela/)
