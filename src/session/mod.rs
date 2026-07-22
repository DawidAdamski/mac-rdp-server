//! The RDP session: the **only** module that knows `ironrdp-server` exists.
//!
//! Everything else in this crate speaks our own types. Keeping the protocol
//! engine behind this boundary is what makes ADR-0001's long-term plan — our
//! own MS-RDPBCGR implementation — a module swap rather than a rewrite.
//! Enforced by `tests/architecture.rs` (F-001.12).

mod convert;
mod display;
mod input;

use std::net::SocketAddr;
use std::path::Path;

use anyhow::{Context as _, Result};
use ironrdp_server::{RdpServer, TlsIdentityCtx};
use tracing::info;

pub use display::MacRdpDisplay;
pub use input::MacRdpInput;

use crate::capture::FrameStream;
use crate::encoder::Encoder;
use crate::encoder::full_frame::FullFrameEncoder;
use crate::input::InputDispatcher;

pub struct SessionParams<'a> {
    pub bind_addr: SocketAddr,
    pub cert_path: &'a Path,
    pub key_path: &'a Path,
    pub stream: FrameStream,
    pub desktop_width: u16,
    pub desktop_height: u16,
    pub dispatcher: InputDispatcher,
}

/// Builds the server and serves connections until it stops.
///
/// `RdpServer::run` accepts one connection at a time and logs — rather than
/// propagates — per-connection errors, so a client disconnecting or
/// misbehaving does not end the process (F-001.16).
pub async fn run(params: SessionParams<'_>) -> Result<()> {
    let SessionParams {
        bind_addr,
        cert_path,
        key_path,
        stream,
        desktop_width,
        desktop_height,
        dispatcher,
    } = params;

    let identity = TlsIdentityCtx::init_from_paths(cert_path, key_path)
        .context("loading TLS identity (is the certificate a valid PEM?)")?;
    let acceptor = identity.make_acceptor().context("building the TLS acceptor")?;

    let make_encoder: display::EncoderFactory = Box::new(|| Box::new(FullFrameEncoder::new()) as Box<dyn Encoder>);
    let display = MacRdpDisplay::new(stream, desktop_width, desktop_height, make_encoder);
    let handler = MacRdpInput::new(dispatcher);

    // TLS only, no NLA: `with_hybrid` is F-101 (ADR-0004).
    let mut server = RdpServer::builder()
        .with_addr(bind_addr)
        .with_tls(acceptor)
        .with_input_handler(handler)
        .with_display_handler(display)
        .build();

    info!(%bind_addr, "serving RDP over TLS");
    server.run().await
}
