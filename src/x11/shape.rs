//! Turning a bitmap mask into the rectangle list the SHAPE extension wants.
//!
//! Classic skinned players describe their window outline as a mask (or a
//! polygon rasterized into one); [`rects_from_mask`] converts any such mask
//! into a compact, exact rectangle cover for
//! [`Connection::set_shape`](super::Connection::set_shape).

use crate::pixel::Rect;

/// Cover the `width × height` mask's set pixels with rectangles.
///
/// Each row is split into runs of set pixels; consecutive rows with
/// identical runs merge into taller rectangles. The result covers exactly
/// the set pixels, never overlaps, and is in Y-then-X order.
///
/// ```
/// use protolith::x11::shape::rects_from_mask;
/// use protolith::Rect;
/// // A 4x3 "plus" sign.
/// let plus = [
///     ".##.",
///     "####",
///     ".##.",
/// ];
/// let rects = rects_from_mask(4, 3, |x, y| plus[y as usize].as_bytes()[x as usize] == b'#');
/// assert_eq!(rects, [Rect::new(1, 0, 2, 1), Rect::new(0, 1, 4, 1), Rect::new(1, 2, 2, 1)]);
/// ```
pub fn rects_from_mask(width: u32, height: u32, mut is_set: impl FnMut(u32, u32) -> bool) -> Vec<Rect> {
    let mut done: Vec<Rect> = Vec::new();
    // Rectangles still growing downward: one per run of the previous row.
    let mut open: Vec<Rect> = Vec::new();
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for y in 0..height {
        runs.clear();
        let mut x = 0;
        while x < width {
            if is_set(x, y) {
                let start = x;
                while x < width && is_set(x, y) {
                    x += 1;
                }
                runs.push((start, x - start));
            } else {
                x += 1;
            }
        }
        let same = runs.len() == open.len() && runs.iter().zip(&open).all(|(&(x0, w), r)| r.x == x0 as i32 && r.width == w);
        if same {
            for r in &mut open {
                r.height += 1;
            }
        } else {
            done.append(&mut open);
            open.extend(runs.iter().map(|&(x0, w)| Rect::new(x0 as i32, y as i32, w, 1)));
        }
    }
    done.append(&mut open);
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cover(rects: &[Rect], w: u32, h: u32) -> Vec<Vec<u8>> {
        let mut grid = vec![vec![0u8; w as usize]; h as usize];
        for r in rects {
            for y in r.y..r.y + r.height as i32 {
                for x in r.x..r.x + r.width as i32 {
                    grid[y as usize][x as usize] += 1;
                }
            }
        }
        grid
    }

    #[test]
    fn full_mask_is_one_rect() {
        assert_eq!(rects_from_mask(275, 116, |_, _| true), [Rect::new(0, 0, 275, 116)]);
    }

    #[test]
    fn empty_mask_is_no_rects() {
        assert!(rects_from_mask(10, 10, |_, _| false).is_empty());
        assert!(rects_from_mask(0, 0, |_, _| true).is_empty());
    }

    #[test]
    fn rounded_corners_merge_the_middle() {
        // A 6x4 rectangle with the four corner pixels cut.
        let set = |x: u32, y: u32| !((x == 0 || x == 5) && (y == 0 || y == 3));
        assert_eq!(rects_from_mask(6, 4, set), [Rect::new(1, 0, 4, 1), Rect::new(0, 1, 6, 2), Rect::new(1, 3, 4, 1)]);
    }

    #[test]
    fn cover_is_exact_and_disjoint_for_a_circle() {
        let (w, h) = (37u32, 29u32);
        let set = |x: u32, y: u32| {
            let dx = x as f64 - 18.0;
            let dy = (y as f64 - 14.0) * 1.3;
            dx * dx + dy * dy <= 16.0 * 16.0
        };
        let rects = rects_from_mask(w, h, set);
        let grid = cover(&rects, w, h);
        for y in 0..h {
            for x in 0..w {
                assert_eq!(grid[y as usize][x as usize], u8::from(set(x, y)), "({x},{y})");
            }
        }
        assert!(rects.len() < (h * 2) as usize, "rows merge: {} rects", rects.len());
    }

    #[test]
    fn ragged_noise_is_covered_exactly() {
        let mut s = 0x1234_5678u32;
        let mut bits = vec![false; 50 * 40];
        for b in bits.iter_mut() {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            *b = s % 3 != 0;
        }
        let set = |x: u32, y: u32| bits[(y * 50 + x) as usize];
        let grid = cover(&rects_from_mask(50, 40, set), 50, 40);
        for y in 0..40 {
            for x in 0..50 {
                assert_eq!(grid[y as usize][x as usize], u8::from(set(x, y)));
            }
        }
    }
}
