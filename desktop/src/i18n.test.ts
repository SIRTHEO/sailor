import { describe, expect, test } from "vitest";
import { CATALOGUES, SOURCE_LANGUAGE, group, look, pickLanguage, t, tryT } from "./i18n";
import { STATE_COLOR } from "./StepNode";
// **THE FILES, NOT WHAT THIS BUILD CARRIES.** The window ships one language
// layer, the one it was built to speak; whether the two files agree is a
// question about the files, and it must be asked of every language there is.
import enOnDisk from "../../i18n/en.json";
import itOnDisk from "../../i18n/it.json";

const en = enOnDisk as Record<string, string>;
const it = itOnDisk as Record<string, string>;

describe("the two catalogues are one catalogue", () => {
  /**
   * **A KEY ONLY ITALIAN HAS IS A DEAD ENTRY NOBODY WILL EVER SEE.** A key
   * renamed in the source leaves its Italian twin behind, well-formed and never
   * read again: the catalogue diverges in silence, and neither file can say
   * which of the two is the wrong one.
   */
  test("EVERY ITALIAN KEY EXISTS IN ENGLISH, which is the source", () => {
    const orphans = Object.keys(it).filter((key) => !(key in en));
    expect(
      orphans,
      `${String(orphans.length)} keys live only in it.json: nobody will ever read them, ` +
        "because the window asks for the keys English declares",
    ).toEqual([]);
  });

  test("no entry is empty, in either language", () => {
    for (const [lang, catalogue] of Object.entries(CATALOGUES)) {
      const blank = Object.entries(catalogue)
        .filter(([, text]) => text.trim() === "")
        .map(([key]) => key);
      expect(blank, `voci vuote in ${lang}.json`).toEqual([]);
    }
  });

  /**
   * **NAMED SUBSTITUTIONS, NEVER POSITIONAL.** Word order moves between the two
   * languages: with `{0}` the correct Italian is impossible to write without
   * editing the English it came from.
   */
  test("SUBSTITUTIONS ARE NAMED, so word order can move between languages", () => {
    for (const [lang, catalogue] of Object.entries(CATALOGUES)) {
      const positional = Object.entries(catalogue)
        .filter(([, text]) => /\{\d+\}/.test(text))
        .map(([key]) => key);
      expect(positional, `sostituzioni posizionali in ${lang}.json`).toEqual([]);
    }
  });

  /**
   * The catalogue holds whole sentences, so this is the shape that keeps
   * genders and plurals out: an entry that is a fragment invites gluing two
   * halves together, and Italian then asks avviato or avviata.
   */
  test("an entry is a whole sentence, not a fragment to assemble", () => {
    const fragments = Object.entries(en)
      .filter(([, text]) => text.trim().endsWith(" is") || text.trim().endsWith(" was"))
      .map(([key]) => key);
    expect(fragments, "entries ending on a verb waiting for a piece").toEqual([]);
  });
});

describe("what the window reads", () => {
  /**
   * **A KEY THE LAYER HAS NOT GOT IS ANSWERED IN ENGLISH, NEVER BY ITS NAME.**
   * Asked of whichever layer this build carries, and of one it does not: both
   * must come back a sentence, because «run.failure.check_failed» on screen is
   * the shape of the hole this rule exists to close.
   */
  test("A MISSING ENTRY FALLS BACK TO ENGLISH, never to the bare key", () => {
    const key = "run.failure.check_failed";
    const orphaned = "window.step.state.went";
    const spoken = pickLanguage(undefined);
    const carried = CATALOGUES[spoken];

    // A language this build does not speak is answered in the one it does.
    expect(look("de", key)).toBe(carried[key]);
    expect(look("de", orphaned)).toBe(carried[orphaned]);

    // A key with no sentence at all comes back nothing, never its own name.
    expect(look(spoken, "window.nothing.declares.this")).toBeUndefined();

    // And every English key has an answer, whichever language shipped: the
    // fallback is put together at build time, so a gap in a layer is filled
    // before the window ever asks.
    const unanswered = Object.keys(en).filter((one) => carried[one] === undefined);
    expect(unanswered, `keys the built catalogue cannot answer: ${unanswered.slice(0, 5).join(", ")}`)
      .toEqual([]);
  });

  test("a key neither language declares comes back undefined, not invented", () => {
    expect(look("it", "window.nothing.declares.this")).toBeUndefined();
    expect(tryT("run.failure.a_class_from_a_newer_engine")).toBeUndefined();
  });

  test("substitutions are filled by name, and an unknown name is left alone", () => {
    // The catalogue this build carries, not the file on disk: the two are put
    // together when the window is built and `t` reads the answer.
    const spoken = CATALOGUES[pickLanguage(undefined)];
    spoken["test.only.greeting"] = "Add to «{name}», not to {other}";
    try {
      expect(t("test.only.greeting", { name: "staffetta" })).toBe(
        "Add to «staffetta», not to {other}",
      );
    } finally {
      delete spoken["test.only.greeting"];
    }
  });

  /**
   * **A BUILD DOES NOT CLAIM A LANGUAGE IT DOES NOT CARRY.** The layer is
   * chosen when the window is built, so asking for one that did not ship must
   * answer the source and not a name with no catalogue behind it.
   */
  test("THE LANGUAGE FALLS BACK TO ENGLISH, not to the machine's locale", () => {
    const carried = Object.keys(CATALOGUES).filter((one) => one !== SOURCE_LANGUAGE);

    expect(pickLanguage(undefined)).toBe(SOURCE_LANGUAGE);
    expect(pickLanguage("")).toBe(SOURCE_LANGUAGE);
    expect(pickLanguage("de")).toBe(SOURCE_LANGUAGE);
    for (const spoken of carried) {
      expect(pickLanguage(spoken)).toBe(spoken);
      expect(pickLanguage(`${spoken}-${spoken.toUpperCase()}`)).toBe(spoken);
    }
    // This build carries only the source, so a layer it did not ship is not a
    // language it speaks — the case that used to be asserted the other way.
    if (carried.length === 0) expect(pickLanguage("it")).toBe(SOURCE_LANGUAGE);
  });
});

/**
 * **CHECKED AGAINST A LIST IT DOES NOT OWN.** `STATE_COLOR` is the other place
 * that names every state, written by hand: a seventh state added there with no
 * word here would show as its own key on a node, in that state alone.
 */
test("EVERY STATE THAT HAS A COLOUR HAS A WORD", () => {
  const words = group("window.step.state.");
  expect(Object.keys(words).sort()).toEqual(Object.keys(STATE_COLOR).sort());
});
