//! Overlay primitives shared by every visualization path.
//!
//! All entry points take signed coordinates and clip internally. This is
//! deliberate: the previous per-module copies took `u32`, so a vector pointing
//! past the left or top edge wrapped to ~4e9 and left the Bresenham loop
//! chasing a target it could never reach.

use image::{Rgb, RgbImage};

use crate::region::SRegion;

/// Draw a clipped line between two points.
///
/// Endpoints may lie outside the image; pixels off-canvas are skipped rather
/// than wrapped. The iteration count is bounded by the Chebyshev distance, so
/// the loop always terminates.
pub fn line(image: &mut RgbImage, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb<u8>) {
    let (width, height) = image.dimensions();
    let (width, height) = (width as i32, height as i32);

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };

    let mut err = dx + dy;
    let mut x = x0;
    let mut y = y0;

    // Bresenham visits at most max(|dx|, |dy|) + 1 points; the counter is a
    // belt-and-braces guard so a malformed endpoint can never hang a caller.
    let max_steps = dx.max(-dy) + 1;

    for _ in 0..=max_steps {
        if x >= 0 && x < width && y >= 0 && y < height {
            image.put_pixel(x as u32, y as u32, color);
        }

        if x == x1 && y == y1 {
            break;
        }

        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Draw a hollow rectangle around `region`, growing outwards by `thickness`.
pub fn rect(image: &mut RgbImage, region: &SRegion, color: Rgb<u8>, thickness: u32) {
    if region.is_empty() {
        return;
    }

    let left = region.x as i32;
    let top = region.y as i32;
    let right = region.right() as i32 - 1;
    let bottom = region.bottom() as i32 - 1;

    for t in 0..thickness.max(1) as i32 {
        line(image, left - t, top - t, right + t, top - t, color);
        line(image, left - t, bottom + t, right + t, bottom + t, color);
        line(image, left - t, top - t, left - t, bottom + t, color);
        line(image, right + t, top - t, right + t, bottom + t, color);
    }
}

/// Alpha-blend `color` over the pixels inside `region`.
pub fn fill(image: &mut RgbImage, region: &SRegion, color: Rgb<u8>, alpha: f32) {
    let (width, height) = image.dimensions();
    let alpha = alpha.clamp(0.0, 1.0);

    if alpha <= 0.0 {
        return;
    }

    for (x, y) in region.clamp_to(width, height).pixels() {
        let existing = *image.get_pixel(x, y);
        let blended = Rgb([
            blend(existing[0], color[0], alpha),
            blend(existing[1], color[1], alpha),
            blend(existing[2], color[2], alpha),
        ]);
        image.put_pixel(x, y, blended);
    }
}

fn blend(base: u8, overlay: u8, alpha: f32) -> u8 {
    ((1.0 - alpha) * base as f32 + alpha * overlay as f32).round() as u8
}

/// Draw a line from (`x0`, `y0`) to (`x1`, `y1`) with an arrowhead at the tip.
pub fn arrow(image: &mut RgbImage, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb<u8>) {
    line(image, x0, y0, x1, y1, color);

    let angle = (y1 as f64 - y0 as f64).atan2(x1 as f64 - x0 as f64);
    let head_size = 8.0;
    let spread = 0.5;

    for offset in [-spread, spread] {
        let head_angle = angle + std::f64::consts::PI + offset;
        let hx = x1 as f64 + head_size * head_angle.cos();
        let hy = y1 as f64 + head_size * head_angle.sin();

        line(image, x1, y1, hx.round() as i32, hy.round() as i32, color);
    }
}

/// Draw a small crosshair centred on (`cx`, `cy`).
pub fn crosshair(image: &mut RgbImage, cx: i32, cy: i32, radius: i32, color: Rgb<u8>) {
    line(image, cx - radius, cy, cx + radius, cy, color);
    line(image, cx, cy - radius, cx, cy + radius, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> RgbImage {
        RgbImage::new(32, 32)
    }

    #[test]
    fn line_with_negative_endpoints_terminates_and_clips() {
        let mut image = canvas();
        // Before the i32 rewrite this wrapped to ~4e9 and looped forever.
        line(&mut image, 5, 5, -400, -400, Rgb([255, 0, 0]));

        assert_eq!(*image.get_pixel(5, 5), Rgb([255, 0, 0]));
        assert_eq!(*image.get_pixel(31, 31), Rgb([0, 0, 0]));
    }

    #[test]
    fn line_past_the_far_edge_terminates() {
        let mut image = canvas();
        line(&mut image, 0, 0, 10_000, 10_000, Rgb([0, 255, 0]));

        assert_eq!(*image.get_pixel(0, 0), Rgb([0, 255, 0]));
        assert_eq!(*image.get_pixel(31, 31), Rgb([0, 255, 0]));
    }

    #[test]
    fn rect_touches_all_four_corners() {
        let mut image = canvas();
        let region = SRegion::new(4, 4, 8, 8);
        rect(&mut image, &region, Rgb([0, 0, 255]), 1);

        for (x, y) in [(4, 4), (11, 4), (4, 11), (11, 11)] {
            assert_eq!(*image.get_pixel(x, y), Rgb([0, 0, 255]), "corner {x},{y}");
        }
        assert_eq!(*image.get_pixel(7, 7), Rgb([0, 0, 0]), "interior stays clear");
    }

    #[test]
    fn fill_clips_to_the_canvas() {
        let mut image = canvas();
        fill(&mut image, &SRegion::new(28, 28, 64, 64), Rgb([255, 255, 255]), 1.0);

        assert_eq!(*image.get_pixel(31, 31), Rgb([255, 255, 255]));
    }
}
