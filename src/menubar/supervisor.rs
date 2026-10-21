//! The bridge between a click on the main thread and the running service.
//!
//! The menu runs on the main thread, which AppKit owns and which must never
//! block. So the two sides share exactly two things: a channel of commands
//! going one way, and a mutex-wrapped [`Toggle`] carrying state back the other.
//! Nothing here knows about AppKit, and nothing here knows about RDP.
//!
//! **Why threads and not tasks** (measured 2026-10-21, while implementing
//! F-111): the session future is not `Send` — the protocol engine holds `Rc`
//! across its awaits — so it cannot be handed to `tokio::spawn`. It has to be
//! the root of a `block_on`, which is exactly what it already is in CLI mode.
//! Each run of the service therefore gets its own thread with its own runtime,
//! and this supervisor is an ordinary thread reading an ordinary channel. That
//! also means a stopped service owns no runtime, no threads and no capture,
//! which is the shape F-111.18 and F-111.19 ask for anyway.

use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::oneshot;
use tracing::{error, info, warn};

use super::state::{Request, Toggle};
use crate::config::Config;
use crate::service;

/// How long [`Supervisor::quit`] waits for the service to come down before
/// letting the process exit anyway.
///
/// Generous enough for a capture stream to stop (hundreds of milliseconds,
/// F-007 §8f) and short enough that "Quit" cannot feel broken.
const QUIT_GRACE: Duration = Duration::from_secs(3);

enum Command {
    Start,
    Stop,
    /// Stop and finish. The acknowledgement is sent once the service is down,
    /// so that Quit from the menu ends as cleanly as `Ctrl-C` does in a
    /// terminal (F-111.8).
    Quit(SyncSender<()>),
}

/// The handle the menu holds. Safe to use from the main thread; never blocks,
/// except in [`Supervisor::quit`], where blocking is the point.
#[derive(Clone)]
pub struct Supervisor {
    commands: Sender<Command>,
}

impl Supervisor {
    /// Asks for a state change.
    pub fn request(&self, request: Request) {
        let command = match request {
            Request::Start => Command::Start,
            Request::Stop => Command::Stop,
        };
        if self.commands.send(command).is_err() {
            error!("the service supervisor is gone; the menu can no longer start or stop the server");
        }
    }

    /// Brings the service down and waits for it, within reason (F-111.8).
    ///
    /// Blocking is correct here and only here: the caller is quitting, so there
    /// is no interface left to keep responsive, and the point of waiting is
    /// that a connected client gets a closed socket rather than a process that
    /// vanished mid-frame.
    pub fn quit(&self) {
        let (ack, wait) = sync_channel(1);
        if self.commands.send(Command::Quit(ack)).is_err() {
            return;
        }
        if wait.recv_timeout(QUIT_GRACE).is_err() {
            error!(
                grace_s = QUIT_GRACE.as_secs(),
                "the service did not stop in time; exiting anyway"
            );
        }
    }
}

/// Starts the supervisor thread and returns the handle for the menu.
///
/// Does **not** start the service: in menu-bar mode that only ever happens
/// because someone asked for it (F-111.18).
pub fn spawn(config: Config, toggle: Arc<Mutex<Toggle>>) -> Supervisor {
    let (commands, receiver) = channel();

    let supervisor = std::thread::Builder::new()
        .name("service-supervisor".to_owned())
        .spawn(move || supervise(config, toggle, receiver));

    if let Err(ref error) = supervisor {
        // The icon would still appear and do nothing, which is worse than
        // saying so: F-111.11's rule about silent failure applies to the
        // machinery as much as to a busy port.
        error!(?error, "could not start the service supervisor thread");
    }

    Supervisor { commands }
}

/// Recovers a poisoned lock instead of propagating the panic.
///
/// A panic while this lock is held would otherwise take the menu with it:
/// every click, every repaint and every state report goes through here, so a
/// poisoned mutex means an icon that is still in the bar and no longer does
/// anything.
fn lock(toggle: &Mutex<Toggle>) -> MutexGuard<'_, Toggle> {
    toggle.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One running service: how to stop it, and how to wait for it.
struct Running {
    stop: oneshot::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

impl Running {
    /// Stops the service and waits for its thread to finish.
    ///
    /// Waiting is not optional: until the thread is gone the port is still
    /// open, so a Start that did not wait would race the socket it is about to
    /// bind (F-111.12).
    fn stop_and_join(self) {
        let _ = self.stop.send(());
        if self.thread.join().is_err() {
            error!("the service thread panicked");
        }
    }
}

fn supervise(config: Config, toggle: Arc<Mutex<Toggle>>, commands: Receiver<Command>) {
    let mut running: Option<Running> = None;

    while let Ok(command) = commands.recv() {
        match command {
            Command::Start => {
                if running.is_some() {
                    info!("the server is already running; ignoring the request to start it");
                    continue;
                }
                match start(&config, &toggle) {
                    Ok(started) => running = Some(started),
                    Err(error) => {
                        let reason = format!("{error:#}");
                        lock(&toggle).failed(reason.clone());
                        error!(%reason, "could not start the service thread");
                    }
                }
            }
            Command::Stop => match running.take() {
                Some(started) => {
                    info!("stopping the server, asked from the menu");
                    started.stop_and_join();
                }
                None => {
                    info!("the server is not running; ignoring the request to stop it");
                    lock(&toggle).stopped();
                }
            },
            Command::Quit(ack) => {
                match running.take() {
                    Some(started) => {
                        info!("quitting, asked from the menu; stopping the server first");
                        started.stop_and_join();
                    }
                    None => info!("quitting, asked from the menu"),
                }
                let _ = ack.send(());
                break;
            }
        }
    }
}

/// Starts one run of the service on its own thread with its own runtime.
fn start(config: &Config, toggle: &Arc<Mutex<Toggle>>) -> std::io::Result<Running> {
    let (stop, wait_for_stop) = oneshot::channel();
    let config = config.clone();
    let toggle = Arc::clone(toggle);

    let thread = std::thread::Builder::new()
        .name("rdp-service".to_owned())
        .spawn(move || run_once(config, toggle, wait_for_stop))?;

    Ok(Running { stop, thread })
}

/// One run of the service, from a click to its end, reporting into the toggle.
///
/// Every outcome is logged as well as shown, which is F-111.17: in menu-bar
/// mode there may be no terminal in sight, and the log is the only place where
/// a reason survives being clicked away.
fn run_once(config: Config, toggle: Arc<Mutex<Toggle>>, stop: oneshot::Receiver<()>) {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            let reason = format!("building the tokio runtime: {error}");
            lock(&toggle).failed(reason.clone());
            error!(%reason, "the server could not start");
            return;
        }
    };

    let announce = {
        let toggle = Arc::clone(&toggle);
        let addr = config.socket_addr();
        move || {
            lock(&toggle).listening();
            info!(%addr, "server started from the menu");
        }
    };

    match runtime.block_on(service::run(&config, stop, announce)) {
        Ok(()) => {
            lock(&toggle).stopped();
            info!("server stopped");
        }
        Err(error) => {
            // `{:#}` keeps the whole anyhow context chain — "cannot listen on
            // …: Address already in use" rather than only the outer half.
            let reason = format!("{error:#}");
            lock(&toggle).failed(reason.clone());
            error!(%reason, "the server could not run");
        }
    }

    // Dropped here rather than at the end of the function body for the sake of
    // saying why: this blocks until the runtime's own tasks are done, and doing
    // it before the thread ends is what makes "stopped" mean stopped.
    drop(runtime);
    warn_if_port_lingers(config.socket_addr());
}

/// Says so if the port is still taken after a stop.
///
/// F-111.12 is about ten cycles leaving nothing behind, and this is the cheap
/// half of that check: it runs on every stop rather than waiting for someone to
/// notice the eleventh start failing.
fn warn_if_port_lingers(addr: std::net::SocketAddr) {
    match std::net::TcpListener::bind(addr) {
        Ok(listener) => drop(listener),
        Err(error) => warn!(%addr, %error, "the address is still taken after stopping the server"),
    }
}
