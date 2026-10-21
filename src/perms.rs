//! macOS privacy permissions (TCC).
//!
//! Two permissions matter to this project: **Screen Recording** for capture
//! (wave 1) and **Accessibility** for input injection (wave 2, ADR-0003).
//!
//! The contract, from `specs/01-architecture.md`: a missing permission never
//! panics. It produces a readable log with instructions and a degraded mode.
//! This matters more than it sounds — `CGEventPost` without Accessibility does
//! nothing at all and returns no error, so without an explicit check the user
//! is left guessing.

// Apple ships no Rust bindings for these four symbols, and pulling in a second
// binding tree for two booleans is not worth it (ADR-0003).
//
// One of three unsafe sites in the crate. The others are a single expression in
// `capture::screen`, where `apple-cf` exposes a locked pixel buffer's bytes
// only through an unsafe fn (ADR-0016) and which carries its own `#[expect]`
// rather than a module-wide allow, and `menubar`, where AppKit cannot be driven
// without it (ADR-0015).
#![allow(unsafe_code)]

use tracing::{error, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    ScreenRecording,
    Accessibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
}

impl Permission {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ScreenRecording => "Screen Recording",
            Self::Accessibility => "Accessibility",
        }
    }
}

unsafe extern "C" {
    /// Returns whether the calling process already has Screen Recording access.
    /// Does not prompt.
    fn CGPreflightScreenCaptureAccess() -> bool;
    /// Prompts for Screen Recording access if it has never been requested.
    fn CGRequestScreenCaptureAccess() -> bool;
    /// Returns whether the calling process is trusted for Accessibility.
    fn AXIsProcessTrusted() -> bool;
}

/// Checks a permission without prompting.
#[must_use]
pub fn check(permission: Permission) -> PermissionState {
    // SAFETY: both functions take no arguments, return a plain `bool`, and are
    // documented as callable from any thread.
    let granted = unsafe {
        match permission {
            Permission::ScreenRecording => CGPreflightScreenCaptureAccess(),
            Permission::Accessibility => AXIsProcessTrusted(),
        }
    };

    if granted {
        PermissionState::Granted
    } else {
        PermissionState::Denied
    }
}

/// Asks the system to prompt for a permission.
///
/// macOS only shows the dialog the first time a given process asks; afterwards
/// the user must go to System Settings. Returns the state after the request.
pub fn request(permission: Permission) -> PermissionState {
    match permission {
        Permission::ScreenRecording => {
            // SAFETY: no arguments, plain `bool` return.
            let granted = unsafe { CGRequestScreenCaptureAccess() };
            if granted {
                PermissionState::Granted
            } else {
                PermissionState::Denied
            }
        }
        // There is no non-deprecated prompt-only API for Accessibility that does
        // not require building a CFDictionary; wave 2 can revisit if needed.
        Permission::Accessibility => check(permission),
    }
}

/// What the user has to do to grant a permission.
///
/// F-110.14: the stale-entry case is part of the instruction, not a footnote.
/// macOS keys a grant to the exact binary it was given to, and both
/// `brew upgrade` and a terminal app's auto-update produce a different binary,
/// so the entry already in the list stops applying — while its switch keeps
/// looking enabled. Someone who only reads "enable it in System Settings"
/// looks at a switch that is on, concludes the server is broken, and is right
/// to. This has cost this project two debugging sessions already.
#[must_use]
pub const fn explain(permission: Permission) -> &'static str {
    match permission {
        Permission::ScreenRecording => {
            "Open System Settings → Privacy & Security → Screen & System Audio Recording, \
             enable the app you launched this server from (Terminal, iTerm, or the binary \
             itself), then restart the server. A binary started from a terminal inherits \
             that terminal's permissions. If you have just upgraded the server or that \
             terminal app, the entry already in the list is stale even though its switch \
             still looks enabled: remove it with the minus button, add the current one \
             again, and restart the server."
        }
        Permission::Accessibility => {
            "Open System Settings → Privacy & Security → Accessibility, enable the app you \
             launched this server from (Terminal, iTerm, or the binary itself), then restart \
             the server. Without this, injected events are silently discarded. If you have \
             just upgraded the server or that terminal app, the entry already in the list is \
             stale even though its switch still looks enabled: remove it with the minus \
             button, add the current one again, and restart the server."
        }
    }
}

/// Reads both permissions without prompting and without side effects.
///
/// F-110.15: `--check-permissions` answers "did my click take effect?", so it
/// must not be the thing that causes the dialog to appear — a reading that
/// changes what it reads is useless for that question.
#[must_use]
pub fn survey() -> Vec<(Permission, PermissionState)> {
    [Permission::ScreenRecording, Permission::Accessibility]
        .into_iter()
        .map(|permission| (permission, check(permission)))
        .collect()
}

/// Whether every permission in a survey is granted.
#[must_use]
pub fn all_granted(readings: &[(Permission, PermissionState)]) -> bool {
    readings.iter().all(|&(_, state)| state == PermissionState::Granted)
}

/// The System Settings panel that grants this permission (F-111.7).
///
/// `x-apple.systempreferences:` is the documented way to open a specific
/// privacy panel; the anchor after `?` is what picks the row. Kept next to
/// [`explain`] on purpose — the words and the destination have to agree, and
/// they only do that if they are read together.
#[must_use]
pub const fn settings_url(permission: Permission) -> &'static str {
    match permission {
        Permission::ScreenRecording => "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
        Permission::Accessibility => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    }
}

/// Renders a survey for a human reading a terminal (F-110.15).
///
/// Pure on purpose: the formatting is the part worth testing, and it is
/// testable without a desktop session or a single granted permission.
#[must_use]
pub fn report(readings: &[(Permission, PermissionState)]) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    for &(permission, state) in readings {
        let mark = match state {
            PermissionState::Granted => "granted",
            PermissionState::Denied => "NOT granted",
        };
        // `write!` into a String cannot fail; the Result is discarded rather
        // than unwrapped so this stays panic-free.
        let _ = writeln!(out, "{}: {mark}", permission.name());
        if state == PermissionState::Denied {
            let _ = writeln!(out, "    {}", explain(permission));
        }
    }

    if all_granted(readings) {
        out.push_str("\nBoth permissions are in place. If capture or input still does nothing, the\n");
        out.push_str("grant belongs to an older build of the program that started the server:\n");
        out.push_str("remove that entry with the minus button, add the current one, and start the\n");
        out.push_str("server again.\n");
    }
    out
}

/// Checks a permission, prompting once if it is missing, and logs the outcome.
///
/// Returns the final state so the caller can degrade rather than fail.
pub fn ensure(permission: Permission) -> PermissionState {
    let name = permission.name();

    if check(permission) == PermissionState::Granted {
        info!(permission = name, "permission granted");
        return PermissionState::Granted;
    }

    warn!(permission = name, "permission missing, requesting it");
    let state = request(permission);

    if state == PermissionState::Granted {
        info!(permission = name, "permission granted after request");
    } else {
        error!(permission = name, "{}", explain(permission));
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_permission_has_a_name_and_instructions() {
        for permission in [Permission::ScreenRecording, Permission::Accessibility] {
            assert!(!permission.name().is_empty());
            let explanation = explain(permission);
            assert!(
                explanation.contains("System Settings"),
                "{permission:?} must tell the user where to click"
            );
            assert!(
                explanation.contains("restart"),
                "{permission:?} must say a restart is needed — TCC changes do not apply live"
            );
        }
    }

    /// F-110.14: the instruction has to cover the case that actually bites —
    /// an entry that is in the list, looks enabled, and no longer applies.
    #[test]
    fn f110_14_instructions_cover_a_stale_entry_after_an_upgrade() {
        for permission in [Permission::ScreenRecording, Permission::Accessibility] {
            let explanation = explain(permission);
            assert!(
                explanation.contains("upgraded"),
                "{permission:?} must name the upgrade as the cause: {explanation}"
            );
            assert!(
                explanation.contains("stale"),
                "{permission:?} must say the existing entry is stale: {explanation}"
            );
            assert!(
                explanation.contains("looks enabled"),
                "{permission:?} must warn that the switch lies: {explanation}"
            );
            assert!(
                explanation.contains("minus button"),
                "{permission:?} must say how to remove it: {explanation}"
            );
        }
    }

    /// F-110.15: a denied permission prints what to click, a granted one does
    /// not bury the reader in instructions it does not need.
    #[test]
    fn f110_15_the_report_names_both_permissions_and_explains_only_the_missing_one() {
        let readings = [
            (Permission::ScreenRecording, PermissionState::Granted),
            (Permission::Accessibility, PermissionState::Denied),
        ];
        let text = report(&readings);

        assert!(text.contains("Screen Recording: granted"), "{text}");
        assert!(text.contains("Accessibility: NOT granted"), "{text}");
        assert!(
            text.contains(explain(Permission::Accessibility)),
            "the denied permission must come with its instruction: {text}"
        );
        assert!(
            !text.contains(explain(Permission::ScreenRecording)),
            "the granted permission must not be explained: {text}"
        );
        assert!(!all_granted(&readings));
    }

    /// F-110.15: all granted and still not working is the stale-entry case,
    /// which is precisely when someone runs this mode.
    #[test]
    fn f110_15_a_clean_report_still_points_at_the_stale_entry() {
        let readings = [
            (Permission::ScreenRecording, PermissionState::Granted),
            (Permission::Accessibility, PermissionState::Granted),
        ];
        let text = report(&readings);

        assert!(all_granted(&readings));
        assert!(text.contains("Both permissions are in place"), "{text}");
        // The wording may change; what must survive is that a reader whose
        // permissions all read "granted" is still told what to do when the
        // server does nothing — remove the stale entry and re-add it.
        assert!(text.contains("minus button"), "{text}");
        assert!(text.contains("add the current one"), "{text}");
    }

    /// F-110.15: the survey covers every permission the project needs, so a
    /// third one cannot be added without this mode learning about it.
    #[test]
    fn f110_15_the_survey_reads_every_permission() {
        let readings = survey();
        assert_eq!(readings.len(), 2);
        assert!(readings.iter().any(|&(p, _)| p == Permission::ScreenRecording));
        assert!(readings.iter().any(|&(p, _)| p == Permission::Accessibility));
    }

    #[test]
    fn checking_a_permission_does_not_panic() {
        // The result depends on how the test runner was launched, so only the
        // absence of a crash is asserted. Wave 1's real check is AC-7, by hand.
        let _ = check(Permission::ScreenRecording);
        let _ = check(Permission::Accessibility);
    }
}
