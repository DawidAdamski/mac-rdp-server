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
use tracing::{info, trace, warn};

use super::keymap::{self, MappedKey};
use super::state::{Buttons, Modifiers, MoveKind, ScreenGeometry};
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
        })
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
                if let Ok(ev) = CGEvent::new_keyboard_event(source.clone(), 0, pressed) {
                    if let Some(c) = char::from_u32(u32::from(ch)) {
                        ev.set_string(&c.to_string());
                    }
                    self.post(&ev);
                }
            }
            KeyEvent::SyncLocks { caps, .. } => {
                self.modifiers.set_caps_lock(caps);
            }
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
}

/// The `(CGEventType, CGMouseButton)` for a button press or release.
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
        let sink = CgEventSink {
            source: None,
            geometry: ScreenGeometry::new(0.0, 0.0, 1.0),
            modifiers: Modifiers::default(),
            buttons: Buttons::default(),
            cursor: CGPoint::new(10.0, 20.0),
            dropped: 0,
        };

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
}
