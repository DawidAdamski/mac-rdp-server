//! F-111 AC-3 — the menu bar is not allowed to change CLI mode.
//!
//! F-111 moved the whole service out of `main` and replaced `#[tokio::main]`
//! with a runtime built by hand (ADR-0015). Both are invisible refactors if
//! they are right and silent regressions if they are not, which is exactly the
//! kind of change that needs a test driving the real binary rather than a unit
//! test of the parts.
//!
//! Needs no GUI, no RDP client and no TCC grant: `--test-pattern` serves
//! synthetic frames, so ScreenCaptureKit is never touched, and `HOME` points at
//! a throwaway directory so the certificate and the default configuration file
//! (F-112.1) cannot reach into the real one.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Drops the colour escapes `tracing` writes when it thinks it has a terminal.
///
/// Needed for any assertion that spans a field name: the compact formatter
/// writes `config` and `=` wrapped in escapes, so a literal `config=…` is
/// never found in the raw bytes. The same trap cost a probe run in F-007 §8i.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        // CSI sequences end at the first byte in `@`..`~`; that is all the
        // compact formatter emits.
        for escape in chars.by_ref() {
            if escape.is_ascii_alphabetic() || escape == '~' {
                break;
            }
        }
    }
    out
}

/// A port nothing is listening on, found by asking the kernel for one.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let port = listener.local_addr().expect("local address").port();
    drop(listener);
    port
}

fn temp_home(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("mac-rdp-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create a throwaway home");
    home
}

/// Starts the server in CLI mode with its output captured.
fn spawn_server(port: u16, home: &PathBuf) -> Child {
    Command::new(env!("CARGO_BIN_EXE_mac-rdp-server"))
        .args([
            "--test-pattern",
            "64x64",
            "--bind-addr",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .env("HOME", home)
        // The server's own level, not whatever the shell running the tests has.
        .env_remove("MAC_RDP_LOG")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the server binary")
}

/// Reads the server's log until `needle` shows up, and returns everything read.
///
/// Reading rather than sleeping: the startup path opens a certificate and a
/// capture source, which takes as long as it takes.
///
/// **Stdout, not stderr**: `tracing_subscriber::fmt` writes there. Waiting on
/// the wrong pipe does not fail, it blocks for ever — `read_line` has no
/// deadline of its own, so the loop below only bounds a stream that is still
/// producing lines. The reader therefore runs on its own thread and the
/// deadline is enforced here, where it can actually be enforced.
fn wait_for_log(child: &mut Child, needle: &str, timeout: Duration) -> String {
    let stdout = child.stdout.take().expect("piped stdout");
    let needle = needle.to_owned();
    let wanted = needle.clone();
    let (found, result) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut seen = String::new();
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    seen.push_str(&line);
                    if line.contains(&wanted) {
                        let _ = found.send(Ok(seen));
                        return;
                    }
                }
            }
        }
        let _ = found.send(Err(seen));
    });

    match result.recv_timeout(timeout) {
        Ok(Ok(log)) => log,
        Ok(Err(log)) => panic!("the server log ended before `{needle}`. Log:\n{log}"),
        Err(_) => {
            let _ = child.kill();
            panic!("never saw `{needle}` in the server log within {timeout:?}");
        }
    }
}

/// AC-3: the startup log still says what it said, the server still listens, and
/// `Ctrl-C` still ends it with a zero exit code (F-001.17).
#[test]
fn f111_3_cli_mode_still_starts_serves_and_stops_on_a_signal() {
    let port = free_port();
    let home = temp_home("signal");
    let mut child = spawn_server(port, &home);

    // "accepting connections", not "serving RDP over TLS": the latter is
    // logged before `server.run()`, which is what binds the socket, so a test
    // that connected on it raced the listener and lost on a busy CI runner
    // (run 39638209007). This line comes from inside the accept loop, after
    // the bind — the same rule as F-111.11, which is about a menu that must
    // not say "listening" before there is a socket.
    let log = strip_ansi(&wait_for_log(
        &mut child,
        "accepting connections",
        Duration::from_secs(30),
    ));

    assert!(
        log.contains("starting mac-rdp-server"),
        "the startup line must survive the refactor:\n{log}"
    );
    assert!(
        log.contains("config=built-in defaults"),
        "F-112.7: the configuration source belongs in the startup line:\n{log}"
    );
    assert!(
        log.contains(&format!("127.0.0.1:{port}")),
        "the address from the flags must be the address in the log:\n{log}"
    );

    // Listening for real, not just claiming to: this is the half of AC-3 that a
    // log line cannot prove.
    std::net::TcpStream::connect(("127.0.0.1", port)).expect("the server must accept a connection");

    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT");
    assert!(status.success(), "could not signal the server");

    let exit = wait_for_exit(&mut child, Duration::from_secs(20));
    assert!(
        exit.success(),
        "Ctrl-C must end CLI mode cleanly, got {exit:?} (F-001.17)"
    );

    let _ = std::fs::remove_dir_all(&home);
}

/// A bad port is still a non-zero exit with a readable reason, rather than a
/// process that sits there having failed (F-111.11 is the menu's half of this).
#[test]
fn f111_11_a_busy_port_is_reported_and_exits_non_zero() {
    let home = temp_home("busy");
    // Held for the whole test: this is the port being fought over.
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("occupy a port");
    let port = occupied.local_addr().expect("local address").port();

    let output = Command::new(env!("CARGO_BIN_EXE_mac-rdp-server"))
        .args([
            "--test-pattern",
            "64x64",
            "--bind-addr",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .env("HOME", &home)
        .env_remove("MAC_RDP_LOG")
        .output()
        .expect("run the server binary");

    assert!(!output.status.success(), "a busy port must not look like success");
    let log = strip_ansi(&format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ));
    assert!(
        log.contains("cannot listen on") && log.contains(&port.to_string()),
        "the error must name the address it could not have:\n{log}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match child.try_wait().expect("waiting for the server") {
            Some(status) => return status,
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let _ = child.kill();
    panic!("the server did not exit within {timeout:?}");
}
