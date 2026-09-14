import { describe, expect, test } from "vitest";
import { PLACES } from "./places";

/**
 * **RULE 2 OF W30: THREE PLACES AT MOST.** A person scans and does not read a
 * list to find their place: `World.tsx` renders one button per row of
 * `PLACES`, and past three the row becomes a list. The fourth place moved to
 * a second level, reached from the run that explains it.
 */
describe("the primary navigation holds at most 3 places", () => {
  test("PLACES.length is 3 or fewer", () => {
    expect(PLACES.length).toBeLessThanOrEqual(3);
  });

  test("named, so a regression says which row broke the count", () => {
    expect(PLACES.map((place) => place.id)).toEqual(["waiting", "terminals", "sailor"]);
  });
});
