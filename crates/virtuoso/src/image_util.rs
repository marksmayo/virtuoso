//! PNG helpers for screenshots: decode, crop, compare, encode.

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn decode(png_bytes: &[u8]) -> Result<Rgba> {
    let mut d = png::Decoder::new(png_bytes);
    d.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut r = d.read_info().context("reading PNG")?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).context("decoding PNG")?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width, info.height);
    let pixels = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => buf.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        other => bail!("unsupported PNG color type {}", other as u8),
    };
    Ok(Rgba { width: w, height: h, pixels })
}

/// An 8-bit RGBA PNG whose pixels are stored, not compressed: only crops and test images are
/// written, and the png crate's encoder (with its own deflate) is ~20 KB of the binary.
pub fn encode(img: &Rgba) -> Vec<u8> {
    let row = img.width as usize * 4;
    // Each scanline starts with filter type 0 (none).
    let mut raw = Vec::with_capacity((row + 1) * img.height as usize);
    for line in img.pixels.chunks(row.max(1)) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    // A zlib stream of stored deflate blocks (at most 65535 bytes each), then Adler-32.
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    if blocks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(b) = blocks.next() {
        z.push(blocks.peek().is_none() as u8);
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&img.width.to_be_bytes());
    ihdr.extend_from_slice(&img.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, no filter, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Appends one PNG chunk: length, type, data, CRC-32 of type and data.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = !0u32;
    for &x in kind.iter().chain(data) {
        crc ^= x as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    out.extend_from_slice(&(!crc).to_be_bytes());
}

pub fn solid_png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let pixels = (0..width * height).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect();
    encode(&Rgba { width, height, pixels })
}

/// The part of `img` inside the rectangle, clamped to the image.
pub fn crop(img: &Rgba, left: i32, top: i32, right: i32, bottom: i32) -> Rgba {
    let l = left.clamp(0, img.width as i32) as u32;
    let t = top.clamp(0, img.height as i32) as u32;
    let r = right.clamp(l as i32, img.width as i32) as u32;
    let b = bottom.clamp(t as i32, img.height as i32) as u32;
    let mut pixels = Vec::with_capacity(((r - l) * (b - t) * 4) as usize);
    for y in t..b {
        let row = ((y * img.width + l) * 4) as usize;
        pixels.extend_from_slice(&img.pixels[row..row + ((r - l) * 4) as usize]);
    }
    Rgba { width: r - l, height: b - t, pixels }
}

/// How alike two images are, in percent: the share of pixels whose channels all differ by at
/// most 16. Images of different sizes are 0% alike.
pub fn similarity(a: &Rgba, b: &Rgba) -> f64 {
    if a.width != b.width || a.height != b.height || a.pixels.is_empty() {
        return 0.0;
    }
    let total = (a.width * a.height) as usize;
    let same = a
        .pixels
        .chunks(4)
        .zip(b.pixels.chunks(4))
        .filter(|(p, q)| p.iter().zip(q.iter()).take(3).all(|(x, y)| x.abs_diff(*y) <= 16))
        .count();
    same as f64 * 100.0 / total as f64
}
