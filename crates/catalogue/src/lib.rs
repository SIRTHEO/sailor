//! What Sailor says, in the language the reader asked for.
//!
//! **ENGLISH IS THE SOURCE, ITALIAN IS A LAYER ON TOP.** The other way round a
//! key born Italian would reach whoever publishes as a sentence they cannot
//! read, with nothing turning red. `desktop/src/i18n.ts` is the same contract
//! over the same files; the test that holds the two together lives on this side.

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The catalogue, embedded rather than read from disk.
///
/// A released binary is one file that may sit anywhere; a catalogue it has to
/// find on disk is a catalogue it will one day not find, and the failure would
/// be «every sentence is its own key», at the user, in production.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", include_str!("../../../i18n/en.json")),
    ("it", include_str!("../../../i18n/it.json")),
];

/// The language every key is written in, and the one a missing entry falls to.
pub const SOURCE_LANGUAGE: &str = "en";

/// The variable that decides. **Never the machine's locale**: what someone
/// publishes must not depend on the settings of the machine that rendered it.
pub const LANGUAGE_VARIABLE: &str = "SAILOR_LANG";

fn parsed() -> &'static BTreeMap<&'static str, BTreeMap<String, String>> {
    static PARSED: OnceLock<BTreeMap<&'static str, BTreeMap<String, String>>> = OnceLock::new();
    PARSED.get_or_init(|| {
        LANGUAGES
            .iter()
            .map(|(name, text)| {
                let entries = serde_json::from_str(text).unwrap_or_else(|error| {
                    panic!("i18n/{name}.json is embedded in this binary and is not an object of strings: {error}")
                });
                (*name, entries)
            })
            .collect()
    })
}

/// The language to speak, from what was asked.
///
/// Takes a tag apart at `-` or `_` so `it-IT` and `it_IT` both mean Italian, and
/// answers [`SOURCE_LANGUAGE`] for anything this catalogue does not carry —
/// including nothing at all. **An unknown language is not an error**: it is
/// someone whose language has not been written yet, and they get English.
pub fn pick_language(asked: Option<&str>) -> &'static str {
    let asked = asked.unwrap_or_default().trim().to_ascii_lowercase();
    let stem = asked.split(['-', '_']).next().unwrap_or_default();
    LANGUAGES
        .iter()
        .find(|(name, _)| *name == stem)
        .map(|(name, _)| *name)
        .unwrap_or(SOURCE_LANGUAGE)
}

/// The language this process speaks, read fresh from the environment.
///
/// Not cached: a cached choice makes two tests that set the variable depend on
/// which ran first, and reading a variable costs nothing next to formatting the
/// sentence it decides.
pub fn language() -> &'static str {
    pick_language(std::env::var(LANGUAGE_VARIABLE).ok().as_deref())
}

/// The entry for a key in a named language, or `None` when neither that language
/// nor the source declares it.
pub fn look(language: &str, key: &str, values: &[(&str, &str)]) -> Option<String> {
    let all = parsed();
    let text = all
        .get(language)
        .and_then(|entries| entries.get(key))
        .or_else(|| all.get(SOURCE_LANGUAGE).and_then(|entries| entries.get(key)))?;
    Some(fill(text, values))
}

/// What Sailor says for a key it declares itself.
///
/// Falls back to the bare key, which is ugly on purpose: an invented sentence
/// would read as if someone had written it.
pub fn say(key: &str, values: &[(&str, &str)]) -> String {
    try_say(key, values).unwrap_or_else(|| key.to_owned())
}

/// The same, for keys that may legitimately be absent — a failure class from a
/// newer engine, say. Showing the raw name is information; inventing a sentence
/// for it is not.
pub fn try_say(key: &str, values: &[(&str, &str)]) -> Option<String> {
    look(language(), key, values)
}

/// **NAMED SUBSTITUTIONS, NEVER POSITIONAL.** Word order moves between the two
/// languages, so `{0}` makes a correct translation impossible without editing
/// the sentence it came from. A name nobody supplied is left standing as
/// `{name}` rather than blanked: the hole stays visible, instead of reading as
/// a sentence with a word cut out of it.
fn fill(text: &str, values: &[(&str, &str)]) -> String {
    if values.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            rest = &rest[open..];
            break;
        };
        let name = &after[..close];
        match values.iter().find(|(supplied, _)| *supplied == name) {
            Some((_, value)) => out.push_str(value),
            None => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Every key this catalogue declares, in the source language.
pub fn every_key() -> impl Iterator<Item = &'static str> {
    parsed()[SOURCE_LANGUAGE].keys().map(String::as_str)
}

/// Every entry a language declares, for the tests that compare the two.
pub fn entries(language: &str) -> Option<&'static BTreeMap<String, String>> {
    parsed().get(language)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_italian_entry_falls_back_to_english_not_to_the_bare_key() {
        let only_in_english = every_key()
            .find(|key| !entries("it").expect("italian is carried").contains_key(*key));
        // Nothing to prove when the two are complete; the invariant is that the
        // fallback exists, and `an_unknown_key_is_not_invented` proves the other
        // half.
        if let Some(key) = only_in_english {
            let italian = look("it", key, &[]).expect("english answers for italian");
            assert_eq!(
                italian,
                look("en", key, &[]).expect("the source declares it"),
                "a key with no italian must come back in english, not as «{key}»"
            );
        }
    }

    #[test]
    fn an_unknown_key_is_not_invented() {
        assert_eq!(look("en", "run.failure.a_class_from_a_newer_engine", &[]), None);
        assert_eq!(try_say("nothing.declares.this", &[]), None);
        assert_eq!(say("nothing.declares.this", &[]), "nothing.declares.this");
    }

    #[test]
    fn the_language_falls_back_to_english_never_to_the_machines_locale() {
        assert_eq!(pick_language(Some("it")), "it");
        assert_eq!(pick_language(Some("it-IT")), "it");
        assert_eq!(pick_language(Some("IT_it")), "it");
        assert_eq!(pick_language(Some("  it  ")), "it");
        assert_eq!(pick_language(Some("fr")), SOURCE_LANGUAGE);
        assert_eq!(pick_language(Some("")), SOURCE_LANGUAGE);
        assert_eq!(pick_language(None), SOURCE_LANGUAGE);
    }

    #[test]
    fn substitutions_are_filled_by_name_and_an_unknown_name_is_left_alone() {
        assert_eq!(fill("the flow {name} ran", &[("name", "nightly")]), "the flow nightly ran");
        assert_eq!(
            fill("{second} before {first}", &[("first", "a"), ("second", "b")]),
            "b before a",
            "the order of the values must not decide the order of the words"
        );
        assert_eq!(fill("{absent} stands", &[("other", "x")]), "{absent} stands");
        assert_eq!(fill("an unclosed { brace", &[("a", "b")]), "an unclosed { brace");
        assert_eq!(fill("{name} twice {name}", &[("name", "x")]), "x twice x");
    }

    #[test]
    fn both_catalogues_parse_and_carry_the_same_keys() {
        let english = entries("en").expect("english is carried");
        let italian = entries("it").expect("italian is carried");
        let born_italian: Vec<_> = italian.keys().filter(|key| !english.contains_key(*key)).collect();
        assert!(
            born_italian.is_empty(),
            "english is the source: these keys exist only in italian and would reach \
             whoever publishes as a sentence they cannot read: {born_italian:?}"
        );
    }
}
