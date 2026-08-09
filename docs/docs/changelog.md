---
layout: docs.liquid
title: Changelog
description: Notable changes, including the correctness fixes that alter results from previous revisions.
---

## Unreleased

A correctness and consolidation pass. **Results from several modules differ from
previous revisions**, in most cases because the previous results were wrong.

### Breaking

- `ForensicsAnalyzer::detect_cop_move` renamed to `detect_copy_move`.
- `FullAnalysisReport::tampering_ability` renamed to `tampering_probability`.
- `ChromaticAbberationConfig` renamed to `ChromaticAberrationConfig`.
- `AnalysisConfig::parallel` removed — nothing read it. Use `RAYON_NUM_THREADS`.
- `ElaAnalyzer::with_threshold` removed — the field it set was never read. The
  region cutoff is derived from the difference distribution; use
  `with_block_size` for granularity.
- `SRegion` moved to the `region` module. It is re-exported from the crate root,
  so `image_forensics::SRegion` still resolves.
- `JpegAnalysisResult` gained a `ghost_quality: Option<u8>` field.
- `Visualizer::visulaize_full_analysis` renamed to `visualize_full_analysis`.
- Every public item now carries a doc comment; the crate builds under
  `#![warn(missing_docs)]`. Run `cargo doc --open` for the API reference.
- Private typos corrected: `weiner_filter`, `find_incosistent_regions`,
  `find_anomlaies`, `calculate_laplcaian_variance`,
  `calculate_mainpulation_probability`, `create_abberation_map`.

### Fixed — wrong results

- **EXIF GPS dropped the seconds component.** Coordinates were parsed out of a
  rendered display string by whitespace position; the seconds index landed on
  the literal word `min`, and the failed parse fell through to `0.0`. Every
  coordinate was truncated to whole arc-minutes — up to 1.85 km of error,
  reported as exact. Now read directly from the rationals.
- **EXIF strings carried literal quote characters.** `display_value()` wraps
  ASCII in double quotes, so `camera_make` was the seven-character `"Canon"`.
- **EXIF tags overwrote each other.** `all_tags` was keyed on tag name alone,
  so thumbnail-IFD entries replaced primary-IFD ones. Keys are now namespaced.
- **EXIF read errors were reported as "no metadata found."** A corrupt file and
  a clean file with no EXIF are opposite conclusions; they now differ.
- **DCT analysed the wrong region of non-square images.** The block row count
  came from the image *width*, so tall images were analysed only down to their
  top square and wide images produced phantom blocks past the bottom edge.
- **The tampering probability could reach 1.0 from one weak signal.** The score
  was normalised by the weight of the signals that *fired*, so an image whose
  only positive was `ghost_detected` scored `0.1 / 0.1 = 1.0`.
- **JPEG ghost detection was unreachable.** It took the global minimum of a
  monotonically falling curve, then required it to be below a quality its own
  search range excluded. `ghost_detected` could never be true. It now looks for
  an interior local minimum, which is what a ghost actually is.
- **JPEG double-compression scored the opposite of periodicity.** It binned
  diagonal pixel differences into an array named `dct_histogram` — no DCT
  involved — and scored `1 - mean(|h1-h2|/(h1+h2))`, which is maximised by a
  *flat* histogram.
- **DCT periodicity was pinned near 1.0.** An uncentred, unnormalised
  correlation routinely exceeded 1 and was clamped, putting a permanent 0.4
  floor under the double-compression probability.
- **The Sobel X kernel was inverted in `luminance_gradient`**, so every gradient
  direction that module produced pointed the wrong way.
- **The dominant lighting direction was arithmetic nonsense** —
  `(bin / bins) + 2*PI - PI`, missing a multiplication — always landing in
  `[PI, PI+1)` regardless of the data.
- **Chromatic aberration measured a cosine similarity, not a correlation.**
  Without mean subtraction it sits near 1.0 for every candidate shift, so the
  arg-max it selected was noise.
- **Chromatic aberration pinned the optical centre to the frame centre**, while
  reporting it as a detected value — a displaced centre being the actual signal.
- **`k_blue` was computed and left out of the model residual.**
- **PRNU `compare_patterns` indexed columns by the row count**, giving the wrong
  mean for non-square overlaps and panicking when height exceeded width.
- **CFA under-scored the GRBG pattern** on every tile via a weight that matched
  no arm of the scoring function.
- **PCA `variance_ratios` always summed to 1.0** because the total came from the
  three extracted eigenvalues rather than the covariance trace.
- **PCA `anomaly_threshold` had no effect on region selection** — the computed
  threshold was discarded for a magic constant.
- **ELA statistics mixed two scales.** The mean came from the amplified,
  saturated map; the variance from the raw values.
- **Shadow light-source counting ignored the wraparound**, splitting any cluster
  straddling 0 rad in two.
- **`merge_regions` transposed a field in one copy**, building merged heights
  from `a.y + a.width`.
- **Region heights were averaged instead of clamped** in `noise` and
  `tampering` — `block_size.midpoint(...)` for `block_size.min(...)`.
- **Copy-move used a 1-D FFT over a flattened block** and called it a DCT, and
  bucketed features by a hash perturbation that only touched the low two bits.
  Now a proper separable 2-D DCT-II with zig-zag feature selection.
- **`estimate_resampling_factor` returned the raw autocorrelation lag** as if it
  were a scaling factor, and never consulted `min_factor`/`max_factor`.
- **`estimate_gamma` was derived from the mean and returned unconditionally**,
  producing a plausible number for essentially every image.

### Fixed — panics and hangs

- **Histogram analysis panicked or hung on any image under 64 px.** It had no
  size guard and computed `0..height - block_size`, underflowing to roughly
  4 billion iterations in release.
- **Chromatic aberration's visualization could loop forever.** Shift vectors
  pointing left or up produced a negative endpoint cast to `u32`.
- **`convolve_gray` underflowed** on images narrower or shorter than 2 px.
- **PRNU `compare_patterns` read out of bounds** on tall inputs.

### Changed

- Region geometry consolidated into `region::SRegion` and `merge_regions`,
  replacing five divergent private copies — one of which carried the transposed
  field above.
- One `image_utils::sobel`, replacing four private copies.
- One `draw` module for lines, rectangles, fills, arrows and crosshairs,
  replacing four private Bresenham implementations with inconsistent guards.
  All take signed coordinates and clip internally.
- `full_blocks` and `clipped_blocks` replace the hand-rolled block loops that
  were the source of the underflow bugs.
- `ensure_min_dimensions` applied consistently; undersized input returns
  `ImageTooSmall` rather than panicking.
- Bilateral filter output clamped to 255 rather than 250, which had crushed the
  brightest 2% of the range into a false plateau.
- Noise local-variance window now centred at the borders instead of biased down
  and right.

### Performance

- Chromatic aberration: coarse-to-fine shift search and `rayon` across tiles,
  replacing an exhaustive 1089-correlations-per-tile search.
- PCA: flat sum/count buffers replace one heap allocation per pixel per map.
- Benford: the 8×8 DCT grid is computed once and shared between the global and
  per-tile passes.
- JPEG: one recompression sweep feeds both quality estimation and ghost
  detection, halving the encode/decode round-trips.
- Shadow: percentile from a 256-bin histogram instead of sorting every pixel.
- Splicing: region dedup via a hash set rather than a linear scan.

### Dependencies

Removed `ndarray-linalg` (which links a BLAS/LAPACK backend and was a common
cause of Windows build failures), `ndarray-stats`, `statrs`, `parking_lot`,
`log`, `num-complex`, `rustfft`, and the `criterion`/`tempfile` dev-dependencies.
All were unreferenced, or became so with the copy-move rewrite.

### Tests and lints

- 100 tests, from 1.
- `cargo clippy --all-targets` clean, from 84 warnings.

### Documentation

- This site: previously only layout fragments and one post existed, with no
  site config, no base layout, and a sidebar linking 24 pages that did not
  exist. Cobalt does not chain layouts, so the `extends:` front matter in the
  layouts was being emitted as literal text.
- The copy-move example in the README and the introductory post did not compile:
  it called `analyze` (the method is `detect`), iterated the result rather than
  `result.matches`, and used `println` without the `!`.
