// macOS-port (v1 honest-surface) — the "AI command modes" section is
// Windows-only on macOS.
//
// The AI command modes (rewrite / expand / summarize) act on selected/
// clipboard text and are triggered by a global command hotkey. That
// hotkey path is `#[cfg(target_os = "windows")]` (the macOS event tap
// only binds the single Right-Option dictation key), so on a Mac these
// modes literally cannot be triggered. Rather than show editable cards
// with Windows shortcut labels — and a model picker that's moot because
// nothing can invoke it — we render this honest "coming soon" note in
// their place, matching the Activity / Knowledge-Graph treatment.
//
// Presentation-only + reversible: when a macOS trigger lands, drop the
// `isMac` gate at the Modes.tsx call site and the real cards return.

import { t } from "../i18n";

import styles from "./Modes.module.css";

/** Inline placeholder for the AI command modes section on macOS. */
export function ModesCommandsComingSoon() {
  return (
    <section className={styles.group} aria-labelledby="modes-cmd-heading">
      <header className={styles.groupHeader}>
        <h2 id="modes-cmd-heading" className={styles.groupTitle}>
          {t("modes.section.commands")}
        </h2>
        <p className={styles.groupHelp}>
          {t("modes.section.commands.comingSoonMac")}
        </p>
      </header>
    </section>
  );
}
