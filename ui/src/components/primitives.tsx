// Small UI primitives used across pages. Keep this file focused —
// when a primitive grows beyond ~50 lines it should be promoted to
// its own module (and gain its own .module.css).

import type { ReactNode } from "react";
import styles from "./primitives.module.css";

/* ------------------------------------------------------------------ */
/* PageHeader — title + optional subtitle + optional right-side slot.  */
/* Every top-level page starts with one of these.                      */
/* ------------------------------------------------------------------ */

interface PageHeaderProps {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
}

export function PageHeader({ title, subtitle, actions }: PageHeaderProps) {
  return (
    <header className={styles.pageHeader}>
      <div className={styles.pageHeaderText}>
        <h1 className={styles.pageTitle}>{title}</h1>
        {subtitle ? <p className={styles.pageSubtitle}>{subtitle}</p> : null}
      </div>
      {actions ? <div className={styles.pageHeaderActions}>{actions}</div> : null}
    </header>
  );
}

/* ------------------------------------------------------------------ */
/* Card — generic surface for grouping content.                        */
/* ------------------------------------------------------------------ */

interface CardProps {
  children: ReactNode;
  title?: string;
  className?: string;
  /** ARIA label for screen readers when no title is rendered. */
  ariaLabel?: string;
}

export function Card({ children, title, className, ariaLabel }: CardProps) {
  return (
    <section
      className={`${styles.card} ${className ?? ""}`}
      aria-label={ariaLabel ?? title}
    >
      {title ? <h2 className={styles.cardTitle}>{title}</h2> : null}
      {children}
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* EmptyState — friendly "nothing here yet" placeholder.               */
/* ------------------------------------------------------------------ */

interface EmptyStateProps {
  title: string;
  subtitle?: string;
  icon?: ReactNode;
  action?: ReactNode;
}

export function EmptyState({ title, subtitle, icon, action }: EmptyStateProps) {
  return (
    <div className={styles.empty} role="status">
      {icon ? <div className={styles.emptyIcon}>{icon}</div> : null}
      <h3 className={styles.emptyTitle}>{title}</h3>
      {subtitle ? <p className={styles.emptySubtitle}>{subtitle}</p> : null}
      {action ? <div className={styles.emptyAction}>{action}</div> : null}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Button — variants: primary / ghost / danger.                        */
/* ------------------------------------------------------------------ */

type ButtonVariant = "primary" | "ghost" | "danger";
type ButtonSize = "sm" | "md";

interface ButtonProps {
  children: ReactNode;
  onClick?: () => void;
  variant?: ButtonVariant;
  size?: ButtonSize;
  type?: "button" | "submit" | "reset";
  disabled?: boolean;
  ariaLabel?: string;
  title?: string;
}

export function Button({
  children,
  onClick,
  variant = "ghost",
  size = "md",
  type = "button",
  disabled,
  ariaLabel,
  title,
}: ButtonProps) {
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={disabled}
      aria-label={ariaLabel}
      title={title}
      className={`${styles.btn} ${styles[`btn_${variant}`]} ${styles[`btn_${size}`]}`}
    >
      {children}
    </button>
  );
}

/* ------------------------------------------------------------------ */
/* Pill — mode badge / source badge / status badge.                    */
/* The `tone` prop is a token name (without the `--` prefix).          */
/* ------------------------------------------------------------------ */

interface PillProps {
  children: ReactNode;
  tone?: string; // e.g. "mode-normal", "status-ok"
  className?: string;
}

export function Pill({ children, tone, className }: PillProps) {
  return (
    <span
      className={`${styles.pill} ${className ?? ""}`}
      style={tone ? { ["--pill-color" as never]: `var(--${tone})` } : undefined}
    >
      {children}
    </span>
  );
}

/* ------------------------------------------------------------------ */
/* Spinner — indeterminate progress. Used while a page is loading its  */
/* initial fetch. Kept dead simple: a CSS-animated ring. Respects      */
/* prefers-reduced-motion (stops spinning, stays visible).             */
/* ------------------------------------------------------------------ */

interface SpinnerProps {
  /** Pixel size of the ring. Default 24. */
  size?: number;
  /** Optional label for screen readers. */
  label?: string;
}

export function Spinner({ size = 24, label = "Loading" }: SpinnerProps) {
  return (
    <div
      className={styles.spinnerWrap}
      role="status"
      aria-label={label}
    >
      <span
        className={styles.spinner}
        style={{ width: size, height: size, borderWidth: Math.max(2, size / 10) }}
      />
    </div>
  );
}
