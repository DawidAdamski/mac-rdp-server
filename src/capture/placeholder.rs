//! Stand-in screens shown when real capture is unavailable (F-002.13, F-002.14).
//!
//! The point is diagnosis: a black screen has a dozen possible causes, whereas
//! a red screen with a "no entry" sign has exactly one. Each placeholder is
//! visually unmistakable both from the real desktop and from the wave 0 test
//! pattern.

use std::time::Instant;

use anyhow::Result;
use bytes::Bytes;
use tokio::sync::watch;

use super::canvas::{BGRA_ALERT, BGRA_BLACK, BGRA_WAITING, BGRA_WHITE, Canvas, padded_stride};
use super::{DisplayId, DisplayInfo, Frame, FrameSource, FrameStream, PixelFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placeholder {
    /// Screen Recording was refused. Deep red, "no entry" sign.
    PermissionDenied,
    /// The stream started but has not produced a frame yet. Deep blue, ellipsis.
    WaitingForFirstFrame,
}

impl Placeholder {
    const fn background(self) -> [u8; 4] {
        match self {
            Self::PermissionDenied => BGRA_ALERT,
            Self::WaitingForFirstFrame => BGRA_WAITING,
        }
    }
}

/// Renders a placeholder screen.
#[must_use]
pub fn render(width: u32, height: u32, kind: Placeholder) -> Frame {
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

    canvas.fill(BGRA_BLACK);
    canvas.rect(0, 0, width, height, kind.background());

    let unit = height / 8;
    let centre_y = height / 2;

    match kind {
        Placeholder::PermissionDenied => {
            // A "no entry" sign: heavy square outline with a bar through it.
            let size = unit * 3;
            let x = width.saturating_sub(size) / 2;
            let y = centre_y.saturating_sub(size / 2);
            let stroke = (unit / 3).max(4);
            canvas.frame(x, y, size, size, stroke, BGRA_WHITE);
            canvas.rect(
                x + stroke * 2,
                centre_y.saturating_sub(stroke / 2),
                size.saturating_sub(stroke * 4),
                stroke,
                BGRA_WHITE,
            );
        }
        Placeholder::WaitingForFirstFrame => {
            // An ellipsis: three squares in a row.
            let dot = unit / 2;
            let gap = dot;
            let total = dot * 3 + gap * 2;
            let x = width.saturating_sub(total) / 2;
            let y = centre_y.saturating_sub(dot / 2);
            for i in 0..3 {
                canvas.rect(x + (dot + gap) * i, y, dot, dot, BGRA_WHITE);
            }
        }
    }

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

/// A [`FrameSource`] that shows a single placeholder screen forever.
///
/// Used when Screen Recording is refused (F-002.13): the server still accepts
/// RDP connections and still shows *something* explanatory, instead of failing
/// to start or serving black.
pub struct PlaceholderSource {
    width: u32,
    height: u32,
    kind: Placeholder,
    /// Held only to keep the channel open; the value never changes.
    tx: Option<watch::Sender<Frame>>,
}

impl PlaceholderSource {
    #[must_use]
    pub fn new(width: u32, height: u32, kind: Placeholder) -> Self {
        Self {
            width,
            height,
            kind,
            tx: None,
        }
    }
}

#[async_trait::async_trait]
impl FrameSource for PlaceholderSource {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(vec![DisplayInfo {
            id: DisplayId(0),
            width_px: self.width,
            height_px: self.height,
            scale_factor: 1.0,
            name: format!("placeholder ({:?})", self.kind),
        }])
    }

    fn desktop_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    async fn start(&mut self) -> Result<FrameStream> {
        // The frame never changes, so the channel needs no producer task: every
        // client gets the current value on its first read and then simply waits.
        let (tx, rx) = watch::channel(render(self.width, self.height, self.kind));
        // Holding the sender keeps the channel open, so `next()` waits rather
        // than erroring on a closed channel.
        self.tx = Some(tx);
        Ok(FrameStream::new(rx))
    }

    async fn stop(&mut self) -> Result<()> {
        self.tx = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_is_consistent() {
        for kind in [Placeholder::PermissionDenied, Placeholder::WaitingForFirstFrame] {
            let frame = render(640, 480, kind);
            assert_eq!(frame.width, 640);
            assert_eq!(frame.height, 480);
            assert_eq!(frame.data.len(), frame.required_len());
            assert!(frame.stride > 640 * 4);
        }
    }

    #[test]
    fn placeholders_are_visually_distinct_from_each_other() {
        // F-002.14: the user must be able to tell "no permission" from
        // "nothing arrived yet" at a glance, without reading the log.
        let denied = render(320, 240, Placeholder::PermissionDenied);
        let waiting = render(320, 240, Placeholder::WaitingForFirstFrame);
        assert_eq!(denied.pixel(4, 4), Some(BGRA_ALERT));
        assert_eq!(waiting.pixel(4, 4), Some(BGRA_WAITING));
        assert_ne!(denied.data, waiting.data);
    }

    #[test]
    fn placeholders_are_distinct_from_the_wave_0_test_pattern() {
        let denied = render(1280, 720, Placeholder::PermissionDenied);
        let pattern = super::super::test_pattern::render(1280, 720, 0);
        assert_ne!(denied.pixel(4, 4), pattern.pixel(4, 4));
    }

    #[test]
    fn the_sign_is_actually_drawn() {
        let frame = render(640, 480, Placeholder::PermissionDenied);
        // Centre of the bar through the sign.
        assert_eq!(frame.pixel(320, 240), Some(BGRA_WHITE));
    }
}
