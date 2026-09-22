import { describe, expect, test } from "vitest";
import { PLACES } from "./places";

/**
 * **RULE 2 OF W30: THREE PLACES AT MOST**, and it counts the row, not the
 * column of icons beside it: `World.tsx` draws one button per row of `PLACES`,
 * and past three the row becomes a list. The column carries five, held by
 * `window/lists.test.ts` — ADR-023.
 */
describe("the primary navigation holds at most 3 places", () => {
  test("PLACES.length is 3 or fewer", () => {
    expect(PLACES.length).toBeLessThanOrEqual(3);
  });

  test("named, so a regression says which row broke the count", () => {
    expect(PLACES.map((place) => place.id)).toEqual(["waiting", "terminals", "sailor"]);
  });
});
