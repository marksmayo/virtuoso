//! The PNG helpers screenshots go through: cropOn writes crops, assertScreenshot compares.

use virtuoso::image_util::{crop, crop_png, decode, encode, similarity, similarity_png, Rgba};

/// A phone screen's worth of pixels that are not all alike.
fn screen(width: u32, height: u32) -> Rgba {
    let pixels = (0..width * height).flat_map(|i| [(i % 251) as u8, (i / 7 % 253) as u8, (i / 1000 % 256) as u8, 255]).collect();
    Rgba { width, height, pixels }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

#[test]
fn an_encoded_png_decodes_to_the_same_pixels_and_its_bytes_do_not_change() {
    // The hashes of what the first encoder wrote: stored blocks (301x217 spans four), Adler-32
    // and CRC-32 must come out byte for byte the same.
    let known = [
        (0, 0, 0x2ebc465320d8e1a4),
        (1, 1, 0x3f09d3947046f62c),
        (3, 0, 0x40dc6b578037815d),
        (130, 140, 0x9251a2730ecd5101),
        (301, 217, 0x6e7829713be920f0),
    ];
    for (w, h, hash) in known {
        let img = screen(w, h);
        let png = encode(&img);
        if w > 0 && h > 0 {
            assert_eq!(decode(&png).unwrap(), img, "{w}x{h}");
        }
        assert_eq!(fnv1a(&png), hash, "{w}x{h}");
    }
}

#[test]
fn a_full_screen_crop_encodes_quickly() {
    let img = screen(1080, 2400);
    let t = std::time::Instant::now();
    let png = encode(&img);
    let took = t.elapsed();
    assert!(png.len() > img.pixels.len());
    // ~10 MB stored: a CRC bit by bit and two divisions per byte took ~1 s in a test build
    // (~210 ms in a release one).
    assert!(took < std::time::Duration::from_millis(400), "{took:?}");
}

#[test]
fn a_full_screen_comparison_is_quick() {
    let a = screen(1080, 2400);
    let mut b = a.clone();
    b.pixels[..4000].fill(0);
    let t = std::time::Instant::now();
    let alike = similarity(&a, &b);
    let took = t.elapsed();
    assert!(alike > 99.0 && alike < 100.0, "{alike}");
    // 2.6M pixels: nested iterators per pixel took ~390 ms in a test build.
    assert!(took < std::time::Duration::from_millis(150), "{took:?}");
}

// ---- decode, checked against the png crate's decoder it replaced ----

/// What `decode` gave when it was the png crate's: EXPAND | STRIP_16 | ALPHA, gray made RGBA.
fn png_crate_decode(bytes: &[u8]) -> Option<Rgba> {
    let mut d = png::Decoder::new(bytes);
    d.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let pixels = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => buf.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        _ => return None,
    };
    Some(Rgba { width: info.width, height: info.height, pixels })
}

fn crc(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |c, &b| (0..8).fold(c ^ b as u32, |c, _| if c & 1 == 1 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 }))
}

fn put_chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let c = crc(&out[start..]);
    out.extend_from_slice(&c.to_be_bytes());
}

fn png_file(ihdr: [u8; 13], plte: Option<&[u8]>, trns: Option<&[u8]>, zlib: &[u8]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    put_chunk(&mut out, b"IHDR", &ihdr);
    if let Some(p) = plte {
        put_chunk(&mut out, b"PLTE", p);
    }
    if let Some(t) = trns {
        put_chunk(&mut out, b"tRNS", t);
    }
    // Split in two, as encoders do with large images.
    let half = zlib.len() / 2;
    put_chunk(&mut out, b"IDAT", &zlib[..half]);
    put_chunk(&mut out, b"IDAT", &zlib[half..]);
    put_chunk(&mut out, b"IEND", &[]);
    out
}

/// The same PNG with its image data in IDAT chunks of 0 to 12 bytes, one after another.
fn in_tiny_chunks(png: &[u8]) -> Vec<u8> {
    let (mut at, mut zlib, mut out) = (8, Vec::new(), png[..8].to_vec());
    while at < png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let kind = &png[at + 4..at + 8];
        if kind == b"IDAT" {
            zlib.extend_from_slice(&png[at + 8..at + 8 + len]);
        } else {
            if kind == b"IEND" {
                let mut rest = &zlib[..];
                for n in (0..13).cycle() {
                    let (now, later) = rest.split_at(n.min(rest.len()));
                    put_chunk(&mut out, b"IDAT", now);
                    rest = later;
                    if rest.is_empty() {
                        break;
                    }
                }
            }
            out.extend_from_slice(&png[at..at + 12 + len]);
        }
        at += 12 + len;
    }
    out
}

fn header(w: u32, h: u32, depth: u8, color: u8, interlace: bool) -> [u8; 13] {
    let mut ihdr = [0u8; 13];
    ihdr[..4].copy_from_slice(&w.to_be_bytes());
    ihdr[4..8].copy_from_slice(&h.to_be_bytes());
    ihdr[8] = depth;
    ihdr[9] = color;
    ihdr[12] = u8::from(interlace);
    ihdr
}

/// A PNG of any colour type, bit depth and interlacing: each scanline filtered with the five
/// filter types in turn, in stored deflate blocks. `sample(x, y, channel)` gives the values.
#[allow(clippy::too_many_arguments)]
fn any_png(w: u32, h: u32, color: u8, depth: u8, interlace: bool, plte: Option<&[u8]>, trns: Option<&[u8]>, sample: &dyn Fn(u32, u32, usize) -> u16) -> Vec<u8> {
    let channels = match color { 0 | 3 => 1, 2 => 3, 4 => 2, _ => 4 };
    let bits = channels * depth as usize;
    let bpp = bits.div_ceil(8);
    let passes: Vec<(u32, u32, u32, u32)> = if interlace { vec![(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)] } else { vec![(0, 0, 1, 1)] };
    let mut raw = Vec::new();
    let mut kind = 0u8;
    for (x0, y0, dx, dy) in passes {
        let (pw, ph) = (w.saturating_sub(x0).div_ceil(dx), h.saturating_sub(y0).div_ceil(dy));
        if pw == 0 || ph == 0 {
            continue;
        }
        let stride = (pw as usize * bits).div_ceil(8);
        let mut prev = vec![0u8; stride];
        for py in 0..ph {
            let mut line = vec![0u8; stride];
            for px in 0..pw {
                for c in 0..channels {
                    let v = sample(x0 + px * dx, y0 + py * dy, c);
                    let i = px as usize * channels + c;
                    match depth {
                        16 => line[2 * i..2 * i + 2].copy_from_slice(&v.to_be_bytes()),
                        8 => line[i] = v as u8,
                        d => line[i * d as usize / 8] |= (v as u8) << (8 - d as usize - i * d as usize % 8),
                    }
                }
            }
            raw.push(kind);
            for i in 0..stride {
                let (a, b) = (if i >= bpp { line[i - bpp] } else { 0 } as i16, prev[i] as i16);
                let c = if i >= bpp { prev[i - bpp] } else { 0 } as i16;
                let predicted = match kind {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => (a + b) / 2,
                    _ => {
                        let p = a + b - c;
                        let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                        if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                    }
                };
                raw.push(line[i].wrapping_sub(predicted as u8));
            }
            kind = (kind + 1) % 5;
            prev = line;
        }
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (n, b) in blocks.iter().enumerate() {
        z.push(u8::from(n + 1 == blocks.len()));
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut s) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        s = (s + a) % 65521;
    }
    z.extend_from_slice(&(s << 16 | a).to_be_bytes());
    png_file(header(w, h, depth, color, interlace), plte, trns, &z)
}

fn noise(x: u32, y: u32, c: usize) -> u16 {
    let mut v = x.wrapping_mul(0x9e37_79b9) ^ y.wrapping_mul(0x85eb_ca6b) ^ (c as u32).wrapping_mul(0xc2b2_ae35);
    v ^= v >> 15;
    v = v.wrapping_mul(0x2c1b_3c6d);
    (v ^ (v >> 12)) as u16
}

#[test]
fn every_kind_of_png_decodes_as_the_png_crate_decoded_it() {
    let images = every_kind_of_png();
    for (what, png) in &images {
        let want = png_crate_decode(png).unwrap_or_else(|| panic!("the png crate read {what}"));
        assert_eq!(decode(png).unwrap(), want, "{what}");
    }
    assert!(images.len() > 200, "{}", images.len());
}

#[test]
fn every_kind_of_png_compares_and_crops_row_by_row_as_decoded_whole() {
    // assertScreenshot and cropOn read a row at a time, not whole frames: the same answers.
    let images = every_kind_of_png();
    let other = any_png(9, 9, 6, 8, false, None, None, &|x, y, c| noise(y, x, c) & 255);
    for (what, png) in &images {
        let img = decode(png).unwrap();
        assert_eq!(similarity_png(png, png).unwrap(), 100.0, "{what}");
        assert_eq!(similarity_png(png, &other).unwrap(), similarity(&img, &decode(&other).unwrap()), "{what}");
        for (l, t, r, b) in [(1, 1, 4, 3), (-5, -5, 100, 100), (2, 2, 2, 9), (8, 8, 1, 1), (0, 2, 33, 3)] {
            assert_eq!(crop_png(png, l, t, r, b).unwrap(), crop(&img, l, t, r, b), "{what} cropped to {l},{t},{r},{b}");
        }
    }
}

#[test]
fn a_full_screen_screenshot_compares_and_crops_row_by_row_as_decoded_whole() {
    // Deflated by the png crate, as phones do: copies reach back across what was let go.
    let img = screen(1080, 2400);
    let mut other = img.clone();
    other.pixels[..1080 * 4 * 600].iter_mut().for_each(|p| *p = p.wrapping_add(100));
    let encoded = |img: &Rgba| {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, img.width, img.height);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
        e.write_header().unwrap().write_image_data(&img.pixels).unwrap();
        out
    };
    let (a, b) = (encoded(&img), encoded(&other));
    assert_eq!(similarity_png(&a, &b).unwrap(), similarity(&img, &other));
    assert_eq!(similarity_png(&encode(&img), &b).unwrap(), similarity(&img, &other));
    assert_eq!(crop_png(&a, 100, 1000, 900, 2300).unwrap(), crop(&img, 100, 1000, 900, 2300));
    assert_eq!(crop_png(&encode(&other), 0, 0, 1080, 2400).unwrap(), other);
}

#[test]
fn image_data_in_many_small_chunks_reads_as_in_one() {
    // Phones write it in 8 KB IDATs; deflate's codes, stored blocks and the zlib header itself
    // run across chunk boundaries, here every few bytes.
    let mut images: Vec<Vec<u8>> = every_kind_of_png().into_iter().map(|(_, png)| png).step_by(7).collect();
    let screen = screen(300, 211);
    images.push(encode(&screen));
    let mut deflated = Vec::new();
    let mut e = png::Encoder::new(&mut deflated, screen.width, screen.height);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
    e.write_header().unwrap().write_image_data(&screen.pixels).unwrap();
    images.push(deflated);
    for png in &images {
        let tiny = in_tiny_chunks(png);
        let img = decode(png).unwrap();
        assert_eq!(decode(&tiny).unwrap(), img);
        assert_eq!(similarity_png(&tiny, png).unwrap(), 100.0);
        assert_eq!(crop_png(&tiny, 1, 1, 200, 100).unwrap(), crop(&img, 1, 1, 200, 100));
    }
    // Cut short inside a stored block: an error, not zeros.
    let stored = in_tiny_chunks(&encode(&screen));
    let cut = in_tiny_chunks(&png_file(header(300, 211, 8, 6, false), None, None, &zlib_of(&stored)[..20_000]));
    assert!(decode(&cut).is_err());
}

/// A PNG's image data, its IDAT chunks joined.
fn zlib_of(png: &[u8]) -> Vec<u8> {
    let (mut at, mut zlib) = (8, Vec::new());
    while at < png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        if &png[at + 4..at + 8] == b"IDAT" {
            zlib.extend_from_slice(&png[at + 8..at + 8 + len]);
        }
        at += 12 + len;
    }
    zlib
}

#[test]
fn a_broken_png_is_an_error_when_compared_or_cropped_too() {
    let good = any_png(9, 9, 6, 8, false, None, None, &noise);
    for cut in [0, 8, 33, good.len() / 2, good.len() - 13] {
        assert!(similarity_png(&good[..cut], &good).is_err(), "cut at {cut}");
        assert!(similarity_png(&good, &good[..cut]).is_err(), "cut at {cut}");
        assert!(crop_png(&good[..cut], 0, 0, 9, 9).is_err(), "cut at {cut}");
    }
    let garbage = png_file(header(9, 9, 8, 6, false), None, None, &[0x78, 0x01, 0xff, 0xfe, 0xfd, 0x00, 0x13]);
    assert!(similarity_png(&garbage, &good).is_err());
    assert!(crop_png(&garbage, 0, 0, 9, 9).is_err());
}

/// 240+ PNGs: every color type and bit depth, with and without tRNS, interlaced or not, in
/// sizes that end mid-byte and mid-pass.
fn every_kind_of_png() -> Vec<(String, Vec<u8>)> {
    let palette: Vec<u8> = (0..20u8).flat_map(|i| [i * 12, 255 - i * 7, i * 3]).collect();
    let mut images = Vec::new();
    for (color, depths) in [(0u8, &[1u8, 2, 4, 8, 16][..]), (2, &[8, 16]), (3, &[1, 2, 4, 8]), (4, &[8, 16]), (6, &[8, 16])] {
        for &depth in depths {
            let max = if depth == 16 { u16::MAX } else { (1u16 << depth) - 1 };
            // Gray and RGB: a tRNS color that some pixels have (the sample function's own).
            let key = |c: usize| (noise(3, 1, c) & max).to_be_bytes();
            let trns_options: Vec<Option<Vec<u8>>> = match color {
                0 => vec![None, Some(key(0).to_vec())],
                2 => vec![None, Some((0..3).flat_map(key).collect())],
                // Palette: alpha for the first few entries; one longer than the palette is ignored.
                3 => vec![None, Some(vec![0, 80, 160]), Some(vec![7; 21])],
                _ => vec![None],
            };
            for trns in &trns_options {
                for interlace in [false, true] {
                    for (w, h) in [(1, 1), (5, 3), (9, 9), (13, 7), (2, 17), (33, 4)] {
                        let sample = |x: u32, y: u32, c: usize| {
                            // Every so often the tRNS color, so transparency shows.
                            if color != 3 && (x + y).is_multiple_of(4) { u16::from_be_bytes(key(c)) } else if color == 3 { noise(x, y, c) % (palette.len() as u16 / 3 + 2).min(max + 1) } else { noise(x, y, c) & max }
                        };
                        let plte = (color == 3).then_some(&palette[..]);
                        let png = any_png(w, h, color, depth, interlace, plte, trns.as_deref(), &sample);
                        images.push((format!("color {color}, {depth} bits, tRNS {trns:?}, interlaced {interlace}, {w}x{h}"), png));
                    }
                }
            }
        }
    }
    images
}

#[test]
fn compressed_pngs_decode_as_the_png_crate_decoded_them() {
    // The png crate's own encoder: deflate's dynamic Huffman blocks and adaptive filters, as
    // phones and image editors write them.
    for (color, ct, channels) in [(png::ColorType::Rgba, 6, 4), (png::ColorType::Rgb, 2, 3), (png::ColorType::Grayscale, 0, 1)] {
        for compression in [png::Compression::Fast, png::Compression::Default, png::Compression::Best] {
            let (w, h) = (300u32, 211u32);
            let data: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| (0..channels).map(move |c| if (x / 16 + y / 16) % 3 == 0 { 200 } else { (noise(x, y, c) % 7) as u8 * 30 }))).collect();
            let mut png_bytes = Vec::new();
            let mut e = png::Encoder::new(&mut png_bytes, w, h);
            e.set_color(color);
            e.set_depth(png::BitDepth::Eight);
            e.set_compression(compression);
            e.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
            e.write_header().unwrap().write_image_data(&data).unwrap();
            assert_eq!(decode(&png_bytes).unwrap(), png_crate_decode(&png_bytes).unwrap(), "color type {ct}, {compression:?}");
        }
    }
    // Deflate's fixed Huffman block (zlib's choice for a small image), with an overlapping copy.
    let fixed = [0x78, 0xda, 0x63, 0xe0, 0x12, 0x91, 0xd3, 0x30, 0xb2, 0x71, 0x0b, 0x88, 0x62, 0x64, 0x85, 0x01, 0x00, 0x19, 0x05, 0x01, 0xf1];
    let png_bytes = png_file(header(3, 2, 8, 2, false), None, None, &fixed);
    let img = decode(&png_bytes).unwrap();
    assert_eq!(img, png_crate_decode(&png_bytes).unwrap());
    assert_eq!(&img.pixels[..8], &[10, 20, 30, 255, 40, 50, 60, 255]);
}

#[test]
fn a_broken_png_is_an_error_not_a_panic() {
    let good = any_png(9, 9, 6, 8, false, None, None, &noise);
    assert!(decode(&good).is_ok());
    for cut in [0, 7, 8, 20, 33, 40, good.len() / 2, good.len() - 13] {
        assert!(decode(&good[..cut]).is_err(), "cut at {cut}");
    }
    let mut bad_crc = good.clone();
    bad_crc[40] ^= 1;
    assert!(decode(&bad_crc).is_err());
    assert!(png_crate_decode(&bad_crc).is_none(), "the png crate refused it too");
    // A deflate stream that is garbage (with right CRCs).
    let garbage = png_file(header(4, 4, 8, 6, false), None, None, &[0x78, 0x01, 0xff, 0xfe, 0xfd, 0x00, 0x13]);
    assert!(decode(&garbage).is_err());
    assert!(decode(b"not a png at all").is_err());
    // A header that claims more pixels than memory holds.
    let huge = png_file(header(100_000, 100_000, 8, 6, false), None, None, &[0x78, 0x01, 0x01, 0x00, 0x00, 0xff, 0xff, 0, 0, 0, 1]);
    assert!(decode(&huge).is_err());
}

#[test]
fn a_full_screen_screenshot_decodes_quickly() {
    // As a phone sends it: 1080x2400 RGBA, deflated with adaptive filters.
    let img = screen(1080, 2400);
    let mut png_bytes = Vec::new();
    let mut e = png::Encoder::new(&mut png_bytes, img.width, img.height);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
    e.write_header().unwrap().write_image_data(&img.pixels).unwrap();
    let t = std::time::Instant::now();
    let got = decode(&png_bytes).unwrap();
    let took = t.elapsed();
    assert_eq!(got, img);
    assert!(took < std::time::Duration::from_millis(1500), "{took:?}");
}
