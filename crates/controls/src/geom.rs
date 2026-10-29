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
}

/// A screen rectangle in pixels. Like the DGUS editor, `right` and `bottom`
/// are `left + width` and `top + height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Area {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}

impl Area {
    pub const fn new(left: u16, top: u16, right: u16, bottom: u16) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub const fn top_left(self) -> Point {
        Point::new(self.left, self.top)
    }

    pub(crate) fn write(self, record: &mut [u8], at: usize) {
        Point::new(self.left, self.top).write(record, at);
        Point::new(self.right, self.bottom).write(record, at + 4);
    }
}

pub(crate) fn put_u16(record: &mut [u8], at: usize, value: u16) {
    record[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

pub(crate) fn put_i32(record: &mut [u8], at: usize, value: i32) {
    record[at..at + 4].copy_from_slice(&value.to_be_bytes());
}
