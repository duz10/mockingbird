// Placeholder — Wave E fleshes this out fully.
import { PageHeader, EmptyState } from "../components/primitives";
import { SparklesIcon } from "../design/Icon";
import { t } from "../i18n";

export function InsightsPage() {
  return (
    <>
      <PageHeader title={t("insights.title")} />
      <EmptyState
        icon={<SparklesIcon size={32} />}
        title="Insights coming up next"
        subtitle="Wave E in progress. Today / mode mix / sparkline / learning loop status."
      />
    </>
  );
}
