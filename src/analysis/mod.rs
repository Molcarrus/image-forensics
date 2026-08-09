//! The sixteen individual analysis modules.
//!
//! Each is independent and follows the same shape: `Analyzer::new()` for the
//! defaults, `Analyzer::with_config(..)` to tune, and `analyze(&image)`
//! returning a module-specific result. [`copy_move`] and
//! [`resampling_detection`] use `detect` instead, for historical reasons.
//!
//! Grouped by the trace they exploit:
//!
//! - **Compression** — [`ela`], [`jpeg_analysis`], [`dct_analysis`], [`benford_analysis`]
//! - **Sensor** — [`prnu_analysis`], [`cfa_analysis`], [`noise`]
//! - **Optical** — [`chromatic_aberration`], [`luminance_gradient`], [`shadow_analysis`]
//! - **Geometric** — [`copy_move`], [`resampling_detection`]
//! - **Statistical** — [`histogram_analysis`], [`pca_analysis`]

/// First-digit statistics of DCT coefficients, tested against Benford's Law.
pub mod benford_analysis;
/// Colour filter array demosaicing traces, and where they break.
pub mod cfa_analysis;
/// Per-channel lens dispersion, fitted to a radial model.
pub mod chromatic_aberration;
/// Regions duplicated within one image.
pub mod copy_move;
/// Frequency-domain view: quantisation tables and coefficient histograms.
pub mod dct_analysis;
/// Error Level Analysis: where recompression error is inconsistent.
pub mod ela;
/// Combs, gaps and clipping left by levels and curves adjustments.
pub mod histogram_analysis;
/// Quality estimation, JPEG ghost detection and the blocking grid.
pub mod jpeg_analysis;
/// Shading direction, and blocks that disagree with the dominant lighting.
pub mod luminance_gradient;
/// The sensor noise floor, and regions that depart from it.
pub mod noise;
/// Patches that reconstruct poorly from the image's own principal subspace.
pub mod pca_analysis;
/// Photo-response non-uniformity: the sensor's fixed-pattern fingerprint.
pub mod prnu_analysis;
/// Periodic interpolation residue left by scaling or rotation.
pub mod resampling_detection;
/// Shadow segmentation and per-shadow light direction.
pub mod shadow_analysis;
