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
// binding tree for two booleans is not worth it (ADR-0003). Declaring them here
// is the entire unsafe surface of this crate.
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

    #[test]
    fn checking_a_permission_does_not_panic() {
        // The result depends on how the test runner was launched, so only the
        // absence of a crash is asserted. Wave 1's real check is AC-7, by hand.
        let _ = check(Permission::ScreenRecording);
        let _ = check(Permission::Accessibility);
    }
}
