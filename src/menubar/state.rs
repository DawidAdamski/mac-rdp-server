//! What the switch may do, and what the menu says while it does it (F-111.15).
//!
//! Deliberately free of AppKit and of the server: this is the part that decides
//! whether a click is allowed, what the icon means and what words the menu
//! shows, and all of that is worth testing without a menu bar, without a
//! display and without a single granted permission (AC-1).

use std::net::SocketAddr;

/// What the service is doing, as far as the menu knows.
///
/// `Starting` and `Stopping` exist so that a second click during either cannot
/// start a second server or race the first one down: a transition is a state,
/// not a gap between states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    /// No socket, no capture. Where the process begins in menu-bar mode
    /// (F-111.18).
    Stopped,
    Starting,
    Listening,
    Stopping,
    /// A start that did not get there, with the reason to show (F-111.11).
    Failed(String),
}

/// What a click asks the supervisor to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Start,
    Stop,
}

/// Which icon the status item shows (F-111.4).
///
/// Four, not two: "working on it" and "it did not start" both need to be
/// distinguishable from plain off, or the icon makes a failed start look like a
/// click that never registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Listening,
    Stopped,
    Busy,
    Failed,
}

impl Icon {
    /// The SF Symbol to draw, in template mode (F-111.9).
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Listening => "bolt.horizontal.circle.fill",
            Self::Stopped => "bolt.horizontal.circle",
            Self::Busy => "ellipsis.circle",
            Self::Failed => "exclamationmark.triangle",
        }
    }

    /// What to show if the symbol cannot be loaded.
    ///
    /// Not decoration: an icon that silently fails to load leaves an empty
    /// clickable gap in the menu bar, which looks exactly like the icon not
    /// being there at all.
    #[must_use]
    pub const fn fallback_text(self) -> &'static str {
        match self {
            Self::Listening => "RDP",
            Self::Stopped => "rdp",
            Self::Busy => "...",
            Self::Failed => "RDP!",
        }
    }
}

/// The switch: current state plus the address it would use.
#[derive(Debug, Clone)]
pub struct Toggle {
    state: ServiceState,
    addr: SocketAddr,
}

impl Toggle {
    /// A switch that is **off** (F-111.18).
    #[must_use]
    pub const fn new(addr: SocketAddr) -> Self {
        Self {
            state: ServiceState::Stopped,
            addr,
        }
    }

    #[must_use]
    pub const fn state(&self) -> &ServiceState {
        &self.state
    }

    #[must_use]
    pub const fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Handles a click on the toggle item, returning what to ask for.
    ///
    /// `None` means the click is not actionable right now, which is also why
    /// [`Self::is_actionable`] exists: the item is greyed out in those states,
    /// so this is the second line of defence rather than the first.
    pub fn click(&mut self) -> Option<Request> {
        match self.state {
            ServiceState::Stopped | ServiceState::Failed(_) => {
                self.state = ServiceState::Starting;
                Some(Request::Start)
            }
            ServiceState::Listening => {
                self.state = ServiceState::Stopping;
                Some(Request::Stop)
            }
            ServiceState::Starting | ServiceState::Stopping => None,
        }
    }

    /// Whether the toggle item should be clickable at all.
    #[must_use]
    pub const fn is_actionable(&self) -> bool {
        matches!(
            self.state,
            ServiceState::Stopped | ServiceState::Failed(_) | ServiceState::Listening
        )
    }

    /// The service reported that the socket is up.
    pub fn listening(&mut self) {
        self.state = ServiceState::Listening;
    }

    /// The service came down, as asked or by itself.
    pub fn stopped(&mut self) {
        self.state = ServiceState::Stopped;
    }

    /// The service could not start, or died with an error (F-111.11).
    pub fn failed(&mut self, reason: impl Into<String>) {
        self.state = ServiceState::Failed(reason.into());
    }

    #[must_use]
    pub const fn icon(&self) -> Icon {
        match self.state {
            ServiceState::Listening => Icon::Listening,
            ServiceState::Stopped => Icon::Stopped,
            ServiceState::Starting | ServiceState::Stopping => Icon::Busy,
            ServiceState::Failed(_) => Icon::Failed,
        }
    }

    /// The toggle item's title: what happens if you click it, not what state it
    /// is in (F-111.5). The state is the icon's job and the status line's.
    #[must_use]
    pub const fn action_title(&self) -> &'static str {
        match self.state {
            ServiceState::Stopped | ServiceState::Failed(_) => "Start the server",
            ServiceState::Listening => "Stop the server",
            ServiceState::Starting => "Starting…",
            ServiceState::Stopping => "Stopping…",
        }
    }

    /// The first line of the menu: what is happening, in words.
    #[must_use]
    pub fn status_line(&self) -> String {
        match &self.state {
            ServiceState::Stopped => "Not listening".to_owned(),
            ServiceState::Starting => "Starting…".to_owned(),
            ServiceState::Listening => format!("Listening on {}", self.addr),
            ServiceState::Stopping => "Stopping…".to_owned(),
            // Reasons arrive from `anyhow` with their context chain joined by
            // ": ", which is long. The menu shows the whole thing anyway: a
            // truncated reason is the kind of help that sends someone to the
            // log, and in this mode there may not be a terminal to read.
            ServiceState::Failed(reason) => format!("Could not start: {reason}"),
        }
    }

    /// The address line, shown in every state (F-111.6).
    ///
    /// Present even when stopped, and worded for it: the question someone opens
    /// this menu with is "where would it be if I switched it on".
    #[must_use]
    pub fn address_line(&self) -> String {
        match self.state {
            ServiceState::Listening => format!("Address: {}", self.addr),
            _ => format!("Would listen on {}", self.addr),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toggle() -> Toggle {
        Toggle::new("127.0.0.1:3389".parse().expect("valid address"))
    }

    /// F-111.18: the process wakes up with the service off. The owner's words:
    /// a running app is not a running service.
    #[test]
    fn f111_18_a_fresh_toggle_is_off() {
        let toggle = toggle();
        assert_eq!(*toggle.state(), ServiceState::Stopped);
        assert_eq!(toggle.icon(), Icon::Stopped);
        assert_eq!(toggle.action_title(), "Start the server");
    }

    /// AC-1: off → on → off, with the request that each click produces.
    #[test]
    fn f111_15_the_switch_goes_both_ways() {
        let mut toggle = toggle();

        assert_eq!(toggle.click(), Some(Request::Start));
        assert_eq!(*toggle.state(), ServiceState::Starting);
        assert_eq!(toggle.icon(), Icon::Busy);

        toggle.listening();
        assert_eq!(toggle.action_title(), "Stop the server");
        assert_eq!(toggle.status_line(), "Listening on 127.0.0.1:3389");

        assert_eq!(toggle.click(), Some(Request::Stop));
        assert_eq!(*toggle.state(), ServiceState::Stopping);

        toggle.stopped();
        assert_eq!(toggle.action_title(), "Start the server");
        assert_eq!(toggle.status_line(), "Not listening");
    }

    /// AC-1 and F-111.11: a start that fails leaves the switch off and carries
    /// the reason. The icon must not stay on "working on it" either — that is
    /// indistinguishable from a click that did nothing.
    #[test]
    fn f111_11_a_failed_start_stays_off_and_says_why() {
        let mut toggle = toggle();
        toggle.click();
        toggle.failed("cannot listen on 127.0.0.1:3389 (Address already in use)");

        assert_eq!(toggle.icon(), Icon::Failed);
        assert_eq!(toggle.action_title(), "Start the server", "clicking must try again");
        assert!(
            toggle.status_line().contains("Address already in use"),
            "the reason has to reach the menu: {}",
            toggle.status_line()
        );
        assert!(toggle.is_actionable(), "a failed start must not wedge the switch");
    }

    /// A second click while the first is still being acted on must not ask for
    /// anything — one click, one server.
    #[test]
    fn f111_12_clicks_during_a_transition_are_ignored() {
        let mut toggle = toggle();
        assert_eq!(toggle.click(), Some(Request::Start));
        assert_eq!(toggle.click(), None, "no second start");
        assert!(!toggle.is_actionable());

        toggle.listening();
        assert_eq!(toggle.click(), Some(Request::Stop));
        assert_eq!(toggle.click(), None, "no second stop");
    }

    /// F-111.12: ten cycles leave the switch exactly where one does. The
    /// socket-level half of this is AC-8; this is the half that can be tested
    /// without a GUI.
    #[test]
    fn f111_12_ten_cycles_end_where_they_started() {
        let mut toggle = toggle();
        for _ in 0..10 {
            assert_eq!(toggle.click(), Some(Request::Start));
            toggle.listening();
            assert_eq!(toggle.click(), Some(Request::Stop));
            toggle.stopped();
        }
        assert_eq!(*toggle.state(), ServiceState::Stopped);
        assert_eq!(toggle.icon(), Icon::Stopped);
    }

    /// F-111.6: the address is in the menu in both states, and the wording
    /// changes with the state rather than claiming to listen when it does not.
    #[test]
    fn f111_6_the_address_is_shown_whether_or_not_it_is_listening() {
        let mut toggle = toggle();
        assert_eq!(toggle.address_line(), "Would listen on 127.0.0.1:3389");
        toggle.listening();
        assert_eq!(toggle.address_line(), "Address: 127.0.0.1:3389");
    }

    /// F-111.6 with F-112: a port from the configuration file reaches the menu,
    /// because the menu is told the address rather than guessing a default.
    #[test]
    fn f111_13_the_menu_shows_the_configured_address() {
        let toggle = Toggle::new("0.0.0.0:33891".parse().expect("valid address"));
        assert!(toggle.address_line().ends_with("0.0.0.0:33891"));
    }

    /// Every state has to produce an icon and a title; a state that renders as
    /// nothing is a blank gap in the menu bar.
    #[test]
    fn f111_4_every_state_has_an_icon_and_a_title() {
        for state in [
            ServiceState::Stopped,
            ServiceState::Starting,
            ServiceState::Listening,
            ServiceState::Stopping,
            ServiceState::Failed("whatever".to_owned()),
        ] {
            let mut toggle = toggle();
            toggle.state = state.clone();
            assert!(!toggle.icon().symbol().is_empty(), "{state:?}");
            assert!(!toggle.icon().fallback_text().is_empty(), "{state:?}");
            assert!(!toggle.action_title().is_empty(), "{state:?}");
            assert!(!toggle.status_line().is_empty(), "{state:?}");
            assert!(!toggle.address_line().is_empty(), "{state:?}");
        }
    }
}
