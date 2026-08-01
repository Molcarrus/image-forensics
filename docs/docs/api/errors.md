---
layout: docs.liquid
title: Errors
description: The ForensicsError enum, when each variant appears, and how to handle it.
---

## `ForensicsError`

```rust
#[derive(Error, Debug)]
pub enum ForensicsError {
    #[error("Image loading error: {0}")]
    ImageLoad(#[from] image::ImageError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Analysis failed: {0}")]
    AnalysisFailed(String),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("Metadata extraction error: {0}")]
    MetadataError(String),

    #[error("Block size must be smaller than image dimensions")]
    InvalidBlockSize,

    #[error("Image too small for analysis (minimum: {0}x{0})")]
    ImageTooSmall(u32),
}

pub type Result<T> = std::result::Result<T, ForensicsError>;
```

## When each variant appears

<div class="table-wrap">

| Variant | Raised by |
|---------|-----------|
| `ImageLoad` | `image::open`, decode failures, and the JPEG round-trips inside ELA and JPEG analysis |
| `Io` | Opening a file for EXIF extraction |
| `InvalidParameter` | `CopyMoveDetector::new` with a block size outside 4–64 |
| `AnalysisFailed` | PCA when no patches could be extracted, or when there are fewer samples than requested components |
| `UnsupportedFormat` | Reserved; not currently produced |
| `MetadataError` | EXIF read failures, and `extract_metadata` on an analyzer built with `from_image` |
| `InvalidBlockSize` | Reserved; not currently produced |
| `ImageTooSmall(n)` | Any analyzer whose minimum edge length `n` the image does not meet |

</div>

## `ImageTooSmall`

The most common error in practice. The payload is the minimum edge length in
pixels, which varies by module and by configured block size — see the
[minimum sizes table](/docs/configuration/#minimum-image-sizes).

```rust
use image_forensics::{analysis::pca_analysis::PcaAnalyzer, error::ForensicsError};

match PcaAnalyzer::new().analyze(&image) {
    Ok(result) => { /* ... */ }
    Err(ForensicsError::ImageTooSmall(min)) => {
        eprintln!("PCA needs at least {min}x{min}; skipping");
    }
    Err(err) => return Err(err),
}
```

Skipping a module that returns this is usually the right response when sweeping
a whole set over one image.

<div class="note">

Modules that used to compute block bounds as `0..height - block_size`
underflowed on small images instead of returning this error — a panic in debug
and a ~4×10⁹-iteration loop in release. The histogram module had no size check
at all and was reachable this way from any image under 64 px. Every module now
either guards with `ensure_min_dimensions` or iterates blocks through
`full_blocks`, which yields nothing rather than underflowing.

</div>

## Metadata errors

EXIF handling distinguishes three outcomes, which used to collapse into one:

```rust
match ExifExtractor::extract("photo.jpg") {
    // Parsed successfully.
    Ok(md) if !md.all_tags.is_empty() => { /* ... */ }

    // Valid file, genuinely no EXIF block. `suspicious_indicators` carries
    // "No EXIF data found".
    Ok(_stripped) => { /* ... */ }

    // Unreadable path, truncated file, malformed IFD.
    Err(err) => eprintln!("could not read metadata: {err}"),
}
```

Reporting a corrupt file as "no metadata found" conflates two opposite forensic
conclusions, so a genuine read failure is now an `Err`.

## Propagating

`ForensicsError` implements `std::error::Error`, so it converts into
`Box<dyn Error>` and `anyhow::Error` freely:

```rust
fn analyze() -> Result<(), Box<dyn std::error::Error>> {
    let image = image::open("photo.jpg")?;           // ImageLoad
    let detector = CopyMoveDetector::new(16, 0.95, 50)?; // InvalidParameter
    let result = detector.detect(&image)?;           // ImageTooSmall
    println!("{}", result.matches.len());
    Ok(())
}
```

`image::ImageError` and `std::io::Error` convert in automatically via `#[from]`,
so `?` works directly on `image::open` and `File::open`.
