---
layout: home.liquid
permalink: "/"
title: Digital image forensics for Rust
description: A Rust library for digital image forensics — sixteen analysis modules plus EXIF metadata, for detecting manipulation, forgery and inconsistency in images.
---

<div class="hero">

# Image forensics in Rust

Sixteen independent analysis modules, plus EXIF metadata, for finding the
traces that editing leaves behind: recompression, duplicated regions, spliced composites, resampling,
broken lighting, and metadata that does not match the pixels.

<div class="button-row">
<a class="button primary" href="/getting-started/">Get started</a>
<a class="button" href="/docs/modules/">Browse the modules</a>
<a class="button" href="{{ site.data.repo.url }}">GitHub</a>
</div>

</div>

## A first analysis

```rust
use image_forensics::{analysis::copy_move::CopyMoveDetector, error::Result};

fn main() -> Result<()> {
    let image = image::open("evidences/copy_move.png")?;

    let detector = CopyMoveDetector::new(16, 0.95, 50)?;
    let result = detector.detect(&image)?;

    println!("Matching regions: {}", result.matches.len());
    println!("Confidence: {:.1}%", result.confidence * 100.0);

    result.visualization.save("output/copy_move.png")?;
    Ok(())
}
```

## What it looks at

Each module targets a different physical or statistical trace. They are
deliberately independent: agreement between several is far more informative
than a high score from any one.

<ul class="module-grid">
{% for module in site.data.modules.modules %}
<li class="module-card">
    <a href="/docs/modules/{{ module.slug }}/">{{ module.name }}</a>
    <p>{{ module.description }}</p>
    <span class="cat">{{ module.category }}</span>
</li>
{% endfor %}
<li class="module-card">
    <a href="/docs/modules/metadata/">Metadata (EXIF)</a>
    <p>Camera make, model, software tags, timestamps and GPS coordinates.</p>
    <span class="cat">metadata</span>
</li>
</ul>

<div class="warning">

**These are signals, not verdicts.** Every module here reports a score derived
from heuristics with hand-chosen thresholds. A high score means "worth a human
look", not "manipulated". Compression, resizing, screenshotting and ordinary
camera processing all produce the same traces that editing does. Read the
*Limitations* section on each module page before acting on its output.

</div>

## Where to go next

- **[Installation](/docs/installation/)** — adding the crate to a project.
- **[Getting started](/getting-started/)** — running your first analysis end to end.
- **[Configuration](/docs/configuration/)** — what every tunable actually controls.
- **[API reference](/docs/api/types/)** — the shared types, traits and errors.
