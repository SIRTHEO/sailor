// The four places of the window, each a question a person actually asks.
//
// **FOUR, NOT ELEVEN.** Every capability of the engine used to ask for a door
// of its own, and a row of nouns is a list of what exists, not a navigation.
// A section opens on what its question wants; everything else lives inside.

export type Section = "board" | "terminals" | "memory" | "sailor";

export interface Place {
  id: Section;
  name: string;
  /** The question the section answers, under its name. */
  asks: string;
  group: "work" | "what happened" | "itself";
}

export const PLACES: Place[] = [
  { id: "board", name: "Board", asks: "what am I doing", group: "work" },
  { id: "terminals", name: "Terminals", asks: "what is running", group: "work" },
  { id: "memory", name: "Memory", asks: "what happened, and what it cost", group: "what happened" },
  { id: "sailor", name: "Sailor", asks: "what it knows, what it can do", group: "itself" },
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
            >
              <span className="places__row">
                <span className="places__name">{place.name}</span>
                {counts[place.id] !== undefined && <span className="places__count">{counts[place.id]}</span>}
              </span>
              <span className="places__asks">{place.asks}</span>
            </button>
          ))}
        </div>
      ))}
    </nav>
  );
}
