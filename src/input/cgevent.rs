//! Wave 2 input sink: injects events into macOS via Quartz Event Services
//! (F-003, ADR-0003).
//!
//! This is the only module that touches `core-graphics`. The decisions about
//! *what* to inject (coordinate mapping, modifier flags, drag-vs-move) live in
//! `state.rs` and `keymap.rs` and are unit-tested there; this module is the thin
//! `CGEvent` layer that cannot be tested without a windowed session.
//!
//! # The silent-failure trap
//!
//! `CGEventPost` returns nothing and fails silently without the Accessibility
//! permission (ADR-0003 §5). So the sink checks the permission up front and, if
//! missing, logs once and drops events instead of pretending to work.

use anyhow::Result;
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode, CGMouseButton, EventField, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use tracing::{debug, info, trace, warn};

use super::keymap::{self, MappedKey};
use super::state::{self, Buttons, Modifiers, MoveKind, ResetAction, ResetCause, ScreenGeometry};
use super::unicode::SurrogateAssembler;
use super::{InputSink, KeyEvent, MouseButton, PointerEvent};
use crate::perms::{self, Permission, PermissionState};

/// Injects input events into the system.
pub struct CgEventSink {
    /// `None` when Accessibility was refused: events are logged, not injected.
    source: Option<CGEventSource>,
    geometry: ScreenGeometry,
    modifiers: Modifiers,
    buttons: Buttons,
    /// Last known cursor position, in points — scroll and button events need a
    /// location, and the client sends moves and clicks separately.
    cursor: CGPoint,
    dropped: u64,
    /// Pending halves of UTF-16 surrogate pairs (F-003a.14).
    surrogates: SurrogateAssembler,
}

impl CgEventSink {
    /// Builds a sink for the given display geometry.
    ///
    /// Succeeds even without the Accessibility permission — it degrades to
    /// logging (F-003.17) rather than refusing to start, so the screen keeps
    /// working. Returns an error only if the event source itself cannot be
    /// created, which would be a genuine system fault.
    pub fn new(geometry: ScreenGeometry) -> Result<Self> {
        let source = if perms::ensure(Permission::Accessibility) == PermissionState::Granted {
            match CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
                Ok(source) => Some(source),
                Err(()) => {
                    warn!("could not create a CGEventSource; input will be logged, not injected");
                    None
                }
            }
        } else {
            warn!("input will be logged, not injected, until Accessibility is granted");
            None
        };

        Ok(Self {
            source,
            geometry,
            modifiers: Modifiers::default(),
            buttons: Buttons::default(),
            cursor: CGPoint::new(geometry.origin_x, geometry.origin_y),
            dropped: 0,
            surrogates: SurrogateAssembler::default(),
        })
    }

    /// Moves the cursor's remembered position into the new coordinate space.
    ///
    /// Without this a geometry change would leave `cursor` — which scroll and
    /// button events are posted at — pointing at wherever the old origin used
    /// to be, so the first click after a display was attached would land on the
    /// wrong screen even though moves had already been corrected.
    fn rebase_cursor(&mut self, previous: ScreenGeometry, current: ScreenGeometry) {
        let x = (self.cursor.x - previous.origin_x) * previous.scale / current.scale + current.origin_x;
        let y = (self.cursor.y - previous.origin_y) * previous.scale / current.scale + current.origin_y;
        self.cursor = CGPoint::new(x, y);
    }

    fn flags(&self) -> CGEventFlags {
        CGEventFlags::from_bits_truncate(self.modifiers.cg_flags())
    }

    /// Posts an event, stamping the current modifier flags first.
    fn post(&self, event: &CGEvent) {
        event.set_flags(self.flags());
        event.post(CGEventTapLocation::HID);
    }

    fn handle_pointer(&mut self, source: &CGEventSource, event: PointerEvent) {
        match event {
            PointerEvent::MoveAbsolute { x, y } => {
                let (px, py) = self.geometry.pixel_to_point(x, y);
                self.cursor = CGPoint::new(px, py);
                let event_type = match self.buttons.move_kind() {
                    MoveKind::Move => CGEventType::MouseMoved,
                    MoveKind::LeftDrag => CGEventType::LeftMouseDragged,
                    MoveKind::RightDrag => CGEventType::RightMouseDragged,
                    MoveKind::OtherDrag => CGEventType::OtherMouseDragged,
                };
                // The button argument is ignored for plain moves; Left is the
                // conventional filler.
                if let Ok(ev) = CGEvent::new_mouse_event(source.clone(), event_type, self.cursor, CGMouseButton::Left) {
                    self.post(&ev);
                }
            }
            PointerEvent::MoveRelative { dx, dy } => {
                // Absolute moves are what real clients send; treat relative as a
                // nudge of the last known position so it is not simply lost.
                let nx = self.cursor.x + f64::from(dx);
                let ny = self.cursor.y + f64::from(dy);
                self.cursor = CGPoint::new(nx, ny);
                if let Ok(ev) = CGEvent::new_mouse_event(
                    source.clone(),
                    CGEventType::MouseMoved,
                    self.cursor,
                    CGMouseButton::Left,
                ) {
                    self.post(&ev);
                }
            }
            PointerEvent::Button { button, pressed } => {
                self.buttons.set(button, pressed);
                if let Some(ev) = self.build_button_event(source, button, pressed) {
                    self.post(&ev);
                }
            }
            PointerEvent::Scroll { dx, dy } => {
                // RDP scroll units are 120ths of a wheel notch; one notch ≈ one
                // line. Sign: RDP positive = away from user = scroll up, which is
                // macOS positive line-1, so the wheel axis passes through as-is.
                let lines_y = dy / 120;
                let lines_x = dx / 120;
                if lines_x == 0 && lines_y == 0 {
                    return;
                }
                if let Ok(ev) = CGEvent::new_scroll_event(source.clone(), ScrollEventUnit::LINE, 2, lines_y, lines_x, 0)
                {
                    self.post(&ev);
                }
            }
        }
    }

    fn handle_key(&mut self, source: &CGEventSource, event: KeyEvent) {
        // A scancode between the two halves of a pair leaves the first half
        // lone (F-003a.15); the scancode itself goes through normally.
        if let KeyEvent::Scancode { pressed, .. } = event
            && let Some(unit) = self.surrogates.interrupt(pressed)
        {
            debug!(
                unit = format_args!("{unit:#06X}"),
                pressed, "lone UTF-16 surrogate dropped"
            );
        }
        match event {
            KeyEvent::Scancode {
                code,
                extended,
                pressed,
            } => match keymap::map(code, extended) {
                Some(MappedKey::Modifier(modifier)) => {
                    self.modifiers.set(modifier, pressed);
                    // Modifier state is applied via flags on subsequent events,
                    // which is what actually makes Cmd+C etc. work.
                }
                Some(MappedKey::Key(keycode)) => {
                    self.post_key(source, keycode, pressed);
                }
                None => trace!(code, extended, "unmapped scancode"),
            },
            KeyEvent::Unicode { ch, pressed } => {
                // Fallback for characters with no scancode mapping (F-003.14).
                // Only a complete character is injected: a lone surrogate must
                // never become a bare keycode-0 event (F-003a.15).
                let step = self.surrogates.push(ch, pressed);
                if let Some(unit) = step.dropped {
                    debug!(
                        unit = format_args!("{unit:#06X}"),
                        pressed, "lone UTF-16 surrogate dropped"
                    );
                }
                if let Some(c) = step.emit
                    && let Some(ev) = build_unicode_event(source, c, pressed)
                {
                    // Not `self.post`: the flags stay cleared (F-003a.16).
                    ev.post(CGEventTapLocation::HID);
                }
            }
            KeyEvent::SyncLocks { caps, .. } => {
                self.reset(Some(source), ResetCause::Sync { caps_lock: caps });
            }
        }
    }

    /// Releases every held modifier and button with real events, then applies
    /// the toggle keys of a Synchronize (F-003.22, F-003.24).
    ///
    /// What to release is decided by [`state::plan_reset`] (F-003.25); this
    /// only injects it. Each action is applied to the state *before* its event
    /// is posted, so a modifier's release carries the flags without it.
    /// `source` is `None` without Accessibility: the state is still reset.
    fn reset(&mut self, source: Option<&CGEventSource>, cause: ResetCause) {
        let lone = self.surrogates.reset();
        if lone > 0 {
            debug!(
                lone,
                cause = cause.label(),
                "pending UTF-16 surrogates dropped on reset"
            );
        }
        let actions = state::plan_reset(self.modifiers, self.buttons, cause);
        for &action in &actions {
            state::apply(&mut self.modifiers, &mut self.buttons, action);
            let Some(source) = source else { continue };
            match action {
                ResetAction::ReleaseModifier(modifier) => {
                    if let Some(keycode) = keymap::modifier_keycode(modifier)
                        && let Ok(ev) = CGEvent::new_keyboard_event(source.clone(), keycode, false)
                    {
                        ev.set_type(CGEventType::FlagsChanged);
                        self.post(&ev);
                    }
                }
                ResetAction::ReleaseButton(button) => {
                    if let Some(ev) = self.build_button_event(source, button, false) {
                        self.post(&ev);
                    }
                }
                ResetAction::SetCapsLock(_) => {}
            }
        }
        // F-003.26: one line per reset, quiet when there was nothing to release.
        let (modifiers, buttons) = state::released_counts(&actions);
        if modifiers + buttons > 0 {
            debug!(cause = cause.label(), modifiers, buttons, "input state reset");
        } else {
            trace!(cause = cause.label(), "input state reset, nothing held");
        }
    }

    fn post_key(&self, source: &CGEventSource, keycode: CGKeyCode, pressed: bool) {
        if let Ok(ev) = CGEvent::new_keyboard_event(source.clone(), keycode, pressed) {
            self.post(&ev);
        }
    }

    /// Builds a mouse button event at the current cursor.
    ///
    /// Split out so the button-number wiring can be unit-tested against a real
    /// `CGEvent` without posting it — that wiring is exactly what was wrong for
    /// the middle button.
    fn build_button_event(&self, source: &CGEventSource, button: MouseButton, pressed: bool) -> Option<CGEvent> {
        let (event_type, cg_button) = button_event(button, pressed);
        let ev = CGEvent::new_mouse_event(source.clone(), event_type, self.cursor, cg_button).ok()?;
        // For the "other" buttons macOS keys off MOUSE_EVENT_BUTTON_NUMBER, not
        // the CGMouseButton, so it must be set explicitly — including for the
        // middle button (number 2), which otherwise does nothing (F-003.7).
        if let Some(number) = other_button_number(button) {
            ev.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, number);
        }
        Some(ev)
    }

    /// Increments the drop counter and warns at power-of-two milestones.
    fn note_dropped(&mut self) {
        self.dropped = self.dropped.saturating_add(1);
        if self.dropped.is_power_of_two() {
            warn!(
                dropped = self.dropped,
                "Accessibility not granted; input events dropped"
            );
        }
    }
}

impl InputSink for CgEventSink {
    fn pointer(&mut self, ev: PointerEvent) -> Result<()> {
        match self.source.clone() {
            Some(source) => self.handle_pointer(&source, ev),
            None => {
                self.note_dropped();
                info!(?ev, "pointer (not injected)");
            }
        }
        Ok(())
    }

    fn key(&mut self, ev: KeyEvent) -> Result<()> {
        match self.source.clone() {
            Some(source) => self.handle_key(&source, ev),
            None => {
                self.note_dropped();
                info!(?ev, "key (not injected)");
            }
        }
        Ok(())
    }

    /// F-007.17. The watcher republishes unconditionally, so most calls are
    /// no-ops and only a real move is logged.
    fn set_geometry(&mut self, geometry: ScreenGeometry) {
        let previous = self.geometry;
        if previous == geometry {
            return;
        }
        self.geometry = geometry;
        self.rebase_cursor(previous, geometry);
        info!(
            origin_x = geometry.origin_x,
            origin_y = geometry.origin_y,
            scale = geometry.scale,
            "captured display moved; input coordinates re-based"
        );
    }
}

/// Builds the keyboard event for a character typed through the Unicode path.
///
/// Its modifier flags are **cleared** (F-003a.16): the character is already the
/// final one, so a held Cmd from an earlier scancode must not turn it into a
/// shortcut. The sink's modifier state is not touched, so the next scancode
/// event still carries Cmd.
fn build_unicode_event(source: &CGEventSource, c: char, pressed: bool) -> Option<CGEvent> {
    let ev = CGEvent::new_keyboard_event(source.clone(), 0, pressed).ok()?;
    let mut units = [0u16; 2];
    ev.set_string_from_utf16_unchecked(c.encode_utf16(&mut units));
    ev.set_flags(CGEventFlags::empty());
    Some(ev)
}

/// The `(CGEventType, CGMouseButton)` for a button press or release.
///
/// NOTE: the `Middle` arm is correct but currently unreachable end-to-end —
/// IronRDP 0.13.0 drops `PTRFLAGS_BUTTON3` in `From<MousePdu>` (decodes it as a
/// move) and exposes no raw-PDU hook, so a middle click never reaches us. Kept
/// intact; it will work unchanged once we own the pointer-PDU parser or upstream
/// is fixed. See ADR-0007.
fn button_event(button: MouseButton, pressed: bool) -> (CGEventType, CGMouseButton) {
    match button {
        MouseButton::Left if pressed => (CGEventType::LeftMouseDown, CGMouseButton::Left),
        MouseButton::Left => (CGEventType::LeftMouseUp, CGMouseButton::Left),
        MouseButton::Right if pressed => (CGEventType::RightMouseDown, CGMouseButton::Right),
        MouseButton::Right => (CGEventType::RightMouseUp, CGMouseButton::Right),
        MouseButton::Middle if pressed => (CGEventType::OtherMouseDown, CGMouseButton::Center),
        MouseButton::Middle => (CGEventType::OtherMouseUp, CGMouseButton::Center),
        MouseButton::Back | MouseButton::Forward if pressed => (CGEventType::OtherMouseDown, CGMouseButton::Center),
        MouseButton::Back | MouseButton::Forward => (CGEventType::OtherMouseUp, CGMouseButton::Center),
    }
}

/// The macOS button number for the "other" (non-left/right) mouse buttons,
/// which macOS reads from `MOUSE_EVENT_BUTTON_NUMBER` rather than from
/// `CGMouseButton` (F-003.7). Left and right carry their number implicitly.
fn other_button_number(button: MouseButton) -> Option<i64> {
    match button {
        MouseButton::Middle => Some(2),
        MouseButton::Back => Some(3),
        MouseButton::Forward => Some(4),
        MouseButton::Left | MouseButton::Right => None,
    }
}

#[cfg(test)]
mod tests {
    use core_graphics::event::KeyCode;

    use super::super::state::flag;
    use super::*;

    // These assert that the constants duplicated in `state.rs` and `keymap.rs`
    // still equal the values in `core-graphics`. If an upstream bump renumbers
    // anything, this fails instead of silently injecting the wrong key.

    #[test]
    fn duplicated_modifier_flags_match_core_graphics() {
        assert_eq!(flag::SHIFT, CGEventFlags::CGEventFlagShift.bits());
        assert_eq!(flag::CONTROL, CGEventFlags::CGEventFlagControl.bits());
        assert_eq!(flag::ALTERNATE, CGEventFlags::CGEventFlagAlternate.bits());
        assert_eq!(flag::COMMAND, CGEventFlags::CGEventFlagCommand.bits());
        assert_eq!(flag::ALPHA_SHIFT, CGEventFlags::CGEventFlagAlphaShift.bits());
    }

    fn mapped_key(code: u8, extended: bool) -> u16 {
        match keymap::map(code, extended) {
            Some(MappedKey::Key(code)) => code,
            other => panic!("expected a key, got {other:?}"),
        }
    }

    #[test]
    fn duplicated_key_codes_match_core_graphics() {
        // Spot-check the keys the shortcuts in AC-7 depend on: our scancode
        // table must land on the same virtual codes `core-graphics` defines.
        assert_eq!(mapped_key(0x2E, false), KeyCode::ANSI_C);
        assert_eq!(mapped_key(0x2F, false), KeyCode::ANSI_V);
        assert_eq!(mapped_key(0x1C, false), KeyCode::RETURN);
        assert_eq!(mapped_key(0x39, false), KeyCode::SPACE);
        assert_eq!(mapped_key(0x4B, true), KeyCode::LEFT_ARROW);
    }

    #[test]
    fn other_buttons_get_the_right_numbers() {
        // The middle-button regression: it must be number 2, not None.
        assert_eq!(other_button_number(MouseButton::Middle), Some(2));
        assert_eq!(other_button_number(MouseButton::Back), Some(3));
        assert_eq!(other_button_number(MouseButton::Forward), Some(4));
        assert_eq!(other_button_number(MouseButton::Left), None);
        assert_eq!(other_button_number(MouseButton::Right), None);
    }

    /// Builds a sink without touching the windowing system: `source: None` is
    /// the degraded mode the constructor itself falls back to.
    fn sink_at(geometry: ScreenGeometry, cursor: CGPoint) -> CgEventSink {
        CgEventSink {
            source: None,
            geometry,
            modifiers: Modifiers::default(),
            buttons: Buttons::default(),
            cursor,
            dropped: 0,
            surrogates: SurrogateAssembler::default(),
        }
    }

    /// F-007.17. Moves are re-mapped by `geometry`, but clicks and scrolls are
    /// posted at the *remembered* cursor — so that has to move too, or the first
    /// click after a display is attached lands on the wrong screen.
    #[test]
    fn a_geometry_change_carries_the_remembered_cursor_with_it() {
        let before = ScreenGeometry::new(0.0, 0.0, 1.0);
        // Cursor sits 300 points into a display that starts at zero.
        let mut sink = sink_at(before, CGPoint::new(300.0, 100.0));

        // The lid opens; macOS pushes the captured display to the right.
        sink.set_geometry(ScreenGeometry::new(1512.0, 0.0, 1.0));

        assert_eq!(
            (sink.cursor.x, sink.cursor.y),
            (1812.0, 100.0),
            "the cursor must stay on the same desktop pixel, not the same screen point"
        );
    }

    #[test]
    fn a_scale_change_rebases_the_cursor_too() {
        let mut sink = sink_at(ScreenGeometry::new(0.0, 0.0, 1.0), CGPoint::new(400.0, 200.0));
        // Same place, now a Retina mode: 400 points at 1x is pixel 400, which
        // at 2x is 200 points.
        sink.set_geometry(ScreenGeometry::new(0.0, 0.0, 2.0));
        assert_eq!((sink.cursor.x, sink.cursor.y), (200.0, 100.0));
    }

    #[test]
    fn republishing_the_same_geometry_changes_nothing() {
        // The watcher resends every tick, so this is the common case.
        let geometry = ScreenGeometry::new(1512.0, 0.0, 2.0);
        let mut sink = sink_at(geometry, CGPoint::new(1600.0, 40.0));
        sink.set_geometry(geometry);
        assert_eq!(
            (sink.cursor.x, sink.cursor.y),
            (1600.0, 40.0),
            "an unchanged geometry must not drift the cursor"
        );
    }

    #[test]
    fn middle_button_event_carries_button_number_two() {
        // Builds a real CGEvent (Accessibility not required to *create* one) and
        // checks the fields, without ever posting it. This is the exact wiring
        // that was wrong: a middle click with no button number does nothing.
        let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
            // No windowing environment (headless CI) — construction is covered
            // by the pure test above; skip the live-object check.
            return;
        };
        let sink = sink_at(ScreenGeometry::new(0.0, 0.0, 1.0), CGPoint::new(10.0, 20.0));

        let ev = sink
            .build_button_event(&source, MouseButton::Middle, true)
            .expect("build middle-button event");
        assert_eq!(ev.get_type() as u32, CGEventType::OtherMouseDown as u32);
        assert_eq!(ev.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER), 2);

        let up = sink
            .build_button_event(&source, MouseButton::Middle, false)
            .expect("build middle-button release");
        assert_eq!(up.get_type() as u32, CGEventType::OtherMouseUp as u32);
    }

    /// F-003.22: modifier releases are posted with the real modifier key codes,
    /// so they must match `core-graphics`; Caps Lock is never "held".
    #[test]
    fn f003_22_modifier_release_key_codes_match_core_graphics() {
        use keymap::Modifier;
        assert_eq!(keymap::modifier_keycode(Modifier::ShiftLeft), Some(KeyCode::SHIFT));
        assert_eq!(
            keymap::modifier_keycode(Modifier::ShiftRight),
            Some(KeyCode::RIGHT_SHIFT)
        );
        assert_eq!(keymap::modifier_keycode(Modifier::ControlLeft), Some(KeyCode::CONTROL));
        assert_eq!(
            keymap::modifier_keycode(Modifier::ControlRight),
            Some(KeyCode::RIGHT_CONTROL)
        );
        assert_eq!(keymap::modifier_keycode(Modifier::AltLeft), Some(KeyCode::OPTION));
        assert_eq!(
            keymap::modifier_keycode(Modifier::AltRight),
            Some(KeyCode::RIGHT_OPTION)
        );
        assert_eq!(keymap::modifier_keycode(Modifier::CommandLeft), Some(KeyCode::COMMAND));
        assert_eq!(
            keymap::modifier_keycode(Modifier::CommandRight),
            Some(KeyCode::RIGHT_COMMAND)
        );
        assert_eq!(keymap::modifier_keycode(Modifier::CapsLock), None);
    }

    /// F-003.22: a Synchronize clears held state even when nothing can be
    /// injected, so the next connection starts from "all up".
    #[test]
    fn f003_22_sync_resets_the_sink_state() {
        let mut sink = sink_at(ScreenGeometry::new(0.0, 0.0, 1.0), CGPoint::new(0.0, 0.0));
        sink.modifiers.set(keymap::Modifier::CommandLeft, true);
        sink.buttons.set(MouseButton::Left, true);

        sink.reset(None, ResetCause::Sync { caps_lock: true });

        assert_eq!(sink.flags().bits(), flag::ALPHA_SHIFT, "Cmd released, Caps Lock set");
        assert_eq!(sink.buttons.move_kind(), MoveKind::Move);
    }

    /// F-003a.16 (AC-9): a Unicode event while Cmd is held has flags 0, and
    /// the next scancode event still gets Cmd.
    #[test]
    fn f003a_16_unicode_events_carry_no_modifier_flags() {
        let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
            return; // No windowing environment; see the middle-button test.
        };
        let mut sink = sink_at(ScreenGeometry::new(0.0, 0.0, 1.0), CGPoint::new(0.0, 0.0));
        sink.modifiers.set(keymap::Modifier::CommandLeft, true);

        let ev = build_unicode_event(&source, '\u{1F600}', true).expect("build unicode event");
        assert!(ev.get_flags().is_empty(), "Unicode must not carry Cmd");
        assert_eq!(ev.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE), 0);

        assert_eq!(sink.flags().bits(), flag::COMMAND, "Cmd is still held for scancodes");
    }

    /// F-003a.17: the surrogate buffer is cleared by a Synchronize reset.
    #[test]
    fn f003a_17_sync_reset_clears_pending_surrogates() {
        let mut sink = sink_at(ScreenGeometry::new(0.0, 0.0, 1.0), CGPoint::new(0.0, 0.0));
        sink.surrogates.push(0xD83D, true);
        sink.reset(None, ResetCause::Sync { caps_lock: false });
        assert_eq!(sink.surrogates.push(0xDE00, true).emit, None, "no pair across a reset");
    }
}
