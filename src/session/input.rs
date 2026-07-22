//! Adapts client input to our [`InputDispatcher`].

use ironrdp_server::{KeyboardEvent, MouseEvent, RdpServerInputHandler};
use tracing::trace;

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
