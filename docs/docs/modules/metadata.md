---
layout: docs.liquid
title: Metadata (EXIF)
description: Extracts camera, software, timestamp and GPS metadata, and flags what does not add up.
---

## What it does

EXIF is the cheapest evidence in the file and the first thing to read. It
records the camera that took the photo, the software that last wrote it, when
it was captured, and often where. It is also trivially editable, so its absence
proves nothing and its presence proves nothing — but *inconsistency* within it
is informative.

## Usage

```rust
use image_forensics::metadata::exif::ExifExtractor;

let metadata = ExifExtractor::extract("evidences/photo.jpg")?;

println!("{:?} {:?}", metadata.camera_make, metadata.camera_model);
println!("software {:?}", metadata.software);
println!("captured {:?}", metadata.date_time);

if let Some((lat, lon)) = metadata.gps_coordinates {
    println!("GPS {lat:.6}, {lon:.6}");
}

for indicator in &metadata.suspicious_indicators {
    println!("  {indicator}");
}

for (tag, value) in &metadata.all_tags {
    println!("{tag}: {value}");
}
```

Or through the bundled analyzer, which requires a path:

```rust
let analyzer = ForensicsAnalyzer::new("photo.jpg")?;
let metadata = analyzer.extract_metadata()?;
```

`ForensicsAnalyzer::from_image` has no path, so `extract_metadata` returns
`ForensicsError::MetadataError` — EXIF lives in the file container, not the
decoded pixels.

## Results

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

`all_tags` keys are the tag name for the primary IFD and `Thumbnail.<tag>` for
the thumbnail IFD.

## Indicators

<div class="table-wrap">

| Indicator | Meaning |
|-----------|---------|
| `No EXIF data found` | The file parsed, but carries no EXIF block |
| `Edited with: <software>` | The `Software` tag names a known image editor |
| `Original datetime missing while file datetime is present` | `DateTime` exists but `DateTimeOriginal` does not — consistent with a re-save that dropped capture metadata |
| `DateTimeOriginal (..) differs from DateTimeDigitized (..)` | Capture and digitisation times disagree |
| `Camera make and model absent from EXIF` | EXIF is present but the camera identity is not |

</div>

The datetime discrepancy is reported as **context, not evidence**: scanned film
legitimately has a capture date decades before its digitisation date.

## Three fixes worth knowing about

<div class="warning">

**GPS coordinates were wrong by up to 1.85 km, and reported as exact.**

`kamadak-exif` renders `GPSLatitude` through a formatter that produces
`55 deg 41 min 30.5 sec`. The old parser split that on whitespace and read the
seconds from index 3 — which is the literal word `min`. The parse failed into
`unwrap_or(0.0)`, silently discarding the seconds and truncating every
coordinate to whole arc-minutes.

The tag value is three rationals. They are now read directly:

```rust
Some(degrees + minutes / 60.0 + seconds / 3600.0)
```

Coordinates are also range-checked, and the hemisphere is read from the
reference tag's bytes rather than by substring-matching a rendered string.

</div>

<div class="note">

**String fields carried literal quote characters.** `Field::display_value`
routes ASCII through a formatter that wraps the text in double quotes and
escapes non-printables, so `camera_make` came back as the seven-character
`"Canon"` — quotes included. Fields are now decoded from their bytes, with
trailing NUL padding stripped.

**Tags were silently overwritten.** `all_tags` was keyed on the tag name alone,
but Make, Model, DateTime and the dimension tags appear in *both* the primary
and thumbnail IFDs, so the thumbnail entry replaced the primary one. Keys are
now namespaced by IFD.

**Read errors were reported as "no metadata".** A truncated file, an unreadable
path or a malformed IFD produced the same empty result as a clean image with no
EXIF block. Those are opposite forensic conclusions. Only a genuine
`NotFound` yields the empty result now; everything else is an `Err`.

</div>

## Interpreting the output

Things worth pursuing:

- **Software naming an editor** — the file was written by Photoshop, GIMP,
  Lightroom, Affinity or similar. Extremely common and often innocuous: raw
  conversion counts.
- **No EXIF at all** on something presented as a camera original. Note that
  every major social platform strips EXIF on upload, so this is the norm for
  anything downloaded from the web.
- **Make/model absent while other EXIF is present** — selective stripping.
- **GPS that contradicts the claimed location**, or a timestamp that contradicts
  the claimed date.
- **A thumbnail that does not match the main image.** The embedded JPEG
  thumbnail is often not regenerated after an edit, so it can preserve the
  original content. `all_tags` exposes the thumbnail IFD entries; extracting and
  comparing the thumbnail image itself is not implemented here.

## Limitations

<div class="warning">

- **EXIF is trivially forged.** Every field can be set to anything with freely
  available tools. Consistent metadata is not evidence of authenticity.
- **Absence is the default on the web.** Platforms strip metadata on upload;
  missing EXIF usually means "downloaded from somewhere", not "hiding
  something".
- **The editor list is a fixed substring match** over Photoshop, Lightroom,
  GIMP, Paint, Affinity and Pixelmator. Other tools, and tools that write no
  `Software` tag, pass unremarked.
- **Maker notes are not decoded.** Manufacturer-specific blocks often carry the
  richest provenance data and appear here only as an opaque value.
- **Nothing is cross-checked against the pixels.** A `Make` of "Canon" on an
  image whose sensor traces say otherwise is not detected; compare with
  [PRNU](/docs/modules/prnu/) and [CFA](/docs/modules/cfa/) by hand.

</div>

## See also

- [PRNU Analysis](/docs/modules/prnu/) — what the sensor says about the camera
- [CFA Analysis](/docs/modules/cfa/)
- [Errors](/docs/api/errors/) — distinguishing "no EXIF" from "unreadable"
