---
layout: docs.liquid
title: Traits
description: The Detector trait, and the conventions the analyzers follow instead of one.
---

## `Detector`

The one trait in the crate. Implemented by `SplicingDetector` and
`TamperingDetector`.

```rust
pub trait Detector {
    fn detect(&self, image: &DynamicImage) -> Result<DetectionResult>;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
}
```

It exists so composite detectors can be held behind a trait object:

```rust
use image_forensics::detection::{
    Detector, splicing::SplicingDetector, tampering::TamperingDetector,
};

let detectors: Vec<Box<dyn Detector>> = vec![
    Box::new(SplicingDetector::new()),
    Box::new(TamperingDetector::new()),
];

for detector in &detectors {
    let result = detector.detect(&image)?;
    println!("{}: {:.1}%", detector.name(), result.overall_score * 100.0);
}
```

`detect` is in scope only when `Detector` is imported — both types define the
method solely through the trait.

## Why the analyzers do not implement it

The sixteen modules under `analysis` are *not* `Detector` implementations.
Each returns its own result type carrying module-specific maps and statistics —
`BenfordAnalysisResult` has a first-digit distribution, `PrnuAnalysisResult`
has sensor-pattern moments, `ShadowAnalysisResult` has per-shadow light
directions. Flattening those into `DetectionResult` would discard exactly the
output that makes each module worth running.

They follow a shared *convention* instead:

```rust
impl XAnalyzer {
    pub fn new() -> Self;
    pub fn with_config(config: XConfig) -> Self;
    pub fn analyze(&self, image: &DynamicImage) -> Result<XAnalysisResult>;
}
```

with `Default` delegating to `new()`. Three modules deviate for historical
reasons:

- `CopyMoveDetector::new(block_size, similarity_threshold, min_distance)` is
  fallible and takes its parameters positionally; its entry point is `detect`.
- `ResamplingDetector`'s entry point is `detect`.
- `ElaAnalyzer`, `JpegAnalyzer`, `NoiseAnalyzer` and
  `LuminanceGradientAnalyzer` configure through builder methods rather than a
  config struct.

<div class="note">

**Planned.** Bringing the analyzers under one trait — something like
`fn analyze(&self, &DynamicImage) -> Result<Outcome>` with a common score,
regions and evidence shape alongside the module-specific payload — would let
`full_analysis` iterate a `Vec<Box<dyn Analyzer>>` instead of hardcoding four
modules, and allow one generic test harness across all of them. It is not
implemented yet.

</div>

## Result conventions

Where the modules do agree:

- Scores named `*_probability`, `*_score` or `confidence` are clamped to
  `[0, 1]`.
- Locations are `Vec<SRegion>`, always clipped to the image bounds.
- Merging uses `merge_regions(regions, gap)` with `gap = block_size / 2`.
- Size failures are `ForensicsError::ImageTooSmall(min_edge)`.
- Modules that render an overlay expose it as an `RgbImage` field with the same
  dimensions as the input.

## Standard library traits

- `SRegion` — `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize`
- Config structs — `Debug, Clone, Default`
- Result structs — `Debug, Clone` (`FullAnalysisReport` is `Debug` only)
- `ConfidenceLevel`, `ManipulationType`, `DetectedManipulation` — `Serialize, Deserialize`
- `ForensicsError` — `std::error::Error` via `thiserror`

Result types holding `RgbImage`/`GrayImage` are not `Serialize`; use
`report::JsonReport` for a serialisable summary.
