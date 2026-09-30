import { sizeWords, type Store } from "../keeps";
import { t } from "../i18n";

/**
 * **THE PANEL LISTS THE STORES, THE FIELD OPENS ONE.** A row carries the size
 * because every store that exists has one; the count is a fact of some stores
 * only, and a column half filled with dashes is read as a fault of the window.
 */
export function DataRows({
  stores,
  chosen,
  onChoose,
}: {
  stores: Store[];
  chosen: string | null;
  onChoose: (what: string) => void;
}) {
  if (stores.length === 0) {
    return <p className="window-panel__empty">{t("window.data.none")}</p>;
  }
  return (
    <ul className="window-rows">
      {stores.map((store) => (
        <li key={store.what}>
          <button
            type="button"
            className="window-row"
            aria-current={store.what === chosen ? "true" : undefined}
            onClick={() => { onChoose(store.what); }}
          >
            <span className="window-row__name">{store.what}</span>
            {store.exists && store.bytes !== null ? (
              <span className="window-row__since">{sizeWords(store.bytes)}</span>
            ) : (
              <span className="window-row__gone">{t("window.data.not_created")}</span>
            )}
          </button>
        </li>
      ))}
    </ul>
  );
}
