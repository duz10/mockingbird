// Placeholder — Wave F fleshes this out fully.
import { PageHeader, EmptyState } from "../components/primitives";
import { SettingsIcon } from "../design/Icon";
import { t } from "../i18n";

export function SettingsPage() {
  return (
    <>
      <PageHeader title={t("settings.title")} />
      <EmptyState
        icon={<SettingsIcon size={32} />}
        title="Settings coming up next"
        subtitle="Waves F/G: General · Models · History · Advanced + Export/Import."
      />
    </>
  );
}
