---
layout: docs.liquid
title: Types
description: The shared types every module produces or consumes.
---

## `SRegion`

An axis-aligned rectangle in pixel coordinates. Every module that reports a
location reports it as an `SRegion`, and every one it returns is clipped to the
image bounds.

```rust
pub struct SRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
```

Derives `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize`.

<div class="table-wrap">

| Method | Returns | Notes |
|--------|---------|-------|
| `new(x, y, w, h)` | `SRegion` | |
| `clipped(x, y, size, img_w, img_h)` | `SRegion` | A `size`×`size` block clipped to the image; empty if the origin is outside |
| `right()` / `bottom()` | `u32` | One past the last covered column / row. Saturating |
| `area()` | `u64` | Widened, so large regions cannot overflow |
| `is_empty()` | `bool` | True when either dimension is zero |
| `center()` | `(u32, u32)` | |
| `overlaps(&other)` | `bool` | Shares at least one pixel. Exclusive at a shared edge |
| `is_adjacent_within(&other, gap)` | `bool` | Overlapping, or separated by at most `gap` px |
| `union(&other)` | `SRegion` | Bounding box of both |
| `clamp_to(w, h)` | `SRegion` | Clipped to an image of that size |
| `pixels()` | `impl Iterator<Item = (u32, u32)>` | Row-major coordinates. Takes `self` |

</div>

### `merge_regions`

```rust
pub fn merge_regions(regions: Vec<SRegion>, gap: u32) -> Vec<SRegion>
```

Collapses regions into connected clusters — joining when they overlap or sit
within `gap` pixels — and replaces each cluster with its bounding box. Merging
is transitive: absorbing a neighbour grows the box, which can pull further
regions in, and the sweep repeats until nothing else joins.

Every module funnels its region output through this, so merge semantics are
identical across the crate.

## `AnalysisConfig`

```rust
pub struct AnalysisConfig {
    pub ela_quality: u8,           // 95
    pub block_size: u32,           // 16
    pub similarity_threshold: f64, // 0.95
    pub min_match_distance: u32,   // 50
}
```

See [Configuration](/docs/configuration/).

## `ForensicsAnalyzer`

The bundled pipeline over ELA, copy-move, noise and JPEG analysis.

```rust
impl ForensicsAnalyzer {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self>;
    pub fn from_image(image: DynamicImage) -> Self;
    pub fn with_config(self, config: AnalysisConfig) -> Self;

    pub fn ela(&self, quality: u8) -> Result<ElaResult>;
    pub fn detect_copy_move(&self) -> Result<CopyMoveResult>;
    pub fn analyze_noise(&self) -> Result<NoiseResult>;
    pub fn analyze_jpeg(&self) -> Result<JpegAnalysisResult>;
    pub fn extract_metadata(&self) -> Result<MetadataResult>;
    pub fn full_analysis(&self) -> Result<FullAnalysisReport>;
}
```

`extract_metadata` fails with `MetadataError` on an analyzer built via
`from_image`: EXIF lives in the file container, not the decoded pixels.

<div class="note">

`detect_copy_move` was previously spelled `detect_cop_move`. The old name is
gone rather than deprecated, since the crate is pre-1.0.

</div>

## Result types

### `ElaResult`

```rust
pub struct ElaResult {
    pub image: RgbImage,              // amplified difference, for viewing
    pub difference_map: GrayImage,    // amplified single-channel
    pub max_difference: f64,          // raw units
    pub mean_difference: f64,         // raw units
    pub std_deviation: f64,           // raw units
    pub suspicious_regions: Vec<SRegion>,
}
```

`save(path)` writes `image`.

The three scalars are all in *raw* difference units, matching one another.
They previously mixed scales — the mean was read off the 10× amplified,
`u8`-saturated map while the variance came from the raw values — so
`std_deviation` and the region threshold were not comparable quantities.

### `CopyMoveResult`

```rust
pub struct CopyMoveResult {
    pub matches: Vec<MatchPair>,
    pub visualization: RgbImage,
    pub confidence: f64, // mean similarity of the retained matches
}

pub struct MatchPair {
    pub source: SRegion,
    pub target: SRegion,
    pub similarity: f64, // Pearson correlation of the DCT features, >= 0
}
```

### `NoiseResult`

```rust
pub struct NoiseResult {
    pub noise_map: GrayImage,
    pub local_variance_map: GrayImage,
    pub inconsistency_score: f64,   // fraction of blocks flagged, [0, 1]
    pub estimated_noise_level: f64, // MAD * 1.4826
    pub anomalous_regions: Vec<SRegion>,
}
```

### `JpegAnalysisResult`

```rust
pub struct JpegAnalysisResult {
    pub quality_estimate: u8,
    pub ghost_detected: bool,
    pub ghost_quality: Option<u8>,
    pub ghost_map: Option<GrayImage>,
    pub blocking_artifact_map: GrayImage,
    pub double_compression_likelihood: f64,
}
```

`ghost_quality` and `ghost_map` are `Some` exactly when `ghost_detected` is
true. `ghost_quality` is new: previously the detected quality was computed and
discarded.

### `MetadataResult`

```rust
pub struct MetadataResult {
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub software: Option<String>,
    pub date_time: Option<String>,
    pub gps_coordinates: Option<(f64, f64)>, // decimal degrees, (lat, lon)
    pub all_tags: HashMap<String, String>,
    pub suspicious_indicators: Vec<String>,
}
```

Strings are plain text. `all_tags` keys are the tag name for the primary IFD
and `Thumbnail.<tag>` for the thumbnail IFD, so entries present in both are no
longer overwritten. See [Metadata](/docs/modules/metadata/).

### `FullAnalysisReport`

```rust
pub struct FullAnalysisReport {
    pub ela: ElaResult,
    pub copy_move: CopyMoveResult,
    pub noise: NoiseResult,
    pub jpeg: JpegAnalysisResult,
    pub metadata: Option<MetadataResult>,
    pub tampering_probability: f64,
}
```

<div class="note">

The field was previously named `tampering_ability`, and its value was computed
by dividing the accumulated score by the weight of the signals that *fired* —
so an image whose only positive was `ghost_detected` scored `0.1 / 0.1 = 1.0`.
It is now divided by the fixed total weight of all four signals.

</div>

## Detection types

Defined in `detection`, produced by `SplicingDetector` and `TamperingDetector`.

```rust
pub enum ConfidenceLevel { None, Low, Medium, High, VeryHigh }

pub enum ManipulationType {
    CopyMove, Splicing, Retouching, Removal,
    Resizing, Rotation, ColorManipulation, AIGenerated, Unknown,
}

pub struct DetectedManipulation {
    pub manipulation_type: ManipulationType,
    pub region: SRegion,
    pub confidence: f64,
    pub confidence_level: ConfidenceLevel,
    pub description: String,
    pub evidence: Vec<String>,
}

pub struct DetectionResult {
    pub manipulations: Vec<DetectedManipulation>,
    pub overall_score: f64,
    pub overall_confidence: ConfidenceLevel,
    pub is_manipulated: bool, // overall_score > 0.3
    pub visualization: RgbImage,
    pub summary: String,
}
```

`ConfidenceLevel::from_score` bands at 0.2 / 0.4 / 0.6 / 0.8.

No module currently emits `AIGenerated`; the variant is reserved.

## Report types

`report::JsonReport` is a serialisable summary built with
`JsonReport::from(&report)` and rendered by `to_json()`. It carries the scalar
findings and region counts, not the image buffers.

## Drawing helpers

`draw` holds the overlay primitives. All take signed coordinates and clip
internally, so an endpoint outside the image is skipped rather than wrapped.

```rust
pub fn line(image: &mut RgbImage, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb<u8>);
pub fn rect(image: &mut RgbImage, region: &SRegion, color: Rgb<u8>, thickness: u32);
pub fn fill(image: &mut RgbImage, region: &SRegion, color: Rgb<u8>, alpha: f32);
pub fn arrow(image: &mut RgbImage, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb<u8>);
pub fn crosshair(image: &mut RgbImage, cx: i32, cy: i32, radius: i32, color: Rgb<u8>);
```

## Image utilities

`image_utils` holds the primitives shared by every module.

<div class="table-wrap">

| Function | Purpose |
|----------|---------|
| `rgb_to_gray(&RgbImage)` | BT.601 luma, rounded |
| `luma(r, g, b)` | Luma of one triple |
| `ensure_min_dimensions(w, h, min)` | `Err(ImageTooSmall)` when either edge is short |
| `full_blocks(w, h, size, stride)` | Complete tiles only; empty when the image is smaller than one tile |
| `clipped_blocks(w, h, size, stride)` | Tiles the whole image, clipping at the edges |
| `sobel(&GrayImage, x, y)` | `(gx, gy)` with edge replication |
| `sobel_polar(&GrayImage, x, y)` | `(magnitude, angle)` |
| `angle_to_u8` / `u8_to_angle` | Pack an angle in `[-PI, PI]` into a map and back |
| `sample_clamped(&GrayImage, x, y)` | Edge-replicating sample at signed coordinates |
| `convolve_gray`, `gaussian_blur_3x3` | 3×3 convolution with edge replication |
| `extract_block(&GrayImage, x, y, size)` | Always `size * size` long; replicates at the border |
| `block_mean`, `block_variance` | Over a block slice |
| `mean_and_variance(&[f64])` | Population statistics |
| `median(&[f64])` | Without mutating the caller's slice |
| `calculate_histogram(&GrayImage)` | 256-bin |
| `gray_to_array`, `array_to_gray`, `normalize_to_u8` | `ndarray` interop |

</div>

There is exactly one `sobel` in the crate. Four private copies previously
existed and one of them had an inverted sign in the X kernel, so every gradient
direction that module produced pointed the wrong way.
