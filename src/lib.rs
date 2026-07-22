//! `mac-rdp-server` — RDP server for macOS.
//!
//! Module layout and the boundaries between the modules are specified in
//! `specs/01-architecture.md`. The one rule worth repeating here: only
//! [`session`] may reference `ironrdp-server`; every other module speaks this
//! crate's own types (enforced by `tests/architecture.rs`).

pub mod capture;
pub mod config;
pub mod encoder;
pub mod input;
pub mod session;
pub mod tls;
