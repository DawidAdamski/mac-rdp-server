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
//!
//! Still used in wave 1 as the fallback when Screen Recording is refused
//! (F-002.13) and as a reference image for `examples/dump_pattern.rs`.

use std::time::{Duration, Instant};

use anyhow::Result;
use bytes::Bytes;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::debug;

use super::canvas::{
    BGRA_BACKGROUND, BGRA_BLACK, BGRA_BLUE, BGRA_GREEN, BGRA_PADDING, BGRA_RED, BGRA_WHITE, Canvas, padded_stride,
};
use super::{DisplayId, DisplayInfo, Frame, FrameSource, FrameStream, PixelFormat};

pub const DEFAULT_WIDTH: u32 = 1280;
pub const DEFAULT_HEIGHT: u32 = 720;

const MARKER_SIZE: u32 = 64;

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
    canvas.number(counter, bar_top + bar_height + height / 12, height / 6, BGRA_WHITE);

    Frame {
        width,
        height,
        stride,
        format: PixelFormat::Bgra8,
        data: Bytes::from(buf),
        sequence: 0,
        captured_at: Instant::now(),
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
    use crate::capture::canvas::STRIDE_ALIGN;

    #[test]
    fn buffer_is_large_enough_for_its_geometry() {
        let frame = render(DEFAULT_WIDTH, DEFAULT_HEIGHT, 0);
        assert_eq!(frame.data.len(), frame.required_len());
        assert!(frame.stride > DEFAULT_WIDTH as usize * 4);
        assert_eq!(frame.stride % STRIDE_ALIGN, 0);
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
