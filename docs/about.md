---
layout: default.liquid
title: About
description: What this project is, what it is not, and how to contribute.
---

`image-forensics` is a Rust library that collects classical image-forensics
algorithms behind one API. It is a learning and research project, not a
certified forensic tool.

## Scope

The crate implements sixteen analysis modules plus EXIF metadata extraction.
Each is independent, takes an `image::DynamicImage`, and returns a typed result
containing maps, regions and a score. Nothing here uses a trained model or
network access; everything is deterministic arithmetic over pixels.

## What it is not

- **Not proof of anything.** The outputs are heuristic scores with hand-tuned
  thresholds. They point at regions worth a human look.
- **Not court-admissible.** Real forensic practice requires validated tooling,
  documented methodology and chain of custody. This is none of those.
- **Not a deepfake or AI-generation detector.** The `ManipulationType::AIGenerated`
  variant exists in the type system but no module currently produces it.

## Design notes

Region geometry, block iteration, Sobel gradients and overlay drawing live in
single shared implementations (`region`, `image_utils`, `draw`). Analysis
modules are expected to build on those rather than reimplement them — several
long-lived bugs came from divergent private copies of the same helper.

Scores are normalised into `[0, 1]` and every reported `SRegion` is clipped to
the image bounds. Analyzers that need a minimum image size return
`ForensicsError::ImageTooSmall` rather than panicking.

## Contributing

Issues and pull requests are welcome at
[the repository]({{ site.data.repo.url }}).

If you are adding a module, please include:

- a size guard via `image_utils::ensure_min_dimensions`,
- regions produced through `image_utils::clipped_blocks` or `full_blocks`,
- a score clamped to `[0, 1]`,
- tests with a synthetic image where the expected answer is known, and
- a *Limitations* section in the docs page saying what benign processing
  triggers it.

Run `cargo test` and `cargo clippy --all-targets` before opening a PR; both are
currently clean.

## Licence

See the repository for licence terms.
