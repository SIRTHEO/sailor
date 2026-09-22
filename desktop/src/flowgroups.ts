import { t } from "./i18n";

/**
 * **THE ORIGINS, IN ONE PLACE.** Two surfaces head the same groups — the table
 * of what runs here, and the panel of the three columns — and a second copy of
 * this map is a second English word for one origin, wrong in one place only.
 */
const GROUP_WORDS: Record<string, string> = {
  workspace: "window.flows.group.workspace",
  declared: "window.flows.group.declared",
  yours: "window.flows.group.yours",
  "built in": "window.flows.group.built_in",
};

/** The heading over a group, or the origin itself when nobody wrote words for
 *  it: a source may declare an origin this window has never heard of. */
export function groupWords(key: string): string {
  return key in GROUP_WORDS ? t(GROUP_WORDS[key]) : key;
}
