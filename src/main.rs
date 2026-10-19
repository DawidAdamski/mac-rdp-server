//! `mac-rdp-server` — RDP server for macOS.
//!
//! Wave 2 (F-003): serves the live Mac desktop over TLS and injects client
//! mouse and keyboard input via CGEvent. See `specs/` for the full plan.

use anyhow::{Context as _, Result};
use clap::Parser as _;
use tracing::{error, info, warn};

use mac_rdp_server::capture::placeholder::{Placeholder, PlaceholderSource};
use mac_rdp_server::capture::screen::ScreenCaptureKitSource;
use mac_rdp_server::capture::test_pattern;
use mac_rdp_server::capture::{DisplayId, FrameSource, FrameStream};
use mac_rdp_server::config::{Cli, Config};
use mac_rdp_server::input::cgevent::CgEventSink;
use mac_rdp_server::input::logging::LoggingSink;
use mac_rdp_server::input::state::ScreenGeometry;
use mac_rdp_server::perms::{Permission, PermissionState};
use mac_rdp_server::{capture, input, perms, session, tls};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // F-110.15: before the configuration, the certificate or the capture stack,
    // because the question this answers is "is the server allowed to run at
    // all" and the answer must not depend on anything else being right.
    if cli.check_permissions {
        let readings = perms::survey();
        print!("{}", perms::report(&readings));
        if !perms::all_granted(&readings) {
            std::process::exit(1);
        }
        return Ok(());
    }

    let config = Config::from_cli(&cli)?;
    init_logging(&config.log_level)?;

    info!(
        version = env!("CARGO_PKG_VERSION"),
        addr = %config.socket_addr(),
        cert = %config.cert_path.display(),
        display = config.display,
        max_fps = config.max_fps,
        "starting mac-rdp-server"
    );

    tls::ensure_self_signed(&config.cert_path, &config.key_path).context("preparing the TLS certificate")?;

    let mut source = build_source(&config).await?;
    log_displays(source.as_ref());

    let geometry = selected_geometry(source.as_ref(), config.display);

    // F-007.22: this used to be able to hang for ever on a sleeping display,
    // with the log stopping after "display available". The wait is bounded now,
    // so the worst case is a non-zero exit with a message that names the cause.
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

    let result = serve(&config, stream, width, height, dispatcher).await;

    if let Some(watcher) = geometry_watcher {
        watcher.abort();
    }

    if let Err(error) = source.stop().await {
        warn!(?error, "failed to stop the frame source cleanly");
    }

    if let Err(ref error) = result {
        error!(?error, "server stopped with an error");
    }
    result
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
/// ScreenCaptureKit, and the translation happens here — the same way `main`
/// already wires `FrameStream` to the session (F-007.17a).
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

async fn serve(
    config: &Config,
    stream: FrameStream,
    width: u32,
    height: u32,
    dispatcher: input::InputDispatcher,
) -> Result<()> {
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
    });

    tokio::select! {
        result = server => result,
        signal = tokio::signal::ctrl_c() => {
            signal.context("waiting for Ctrl-C")?;
            info!("interrupted, shutting down");
            Ok(())
        }
    }
}

fn init_logging(level: &str) -> Result<()> {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::prelude::*;

    // MAC_RDP_LOG wins over --log-level, so a stuck config file can always be
    // overridden from the shell (F-001.15).
    let filter = EnvFilter::builder()
        .with_default_directive(level.parse().context("invalid log level")?)
        .with_env_var("MAC_RDP_LOG")
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().compact())
        .with(filter)
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to install the tracing subscriber: {e}"))
}
