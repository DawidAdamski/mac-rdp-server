//! `mac-rdp-server` — RDP server for macOS.
//!
//! Wave 1 (F-002): serves the live Mac desktop over TLS, with client input
//! logged rather than injected. See `specs/` for the full plan.

use anyhow::{Context as _, Result};
use clap::Parser as _;
use tracing::{error, info, warn};

use mac_rdp_server::capture::placeholder::{Placeholder, PlaceholderSource};
use mac_rdp_server::capture::screen::ScreenCaptureKitSource;
use mac_rdp_server::capture::test_pattern;
use mac_rdp_server::capture::{FrameSource, FrameStream};
use mac_rdp_server::config::{Cli, Config};
use mac_rdp_server::input::logging::LoggingSink;
use mac_rdp_server::perms::{Permission, PermissionState};
use mac_rdp_server::{input, perms, session, tls};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
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

    let mut source = build_source(&config)?;
    log_displays(source.as_ref());

    let stream = source.start().await.context("starting the frame source")?;
    // Read the size only after starting: ScreenCaptureKitSource corrects it
    // from the first real frame, which is the only size guaranteed to match
    // the pixels we actually send (F-002, AC-5).
    let (width, height) = source.desktop_size();
    info!(width, height, "desktop size");

    let dispatcher = input::spawn(Box::new(LoggingSink::new()), input::DEFAULT_QUEUE_CAPACITY);
    let result = serve(&config, stream, width, height, dispatcher).await;

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
fn build_source(config: &Config) -> Result<Box<dyn FrameSource>> {
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
    let source =
        ScreenCaptureKitSource::new(config.display, config.max_fps).context("initialising ScreenCaptureKit capture")?;
    Ok(Box::new(source))
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

    let server = session::run(session::SessionParams {
        bind_addr: config.socket_addr(),
        cert_path: &config.cert_path,
        key_path: &config.key_path,
        stream,
        desktop_width,
        desktop_height,
        dispatcher,
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
