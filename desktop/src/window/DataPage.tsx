import { countWords, sizeWords, whenWords, type Store } from "../keeps";
import { t } from "../i18n";

/**
 * **A STORE THAT IS NOT THERE SAYS SO, AND STILL SAYS WHERE.** A plausible zero
 * for a store nobody has written to yet is not hiding a fact, it is stating a
 * false one; and the path is worth reading before the store exists, because it
 * is where it will be.
 */
export function DataPage({ store }: { store: Store }) {
  return (
    <dl className="window-facts">
      <dt>{t("window.data.where")}</dt>
      <dd className="window-facts__path">{store.where}</dd>
      {store.exists ? (
        <>
          <dt>{t("window.data.how_many")}</dt>
          <dd>
            {store.how_many === null
              ? <span className="window-facts__absent">{t("window.data.not_counted")}</span>
              : countWords(store.how_many)}
          </dd>
          <dt>{t("window.data.size")}</dt>
          <dd>{store.bytes === null ? <span className="window-facts__absent">{t("window.data.not_counted")}</span> : sizeWords(store.bytes)}</dd>
          <dt>{t("window.data.since")}</dt>
          <dd>
            {store.since === null
              ? <span className="window-facts__absent">{t("window.data.no_date")}</span>
              : whenWords(store.since)}
          </dd>
        </>
      ) : (
        // It answers for the whole store and not for a key of it, so it takes
        // the row instead of the 140px label column.
        <dd className="window-facts__state window-facts__absent">{t("window.data.nothing_written")}</dd>
      )}
    </dl>
  );
}
