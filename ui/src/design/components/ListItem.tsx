// ListItem — a row with optional avatar/leading slot, title, meta
// text, and optional trailing slot. The transcript-history surface
// is built from these. Wave 3 of the Design Language Phase.
// ADR 0023.

import type { ReactNode } from "react";
import styles from "./ListItem.module.css";

export interface ListItemProps {
  /** Optional leading element — avatar, mark, icon, image. */
  leading?: ReactNode;
  title: ReactNode;
  /** Optional second line under the title (size/duration/meta). */
  meta?: ReactNode;
  /** Optional trailing element — chip, time, action icon. */
  trailing?: ReactNode;
  onClick?: () => void;
  /** Visual selected state (e.g. for selected-row in History). */
  selected?: boolean;
  className?: string;
  /** ARIA label override; defaults to the title's text. */
  ariaLabel?: string;
}

export function ListItem({
  leading,
  title,
  meta,
  trailing,
  onClick,
  selected,
  className,
  ariaLabel,
}: ListItemProps) {
  const interactive = !!onClick;
  const classes = [
    styles.row,
    interactive ? styles.row_interactive : "",
    selected ? styles.row_selected : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <div
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
      aria-pressed={interactive && selected ? true : undefined}
    >
      {leading ? <span className={styles.leading}>{leading}</span> : null}
      <span className={styles.text}>
        <span className={styles.title}>{title}</span>
        {meta ? <span className={styles.meta}>{meta}</span> : null}
      </span>
      {trailing ? <span className={styles.trailing}>{trailing}</span> : null}
    </div>
  );
}
