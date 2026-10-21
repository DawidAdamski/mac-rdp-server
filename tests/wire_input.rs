//! F-003.7: a mouse button has to survive the whole wire path, not just our
//! conversion function.
//!
//! The middle button was missing for months with every unit test green, because
//! the loss happened inside the protocol engine: `ironrdp-server` 0.13.0 as
//! published decoded PTRFLAGS_BUTTON3 as a plain move (ADR-0007). Since
//! ADR-0014 the engine is our fork, and this test is what proves the patch is
//! actually in the build: a real IronRDP client connects over TLS and sends a
//! Share Data Input PDU, and the test reads what came out of the server's
//! input queue.
//!
//! Nothing is injected into macOS. The server is given an [`InputDispatcher`]
//! whose receiving end belongs to the test instead of to a sink, so the test
//! needs no Screen Recording and no Accessibility grant and clicks nothing on
//! the machine it runs on.
//!
//! The connection helpers are copied from `tests/rdp_pixels.rs`, which F-003.20
//! and F-008.13 do not allow us to change.

use core::time::Duration;
use std::io::Write as _;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;

use ironrdp_connector::{BitmapConfig, ClientConnector, ConnectionResult, Credentials, DesktopSize};
use ironrdp_pdu::WriteBuf;
use ironrdp_pdu::gcc::KeyboardType;
use ironrdp_pdu::input::mouse::PointerFlags;
use ironrdp_pdu::input::{InputEvent as PduInputEvent, InputEventPdu, MousePdu};
use ironrdp_pdu::rdp::capability_sets::{BitmapCodecs, MajorPlatformType};
use ironrdp_pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp_pdu::rdp::headers::ShareDataPdu;
use ironrdp_session::{ActiveStage, ActiveStageBuilder};
use mac_rdp_server::capture::FrameSource as _;
use mac_rdp_server::capture::test_pattern::TestPatternSource;
use mac_rdp_server::config::EncoderKind;
use mac_rdp_server::input::{InputDispatcher, InputEvent, MouseButton, PointerEvent};
use mac_rdp_server::{session, tls};
use tokio_rustls::rustls;

const PATTERN_WIDTH: u32 = 640;
const PATTERN_HEIGHT: u32 = 480;
const OBSERVE: Duration = Duration::from_secs(5);
/// How long to wait for the events to come back out of the server.
const DELIVERY: Duration = Duration::from_secs(10);

/// What the client sends: middle button down, middle button up, and a wheel
/// rotation, in that order.
///
/// The wheel is in the same burst on purpose. PTRFLAGS_BUTTON3 is named
/// "middle button **or wheel**" in MS-RDPBCGR 2.2.8.1.1.3.1.1.3, and the
/// obvious wrong fix — checking that flag before the wheel flags — turns every
/// scroll into a middle click. One test, both halves.
fn client_burst(addr: SocketAddr) -> anyhow::Result<()> {
    let (result, mut framed) = connect(addr, BitmapCodecs(Vec::new()))?;
    let stage = build_stage(&result, 0);

    let mouse = |flags, wheel| {
        PduInputEvent::Mouse(MousePdu {
            flags,
            number_of_wheel_rotation_units: wheel,
            x_position: 32,
            y_position: 64,
        })
    };
    let pdu = ShareDataPdu::Input(InputEventPdu(vec![
        mouse(PointerFlags::MIDDLE_BUTTON_OR_WHEEL | PointerFlags::DOWN, 0),
        mouse(PointerFlags::MIDDLE_BUTTON_OR_WHEEL, 0),
        mouse(PointerFlags::VERTICAL_WHEEL, -120),
    ]));

    let mut buf = WriteBuf::new();
    let written = stage.encode_static(&mut buf, pdu)?;
    framed.write_all(&buf[..written])?;

    // Stay connected long enough for the server to read the PDU; a client that
    // disconnects immediately would also trigger the F-003.24 session reset
    // before the events are read.
    std::thread::sleep(Duration::from_millis(500));
    Ok(())
}

/// Runs the server with the test holding the input queue, lets `client` talk to
/// it, and returns the pointer events the server produced.
fn pointer_events_from_the_wire() -> Vec<PointerEvent> {
    let dir = std::env::temp_dir().join(format!("mac-rdp-wire-input-{}", std::process::id()));
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

    let client = std::thread::spawn(move || client_burst(addr));

    runtime.block_on(async move {
        let mut source = TestPatternSource::new(PATTERN_WIDTH, PATTERN_HEIGHT);
        let stream = source.start().await.expect("start the frame source");
        // No sink: the receiving end of the queue is the assertion.
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let dispatcher = InputDispatcher::new(tx);

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
                        encoder: EncoderKind::Damage,
                        clients: tokio::sync::watch::channel(0).0,
                        // These tests wait on the socket itself, so the
                        // readiness callback has nothing to tell them.
                        ready: Box::new(|| {}),
                    })
                    .await;
                });

                let collected = tokio::time::timeout(DELIVERY, async {
                    let mut pointers = Vec::new();
                    while pointers.len() < 3 {
                        match rx.recv().await {
                            Some(InputEvent::Pointer(ev)) => pointers.push(ev),
                            // Geometry and session resets are not what this
                            // test is about.
                            Some(_) => {}
                            None => break,
                        }
                    }
                    pointers
                })
                .await
                .unwrap_or_default();

                let outcome = client.join().expect("client thread");
                server.abort();
                source.stop().await.ok();
                let _ = std::fs::remove_dir_all(&dir);
                outcome.expect("the client failed to talk to the server");
                collected
            })
            .await
    })
}

/// F-003.7: PTRFLAGS_BUTTON3 arrives as a middle button press and release, and
/// a wheel rotation in the same burst is still a scroll.
#[test]
fn f003_7_the_middle_button_survives_the_whole_wire_path() {
    let events = pointer_events_from_the_wire();

    assert_eq!(
        events,
        vec![
            PointerEvent::Button {
                button: MouseButton::Middle,
                pressed: true
            },
            PointerEvent::Button {
                button: MouseButton::Middle,
                pressed: false
            },
            PointerEvent::Scroll { dx: 0, dy: -120 },
        ],
        "the engine lost or mistranslated the middle button; is the ADR-0014 patch in this build?"
    );
}

/// The test negotiates no static channels, so each stage gets an empty set.
fn build_stage(result: &ConnectionResult, share_id: u32) -> ActiveStage {
    ActiveStageBuilder {
        static_channels: Default::default(),
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id,
        compression_type: result.compression_type,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    }
    .build()
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
