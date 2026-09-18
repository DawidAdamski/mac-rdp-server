//! Adapts client input to our [`InputDispatcher`].

use std::net::SocketAddr;
use std::time::Duration;

use ironrdp_server::{ConnectionHandler, KeyboardEvent, MouseEvent, PostConnectionAction, RdpServerInputHandler};
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

/// Turns connection lifecycle hooks into [`InputEvent::SessionReset`]
/// (F-003.24), so no key or button held by one client outlives its connection.
///
/// `on_disconnected` runs after every connection, clean or broken; `on_accept`
/// sends the same reset once more, so a new connection starts from "all up"
/// even if the first reset was dropped by a full queue. Both only `try_send`
/// through the dispatcher and never block the accept loop.
pub struct SessionResetHandler {
    dispatcher: InputDispatcher,
}

impl SessionResetHandler {
    #[must_use]
    pub fn new(dispatcher: InputDispatcher) -> Self {
        Self { dispatcher }
    }
}

impl ConnectionHandler for SessionResetHandler {
    fn on_accept(&mut self, _peer: SocketAddr) -> bool {
        self.dispatcher.send(InputEvent::SessionReset);
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

    /// F-003.24: both ends of a connection — including one that ended with an
    /// error — put a reset on the input queue and let the server go on.
    #[test]
    fn f003_24_connection_lifecycle_sends_session_resets() {
        let (tx, mut rx) = mpsc::channel(4);
        let mut handler = SessionResetHandler::new(InputDispatcher::new(tx));

        assert!(handler.on_accept(peer()));
        let error = anyhow::anyhow!("connection reset by peer");
        let action = handler.on_disconnected(peer(), Duration::from_secs(1), Some(&error));

        assert_eq!(action, PostConnectionAction::Continue);
        assert_eq!(rx.try_recv().ok(), Some(InputEvent::SessionReset));
        assert_eq!(rx.try_recv().ok(), Some(InputEvent::SessionReset));
    }

    /// F-003.24: a full queue drops the reset instead of blocking the server.
    #[test]
    fn f003_24_a_full_queue_does_not_block_the_disconnect_hook() {
        let (tx, _rx) = mpsc::channel(1);
        let mut dispatcher = InputDispatcher::new(tx);
        dispatcher.send(InputEvent::SessionReset);
        let mut handler = SessionResetHandler::new(dispatcher);

        let action = handler.on_disconnected(peer(), Duration::ZERO, None);
        assert_eq!(action, PostConnectionAction::Continue);
    }
}
