// How a sum that does not contain everything is written.
//
// **THE MISSING PART TAKES A PLACE IN THE NUMBER, NOT A NOTE BESIDE IT.** A
// figure drawn large with a small caveat under it is read as the figure: the
// reader keeps `$12.40` and drops the line that says it is a floor. So the
// question mark stands where the unmeasured part would have stood, on the same
// line and at the same size, and the whole expression is the number.
//
// **AND NOTHING PROPORTIONAL EVER DRAWS IT.** A ring, a bar, a filled box all
// assert a whole the reader can see the fraction of, and here the whole is
// precisely what nobody knows.

/** A cost, and whether what it is made of covers what it claims. */
export type Figure =
  | { shape: "unknown" }
  | { shape: "floor"; micros: number }
  | { shape: "whole"; micros: number };

/**
 * `missing` counts the calls that declared no price. With nothing priced there
 * is no floor to stand on: `at least $0.00` is true, reads as a small outlay,
 * and is the lie this shape exists to stop.
 */
export function figureOf(micros: number, missing: number): Figure {
  if (missing === 0) return { shape: "whole", micros };
  return micros === 0 ? { shape: "unknown" } : { shape: "floor", micros };
}

/** The mark standing in for what was never measured. */
export const MISSING = "?";

/** The figure after a label that already names the cost: `?`, `at least $1.00 + ?`. */
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

/** The figure standing on its own, where no label precedes it. */
export function figureWords(figure: Figure, money: (micros: number) => string): string {
  return figure.shape === "unknown" ? `cost ${MISSING}` : figureUnits(figure, money);
}
