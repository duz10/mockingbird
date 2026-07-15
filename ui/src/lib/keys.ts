// Platform-aware keyboard label helpers.
//
// macOS uses different modifier names + glyphs than Windows (Alt →
// Option/⌥, the Windows/Meta key → Command/⌘). Every helper takes an
// `isMac` flag and only diverges when it's true, so Windows labels stay
// byte-identical while macOS shows native names/symbols.
//
// Kept tiny + pure (no IPC, no React) so it's trivially unit-testable
// and importable from any webview — including the Command Center, which
// runs in its own bundle and resolves `isMac` via `api.host_os()`.

/** macOS modifier glyphs, for building native shortcut labels. */
export const MAC_MODIFIER_SYMBOL = {
  cmd: "\u2318", // ⌘
  option: "\u2325", // ⌥
  ctrl: "\u2303", // ⌃
  shift: "\u21e7", // ⇧
} as const;

/**
 * Label for the push-to-talk dictation hotkey. The physical key is the
 * right-hand modifier: macOS calls it "Right Option" (⌥), Windows
 * "Right Alt". Same key, platform-native name.
 */
export function dictationPttLabel(isMac: boolean): string {
  return isMac ? "Right Option" : "Right Alt";
}
