//! Input events and their destinations.
//!
//! Deliberately uses our own event types rather than `ironrdp-server`'s, so
//! that the RDP engine can be replaced without touching the injection code
//! (`specs/01-architecture.md` §1).

pub mod logging;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Pointer(PointerEvent),
    Key(KeyEvent),
}

/// Destination for input events.
///
/// Implementations: [`logging::LoggingSink`] (wave 0), `CgEventSink`
/// (wave 2, ADR-0003).
pub trait InputSink: Send {
    fn pointer(&mut self, ev: PointerEvent) -> Result<()>;
    fn key(&mut self, ev: KeyEvent) -> Result<()>;
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

/// Spawns the task that drains the queue into `sink`, returning the dispatcher
/// to hand to the RDP session.
pub fn spawn(mut sink: Box<dyn InputSink>, capacity: usize) -> InputDispatcher {
    let (tx, mut rx) = mpsc::channel(capacity);

    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let result = match event {
                InputEvent::Pointer(ev) => sink.pointer(ev),
                InputEvent::Key(ev) => sink.key(ev),
            };
            if let Err(error) = result {
                warn!(?error, "input sink rejected an event");
            }
        }
        info!("input queue closed");
    });

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

    #[tokio::test]
    async fn events_reach_the_sink() {
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
        }

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut dispatcher = spawn(Box::new(Collector(std::sync::Arc::clone(&seen))), 8);

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
}
