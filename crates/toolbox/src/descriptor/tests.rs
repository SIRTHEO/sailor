//! What happens to a descriptor when this version of Sailor learns a field
//! the previous one did not know.

use super::*;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("sailor-descrittori-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("working directory");
    dir
}

fn loaded(name: &str, text: &str) -> Catalog {
    let dir = scratch(name);
    let file = dir.join("descriptors.json");
    std::fs::write(&file, text).expect("write the descriptors");
    Catalog::load(&[Source::File(file)])
}

/// A descriptor written before `usage` existed loads identically, and a new
/// field must not make that worse: an engine that lacks it keeps working, or
/// a new Sailor would silently switch off the tools declared with the old.
#[test]
fn a_descriptor_written_before_usage_existed_still_loads() {
    let catalog = loaded(
        "without-usage",
        r#"[{
          "id": "vecchio", "family": "ai_cli", "label": "Vecchio",
          "detect": { "command": "vecchio" },
          "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["quota"] }
        }]"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    assert_eq!(catalog.descriptors.len(), 1);
    let descriptor = &catalog.descriptors[0].descriptor;
    assert!(descriptor.usage.is_none(), "absent means absent");
    assert!(descriptor.ask.is_some(), "and the rest arrives intact");
}

/// Every descriptor shipped with the product loads: if the new field made
/// even one of them unreadable, that tool would vanish from the machine of
/// anyone who updates.
#[test]
fn every_shipped_descriptor_still_loads() {
    let catalog = Catalog::load(&[Source::Builtin]);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    assert!(catalog.descriptors.len() > 5);
}

/// The `usage` block in the key-path form.
#[test]
fn a_json_usage_block_is_read_pointer_by_pointer() {
    let catalog = loaded(
        "usage-json",
        r#"[{
          "id": "nuovo", "family": "ai_cli",
          "detect": { "command": "nuovo" },
          "ask": { "args": ["-p"], "prompt": "stdin" },
          "usage": {
            "args": ["--output-format", "json"],
            "read": "json",
            "input_tokens": ["usage", "input_tokens"],
            "cached_tokens": ["usage", "cache_read_input_tokens"],
            "cost": ["total_cost_usd"],
            "model": ["model"],
            "answer": ["result"]
          }
        }]"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let usage = catalog.descriptors[0]
        .descriptor
        .usage
        .as_ref()
        .expect("the block is there");
    assert_eq!(usage.args, vec!["--output-format", "json"]);
    assert_eq!(usage.read, ReadAs::Json);
    assert_eq!(
        usage.input_tokens,
        Some(Where::Path(vec!["usage".into(), "input_tokens".into()]))
    );
    assert_eq!(
        usage.cached_tokens,
        Some(Where::Path(vec![
            "usage".into(),
            "cache_read_input_tokens".into()
        ])),
        "the cache has a pointer of its own, separate from input"
    );
    assert_eq!(usage.answer, Some(Where::Path(vec!["result".into()])));
    assert_eq!(
        usage.output_tokens, None,
        "what is not written is not there"
    );
}

/// The text form: the pointers are regular expressions.
#[test]
fn a_text_usage_block_reads_its_pointers_as_patterns() {
    let catalog = loaded(
        "usage-text",
        r#"[{
          "id": "nuovo", "family": "ai_cli",
          "detect": { "command": "nuovo" },
          "usage": { "read": "text", "total_tokens": "tokens used\\s*\\n\\s*([\\d.,]+)" }
        }]"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let usage = catalog.descriptors[0].descriptor.usage.as_ref().unwrap();
    assert_eq!(usage.read, ReadAs::Text);
    assert_eq!(
        usage.total_tokens,
        Some(Where::Pattern(
            "tokens used\\s*\\n\\s*([\\d.,]+)".to_owned()
        ))
    );
}

/// **AN INVENTED FIELD IS NAMED, BUT DOES NOT TAKE THE TOOL AWAY.**
///
/// The naming still matters — a silence that later leaves the usage unknown
/// without saying why is the thing to avoid. What changed is the price: the
/// engine used to be lost, now only the unreadable field is.
#[test]
fn an_invented_field_inside_usage_is_named_without_losing_the_tool() {
    let catalog = loaded(
        "usage-wrong",
        r#"[{
          "id": "nuovo", "family": "ai_cli",
          "detect": { "command": "nuovo" },
          "usage": { "read": "json", "token_di_ingresso": ["a"] }
        }]"#,
    );

    assert_eq!(catalog.descriptors.len(), 1, "the tool stays usable");
    assert!(
        catalog.problems.is_empty(),
        "and it is not a lost entry: {:?}",
        catalog.problems
    );
    assert_eq!(catalog.notes.len(), 1, "but it is not a silence either");
    assert!(
        catalog.notes[0].reason.contains("usage.token_di_ingresso"),
        "the note says which field and where it sits: {}",
        catalog.notes[0].reason
    );
}

/// **AN INVENTED FIELD AT THE TOP LEVEL, SAME RULE**: a descriptor written
/// for a newer Sailor, or copied from a more recent example. The name here
/// used to be `capabilities`, and this version now knows it, so it stopped
/// being unknown: an example that ages that way does not turn the test red
/// in the right place, it just turns it red. The unknown name must be one no
/// version will ever read — the flaw in every test using a plausible one.
#[test]
fn a_descriptor_from_a_newer_version_still_loads() {
    let catalog = loaded(
        "from-the-future",
        r#"[{
          "id": "nuovo", "family": "ai_cli",
          "detect": { "command": "nuovo" },
          "streams_partial_answers": true
        }]"#,
    );

    assert_eq!(catalog.descriptors.len(), 1);
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    assert_eq!(catalog.notes.len(), 1);
    assert!(
        catalog.notes[0].reason.contains("streams_partial_answers"),
        "{}",
        catalog.notes[0].reason
    );
}

/// **AND WHAT IS NOT UNDERSTOOD IS NOT LOST BY REWRITING IT.** A descriptor
/// read back and rewritten by this version keeps the fields from the future:
/// losing them would be the opposite defect, and just as silent.
#[test]
fn what_this_version_does_not_understand_survives_a_round_trip() {
    let catalog = loaded(
        "round-trip",
        r#"[{
          "id": "nuovo", "family": "ai_cli",
          "detect": { "command": "nuovo" },
          "streams_partial_answers": true
        }]"#,
    );

    let written = serde_json::to_value(&catalog.descriptors[0].descriptor)
        .expect("a descriptor in memory always rewrites");

    assert_eq!(
        written["streams_partial_answers"],
        serde_json::json!(true),
        "the unknown field comes back out as it was: {written}"
    );
}

/// **AN ENGINE THAT DECLARES NOTHING KEEPS WORKING.** A descriptor written
/// before `capabilities` existed loads identically and answers "nobody
/// looked" to every question: an absent capability is not an error.
#[test]
fn a_descriptor_written_before_capabilities_existed_still_loads() {
    let catalog = loaded(
        "without-capabilities",
        r#"[{
          "id": "vecchio", "family": "ai_cli", "label": "Vecchio",
          "detect": { "command": "vecchio" },
          "ask": { "args": ["-p"], "prompt": "stdin" }
        }]"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    assert!(catalog.notes.is_empty(), "{:?}", catalog.notes);
    let descriptor = &catalog.descriptors[0].descriptor;

    assert!(descriptor.capabilities.is_empty());
    assert_eq!(
        descriptor.capability("response_shape"),
        CapabilityState::NotLookedAt
    );
    assert!(descriptor.ask.is_some(), "and the rest arrives intact");
}

/// **THE THREE POSSIBLE ANSWERS ABOUT A CAPABILITY, IN ONE TEST.** If
/// "declared absent" and "never looked at" gave the same answer, the whole
/// block would be pointless: you could list only what is there, and every
/// silence would pass for a measurement.
#[test]
fn a_capability_can_be_present_declared_absent_or_never_looked_at() {
    let catalog = loaded(
        "three-states",
        r#"[{
          "id": "nuovo", "family": "ai_cli",
          "detect": { "command": "nuovo" },
          "capabilities": {
            "choose_model": { "args": ["--model"], "takes_value": true },
            "fork_session": false
          }
        }]"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let descriptor = &catalog.descriptors[0].descriptor;

    assert_eq!(
        descriptor.capability("choose_model"),
        CapabilityState::Available
    );
    assert_eq!(
        descriptor.capability("fork_session"),
        CapabilityState::Absent,
        "written `false` means somebody looked"
    );
    assert_eq!(
        descriptor.capability("resume_session"),
        CapabilityState::NotLookedAt,
        "unnamed does not mean absent"
    );
}

/// A capability with several ways is written as a list; one with a single
/// way without square brackets. Both forms live in the same block.
#[test]
fn one_way_needs_no_brackets_and_several_ways_are_a_list() {
    let catalog = loaded(
        "ways",
        r#"[{
          "id": "nuovo", "family": "ai_cli",
          "detect": { "command": "nuovo" },
          "capabilities": {
            "resume_session": [
              { "args": ["--resume"] },
              { "args": ["--session-id"], "takes_value": true }
            ],
            "fork_session": { "args": ["--fork-session"] }
          }
        }]"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let descriptor = &catalog.descriptors[0].descriptor;

    let resume = &descriptor.capabilities["resume_session"];
    assert_eq!(resume.forms().len(), 2);
    assert_eq!(resume.forms()[0].args, vec!["--resume"]);
    assert!(
        !resume.forms()[0].takes_value,
        "a flag does not want a value attached"
    );
    assert!(
        resume.forms()[1].takes_value,
        "and `--session-id` does: whoever composes the line reads it from the data"
    );

    let fork = &descriptor.capabilities["fork_session"];
    assert_eq!(fork.forms().len(), 1, "one way only, no square brackets");
}

/// **AN INVENTED FIELD INSIDE A FORM DOES NOT TAKE THE TOOL AWAY.** Same
/// rule on the newer block: it holds whole or it does not hold.
#[test]
fn an_invented_field_inside_a_capability_is_named_without_losing_the_tool() {
    let catalog = loaded(
        "capability-wrong",
        r#"[{
          "id": "nuovo", "family": "ai_cli",
          "detect": { "command": "nuovo" },
          "capabilities": { "resume_session": { "opzioni": ["--resume"] } }
        }]"#,
    );

    assert_eq!(catalog.descriptors.len(), 1, "the tool stays usable");
    assert!(
        catalog.problems.is_empty(),
        "and it is not a lost entry: {:?}",
        catalog.problems
    );
    assert_eq!(catalog.notes.len(), 1, "but it is not a silence either");
    assert!(
        catalog.notes[0]
            .reason
            .contains("capabilities.resume_session.opzioni"),
        "the note says which field, inside which capability: {}",
        catalog.notes[0].reason
    );
}

/// **THE SHIPPED ENGINES ANSWER ABOUT EVERY CAPABILITY IN THE VOCABULARY.**
///
/// Not that they have it: that somebody **looked**. An engine silent about a
/// capability is indistinguishable from one that does not have it, and the
/// block exists not to confuse them — so the first place the distinction
/// must be respected is the descriptors shipped with the product.
#[test]
fn every_shipped_engine_answers_about_every_capability() {
    let catalog = Catalog::load(&[Source::Builtin]);
    let vocabulary = [
        "ask_without_interaction",
        "response_shape",
        "resume_session",
        "fork_session",
        "isolate_from_user_config",
        "receive_equipment",
        "native_spend_cap",
        "choose_model",
        "fallback_model",
    ];
    for id in ["claude-code", "codex", "agy", "gemini-cli"] {
        let engine = catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == id)
            .unwrap_or_else(|| panic!("{id} is shipped with the product"));
        for name in vocabulary {
            assert_ne!(
                engine.descriptor.capability(name),
                CapabilityState::NotLookedAt,
                "{id} says nothing about «{name}»: «does not have it» and «nobody \
                 looked» are two different facts"
            );
        }
    }
}

/// And at least one declared absence really is there: without it, the test
/// above would pass with everything declared present.
#[test]
fn a_shipped_engine_declares_a_capability_it_does_not_have() {
    let catalog = Catalog::load(&[Source::Builtin]);
    let agy = catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == "agy")
        .expect("agy is shipped with the product");
    assert_eq!(
        agy.descriptor.capability("native_spend_cap"),
        CapabilityState::Absent,
        "measured with --help: agy has no spend cap of its own"
    );
}

/// **AN ENGINE THAT SAYS HOW IT IS ASKED ALSO SAYS HOW IT REFUSES.** Twin of
/// the test above, on `ask` instead of `capabilities`: a composed line nobody
/// ever ran is how options read from the documentation, each right on its
/// own, end up wrong together. **And it cannot be green by omission** — the
/// field fills only by composing the real line and watching the engine
/// answer. It was born red on all three engines that have an `ask` block.
#[test]
fn every_shipped_engine_that_asks_declares_how_it_refuses_without_a_prompt() {
    let catalog = Catalog::load(&[Source::Builtin]);
    let mut checked = 0;
    for loaded in catalog.live() {
        let Some(ask) = loaded.descriptor.ask.as_ref() else {
            continue;
        };
        checked += 1;
        assert!(
            !ask.refuses_without_prompt.is_empty() || ask.silent_without_prompt,
            "«{}» declares how a question is put to it but not how it refuses \
             the line composed without a question: its command line has never \
             been run, and no check would notice. Measure it by composing the \
             line and withholding the text — it costs nothing",
            loaded.descriptor.id
        );
        for mark in &ask.refuses_without_prompt {
            assert!(
                !mark.trim().is_empty(),
                "«{}» declares an empty fragment, which matches any output at \
                 all and would pass every broken line as sound",
                loaded.descriptor.id
            );
        }
    }
    // Without this line the test would be green on an emptied catalog, which
    // is the quietest way to stop checking.
    assert!(
        checked >= 3,
        "only {checked} shipped engines have an `ask` block: there were three, \
         so if there are fewer somebody removed one"
    );
}

/// **AN ENGINE THAT DECLARES HOW IT IS ASKED DECLARES BOTH ANSWERS**, since
/// half a declaration errs the comfortable way. **And the yes words must not
/// sit inside the no words**: "logged in" is contained in "not logged in".
/// The code reads the no first on purpose, but a descriptor standing only on
/// that order tells its reader a falsehood — and `sailor profiles list`
/// shows those words to a person.
#[test]
fn every_shipped_engine_that_asks_about_login_declares_both_answers() {
    let catalog = Catalog::load(&[Source::Builtin]);
    let mut checked = 0;
    for loaded in catalog.live() {
        let Some(login) = loaded.descriptor.login_status.as_ref() else {
            continue;
        };
        let id = &loaded.descriptor.id;
        checked += 1;
        assert!(
            !login.args.is_empty(),
            "«{id}» declares how the answer is recognised and not how the question \
             is asked: there is nothing to run"
        );
        for (which, marks) in [
            ("logged_in_when", &login.logged_in_when),
            ("logged_out_when", &login.logged_out_when),
        ] {
            assert!(
                marks.iter().any(|mark| !mark.trim().is_empty()),
                "«{id}» does not declare `{which}`: half a declaration \
                 distinguishes nothing, and the error would fall on the \
                 reassuring side"
            );
        }
        for yes in &login.logged_in_when {
            for no in &login.logged_out_when {
                assert!(
                    !no.to_lowercase().contains(&yes.to_lowercase()),
                    "«{id}»: the yes words («{yes}») sit inside the no words \
                     («{no}»), so an empty home looks like a full one. Declare \
                     longer, measured words"
                );
            }
        }
    }
    // Without this line the test would stay green on a catalog someone
    // removed the block from: the quietest way to stop.
    assert!(
        checked >= 2,
        "only {checked} shipped engines declare `login_status`: there were two \
         (claude-code and codex), so if there are fewer somebody removed one"
    );
}

/// **NO SHIPPED DESCRIPTOR CONTRADICTS ITSELF, WITH NO REGISTERED
/// EXCEPTIONS.** A list of exceptions is the shape a rule takes when written
/// before it can be respected. The rule lives in one place,
/// `Descriptor::contradictions`, and `sailor flow check` asks it about the
/// launcher's own descriptors too.
#[test]
fn no_shipped_descriptor_contradicts_itself() {
    let catalog = Catalog::load(&[Source::Builtin]);
    let found = catalog.contradictions();
    assert!(
        found.is_empty(),
        "descriptors saying two different things about the same fact: {}",
        found
            .iter()
            .map(Contradiction::line)
            .collect::<Vec<_>>()
            .join("; ")
    );
    // Without this line the test would be green on an emptied catalog, which
    // is the quietest way to stop checking.
    let engines = catalog
        .live()
        .into_iter()
        .filter(|loaded| loaded.descriptor.ask.is_some())
        .count();
    assert!(
        engines >= 4,
        "only {engines} shipped engines have an `ask` block: there were four, so \
         if there are fewer somebody removed one instead of repairing it"
    );
}

/// **AND THE GUARD CATCHES ALL FOUR SHAPES, ON DESCRIPTORS WRITTEN FOR THE
/// PURPOSE.** Needed because the test above, alone, would stay green even if
/// `contradictions` always returned the empty list: a check that checks
/// nothing looks exactly like a healthy world. Here the world is sick by
/// construction, and the guard has to say so.
#[test]
fn the_guard_names_every_way_two_blocks_can_disagree() {
    let catalog = loaded(
        "contradictory",
        r#"[
          {
            "id": "says-yes-and-has-no-line", "family": "ai_cli",
            "detect": { "command": "first" },
            "capabilities": { "ask_without_interaction": { "args": ["-p"] } }
          },
          {
            "id": "has-the-line-and-stays-silent", "family": "ai_cli",
            "detect": { "command": "second" },
            "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["quota"] }
          },
          {
            "id": "two-different-options", "family": "ai_cli",
            "detect": { "command": "third" },
            "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["quota"] },
            "capabilities": { "ask_without_interaction": { "args": ["--print"] } }
          },
          {
            "id": "an-empty-fragment", "family": "ai_cli",
            "detect": { "command": "fourth" },
            "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["   "] },
            "capabilities": { "ask_without_interaction": { "args": ["-p"] } }
          }
        ]"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);

    let said: BTreeMap<String, String> = catalog
        .contradictions()
        .into_iter()
        .map(|found| (found.tool, found.said))
        .collect();

    assert_eq!(
        said.len(),
        4,
        "one per descriptor, and there are four: {said:?}"
    );
    assert!(
        said["says-yes-and-has-no-line"].contains("no `ask` block"),
        "{said:?}"
    );
    assert!(
        said["has-the-line-and-stays-silent"].contains("does not declare that it can receive one"),
        "{said:?}"
    );
    assert!(
        said["two-different-options"].contains("--print"),
        "the option that does not match is named, or there is no knowing what to fix: {said:?}"
    );
    assert!(
        said["an-empty-fragment"].contains("empty fragment"),
        "{said:?}"
    );
}

/// **AN ENGINE THAT DOES NOT SAY HOW IT RUNS OUT CANNOT BE A FALLBACK, AND
/// ONE THAT DOES CAN.** Both halves sit in one test: with only the first, a
/// `cannot_be_a_fallback` always answering "no" would be green; with only
/// the second, one always answering "yes" would be. **The world here is
/// written on purpose** — the negative half used to be a shipped engine, and
/// died the day somebody measured it and did the right thing.
#[test]
fn only_an_engine_that_says_how_it_runs_out_can_be_a_fallback() {
    // Whether the *shipped* engines are in order is a different question,
    // asked on the real flows, where it has a consequence, by
    // `every_engine_that_is_not_last_in_a_chain_says_how_it_is_exhausted`.
    let catalog = loaded(
        "fallbacks",
        r#"[
          {
            "id": "says-how-it-runs-out", "family": "ai_cli",
            "detect": { "command": "first" },
            "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["weekly limit"] }
          },
          {
            "id": "stays-silent", "family": "ai_cli",
            "detect": { "command": "second" },
            "ask": { "args": ["-p"], "prompt": "stdin" }
          },
          {
            "id": "says-only-empty-fragments", "family": "ai_cli",
            "detect": { "command": "third" },
            "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["   "] }
          }
        ]"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let of = |id: &str| {
        catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == id)
            .unwrap_or_else(|| panic!("{id} is in the catalog written here"))
            .descriptor
            .cannot_be_a_fallback()
    };

    assert!(
        of("says-how-it-runs-out").is_none(),
        "an engine that declares its own words can sit in the middle: the work moves on"
    );

    let why = of("stays-silent").expect("an engine that declares nothing cannot be a fallback");
    assert!(why.contains("unusable_when"), "{why}");
    assert!(
        why.contains("never start"),
        "the reason says what is lost, not only what is missing: {why}"
    );

    // **A LIST OF EMPTY FRAGMENTS IS NOT A LIST.** `mentions_any` discards
    // them one by one, so `says_it_cannot_work` stays `false` and the engine
    // is a plug exactly like a silent one — while to whoever reads the
    // descriptor it looks as though somebody had looked.
    assert!(
        of("says-only-empty-fragments").is_some(),
        "an `unusable_when` of nothing but empty fragments behaves like an empty \
         list, and that must be said: otherwise the form of a declaration passes \
         for a declaration"
    );
}

/// **A WORD FOR A SPENT QUOTA IS ONE OF THE WORDS FOR «CANNOT WORK».** The
/// run reads `exhausted_when` first and the dry run only `unusable_when`:
/// a word in the first that no word of the second covers gives two
/// verdicts on one output. Covering is containment, so «insufficient_quota»
/// is covered by «quota»; and an empty fragment is refused there as in the
/// other two lists.
#[test]
fn a_word_for_a_spent_quota_that_no_word_for_cannot_work_covers_is_a_contradiction() {
    let catalog = loaded(
        "spent-quota-words",
        r#"[
          {
            "id": "coperto", "family": "ai_cli",
            "detect": { "command": "first" },
            "ask": { "args": ["-p"], "prompt": "stdin",
                     "unusable_when": ["quota", "401"],
                     "exhausted_when": ["insufficient_quota"] }
          },
          {
            "id": "scoperto", "family": "ai_cli",
            "detect": { "command": "second" },
            "ask": { "args": ["-p"], "prompt": "stdin",
                     "unusable_when": ["401"],
                     "exhausted_when": ["weekly limit"] }
          },
          {
            "id": "frammento-vuoto", "family": "ai_cli",
            "detect": { "command": "third" },
            "ask": { "args": ["-p"], "prompt": "stdin",
                     "unusable_when": ["401"],
                     "exhausted_when": ["   "] }
          }
        ]"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    // Only what is said about this list: the fixtures declare no
    // capabilities, and that contradiction is another test's.
    let of = |id: &str| -> Vec<String> {
        catalog
            .contradictions()
            .into_iter()
            .filter(|found| found.tool == id && found.said.contains("exhausted_when"))
            .map(|found| found.said)
            .collect()
    };

    assert!(
        of("coperto").is_empty(),
        "a word contained in a «cannot work» word is covered: {:?}",
        of("coperto")
    );

    let uncovered = of("scoperto");
    assert_eq!(uncovered.len(), 1, "{uncovered:?}");
    assert!(
        uncovered[0].contains("weekly limit") && uncovered[0].contains("exhausted_when"),
        "the contradiction names the word and the field: {}",
        uncovered[0]
    );

    let empty = of("frammento-vuoto");
    assert!(
        empty.iter().any(|said| said.contains("empty fragment") && said.contains("exhausted_when")),
        "an empty fragment in `exhausted_when` matches everything and must be named: {empty:?}"
    );
}

/// An empty fragment among the words for waiting on a person would stop
/// every engine on its first byte: it is named like the other lists'.
#[test]
fn an_empty_fragment_among_the_words_for_waiting_on_a_person_is_named() {
    let catalog = loaded(
        "waiting-words",
        r#"[
          {
            "id": "waits-on-nothing", "family": "ai_cli",
            "detect": { "command": "first" },
            "ask": { "args": ["-p"], "prompt": "stdin",
                     "unusable_when": ["401"],
                     "waits_for_a_person_when": ["Waiting for a code", " "] }
          }
        ]"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let named = catalog
        .contradictions()
        .into_iter()
        .any(|found| found.said.contains("empty fragment") && found.said.contains("waits_for_a_person_when"));
    assert!(named, "the empty fragment must be named with its field");
}

/// The shipped `codex` descriptor declares how its usage is read, in the
/// text form: the only format actually measured for it.
#[test]
fn the_shipped_codex_descriptor_declares_how_to_read_its_tokens() {
    let catalog = Catalog::load(&[Source::Builtin]);
    let codex = catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == "codex")
        .expect("codex is shipped with the product");
    let usage = codex
        .descriptor
        .usage
        .as_ref()
        .expect("codex declares its own usage");
    assert_eq!(usage.read, ReadAs::Text);
    assert!(usage.total_tokens.is_some());
    assert!(
        usage.args.is_empty(),
        "codex already writes its tokens on its own: asking it for anything \
         more would change its command line for nothing"
    );
    assert!(
        usage.answer.is_none(),
        "no envelope asked for, so nothing to unwrap: the step's output stays \
         what it always was"
    );
}
