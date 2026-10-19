//! F-007.26, F-007.28, F-007.29 — the display is kept awake while a client is
//! connected, and only then.
//!
//! Needs no RDP client and no GUI: the contract between `session` and `power`
//! is a client count, so the count can be driven directly. That matters,
//! because the thing this guards against took eleven minutes of a real session
//! to show up once (F-007 §8e).
//!
//! Both phases live in one test on purpose. They inspect process state that is
//! global to this test binary, so running them concurrently would let each see
//! the other's `caffeinate`.

use std::process::Command;
use std::time::{Duration, Instant};

use mac_rdp_server::power::{DisplayWakeLock, keep_display_awake};
use tokio::sync::watch;

/// Is a `caffeinate` holding the display awake on this process's behalf?
///
/// Matches on our own pid in `-w`, so a `caffeinate` someone else started —
/// the owner's own `caffeinate -i`, say — cannot make this pass.
fn held_for_us() -> bool {
    let pattern = format!("caffeinate -d -w {}", std::process::id());
    Command::new("pgrep")
        .args(["-f", &pattern])
        .output()
        .is_ok_and(|out| !out.stdout.is_empty())
}

/// Does macOS agree that something is preventing display sleep?
///
/// The process check alone would pass even if `caffeinate` were failing, so
/// this asks the system, which is the observable that actually matters.
fn system_reports_assertion() -> bool {
    Command::new("pmset")
        .args(["-g", "assertions"])
        .output()
        .is_ok_and(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| l.contains("PreventUserIdleDisplaySleep"))
                .any(|l| l.split_whitespace().last().is_some_and(|v| v != "0"))
        })
}

/// Polls `check` until it matches, or gives up. `caffeinate` takes a moment to
/// start and to register its assertion, so this cannot be a bare assert.
///
/// The wait is `tokio::time::sleep`, not `std::thread::sleep`. The first cut of
/// this test used the blocking one and failed: it never yields, so the spawned
/// `keep_display_awake` task was never polled and the lock was never taken.
/// The test was wrong, not the code.
async fn settles_to(expected: bool, check: fn() -> bool, what: &str) {
    const LIMIT: Duration = Duration::from_secs(5);
    let deadline = Instant::now() + LIMIT;
    while Instant::now() < deadline {
        if check() == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("{what}: expected {expected} within {LIMIT:?}, still {}", !expected);
}

#[tokio::test]
async fn f007_26_the_display_is_kept_awake_only_while_a_client_is_connected() {
    // Someone else's assertion would make the system check meaningless, so the
    // test says so rather than failing confusingly.
    assert!(
        !held_for_us(),
        "a caffeinate from an earlier run is still alive for this pid"
    );

    // --- F-007.26: taken while a client is connected ----------------------
    let (clients, rx) = watch::channel(0_usize);
    let task = tokio::spawn(keep_display_awake(rx));

    // Nothing connected: nothing held. An assertion taken for an idle server
    // would keep the owner's Mac awake for no reason.
    settles_to(false, held_for_us, "no client, no lock").await;

    clients.send_replace(1);
    settles_to(true, held_for_us, "one client connected").await;
    settles_to(true, system_reports_assertion, "macOS sees the assertion").await;

    // A second client must not release it, and the first leaving must not
    // either — only the last one out.
    clients.send_replace(2);
    settles_to(true, held_for_us, "two clients connected").await;
    clients.send_replace(1);
    settles_to(true, held_for_us, "one of two left").await;

    // --- F-007.26: released when the last client leaves -------------------
    clients.send_replace(0);
    settles_to(false, held_for_us, "the last client left").await;

    task.abort();

    // --- F-007.28: nothing outlives the lock ------------------------------
    // Taken and dropped by hand, because the task path above could hide a
    // leak behind its own abort.
    let lock = DisplayWakeLock::hold().expect("caffeinate should be available on macOS");
    settles_to(true, held_for_us, "lock taken directly").await;
    drop(lock);
    settles_to(false, held_for_us, "lock dropped").await;
}
