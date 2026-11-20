//! A tiny software rasterizer: 32-bit ARGB canvas plus the handful of drawing
//! primitives the UI needs (rectangles, rounded rectangles, lines, circles and
//! scaled bitmap blits).
//!
//! Every platform shell blits this buffer verbatim (Windows GDI `StretchDIBits`
//! and X11 `XPutImage` both accept the exact same `0xAARRGGBB` little-endian
//! layout), so the UI looks pixel-identical on Windows and Linux.

use crate::geom::{clamp, Rect};

/// Packed `0xAARRGGBB` colour. The alpha channel is used while compositing but
/// is ignored by the platform blit (it is the padding byte in X11 and GDI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const TRANSPARENT: Color = Color(0x0000_0000);

    /// Opaque colour from RGB components.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color(0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    pub const fn hex(v: u32) -> Color {
        Color(0xFF00_0000 | (v & 0x00FF_FFFF))
    }

    /// Colour from a packed `0xAARRGGBB` value (used by persisted highlights).
    pub const fn from_argb(v: u32) -> Color {
        Color(v)
    }

    /// Packed `0xAARRGGBB` value.
    pub const fn argb(self) -> u32 {
        self.0
    }

    pub fn a(self) -> u8 {
        (self.0 >> 24) as u8
    }

    pub fn r(self) -> u8 {
        (self.0 >> 16) as u8
    }

    pub fn g(self) -> u8 {
        (self.0 >> 8) as u8
    }

    pub fn b(self) -> u8 {
        self.0 as u8
    }

    /// Returns the colour with a replaced alpha channel.
    pub fn with_alpha(self, a: u8) -> Color {
        Color((self.0 & 0x00FF_FFFF) | ((a as u32) << 24))
    }

    /// Multiplies the existing alpha by `factor` (0.0..=1.0).
    pub fn fade(self, factor: f32) -> Color {
        let a = (self.a() as f32 * clamp(factor, 0.0, 1.0)).round() as u8;
        self.with_alpha(a)
    }

    pub fn blend_over(self, dst: Color) -> Color {
        let sa = self.a() as u32;
        if sa == 255 {
            return self;
        }
        if sa == 0 {
            return dst;
        }
        let da = dst.a() as u32;
        let inv = 255 - sa;
        let out_a = sa + da * inv / 255;
        if out_a == 0 {
            return Color::TRANSPARENT;
        }
        let mix = |s: u8, d: u8| {
            ((s as u32 * sa * 255 + d as u32 * da * inv) / (out_a * 255)).min(255) as u8
        };
        Color::rgba(
            mix(self.r(), dst.r()),
            mix(self.g(), dst.g()),
            mix(self.b(), dst.b()),
            out_a as u8,
        )
    }
}

/// A 32-bit ARGB pixel buffer.
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// Row-major pixels, `0xAARRGGBB`.
    pub px: Vec<u32>,
}

impl Bitmap {
    pub fn new(width: u32, height: u32) -> Self {
        let mut bmp = Bitmap {
            width,
            height,
            px: Vec::new(),
        };
        bmp.resize(width, height);
        bmp
    }

    pub fn filled(width: u32, height: u32, color: Color) -> Self {
        let mut bmp = Bitmap::new(width, height);
        bmp.clear(color);
        bmp
    }

    /// Resizes the buffer (contents undefined).
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.px
            .resize((self.width as usize) * (self.height as usize), 0);
    }

    pub fn clear(&mut self, color: Color) {
        self.px.fill(color.0);
    }

    #[inline]
    pub fn index(&self, x: u32, y: u32) -> usize {
        (y as usize) * (self.width as usize) + x as usize
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> Color {
        if x >= self.width || y >= self.height {
            return Color::TRANSPARENT;
        }
        Color(self.px[self.index(x, y)])
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, c: Color) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = self.index(x, y);
        self.px[i] = c.0;
    }

    /// Source-over blends `c` onto the pixel at `(x, y)`.
    #[inline]
    pub fn blend(&mut self, x: u32, y: u32, c: Color) {
        if x >= self.width || y >= self.height || c.a() == 0 {
            return;
        }
        let i = self.index(x, y);
        if c.a() == 255 {
            self.px[i] = c.0;
        } else {
            self.px[i] = c.blend_over(Color(self.px[i])).0;
        }
    }

    /// Adds `coverage` (0..=1) of `c` to the pixel, used by text and AA shapes.
    #[inline]
    pub fn blend_coverage(&mut self, x: u32, y: u32, c: Color, coverage: f32) {
        if x >= self.width || y >= self.height || coverage <= 0.0 {
            return;
        }
        let a = (c.a() as f32 * coverage.clamp(0.0, 1.0)).round() as u8;
        if a == 0 {
            return;
        }
        self.blend(x, y, c.with_alpha(a));
    }

    pub fn fill_rect(&mut self, r: Rect, c: Color) {
        if c.a() == 0 {
            return;
        }
        let x0 = r.x.floor().max(0.0) as u32;
        let y0 = r.y.floor().max(0.0) as u32;
        let x1 = (r.right().ceil().max(0.0) as u32).min(self.width);
        let y1 = (r.bottom().ceil().max(0.0) as u32).min(self.height);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        if c.a() == 255 {
            for y in y0..y1 {
                let base = (y as usize) * (self.width as usize);
                self.px[base + x0 as usize..base + x1 as usize].fill(c.0);
            }
        } else {
            for y in y0..y1 {
                let base = (y as usize) * (self.width as usize);
                for x in x0..x1 {
                    let i = base + x as usize;
                    self.px[i] = c.blend_over(Color(self.px[i])).0;
                }
            }
        }
    }

    pub fn hline(&mut self, x0: f32, x1: f32, y: f32, c: Color) {
        self.fill_rect(Rect::new(x0.min(x1), y, (x1 - x0).abs().max(1.0), 1.0), c);
    }

    pub fn vline(&mut self, x: f32, y0: f32, y1: f32, c: Color) {
        self.fill_rect(Rect::new(x, y0.min(y1), 1.0, (y1 - y0).abs().max(1.0)), c);
    }

    /// Filled rectangle with anti-aliased rounded corners.
    pub fn fill_round_rect(&mut self, r: Rect, radius: f32, c: Color) {
        if c.a() == 0 || r.is_empty() {
            return;
        }
        let radius = radius.min(r.w * 0.5).min(r.h * 0.5).max(0.0);
        if radius < 0.5 {
            self.fill_rect(r, c);
            return;
        }
        let body = Rect::new(r.x + radius, r.y, r.w - 2.0 * radius, r.h);
        self.fill_rect(body, c);
        self.fill_rect(Rect::new(r.x, r.y + radius, radius, r.h - 2.0 * radius), c);
        self.fill_rect(Rect::new(
            r.right() - radius,
            r.y + radius,
            radius,
            r.h - 2.0 * radius,
        ), c);
        // Corner quarter discs with analytic coverage.
        let corners = [
            (r.x + radius, r.y + radius, -1.0f32, -1.0f32),
            (r.right() - radius, r.y + radius, 1.0, -1.0),
            (r.x + radius, r.bottom() - radius, -1.0, 1.0),
            (r.right() - radius, r.bottom() - radius, 1.0, 1.0),
        ];
        for (cx, cy, sx, sy) in corners {
            let x0 = (cx - radius).floor().max(0.0) as u32;
            let x1 = ((cx + radius).ceil() as u32).min(self.width);
            let y0 = (cy - radius).floor().max(0.0) as u32;
            let y1 = ((cy + radius).ceil() as u32).min(self.height);
            for y in y0..y1 {
                for x in x0..x1 {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    // Only the quadrant facing the corner needs coverage work.
                    if (px - cx) * sx < 0.0 || (py - cy) * sy < 0.0 {
                        continue;
                    }
                    let dx = px - cx;
                    let dy = py - cy;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let cov = clamp(radius - dist + 0.5, 0.0, 1.0);
                    self.blend_coverage(x, y, c, cov);
                }
            }
        }
    }

    /// Anti-aliased outline of a rounded rectangle (focus rings, cards, popups).
    ///
    /// Only the border band is visited: stroking a full-page rectangle used to
    /// cost a signed-distance evaluation per pixel of the whole page.
    pub fn stroke_round_rect(&mut self, r: Rect, radius: f32, c: Color, thickness: f32) {
        if r.is_empty() || c.a() == 0 {
            return;
        }
        let t = thickness.max(0.5);
        if radius <= 1.5 {
            // Plain rectangle: four thin fills, no distance field needed.
            self.fill_rect(Rect::new(r.x, r.y, r.w, t), c);
            self.fill_rect(Rect::new(r.x, r.bottom() - t, r.w, t), c);
            self.fill_rect(Rect::new(r.x, r.y, t, r.h), c);
            self.fill_rect(Rect::new(r.right() - t, r.y, t, r.h), c);
            return;
        }
        let radius = radius.min(r.w * 0.5).min(r.h * 0.5).max(0.0);
        let x0 = r.x.floor().max(0.0) as u32;
        let y0 = r.y.floor().max(0.0) as u32;
        let x1 = (r.right().ceil().max(0.0) as u32).min(self.width);
        let y1 = (r.bottom().ceil().max(0.0) as u32).min(self.height);
        let band = (t.ceil() as u32 + 1).max(2);
        let outer = radius.max(t) + 1.0;
        let x_start = x0.max((r.x + outer).min(x1 as f32) as u32);
        let x_end = (x1).min((r.right() - outer).max(x0 as f32) as u32);
        for y in y0..y1 {
            let in_vertical_band = y < y0 + band || y + band >= y1;
            let mut span = |bmp: &mut Self, from: u32, to: u32| {
                for x in from..to {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let d_out = sd_round_box(
                        px - r.x - r.w * 0.5,
                        py - r.y - r.h * 0.5,
                        r.w * 0.5,
                        r.h * 0.5,
                        radius,
                    );
                    let d_in = sd_round_box(
                        px - r.x - r.w * 0.5,
                        py - r.y - r.h * 0.5,
                        r.w * 0.5 - t,
                        r.h * 0.5 - t,
                        (radius - t).max(0.0),
                    );
                    let cov = clamp(0.5 - d_out, 0.0, 1.0) - clamp(0.5 - d_in, 0.0, 1.0);
                    bmp.blend_coverage(x, y, c, cov);
                }
            };
            if in_vertical_band {
                span(self, x0, x1);
            } else {
                span(self, x0, x_start.min(x1));
                span(self, x_end.max(x0), x1);
            }
        }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, c: Color) {
        let x0 = (cx - radius).floor().max(0.0) as u32;
        let x1 = ((cx + radius).ceil() as u32).min(self.width);
        let y0 = (cy - radius).floor().max(0.0) as u32;
        let y1 = ((cy + radius).ceil() as u32).min(self.height);
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                let cov = clamp(radius - dist + 0.5, 0.0, 1.0);
                self.blend_coverage(x, y, c, cov);
            }
        }
    }

    pub fn stroke_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, w: f32, c: Color) {
        let w = w.max(1.0);
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.001 {
            self.fill_circle(x0, y0, w * 0.5, c);
            return;
        }
        let steps = (len.ceil() as u32).max(1);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            self.fill_circle(x0 + dx * t, y0 + dy * t, w * 0.5, c);
        }
    }

    /// Blits `src` into the destination rectangle with bilinear filtering.
    /// A 1:1 blit takes a fast copying path.
    pub fn blit_scaled(&mut self, src: &Bitmap, dst: Rect, opacity: f32) {
        if src.width == 0 || src.height == 0 || dst.is_empty() {
            return;
        }
        let opacity = opacity.clamp(0.0, 1.0);
        if opacity <= 0.0 {
            return;
        }
        let dx0 = dst.x.floor().max(0.0) as u32;
        let dy0 = dst.y.floor().max(0.0) as u32;
        let dx1 = (dst.right().ceil().max(0.0) as u32).min(self.width);
        let dy1 = (dst.bottom().ceil().max(0.0) as u32).min(self.height);
        if dx0 >= dx1 || dy0 >= dy1 {
            return;
        }
        let sx = src.width as f32 / dst.w.max(0.0001);
        let sy = src.height as f32 / dst.h.max(0.0001);
        // Opaque fast path: nearest-neighbour row copies. Page bitmaps are
        // rendered within a couple of pixels of the size they are drawn at, so
        // this is the hot path while scrolling and zooming.
        if opacity >= 0.999
            && (src.width as f32 - dst.w).abs() <= 2.0
            && (src.height as f32 - dst.h).abs() <= 2.0
        {
            let off_x = dst.x.round() as i64;
            let off_y = dst.y.round() as i64;
            let x_from = (dx0 as i64).max(off_x).max(0) as u32;
            let x_to = (dx1 as i64).min(off_x + src.width as i64) as u32;
            if x_from >= x_to {
                return;
            }
            let sxi0 = (x_from as i64 - off_x) as usize;
            let run = (x_to - x_from) as usize;
            for y in dy0..dy1 {
                let syi = y as i64 - off_y;
                if syi < 0 || syi >= src.height as i64 {
                    continue;
                }
                let sbase = syi as usize * src.width as usize + sxi0;
                let dbase = y as usize * self.width as usize + x_from as usize;
                let src_row = &src.px[sbase..sbase + run];
                let dst_row = &mut self.px[dbase..dbase + run];
                if src_row.iter().all(|p| (p >> 24) == 255) {
                    dst_row.copy_from_slice(src_row);
                } else {
                    for (d, s) in dst_row.iter_mut().zip(src_row) {
                        let c = Color(*s);
                        if c.a() == 255 {
                            *d = c.0;
                        } else if c.a() > 0 {
                            *d = c.blend_over(Color(*d)).0;
                        }
                    }
                }
            }
            return;
        }
        for y in dy0..dy1 {
            let v = ((y as f32 + 0.5 - dst.y) * sy - 0.5).max(0.0);
            let y0 = v.floor();
            let fy = v - y0;
            let y0i = (y0 as u32).min(src.height - 1);
            let y1i = (y0i + 1).min(src.height - 1);
            for x in dx0..dx1 {
                let u = ((x as f32 + 0.5 - dst.x) * sx - 0.5).max(0.0);
                let x0 = u.floor();
                let fx = u - x0;
                let x0i = (x0 as u32).min(src.width - 1);
                let x1i = (x0i + 1).min(src.width - 1);
                let p00 = src.px[(y0i as usize) * (src.width as usize) + x0i as usize];
                let p10 = src.px[(y0i as usize) * (src.width as usize) + x1i as usize];
                let p01 = src.px[(y1i as usize) * (src.width as usize) + x0i as usize];
                let p11 = src.px[(y1i as usize) * (src.width as usize) + x1i as usize];
                let mut acc = [0f32; 4];
                for (c, w) in [(p00, (1.0 - fx) * (1.0 - fy)), (p10, fx * (1.0 - fy)), (p01, (1.0 - fx) * fy), (p11, fx * fy)] {
                    let col = Color(c);
                    acc[0] += col.a() as f32 * w;
                    acc[1] += col.r() as f32 * w;
                    acc[2] += col.g() as f32 * w;
                    acc[3] += col.b() as f32 * w;
                }
                let a = acc[0];
                if a < 1.0 {
                    continue;
                }
                let c = Color::rgba(
                    (acc[1] / a * 255.0).min(255.0) as u8,
                    (acc[2] / a * 255.0).min(255.0) as u8,
                    (acc[3] / a * 255.0).min(255.0) as u8,
                    (a * opacity).min(255.0) as u8,
                );
                let i = (y as usize) * (self.width as usize) + x as usize;
                self.px[i] = c.blend_over(Color(self.px[i])).0;
            }
        }
    }

    /// Draws a soft drop shadow around `r` (used behind pages and popups).
    pub fn drop_shadow(&mut self, r: Rect, blur: f32, opacity: f32, color: Color) {
        let steps = 4;
        for i in 0..steps {
            let t = (i as f32 + 1.0) / steps as f32;
            let grow = blur * t;
            let a = opacity * (1.0 - t) / steps as f32 * 2.0;
            let rr = Rect::new(r.x - grow, r.y - grow + blur * 0.25, r.w + grow * 2.0, r.h + grow * 2.0);
            self.fill_round_rect(rr, 6.0 + grow, color.fade(a));
        }
    }

    /// Vertical linear gradient fill (2 colours).
    pub fn fill_rect_gradient(&mut self, r: Rect, top: Color, bottom: Color) {
        if r.is_empty() {
            return;
        }
        let y0 = r.y.floor().max(0.0) as u32;
        let y1 = (r.bottom().ceil().max(0.0) as u32).min(self.height);
        let x0 = r.x.floor().max(0.0) as u32;
        let x1 = (r.right().ceil().max(0.0) as u32).min(self.width);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for y in y0..y1 {
            let t = if r.h <= 0.0 {
                0.0
            } else {
                ((y as f32 + 0.5 - r.y) / r.h).clamp(0.0, 1.0)
            };
            let c = Color::rgba(
                (top.r() as f32 * (1.0 - t) + bottom.r() as f32 * t) as u8,
                (top.g() as f32 * (1.0 - t) + bottom.g() as f32 * t) as u8,
                (top.b() as f32 * (1.0 - t) + bottom.b() as f32 * t) as u8,
                (top.a() as f32 * (1.0 - t) + bottom.a() as f32 * t) as u8,
            );
            let base = (y as usize) * (self.width as usize);
            for x in x0..x1 {
                let i = base + x as usize;
                self.px[i] = c.blend_over(Color(self.px[i])).0;
            }
        }
    }
}

/// Signed distance to a rounded box centred on the origin; negative inside.
#[inline]
fn sd_round_box(px: f32, py: f32, half_w: f32, half_h: f32, radius: f32) -> f32 {
    let qx = px.abs() - (half_w - radius).max(0.0);
    let qy = py.abs() - (half_h - radius).max(0.0);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blending_opaque_replaces() {
        let c = Color::rgb(255, 0, 0).with_alpha(255);
        assert_eq!(c.blend_over(Color::rgb(0, 0, 255)), c);
    }

    #[test]
    fn blending_half_alpha_averages() {
        let over = Color::rgb(0, 0, 0).with_alpha(128).blend_over(Color::rgb(255, 255, 255));
        assert!(over.r() > 120 && over.r() < 136, "got {}", over.r());
    }

    #[test]
    fn fill_rect_clips_to_bounds() {
        let mut b = Bitmap::filled(4, 4, Color::rgb(0, 0, 0));
        b.fill_rect(Rect::new(-10.0, -10.0, 100.0, 100.0), Color::rgb(9, 9, 9));
        assert_eq!(b.get(0, 0), Color::rgb(9, 9, 9));
        assert_eq!(b.get(3, 3), Color::rgb(9, 9, 9));
        // Out-of-range access never panics.
        b.fill_rect(Rect::new(100.0, 100.0, 10.0, 10.0), Color::rgb(1, 2, 3));
    }

    #[test]
    fn scaled_blit_downscales() {
        let src = Bitmap::filled(10, 10, Color::rgb(10, 20, 30));
        let mut dst = Bitmap::filled(20, 20, Color::rgb(0, 0, 0));
        dst.blit_scaled(&src, Rect::new(5.0, 5.0, 5.0, 5.0), 1.0);
        assert_eq!(dst.get(6, 6), Color::rgb(10, 20, 30));
        assert_eq!(dst.get(0, 0), Color::rgb(0, 0, 0));
    }
}
