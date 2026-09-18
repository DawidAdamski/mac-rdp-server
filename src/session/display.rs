//! Adapts our capture/encoder pipeline to `RdpServerDisplay`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Result;
use ironrdp_server::{DesktopSize, DisplayUpdate, RdpServerDisplay, RdpServerDisplayUpdates};
use tracing::{debug, info};

use super::convert;
use super::suppress::{SuppressState, SuppressTracker, Transition};
use crate::capture::FrameStream;
use crate::encoder::Encoder;

/// Builds a fresh encoder for each connection.
pub type EncoderFactory = Box<dyn Fn() -> Box<dyn Encoder> + Send>;

/// How often the pause flag is re-read when no frame arrives (F-008.5: a
/// change must be noticed within 100 ms, even on a static desktop).
const SUPPRESS_POLL: Duration = Duration::from_millis(50);

pub struct MacRdpDisplay {
    stream: FrameStream,
    size: DesktopSize,
    make_encoder: EncoderFactory,
    /// The client's "suppress output" flag, written by IronRDP (F-008.3).
    suppressed: Arc<AtomicBool>,
    /// The tracker of a connection that is being reactivated for a full
    /// refresh (F-008.7). IronRDP calls `updates()` again after the
    /// reactivation; the new stream picks the tracker up here, so the 1 s
    /// refresh spacing (F-008.9) survives it. Empty for a new connection.
    reactivating: Arc<Mutex<Option<SuppressTracker>>>,
}

impl MacRdpDisplay {
    /// `suppressed` must be the same flag handed to
    /// `RdpServerBuilder::with_display_suppressed_handle`.
    pub fn new(
        stream: FrameStream,
        width: u16,
        height: u16,
        make_encoder: EncoderFactory,
        suppressed: Arc<AtomicBool>,
    ) -> Self {
        Self {
            stream,
            size: DesktopSize { width, height },
            make_encoder,
            suppressed,
            reactivating: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl RdpServerDisplay for MacRdpDisplay {
    async fn size(&mut self) -> DesktopSize {
        self.size
    }

    async fn updates(&mut self) -> Result<Box<dyn RdpServerDisplayUpdates>> {
        // Each connection gets its own stream clone, so it receives the current
        // frame immediately instead of waiting for the next one.
        //
        // After a refresh reactivation this is exactly the reset F-008.7 relies
        // on: a fresh encoder and a stream that yields the current frame, so the
        // first update is the whole desktop.
        let tracker = self
            .reactivating
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .unwrap_or_default();
        Ok(Box::new(MacRdpDisplayUpdates {
            stream: self.stream.clone(),
            encoder: (self.make_encoder)(),
            pending: VecDeque::new(),
            size: self.size,
            suppressed: Arc::clone(&self.suppressed),
            tracker,
            reactivating: Arc::clone(&self.reactivating),
        }))
    }

    // `request_layout` keeps its default (log-only) implementation: honouring a
    // client-requested resize is F-006, wave 3.
}

pub struct MacRdpDisplayUpdates {
    stream: FrameStream,
    encoder: Box<dyn Encoder>,
    pending: VecDeque<DisplayUpdate>,
    size: DesktopSize,
    suppressed: Arc<AtomicBool>,
    tracker: SuppressTracker,
    reactivating: Arc<Mutex<Option<SuppressTracker>>>,
}

impl MacRdpDisplayUpdates {
    /// F-008.2: one `info` line per change of the pause state.
    fn log_transition(&self, transition: Transition) {
        match transition {
            Transition::Suppressed => info!("client paused display updates (suppress output)"),
            Transition::Resumed {
                paused_for,
                skipped_frames,
            } => info!(
                paused_ms = paused_for.as_millis(),
                skipped_frames, "client resumed display updates"
            ),
        }
    }
}

#[async_trait::async_trait]
impl RdpServerDisplayUpdates for MacRdpDisplayUpdates {
    /// # Cancel safety
    ///
    /// Required by the trait: this is awaited inside `tokio::select!`. The only
    /// awaits are `FrameStream::next`, which is cancel-safe, and a sleep; the
    /// tracker is updated synchronously between them, and updates already
    /// produced sit in `pending` rather than in a local, so nothing is lost.
    ///
    /// The pause flag is re-read at least every [`SUPPRESS_POLL`], not only
    /// when a frame arrives (F-008.5). While paused, frames are taken and
    /// dropped without encoding (F-008.4); on resume the whole desktop is
    /// repainted through a same-size `Resize` (F-008.6, F-008.7). All of this
    /// sits above the encoder, so it is the same for every `--encoder`
    /// (F-008.10).
    async fn next_update(&mut self) -> Result<Option<DisplayUpdate>> {
        loop {
            if let Some(update) = self.pending.pop_front() {
                return Ok(Some(update));
            }

            let now = Instant::now();
            if let Some(transition) = self.tracker.observe(self.suppressed.load(Ordering::Relaxed), now) {
                self.log_transition(transition);
            }

            match self.tracker.state(now) {
                SuppressState::RefreshDue => {
                    self.tracker.refresh_started(now);
                    *self.reactivating.lock().unwrap_or_else(PoisonError::into_inner) = Some(self.tracker.clone());
                    info!(
                        width = self.size.width,
                        height = self.size.height,
                        "full refresh: reactivating at the same desktop size"
                    );
                    return Ok(Some(DisplayUpdate::Resize(self.size)));
                }
                SuppressState::Suppressed => {
                    tokio::select! {
                        frame = self.stream.next() => {
                            frame?;
                            self.tracker.frame_skipped();
                        }
                        () = tokio::time::sleep(SUPPRESS_POLL) => {}
                    }
                }
                SuppressState::Idle => {
                    tokio::select! {
                        frame = self.stream.next() => {
                            for update in self.encoder.encode(frame?)? {
                                self.pending.push_back(convert::display_update(update)?);
                            }
                            if self.pending.is_empty() {
                                debug!("encoder produced no updates for this frame");
                            }
                        }
                        () = tokio::time::sleep(SUPPRESS_POLL) => {}
                    }
                }
            }
        }
    }
}
