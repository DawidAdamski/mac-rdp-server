//! The authentication audit trail (F-101.13, F-101.14).
//!
//! Two lines, and the hard part of both is *when*. The protocol engine offers
//! exactly two connection hooks — one before the TLS upgrade and CredSSP, one
//! after the whole connection has ended — so neither of them is the moment an
//! authentication succeeds.
//!
//! What stands in for that moment is the first call the engine makes that it
//! can only reach past CredSSP: asking the display handler for its update
//! stream (`RdpServerDisplay::updates`, engine `server.rs:1236`, inside the
//! running connection). The desktop *size* is asked for earlier, while the
//! acceptor is still being built (`server.rs:813`), so it is no good here.
//!
//! A rejection is the other way round: it arrives as the connection's error,
//! and is told apart from every other connection failure by downcasting to the
//! engine's own `ConnectorError` and reading its kind. That keeps the test on a
//! type rather than on the wording of an upstream message, which is the
//! difference between a check that survives a dependency bump and one that goes
//! quietly false.

use std::net::SocketAddr;
use std::sync::Mutex;

use ironrdp_connector::{ConnectorError, ConnectorErrorKind};
use tracing::{debug, info, warn};

/// Logs accepted and rejected authentications for one running server.
///
/// Shared by the connection handler (which knows the peer) and the display
/// handler (which knows the session started), hence the interior mutability:
/// both see it as `&self`.
pub struct AuthAudit {
    /// `None` when authentication is not configured. There is then nothing to
    /// audit, and both entry points stay silent at `info`/`warn` so that a
    /// loopback diagnostic run does not grow an audit trail about a password
    /// it never asked for.
    user: Option<String>,
    /// The peer of the connection currently being set up.
    ///
    /// The engine serves one connection at a time (`server.rs`, the accept
    /// loop awaits `run_connection`), so one slot is enough; a `Mutex` rather
    /// than a cell because the display handler is on another task.
    peer: Mutex<Option<SocketAddr>>,
}

impl AuthAudit {
    #[must_use]
    pub fn new(user: Option<String>) -> Self {
        Self {
            user,
            peer: Mutex::new(None),
        }
    }

    /// A connection arrived. Nothing is authenticated yet.
    pub fn connection_opened(&self, peer: SocketAddr) {
        *self.peer.lock().unwrap_or_else(|e| e.into_inner()) = Some(peer);
        if let Some(user) = &self.user {
            debug!(%peer, user, "connection accepted; authenticating");
        }
    }

    /// The session is running, which with NLA can only happen after CredSSP
    /// succeeded — so this is an accepted authentication (F-101.13).
    pub fn session_started(&self) {
        let Some(user) = &self.user else {
            return;
        };
        match *self.peer.lock().unwrap_or_else(|e| e.into_inner()) {
            Some(peer) => info!(%peer, user, "authentication accepted"),
            // Would mean a session without a preceding accept, which the
            // engine's loop does not do. Report it rather than skip the audit
            // line: a missing address is worth less than no line at all.
            None => info!(peer = "unknown", user, "authentication accepted"),
        }
    }

    /// A connection ended. Logs the rejection line when the failure was an
    /// authentication failure rather than any of the ordinary ways a client
    /// goes away (F-101.14).
    pub fn connection_closed(&self, peer: SocketAddr, error: Option<&anyhow::Error>) {
        let Some(user) = &self.user else {
            return;
        };
        if let Some(reason) = error.and_then(rejection_reason) {
            warn!(%peer, user, reason, "authentication rejected");
        }
    }
}

/// Why this connection error counts as an authentication rejection, if it does.
///
/// `None` for every other failure — a client closing the window, a broken pipe,
/// a decode error — because a `warn` about authentication on an ordinary
/// disconnect would make the audit trail useless for the one thing it exists
/// for.
///
/// Pure, and the reason it is a free function: it is the whole testable part of
/// F-101.14.
#[must_use]
pub fn rejection_reason(error: &anyhow::Error) -> Option<&'static str> {
    // The error travels up from `accept_credssp` through `anyhow`, so the
    // engine's error can be anywhere in the chain rather than at its head.
    error.chain().find_map(|link| {
        let kind = link.downcast_ref::<ConnectorError>()?.kind();
        match kind {
            // A bad password fails inside SSPI and arrives here (engine
            // `credssp.rs:165`, `acceptor/src/lib.rs` credssp_loop).
            ConnectorErrorKind::Credssp(_) => Some("credssp"),
            // The engine's own refusal, used on the standard-security path.
            ConnectorErrorKind::AccessDenied => Some("access denied"),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credssp_error() -> anyhow::Error {
        let sspi_error = ironrdp_connector::sspi::Error::new(
            ironrdp_connector::sspi::ErrorKind::LogonDenied,
            "the password did not match",
        );
        anyhow::Error::new(ConnectorError::new("CredSSP", ConnectorErrorKind::Credssp(sspi_error)))
    }

    /// F-101.14: the failure the requirement is about.
    #[test]
    fn f101_14_a_credssp_failure_is_an_authentication_rejection() {
        assert_eq!(rejection_reason(&credssp_error()), Some("credssp"));
    }

    #[test]
    fn f101_14_an_access_denied_is_an_authentication_rejection() {
        let error = anyhow::Error::new(ConnectorError::new("denied", ConnectorErrorKind::AccessDenied));
        assert_eq!(rejection_reason(&error), Some("access denied"));
    }

    /// F-101.14: and the failures it is **not** about. Without this, every
    /// client that closes its window would be logged as a rejected login and
    /// the audit trail would say nothing.
    #[test]
    fn f101_14_an_ordinary_connection_failure_is_not_a_rejection() {
        assert_eq!(rejection_reason(&anyhow::anyhow!("broken pipe")), None);
        let general = anyhow::Error::new(ConnectorError::new("oops", ConnectorErrorKind::General));
        assert_eq!(rejection_reason(&general), None);
        let reason = anyhow::Error::new(ConnectorError::new(
            "oops",
            ConnectorErrorKind::Reason("client went away".to_owned()),
        ));
        assert_eq!(rejection_reason(&reason), None);
    }

    /// The error does not arrive bare: it is wrapped by `anyhow` context on the
    /// way out of the engine, so the search has to walk the chain.
    #[test]
    fn f101_14_a_wrapped_credssp_failure_is_still_found() {
        let wrapped = credssp_error().context("running the connection").context("serving");
        assert_eq!(rejection_reason(&wrapped), Some("credssp"));
    }

    /// Without a configured user there is nothing to audit, and the audit must
    /// not invent an entry — a server on loopback with no password has no
    /// authentication to report on.
    #[test]
    fn f101_13_an_unauthenticated_server_has_no_audit_trail() {
        let audit = AuthAudit::new(None);
        let peer: SocketAddr = "127.0.0.1:50000".parse().expect("valid address");
        // Exercises every entry point: none of them may panic, and none of
        // them has an address or a user to log.
        audit.connection_opened(peer);
        audit.session_started();
        audit.connection_closed(peer, Some(&credssp_error()));
        assert_eq!(audit.user, None);
    }

    /// The accept records the peer, so the acceptance line a moment later can
    /// name it (F-101.13).
    #[test]
    fn f101_13_the_peer_is_remembered_from_the_accept() {
        let audit = AuthAudit::new(Some("rdp".to_owned()));
        let peer: SocketAddr = "192.168.1.10:51000".parse().expect("valid address");
        audit.connection_opened(peer);
        assert_eq!(*audit.peer.lock().expect("not poisoned"), Some(peer));
        audit.session_started();
    }
}
