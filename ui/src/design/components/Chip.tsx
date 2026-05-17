// Chip — small pill-shaped label. Useful for: tags, mode badges,
// session markers, source-app stamps. Two visual roles:
//
//   - "neutral"  baseline tone, on-surface chrome
//   - "accent"   primary tone, for currently-selected / active chips
//
// Optional dismiss button (×) renders a close affordance and fires
// onDismiss. Optional leading icon.
//
// Wave 3 of the Design Language Phase. ADR 0023.

import type { ReactNode } from "react";
import styles from "./Chip.module.css";

export type ChipTone = "neutral" | "accent";

export interface ChipProps {
  children: ReactNode;
  tone?: ChipTone;
  leadingIcon?: ReactNode;
  onDismiss?: () => void;
  /** Optional click handler — turns the chip into a button. */
  onClick?: () => void;
  className?: string;
  /** Optional ARIA label override for the chip itself. */
  ariaLabel?: string;
}

export function Chip({
  children,
  tone = "neutral",
  leadingIcon,
  onDismiss,
  onClick,
  className,
  ariaLabel,
}: ChipProps) {
  const classes = [styles.chip, styles[`chip_${tone}`], className]
    .filter(Boolean)
    .join(" ");
  const interactive = !!onClick;
  return (
    <span
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onClick={onClick}
      onKeyDown={
        interactive
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onClick?.();
              }
            }
          : undefined
      }
      className={classes}
      aria-label={ariaLabel}
    >
      {leadingIcon ? <span className={styles.icon}>{leadingIcon}</span> : null}
      <span className={styles.label}>{children}</span>
      {onDismiss ? (
        <button
          type="button"
          className={styles.dismiss}
          onClick={(e) => {
            e.stopPropagation();
            onDismiss();
          }}
          aria-label="Remove"
        >
          ×
        </button>
      ) : null}
    </span>
  );
}
