// ChipInput — small reusable primitive for typing a list of short
// strings as removable pills. Used by the Dictionary form to collect
// "common misspellings" without the awkwardness of comma-separated
// text.
//
// Behavior:
//   - Type a word and press Enter or comma to commit it as a chip.
//   - Backspace on an empty input removes the last chip.
//   - Click the [x] on a chip (provided by the design-system Chip
//     primitive) to remove that chip.
//   - Trims on commit. De-dupes case-insensitively (so "Hooli" and
//     "hooli" collide; first one wins).
//
// Styling leans on the existing design-system <Chip> so we don't grow
// yet another pill flavor. The wrapper box mimics the .inlineInput
// look in Dictionary.module.css so the form feels consistent with the
// other text fields on the page.

import { useRef, type KeyboardEvent } from "react";

import { Chip } from "../design/components/Chip";

import styles from "./ChipInput.module.css";

interface ChipInputProps {
  value: string[];
  onChange: (next: string[]) => void;
  placeholder?: string;
  ariaLabel?: string;
  id?: string;
}

export function ChipInput({
  value,
  onChange,
  placeholder,
  ariaLabel,
  id,
}: ChipInputProps) {
  const inputRef = useRef<HTMLInputElement | null>(null);

  function commitCurrent() {
    const el = inputRef.current;
    if (!el) return;
    const raw = el.value.trim();
    if (!raw) return;
    // Case-insensitive dedupe; keep the first occurrence's casing.
    const lower = raw.toLowerCase();
    if (value.some((v) => v.toLowerCase() === lower)) {
      el.value = "";
      return;
    }
    onChange([...value, raw]);
    el.value = "";
  }

  function handleKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      commitCurrent();
      return;
    }
    if (e.key === "Backspace") {
      const el = inputRef.current;
      if (el && el.value.length === 0 && value.length > 0) {
        e.preventDefault();
        onChange(value.slice(0, -1));
      }
    }
  }

  function removeAt(idx: number) {
    onChange(value.filter((_, i) => i !== idx));
  }

  return (
    <div
      className={styles.box}
      onClick={() => inputRef.current?.focus()}
      role="group"
      aria-label={ariaLabel}
    >
      {value.map((chip, idx) => (
        <Chip
          key={`${chip}-${idx}`}
          tone="neutral"
          onDismiss={() => removeAt(idx)}
        >
          {chip}
        </Chip>
      ))}
      <input
        ref={inputRef}
        id={id}
        type="text"
        className={styles.input}
        placeholder={value.length === 0 ? placeholder : ""}
        onKeyDown={handleKeyDown}
        // Commit any pending text on blur so a user who types
        // "Hooli" and clicks Save doesn't silently lose it.
        onBlur={commitCurrent}
        aria-label={ariaLabel}
      />
    </div>
  );
}
