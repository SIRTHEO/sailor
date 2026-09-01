import en from "../../i18n/en.json";
import it from "../../i18n/it.json";

/**
 * **ENGLISH IS THE SOURCE, ITALIAN IS A LAYER ON TOP.** The other way round the
 * default rots first: a key born Italian would reach whoever publishes as a
 * sentence they cannot read, with nothing turning red. This way a key with no
 * Italian falls back to English, which is behaviour and not a hole.
 */
export const CATALOGUES: Record<string, Record<string, string>> = { en, it };

export const SOURCE_LANGUAGE = "en";

/**
 * The language the window speaks. Comes from `SAILOR_LANG`, never from the
 * machine's locale: what someone publishes must not depend on the settings of
 * the machine that renders it.
 */
export function pickLanguage(asked: string | undefined | null): string {
  const wanted = (asked ?? "").trim().toLowerCase().split(/[-_]/)[0];
  return wanted in CATALOGUES ? wanted : SOURCE_LANGUAGE;
}

/**
 * **NAMED SUBSTITUTIONS, NEVER POSITIONAL.** Word order moves between the two
 * languages, so `{0}` makes a correct translation impossible without editing
 * the source it came from.
 */
function fill(text: string, vars: Record<string, string | number> | undefined): string {
  if (!vars) return text;
  return text.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in vars ? String(vars[name]) : whole,
  );
}

/** The catalogue entry, or `undefined` when neither language has the key. */
export function look(
  lang: string,
  key: string,
  vars?: Record<string, string | number>,
): string | undefined {
  const text = CATALOGUES[lang]?.[key] ?? CATALOGUES[SOURCE_LANGUAGE][key];
  return text === undefined ? undefined : fill(text, vars);
}

const LANGUAGE = pickLanguage(
  (import.meta as { env?: Record<string, string | undefined> }).env?.SAILOR_LANG,
);

/** What the window says for a key it declares itself. */
export function t(key: string, vars?: Record<string, string | number>): string {
  return look(LANGUAGE, key, vars) ?? key;
}

/**
 * The same, for keys that arrive from the engine and may legitimately be absent
 * — a failure class this window has never heard of. Showing the raw name is
 * information; inventing a sentence for it is not.
 */
export function tryT(key: string, vars?: Record<string, string | number>): string | undefined {
  return look(LANGUAGE, key, vars);
}

/** Every entry under a prefix, keyed by the last segment. */
export function group(prefix: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const key of Object.keys(CATALOGUES[SOURCE_LANGUAGE])) {
    if (key.startsWith(prefix)) out[key.slice(prefix.length)] = t(key);
  }
  return out;
}
