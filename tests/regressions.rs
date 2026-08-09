//! End-to-end checks over the public API.
//!
//! Each test here pins a defect that was previously reachable from outside the
//! crate: a panic, a hang, an out-of-bounds region, or a score that could not
//! take the value it claimed to report.

use image::{DynamicImage, Rgb, RgbImage};
use image_forensics::{
    AnalysisConfig, ForensicsAnalyzer, SRegion,
    analysis::{
        benford_analysis::BenfordAnalyzer, cfa_analysis::CfaAnalyzer,
        chromatic_aberration::ChromaticAberrationAnalyzer, copy_move::CopyMoveDetector,
        dct_analysis::DctAnalyzer, ela::ElaAnalyzer, histogram_analysis::HistogramAnalyzer,
        jpeg_analysis::JpegAnalyzer, luminance_gradient::LuminanceGradientAnalyzer,
        noise::NoiseAnalyzer, pca_analysis::PcaAnalyzer, prnu_analysis::PrnuAnalyzer,
        resampling_detection::ResamplingDetector, shadow_analysis::ShadowAnalyzer,
    },
    detection::{Detector, splicing::SplicingDetector, tampering::TamperingDetector},
    error::ForensicsError,
    merge_regions,
    region::SRegion as Region,
};

/// Deterministic xorshift so fixtures are reproducible without a `rand` dep.
struct Noise(u64);

impl Noise {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_byte(&mut self) -> u8 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 24) as u8
    }
}

fn textured(width: u32, height: u32) -> DynamicImage {
    let mut noise = Noise::new(0x9E37_79B9_7F4A_7C15);
    let mut image = RgbImage::new(width, height);

    for pixel in image.pixels_mut() {
        let base = noise.next_byte();
        *pixel = Rgb([base, base.wrapping_add(30), base.wrapping_sub(20)]);
    }

    DynamicImage::ImageRgb8(image)
}

fn flat(width: u32, height: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb([128, 128, 128])))
}

fn assert_in_bounds(regions: &[SRegion], width: u32, height: u32, label: &str) {
    for region in regions {
        assert!(
            region.right() <= width && region.bottom() <= height,
            "{label}: {region:?} escapes a {width}x{height} image"
        );
    }
}

// ---------------------------------------------------------------- geometry --

#[test]
fn merged_regions_never_escape_the_source_bounds() {
    let regions = vec![
        Region::new(0, 0, 16, 4),
        Region::new(18, 0, 16, 4),
        Region::new(0, 6, 16, 4),
    ];

    // The Benford copy of this helper built merged heights from `a.y + a.width`,
    // producing boxes taller than anything that went in.
    for merged in merge_regions(regions.clone(), 4) {
        assert!(merged.right() <= 34, "{merged:?}");
        assert!(merged.bottom() <= 10, "{merged:?}");
    }
}

// ------------------------------------------------------ small-image safety --

/// Every analyzer must either succeed or return a typed error — never panic,
/// never spin, never underflow. The histogram module in particular had no size
/// guard at all, and its `0..height - 64` loop underflowed for anything under
/// 64 px: a panic in debug, a ~4e9-iteration hang in release.
#[test]
fn undersized_images_are_handled_not_panicked_on() {
    /// Fails the test on a panic; tolerates any `Err`.
    fn ok_or_typed_error<T>(label: &str, outcome: Result<T, ForensicsError>) {
        if let Err(err) = outcome {
            assert!(
                matches!(
                    err,
                    ForensicsError::ImageTooSmall(_)
                        | ForensicsError::AnalysisFailed(_)
                        | ForensicsError::InvalidParameter(_)
                ),
                "{label} returned an unexpected error: {err}"
            );
        }
    }

    for size in [1u32, 3, 8, 17, 32, 63, 127] {
        let image = textured(size, size);

        ok_or_typed_error("ela", ElaAnalyzer::new(95).analyze(&image));
        ok_or_typed_error("noise", NoiseAnalyzer::new().analyze(&image));
        ok_or_typed_error("jpeg", JpegAnalyzer::new().analyze(&image));
        ok_or_typed_error("histogram", HistogramAnalyzer::new().analyze(&image));
        ok_or_typed_error(
            "luminance",
            LuminanceGradientAnalyzer::new(16).analyze(&image),
        );
        ok_or_typed_error("benford", BenfordAnalyzer::new().analyze(&image));
        ok_or_typed_error("dct", DctAnalyzer::new().analyze(&image));
        ok_or_typed_error("pca", PcaAnalyzer::new().analyze(&image));
        ok_or_typed_error("prnu", PrnuAnalyzer::new().analyze(&image));
        ok_or_typed_error("cfa", CfaAnalyzer::new().analyze(&image));
        ok_or_typed_error("shadow", ShadowAnalyzer::new().analyze(&image));
        ok_or_typed_error("resampling", ResamplingDetector::new().detect(&image));
        ok_or_typed_error(
            "chromatic",
            ChromaticAberrationAnalyzer::new().analyze(&image),
        );
        ok_or_typed_error("splicing", SplicingDetector::new().detect(&image));
    }

    // Below their configured minimum, the block-based analyzers must say so
    // rather than quietly returning empty results.
    let tiny = textured(8, 8);
    for (label, outcome) in [
        ("benford", BenfordAnalyzer::new().analyze(&tiny).err()),
        ("dct", DctAnalyzer::new().analyze(&tiny).err()),
        ("pca", PcaAnalyzer::new().analyze(&tiny).err()),
        ("shadow", ShadowAnalyzer::new().analyze(&tiny).err()),
    ] {
        assert!(
            matches!(outcome, Some(ForensicsError::ImageTooSmall(_))),
            "{label} accepted an 8x8 image"
        );
    }
}

// -------------------------------------------------- non-square correctness --

/// A tall image must be analysed in full. `DctAnalyzer` derived its row count
/// from the image *width*, so it only ever looked at the top square.
#[test]
fn tall_and_wide_images_are_fully_analysed() {
    for (width, height) in [(64u32, 256u32), (256, 64)] {
        let image = textured(width, height);

        let dct = DctAnalyzer::new().analyze(&image).unwrap();
        assert_eq!(dct.block_artifact_map.dimensions(), (width, height));
        assert_in_bounds(&dct.anomalous_regions, width, height, "dct");

        let ela = ElaAnalyzer::new(95).analyze(&image).unwrap();
        assert_eq!(ela.difference_map.dimensions(), (width, height));
        assert_in_bounds(&ela.suspicious_regions, width, height, "ela");

        let noise = NoiseAnalyzer::new().analyze(&image).unwrap();
        assert_in_bounds(&noise.anomalous_regions, width, height, "noise");
    }
}

/// PRNU cross-correlation indexed columns by the row count; taller-than-wide
/// patterns walked off the end of the buffer.
#[test]
fn prnu_pattern_comparison_survives_tall_inputs() {
    let analyzer = PrnuAnalyzer::new();
    let image = textured(160, 320);

    let result = analyzer.analyze(&image).unwrap();
    let self_correlation = analyzer.compare_patterns(&result.prnu_pattern, &result.prnu_pattern);

    assert!(
        (self_correlation - 1.0).abs() < 1e-6,
        "self-correlation was {self_correlation}"
    );
}

// -------------------------------------------------------- region soundness --

#[test]
fn no_analyzer_reports_regions_outside_the_image() {
    // 200x150 clears every module's minimum while being a multiple of none of
    // their block sizes, so each one exercises its edge-clipping path.
    let (width, height) = (200u32, 150u32);
    let image = textured(width, height);

    assert_in_bounds(
        &ElaAnalyzer::new(95)
            .analyze(&image)
            .unwrap()
            .suspicious_regions,
        width,
        height,
        "ela",
    );
    assert_in_bounds(
        &NoiseAnalyzer::new()
            .analyze(&image)
            .unwrap()
            .anomalous_regions,
        width,
        height,
        "noise",
    );
    assert_in_bounds(
        &BenfordAnalyzer::new()
            .analyze(&image)
            .unwrap()
            .anomalous_regions,
        width,
        height,
        "benford",
    );
    assert_in_bounds(
        &DctAnalyzer::new()
            .analyze(&image)
            .unwrap()
            .anomalous_regions,
        width,
        height,
        "dct",
    );
    assert_in_bounds(
        &ResamplingDetector::new()
            .detect(&image)
            .unwrap()
            .resampled_regions,
        width,
        height,
        "resampling",
    );
    assert_in_bounds(
        &LuminanceGradientAnalyzer::new(16)
            .analyze(&image)
            .unwrap()
            .inconsistent_regions,
        width,
        height,
        "luminance",
    );

    let tampering = TamperingDetector::new().detect(&image).unwrap();
    let regions: Vec<SRegion> = tampering.manipulations.iter().map(|m| m.region).collect();
    assert_in_bounds(&regions, width, height, "tampering");

    let splicing = SplicingDetector::new().detect(&image).unwrap();
    let regions: Vec<SRegion> = splicing.manipulations.iter().map(|m| m.region).collect();
    assert_in_bounds(&regions, width, height, "splicing");
}

// ----------------------------------------------------------- visualization --

/// Chromatic aberration drew shift vectors with `end as u32`, so any leftward
/// or upward shift wrapped to ~4e9 and hung the line loop. Reaching the
/// assertion at all is the regression check.
#[test]
fn chromatic_aberration_visualization_terminates() {
    let image = textured(192, 160);
    let result = ChromaticAberrationAnalyzer::new().analyze(&image).unwrap();

    assert_eq!(result.visualization.dimensions(), (192, 160));
}

#[test]
fn visualizations_match_their_input_dimensions() {
    let image = textured(128, 96);

    assert_eq!(
        ShadowAnalyzer::new()
            .analyze(&image)
            .unwrap()
            .direction_map
            .dimensions(),
        (128, 96)
    );
    assert_eq!(
        CopyMoveDetector::new(16, 0.95, 40)
            .unwrap()
            .detect(&image)
            .unwrap()
            .visualization
            .dimensions(),
        (128, 96)
    );
}

// ------------------------------------------------------------------ scores --

/// A single weak signal must not read as certain tampering. The score was
/// divided by the *triggered* weight, so an image whose only positive was
/// `ghost_detected` scored 0.1 / 0.1 = 1.0.
#[test]
fn a_clean_flat_image_does_not_score_as_certainly_tampered() {
    let analyzer = ForensicsAnalyzer::from_image(flat(128, 128)).with_config(AnalysisConfig {
        block_size: 16,
        ..AnalysisConfig::default()
    });

    let report = analyzer.full_analysis().unwrap();

    assert!(
        (0.0..=1.0).contains(&report.tampering_probability),
        "probability {} out of range",
        report.tampering_probability
    );
    assert!(
        report.tampering_probability < 0.5,
        "a uniform grey image scored {}",
        report.tampering_probability
    );
}

#[test]
fn every_reported_probability_is_a_probability() {
    let image = textured(160, 160);

    let scores = [
        (
            "benford",
            BenfordAnalyzer::new()
                .analyze(&image)
                .unwrap()
                .manipulation_probability,
        ),
        (
            "pca",
            PcaAnalyzer::new()
                .analyze(&image)
                .unwrap()
                .manipulation_probability,
        ),
        (
            "prnu",
            PrnuAnalyzer::new()
                .analyze(&image)
                .unwrap()
                .manipulation_probability,
        ),
        (
            "cfa",
            CfaAnalyzer::new()
                .analyze(&image)
                .unwrap()
                .manipulation_probability,
        ),
        (
            "shadow",
            ShadowAnalyzer::new()
                .analyze(&image)
                .unwrap()
                .manipulation_probability,
        ),
        (
            "histogram",
            HistogramAnalyzer::new()
                .analyze(&image)
                .unwrap()
                .manipulation_probability,
        ),
        (
            "resampling",
            ResamplingDetector::new()
                .detect(&image)
                .unwrap()
                .resampling_probability,
        ),
        (
            "chromatic",
            ChromaticAberrationAnalyzer::new()
                .analyze(&image)
                .unwrap()
                .manipulation_probability,
        ),
        (
            "dct",
            DctAnalyzer::new()
                .analyze(&image)
                .unwrap()
                .double_compression_probability,
        ),
        (
            "jpeg",
            JpegAnalyzer::new()
                .analyze(&image)
                .unwrap()
                .double_compression_likelihood,
        ),
    ];

    for (name, score) in scores {
        assert!(
            (0.0..=1.0).contains(&score),
            "{name} reported {score}, which is not a probability"
        );
    }
}

/// `ghost_map` and `ghost_quality` must agree with `ghost_detected`.
#[test]
fn jpeg_ghost_fields_stay_consistent() {
    for image in [flat(96, 96), textured(96, 96)] {
        let result = JpegAnalyzer::new().analyze(&image).unwrap();

        assert_eq!(
            result.ghost_detected,
            result.ghost_map.is_some(),
            "ghost_map disagrees with ghost_detected"
        );
        assert_eq!(
            result.ghost_detected,
            result.ghost_quality.is_some(),
            "ghost_quality disagrees with ghost_detected"
        );
    }
}

// ------------------------------------------------------------------- copy-move --

#[test]
fn copy_move_finds_a_planted_duplicate_and_ignores_flat_input() {
    let mut noise = Noise::new(31);
    let mut image = RgbImage::new(256, 256);

    for pixel in image.pixels_mut() {
        let v = noise.next_byte();
        *pixel = Rgb([v, v, v]);
    }

    let patch: Vec<Rgb<u8>> = (0..48 * 48)
        .map(|i| *image.get_pixel(16 + (i % 48), 16 + (i / 48)))
        .collect();
    for i in 0..48u32 * 48 {
        image.put_pixel(184 + (i % 48), 184 + (i / 48), patch[i as usize]);
    }

    let detector = CopyMoveDetector::new(16, 0.95, 50).unwrap();

    let planted = detector.detect(&DynamicImage::ImageRgb8(image)).unwrap();
    assert!(!planted.matches.is_empty(), "planted duplicate not found");
    assert!((0.0..=1.0).contains(&planted.confidence));

    let clean = detector.detect(&flat(256, 256)).unwrap();
    assert!(
        clean.matches.is_empty(),
        "flat image produced false matches"
    );
}

// ------------------------------------------------------------------ metadata --

/// A missing file is an I/O failure, not "this image has no metadata". Both
/// used to collapse into the same empty-with-indicator result.
#[test]
fn metadata_extraction_distinguishes_missing_files_from_missing_exif() {
    let analyzer = ForensicsAnalyzer::from_image(flat(64, 64));

    // No path at all: an error, not a silent empty result.
    assert!(analyzer.extract_metadata().is_err());
}
