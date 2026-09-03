// The places of the window, each a question a person actually asks.
//
// One navigation, in a column, divided by what you are doing. A glyph, a name
// and a count — not a sentence each: five sentences stacked, all the same
// weight, are a wall of text. What a place answers belongs to the place.

import { WorkspaceSwitcher } from "./Workspace";

export type Section = "board" | "terminals" | "ledger" | "memory" | "sailor";

export interface Place {
  id: Section;
  name: string;
  /** The mark beside the name. It carries no state: it makes a row findable. */
  glyph: string;
  /** The question the section answers, shown where the section opens. */
  asks: string;
  group: "work" | "what happened" | "itself";
}

export const PLACES: Place[] = [
  { id: "board", name: "Board", glyph: "◈", asks: "what am I doing", group: "work" },
  { id: "terminals", name: "Terminals", glyph: "▮", asks: "what is running", group: "work" },
  {
    id: "ledger",
    name: "Ledger",
    glyph: "▤",
    asks: "the tables, as they are",
    group: "what happened",
  },
  {
    id: "memory",
    name: "Runs",
    glyph: "◷",
    asks: "what happened, and what it cost",
    group: "what happened",
  },
  { id: "sailor", name: "Sailor", glyph: "⚓", asks: "what it knows, what it can do", group: "itself" },
];

const GROUPS: Place["group"][] = ["work", "what happened", "itself"];

export function Rail({
  here,
  onGo,
  counts,
  native,
  onMoved,
}: {
  here: Section;
  onGo: (section: Section) => void;
  counts: Partial<Record<Section, number>>;
  native: boolean;
  /** Called once the window has moved into another workspace. */
  onMoved: () => void;
}) {
  return (
    <nav className="places" aria-label="places">
      {/* The workspace is the spine, not a tab three places in. */}
      <WorkspaceSwitcher native={native} onMoved={onMoved} />
      {GROUPS.map((group) => (
        <div className="places__group" key={group}>
          <div className="places__heading">{group}</div>
          {PLACES.filter((place) => place.group === group).map((place) => (
            <button
              type="button"
              key={place.id}
              className="places__item"
              data-here={here === place.id || undefined}
              onClick={() => onGo(place.id)}
              title={place.asks}
            >
              <span className="places__glyph" aria-hidden="true">
                {place.glyph}
              </span>
              <span className="places__name">{place.name}</span>
              {counts[place.id] !== undefined && (
                <span className="places__count">{counts[place.id]}</span>
              )}
            </button>
          ))}
        </div>
      ))}
    </nav>
  );
}
