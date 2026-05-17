// Segmented control — a strip of mutually-exclusive toggle buttons.
// Used wherever a small enum decides view state (e.g. transcript
// "Live / Recent / All", or sort orders). Wave 3. ADR 0023.

import { useId, type ReactNode } from "react";
import styles from "./Segmented.module.css";

export interface SegmentedOption<T extends string> {
  value: T;
  label: ReactNode;
  /** Optional ARIA description; defaults to plain label. */
  ariaLabel?: string;
}

export interface SegmentedProps<T extends string> {
  value: T;
  onChange: (value: T) => void;
  options: SegmentedOption<T>[];
  /** Group ARIA label (for screen readers). */
  ariaLabel?: string;
  className?: string;
}

export function Segmented<T extends string>({
  value,
  onChange,
  options,
  ariaLabel,
  className,
}: SegmentedProps<T>) {
  const reactId = useId();
  const name = `mb-seg-${reactId.replace(/[:]/g, "")}`;
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      className={[styles.group, className].filter(Boolean).join(" ")}
    >
      {options.map((opt) => {
        const id = `${name}-${opt.value}`;
        const selected = opt.value === value;
        return (
          <label
            key={opt.value}
            htmlFor={id}
            className={[styles.segment, selected ? styles.segment_active : ""]
              .filter(Boolean)
              .join(" ")}
            aria-label={opt.ariaLabel}
          >
            <input
              id={id}
              type="radio"
              name={name}
              checked={selected}
              onChange={() => onChange(opt.value)}
              className={styles.input}
            />
            <span className={styles.label}>{opt.label}</span>
          </label>
        );
      })}
    </div>
  );
}
