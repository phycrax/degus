/// A screen position in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}

impl Point {
    pub const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }

    pub(crate) fn write(self, record: &mut [u8], at: usize) {
        put_u16(record, at, self.x);
        put_u16(record, at + 2, self.y);
    }

    pub(crate) fn read(record: &[u8], at: usize) -> Self {
        Self::new(get_u16(record, at), get_u16(record, at + 2))
    }
}

/// A screen rectangle in pixels. Like the DGUS editor, `bottom_right` is
/// `top_left` plus the size, so it lies just outside the rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Area {
    pub top_left: Point,
    pub bottom_right: Point,
}

impl Area {
    pub const fn new(left: u16, top: u16, right: u16, bottom: u16) -> Self {
        Self {
            top_left: Point::new(left, top),
            bottom_right: Point::new(right, bottom),
        }
    }

    /// The area of `w` × `h` pixels whose top left corner is `(x, y)`.
    pub const fn sized(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self::new(x, y, x.saturating_add(w), y.saturating_add(h))
    }

    /// The width in pixels, or 0 when `bottom_right` is left of `top_left`.
    pub const fn width(self) -> u16 {
        self.bottom_right.x.saturating_sub(self.top_left.x)
    }

    /// The height in pixels, or 0 when `bottom_right` is above `top_left`.
    pub const fn height(self) -> u16 {
        self.bottom_right.y.saturating_sub(self.top_left.y)
    }

    pub(crate) fn write(self, record: &mut [u8], at: usize) {
        self.top_left.write(record, at);
        self.bottom_right.write(record, at + 4);
    }

    pub(crate) fn read(record: &[u8], at: usize) -> Self {
        Self {
            top_left: Point::read(record, at),
            bottom_right: Point::read(record, at + 4),
        }
    }
}

pub(crate) fn put_u16(record: &mut [u8], at: usize, value: u16) {
    record[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

pub(crate) fn put_i32(record: &mut [u8], at: usize, value: i32) {
    record[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

pub(crate) fn get_u16(record: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([record[at], record[at + 1]])
}

pub(crate) fn get_i32(record: &[u8], at: usize) -> i32 {
    i32::from_be_bytes([record[at], record[at + 1], record[at + 2], record[at + 3]])
}
