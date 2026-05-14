//! Generate small colored-circle PNGs:
//! - `for_color` (default 22×22 filled): used as the menubar tray icon
//! - `for_color_filled(hex, size)`: filled circle, used as the active-profile
//!   menu-item icon
//! - `for_color_ring(hex, size)`: hollow ring (transparent center), used as
//!   the inactive-profile menu-item icon

use anyhow::Result;
use image::{ImageBuffer, Rgba, RgbaImage};

/// Default size for menu-item icons. macOS menu items render best around 16pt.
pub const MENU_ITEM_SIZE: u32 = 18;

pub fn for_color_filled(hex: &str, size: u32) -> Result<Vec<u8>> {
    let (r, g, b) = parse_hex(hex)?;
    let mut img: RgbaImage = ImageBuffer::from_pixel(size, size, Rgba([0, 0, 0, 0]));
    let cx = (size as f32 - 1.0) / 2.0;
    let cy = cx;
    let radius = (size as f32) / 2.0 - 1.5;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha = circle_alpha(dist, radius);
            if alpha > 0 {
                img.put_pixel(x, y, Rgba([r, g, b, alpha]));
            }
        }
    }
    encode_png(&img)
}

pub fn for_color_ring(hex: &str, size: u32) -> Result<Vec<u8>> {
    let (r, g, b) = parse_hex(hex)?;
    let mut img: RgbaImage = ImageBuffer::from_pixel(size, size, Rgba([0, 0, 0, 0]));
    let cx = (size as f32 - 1.0) / 2.0;
    let cy = cx;
    let outer = (size as f32) / 2.0 - 1.5;
    let stroke = ((size as f32) / 8.0).max(1.5);
    let inner = (outer - stroke).max(0.0);

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let outer_a = circle_alpha(dist, outer);
            // Carve out the inner circle so the ring shows the menu background.
            let inner_a = circle_alpha(dist, inner);
            let alpha = outer_a.saturating_sub(inner_a);
            if alpha > 0 {
                img.put_pixel(x, y, Rgba([r, g, b, alpha]));
            }
        }
    }
    encode_png(&img)
}

fn circle_alpha(dist: f32, radius: f32) -> u8 {
    if dist <= radius - 0.5 {
        255
    } else if dist <= radius + 0.5 {
        let t = ((radius + 0.5) - dist).clamp(0.0, 1.0);
        (t * 255.0) as u8
    } else {
        0
    }
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(512);
    image::DynamicImage::ImageRgba8(img.clone())
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)?;
    Ok(out)
}

fn parse_hex(hex: &str) -> Result<(u8, u8, u8)> {
    let s = hex.trim_start_matches('#');
    if s.len() != 6 {
        anyhow::bail!("expected #RRGGBB, got {hex}");
    }
    let r = u8::from_str_radix(&s[0..2], 16)?;
    let g = u8::from_str_radix(&s[2..4], 16)?;
    let b = u8::from_str_radix(&s[4..6], 16)?;
    Ok((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_produces_png_bytes() {
        let bytes = for_color_filled("#7C3AED", MENU_ITEM_SIZE).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn ring_produces_png_bytes() {
        let bytes = for_color_ring("#7C3AED", MENU_ITEM_SIZE).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn rejects_bad_hex() {
        assert!(for_color_filled("nope", 16).is_err());
        assert!(for_color_filled("#12345", 16).is_err());
        assert!(for_color_ring("oops", 16).is_err());
    }
}
