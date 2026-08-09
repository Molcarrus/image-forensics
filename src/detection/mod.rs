//! Composite detectors that run several analyses and combine their verdicts.
//!
//! Unlike the modules in [`analysis`](crate::analysis), these share one result
//! type ([`DetectionResult`]) behind the [`Detector`] trait, so they can be held
//! as trait objects.

/// Content composited in from a different image.
pub mod splicing;
/// The broadest composite detector.
pub mod tampering;

use image::RgbImage;
use serde::{Deserialize, Serialize};

use crate::{SRegion, error::Result};

/// How strongly a finding is supported, banded from a `[0, 1]` score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfidenceLevel {
    /// Below 0.2. Nothing the method can see.
    None,
    /// 0.2 to 0.4. Ordinary processing frequently lands here.
    Low,
    /// 0.4 to 0.6.
    Medium,
    /// 0.6 to 0.8. Worth looking at the region maps directly.
    High,
    /// 0.8 and above.
    VeryHigh,
}

impl ConfidenceLevel {
    /// Band a `[0, 1]` score into a level, at 0.2 / 0.4 / 0.6 / 0.8.
    pub fn from_score(score: f64) -> Self {
        match score {
            s if s < 0.2 => ConfidenceLevel::None,
            s if s < 0.4 => ConfidenceLevel::Low,
            s if s < 0.6 => ConfidenceLevel::Medium,
            s if s < 0.8 => ConfidenceLevel::High,
            _ => ConfidenceLevel::VeryHigh,
        }
    }

    /// A representative score for the band.
    pub fn to_score(&self) -> f64 {
        match self {
            ConfidenceLevel::None => 0.0,
            ConfidenceLevel::Low => 0.3,
            ConfidenceLevel::Medium => 0.5,
            ConfidenceLevel::High => 0.7,
            ConfidenceLevel::VeryHigh => 0.9,
        }
    }
}

/// What kind of alteration a finding appears to be.
///
/// Several variants are reserved: no module currently produces `Removal`,
/// `Resizing`, `Rotation`, `ColorManipulation` or `AIGenerated`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ManipulationType {
    /// A region duplicated from elsewhere in the same image.
    CopyMove,
    /// Content composited in from a different image.
    Splicing,
    /// Local cloning, healing, blurring or sharpening.
    Retouching,
    /// Something painted out of the scene. Reserved; not currently produced.
    Removal,
    /// Geometric rescaling. Reserved; not currently produced.
    Resizing,
    /// Geometric rotation. Reserved; not currently produced.
    Rotation,
    /// Tonal or colour adjustment. Reserved; not currently produced.
    ColorManipulation,
    /// Synthetic imagery. Reserved; no module currently emits this.
    AIGenerated,
    /// Something anomalous that does not fit the other categories.
    Unknown,
}

/// One suspected alteration, its location and the evidence behind it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedManipulation {
    /// What kind of alteration this appears to be.
    pub manipulation_type: ManipulationType,
    /// Where in the image, clipped to its bounds.
    pub region: SRegion,
    /// Strength of the finding, in `[0, 1]`.
    pub confidence: f64,
    /// `confidence`, banded.
    pub confidence_level: ConfidenceLevel,
    /// One-line human-readable summary.
    pub description: String,
    /// Which individual signals fired. More entries is stronger evidence than a
    /// high score from one.
    pub evidence: Vec<String>,
}

/// Combined output of a [`Detector`].
#[derive(Debug, Clone)]
pub struct DetectionResult {
    /// Every finding, in the order it was added.
    pub manipulations: Vec<DetectedManipulation>,
    /// Mean confidence across the findings. Many weak findings dilute a strong
    /// one, so read `manipulations` rather than this alone.
    pub overall_score: f64,
    /// `overall_score`, banded.
    pub overall_confidence: ConfidenceLevel,
    /// Whether `overall_score` exceeds 0.3. A hint, not a verdict.
    pub is_manipulated: bool,
    /// The original with every finding drawn over it.
    pub visualization: RgbImage,
    /// One-line summary of the findings.
    pub summary: String,
}

impl DetectionResult {
    /// An empty result carrying a copy of `image` as its visualization.
    pub fn new(image: &RgbImage) -> Self {
        Self {
            manipulations: Vec::new(),
            overall_score: 0.0,
            overall_confidence: ConfidenceLevel::None,
            is_manipulated: false,
            visualization: image.clone(),
            summary: String::new(),
        }
    }

    /// Record a finding and recompute the aggregate score.
    pub fn add_manipulation(&mut self, manipulation: DetectedManipulation) {
        self.manipulations.push(manipulation);
        self.recalculate_overall();
    }

    fn recalculate_overall(&mut self) {
        if self.manipulations.is_empty() {
            self.overall_score = 0.0;
            self.overall_confidence = ConfidenceLevel::None;
            self.is_manipulated = false;
            return;
        }

        let total_confidence = self.manipulations.iter().map(|m| m.confidence).sum::<f64>();

        self.overall_score = total_confidence / self.manipulations.len() as f64;
        self.overall_confidence = ConfidenceLevel::from_score(self.overall_score);
        self.is_manipulated = self.overall_score > 0.3;

        self.summary = format!(
            "Detected {} potential manipulation(s) with {:.1}% overall confidence",
            self.manipulations.len(),
            self.overall_score * 100.0
        );
    }
}

/// A detector that combines several analyses into one verdict.
///
/// Implemented by [`SplicingDetector`](splicing::SplicingDetector) and
/// [`TamperingDetector`](tampering::TamperingDetector), so both can be held
/// behind a trait object:
///
/// ```no_run
/// use image_forensics::detection::{
///     Detector, splicing::SplicingDetector, tampering::TamperingDetector,
/// };
///
/// let detectors: Vec<Box<dyn Detector>> = vec![
///     Box::new(SplicingDetector::new()),
///     Box::new(TamperingDetector::new()),
/// ];
/// ```
///
/// The modules in [`analysis`](crate::analysis) deliberately do *not* implement
/// this: each returns module-specific maps and statistics that flattening into
/// [`DetectionResult`] would discard.
pub trait Detector {
    /// Run every enabled analysis and combine the findings.
    fn detect(&self, image: &image::DynamicImage) -> Result<DetectionResult>;

    /// Short human-readable name.
    fn name(&self) -> &str;

    /// What this detector looks for and how.
    fn description(&self) -> &str;
}
