// Mockingbird design-language v1 — Button family.
//
// Seven variants, all sharing the same React component. The variant
// name decides the visual weight (filled is the loudest, text is the
// quietest, glass is for floating chrome over the ambient blob).
// Use the variant matrix from docs/design/design-language-v1.html §10
// when choosing — TL;DR:
//
//   - filled    primary action on a surface ("Save")
//   - tonal     secondary primary action on the same surface
//   - outlined  tertiary action, used sparingly
//   - text      ambient action ("Cancel", inline nav)
//   - glass     floating-chrome action over the ambient bg
//   - icon      square icon-only button (toolbar, list-item end)
//   - fab       extended floating-action — "New session" / "Record"
//
// Sizes: sm | md | lg. "md" is the default; "lg" only for hero CTAs
// on splash / empty-state.
//
// Accessibility: native <button>, focus-visible outline via the v2
// outline token, disabled state lowers opacity and disables pointer.
//
// Wave 3 of the Design Language Phase. ADR 0023.

import type { ButtonHTMLAttributes, ReactNode } from "react";
import styles from "./Button.module.css";

export type ButtonVariant =
  | "filled"
  | "tonal"
  | "outlined"
  | "text"
  | "glass"
  | "icon"
  | "fab";

export type ButtonSize = "sm" | "md" | "lg";

export interface ButtonProps
  extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  children: ReactNode;
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Optional leading icon — rendered before children. */
  leadingIcon?: ReactNode;
  /** Optional trailing icon — rendered after children. */
  trailingIcon?: ReactNode;
}

export function Button({
  children,
  variant = "filled",
  size = "md",
  leadingIcon,
  trailingIcon,
  className,
  type = "button",
  ...rest
}: ButtonProps) {
  const classes = [
    styles.btn,
    styles[`btn_${variant}`],
    styles[`btn_${size}`],
    className,
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <button type={type} className={classes} {...rest}>
      {leadingIcon ? <span className={styles.icon}>{leadingIcon}</span> : null}
      <span className={styles.label}>{children}</span>
      {trailingIcon ? <span className={styles.icon}>{trailingIcon}</span> : null}
    </button>
  );
}
