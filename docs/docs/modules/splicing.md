---
layout: docs.liquid
title: Splicing Detection
description: Combines colour, edge, noise and ELA evidence, and reports only the regions where at least two agree.
---

## What it does

Splicing is compositing content from a *different* image. Unlike copy-move, the
pasted material carries the wrong colour statistics, the wrong noise floor and
the wrong compression history — but no single one of those is reliable alone.

This detector runs four checks and reports a region only where **at least two
of them independently flag it**. That is the module's whole design: individual
signals here are weak, and requiring corroboration is what makes the output
usable.

<div class="table-wrap">

| Signal | What it looks at | Weight |
|--------|------------------|--------|
| Colour consistency | Per-tile 8×8×8 RGB histogram vs. the global histogram | 0.25 |
| Edge regularity | Unnaturally regular edge spacing, characteristic of a pasted boundary | 0.25 |
| Noise | `NoiseAnalyzer` anomalous regions | 0.25 |
| ELA | `ElaAnalyzer` suspicious regions | 0.25 |

</div>

## Usage

```rust
use image_forensics::detection::{Detector, splicing::{SplicingConfig, SplicingDetector}};

let detector = SplicingDetector::with_config(SplicingConfig {
    block_size: 16,
    color_sensitivity: 0.5,
    noise_sensitivity: 0.5,
    edge_sensitivity: 0.5,
    min_region_size: 1000,
    ela_quality: 95,
});

let result = detector.detect(&image)?;

println!("manipulated {}", result.is_manipulated);
println!("score {:.1}%", result.overall_score * 100.0);
println!("{}", result.summary);

for m in &result.manipulations {
    println!("{:?} at {:?} ({:.0}%)", m.manipulation_type, m.region, m.confidence * 100.0);
    for evidence in &m.evidence {
        println!("    {evidence}");
    }
}

result.visualization.save("output/splicing.png")?;
```

`detect` comes from the `Detector` trait, which must be in scope.

Requires at least `2 * block_size` in both dimensions.

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `block_size` | `16` | Tile size for the colour and edge sweeps |
| `color_sensitivity` | `0.5` | Scales both the inconsistency map and the 0.3 flagging cutoff |
| `edge_sensitivity` | `0.5` | Higher lowers the edge-regularity cutoff, flagging more |
| `min_region_size` | `1000` | Merged regions smaller than this many pixels are dropped |
| `ela_quality` | `95` | Passed to the internal `ElaAnalyzer` |

</div>

`noise_sensitivity` is currently carried in the config but not wired into the
internal `NoiseAnalyzer`.

## Results

Returns a `DetectionResult` — see [Types](/docs/api/types/). Each
`DetectedManipulation` carries `ManipulationType::Splicing`, its region, a
confidence of 0.25 per corroborating signal, and an `evidence` list naming the
signals that fired.

The visualization boxes each region, shading from green (low confidence)
towards red (high).

## Interpreting the output

- **`evidence` with three or four entries** is much stronger than two. The
  confidence is literally 0.25 per signal, so read it as a count.
- **Colour + ELA together** is the classic splice signature: different source
  image, different compression history.
- **Noise + edge together** suggests a pasted boundary that was not blended.
- **`is_manipulated`** is just `overall_score > 0.3` — treat it as a hint, not
  a verdict.

## Limitations

<div class="warning">

- **`min_region_size` of 1000 px hides small splices.** A pasted face or licence
  plate can easily be under that; lower it if you are looking for small
  insertions.
- **Colour histogram comparison flags legitimate variety.** A photograph with a
  blue sky and a green field has tiles that differ sharply from the global
  histogram, and both will flag.
- **Requiring two signals suppresses real single-signal splices** as well as
  false positives. That trade is deliberate but it is a trade.
- **A resave at uniform quality erases the ELA and much of the noise
  evidence**, which typically drops a genuine splice below the two-signal bar.
- **Runs ELA and noise analysis internally**, so it costs roughly the sum of
  those two plus its own sweeps.

</div>

## See also

- [Copy-move](/docs/modules/copy-move/) — duplication *within* one image
- [Tampering Detection](/docs/modules/tampering/) — runs this plus copy-move and retouching
- [ELA](/docs/modules/ela/), [Noise](/docs/modules/noise/) — the components
