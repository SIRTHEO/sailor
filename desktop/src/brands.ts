/**
 * **NO MARK IS NAMED HERE**: a descriptor declares a slug, and the drawing and
 * the official colour come from a catalogue somebody else maintains. The two
 * rules below are why that catalogue never has to be complete — a slug it has
 * not heard of still draws, and a colour too near the ground is lifted.
 */
import MARKS, { type BrandMark } from "virtual:brand-marks";

export type { BrandMark };
export { MARKS };

/** The panels the marks sit on. A mark is not text, so the floor is WCAG's 3:1
 *  for a graphical object and not prohibition 6's 4.5:1 for words. */
export const GROUND = "#211e26";
const FLOOR = 3;

/** The letter a monogram carries. It is text, so prohibition 6 applies to it. */
const LETTER = "#19171d";
const LETTER_FLOOR = 4.5;

interface Rgb {
  red: number;
  green: number;
  blue: number;
}

export function rgbOf(hex: string): Rgb {
  const six = hex.replace("#", "");
  return {
    red: parseInt(six.slice(0, 2), 16),
    green: parseInt(six.slice(2, 4), 16),
    blue: parseInt(six.slice(4, 6), 16),
  };
}

function hexOf({ red, green, blue }: Rgb): string {
  const two = (value: number) => Math.round(Math.min(255, Math.max(0, value))).toString(16).padStart(2, "0");
  return `#${two(red)}${two(green)}${two(blue)}`;
}

function luminance(colour: Rgb): number {
  const channel = (raw: number) => {
    const part = raw / 255;
    return part <= 0.04045 ? part / 12.92 : ((part + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(colour.red) + 0.7152 * channel(colour.green) + 0.0722 * channel(colour.blue);
}

export function contrast(one: string, other: string): number {
  const [bright, dark] = [luminance(rgbOf(one)), luminance(rgbOf(other))].sort((a, b) => b - a);
  return (bright + 0.05) / (dark + 0.05);
}

function hslOf({ red, green, blue }: Rgb): [number, number, number] {
  const [r, g, b] = [red / 255, green / 255, blue / 255];
  const high = Math.max(r, g, b);
  const low = Math.min(r, g, b);
  const light = (high + low) / 2;
  if (high === low) return [0, 0, light];
  const span = high - low;
  const saturation = light > 0.5 ? span / (2 - high - low) : span / (high + low);
  const hue =
    high === r ? ((g - b) / span + (g < b ? 6 : 0)) : high === g ? (b - r) / span + 2 : (r - g) / span + 4;
  return [hue / 6, saturation, light];
}

function rgbOfHsl(hue: number, saturation: number, light: number): Rgb {
  if (saturation === 0) {
    const flat = light * 255;
    return { red: flat, green: flat, blue: flat };
  }
  const q = light < 0.5 ? light * (1 + saturation) : light + saturation - light * saturation;
  const p = 2 * light - q;
  const channel = (shift: number) => {
    let t = hue + shift;
    if (t < 0) t += 1;
    if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  return { red: channel(1 / 3) * 255, green: channel(0) * 255, blue: channel(-1 / 3) * 255 };
}

/**
 * A brand's colour, moved as far as it takes to be seen on `ground` and no
 * further. **ONLY LIGHTNESS MOVES**, so Ollama's black becomes a grey and never
 * a blue, and Claude's `#D97757` — already 6.19:1 — comes back untouched.
 */
export function legible(colour: string, ground: string = GROUND, floor: number = FLOOR): string {
  if (contrast(colour, ground) >= floor) return colour;
  const [hue, saturation, light] = hslOf(rgbOf(colour));
  const up = luminance(rgbOf(ground)) < 0.5;
  for (let step = 1; step <= 100; step += 1) {
    const moved = hexOf(rgbOfHsl(hue, saturation, Math.min(1, Math.max(0, light + (up ? step : -step) / 100))));
    if (contrast(moved, ground) >= floor) return moved;
  }
  return up ? "#ffffff" : "#000000";
}

/**
 * The colour of a brand nobody has a mark for, taken from the slug itself: the
 * same engine is the same colour everywhere, and nothing here is a list.
 */
export function hueOfSlug(slug: string): string {
  let hash = 0;
  for (const letter of slug) hash = (hash * 31 + letter.charCodeAt(0)) % 360;
  // Lifted against the LETTER: one hue in seven came back at 4.45:1 when it
  // was lifted only for the wall, and a square legible enough to carry its
  // letter clears the ground's 3:1 anyway.
  return legible(hexOf(rgbOfHsl(hash / 360, 0.42, 0.55)), LETTER, LETTER_FLOOR);
}

export function monogramOf(label: string): string {
  const letter = [...label].find((one) => /\p{L}|\p{N}/u.test(one));
  return (letter ?? "?").toUpperCase();
}
