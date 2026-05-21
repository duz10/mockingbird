//! Virtual-key name → Windows VK code mapping for meeting chord
//! settings.
//!
//! ## Why this module exists
//!
//! The settings DB stores the meeting chord as two string values
//! (`meeting_hotkey_modifier` = `"VK_RCONTROL"`,
//! `meeting_hotkey_key` = `"VK_OEM_PERIOD"`, …) because JSON does not
//! have native u32 literals and a string is humane to inspect with
//! `sqlite3 mockingbird.db`. The activation hook needs a `u32`. This
//! module is the single boundary that translates between them.
//!
//! ## Bug-history context (mb-fc1 / 2026-05-23 hotfix)
//!
//! The pre-hotfix runtime called `MeetingRuntimeConfig::defaults_with`
//! at startup and never read from the settings DB. So even though the
//! settings tab exposed a chord picker, changes were silently
//! ignored. That dead-code situation also masked a *second* bug: the
//! `VK_M` default collided with Microsoft 365 Copilot on Windows 11,
//! and Copilot's chord handler fires regardless of whether we also
//! see the keystroke. mb-fc1 fixes both at once: (a) wires
//! settings → ChordConfig at runtime spawn, and (b) changes the
//! default to `RightCtrl + Period` (a chord no major Microsoft app
//! claims).
//!
//! ## Scope
//!
//! Names this module accepts:
//!
//! - Modifiers: `VK_RCONTROL`, `VK_LCONTROL`, `VK_RMENU` (Right Alt),
//!   `VK_LMENU` (Left Alt), `VK_RSHIFT`, `VK_LSHIFT`,
//!   `VK_RWIN`, `VK_LWIN`.
//! - Main keys: `VK_A`..`VK_Z`, `VK_0`..`VK_9`, `VK_F1`..`VK_F24`,
//!   `VK_OEM_PERIOD`, `VK_OEM_COMMA`, `VK_OEM_1` (`;:`),
//!   `VK_OEM_2` (`/?`), `VK_OEM_5` (`\\|`), `VK_OEM_6` (`]}`),
//!   `VK_OEM_MINUS`, `VK_OEM_PLUS`, `VK_OEM_3` (\`~), `VK_SPACE`.
//!
//! Anything else returns `Err` — the caller is expected to fall back
//! to the documented default for that setting.

use crate::error::{AppError, AppResult};

/// Translate a `VK_*` name string to the matching Windows virtual-key
/// code. Case-insensitive (forgiving to a user who hand-edited the
/// settings DB).
///
/// Returns [`AppError::Other`] for unknown names so the caller can
/// log + fall through to the documented default rather than panicking
/// the runtime spawn. (The settings layer doesn't yet have its own
/// `AppError` variant; this stays catch-all to avoid touching
/// `error.rs` for a leaf parser.)
pub fn vk_name_to_code(name: &str) -> AppResult<u32> {
    // Folded to upper-case once so we don't pay for the comparison
    // mismatch on every match arm.
    let folded = name.trim().to_ascii_uppercase();
    let code = match folded.as_str() {
        // ----- Modifiers --------------------------------------------------
        "VK_RCONTROL" => 0xA3,
        "VK_LCONTROL" => 0xA2,
        "VK_CONTROL" => 0x11, // generic ctrl — caller's problem if it's ambiguous
        "VK_RMENU" => 0xA5,   // Right Alt
        "VK_LMENU" => 0xA4,   // Left Alt
        "VK_MENU" => 0x12,    // generic alt
        "VK_RSHIFT" => 0xA1,
        "VK_LSHIFT" => 0xA0,
        "VK_SHIFT" => 0x10,
        "VK_RWIN" => 0x5C,
        "VK_LWIN" => 0x5B,

        // ----- Function keys ----------------------------------------------
        "VK_F1" => 0x70,
        "VK_F2" => 0x71,
        "VK_F3" => 0x72,
        "VK_F4" => 0x73,
        "VK_F5" => 0x74,
        "VK_F6" => 0x75,
        "VK_F7" => 0x76,
        "VK_F8" => 0x77,
        "VK_F9" => 0x78,
        "VK_F10" => 0x79,
        "VK_F11" => 0x7A,
        "VK_F12" => 0x7B,
        "VK_F13" => 0x7C,
        "VK_F14" => 0x7D,
        "VK_F15" => 0x7E,
        "VK_F16" => 0x7F,
        "VK_F17" => 0x80,
        "VK_F18" => 0x81,
        "VK_F19" => 0x82,
        "VK_F20" => 0x83,
        "VK_F21" => 0x84,
        "VK_F22" => 0x85,
        "VK_F23" => 0x86,
        "VK_F24" => 0x87,

        // ----- OEM punctuation (the safe-chord territory) ----------------
        "VK_OEM_1" => 0xBA,      // ;:
        "VK_OEM_PLUS" => 0xBB,   // =+
        "VK_OEM_COMMA" => 0xBC,  // ,<
        "VK_OEM_MINUS" => 0xBD,  // -_
        "VK_OEM_PERIOD" => 0xBE, // .>  ← the new Phase MC default
        "VK_OEM_2" => 0xBF,      // /?
        "VK_OEM_3" => 0xC0,      // `~
        "VK_OEM_4" => 0xDB,      // [{
        "VK_OEM_5" => 0xDC,      // \|
        "VK_OEM_6" => 0xDD,      // ]}
        "VK_OEM_7" => 0xDE,      // '"

        // ----- Misc usable keys ------------------------------------------
        "VK_SPACE" => 0x20,
        "VK_BACK" => 0x08,
        "VK_TAB" => 0x09,
        "VK_RETURN" => 0x0D,
        "VK_ESCAPE" => 0x1B,
        "VK_INSERT" => 0x2D,
        "VK_DELETE" => 0x2E,

        // ----- A..Z / 0..9 -----------------------------------------------
        // Resolved in fall-through arms below because the byte arithmetic
        // is identical for all 36 keys.
        other => return vk_alnum(other).ok_or(unknown(name)),
    };
    Ok(code)
}

/// Reverse mapping — VK code back to the canonical `VK_*` name string
/// we persist. Used by the settings UI snapshot path so the picker
/// can show the persisted value even if the DB row contains a
/// historically-spelled variant (e.g. lowercase from a hand edit).
///
/// Returns `None` for VK codes outside the supported set; the caller
/// then falls back to the default name string.
pub fn vk_code_to_name(code: u32) -> Option<&'static str> {
    // Hand-coded reverse table — small, hot, and the duplication keeps
    // the forward map free of allocation overhead. Symmetric with
    // `vk_name_to_code` arm-for-arm.
    let name = match code {
        0xA3 => "VK_RCONTROL",
        0xA2 => "VK_LCONTROL",
        0x11 => "VK_CONTROL",
        0xA5 => "VK_RMENU",
        0xA4 => "VK_LMENU",
        0x12 => "VK_MENU",
        0xA1 => "VK_RSHIFT",
        0xA0 => "VK_LSHIFT",
        0x10 => "VK_SHIFT",
        0x5C => "VK_RWIN",
        0x5B => "VK_LWIN",
        0x70..=0x87 => return Some(F_KEY_NAMES[(code - 0x70) as usize]),
        0xBA => "VK_OEM_1",
        0xBB => "VK_OEM_PLUS",
        0xBC => "VK_OEM_COMMA",
        0xBD => "VK_OEM_MINUS",
        0xBE => "VK_OEM_PERIOD",
        0xBF => "VK_OEM_2",
        0xC0 => "VK_OEM_3",
        0xDB => "VK_OEM_4",
        0xDC => "VK_OEM_5",
        0xDD => "VK_OEM_6",
        0xDE => "VK_OEM_7",
        0x20 => "VK_SPACE",
        0x08 => "VK_BACK",
        0x09 => "VK_TAB",
        0x0D => "VK_RETURN",
        0x1B => "VK_ESCAPE",
        0x2D => "VK_INSERT",
        0x2E => "VK_DELETE",
        0x30..=0x39 => return Some(DIGIT_NAMES[(code - 0x30) as usize]),
        0x41..=0x5A => return Some(LETTER_NAMES[(code - 0x41) as usize]),
        _ => return None,
    };
    Some(name)
}

/// Static `["VK_F1", … "VK_F24"]` lookup table.
static F_KEY_NAMES: [&str; 24] = [
    "VK_F1", "VK_F2", "VK_F3", "VK_F4", "VK_F5", "VK_F6", "VK_F7", "VK_F8", "VK_F9", "VK_F10",
    "VK_F11", "VK_F12", "VK_F13", "VK_F14", "VK_F15", "VK_F16", "VK_F17", "VK_F18", "VK_F19",
    "VK_F20", "VK_F21", "VK_F22", "VK_F23", "VK_F24",
];

/// Static `["VK_0", … "VK_9"]` lookup table.
static DIGIT_NAMES: [&str; 10] = [
    "VK_0", "VK_1", "VK_2", "VK_3", "VK_4", "VK_5", "VK_6", "VK_7", "VK_8", "VK_9",
];

/// Static `["VK_A", … "VK_Z"]` lookup table.
static LETTER_NAMES: [&str; 26] = [
    "VK_A", "VK_B", "VK_C", "VK_D", "VK_E", "VK_F", "VK_G", "VK_H", "VK_I", "VK_J", "VK_K", "VK_L",
    "VK_M", "VK_N", "VK_O", "VK_P", "VK_Q", "VK_R", "VK_S", "VK_T", "VK_U", "VK_V", "VK_W", "VK_X",
    "VK_Y", "VK_Z",
];

fn vk_alnum(folded: &str) -> Option<u32> {
    // `VK_A`..`VK_Z` and `VK_0`..`VK_9` map by ASCII identity.
    let body = folded.strip_prefix("VK_")?;
    let bytes = body.as_bytes();
    if bytes.len() != 1 {
        return None;
    }
    let b = bytes[0];
    if b.is_ascii_uppercase() || b.is_ascii_digit() {
        Some(b as u32)
    } else {
        None
    }
}

fn unknown(name: &str) -> AppError {
    AppError::Other(format!("unknown VK name: {name:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_new_default_main_key() {
        // The mb-fc1 hotfix flipped the default from VK_M to
        // VK_OEM_PERIOD. The runtime spawn path lives or dies on this
        // single mapping.
        assert_eq!(vk_name_to_code("VK_OEM_PERIOD").unwrap(), 0xBE);
    }

    #[test]
    fn parses_the_default_modifier() {
        assert_eq!(vk_name_to_code("VK_RCONTROL").unwrap(), 0xA3);
    }

    #[test]
    fn is_case_insensitive() {
        // Folks editing the settings DB with sqlite3 will type
        // whatever case feels good in the moment. Forgive them.
        assert_eq!(vk_name_to_code("vk_rcontrol").unwrap(), 0xA3);
        assert_eq!(vk_name_to_code("Vk_Oem_Period").unwrap(), 0xBE);
    }

    #[test]
    fn trims_surrounding_whitespace() {
        // Settings round-tripped through JSON sometimes pick up a
        // trailing newline if a script appended one. Be charitable.
        assert_eq!(vk_name_to_code("  VK_M\n").unwrap(), 0x4D);
    }

    #[test]
    fn parses_letter_keys() {
        assert_eq!(vk_name_to_code("VK_A").unwrap(), 0x41);
        assert_eq!(vk_name_to_code("VK_M").unwrap(), 0x4D); // legacy default
        assert_eq!(vk_name_to_code("VK_Z").unwrap(), 0x5A);
    }

    #[test]
    fn parses_digit_keys() {
        assert_eq!(vk_name_to_code("VK_0").unwrap(), 0x30);
        assert_eq!(vk_name_to_code("VK_9").unwrap(), 0x39);
    }

    #[test]
    fn parses_function_keys_f1_through_f24() {
        for (n, expected) in (1..=24).zip(0x70u32..=0x87) {
            let name = format!("VK_F{n}");
            assert_eq!(
                vk_name_to_code(&name).unwrap(),
                expected,
                "VK_F{n} should be {expected:#x}"
            );
        }
    }

    #[test]
    fn parses_safe_oem_punctuation() {
        // The mb-fc1 hotfix specifically vetted these as Microsoft-365-
        // Copilot-safe defaults. Don't drift these mappings.
        assert_eq!(vk_name_to_code("VK_OEM_PERIOD").unwrap(), 0xBE);
        assert_eq!(vk_name_to_code("VK_OEM_COMMA").unwrap(), 0xBC);
        assert_eq!(vk_name_to_code("VK_OEM_1").unwrap(), 0xBA); // ;:
        assert_eq!(vk_name_to_code("VK_OEM_5").unwrap(), 0xDC); // \|
    }

    #[test]
    fn parses_all_modifier_variants() {
        for (name, code) in [
            ("VK_RCONTROL", 0xA3u32),
            ("VK_LCONTROL", 0xA2),
            ("VK_RMENU", 0xA5),
            ("VK_LMENU", 0xA4),
            ("VK_RSHIFT", 0xA1),
            ("VK_LSHIFT", 0xA0),
            ("VK_RWIN", 0x5C),
            ("VK_LWIN", 0x5B),
        ] {
            assert_eq!(vk_name_to_code(name).unwrap(), code, "{name}");
        }
    }

    #[test]
    fn rejects_unknown_names_cleanly() {
        let err = vk_name_to_code("VK_SOMETHING_INVENTED").unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("VK_SOMETHING_INVENTED"),
            "error should echo the bad name; got: {msg}"
        );
    }

    #[test]
    fn rejects_empty_string() {
        assert!(vk_name_to_code("").is_err());
    }

    #[test]
    fn rejects_missing_prefix() {
        assert!(vk_name_to_code("OEM_PERIOD").is_err());
        assert!(vk_name_to_code("M").is_err());
    }

    #[test]
    fn rejects_multi_char_body_for_alnum_fallback() {
        // `VK_MM` is not a real key. The alnum fall-through arm only
        // accepts single chars.
        assert!(vk_name_to_code("VK_MM").is_err());
    }

    #[test]
    fn reverse_mapping_for_canonical_names() {
        assert_eq!(vk_code_to_name(0xA3), Some("VK_RCONTROL"));
        assert_eq!(vk_code_to_name(0xBE), Some("VK_OEM_PERIOD"));
        assert_eq!(vk_code_to_name(0x4D), Some("VK_M"));
        assert_eq!(vk_code_to_name(0x7C), Some("VK_F13"));
        assert_eq!(vk_code_to_name(0x41), Some("VK_A"));
        assert_eq!(vk_code_to_name(0x30), Some("VK_0"));
    }

    #[test]
    fn reverse_mapping_returns_none_for_unknown_codes() {
        // 0x99 isn't in our supported set; we return None and the
        // caller falls back to the default name string.
        assert_eq!(vk_code_to_name(0x99), None);
        assert_eq!(vk_code_to_name(0xFF), None);
    }

    #[test]
    fn round_trip_every_supported_name_is_lossless() {
        // Every canonical name parses to a code that maps back to the
        // same name. Catches drift in either direction.
        let canonicals = [
            "VK_RCONTROL",
            "VK_LCONTROL",
            "VK_RMENU",
            "VK_LMENU",
            "VK_RSHIFT",
            "VK_LSHIFT",
            "VK_RWIN",
            "VK_LWIN",
            "VK_M",
            "VK_OEM_PERIOD",
            "VK_OEM_COMMA",
            "VK_OEM_1",
            "VK_OEM_5",
            "VK_F13",
            "VK_F24",
            "VK_SPACE",
            "VK_A",
            "VK_Z",
            "VK_0",
            "VK_9",
        ];
        for name in canonicals {
            let code = vk_name_to_code(name).unwrap_or_else(|_| panic!("forward {name}"));
            let back =
                vk_code_to_name(code).unwrap_or_else(|| panic!("reverse {name} -> {code:#x}"));
            assert_eq!(back, name, "round-trip drift on {name}");
        }
    }
}
