// Placeholder — Wave D fleshes this out fully.
import { PageHeader, EmptyState } from "../components/primitives";
import { BookIcon } from "../design/Icon";
import { t } from "../i18n";

export function DictionaryPage() {
  return (
    <>
      <PageHeader title={t("dictionary.title")} subtitle={t("dictionary.subtitle")} />
      <EmptyState
        icon={<BookIcon size={32} />}
        title="Dictionary coming up next"
        subtitle="Wave D: CRUD + app-context filter + virtualized list."
      />
    </>
  );
}
