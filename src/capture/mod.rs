//! Frame sources.
//!
//! This module knows nothing about RDP. See `specs/01-architecture.md` §1.

pub mod test_pattern;

use std::time::Instant;

use anyhow::Result;
use bytes::Bytes;
use tokio::sync::watch;

/// Pixel layout of a [`Frame`] buffer.
///
/// Only one variant for now: ScreenCaptureKit is configured for
/// `kCVPixelFormatType_32BGRA`, i.e. bytes in memory are B, G, R, A (ADR-0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra8,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Bgra8 => 4,
        }
    }
}

/// A single captured image.
///
/// `stride` is the number of bytes per row and **may be larger than
/// `width * bytes_per_pixel`** — CoreVideo pads rows. Any code indexing into
/// `data` must use `y * stride + x * bpp`, never `y * width * bpp` (ADR-0005).
#[derive(Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub format: PixelFormat,
    pub data: Bytes,
    pub captured_at: Instant,
}

impl Frame {
    /// Returns the pixel at `(x, y)` in the buffer's native byte order,
    /// or `None` if the coordinates are outside the frame.
    ///
    /// Exists primarily so tests can assert on geometry without duplicating
    /// the stride arithmetic they are meant to verify.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let bpp = self.format.bytes_per_pixel();
        let offset = (y as usize)
            .checked_mul(self.stride)?
            .checked_add((x as usize).checked_mul(bpp)?)?;
        let slice = self.data.get(offset..offset.checked_add(bpp)?)?;
        Some([slice[0], slice[1], slice[2], slice[3]])
    }

    /// Number of bytes the buffer must hold for this geometry.
    #[must_use]
    pub fn required_len(&self) -> usize {
        (self.height as usize).saturating_mul(self.stride)
    }
}

impl core::fmt::Debug for Frame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("stride", &self.stride)
            .field("format", &self.format)
            .field("data_len", &self.data.len())
            .finish()
    }
}

/// Identifier of a capturable display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayId(pub u32);

#[derive(Debug, Clone)]
pub struct DisplayInfo {
    pub id: DisplayId,
    /// Backing-store pixels, not points.
    pub width_px: u32,
    pub height_px: u32,
    /// 2.0 on Retina. Input needs this to convert RDP pixels to macOS points (ADR-0003).
    pub scale_factor: f64,
    pub name: String,
}

/// A source of frames.
///
/// Implementations: [`test_pattern::TestPatternSource`] (wave 0),
/// `ScreenCaptureKitSource` (wave 1, ADR-0002).
#[async_trait::async_trait]
pub trait FrameSource: Send {
    fn displays(&self) -> Result<Vec<DisplayInfo>>;
    fn desktop_size(&self) -> (u32, u32);
    async fn start(&mut self) -> Result<FrameStream>;
    async fn stop(&mut self) -> Result<()>;
}

/// Receiving end of a frame source.
///
/// Backed by a `watch` channel, so frames **coalesce**: a slow consumer misses
/// intermediate frames but always gets the newest one, and nothing queues up
/// in memory (`01-architecture.md` §4b).
pub struct FrameStream {
    rx: watch::Receiver<Frame>,
    delivered_initial: bool,
}

impl FrameStream {
    pub(crate) fn new(rx: watch::Receiver<Frame>) -> Self {
        Self {
            rx,
            delivered_initial: false,
        }
    }

    /// Waits for the next frame.
    ///
    /// The first call returns the current frame immediately, so a freshly
    /// connected client does not stare at a blank screen until something moves.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel-safe: it is awaited inside `tokio::select!` by
    /// `ironrdp-server`, which requires that no frame is lost on cancellation.
    /// The only await point is `watch::Receiver::changed`, which is cancel-safe.
    pub async fn next(&mut self) -> Result<Frame> {
        if !self.delivered_initial {
            self.delivered_initial = true;
            self.rx.mark_unchanged();
            return Ok(self.rx.borrow().clone());
        }
        self.rx.changed().await?;
        Ok(self.rx.borrow_and_update().clone())
    }
}

impl Clone for FrameStream {
    /// A cloned stream starts over: the new consumer gets the current frame
    /// immediately rather than waiting for the next one. Each RDP connection
    /// takes its own clone.
    fn clone(&self) -> Self {
        Self {
            rx: self.rx.clone(),
            delivered_initial: false,
        }
    }
}
