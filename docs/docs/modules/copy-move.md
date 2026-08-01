---
layout: docs.liquid
title: Copy-Move Detection
description: Finds regions duplicated within the same image by matching DCT features between blocks.
---

## What it does

Copy-move forgery hides or duplicates something using material from the same
photo — cloning foliage over an object, duplicating a crowd. Because the source
is the same image, the copy matches the original in noise, lighting and
compression history, which defeats most cross-source detectors.

This module slides overlapping blocks across the image, computes a
low-frequency DCT feature for each, buckets them by a similarity hash, and
correlates the candidates within each bucket. Pairs that are similar enough and
far enough apart are reported.

## Usage

```rust
use image_forensics::analysis::copy_move::CopyMoveDetector;

let detector = CopyMoveDetector::new(
    16,   // block_size: 4..=64
    0.95, // similarity_threshold
    50,   // min_distance in pixels
)?;

let result = detector.detect(&image)?;

println!("matches: {}", result.matches.len());
println!("confidence: {:.1}%", result.confidence * 100.0);

for pair in &result.matches {
    println!(
        "({}, {}) -> ({}, {}) at {:.1}%",
        pair.source.x, pair.source.y,
        pair.target.x, pair.target.y,
        pair.similarity * 100.0,
    );
}

result.visualization.save("output/copy_move.png")?;
```

`new` returns `Err(ForensicsError::InvalidParameter)` for a block size outside
4–64.

## Configuration

<div class="table-wrap">

| Parameter | Effect |
|-----------|--------|
| `block_size` | Side of the compared square, 4–64. Smaller finds smaller copies, costs more, yields more false pairs |
| `similarity_threshold` | Minimum Pearson correlation between two feature vectors. `0.95` strict, `0.90` a sensible floor |
| `min_distance` | Minimum separation in pixels before a pair counts. Suppresses trivial self-matches with neighbouring blocks |

</div>

`with_variance_threshold(f64)` sets the flatness cutoff (default `100.0`).
Blocks below it are skipped entirely — flat sky matches every other patch of
flat sky, and skipping them is what keeps the pairwise stage tractable.

## How the feature works

Each block is transformed with a proper separable 2-D DCT-II (`B * X * Bᵀ`,
with the JPEG level shift), and the first sixteen coefficients in zig-zag scan
order are kept. Low-frequency coefficients are robust to the small changes a
paste introduces while still discriminating between different content.

<div class="note">

This previously ran a 1-D FFT over the *flattened* block and took magnitudes —
neither a DCT nor two-dimensional — so visually distinct blocks produced
colliding features. The hash bucketing was also broken: it perturbed only the
low two bits of the hash while inserting every feature into four buckets, so
most pairs were compared redundantly and genuine near-matches were missed.

</div>

## Results

```rust
pub struct CopyMoveResult {
    pub matches: Vec<MatchPair>,
    pub visualization: RgbImage,
    pub confidence: f64, // mean similarity of retained matches
}

pub struct MatchPair {
    pub source: SRegion,
    pub target: SRegion,
    pub similarity: f64,
}
```

Overlapping matches are filtered: matches are sorted by similarity and a
candidate is dropped if either of its regions overlaps a region already kept.
The visualization boxes each pair in its own hue and draws a line between the
centres.

## Interpreting the output

A genuine copy-move usually shows as **many matches sharing one offset vector**
— a cloned area produces a whole cluster of block pairs all displaced by the
same `(dx, dy)`. Scattered isolated matches with no common offset are more
likely coincidence in repetitive texture.

Similarities at or very near 100% mean an exact pixel copy, which is what
naive clone-stamping produces.

## Limitations

<div class="warning">

- **Only exact-ish copies.** A copy that was rotated, scaled or heavily blended
  after pasting will not match. This module compares blocks at one orientation
  and one scale.
- **Repetitive content produces false positives.** Brick walls, windows in an
  office block, tiled floors, fence posts, ocean waves and crowds all legitimately
  contain near-identical blocks. Check whether matches share a common offset.
- **Block alignment matters.** Features are extracted on a `block_size / 2`
  stride. A copy displaced by an amount not congruent to that stride may have
  no aligned pair to match against; vary `block_size` if you suspect a copy the
  detector is missing.
- **Cost grows with the number of textured blocks.** Raise `block_size` or the
  variance threshold on large images.

</div>

## See also

- [Splicing Detection](/docs/modules/splicing/) — for material from *other* images
- [Resampling](/docs/modules/resampling/) — for copies that were scaled
- [Tampering Detection](/docs/modules/tampering/) — runs this alongside other methods
