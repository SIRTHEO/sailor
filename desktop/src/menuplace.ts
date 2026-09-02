/**
 * Where a menu opened at a point actually goes. Placed straight at the pointer
 * it walks off the bottom of the window and its items cannot be clicked at all
 * — found dropping a wire low on the canvas. It flips, or slides in.
 */
export interface Placed {
  left: number;
  top: number;
}

/** Kept off the very edge: a menu touching the frame reads as clipped. */
const MARGIN = 8;

export function placeMenu(
  at: { x: number; y: number },
  size: { width: number; height: number },
  viewport: { width: number; height: number },
): Placed {
  // Below the point if it fits, above it if it does not, and pinned to the top
  // margin when it fits neither — a menu taller than the window is still read
  // from its first item down.
  const below = at.y + size.height + MARGIN <= viewport.height;
  const above = at.y - size.height - MARGIN >= 0;
  const top = below ? at.y : above ? at.y - size.height : Math.max(MARGIN, viewport.height - size.height - MARGIN);

  const right = at.x + size.width + MARGIN <= viewport.width;
  const left = right ? at.x : Math.max(MARGIN, viewport.width - size.width - MARGIN);

  return { left, top };
}
