//! Keeping the Mac's display awake while a client is connected (F-007.26).
//!
//! Measured on a real session (F-007 §8e): the display idled to sleep ten
//! minutes into a live RDP connection, ScreenCaptureKit lost the display, and
//! capture stayed dead for 10 minutes 50 seconds across 130 recovery attempts.
//! Recovery cannot fix that, because a display that does not exist cannot be
//! captured. A remote desktop server therefore has to stop the display from
//! sleeping while somebody is using the machine remotely and, by definition,
//! not touching it locally.
//!
//! This module knows macOS and knows nothing about RDP, the same way `capture`
//! does (`CLAUDE.md` §Granice). `session` publishes how many clients are
//! connected; what that means for power management is decided here.

use std::process::{Child, Command, Stdio};

use anyhow::{Context as _, Result};
use tokio::sync::watch;
use tracing::{info, warn};

/// Stops the display idling to sleep for as long as this value lives.
///
/// Implemented by spawning `caffeinate -d`, not by an IOKit assertion
/// (owner's decision 2026-10-19): no `unsafe`, no new dependency, and the tool
/// ships with macOS. `-w <our pid>` makes the child exit when this process
/// does, so even a `kill -9` cannot leave the display pinned awake — which is
/// F-007.28, and the reason this is not simply `caffeinate -d` detached.
pub struct DisplayWakeLock {
    child: Child,
}

impl DisplayWakeLock {
    /// Takes the lock. Logs it, because "could the display have slept" must be
    /// answerable from the log rather than by experiment (F-007.29).
    pub fn hold() -> Result<Self> {
        let server_pid = std::process::id();
        let child = Command::new("/usr/bin/caffeinate")
            .args(["-d", "-w", &server_pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("spawning caffeinate to keep the display awake")?;

        info!(
            caffeinate_pid = child.id(),
            server_pid, "holding the display awake while a client is connected"
        );
        Ok(Self { child })
    }
}

impl Drop for DisplayWakeLock {
    fn drop(&mut self) {
        // `-w` already ends the child with this process; killing it here is
        // about releasing the display *early*, when the last client leaves, so
        // an unattended Mac can go back to sleeping.
        if let Err(error) = self.child.kill() {
            warn!(
                %error,
                "could not stop caffeinate; the display stays awake until this process exits"
            );
            return;
        }
        // Reap it, or the child lingers as a zombie for the life of the server.
        if let Err(error) = self.child.wait() {
            warn!(%error, "caffeinate was killed but could not be reaped");
        }
        info!("released the display; it may idle to sleep again");
    }
}

/// Holds the lock exactly while at least one client is connected (F-007.26).
///
/// Driven by a `watch` of the client count rather than by callbacks, so the
/// only thing crossing the boundary from `session` is a number.
///
/// A count that never comes back down would keep the display awake for the
/// life of the server. That is the harmless direction of the failure, and
/// `-w` still bounds it by the process.
pub async fn keep_display_awake(mut clients: watch::Receiver<usize>) {
    let mut lock: Option<DisplayWakeLock> = None;

    loop {
        let connected = *clients.borrow_and_update();
        match (connected, lock.is_some()) {
            (1.., false) => match DisplayWakeLock::hold() {
                Ok(held) => lock = Some(held),
                // Not fatal: the session still works, it just becomes
                // vulnerable to the display sleeping under it again.
                Err(error) => warn!(
                    %error,
                    "could not keep the display awake; a long session may freeze until the display wakes"
                ),
            },
            // Dropping the lock is what releases it, and what logs it.
            (0, true) => lock = None,
            _ => {}
        }

        if clients.changed().await.is_err() {
            // The server is gone, so the lock goes with it.
            break;
        }
    }
}
