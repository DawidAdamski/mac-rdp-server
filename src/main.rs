//! `mac-rdp-server` — RDP server for macOS.
//!
//! Serves the live Mac desktop over TLS and injects client mouse and keyboard
//! input via CGEvent. See `specs/` for the full plan.
//!
//! This file is the entry point and nothing more: which mode to run, where the
//! configuration came from, and who owns the main thread. The service itself
//! lives in [`service`], which exists because the menu bar has to start and
//! stop it many times in one process (F-111.10).

use anyhow::{Context as _, Result};
use clap::Parser as _;
use tokio::sync::oneshot;
use tracing::{error, info, warn};

use mac_rdp_server::config::{Cli, Config};
use mac_rdp_server::perms;
use mac_rdp_server::{auth, menubar, service};

fn main() -> Result<()> {
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

    let (config, config_source) = Config::from_cli(&cli)?;
    init_logging(&config.log_level)?;

    // F-101.8: a mode, not an option. It stores a password and exits; it never
    // starts a server, and it runs before anything that could prompt for a
    // permission or open the screen, because none of that is needed to write
    // one Keychain item.
    if cli.set_password {
        let Some(username) = config.auth_user.as_deref() else {
            anyhow::bail!(
                "--set-password needs to know whose password it is: pass --auth-user NAME, \
                 or put `auth_user` in the configuration file"
            );
        };
        return auth::set_password_interactively(username);
    }

    info!(
        version = env!("CARGO_PKG_VERSION"),
        // F-112.7: the values below are the result; this is where they came
        // from. Without it, "why is it listening there" needs an experiment.
        config = %config_source,
        addr = %config.socket_addr(),
        cert = %config.cert_path.display(),
        display = config.display,
        max_fps = config.max_fps,
        menu_bar = cli.menu_bar,
        // F-101.16: whether a password is required, and whose. Reported from
        // the configured user name rather than from a Keychain read, which is
        // the same answer: a configured user without a secret does not get
        // past `service::run` (F-101.11).
        auth = %if config.auth_user.is_some() { "nla" } else { "none" },
        user = %config.auth_user.as_deref().unwrap_or("-"),
        "starting mac-rdp-server"
    );

    // Menu-bar mode hands the main thread to AppKit and never gets it back, so
    // it cannot be reached from inside a runtime — which is also why this
    // function is no longer `#[tokio::main]` (ADR-0015, F-111.16). It starts
    // nothing: in that mode the service waits to be switched on (F-111.18),
    // and each run gets its own thread and runtime.
    if cli.menu_bar {
        return menubar::run(&config);
    }

    // CLI mode is unchanged from the outside, which is the requirement
    // (F-111.1, AC-3): one runtime, one service, running until interrupted.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;

    runtime.block_on(serve_until_interrupted(&config))
}

/// CLI mode: the service runs until it ends or `Ctrl-C` arrives (F-001.17).
///
/// The signal is handled here rather than inside [`service`] because what it
/// means depends on how the program was started: here it ends the process, and
/// in menu-bar mode it would have to end the application instead.
async fn serve_until_interrupted(config: &Config) -> Result<()> {
    let (stop, wait_for_stop) = oneshot::channel();

    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("interrupted, shutting down");
                let _ = stop.send(());
            }
            Err(error) => warn!(?error, "cannot watch for Ctrl-C; the server will run until it ends"),
        }
    });

    let result = service::run(config, wait_for_stop, || {}).await;

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
