// Phase 1D Wave 1D.2 (`mb-j00j`, ADR 0052) -- Knowledge Graph
// top-level route.
//
// This is the first-class "New UI surface" promised by
// PHASE-0-5-REPORT §7 / spec §15.3. Phase 1C narrowed the original
// dashboard intent to filter chips on the Dictations page; 1D.2
// corrects that by spinning up the dedicated /knowledge-graph
// destination. Future waves graduate from this scaffold:
//
//   * 1D.3 (`mb-rj9p`) -- adds audio + text capture surfaces.
//   * 1D.4 (`mb-vxnl`) -- relocates the concept modal here from
//     the Dictations page; wires recent-activity row clicks.
//   * 1D.5 (`mb-3lf8`) -- vault path + vocab management.
//
// **Route-level graph-off guard.** When `kgGraphEnabled = false`
// (the default until the user opts in via Settings -> KG), the page
// renders a friendly "Knowledge Graph is off" state with a CTA
// link to Settings. Critically, NO `kg_*` IPC fires from this
// guard path -- the Dashboard component (which makes the IPCs)
// isn't mounted at all. This honors the graph-off-UI invariant
// extended to /knowledge-graph (see
// `ui/tests/kg-graph-off-invariant.spec.ts`).
//
// The KgGraphEnabled lookup goes through the zustand app store,
// which is fed at boot (App.tsx) and updated reactively by
// SettingsKgTab on toggle flip. We DO render a brief loading
// state when `kgGraphEnabled` is still `null` (pre-boot) so the
// page doesn't flash the disabled-state copy before the real
// value arrives.

import { Link } from "react-router-dom";

import { EmptyState, PageHeader, Spinner } from "../../components/primitives";
import { t } from "../../i18n";
import { useAppStore } from "../../lib/store";

import { KnowledgeGraphDashboard } from "./Dashboard";
import styles from "./Dashboard.module.css";

export function KnowledgeGraphPage() {
  const kgGraphEnabled = useAppStore((s) => s.kgGraphEnabled);

  // Pre-boot: store hasn't resolved yet. Render a spinner rather
  // than the disabled-state so the page doesn't flash incorrect
  // copy. The boot fetch is parallel to the rest of the App boot
  // (see App.tsx), so this state is sub-second under normal load.
  if (kgGraphEnabled === null) {
    return (
      <div className={styles.page}>
        <PageHeader
          title={t("kg.dashboard.title")}
          subtitle={t("kg.dashboard.subtitle")}
        />
        <Spinner label={t("kg.dashboard.title")} />
      </div>
    );
  }

  if (!kgGraphEnabled) {
    return (
      <div className={styles.page}>
        <PageHeader
          title={t("kg.dashboard.title")}
          subtitle={t("kg.dashboard.subtitle")}
        />
        <EmptyState
          title={t("kg.dashboard.disabled.title")}
          subtitle={t("kg.dashboard.disabled.body")}
          action={
            <Link
              to="/settings"
              className={styles.disabledCta}
              aria-label={t("kg.dashboard.disabled.cta")}
            >
              {t("kg.dashboard.disabled.cta")}
            </Link>
          }
        />
      </div>
    );
  }

  return <KnowledgeGraphDashboard />;
}
