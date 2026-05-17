// Placeholder — Wave C fleshes this out fully.
import { PageHeader, EmptyState } from "../components/primitives";
import { HistoryIcon } from "../design/Icon";
import { t } from "../i18n";

export function HistoryPage() {
  return (
    <>
      <PageHeader title={t("history.title")} />
      <EmptyState
        icon={<HistoryIcon size={32} />}
        title="History coming up next"
        subtitle="Wave C: virtualized session list + 3-pane detail + FTS5 search."
      />
    </>
  );
}
