---
layout: docs.liquid
title: Shadow Analysis
description: Segments shadow regions, estimates a light direction for each, and flags those disagreeing with the dominant direction.
---

## What it does

A scene lit by one dominant source casts shadows in one consistent direction.
Objects composited in from elsewhere usually bring the wrong one — and shadow
direction is something people are poor at judging by eye, which makes it a
useful automatic check.

The module segments low-intensity, low-saturation regions as shadow candidates,
cleans them morphologically, walks each connected component, estimates a light
direction from the gradient orientations along its boundary, and compares each
against the frame's dominant direction.

## Usage

```rust
use image_forensics::analysis::shadow_analysis::{ShadowAnalyzer, ShadowConfig};

let analyzer = ShadowAnalyzer::with_config(ShadowConfig {
    block_size: 32,
    edge_threshold: 30.0,
    shadow_threshold: 80,
    min_shadow_size: 100,
    angle_tolerance: 20.0,
    gradient_threshold: 15.0,
});

let result = analyzer.analyze(&image)?;

println!("dominant direction {:.2} rad", result.dominant_light_direction);
println!("confidence  {:.1}%", result.dominant_direction_confidence * 100.0);
println!("light sources {}", result.estimated_light_sources);
println!("consistency {:.1}%", result.consistency_score * 100.0);

result.shadow_mask.save("output/shadow_mask.png")?;
result.direction_map.save("output/shadow_directions.png")?;
```

Requires at least `2 * block_size` in both dimensions.

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `shadow_threshold` | `80` | Intensity ceiling for a shadow pixel. Combined with an adaptive 10th-percentile estimate |
| `min_shadow_size` | `100` | Minimum component **area** in pixels |
| `angle_tolerance` | `20.0` | Degrees a region may deviate from the dominant direction before being flagged |
| `gradient_threshold` | `15.0` | Boundary gradient magnitude needed to contribute a direction sample |

</div>

## Results

```rust
pub struct ShadowAnalysisResult {
    pub shadow_regions: Vec<ShadowRegion>,
    pub dominant_light_direction: f64,       // radians, [-PI, PI]
    pub dominant_direction_confidence: f64,  // [0, 1]
    pub inconsistent_regions: Vec<SRegion>,
    pub direction_map: RgbImage,
    pub shadow_mask: GrayImage,
    pub consistency_score: f64,
    pub manipulation_probability: f64,
    pub estimated_light_sources: usize,
}

pub struct ShadowRegion {
    pub region: SRegion,
    pub light_direction: f64,
    pub direction_confidence: f64,
    pub intensity: f64,
    pub edge_sharpness: f64,
}
```

Directions are combined as unit vectors — a circular mean — so averaging works
correctly across the ±π seam. The confidence is the resultant length: 1 when
every sample agrees, 0 when they cancel.

`direction_map` draws each shadow boxed in green (consistent) or red
(inconsistent) with an arrow for its light direction, plus a yellow arrow in the
corner for the dominant direction.

<div class="note">

**Light-source counting ignored the wraparound.** Directions were sorted and
gaps counted, but not the gap from the last direction back around to the first —
so a single cluster straddling 0 rad was split in two, inflating
`estimated_light_sources` and adding 0.15 to the manipulation probability for
one ordinary shadow.

**A size filter compared the wrong quantity.** Regions were additionally
filtered on bounding-box *width or height* against `min_shadow_size`, which is
an *area* threshold, discarding long thin shadows for no reason. The area check
in the component walk was already correct, so the extra filter is gone.

</div>

## Interpreting the output

- **`estimated_light_sources` of 1** with a high `consistency_score` is the
  expected result for an outdoor scene.
- **2 or more** is worth examining — but is entirely normal indoors, at dusk,
  or anywhere with mixed natural and artificial light.
- **`inconsistent_regions`** are the shadows to look at first. Check them
  against the scene by eye: the module cannot reason about occluders it cannot
  see.
- **`edge_sharpness`** distinguishes hard shadows (direct sun, point source)
  from soft ones (overcast, large source). An object with a hard shadow in a
  scene of soft ones is suspicious regardless of direction.

## Limitations

<div class="warning">

- **Shadow segmentation is the weak link.** It thresholds on intensity and
  saturation, so dark clothing, dark paint, black objects and deep foliage are
  routinely segmented as shadow.
- **Direction is inferred from boundary gradients**, which is a rough proxy for
  the geometry that actually determines shadow direction. It does not do
  vanishing-point or cast-shadow-constraint analysis.
- **Multiple light sources are normal**, not evidence.
- **Soft shadows carry little directional information** — an overcast scene may
  produce no usable estimate at all.
- **Nothing here reasons about 3-D geometry.** A shadow falling across an
  uneven surface bends, and the module does not know that.

</div>

## See also

- [Luminance Gradient](/docs/modules/luminance-gradient/) — lighting direction from shading rather than shadows
- [Chromatic Aberration](/docs/modules/chromatic-aberration/)
- [Splicing Detection](/docs/modules/splicing/)
