// macOS-port (v1 honest-surface) — the shared "Coming soon on macOS"
// page state.
//
// The macOS v1 release ships exactly two proven features: Dictation
// and Meetings. Everything else that hasn't been developed + validated
// on macOS (Activity capture, the Knowledge Graph, Mobile Sync) is
// still Windows-only under the hood (the backend workers are
// `#[cfg(target_os = "windows")]`), so surfacing a half-working UI on
// a Mac would be dishonest — and, worse, could let a user trigger a
// path that captures nothing. Instead we render this friendly state.
//
// This is presentation-only and reversible: when a feature's macOS
// backend lands, drop the `isMac` gate at the call site and the real
// page returns. Windows never sees this component (its call sites are
// all gated on `isMac`, which is `false` on Windows).

import { EmptyState, PageHeader } from "./primitives";
import { t } from "../i18n";

interface ComingSoonProps {
  /** Page title (e.g. the feature name), shown in the header. */
  title: string;
  /** Feature-specific explanation body. */
  body: string;
  /** Optional leading icon for the empty-state. */
  icon?: React.ReactNode;
}

/** Full-page "Coming soon on macOS" placeholder for a not-yet-ported
 *  feature. Kept deliberately tiny so it stays a single source of
 *  truth across Activity + Knowledge Graph (DRY). */
export function ComingSoon({ title, body, icon }: ComingSoonProps) {
  return (
    <>
      <PageHeader title={title} />
      <EmptyState
        title={t("comingSoon.title")}
        subtitle={body}
        icon={icon}
      />
    </>
  );
}
