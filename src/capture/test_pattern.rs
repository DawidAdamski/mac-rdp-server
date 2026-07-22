//! Wave 0 frame source: a synthetic test pattern (F-001.10, F-001.11).
//!
//! The pattern is designed to make three specific failures obvious on screen
//! instead of subtle:
//!
//! * **swapped colour channels** — three saturated bars, one per channel;
//! * **broken stride handling** — the buffer is deliberately padded so that
//!   `stride != width * 4`, and the padding is filled with magenta, so a
//!   consumer that ignores stride produces a diagonal magenta shear rather
//!   than a plausible-looking image;
//! * **a frozen update path** — a seconds counter that must tick.

use std::time::{Duration, Instant};

use anyhow::Result;
use bytes::Bytes;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::debug;

use super::{DisplayId, DisplayInfo, Frame, FrameSource, FrameStream, PixelFormat};

pub const DEFAULT_WIDTH: u32 = 1280;
pub const DEFAULT_HEIGHT: u32 = 720;

/// Row alignment for the generated buffer, in bytes.
const STRIDE_ALIGN: usize = 64;

// Colours, written as BGRA bytes (ADR-0005).
const BGRA_BACKGROUND: [u8; 4] = [0x20, 0x20, 0x20, 0xFF];
const BGRA_RED: [u8; 4] = [0x00, 0x00, 0xFF, 0xFF];
const BGRA_GREEN: [u8; 4] = [0x00, 0xFF, 0x00, 0xFF];
const BGRA_BLUE: [u8; 4] = [0xFF, 0x00, 0x00, 0xFF];
const BGRA_WHITE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
const BGRA_BLACK: [u8; 4] = [0x00, 0x00, 0x00, 0xFF];
/// Fills the row padding. Never visible unless stride handling is wrong.
const BGRA_PADDING: [u8; 4] = [0xFF, 0x00, 0xFF, 0xFF];

const MARKER_SIZE: u32 = 64;

/// Computes the padded stride for a given width.
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

/// Renders one frame of the test pattern.
///
/// `counter` is drawn as decimal digits; callers pass elapsed whole seconds.
#[must_use]
pub fn render(width: u32, height: u32, counter: u64) -> Frame {
    let bpp = PixelFormat::Bgra8.bytes_per_pixel();
    let stride = padded_stride(width, bpp);
    let mut buf = vec![0u8; (height as usize).saturating_mul(stride)];

    let mut canvas = Canvas {
        buf: &mut buf,
        width,
        height,
        stride,
        bpp,
    };

    // Padding first, so every later write lands on top of a loud background.
    canvas.fill(BGRA_PADDING);
    canvas.rect(0, 0, width, height, BGRA_BACKGROUND);

    // Three saturated bars — one per channel (AC-5).
    let bar_top = height / 8;
    let bar_height = height / 3;
    let bar_width = width / 4;
    let bar_gap = width / 32;
    let bars_left = (width - (bar_width * 3 + bar_gap * 2)) / 2;
    for (i, colour) in [BGRA_RED, BGRA_GREEN, BGRA_BLUE].into_iter().enumerate() {
        let x = bars_left + (bar_width + bar_gap) * i as u32;
        canvas.rect(x, bar_top, bar_width, bar_height, colour);
    }

    // Orientation markers (AC-6).
    canvas.rect(0, 0, MARKER_SIZE, MARKER_SIZE, BGRA_WHITE);
    canvas.rect(
        width.saturating_sub(MARKER_SIZE),
        height.saturating_sub(MARKER_SIZE),
        MARKER_SIZE,
        MARKER_SIZE,
        BGRA_BLACK,
    );

    // Seconds counter (AC-7).
    let digits = counter.to_string();
    let digit_h = height / 6;
    let digit_w = digit_h / 2;
    let thickness = (digit_h / 8).max(2);
    let spacing = digit_w / 4;
    let total_w = (digit_w + spacing) * digits.len() as u32;
    let digits_x = (width.saturating_sub(total_w)) / 2;
    let digits_y = bar_top + bar_height + height / 12;
    for (i, ch) in digits.chars().enumerate() {
        let value = ch.to_digit(10).unwrap_or(0) as u8;
        let glyph = Glyph {
            x: digits_x + (digit_w + spacing) * i as u32,
            y: digits_y,
            w: digit_w,
            h: digit_h,
            t: thickness,
        };
        canvas.seven_segment(glyph, value, BGRA_WHITE);
    }

    Frame {
        width,
        height,
        stride,
        format: PixelFormat::Bgra8,
        data: Bytes::from(buf),
        captured_at: Instant::now(),
    }
}

/// Placement of one seven-segment digit: position, size and stroke thickness.
#[derive(Debug, Clone, Copy)]
struct Glyph {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    t: u32,
}

struct Canvas<'a> {
    buf: &'a mut [u8],
    width: u32,
    height: u32,
    stride: usize,
    bpp: usize,
}

impl Canvas<'_> {
    /// Fills the entire allocation, padding bytes included.
    fn fill(&mut self, colour: [u8; 4]) {
        for chunk in self.buf.chunks_exact_mut(self.bpp) {
            chunk.copy_from_slice(&colour);
        }
    }

    fn put(&mut self, x: u32, y: u32, colour: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y as usize) * self.stride + (x as usize) * self.bpp;
        if let Some(px) = self.buf.get_mut(offset..offset + self.bpp) {
            px.copy_from_slice(&colour);
        }
    }

    fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, colour: [u8; 4]) {
        for dy in 0..h {
            for dx in 0..w {
                self.put(x.saturating_add(dx), y.saturating_add(dy), colour);
            }
        }
    }

    /// Draws one decimal digit as a seven-segment glyph.
    ///
    /// Segment order is the conventional `a b c d e f g`:
    /// top, top-right, bottom-right, bottom, bottom-left, top-left, middle.
    fn seven_segment(&mut self, glyph: Glyph, digit: u8, colour: [u8; 4]) {
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
}

/// Wave 0 [`FrameSource`]: renders the test pattern once per second.
pub struct TestPatternSource {
    width: u32,
    height: u32,
    task: Option<JoinHandle<()>>,
}

impl TestPatternSource {
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            task: None,
        }
    }
}

impl Default for TestPatternSource {
    fn default() -> Self {
        Self::new(DEFAULT_WIDTH, DEFAULT_HEIGHT)
    }
}

#[async_trait::async_trait]
impl FrameSource for TestPatternSource {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(vec![DisplayInfo {
            id: DisplayId(0),
            width_px: self.width,
            height_px: self.height,
            scale_factor: 1.0,
            name: "test pattern".to_owned(),
        }])
    }

    fn desktop_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    async fn start(&mut self) -> Result<FrameStream> {
        let (width, height) = (self.width, self.height);
        // Render synchronously so the stream has a frame before anyone subscribes.
        let (tx, rx) = watch::channel(render(width, height, 0));

        let task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));
            ticker.tick().await; // fires immediately; frame 0 is already published
            let mut counter: u64 = 0;
            loop {
                ticker.tick().await;
                counter = counter.wrapping_add(1);
                if tx.send(render(width, height, counter)).is_err() {
                    debug!("no frame consumers left, stopping test pattern");
                    break;
                }
            }
        });
        self.task = Some(task);

        Ok(FrameStream::new(rx))
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stride_is_padded_beyond_the_row_width() {
        // 1280 * 4 = 5120 is already 64-byte aligned; the pattern must still pad,
        // otherwise wave 0 would never exercise the stride path (F-001.11).
        assert_eq!(1280 * 4 % STRIDE_ALIGN, 0, "precondition for this test");
        let stride = padded_stride(1280, 4);
        assert!(stride > 1280 * 4, "stride {stride} must exceed the row width");
        assert_eq!(stride % STRIDE_ALIGN, 0);
    }

    #[test]
    fn buffer_is_large_enough_for_its_geometry() {
        let frame = render(DEFAULT_WIDTH, DEFAULT_HEIGHT, 0);
        assert_eq!(frame.data.len(), frame.required_len());
        assert!(frame.stride > DEFAULT_WIDTH as usize * 4);
    }

    #[test]
    fn orientation_markers_land_in_the_right_corners() {
        let frame = render(DEFAULT_WIDTH, DEFAULT_HEIGHT, 0);
        assert_eq!(frame.pixel(0, 0), Some(BGRA_WHITE), "top-left must be white");
        assert_eq!(
            frame.pixel(DEFAULT_WIDTH - 1, DEFAULT_HEIGHT - 1),
            Some(BGRA_BLACK),
            "bottom-right must be black"
        );
        // Just outside the white marker.
        assert_eq!(frame.pixel(MARKER_SIZE, MARKER_SIZE), Some(BGRA_BACKGROUND));
    }

    #[test]
    fn colour_bars_carry_one_saturated_channel_each() {
        let frame = render(DEFAULT_WIDTH, DEFAULT_HEIGHT, 0);
        let bar_top = DEFAULT_HEIGHT / 8;
        let bar_height = DEFAULT_HEIGHT / 3;
        let bar_width = DEFAULT_WIDTH / 4;
        let bar_gap = DEFAULT_WIDTH / 32;
        let bars_left = (DEFAULT_WIDTH - (bar_width * 3 + bar_gap * 2)) / 2;
        let y = bar_top + bar_height / 2;

        for (i, expected) in [BGRA_RED, BGRA_GREEN, BGRA_BLUE].into_iter().enumerate() {
            let x = bars_left + (bar_width + bar_gap) * i as u32 + bar_width / 2;
            assert_eq!(frame.pixel(x, y), Some(expected), "bar {i} at ({x},{y})");
        }
    }

    #[test]
    fn pixel_lookup_respects_stride_not_row_width() {
        let frame = render(DEFAULT_WIDTH, DEFAULT_HEIGHT, 0);
        // Reading row 1 with the naive `y * width * 4` formula lands inside row 0's
        // padding; the correct formula must not. This is the whole point of ADR-0005.
        let naive_offset = DEFAULT_WIDTH as usize * 4;
        let naive = &frame.data[naive_offset..naive_offset + 4];
        assert_eq!(naive, BGRA_PADDING, "naive indexing must hit the padding");
        assert_eq!(
            frame.pixel(0, 1),
            Some(BGRA_WHITE),
            "stride-aware indexing must hit the white marker"
        );
    }

    #[test]
    fn padding_is_present_and_marked() {
        let frame = render(DEFAULT_WIDTH, DEFAULT_HEIGHT, 0);
        let row_bytes = DEFAULT_WIDTH as usize * 4;
        let pad_start = row_bytes;
        assert_eq!(&frame.data[pad_start..pad_start + 4], BGRA_PADDING);
    }

    #[test]
    fn counter_changes_the_rendered_image() {
        let a = render(DEFAULT_WIDTH, DEFAULT_HEIGHT, 1);
        let b = render(DEFAULT_WIDTH, DEFAULT_HEIGHT, 2);
        assert_ne!(a.data, b.data, "counter must be visible in the pixels");
    }

    #[test]
    fn pixel_out_of_bounds_returns_none() {
        let frame = render(64, 64, 0);
        assert!(frame.pixel(64, 0).is_none());
        assert!(frame.pixel(0, 64).is_none());
    }
}
