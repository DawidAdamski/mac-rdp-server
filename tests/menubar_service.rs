//! F-111 without the menu bar: the switch, the service and the socket.
//!
//! The AppKit half of F-111 needs eyes (AC-4 … AC-12, block G of
//! `specs/testy-reczne.md`). Everything underneath it does not, and that is
//! where the risk actually is: starting and stopping a server inside one
//! process, ten times over, without leaking a socket or a thread (F-111.10,
//! F-111.11, F-111.12).
//!
//! Needs no GUI and no TCC grant: the configuration asks for the synthetic test
//! pattern, so ScreenCaptureKit is never touched.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mac_rdp_server::config::{Config, EncoderKind};
use mac_rdp_server::menubar::state::{Request, ServiceState, Toggle};
use mac_rdp_server::menubar::supervisor;

/// Long enough for a certificate, a frame source and a bind; short enough that
/// a wedged switch fails the test rather than the patience of whoever runs it.
const SETTLE: Duration = Duration::from_secs(20);

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let port = listener.local_addr().expect("local address").port();
    drop(listener);
    port
}

fn test_config(tag: &str, port: u16) -> Config {
    let dir = std::env::temp_dir().join(format!("mac-rdp-menubar-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create a throwaway directory");
    Config {
        bind_addr: "127.0.0.1".parse().expect("valid address"),
        port,
        cert_path: dir.join("cert.pem"),
        key_path: dir.join("key.pem"),
        display: 0,
        max_fps: 30,
        log_level: "info".to_owned(),
        // No capture, no permission prompt, no screen: this test is about the
        // socket and the switch.
        test_pattern: Some((64, 64)),
        encoder: EncoderKind::Damage,
        // F-101.4, AC-6: loopback with no user name configured must still
        // start, which is what this test depends on.
        auth_user: None,
    }
}

fn state(toggle: &Mutex<Toggle>) -> ServiceState {
    toggle.lock().expect("the toggle lock").state().clone()
}

/// Waits for the switch to reach a state, returning what it actually reached.
fn wait_for<F>(toggle: &Mutex<Toggle>, timeout: Duration, wanted: F) -> ServiceState
where
    F: Fn(&ServiceState) -> bool,
{
    let deadline = Instant::now() + timeout;
    loop {
        let current = state(toggle);
        if wanted(&current) || Instant::now() > deadline {
            return current;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// F-111.18: nothing is listening until something asks for it. The visible half
/// of this is AC-12; this is the half a test can hold on to.
#[test]
fn f111_18_a_fresh_supervisor_listens_on_nothing() {
    let port = free_port();
    let config = test_config("fresh", port);
    let addr: SocketAddr = config.socket_addr();
    let toggle = Arc::new(Mutex::new(Toggle::new(addr)));

    let _supervisor = supervisor::spawn(config, Arc::clone(&toggle));
    // Nothing is asked for. A quarter of a second is plenty for a thread that
    // was going to start a server by itself to have done it.
    std::thread::sleep(Duration::from_millis(250));

    assert_eq!(state(&toggle), ServiceState::Stopped);
    assert!(
        TcpStream::connect(addr).is_err(),
        "the port must be closed until the service is started"
    );
}

/// F-111.10 and AC-6/AC-7 at the socket level: on, a client connects; off, the
/// port is gone — and all of it inside one process.
#[test]
fn f111_10_the_switch_opens_and_closes_the_socket() {
    let port = free_port();
    let config = test_config("switch", port);
    let addr = config.socket_addr();
    let toggle = Arc::new(Mutex::new(Toggle::new(addr)));
    let supervisor = supervisor::spawn(config, Arc::clone(&toggle));

    toggle.lock().expect("lock").click();
    supervisor.request(Request::Start);
    assert_eq!(
        wait_for(&toggle, SETTLE, |s| matches!(s, ServiceState::Listening)),
        ServiceState::Listening,
        "the service did not come up"
    );
    TcpStream::connect(addr).expect("a started service must accept a connection");

    toggle.lock().expect("lock").click();
    supervisor.request(Request::Stop);
    assert_eq!(
        wait_for(&toggle, SETTLE, |s| matches!(s, ServiceState::Stopped)),
        ServiceState::Stopped,
        "the service did not go down"
    );
    assert!(
        TcpStream::connect(addr).is_err(),
        "a stopped service must not leave the port open (F-111.10)"
    );
}

/// F-111.12 and the measurable half of AC-8: ten cycles, each one ending with
/// the port actually free. A leaked socket shows up as the next start failing,
/// a leaked runtime as the test machine slowing to a crawl.
#[test]
fn f111_12_ten_cycles_leave_nothing_behind() {
    let port = free_port();
    let config = test_config("cycles", port);
    let addr = config.socket_addr();
    let toggle = Arc::new(Mutex::new(Toggle::new(addr)));
    let supervisor = supervisor::spawn(config, Arc::clone(&toggle));

    for cycle in 1..=10 {
        toggle.lock().expect("lock").click();
        supervisor.request(Request::Start);
        assert_eq!(
            wait_for(&toggle, SETTLE, |s| matches!(s, ServiceState::Listening)),
            ServiceState::Listening,
            "cycle {cycle} did not start"
        );
        TcpStream::connect(addr).unwrap_or_else(|e| panic!("cycle {cycle} refused a connection: {e}"));

        toggle.lock().expect("lock").click();
        supervisor.request(Request::Stop);
        assert_eq!(
            wait_for(&toggle, SETTLE, |s| matches!(s, ServiceState::Stopped)),
            ServiceState::Stopped,
            "cycle {cycle} did not stop"
        );
        assert!(
            TcpListener::bind(addr).is_ok(),
            "cycle {cycle} left the port taken (F-111.12)"
        );
    }
}

/// F-111.11: a start that cannot have its port leaves the switch off and
/// carries the reason. In the menu this is the difference between a line
/// saying why and an icon that looks like the click never landed.
#[test]
fn f111_11_a_busy_port_fails_with_a_reason_and_stays_off() {
    // Held for the whole test: this is the port being fought over.
    let occupied = TcpListener::bind("127.0.0.1:0").expect("occupy a port");
    let port = occupied.local_addr().expect("local address").port();
    let config = test_config("busy", port);
    let addr = config.socket_addr();
    let toggle = Arc::new(Mutex::new(Toggle::new(addr)));
    let supervisor = supervisor::spawn(config, Arc::clone(&toggle));

    toggle.lock().expect("lock").click();
    supervisor.request(Request::Start);

    let reached = wait_for(&toggle, SETTLE, |s| matches!(s, ServiceState::Failed(_)));
    match reached {
        ServiceState::Failed(reason) => {
            assert!(
                reason.contains(&port.to_string()),
                "the reason must name the address: {reason}"
            );
        }
        other => panic!("expected a failed start, got {other:?}"),
    }

    // And the switch is usable again rather than wedged: clicking asks for a
    // fresh attempt.
    assert_eq!(toggle.lock().expect("lock").click(), Some(Request::Start));
}

/// The certificate is created by the service, not by the menu: a first start
/// with nothing on disk has to work, because in menu-bar mode there is no
/// earlier CLI run to have made one.
#[test]
fn f111_18_the_first_start_creates_its_own_certificate() {
    let port = free_port();
    let config = test_config("cert", port);
    let cert: PathBuf = config.cert_path.clone();
    let _ = std::fs::remove_file(&cert);
    let addr = config.socket_addr();
    let toggle = Arc::new(Mutex::new(Toggle::new(addr)));
    let supervisor = supervisor::spawn(config, Arc::clone(&toggle));

    toggle.lock().expect("lock").click();
    supervisor.request(Request::Start);
    assert_eq!(
        wait_for(&toggle, SETTLE, |s| matches!(s, ServiceState::Listening)),
        ServiceState::Listening
    );
    assert!(cert.is_file(), "the service must generate its own certificate");

    supervisor.request(Request::Stop);
    wait_for(&toggle, SETTLE, |s| matches!(s, ServiceState::Stopped));
}
