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
        other => bail!("unsupported PNG color type {other:?}"),
    };
    Ok(Rgba { width: w, height: h, pixels })
}

pub fn encode(img: &Rgba) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, img.width, img.height);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().unwrap();
        w.write_image_data(&img.pixels).unwrap();
    }
    out
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
