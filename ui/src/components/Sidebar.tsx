// Sidebar nav. Two sections: primary nav + a footer with version.
// Keyboard-friendly: each NavLink is a focusable button-like anchor.

import type { ComponentType } from "react";
import { useMemo } from "react";
import { NavLink } from "react-router-dom";

import {
  ActivityIcon,
  BookIcon,
  HistoryIcon,
  InfoIcon,
  KnowledgeGraphIcon,
  MeetingsIcon,
  SettingsIcon,
  SlidersIcon,
  SparklesIcon,
} from "../design/Icon";
import { MockingbirdMark } from "../design/components/MockingbirdMark";
import { t } from "../i18n";
import { useAppStore } from "../lib/store";

import styles from "./Sidebar.module.css";

interface NavItem {
  to: string;
  label: string;
  Icon: ComponentType<{ size?: number }>;
  /** Renders a small BETA pill next to the label. Used for subsystems
   * whose concept is proven but whose UX/quality hasn't crossed the
   * release-grade bar yet (mb-aho4). */
  beta?: boolean;
}

// Base nav. The KG entry is spliced in conditionally below based on
// the KgGraphEnabled store flag (Phase 1D Wave 1D.2 / ADR 0052):
// while OFF the sidebar must NOT show a KG affordance (graph-off-UI
// invariant J3 / 1D.2-extended). The item is inserted just after
// Dictations so the captured-text destinations stay grouped at the
// top of the nav, separate from the configuration cluster
// (Dictionary / Modes / Settings / About).
const NAV_BASE: NavItem[] = [
  { to: "/insights",   label: t("nav.insights"),   Icon: SparklesIcon },
  { to: "/dictations", label: t("nav.dictations"), Icon: HistoryIcon },
  { to: "/meetings",   label: t("nav.meetings"),   Icon: MeetingsIcon },
  { to: "/activity",   label: t("nav.activity"),   Icon: ActivityIcon, beta: true },
  { to: "/dictionary", label: t("nav.dictionary"), Icon: BookIcon },
  { to: "/modes",      label: t("nav.modes"),      Icon: SlidersIcon },
  { to: "/settings",   label: t("nav.settings"),   Icon: SettingsIcon },
  { to: "/about",      label: t("nav.about"),      Icon: InfoIcon },
];

const KG_NAV_ITEM: NavItem = {
  to: "/knowledge-graph",
  label: t("nav.knowledgeGraph"),
  Icon: KnowledgeGraphIcon,
  beta: true,
};

export function Sidebar() {
  // Surface the active transcription mode under the brand. Tiny but
  // valuable: "which mode does Right-Alt use right now?" is currently
  // a question that requires opening the Modes page to answer.
  const activeModeSlug = useAppStore((s) => s.activeModeSlug);
  const modes = useAppStore((s) => s.modes);
  // Phase 1D Wave 1D.2 (ADR 0052) -- conditional KG nav item.
  // `null` (initial / pre-boot) is treated as OFF; the App boot
  // effect resolves it within one tick of mount. This means the
  // KG nav item never "flashes visible" before the boot fetch
  // completes -- safer than the reverse.
  const kgGraphEnabled = useAppStore((s) => s.kgGraphEnabled);
  // `mb-v7pd` (v0.2.0-beta.1 smoke fix Bug 3) -- runtime app version
  // from Tauri's `getVersion()`, hydrated at App boot. We display
  // a thin-space placeholder (NOT `v0.1.0`) while null so the
  // footer doesn't reflow when the value resolves a tick later.
  const appVersion = useAppStore((s) => s.appVersion);
  const activeModeLabel = useMemo(() => {
    if (!activeModeSlug) return null;
    return modes.find((m) => m.slug === activeModeSlug)?.label ?? activeModeSlug;
  }, [activeModeSlug, modes]);
  const nav = useMemo<NavItem[]>(() => {
    if (!kgGraphEnabled) return NAV_BASE;
    // Splice KG just after Dictations so capture destinations stay
    // grouped. Index 2 = before Meetings; flip Meetings/KG order if
    // the visual hierarchy ever wants KG below Meetings.
    const out = [...NAV_BASE];
    out.splice(2, 0, KG_NAV_ITEM);
    return out;
  }, [kgGraphEnabled]);

  return (
    <aside className={styles.sidebar} aria-label="Primary navigation">
      <div className={styles.brand}>
        <MockingbirdMark size={22} state="static" />
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
        {nav.map(({ to, label, Icon, beta }) => (
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
            {beta ? (
              <span className={styles.betaPill} aria-label="Beta feature">
                {t("sidebar.betaTag")}
              </span>
            ) : null}
          </NavLink>
        ))}
      </nav>
      <div className={styles.footer}>
        <span className={styles.version} aria-label="App version">
          {appVersion ? `v${appVersion}` : "\u2009"}
        </span>
      </div>
    </aside>
  );
}
