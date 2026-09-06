//! A chain of engines is worth having only if the fallback can fire.
//!
//! **FAULT 31.** A step writing `"tool": ["claude-code", "agy", "codex"]` looks
//! like it has two fallbacks. It has as many as there are engines that
//! **declare how they say they cannot work**: `says_it_cannot_work` over an
//! empty list is `false`, so a silent engine kills the step on its own failure
//! and the engines behind it never start. `agy` was silent and sat **in the
//! middle** of twelve chains in this tree, so `codex`, which declares its own
//! 401, started from none of them.
//!
//! **STILL UNMEASURED** is how `agy` says its **quota** is spent: searched in
//! the help, in the nested subcommands and in the binary's string table, and
//! never found. It is written in the descriptor, where a reader of one word can
//! tell which half of the field it covers. The mechanism itself is proved
//! hermetically in `crates/actions`; here the question is whether that defect
//! is **in our own house**, on the shipped flows and descriptors.

use std::collections::BTreeSet;

/// An engine named in a chain, with the place it holds.
struct InChain {
    flow: String,
    step: String,
    tool: String,
    last: bool,
}

/// Every engine a chain names, across all the flows of this tree.
///
/// **ORDER MATTERS AND IS READ FROM THE FILE.** A `BTreeSet` would say *which*
/// engines, never *which comes after which* — and that is exactly the question
/// here: the last of a chain has nobody to hand the work to, and demanding an
/// exhaustion declaration from it would demand a measure worth nothing.
fn engines_in_chains() -> Vec<InChain> {
    // Read from the flows compiled into the binary, not from a directory of the
    // repository. What the product hands out is what has to hold this rule;
    // a check that reads our own workshop is green here and false everywhere.
    let mut found = Vec::new();
    for (name, text) in flow::system::FLOWS {
        let Ok(file) = serde_json::from_str::<serde_json::Value>(text) else {
            continue;
        };
        let flow = (*name).to_owned();
        let Some(steps) = file["graph"]["steps"].as_array() else {
            continue;
        };
        for step in steps {
            // The run's own reader: a name written as a word is a chain of one.
            let names = actions::engines_named_in(&step["with"]);
            for (place, tool) in names.iter().enumerate() {
                found.push(InChain {
                    flow: flow.clone(),
                    step: step["id"].as_str().unwrap_or_default().to_owned(),
                    tool: tool.clone(),
                    last: place + 1 == names.len(),
                });
            }
        }
    }
    found
}

/// Why a shipped engine cannot be a fallback, when it cannot.
///
/// **THE RULE IS NOT WRITTEN HERE**, and that is the difference that counts: it
/// lives in `toolbox::Descriptor::cannot_be_a_fallback`, in one place, and from
/// there `sailor flow check` reads it too, over the flows and descriptors of
/// whoever runs it. A copy written inside this test would have watched the four
/// flows of this tree and none of anybody else's.
fn why_it_cannot_be_a_fallback(tool: &str) -> Option<String> {
    let catalog = toolbox::Catalog::load(&[toolbox::descriptor::Source::Builtin]);
    catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == tool)
        .and_then(|loaded| loaded.descriptor.cannot_be_a_fallback())
}

/// **THE RULE.** Every engine that appears in a chain and **is not the last**
/// must declare at least one word saying it cannot work. One that declares none
/// is not a fallback: it is a plug. No shipped engine is exempt; where each sits
/// in a chain is decided on cost, authentication and answers given, not on who
/// stays silent. Of `agy` only the missing credentials are measured, not the
/// spent quota — an `agy` out of quota still passes for an ordinary failure —
/// because the rule asks that the field not be empty, not that it be complete.
#[test]
fn every_engine_that_is_not_last_in_a_chain_says_how_it_is_exhausted() {
    let mut silent: BTreeSet<String> = BTreeSet::new();
    for engine in engines_in_chains() {
        if engine.last {
            continue;
        }
        if let Some(why) = why_it_cannot_be_a_fallback(&engine.tool) {
            silent.insert(format!(
                "{} · {} · {}: {why}",
                engine.flow, engine.step, engine.tool
            ));
        }
    }
    assert!(
        silent.is_empty(),
        "questi motori stanno in mezzo a una catena senza dichiarare come dicono \
         di non poter lavorare: quando si esauriscono uccidono il passo, e i \
         motori dopo di loro non partono. È il guasto 31.\n{}",
        silent.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// **A STEP THAT NAMES ONE ENGINE AS A WORD IS A CHAIN OF ONE**, read by the
/// reader the run uses. It stands in the list as the last of its chain, with
/// nothing required of it, instead of escaping the list — where a reader of
/// arrays alone left it, and no rule on chains could see it.
#[test]
fn a_step_naming_one_engine_as_a_word_is_a_chain_of_one() {
    let engines = engines_in_chains();
    let written_as_a_word: Vec<&InChain> = engines
        .iter()
        .filter(|engine| engine.flow == "dispatch-the-work" && engine.step == "engine_b")
        .collect();
    assert_eq!(
        written_as_a_word.len(),
        1,
        "the step that writes its engine as a word must be listed once, as a chain of one"
    );
    assert!(
        written_as_a_word[0].last,
        "a chain of one has no engine after it, so nothing is required of it"
    );
}

/// The rule is not kept by emptying the chains. If `cannot_be_a_fallback`
/// began answering "no" to everyone, the test above would stay green over
/// chains that no longer fall back. The canary used to perch on the flows of
/// this tree; those left the repository, so it asks the shipped descriptors.
#[test]
fn an_engine_that_declares_its_words_is_still_allowed_in_the_middle() {
    let catalog = toolbox::Catalog::load(&[toolbox::descriptor::Source::Builtin]);
    let allowed: BTreeSet<String> = catalog
        .live()
        .into_iter()
        .filter(|loaded| loaded.descriptor.cannot_be_a_fallback().is_none())
        .map(|loaded| loaded.descriptor.id.clone())
        .collect();
    assert!(
        !allowed.is_empty(),
        "nessuno strumento spedito può stare in mezzo a una catena: la regola \
         dice «no» a tutti, e la prova qui sopra è verde perché non ha niente \
         da guardare"
    );
}

/// **THE RULE ABOVE MUST NOT BE ABLE TO GO EMPTY IN SILENCE.** It says one
/// thing: the flows of this tree hold chains with an engine that is not the
/// last. The day none were left — flows change — the rule above would pass
/// having looked at nothing, and nobody would know: fault 22 applied to a check
/// instead of a total, a zero never computed passing for a measure. It watches
/// that chains exist, not that they watch every engine.
#[test]
fn there_are_chains_whose_fallback_can_actually_be_needed() {
    let engines = engines_in_chains();
    let not_last = engines.iter().filter(|engine| !engine.last).count();
    assert!(
        not_last > 0,
        "nessuna catena ha un motore prima dell'ultimo: la regola sul ripiego \
         non guarderebbe più niente, e resterebbe verde per vuoto"
    );
}

/// **THE WORDS IN THE DESCRIPTOR ARE THE ONES THE ENGINE SAID.** The measure,
/// for whoever wants to redo it: `HOME` pointed at an empty directory, and the
/// line Sailor really builds — `agy --mode plan --output-format json --print
/// "<question>"`. With no credentials it calls no provider, costs nothing, exits
/// **1**, and says the words below — twice out of two, identical. Running the
/// real `agy` from a test would go green or red on the house of whoever ran it,
/// so the measured text is copied here once and what is proved is the link
/// nobody watched: the **shipped descriptor's words match the real output**. It
/// covers the missing-credentials half of `unusable_when`, the half `codex`
/// declares its 401 with; the quota words stay unmeasured.
#[test]
fn what_agy_declares_matches_what_agy_really_said() {
    // The real output, taken as it arrived. The stdout is the JSON that
    // `--output-format json` produces when the house has no credentials.
    let stdout = r#"{"conversation_id":"","status":"ERROR","response":"","error":"authentication failed or timed out","duration_seconds":0,"num_turns":0,"usage":{"input_tokens":0,"output_tokens":0,"thinking_tokens":0,"cache_read_tokens":0,"total_tokens":0}}"#;
    let stderr = "Authentication required. Please visit the URL to log in:\n  \
                  https://accounts.google.com/o/oauth2/auth?access_type=offline\n\n\
                  Waiting for authentication (timeout 60s)...\n\
                  Or, paste the authorization code here and press Enter:\n\
                  Error: authentication timed out.";

    // The recipe is not rewritten here: it is asked of whoever really composes
    // it, or the test would watch a copy while the shipped descriptor said
    // something else.
    let tools = toolbox::Tools::new(
        toolbox::Catalog::load(&[toolbox::descriptor::Source::Builtin]),
        toolbox::Machine::bare(std::path::PathBuf::from(toolbox::probe::NOWHERE)),
    );
    let recipe = actions::ToolResolver::ask_recipe(&tools, "agy")
        .expect("«agy» è spedito e dichiara come lo si interroga");

    match actions::judge_dry_run(&recipe, stdout, stderr) {
        actions::ProbeVerdict::CannotWork { said } => {
            assert!(
                said.contains("authentication"),
                "il motivo deve portare le parole del motore: {said}"
            );
        }
        other => panic!(
            "«agy» senza credenziali deve risultare «non può lavorare adesso», \
             invece è {other:?}. Le parole scritte in `unusable_when` non \
             combaciano più con quello che agy dice davvero"
        ),
    }

    // The line it prints right before the sixty-second wait is the one the
    // step stops it on: it must be in the same measured output, or nothing
    // stops and every step of a chain pays the minute.
    assert!(
        !recipe.waits_for_a_person_when.is_empty(),
        "the shipped descriptor declares no words for the wait, so the wait is paid"
    );
    let said = stderr.to_lowercase();
    for word in &recipe.waits_for_a_person_when {
        assert!(
            said.contains(&word.to_lowercase()),
            "«{word}» is declared as the word before the wait and the engine never said it"
        );
    }
}

/// **AND AN ORDINARY ANSWER MUST NOT MATCH.** The danger of `unusable_when` is
/// not that it is empty but that it is **wide**: the comparison reads the whole
/// output, and an engine's output holds its answer, so a step asking `agy` to
/// *talk about* authentication would slide the chain to the next engine **on a
/// successful call** — paid twice, and credited to an engine that never gave it.
/// That is the defect the field declares it avoids: the provider's words are
/// declared, never a general rule. Replacing the three measured sentences with
/// `"authentication"` would otherwise stay green.
#[test]
fn a_real_answer_from_agy_is_not_mistaken_for_an_exhausted_engine() {
    let stdout = r#"{"conversation_id":"c-1","status":"OK","response":"Il difetto sta nel middleware di authentication: il token scade e nessuno lo rinnova.","duration_seconds":2,"num_turns":1,"usage":{"input_tokens":12,"output_tokens":3,"thinking_tokens":0,"cache_read_tokens":0,"total_tokens":15}}"#;
    let tools = toolbox::Tools::new(
        toolbox::Catalog::load(&[toolbox::descriptor::Source::Builtin]),
        toolbox::Machine::bare(std::path::PathBuf::from(toolbox::probe::NOWHERE)),
    );
    let recipe = actions::ToolResolver::ask_recipe(&tools, "agy")
        .expect("«agy» è spedito e dichiara come lo si interroga");

    assert!(
        !matches!(
            actions::judge_dry_run(&recipe, stdout, ""),
            actions::ProbeVerdict::CannotWork { .. }
        ),
        "una risposta buona di «agy» viene letta come un motore che non può \
         lavorare: le parole di `unusable_when` sono troppo larghe, e la catena \
         scivolerebbe al motore dopo su ogni chiamata riuscita"
    );
}
