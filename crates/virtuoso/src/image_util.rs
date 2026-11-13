//! PNG helpers for screenshots: decode, crop, compare, encode.

use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Any PNG as 8-bit RGBA: palette and gray images expanded (gray below 8 bits scaled up),
/// 16-bit samples cut to their high byte, tRNS made alpha, Adam7 interlacing undone, as the png
/// crate did with EXPAND | STRIP_16 | ALPHA. Its decoder was ~110 KB of the binary, for the two
/// commands that read pixels (cropOn, assertScreenshot); this one is a few KB.
pub fn decode(png: &[u8]) -> Result<Rgba> {
    whole(&mut Png::open(png)?)
}

/// All of a PNG's pixels, pass by pass (one pass unless it is interlaced).
fn whole(p: &mut Png) -> Result<Rgba> {
    let (wu, hu) = (p.w, p.h);
    let mut pixels = vec![0u8; wu * hu * 4];
    const ADAM7: [(usize, usize, usize, usize); 7] = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];
    let passes: &[(usize, usize, usize, usize)] = if p.interlaced { &ADAM7 } else { &[(0, 0, 1, 1)] };
    for &(x0, y0, dx, dy) in passes {
        let (pw, ph) = (wu.saturating_sub(x0).div_ceil(dx), hu.saturating_sub(y0).div_ceil(dy));
        if pw == 0 || ph == 0 {
            continue;
        }
        let stride = p.stride(pw);
        let (mut prev, mut row) = (vec![0u8; stride], vec![0u8; stride]);
        for py in 0..ph {
            p.line(&mut prev, &mut row)?;
            let o = (y0 + py * dy) * wu * 4;
            p.put(&row, &mut pixels[o..o + wu * 4], x0, dx, pw);
        }
    }
    Ok(Rgba { width: wu as u32, height: hu as u32, pixels })
}

/// A PNG's header and palette, and its image data inflated as its scanlines are read.
struct Png<'a> {
    w: usize,
    h: usize,
    depth: usize,
    color: u8,
    channels: usize,
    interlaced: bool,
    /// Every palette index's RGBA.
    table: [[u8; 4]; 256],
    /// Gray and RGB's transparent color, as the samples' own bytes (16 bits) or low bytes.
    key: Option<Vec<u8>>,
    z: Inflate<'a>,
}

impl<'a> Png<'a> {
    fn open(png: &'a [u8]) -> Result<Png<'a>> {
        if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
            bail!("reading PNG: no PNG signature");
        }
        let (mut header, mut palette, mut trns, mut idat) = (None, None, None, Vec::new());
        // CRC-32 four bytes at a time over MBs of image data: its tables (4 KB) made here, from
        // the nibble one, rather than kept in the binary.
        let t0: [u32; 256] = std::array::from_fn(|i| crc32(0, &[i as u8]));
        let next = |t: &[u32; 256]| -> [u32; 256] { std::array::from_fn(|i| (t[i] >> 8) ^ t0[(t[i] & 255) as usize]) };
        let (t1, t2, t3) = {
            let t1 = next(&t0);
            let t2 = next(&t1);
            (t1, t2, next(&t2))
        };
        let crc = |bytes: &[u8]| -> u32 {
            let (quads, rest) = bytes.as_chunks::<4>();
            let c = quads.iter().fold(!0u32, |c, q| {
                let c = c ^ u32::from_le_bytes(*q);
                t3[(c & 255) as usize] ^ t2[(c >> 8 & 255) as usize] ^ t1[(c >> 16 & 255) as usize] ^ t0[(c >> 24) as usize]
            });
            !rest.iter().fold(c, |c, &b| t0[((c ^ b as u32) & 255) as usize] ^ (c >> 8))
        };
        let mut at = 8;
        loop {
            let len = png.get(at..at + 4).map_or(0, |b| u32::from_be_bytes(b.try_into().unwrap())) as usize;
            let (Some(body), Some(sum)) = (png.get(at + 4..at + 8 + len), png.get(at + 8 + len..at + 12 + len)) else {
                bail!("reading PNG: it ends early");
            };
            if crc(body) != u32::from_be_bytes(sum.try_into().unwrap()) {
                bail!("reading PNG: a chunk's CRC does not match");
            }
            let (kind, data) = body.split_at(4);
            match kind {
                b"IHDR" if data.len() == 13 => header = Some(data),
                b"PLTE" => palette = Some(data),
                b"tRNS" => trns = Some(data),
                b"IDAT" => idat.push(data),
                b"IEND" => break,
                _ => {}
            }
            at += 12 + len;
        }
        let Some(ihdr) = header else { bail!("reading PNG: no header") };
        let (w, h) = (u32::from_be_bytes(ihdr[..4].try_into().unwrap()), u32::from_be_bytes(ihdr[4..8].try_into().unwrap()));
        let (depth, color) = (ihdr[8] as usize, ihdr[9]);
        let channels = match (color, depth) {
            (0, 1 | 2 | 4 | 8 | 16) | (3, 1 | 2 | 4 | 8) => 1,
            (2, 8 | 16) => 3,
            (4, 8 | 16) => 2,
            (6, 8 | 16) => 4,
            _ => bail!("reading PNG: color type {color} at {depth} bits"),
        };
        if w == 0 || h == 0 || ihdr[10] != 0 || ihdr[11] != 0 || ihdr[12] > 1 || (color == 3 && palette.is_none()) || (trns.is_some() && color & 4 != 0) {
            bail!("reading PNG: a header it cannot read");
        }
        if trns.is_some_and(|t| color != 3 && t.len() < 2 * channels) {
            bail!("reading PNG: a short tRNS");
        }
        let key = match trns {
            Some(t) if depth == 16 && color != 3 => Some(t.to_vec()),
            Some(t) if color != 3 => Some(t.iter().skip(1).step_by(2).take(channels).copied().collect()),
            _ => None,
        };
        // Black and opaque past the palette's end; a tRNS longer than the palette is ignored.
        let mut table = [[0, 0, 0, 255]; 256];
        if let Some(p) = palette.filter(|_| color == 3) {
            let alpha = trns.filter(|t| t.len() <= p.len() / 3).unwrap_or(&[]);
            for (i, rgb) in p.as_chunks::<3>().0.iter().take(256).enumerate() {
                table[i] = [rgb[0], rgb[1], rgb[2], *alpha.get(i).unwrap_or(&255)];
            }
        }
        let (wu, hu) = (w as usize, h as usize);
        // A header's size is not believed past 1 GB of pixels (nor allowed to overflow).
        if wu.checked_mul(hu).and_then(|n| n.checked_mul(4)).is_none_or(|n| n > 1 << 30) {
            bail!("reading PNG: {w}x{h} is too large");
        }
        Ok(Png { w: wu, h: hu, depth, color, channels, interlaced: ihdr[12] == 1, table, key, z: Inflate::new(idat)? })
    }

    /// The bytes of a scanline `pw` pixels wide.
    fn stride(&self, pw: usize) -> usize {
        (pw * self.channels * self.depth).div_ceil(8)
    }

    /// The next scanline into `row`, unfiltered against the one before it (moved to `prev`).
    fn line(&mut self, prev: &mut Vec<u8>, row: &mut Vec<u8>) -> Result<()> {
        std::mem::swap(prev, row);
        let line = self.z.take(1 + row.len())?;
        row.copy_from_slice(&line[1..]);
        unfilter(line[0], row, prev, (self.channels * self.depth).div_ceil(8))
    }

    /// A scanline's `pw` pixels as RGBA into `out`, an image row: at x0, x0 + dx, ...
    fn put(&self, row: &[u8], out: &mut [u8], x0: usize, dx: usize, pw: usize) {
        let (depth, color, channels) = (self.depth, self.color, self.channels);
        // Screenshots (8-bit RGBA or RGB, whole rows): copied, not a pixel at a time.
        if depth == 8 && dx == 1 && color == 6 {
            out.copy_from_slice(row);
            return;
        }
        if depth == 8 && dx == 1 && color == 2 && self.key.is_none() {
            for (p, s) in out.as_chunks_mut::<4>().0.iter_mut().zip(row.as_chunks::<3>().0) {
                p.copy_from_slice(&[s[0], s[1], s[2], 255]);
            }
            return;
        }
        // The i-th sample of a row: bytes, 16-bit pairs, or bits from the left.
        let sample = |i: usize| -> usize {
            match depth {
                8 => row[i] as usize,
                16 => (row[2 * i] as usize) << 8 | row[2 * i + 1] as usize,
                d => (row[i * d / 8] as usize >> (8 - d - i * d % 8)) & ((1 << d) - 1),
            }
        };
        let eight = |v: usize| if depth == 16 { (v >> 8) as u8 } else if depth < 8 { (v * (255 / ((1 << depth) - 1))) as u8 } else { v as u8 };
        for px in 0..pw {
            let s = |c: usize| sample(px * channels + c);
            let rgba = match color {
                3 => self.table[s(0)],
                4 => [eight(s(0)), eight(s(0)), eight(s(0)), eight(s(1))],
                6 => [eight(s(0)), eight(s(1)), eight(s(2)), eight(s(3))],
                _ => {
                    // Gray or RGB: transparent where the pixel is the tRNS color.
                    let clear = self.key.as_ref().is_some_and(|k| match depth {
                        16 => k[..] == row[px * channels * 2..(px + 1) * channels * 2],
                        _ => (0..channels).all(|c| k[c] as usize == s(c)),
                    });
                    let (r, g, b) = if channels == 1 { (s(0), s(0), s(0)) } else { (s(0), s(1), s(2)) };
                    [eight(r), eight(g), eight(b), if clear { 0 } else { 255 }]
                }
            };
            let o = (x0 + px * dx) * 4;
            out[o..o + 4].copy_from_slice(&rgba);
        }
    }
}

/// A PNG's rows as 8-bit RGBA, one at a time: a whole screenshot is ~10 MB of pixels, and so
/// is its inflated image data; read this way only a row of each (and deflate's 32 KB window).
struct Rows<'a> {
    p: Png<'a>,
    prev: Vec<u8>,
    row: Vec<u8>,
    rgba: Vec<u8>,
    y: usize,
    /// An interlaced PNG comes in passes, not rows: decoded whole.
    whole: Option<Rgba>,
}

impl<'a> Rows<'a> {
    fn new(png: &'a [u8]) -> Result<Rows<'a>> {
        let mut p = Png::open(png)?;
        let whole = if p.interlaced { Some(whole(&mut p)?) } else { None };
        let stride = p.stride(p.w);
        Ok(Rows { prev: vec![0; stride], row: vec![0; stride], rgba: vec![0; p.w * 4], y: 0, whole, p })
    }

    fn next(&mut self) -> Result<Option<&[u8]>> {
        let (y, line) = (self.y, self.p.w * 4);
        if y >= self.p.h {
            return Ok(None);
        }
        self.y += 1;
        if let Some(img) = &self.whole {
            return Ok(Some(&img.pixels[y * line..(y + 1) * line]));
        }
        self.p.line(&mut self.prev, &mut self.row)?;
        self.p.put(&self.row, &mut self.rgba, 0, 1, self.p.w);
        Ok(Some(&self.rgba))
    }
}

/// `similarity` of two PNGs, read a row of each at a time.
pub fn similarity_png(a: &[u8], b: &[u8]) -> Result<f64> {
    let (mut ra, mut rb) = (Rows::new(a)?, Rows::new(b)?);
    let (w, h) = (ra.p.w, ra.p.h);
    if w != rb.p.w || h != rb.p.h {
        return Ok(0.0);
    }
    let mut same = 0usize;
    while let Some(x) = ra.next()? {
        let Some(y) = rb.next()? else { break };
        same += alike(x, y);
    }
    Ok(same as f64 * 100.0 / (w * h) as f64)
}

/// `crop` of a PNG, keeping only the rows and columns inside the rectangle as it reads.
pub fn crop_png(png: &[u8], left: i32, top: i32, right: i32, bottom: i32) -> Result<Rgba> {
    let mut rows = Rows::new(png)?;
    let (w, h) = (rows.p.w as u32, rows.p.h as u32);
    let l = left.clamp(0, w as i32) as u32;
    let t = top.clamp(0, h as i32) as u32;
    let r = right.clamp(l as i32, w as i32) as u32;
    let b = bottom.clamp(t as i32, h as i32) as u32;
    let mut pixels = Vec::with_capacity(((r - l) * (b - t) * 4) as usize);
    for y in 0..b {
        let Some(row) = rows.next()? else { bail!("decoding PNG: too few rows") };
        if y >= t {
            pixels.extend_from_slice(&row[(l * 4) as usize..(r * 4) as usize]);
        }
    }
    Ok(Rgba { width: r - l, height: b - t, pixels })
}

/// Undoes a scanline's filter in place, given the line above (zeros for the first).
fn unfilter(kind: u8, row: &mut [u8], prev: &[u8], bpp: usize) -> Result<()> {
    // A loop per filter type, not a choice per byte.
    let n = row.len();
    match kind {
        0 => {}
        1 => {
            for i in bpp..n {
                row[i] = row[i].wrapping_add(row[i - bpp]);
            }
        }
        2 => {
            for (x, &up) in row.iter_mut().zip(prev) {
                *x = x.wrapping_add(up);
            }
        }
        3 => {
            for i in 0..n {
                let left = if i >= bpp { row[i - bpp] } else { 0 };
                row[i] = row[i].wrapping_add(((left as u16 + prev[i] as u16) / 2) as u8);
            }
        }
        4 => {
            for i in 0..n {
                let (left, up, corner) = if i >= bpp { (row[i - bpp], prev[i], prev[i - bpp]) } else { (0, prev[i], 0) };
                let p = left as i16 + up as i16 - corner as i16;
                let (pa, pb, pc) = ((p - left as i16).abs(), (p - up as i16).abs(), (p - corner as i16).abs());
                row[i] = row[i].wrapping_add(if pa <= pb && pa <= pc {
                    left
                } else if pb <= pc {
                    up
                } else {
                    corner
                });
            }
        }
        _ => bail!("decoding PNG: filter type {kind}"),
    }
    Ok(())
}

/// A zlib stream (RFC 1950/1951) inflated as its bytes are asked for: what was taken is let go,
/// but for deflate's 32 KB window that later copies reach back into. Its Adler-32 is not
/// checked: the png crate did not check it either, and each chunk's CRC already was.
struct Inflate<'a> {
    b: Bits<'a>,
    out: Vec<u8>,
    /// How much of `out` was taken.
    pos: usize,
    block: Block,
    /// The block being read is the stream's last.
    last: bool,
}

enum Block {
    /// A block's header is next.
    Header,
    /// The bytes left of a stored block.
    Stored(usize),
    /// A Huffman-coded block's literal/length and distance codes.
    Codes(Huffman, Huffman),
}

impl<'a> Inflate<'a> {
    fn new(chunks: Vec<&'a [u8]>) -> Result<Inflate<'a>> {
        let mut b = Bits { chunks, next: 0, data: &[], at: 0, buf: 0, n: 0 };
        let (cmf, flg) = (b.take(8)?, b.take(8)?);
        if cmf & 15 != 8 || !(cmf * 256 + flg).is_multiple_of(31) || flg & 32 != 0 {
            bail!("decoding PNG: not a zlib stream");
        }
        Ok(Inflate { b, out: Vec::new(), pos: 0, block: Block::Header, last: false })
    }

    /// The stream's next `n` bytes.
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        // What was taken goes now and then (a 32 KB move each 64 KB), the window stays.
        if self.pos >= 1 << 16 {
            let cut = self.pos.min(self.out.len() - (1 << 15));
            self.out.drain(..cut);
            self.pos -= cut;
        }
        while self.out.len() < self.pos + n {
            self.more(self.pos + n)?;
        }
        self.pos += n;
        Ok(&self.out[self.pos - n..self.pos])
    }

    /// Inflates until `out` holds `want` bytes or the block ends.
    fn more(&mut self, want: usize) -> Result<()> {
        let b = &mut self.b;
        match &mut self.block {
            Block::Header => {
                if self.last {
                    bail!("decoding PNG: its image data ends early");
                }
                self.last = b.take(1)? == 1;
                self.block = match b.take(2)? {
                    0 => {
                        // Stored: whole bytes, after the rest of this one.
                        b.take(b.n % 8)?;
                        let (len, nlen) = (b.take(16)?, b.take(16)?);
                        if len != !nlen & 0xffff {
                            bail!("decoding PNG: a stored block's length does not match");
                        }
                        Block::Stored(len as usize)
                    }
                    1 => {
                        let mut lengths = [8u8; 288 + 32];
                        lengths[144..256].fill(9);
                        lengths[256..280].fill(7);
                        lengths[288..].fill(5);
                        Block::Codes(Huffman::new(&lengths[..288])?, Huffman::new(&lengths[288..])?)
                    }
                    2 => {
                        let (lits, dists, clens) = (b.take(5)? as usize + 257, b.take(5)? as usize + 1, b.take(4)? as usize + 4);
                        let mut order = [0u8; 19];
                        for &i in &[16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15][..clens] {
                            order[i] = b.take(3)? as u8;
                        }
                        let lens = Huffman::new(&order)?;
                        let mut lengths = vec![0u8; lits + dists];
                        let mut i = 0;
                        while i < lengths.len() {
                            let (sym, n) = match lens.read(b)? {
                                s @ 0..=15 => (s as u8, 1),
                                16 if i > 0 => (lengths[i - 1], 3 + b.take(2)?),
                                17 => (0, 3 + b.take(3)?),
                                18 => (0, 11 + b.take(7)?),
                                _ => bail!("decoding PNG: bad code lengths"),
                            };
                            let Some(run) = lengths.get_mut(i..i + n as usize) else { bail!("decoding PNG: bad code lengths") };
                            run.fill(sym);
                            i += n as usize;
                        }
                        Block::Codes(Huffman::new(&lengths[..lits])?, Huffman::new(&lengths[lits..])?)
                    }
                    _ => bail!("decoding PNG: a bad deflate block"),
                };
            }
            Block::Stored(left) => {
                let n = (*left).min(want - self.out.len());
                b.copy(&mut self.out, n)?;
                *left -= n;
                if *left == 0 {
                    self.block = Block::Header;
                }
            }
            Block::Codes(lit, dist) => {
                if codes(b, &mut self.out, lit, dist, want)? {
                    self.block = Block::Header;
                }
            }
        }
        Ok(())
    }
}

/// A Huffman-coded block's literals and copies until `out` holds `want` bytes (false) or the
/// block's end code (true).
fn codes(b: &mut Bits, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman, want: usize) -> Result<bool> {
    const LEN: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
    const DIST: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
    while out.len() < want {
        let sym = lit.read(b)? as usize;
        if sym < 256 {
            out.push(sym as u8);
            continue;
        }
        if sym == 256 {
            return Ok(true);
        }
        let i = sym - 257;
        // Extra bits: none for the first 8 lengths (nor 258), then one more every 4.
        let Some(&base) = LEN.get(i) else { bail!("decoding PNG: bad length code") };
        let len = base as usize + b.take(if (8..28).contains(&i) { (i as u32 - 4) / 4 } else { 0 })? as usize;
        let d = dist.read(b)? as usize;
        let Some(&base) = DIST.get(d) else { bail!("decoding PNG: bad distance code") };
        let back = base as usize + b.take(if d >= 4 { (d as u32 - 2) / 2 } else { 0 })? as usize;
        if back > out.len() {
            bail!("decoding PNG: a copy from before the start");
        }
        // A copy that overlaps what it writes repeats the last `back` bytes: copied from the
        // same start in ever longer runs (each a whole number of repeats), not a byte at a time.
        let from = out.len() - back;
        let mut done = 0;
        while done < len {
            let n = (len - done).min(back + done);
            out.extend_from_within(from..from + n);
            done += n;
        }
    }
    Ok(false)
}

/// Deflate's bits, least significant first, read from the IDAT chunks where they lie (phones
/// write 8 KB chunks: joined, they were a copy of the whole compressed image).
struct Bits<'a> {
    chunks: Vec<&'a [u8]>,
    /// The chunk after `data`.
    next: usize,
    data: &'a [u8],
    at: usize,
    buf: u64,
    n: u32,
}

impl Bits<'_> {
    /// At least `want` bits in hand; past the end, zeros (a few bytes' worth at most).
    #[inline(always)]
    fn fill(&mut self, want: u32) -> Result<()> {
        if self.n >= want {
            return Ok(());
        }
        // Seven whole bytes at once while there are eight to read in this chunk.
        if let Some(b) = self.data.get(self.at..self.at + 8) {
            self.buf |= u64::from_le_bytes(b.try_into().unwrap()) << self.n;
            self.at += ((63 - self.n) / 8) as usize;
            self.n |= 56;
            return Ok(());
        }
        while self.n < want {
            if self.at == self.data.len() && self.next_chunk() {
                continue;
            }
            if self.at >= self.data.len() + 4 {
                bail!("decoding PNG: its data ends early");
            }
            self.buf |= (*self.data.get(self.at).unwrap_or(&0) as u64) << self.n;
            self.at += 1;
            self.n += 8;
        }
        Ok(())
    }

    /// On to the next chunk, if there is one.
    fn next_chunk(&mut self) -> bool {
        let Some(&c) = self.chunks.get(self.next) else { return false };
        (self.data, self.at, self.next) = (c, 0, self.next + 1);
        true
    }

    /// A stored block's next `n` bytes onto `out`: the whole ones in hand, then the chunks'.
    fn copy(&mut self, out: &mut Vec<u8>, mut n: usize) -> Result<()> {
        // In hand past the end are the zeros `fill` made up, not data.
        let made_up = self.at.saturating_sub(self.data.len());
        while n > 0 && self.n as usize / 8 > made_up {
            out.push(self.take(8)? as u8);
            n -= 1;
        }
        // `fill` leaves bits it read ahead above the ones in hand: read again from the chunk.
        self.buf &= (1 << self.n) - 1;
        while n > 0 {
            if self.at >= self.data.len() && !self.next_chunk() {
                bail!("decoding PNG: its data ends early");
            }
            let now = n.min(self.data.len() - self.at);
            out.extend_from_slice(&self.data[self.at..self.at + now]);
            (self.at, n) = (self.at + now, n - now);
        }
        Ok(())
    }

    #[inline(always)]
    fn take(&mut self, n: u32) -> Result<u32> {
        self.fill(n)?;
        let v = (self.buf & ((1 << n) - 1)) as u32;
        self.buf >>= n;
        self.n -= n;
        Ok(v)
    }
}

/// A canonical Huffman code as one table indexed by the next `bits` bits of input: each entry
/// is the symbol and its code's length (0: no code there).
struct Huffman {
    table: Vec<u16>,
    bits: u32,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Huffman> {
        let bits = lengths.iter().copied().max().unwrap_or(0).max(1) as u32;
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut next = [0u16; 16];
        let mut code = 0u16;
        for l in 1..16 {
            code = (code + count[l - 1]) << 1;
            next[l] = code;
        }
        let mut table = vec![0u16; 1 << bits];
        for (sym, &l) in lengths.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let c = next[l as usize];
            next[l as usize] += 1;
            if c >> l != 0 {
                bail!("decoding PNG: an over-full Huffman code");
            }
            // Codes come most significant bit first: the table is indexed by them reversed.
            let r = c.reverse_bits() >> (16 - l);
            for i in (r as usize..1 << bits).step_by(1 << l) {
                table[i] = (sym as u16) << 4 | l as u16;
            }
        }
        Ok(Huffman { table, bits })
    }

    #[inline(always)]
    fn read(&self, b: &mut Bits) -> Result<u16> {
        b.fill(self.bits)?;
        let e = self.table[(b.buf & ((1 << self.bits) - 1)) as usize];
        if e == 0 {
            bail!("decoding PNG: a code that is not in its table");
        }
        b.buf >>= e & 15;
        b.n -= (e & 15) as u32;
        Ok(e >> 4)
    }
}

/// An 8-bit RGBA PNG whose pixels are stored, not compressed: only crops and test images are
/// written, and the png crate's encoder (with its own deflate) is ~20 KB of the binary.
pub fn encode(img: &Rgba) -> Vec<u8> {
    let row = img.width as usize * 4;
    let lines = img.pixels.chunks(row.max(1));
    // Each scanline starts with filter type 0 (none), in stored deflate blocks of at most
    // 65535 bytes, as a zlib stream (header, blocks, Adler-32): the IDAT chunk's data.
    let raw_len = lines.len() + img.pixels.len();
    let idat_len = 2 + raw_len.div_ceil(65535).max(1) * 5 + raw_len + 4;
    let mut out = Vec::with_capacity(8 + 25 + 12 + idat_len + 12);
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&img.width.to_be_bytes());
    ihdr.extend_from_slice(&img.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, no filter, no interlace
    chunk(&mut out, b"IHDR", &ihdr);

    // Written straight into the file: no copy of the scanlines, nor of the zlib stream.
    out.extend_from_slice(&(idat_len as u32).to_be_bytes());
    let idat = out.len();
    out.extend_from_slice(b"IDAT");
    out.extend_from_slice(&[0x78, 0x01]);
    if raw_len == 0 {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    let mut z = Stored { raw_len, written: 0, left: 0, a: 1, b: 0 };
    for line in lines {
        z.put(&mut out, &[0]);
        z.put(&mut out, line);
    }
    out.extend_from_slice(&((z.b << 16) | z.a).to_be_bytes());
    let crc = crc32(!0, &out[idat..]);
    out.extend_from_slice(&(!crc).to_be_bytes());
    chunk(&mut out, b"IEND", &[]);
    out
}

/// The scanlines as stored deflate blocks, and their Adler-32 (`a`, `b`) as they go by.
struct Stored {
    raw_len: usize,
    written: usize,
    /// What is left of the current block.
    left: usize,
    a: u32,
    b: u32,
}

impl Stored {
    fn put(&mut self, out: &mut Vec<u8>, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            if self.left == 0 {
                let len = (self.raw_len - self.written).min(65535);
                out.push(u8::from(self.written + len == self.raw_len));
                out.extend_from_slice(&(len as u16).to_le_bytes());
                out.extend_from_slice(&(!(len as u16)).to_le_bytes());
                self.left = len;
            }
            let (now, rest) = bytes.split_at(self.left.min(bytes.len()));
            out.extend_from_slice(now);
            // zlib's bound: 5552 bytes cannot overflow the sums, so one modulo each per run.
            for run in now.chunks(5552) {
                for &x in run {
                    self.a += x as u32;
                    self.b += self.a;
                }
                self.a %= 65521;
                self.b %= 65521;
            }
            self.left -= now.len();
            self.written += now.len();
            bytes = rest;
        }
    }
}

/// Appends one PNG chunk: length, type, data, CRC-32 of type and data.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(crc32(!0, kind), data);
    out.extend_from_slice(&(!crc).to_be_bytes());
}

/// CRC-32 (PNG's and zlib's) four bits at a time: 64 bytes of table, not 1 KB, and a quarter
/// of the steps of a bit at a time.
const CRC_NIBBLES: [u32; 16] = {
    let mut t = [0u32; 16];
    let mut i = 0;
    while i < 16 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 4 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

/// `crc` carried on over `bytes` (start with !0, and invert at the end).
fn crc32(mut crc: u32, bytes: &[u8]) -> u32 {
    for &x in bytes {
        crc ^= x as u32;
        crc = (crc >> 4) ^ CRC_NIBBLES[(crc & 15) as usize];
        crc = (crc >> 4) ^ CRC_NIBBLES[(crc & 15) as usize];
    }
    crc
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
    alike(&a.pixels, &b.pixels) as f64 * 100.0 / total as f64
}

/// How many RGBA pixels of `a` and `b` have RGB channels that all differ by at most 16.
fn alike(a: &[u8], b: &[u8]) -> usize {
    // Pixel by pixel, RGB only: [u8; 4]s the compiler indexes without checks.
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .filter(|(p, q)| p[0].abs_diff(q[0]) <= 16 && p[1].abs_diff(q[1]) <= 16 && p[2].abs_diff(q[2]) <= 16)
        .count()
}
