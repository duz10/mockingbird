//! Pure macOS keycode / modifier translation — the OS-free half of the
//! `CGEventTap` hotkey listener (ADR 0057).
//!
//! Everything here is deterministic and unit-testable on any build host
//! (no Input Monitoring permission, no event tap). The OS machinery
//! that *drives* this lives in [`super`] (`macos/mod.rs`); this is the
//! macOS analogue of `windows.rs::classify_keystroke`.
//!
//! ### Modifier vs. ordinary keys
//!
//! macOS does **not** emit key-down/key-up for modifier keys (Option,
//! Command, Control, Shift) — it emits `flagsChanged`, and you infer
//! press-vs-release from whether the modifier's bit is set in the
//! post-event flags. The Windows default hotkey is *Right Alt*; the
//! macOS parity default is *Right Option* (`kVK_RightOption`), a
//! modifier — so [`classify_event`] handles the `flagsChanged` path
//! using the device-dependent left/right flag bits (the generic
//! `kCGEventFlagMaskAlternate` can't tell left Option from right).

use std::time::Instant;

use crate::hotkey::HotkeyEvent;

// --------------------------------------------------------------------
// Key codes & modifier flag bits (pure constants — no OS calls)
// --------------------------------------------------------------------

/// `kVK_RightOption` (Carbon `HIToolbox/Events.h`). The Phase 3
/// default hotkey — parity with Windows' Right Alt (`VK_RMENU`).
pub const KVK_RIGHT_OPTION: u32 = 0x3D;
/// `kVK_Option` (left Option).
pub const KVK_LEFT_OPTION: u32 = 0x3A;
/// `kVK_RightCommand`.
pub const KVK_RIGHT_COMMAND: u32 = 0x36;
/// `kVK_Command` (left Command).
pub const KVK_LEFT_COMMAND: u32 = 0x37;
/// `kVK_RightControl`.
pub const KVK_RIGHT_CONTROL: u32 = 0x3E;
/// `kVK_Control` (left Control).
pub const KVK_LEFT_CONTROL: u32 = 0x3B;
/// `kVK_RightShift`.
pub const KVK_RIGHT_SHIFT: u32 = 0x3C;
/// `kVK_Shift` (left Shift).
pub const KVK_LEFT_SHIFT: u32 = 0x38;
/// `kVK_Escape` — surfaced as [`HotkeyEvent::Escape`] for the §6.1
/// cancel-during-recording path, regardless of the configured hotkey.
pub const KVK_ESCAPE: u32 = 0x35;

/// Default macOS hotkey: Right Option. ADR 0019's conflict-probe
/// fallback chain may rebind this at startup on a later wave.
pub const DEFAULT_VK: u32 = KVK_RIGHT_OPTION;

// Device-dependent modifier flag bits (IOKit `IOLLEvent.h`, the
// `NX_DEVICE*KEYMASK` family). `CGEventFlags` carries these low bits
// in addition to the device-*independent* masks (e.g.
// `kCGEventFlagMaskAlternate`), and — unlike the generic masks — they
// distinguish LEFT from RIGHT modifiers. We need that because the
// default hotkey is specifically the *right* Option key.
const NX_DEVICE_LCTL: u64 = 0x0000_0001;
const NX_DEVICE_LSHIFT: u64 = 0x0000_0002;
const NX_DEVICE_RSHIFT: u64 = 0x0000_0004;
const NX_DEVICE_LCMD: u64 = 0x0000_0008;
const NX_DEVICE_RCMD: u64 = 0x0000_0010;
const NX_DEVICE_LALT: u64 = 0x0000_0020;
const NX_DEVICE_RALT: u64 = 0x0000_0040;
const NX_DEVICE_RCTL: u64 = 0x0000_2000;

/// Map a modifier keycode to its device-dependent flag bit, or `None`
/// if the keycode is an ordinary (non-modifier) key.
///
/// Pure — the linchpin that lets [`classify_event`] decide whether a
/// key is driven by `flagsChanged` (modifiers) or key-down/up
/// (everything else) without any OS state.
pub fn modifier_mask_for_keycode(keycode: u32) -> Option<u64> {
    Some(match keycode {
        KVK_RIGHT_OPTION => NX_DEVICE_RALT,
        KVK_LEFT_OPTION => NX_DEVICE_LALT,
        KVK_RIGHT_COMMAND => NX_DEVICE_RCMD,
        KVK_LEFT_COMMAND => NX_DEVICE_LCMD,
        KVK_RIGHT_CONTROL => NX_DEVICE_RCTL,
        KVK_LEFT_CONTROL => NX_DEVICE_LCTL,
        KVK_RIGHT_SHIFT => NX_DEVICE_RSHIFT,
        KVK_LEFT_SHIFT => NX_DEVICE_LSHIFT,
        _ => return None,
    })
}

/// `true` iff `keycode` names a modifier key (Option/Command/Control/
/// Shift, left or right).
#[inline]
pub fn is_modifier_keycode(keycode: u32) -> bool {
    modifier_mask_for_keycode(keycode).is_some()
}

// --------------------------------------------------------------------
// Pure classifier — testable without an OS tap
// --------------------------------------------------------------------

/// OS-independent shape of the three `CGEventType`s we care about.
///
/// Keeping the classifier free of `core-graphics` types means every
/// translation case is unit-testable on a build host without poking
/// the real event-tap machinery (which needs Input Monitoring).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacEventKind {
    /// `kCGEventKeyDown` — an ordinary key was pressed.
    KeyDown,
    /// `kCGEventKeyUp` — an ordinary key was released.
    KeyUp,
    /// `kCGEventFlagsChanged` — a modifier key changed state. Press vs.
    /// release is inferred from `flags`.
    FlagsChanged,
}

/// Translate one macOS keyboard event into an optional
/// [`HotkeyEvent`] for the shared §6.1 state machine.
///
/// This is the macOS analogue of `windows.rs::classify_keystroke`:
///
/// * Ordinary key (`configured_vk` is **not** a modifier): `KeyDown`
///   / `KeyUp` of `configured_vk` map straight through.
/// * Modifier key (`configured_vk` **is** a modifier — the default
///   Right Option case): we only see `FlagsChanged`. Press vs. release
///   is read from the device-dependent flag bit
///   ([`modifier_mask_for_keycode`]) in `flags`.
/// * `Escape` key-down is **always** surfaced as
///   [`HotkeyEvent::Escape`], regardless of `configured_vk`, so the
///   cancel path works from any source (mirrors Windows).
///
/// `flags` is the raw `CGEventFlags` bit-set on the event (only read
/// for the `FlagsChanged` path). Returns `None` for anything
/// irrelevant. Pure — every branch is deterministic and testable.
pub fn classify_event(
    kind: MacEventKind,
    keycode: u32,
    flags: u64,
    configured_vk: u32,
    at: Instant,
) -> Option<HotkeyEvent> {
    match kind {
        MacEventKind::KeyDown => {
            if keycode == KVK_ESCAPE {
                Some(HotkeyEvent::Escape { at })
            } else if keycode == configured_vk && !is_modifier_keycode(keycode) {
                Some(HotkeyEvent::KeyDown { vk: keycode, at })
            } else {
                None
            }
        }
        MacEventKind::KeyUp => {
            if keycode == configured_vk && !is_modifier_keycode(keycode) {
                Some(HotkeyEvent::KeyUp { vk: keycode, at })
            } else {
                None
            }
        }
        MacEventKind::FlagsChanged => {
            if keycode != configured_vk {
                return None;
            }
            // Only meaningful when the configured key is a modifier;
            // for an ordinary key this yields `None` (the OS would
            // never send flagsChanged for it anyway).
            let mask = modifier_mask_for_keycode(keycode)?;
            if flags & mask != 0 {
                Some(HotkeyEvent::KeyDown { vk: keycode, at })
            } else {
                Some(HotkeyEvent::KeyUp { vk: keycode, at })
            }
        }
    }
}

// --------------------------------------------------------------------
// Tests — pure classifier (no OS tap; runs without Input Monitoring)
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    // ---- ordinary-key path (KeyDown / KeyUp) ----

    #[test]
    fn keydown_of_configured_ordinary_key_yields_keydown() {
        // F13 (0x69) is an ordinary key, not a modifier.
        let f13 = 0x69;
        let ev = classify_event(MacEventKind::KeyDown, f13, 0, f13, now()).unwrap();
        match ev {
            HotkeyEvent::KeyDown { vk, .. } => assert_eq!(vk, f13),
            other => panic!("expected KeyDown, got {other:?}"),
        }
    }

    #[test]
    fn keyup_of_configured_ordinary_key_yields_keyup() {
        let f13 = 0x69;
        let ev = classify_event(MacEventKind::KeyUp, f13, 0, f13, now()).unwrap();
        assert!(matches!(ev, HotkeyEvent::KeyUp { .. }));
    }

    #[test]
    fn keydown_of_escape_yields_escape_regardless_of_configured_vk() {
        let ev = classify_event(
            MacEventKind::KeyDown,
            KVK_ESCAPE,
            0,
            KVK_RIGHT_OPTION,
            now(),
        )
        .unwrap();
        assert!(matches!(ev, HotkeyEvent::Escape { .. }));
    }

    #[test]
    fn keyup_of_escape_is_not_surfaced() {
        // Only Escape key-down matters; key-up is ignored.
        assert!(
            classify_event(MacEventKind::KeyUp, KVK_ESCAPE, 0, KVK_RIGHT_OPTION, now()).is_none()
        );
    }

    #[test]
    fn keydown_of_unrelated_key_yields_none() {
        // 0x0B is 'B' — an ordinary key that is not configured.
        assert!(classify_event(MacEventKind::KeyDown, 0x0B, 0, 0x69, now()).is_none());
    }

    #[test]
    fn keydown_for_a_modifier_keycode_is_ignored() {
        // The OS never emits KeyDown for a modifier; even if it did we
        // ignore it (the flagsChanged path owns modifiers).
        assert!(classify_event(
            MacEventKind::KeyDown,
            KVK_RIGHT_OPTION,
            NX_DEVICE_RALT,
            KVK_RIGHT_OPTION,
            now()
        )
        .is_none());
    }

    // ---- modifier path (FlagsChanged) ----

    #[test]
    fn flagschanged_right_option_press_yields_keydown() {
        // Right Option bit set → press.
        let ev = classify_event(
            MacEventKind::FlagsChanged,
            KVK_RIGHT_OPTION,
            NX_DEVICE_RALT,
            KVK_RIGHT_OPTION,
            now(),
        )
        .unwrap();
        match ev {
            HotkeyEvent::KeyDown { vk, .. } => assert_eq!(vk, KVK_RIGHT_OPTION),
            other => panic!("expected KeyDown, got {other:?}"),
        }
    }

    #[test]
    fn flagschanged_right_option_release_yields_keyup() {
        // Right Option bit cleared → release.
        let ev = classify_event(
            MacEventKind::FlagsChanged,
            KVK_RIGHT_OPTION,
            0,
            KVK_RIGHT_OPTION,
            now(),
        )
        .unwrap();
        assert!(matches!(ev, HotkeyEvent::KeyUp { .. }));
    }

    #[test]
    fn flagschanged_left_option_does_not_match_right_option_config() {
        // Left Option's keycode differs → no event when Right Option
        // is configured. Guards the L/R distinction.
        assert!(classify_event(
            MacEventKind::FlagsChanged,
            KVK_LEFT_OPTION,
            NX_DEVICE_LALT,
            KVK_RIGHT_OPTION,
            now()
        )
        .is_none());
    }

    #[test]
    fn flagschanged_for_ordinary_configured_key_yields_none() {
        // If the user configured an ordinary key, a stray flagsChanged
        // carrying its keycode must not produce an event.
        let f13 = 0x69;
        assert!(classify_event(MacEventKind::FlagsChanged, f13, 0, f13, now()).is_none());
    }

    #[test]
    fn rebinding_to_right_command_works() {
        // Conflict-probe rebind: Right Command becomes the hotkey.
        let press = classify_event(
            MacEventKind::FlagsChanged,
            KVK_RIGHT_COMMAND,
            NX_DEVICE_RCMD,
            KVK_RIGHT_COMMAND,
            now(),
        );
        assert!(matches!(press, Some(HotkeyEvent::KeyDown { .. })));
        // Right Option events now pass through untouched.
        assert!(classify_event(
            MacEventKind::FlagsChanged,
            KVK_RIGHT_OPTION,
            NX_DEVICE_RALT,
            KVK_RIGHT_COMMAND,
            now()
        )
        .is_none());
    }

    // ---- modifier-mask helper ----

    #[test]
    fn modifier_mask_classifies_modifiers_and_ordinary_keys() {
        assert!(is_modifier_keycode(KVK_RIGHT_OPTION));
        assert!(is_modifier_keycode(KVK_LEFT_SHIFT));
        assert!(!is_modifier_keycode(KVK_ESCAPE)); // Escape is ordinary
        assert!(!is_modifier_keycode(0x69)); // F13
    }
}
