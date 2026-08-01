---
layout: docs.liquid
title: Luminance Gradient
description: Maps the direction of brightness change across the image and flags blocks whose shading disagrees with the dominant lighting.
---

## What it does

Surfaces lit from one direction shade in a consistent way: the brightness
gradient across curved and angled surfaces points, broadly, away from the light.
An object composited from a differently-lit source shades the wrong way.

The module computes a Sobel gradient at every pixel, records magnitude and
orientation, takes a magnitude-weighted circular mean as the dominant
direction, then flags blocks whose own mean direction departs from it.

## Usage

```rust
use image_forensics::analysis::luminance_gradient::LuminanceGradientAnalyzer;

let analyzer = LuminanceGradientAnalyzer::new(16)     // block_size
    .with_magnitude_threshold(30.0)                   // default 30.0
    .with_angle_tolerance(std::f64::consts::PI / 4.0); // default PI/4

let result = analyzer.analyze(&image)?;

println!("dominant direction {:.3} rad", result.dominant_direction);
println!("confidence         {:.2}", result.direction_confidence);
println!("inconsistent blocks {}", result.inconsistent_regions.len());

result.gradient_map.save("output/gradient.png")?;
result.direction_map.save("output/direction.png")?;
```

No minimum image size.

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `block_size` | — | Tile size for the consistency sweep |
| `magnitude_threshold` | `30.0` | Gradients weaker than this are treated as flat. Lower it for gentle ramps — a smooth luminance ramp carries only a couple of levels per pixel |
| `angle_tolerance` | `PI / 4` | Angular deviation from the dominant direction before a block is flagged |

</div>

A block is only considered when at least a quarter of its pixels carry usable
gradient, and its own directional confidence exceeds 0.3.

## Results

```rust
pub struct LuminanceGradientResult {
    pub gradient_map: GrayImage,
    pub direction_map: GrayImage,
    pub inconsistent_regions: Vec<SRegion>,
    pub dominant_direction: f64,     // radians, [-PI, PI]
    pub direction_confidence: f64,   // [0, 1]
}
```

`direction_map` packs the orientation into 0–255 across the full ±π range;
`image_utils::u8_to_angle` decodes it.

<div class="note">

Two defects made this module's output meaningless.

**The Sobel X kernel had an inverted sign.** The middle-right tap was
`-2.0 * p(1, 0)` where it must be `+2.0 * p(1, 0)`, so it was not a Sobel
operator at all and every gradient direction pointed wrongly. There is now one
shared `image_utils::sobel` used by every module.

**The dominant direction was arithmetic nonsense.** It was computed as
`(bin / bins) + 2.0 * PI - PI` — a missing multiplication — so the result
always landed in `[π, π + 1)` regardless of the histogram. Directions are now
combined as unit vectors (a circular mean), which also handles the ±π seam
correctly; averaging the raw packed codes, as the old block comparison did,
wraps incorrectly there.

`direction_confidence` is new: it is the resultant length of that circular
mean, so you can tell "everything agrees on this direction" from "the samples
cancelled out".

</div>

## Interpreting the output

- **High `direction_confidence`** (above ~0.7) means the frame has a clear
  dominant shading direction. Below ~0.3 there is no coherent lighting signal
  and `dominant_direction` should be ignored.
- **`gradient_map`** is a plain edge map, useful for confirming there is enough
  structure to work with.
- **`inconsistent_regions`** are blocks with strong, coherent shading pointing
  somewhere other than the dominant direction.

Bear in mind that the gradient direction is dominated by *edges*, not by
surface shading, in most photographs. This module is at its most useful on
images with large smoothly-shaded surfaces.

## Limitations

<div class="warning">

- **Edges dominate the measurement.** Texture and object boundaries produce far
  stronger gradients than surface shading, so on a detailed scene the "dominant
  direction" largely reflects edge orientation statistics rather than lighting.
- **Multiple light sources**, reflections and bounced light are normal and
  break the single-direction assumption.
- **Texture and albedo changes** are indistinguishable from shading here. A
  dark-to-light painted stripe reads exactly like a shading ramp.
- **The default threshold of 30 discards gentle gradients.** If you are
  analysing a smoothly-lit scene, lower it with `with_magnitude_threshold`.

</div>

## See also

- [Shadow Analysis](/docs/modules/shadow/) — lighting direction from cast shadows
- [Chromatic Aberration](/docs/modules/chromatic-aberration/)
- [Noise Analysis](/docs/modules/noise/)
