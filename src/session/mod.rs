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
use ironrdp_server::{RdpServer, ServerEvent, TlsIdentityCtx};
use tokio::sync::{oneshot, watch};
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
    /// Published client count (F-007.26). `session` only counts; the decision
    /// about what to do with the number is made outside this boundary.
    pub clients: watch::Sender<usize>,
    /// Called once the engine is **actually accepting connections** (F-111.11).
    ///
    /// Not a convenience: the menu bar paints "listening" from this, and a
    /// caller cannot see the moment the socket goes up, because the engine
    /// binds inside its own run loop. Telling the menu "listening" before that
    /// produced exactly the failure this parameter exists to prevent — a
    /// client refused by a server the menu said was up (caught by CI,
    /// 2026-10-21).
    pub ready: ReadyNotifier,
}

/// Called once the server is accepting connections. See [`SessionParams::ready`].
pub type ReadyNotifier = Box<dyn FnOnce() + Send>;

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
        clients,
        ready,
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
    let resets = SessionResetHandler::new(dispatcher.clone(), clients);
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

    // The engine answers events only from inside its accept loop, which it
    // reaches after a successful bind — so an answer to this question *is* the
    // readiness signal. Asking before `run()` is safe: the channel is
    // unbounded, and the question waits in it until the loop starts. If the
    // bind fails instead, the server is dropped with the question still in its
    // queue, the reply channel closes, and this task ends without notifying
    // anyone.
    let events = server.event_sender().clone();
    let announce = tokio::spawn(async move {
        let (reply, answer) = oneshot::channel();
        if events.send(ServerEvent::GetLocalAddr(reply)).is_err() {
            return;
        }
        if let Ok(local_addr) = answer.await {
            // `None` would mean the engine is in its loop without having
            // recorded an address, which it never is; either way, being in the
            // loop is the fact worth reporting.
            info!(?local_addr, "accepting connections");
            ready();
        }
    });

    info!(%bind_addr, ?encoder, "serving RDP over TLS");
    let result = server.run().await;
    announce.abort();
    result
}
