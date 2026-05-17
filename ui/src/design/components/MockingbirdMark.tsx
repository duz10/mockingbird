// The Mockingbird brand mark.
//
// Four ellipses-of-decreasing-size + a play triangle. Different
// proportions encode different states — the mark itself carries the
// app's runtime state, no separate badge required:
//
//   - "static"  → no animation, full colour. The default — use this
//                 anywhere the mark is decorative chrome (sidebar
//                 brand, About page hero, splash screens after the
//                 entrance animation completes).
//   - "idle"    → ellipses collapsed to ~circle proportions, triangle
//                 stays at full size. Reads as "I'm here, waiting."
//   - "active"  → ellipses oscillate at slightly different phases +
//                 amplitudes — the visual analogue of an equaliser
//                 reading live audio. Use while a dictation is in
//                 flight.
//   - "splash"  → entrance animation: ellipses scale-up in sequence
//                 with a slight overshoot, then the triangle pops.
//                 Use once on app start.
//   - "exit"    → reverse of splash: triangle shrinks first, then
//                 ellipses collapse outward-in. Use on shutdown
//                 splash / window close.
//
// The animation CSS lives in MockingbirdMark.module.css and matches
// the keyframes published in docs/design/design-language-v1.html §08
// 1:1 (mb-wave-1/2/3, mb-splash-ellipse, mb-splash-triangle,
// mb-exit-ellipse, mb-exit-triangle).
//
// Reduced-motion users see the static state regardless of the
// requested state — explicit override in the module CSS, not just
// duration-zeroing, because a frozen mid-animation frame looks
// broken.
//
// Wave 2 of the Design Language Phase. ADR 0023.

import { useId } from "react";
import styles from "./MockingbirdMark.module.css";

export type MarkState = "static" | "idle" | "active" | "splash" | "exit";

export interface MockingbirdMarkProps {
  /** Animation state. See module docstring. Defaults to "static". */
  state?: MarkState;
  /**
   * Render width in pixels. Height matches (the mark is square).
   * Defaults to 24 — sidebar-friendly. Use 96–160 for hero / splash.
   */
  size?: number;
  /**
   * Override the gradient colour stops. Defaults to the brand
   * gradient (terracotta 0% → deep terracotta 100%). Use a muted
   * pair (e.g. `["#8A7A6E", "#6B5A4E"]`) for the idle-tint variant
   * shown in the design doc's "idle" pill state.
   */
  gradient?: [string, string];
  /** Optional className passthrough (e.g. for sizing in flex rows). */
  className?: string;
  /** Optional accessible label. Defaults to "Mockingbird". */
  title?: string;
}

const DEFAULT_GRADIENT: [string, string] = ["#DDA17A", "#944730"];

export function MockingbirdMark({
  state = "static",
  size = 24,
  gradient = DEFAULT_GRADIENT,
  className,
  title = "Mockingbird",
}: MockingbirdMarkProps) {
  // useId gives every instance a unique gradient id so multiple marks
  // on the same page don't collide on a single <defs> definition.
  const gradId = `mb-grad-${useId().replace(/[:]/g, "")}`;

  const stateClass =
    state === "static" ? "" : styles[`state_${state}`] ?? "";

  return (
    <svg
      className={[styles.mark, stateClass, className].filter(Boolean).join(" ")}
      viewBox="0 0 256 256"
      width={size}
      height={size}
      role="img"
      aria-label={title}
    >
      <title>{title}</title>
      <defs>
        <linearGradient
          id={gradId}
          x1="0"
          y1="0"
          x2="256"
          y2="256"
          gradientUnits="userSpaceOnUse"
        >
          <stop offset="0%" stopColor={gradient[0]} />
          <stop offset="100%" stopColor={gradient[1]} />
        </linearGradient>
      </defs>
      <g fill={`url(#${gradId})`}>
        {/* 3 ellipses — the bird silhouette. ORDER MATTERS for the
            animations (nth-of-type selectors in module.css). */}
        <ellipse cx="42" cy="128" rx="18" ry="72" />
        <ellipse cx="104" cy="128" rx="26" ry="100" />
        <ellipse cx="163" cy="128" rx="15" ry="40" />
        {/* play triangle — the "dictation in flight" cue. */}
        <path d="M196 105 L196 151 Q196 157 201 153 L229 132 Q234 128 229 124 L201 103 Q196 99 196 105 Z" />
      </g>
    </svg>
  );
}
