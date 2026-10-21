//! The RDP service as one startable and stoppable unit (F-111.10).
//!
//! Everything the server needs to serve — capture, input injection, the
//! display wake lock, the listening socket — is built here, run until asked to
//! stop, and torn down again. `main` used to do this once per process; the menu
//! bar needs it to happen many times in one process, which is the only reason
//! this module exists.
//!
//! Three things follow from F-111.18 ("a running app is not a running
//! service") and they shape the whole file:
//!
//! 1. **Nothing is built before the service is started.** Capture in
//!    particular: an open ScreenCaptureKit stream lights the system's
//!    screen-recording indicator, so a stopped service that still captures
//!    would be lying about access to the screen (F-111.19).
//! 2. **Startup failures are values, not exits.** In CLI mode a bad port ends
//!    the process; from a menu it has to become a line in that menu
//!    (F-111.11), so every failure comes back as `Err` for the caller to
//!    display.
//! 3. **Stopping is cooperative.** The stop signal cancels the session future,
//!    which drops the listener and the connection with it, and then the
//!    teardown below still gets to run — the same shape `Ctrl-C` has had since
//!    F-001.17, and the reason no `caffeinate` survives a shutdown (F-007.28).
//!
//! This module does **not** handle signals. Which signal means what depends on
//! how the program was started — in CLI mode `Ctrl-C` ends the process, in
//! menu-bar mode it ends the application — so that decision belongs to the
//! entry point, not here.

use std::net::SocketAddr;

use anyhow::{Context as _, Result};
use tokio::sync::oneshot;
use tracing::{info, warn};

use crate::capture::placeholder::{Placeholder, PlaceholderSource};
use crate::capture::screen::ScreenCaptureKitSource;
use crate::capture::test_pattern;
use crate::capture::{self, DisplayId, FrameSource, FrameStream};
use crate::config::Config;
use crate::input::cgevent::CgEventSink;
use crate::input::logging::LoggingSink;
use crate::input::state::ScreenGeometry;
use crate::perms::{Permission, PermissionState};
use crate::{auth, input, perms, power, session, tls};

/// Runs the service until `stop` fires, the session ends, or something fails.
///
/// `on_listening` is called once, from the moment the protocol engine is
/// **actually accepting connections** — not when it has been asked to start.
/// A caller that paints a state somewhere (the menu bar does) can trust it:
/// being told "listening" and then having a client refused is exactly what
/// F-111.11 forbids, and is what happened while this was reported a few
/// instructions too early (CI, 2026-10-21).
pub async fn run<F>(config: &Config, stop: oneshot::Receiver<()>, on_listening: F) -> Result<()>
where
    F: FnOnce() + Send + 'static,
{
    // F-101.10 comes before everything, including the bind. Refusing to put a
    // socket on a reachable address without a password is the point of the
    // requirement, and opening that socket for even a moment to find out the
    // port is free would be doing the thing we are refusing to do.
    //
    // Resolved here rather than once in `main` so that the Keychain is read on
    // every start. In menu-bar mode the service starts and stops many times in
    // one process, and a password changed in between should take effect at the
    // next start rather than at the next launch.
    let auth = auth::resolve(config.auth_user.as_deref(), config.bind_addr)?;

    // Checked next because it is the cheapest thing that can fail and the
    // likeliest: a second server, or a copy left running in another terminal.
    // Everything after this point costs a permission prompt or a capture
    // stream, which is a lot to spend before finding out the port is taken.
    preflight_bind(config.socket_addr()).await?;

    tls::ensure_self_signed(&config.cert_path, &config.key_path).context("preparing the TLS certificate")?;

    let mut source = build_source(config).await?;
    log_displays(source.as_ref());

    let geometry = selected_geometry(source.as_ref(), config.display);

    // F-007.22: this used to be able to hang for ever on a sleeping display,
    // with the log stopping after "display available". The wait is bounded now,
    // so the worst case is an error that names the cause.
    let stream = source.start().await.context(
        "starting the frame source (if ScreenCaptureKit did not answer, the display may be \
         asleep — see the known bug in README)",
    )?;
    // Read the size only after starting: ScreenCaptureKitSource corrects it
    // from the first real frame, which is the only size guaranteed to match
    // the pixels we actually send (F-002, AC-5).
    let (width, height) = source.desktop_size();
    info!(width, height, "desktop size");

    // The sink is built on the injection thread because `CGEventSource` is
    // `!Send` (ADR-0003) — hence a factory, not a value.
    let dispatcher = input::spawn(
        move || match CgEventSink::new(geometry) {
            Ok(sink) => Box::new(sink) as Box<dyn input::InputSink>,
            Err(error) => {
                warn!(?error, "falling back to logging input sink");
                Box::new(LoggingSink::new())
            }
        },
        input::DEFAULT_QUEUE_CAPACITY,
    );

    let geometry_watcher = selected_display_id(source.as_ref(), config.display)
        .map(|id| tokio::spawn(watch_geometry(id, dispatcher.clone())));

    // F-007.26. The second place where `capture`/`session` and a macOS concern
    // meet, and it meets them the same way `watch_geometry` does: `session`
    // publishes a number, `power` decides what it means. Measured reason:
    // the display slept ten minutes into a live session and killed capture for
    // eleven (F-007 §8e).
    let (clients_tx, clients_rx) = tokio::sync::watch::channel(0_usize);
    let display_keeper = tokio::spawn(power::keep_display_awake(clients_rx));

    let result = serve(
        config,
        stream,
        width,
        height,
        dispatcher,
        clients_tx,
        auth,
        stop,
        on_listening,
    )
    .await;

    // Abort before the stream stops, so the display is released while the log
    // is still being written rather than after the process is already gone.
    display_keeper.abort();
    if let Some(watcher) = geometry_watcher {
        watcher.abort();
    }

    if let Err(error) = source.stop().await {
        warn!(?error, "failed to stop the frame source cleanly");
    }

    result
}

/// Proves the address is free before anything expensive is built (F-111.11).
///
/// The protocol engine binds its own socket inside its run loop, so without
/// this the only way to learn that a port is taken is to start capture, open
/// the certificate, and then read an error — after the menu has already been
/// told the service is up.
///
/// The listener is closed again immediately, which leaves a gap in which
/// something else could take the port. That is a race worth accepting: the
/// alternative is reporting success and failing a moment later, and the engine
/// sets `SO_REUSEADDR`, so our own closed socket is never the obstacle.
async fn preflight_bind(addr: SocketAddr) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| {
        format!("cannot listen on {addr} (another server on that port, or an address this machine does not have?)")
    })?;
    drop(listener);
    Ok(())
}

/// Picks a frame source, degrading to an explanatory placeholder rather than
/// failing when Screen Recording is refused (F-002.13).
async fn build_source(config: &Config) -> Result<Box<dyn FrameSource>> {
    if let Some((width, height)) = config.test_pattern {
        warn!(
            width,
            height, "serving the synthetic test pattern instead of the screen"
        );
        return Ok(Box::new(test_pattern::TestPatternSource::new(width, height)));
    }

    if perms::ensure(Permission::ScreenRecording) == PermissionState::Denied {
        warn!("capturing the screen is not possible; serving the permission placeholder instead");
        return Ok(Box::new(PlaceholderSource::new(
            test_pattern::DEFAULT_WIDTH,
            test_pattern::DEFAULT_HEIGHT,
            Placeholder::PermissionDenied,
        )));
    }

    // Anything failing past this point is a real configuration or system error
    // — an out-of-range display index must be reported, not papered over.
    let source = ScreenCaptureKitSource::new(config.display, config.max_fps)
        .await
        .context("initialising ScreenCaptureKit capture")?;
    Ok(Box::new(source))
}

/// Picks the geometry of the display being captured, for input coordinate
/// mapping. Falls back to a 1:1 origin-zero mapping if enumeration fails.
fn selected_geometry(source: &dyn FrameSource, index: u32) -> ScreenGeometry {
    let displays = source.displays().unwrap_or_default();
    let chosen = displays.get(index as usize).or_else(|| displays.first());
    match chosen {
        Some(d) => ScreenGeometry::new(d.origin_x, d.origin_y, d.scale_factor),
        None => ScreenGeometry::new(0.0, 0.0, 1.0),
    }
}

/// Identifier of the display being captured, if it can be resolved.
fn selected_display_id(source: &dyn FrameSource, index: u32) -> Option<DisplayId> {
    let displays = source.displays().unwrap_or_default();
    displays.get(index as usize).or_else(|| displays.first()).map(|d| d.id)
}

/// Keeps the input sink's coordinate mapping in step with the display (F-007.17).
///
/// This task is the only place `capture` and `input` meet, which is the point:
/// `capture::geometry` publishes our own `DisplayInfo` and knows nothing about
/// input, the sink consumes a `ScreenGeometry` and knows nothing about
/// ScreenCaptureKit, and the translation happens here (F-007.17a).
///
/// It republishes **on every tick**, not only on change. A geometry update
/// travelling the bounded input queue can be dropped under a flood of mouse
/// moves, and an update sent once would then be lost for good, leaving the
/// mapping wrong until restart. Resending makes that self-correcting at the
/// cost of one event every couple of seconds.
async fn watch_geometry(id: DisplayId, mut dispatcher: input::InputDispatcher) {
    /// Display layout changes at human speed; polling faster buys nothing.
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

    let mut ticker = tokio::time::interval(INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Announce the first reading. A silent watcher and a broken one look
    // identical otherwise — nothing is logged until the geometry moves, and
    // that ambiguity is exactly what makes a failed verification hard to read.
    let mut announced = false;

    loop {
        ticker.tick().await;
        // `None` means the display is not currently active — the lid is shut or
        // the cable is out. Keep the last known mapping rather than steering
        // input through a null rectangle.
        if let Some(info) = capture::geometry::probe(id) {
            if !announced {
                announced = true;
                info!(
                    display = id.0,
                    origin_x = info.origin_x,
                    origin_y = info.origin_y,
                    scale = info.scale_factor,
                    interval_s = INTERVAL.as_secs(),
                    "watching the captured display for layout changes"
                );
            }
            let geometry = ScreenGeometry::new(info.origin_x, info.origin_y, info.scale_factor);
            dispatcher.send(input::InputEvent::Geometry(geometry));
        }
    }
}

fn log_displays(source: &dyn FrameSource) {
    match source.displays() {
        // Not named `display`: `tracing` reserves that identifier for its own
        // formatting sigil and would try to call it as a function.
        Ok(screens) => {
            for screen in screens {
                info!(
                    id = screen.id.0,
                    name = %screen.name,
                    width_px = screen.width_px,
                    height_px = screen.height_px,
                    scale_factor = screen.scale_factor,
                    "display available"
                );
            }
        }
        Err(error) => warn!(?error, "could not enumerate displays"),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the alternative is a parameter struct whose only job is to be unpacked here"
)]
async fn serve<F>(
    config: &Config,
    stream: FrameStream,
    width: u32,
    height: u32,
    dispatcher: input::InputDispatcher,
    clients: tokio::sync::watch::Sender<usize>,
    auth: auth::Mode,
    stop: oneshot::Receiver<()>,
    on_listening: F,
) -> Result<()>
where
    F: FnOnce() + Send + 'static,
{
    let desktop_width = u16::try_from(width).context("desktop width does not fit in u16")?;
    let desktop_height = u16::try_from(height).context("desktop height does not fit in u16")?;

    // F-004.9. The RDP bitmap encoder rejects widths that are not a multiple of
    // 4 (`ironrdp-server/src/encoder/bitmap.rs:30`). Every real display satisfies
    // this, so the assumption has been silent since wave 0 — say it out loud
    // here rather than let it surface as a bare "Width must be a multiple of 4"
    // from inside the protocol engine, with no hint of what to do about it.
    if !desktop_width.is_multiple_of(4) {
        anyhow::bail!(
            "the desktop is {desktop_width} px wide, which RDP bitmap encoding cannot carry — \
             it requires a multiple of 4. Pick another display with --display, or force a size \
             with --test-pattern WIDTHxHEIGHT to check the rest of the pipeline."
        );
    }

    let server = session::run(session::SessionParams {
        bind_addr: config.socket_addr(),
        cert_path: &config.cert_path,
        key_path: &config.key_path,
        stream,
        desktop_width,
        desktop_height,
        dispatcher,
        encoder: config.encoder,
        clients,
        auth,
        ready: Box::new(on_listening),
    });

    tokio::select! {
        result = server => result,
        // Dropping the session future closes the listening socket and the
        // connection on it, which is what "turn the service off" has to mean
        // for a client that is still connected (F-111.10).
        _ = stop => Ok(()),
    }
}
