//! Minimal 2D geometry primitives used by the UI and the layout code.
//!
//! Coordinates are in logical pixels, origin top-left, y growing downwards —
//! the same convention as the framebuffer and (after a single flip) as PDFium.

/// A point in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }

    pub fn round(self) -> (i32, i32) {
        (self.x.round() as i32, self.y.round() as i32)
    }
}

/// Width/height pair in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const ZERO: Size = Size { w: 0.0, h: 0.0 };

    pub fn new(w: f32, h: f32) -> Self {
        Size { w, h }
    }

    pub fn is_empty(self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }
}

/// An axis-aligned rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn from_size(size: Size) -> Self {
        Rect::new(0.0, 0.0, size.w, size.h)
    }

    pub fn right(self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    pub fn center(self) -> Point {
        Point::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn size(self) -> Size {
        Size::new(self.w, self.h)
    }

    pub fn is_empty(self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    pub fn inset(self, d: f32) -> Rect {
        self.inset_xy(d, d)
    }

    pub fn inset_xy(self, dx: f32, dy: f32) -> Rect {
        Rect::new(
            self.x + dx,
            self.y + dy,
            (self.w - 2.0 * dx).max(0.0),
            (self.h - 2.0 * dy).max(0.0),
        )
    }

    pub fn intersect(self, other: Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        Rect::new(x, y, (r - x).max(0.0), (b - y).max(0.0))
    }

    pub fn translate(self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// Splits off a horizontal band of `h` logical pixels from the top.
    pub fn split_top(&mut self, h: f32) -> Rect {
        let h = h.min(self.h);
        let out = Rect::new(self.x, self.y, self.w, h);
        self.y += h;
        self.h -= h;
        out
    }

    /// Splits off a horizontal band of `h` logical pixels from the bottom.
    pub fn split_bottom(&mut self, h: f32) -> Rect {
        let h = h.min(self.h);
        let out = Rect::new(self.x, self.bottom() - h, self.w, h);
        self.h -= h;
        out
    }

    /// Splits off a vertical band of `w` logical pixels from the left.
    pub fn split_left(&mut self, w: f32) -> Rect {
        let w = w.min(self.w);
        let out = Rect::new(self.x, self.y, w, self.h);
        self.x += w;
        self.w -= w;
        out
    }

    /// Splits off a vertical band of `w` logical pixels from the right.
    pub fn split_right(&mut self, w: f32) -> Rect {
        let w = w.min(self.w);
        let out = Rect::new(self.right() - w, self.y, w, self.h);
        self.w -= w;
        out
    }
}

/// Clamps a value into `[lo, hi]`.
pub fn clamp<T: PartialOrd>(v: T, lo: T, hi: T) -> T {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Smooth-step easing used for the (very short) scroll animations.
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_helpers_partition_the_rect() {
        let mut r = Rect::new(0.0, 0.0, 100.0, 80.0);
        let top = r.split_top(20.0);
        let left = r.split_left(30.0);
        let bottom = r.split_bottom(10.0);
        assert_eq!(top, Rect::new(0.0, 0.0, 100.0, 20.0));
        assert_eq!(left, Rect::new(0.0, 20.0, 30.0, 60.0));
        assert_eq!(bottom, Rect::new(30.0, 70.0, 70.0, 10.0));
        assert!(r.h > 0.0 && r.w > 0.0);
    }

    #[test]
    fn intersection_and_containment() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.intersect(b), Rect::new(5.0, 5.0, 5.0, 5.0));
        assert!(a.contains(Point::new(9.9, 0.0)));
        assert!(!a.contains(Point::new(10.0, 0.0)));
    }
}
