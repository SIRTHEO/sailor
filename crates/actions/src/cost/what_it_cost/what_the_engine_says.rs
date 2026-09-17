//! What an engine says, read the way the ledger and the chain need it: the
//! model a step asked for, the answer inside an envelope, and the words of a
//! refusal that belong to the engine rather than to the prompt it echoed.

use super::*;

struct NamesAModel(Declares);

impl ToolResolver for NamesAModel {
    fn resolve(&self, id: &str) -> Result<String, String> {
        self.0.resolve(id)
    }
    fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
        self.0.ask_recipe(id)
    }
    fn model_option(&self, _id: &str) -> Option<Vec<String>> {
        Some(vec!["--model".to_owned()])
    }
}

/// An engine that names no model in its answer is priced at the model the
/// step asked it for, and the row says which one that was.
#[test]
fn a_call_whose_engine_names_no_model_is_priced_at_the_model_asked_for() {
    let dir = scratch("asked-for");
    let price_list = dir.join("pricing.json");
    std::fs::write(
        &price_list,
        r#"{"currency": "USD", "models": [
            {"id": "the-model-asked", "input_per_million": 4.0, "output_per_million": 0.0}
        ]}"#,
    )
    .expect("write the price list");
    let bin = fake_engine(
        &dir,
        "motore",
        r#"cat > /dev/null
printf '{"result":"ok","usage":{"input_tokens":1000000,"output_tokens":0}}'"#,
    );
    let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
    let recipe = AskRecipe {
        usage: declaring_recipe().usage.map(|usage| UsageRecipe {
            declared: Declared { model: None, ..usage.declared },
            ..usage
        }),
        ..declaring_recipe()
    };
    let action = ExternalEngineAction::resolving_with(NamesAModel(Declares {
        bin,
        recipe: Some(recipe),
    }))
    .recording_to(Some(ledger));
    let input = json!({
        "tool": "motore-di-prova",
        "model": {"motore-di-prova": "the-model-asked"},
        "stdin": "ciao",
        "timeout_secs": 10
    });

    with_price_list(Some(&price_list), || {
        action.execute(&input, &shared("corsa-asked", "passo"))
    })
    .expect("the engine answers");

    let calls = calls_in(&dir.join("deposito"));
    assert_eq!(calls[0].actual_model, "");
    assert_eq!(calls[0].requested_model, "the-model-asked");
    assert_eq!(calls[0].cost_micros, Some(4_000_000));
}
/// agy's envelope carries the answer as a JSON text inside `response`; the
/// step's shape is judged on that answer, not on the envelope. Fault 194.
#[test]
fn an_answer_inside_the_envelope_is_the_one_the_shape_judges() {
    let dir = scratch("envelope-answer");
    let bin = fake_engine(
        &dir,
        "motore-busta",
        r#"cat > /dev/null
printf '%s' '{"conversation_id":"f48ce772","status":"SUCCESS","response":"{\"printed\": \"     662 LICENSE\\n\"}\n","num_turns":1,"usage":{"input_tokens":23070,"output_tokens":8616,"cache_read_tokens":12210,"total_tokens":31686}}'"#,
    );
    let recipe = AskRecipe {
        usage: Some(UsageRecipe {
            args: vec!["--output-format".to_owned(), "json".to_owned()],
            declared: Declared {
                input_tokens: path(&["usage", "input_tokens"]),
                output_tokens: path(&["usage", "output_tokens"]),
                cached_tokens: path(&["usage", "cache_read_tokens"]),
                total_tokens: path(&["usage", "total_tokens"]),
                answer: path(&["response"]),
                ..Declared::default()
            },
        }),
        ..declaring_recipe()
    };
    let action = ExternalEngineAction::resolving_with(Declares {
        bin,
        recipe: Some(recipe),
    });
    let shape = json!({
        "type": "object",
        "properties": {"printed": {"type": "string"}},
        "required": ["printed"],
        "allow_extra": false
    });
    let input = json!({
        "tool": "motore-di-prova",
        "stdin": format!("run wc -c LICENSE and answer in this shape: {shape}"),
        "timeout_secs": 10,
        "answer_shape": shape
    });

    let outcome = with_price_list(None, || action.execute(&input, &shared("corsa-busta", "passo")))
        .expect("the answer inside the envelope is in shape");

    let ActionOutcome::Went(output) = outcome else {
        panic!("an answer in shape is Went")
    };
    assert_eq!(output["answer"]["printed"], "     662 LICENSE\n");
}

/// An engine that writes the prompt it received back on its error channel
/// is not refusing when that prompt happens to hold a refusal's words: the
/// words it echoed are the step's, not the engine's. See fault 190.
#[test]
fn a_refusal_word_the_engine_only_echoed_from_the_prompt_is_not_a_refusal() {
    let dir = scratch("echoed-prompt");
    let bin = fake_engine(&dir, "motore-eco", "cat >&2
exit 1");
    let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
    let mut recipe = declaring_recipe();
    recipe.unusable_when = vec!["usage limit".to_owned()];
    recipe.exhausted_when = vec!["usage limit".to_owned()];
    let action = ExternalEngineAction::resolving_with(Declares {
        bin,
        recipe: Some(recipe),
    })
    .recording_to(Some(ledger));
    let input = json!({
        "tool": "motore-di-prova",
        "stdin": "Write a paragraph.\nKeep the brief under the usage limit we agreed.",
        "timeout_secs": 10
    });

    let error = with_price_list(None, || action.execute(&input, &shared("corsa-eco", "passo")))
        .expect_err("an engine that exits one has not answered");

    assert_eq!(error.class, "engine_exit_error");
    let calls = calls_in(&dir.join("deposito"));
    assert_eq!(calls[0].error_type.as_deref(), Some("exit_error"));
}

/// The negative control: the same words written by the engine itself, on a
/// line the prompt does not hold, are still its refusal.
#[test]
fn a_refusal_word_the_engine_wrote_itself_is_still_a_refusal() {
    let dir = scratch("own-refusal");
    let bin = fake_engine(
        &dir,
        "motore-esaurito",
        "cat >&2\necho 'error: usage limit reached' >&2\nexit 1",
    );
    let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
    let mut recipe = declaring_recipe();
    recipe.unusable_when = vec!["usage limit".to_owned()];
    recipe.exhausted_when = vec!["usage limit".to_owned()];
    let action = ExternalEngineAction::resolving_with(Declares {
        bin,
        recipe: Some(recipe),
    })
    .recording_to(Some(ledger));
    let input = json!({
        "tool": "motore-di-prova",
        "stdin": "Keep the brief under the usage limit we agreed.",
        "timeout_secs": 10
    });

    let error = with_price_list(None, || action.execute(&input, &shared("corsa-propria", "passo")))
        .expect_err("a spent engine has not answered");

    assert_eq!(error.class, "engine_exhausted");
}
