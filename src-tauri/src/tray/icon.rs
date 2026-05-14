//! Generate small icon PNGs:
//! - `for_brand_with_ring(hex)`: the embedded brand artwork wrapped in a
//!   profile-colored ring; used as the menubar tray icon so the active
//!   profile color is always visible at a glance.
//! - `for_color_filled(hex, size)`: filled circle for the active-profile
//!   menu-item.
//! - `for_color_ring(hex, size)`: hollow ring for inactive-profile menu items.

use anyhow::Result;
use image::{ImageBuffer, Rgba, RgbaImage};

/// Embedded brand artwork — the scope + `</>` mark on dark navy. Compiled
/// in so the menubar tray icon stays consistent across dev and bundled
/// builds and never depends on a runtime resource lookup.
const BRAND_ICON_PNG: &[u8] = include_bytes!("../../icons/32x32.png");

/// Default size for menu-item icons. macOS menu items render best around 16pt.
pub const MENU_ITEM_SIZE: u32 = 18;

/// Composes a 44×44 PNG (= 22pt @2x for crisp retina rendering): the brand
/// artwork masked to a circle, plus a 2-pixel ring of `ring_hex` on the
/// outside. At @2x display that 2-pixel stroke reads as a 1-point stroke,
/// which is what the user asked for.
pub fn for_brand_with_ring(ring_hex: &str) -> Result<Vec<u8>> {
    /// 22pt @ 2x retina. Anything smaller and the ring loses antialiasing.
    const SIZE: u32 = 44;
    /// 2 actual pixels = 1 logical point on a retina menubar.
    const STROKE: f32 = 2.0;
    /// Brand fills nearly to the ring; masked to circle so corners are
    /// clipped cleanly.
    const INNER: u32 = 38;

    let (r, g, b) = parse_hex(ring_hex)?;
    let mut canvas: RgbaImage = ImageBuffer::from_pixel(SIZE, SIZE, Rgba([0, 0, 0, 0]));
    let cx = (SIZE as f32 - 1.0) / 2.0;
    let outer = (SIZE as f32) / 2.0 - 0.5;
    let inner = outer - STROKE;

    // 1) Outer colored ring.
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - cx;
            let dy = y as f32 - cx;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha = circle_alpha(dist, outer).saturating_sub(circle_alpha(dist, inner));
            if alpha > 0 {
                canvas.put_pixel(x, y, Rgba([r, g, b, alpha]));
            }
        }
    }

    // 2) Resize embedded brand to INNER × INNER and mask to the circle just
    //    inside the ring (so the navy square's corners don't poke through).
    let brand = image::load_from_memory(BRAND_ICON_PNG)?.to_rgba8();
    let mut masked =
        image::imageops::resize(&brand, INNER, INNER, image::imageops::FilterType::Lanczos3);
    let offset = ((SIZE - INNER) / 2) as f32;
    let mask_radius = inner - 0.5;
    for y in 0..INNER {
        for x in 0..INNER {
            let canvas_x = x as f32 + offset;
            let canvas_y = y as f32 + offset;
            let dx = canvas_x - cx;
            let dy = canvas_y - cx;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > mask_radius - 0.5 {
                let factor = if dist <= mask_radius + 0.5 {
                    ((mask_radius + 0.5) - dist).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let p = masked.get_pixel_mut(x, y);
                p[3] = (p[3] as f32 * factor) as u8;
            }
        }
    }
    image::imageops::overlay(&mut canvas, &masked, offset as i64, offset as i64);

    encode_png(&canvas)
}

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
    fn brand_with_ring_produces_png_bytes() {
        let bytes = for_brand_with_ring("#3B82F6").unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }

    /// Renders the menubar icon at 6× scale per profile color so the
    /// composition can be eyeballed without launching the app. Run with:
    ///   cargo test --lib tray::icon::tests::dump_brand_with_ring -- --include-ignored
    #[test]
    #[ignore]
    fn dump_brand_with_ring() {
        for (name, hex) in [
            ("purple", "#7C3AED"),
            ("blue", "#3B82F6"),
            ("orange", "#F97316"),
        ] {
            let bytes = for_brand_with_ring(hex).unwrap();
            std::fs::write(format!("/tmp/tray-{name}.png"), bytes).unwrap();
        }
    }

    #[test]
    fn rejects_bad_hex() {
        assert!(for_color_filled("nope", 16).is_err());
        assert!(for_color_filled("#12345", 16).is_err());
        assert!(for_color_ring("oops", 16).is_err());
        assert!(for_brand_with_ring("nope").is_err());
    }
}
