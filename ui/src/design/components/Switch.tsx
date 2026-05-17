// Switch — on/off toggle, M3 style. Native checkbox underneath for
// keyboard + form-submission semantics; visually rendered as a track
// with a sliding thumb. Wave 3. ADR 0023.

import { useId, type InputHTMLAttributes } from "react";
import styles from "./Switch.module.css";

export interface SwitchProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "size"> {
  label?: string;
  /** Description text rendered under the label. */
  description?: string;
}

export function Switch({
  label,
  description,
  id: idProp,
  className,
  checked,
  ...rest
}: SwitchProps) {
  const reactId = useId();
  const id = idProp ?? `mb-switch-${reactId.replace(/[:]/g, "")}`;
  return (
    <label htmlFor={id} className={[styles.row, className].filter(Boolean).join(" ")}>
      {label || description ? (
        <span className={styles.text}>
          {label ? <span className={styles.label}>{label}</span> : null}
          {description ? (
            <span className={styles.description}>{description}</span>
          ) : null}
        </span>
      ) : null}
      <span className={styles.switchWrap}>
        <input
          id={id}
          type="checkbox"
          checked={checked}
          className={styles.input}
          {...rest}
        />
        <span className={styles.track} aria-hidden>
          <span className={styles.thumb} />
        </span>
      </span>
    </label>
  );
}
