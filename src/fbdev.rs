//! The Linux console framebuffer (`/dev/fb0`): for machines with no X at
//! all.
//!
//! Geometry comes from the plain-text sysfs files under
//! `/sys/class/graphics/fbN/` rather than the `FBIOGET_VSCREENINFO` ioctl,
//! and pixels are written with positioned writes (`pwrite`) rather than an
//! `mmap` — both because those need `unsafe`, which this crate forbids.
//! Positioned writes still allow partial updates: [`Framebuffer::present_rect`]
//! touches only the rows and columns of its rectangle.
//!
//! The visible area is the current `mode` within the `virtual_size`,
//! offset by the current `pan` (re-read on every present, since the console
//! scrolls by panning).
//!
//! Pixel layout is inferred from `bits_per_pixel`: 32 → `XRGB8888`, 24 →
//! packed `RGB888`, 16 → `RGB565`, all little-endian (every x86 and ARM
//! console framebuffer in practice). Other depths are refused.
//!
//! The text console keeps drawing on the same framebuffer (its cursor
//! blinks, and kernel messages scroll). For a clean screen, run on a VT
//! nobody is typing on and hide the cursor first (`setterm -cursor off`,
//! or write `0` to `/sys/class/graphics/fbcon/cursor_blink`).

use std::fs::{File, OpenOptions};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::pixel::{ByteOrder, Frame, PixelFormat, Rect};
use crate::Present;

/// Framebuffer geometry, as sysfs reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FbInfo {
    /// Visible width in pixels.
    pub width: u32,
    /// Visible height in pixels.
    pub height: u32,
    /// Bits per pixel.
    pub bits_per_pixel: u32,
    /// Bytes per row (at least `width * bytes_per_pixel`).
    pub stride: u32,
}

impl FbInfo {
    /// The pixel layout for `bits_per_pixel`.
    pub fn pixel_format(&self) -> Result<PixelFormat> {
        match self.bits_per_pixel {
            32 => Ok(PixelFormat::XRGB8888),
            24 => PixelFormat::new(0xff_0000, 0xff00, 0xff, 3, ByteOrder::LsbFirst).map_err(|e| Error::Unsupported(e.to_string())),
            16 => Ok(PixelFormat::RGB565),
            n => Err(Error::Unsupported(format!("framebuffer depth of {n} bits per pixel"))),
        }
    }
}

fn parse_u32(s: &str, what: &str) -> Result<u32> {
    s.trim().parse().map_err(|_| Error::Protocol(format!("{what}: not a number: {:?}", s.trim())))
}

/// Parse a `WIDTH,HEIGHT` pair (`virtual_size`).
fn parse_pair(s: &str) -> Result<(u32, u32)> {
    let (w, h) = s.trim().split_once(',').ok_or_else(|| Error::Protocol(format!("virtual_size: expected WIDTH,HEIGHT: {:?}", s.trim())))?;
    Ok((parse_u32(w, "virtual_size width")?, parse_u32(h, "virtual_size height")?))
}

/// The visible resolution from a sysfs mode line (`U:1024x768p-75`), if
/// it parses.
fn parse_mode(modes: &str) -> Option<(u32, u32)> {
    let line = modes.lines().next()?;
    let spec = line.split_once(':').map_or(line, |(_, s)| s);
    let (w, rest) = spec.split_once('x')?;
    let h: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((w.trim().parse().ok()?, h.parse().ok()?))
}

/// Build [`FbInfo`] from the contents of the sysfs files. `mode` (the
/// current mode, e.g. `U:1024x768p-75`) may be empty, in which case the
/// virtual size is used. Validates that a row fits in the stride.
pub fn parse_fb_info(virtual_size: &str, bits_per_pixel: &str, stride: &str, mode: &str) -> Result<FbInfo> {
    let (vw, vh) = parse_pair(virtual_size)?;
    let (width, height) = parse_mode(mode).filter(|&(w, h)| w <= vw && h <= vh && w > 0 && h > 0).unwrap_or((vw, vh));
    let info = FbInfo { width, height, bits_per_pixel: parse_u32(bits_per_pixel, "bits_per_pixel")?, stride: parse_u32(stride, "stride")? };
    let row = u64::from(info.width) * u64::from(info.bits_per_pixel.div_ceil(8));
    if row > u64::from(info.stride) {
        return Err(Error::Protocol(format!("stride {} is shorter than a {}-pixel row", info.stride, info.width)));
    }
    Ok(info)
}

/// Read and parse the geometry files in `sysfs_dir`.
pub fn read_fb_info(sysfs_dir: &Path) -> Result<FbInfo> {
    let read = |name: &str| {
        std::fs::read_to_string(sysfs_dir.join(name)).map_err(|e| Error::Io(std::io::Error::new(e.kind(), format!("{}: {e}", sysfs_dir.join(name).display()))))
    };
    // `mode` is the current mode; `modes` lists the driver's modes with the
    // boot/preferred one first, which is only a fallback.
    let mode = std::fs::read_to_string(sysfs_dir.join("mode"))
        .ok()
        .filter(|m| parse_mode(m).is_some())
        .or_else(|| std::fs::read_to_string(sysfs_dir.join("modes")).ok())
        .unwrap_or_default();
    parse_fb_info(&read("virtual_size")?, &read("bits_per_pixel")?, &read("stride")?, &mode)
}

/// An open framebuffer device.
#[derive(Debug)]
pub struct Framebuffer {
    device: File,
    path: PathBuf,
    info: FbInfo,
    format: PixelFormat,
    scratch: Vec<u8>,
    /// The sysfs `pan` file (`xoffset,yoffset` of the visible area within
    /// the virtual screen), when the driver has one.
    pan: Option<PathBuf>,
}

impl Framebuffer {
    /// Open `/dev/fb0`.
    pub fn open() -> Result<Framebuffer> {
        Framebuffer::open_index(0)
    }

    /// Open `/dev/fb<n>`.
    pub fn open_index(n: u32) -> Result<Framebuffer> {
        Framebuffer::open_paths(Path::new(&format!("/sys/class/graphics/fb{n}")), Path::new(&format!("/dev/fb{n}")))
    }

    /// Open a device with geometry from an explicit sysfs directory (for
    /// unusual setups, and for testing against ordinary files).
    pub fn open_paths(sysfs_dir: &Path, device: &Path) -> Result<Framebuffer> {
        let info = read_fb_info(sysfs_dir)?;
        let format = info.pixel_format()?;
        let file = OpenOptions::new()
            .write(true)
            .read(true)
            .open(device)
            .map_err(|e| Error::Io(std::io::Error::new(e.kind(), format!("{}: {e}", device.display()))))?;
        let pan = Some(sysfs_dir.join("pan")).filter(|p| p.exists());
        Ok(Framebuffer { device: file, path: device.to_path_buf(), info, format, scratch: Vec::new(), pan })
    }

    /// Geometry.
    pub fn info(&self) -> FbInfo {
        self.info
    }

    /// The device's pixel layout.
    pub fn pixel_format(&self) -> PixelFormat {
        self.format
    }

    /// Show `frame` at the top-left of the screen.
    pub fn present(&mut self, frame: &Frame<'_>) -> Result<()> {
        self.present_rect(frame, frame.bounds())
    }

    /// Show only `area` of `frame` (frame and screen coordinates coincide).
    pub fn present_rect(&mut self, frame: &Frame<'_>, area: Rect) -> Result<()> {
        let screen = Rect::from_size(self.info.width, self.info.height);
        let area = area.intersect(&screen);
        let bpp = self.format.bytes_per_pixel();
        let stride = self.info.stride as usize;
        // Rows that span the whole stride (no padding) are contiguous on
        // the device and go out in one write. Padding is never written: on
        // a panning framebuffer it is live, off-screen virtual memory.
        let full_rows = area.x == 0 && area.width as usize * bpp == stride;
        let dst_stride = area.width as usize * bpp;
        let done = self.format.convert(frame, area, dst_stride, &mut self.scratch);
        if done.is_empty() {
            return Ok(());
        }
        // The console pans to scroll (vesafb ypan, fbcon): the visible area
        // starts at the current pan offset, which can change at any time.
        let (px, py) = self.pan.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| parse_pair(&t).ok()).unwrap_or((0, 0));
        let offset = |y: i32, x: i32| (u64::from(py) + y as u64) * stride as u64 + (u64::from(px) + x as u64) * bpp as u64;
        let r = if full_rows && done.width == area.width {
            self.device.write_all_at(&self.scratch, offset(done.y, 0))
        } else {
            let row = done.width as usize * bpp;
            (0..done.height as usize)
                .try_for_each(|i| self.device.write_all_at(&self.scratch[i * dst_stride..i * dst_stride + row], offset(done.y + i as i32, done.x)))
        };
        r.map_err(|e| Error::Io(std::io::Error::new(e.kind(), format!("{}: {e}", self.path.display()))))
    }

    /// Fill the whole screen with one `0x00RRGGBB` color.
    pub fn clear(&mut self, xrgb: u32) -> Result<()> {
        let full = vec![xrgb; self.info.width as usize * self.info.height as usize];
        self.present(&Frame::xrgb(&full, self.info.width, self.info.height)?)
    }
}

impl Present for Framebuffer {
    fn size(&self) -> Result<(u32, u32)> {
        Ok((self.info.width, self.info.height))
    }

    fn present(&mut self, frame: &Frame<'_>) -> Result<()> {
        Framebuffer::present(self, frame)
    }

    fn present_rect(&mut self, frame: &Frame<'_>, area: Rect) -> Result<()> {
        Framebuffer::present_rect(self, frame, area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typical_geometry() {
        let info = parse_fb_info("1024,768\n", "32\n", "4096\n", "U:1024x768p-0\n").unwrap();
        assert_eq!(info, FbInfo { width: 1024, height: 768, bits_per_pixel: 32, stride: 4096 });
        assert_eq!(info.pixel_format().unwrap(), PixelFormat::XRGB8888);
    }

    #[test]
    fn visible_mode_wins_over_a_panning_virtual_size() {
        let info = parse_fb_info("1024,1536", "16", "2048", "U:1024x768p-75\nV:800x600p-60\n").unwrap();
        assert_eq!((info.width, info.height), (1024, 768));
        assert_eq!(info.pixel_format().unwrap(), PixelFormat::RGB565);
    }

    #[test]
    fn missing_or_bogus_modes_fall_back_to_virtual_size() {
        assert_eq!(parse_fb_info("640,480", "32", "2560", "").unwrap().height, 480);
        assert_eq!(parse_fb_info("640,480", "32", "2560", "garbage").unwrap().height, 480);
        assert_eq!(parse_fb_info("640,480", "32", "2560", "U:4096x4096p-0").unwrap().width, 640, "mode larger than virtual is ignored");
    }

    #[test]
    fn mode_parser_variants() {
        assert_eq!(parse_mode("U:1024x768p-75"), Some((1024, 768)));
        assert_eq!(parse_mode("S:640x480i-60"), Some((640, 480)));
        assert_eq!(parse_mode("800x600"), Some((800, 600)));
        assert_eq!(parse_mode(""), None);
        assert_eq!(parse_mode("U:x"), None);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_fb_info("", "32", "4096", "").is_err());
        assert!(parse_fb_info("1024", "32", "4096", "").is_err());
        assert!(parse_fb_info("1024,768", "lots", "4096", "").is_err());
        assert!(parse_fb_info("1024,768", "32", "-1", "").is_err());
        assert!(parse_fb_info("1024,768", "32", "1000", "").is_err(), "stride shorter than a row");
    }

    #[test]
    fn depths() {
        let at = |bpp| FbInfo { width: 1, height: 1, bits_per_pixel: bpp, stride: 4 }.pixel_format();
        assert_eq!(at(24).unwrap().bytes_per_pixel(), 3);
        assert!(matches!(at(8), Err(Error::Unsupported(_))));
        assert!(matches!(at(1), Err(Error::Unsupported(_))));
    }
}
