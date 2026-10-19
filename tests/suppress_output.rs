//! F-008.12: Suppress Output end to end, with a real IronRDP client.
//!
//! The client pauses display updates, and the server must go quiet even though
//! the test pattern keeps changing (F-008.4). When the client resumes, the server
//! must repaint the **whole** desktop without a deactivation-reactivation
//! (F-008.6, F-008.7v2). The check is that the client's image, stale after the
//! pause, matches a rendered pattern frame **exactly** once the repaint has
//! arrived: no fragment of the paused desktop may survive.
//!
//! What this cannot check — and F-008.7v2 records it as a deviation — is a client
//! that throws its image away while paused. IronRDP diffs every bitmap against
//! its own copy of what it sent and drops the tiles that match, so such a client
//! would not get them back. Asserting a repaint onto a freshly black image fails
//! for exactly that reason.
//!
//! The resume is sent the way real clients send it and the way that used to drop
//! the session: Refresh Rect and Suppress Output back to back, with no wait in
//! between (F-008 §6, 2026-09-24).
//!
//! The connection helpers are copied from `tests/rdp_pixels.rs`, which F-008.13
//! does not allow us to change.

use core::time::Duration;
use std::io::Write as _;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;

use ironrdp_connector::{BitmapConfig, ClientConnector, ConnectionResult, Credentials, DesktopSize};
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_pdu::WriteBuf;
use ironrdp_pdu::gcc::KeyboardType;
use ironrdp_pdu::geometry::InclusiveRectangle;
use ironrdp_pdu::rdp::capability_sets::{BitmapCodecs, MajorPlatformType};
use ironrdp_pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp_pdu::rdp::headers::ShareDataPdu;
use ironrdp_pdu::rdp::refresh_rectangle::RefreshRectanglePdu;
use ironrdp_pdu::rdp::suppress_output::SuppressOutputPdu;
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{ActiveStage, ActiveStageBuilder, ActiveStageOutput};
use mac_rdp_server::capture::FrameSource as _;
use mac_rdp_server::capture::test_pattern::TestPatternSource;
use mac_rdp_server::config::EncoderKind;
use mac_rdp_server::input::{self, logging::LoggingSink};
use mac_rdp_server::{session, tls};
use tokio_rustls::rustls;

const PATTERN_WIDTH: u32 = 640;
const PATTERN_HEIGHT: u32 = 480;
const OBSERVE: Duration = Duration::from_secs(5);
/// Updates already on the wire when the pause is sent are allowed to arrive.
const GRACE: Duration = Duration::from_millis(300);
/// F-008.12 asks for at least 500 ms of silence; the pattern ticks once a
/// second, so this window is guaranteed to contain a change.
const QUIET: Duration = Duration::from_millis(2_000);

/// What the client saw.
#[derive(Debug, Default)]
struct Observation {
    first_frame: bool,
    updates_while_paused: usize,
    /// The session was deactivated — which is what must no longer happen.
    deactivated: bool,
    /// The pattern frame the client was left with while paused.
    counter_before_resume: Option<u64>,
    repaint_matched: Option<u64>,
}

fn run_server_with<T: Send + 'static>(
    encoder: EncoderKind,
    client: impl FnOnce(SocketAddr) -> T + Send + 'static,
) -> T {
    let dir = std::env::temp_dir().join(format!("mac-rdp-suppress-{}-{encoder:?}", std::process::id()));
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

    let client = std::thread::spawn(move || client(addr));

    runtime.block_on(async move {
        let mut source = TestPatternSource::new(PATTERN_WIDTH, PATTERN_HEIGHT);
        let stream = source.start().await.expect("start the frame source");
        let dispatcher = input::spawn(|| Box::new(LoggingSink::new()), 16);

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
                        encoder,
                        clients: tokio::sync::watch::channel(0).0,
                    })
                    .await;
                });

                let result = tokio::task::spawn_blocking(move || client.join().expect("client thread"))
                    .await
                    .expect("join client");

                server.abort();
                source.stop().await.ok();
                let _ = std::fs::remove_dir_all(&dir);
                result
            })
            .await
    })
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

fn set_read_timeout(framed: &mut Framed, timeout: Duration) {
    framed
        .get_inner_mut()
        .0
        .sock
        .set_read_timeout(Some(timeout))
        .expect("set read timeout");
}

/// Sends Refresh Rect immediately followed by Suppress Output
/// `allowDisplayUpdates = 1`, in one write, exactly as the clients that dropped
/// the session did (F-008.12, amendment of 2026-09-24).
fn send_resume_burst(stage: &ActiveStage, framed: &mut Framed) {
    let area = InclusiveRectangle {
        left: 0,
        top: 0,
        right: u16::try_from(PATTERN_WIDTH - 1).expect("fits"),
        bottom: u16::try_from(PATTERN_HEIGHT - 1).expect("fits"),
    };
    let mut buf = WriteBuf::new();
    let refresh = stage
        .encode_static(
            &mut buf,
            ShareDataPdu::RefreshRectangle(RefreshRectanglePdu {
                areas_to_refresh: vec![area.clone()],
            }),
        )
        .expect("encode Refresh Rect");
    let mut burst = buf[..refresh].to_vec();
    let mut buf = WriteBuf::new();
    let suppress = stage
        .encode_static(
            &mut buf,
            ShareDataPdu::SuppressOutput(SuppressOutputPdu {
                desktop_rect: Some(area),
            }),
        )
        .expect("encode Suppress Output");
    burst.extend_from_slice(&buf[..suppress]);
    framed.write_all(&burst).expect("send the resume burst");
}

fn send_suppress(stage: &ActiveStage, framed: &mut Framed, allow: bool) {
    let desktop_rect = allow.then(|| InclusiveRectangle {
        left: 0,
        top: 0,
        right: u16::try_from(PATTERN_WIDTH - 1).expect("fits"),
        bottom: u16::try_from(PATTERN_HEIGHT - 1).expect("fits"),
    });
    let mut buf = WriteBuf::new();
    let written = stage
        .encode_static(
            &mut buf,
            ShareDataPdu::SuppressOutput(SuppressOutputPdu { desktop_rect }),
        )
        .expect("encode Suppress Output");
    framed.write_all(&buf[..written]).expect("send Suppress Output");
}

/// Reads and processes PDUs until `until` returns true or `limit` passes.
/// Returns how many graphics updates were applied.
fn pump(
    stage: &mut ActiveStage,
    framed: &mut Framed,
    image: &mut DecodedImage,
    limit: Duration,
    mut until: impl FnMut(&ActiveStageOutput, &DecodedImage) -> bool,
) -> (usize, bool) {
    let deadline = std::time::Instant::now() + limit;
    let mut updates = 0;
    while std::time::Instant::now() < deadline {
        let Ok((action, payload)) = framed.read_pdu() else {
            continue; // read timeout: nothing arrived, keep waiting
        };
        let outputs = stage.process(image, action, &payload).expect("process PDU");
        for out in outputs {
            match &out {
                ActiveStageOutput::ResponseFrame(frame) => framed.write_all(frame).expect("send response"),
                ActiveStageOutput::GraphicsUpdate(_) => updates += 1,
                _ => {}
            }
            if until(&out, image) {
                return (updates, true);
            }
        }
    }
    (updates, false)
}

fn run_suppress_client(addr: SocketAddr) -> Observation {
    let (result, mut framed) = connect(addr, BitmapCodecs(Vec::new())).expect("connect to the server under test");
    let mut observation = Observation::default();
    let size = result.desktop_size;
    let mut image = DecodedImage::new(PixelFormat::RgbA32, size.width, size.height);
    let mut stage = build_stage(&result, result.share_id);
    set_read_timeout(&mut framed, Duration::from_millis(100));

    // 1. The session starts with a full frame.
    let (_, first) = pump(&mut stage, &mut framed, &mut image, OBSERVE, |out, _| {
        matches!(out, ActiveStageOutput::GraphicsUpdate(_))
    });
    observation.first_frame = first;

    // 2. Pause. After the grace period nothing may arrive, though the pattern
    //    keeps ticking.
    send_suppress(&stage, &mut framed, false);
    pump(&mut stage, &mut framed, &mut image, GRACE, |_, _| false);
    let (paused, _) = pump(&mut stage, &mut framed, &mut image, QUIET, |_, _| false);
    observation.updates_while_paused = paused;

    // 3. Resume, with the burst that used to kill the session. The image is
    //    stale by now — several pattern ticks old — so an exact match means the
    //    repaint really arrived.
    let before = matching_counter(&image);
    observation.counter_before_resume = before;
    send_resume_burst(&stage, &mut framed);
    let mut matched = None;
    let mut deactivated = false;
    pump(&mut stage, &mut framed, &mut image, OBSERVE, |out, image| {
        match out {
            ActiveStageOutput::GraphicsUpdate(_) => {
                // A *newer* pattern frame: the picture the client was left
                // with during the pause no longer counts.
                matched = matching_counter(image).filter(|counter| Some(*counter) != before);
            }
            ActiveStageOutput::DeactivateAll => deactivated = true,
            ActiveStageOutput::Terminate(_) => deactivated = true,
            _ => {}
        }
        matched.is_some() || deactivated
    });
    observation.deactivated = deactivated;
    observation.repaint_matched = matched;
    observation
}

fn matching_counter(image: &DecodedImage) -> Option<u64> {
    (0..=60).find(|counter| matches_pattern(image, *counter))
}

fn matches_pattern(image: &DecodedImage, counter: u64) -> bool {
    let expected = mac_rdp_server::capture::test_pattern::render(PATTERN_WIDTH, PATTERN_HEIGHT, counter);
    let decoded = image.data();
    let decoded_stride = usize::from(image.width()) * 4;

    for y in 0..PATTERN_HEIGHT {
        for x in 0..PATTERN_WIDTH {
            let Some([b, g, r, _]) = expected.pixel(x, y) else {
                return false;
            };
            let offset = (y as usize) * decoded_stride + (x as usize) * 4;
            let Some(px) = decoded.get(offset..offset + 3) else {
                return false;
            };
            if px != [r, g, b] {
                return false;
            }
        }
    }
    true
}

fn check(observation: &Observation) {
    assert!(observation.first_frame, "the session never sent its first frame");
    assert_eq!(
        observation.updates_while_paused, 0,
        "F-008.4: the server kept sending while the client had paused updates"
    );
    assert!(
        !observation.deactivated,
        "F-008.7v2: resuming must not deactivate the session — that is what dropped real clients"
    );
    assert_ne!(
        observation.repaint_matched, observation.counter_before_resume,
        "the picture after the resume must be newer than the one left by the pause"
    );
    assert!(
        observation.repaint_matched.is_some(),
        "F-008.6: after the resume the client's image never became a complete, current pattern frame"
    );
}

/// F-008.12, F-008.4, F-008.6, F-008.7v2 with the damage encoder.
#[test]
fn f008_12_suppress_output_pauses_and_resume_repaints_everything() {
    check(&run_server_with(EncoderKind::Damage, run_suppress_client));
}

/// F-008.10: the same with the full-frame encoder.
#[test]
fn f008_10_suppress_output_behaves_the_same_with_the_full_encoder() {
    check(&run_server_with(EncoderKind::Full, run_suppress_client));
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
