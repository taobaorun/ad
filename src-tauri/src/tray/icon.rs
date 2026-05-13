//! Generate a 22×22 PNG of a filled colored circle, suitable for the macOS
//! menubar tray.

use anyhow::Result;
use image::{ImageBuffer, Rgba, RgbaImage};

const SIZE: u32 = 22;

pub fn for_color(hex: &str) -> Result<Vec<u8>> {
    let (r, g, b) = parse_hex(hex)?;
    let mut img: RgbaImage = ImageBuffer::from_pixel(SIZE, SIZE, Rgba([0, 0, 0, 0]));

    let cx = (SIZE as f32 - 1.0) / 2.0;
    let cy = cx;
    let radius = (SIZE as f32) / 2.0 - 1.5;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha = if dist <= radius - 0.5 {
                255
            } else if dist <= radius + 0.5 {
                // simple antialias on the edge
                let t = ((radius + 0.5) - dist).clamp(0.0, 1.0);
                (t * 255.0) as u8
            } else {
                0
            };
            if alpha > 0 {
                img.put_pixel(x, y, Rgba([r, g, b, alpha]));
            }
        }
    }

    let mut out = Vec::with_capacity(512);
    image::DynamicImage::ImageRgba8(img)
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
    fn produces_png_bytes() {
        let bytes = for_color("#7C3AED").unwrap();
        // PNG magic header
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn rejects_bad_hex() {
        assert!(for_color("nope").is_err());
        assert!(for_color("#12345").is_err());
    }
}
