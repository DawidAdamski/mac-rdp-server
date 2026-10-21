//! Adapts our capture/encoder pipeline to `RdpServerDisplay`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use ironrdp_server::{DesktopSize, DisplayUpdate, RdpServerDisplay, RdpServerDisplayUpdates};
use tracing::{debug, info};

use super::audit::AuthAudit;
use super::convert;
use super::suppress::{SuppressState, SuppressTracker, Transition};
use crate::capture::Frame;
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
    /// The authentication audit trail (F-101.13).
    ///
    /// A display handler holding an audit object looks out of place, and it is
    /// here for one reason: [`Self::updates`] is the first thing the engine
    /// asks of us that it cannot reach until CredSSP has succeeded. See
    /// `audit.rs` for the two hooks that were available and why neither is the
    /// moment an authentication is accepted.
    audit: Arc<AuthAudit>,
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
        audit: Arc<AuthAudit>,
    ) -> Self {
        Self {
            stream,
            size: DesktopSize { width, height },
            make_encoder,
            suppressed,
            audit,
        }
    }
}

#[async_trait::async_trait]
impl RdpServerDisplay for MacRdpDisplay {
    async fn size(&mut self) -> DesktopSize {
        self.size
    }

    async fn updates(&mut self) -> Result<Box<dyn RdpServerDisplayUpdates>> {
        // F-101.13. The engine calls this from inside the running connection,
        // which with NLA is past CredSSP — so reaching this line *is* the
        // accepted authentication. `size()` would be the wrong place: the
        // engine asks for it while still building the acceptor, before any
        // password has been seen.
        self.audit.session_started();

        // Each connection gets its own stream clone, so it receives the current
        // frame immediately instead of waiting for the next one.
        Ok(Box::new(MacRdpDisplayUpdates {
            stream: self.stream.clone(),
            encoder: (self.make_encoder)(),
            pending: VecDeque::new(),
            suppressed: Arc::clone(&self.suppressed),
            tracker: SuppressTracker::default(),
            last_paused_frame: None,
        }))
    }

    // `request_layout` keeps its default (log-only) implementation: honouring a
    // client-requested resize is F-006, wave 3.
}

pub struct MacRdpDisplayUpdates {
    stream: FrameStream,
    encoder: Box<dyn Encoder>,
    pending: VecDeque<DisplayUpdate>,
    suppressed: Arc<AtomicBool>,
    tracker: SuppressTracker,
    /// The newest frame received while paused, kept so the repaint on resume
    /// has something to send even if the desktop is static afterwards
    /// (F-008.5, AC-4).
    last_paused_frame: Option<Frame>,
}

impl MacRdpDisplayUpdates {
    /// Encodes one frame into `pending` (F-008.10: the same path for every
    /// encoder).
    fn encode(&mut self, frame: Frame) -> Result<()> {
        for update in self.encoder.encode(frame)? {
            self.pending.push_back(convert::display_update(update)?);
        }
        if self.pending.is_empty() {
            debug!("encoder produced no updates for this frame");
        }
        Ok(())
    }

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
    /// dropped without encoding (F-008.4), except for the newest one, which is
    /// kept. On resume the encoder's baseline is dropped and that frame is
    /// encoded whole (F-008.6, F-008.7v2) — **without** a `Resize`, because a
    /// deactivation-reactivation drops real clients: `ironrdp-acceptor` 0.10.0
    /// fails the connection on any Share Data PDU that arrives inside the
    /// reactivation window, and clients send Refresh Rect and Suppress Output
    /// back to back (F-008 §6, 2026-09-24). All of this sits above the encoder,
    /// so it is the same for every `--encoder` (F-008.10).
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
                    self.encoder.invalidate_baseline();
                    info!("full refresh: the next frame is sent whole");
                    // The frame kept from the pause, so a desktop that went
                    // still is repainted immediately rather than at its next
                    // change.
                    if let Some(frame) = self.last_paused_frame.take() {
                        self.encode(frame)?;
                    }
                }
                SuppressState::Suppressed => {
                    tokio::select! {
                        frame = self.stream.next() => {
                            self.last_paused_frame = Some(frame?);
                            self.tracker.frame_skipped();
                        }
                        () = tokio::time::sleep(SUPPRESS_POLL) => {}
                    }
                }
                SuppressState::Idle => {
                    tokio::select! {
                        frame = self.stream.next() => {
                            self.encode(frame?)?;
                        }
                        () = tokio::time::sleep(SUPPRESS_POLL) => {}
                    }
                }
            }
        }
    }
}
