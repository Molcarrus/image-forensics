---
layout: docs.liquid
title: PCA Analysis
description: Learns the image's own principal patch subspace and flags patches that reconstruct badly from it.
---

## What it does

Overlapping patches from one photograph mostly live in a low-dimensional
subspace: the same textures, the same noise characteristics, the same
compression signature recur throughout. Principal component analysis finds that
subspace from the image itself, and content from a different source
reconstructs from it poorly.

The module extracts overlapping patches, computes a covariance matrix,
extracts the leading eigenvectors by power iteration, projects every patch, and
measures the reconstruction error.

## Usage

```rust
use image_forensics::analysis::pca_analysis::{PcaAnalyzer, PcaConfig};

let analyzer = PcaAnalyzer::with_config(PcaConfig {
    block_size: 64,
    num_components: 3,
    patch_size: 8,
    patch_stride: 4,
    anomaly_threshold: 2.5,
    min_variance_ratio: 0.01,
});

let result = analyzer.analyze(&image)?;

println!("anomaly score {:.2}", result.overall_anomaly_score);
println!("variance ratios {:?}", result.variance_ratios);

result.anomaly_map.save("output/pca_anomaly.png")?;
result.pc1_map.save("output/pca_pc1.png")?;
```

Requires at least `2 * block_size` in both dimensions.

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `patch_size` | `8` | Side of the patch, so 64 features |
| `patch_stride` | `4` | Step between patches — 50% overlap at the default |
| `num_components` | `3` | Eigenvectors retained. More captures more variance and leaves less residual to flag |
| `anomaly_threshold` | `2.5` | Standard deviations of reconstruction error above the mean before a patch counts as anomalous |
| `block_size` | `64` | Tile size for aggregating patch anomalies into regions |

</div>

Covariance is estimated from at most 5000 patches, sampled evenly.

## Results

```rust
pub struct PcaAnalysisResult {
    pub anomaly_map: GrayImage,
    pub pc1_map: GrayImage,
    pub pc2_map: GrayImage,
    pub pc3_map: GrayImage,
    pub anomalous_regions: Vec<SRegion>,
    pub variance_ratios: Vec<f64>,
    pub overall_anomaly_score: f64,
    pub manipulation_probability: f64,
}
```

A tile is flagged when at least a fifth of the patches covering it exceed the
error threshold.

<div class="note">

Three fixes here.

**`variance_ratios` was meaningless.** The total was the sum of the three
extracted eigenvalues, so the ratios always summed to exactly 1.0 no matter how
much of the image's variance those three components actually explained. The
total is now the trace of the covariance matrix, so the ratios legitimately sum
to less than 1 and mean what "explained variance" normally means.

**`anomaly_threshold` had no effect on region selection.** The threshold was
computed from the error distribution and then discarded in favour of the magic
constant `128.0 + anomaly_threshold * 30.0` tested against the *rendered* map;
the `errors` and `positions` arguments went unused. Regions are now selected
from the error distribution the parameter describes.

**Memory.** Overlapping patches were accumulated into a
`Vec<Vec<Vec<f64>>>` — one heap allocation per pixel, twelve million on a 12 MP
image, repeated for each of the three component maps and again for the anomaly
map. A running sum and count over two flat buffers replaces it.

</div>

## Interpreting the output

- **`variance_ratios`** describe the image's complexity. A first component
  above ~0.8 means the image is dominated by one pattern; a flat spread means
  varied content.
- **`anomaly_map`** is the output that matters. It is z-scored: mid-grey is
  average reconstruction error, bright is worse than average, dark is better.
- **`overall_anomaly_score`** blends the proportion of anomalous patches with
  the spread of the error distribution.

A high error can mean either "unusual content" or "content from another
source", and PCA cannot tell you which.

## Limitations

<div class="warning">

- **It flags unusual, not foreign.** The single most distinctive genuine object
  in a photograph is exactly what this reports.
- **Content dominates.** Patch statistics are driven far more by what is
  depicted than by provenance.
- **A large tampered region contaminates the basis.** The subspace is learned
  from the image including the tampering, so if the pasted area is big enough
  it becomes part of what counts as normal.
- **Only three components by default**, over 64 features — a coarse
  approximation that leaves substantial residual on ordinary content.
- **Power iteration with deflation** accumulates numerical error across
  components; the third is noticeably less reliable than the first.

</div>

## See also

- [Noise Analysis](/docs/modules/noise/) — a more targeted statistical check
- [Benford Analysis](/docs/modules/benford/)
- [Splicing Detection](/docs/modules/splicing/)
