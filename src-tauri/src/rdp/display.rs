//! Bounded damage tracking and binary display packets. Cursor changes never invalidate pixels.
use ironrdp_graphics::pointer::DecodedPointer;
use ironrdp_pdu::geometry::InclusiveRectangle;
use ironrdp_session::image::DecodedImage;
use std::sync::Arc;

const TILE: usize = 64;
pub const FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_nanos(1_000_000_000 / 60);
pub const MAX_IN_FLIGHT: usize = 2;

pub struct Damage {
    width: u16,
    height: u16,
    columns: usize,
    tiles: Vec<bool>,
    any: bool,
    bounds: Option<InclusiveRectangle>,
}
impl Damage {
    pub fn new(width: u16, height: u16) -> Self {
        let columns = usize::from(width).div_ceil(TILE);
        Self {
            width,
            height,
            columns,
            tiles: vec![false; columns * usize::from(height).div_ceil(TILE)],
            any: false,
            bounds: None,
        }
    }
    pub fn is_empty(&self) -> bool {
        !self.any
    }
    pub fn all(&mut self) {
        self.tiles.fill(true);
        self.any = true;
        self.bounds = Some(InclusiveRectangle {
            left: 0,
            top: 0,
            right: self.width - 1,
            bottom: self.height - 1,
        });
    }
    pub fn mark(&mut self, rect: InclusiveRectangle) {
        if rect.left > rect.right
            || rect.top > rect.bottom
            || rect.left >= self.width
            || rect.top >= self.height
        {
            return;
        }
        let rect = InclusiveRectangle {
            right: rect.right.min(self.width - 1),
            bottom: rect.bottom.min(self.height - 1),
            ..rect
        };
        self.bounds = Some(if let Some(old) = self.bounds.take() {
            InclusiveRectangle {
                left: old.left.min(rect.left),
                top: old.top.min(rect.top),
                right: old.right.max(rect.right),
                bottom: old.bottom.max(rect.bottom),
            }
        } else {
            rect.clone()
        });
        let right = usize::from(rect.right.min(self.width - 1)) / TILE;
        let bottom = usize::from(rect.bottom.min(self.height - 1)) / TILE;
        for y in usize::from(rect.top) / TILE..=bottom {
            for x in usize::from(rect.left) / TILE..=right {
                self.tiles[y * self.columns + x] = true;
            }
        }
        self.any = true;
    }
    /// Merge horizontal runs and matching runs in adjacent rows, without filling distant gaps.
    pub fn take(&mut self) -> Vec<InclusiveRectangle> {
        if self.is_empty() {
            return Vec::new();
        }
        let mut rectangles: Vec<InclusiveRectangle> = Vec::new();
        let mut previous: Vec<Option<usize>> = vec![None; self.columns];
        let mut current = previous.clone();
        for y in 0..usize::from(self.height).div_ceil(TILE) {
            current.fill(None);
            let mut x = 0;
            while x < self.columns {
                if !self.tiles[y * self.columns + x] {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < self.columns && self.tiles[y * self.columns + x] {
                    x += 1;
                }
                let right = (x * TILE).min(usize::from(self.width)) as u16 - 1;
                let bottom = ((y + 1) * TILE).min(usize::from(self.height)) as u16 - 1;
                let index = if let Some(index) =
                    previous[start].filter(|&index| rectangles[index].right == right)
                {
                    rectangles[index].bottom = bottom;
                    index
                } else {
                    rectangles.push(InclusiveRectangle {
                        left: (start * TILE) as u16,
                        top: (y * TILE) as u16,
                        right,
                        bottom,
                    });
                    rectangles.len() - 1
                };
                current[start] = Some(index);
            }
            std::mem::swap(&mut previous, &mut current);
        }
        self.tiles.fill(false);
        self.any = false;
        // Preserve exact small updates instead of expanding a single glyph to an entire tile.
        let bytes = |r: &InclusiveRectangle| {
            8 + usize::from(r.right - r.left + 1) * usize::from(r.bottom - r.top + 1) * 4
        };
        if let Some(bounds) = self.bounds.take() {
            if bytes(&bounds) <= rectangles.iter().map(bytes).sum() {
                return vec![bounds];
            }
        }
        rectangles
    }
}

pub enum Cursor {
    Default,
    Hidden,
    Bitmap(Arc<DecodedPointer>),
}
impl Cursor {
    pub fn bitmap(pointer: Arc<DecodedPointer>) -> Self {
        // Webviews impose a 128 px CSS cursor bound. Keep a responsive local arrow for larger shapes.
        if pointer.width == 0 || pointer.height == 0 {
            return Self::Hidden;
        }
        if pointer.width > 128
            || pointer.height > 128
            || pointer.hotspot_x >= pointer.width
            || pointer.hotspot_y >= pointer.height
            || pointer.bitmap_data.len()
                != usize::from(pointer.width) * usize::from(pointer.height) * 4
        {
            return Self::Default;
        }
        Self::Bitmap(pointer)
    }
    pub fn update(&mut self, next: Self) -> bool {
        let same = match (&self, &next) {
            (Self::Default, Self::Default) | (Self::Hidden, Self::Hidden) => true,
            (Self::Bitmap(a), Self::Bitmap(b)) => {
                Arc::ptr_eq(a, b)
                    || (a.width == b.width
                        && a.height == b.height
                        && a.hotspot_x == b.hotspot_x
                        && a.hotspot_y == b.hotspot_y
                        && a.bitmap_data == b.bitmap_data)
            }
            _ => false,
        };
        if !same {
            *self = next;
        }
        !same
    }
}

pub fn frame(
    image: &DecodedImage,
    rectangles: &[InclusiveRectangle],
    sequence: u32,
    cursor: Option<&Cursor>,
) -> Vec<u8> {
    let (kind, width, height, hot_x, hot_y, pixels): (u16, u16, u16, u16, u16, &[u8]) = match cursor
    {
        None => (0, 0, 0, 0, 0, &[]),
        Some(Cursor::Default) => (1, 0, 0, 0, 0, &[]),
        Some(Cursor::Hidden) => (2, 0, 0, 0, 0, &[]),
        Some(Cursor::Bitmap(p)) => (
            3,
            p.width,
            p.height,
            p.hotspot_x,
            p.hotspot_y,
            &p.bitmap_data,
        ),
    };
    let pixel_bytes: usize = rectangles
        .iter()
        .map(|r| usize::from(r.right - r.left + 1) * usize::from(r.bottom - r.top + 1) * 4)
        .sum();
    let mut bytes = Vec::with_capacity(24 + pixels.len() + rectangles.len() * 8 + pixel_bytes);
    bytes.extend_from_slice(b"NRD2");
    bytes.extend_from_slice(&sequence.to_le_bytes());
    for value in [
        image.width(),
        image.height(),
        rectangles.len() as u16,
        kind,
        width,
        height,
        hot_x,
        hot_y,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(pixels);
    for rect in rectangles {
        let width = rect.right - rect.left + 1;
        for value in [rect.left, rect.top, width, rect.bottom - rect.top + 1] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for y in rect.top..=rect.bottom {
            let start = usize::from(y) * image.stride() + usize::from(rect.left) * 4;
            bytes.extend_from_slice(&image.data()[start..start + usize::from(width) * 4]);
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use ironrdp_graphics::image_processing::PixelFormat;
    #[test]
    fn sparse_damage_does_not_upload_the_gap_between_far_apart_updates() {
        let image = DecodedImage::new(PixelFormat::RgbA32, 1600, 900);
        let mut damage = Damage::new(1600, 900);
        for (x, y) in [(0, 0), (1599, 899)] {
            damage.mark(InclusiveRectangle {
                left: x,
                top: y,
                right: x,
                bottom: y,
            });
        }
        let rectangles = damage.take();
        assert_eq!(rectangles.len(), 2);
        let bytes = frame(&image, &rectangles, 1, None);
        assert!(
            bytes.len() < 40_000,
            "two tiny updates must not copy the whole screen: {} bytes",
            bytes.len()
        );
        eprintln!(
            "RDP sparse damage: {} bytes; previous bounding rectangle: {} bytes",
            bytes.len(),
            24 + 1600 * 900 * 4
        );
        assert!(damage.is_empty());
        assert!(damage.take().is_empty());
        damage.mark(InclusiveRectangle {
            left: 10,
            top: 20,
            right: 25,
            bottom: 35,
        });
        assert_eq!(
            damage.take(),
            vec![InclusiveRectangle {
                left: 10,
                top: 20,
                right: 25,
                bottom: 35
            }],
            "a small patch should keep its exact bounds instead of expanding to a tile"
        );
        damage.all();
        assert_eq!(
            damage.take(),
            vec![InclusiveRectangle {
                left: 0,
                top: 0,
                right: 1599,
                bottom: 899
            }]
        );
    }
    #[test]
    fn damage_is_bounded_clipped_disjoint_and_covers_every_changed_pixel() {
        let (width, height) = (3840, 2160);
        let mut damage = Damage::new(width, height);
        for (x, y) in [
            (0, 0),
            (63, 63),
            (64, 64),
            (3839, 2159),
            (100, 1000),
            (2000, 2000),
        ] {
            damage.mark(InclusiveRectangle {
                left: x,
                top: y,
                right: x + 20,
                bottom: y + 20,
            });
        }
        damage.mark(InclusiveRectangle {
            left: 4000,
            top: 10,
            right: 4001,
            bottom: 11,
        });
        let rectangles = damage.take();
        assert!(rectangles.len() <= 2040);
        for (i, rect) in rectangles.iter().enumerate() {
            assert!(rect.right < width && rect.bottom < height);
            for other in &rectangles[i + 1..] {
                assert!(
                    rect.right < other.left
                        || other.right < rect.left
                        || rect.bottom < other.top
                        || other.bottom < rect.top
                );
            }
        }
        for (x, y) in [
            (0, 0),
            (63, 63),
            (64, 64),
            (3839, 2159),
            (100, 1000),
            (2000, 2000),
        ] {
            for px in x..=(x + 20).min(width - 1) {
                for py in y..=(y + 20).min(height - 1) {
                    assert!(rectangles
                        .iter()
                        .any(|r| r.left <= px && px <= r.right && r.top <= py && py <= r.bottom));
                }
            }
        }
    }
    #[test]
    fn cursor_changes_are_small_separate_from_screen_pixels_and_deduplicated() {
        let image = DecodedImage::new(PixelFormat::RgbA32, 3840, 2160);
        let pointer = Arc::new(DecodedPointer {
            width: 2,
            height: 2,
            hotspot_x: 1,
            hotspot_y: 1,
            bitmap_data: vec![255; 16],
        });
        let mut cursor = Cursor::Default;
        assert!(cursor.update(Cursor::bitmap(pointer.clone())));
        assert!(!cursor.update(Cursor::bitmap(pointer)));
        let bytes = frame(&image, &[], 42, Some(&cursor));
        assert_eq!(bytes.len(), 40);
        assert_eq!(&bytes[..4], b"NRD2");
        assert_eq!(u16::from_le_bytes(bytes[12..14].try_into().unwrap()), 0);
        assert_eq!(u16::from_le_bytes(bytes[14..16].try_into().unwrap()), 3);
        assert!(cursor.update(Cursor::Hidden));
        assert!(!cursor.update(Cursor::Hidden));
    }
}
