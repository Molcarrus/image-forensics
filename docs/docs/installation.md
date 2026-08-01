---
layout: docs.liquid
title: Installation
description: Requirements, adding the dependency, and building from source.
---

## Requirements

- **Rust 1.85 or newer.** The crate uses edition 2024 and `u32::div_ceil`.
- No system libraries. Every dependency is pure Rust; there is no OpenCV,
  ImageMagick or BLAS to install.

<div class="note">

Earlier revisions depended on `ndarray-linalg`, which links a BLAS/LAPACK
backend and was a frequent cause of build failures on Windows. It was
unreferenced and has been removed, along with `statrs`, `parking_lot`, `log`,
`num-complex` and `rustfft`. A plain `cargo build` now works on a clean
toolchain.

</div>

## As a dependency

The crate is not on crates.io. Depend on the Git repository:

```toml
[dependencies]
image-forensics = { git = "{{ site.data.repo.url }}" }
image = "0.25"
```

To pin a revision:

```toml
[dependencies]
image-forensics = { git = "{{ site.data.repo.url }}", rev = "abc1234" }
```

## Building from source

```bash
git clone {{ site.data.repo.url }}.git
cd image-forensics
cargo build --release
```

Run the test suite:

```bash
cargo test
```

And the lints:

```bash
cargo clippy --all-targets
```

## Running the examples

Each analysis module has a runnable example driven by a file in `evidences/`.
Create the output directory first — the examples write into it and will not
create it themselves:

```bash
mkdir -p output
cargo run --release --example copy_move
```

<div class="warning">

Always use `--release` for the examples. These modules are dense numeric loops;
a debug build runs roughly ten times slower, and the heavier ones (chromatic
aberration, PRNU, PCA) become impractically slow.

</div>

## Current dependencies

<div class="table-wrap">

| Crate | Purpose |
|-------|---------|
| `image` | Decoding, encoding and the `DynamicImage` type |
| `imageproc` | Supplementary image operations |
| `kamadak-exif` | EXIF parsing |
| `ndarray` | 2-D array helpers in `image_utils` |
| `rayon` | Parallel block processing in copy-move and chromatic aberration |
| `serde`, `serde_json` | Serialising regions and JSON reports |
| `thiserror` | The `ForensicsError` enum |

</div>

## Platform notes

Everything is portable Rust and is developed on Windows. Analysis is
CPU-bound; `rayon` will use all available cores in the modules that are
parallelised. Set `RAYON_NUM_THREADS` to limit that.
