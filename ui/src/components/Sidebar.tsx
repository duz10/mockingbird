// Sidebar nav. Two sections: primary nav + a footer with version.
// Keyboard-friendly: each NavLink is a focusable button-like anchor.

import type { ComponentType } from "react";
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

import styles from "./Sidebar.module.css";

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
  return (
    <aside className={styles.sidebar} aria-label="Primary navigation">
      <div className={styles.brand}>
        <MicIcon size={20} />
        <span className={styles.brandText}>Mockingbird</span>
      </div>
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
        <span className={styles.version} aria-label="App version">
          v0.1.0
        </span>
      </div>
    </aside>
  );
}
