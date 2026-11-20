//! A compact TrueType font engine used for all UI text.
//!
//! Rendering UI text with our own rasterizer (instead of GDI/Xft) keeps the
//! Windows and Linux builds visually identical, removes a platform dependency
//! and avoids an FFI surface. The supported feature set is exactly what a
//! desktop UI needs: simple and composite glyphs, `cmap` formats 4 and 12,
//! horizontal metrics, anti-aliased scanline rasterization and cheap faux-bold.
//!
//! The embedded DejaVu Sans face (see `assets/fonts`) covers Latin, Greek and
//! Cyrillic; other scripts fall back to `.notdef` boxes.

use std::cell::RefCell;
use std::collections::HashMap;

use super::canvas::{Bitmap, Color};

/// Embedded UI face (DejaVu Sans, Bitstream Vera derived licence, see
/// `assets/fonts/DejaVuSans.LICENSE`).
pub static UI_FONT_DATA: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans.ttf");

#[derive(Debug)]
pub struct FontError(pub String);

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "font error: {}", self.0)
    }
}

impl std::error::Error for FontError {}

fn err<T>(msg: &str) -> Result<T, FontError> {
    Err(FontError(msg.to_string()))
}

#[inline]
fn be_u16(d: &[u8], o: usize) -> Result<u16, FontError> {
    if o + 2 > d.len() {
        return err("read past end of font data");
    }
    Ok(u16::from_be_bytes([d[o], d[o + 1]]))
}

#[inline]
fn be_i16(d: &[u8], o: usize) -> Result<i16, FontError> {
    Ok(be_u16(d, o)? as i16)
}

#[inline]
fn be_u32(d: &[u8], o: usize) -> Result<u32, FontError> {
    if o + 4 > d.len() {
        return err("read past end of font data");
    }
    Ok(u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]))
}

#[inline]
fn f2dot14(v: i16) -> f32 {
    v as f32 / 16384.0
}

/// A point on a glyph outline in font units.
#[derive(Clone, Copy, Debug)]
struct Pt {
    x: f32,
    y: f32,
    on: bool,
}

/// Parsed, immutable font tables.
pub struct Face {
    data: &'static [u8],
    head: usize,
    maxp: usize,
    hhea: usize,
    hmtx: usize,
    cmap: Option<(usize, u16)>,
    loca: usize,
    glyf: usize,
    units_per_em: f32,
    index_to_loc: i16,
    num_glyphs: u16,
    num_hmetrics: u16,
    pub ascender: f32,
    pub descender: f32,
    pub line_gap: f32,
}

impl Face {
    pub fn parse(data: &'static [u8]) -> Result<Face, FontError> {
        if data.len() < 12 {
            return err("file too small to be a font");
        }
        let version = be_u32(data, 0)?;
        if version != 0x0001_0000 && version != 0x7472_7565 {
            return err("unsupported font version (only TrueType is supported)");
        }
        let num_tables = be_u16(data, 4)? as usize;
        let mut tables: HashMap<[u8; 4], (usize, usize)> = HashMap::new();
        for i in 0..num_tables {
            let rec = 12 + i * 16;
            if rec + 16 > data.len() {
                return err("truncated table directory");
            }
            let tag = [data[rec], data[rec + 1], data[rec + 2], data[rec + 3]];
            let offset = be_u32(data, rec + 8)? as usize;
            let len = be_u32(data, rec + 12)? as usize;
            if offset > data.len() {
                return err("table offset out of range");
            }
            tables.insert(tag, (offset, len.min(data.len() - offset)));
        }
        let get = |tag: &[u8; 4]| -> Result<usize, FontError> {
            tables
                .get(tag)
                .map(|(o, _)| *o)
                .ok_or_else(|| FontError(format!("missing '{}' table", String::from_utf8_lossy(tag))))
        };
        let head = get(b"head")?;
        let maxp = get(b"maxp")?;
        let hhea = get(b"hhea")?;
        let hmtx = get(b"hmtx")?;
        let loca = get(b"loca")?;
        let glyf = get(b"glyf")?;

        let units_per_em = be_u16(data, head + 18)? as f32;
        if units_per_em <= 0.0 {
            return err("invalid unitsPerEm");
        }
        let index_to_loc = be_i16(data, head + 50)?;
        let num_glyphs = be_u16(data, maxp + 4)?;
        let num_hmetrics = be_u16(data, hhea + 34)?;
        let ascender = be_i16(data, hhea + 4)? as f32;
        let descender = be_i16(data, hhea + 6)? as f32;
        let line_gap = be_i16(data, hhea + 8)? as f32;

        // Pick the best unicode cmap subtable: prefer full-repertoire (format 12).
        let cmap_off = get(b"cmap")?;
        let n = be_u16(data, cmap_off + 2)? as usize;
        let mut best: Option<(usize, u16)> = None;
        for i in 0..n {
            let rec = cmap_off + 4 + i * 8;
            if rec + 8 > data.len() {
                break;
            }
            let platform = be_u16(data, rec)?;
            let encoding = be_u16(data, rec + 1 + 1)?;
            let sub = cmap_off + be_u32(data, rec + 4)? as usize;
            let format = be_u16(data, sub).unwrap_or(0);
            let unicode = platform == 0 || (platform == 3 && (encoding == 1 || encoding == 10));
            if !unicode {
                continue;
            }
            let score = match format {
                12 => 3,
                4 => 2,
                _ => 0,
            };
            if score == 0 {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, f)) => match (f, format) {
                    (12, _) => false,
                    (4, 12) => true,
                    (4, 4) => false,
                    _ => true,
                },
            };
            if better {
                best = Some((sub, format));
            }
        }

        Ok(Face {
            data,
            head,
            maxp,
            hhea,
            hmtx,
            cmap: best,
            loca,
            glyf,
            units_per_em,
            index_to_loc,
            num_glyphs,
            num_hmetrics,
            ascender,
            descender,
            line_gap,
        })
    }

    pub fn units_per_em(&self) -> f32 {
        self.units_per_em
    }

    pub fn num_glyphs(&self) -> u16 {
        self.num_glyphs
    }

    /// Maps a unicode scalar to a glyph id using the selected cmap subtable.
    pub fn glyph_index(&self, ch: char) -> u16 {
        let Some((sub, format)) = self.cmap else {
            return 0;
        };
        let cp = ch as u32;
        match format {
            4 => self.cmap_format4(sub, cp),
            12 => self.cmap_format12(sub, cp),
            _ => 0,
        }
    }

    fn cmap_format4(&self, sub: usize, cp: u32) -> u16 {
        if cp > 0xFFFF {
            return 0;
        }
        let d = self.data;
        let Ok(seg_x2) = be_u16(d, sub + 6) else {
            return 0;
        };
        let seg = (seg_x2 / 2) as usize;
        let end_base = sub + 14;
        let start_base = end_base + seg * 2 + 2;
        let delta_base = start_base + seg * 2;
        let range_base = delta_base + seg * 2;
        let cp16 = cp as u16;
        for i in 0..seg {
            let Ok(end) = be_u16(d, end_base + i * 2) else {
                return 0;
            };
            if cp16 > end {
                continue;
            }
            let Ok(start) = be_u16(d, start_base + i * 2) else {
                return 0;
            };
            if cp16 < start {
                return 0;
            }
            let Ok(delta) = be_u16(d, delta_base + i * 2) else {
                return 0;
            };
            let Ok(range_off) = be_u16(d, range_base + i * 2) else {
                return 0;
            };
            if range_off == 0 {
                return cp16.wrapping_add(delta);
            }
            let addr = range_base + i * 2 + range_off as usize + (cp16 - start) as usize * 2;
            let Ok(gid) = be_u16(d, addr) else {
                return 0;
            };
            if gid == 0 {
                return 0;
            }
            return gid.wrapping_add(delta);
        }
        0
    }

    fn cmap_format12(&self, sub: usize, cp: u32) -> u16 {
        let d = self.data;
        let Ok(ngroups) = be_u32(d, sub + 12) else {
            return 0;
        };
        let groups = ngroups as usize;
        let mut lo = 0usize;
        let mut hi = groups;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let g = sub + 16 + mid * 12;
            let (Ok(start), Ok(end), Ok(gid)) = (be_u32(d, g), be_u32(d, g + 4), be_u32(d, g + 8)) else {
                return 0;
            };
            if cp < start {
                hi = mid;
            } else if cp > end {
                lo = mid + 1;
            } else {
                let glyph = gid.saturating_add(cp.saturating_sub(start));
                return if glyph > 0xFFFF { 0 } else { glyph as u16 };
            }
        }
        0
    }

    /// Advance width of `gid` in font units.
    pub fn advance_units(&self, gid: u16) -> f32 {
        let n = self.num_hmetrics.max(1) as u16;
        let idx = gid.min(n.saturating_sub(1));
        match be_u16(self.data, self.hmtx + idx as usize * 4) {
            Ok(adv) => adv as f32,
            Err(_) => 0.0,
        }
    }

    fn glyph_range(&self, gid: u16) -> Option<(usize, usize)> {
        let d = self.data;
        let gid = gid as usize;
        if gid >= self.num_glyphs as usize {
            return None;
        }
        let (start, end) = if self.index_to_loc == 0 {
            let s = be_u16(d, self.loca + gid * 2).ok()? as usize * 2;
            let e = be_u16(d, self.loca + gid * 2 + 2).ok()? as usize * 2;
            (s, e)
        } else {
            let s = be_u32(d, self.loca + gid * 4).ok()? as usize;
            let e = be_u32(d, self.loca + gid * 4 + 4).ok()? as usize;
            (s, e)
        };
        // Empty glyph (e.g. space) has start == end.
        Some((self.glyf + start, self.glyf + end))
    }

    /// Collects the flattened outline of `gid` in font units into `out`.
    fn outline(&self, gid: u16, out: &mut Vec<Vec<Pt>>, depth: u8) {
        if depth > 5 {
            return;
        }
        let Some((start, end)) = self.glyph_range(gid) else {
            return;
        };
        if start >= end || end > self.data.len() {
            return;
        }
        let d = self.data;
        let Ok(contours) = be_i16(d, start) else {
            return;
        };
        if contours >= 0 {
            self.simple_outline(start, contours as usize, out);
        } else {
            self.composite_outline(start, out, depth);
        }
    }

    fn simple_outline(&self, start: usize, contours: usize, out: &mut Vec<Vec<Pt>>) {
        let d = self.data;
        if contours == 0 || contours > 5000 {
            return;
        }
        let mut end_pts = Vec::with_capacity(contours);
        for i in 0..contours {
            match be_u16(d, start + 10 + i * 2) {
                Ok(v) => end_pts.push(v as usize),
                Err(_) => return,
            }
        }
        let n_points = match end_pts.last() {
            Some(v) => v + 1,
            None => return,
        };
        let ins_len = match be_u16(d, start + 10 + contours * 2) {
            Ok(v) => v as usize,
            Err(_) => return,
        };
        let mut p = start + 12 + contours * 2 + ins_len;
        if p > d.len() {
            return;
        }
        // Flags (run-length encoded).
        let mut flags = Vec::with_capacity(n_points);
        while flags.len() < n_points && p < d.len() {
            let f = d[p];
            p += 1;
            flags.push(f);
            if f & 0x08 != 0 {
                if p >= d.len() {
                    return;
                }
                let repeat = d[p];
                p += 1;
                for _ in 0..repeat {
                    if flags.len() >= n_points {
                        break;
                    }
                    flags.push(f);
                }
            }
        }
        if flags.len() < n_points {
            return;
        }
        // X coordinates.
        let mut xs = Vec::with_capacity(n_points);
        let mut x = 0i32;
        for f in &flags {
            let short = f & 0x02 != 0;
            let same = f & 0x10 != 0;
            if short {
                if p >= d.len() {
                    return;
                }
                let v = d[p] as i32;
                p += 1;
                x += if same { v } else { -v };
            } else if !same {
                match be_i16(d, p) {
                    Ok(v) => x += v as i32,
                    Err(_) => return,
                }
                p += 2;
            }
            xs.push(x);
        }
        // Y coordinates.
        let mut ys = Vec::with_capacity(n_points);
        let mut y = 0i32;
        for f in &flags {
            let short = f & 0x04 != 0;
            let same = f & 0x20 != 0;
            if short {
                if p >= d.len() {
                    return;
                }
                let v = d[p] as i32;
                p += 1;
                y += if same { v } else { -v };
            } else if !same {
                match be_i16(d, p) {
                    Ok(v) => y += v as i32,
                    Err(_) => return,
                }
                p += 2;
            }
            ys.push(y);
        }
        let mut first = 0usize;
        for &last in &end_pts {
            if last < first || last >= n_points {
                first = last + 1;
                continue;
            }
            let mut contour = Vec::with_capacity(last - first + 1);
            for i in first..=last {
                contour.push(Pt {
                    x: xs[i] as f32,
                    y: ys[i] as f32,
                    on: flags[i] & 0x01 != 0,
                });
            }
            out.push(contour);
            first = last + 1;
        }
    }

    fn composite_outline(&self, start: usize, out: &mut Vec<Vec<Pt>>, depth: u8) {
        let d = self.data;
        let mut p = start + 10;
        loop {
            let Ok(flags) = be_u16(d, p) else { return };
            let Ok(comp_gid) = be_u16(d, p + 2) else {
                return;
            };
            p += 4;
            let (dx, dy) = if flags & 0x0001 != 0 {
                let (Ok(a), Ok(b)) = (be_i16(d, p), be_i16(d, p + 2)) else {
                    return;
                };
                p += 4;
                (a as f32, b as f32)
            } else {
                if p + 2 > d.len() {
                    return;
                }
                let a = d[p] as i8 as f32;
                let b = d[p + 1] as i8 as f32;
                p += 2;
                (a, b)
            };
            let (mut a, mut b, mut c, mut dd) = (1.0f32, 0.0f32, 0.0f32, 1.0f32);
            if flags & 0x0008 != 0 {
                let Ok(s) = be_i16(d, p) else { return };
                let s = f2dot14(s);
                a = s;
                dd = s;
                p += 2;
            } else if flags & 0x0040 != 0 {
                let (Ok(sx), Ok(sy)) = (be_i16(d, p), be_i16(d, p + 2)) else {
                    return;
                };
                a = f2dot14(sx);
                dd = f2dot14(sy);
                p += 4;
            } else if flags & 0x0080 != 0 {
                let (Ok(s0), Ok(s1), Ok(s2), Ok(s3)) =
                    (be_i16(d, p), be_i16(d, p + 2), be_i16(d, p + 4), be_i16(d, p + 6))
                else {
                    return;
                };
                a = f2dot14(s0);
                b = f2dot14(s1);
                c = f2dot14(s2);
                dd = f2dot14(s3);
                p += 8;
            }
            let before = out.len();
            self.outline(comp_gid, out, depth + 1);
            let point_offsets = flags & 0x0002 == 0;
            if !point_offsets {
                for contour in &mut out[before..] {
                    for pt in contour.iter_mut() {
                        let x = a * pt.x + c * pt.y + dx;
                        let y = b * pt.x + dd * pt.y + dy;
                        pt.x = x;
                        pt.y = y;
                    }
                }
            }
            if flags & 0x0020 == 0 {
                break;
            }
        }
    }
}

/// A rasterized glyph: 8-bit coverage plus metrics in pixels.
pub struct Glyph {
    pub width: u32,
    pub height: u32,
    /// Offset from the pen position to the left edge of the bitmap.
    pub left: i32,
    /// Offset from the baseline to the top edge of the bitmap.
    pub top: i32,
    pub advance: f32,
    pub coverage: Vec<u8>,
}

impl Glyph {
    fn blank(advance: f32) -> Glyph {
        Glyph {
            width: 0,
            height: 0,
            left: 0,
            top: 0,
            advance,
            coverage: Vec::new(),
        }
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
struct GlyphKey {
    gid: u16,
    /// Font size in quarter pixels, to keep the cache small yet precise.
    size_q: u16,
    bold: bool,
}

/// A font instance with a glyph cache.
pub struct Font {
    face: Face,
    cache: RefCell<HashMap<GlyphKey, std::rc::Rc<Glyph>>>,
}

impl Font {
    pub fn new(data: &'static [u8]) -> Result<Font, FontError> {
        Ok(Font {
            face: Face::parse(data)?,
            cache: RefCell::new(HashMap::new()),
        })
    }

    /// The embedded UI font. Parsing is cheap; callers cache the instance.
    pub fn ui() -> Font {
        Font::new(UI_FONT_DATA).expect("embedded UI font is valid")
    }

    pub fn face(&self) -> &Face {
        &self.face
    }

    /// Distance from the baseline to the top of the ascent, in pixels.
    pub fn ascent(&self, px: f32) -> f32 {
        self.face.ascender / self.face.units_per_em * px
    }

    /// Distance from the baseline to the bottom of the descent (positive).
    pub fn descent(&self, px: f32) -> f32 {
        -self.face.descender / self.face.units_per_em * px
    }

    /// Recommended line height for `px` including the font's line gap.
    pub fn line_height(&self, px: f32) -> f32 {
        (self.face.ascender - self.face.descender + self.face.line_gap) / self.face.units_per_em * px
    }

    /// Width of `text` in pixels at `px` size.
    pub fn text_width(&self, text: &str, px: f32) -> f32 {
        let scale = px / self.face.units_per_em;
        let mut w = 0.0;
        for ch in text.chars() {
            w += self.face.advance_units(self.face.glyph_index(ch)) * scale;
        }
        w
    }

    /// Truncates `text` with an ellipsis so that it fits into `max_w`.
    pub fn ellipsize<'a>(&self, text: &'a str, px: f32, max_w: f32) -> std::borrow::Cow<'a, str> {
        if self.text_width(text, px) <= max_w {
            return std::borrow::Cow::Borrowed(text);
        }
        let ellipsis = "…";
        let ell_w = self.text_width(ellipsis, px);
        let mut acc = 0.0;
        let mut cut = 0;
        for (idx, ch) in text.char_indices() {
            let adv = self.face.advance_units(self.face.glyph_index(ch)) * px / self.face.units_per_em;
            if acc + adv + ell_w > max_w {
                cut = idx;
                break;
            }
            acc += adv;
            cut = idx + ch.len_utf8();
        }
        let mut out = String::with_capacity(cut + ellipsis.len());
        out.push_str(&text[..cut]);
        out.push_str(ellipsis);
        std::borrow::Cow::Owned(out)
    }

    /// Returns (and caches) the rasterized glyph for `ch`.
    pub fn glyph(&self, ch: char, px: f32, bold: bool) -> std::rc::Rc<Glyph> {
        let gid = self.face.glyph_index(ch);
        let size_q = (px * 4.0).round().clamp(1.0, 4096.0) as u16;
        let key = GlyphKey {
            gid,
            size_q,
            bold,
        };
        if let Some(g) = self.cache.borrow().get(&key) {
            return g.clone();
        }
        let size = size_q as f32 / 4.0;
        let glyph = std::rc::Rc::new(self.rasterize(gid, size, bold));
        let mut cache = self.cache.borrow_mut();
        if cache.len() > 1024 {
            cache.clear();
        }
        cache.insert(key, glyph.clone());
        glyph
    }

    fn rasterize(&self, gid: u16, px: f32, bold: bool) -> Glyph {
        let scale = px / self.face.units_per_em;
        let advance = self.face.advance_units(gid) * scale + if bold { 0.35 } else { 0.0 };
        let mut contours: Vec<Vec<Pt>> = Vec::new();
        self.face.outline(gid, &mut contours, 0);
        if contours.is_empty() {
            return Glyph::blank(advance);
        }
        // Flatten quadratic splines into polylines (in pixel space, y down).
        let mut edges: Vec<(f32, f32, f32, f32)> = Vec::new();
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for contour in &contours {
            let poly = flatten_contour(contour, scale);
            if poly.len() < 2 {
                continue;
            }
            for i in 0..poly.len() {
                let (x0, y0) = poly[i];
                let (x1, y1) = poly[(i + 1) % poly.len()];
                min_x = min_x.min(x0).min(x1);
                max_x = max_x.max(x0).max(x1);
                min_y = min_y.min(y0).min(y1);
                max_y = max_y.max(y0).max(y1);
                if (y0 - y1).abs() > 1e-6 {
                    edges.push((x0, y0, x1, y1));
                }
            }
        }
        if edges.is_empty() {
            return Glyph::blank(advance);
        }
        let pad = 1.0;
        let bx = (min_x - pad).floor();
        let by = (min_y - pad).floor();
        let width = ((max_x + pad).ceil() - bx).max(1.0) as u32;
        let height = ((max_y + pad).ceil() - by).max(1.0) as u32;
        // Guard against pathological outlines.
        if width > 512 || height > 512 {
            return Glyph::blank(advance);
        }
        let mut cov = vec![0f32; (width * height) as usize];
        rasterize_edges(&edges, bx, by, width, height, &mut cov);
        if bold {
            let mut shifted = vec![0f32; cov.len()];
            let dx = 0.45f32;
            let shifted_edges: Vec<(f32, f32, f32, f32)> = edges
                .iter()
                .map(|&(x0, y0, x1, y1)| (x0 + dx, y0, x1 + dx, y1))
                .collect();
            rasterize_edges(&shifted_edges, bx, by, width, height, &mut shifted);
            for (a, b) in cov.iter_mut().zip(shifted.iter()) {
                *a = a.max(*b);
            }
        }
        let coverage = cov
            .into_iter()
            .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        Glyph {
            width,
            height,
            left: bx.round() as i32,
            top: by.round() as i32,
            advance,
            coverage,
        }
    }

    /// Draws `text` with the pen at (`x`, `baseline`).
    pub fn draw_text(
        &self,
        canvas: &mut Bitmap,
        x: f32,
        baseline: f32,
        px: f32,
        color: Color,
        text: &str,
    ) -> f32 {
        let mut pen = x.round();
        let base = baseline.round();
        let bold = false;
        for ch in text.chars() {
            let g = self.glyph(ch, px, bold);
            if g.width > 0 {
                let gx = pen as i32 + g.left;
                let gy = base as i32 + g.top;
                blit_glyph(canvas, &g, gx, gy, color);
            }
            pen += g.advance.round().max(0.0);
        }
        pen - x.round()
    }

    /// Draws `text` horizontally centred inside the rectangle `area`.
    pub fn draw_text_centered(
        &self,
        canvas: &mut Bitmap,
        area: crate::geom::Rect,
        px: f32,
        color: Color,
        text: &str,
    ) {
        let w = self.text_width(text, px);
        let x = area.x + (area.w - w) * 0.5;
        let baseline = area.y + (area.h + self.ascent(px) - self.descent(px)) * 0.5;
        self.draw_text(canvas, x, baseline, px, color, text);
    }
}

fn blit_glyph(canvas: &mut Bitmap, g: &Glyph, gx: i32, gy: i32, color: Color) {
    if gx + g.width as i32 <= 0 || gy + g.height as i32 <= 0 {
        return;
    }
    let x0 = gx.max(0) as u32;
    let y0 = gy.max(0) as u32;
    let x1 = (gx + g.width as i32).min(canvas.width as i32).max(0) as u32;
    let y1 = (gy + g.height as i32).min(canvas.height as i32).max(0) as u32;
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    for y in y0..y1 {
        let sy = (y as i32 - gy) as u32;
        for x in x0..x1 {
            let sx = (x as i32 - gx) as u32;
            let cov = g.coverage[(sy * g.width + sx) as usize];
            if cov > 0 {
                canvas.blend_coverage(x, y, color, cov as f32 / 255.0);
            }
        }
    }
}

/// Converts a TrueType contour (with on/off curve points) to a polyline in
/// pixel space with y pointing down.
fn flatten_contour(contour: &[Pt], scale: f32) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(contour.len() * 2);
    let n = contour.len();
    // Find a starting on-curve point (or synthesize one).
    let start_idx = contour.iter().position(|p| p.on).unwrap_or(0);
    let mut start = contour[start_idx];
    if !start.on {
        // Two consecutive off-curve points: midpoint is on the curve.
        let next = contour[(start_idx + 1) % n];
        start = Pt {
            x: (start.x + next.x) * 0.5,
            y: (start.y + next.y) * 0.5,
            on: true,
        };
    }
    let to_px = |p: &Pt| (p.x * scale, -p.y * scale);
    let s = to_px(&start);
    out.push(s);
    let mut cur = start;
    let mut i = 1;
    while i <= n {
        let p = contour[(start_idx + i) % n];
        if p.on {
            out.push(to_px(&p));
            cur = p;
            i += 1;
        } else {
            let next = contour[(start_idx + i + 1) % n];
            let end = if next.on {
                next
            } else {
                Pt {
                    x: (p.x + next.x) * 0.5,
                    y: (p.y + next.y) * 0.5,
                    on: true,
                }
            };
            let p0 = to_px(&cur);
            let p1 = to_px(&p);
            let p2 = to_px(&end);
            let approx = ((p1.0 - p0.0).powi(2) + (p1.1 - p0.1).powi(2)).sqrt()
                + ((p2.0 - p1.0).powi(2) + (p2.1 - p1.1).powi(2)).sqrt();
            let steps = (approx / 0.35).clamp(1.0, 32.0) as usize;
            for k in 1..=steps {
                let t = k as f32 / steps as f32;
                let mt = 1.0 - t;
                let x = mt * mt * p0.0 + 2.0 * mt * t * p1.0 + t * t * p2.0;
                let y = mt * mt * p0.1 + 2.0 * mt * t * p1.1 + t * t * p2.1;
                out.push((x, y));
            }
            cur = end;
            i += if next.on { 2 } else { 1 };
        }
    }
    // Drop an accidental duplicate of the start point at the end.
    if out.len() > 1 {
        let first = out[0];
        let last = *out.last().unwrap();
        if (first.0 - last.0).abs() < 1e-4 && (first.1 - last.1).abs() < 1e-4 {
            out.pop();
        }
    }
    out
}

/// Scanline rasterizer with 4x vertical supersampling and exact horizontal
/// coverage accumulation (non-zero winding rule).
fn rasterize_edges(
    edges: &[(f32, f32, f32, f32)],
    ox: f32,
    oy: f32,
    width: u32,
    height: u32,
    cov: &mut [f32],
) {
    const SUB: u32 = 4;
    let mut crossings: Vec<(f32, i32)> = Vec::with_capacity(edges.len());
    for row in 0..height {
        let mut row_cov = vec![0f32; width as usize];
        for sub in 0..SUB {
            let sy = oy + row as f32 + (sub as f32 + 0.5) / SUB as f32;
            crossings.clear();
            for &(x0, y0, x1, y1) in edges {
                let (y_top, y_bot, x_top, x_bot) = if y0 < y1 {
                    (y0, y1, x0, x1)
                } else {
                    (y1, y0, x1, x0)
                };
                if sy < y_top || sy >= y_bot {
                    continue;
                }
                let t = (sy - y_top) / (y_bot - y_top);
                let x = x_top + t * (x_bot - x_top);
                crossings.push((x, if y0 < y1 { 1 } else { -1 }));
            }
            if crossings.len() < 2 {
                continue;
            }
            crossings.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut winding = 0;
            let mut span_start = 0.0f32;
            for &(x, dir) in crossings.iter() {
                let was_zero = winding == 0;
                winding += dir;
                if was_zero && winding != 0 {
                    span_start = x;
                } else if !was_zero && winding == 0 {
                    accumulate_span(&mut row_cov, span_start - ox, x - ox, width);
                }
            }
        }
        let base = (row * width) as usize;
        for x in 0..width as usize {
            cov[base + x] = row_cov[x] / SUB as f32;
        }
    }
}

/// Adds fractional horizontal coverage for the span `[xa, xb)` to `row`.
fn accumulate_span(row: &mut [f32], xa: f32, xb: f32, width: u32) {
    let xa = xa.max(0.0);
    let xb = xb.min(width as f32);
    if xb <= xa {
        return;
    }
    let i0 = xa.floor() as usize;
    let i1 = xb.ceil() as usize;
    for i in i0..i1.min(width as usize) {
        let l = (i as f32).max(xa);
        let r = ((i + 1) as f32).min(xb);
        if r > l {
            row[i] += r - l;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Font {
        Font::ui()
    }

    #[test]
    fn parses_embedded_face() {
        let f = font();
        assert!(f.face().units_per_em() > 0.0);
        assert!(f.face().num_glyphs() > 100);
        assert!(f.ascent(16.0) > 10.0);
        assert!(f.line_height(16.0) > f.ascent(16.0));
    }

    #[test]
    fn glyph_lookup_covers_ascii() {
        let f = font();
        for ch in 'A'..='Z' {
            assert_ne!(f.face().glyph_index(ch), 0, "missing glyph for {ch}");
        }
        for ch in 'a'..='z' {
            assert_ne!(f.face().glyph_index(ch), 0, "missing glyph for {ch}");
        }
        assert_ne!(f.face().glyph_index('0'), 0);
        assert_eq!(f.face().glyph_index(' '), f.face().glyph_index(' '));
    }

    #[test]
    fn space_has_advance_but_no_ink() {
        let f = font();
        let g = f.glyph(' ', 14.0, false);
        assert!(g.advance > 2.0);
        assert_eq!(g.width, 0);
    }

    #[test]
    fn letter_has_coverage() {
        let f = font();
        let g = f.glyph('A', 16.0, false);
        assert!(g.width >= 8 && g.height >= 8, "{}x{}", g.width, g.height);
        let inked = g.coverage.iter().filter(|c| **c > 32).count();
        assert!(inked > 20, "only {inked} inked pixels");
        // The bitmap should not be fully saturated anywhere (AA text).
        assert!(g.coverage.iter().any(|c| *c > 8 && *c < 240));
    }

    #[test]
    fn text_width_grows_with_size_and_length() {
        let f = font();
        let a = f.text_width("Hello", 12.0);
        let b = f.text_width("Hello", 24.0);
        let c = f.text_width("Hello world", 12.0);
        assert!((b - 2.0 * a).abs() < 1.0);
        assert!(c > a);
    }

    #[test]
    fn ellipsize_shortens_long_text() {
        let f = font();
        let out = f.ellipsize("a very long document title indeed", 12.0, 60.0);
        assert!(out.ends_with('…'));
        assert!(f.text_width(&out, 12.0) <= 62.0);
    }

    #[test]
    fn drawing_marks_pixels() {
        let mut bmp = Bitmap::filled(120, 40, Color::rgb(255, 255, 255));
        let f = font();
        f.draw_text(&mut bmp, 4.0, 26.0, 18.0, Color::rgb(0, 0, 0), "Wg");
        let dark = bmp.px.iter().filter(|p| (**p & 0xFF) < 200).count();
        assert!(dark > 30, "no text pixels drawn ({dark})");
    }

    #[test]
    fn bogus_font_data_is_rejected() {
        let data: &'static [u8] = Box::leak(vec![0u8; 64].into_boxed_slice());
        assert!(Face::parse(data).is_err());
    }
}
