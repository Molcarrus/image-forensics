---
layout: docs.liquid
title: Tampering Detection
description: The broadest composite detector — copy-move, splicing, retouching and double compression in one pass.
---

## What it does

`TamperingDetector` is the highest-level entry point in the crate. It runs
copy-move detection, the full splicing detector, its own retouching analysis,
and a double-compression check, then merges everything into one
`DetectionResult` with a combined visualization.

Use it for a first look at an unfamiliar image; use the individual modules once
you know what you are chasing.

## Usage

```rust
use image_forensics::detection::{
    Detector, tampering::{TamperingConfig, TamperingDetector},
};

let detector = TamperingDetector::with_config(TamperingConfig {
    detect_copy_move: true,
    detect_splicing: true,
    detect_retouching: true,
    block_size: 16,
    sensitivity: 0.5,
    min_confidence: 0.3,
});

let result = detector.detect(&image)?;

println!("{}", detector.name());
println!("{}", result.summary);
println!("manipulated {}", result.is_manipulated);
println!("score {:.1}%  ({:?})", result.overall_score * 100.0, result.overall_confidence);

for m in &result.manipulations {
    println!("{:?}: {}", m.manipulation_type, m.description);
}

result.visualization.save("output/tampering.png")?;
```

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `detect_copy_move` | `true` | Runs `CopyMoveDetector` at similarity 0.9, min distance 50 |
| `detect_splicing` | `true` | Runs the full `SplicingDetector`, which itself runs ELA and noise |
| `detect_retouching` | `true` | Runs the texture and blur consistency checks below |
| `block_size` | `16` | Tile size, forwarded to copy-move |
| `sensitivity` | `0.5` | Scales the z-score cutoffs: texture at `2.0 × sensitivity`, blur at `2.5 × sensitivity`. **Lower means stricter** |
| `min_confidence` | `0.3` | Retouching findings below this are dropped |

</div>

<div class="note">

Note the direction of `sensitivity`: it multiplies a threshold, so a *lower*
value produces *more* detections, which is the opposite of what the name
suggests.

</div>

## Retouching analysis

Two checks, both z-scored across the frame:

- **Texture consistency** — mean gradient magnitude per tile. A tile far from
  the image mean has unusual texture density, which is what cloning and healing
  produce.
- **Blur consistency** — Laplacian variance per tile, the standard sharpness
  measure. A region markedly softer or sharper than its surroundings has been
  blurred, sharpened, or came from a differently-focused source.

<div class="note">

The blur check built its regions with `block_size.midpoint(height - by)` for
their height — a typo for `.min(...)` that averaged the block size with the
remaining rows, so flagged regions ran past the bottom edge of the image. The
same typo existed in the noise module.

</div>

## Colour key in the visualization

<div class="table-wrap">

| Colour | Manipulation type |
|--------|-------------------|
| Red | Copy-move |
| Orange | Splicing |
| Yellow | Retouching |
| Magenta | Removal |
| Cyan | Other or unknown |

</div>

Border thickness scales with confidence, and the region interior is tinted.

## Interpreting the output

- **`overall_score`** is the mean confidence across all findings, so many
  low-confidence findings dilute a few strong ones. Read the individual
  `manipulations` rather than the aggregate.
- **Copy-move findings appear in pairs** — one entry for the source region, one
  for the target.
- **Double compression** is reported as `ManipulationType::Unknown` covering the
  whole image, above a likelihood of 0.6.

## Limitations

<div class="warning">

- **Slowest path in the crate.** It runs copy-move, splicing (which runs ELA
  and noise internally) and a full JPEG analysis sweep. Prefer targeted modules
  once you know what you are looking for.
- **`overall_score` averages, so it dilutes.** Twenty weak retouching findings
  will pull the score below one strong copy-move detection.
- **Retouching detection flags legitimate variety.** Shallow depth of field
  produces exactly the blur inconsistency it looks for; so does any deliberate
  bokeh.
- **Double compression is not tampering.** Almost every image that has passed
  through a platform has been recompressed.
- **`ManipulationType` is broader than what is implemented.** `Removal`,
  `Resizing`, `Rotation`, `ColorManipulation` and `AIGenerated` are never
  produced by this detector.

</div>

## See also

- [Splicing Detection](/docs/modules/splicing/) — the largest component
- [Copy-move](/docs/modules/copy-move/)
- [Configuration](/docs/configuration/)
