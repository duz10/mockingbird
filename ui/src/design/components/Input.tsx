// Input — text + search-glass variants.
//
// `variant="text"` is the standard filled text field (label floats
// above value). `variant="search"` is the glass search bar from the
// design doc — for surfaces that sit over the ambient warm-blob bg
// where a filled field would look opaque and dead.
//
// Wave 3 of the Design Language Phase. ADR 0023.

import { useId, type InputHTMLAttributes, type ReactNode } from "react";
import styles from "./Input.module.css";

export type InputVariant = "text" | "search";

export interface InputProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, "size"> {
  label?: string;
  helperText?: string;
  error?: string;
  variant?: InputVariant;
  /** Optional leading icon — rendered inside the field's start. */
  leadingIcon?: ReactNode;
  /** Optional trailing icon / element — rendered inside the field's end. */
  trailingIcon?: ReactNode;
}

export function Input({
  label,
  helperText,
  error,
  variant = "text",
  leadingIcon,
  trailingIcon,
  className,
  id: idProp,
  ...rest
}: InputProps) {
  const reactId = useId();
  const id = idProp ?? `mb-input-${reactId.replace(/[:]/g, "")}`;
  const helperId = helperText || error ? `${id}-helper` : undefined;
  const classes = [
    styles.field,
    styles[`field_${variant}`],
    error ? styles.field_error : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={classes}>
      {label ? (
        <label htmlFor={id} className={styles.label}>
          {label}
        </label>
      ) : null}
      <div className={styles.wrap}>
        {leadingIcon ? <span className={styles.iconLead}>{leadingIcon}</span> : null}
        <input
          id={id}
          aria-describedby={helperId}
          aria-invalid={error ? true : undefined}
          className={styles.input}
          {...rest}
        />
        {trailingIcon ? <span className={styles.iconTrail}>{trailingIcon}</span> : null}
      </div>
      {error ? (
        <p id={helperId} className={styles.error} role="alert">
          {error}
        </p>
      ) : helperText ? (
        <p id={helperId} className={styles.helper}>
          {helperText}
        </p>
      ) : null}
    </div>
  );
}
