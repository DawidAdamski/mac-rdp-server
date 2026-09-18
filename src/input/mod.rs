//! Input events and their destinations.
//!
//! Deliberately uses our own event types rather than `ironrdp-server`'s, so
//! that the RDP engine can be replaced without touching the injection code
//! (`specs/01-architecture.md` §1).

pub mod cgevent;
pub mod keymap;
pub mod logging;
pub mod state;
pub mod unicode;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    /// Mouse button 4 ("back" on most mice).
    Back,
    /// Mouse button 5 ("forward" on most mice).
    Forward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEvent {
    /// Absolute position, in pixels of the RDP desktop.
    MoveAbsolute {
        x: u32,
        y: u32,
    },
    MoveRelative {
        dx: i32,
        dy: i32,
    },
    Button {
        button: MouseButton,
        pressed: bool,
    },
    Scroll {
        dx: i32,
        dy: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    /// PC scancode set 1: `code` is the make code, `extended` the `E0` prefix.
    Scancode {
        code: u8,
        extended: bool,
        pressed: bool,
    },
    Unicode {
        ch: u16,
        pressed: bool,
    },
    /// Client-reported lock key state, sent on focus changes.
    SyncLocks {
        caps: bool,
        num: bool,
        scroll: bool,
    },
}

/// No `Eq`: [`state::ScreenGeometry`] carries floats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    Pointer(PointerEvent),
    Key(KeyEvent),
    /// The captured display moved or changed scale (F-007.17).
    ///
    /// Deliberately travels the **same channel** as pointer and key events
    /// rather than a side channel: coordinate mapping must change exactly
    /// between two events, never in the middle of a burst of moves, and one
    /// ordered queue gives that for free.
    Geometry(state::ScreenGeometry),
    /// An RDP connection ended or a new one is starting (F-003.24): release
    /// everything still held, leave the toggle keys alone.
    ///
    /// Same channel as the input for the same reason as `Geometry`: the reset
    /// must land after the last event of the old connection and before the
    /// first one of the new (F-003.23).
    SessionReset,
}

/// Destination for input events.
///
/// Implementations: [`logging::LoggingSink`] (wave 0),
/// [`cgevent::CgEventSink`] (wave 2, ADR-0003).
///
/// Note there is **no `Send` bound**. `CgEventSink` holds a `CGEventSource`,
/// which is `!Send`, so a sink is built and lives on the dedicated injection
/// thread (see [`spawn`]) and is never moved between threads.
pub trait InputSink {
    fn pointer(&mut self, ev: PointerEvent) -> Result<()>;
    fn key(&mut self, ev: KeyEvent) -> Result<()>;

    /// Adopts new geometry for the captured display (F-007.17).
    ///
    /// Defaults to ignoring it: only a sink that converts desktop pixels into
    /// screen coordinates has anything to do here.
    fn set_geometry(&mut self, geometry: state::ScreenGeometry) {
        let _ = geometry;
    }

    /// Releases everything held at the end of a connection (F-003.24).
    ///
    /// Defaults to nothing: only a sink that injects has state to release.
    fn reset_session(&mut self) {}
}

/// Default capacity of the channel between the RDP thread and the sink.
///
/// Large enough to absorb a burst of mouse moves, small enough that a stalled
/// sink is noticed rather than silently buffering.
pub const DEFAULT_QUEUE_CAPACITY: usize = 256;

/// Non-blocking front end to an [`InputSink`].
///
/// `RdpServerInputHandler::keyboard`/`mouse` are synchronous and run on the
/// server's connection loop, so they must never block: doing the actual
/// injection there would stall the whole session (`01-architecture.md` §4a).
/// This dispatcher only converts and hands off.
///
/// The channel is **bounded on purpose**: under a flood of events we drop and
/// warn rather than grow unboundedly.
///
/// `Clone`able so the geometry watcher can feed the same queue as the RDP
/// session (F-007.17). Each clone counts only its own drops.
#[derive(Clone)]
pub struct InputDispatcher {
    tx: mpsc::Sender<InputEvent>,
    dropped: u64,
}

impl InputDispatcher {
    #[must_use]
    pub fn new(tx: mpsc::Sender<InputEvent>) -> Self {
        Self { tx, dropped: 0 }
    }

    pub fn send(&mut self, event: InputEvent) {
        if self.tx.try_send(event).is_err() {
            self.dropped = self.dropped.saturating_add(1);
            // One line per drop would itself become the bottleneck under a flood.
            if self.dropped.is_power_of_two() {
                warn!(dropped = self.dropped, "input queue full, dropping events");
            }
        }
    }

    /// Number of events dropped because the queue was full.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// Spawns the injection worker and returns the dispatcher to hand to the RDP
/// session.
///
/// The sink is built by `make_sink` **on a dedicated OS thread**, not a Tokio
/// task, and stays there for its whole life. This is required, not stylistic:
/// `CGEventSource` is `!Send`, so it cannot be created on one thread and used
/// on another, and a Tokio task may migrate between worker threads. The thread
/// drains the bounded channel with `blocking_recv`.
///
/// `make_sink` runs on the worker thread, so a failure to acquire the injection
/// backend (e.g. no Accessibility) is handled inside the sink, not here.
pub fn spawn<F>(make_sink: F, capacity: usize) -> InputDispatcher
where
    F: FnOnce() -> Box<dyn InputSink> + Send + 'static,
{
    let (tx, mut rx) = mpsc::channel(capacity);

    std::thread::Builder::new()
        .name("input-injection".to_owned())
        .spawn(move || {
            let mut sink = make_sink();
            while let Some(event) = rx.blocking_recv() {
                let result = match event {
                    InputEvent::Pointer(ev) => sink.pointer(ev),
                    InputEvent::Key(ev) => sink.key(ev),
                    InputEvent::Geometry(geometry) => {
                        sink.set_geometry(geometry);
                        Ok(())
                    }
                    InputEvent::SessionReset => {
                        sink.reset_session();
                        Ok(())
                    }
                };
                if let Err(error) = result {
                    warn!(?error, "input sink rejected an event");
                }
            }
            info!("input queue closed");
        })
        .expect("spawn input-injection thread");

    InputDispatcher::new(tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn full_queue_drops_instead_of_blocking() {
        let (tx, _rx) = mpsc::channel(2);
        let mut dispatcher = InputDispatcher::new(tx);

        for _ in 0..10 {
            dispatcher.send(InputEvent::Key(KeyEvent::Scancode {
                code: 0x1E,
                extended: false,
                pressed: true,
            }));
        }

        assert_eq!(dispatcher.dropped(), 8, "capacity 2, so 8 of 10 must be dropped");
    }

    struct Collector(std::sync::Arc<std::sync::Mutex<Vec<InputEvent>>>);

    impl InputSink for Collector {
        fn pointer(&mut self, ev: PointerEvent) -> Result<()> {
            self.0.lock().expect("poisoned").push(InputEvent::Pointer(ev));
            Ok(())
        }
        fn key(&mut self, ev: KeyEvent) -> Result<()> {
            self.0.lock().expect("poisoned").push(InputEvent::Key(ev));
            Ok(())
        }
        fn set_geometry(&mut self, geometry: state::ScreenGeometry) {
            self.0.lock().expect("poisoned").push(InputEvent::Geometry(geometry));
        }
    }

    #[tokio::test]
    async fn events_reach_the_sink() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let collector = Collector(std::sync::Arc::clone(&seen));
        let mut dispatcher = spawn(move || Box::new(collector) as Box<dyn InputSink>, 8);

        dispatcher.send(InputEvent::Pointer(PointerEvent::MoveAbsolute { x: 7, y: 9 }));
        dispatcher.send(InputEvent::Key(KeyEvent::Unicode { ch: 65, pressed: true }));
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let seen = seen.lock().expect("poisoned");
        assert_eq!(
            *seen,
            vec![
                InputEvent::Pointer(PointerEvent::MoveAbsolute { x: 7, y: 9 }),
                InputEvent::Key(KeyEvent::Unicode { ch: 65, pressed: true }),
            ]
        );
    }

    /// F-007.17: geometry rides the same queue as the events it affects, so the
    /// mapping can only change *between* two events, never inside a burst.
    #[tokio::test]
    async fn geometry_updates_reach_the_sink_in_order() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let collector = Collector(std::sync::Arc::clone(&seen));
        let mut dispatcher = spawn(move || Box::new(collector) as Box<dyn InputSink>, 8);

        let moved = state::ScreenGeometry::new(1512.0, 0.0, 1.0);
        dispatcher.send(InputEvent::Pointer(PointerEvent::MoveAbsolute { x: 1, y: 1 }));
        dispatcher.send(InputEvent::Geometry(moved));
        dispatcher.send(InputEvent::Pointer(PointerEvent::MoveAbsolute { x: 2, y: 2 }));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let seen = seen.lock().expect("poisoned");
        assert_eq!(
            *seen,
            vec![
                InputEvent::Pointer(PointerEvent::MoveAbsolute { x: 1, y: 1 }),
                InputEvent::Geometry(moved),
                InputEvent::Pointer(PointerEvent::MoveAbsolute { x: 2, y: 2 }),
            ]
        );
    }

    /// F-003.24, F-003.23: the end-of-connection reset rides the input queue,
    /// so it lands exactly between the old connection's events and the new one's.
    #[tokio::test]
    async fn f003_24_session_reset_reaches_the_sink_in_order() {
        struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<InputEvent>>>);
        impl InputSink for Recorder {
            fn pointer(&mut self, ev: PointerEvent) -> Result<()> {
                self.0.lock().expect("poisoned").push(InputEvent::Pointer(ev));
                Ok(())
            }
            fn key(&mut self, ev: KeyEvent) -> Result<()> {
                self.0.lock().expect("poisoned").push(InputEvent::Key(ev));
                Ok(())
            }
            fn reset_session(&mut self) {
                self.0.lock().expect("poisoned").push(InputEvent::SessionReset);
            }
        }

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = Recorder(std::sync::Arc::clone(&seen));
        let mut dispatcher = spawn(move || Box::new(recorder) as Box<dyn InputSink>, 8);

        let cmd_down = InputEvent::Key(KeyEvent::Scancode {
            code: 0x5B,
            extended: true,
            pressed: true,
        });
        let a_down = InputEvent::Key(KeyEvent::Scancode {
            code: 0x1E,
            extended: false,
            pressed: true,
        });
        dispatcher.send(cmd_down);
        dispatcher.send(InputEvent::SessionReset);
        dispatcher.send(a_down);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(
            *seen.lock().expect("poisoned"),
            vec![cmd_down, InputEvent::SessionReset, a_down]
        );
    }

    /// A sink that does not map coordinates must not have to care.
    #[tokio::test]
    async fn a_sink_without_geometry_handling_ignores_the_update() {
        struct Deaf;
        impl InputSink for Deaf {
            fn pointer(&mut self, _: PointerEvent) -> Result<()> {
                Ok(())
            }
            fn key(&mut self, _: KeyEvent) -> Result<()> {
                Ok(())
            }
        }

        let mut dispatcher = spawn(|| Box::new(Deaf) as Box<dyn InputSink>, 8);
        dispatcher.send(InputEvent::Geometry(state::ScreenGeometry::new(1.0, 2.0, 2.0)));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(dispatcher.dropped(), 0);
    }
}
