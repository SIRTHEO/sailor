// The places of the window, each a question a person actually asks.
//
// **ONE NAVIGATION, IN A COLUMN, DIVIDED BY WHAT YOU ARE DOING** and not by
// how the program is built. A row of nouns is a list of what exists.
//
// **A GLYPH, A NAME AND A COUNT — NOT A SENTENCE EACH.** The column carried
// what every place answers under its name, on two lines: five sentences
// stacked, all the same weight, which is a wall of text and not a
// navigation. What a place answers belongs to the place, once you are in it.

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
}: {
  here: Section;
  onGo: (section: Section) => void;
  counts: Partial<Record<Section, number>>;
}) {
  return (
    <nav className="places" aria-label="places">
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
