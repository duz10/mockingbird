// Sidebar nav. Two sections: primary nav + a footer with version.
// Keyboard-friendly: each NavLink is a focusable button-like anchor.

import type { ComponentType } from "react";
import { useMemo } from "react";
import { NavLink } from "react-router-dom";

import {
  BookIcon,
  HistoryIcon,
  InfoIcon,
  MicIcon,
  SettingsIcon,
  SlidersIcon,
  SparklesIcon,
} from "../design/Icon";
import { t } from "../i18n";
import { useAppStore } from "../lib/store";
import type { DesignVersion } from "../lib/store";

import styles from "./Sidebar.module.css";

// Dev-only toggle between the legacy v1 design and the new Design
// Language v1 (v2) — visible badge in the sidebar footer. Goes away
// in W6 cutover. ADR 0023.
function DesignBadge() {
  const designVersion = useAppStore((s) => s.designVersion);
  const setDesignVersion = useAppStore((s) => s.setDesignVersion);
  const next: DesignVersion = designVersion === "v2" ? "v1" : "v2";
  return (
    <button
      type="button"
      className={styles.designBadge}
      data-active={designVersion}
      onClick={() => {
        setDesignVersion(next);
        // Force a full reload so any v1 CSS that doesn't react to
        // the bridge tokens (e.g. images, JS-driven inline styles)
        // gets a clean pass. Cheap and unambiguous.
        window.location.reload();
      }}
      title={`Switch design system to ${next.toUpperCase()} (dev only)`}
      aria-label={`Design system: ${designVersion.toUpperCase()}. Click to switch to ${next.toUpperCase()}.`}
    >
      <span className={styles.designBadgeLabel}>design</span>
      <span className={styles.designBadgeValue}>{designVersion.toUpperCase()}</span>
    </button>
  );
}

interface NavItem {
  to: string;
  label: string;
  Icon: ComponentType<{ size?: number }>;
}

const NAV: NavItem[] = [
  { to: "/insights",   label: t("nav.insights"),   Icon: SparklesIcon },
  { to: "/history",    label: t("nav.history"),    Icon: HistoryIcon },
  { to: "/dictionary", label: t("nav.dictionary"), Icon: BookIcon },
  { to: "/modes",      label: t("nav.modes"),      Icon: SlidersIcon },
  { to: "/settings",   label: t("nav.settings"),   Icon: SettingsIcon },
  { to: "/about",      label: t("nav.about"),      Icon: InfoIcon },
];

export function Sidebar() {
  // Surface the active transcription mode under the brand. Tiny but
  // valuable: "which mode does Right-Alt use right now?" is currently
  // a question that requires opening the Modes page to answer.
  const activeModeSlug = useAppStore((s) => s.activeModeSlug);
  const modes = useAppStore((s) => s.modes);
  const activeModeLabel = useMemo(() => {
    if (!activeModeSlug) return null;
    return modes.find((m) => m.slug === activeModeSlug)?.label ?? activeModeSlug;
  }, [activeModeSlug, modes]);

  return (
    <aside className={styles.sidebar} aria-label="Primary navigation">
      <div className={styles.brand}>
        <MicIcon size={20} />
        <span className={styles.brandText}>Mockingbird</span>
      </div>
      {activeModeLabel ? (
        <div className={styles.activeMode} aria-live="polite">
          <span
            className={styles.activeModeDot}
            style={{
              background: `var(--mode-${activeModeSlug}, var(--mode-normal))`,
            }}
            aria-hidden
          />
          <span className={styles.activeModeLabel}>
            {t("sidebar.activeMode")}: <strong>{activeModeLabel}</strong>
          </span>
        </div>
      ) : null}
      <nav className={styles.nav}>
        {NAV.map(({ to, label, Icon }) => (
          <NavLink
            key={to}
            to={to}
            className={({ isActive }) =>
              isActive ? `${styles.link} ${styles.active}` : styles.link
            }
            // Stops sidebar links from triggering double navigation on
            // Tauri's webview (a quirk of HashRouter + middle-click).
            onAuxClick={(e) => e.preventDefault()}
          >
            <Icon size={18} />
            <span className={styles.linkLabel}>{label}</span>
          </NavLink>
        ))}
      </nav>
      <div className={styles.footer}>
        <DesignBadge />
        <span className={styles.version} aria-label="App version">
          v0.1.0
        </span>
      </div>
    </aside>
  );
}
