// The graphic mark of a tool.
//
// **THE MARK IS DATA, NOT A BRANCH OF CODE.** Below there is a map from
// identifier to drawing, and the component knows how to draw *a shape*, never a
// shape in particular. No `if (tool === "claude")` exists: adding a mark is
// adding an entry to the map, and not adding it takes nothing from anyone.
//
// **THE FALLBACK IS THE NORMAL CASE.** Whoever installs Sailor has tools on
// disk I have never seen: the map below covers a handful of identifiers, and
// all the rest — the majority, on any real machine — takes a monogram on a tint
// computed from the identifier itself. It must therefore be dignified, not an
// empty box: it is what will be seen almost always.
//
// **THE DRAWINGS ARE INLINE AND MADE HERE.** No network request: the shell
// makes none, and a canvas waiting for a logo from a CDN is a canvas that stays
// blank on someone else's laptop. They are geometric shapes that tell apart at
// a glance, not reproductions of the brands: they make a row in a list
// recognisable, and for that they are enough.

import type { CSSProperties } from "react";

export interface ToolMarkShape {
  /** The strokes of the drawing, on a 24×24 grid. */
  paths: Array<{ d: string; fill?: boolean }>;
  /** The colour of the mark. */
  tint: string;
}

/**
 * The marks I know, by tool identifier as the engine declares it (`id` in
 * `discover_tools`). One more entry here requires touching nothing else.
 */
const MARKS: Record<string, ToolMarkShape> = {
  "claude-code": {
    tint: "#d97757",
    // An asterisk of rays leaving the centre.
    paths: [
      { d: "M12 3v7M12 14v7M4.2 7.5l6.1 3.5M13.7 13l6.1 3.5M4.2 16.5l6.1-3.5M13.7 11l6.1-3.5" },
    ],
  },
  "codex": {
    tint: "#10a37f",
    // A broken ring with a knot at the centre: a cycle passing through a point.
    paths: [
      { d: "M19 12a7 7 0 1 1-3.5-6.1" },
      { d: "M12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5z", fill: true },
    ],
  },
  "gemini-cli": {
    tint: "#4285f4",
    // A four-pointed star, all in one stroke.
    paths: [{ d: "M12 2c0 5.5 4.5 10 10 10-5.5 0-10 4.5-10 10 0-5.5-4.5-10-10-10 5.5 0 10-4.5 10-10z", fill: true }],
  },
  "ollama": {
    tint: "#7c3aed",
    // Two arcs like ears above a round body.
    paths: [
      { d: "M7.5 9c-.8-1.6-.9-3.4-.4-5 1.4.7 2.5 2 3 3.6M16.5 9c.8-1.6.9-3.4.4-5-1.4.7-2.5 2-3 3.6" },
      { d: "M12 21c-3.6 0-6-2.4-6-6s2.4-7 6-7 6 3.4 6 7-2.4 6-6 6z" },
    ],
  },
  git: {
    tint: "#f05033",
    // Three nodes and the branches that join them.
    paths: [
      { d: "M6 4v10M6 14a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM17 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM17 9c0 4-5 3-11 5" },
    ],
  },
  gh: {
    tint: "#8b949e",
    // A circle with a tail: the outline everyone knows, cut to the bone.
    paths: [
      { d: "M12 2.5a9.5 9.5 0 0 0-3 18.5v-3.2c-2.4.5-3-1.2-3-1.2-.4-1-1-1.3-1-1.3-.9-.6.1-.6.1-.6 1 .1 1.5 1 1.5 1 .9 1.5 2.3 1.1 2.9.8.1-.6.3-1.1.6-1.3-2-.2-4-1-4-4.4 0-1 .3-1.8.9-2.4-.1-.2-.4-1.1.1-2.3 0 0 .7-.2 2.4.9a8.3 8.3 0 0 1 4.4 0c1.7-1.1 2.4-.9 2.4-.9.5 1.2.2 2.1.1 2.3.6.6.9 1.4.9 2.4 0 3.4-2 4.2-4 4.4.3.3.6.9.6 1.8V21A9.5 9.5 0 0 0 12 2.5z", fill: true },
    ],
  },
  docker: {
    tint: "#2496ed",
    // Containers stacked above the waterline.
    paths: [
      { d: "M4 12h4v4H4zM9 12h4v4H9zM14 12h4v4h-4zM9 7h4v4H9z", fill: true },
      { d: "M2 17c3 2 7 2.5 11 1.5 3-.7 5.4-2.4 6.5-4.5" },
    ],
  },
  node: {
    tint: "#5fa04e",
    // The hexagon, with nothing else inside.
    paths: [{ d: "M12 2.5l8.2 4.75v9.5L12 21.5l-8.2-4.75v-9.5z" }],
  },
  npm: {
    tint: "#cb3837",
    // The solid block with the notch.
    paths: [
      { d: "M2 6h20v12h-10v-9h-4v9H2z", fill: true },
    ],
  },
  cargo: {
    tint: "#c96a3f",
    // A cogwheel: a ring and its teeth.
    paths: [
      { d: "M12 8.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z" },
      { d: "M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2.1 2.1M16.9 16.9L19 19M19 5l-2.1 2.1M7.1 16.9L5 19" },
    ],
  },
  kubectl: {
    tint: "#326ce5",
    // The helm: a circle and its spokes.
    paths: [
      { d: "M12 3l7.8 4.5v9L12 21l-7.8-4.5v-9z" },
      { d: "M12 9.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5z", fill: true },
      { d: "M12 3v6.5M19.8 7.5l-5.6 3.3M19.8 16.5l-5.6-3.3M12 21v-6.5M4.2 16.5l5.6-3.3M4.2 7.5l5.6 3.3" },
    ],
  },
  curl: {
    tint: "#0b7285",
    // A wave going in and a wave going out.
    paths: [{ d: "M2 9c3-3 5 3 8 0s5 3 8 0M2 16c3-3 5 3 8 0s5 3 8 0" }],
  },
  python: {
    tint: "#3776ab",
    // Two bodies that interlock.
    paths: [
      { d: "M12 2.5c-3 0-4.5 1.2-4.5 3.2V9h4.5v1H6.2C4 10 3 11.6 3 14.5S4 19 6.2 19H8v-3.2C8 13.6 9.4 12 11.5 12h4" },
      { d: "M12 21.5c3 0 4.5-1.2 4.5-3.2V15H12v-1h5.8c2.2 0 3.2-1.6 3.2-4.5S20 5 17.8 5H16v3.2c0 2.2-1.4 3.8-3.5 3.8h-4" },
    ],
  },
};

/**
 * The fallback tint, computed from the identifier.
 *
 * It must be *stable* and *distinct*: the same tool always has the same colour
 * on every machine and at every start — otherwise the colour helps recognise
 * nothing — and two tools next to each other in the list tend to fall far apart
 * on the wheel, so that what differentiates is the high digits of the hash.
 */
function fallbackTint(id: string): string {
  let hash = 0;
  for (let index = 0; index < id.length; index += 1) {
    hash = (hash * 31 + id.charCodeAt(index)) >>> 0;
  }
  const hue = hash % 360;
  // Saturation and lightness stay in a narrow band: the mark must read on light
  // and on dark without anyone choosing it by hand.
  return `hsl(${hue} 52% 48%)`;
}

/**
 * The monogram: the first letter of each piece of the identifier, at most two.
 * `docker-compose` gives «DC», `socraticode` gives «S». It is nobody's official
 * abbreviation — it is a handhold for the eye, and the whole name is written
 * beside it anyway.
 */
export function monogram(id: string): string {
  const parts = id.split(/[-_. ]+/).filter((part) => part !== "");
  if (parts.length === 0) return "?";
  const letters = parts.slice(0, 2).map((part) => part[0]!.toUpperCase());
  return letters.join("");
}

/** True if I know a drawing for this identifier. */
export function hasMark(id: string): boolean {
  return id in MARKS;
}

export interface ToolMarkProps {
  /** The tool identifier as the engine declares it. */
  id: string;
  size?: number;
  /**
   * A tool that is not there shows as off — visible and grey — not hidden: a
   * reader looking at a node that cannot run must see that from the canvas.
   */
  off?: boolean;
  title?: string;
}

/**
 * A tool's mark: its drawing if I know it, the monogram if I do not.
 *
 * It knows no tool by name — it asks the map, and the map can be empty without
 * this component changing by a line.
 */
export function ToolMark({ id, size = 18, off = false, title }: ToolMarkProps) {
  const shape = MARKS[id];
  const tint = off ? "#94a3b8" : (shape?.tint ?? fallbackTint(id));

  return (
    <span
      className="tool-mark"
      data-off={off || undefined}
      // The tint travels as a custom property too: the monogram uses it for its
      // own background, and cannot read it from `currentColor` because on
      // itself it declares the white of the text.
      style={{ width: size, height: size, color: tint, "--mark-tint": tint } as CSSProperties}
      title={title ?? id}
      aria-hidden={title === undefined || undefined}
    >
      {shape ? (
        <svg viewBox="0 0 24 24" width={size} height={size} role="presentation">
          {shape.paths.map((path, index) => (
            <path
              key={index}
              d={path.d}
              fill={path.fill ? "currentColor" : "none"}
              stroke={path.fill ? "none" : "currentColor"}
              strokeWidth={1.8}
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          ))}
        </svg>
      ) : (
        // The fallback does not imitate a logo I do not have: it says the
        // initials on a pill, and reads as a fallback instead of looking like a
        // wrong official mark.
        <span className="tool-mark__monogram" style={{ fontSize: Math.round(size * 0.44) }}>
          {monogram(id)}
        </span>
      )}
    </span>
  );
}
