// **THE MISSING PART TAKES A PLACE IN THE NUMBER, NOT A NOTE BESIDE IT.** A
// figure drawn large with a caveat under it is read as the figure, so the mark
// stands where the unmeasured part would have stood, at the same size. Nothing
// proportional draws it: a ring asserts a whole, and the whole is unknown.

export type Figure =
  | { shape: "unknown" }
  | { shape: "floor"; micros: number }
  | { shape: "whole"; micros: number };

/** With nothing priced there is no floor: `at least $0.00` reads as an outlay. */
export function figureOf(micros: number, missing: number): Figure {
  if (missing === 0) return { shape: "whole", micros };
  return micros === 0 ? { shape: "unknown" } : { shape: "floor", micros };
}

export const MISSING = "?";

/** After a label that already names the cost: `?`, `at least $1.00 + ?`. */
export function figureUnits(figure: Figure, money: (micros: number) => string): string {
  switch (figure.shape) {
    case "unknown":
      return MISSING;
    case "floor":
      return `at least ${money(figure.micros)} + ${MISSING}`;
    case "whole":
      return money(figure.micros);
  }
}

/** Standing on its own, where no label precedes it. */
export function figureWords(figure: Figure, money: (micros: number) => string): string {
  return figure.shape === "unknown" ? `cost ${MISSING}` : figureUnits(figure, money);
}
