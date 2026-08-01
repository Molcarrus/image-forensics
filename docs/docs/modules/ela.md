---
layout: docs.liquid
title: ELA — Error Level Analysis
description: Recompresses the image and measures where the error differs, revealing regions saved at a different quality.
---

## What it does

Saving a JPEG is lossy but *idempotent-ish*: recompressing an image at the
quality it already carries changes it very little, because the coefficients are
already sitting on that quantisation grid. A region pasted in from a source
saved at a different quality has not settled onto the same grid, so it moves
more when recompressed.

ELA recompresses the whole image at a chosen quality, takes the absolute
per-pixel difference, and amplifies it for viewing. Regions that light up are
at a different point in their compression history than their surroundings.

## Usage

```rust
use image_forensics::analysis::ela::ElaAnalyzer;

let analyzer = ElaAnalyzer::new(95)      // recompression quality
    .with_amplification(15.0)            // display gain, default 10.0
    .with_block_size(16);                // region granularity, default 16

let result = analyzer.analyze(&image)?;

println!("max  {:.2}", result.max_difference);
println!("mean {:.2}", result.mean_difference);
println!("std  {:.2}", result.std_deviation);
println!("regions {}", result.suspicious_regions.len());

result.save("output/ela.png")?;
```

## Configuration

<div class="table-wrap">

| Parameter | Default | Effect |
|-----------|---------|--------|
| `quality` | — | Recompression quality. 90–98 is the useful band |
| `amplification` | `10.0` | Display gain on `image` and `difference_map`. Visual only |
| `block_size` | `16` | Tile size for region detection |

</div>

The region threshold is not configurable: it is derived from the data as
`mean + 2 * std_deviation` over the raw differences.

## Results

```rust
pub struct ElaResult {
    pub image: RgbImage,           // amplified, for viewing
    pub difference_map: GrayImage, // amplified, single channel
    pub max_difference: f64,       // raw units
    pub mean_difference: f64,      // raw units
    pub std_deviation: f64,        // raw units
    pub suspicious_regions: Vec<SRegion>,
}
```

All three scalars are in raw difference units, so they are directly comparable:
`mean_difference` is always at most `max_difference`.

<div class="note">

These previously mixed scales. `mean_difference` was computed from the
amplified, `u8`-saturated difference map, while `std_deviation` came from the
raw values — so the reported standard deviation and the region threshold were
in different units and neither meant what it said.

</div>

## Interpreting the output

Look at `image`, not the numbers. In a healthy single-compression JPEG the ELA
image is close to uniform, with brightness concentrated on edges — edges always
carry the most quantisation error.

Signs worth pursuing:

- A rectangular region distinctly brighter or darker than its surroundings.
- Sharp text or a logo with a bright halo, when the rest of the image is dark.
- One object at a visibly different error level from everything around it.

## Choosing the quality

Sweep it. A pasted region will stand out at some qualities and not others:

```rust
for quality in [95, 90, 85] {
    let result = ElaAnalyzer::new(quality).analyze(&image)?;
    result.save(format!("output/ela_q{quality}.png"))?;
}
```

## Limitations

<div class="warning">

ELA is the most widely misread tool in image forensics. It shows *compression
history*, not editing.

- **PNG, BMP and other lossless input** has no compression history. ELA on it
  measures nothing but the first JPEG pass you just applied.
- **Texture dominates.** Detailed regions always show higher error than smooth
  ones. A bright bush next to a flat sky is normal, not evidence.
- **Resaving flattens it.** One resave of the whole composite at uniform
  quality erases the difference entirely. A negative ELA result says almost
  nothing.
- **Scaling erases it too.** Any resize resamples the pasted region onto the
  new grid.
- **Bright regions are not "the edit".** Sharp edges, high-contrast text and
  saturated colour all read bright in a perfectly authentic image.

</div>

## See also

- [JPEG Analysis](/docs/modules/jpeg/) — ghost detection across a quality sweep
- [DCT Analysis](/docs/modules/dct/) — the coefficient-domain view
- [Splicing Detection](/docs/modules/splicing/) — combines ELA with noise and colour
