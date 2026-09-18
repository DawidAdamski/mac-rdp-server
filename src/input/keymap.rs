//! Maps PC scancodes (RDP) to macOS virtual key codes (F-003.11, F-003.15).
//!
//! RDP delivers **scancode set 1** make codes: a byte, plus an `extended` flag
//! standing in for the `E0` prefix (`KeyboardEvent` in ADR-0001). macOS wants a
//! `CGKeyCode`, a *positional* virtual key code — position on the physical
//! keyboard, independent of the character it produces under the active layout.
//!
//! Because both sides are positional, this is a fixed table, not a
//! character-based lookup. The character a key ultimately types is decided by
//! whatever layout the user has active in macOS — so on a Polish "programmer's"
//! layout, `Option`+the same physical key already produces `ą`, `ę`, etc.,
//! without us mapping anything per-language. Layout-specific work
//! (dead keys, ISO extra keys, IME) is F-003a; this table is the ASCII-and-
//! modifiers backbone F-003 is specified against.
//!
//! This module has no macOS dependency and is fully unit-tested.

/// A macOS virtual key code. Mirrors `core_graphics::event::CGKeyCode` (a `u16`)
/// without depending on the crate, so the mapping stays pure and testable.
pub type MacKeyCode = u16;

/// One physical key on the Mac keyboard, split so the modifier keys can be
/// recognised (they set `CGEventFlags`, they are not typed like other keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedKey {
    /// An ordinary key sent as KeyDown/KeyUp with this virtual code.
    Key(MacKeyCode),
    /// A modifier key. Held state drives `CGEventFlags`; see [`Modifier`].
    Modifier(Modifier),
}

/// The modifier keys, kept distinct left/right because RDP and macOS both
/// distinguish them and skipping that would merge right-Alt into left-Alt
/// (F-003.15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    /// Left Alt / Option.
    AltLeft,
    /// Right Alt / Option (arrives as `E0 38`).
    AltRight,
    CommandLeft,
    CommandRight,
    CapsLock,
}

// Virtual key codes, from `core_graphics::event::KeyCode`. Duplicated as plain
// constants so this module needs no macOS crate. A test in `input::cgevent`
// asserts these still equal the crate's values, so a rename upstream is caught.
mod vk {
    use super::MacKeyCode;
    pub const A: MacKeyCode = 0x00;
    pub const S: MacKeyCode = 0x01;
    pub const D: MacKeyCode = 0x02;
    pub const F: MacKeyCode = 0x03;
    pub const H: MacKeyCode = 0x04;
    pub const G: MacKeyCode = 0x05;
    pub const Z: MacKeyCode = 0x06;
    pub const X: MacKeyCode = 0x07;
    pub const C: MacKeyCode = 0x08;
    pub const V: MacKeyCode = 0x09;
    pub const B: MacKeyCode = 0x0B;
    pub const Q: MacKeyCode = 0x0C;
    pub const W: MacKeyCode = 0x0D;
    pub const E: MacKeyCode = 0x0E;
    pub const R: MacKeyCode = 0x0F;
    pub const Y: MacKeyCode = 0x10;
    pub const T: MacKeyCode = 0x11;
    pub const N1: MacKeyCode = 0x12;
    pub const N2: MacKeyCode = 0x13;
    pub const N3: MacKeyCode = 0x14;
    pub const N4: MacKeyCode = 0x15;
    pub const N6: MacKeyCode = 0x16;
    pub const N5: MacKeyCode = 0x17;
    pub const EQUAL: MacKeyCode = 0x18;
    pub const N9: MacKeyCode = 0x19;
    pub const N7: MacKeyCode = 0x1A;
    pub const MINUS: MacKeyCode = 0x1B;
    pub const N8: MacKeyCode = 0x1C;
    pub const N0: MacKeyCode = 0x1D;
    pub const RIGHT_BRACKET: MacKeyCode = 0x1E;
    pub const O: MacKeyCode = 0x1F;
    pub const U: MacKeyCode = 0x20;
    pub const LEFT_BRACKET: MacKeyCode = 0x21;
    pub const I: MacKeyCode = 0x22;
    pub const P: MacKeyCode = 0x23;
    pub const L: MacKeyCode = 0x25;
    pub const J: MacKeyCode = 0x26;
    pub const QUOTE: MacKeyCode = 0x27;
    pub const K: MacKeyCode = 0x28;
    pub const SEMICOLON: MacKeyCode = 0x29;
    pub const BACKSLASH: MacKeyCode = 0x2A;
    pub const COMMA: MacKeyCode = 0x2B;
    pub const SLASH: MacKeyCode = 0x2C;
    pub const N: MacKeyCode = 0x2D;
    pub const M: MacKeyCode = 0x2E;
    pub const PERIOD: MacKeyCode = 0x2F;
    pub const GRAVE: MacKeyCode = 0x32;
    pub const RETURN: MacKeyCode = 0x24;
    pub const TAB: MacKeyCode = 0x30;
    pub const SPACE: MacKeyCode = 0x31;
    pub const DELETE: MacKeyCode = 0x33;
    pub const ESCAPE: MacKeyCode = 0x35;
    // Keypad.
    pub const KP_DECIMAL: MacKeyCode = 0x41;
    pub const KP_MULTIPLY: MacKeyCode = 0x43;
    pub const KP_PLUS: MacKeyCode = 0x45;
    pub const KP_DIVIDE: MacKeyCode = 0x4B;
    pub const KP_ENTER: MacKeyCode = 0x4C;
    pub const KP_MINUS: MacKeyCode = 0x4E;
    pub const KP_0: MacKeyCode = 0x52;
    pub const KP_1: MacKeyCode = 0x53;
    pub const KP_2: MacKeyCode = 0x54;
    pub const KP_3: MacKeyCode = 0x55;
    pub const KP_4: MacKeyCode = 0x56;
    pub const KP_5: MacKeyCode = 0x57;
    pub const KP_6: MacKeyCode = 0x58;
    pub const KP_7: MacKeyCode = 0x59;
    pub const KP_8: MacKeyCode = 0x5B;
    pub const KP_9: MacKeyCode = 0x5C;
    // Navigation / function (all extended, i.e. E0-prefixed).
    pub const HOME: MacKeyCode = 0x73;
    pub const PAGE_UP: MacKeyCode = 0x74;
    pub const FORWARD_DELETE: MacKeyCode = 0x75;
    pub const END: MacKeyCode = 0x77;
    pub const PAGE_DOWN: MacKeyCode = 0x79;
    pub const LEFT_ARROW: MacKeyCode = 0x7B;
    pub const RIGHT_ARROW: MacKeyCode = 0x7C;
    pub const DOWN_ARROW: MacKeyCode = 0x7D;
    pub const UP_ARROW: MacKeyCode = 0x7E;
    pub const F1: MacKeyCode = 0x7A;
    pub const F2: MacKeyCode = 0x78;
    pub const F3: MacKeyCode = 0x63;
    pub const F4: MacKeyCode = 0x76;
    pub const F5: MacKeyCode = 0x60;
    pub const F6: MacKeyCode = 0x61;
    pub const F7: MacKeyCode = 0x62;
    pub const F8: MacKeyCode = 0x64;
    pub const F9: MacKeyCode = 0x65;
    pub const F10: MacKeyCode = 0x6D;
    pub const F11: MacKeyCode = 0x67;
    pub const F12: MacKeyCode = 0x6F;
    // Modifiers. Only needed to *release* a held modifier (F-003.22): pressing
    // one is modelled purely through `CGEventFlags` on later events.
    pub const COMMAND: MacKeyCode = 0x37;
    pub const SHIFT: MacKeyCode = 0x38;
    pub const OPTION: MacKeyCode = 0x3A;
    pub const CONTROL: MacKeyCode = 0x3B;
    pub const RIGHT_COMMAND: MacKeyCode = 0x36;
    pub const RIGHT_SHIFT: MacKeyCode = 0x3C;
    pub const RIGHT_OPTION: MacKeyCode = 0x3D;
    pub const RIGHT_CONTROL: MacKeyCode = 0x3E;
}

/// The Mac virtual key code of a modifier key, for posting its release
/// (F-003.22). `None` for Caps Lock, which is a toggle and is never "held".
#[must_use]
pub fn modifier_keycode(modifier: Modifier) -> Option<MacKeyCode> {
    match modifier {
        Modifier::ShiftLeft => Some(vk::SHIFT),
        Modifier::ShiftRight => Some(vk::RIGHT_SHIFT),
        Modifier::ControlLeft => Some(vk::CONTROL),
        Modifier::ControlRight => Some(vk::RIGHT_CONTROL),
        Modifier::AltLeft => Some(vk::OPTION),
        Modifier::AltRight => Some(vk::RIGHT_OPTION),
        Modifier::CommandLeft => Some(vk::COMMAND),
        Modifier::CommandRight => Some(vk::RIGHT_COMMAND),
        Modifier::CapsLock => None,
    }
}

use Modifier::*;

/// Maps a scancode-set-1 make code to a Mac key.
///
/// `extended` is the `E0` prefix. Returns `None` for keys this wave does not
/// model (F-003a fills the gaps).
#[must_use]
pub fn map(code: u8, extended: bool) -> Option<MappedKey> {
    if extended { map_extended(code) } else { map_base(code) }
}

/// Non-extended make codes (scancode set 1). This is the standard PC/XT map.
fn map_base(code: u8) -> Option<MappedKey> {
    use MappedKey::{Key, Modifier as Mod};
    let mapped = match code {
        0x01 => Key(vk::ESCAPE),
        0x02 => Key(vk::N1),
        0x03 => Key(vk::N2),
        0x04 => Key(vk::N3),
        0x05 => Key(vk::N4),
        0x06 => Key(vk::N5),
        0x07 => Key(vk::N6),
        0x08 => Key(vk::N7),
        0x09 => Key(vk::N8),
        0x0A => Key(vk::N9),
        0x0B => Key(vk::N0),
        0x0C => Key(vk::MINUS),
        0x0D => Key(vk::EQUAL),
        0x0E => Key(vk::DELETE), // Backspace
        0x0F => Key(vk::TAB),
        0x10 => Key(vk::Q),
        0x11 => Key(vk::W),
        0x12 => Key(vk::E),
        0x13 => Key(vk::R),
        0x14 => Key(vk::T),
        0x15 => Key(vk::Y),
        0x16 => Key(vk::U),
        0x17 => Key(vk::I),
        0x18 => Key(vk::O),
        0x19 => Key(vk::P),
        0x1A => Key(vk::LEFT_BRACKET),
        0x1B => Key(vk::RIGHT_BRACKET),
        0x1C => Key(vk::RETURN),
        0x1D => Mod(ControlLeft),
        0x1E => Key(vk::A),
        0x1F => Key(vk::S),
        0x20 => Key(vk::D),
        0x21 => Key(vk::F),
        0x22 => Key(vk::G),
        0x23 => Key(vk::H),
        0x24 => Key(vk::J),
        0x25 => Key(vk::K),
        0x26 => Key(vk::L),
        0x27 => Key(vk::SEMICOLON),
        0x28 => Key(vk::QUOTE),
        0x29 => Key(vk::GRAVE),
        0x2A => Mod(ShiftLeft),
        0x2B => Key(vk::BACKSLASH),
        0x2C => Key(vk::Z),
        0x2D => Key(vk::X),
        0x2E => Key(vk::C),
        0x2F => Key(vk::V),
        0x30 => Key(vk::B),
        0x31 => Key(vk::N),
        0x32 => Key(vk::M),
        0x33 => Key(vk::COMMA),
        0x34 => Key(vk::PERIOD),
        0x35 => Key(vk::SLASH),
        0x36 => Mod(ShiftRight),
        0x37 => Key(vk::KP_MULTIPLY),
        0x38 => Mod(AltLeft),
        0x39 => Key(vk::SPACE),
        0x3A => Mod(CapsLock),
        0x3B => Key(vk::F1),
        0x3C => Key(vk::F2),
        0x3D => Key(vk::F3),
        0x3E => Key(vk::F4),
        0x3F => Key(vk::F5),
        0x40 => Key(vk::F6),
        0x41 => Key(vk::F7),
        0x42 => Key(vk::F8),
        0x43 => Key(vk::F9),
        0x44 => Key(vk::F10),
        0x47 => Key(vk::KP_7),
        0x48 => Key(vk::KP_8),
        0x49 => Key(vk::KP_9),
        0x4A => Key(vk::KP_MINUS),
        0x4B => Key(vk::KP_4),
        0x4C => Key(vk::KP_5),
        0x4D => Key(vk::KP_6),
        0x4E => Key(vk::KP_PLUS),
        0x4F => Key(vk::KP_1),
        0x50 => Key(vk::KP_2),
        0x51 => Key(vk::KP_3),
        0x52 => Key(vk::KP_0),
        0x53 => Key(vk::KP_DECIMAL),
        0x57 => Key(vk::F11),
        0x58 => Key(vk::F12),
        _ => return None,
    };
    Some(mapped)
}

/// Extended (`E0`-prefixed) make codes: the duplicate keys on the right of the
/// keyboard and the navigation cluster. Getting these right is F-003.15 —
/// e.g. `E0 38` is right Alt, a *different* Mac key from left Alt.
fn map_extended(code: u8) -> Option<MappedKey> {
    use MappedKey::{Key, Modifier as Mod};
    let mapped = match code {
        0x1C => Key(vk::KP_ENTER),
        0x1D => Mod(ControlRight),
        0x35 => Key(vk::KP_DIVIDE),
        0x38 => Mod(AltRight),
        0x47 => Key(vk::HOME),
        0x48 => Key(vk::UP_ARROW),
        0x49 => Key(vk::PAGE_UP),
        0x4B => Key(vk::LEFT_ARROW),
        0x4D => Key(vk::RIGHT_ARROW),
        0x4F => Key(vk::END),
        0x50 => Key(vk::DOWN_ARROW),
        0x51 => Key(vk::PAGE_DOWN),
        0x52 => Key(vk::HOME), // Insert has no Mac equivalent; Home is the least-surprising
        0x53 => Key(vk::FORWARD_DELETE),
        0x5B => Mod(CommandLeft),  // Left GUI / Windows key
        0x5C => Mod(CommandRight), // Right GUI
        _ => return None,
    };
    Some(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_map_to_their_positional_keys() {
        assert_eq!(map(0x1E, false), Some(MappedKey::Key(vk::A)));
        assert_eq!(map(0x2E, false), Some(MappedKey::Key(vk::C)));
        assert_eq!(map(0x2F, false), Some(MappedKey::Key(vk::V)));
        assert_eq!(map(0x11, false), Some(MappedKey::Key(vk::W)));
    }

    #[test]
    fn digits_and_common_keys_map() {
        assert_eq!(map(0x02, false), Some(MappedKey::Key(vk::N1)));
        assert_eq!(map(0x0B, false), Some(MappedKey::Key(vk::N0)));
        assert_eq!(map(0x1C, false), Some(MappedKey::Key(vk::RETURN)));
        assert_eq!(map(0x39, false), Some(MappedKey::Key(vk::SPACE)));
        assert_eq!(map(0x0F, false), Some(MappedKey::Key(vk::TAB)));
        assert_eq!(map(0x0E, false), Some(MappedKey::Key(vk::DELETE)));
        assert_eq!(map(0x01, false), Some(MappedKey::Key(vk::ESCAPE)));
    }

    #[test]
    fn left_and_right_modifiers_are_distinct() {
        // The whole point of F-003.15.
        assert_eq!(map(0x38, false), Some(MappedKey::Modifier(AltLeft)));
        assert_eq!(map(0x38, true), Some(MappedKey::Modifier(AltRight)));
        assert_eq!(map(0x1D, false), Some(MappedKey::Modifier(ControlLeft)));
        assert_eq!(map(0x1D, true), Some(MappedKey::Modifier(ControlRight)));
        assert_eq!(map(0x2A, false), Some(MappedKey::Modifier(ShiftLeft)));
        assert_eq!(map(0x36, false), Some(MappedKey::Modifier(ShiftRight)));
    }

    #[test]
    fn the_command_key_comes_in_extended() {
        assert_eq!(map(0x5B, true), Some(MappedKey::Modifier(CommandLeft)));
        assert_eq!(map(0x5C, true), Some(MappedKey::Modifier(CommandRight)));
    }

    #[test]
    fn arrows_and_navigation_need_the_extended_flag() {
        // Without E0 these codes are the numeric keypad; with it, the nav cluster.
        assert_eq!(map(0x4B, false), Some(MappedKey::Key(vk::KP_4)));
        assert_eq!(map(0x4B, true), Some(MappedKey::Key(vk::LEFT_ARROW)));
        assert_eq!(map(0x48, false), Some(MappedKey::Key(vk::KP_8)));
        assert_eq!(map(0x48, true), Some(MappedKey::Key(vk::UP_ARROW)));
    }

    #[test]
    fn keypad_enter_and_slash_are_extended_variants() {
        assert_eq!(map(0x1C, true), Some(MappedKey::Key(vk::KP_ENTER)));
        assert_eq!(map(0x35, false), Some(MappedKey::Key(vk::SLASH)));
        assert_eq!(map(0x35, true), Some(MappedKey::Key(vk::KP_DIVIDE)));
    }

    #[test]
    fn caps_lock_is_a_modifier() {
        assert_eq!(map(0x3A, false), Some(MappedKey::Modifier(CapsLock)));
    }

    #[test]
    fn unmapped_codes_return_none_rather_than_a_wrong_key() {
        assert_eq!(map(0x00, false), None);
        assert_eq!(map(0xFF, false), None);
        assert_eq!(map(0x01, true), None); // extended Escape is not a thing
    }
}
