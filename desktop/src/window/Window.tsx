import type { ReactNode } from "react";
import { Rail } from "./Rail";
import type { ListName } from "./lists";

/**
 * **THE THREE COLUMNS ARE THE WHOLE WINDOW**: the column picks which list, the
 * panel picks which thing, the field holds what is open. Every screen of the
 * product is one of the three filled differently, which is why none of them
 * carries a navigation of its own.
 */
export function Window({
  place,
  onPlace,
  panel,
  field,
  footing,
}: {
  place: ListName;
  onPlace: (place: ListName) => void;
  panel: ReactNode;
  field: ReactNode;
  /** The strip along the bottom: where you are, and what it is costing. */
  footing?: ReactNode;
}) {
  return (
    <div className="window-shell">
      <div className="window-shell__columns">
        <Rail place={place} onPlace={onPlace} />
        {panel}
        {field}
      </div>
      {footing === undefined ? null : <div className="window-shell__footing">{footing}</div>}
    </div>
  );
}
