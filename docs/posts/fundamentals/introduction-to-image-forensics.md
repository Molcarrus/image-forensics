---
layout: post.liquid
title: "Introduction to Image Forensics"
description: "What the traces are, why they survive, and how this library looks for them."
published_date: 2025-01-15 09:00:00 +0000
data:
  category: "fundamentals"
  reading_time: 8
  tags:
    - fundamentals
    - overview
---

Every image carries a record of how it was made. A lens bends light in a way
that varies with distance from its centre. A sensor has a fixed pattern of
per-pixel gain variation. A colour filter array interpolates two of every three
channel values. A JPEG encoder quantises frequency coefficients onto a grid.

None of these are visible. All of them are measurable — and all of them are
*global*: they apply uniformly across an authentic frame. Editing is almost
always *local*. That mismatch is the whole basis of image forensics.

## Four families of trace

### Compression

JPEG divides the image into 8×8 blocks, transforms each into the frequency
domain, and divides the coefficients by a quantisation table. Surviving
coefficients cluster on multiples of the quantisation step.

Paste a region from an image saved at a different quality and its coefficients
sit on a different grid. Recompressing the composite moves that region more
than its surroundings — the principle behind
[Error Level Analysis](/docs/modules/ela/). Save an image twice and the second
quantisation leaves periodic gaps in the coefficient histogram, which
[DCT analysis](/docs/modules/dct/) and
[JPEG ghost detection](/docs/modules/jpeg/) look for.

### Sensor

Manufacturing variation gives every photosite a slightly different response.
That pattern — photo-response non-uniformity — is stable across every frame a
sensor produces, which is why [PRNU](/docs/modules/prnu/) is sometimes called a
camera fingerprint.

Separately, most cameras capture one colour per photosite through a Bayer
filter and interpolate the rest. The interpolation leaves a periodic
correlation structure that [CFA analysis](/docs/modules/cfa/) looks for.

### Optical and physical

A lens disperses wavelengths, displacing the red and blue channels relative to
green by an amount that grows radially from the optical centre. Composited
content rarely carries the right displacement for its position, which
[chromatic aberration analysis](/docs/modules/chromatic-aberration/) exploits.

Similarly, a scene lit by one dominant source casts shadows in one direction
and shades surfaces consistently — assumptions that
[shadow analysis](/docs/modules/shadow/) and the
[luminance gradient](/docs/modules/luminance-gradient/) test. Humans are
notoriously poor at judging shadow consistency by eye, which makes it a good
candidate for automation.

### Geometric and statistical

Two more direct signals. [Copy-move detection](/docs/modules/copy-move/) finds
regions duplicated within one image — the signature of clone-stamping.
[Resampling detection](/docs/modules/resampling/) finds the periodic
interpolation residue that scaling or rotating leaves behind, which is how a
pasted object usually gets sized to fit.

## Why no single trace is enough

Each of these methods has a benign explanation that produces the same signal.

- ELA lights up on texture and sharp edges in perfectly authentic images.
- Double compression happens to every photograph that passes through a
  messaging app.
- Noise varies naturally between shadows and highlights, and modern phones
  denoise different parts of a frame differently.
- CFA traces are destroyed by any resize, and by most JPEG compression.
- Multiple light sources are ordinary indoors.

Any one of these will fire on a large fraction of untouched photographs. The
usable signal comes from **agreement between methods that fail for unrelated
reasons**. If the compression history, the noise floor and the colour
statistics all change at the same rectangle, that rectangle is worth
investigating. If one score is high and nothing else concurs, the most likely
explanation is the benign one.

This is why the [splicing detector](/docs/modules/splicing/) requires at least
two of its four signals to agree before reporting a region at all.

## Running it

```rust
use image_forensics::{analysis::copy_move::CopyMoveDetector, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/copy_move.png")?;

    // block_size, similarity_threshold, min_distance
    let detector = CopyMoveDetector::new(16, 0.92, 50)?;
    let result = detector.detect(&image)?;

    result.visualization.save("output/copy_move_result.png")?;

    println!("Matching regions found: {}", result.matches.len());
    println!("Confidence: {:.1}%", result.confidence * 100.0);

    for (i, pair) in result.matches.iter().enumerate() {
        println!(
            "{}. source ({}, {}) -> target ({}, {}) | similarity {:.1}%",
            i + 1,
            pair.source.x,
            pair.source.y,
            pair.target.x,
            pair.target.y,
            pair.similarity * 100.0,
        );
    }

    Ok(())
}
```

![Copy-move detection output](/assets/img/copy_move_result.png)

A genuine copy-move usually appears as a *cluster* of matches sharing one
offset vector: a cloned area produces many block pairs all displaced by the
same amount. Scattered isolated matches with no common offset are more likely
coincidence in repetitive texture — brickwork, foliage, a crowd.

## What this does not tell you

These are heuristics with hand-chosen thresholds, not measurements with error
bars. They point at regions worth a human look. Real forensic practice needs
validated tooling, documented methodology and chain of custody, and none of
that is what this library provides.

Read the *Limitations* section on each [module page](/docs/modules/) before
drawing a conclusion from its output. Every one of them says, in different
words, the same thing: establish what ordinary processing the image has already
been through, because ordinary processing produces most of these signals.

## Where to start

- [Getting started](/getting-started/) — installation through to a first result
- [Analysis modules](/docs/modules/) — what each method detects and cannot detect
- [Configuration](/docs/configuration/) — what each tunable actually controls
