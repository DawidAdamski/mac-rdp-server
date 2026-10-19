//! Adapts client input to our [`InputDispatcher`].

use std::net::SocketAddr;
use std::time::Duration;

use ironrdp_server::{ConnectionHandler, KeyboardEvent, MouseEvent, PostConnectionAction, RdpServerInputHandler};
use tokio::sync::watch;
use tracing::{debug, trace};

use super::convert;
use crate::input::{InputDispatcher, InputEvent};

pub struct MacRdpInput {
    dispatcher: InputDispatcher,
}

impl MacRdpInput {
    #[must_use]
    pub fn new(dispatcher: InputDispatcher) -> Self {
        Self { dispatcher }
    }
}

impl RdpServerInputHandler for MacRdpInput {
    /// Runs on the server's connection loop and must not block
    /// (`01-architecture.md` §4a) — convert and hand off, nothing more.
    fn keyboard(&mut self, event: KeyboardEvent) {
        // F-003a.19: trace only — this records everything the user types.
        let received = KeyTrace::of(&event);
        trace!(
            kind = received.kind,
            code = format_args!("{:#06X}", received.code),
            extended = received.extended,
            direction = received.direction,
            "key received"
        );
        match convert::key_event(&event) {
            Some(ev) => self.dispatcher.send(InputEvent::Key(ev)),
            None => trace!(?event, "unhandled keyboard event"),
        }
    }

    fn mouse(&mut self, event: MouseEvent) {
        match convert::pointer_event(&event) {
            Some(ev) => self.dispatcher.send(InputEvent::Pointer(ev)),
            None => trace!(?event, "unhandled mouse event"),
        }
    }
}

/// What a received keyboard event was, before conversion (F-003a.19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KeyTrace {
    kind: &'static str,
    /// Scancode, UTF-16 code unit, or the raw Synchronize flags.
    code: u16,
    extended: bool,
    direction: &'static str,
}

impl KeyTrace {
    fn of(event: &KeyboardEvent) -> Self {
        let (kind, code, extended, direction) = match *event {
            KeyboardEvent::Pressed { code, extended } => ("scancode", u16::from(code), extended, "down"),
            KeyboardEvent::Released { code, extended } => ("scancode", u16::from(code), extended, "up"),
            KeyboardEvent::UnicodePressed(unit) => ("unicode", unit, false, "down"),
            KeyboardEvent::UnicodeReleased(unit) => ("unicode", unit, false, "up"),
            KeyboardEvent::Synchronize(flags) => ("sync", u16::from(flags.bits()), false, "-"),
        };
        Self {
            kind,
            code,
            extended,
            direction,
        }
    }
}

/// Turns connection lifecycle hooks into [`InputEvent::SessionReset`]
/// (F-003.24), so no key or button held by one client outlives its connection.
///
/// `on_disconnected` runs after every connection, clean or broken; `on_accept`
/// sends the same reset once more, so a new connection starts from "all up"
/// even if the first reset was dropped by a full queue. Both only `try_send`
/// through the dispatcher and never block the accept loop.
pub struct SessionResetHandler {
    dispatcher: InputDispatcher,
    /// How many clients are connected, published for whoever cares (F-007.26).
    ///
    /// A plain number is the whole contract. `session` must not know that
    /// something on the other end keeps the Mac's display awake, any more than
    /// it knows about ScreenCaptureKit (`CLAUDE.md` §Granice).
    clients: watch::Sender<usize>,
}

impl SessionResetHandler {
    #[must_use]
    pub fn new(dispatcher: InputDispatcher, clients: watch::Sender<usize>) -> Self {
        Self { dispatcher, clients }
    }
}

impl ConnectionHandler for SessionResetHandler {
    fn on_accept(&mut self, _peer: SocketAddr) -> bool {
        self.dispatcher.send(InputEvent::SessionReset);
        self.clients.send_modify(|connected| *connected += 1);
        true
    }

    fn on_disconnected(
        &mut self,
        peer: SocketAddr,
        duration: Duration,
        error: Option<&anyhow::Error>,
    ) -> PostConnectionAction {
        debug!(%peer, ?duration, failed = error.is_some(), "connection ended; resetting input state");
        self.dispatcher.send(InputEvent::SessionReset);
        // Saturating, so an unbalanced disconnect cannot wrap the count around
        // to a huge number and pin the display awake for ever.
        self.clients
            .send_modify(|connected| *connected = connected.saturating_sub(1));
        PostConnectionAction::Continue
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;

    fn peer() -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 50_000))
    }

    /// A client counter whose value nobody reads.
    fn counter() -> watch::Sender<usize> {
        watch::channel(0).0
    }

    /// F-003a.19: every received key is described with kind, code and
    /// direction — in particular each UTF-16 half of an emoji.
    #[test]
    fn f003a_19_received_keys_are_described_for_the_trace() {
        assert_eq!(
            KeyTrace::of(&KeyboardEvent::Pressed {
                code: 0x5B,
                extended: true
            }),
            KeyTrace {
                kind: "scancode",
                code: 0x5B,
                extended: true,
                direction: "down"
            }
        );
        assert_eq!(
            KeyTrace::of(&KeyboardEvent::UnicodePressed(0xD83D)),
            KeyTrace {
                kind: "unicode",
                code: 0xD83D,
                extended: false,
                direction: "down"
            }
        );
        assert_eq!(KeyTrace::of(&KeyboardEvent::UnicodeReleased(0xDE00)).direction, "up");
        assert_eq!(
            KeyTrace::of(&KeyboardEvent::Released {
                code: 0x1E,
                extended: false
            })
            .direction,
            "up"
        );
    }

    /// F-003.24: both ends of a connection — including one that ended with an
    /// error — put a reset on the input queue and let the server go on.
    #[test]
    fn f003_24_connection_lifecycle_sends_session_resets() {
        let (tx, mut rx) = mpsc::channel(4);
        let mut handler = SessionResetHandler::new(InputDispatcher::new(tx), counter());

        assert!(handler.on_accept(peer()));
        let error = anyhow::anyhow!("connection reset by peer");
        let action = handler.on_disconnected(peer(), Duration::from_secs(1), Some(&error));

        assert_eq!(action, PostConnectionAction::Continue);
        assert_eq!(rx.try_recv().ok(), Some(InputEvent::SessionReset));
        assert_eq!(rx.try_recv().ok(), Some(InputEvent::SessionReset));
    }

    /// F-007.26: the published count follows connections, because that number
    /// is the only thing deciding whether the display is kept awake.
    #[test]
    fn f007_26_the_client_count_follows_connections() {
        let (tx, _rx) = mpsc::channel(8);
        let clients = watch::channel(0).0;
        let seen = clients.subscribe();
        let mut handler = SessionResetHandler::new(InputDispatcher::new(tx), clients);

        assert_eq!(*seen.borrow(), 0, "nothing is connected before the first accept");

        handler.on_accept(peer());
        assert_eq!(*seen.borrow(), 1);

        // Two at once is allowed, and the display must stay awake for both.
        handler.on_accept(peer());
        assert_eq!(*seen.borrow(), 2);

        handler.on_disconnected(peer(), Duration::ZERO, None);
        assert_eq!(*seen.borrow(), 1, "one left, one still connected");

        handler.on_disconnected(peer(), Duration::ZERO, None);
        assert_eq!(*seen.borrow(), 0, "the last one leaving releases the display");
    }

    /// F-007.26: an unbalanced disconnect must not wrap the count around.
    ///
    /// `0 - 1` on a `usize` would become `usize::MAX`, and the display would
    /// then be held awake for the life of the server with no client in sight.
    #[test]
    fn f007_26_an_extra_disconnect_cannot_wrap_the_count() {
        let (tx, _rx) = mpsc::channel(8);
        let clients = watch::channel(0).0;
        let seen = clients.subscribe();
        let mut handler = SessionResetHandler::new(InputDispatcher::new(tx), clients);

        handler.on_disconnected(peer(), Duration::ZERO, None);
        assert_eq!(*seen.borrow(), 0);
    }

    /// F-003.24: a full queue drops the reset instead of blocking the server.
    #[test]
    fn f003_24_a_full_queue_does_not_block_the_disconnect_hook() {
        let (tx, _rx) = mpsc::channel(1);
        let mut dispatcher = InputDispatcher::new(tx);
        dispatcher.send(InputEvent::SessionReset);
        let mut handler = SessionResetHandler::new(dispatcher, counter());

        let action = handler.on_disconnected(peer(), Duration::ZERO, None);
        assert_eq!(action, PostConnectionAction::Continue);
    }
}
