//! A headless RDP client that connects to our own server and reports what it
//! actually received.
//!
//! Exists because "the client shows a black screen" is not a diagnosis. This
//! probe answers the three questions that separate the possible causes:
//!
//! * what desktop size did the server negotiate?
//! * did any bitmap updates arrive at all?
//! * is the decoded image actually non-black?
//!
//! It is a diagnostic tool, not part of the server: it is the one place outside
//! `src/session/` allowed to speak IronRDP, and it does so from the *client*
//! side (`tests/architecture.rs` only guards `src/`).
//!
//! ```text
//! cargo run --release --example probe_client -- 127.0.0.1 3389 /tmp/probe.ppm
//! ```

#![allow(clippy::print_stdout)]

use core::time::Duration;
use std::io::Write as _;
use std::net::TcpStream;

use anyhow::{Context as _, Result, bail};
use ironrdp_connector::sspi::network_client::NetworkClient;
use ironrdp_connector::sspi::{Error as SspiError, ErrorKind as SspiErrorKind, NetworkRequest};
use ironrdp_connector::{ClientConnector, ConnectionResult, Credentials, DesktopSize};
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_pdu::gcc::KeyboardType;
use ironrdp_pdu::rdp::capability_sets::{BitmapCodecs, MajorPlatformType};
use ironrdp_pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{ActiveStageBuilder, ActiveStageOutput};
use tokio_rustls::rustls;

/// How long to keep reading updates before reporting.
const OBSERVE: Duration = Duration::from_secs(4);

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_env("IRONRDP_LOG"))
        .init();

    let mut args = std::env::args().skip(1);
    let host = args.next().unwrap_or_else(|| "127.0.0.1".to_owned());
    let port: u16 = args.next().and_then(|p| p.parse().ok()).unwrap_or(3389);
    let output = args.next().filter(|p| p != "-");

    println!("connecting to {host}:{port}");
    let (result, framed) = connect(&host, port)?;

    let size = result.desktop_size;
    println!("negotiated desktop size: {}x{}", size.width, size.height);

    let mut image = DecodedImage::new(PixelFormat::RgbA32, size.width, size.height);
    let stats = active_stage(result, framed, &mut image)?;

    println!("frames processed: {}", stats.frames);
    println!("graphics updates: {}", stats.updates);
    if let Some(error) = &stats.error {
        println!("stopped on: {error}");
    }

    let non_black = image
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[0] | px[1] | px[2] != 0)
        .count();
    let total = usize::from(image.width()) * usize::from(image.height());
    #[expect(clippy::cast_precision_loss, reason = "diagnostic percentage only")]
    let percent = non_black as f64 * 100.0 / total as f64;
    println!("non-black pixels: {non_black}/{total} ({percent:.1}%)");

    if let Some(path) = output {
        let mut out = Vec::new();
        write!(out, "P6\n{} {}\n255\n", image.width(), image.height())?;
        for px in image.data().as_chunks::<4>().0 {
            out.extend_from_slice(&px[0..3]);
        }
        std::fs::write(&path, out)?;
        println!("wrote {path}");
    }

    if stats.updates == 0 {
        bail!("no graphics updates arrived — the server negotiated a session but sent nothing usable");
    }
    if non_black == 0 {
        bail!("updates arrived but every pixel is black");
    }
    Ok(())
}

#[derive(Default)]
struct Stats {
    frames: usize,
    updates: usize,
    error: Option<String>,
}

/// CredSSP is disabled, so this is never called; it exists to satisfy the
/// signature of `connect_finalize`.
struct NoNetworkClient;

impl NetworkClient for NoNetworkClient {
    fn send(&self, _request: &NetworkRequest) -> ironrdp_connector::sspi::Result<Vec<u8>> {
        Err(SspiError::new(
            SspiErrorKind::UnsupportedFunction,
            "the probe client does not perform CredSSP",
        ))
    }
}

fn client_config() -> ironrdp_connector::Config {
    ironrdp_connector::Config {
        credentials: Credentials::UsernamePassword {
            username: "probe".to_owned(),
            password: "probe".to_owned(),
        },
        domain: None,
        // Request PROTOCOL_SSL in the X.224 negotiation; the TLS upgrade
        // itself is performed below, by us.
        enable_tls: true,
        // Our server is TLS-only in wave 1 (ADR-0004).
        enable_credssp: false,
        keyboard_type: KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_layout: 0,
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        // Deliberately small: the server does not honour the client's request
        // by default, so whatever comes back is the server's own choice — which
        // is exactly what we want to observe.
        desktop_size: DesktopSize {
            width: 1280,
            height: 1024,
        },
        // Advertise no codecs at all, forcing the server onto the plain
        // RDP 6.0 bitmap path that wave 1 is specified against. Passing `None`
        // here would advertise RemoteFX and QOI and let the server choose.
        // Which codecs the probe advertises is the whole point of the probe:
        // pass a comma-separated list as the 4th argument (e.g. `remotefx`),
        // or nothing to advertise none and force the plain RDP 6.0 bitmap path.
        bitmap: Some(ironrdp_connector::BitmapConfig {
            lossy_compression: false,
            color_depth: 32,
            codecs: requested_codecs(),
        }),
        client_build: 0,
        client_name: "mac-rdp-server-probe".to_owned(),
        client_dir: "C:\\Windows\\System32\\mstscax.dll".to_owned(),
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
    }
}

/// Reads the codec list from the 4th CLI argument.
///
/// `none` (or a missing argument) advertises no codecs at all, forcing the
/// server onto the plain RDP 6.0 bitmap path. Note that an *empty* list passed
/// to `client_codecs_capabilities` means "the defaults" (RemoteFX + QOI), not
/// "nothing" — which is exactly the trap this server fell into.
fn requested_codecs() -> BitmapCodecs {
    let list = std::env::args().nth(4).unwrap_or_else(|| "none".to_owned());
    if list == "none" {
        return BitmapCodecs(Vec::new());
    }
    let names: Vec<&str> = list.split(',').filter(|s| !s.is_empty()).collect();
    ironrdp_pdu::rdp::capability_sets::client_codecs_capabilities(&names)
        .unwrap_or_else(|e| panic!("bad codec list: {e}"))
}

type UpgradedFramed = ironrdp_blocking::Framed<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>;

fn connect(host: &str, port: u16) -> Result<(ConnectionResult, UpgradedFramed)> {
    use std::net::ToSocketAddrs as _;

    let addr = (host, port)
        .to_socket_addrs()?
        .next()
        .context("could not resolve the server address")?;

    let tcp = TcpStream::connect(addr).context("TCP connect")?;
    // Bounds the active stage: once the server goes quiet we stop reading.
    tcp.set_read_timeout(Some(OBSERVE)).context("set read timeout")?;
    let client_addr = tcp.local_addr()?;

    let mut framed = ironrdp_blocking::Framed::new(tcp);
    let mut connector = ClientConnector::new(client_config(), client_addr);

    let should_upgrade = ironrdp_blocking::connect_begin(&mut framed, &mut connector).context("connect_begin")?;

    let initial = framed.into_inner_no_leftover();
    let (tls_stream, server_public_key) = tls_upgrade(initial, host).context("TLS upgrade")?;
    let upgraded = ironrdp_blocking::mark_as_upgraded(should_upgrade, &mut connector);
    let mut framed = ironrdp_blocking::Framed::new(tls_stream);

    let result = ironrdp_blocking::connect_finalize(
        upgraded,
        connector,
        &mut framed,
        &mut NoNetworkClient,
        host.to_owned().into(),
        server_public_key,
        None,
    )
    .context("connect_finalize")?;

    Ok((result, framed))
}

fn active_stage(result: ConnectionResult, mut framed: UpgradedFramed, image: &mut DecodedImage) -> Result<Stats> {
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

    let mut stats = Stats::default();
    let deadline = std::time::Instant::now() + OBSERVE;

    while std::time::Instant::now() < deadline {
        let (action, payload) = match framed.read_pdu() {
            Ok(pdu) => pdu,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => break,
            Err(e) => {
                stats.error = Some(e.to_string());
                break;
            }
        };
        stats.frames += 1;

        let outputs = match stage.process(image, action, &payload) {
            Ok(outputs) => outputs,
            Err(e) => {
                stats.error = Some(e.to_string());
                break;
            }
        };

        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(frame) => framed.write_all(&frame).context("write response")?,
                ActiveStageOutput::GraphicsUpdate(_) => stats.updates += 1,
                ActiveStageOutput::Terminate(_) => return Ok(stats),
                _ => {}
            }
        }
    }

    Ok(stats)
}

fn tls_upgrade(
    stream: TcpStream,
    server_name: &str,
) -> Result<(rustls::StreamOwned<rustls::ClientConnection, TcpStream>, Vec<u8>)> {
    let mut config = rustls::client::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(danger::NoCertificateVerification))
        .with_no_client_auth();
    config.key_log = std::sync::Arc::new(rustls::KeyLogFile::new());
    config.resumption = rustls::client::Resumption::disabled();

    let name = rustls::pki_types::ServerName::try_from(server_name.to_owned())?;
    let client = rustls::ClientConnection::new(std::sync::Arc::new(config), name)?;
    let mut tls = rustls::StreamOwned::new(client, stream);
    tls.flush()?;

    let cert = tls
        .conn
        .peer_certificates()
        .and_then(|certs| certs.first())
        .context("the server presented no certificate")?;

    let public_key = {
        use x509_cert::der::Decode as _;
        let cert = x509_cert::Certificate::from_der(cert)?;
        cert.tbs_certificate()
            .subject_public_key_info()
            .subject_public_key
            .as_bytes()
            .context("subject public key BIT STRING is not aligned")?
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
                SignatureScheme::RSA_PKCS1_SHA512,
                SignatureScheme::ECDSA_NISTP521_SHA512,
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::RSA_PSS_SHA512,
                SignatureScheme::ED25519,
            ]
        }
    }
}
