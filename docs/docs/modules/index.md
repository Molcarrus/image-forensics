---
layout: docs.liquid
permalink: "/docs/modules/"
title: Analysis Modules
description: Seventeen independent analyses, what each detects, and how to combine them.
---

Every module is independent: it takes an `image::DynamicImage`, returns its own
result type, and knows nothing about the others. That independence is the
point — the useful signal is *agreement between methods that fail differently*,
not a high score from any single one.

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

## By what they exploit

**Compression traces** — [ELA](/docs/modules/ela/),
[JPEG](/docs/modules/jpeg/), [DCT](/docs/modules/dct/),
[Benford](/docs/modules/benford/). A JPEG carries the fingerprint of its
quantisation. A region pasted in from a differently-compressed source, or an
image saved twice, leaves that fingerprint inconsistent.

**Sensor traces** — [PRNU](/docs/modules/prnu/), [CFA](/docs/modules/cfa/),
[Noise](/docs/modules/noise/). Every sensor has a fixed pattern of pixel gain
variation and a colour filter array whose interpolation leaves a periodic
signature. Content from another camera carries the wrong ones.

**Optical traces** — [Chromatic aberration](/docs/modules/chromatic-aberration/),
[Luminance gradient](/docs/modules/luminance-gradient/),
[Shadow](/docs/modules/shadow/). A lens bends colours by an amount that grows
with distance from the optical centre; a scene has a consistent light
direction. Composites usually break both.

**Geometric traces** — [Copy-move](/docs/modules/copy-move/),
[Resampling](/docs/modules/resampling/). Duplicated regions, and the periodic
interpolation residue that scaling or rotating leaves behind.

**Statistical traces** — [Histogram](/docs/modules/histogram/),
[PCA](/docs/modules/pca/). Level adjustments comb the histogram; patches that
do not fit the image's own principal subspace stand out.

**Composite** — [Splicing](/docs/modules/splicing/),
[Tampering](/docs/modules/tampering/) run several of the above and combine
them.

## Choosing where to start

<div class="table-wrap">

| If you suspect | Start with |
|----------------|------------|
| A region was pasted from elsewhere in the same image | [Copy-move](/docs/modules/copy-move/) |
| A region was pasted from a different photo | [Splicing](/docs/modules/splicing/), [Noise](/docs/modules/noise/), [ELA](/docs/modules/ela/) |
| The image was saved, edited, and saved again | [JPEG](/docs/modules/jpeg/), [DCT](/docs/modules/dct/) |
| Something was scaled or rotated into place | [Resampling](/docs/modules/resampling/) |
| An object was added to a scene | [Shadow](/docs/modules/shadow/), [Chromatic aberration](/docs/modules/chromatic-aberration/) |
| Levels, curves or gamma were adjusted | [Histogram](/docs/modules/histogram/) |
| The image did not come from the camera it claims | [Metadata](/docs/modules/metadata/), [PRNU](/docs/modules/prnu/), [CFA](/docs/modules/cfa/) |

</div>

## Reading a score

Every module reports at least one number in `[0, 1]`. They are **not
calibrated against each other** and none is a probability in any formal sense:
each is a weighted combination of heuristics with thresholds chosen by hand.

A workable reading:

- **Below ~0.3** — nothing that method can see.
- **0.3 to 0.6** — something is unusual. Ordinary processing frequently lands
  here.
- **Above ~0.6** — worth looking at the region maps directly.

The reliable move is to run several modules and see whether they flag *the same
region*. One module at 0.8 is weak evidence. Three independent modules
converging on one rectangle is worth investigating.

<div class="warning">

**Things that trip these detectors without any manipulation:** saving a JPEG
more than once, resizing for the web, screenshotting, a phone's built-in noise
reduction and sharpening, HDR merging, panorama stitching, lens-correction
profiles, and any watermark or caption overlay. Establish what benign
processing the image has been through before reading anything into a score.

</div>

## Cost

Rough ordering on a few-megapixel image, cheapest first:

1. Histogram, luminance gradient, noise — one or two passes over the pixels.
2. ELA, Benford, DCT — a transform or a JPEG round-trip per block.
3. JPEG analysis — a sweep of full encode/decode round-trips.
4. Copy-move, splicing, tampering — per-block features plus pairwise work.
5. PCA — patch extraction and a covariance decomposition.
6. PRNU — an iterated bilateral filter over every pixel.
7. Chromatic aberration — a shift search per block. The most expensive by a
   wide margin.

Copy-move and chromatic aberration are parallelised with `rayon`; the rest are
currently single-threaded.
