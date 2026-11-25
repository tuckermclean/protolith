//! Paint a test card straight onto the Linux console framebuffer.
//!
//! Run from a text console (not inside X), as a user that can write
//! `/dev/fb0` (usually the `video` group):
//!
//!     cargo run --example fbdev [N]     # /dev/fbN, default 0

use protolith::fbdev::Framebuffer;
use protolith::{Frame, Present};

fn main() -> protolith::Result<()> {
    let n = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(0);
    let mut fb = Framebuffer::open_index(n)?;
    let info = fb.info();
    eprintln!("/dev/fb{n}: {}x{} at {} bpp, stride {}", info.width, info.height, info.bits_per_pixel, info.stride);
    let (w, h) = fb.size()?;

    // Eight vertical color bars over a horizontal gray ramp.
    const BARS: [u32; 8] = [0xffffff, 0xffff00, 0x00ffff, 0x00ff00, 0xff00ff, 0xff0000, 0x0000ff, 0x000000];
    let pixels: Vec<u32> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if y < h * 3 / 4 {
                BARS[(x * 8 / w) as usize]
            } else {
                let g = x * 255 / w.max(1);
                g << 16 | g << 8 | g
            }
        })
        .collect();
    fb.present(&Frame::xrgb(&pixels, w, h)?)
}
