import { describe, expect, test } from "vitest";
import { CATALOGUES, SOURCE_LANGUAGE, group, look, pickLanguage, t, tryT } from "./i18n";
import { STATE_COLOR } from "./StepNode";

const en = CATALOGUES.en;
const it = CATALOGUES.it;

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
      `${String(orphans.length)} chiavi vivono solo in it.json: nessuno le leggerà mai, ` +
        "perché la finestra chiede le chiavi che l'inglese dichiara",
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
   * genders and plurals out: an entry that is a fragment invites assembling
   * «il flusso è» + «avviato», and Italian then asks avviato or avviata.
   */
  test("an entry is a whole sentence, not a fragment to assemble", () => {
    const fragments = Object.entries(en)
      .filter(([, text]) => text.trim().endsWith(" is") || text.trim().endsWith(" was"))
      .map(([key]) => key);
    expect(fragments, "voci che finiscono con un verbo in attesa di un pezzo").toEqual([]);
  });
});

describe("what the window reads", () => {
  test("A MISSING ITALIAN ENTRY FALLS BACK TO ENGLISH, never to the bare key", () => {
    const key = "run.failure.check_failed";
    expect(look("it", key)).toBe(it[key]);
    // The same key with no Italian: the answer is the English sentence, and it
    // is a sentence, not «run.failure.check_failed» on screen.
    const orphaned = "window.step.state.went";
    const saved = it[orphaned];
    delete it[orphaned];
    try {
      expect(look("it", orphaned)).toBe(en[orphaned]);
    } finally {
      it[orphaned] = saved;
    }
  });

  test("a key neither language declares comes back undefined, not invented", () => {
    expect(look("it", "window.nothing.declares.this")).toBeUndefined();
    expect(tryT("run.failure.a_class_from_a_newer_engine")).toBeUndefined();
  });

  test("substitutions are filled by name, and an unknown name is left alone", () => {
    en["test.only.greeting"] = "Add to «{name}», not to {other}";
    try {
      expect(t("test.only.greeting", { name: "staffetta" })).toBe(
        "Add to «staffetta», not to {other}",
      );
    } finally {
      delete en["test.only.greeting"];
    }
  });

  test("THE LANGUAGE FALLS BACK TO ENGLISH, not to the machine's locale", () => {
    expect(pickLanguage(undefined)).toBe(SOURCE_LANGUAGE);
    expect(pickLanguage("")).toBe(SOURCE_LANGUAGE);
    expect(pickLanguage("de")).toBe(SOURCE_LANGUAGE);
    expect(pickLanguage("it")).toBe("it");
    expect(pickLanguage("it-IT")).toBe("it");
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
