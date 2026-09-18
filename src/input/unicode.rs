//! Assembly of UTF-16 surrogate pairs from RDP Unicode keyboard events
//! (F-003a.14, F-003a.15, F-003a.17).
//!
//! A Unicode keyboard event carries one UTF-16 code unit (MS-RDPBCGR
//! 2.2.8.1.1.3.1.1.2), so a character outside the BMP — an emoji — arrives as
//! two events: a high surrogate, then a low one. Pure state machine, no macOS
//! calls; `cgevent` feeds it and injects only what it emits.

/// Assembles surrogate pairs, with a separate buffer for presses and for
/// releases.
///
/// The buffers are separate because clients interleave directions: both
/// `↓H ↓L ↑H ↑L` and `↓H ↑H ↓L ↑L` must yield one press and one release of the
/// same character (F-003a.14).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SurrogateAssembler {
    pending_press: Option<u16>,
    pending_release: Option<u16>,
}

/// What one code unit produced.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// A complete character to inject in this event's direction.
    pub emit: Option<char>,
    /// A lone surrogate thrown away (F-003a.15), for the debug log.
    pub dropped: Option<u16>,
}

fn is_high(unit: u16) -> bool {
    (0xD800..=0xDBFF).contains(&unit)
}

fn is_low(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

impl SurrogateAssembler {
    fn pending(&mut self, pressed: bool) -> &mut Option<u16> {
        if pressed {
            &mut self.pending_press
        } else {
            &mut self.pending_release
        }
    }

    /// Feeds one code unit in the given direction.
    pub fn push(&mut self, unit: u16, pressed: bool) -> Step {
        let pending = self.pending(pressed);
        if is_high(unit) {
            // A high surrogate followed by another high one: the first is lone.
            let dropped = pending.replace(unit);
            return Step { emit: None, dropped };
        }
        if is_low(unit) {
            return match pending.take() {
                Some(high) => Step {
                    emit: char::decode_utf16([high, unit]).next().and_then(Result::ok),
                    dropped: None,
                },
                None => Step {
                    emit: None,
                    dropped: Some(unit),
                },
            };
        }
        // A BMP character: processed normally, but it breaks any pending pair.
        Step {
            emit: char::from_u32(u32::from(unit)),
            dropped: pending.take(),
        }
    }

    /// A non-Unicode key event in the given direction: it interrupts a pending
    /// pair, which is then lone (F-003a.15). Returns the dropped unit, if any.
    pub fn interrupt(&mut self, pressed: bool) -> Option<u16> {
        self.pending(pressed).take()
    }

    /// Clears both buffers on an input reset (F-003a.17). Returns how many
    /// pending high surrogates were thrown away.
    pub fn reset(&mut self) -> usize {
        usize::from(self.pending_press.take().is_some()) + usize::from(self.pending_release.take().is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIGH: u16 = 0xD83D;
    const LOW: u16 = 0xDE00;
    const GRINNING: char = '\u{1F600}';

    fn emitted(assembler: &mut SurrogateAssembler, events: &[(u16, bool)]) -> Vec<(char, bool)> {
        events
            .iter()
            .filter_map(|&(unit, pressed)| assembler.push(unit, pressed).emit.map(|c| (c, pressed)))
            .collect()
    }

    /// F-003a.14, F-003a.18 (AC-9): `↓D83D ↓DE00 ↑D83D ↑DE00` ⇒ one press and
    /// one release of 😀.
    #[test]
    fn f003a_14_a_surrogate_pair_becomes_one_character() {
        let mut assembler = SurrogateAssembler::default();
        let out = emitted(
            &mut assembler,
            &[(HIGH, true), (LOW, true), (HIGH, false), (LOW, false)],
        );
        assert_eq!(out, vec![(GRINNING, true), (GRINNING, false)]);
    }

    /// F-003a.14 (AC-9): the interleaved order gives the same result, because
    /// presses and releases are buffered separately.
    #[test]
    fn f003a_14_interleaved_directions_give_the_same_result() {
        let mut assembler = SurrogateAssembler::default();
        let out = emitted(
            &mut assembler,
            &[(HIGH, true), (HIGH, false), (LOW, true), (LOW, false)],
        );
        assert_eq!(out, vec![(GRINNING, true), (GRINNING, false)]);
    }

    /// F-003a.14 (AC-9): a BMP character `↓0105` ⇒ one press of `ą`, unbuffered.
    #[test]
    fn f003a_14_a_bmp_character_passes_straight_through() {
        let mut assembler = SurrogateAssembler::default();
        assert_eq!(
            assembler.push(0x0105, true),
            Step {
                emit: Some('ą'),
                dropped: None
            }
        );
    }

    /// F-003a.15 (AC-9): a lone low surrogate `↓DE00` ⇒ nothing.
    #[test]
    fn f003a_15_a_lone_low_surrogate_is_dropped() {
        let mut assembler = SurrogateAssembler::default();
        assert_eq!(
            assembler.push(LOW, true),
            Step {
                emit: None,
                dropped: Some(LOW)
            }
        );
    }

    /// F-003a.15 (AC-9): `↓D83D ↓0061` ⇒ only `a`; the high surrogate is dropped.
    #[test]
    fn f003a_15_a_high_surrogate_interrupted_by_a_character_is_dropped() {
        let mut assembler = SurrogateAssembler::default();
        assert_eq!(assembler.push(HIGH, true), Step::default());
        assert_eq!(
            assembler.push(0x0061, true),
            Step {
                emit: Some('a'),
                dropped: Some(HIGH)
            }
        );
    }

    /// F-003a.15: two high surrogates in a row — the first is lone, the second
    /// still pairs.
    #[test]
    fn f003a_15_a_second_high_surrogate_replaces_the_first() {
        let mut assembler = SurrogateAssembler::default();
        assembler.push(HIGH, true);
        assert_eq!(assembler.push(HIGH, true).dropped, Some(HIGH));
        assert_eq!(assembler.push(LOW, true).emit, Some(GRINNING));
    }

    /// F-003a.15: a scancode event in the same direction interrupts the pair;
    /// the other direction's buffer is untouched.
    #[test]
    fn f003a_15_a_scancode_interrupts_only_its_own_direction() {
        let mut assembler = SurrogateAssembler::default();
        assembler.push(HIGH, true);
        assembler.push(HIGH, false);
        assert_eq!(assembler.interrupt(true), Some(HIGH));
        assert_eq!(assembler.push(LOW, true).emit, None, "the press pair was broken");
        assert_eq!(
            assembler.push(LOW, false).emit,
            Some(GRINNING),
            "the release pair was not"
        );
    }

    /// F-003a.17, F-003a.15 (AC-9): `↓D83D`, reset ⇒ nothing, and a following
    /// `↓DE00` gives nothing either.
    #[test]
    fn f003a_17_reset_clears_both_buffers() {
        let mut assembler = SurrogateAssembler::default();
        assembler.push(HIGH, true);
        assembler.push(HIGH, false);
        assert_eq!(assembler.reset(), 2);
        assert_eq!(
            assembler.push(LOW, true),
            Step {
                emit: None,
                dropped: Some(LOW)
            }
        );
        assert_eq!(assembler.reset(), 0, "nothing left after the first reset");
    }
}
