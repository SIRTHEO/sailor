import { describe, expect, test } from "vitest";
import { PLACES } from "./places";

/**
 * **RULE 2 OF W30, LIT ON THE OWNER'S WORD OF 14/09 ("tre voci").** A person
 * scans, per the owner's own rule, and does not read a list to find their
 * place in it. `World.tsx` renders one button per row of `PLACES`: past
 * three, the row stops being a scan and becomes a list read like any other.
 *
 * This judge was written once already, in `w30-window-architecture.md`, and
 * stayed unlit because `places.ts` held 4 rows the day it was proposed — a
 * true judge that fires on the tree it guards is worse than none. It lights
 * now because the fourth ("memory"/"Why") moved to a second level, reached
 * from the run that explains it (`RunGlimpse`'s "Why" link), not scanned for
 * on the first screen.
 */
describe("the primary navigation holds at most 3 places", () => {
  test("PLACES.length is 3 or fewer", () => {
    expect(PLACES.length).toBeLessThanOrEqual(3);
  });

  test("named, so a regression says which row broke the count", () => {
    expect(PLACES.map((place) => place.id)).toEqual(["waiting", "terminals", "sailor"]);
  });
});
