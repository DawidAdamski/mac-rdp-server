//! Frame sources.
//!
//! This module knows nothing about RDP. See `specs/01-architecture.md` §1.

mod backoff;
pub mod canvas;
pub mod geometry;
pub mod placeholder;
mod recovery;
pub mod screen;
pub mod test_pattern;
mod watchdog;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use anyhow::Result;
use bytes::Bytes;
use tokio::sync::watch;

/// Pixel layout of a [`Frame`] buffer.
///
/// Only one variant: ScreenCaptureKit is configured for
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
    /// Monotonic counter assigned by the source. Gaps observed by a consumer
    /// are frames lost to coalescing, which is how [`CaptureStats`] measures it.
    pub sequence: u64,
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
            .field("sequence", &self.sequence)
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
    /// Top-left of the display in the global macOS **point** space. Zero for the
    /// primary display; non-zero for secondary displays. Input needs it so a
    /// click at desktop (0,0) lands on this display, not the primary (F-003.6).
    pub origin_x: f64,
    pub origin_y: f64,
    pub name: String,
}

/// Diagnostic counters for a capture session (F-002.18).
#[derive(Debug, Default)]
pub struct CaptureStats {
    sequence: AtomicU64,
    received: AtomicU64,
    delivered: AtomicU64,
    coalesced: AtomicU64,
    skipped: AtomicU64,
    rejected: AtomicU64,
    bytes: AtomicU64,
    logged_rejections: AtomicU64,
}

/// A consistent-enough read of [`CaptureStats`] for logging.
#[derive(Debug, Clone, Copy)]
pub struct StatsSnapshot {
    pub received: u64,
    pub delivered: u64,
    pub coalesced: u64,
    pub skipped: u64,
    pub rejected: u64,
    pub bytes: u64,
}

impl StatsSnapshot {
    #[must_use]
    pub fn average_frame_bytes(&self) -> u64 {
        self.bytes.checked_div(self.received).unwrap_or(0)
    }
}

impl CaptureStats {
    pub(crate) fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn record_received(&self, bytes: u64) {
        self.received.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(crate) fn record_delivered(&self, skipped: u64) {
        self.delivered.fetch_add(1, Ordering::Relaxed);
        if skipped > 0 {
            self.coalesced.fetch_add(skipped, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_skipped(&self) {
        self.skipped.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_rejected(&self) {
        self.rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// Rate limiter for rejection logging: a bad pixel format is bad for every
    /// frame, and one warning per frame at 60 fps drowns the log.
    pub(crate) fn should_log_rejection(&self) -> bool {
        let n = self.logged_rejections.fetch_add(1, Ordering::Relaxed);
        n < 3 || n.is_power_of_two()
    }

    /// Restarts the rejection rate limiter after a successful recovery
    /// (F-007.18).
    ///
    /// The counter is cumulative, so without this it keeps climbing across
    /// incidents: after one bad session it sits in the hundreds, and the *next*
    /// failure would stay silent until its thousandth rejected frame — half a
    /// minute of nothing in the log exactly when something is wrong.
    pub(crate) fn reset_rejection_log(&self) {
        self.logged_rejections.store(0, Ordering::Relaxed);
    }

    #[must_use]
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            received: self.received.load(Ordering::Relaxed),
            delivered: self.delivered.load(Ordering::Relaxed),
            coalesced: self.coalesced.load(Ordering::Relaxed),
            skipped: self.skipped.load(Ordering::Relaxed),
            rejected: self.rejected.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
        }
    }
}

/// A source of frames.
///
/// Implementations: [`test_pattern::TestPatternSource`] and
/// [`placeholder::PlaceholderSource`] (synthetic), and
/// [`screen::ScreenCaptureKitSource`] (the real desktop, ADR-0002).
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
    last_sequence: Option<u64>,
    stats: Option<Arc<CaptureStats>>,
}

impl FrameStream {
    pub(crate) fn new(rx: watch::Receiver<Frame>) -> Self {
        Self {
            rx,
            delivered_initial: false,
            last_sequence: None,
            stats: None,
        }
    }

    #[must_use]
    pub(crate) fn with_stats(mut self, stats: Arc<CaptureStats>) -> Self {
        self.stats = Some(stats);
        self
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
        let frame = if self.delivered_initial {
            self.rx.changed().await?;
            self.rx.borrow_and_update().clone()
        } else {
            self.delivered_initial = true;
            self.rx.mark_unchanged();
            self.rx.borrow().clone()
        };

        if let Some(stats) = &self.stats {
            let skipped = self
                .last_sequence
                .map_or(0, |last| frame.sequence.saturating_sub(last).saturating_sub(1));
            stats.record_delivered(skipped);
        }
        self.last_sequence = Some(frame.sequence);

        Ok(frame)
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
            last_sequence: None,
            stats: self.stats.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(sequence: u64) -> Frame {
        let mut frame = test_pattern::render(16, 16, sequence);
        frame.sequence = sequence;
        frame
    }

    #[tokio::test]
    async fn the_first_read_returns_the_current_frame_without_waiting() {
        let (_tx, rx) = watch::channel(frame(7));
        let mut stream = FrameStream::new(rx);
        let first = stream.next().await.expect("first frame");
        assert_eq!(first.sequence, 7);
    }

    #[tokio::test]
    async fn a_clone_starts_over_so_a_new_client_sees_the_current_frame() {
        let (_tx, rx) = watch::channel(frame(3));
        let mut stream = FrameStream::new(rx);
        stream.next().await.expect("first frame");

        let mut fresh = stream.clone();
        assert_eq!(fresh.next().await.expect("clone's first frame").sequence, 3);
    }

    #[tokio::test]
    async fn coalesced_frames_are_counted_from_sequence_gaps() {
        let stats = Arc::new(CaptureStats::default());
        let (tx, rx) = watch::channel(frame(0));
        let mut stream = FrameStream::new(rx).with_stats(Arc::clone(&stats));

        stream.next().await.expect("frame 0");
        // Three frames produced while the consumer was busy; only the last survives.
        tx.send(frame(1)).expect("send");
        tx.send(frame(2)).expect("send");
        tx.send(frame(3)).expect("send");
        let latest = stream.next().await.expect("latest frame");

        assert_eq!(latest.sequence, 3, "the newest frame must win");
        assert_eq!(stats.snapshot().coalesced, 2, "frames 1 and 2 were skipped");
        assert_eq!(stats.snapshot().delivered, 2);
    }

    #[test]
    fn average_frame_size_of_an_empty_session_is_zero_not_a_panic() {
        let snapshot = CaptureStats::default().snapshot();
        assert_eq!(snapshot.average_frame_bytes(), 0);
    }

    #[test]
    fn rejection_logging_is_rate_limited() {
        let stats = CaptureStats::default();
        let logged = (0..1000).filter(|_| stats.should_log_rejection()).count();
        assert!(
            (5..20).contains(&logged),
            "1000 bad frames should produce a handful of lines, got {logged}"
        );
    }
}
