//! End-to-end pixel check: a real RDP client connects and must see the pixels
//! the server intended to send.
//!
//! This test exists because of a specific failure that every other test passed
//! through unnoticed. The server never called `with_bitmap_codecs`, so it
//! inherited IronRDP's defaults (RemoteFX, QOI, QOIZ). Every real client
//! advertises RemoteFX, so the server switched to Surface Bits, and the client
//! rendered a fully black screen. Unit tests were green, the TLS handshake test
//! was green, and the frames leaving `capture` were provably correct — the
//! damage happened between the encoder and the wire.
//!
//! So: assert on the decoded pixels, and assert it for a client that advertises
//! codecs, because that is the case that broke.

use core::time::Duration;
use std::io::Write as _;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;

use ironrdp_connector::{BitmapConfig, ClientConnector, ConnectionResult, Credentials, DesktopSize};
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_pdu::gcc::KeyboardType;
use ironrdp_pdu::rdp::capability_sets::{BitmapCodecs, MajorPlatformType, client_codecs_capabilities};
use ironrdp_pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{ActiveStageBuilder, ActiveStageOutput};
use mac_rdp_server::capture::FrameSource as _;
use mac_rdp_server::capture::test_pattern::TestPatternSource;
use mac_rdp_server::input::{self, logging::LoggingSink};
use mac_rdp_server::{session, tls};
use tokio_rustls::rustls;

const PATTERN_WIDTH: u32 = 640;
const PATTERN_HEIGHT: u32 = 480;
const OBSERVE: Duration = Duration::from_secs(5);

/// Runs the server against a client advertising `codecs`, returning the
/// fraction of decoded pixels that are not black.
fn non_black_fraction(codecs: BitmapCodecs) -> f64 {
    let dir = std::env::temp_dir().join(format!("mac-rdp-pixels-{}-{:?}", std::process::id(), codecs.0.len()));
    let _ = std::fs::remove_dir_all(&dir);
    let cert = dir.join("cert.pem");
    let key = dir.join("key.pem");
    tls::ensure_self_signed(&cert, &key).expect("generate certificate");

    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve a port");
    let addr = listener.local_addr().expect("read the port");
    drop(listener);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build runtime");

    // The client runs on its own thread: `RdpServer::run` is `!Send` and must
    // stay on the runtime's LocalSet (ADR-0001).
    let client = std::thread::spawn(move || {
        // Give the listener a moment; `connect_with_retries` handles the rest.
        run_client(addr, codecs)
    });

    runtime.block_on(async move {
        let mut source = TestPatternSource::new(PATTERN_WIDTH, PATTERN_HEIGHT);
        let stream = source.start().await.expect("start the frame source");
        let dispatcher = input::spawn(Box::new(LoggingSink::new()), 16);

        let local = tokio::task::LocalSet::new();
        local
            .run_until(async move {
                let server = tokio::task::spawn_local(async move {
                    let _ = session::run(session::SessionParams {
                        bind_addr: addr,
                        cert_path: &cert,
                        key_path: &key,
                        stream,
                        desktop_width: u16::try_from(PATTERN_WIDTH).expect("fits"),
                        desktop_height: u16::try_from(PATTERN_HEIGHT).expect("fits"),
                        dispatcher,
                    })
                    .await;
                });

                let fraction = tokio::task::spawn_blocking(move || client.join().expect("client thread"))
                    .await
                    .expect("join client");

                server.abort();
                source.stop().await.ok();
                let _ = std::fs::remove_dir_all(&dir);
                fraction
            })
            .await
    })
}

fn run_client(addr: SocketAddr, codecs: BitmapCodecs) -> f64 {
    let (result, framed) = connect(addr, codecs).expect("connect to the server under test");
    let size = result.desktop_size;
    let mut image = DecodedImage::new(PixelFormat::RgbA32, size.width, size.height);
    drive(result, framed, &mut image);

    let non_black = image
        .data()
        .chunks_exact(4)
        .filter(|px| px[0] | px[1] | px[2] != 0)
        .count();
    let total = usize::from(image.width()) * usize::from(image.height());
    #[expect(clippy::cast_precision_loss, reason = "ratio for an assertion, not arithmetic")]
    {
        non_black as f64 / total as f64
    }
}

struct NoNetworkClient;

impl ironrdp_connector::sspi::network_client::NetworkClient for NoNetworkClient {
    fn send(&self, _: &ironrdp_connector::sspi::NetworkRequest) -> ironrdp_connector::sspi::Result<Vec<u8>> {
        Err(ironrdp_connector::sspi::Error::new(
            ironrdp_connector::sspi::ErrorKind::UnsupportedFunction,
            "CredSSP is disabled",
        ))
    }
}

type Framed = ironrdp_blocking::Framed<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>;

fn connect(addr: SocketAddr, codecs: BitmapCodecs) -> anyhow::Result<(ConnectionResult, Framed)> {
    let tcp = connect_with_retries(addr);
    tcp.set_read_timeout(Some(OBSERVE))?;
    let client_addr = tcp.local_addr()?;

    let config = ironrdp_connector::Config {
        credentials: Credentials::UsernamePassword {
            username: "test".to_owned(),
            password: "test".to_owned(),
        },
        domain: None,
        enable_tls: true,
        enable_credssp: false,
        keyboard_type: KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_layout: 0,
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        desktop_size: DesktopSize {
            width: 1280,
            height: 1024,
        },
        bitmap: Some(BitmapConfig {
            lossy_compression: false,
            color_depth: 32,
            codecs,
        }),
        client_build: 0,
        client_name: "mac-rdp-server-test".to_owned(),
        client_dir: String::new(),
        platform: MajorPlatformType::MACINTOSH,
        enable_server_pointer: false,
        request_data: None,
        autologon: false,
        enable_audio_playback: false,
        compression_type: None,
        pointer_software_rendering: true,
        multitransport_flags: None,
        performance_flags: PerformanceFlags::default(),
        desktop_scale_factor: 0,
        hardware_id: None,
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        alternate_shell: String::new(),
        work_dir: String::new(),
    };

    let mut framed = ironrdp_blocking::Framed::new(tcp);
    let mut connector = ClientConnector::new(config, client_addr);
    let should_upgrade = ironrdp_blocking::connect_begin(&mut framed, &mut connector)?;
    let initial = framed.into_inner_no_leftover();
    let (tls, public_key) = tls_upgrade(initial)?;
    let upgraded = ironrdp_blocking::mark_as_upgraded(should_upgrade, &mut connector);
    let mut framed = ironrdp_blocking::Framed::new(tls);

    let result = ironrdp_blocking::connect_finalize(
        upgraded,
        connector,
        &mut framed,
        &mut NoNetworkClient,
        "localhost".to_owned().into(),
        public_key,
        None,
    )?;
    Ok((result, framed))
}

fn connect_with_retries(addr: SocketAddr) -> TcpStream {
    for _ in 0..200 {
        if let Ok(stream) = TcpStream::connect(addr) {
            return stream;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("the server never started listening on {addr}");
}

fn drive(result: ConnectionResult, mut framed: Framed, image: &mut DecodedImage) {
    let mut stage = ActiveStageBuilder {
        static_channels: result.static_channels,
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id: result.share_id,
        compression_type: result.compression_type,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    }
    .build();

    let deadline = std::time::Instant::now() + OBSERVE;
    let mut updates = 0_usize;

    // One full-screen update is enough; keep reading only until we have it.
    while std::time::Instant::now() < deadline && updates == 0 {
        let Ok((action, payload)) = framed.read_pdu() else {
            break;
        };
        let Ok(outputs) = stage.process(image, action, &payload) else {
            break;
        };
        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(frame) => {
                    if framed.write_all(&frame).is_err() {
                        return;
                    }
                }
                ActiveStageOutput::GraphicsUpdate(_) => updates += 1,
                ActiveStageOutput::Terminate(_) => return,
                _ => {}
            }
        }
    }
}

fn tls_upgrade(
    stream: TcpStream,
) -> anyhow::Result<(rustls::StreamOwned<rustls::ClientConnection, TcpStream>, Vec<u8>)> {
    use anyhow::Context as _;

    let mut config = rustls::client::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification))
        .with_no_client_auth();
    config.resumption = rustls::client::Resumption::disabled();

    let name = rustls::pki_types::ServerName::try_from("localhost")?;
    let client = rustls::ClientConnection::new(Arc::new(config), name)?;
    let mut tls = rustls::StreamOwned::new(client, stream);
    tls.flush()?;

    let cert = tls
        .conn
        .peer_certificates()
        .and_then(|c| c.first())
        .context("no server certificate")?;

    let public_key = {
        use x509_cert::der::Decode as _;
        let cert = x509_cert::Certificate::from_der(cert)?;
        cert.tbs_certificate()
            .subject_public_key_info()
            .subject_public_key
            .as_bytes()
            .context("misaligned public key")?
            .to_owned()
    };
    Ok((tls, public_key))
}

#[test]
fn a_client_advertising_remotefx_still_sees_the_picture() {
    // The regression. A client that advertises codecs must not end up with a
    // black screen: the server is expected to ignore them and send Bitmap
    // Updates (F-002, ADR-0001, ADR-0006).
    let codecs = client_codecs_capabilities(&["remotefx"]).expect("build codec capabilities");
    let fraction = non_black_fraction(codecs);
    assert!(
        fraction > 0.5,
        "only {:.1}% of pixels were non-black; the client advertised RemoteFX and got a black screen",
        fraction * 100.0
    );
}

#[test]
fn a_client_advertising_no_codecs_sees_the_picture() {
    let fraction = non_black_fraction(BitmapCodecs(Vec::new()));
    assert!(
        fraction > 0.5,
        "only {:.1}% of pixels were non-black on the plain bitmap path",
        fraction * 100.0
    );
}

mod danger {
    use tokio_rustls::rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use tokio_rustls::rustls::{DigitallySignedStruct, Error, SignatureScheme, pki_types};

    #[derive(Debug)]
    pub(super) struct NoCertificateVerification;

    impl ServerCertVerifier for NoCertificateVerification {
        fn verify_server_cert(
            &self,
            _: &pki_types::CertificateDer<'_>,
            _: &[pki_types::CertificateDer<'_>],
            _: &pki_types::ServerName<'_>,
            _: &[u8],
            _: pki_types::UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &pki_types::CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &pki_types::CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![
                SignatureScheme::RSA_PKCS1_SHA256,
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::RSA_PKCS1_SHA384,
                SignatureScheme::ECDSA_NISTP384_SHA384,
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::ED25519,
            ]
        }
    }
}
