import { LISTS } from "./lists";
import type { ListName } from "./lists";
import { t } from "../i18n";

/**
 * The icons carry no state. A mark that means «something is waiting» belongs to
 * the row it happened on; on the column it would tint an icon that is also the
 * only way to reach four other lists.
 */
export function Rail({
  place,
  onPlace,
}: {
  place: ListName;
  onPlace: (place: ListName) => void;
}) {
  return (
    <nav className="window-rail" aria-label={t("window.rail.label")}>
      {LISTS.map((entry) => {
        const Icon = entry.icon;
        return (
          <button
            key={entry.id}
            type="button"
            className="window-rail__button"
            // The name twice: to a reader, and to a pointer resting on it.
            aria-label={entry.name}
            title={entry.name}
            aria-current={entry.id === place ? "page" : undefined}
            onClick={() => { onPlace(entry.id); }}
          >
            <Icon size={21} strokeWidth={1.6} aria-hidden="true" />
          </button>
        );
      })}
    </nav>
  );
}
