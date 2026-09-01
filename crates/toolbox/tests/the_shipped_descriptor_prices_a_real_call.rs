//! The **shipped** descriptor reads a **real** output and prices it.
//!
//! The other usage tests try one piece at a time on built data, and pass even
//! when the pieces do not touch. All three are real here: the shipped
//! descriptor, an output a real engine wrote, and the **shipped price list** —
//! one written beside a test keeps the chain whole only inside that file.

use actions::ToolResolver;
use toolbox::descriptor::{Catalog, Source};
use toolbox::probe::Machine;
use toolbox::resolver::Tools;

/// The real output, cut down to the fields the descriptor names. The numbers are
/// not invented: 2 input tokens, 4 output, 9,922 read from the cache, 12,347
/// written to a long-lived cache, and 0.128541 dollars declared.
const REAL_OUTPUT: &str = r#"{
  "stop_reason": "end_turn",
  "num_turns": 3,
  "total_cost_usd": 0.128541,
  "usage": {
    "input_tokens": 2,
    "cache_creation_input_tokens": 12347,
    "cache_read_input_tokens": 9922,
    "output_tokens": 4,
    "cache_creation": {
      "ephemeral_1h_input_tokens": 12347,
      "ephemeral_5m_input_tokens": 0
    }
  },
  "modelUsage": {
    "claude-opus-5[1m]": {
      "inputTokens": 2,
      "costUSD": 0.128541,
      "canonicalModel": "claude-opus-5"
    }
  },
  "result": "ok",
  "type": "result"
}"#;

/// Only the descriptors shipped with the product: what anyone installing it
/// gets, with nothing of this machine around it.
fn shipped_only() -> Tools {
    let machine = Machine {
        path_dirs: Vec::new(),
        home: std::path::PathBuf::from("/home/nobody"),
        env: Default::default(),
        version_probes: false,
    };
    Tools::new(Catalog::load(&[Source::Builtin]), machine)
}

#[test]
fn the_shipped_claude_descriptor_reads_a_real_call_and_prices_it() {
    let recipe = shipped_only()
        .ask_recipe("claude-code")
        .expect("claude-code declares how a question is put to it");
    let usage = recipe
        .usage
        .expect("and also declares how its usage is read");

    let reading = models::usage::read_declared(REAL_OUTPUT, &usage.declared);

    // The counts, one by one, from the real output.
    assert_eq!(reading.input_tokens, Some(2));
    assert_eq!(reading.output_tokens, Some(4));
    assert_eq!(reading.cached_tokens, Some(9_922));
    assert_eq!(reading.cache_write_tokens, Some(0), "short cache: none");
    assert_eq!(reading.cache_write_long_tokens, Some(12_347));
    // The model is the KEY of `modelUsage`, not a field.
    assert_eq!(reading.model.as_deref(), Some("claude-opus-5[1m]"));
    // And the step's output stays the answer, not the envelope.
    assert_eq!(reading.answer.as_deref(), Some("ok"));

    // **THE PRICE LIST IS THE SHIPPED ONE, NOT ONE WRITTEN BESIDE THIS TEST.**
    // Empty `models` in `crates/models/pricing.default.json` and this line falls.
    // While it lived here the product shipped the descriptor and not the list,
    // so on a machine with no `~/.config/sailor/pricing.json` this same call
    // cost zero: a green test on a piece that does not travel with the product
    // is exactly how that stayed invisible.
    let prices = models::pricing::shipped();
    let entry = prices
        .find(reading.model.as_deref().unwrap())
        .expect("the alias leads to an entry of the shipped price list");
    let cost = models::pricing::cost_micros(
        models::pricing::TokenCounts {
            input: reading.input_tokens,
            output: reading.output_tokens,
            cached: reading.cached_tokens,
            cache_write: reading.cache_write_tokens,
            cache_write_long: reading.cache_write_long_tokens,
        },
        entry.micros(),
    )
    .expect("the cost computes");

    // TURNS ARE READ FROM THE SAME OUTPUT AS THE TOKENS, and used to be thrown
    // away. They are the quantity that explains the bill of a chain of steps: a
    // four-step flow reads 8% more per turn than a single session doing the same
    // work, and costs twice as much, because it takes twice the turns.
    assert_eq!(
        reading.turns,
        Some(3),
        "the shipped descriptor reads `num_turns`"
    );

    // The engine declared $0.128541. Our own count, starting from the counts and
    // the price list alone, reaches the same figure to the micro.
    let declared = (reading.declared_cost.unwrap() * 1_000_000.0).round() as i64;
    assert_eq!(declared, 128_541);
    assert_eq!(
        cost, declared,
        "the price-list count and the engine's must coincide"
    );
}

/// The same chain on an engine that declares **less**: agy gives the tokens but
/// not the model. The cost must stay unknown — not zero, and not a guessed
/// model.
#[test]
fn a_engine_that_names_no_model_leaves_the_cost_unknown_not_zero() {
    let recipe = shipped_only().ask_recipe("agy").expect("agy is shipped");
    let usage = recipe.usage.expect("and declares its own usage");

    let said = r#"{"status":"SUCCESS","response":"ok\n",
        "usage":{"input_tokens":14514,"output_tokens":195,
                 "cache_read_tokens":0,"total_tokens":14709}}"#;
    let reading = models::usage::read_declared(said, &usage.declared);

    assert_eq!(reading.input_tokens, Some(14_514));
    assert_eq!(reading.total_tokens, Some(14_709));
    assert_eq!(reading.answer.as_deref(), Some("ok\n"));
    assert_eq!(
        reading.model, None,
        "agy names no model, and nobody invents one for it"
    );

    let prices = models::pricing::shipped();
    assert!(
        reading
            .model
            .as_deref()
            .and_then(|name| prices.find(name))
            .is_none(),
        "without a name there is no price-list entry, so no price to apply"
    );
}

/// The command line the **shipped** descriptor composes for `agy`, where the
/// prompt is an argument and not standard input. **Two blocks right separately
/// and wrong together**: the `usage` options, appended after `ask`'s, slipped
/// between `--print` and the question, and `agy` answered `--print took
/// "--output-format" as its prompt`, ignoring the real text — exactly what a
/// test written one block at a time cannot see.
#[test]
fn the_prompt_flag_stays_glued_to_the_prompt_it_introduces() {
    // It executes nothing: the code decides the sequence, so this is identical
    // on a loaded machine, with no network and with no `agy` installed. What
    // stays uncovered — actually executing every composed line — is still
    // fault 1's cure, written down beside it and never built.
    let recipe = shipped_only().ask_recipe("agy").expect("agy is shipped");
    assert_eq!(
        recipe.prompt,
        actions::PromptVia::LastArg,
        "this test only means something if the question is an argument"
    );

    let line = actions::command_line(&recipe);
    assert_eq!(
        line,
        vec!["--mode", "plan", "--output-format", "json", "--print"],
        "the usage options go before the one that introduces the question"
    );
    assert_eq!(
        line.last().map(String::as_str),
        Some("--print"),
        "nothing gets between the flag introducing the question and the question"
    );
}
