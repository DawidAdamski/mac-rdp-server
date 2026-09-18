//! The RDP session: the **only** module that knows `ironrdp-server` exists.
//!
//! Everything else in this crate speaks our own types. Keeping the protocol
//! engine behind this boundary is what makes ADR-0001's long-term plan — our
//! own MS-RDPBCGR implementation — a module swap rather than a rewrite.
//! Enforced by `tests/architecture.rs` (F-001.12).

mod convert;
mod display;
mod input;
mod suppress;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context as _, Result};
use ironrdp_pdu::rdp::capability_sets::BitmapCodecs;
use ironrdp_server::{RdpServer, TlsIdentityCtx};
use tracing::info;

pub use display::MacRdpDisplay;
pub use input::{MacRdpInput, SessionResetHandler};

use crate::capture::FrameStream;
use crate::config::EncoderKind;
use crate::encoder::Encoder;
use crate::encoder::damage::DamageEncoder;
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
    pub encoder: EncoderKind,
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
        encoder,
    } = params;

    let identity = TlsIdentityCtx::init_from_paths(cert_path, key_path)
        .context("loading TLS identity (is the certificate a valid PEM?)")?;
    let acceptor = identity.make_acceptor().context("building the TLS acceptor")?;

    // One encoder per connection, built here rather than shared: the damage
    // encoder's baseline is "what this client has already been sent", so two
    // clients at different speeds must not read each other's history
    // (ADR-0009 §1).
    let make_encoder: display::EncoderFactory = match encoder {
        EncoderKind::Damage => Box::new(|| Box::new(DamageEncoder::new()) as Box<dyn Encoder>),
        EncoderKind::Full => Box::new(|| Box::new(FullFrameEncoder::new()) as Box<dyn Encoder>),
    };
    // F-008.3: one flag, written by IronRDP on Suppress Output / Refresh Rect
    // and read by our display loop. It does not leave `session`.
    let suppressed = Arc::new(AtomicBool::new(false));
    let display = MacRdpDisplay::new(
        stream,
        desktop_width,
        desktop_height,
        make_encoder,
        Arc::clone(&suppressed),
    );
    // F-003.24: connection ends reset the input state through the same queue.
    let resets = SessionResetHandler::new(dispatcher.clone());
    let handler = MacRdpInput::new(dispatcher);

    // TLS only, no NLA: `with_hybrid` is F-101 (ADR-0004).
    let mut server = RdpServer::builder()
        .with_addr(bind_addr)
        .with_tls(acceptor)
        .with_input_handler(handler)
        .with_display_handler(display)
        // Advertise NO bitmap codecs, so the server stays on the plain RDP 6.0
        // bitmap path that F-002 and ADR-0001 specify for this wave.
        //
        // This call is not optional. Without it the builder defaults to
        // `server_codecs_capabilities(&[])`, which despite the empty slice means
        // "the defaults" — RemoteFX, QOI and QOIZ. Any client that advertises one
        // of those (every real client advertises RemoteFX) then gets Surface Bits
        // instead of Bitmap Updates, and the result decodes to a fully black
        // screen. Measured with `examples/probe_client`, see ADR-0006.
        .with_bitmap_codecs(BitmapCodecs(Vec::new()))
        .with_connection_handler(Some(Box::new(resets)))
        .with_display_suppressed_handle(suppressed)
        .build();

    info!(%bind_addr, ?encoder, "serving RDP over TLS");
    server.run().await
}
