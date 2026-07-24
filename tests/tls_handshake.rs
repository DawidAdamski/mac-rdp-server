//! F-001.19: the listener really completes an RDP-style TLS handshake.
//!
//! RDP does not start with TLS. Per MS-RDPBCGR 2.2.1.1/2.2.1.2 the client first
//! sends an X.224 Connection Request carrying RDP_NEG_REQ, the server answers
//! with a Connection Confirm selecting a protocol, and only then does TLS begin.
//! This test walks that sequence, so a regression in either half is caught
//! without needing a GUI client.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use ironrdp_server::tokio_rustls::TlsConnector;
use ironrdp_server::tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use ironrdp_server::tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use ironrdp_server::tokio_rustls::rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use mac_rdp_server::capture::FrameSource as _;
use mac_rdp_server::capture::test_pattern::TestPatternSource;
use mac_rdp_server::input::{self, logging::LoggingSink};
use mac_rdp_server::{session, tls};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

/// X.224 Connection Request with RDP_NEG_REQ requesting PROTOCOL_SSL.
///
/// TPKT header (03 00 00 13) + X.224 CR TPDU (0E E0 00 00 00 00 00)
/// + RDP_NEG_REQ (type 01, flags 00, length 0008, protocols 00000001).
const X224_CONNECTION_REQUEST: [u8; 19] = [
    0x03, 0x00, 0x00, 0x13, // TPKT: version 3, reserved, length 19
    0x0E, 0xE0, 0x00, 0x00, 0x00, 0x00, 0x00, // X.224 CR, class 0
    0x01, 0x00, 0x08, 0x00, 0x01, 0x00, 0x00, 0x00, // RDP_NEG_REQ, PROTOCOL_SSL
];

const X224_CONNECTION_CONFIRM: u8 = 0xD0;
const RDP_NEG_RSP: u8 = 0x02;
const PROTOCOL_SSL: u32 = 0x0000_0001;

/// Accepts any certificate: the server's is self-signed by design (ADR-0004),
/// and this test is about the handshake completing, not about trust.
#[derive(Debug)]
struct AcceptAnyCert(Arc<ironrdp_server::tokio_rustls::rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, ironrdp_server::tokio_rustls::rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, ironrdp_server::tokio_rustls::rustls::Error> {
        ironrdp_server::tokio_rustls::rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, ironrdp_server::tokio_rustls::rustls::Error> {
        ironrdp_server::tokio_rustls::rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Reserves an ephemeral port and releases it again.
///
/// Mildly racy, but the server sets `SO_REUSEADDR` and nothing else in the test
/// suite listens, so in practice the port is still free a moment later.
fn free_port() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve an ephemeral port");
    let addr = listener.local_addr().expect("read the reserved port");
    drop(listener);
    addr
}

async fn connect_with_retries(addr: SocketAddr) -> TcpStream {
    for _ in 0..100 {
        if let Ok(stream) = TcpStream::connect(addr).await {
            return stream;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never started listening on {addr}");
}

#[tokio::test]
async fn server_negotiates_x224_then_completes_tls() {
    let dir = std::env::temp_dir().join(format!("mac-rdp-server-handshake-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cert = dir.join("cert.pem");
    let key = dir.join("key.pem");
    tls::ensure_self_signed(&cert, &key).expect("generate certificate");

    let addr = free_port();

    let mut source = TestPatternSource::new(320, 240);
    let stream = source.start().await.expect("start frame source");
    let dispatcher = input::spawn(|| Box::new(LoggingSink::new()), 16);

    // `RdpServer::run` returns a `!Send` future (it holds `Rc`s across awaits),
    // so it cannot go through `tokio::spawn` — it needs a LocalSet.
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let server = tokio::task::spawn_local({
                let cert = cert.clone();
                let key = key.clone();
                async move {
                    let _ = session::run(session::SessionParams {
                        bind_addr: addr,
                        cert_path: &cert,
                        key_path: &key,
                        stream,
                        desktop_width: 320,
                        desktop_height: 240,
                        dispatcher,
                    })
                    .await;
                }
            });

            handshake(addr).await;
            server.abort();
        })
        .await;

    source.stop().await.expect("stop frame source");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Drives one client through X.224 negotiation and the TLS handshake.
async fn handshake(addr: SocketAddr) {
    let mut tcp = connect_with_retries(addr).await;

    // --- X.224 negotiation (MS-RDPBCGR 2.2.1.1 / 2.2.1.2) ---
    tcp.write_all(&X224_CONNECTION_REQUEST)
        .await
        .expect("send connection request");

    let mut tpkt = [0u8; 4];
    tcp.read_exact(&mut tpkt).await.expect("read TPKT header");
    assert_eq!(tpkt[0], 0x03, "TPKT version must be 3");
    let total = usize::from(u16::from_be_bytes([tpkt[2], tpkt[3]]));
    assert!(total > 4, "TPKT length {total} must cover more than the header");

    let mut rest = vec![0u8; total - 4];
    tcp.read_exact(&mut rest).await.expect("read connection confirm");
    assert_eq!(rest[1], X224_CONNECTION_CONFIRM, "expected an X.224 Connection Confirm");

    // The negotiation response follows the 7-byte X.224 header.
    let neg = &rest[7..];
    assert_eq!(neg[0], RDP_NEG_RSP, "expected RDP_NEG_RSP");
    let selected = u32::from_le_bytes([neg[4], neg[5], neg[6], neg[7]]);
    assert_eq!(selected, PROTOCOL_SSL, "server must select TLS, not NLA (ADR-0004)");

    // --- TLS handshake ---
    let provider = ironrdp_server::tokio_rustls::rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(ironrdp_server::tokio_rustls::rustls::crypto::aws_lc_rs::default_provider()));

    let config = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .expect("default protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCert(provider)))
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(config));
    let server_name = ServerName::try_from("localhost").expect("valid server name");
    let tls_stream = tokio::time::timeout(Duration::from_secs(10), connector.connect(server_name, tcp))
        .await
        .expect("TLS handshake timed out")
        .expect("TLS handshake failed");

    let (_, connection) = tls_stream.get_ref();
    assert!(
        connection.protocol_version().is_some(),
        "handshake must have settled on a TLS version"
    );
}
