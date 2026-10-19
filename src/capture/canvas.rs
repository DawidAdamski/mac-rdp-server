//! Minimal BGRA drawing surface shared by the synthetic frame sources.
//!
//! Exists so that the test pattern (F-001.10) and the placeholder screens
//! (F-002.14) draw through the same stride-aware primitives — the arithmetic
//! that ADR-0005 warns about is written once.

/// Row alignment for generated buffers, in bytes.
pub const STRIDE_ALIGN: usize = 64;

// Colours, written as BGRA bytes (ADR-0005).
pub const BGRA_BACKGROUND: [u8; 4] = [0x20, 0x20, 0x20, 0xFF];
pub const BGRA_RED: [u8; 4] = [0x00, 0x00, 0xFF, 0xFF];
pub const BGRA_GREEN: [u8; 4] = [0x00, 0xFF, 0x00, 0xFF];
pub const BGRA_BLUE: [u8; 4] = [0xFF, 0x00, 0x00, 0xFF];
pub const BGRA_WHITE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
pub const BGRA_BLACK: [u8; 4] = [0x00, 0x00, 0x00, 0xFF];
/// Deep red — the "you have no Screen Recording permission" screen.
pub const BGRA_ALERT: [u8; 4] = [0x20, 0x20, 0xB0, 0xFF];
/// Deep blue — the "waiting for the first frame" screen.
pub const BGRA_WAITING: [u8; 4] = [0x80, 0x40, 0x10, 0xFF];

/// Amber: capture is temporarily unavailable — the display went away and
/// recovery is retrying (F-007.27). Distinct from the alert red of a refused
/// permission and from the waiting blue, because the three mean three
/// different things and the user has only the colour to go on.
pub const BGRA_STALLED: [u8; 4] = [0x10, 0x80, 0xD0, 0xFF];
/// Fills the row padding. Never visible unless stride handling is wrong.
pub const BGRA_PADDING: [u8; 4] = [0xFF, 0x00, 0xFF, 0xFF];

/// Computes a padded stride for a given width.
///
/// Guarantees `stride > width * bpp` so the stride path is exercised even for
/// widths that happen to be naturally aligned (F-001.11). 1280 px is exactly
/// such a width: `1280 * 4 = 5120` is already 64-byte aligned.
#[must_use]
pub fn padded_stride(width: u32, bpp: usize) -> usize {
    let row = (width as usize).saturating_mul(bpp);
    let aligned = row.div_ceil(STRIDE_ALIGN).saturating_mul(STRIDE_ALIGN);
    aligned.saturating_add(STRIDE_ALIGN)
}

/// Placement of one seven-segment digit: position, size and stroke thickness.
#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub t: u32,
}

pub struct Canvas<'a> {
    pub buf: &'a mut [u8],
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub bpp: usize,
}

impl Canvas<'_> {
    /// Fills the entire allocation, padding bytes included.
    pub fn fill(&mut self, colour: [u8; 4]) {
        for chunk in self.buf.chunks_exact_mut(self.bpp) {
            chunk.copy_from_slice(&colour);
        }
    }

    pub fn put(&mut self, x: u32, y: u32, colour: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y as usize) * self.stride + (x as usize) * self.bpp;
        if let Some(px) = self.buf.get_mut(offset..offset + self.bpp) {
            px.copy_from_slice(&colour);
        }
    }

    pub fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, colour: [u8; 4]) {
        for dy in 0..h {
            for dx in 0..w {
                self.put(x.saturating_add(dx), y.saturating_add(dy), colour);
            }
        }
    }

    /// Draws the outline of a rectangle with the given stroke thickness.
    pub fn frame(&mut self, x: u32, y: u32, w: u32, h: u32, t: u32, colour: [u8; 4]) {
        self.rect(x, y, w, t, colour);
        self.rect(x, y + h.saturating_sub(t), w, t, colour);
        self.rect(x, y, t, h, colour);
        self.rect(x + w.saturating_sub(t), y, t, h, colour);
    }

    /// Draws one decimal digit as a seven-segment glyph.
    ///
    /// Segment order is the conventional `a b c d e f g`:
    /// top, top-right, bottom-right, bottom, bottom-left, top-left, middle.
    pub fn seven_segment(&mut self, glyph: Glyph, digit: u8, colour: [u8; 4]) {
        let Glyph { x, y, w, h, t } = glyph;
        const SEGMENTS: [[bool; 7]; 10] = [
            [true, true, true, true, true, true, false],     // 0
            [false, true, true, false, false, false, false], // 1
            [true, true, false, true, true, false, true],    // 2
            [true, true, true, true, false, false, true],    // 3
            [false, true, true, false, false, true, true],   // 4
            [true, false, true, true, false, true, true],    // 5
            [true, false, true, true, true, true, true],     // 6
            [true, true, true, false, false, false, false],  // 7
            [true, true, true, true, true, true, true],      // 8
            [true, true, true, true, false, true, true],     // 9
        ];
        let Some(on) = SEGMENTS.get(usize::from(digit)) else {
            return;
        };
        let half = h / 2;
        if on[0] {
            self.rect(x, y, w, t, colour);
        }
        if on[1] {
            self.rect(x + w.saturating_sub(t), y, t, half, colour);
        }
        if on[2] {
            self.rect(x + w.saturating_sub(t), y + half, t, half, colour);
        }
        if on[3] {
            self.rect(x, y + h.saturating_sub(t), w, t, colour);
        }
        if on[4] {
            self.rect(x, y + half, t, half, colour);
        }
        if on[5] {
            self.rect(x, y, t, half, colour);
        }
        if on[6] {
            self.rect(x, y + half.saturating_sub(t / 2), w, t, colour);
        }
    }

    /// Draws a decimal number centred horizontally at `y`.
    pub fn number(&mut self, value: u64, y: u32, digit_h: u32, colour: [u8; 4]) {
        let digits = value.to_string();
        let digit_w = digit_h / 2;
        let thickness = (digit_h / 8).max(2);
        let spacing = digit_w / 4;
        let total_w = (digit_w + spacing) * digits.len() as u32;
        let start_x = self.width.saturating_sub(total_w) / 2;

        for (i, ch) in digits.chars().enumerate() {
            let digit = ch.to_digit(10).unwrap_or(0) as u8;
            let glyph = Glyph {
                x: start_x + (digit_w + spacing) * i as u32,
                y,
                w: digit_w,
                h: digit_h,
                t: thickness,
            };
            self.seven_segment(glyph, digit, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stride_is_padded_beyond_the_row_width() {
        // 1280 * 4 = 5120 is already 64-byte aligned; we must still pad,
        // otherwise the stride path would never be exercised (F-001.11).
        assert_eq!(1280 * 4 % STRIDE_ALIGN, 0, "precondition for this test");
        let stride = padded_stride(1280, 4);
        assert!(stride > 1280 * 4, "stride {stride} must exceed the row width");
        assert_eq!(stride % STRIDE_ALIGN, 0);
    }

    #[test]
    fn put_writes_at_stride_offsets_not_row_offsets() {
        let (width, height, bpp) = (4u32, 3u32, 4usize);
        let stride = padded_stride(width, bpp);
        let mut buf = vec![0u8; height as usize * stride];
        let mut canvas = Canvas {
            buf: &mut buf,
            width,
            height,
            stride,
            bpp,
        };
        canvas.put(0, 1, BGRA_WHITE);

        assert_eq!(&buf[stride..stride + 4], BGRA_WHITE, "row 1 starts at one stride");
        assert_eq!(
            &buf[width as usize * bpp..width as usize * bpp + 4],
            &[0, 0, 0, 0],
            "the naive row offset must still be untouched padding"
        );
    }

    #[test]
    fn drawing_outside_the_visible_area_is_ignored() {
        let (width, height, bpp) = (4u32, 3u32, 4usize);
        let stride = padded_stride(width, bpp);
        let mut buf = vec![0u8; height as usize * stride];
        let mut canvas = Canvas {
            buf: &mut buf,
            width,
            height,
            stride,
            bpp,
        };
        // Deliberately overflows both axes; must clip, not panic or wrap.
        canvas.rect(3, 2, 100, 100, BGRA_WHITE);
        assert_eq!(&buf[2 * stride + 3 * bpp..2 * stride + 3 * bpp + 4], BGRA_WHITE);
    }
}
