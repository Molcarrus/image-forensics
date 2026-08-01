use std::collections::HashMap;

use image::{DynamicImage, GrayImage, Rgb, RgbImage};
use rayon::prelude::*;

use crate::{
    CopyMoveResult, MatchPair, SRegion, draw,
    error::{ForensicsError, Result},
    image_utils::{block_variance, extract_block, rgb_to_gray},
};

/// Number of low-frequency DCT coefficients kept per block.
const FEATURE_LEN: usize = 16;

pub struct CopyMoveDetector {
    block_size: u32,
    similarity_threshold: f64,
    min_distance: u32,
    variance_threshold: f64,
    /// Separable DCT-II basis, `block_size` x `block_size`, row-major.
    dct_basis: Vec<f64>,
    /// Zig-zag scan order over the block, truncated to [`FEATURE_LEN`].
    zigzag: Vec<(usize, usize)>,
}

#[derive(Clone)]
struct BlockFeature {
    x: u32,
    y: u32,
    dct_coeffs: Vec<f64>,
    hash: u64,
}

impl CopyMoveDetector {
    pub fn new(block_size: u32, similarity_threshold: f64, min_distance: u32) -> Result<Self> {
        if !(4..=64).contains(&block_size) {
            return Err(ForensicsError::InvalidParameter(
                "block size must be between 4 and 64".into(),
            ));
        }

        Ok(Self {
            block_size,
            similarity_threshold,
            min_distance,
            variance_threshold: 100.0,
            dct_basis: dct_basis(block_size as usize),
            zigzag: zigzag_order(block_size as usize, FEATURE_LEN),
        })
    }

    pub fn with_variance_threshold(mut self, threshold: f64) -> Self {
        self.variance_threshold = threshold;
        self
    }

    pub fn detect(&self, image: &DynamicImage) -> Result<CopyMoveResult> {
        let rgb = image.to_rgb8();
        let gray = rgb_to_gray(&rgb);
        let (width, height) = gray.dimensions();

        if width < self.block_size * 2 || height < self.block_size * 2 {
            return Err(ForensicsError::ImageTooSmall(self.block_size * 2));
        }

        let features = self.extract_features(&gray);
        let matches = self.find_matches(&features);
        let visualization = self.create_visualization(&rgb, &matches);

        let confidence = if matches.is_empty() {
            0.0
        } else {
            matches.iter().map(|m| m.similarity).sum::<f64>() / matches.len() as f64
        };

        Ok(CopyMoveResult {
            matches,
            visualization,
            confidence,
        })
    }

    fn extract_features(&self, gray: &GrayImage) -> Vec<BlockFeature> {
        let (width, height) = gray.dimensions();
        let step = (self.block_size / 2).max(1);

        let positions: Vec<(u32, u32)> = crate::image_utils::full_blocks(
            width,
            height,
            self.block_size,
            step,
        )
        .map(|region| (region.x, region.y))
        .collect();

        positions
            .par_iter()
            .filter_map(|&(x, y)| self.extract_block_feature(gray, x, y))
            .collect()
    }

    fn extract_block_feature(&self, gray: &GrayImage, x: u32, y: u32) -> Option<BlockFeature> {
        let block = extract_block(gray, x, y, self.block_size);

        // Flat blocks match everything; skipping them is what keeps the
        // pairwise stage tractable.
        if block_variance(&block) < self.variance_threshold {
            return None;
        }

        let dct_coeffs = self.compute_dct(&block);
        let hash = Self::compute_hash(&dct_coeffs);

        Some(BlockFeature {
            x,
            y,
            dct_coeffs,
            hash,
        })
    }

    /// Low-frequency coefficients of the 2-D DCT-II of a block.
    ///
    /// The previous implementation ran a 1-D FFT over the flattened block and
    /// took magnitudes: neither a DCT nor two-dimensional, so features from
    /// visually distinct blocks collided readily. This is the standard
    /// separable transform, evaluated as `B * X * B^T`.
    fn compute_dct(&self, block: &[u8]) -> Vec<f64> {
        let n = self.block_size as usize;

        // Row transform: temp = B * X
        let mut temp = vec![0.0f64; n * n];
        for u in 0..n {
            for j in 0..n {
                let mut sum = 0.0;
                for i in 0..n {
                    sum += self.dct_basis[u * n + i] * (block[i * n + j] as f64 - 128.0);
                }
                temp[u * n + j] = sum;
            }
        }

        // Column transform: coeffs = temp * B^T
        let mut coeffs = vec![0.0f64; n * n];
        for u in 0..n {
            for v in 0..n {
                let mut sum = 0.0;
                for j in 0..n {
                    sum += temp[u * n + j] * self.dct_basis[v * n + j];
                }
                coeffs[u * n + v] = sum;
            }
        }

        self.zigzag
            .iter()
            .map(|&(u, v)| coeffs[u * n + v])
            .collect()
    }

    /// Sign-of-deviation hash over the feature vector, used for bucketing.
    fn compute_hash(coeffs: &[f64]) -> u64 {
        let mean = coeffs.iter().sum::<f64>() / coeffs.len() as f64;
        let mut hash = 0u64;

        for (i, &c) in coeffs.iter().enumerate().take(64) {
            if c > mean {
                hash |= 1 << i;
            }
        }

        hash
    }

    fn find_matches(&self, features: &[BlockFeature]) -> Vec<MatchPair> {
        // Bucket by exact hash and by each single-bit neighbour, so blocks
        // straddling a threshold still meet. The old `hash ^ offset` for
        // `offset in 0..4` only ever perturbed the low two bits while inserting
        // every feature into four buckets, so most pairs were compared
        // redundantly and genuine near-matches were missed.
        let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();

        for (i, feature) in features.iter().enumerate() {
            buckets.entry(feature.hash).or_default().push(i);

            for bit in 0..FEATURE_LEN.min(64) {
                buckets
                    .entry(feature.hash ^ (1u64 << bit))
                    .or_default()
                    .push(i);
            }
        }

        let mut matches = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for indices in buckets.values() {
            if indices.len() < 2 {
                continue;
            }

            for i in 0..indices.len() {
                for j in (i + 1)..indices.len() {
                    let (a, b) = (indices[i].min(indices[j]), indices[i].max(indices[j]));
                    if !seen.insert((a, b)) {
                        continue;
                    }

                    let f1 = &features[a];
                    let f2 = &features[b];

                    let dx = (f1.x as i64 - f2.x as i64) as f64;
                    let dy = (f1.y as i64 - f2.y as i64) as f64;

                    if (dx * dx + dy * dy).sqrt() < self.min_distance as f64 {
                        continue;
                    }

                    let similarity = Self::calculate_similarity(&f1.dct_coeffs, &f2.dct_coeffs);

                    if similarity >= self.similarity_threshold {
                        matches.push(MatchPair {
                            source: SRegion::new(f1.x, f1.y, self.block_size, self.block_size),
                            target: SRegion::new(f2.x, f2.y, self.block_size, self.block_size),
                            similarity,
                        });
                    }
                }
            }
        }

        self.filter_matches(matches)
    }

    /// Pearson correlation between two feature vectors, floored at zero.
    fn calculate_similarity(coeffs1: &[f64], coeffs2: &[f64]) -> f64 {
        if coeffs1.len() != coeffs2.len() || coeffs1.is_empty() {
            return 0.0;
        }

        let mean1 = coeffs1.iter().sum::<f64>() / coeffs1.len() as f64;
        let mean2 = coeffs2.iter().sum::<f64>() / coeffs2.len() as f64;

        let mut numerator = 0.0;
        let mut denom1 = 0.0;
        let mut denom2 = 0.0;

        for (&c1, &c2) in coeffs1.iter().zip(coeffs2.iter()) {
            let d1 = c1 - mean1;
            let d2 = c2 - mean2;
            numerator += d1 * d2;
            denom1 += d1 * d1;
            denom2 += d2 * d2;
        }

        let denom = (denom1 * denom2).sqrt();
        if denom < 1e-10 {
            0.0
        } else {
            (numerator / denom).max(0.0)
        }
    }

    /// Keep the strongest non-overlapping matches, best first.
    fn filter_matches(&self, mut matches: Vec<MatchPair>) -> Vec<MatchPair> {
        matches.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut filtered: Vec<MatchPair> = Vec::new();

        for candidate in matches {
            let overlaps = filtered.iter().any(|existing| {
                candidate.source.overlaps(&existing.source)
                    || candidate.target.overlaps(&existing.source)
                    || candidate.source.overlaps(&existing.target)
                    || candidate.target.overlaps(&existing.target)
            });

            if !overlaps {
                filtered.push(candidate);
            }
        }

        filtered
    }

    fn create_visualization(&self, original: &RgbImage, matches: &[MatchPair]) -> RgbImage {
        let mut vis = original.clone();

        for (i, match_pair) in matches.iter().enumerate() {
            // Golden-angle hue stepping keeps adjacent matches distinguishable.
            let color = hue_to_rgb((i as f64 * 137.5) % 360.0);

            draw::rect(&mut vis, &match_pair.source, color, 1);
            draw::rect(&mut vis, &match_pair.target, color, 1);

            let (sx, sy) = match_pair.source.center();
            let (tx, ty) = match_pair.target.center();
            draw::line(&mut vis, sx as i32, sy as i32, tx as i32, ty as i32, color);
        }

        vis
    }
}

/// Orthonormal DCT-II basis of size `n`, row-major.
fn dct_basis(n: usize) -> Vec<f64> {
    let mut basis = vec![0.0f64; n * n];

    for u in 0..n {
        let scale = if u == 0 {
            (1.0 / n as f64).sqrt()
        } else {
            (2.0 / n as f64).sqrt()
        };

        for i in 0..n {
            basis[u * n + i] = scale
                * (std::f64::consts::PI * (2.0 * i as f64 + 1.0) * u as f64 / (2.0 * n as f64))
                    .cos();
        }
    }

    basis
}

/// The first `count` positions of an `n` x `n` zig-zag scan.
fn zigzag_order(n: usize, count: usize) -> Vec<(usize, usize)> {
    let mut order = Vec::with_capacity(n * n);

    for diagonal in 0..(2 * n - 1) {
        let cells: Vec<(usize, usize)> = (0..=diagonal)
            .filter_map(|u| {
                let v = diagonal.checked_sub(u)?;
                (u < n && v < n).then_some((u, v))
            })
            .collect();

        // Alternate direction along each diagonal, as in the JPEG scan.
        if diagonal % 2 == 0 {
            order.extend(cells.into_iter().rev());
        } else {
            order.extend(cells);
        }
    }

    order.truncate(count.min(n * n));
    order
}

fn hue_to_rgb(hue: f64) -> Rgb<u8> {
    let sector = hue / 60.0;
    let x = 1.0 - (sector % 2.0 - 1.0).abs();

    let (r, g, b) = match sector as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };

    Rgb([
        (r * 255.0) as u8,
        (g * 255.0) as u8,
        (b * 255.0) as u8,
    ])
}

#[cfg(test)]
mod tests {
    use image::RgbImage;

    use super::*;

    /// Deterministic xorshift, so the fixture needs no `rand` dependency.
    fn noise_byte(state: &mut u64) -> u8 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        (*state >> 24) as u8
    }

    /// Textured noise, then a patch copied to a distant location.
    fn image_with_duplicate_patch() -> DynamicImage {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut image = RgbImage::new(256, 256);

        for pixel in image.pixels_mut() {
            let v = noise_byte(&mut state);
            *pixel = Rgb([v, v, v]);
        }

        // Both corners sit on the 8px feature stride, so a source block and its
        // copy land on the same sub-patch offset and can actually be paired.
        let patch: Vec<Rgb<u8>> = (0..48 * 48)
            .map(|i| *image.get_pixel(16 + (i % 48), 16 + (i / 48)))
            .collect();

        for i in 0..48u32 * 48 {
            image.put_pixel(184 + (i % 48), 184 + (i / 48), patch[i as usize]);
        }

        DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn rejects_out_of_range_block_sizes() {
        assert!(CopyMoveDetector::new(2, 0.9, 10).is_err());
        assert!(CopyMoveDetector::new(128, 0.9, 10).is_err());
        assert!(CopyMoveDetector::new(16, 0.9, 10).is_ok());
    }

    #[test]
    fn finds_a_copied_patch() {
        let detector = CopyMoveDetector::new(16, 0.95, 50).unwrap();
        let result = detector.detect(&image_with_duplicate_patch()).unwrap();

        assert!(
            !result.matches.is_empty(),
            "no duplicate region found in a synthetic copy-move image"
        );
    }

    #[test]
    fn reports_no_matches_on_a_flat_image() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(128, 128, Rgb([90, 90, 90])));
        let result = CopyMoveDetector::new(16, 0.95, 50)
            .unwrap()
            .detect(&image)
            .unwrap();

        assert!(result.matches.is_empty());
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn dct_basis_is_orthonormal() {
        let n = 8;
        let basis = dct_basis(n);

        for u in 0..n {
            for v in 0..n {
                let dot: f64 = (0..n).map(|i| basis[u * n + i] * basis[v * n + i]).sum();
                let expected = if u == v { 1.0 } else { 0.0 };
                assert!((dot - expected).abs() < 1e-9, "row {u} . row {v} = {dot}");
            }
        }
    }

    #[test]
    fn zigzag_starts_at_dc_and_has_no_duplicates() {
        let order = zigzag_order(8, 16);
        assert_eq!(order[0], (0, 0));
        assert_eq!(order.len(), 16);

        let unique: std::collections::HashSet<_> = order.iter().collect();
        assert_eq!(unique.len(), 16);
    }

    #[test]
    fn image_too_small_is_reported() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(20, 20));
        let result = CopyMoveDetector::new(16, 0.9, 10).unwrap().detect(&image);

        assert!(matches!(
            result,
            Err(ForensicsError::ImageTooSmall(32))
        ));
    }
}
