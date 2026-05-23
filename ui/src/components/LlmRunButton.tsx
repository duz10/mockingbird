// Shared "Run LLM pass" button.
//
// Both the Dictations page and the Meetings page render a button that
// kicks off an LLM-pass run. They had the same shape — sparkle icon +
// "Run" / "Running…" label — but two different visual treatments:
// Dictations used a ghost button with the SparklesIcon prefix; Meetings
// used a primary (filled) button without the icon. mb-l8ey unifies them
// on the Dictations look (ghost glass + sparkle), per Dustin's nudge.
//
// Keeping this dead simple: it's a thin wrapper over <Button> that
// (a) always renders the sparkle, (b) pins variant=ghost, (c) handles
// the running/idle label swap. Pages still own the click handler +
// their own "Running…" / "Run" label strings (so dictation-specific
// vs meeting-specific copy doesn't leak across pages).
//
// DRY: this is the only place that knows the button looks like a
// glass-ghost button with a sparkle icon. If the visual ever changes,
// it changes here.

import type { ReactNode } from "react";

import { Button } from "./primitives";
import { SparklesIcon } from "../design/Icon";

interface LlmRunButtonProps {
  /** Click handler — typically calls the page's LLM-pass IPC. */
  onClick: () => void;
  /** Whether the pass is currently in flight; disables + swaps label. */
  running: boolean;
  /** Label rendered when idle. e.g. "Run" or "Run LLM pass". */
  idleLabel: string;
  /** Label rendered while `running` is true. e.g. "Running…". */
  runningLabel: string;
  /** Optional ARIA label override (defaults to whatever label is showing). */
  ariaLabel?: string;
  /** Extra children rendered after the label (rare; e.g. a kbd hint). */
  trailing?: ReactNode;
}

export function LlmRunButton({
  onClick,
  running,
  idleLabel,
  runningLabel,
  ariaLabel,
  trailing,
}: LlmRunButtonProps) {
  const label = running ? runningLabel : idleLabel;
  return (
    <Button
      variant="ghost"
      onClick={onClick}
      disabled={running}
      ariaLabel={ariaLabel ?? label}
    >
      <SparklesIcon size={14} />
      {label}
      {trailing}
    </Button>
  );
}
