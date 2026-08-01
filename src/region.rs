use serde::{Deserialize, Serialize};

/// An axis-aligned rectangle in image pixel coordinates.
///
/// `x`/`y` are the top-left corner; the region covers `x..x + width` and
/// `y..y + height`. All accessors saturate rather than wrap, so a region that
/// was built from a clamped block never overflows when its edges are queried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl SRegion {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// A `size` x `size` block at (`x`, `y`), clipped to the image bounds.
    ///
    /// Returns an empty region when the origin lies outside the image, which
    /// keeps callers free of the `size.min(width - x)` underflow that this
    /// replaces.
    pub fn clipped(x: u32, y: u32, size: u32, image_width: u32, image_height: u32) -> Self {
        Self {
            x,
            y,
            width: size.min(image_width.saturating_sub(x)),
            height: size.min(image_height.saturating_sub(y)),
        }
    }

    /// One past the rightmost column covered by the region.
    pub fn right(&self) -> u32 {
        self.x.saturating_add(self.width)
    }

    /// One past the bottom row covered by the region.
    pub fn bottom(&self) -> u32 {
        self.y.saturating_add(self.height)
    }

    /// Area in pixels. Widened to `u64` so large regions cannot overflow.
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn center(&self) -> (u32, u32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }

    /// True when the two regions share at least one pixel.
    pub fn overlaps(&self, other: &SRegion) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// True when the regions overlap or are separated by at most `gap` pixels.
    pub fn is_adjacent_within(&self, other: &SRegion, gap: u32) -> bool {
        !(self.right().saturating_add(gap) < other.x
            || other.right().saturating_add(gap) < self.x
            || self.bottom().saturating_add(gap) < other.y
            || other.bottom().saturating_add(gap) < self.y)
    }

    /// Smallest region containing both inputs.
    pub fn union(&self, other: &SRegion) -> SRegion {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());

        SRegion {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }

    /// Clip the region to an image of the given size.
    pub fn clamp_to(&self, image_width: u32, image_height: u32) -> SRegion {
        let x = self.x.min(image_width);
        let y = self.y.min(image_height);

        SRegion {
            x,
            y,
            width: self.right().min(image_width) - x,
            height: self.bottom().min(image_height) - y,
        }
    }

    /// Iterate the pixel coordinates covered by the region, row-major.
    pub fn pixels(self) -> impl Iterator<Item = (u32, u32)> {
        let (x, y, w, h) = (self.x, self.y, self.width, self.height);
        (y..y.saturating_add(h)).flat_map(move |py| (x..x.saturating_add(w)).map(move |px| (px, py)))
    }
}

/// Collapse a set of regions into connected clusters.
///
/// Two regions join when they overlap or sit within `gap` pixels of one
/// another; each cluster is replaced by its bounding box. Every analyzer that
/// reports `Vec<SRegion>` funnels through this so the merge semantics stay
/// identical across modules.
pub fn merge_regions(regions: Vec<SRegion>, gap: u32) -> Vec<SRegion> {
    if regions.is_empty() {
        return regions;
    }

    let mut merged = Vec::new();
    let mut used = vec![false; regions.len()];

    for i in 0..regions.len() {
        if used[i] {
            continue;
        }

        let mut current = regions[i];
        used[i] = true;

        // Absorbing a neighbour grows the bounding box, which can bring further
        // regions into range, so keep sweeping until nothing else joins.
        loop {
            let mut grew = false;

            for j in 0..regions.len() {
                if used[j] {
                    continue;
                }

                if current.is_adjacent_within(&regions[j], gap) {
                    current = current.union(&regions[j]);
                    used[j] = true;
                    grew = true;
                }
            }

            if !grew {
                break;
            }
        }

        merged.push(current);
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipped_never_underflows_outside_the_image() {
        let region = SRegion::clipped(120, 90, 64, 100, 100);
        assert_eq!(region.width, 0);
        assert_eq!(region.height, 10);
        assert!(region.is_empty());
    }

    #[test]
    fn clipped_truncates_at_the_edge() {
        let region = SRegion::clipped(80, 80, 64, 100, 100);
        assert_eq!((region.width, region.height), (20, 20));
    }

    #[test]
    fn union_grows_in_both_axes() {
        let a = SRegion::new(0, 0, 10, 4);
        let b = SRegion::new(20, 30, 5, 5);
        let u = a.union(&b);

        assert_eq!(u, SRegion::new(0, 0, 25, 35));
    }

    #[test]
    fn overlaps_is_exclusive_at_the_shared_edge() {
        let a = SRegion::new(0, 0, 10, 10);
        let touching = SRegion::new(10, 0, 10, 10);
        let intersecting = SRegion::new(9, 0, 10, 10);

        assert!(!a.overlaps(&touching));
        assert!(a.overlaps(&intersecting));
        assert!(a.is_adjacent_within(&touching, 0));
    }

    #[test]
    fn merge_joins_transitively() {
        let regions = vec![
            SRegion::new(0, 0, 10, 10),
            SRegion::new(12, 0, 10, 10),
            SRegion::new(24, 0, 10, 10),
            SRegion::new(200, 200, 10, 10),
        ];

        let merged = merge_regions(regions, 4);

        assert_eq!(merged.len(), 2);
        assert!(merged.contains(&SRegion::new(0, 0, 34, 10)));
        assert!(merged.contains(&SRegion::new(200, 200, 10, 10)));
    }

    #[test]
    fn area_does_not_overflow_u32() {
        let region = SRegion::new(0, 0, 100_000, 100_000);
        assert_eq!(region.area(), 10_000_000_000);
    }
}
