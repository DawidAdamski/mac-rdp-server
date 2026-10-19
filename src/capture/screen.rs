//! Wave 1 frame source: the real Mac desktop via ScreenCaptureKit (F-002).
//!
//! Two things here are load-bearing and easy to get wrong:
//!
//! * **Stride.** `CVPixelBuffer` rows are padded; `bytes_per_row` is carried
//!   straight through to `BitmapUpdate` and rows are never repacked (ADR-0005).
//! * **Pixel format.** On macOS 26 / Apple Silicon the system's own default is
//!   `420v` (bi-planar YCbCr), not BGRA. We request BGRA *and* verify what
//!   actually arrives, because a silent YCbCr buffer read as BGRA is garbage
//!   that still looks like a plausible image (F-002.9).

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail};
use bytes::Bytes;
use screencapturekit::cm::{CMSampleBuffer, CMSampleBufferExt as _, CMSampleBufferSCExt as _, SCFrameStatus};
use screencapturekit::error::SCError;
use screencapturekit::shareable_content::{SCDisplay, SCShareableContent, SCShareableContentInfo};
use screencapturekit::stream::configuration::{PixelFormat as ScPixelFormat, SCStreamConfiguration};
use screencapturekit::stream::content_filter::SCContentFilter;
use screencapturekit::stream::delegate_trait::SCStreamDelegateTrait;
use screencapturekit::stream::output_trait::SCStreamOutputTrait;
use screencapturekit::stream::output_type::SCStreamOutputType;
use screencapturekit::stream::sc_stream::SCStream;
use screencapturekit::utils::completion;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tracing::{debug, info, trace, warn};

use super::backoff::Backoff;
use super::placeholder::{self, Placeholder};
use super::recovery::{FailureReporter, Report};
use super::watchdog::StallWatchdog;
use super::{CaptureStats, DisplayId, DisplayInfo, Frame, FrameSource, FrameStream, PixelFormat};

/// Backoff bounds for stream recovery (F-007.3): quick first retry, capped so a
/// permanently missing display settles instead of spinning.
const RECOVERY_MIN_BACKOFF: Duration = Duration::from_millis(500);
const RECOVERY_MAX_BACKOFF: Duration = Duration::from_secs(5);

/// How often the supervisor samples the capture counters looking for a stall
/// (F-007.14). Comfortably longer than a frame interval at any sane `max_fps`,
/// so a healthy stream always shows progress between two polls.
const WATCHDOG_POLL: Duration = Duration::from_millis(500);

/// How long rejects must keep arriving with no delivered frame before the
/// stream is declared stalled. Long enough to ride out a hiccup, short enough
/// that a user watching a frozen desktop sees it come back (F-007.14).
const WATCHDOG_STALL_THRESHOLD: Duration = Duration::from_secs(3);

/// How many rejects must accumulate before the threshold means anything. Keeps
/// one transient bad sample on an idle desktop from triggering a rebuild, and
/// keeps the watchdog honest at `--max-fps 1` (F-007.14a).
const WATCHDOG_MIN_REJECTIONS: u64 = 10;

/// How many frames ScreenCaptureKit may buffer. Low, because we coalesce
/// anyway and a deep queue only adds latency.
const QUEUE_DEPTH: u32 = 3;

/// How long `start` waits for the first real frame before giving up and
/// leaving the "waiting" placeholder in place.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// How often the diagnostic counters are logged (F-002.18).
const STATS_INTERVAL: Duration = Duration::from_secs(10);

/// How long shutdown waits for the supervisor to stop the stream cleanly
/// (F-007.23).
///
/// Deliberately short, because Ctrl-C has to feel immediate. A healthy
/// `stop_capture` returns in milliseconds, so this only expires on a stream
/// that is already sick — and then the process is exiting anyway, which makes
/// macOS release whatever it was holding.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// How often a *continuing* recovery failure earns a warning (F-007.21).
///
/// Every attempt is logged; only one per interval is loud. A display asleep
/// for an hour would otherwise produce a warning every few seconds, which is
/// the flooding AC-7 forbids.
const FAILURE_REPORT_INTERVAL: Duration = Duration::from_secs(30);

/// Checks that ScreenCaptureKit calls cannot wait forever (F-007.19).
///
/// Since `screencapturekit` 11 every wait on a completion handler is bounded,
/// and `SCREENCAPTUREKIT_COMPLETION_TIMEOUT_SECS=0` turns that bound off. With
/// it off we are back to the failure this requirement exists for: a display
/// sleep parks recovery on a condvar for the rest of the process's life
/// (F-007 §8c). An invariant the whole design leans on is worth asserting once
/// rather than hoping for, so the server refuses to start without it.
fn ensure_bounded_waits() -> Result<Duration> {
    completion::default_timeout().ok_or_else(|| {
        anyhow!(
            "ScreenCaptureKit completion waits are unbounded because {}=0. Unset it, or give \
             it a number of seconds: with no bound, a display sleep freezes capture for good \
             (F-007.19).",
            completion::TIMEOUT_ENV_VAR
        )
    })
}

/// Runs one blocking ScreenCaptureKit call off the async runtime (F-007.20).
///
/// Every call that waits for a completion handler goes through here. They are
/// bounded now, but bounded is not instant: a sick stream can hold one for the
/// whole bound, and holding that on a runtime thread costs a worker, while
/// holding it on the main thread inside `block_on` stops the server dead with
/// nothing after "display available" in the log. Both were measured
/// (F-007 §8c), which is why this is a rule and not a preference.
async fn off_runtime<T>(what: &'static str, call: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T>
where
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(call).await {
        Ok(result) => result,
        Err(error) => Err(anyhow!("the blocking task for {what} did not finish: {error}")),
    }
}

/// Turns a ScreenCaptureKit error into one that says whether the bound expired.
///
/// "did not answer in time" and "refused" need different reactions from
/// whoever reads the log, and the crate distinguishes them only by a message
/// prefix.
fn describe(error: &SCError, doing: &str) -> anyhow::Error {
    let message = error.to_string();
    if completion::is_timeout_error(&message) {
        anyhow!("ScreenCaptureKit did not answer while {doing} ({message}); is the display asleep?")
    } else {
        anyhow!("{message}")
    }
}

/// Why a sample buffer could not become a [`Frame`].
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("sample buffer carries no image buffer")]
    NoImageBuffer,
    #[error("planar pixel buffers are not supported (got {planes} planes)")]
    Planar { planes: usize },
    #[error("unexpected pixel format {actual} (expected BGRA); reading it as BGRA would be garbage")]
    UnexpectedFormat { actual: String },
    #[error("failed to lock the pixel buffer: CVReturn {0}")]
    Lock(i32),
    #[error("the locked pixel buffer exposed no contiguous bytes (null base address?)")]
    NoBaseAddress,
    #[error("inconsistent geometry: {width}x{height}, bytes_per_row {bytes_per_row}, buffer {len} bytes")]
    Geometry {
        width: usize,
        height: usize,
        bytes_per_row: usize,
        len: usize,
    },
}

/// Checks that a buffer's reported geometry is self-consistent before any
/// pointer arithmetic depends on it (F-002.19).
///
/// Pure, so it can be tested without a live capture session.
pub(crate) fn validate_geometry(
    width: usize,
    height: usize,
    bytes_per_row: usize,
    len: usize,
    bpp: usize,
) -> Result<(), FrameError> {
    let too_narrow = bytes_per_row < width.saturating_mul(bpp);
    let too_short = len < height.saturating_mul(bytes_per_row);
    if width == 0 || height == 0 || too_narrow || too_short {
        return Err(FrameError::Geometry {
            width,
            height,
            bytes_per_row,
            len,
        });
    }
    Ok(())
}

/// Whether a CoreVideo FourCC is a layout we can forward as-is.
///
/// Uses the binding's own FourCC mapping rather than a hand-written constant,
/// so the two cannot drift apart.
#[must_use]
pub(crate) fn is_supported_format(fourcc: u32) -> bool {
    ScPixelFormat::from(fourcc) == ScPixelFormat::BGRA
}

/// Converts one sample buffer into a [`Frame`].
///
/// Returns `Ok(None)` for frames that carry no new content — ScreenCaptureKit
/// emits `Idle`/`Blank` frames that would otherwise be re-encoded and resent
/// for nothing.
fn frame_from_sample(sample: &CMSampleBuffer, sequence: u64) -> Result<Option<Frame>, FrameError> {
    if let Some(status) = sample.frame_status()
        && status != SCFrameStatus::Complete
    {
        trace!(?status, "skipping frame without new content");
        return Ok(None);
    }

    let buffer = sample.pixel_buffer().ok_or(FrameError::NoImageBuffer)?;

    if buffer.is_planar() {
        return Err(FrameError::Planar {
            planes: buffer.plane_count(),
        });
    }

    let fourcc = buffer.pixel_format();
    if !is_supported_format(fourcc) {
        return Err(FrameError::UnexpectedFormat {
            actual: ScPixelFormat::from(fourcc).to_string(),
        });
    }

    let width = buffer.width();
    let height = buffer.height();
    let stride = buffer.bytes_per_row();
    let bpp = PixelFormat::Bgra8.bytes_per_pixel();

    // Hold the lock for exactly one memcpy and nothing else (F-002.10):
    // the compositor cannot reuse this surface until we let go.
    let data = {
        let guard = buffer.lock_read_only().map_err(FrameError::Lock)?;
        // SAFETY: the guard keeps the base address mapped for as long as it
        // lives, and the buffer is non-planar (checked above), so its bytes are
        // one contiguous run of `data_size()`. `as_slice` hands back `None`
        // instead of a dangling slice when the base address is null, and that
        // case is handled right here rather than trusted away.
        //
        // This is the second unsafe site in the crate, after `perms`. It is not
        // a choice: `screencapturekit` 8.0.1 did this very read behind a safe
        // wrapper, and 11.0.0 moved the pixel buffer to `apple-cf`, which
        // exposes the bytes only through an unsafe fn. Recorded in ADR-0016.
        #[expect(
            unsafe_code,
            reason = "apple-cf 0.11 offers no safe path to the locked bytes; see ADR-0016"
        )]
        let slice = unsafe { guard.as_slice() }.ok_or(FrameError::NoBaseAddress)?;
        validate_geometry(width, height, stride, slice.len(), bpp)?;
        Bytes::copy_from_slice(slice)
    };

    Ok(Some(Frame {
        width: u32::try_from(width).unwrap_or(u32::MAX),
        height: u32::try_from(height).unwrap_or(u32::MAX),
        stride,
        format: PixelFormat::Bgra8,
        data,
        sequence,
        captured_at: Instant::now(),
    }))
}

/// Receives frames on ScreenCaptureKit's dispatch queue and publishes them.
///
/// Must not block: this runs on Apple's queue, and stalling it stalls capture
/// for the whole system (F-002.11). Publishing to a `watch` channel is a lock
/// plus a move, and it coalesces by construction.
struct FrameHandler {
    tx: watch::Sender<Frame>,
    stats: Arc<CaptureStats>,
}

impl SCStreamOutputTrait for FrameHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if of_type != SCStreamOutputType::Screen {
            return;
        }

        let sequence = self.stats.next_sequence();
        match frame_from_sample(&sample, sequence) {
            Ok(Some(frame)) => {
                self.stats.record_received(frame.data.len() as u64);
                // Fails only once every consumer is gone, which is normal at
                // shutdown and not worth a log line per frame.
                let _ = self.tx.send(frame);
            }
            Ok(None) => self.stats.record_skipped(),
            Err(error) => {
                self.stats.record_rejected();
                // Rate-limited: a wrong pixel format is wrong for every frame,
                // and 60 identical lines per second helps nobody.
                if self.stats.should_log_rejection() {
                    warn!(%error, "dropping frame");
                }
            }
        }
    }
}

/// Watches one `SCStream` for unexpected death and signals the supervisor.
///
/// `did_stop_with_error` is ScreenCaptureKit's only stop notification (F-007.1)
/// and fires on the framework's own thread, so it MUST NOT block: it only sends
/// on an unbounded channel and returns. A *clean* `stop_capture` we requested is
/// deliberately not reported here, so our own shutdown never triggers recovery.
struct StreamDelegate {
    death_tx: mpsc::UnboundedSender<String>,
}

impl SCStreamDelegateTrait for StreamDelegate {
    fn did_stop_with_error(&self, error: SCError) {
        let _ = self.death_tx.send(error.to_string());
    }
}

/// Captures a display with ScreenCaptureKit.
pub struct ScreenCaptureKitSource {
    displays: Vec<DisplayInfo>,
    selected: DisplayInfo,
    filter: Arc<SCContentFilter>,
    max_fps: u32,
    /// Geometry actually observed on the first frame; falls back to the
    /// configured estimate until one arrives.
    desktop: (u32, u32),
    stats: Arc<CaptureStats>,
    stats_task: Option<JoinHandle<()>>,
    /// Owns the live `SCStream` and rebuilds it after an unexpected stop
    /// (F-007.2). `None` until [`FrameSource::start`] runs.
    supervisor: Option<JoinHandle<()>>,
    /// Tells the supervisor to stop the stream cleanly and exit.
    stop_tx: Option<oneshot::Sender<()>>,
}

impl ScreenCaptureKitSource {
    /// Enumerates displays, picks one, and works out its true pixel geometry.
    ///
    /// Does not start capturing — that is [`FrameSource::start`].
    pub async fn new(display_index: u32, max_fps: u32) -> Result<Self> {
        let bound = ensure_bounded_waits()?;
        info!(
            bound_s = bound.as_secs(),
            env_var = completion::TIMEOUT_ENV_VAR,
            "ScreenCaptureKit completion waits are bounded"
        );

        let found = off_runtime("listing shareable content", move || enumerate(display_index)).await?;

        Ok(Self {
            desktop: (found.selected.width_px, found.selected.height_px),
            displays: found.displays,
            selected: found.selected,
            filter: Arc::new(found.filter),
            max_fps,
            stats: Arc::new(CaptureStats::default()),
            stats_task: None,
            supervisor: None,
            stop_tx: None,
        })
    }
}

/// What [`enumerate`] found: every display, the chosen one, and its filter.
struct Enumerated {
    displays: Vec<DisplayInfo>,
    selected: DisplayInfo,
    filter: SCContentFilter,
}

/// Enumerates displays and builds the content filter for the chosen one.
///
/// Blocking, because `SCShareableContent::get` waits for a completion handler,
/// so it is only ever reached through [`off_runtime`] (F-007.20).
fn enumerate(display_index: u32) -> Result<Enumerated> {
    let content = SCShareableContent::get()
        .map_err(|e| anyhow!("{e}"))
        .context("listing shareable content (is Screen Recording permitted?)")?;

    let sc_displays = content.displays();
    if sc_displays.is_empty() {
        bail!("ScreenCaptureKit reports no displays");
    }

    let index = usize::try_from(display_index).unwrap_or(usize::MAX);
    let Some(sc_display) = sc_displays.get(index) else {
        bail!(
            "display index {display_index} is out of range; available displays: {}",
            describe_displays(&sc_displays)
        );
    };

    let filter = SCContentFilter::create()
        .with_display(sc_display)
        .with_excluding_windows(&[])
        .build()
        .map_err(|e| anyhow!("{e}"))
        .context("building the content filter for the selected display")?;

    let selected = display_info(Some(&filter), sc_display);

    let displays = sc_displays
        .iter()
        .map(|d| {
            // A display whose filter will not build is still listed, with
            // its size read from the display alone. Dropping it would shift
            // every later index and point `--display` at another screen
            // without saying so.
            match SCContentFilter::create()
                .with_display(d)
                .with_excluding_windows(&[])
                .build()
            {
                Ok(f) => display_info(Some(&f), d),
                Err(error) => {
                    warn!(
                        %error,
                        id = d.display_id(),
                        "no content filter for this display; reporting its size in points"
                    );
                    display_info(None, d)
                }
            }
        })
        .collect();

    Ok(Enumerated {
        displays,
        selected,
        filter,
    })
}

impl ScreenCaptureKitSource {
    #[must_use]
    pub fn stats(&self) -> Arc<CaptureStats> {
        Arc::clone(&self.stats)
    }
}

/// Builds a [`DisplayInfo`] for one display, including its point-space origin.
///
/// The origin comes from `SCDisplay::frame()` (in points) — needed so input can
/// map desktop pixels onto the right display in a multi-monitor layout (F-003.6).
fn display_info(filter: Option<&SCContentFilter>, display: &SCDisplay) -> DisplayInfo {
    let (width_px, height_px, scale_factor) = pixel_geometry(filter, display);
    let frame = display.frame();
    DisplayInfo {
        id: DisplayId(display.display_id()),
        width_px,
        height_px,
        scale_factor,
        origin_x: frame.origin.x,
        origin_y: frame.origin.y,
        name: format!("Display {}", display.display_id()),
    }
}

/// Resolves a display's backing-store pixel size and scale factor.
///
/// `SCDisplay::width`/`height` are in **points**, so on a Retina panel they are
/// half the pixel count. `SCShareableContentInfo` gives the authoritative pixel
/// size; if it is unavailable we fall back to points multiplied by the filter's
/// scale factor.
fn pixel_geometry(filter: Option<&SCContentFilter>, display: &SCDisplay) -> (u32, u32, f64) {
    let scale = filter.map_or(1.0, |f| f64::from(f.point_pixel_scale()));
    let scale = if scale > 0.0 { scale } else { 1.0 };

    if let Some(filter) = filter
        && let Some(info) = SCShareableContentInfo::for_filter(filter)
    {
        let (w, h) = info.pixel_size();
        if w > 0 && h > 0 {
            return (w, h, f64::from(info.point_pixel_scale()));
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "display sizes are small and positive"
    )]
    let width = (f64::from(display.width()) * scale) as u32;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "display sizes are small and positive"
    )]
    let height = (f64::from(display.height()) * scale) as u32;
    (width, height, scale)
}

fn describe_displays(displays: &[SCDisplay]) -> String {
    displays
        .iter()
        .enumerate()
        .map(|(i, d)| format!("[{i}] id={} {}x{} points", d.display_id(), d.width(), d.height()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[async_trait::async_trait]
impl FrameSource for ScreenCaptureKitSource {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(self.displays.clone())
    }

    fn desktop_size(&self) -> (u32, u32) {
        self.desktop
    }

    async fn start(&mut self) -> Result<FrameStream> {
        let (width, height) = (self.selected.width_px, self.selected.height_px);
        let config = build_config(width, height, self.max_fps);

        // The stream needs somewhere to publish before it starts, so the channel
        // opens on a placeholder. If the first real frame never arrives, that
        // placeholder is what the client sees — which is the point.
        let (tx, rx) = watch::channel(placeholder::render(width, height, Placeholder::WaitingForFirstFrame));
        let (death_tx, death_rx) = mpsc::unbounded_channel();

        // F-007.20/F-007.22: the start waits for a completion handler, so it
        // goes to a blocking thread. It is bounded, so a sleeping display now
        // costs an error after the bound instead of a server that hangs with
        // nothing in the log.
        let filter = Arc::clone(&self.filter);
        let stats = Arc::clone(&self.stats);
        let handler_tx = tx.clone();
        let handler_death_tx = death_tx.clone();
        let stream = off_runtime("starting the capture stream", move || {
            build_stream(&filter, &config, handler_tx, stats, handler_death_tx)
        })
        .await
        .context("starting the ScreenCaptureKit stream")?;

        info!(
            display = %self.selected.name,
            requested_width = width,
            requested_height = height,
            scale_factor = self.selected.scale_factor,
            fps = self.max_fps,
            "capture started"
        );

        self.await_first_frame(rx.clone()).await;
        self.spawn_stats_task();

        // The supervisor owns the stream from here on: it stops it cleanly on
        // shutdown and rebuilds it after an unexpected death (F-007.2).
        let (stop_tx, stop_rx) = oneshot::channel();
        self.supervisor = Some(tokio::spawn(supervise(SupervisorParams {
            display_id: self.selected.id,
            desktop: self.desktop,
            max_fps: self.max_fps,
            tx,
            stats: Arc::clone(&self.stats),
            death_tx,
            death_rx,
            initial: stream,
            stop_rx,
        })));
        self.stop_tx = Some(stop_tx);

        Ok(FrameStream::new(rx).with_stats(Arc::clone(&self.stats)))
    }

    async fn stop(&mut self) -> Result<()> {
        if let Some(task) = self.stats_task.take() {
            task.abort();
        }
        // Ask the supervisor to stop the stream cleanly, then wait for it: only
        // an explicit `stop_capture` makes the macOS recording indicator go out
        // (AC-9), and it happens inside the supervisor task.
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(supervisor) = self.supervisor.take() {
            match tokio::time::timeout(SHUTDOWN_GRACE, supervisor).await {
                Ok(Ok(())) => info!("capture stopped"),
                Ok(Err(error)) => warn!(?error, "capture supervisor ended abnormally"),
                // F-007.23: this used to mean the supervisor was wedged on an
                // unbounded `stop_capture` and would stay wedged for ever.
                // Waits are bounded now, so it means the stream was too sick to
                // stop inside a deadline chosen for a responsive Ctrl-C. The
                // process is exiting, which releases it either way; the
                // recording indicator may linger a moment.
                Err(_) => warn!(
                    grace_s = SHUTDOWN_GRACE.as_secs(),
                    timed_out_calls = completion::timed_out_context_count(),
                    "the capture stream did not stop within the shutdown grace period; exiting anyway"
                ),
            }
        }
        Ok(())
    }
}

/// Builds the stream configuration shared by the initial start and every
/// recovery rebuild, so the two cannot drift apart.
fn build_config(width: u32, height: u32, max_fps: u32) -> SCStreamConfiguration {
    SCStreamConfiguration::new()
        .with_width(width)
        .with_height(height)
        // Explicit even though the binding pins it: on macOS 26 the system
        // default is bi-planar YCbCr.
        .with_pixel_format(ScPixelFormat::BGRA)
        .with_fps(max_fps.max(1))
        .with_queue_depth(QUEUE_DEPTH)
        // Wave 1 burns the cursor into the frame; F-005 replaces it with a
        // real RDP pointer and turns this off.
        .with_shows_cursor(true)
}

/// Builds a delegated stream, attaches the frame handler, and starts it.
///
/// The delegate carries `death_tx` so an unexpected stop reaches the supervisor;
/// the handler publishes to `tx`, the same channel across rebuilds so the client
/// never has to reconnect (F-007.2).
fn build_stream(
    filter: &SCContentFilter,
    config: &SCStreamConfiguration,
    tx: watch::Sender<Frame>,
    stats: Arc<CaptureStats>,
    death_tx: mpsc::UnboundedSender<String>,
) -> Result<SCStream> {
    let mut stream = SCStream::new_with_delegate(filter, config, StreamDelegate { death_tx })
        .map_err(|e| anyhow!("{e}"))
        .context("creating the capture stream")?;
    stream
        .add_output_handler(FrameHandler { tx, stats }, SCStreamOutputType::Screen)
        .map_err(|e| anyhow!("{e}"))
        .context("attaching the frame handler to the capture stream")?;
    stream.start_capture().map_err(|e| describe(&e, "starting capture"))?;
    Ok(stream)
}

/// Everything the supervisor task needs; grouped to keep the spawn readable and
/// under Clippy's argument-count limit.
struct SupervisorParams {
    display_id: DisplayId,
    desktop: (u32, u32),
    max_fps: u32,
    tx: watch::Sender<Frame>,
    stats: Arc<CaptureStats>,
    death_tx: mpsc::UnboundedSender<String>,
    death_rx: mpsc::UnboundedReceiver<String>,
    initial: SCStream,
    stop_rx: oneshot::Receiver<()>,
}

/// Why the supervisor decided the current stream has to be rebuilt.
///
/// Two triggers, because ScreenCaptureKit has two ways of failing (F-007 §8).
enum Trigger {
    /// `did_stop_with_error`: the framework told us the stream stopped.
    Reported(String),
    /// The stream never stopped but produces only unusable samples — the
    /// failure mode that opening the laptop lid actually causes (F-007.14).
    Stalled(Duration),
}

/// Owns the live stream, rebuilding it after an unexpected stop until asked to
/// quit (F-007.2, F-007.8). Runs as its own task so a rebuild never blocks the
/// server loop or ScreenCaptureKit's delivery thread.
async fn supervise(params: SupervisorParams) {
    let SupervisorParams {
        display_id,
        desktop,
        max_fps,
        tx,
        stats,
        death_tx,
        mut death_rx,
        initial,
        mut stop_rx,
    } = params;

    let mut current = initial;
    let mut backoff = Backoff::new(RECOVERY_MIN_BACKOFF, RECOVERY_MAX_BACKOFF);
    let mut reporter = FailureReporter::new(FAILURE_REPORT_INTERVAL);
    let mut watchdog = StallWatchdog::new(WATCHDOG_STALL_THRESHOLD, WATCHDOG_MIN_REJECTIONS);

    let mut ticker = tokio::time::interval(WATCHDOG_POLL);
    // `interval` yields its first tick immediately; spend it here so the loop
    // starts with a real wait. Delay (not Burst) on missed ticks: a recovery
    // that takes seconds must not be followed by a flurry of catch-up polls.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;

    loop {
        let trigger = tokio::select! {
            _ = &mut stop_rx => break,
            death = death_rx.recv() => {
                let Some(reason) = death else { break };
                Trigger::Reported(reason)
            }
            _ = ticker.tick() => {
                match watchdog.observe(&stats.snapshot(), Instant::now()) {
                    Some(elapsed) => Trigger::Stalled(elapsed),
                    None => continue,
                }
            }
        };

        // F-007.16: say which trigger fired. Last time this failed, the absence
        // of a recovery line was the whole diagnosis — the two paths must stay
        // distinguishable in the log.
        match &trigger {
            Trigger::Reported(reason) => {
                warn!(%reason, display = display_id.0, "capture stream stopped unexpectedly; recovering");
            }
            Trigger::Stalled(elapsed) => warn!(
                display = display_id.0,
                stalled_for_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
                "capture stream is alive but delivering no usable frames (stale content filter?); recovering"
            ),
        }

        // Best effort on the old stream; a clean stop is not reported back to
        // the delegate, so this cannot loop us.
        stop_stream(&current, "before rebuilding").await;
        match recover(
            display_id,
            desktop,
            max_fps,
            &tx,
            &stats,
            &death_tx,
            &mut backoff,
            &mut reporter,
            &mut death_rx,
            &mut stop_rx,
        )
        .await
        {
            Some(stream) => {
                current = stream;
                backoff.reset();
                // A failure after a healed one is a new fault, so the next one
                // is loud again instead of being swallowed as a repeat.
                reporter.reset();
                // The counters are shared across streams, so the new one must
                // be judged on its own evidence.
                watchdog.reset();
                // F-007.18: so the next failure is visible from its first frame
                // rather than from its thousandth.
                stats.reset_rejection_log();
                info!(display = display_id.0, "capture recovered");
            }
            None => break,
        }
    }

    // Whatever stream is current, stop it so the recording indicator goes out.
    stop_stream(&current, "at shutdown").await;
}

/// Stops a stream off the async runtime, reporting rather than propagating.
///
/// `stop_capture` waits for a completion handler too, and that was the other
/// half of the freeze: the supervisor's own clean-up blocked before recovery
/// could even begin (F-007.20, F-007.23). Failing to stop a stream we are
/// about to replace is not fatal, so this logs and returns.
async fn stop_stream(stream: &SCStream, when: &'static str) {
    let handle = stream.clone();
    if let Err(error) = off_runtime("stopping the capture stream", move || {
        handle.stop_capture().map_err(|e| describe(&e, "stopping capture"))
    })
    .await
    {
        warn!(
            %error,
            when,
            timed_out_calls = completion::timed_out_context_count(),
            "could not stop the capture stream cleanly"
        );
    }
}

/// Retries rebuilding the stream with backoff until it starts again, or returns
/// `None` if a stop was requested mid-recovery.
#[expect(
    clippy::too_many_arguments,
    reason = "recovery state threaded through by reference; grouping it would not clarify"
)]
async fn recover(
    display_id: DisplayId,
    desktop: (u32, u32),
    max_fps: u32,
    tx: &watch::Sender<Frame>,
    stats: &Arc<CaptureStats>,
    death_tx: &mpsc::UnboundedSender<String>,
    backoff: &mut Backoff,
    reporter: &mut FailureReporter,
    death_rx: &mut mpsc::UnboundedReceiver<String>,
    stop_rx: &mut oneshot::Receiver<()>,
) -> Option<SCStream> {
    loop {
        let delay = backoff.next_delay();
        tokio::select! {
            _ = &mut *stop_rx => return None,
            () = tokio::time::sleep(delay) => {}
        }

        // Drain death signals queued while the old stream was dying, so a
        // backlog does not immediately re-trigger recovery on the new one.
        while death_rx.try_recv().is_ok() {}

        // F-007.20: the rebuild re-enumerates content and starts a stream,
        // both of which wait for completion handlers. Run on a runtime thread
        // this is what wedged the supervisor for the life of the process.
        let attempt = {
            let tx = tx.clone();
            let stats = Arc::clone(stats);
            let death_tx = death_tx.clone();
            off_runtime("rebuilding the capture stream", move || {
                try_rebuild(display_id, desktop, max_fps, tx, stats, death_tx)
            })
            .await
        };

        match attempt {
            Ok(stream) => return Some(stream),
            // F-007.21: every attempt is reported. Only the first, and then one
            // per interval, is loud — a display asleep for an hour must not
            // bury the log (AC-7), but it must not be silent either, which is
            // exactly how the freeze stayed invisible (F-007 §8c).
            Err(error) => {
                let delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX);
                match reporter.observe(Instant::now()) {
                    Report::Loud { suppressed } => warn!(
                        %error,
                        display = display_id.0,
                        delay_ms,
                        suppressed_since_last = suppressed,
                        timed_out_calls = completion::timed_out_context_count(),
                        "recovery attempt failed; backing off"
                    ),
                    Report::Quiet => debug!(
                        %error,
                        display = display_id.0,
                        delay_ms,
                        "recovery attempt failed again; backing off"
                    ),
                }
            }
        }
    }
}

/// Re-enumerates shareable content, finds the same display by id, and rebuilds
/// the stream (F-007.2). Fails if the display is not currently present — the
/// caller keeps retrying with backoff until it returns (switching to a different
/// display is deferred to F-006, see F-007.4).
fn try_rebuild(
    display_id: DisplayId,
    desktop: (u32, u32),
    max_fps: u32,
    tx: watch::Sender<Frame>,
    stats: Arc<CaptureStats>,
    death_tx: mpsc::UnboundedSender<String>,
) -> Result<SCStream> {
    let content = SCShareableContent::get()
        .map_err(|e| anyhow!("{e}"))
        .context("re-enumerating shareable content during recovery")?;

    let displays = content.displays();
    let display = displays
        .iter()
        .find(|d| d.display_id() == display_id.0)
        .ok_or_else(|| anyhow!("display {} is not currently available", display_id.0))?;

    let filter = SCContentFilter::create()
        .with_display(display)
        .with_excluding_windows(&[])
        .build()
        .map_err(|e| anyhow!("{e}"))
        .context("building the content filter during recovery")?;

    let (width, height, _scale) = pixel_geometry(Some(&filter), display);
    if (width, height) != desktop {
        // The negotiated desktop size is fixed until F-006 wires up dynamic
        // resize; a changed resolution would need scaling/cropping (F-007.6).
        warn!(
            observed = ?(width, height),
            negotiated = ?desktop,
            "recovered display geometry differs from the negotiated desktop; sending at the new size (resize is F-006)"
        );
    }

    let config = build_config(width, height, max_fps);
    build_stream(&filter, &config, tx, stats, death_tx)
}

impl ScreenCaptureKitSource {
    /// Waits for the first real frame and adopts its geometry.
    ///
    /// The desktop size we advertise over RDP must match the pixels we actually
    /// send. Trusting the *requested* size would put a points-vs-pixels mistake
    /// on screen as a sheared or cropped image; taking it from a real frame
    /// cannot be wrong.
    async fn await_first_frame(&mut self, mut rx: watch::Receiver<Frame>) {
        match tokio::time::timeout(FIRST_FRAME_TIMEOUT, rx.changed()).await {
            Ok(Ok(())) => {
                let frame = rx.borrow_and_update();
                let observed = (frame.width, frame.height);
                if observed != self.desktop {
                    warn!(
                        requested = ?self.desktop,
                        observed = ?observed,
                        "ScreenCaptureKit delivered a different size than requested; using the observed one"
                    );
                }
                self.desktop = observed;
                info!(
                    width = frame.width,
                    height = frame.height,
                    stride = frame.stride,
                    "first frame received"
                );
            }
            Ok(Err(_)) => warn!("frame channel closed before the first frame arrived"),
            Err(_) => warn!(
                timeout_s = FIRST_FRAME_TIMEOUT.as_secs(),
                "no frame arrived in time; the client will see the waiting placeholder"
            ),
        }
    }

    fn spawn_stats_task(&mut self) {
        let stats = Arc::clone(&self.stats);
        self.stats_task = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(STATS_INTERVAL);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let snapshot = stats.snapshot();
                debug!(
                    received = snapshot.received,
                    delivered = snapshot.delivered,
                    coalesced = snapshot.coalesced,
                    skipped_idle = snapshot.skipped,
                    rejected = snapshot.rejected,
                    avg_frame_bytes = snapshot.average_frame_bytes(),
                    "capture stats"
                );
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BPP: usize = 4;

    #[test]
    fn geometry_accepts_a_padded_buffer() {
        // The realistic case: rows padded well beyond width * 4.
        assert!(validate_geometry(1920, 1080, 7808, 1080 * 7808, BPP).is_ok());
    }

    #[test]
    fn geometry_accepts_an_exactly_packed_buffer() {
        assert!(validate_geometry(640, 480, 640 * 4, 480 * 640 * 4, BPP).is_ok());
    }

    #[test]
    fn geometry_rejects_a_stride_narrower_than_a_row() {
        // Would read past the end of every row.
        assert!(validate_geometry(1920, 1080, 1920 * 3, 1080 * 1920 * 3, BPP).is_err());
    }

    #[test]
    fn geometry_rejects_a_truncated_buffer() {
        assert!(validate_geometry(640, 480, 2560, 480 * 2560 - 1, BPP).is_err());
    }

    #[test]
    fn geometry_rejects_empty_dimensions() {
        assert!(validate_geometry(0, 480, 2560, 1 << 20, BPP).is_err());
        assert!(validate_geometry(640, 0, 2560, 1 << 20, BPP).is_err());
    }

    #[test]
    fn only_bgra_is_accepted() {
        // `kCVPixelFormatType_32BGRA` is the FourCC 'BGRA', big-endian packed.
        let bgra = u32::from_be_bytes(*b"BGRA");
        assert!(is_supported_format(bgra), "BGRA must be accepted");

        // The formats macOS actually hands out instead when the pin is missing.
        for rejected in [b"420v", b"420f", b"l10r", b"xf44"] {
            let fourcc = u32::from_be_bytes(*rejected);
            assert!(
                !is_supported_format(fourcc),
                "{} must be rejected, not read as BGRA",
                String::from_utf8_lossy(rejected)
            );
        }
    }
}
