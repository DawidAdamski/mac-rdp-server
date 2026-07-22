//! `mac-rdp-server` — RDP server for macOS.
//!
//! Wave 0 (F-001): TLS listener serving a synthetic test pattern, with client
//! input logged rather than injected. See `specs/` for the full plan.

use anyhow::{Context as _, Result};
use clap::Parser as _;
use tracing::{error, info};

use mac_rdp_server::capture::{FrameSource as _, test_pattern::TestPatternSource};
use mac_rdp_server::config::{Cli, Config};
use mac_rdp_server::input::logging::LoggingSink;
use mac_rdp_server::{input, session, tls};

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

    let mut source = TestPatternSource::default();
    // Not named `display`: `tracing` reserves that identifier for its own
    // formatting sigil and would try to call it as a function.
    for screen in source.displays().context("enumerating displays")? {
        info!(
            id = screen.id.0,
            name = %screen.name,
            width_px = screen.width_px,
            height_px = screen.height_px,
            scale_factor = screen.scale_factor,
            "display available"
        );
    }

    let (width, height) = source.desktop_size();
    let stream = source.start().await.context("starting the frame source")?;
    info!(width, height, "desktop size");

    let dispatcher = input::spawn(Box::new(LoggingSink::new()), input::DEFAULT_QUEUE_CAPACITY);

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

    let result = tokio::select! {
        result = server => result,
        signal = tokio::signal::ctrl_c() => {
            signal.context("waiting for Ctrl-C")?;
            info!("interrupted, shutting down");
            Ok(())
        }
    };

    source.stop().await.ok();

    if let Err(ref error) = result {
        error!(?error, "server stopped with an error");
    }
    result
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
