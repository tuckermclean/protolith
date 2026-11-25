//! The framebuffer presenter against a scratch "device": an ordinary file
//! plus a fake sysfs directory. Exercises the real open/`pwrite` path and
//! checks every byte that lands — and every byte that must not.

use std::path::{Path, PathBuf};

use protolith::fbdev::Framebuffer;
use protolith::{Error, Frame, PixelFormat, Present, Rect};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fake fb: `width x height` at `bpp`, rows `stride` bytes apart, the
/// device pre-filled with `0xAA` so untouched bytes are visible.
fn fake_fb(width: u32, height: u32, bpp: u32, stride: u32, modes: Option<&str>) -> (Scratch, PathBuf, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let root = std::env::temp_dir().join(format!("protolith-fb-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let sysfs = root.join("fb0");
    std::fs::create_dir_all(&sysfs).unwrap();
    std::fs::write(sysfs.join("virtual_size"), format!("{width},{height}\n")).unwrap();
    std::fs::write(sysfs.join("bits_per_pixel"), format!("{bpp}\n")).unwrap();
    std::fs::write(sysfs.join("stride"), format!("{stride}\n")).unwrap();
    if let Some(m) = modes {
        std::fs::write(sysfs.join("modes"), m).unwrap();
    }
    let dev = root.join("dev-fb0");
    std::fs::write(&dev, vec![0xAA; (stride * height) as usize]).unwrap();
    (Scratch(root), sysfs, dev)
}

fn pattern(w: u32, h: u32) -> Vec<u32> {
    (0..w * h).map(|i| i.wrapping_mul(0x9e37_79b9) >> 8).collect()
}

fn pixel_at(bytes: &[u8], fmt: PixelFormat, stride: u32, x: u32, y: u32) -> u32 {
    let off = (y * stride) as usize + x as usize * fmt.bytes_per_pixel();
    fmt.unpack(fmt.load(&bytes[off..]).unwrap())
}

fn check(dev: &Path, fmt: PixelFormat, stride: u32, w: u32, h: u32, expect: impl Fn(u32, u32) -> Option<u32>) {
    let bytes = std::fs::read(dev).unwrap();
    assert_eq!(bytes.len(), (stride * h) as usize, "the device never grows");
    for y in 0..h {
        for x in 0..w {
            match expect(x, y) {
                Some(c) => assert_eq!(pixel_at(&bytes, fmt, stride, x, y), fmt.quantize(c), "({x},{y})"),
                None => {
                    let off = (y * stride) as usize + x as usize * fmt.bytes_per_pixel();
                    assert!(bytes[off..off + fmt.bytes_per_pixel()].iter().all(|&b| b == 0xAA), "({x},{y}) must be untouched");
                }
            }
        }
        // Row padding beyond the visible width is never written.
        let pad_start = (y * stride) as usize + w as usize * fmt.bytes_per_pixel();
        assert!(bytes[pad_start..((y + 1) * stride) as usize].iter().all(|&b| b == 0xAA), "row {y} padding");
    }
}

#[test]
fn full_present_at_32_bpp_with_padded_stride() {
    let (_s, sysfs, dev) = fake_fb(37, 21, 32, 37 * 4 + 12, None);
    let mut fb = Framebuffer::open_paths(&sysfs, &dev).unwrap();
    assert_eq!(fb.pixel_format(), PixelFormat::XRGB8888);
    let px = pattern(37, 21);
    fb.present(&Frame::xrgb(&px, 37, 21).unwrap()).unwrap();
    check(&dev, PixelFormat::XRGB8888, 37 * 4 + 12, 37, 21, |x, y| Some(px[(y * 37 + x) as usize]));
}

#[test]
fn full_present_at_16_and_24_bpp() {
    for (bpp, stride) in [(16, 64), (24, 3 * 20 + 4)] {
        let (_s, sysfs, dev) = fake_fb(20, 9, bpp, stride, None);
        let mut fb = Framebuffer::open_paths(&sysfs, &dev).unwrap();
        let fmt = fb.pixel_format();
        assert_eq!(fmt.bytes_per_pixel() as u32 * 8, bpp);
        let px = pattern(20, 9);
        fb.present(&Frame::xrgb(&px, 20, 9).unwrap()).unwrap();
        check(&dev, fmt, stride, 20, 9, |x, y| Some(px[(y * 20 + x) as usize]));
    }
}

#[test]
fn present_rect_writes_only_its_rectangle() {
    let (_s, sysfs, dev) = fake_fb(30, 20, 32, 30 * 4, None);
    let mut fb = Framebuffer::open_paths(&sysfs, &dev).unwrap();
    let px = pattern(30, 20);
    let area = Rect::new(4, 3, 9, 7);
    fb.present_rect(&Frame::xrgb(&px, 30, 20).unwrap(), area).unwrap();
    check(&dev, PixelFormat::XRGB8888, 120, 30, 20, |x, y| area.contains(x as i32, y as i32).then(|| px[(y * 30 + x) as usize]));
}

#[test]
fn a_smaller_frame_leaves_the_rest_untouched_and_a_larger_one_is_clipped() {
    let (_s, sysfs, dev) = fake_fb(16, 16, 32, 64, None);
    let mut fb = Framebuffer::open_paths(&sysfs, &dev).unwrap();
    let small = pattern(5, 4);
    fb.present(&Frame::xrgb(&small, 5, 4).unwrap()).unwrap();
    check(&dev, PixelFormat::XRGB8888, 64, 16, 16, |x, y| (x < 5 && y < 4).then(|| small[(y * 5 + x) as usize]));

    let big = pattern(40, 40);
    fb.present(&Frame::xrgb(&big, 40, 40).unwrap()).unwrap();
    check(&dev, PixelFormat::XRGB8888, 64, 16, 16, |x, y| Some(big[(y * 40 + x) as usize]));
}

#[test]
fn panning_virtual_size_uses_the_visible_mode() {
    // A double-height virtual screen; only the visible top half is ours.
    let (_s, sysfs, dev) = fake_fb(8, 12, 32, 32, Some("U:8x6p-60\n"));
    let mut fb = Framebuffer::open_paths(&sysfs, &dev).unwrap();
    assert_eq!(Present::size(&fb).unwrap(), (8, 6));
    fb.clear(0x123456).unwrap();
    let bytes = std::fs::read(&dev).unwrap();
    assert!(bytes[..6 * 32].chunks(4).all(|p| p == [0x56, 0x34, 0x12, 0]));
    assert!(bytes[6 * 32..].iter().all(|&b| b == 0xAA), "the off-screen half is untouched");
}

#[test]
fn works_through_the_present_trait() {
    fn draw(target: &mut dyn Present) -> protolith::Result<()> {
        let (w, h) = target.size()?;
        let px = vec![0x00ff_0000u32; (w * h) as usize];
        target.present(&Frame::xrgb(&px, w, h)?)
    }
    let (_s, sysfs, dev) = fake_fb(4, 4, 16, 8, None);
    let mut fb = Framebuffer::open_paths(&sysfs, &dev).unwrap();
    draw(&mut fb).unwrap();
    check(&dev, PixelFormat::RGB565, 8, 4, 4, |_, _| Some(0xff0000));
}

#[test]
fn clean_errors() {
    let missing = std::env::temp_dir().join(format!("protolith-definitely-missing-{}", std::process::id()));
    assert!(matches!(Framebuffer::open_paths(&missing, &missing), Err(Error::Io(_))));

    let (_s, sysfs, dev) = fake_fb(4, 4, 8, 4, None);
    assert!(matches!(Framebuffer::open_paths(&sysfs, &dev), Err(Error::Unsupported(_))), "8 bpp palette");

    let (_s2, sysfs, _) = fake_fb(4, 4, 32, 16, None);
    assert!(matches!(Framebuffer::open_paths(&sysfs, &missing), Err(Error::Io(_))), "no device node");

    std::fs::write(sysfs.join("stride"), "8\n").unwrap();
    assert!(matches!(Framebuffer::open_paths(&sysfs, &missing), Err(Error::Protocol(_))), "stride shorter than a row");
}
