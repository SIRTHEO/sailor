import { describe, expect, test } from "vitest";
import { placeMenu } from "./menuplace";

const SIZE = { width: 200, height: 260 };
const WINDOW = { width: 1440, height: 900 };

describe("where a menu opened at a point goes", () => {
  test("below the point when it fits", () => {
    expect(placeMenu({ x: 400, y: 100 }, SIZE, WINDOW)).toEqual({ left: 400, top: 100 });
  });

  /**
   * THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY, and it did: a wire let go
   * low on the canvas opened the menu below the window, and its items could not
   * be clicked at all. Found in a browser, not argued.
   */
  test("above the point when there is no room below", () => {
    const { top } = placeMenu({ x: 400, y: 800 }, SIZE, WINDOW);
    expect(top).toBe(800 - SIZE.height);
    expect(top + SIZE.height).toBeLessThanOrEqual(WINDOW.height);
  });

  test("it slides in from the right edge rather than hanging off it", () => {
    const { left } = placeMenu({ x: 1400, y: 100 }, SIZE, WINDOW);
    expect(left + SIZE.width).toBeLessThanOrEqual(WINDOW.width);
  });

  /* Taller than the window: it is still read from its first item down, so it
     pins to the top rather than centring and losing both ends. */
  test("a menu taller than the window starts at the top", () => {
    const { top } = placeMenu({ x: 400, y: 800 }, { width: 200, height: 1200 }, WINDOW);
    expect(top).toBe(8);
  });

  test("it never touches the frame", () => {
    for (const point of [{ x: 0, y: 0 }, { x: 1440, y: 900 }, { x: 1440, y: 0 }, { x: 0, y: 900 }]) {
      const { left, top } = placeMenu(point, SIZE, WINDOW);
      expect(left).toBeGreaterThanOrEqual(0);
      expect(top).toBeGreaterThanOrEqual(0);
    }
  });
});
