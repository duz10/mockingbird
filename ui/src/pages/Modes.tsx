// Placeholder — Wave D fleshes this out fully.
import { PageHeader, EmptyState } from "../components/primitives";
import { SlidersIcon } from "../design/Icon";
import { t } from "../i18n";

export function ModesPage() {
  return (
    <>
      <PageHeader title={t("modes.title")} subtitle={t("modes.subtitle")} />
      <EmptyState
        icon={<SlidersIcon size={32} />}
        title="Modes editor coming up next"
        subtitle="Wave D: enable/disable, hotkey picker, per-mode provider/model/temperature."
      />
    </>
  );
}
