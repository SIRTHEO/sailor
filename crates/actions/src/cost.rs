//! What a call cost, and where it is written down: the price list, the row
//! the ledger keeps for every call, and the counter that keeps two rows apart.

use crate::candidates::Candidate;
use crate::{Reading, EXTERNAL_ENGINE_ACTION};
use flow::SharedState;
use ledger::{EngineIdentity, Ledger, ModelCallRecord};

// ── what a call cost ─────────────────────────────────────────────────────

/// Where the local price list lives: a JSON file in Sailor's home, beside the
/// ledger and the flows, so it can be rewritten with a text editor instead of
/// recompiled. `SAILOR_PRICING` moves it elsewhere, for the tests and for
/// whoever keeps several lists. **IT IS NOT `modelli.json`**, which holds the
/// user's *choice* of model: mixing «what I want» with «what it costs» would
/// make changing a preference touch a price list, and the reverse.
const PRICING_ENV: &str = "SAILOR_PRICING";
const PRICING_FILE: &str = "pricing.json";

/// The price list to apply: the shipped one, overridden by the home one.
///
/// **PURE, AND NOT A FLOURISH.** The home text arrives as an argument, so «with
/// nothing at home the cost is still known» can be asked without the disk and
/// without an environment variable — per **process**, and fatal to tests run in
/// parallel inside it. Reading the file is [`load_pricing`]. An unreadable home
/// file disarms nobody: it falls back to the shipped list, since a malformed
/// JSON once left the cost unknown for a whole run — fault 35 at its quietest, a
/// typo that switches off a spending cap.
pub fn price_list_from(home_text: Option<&str>) -> models::pricing::PriceList {
    let shipped = models::pricing::shipped();
    match home_text.and_then(|text| models::pricing::PriceList::parse(text).ok()) {
        Some(home) => shipped.overridden_by(home),
        None => shipped,
    }
}

/// This machine's price list: the shipped one, overridden by the home file.
///
/// **RE-READ ON EVERY CALL, NEVER CACHED**: a price changed halfway through a
/// long run counts from the next call, not from the next restart, and reading a
/// small file beside the launch of an external process costs nothing next to
/// what is about to happen. **PUBLIC BECAUSE `sailor flow check` MUST SAY WHAT
/// IT CANNOT PRICE**: a brake that does not brake has to be seen before
/// launching, and showing it is a command's job, not this crate's.
pub fn current_price_list() -> models::pricing::PriceList {
    let path = match std::env::var_os(PRICING_ENV).filter(|value| !value.is_empty()) {
        Some(declared) => Some(std::path::PathBuf::from(declared)),
        None => ledger::sailor_home().map(|home| home.join(PRICING_FILE)),
    };
    let text = path.and_then(|path| std::fs::read_to_string(path).ok());
    price_list_from(text.as_deref())
}

/// Where to record what was spent: ledger, run and step.
///
/// All three are needed. Missing one, **no row is written at all** rather than
/// one attributed to nobody: a row with no run sums with nothing else and would
/// foul the accounts worse than a missing row. Same rule `sink_for_step` already
/// applies to the live text.
pub(crate) struct Recording<'a> {
    pub(crate) ledger: &'a Ledger,
    pub(crate) run_id: String,
    pub(crate) step_id: String,
}

pub(crate) fn recording_for<'a>(ledger: &'a Option<Ledger>, shared: &SharedState) -> Option<Recording<'a>> {
    Some(Recording {
        ledger: ledger.as_ref()?,
        run_id: shared.get(flow::CURRENT_RUN)?.as_str()?.to_owned(),
        step_id: shared.get(flow::CURRENT_STEP)?.as_str()?.to_owned(),
    })
}

/// A per-process counter, so two calls in the same second inside the same step
/// do not overwrite each other: `call_id` is the primary key, and a collision
/// would make a charge vanish instead of summing it.
static CALLS_SO_FAR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// What the chain did before this engine was asked.
///
/// The two lists answer two different questions and never merge: `tried_before`
/// is who was started and could not work, `fell_back_from` is who was never
/// reached at all.
#[derive(Default)]
pub(crate) struct Chain {
    pub(crate) tried_before: Vec<String>,
    pub(crate) fell_back_from: Vec<String>,
}

/// What is known of a call just finished, before it is priced.
pub(crate) struct Spent {
    pub(crate) reading: Reading,
    pub(crate) error_type: Option<&'static str>,
    pub(crate) started_at: i64,
    pub(crate) ended_at: i64,
    /// The session this call ran under, when it is known.
    pub(crate) session_id: Option<String>,
    /// The identity the process started under: which home, and how it was
    /// chosen. Without it two runs of the same flow are not the same measure —
    /// the row carries no reason for the two usages differing.
    pub(crate) identity: EngineIdentity,
    /// The kind of work the step declared, for the sum per kind.
    pub(crate) work_kind: Option<String>,
    /// What this call did with the session of its flow, the case where it
    /// asked to continue one and started from nothing included.
    pub(crate) session_mode: Option<ledger::SessionMode>,
}

/// Writes the row of **this** call into the ledger.
///
/// **IT IS WRITTEN EVEN WHEN THE CALL WENT BADLY**, deliberately: an interrupted
/// turn burns quota all the same, and zeroing its cost would understate spending
/// exactly in the minutes before an exhaustion — when the measure is wanted.
/// Telling «useful work» from «quota consumed» belongs to whoever reads the
/// rows. **AND EVEN WHEN THE TOKENS ARE UNKNOWN**: «this engine was called forty
/// times, tokens undeclared» can be acted on, while silence hides the hole, and
/// a total presenting itself as complete while it is partial is the lie this
/// work exists to end. A ledger failure does not break the step: the measure
/// serves the work, and failing a call that already succeeded for want of a note
/// would be the opposite of what is being built.
pub(crate) fn record_the_call(
    record: &Recording<'_>,
    candidate: &Candidate,
    chain: &Chain,
    spent: Spent,
) {
    let Some(cli) = candidate.id.as_deref() else {
        // A `bin` written by hand in the step is not a model call: `sh -c echo`
        // burns no quota, and filling the ledger with it would make unreadable
        // the very view this work exists to make readable.
        return;
    };
    if !candidate.can_be_asked {
        // **NOR IS A TOOL THAT CANNOT BE ASKED ANYTHING.** `git` and `cargo` are
        // in the catalogue, run from a step, and burn no subscription's quota.
        // Counting them among model calls is worse than noise: they arrive with
        // no cost, so they make `Spend::is_complete()` false on **every** real
        // run — three rows out of twenty-four on this machine's ledger — and the
        // cap's honesty line («the true spend is higher») lights up always, even
        // with nothing unknown. An always-lit warning is read by nobody, and the
        // one that gets lost is the real one.
        return;
    }
    let reading = spent.reading;
    let price_list = current_price_list();
    // The link to the price list runs through the name the engine itself states,
    // never a guess: a presumed model would be an invented number wearing the
    // face of a measure, believed for ever by whoever reads it.
    let entry = reading
        .model
        .as_deref()
        .and_then(|name| price_list.find(name));
    let prices = entry
        .map(models::pricing::Price::micros)
        .unwrap_or_default();
    let priced = models::pricing::cost_micros(
        models::pricing::TokenCounts {
            input: reading.input_tokens,
            output: reading.output_tokens,
            cached: reading.cached_tokens,
            cache_write: reading.cache_write_tokens,
            cache_write_long: reading.cache_write_long_tokens,
        },
        prices,
    );
    // **A COUNT OF ONE MODEL IS NOT THE COST OF A CALL THAT CROSSED SEVERAL.**
    // The engine states each model's tokens apart, this row prices the one the
    // engine put first, and the rest go to a price of their own that no line of
    // this row can carry. Unknown, never a third of the truth: see fault 121.
    let cost_micros = reading.counts_the_whole_call().then_some(priced).flatten();
    let sequence = CALLS_SO_FAR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let written = ModelCallRecord {
        call_id: format!(
            "{}:{}:{}:{sequence}",
            record.run_id, record.step_id, spent.started_at
        ),
        run_id: record.run_id.clone(),
        step_id: Some(record.step_id.clone()),
        purpose: EXTERNAL_ENGINE_ACTION.to_owned(),
        cli: cli.to_owned(),
        // A step names the tool, not the model: nobody here *asks* for a model,
        // and writing one down would be inventing it. Empty means «undeclared»,
        // and the window shows it as such.
        requested_model: String::new(),
        actual_model: reading.model.clone().unwrap_or_default(),
        // Turns come from the same output the tokens do, and they are the
        // quantity that explains why a chain of steps costs more than a single
        // session.
        turns: reading.turns,
        input_tokens: reading.input_tokens,
        output_tokens: reading.output_tokens,
        cached_tokens: reading.cached_tokens,
        cache_write_tokens: reading.cache_write_tokens,
        cache_write_long_tokens: reading.cache_write_long_tokens,
        total_tokens: reading.total_tokens,
        cost_micros,
        // The engine's own cost sits beside ours, never in its place.
        declared_cost_micros: reading
            .declared_cost
            .map(|usd| (usd * 1_000_000.0).round() as i64),
        // The currency is that of the price list the sum was made with: with no
        // sum made there is no currency to declare.
        price_currency: cost_micros.map(|_| price_list.currency.clone()),
        input_price_micros_per_million: prices.input,
        output_price_micros_per_million: prices.output,
        cached_price_micros_per_million: prices.cached,
        cache_write_price_micros_per_million: prices.cache_write,
        cache_write_long_price_micros_per_million: prices.cache_write_long,
        // **THE IDENTITY THIS PROCESS STARTED UNDER.** Not «under which
        // profile»: which home, and how it was chosen. A step that sets the home
        // variable itself starts the engine elsewhere, while what would land
        // here is the name of the active profile.
        engine_identity: spent.identity,
        retry_chain: chain.tried_before.clone(),
        fell_back_from: chain.fell_back_from.clone(),
        error_type: spent.error_type.map(str::to_owned),
        started_at: spent.started_at,
        ended_at: Some(spent.ended_at),
        session_id: spent.session_id,
        work_kind: spent.work_kind,
        session_mode: spent.session_mode,
    };
    let _ = record.ledger.record_model_call(&written);
}

#[cfg(test)]
mod what_it_cost {
    //! The proofs of the measure: what a call consumed, where it ends up
    //! written, and what happens to an engine that declares none of it.
    //!
    //! **NO REAL ENGINE AND NO PAID CALL.** The engines in here are shell
    //! scripts written on the fly, as everywhere else in this file: the one way
    //! to prove a measure without buying it.

    use super::*;
    use crate::cooldown;
    use crate::engine::ExternalEngineAction;
    use crate::recipe::{AskRecipe, PromptVia, ToolResolver, UsageRecipe};
    use crate::{Declared, Pointer, Shape};
    use flow::{Action, ActionOutcome};
    use ledger::Ledger;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sailor-cost-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch directory");
        dir
    }

    /// An executable script that behaves as it is told.
    fn fake_engine(dir: &std::path::Path, name: &str, body: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write the fake engine");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make it executable");
        path.to_string_lossy().into_owned()
    }

    /// The test price list, with cache at a tenth of input: the difference
    /// criterion 3 of the brief exists so as not to lose.
    const PRICE_LIST: &str = r#"{
      "currency": "USD",
      "dated": "2026-08-29",
      "models": [
        { "id": "modello-di-prova", "aliases": ["prova"],
          "input_per_million": 3.0, "output_per_million": 15.0,
          "cached_per_million": 0.3 }
      ]
    }"#;

    fn write_price_list(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("pricing.json");
        std::fs::write(&path, PRICE_LIST).expect("write the price list");
        path
    }

    /// A shared state like the one the executor prepares before every action:
    /// run and step, under `flow`'s reserved keys.
    fn shared(run: &str, step: &str) -> SharedState {
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_RUN.to_owned(), json!(run));
        shared.insert(flow::CURRENT_STEP.to_owned(), json!(step));
        shared
    }

    /// A resolver pointing at a script with whatever recipe it is handed: the
    /// place where, in real life, a descriptor arrives.
    struct Declares {
        bin: String,
        recipe: Option<AskRecipe>,
    }

    impl ToolResolver for Declares {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                "motore-di-prova" => Ok(self.bin.clone()),
                other => Err(format!("«{other}» is not on this machine")),
            }
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            self.recipe.clone()
        }
    }

    fn path(keys: &[&str]) -> Option<Pointer> {
        Some(Pointer::Path(
            keys.iter().map(|k| (*k).to_owned()).collect(),
        ))
    }

    /// The recipe of an engine able to say what it consumed: it asks for the
    /// envelope and declares where the numbers, the model and the answer are.
    fn declaring_recipe() -> AskRecipe {
        AskRecipe {
            args: Vec::new(),
            prompt: PromptVia::Stdin,
            args_before_prompt: Vec::new(),
            unusable_when: Vec::new(),
            silent_without_prompt: false,
            refuses_without_prompt: Vec::new(),
            exhausted_when: Vec::new(),
            cooldown_secs: None,
            waits_for_a_person_when: Vec::new(),
            usage: Some(UsageRecipe {
                args: vec!["--output-format".to_owned(), "json".to_owned()],
                declared: Declared {
                    read: Shape::Json,
                    from: models::usage::Heard::Stdout,
                    reports: crate::Reports::PerCall,
                    input_tokens: path(&["usage", "input_tokens"]),
                    output_tokens: path(&["usage", "output_tokens"]),
                    cached_tokens: path(&["usage", "cache_read_input_tokens"]),
                    cache_write_tokens: path(&["usage", "cache_creation_input_tokens"]),
                    cache_write_long_tokens: None,
                    total_tokens: None,
                    turns: None,
                    cost: path(&["total_cost_usd"]),
                    model: path(&["model"]),
                    answer: path(&["result"]),
                },
            }),
        }
    }

    /// An engine that counts each model apart and names it as the KEY of that
    /// count.
    fn counting_each_model_apart() -> AskRecipe {
        AskRecipe {
            usage: Some(UsageRecipe {
                args: Vec::new(),
                declared: Declared {
                    read: Shape::Json,
                    from: models::usage::Heard::Stdout,
                    reports: crate::Reports::PerCall,
                    input_tokens: path(&["usage", "input_tokens"]),
                    output_tokens: path(&["usage", "output_tokens"]),
                    cached_tokens: path(&["usage", "cache_read_input_tokens"]),
                    cache_write_tokens: path(&[
                        "usage",
                        "cache_creation",
                        "ephemeral_5m_input_tokens",
                    ]),
                    cache_write_long_tokens: path(&[
                        "usage",
                        "cache_creation",
                        "ephemeral_1h_input_tokens",
                    ]),
                    total_tokens: None,
                    turns: path(&["num_turns"]),
                    cost: path(&["total_cost_usd"]),
                    model: Some(Pointer::FirstKey(vec!["modelUsage".to_owned()])),
                    answer: path(&["result"]),
                },
            }),
            ..declaring_recipe()
        }
    }

    /// The real output of a call that opened four helpers on a second model:
    /// `usage` carries the first, `modelUsage` names both.
    const ANSWERS_FOR_TWO_MODELS: &str = r#"cat > /dev/null
printf '%s' '{"result":"la risposta vera","num_turns":36,"total_cost_usd":20.00991825,"usage":{"input_tokens":1028,"cache_creation_input_tokens":155602,"cache_read_input_tokens":2912773,"output_tokens":43363,"cache_creation":{"ephemeral_1h_input_tokens":155602,"ephemeral_5m_input_tokens":0}},"modelUsage":{"claude-fable-5-1":{"inputTokens":1028,"costUSD":6.0186632499999995},"claude-opus-5[1m]":{"inputTokens":288,"costUSD":13.991254999999994}}}'"#;

    const TWO_MODEL_PRICE_LIST: &str = r#"{
      "currency": "USD",
      "dated": "2026-09-06",
      "models": [
        { "id": "claude-fable-5-1",
          "input_per_million": 10.0, "output_per_million": 50.0,
          "cached_per_million": 0.25,
          "cache_write_per_million": 12.5, "cache_write_long_per_million": 20.0 }
      ]
    }"#;

    /// An engine that answers with the envelope **only** when asked for
    /// `--output-format json`, and in plain text otherwise: the real behaviour
    /// of a command line, and without it the proof on the unchanged output would
    /// prove nothing.
    const WRAPS_ON_DEMAND: &str = r#"cat > /dev/null
printf '%s\n' "$@" > "$(dirname "$0")/argv"
if [ "$1" = "--output-format" ] && [ "$2" = "json" ]; then
  printf '{"result":"the true answer","model":"modello-di-prova","total_cost_usd":0.5,"usage":{"input_tokens":1000000,"output_tokens":1000000,"cache_read_input_tokens":1000000}}'
else
  printf 'the true answer'
fi"#;

    /// **AN ENGINE THAT ANSWERS IN JSON WITHOUT ANYBODY ASKING IT TO.** It
    /// proves the usage is read because a DESCRIPTOR declares it, not because
    /// the output happens to resemble a known format: were a branch wired to one
    /// vendor's keys to appear in here, its tokens would be read all the same,
    /// which is exactly what the model-independence constraint forbids.
    const ALWAYS_WRAPS: &str = r#"cat > /dev/null
printf '{"result":"the true answer","model":"modello-di-prova","total_cost_usd":0.5,"usage":{"input_tokens":1000000,"output_tokens":1000000,"cache_read_input_tokens":1000000}}'"#;

    /// The command line the fake engine was really invoked with.
    fn argv_of(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("argv"))
            .expect("the fake engine wrote down its own command line")
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn calls_in(dir: &std::path::Path) -> Vec<ledger::ModelCallRecord> {
        let ledger = Ledger::open(dir).expect("reopen the ledger");
        let dump = ledger
            .projection_dump()
            .expect("the ledger can say what it holds");
        // **ONE READER OF THE PROJECTION, AND IT IS NOT HERE.** A private copy
        // of `ui::parse::parse_model_call_row` — twenty-eight hand-written
        // indices — would let a moved column break **both readings the same
        // way**, leaving the tests that compare one against the other green.
        // `actions` may depend on `ui`: `ui` never depended on `actions`, and
        // cargo allows a test-only cycle on purpose, as `flow`'s `Cargo.toml`
        // says. Keeping the single reader honest is
        // `ledger::MODEL_CALL_DUMP_COLUMNS`, neither reading nor writing.
        ui::parse::parse_model_calls(&dump)
    }

    /// The price list lives in a file, and the tests must not fight over the
    /// home of whoever runs them: `SAILOR_PRICING` moves it. A lock, because the
    /// tests run in parallel inside one process and there is a single
    /// environment variable — two tests would take the list from each other.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_price_list<T>(price_list: Option<&std::path::Path>, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match price_list {
            Some(path) => std::env::set_var(PRICING_ENV, path),
            None => std::env::set_var(PRICING_ENV, "/no/price/list/here"),
        }
        let out = body();
        std::env::remove_var(PRICING_ENV);
        out
    }

    // ── (a) who declares: true tokens, cache apart, cost from the list ─

    /// **CRITERION 2 AND CRITERION 3 TOGETHER.** An engine that declares how its
    /// usage is read produces a ledger row with the true tokens, the cache in a
    /// column of its own, and the cost computed from the local price list — not
    /// the one the engine itself states.
    #[test]
    fn a_declaring_engine_writes_a_row_with_true_tokens_and_a_cost_from_the_price_list() {
        let dir = scratch("declares");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let outcome = with_price_list(Some(&price_list), || {
            action.execute(&input, &shared("corsa-1", "passo-1"))
        })
        .expect("the engine answers");
        let ActionOutcome::Went(output) = outcome else {
            panic!("an engine that answers is always Went")
        };

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "one call, one row");
        let call = &calls[0];
        assert_eq!(call.run_id, "corsa-1");
        assert_eq!(call.step_id.as_deref(), Some("passo-1"));
        assert_eq!(call.cli, "motore-di-prova");
        assert_eq!(call.actual_model, "modello-di-prova");
        assert_eq!(call.input_tokens, Some(1_000_000));
        assert_eq!(call.output_tokens, Some(1_000_000));
        assert_eq!(
            call.cached_tokens,
            Some(1_000_000),
            "the cache has a column of its own and does not end up inside the input"
        );
        // 1M at 3 $ + 1M at 15 $ + 1M of cache at 0.30 $ = 18.30 $ = 18 300 000 micros.
        assert_eq!(call.cost_micros, Some(18_300_000));
        assert_eq!(call.price_currency.as_deref(), Some("USD"));
        assert_eq!(call.cached_price_micros_per_million, Some(300_000));
        // The cost the engine states of its own sits beside, never in place:
        // 0.5 $ is deliberately different from the price list's sum.
        assert_eq!(call.declared_cost_micros, Some(500_000));
        assert_eq!(call.error_type, None);
        assert!(call.ended_at.is_some());

        // And the step's output is the text, not the envelope.
        assert_eq!(output["stdout"], "the true answer");
    }

    /// The row of a call that crossed two models: the engine's own figure, and
    /// **no** figure from the price list. See fault 121.
    #[test]
    fn a_call_across_two_models_leaves_the_price_list_figure_unknown() {
        let dir = scratch("two-models");
        let price_list = dir.join("pricing.json");
        std::fs::write(&price_list, TWO_MODEL_PRICE_LIST).expect("write the price list");
        let bin = fake_engine(&dir, "motore", ANSWERS_FOR_TWO_MODELS);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(counting_each_model_apart()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let outcome = with_price_list(Some(&price_list), || {
            action.execute(&input, &shared("corsa-121", "struttura"))
        })
        .expect("the engine answers");
        assert!(matches!(outcome, ActionOutcome::Went(_)));

        let calls = calls_in(&dir.join("deposito"));
        let call = &calls[0];
        assert_eq!(call.actual_model, "claude-fable-5-1");
        assert_eq!(call.cache_write_long_tokens, Some(155_602));
        assert_eq!(
            call.cost_micros, None,
            "unknown, not the 6_018_663 of one model out of two"
        );
        assert_eq!(call.declared_cost_micros, Some(20_009_918));
        assert_eq!(call.price_currency, None, "no count, no currency");
    }

    /// **CRITERION 3, FROM THE SIDE WHERE IT BREAKS.** Were the cache counted at
    /// the input price instead of its own, this cost would come out ten times
    /// dearer on the cache's share. The proof above pins the number; this one
    /// says why that number and no other.
    #[test]
    fn cache_priced_as_input_would_cost_ten_times_more() {
        let solo_cache = models::pricing::cost_micros(
            models::pricing::TokenCounts {
                input: Some(0),
                output: Some(0),
                cached: Some(1_000_000),
                ..models::pricing::TokenCounts::default()
            },
            models::pricing::PriceList::parse(PRICE_LIST)
                .unwrap()
                .find("prova")
                .unwrap()
                .micros(),
        );
        assert_eq!(solo_cache, Some(300_000), "1M of cache costs 0.30 $");
        assert!(
            solo_cache.unwrap() * 5 < 3_000_000,
            "and not the 3.00 $ it would cost as fresh input"
        );
    }

    // ── (b) who declares nothing: identical, and unknown ───────────────

    /// **CRITERION 4.** An engine with no `usage` block produces exactly the
    /// same output as before, and its row carries the tokens as UNKNOWN. Never
    /// zero: a zero sums, and no downstream view can correct it.
    #[test]
    fn an_engine_that_declares_nothing_is_unchanged_and_leaves_the_tokens_unknown() {
        let dir = scratch("declares-nothing");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(AskRecipe {
                args: Vec::new(),
                prompt: PromptVia::Stdin,
                args_before_prompt: Vec::new(),
                unusable_when: Vec::new(),
                silent_without_prompt: false,
                refuses_without_prompt: Vec::new(),
                exhausted_when: Vec::new(),
                cooldown_secs: None,
                waits_for_a_person_when: Vec::new(),
                usage: None,
            }),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let outcome = with_price_list(Some(&price_list), || {
            action.execute(&input, &shared("corsa-2", "passo-2"))
        })
        .expect("the engine answers");
        let ActionOutcome::Went(output) = outcome else {
            panic!("an engine that answers is always Went")
        };

        // The same output as always: no extra field, no envelope.
        assert_eq!(output["status"], "ok");
        assert_eq!(output["stdout"], "the true answer");
        assert_eq!(
            output.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["status", "stdout", "stderr"],
            "the step's output gains no field because somebody is measuring"
        );

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "the call is recorded all the same");
        let call = &calls[0];
        assert_eq!(call.input_tokens, None, "unknown, not zero");
        assert_eq!(call.output_tokens, None, "unknown, not zero");
        assert_eq!(call.cached_tokens, None, "unknown, not zero");
        assert_eq!(call.total_tokens, None);
        assert_eq!(call.cost_micros, None, "no tokens, no cost");
        assert_eq!(call.actual_model, "", "no model was declared");
    }

    // ── (c) a failed call writes its row all the same ──────────────────

    /// **CRITERION 5.** An engine that exited in error writes its row with the
    /// cause: an interrupted turn burns quota all the same, and zeroing its cost
    /// would understate spending in the very minutes before an exhaustion.
    #[test]
    fn a_failed_call_still_writes_its_row_with_the_cause() {
        let dir = scratch("failed");
        let bin = fake_engine(
            &dir,
            "motore",
            "cat > /dev/null\necho 'it went badly' >&2\nexit 3",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &shared("corsa-3", "passo-3"))
        })
        .expect_err("an exit other than zero breaks the step");
        assert_eq!(error.class, "engine_exit_error");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "a failure leaves its row too");
        assert_eq!(calls[0].error_type.as_deref(), Some("exit_error"));
        assert_eq!(calls[0].cli, "motore-di-prova");
        assert_eq!(calls[0].input_tokens, None, "it had no time to say");
    }

    /// **SPENT AND BROKEN ARE TWO THINGS, EVEN WITH A LONE ENGINE.**
    ///
    /// Fault 14 in full. Claude at its weekly limit stopped the step with an
    /// error reading «exited in error», sending its reader after a fault that
    /// was not there when the thing to do was wait for seven or change engine:
    /// the distinction existed in the code but was guarded behind «there is a
    /// chain», so it never held in the case that happened. Two places are
    /// watched because there are two readers: a person reads the error class
    /// now, a sum a month from now reads `error_type` in the ledger — and a sum
    /// that mixes spent quotas with real faults tells nobody anything.
    #[test]
    fn a_single_engine_that_ran_out_is_not_reported_as_broken() {
        let dir = scratch("spent-alone");
        let bin = fake_engine(
            &dir,
            "motore-esaurito",
            "cat > /dev/null\necho \"You've hit your weekly limit · resets 7am\"\nexit 1",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &shared("corsa-esaurita", "passo-1"))
        })
        .expect_err("a spent engine on its own cannot do the work");

        assert_eq!(
            error.class, "engine_exhausted",
            "not «engine_exit_error»: whoever reads it has to know the quota ran out"
        );
        assert!(
            error.said.contains("quota"),
            "and the message says so in words: {}",
            error.said
        );

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "the call burned quota: the row is there");
        assert_eq!(
            calls[0].error_type.as_deref(),
            Some("exhausted"),
            "and the row tells a spent quota apart from a fault"
        );
    }

    /// **A REAL FAULT STAYS A FAULT.** The twin of the proof above: same lone
    /// engine, same recipe with the same exhaustion words, but an output that
    /// does not contain them. Without it, making *any* failure say «exhausted»
    /// would pass green.
    #[test]
    fn a_single_engine_that_truly_broke_is_still_reported_as_broken() {
        let dir = scratch("broken-alone");
        let bin = fake_engine(
            &dir,
            "motore-rotto",
            "cat > /dev/null\necho 'error: the brief makes no sense' >&2\nexit 3",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &shared("corsa-rotta", "passo-1"))
        })
        .expect_err("an exit other than zero breaks the step");

        assert_eq!(error.class, "engine_exit_error");
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].error_type.as_deref(), Some("exit_error"));
    }

    /// An engine that answered in the shape the step declared has worked, and
    /// the words that mean a refusal are not looked for inside its answer.
    #[test]
    fn an_answer_in_the_declared_shape_is_not_read_as_a_refusal() {
        let dir = scratch("shape-against-refusal");
        // The answer says the word the descriptor declares, because it is about
        // it: an engine reading this tree prints it while working.
        let bin = fake_engine(
            &dir,
            "engine-that-talks-about-quotas",
            "cat > /dev/null\necho '{\"found\": \"the weekly limit sentence lives in the descriptor\"}'",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        recipe.exhausted_when = vec!["weekly limit".to_owned()];
        recipe.cooldown_secs = Some(1800);
        let aside = dir.join("cooldowns.json");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger))
        .cooling_down_in(Some(aside.clone()));
        let input = json!({
            "tool": "motore-di-prova",
            "stdin": "look at the tree, and answer in this shape: {\"type\":\"object\",\"properties\":{\"found\":{\"type\":\"string\"}},\"required\":[\"found\"],\"allow_extra\":false}",
            "timeout_secs": 10,
            "answer_shape": {
                "type": "object",
                "properties": {"found": {"type": "string"}},
                "required": ["found"],
                "allow_extra": false
            }
        });

        let outcome = with_price_list(None, || {
            action.execute(&input, &shared("corsa-in-forma", "passo-1"))
        })
        .expect("an answer in the declared shape is the step's answer");

        assert!(
            format!("{outcome:?}").contains("descriptor"),
            "the answer reaches the step: {outcome:?}"
        );
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].error_type, None, "no refusal was recorded");
        assert!(
            !aside.exists(),
            "an engine that worked is not set aside for half an hour"
        );
    }

    /// **THE TWIN, AND THE HALF THAT WAS UNGUARDED.** Same recipe, same
    /// declared shape, same words: only the engine's output changes, and it
    /// does not fit the shape. Then the words are all we have and they mean a
    /// refusal — the step closes `exhausted` and the engine goes aside. Read
    /// `in_shape` as «a shape was declared» and the whole crate stays green.
    #[test]
    fn an_answer_off_the_declared_shape_is_still_read_as_a_refusal() {
        let dir = scratch("off-shape-and-refusal");
        let bin = fake_engine(
            &dir,
            "motore-a-secco-in-forma-libera",
            "cat > /dev/null\necho \"You've hit your weekly limit · resets 7am\"",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        recipe.exhausted_when = vec!["weekly limit".to_owned()];
        recipe.cooldown_secs = Some(1800);
        let aside = dir.join("cooldowns.json");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger))
        .cooling_down_in(Some(aside.clone()));
        let input = json!({
            "tool": "motore-di-prova",
            "stdin": "look at the tree, and answer in this shape: {\"type\":\"object\",\"properties\":{\"found\":{\"type\":\"string\"}},\"required\":[\"found\"],\"allow_extra\":false}",
            "timeout_secs": 10,
            "answer_shape": {
                "type": "object",
                "properties": {"found": {"type": "string"}},
                "required": ["found"],
                "allow_extra": false
            }
        });

        let error = with_price_list(None, || {
            action.execute(&input, &shared("corsa-fuori-forma", "passo-1"))
        })
        .expect_err("an output that fits no shape and says it cannot work is a refusal");

        assert_eq!(error.class, "engine_exhausted", "{}", error.said);
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].error_type.as_deref(), Some("quota_exhausted"));
        let set = cooldown::set_aside_until(&aside, "motore-di-prova", now_secs())
            .expect("an engine that refused is set aside");
        assert!(set.said.contains("weekly limit"), "{set:?}");
    }

    /// A spent quota is its own class, and the engine is set aside for the
    /// time its descriptor declares: the second step in the same window does
    /// not knock on it. Without `exhausted_when` the same output stays the
    /// plain `exhausted` of before, and nobody is set aside.
    #[test]
    fn a_spent_quota_is_its_own_class_and_sets_the_engine_aside() {
        let dir = scratch("quota-spent");
        let bin = fake_engine(
            &dir,
            "motore-a-secco",
            "cat > /dev/null\necho \"You've hit your weekly limit · resets 7am\"\nexit 1",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        recipe.exhausted_when = vec!["weekly limit".to_owned()];
        recipe.cooldown_secs = Some(1800);
        let aside = dir.join("cooldowns.json");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(recipe.clone()),
        })
        .recording_to(Some(ledger))
        .cooling_down_in(Some(aside.clone()));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &shared("corsa-a-secco", "passo-1"))
        })
        .expect_err("a spent engine alone cannot do the work");
        assert_eq!(error.class, "engine_exhausted");
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].error_type.as_deref(), Some("quota_exhausted"));
        let set = cooldown::set_aside_until(&aside, "motore-di-prova", now_secs()).expect("set aside");
        assert!(set.said.contains("weekly limit"), "{set:?}");

        // The second knock is refused before spending, and says until when.
        let again = with_price_list(None, || {
            action.execute(&input, &shared("corsa-a-secco", "passo-2"))
        })
        .expect_err("an engine set aside is not tried");
        assert_eq!(again.class, "no_usable_engine");
        assert!(again.said.contains("set aside until"), "{}", again.said);
        assert_eq!(calls_in(&dir.join("deposito")).len(), 1, "nothing was spent on the second knock");

        // The control: the same words without `exhausted_when` are the old class, and nobody is aside.
        let plain = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(AskRecipe { exhausted_when: Vec::new(), cooldown_secs: None, ..recipe }),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito-2")).expect("second ledger")))
        .cooling_down_in(Some(dir.join("cooldowns-2.json")));
        with_price_list(None, || plain.execute(&input, &shared("corsa-piana", "passo-1")))
            .expect_err("still cannot work");
        assert_eq!(calls_in(&dir.join("deposito-2"))[0].error_type.as_deref(), Some("exhausted"));
        assert!(cooldown::set_aside_until(&dir.join("cooldowns-2.json"), "motore-di-prova", now_secs()).is_none());
    }

    /// A cap per engine on a window, declared by the person in a file: the
    /// first priced call fits, the second finds the window full and is refused
    /// before spending, naming the sum. A cap on another engine changes nothing.
    #[test]
    fn an_engine_over_its_budget_is_refused_before_spending() {
        let dir = scratch("a-cap");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let budgets = dir.join("budgets.json");
        // **THE ENGINE'S OWN FIGURE IS WHAT THE WINDOW COUNTS**, as it is for a
        // run: this one charges 0.50 $ and the price list works out 18.30 $. A
        // cap of 0.25 $ lets the first through and finds the window full after.
        std::fs::write(
            &budgets,
            r#"{"motore-di-prova": {"cap_micros": 250000, "window_secs": 3600}}"#,
        )
        .expect("write the caps");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger))
        .budgeted_by(Some(budgets));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        with_price_list(Some(&price_list), || {
            action.execute(&input, &shared("corsa-1", "passo-1"))
        })
        .expect("the first call fits under the cap");
        let refused = with_price_list(Some(&price_list), || {
            action.execute(&input, &shared("corsa-2", "passo-1"))
        })
        .expect_err("the second call finds the window full");
        assert_eq!(refused.class, "no_usable_engine");
        assert!(
            refused.said.contains("over its budget: spent 0.5000 $ of 0.2500 $"),
            "the window counted the price list's figure over the engine's own: {}",
            refused.said
        );
        assert_eq!(calls_in(&dir.join("deposito")).len(), 1, "the refusal spent nothing");

        // The control: a cap declared for some other engine does not bind this one.
        let others = dir.join("budgets-others.json");
        std::fs::write(&others, r#"{"another-engine": {"cap_micros": 1, "window_secs": 3600}}"#)
            .expect("write the other caps");
        let unbound = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito")).expect("reopen")))
        .budgeted_by(Some(others));
        with_price_list(Some(&price_list), || {
            unbound.execute(&input, &shared("corsa-3", "passo-1"))
        })
        .expect("a cap on another engine is not this engine's");
        assert_eq!(calls_in(&dir.join("deposito")).len(), 2);
    }

    /// **A DOOR KNOWN TO BE SHUT IS NOT KNOCKED ON AGAIN.** The file's
    /// arithmetic was proved; the chain that fills it was not. Here a real
    /// engine says its quota is spent, and the next chain refuses it without
    /// starting it — naming until when, and what it said.
    #[test]
    fn an_engine_that_said_its_quota_was_spent_is_not_started_again() {
        let dir = scratch("set-aside");
        let bin = fake_engine(
            &dir,
            "motore-esaurito",
            "cat > /dev/null\necho \"You've hit your weekly limit\"\nexit 0",
        );
        let aside = dir.join("cooldowns.json");
        let mut recipe = declaring_recipe();
        recipe.exhausted_when = vec!["weekly limit".to_owned()];
        recipe.cooldown_secs = Some(3_600);
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(recipe.clone()),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito")).expect("open the ledger")))
        .cooling_down_in(Some(aside.clone()));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        // The first chain runs it, and it says the quota is spent.
        let broke = with_price_list(None, || {
            action.execute(&input, &shared("corsa-1", "passo-1"))
        })
        .expect_err("a spent quota is not a step that went");
        assert_eq!(broke.class, "engine_exhausted");
        assert!(aside.exists(), "nothing was set aside: {}", broke.said);

        // The second chain does not start it at all: the refusal is the list's.
        let refused = with_price_list(None, || {
            action.execute(&input, &shared("corsa-2", "passo-1"))
        })
        .expect_err("the door is known to be shut");
        assert!(
            refused.said.contains("set aside until") && refused.said.contains("weekly limit"),
            "the refusal says neither until when nor what it said: {}",
            refused.said
        );
        assert_eq!(
            calls_in(&dir.join("deposito")).len(),
            1,
            "the second chain started the engine again"
        );

        // THE CONTROL: past its time the same engine is knocked on again, or
        // the code could set one aside for ever and pass. The instant is read
        // from the list and never recomputed: guessing it as `now + 3600`
        // guesses what the clock said when the file was written, and one second
        // of load left the file untouched and this arm red for nothing.
        let past = std::fs::read_to_string(&aside).expect("the list");
        let mut written: serde_json::Value = serde_json::from_str(&past).expect("the list is JSON");
        let now = now_secs();
        for (_, aside) in written.as_object_mut().expect("one entry per engine") {
            aside["until"] = json!(now - 1);
        }
        std::fs::write(&aside, serde_json::to_string(&written).expect("write it back"))
            .expect("bring its time forward");
        let again = with_price_list(None, || {
            action.execute(&input, &shared("corsa-3", "passo-1"))
        })
        .expect_err("it is spent again, but it was asked");
        assert!(
            !again.said.contains("set aside until"),
            "past its time it was still refused from the list: {}",
            again.said
        );
        assert_eq!(calls_in(&dir.join("deposito")).len(), 2);
    }

    /// An engine that resolves, with the pact its descriptor would declare.
    struct Pacted {
        bin: String,
        pact: models::pact::DataPact,
    }

    impl ToolResolver for Pacted {
        fn resolve(&self, _id: &str) -> Result<String, String> {
            Ok(self.bin.clone())
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(declaring_recipe())
        }
        fn data_pact(&self, _id: &str) -> models::pact::DataPact {
            self.pact
        }
    }

    /// A step that says its text is private never resolves to an engine whose
    /// pact is `trains` or `unknown`, and the refusal names the pact; the same
    /// step said public, or the same engine under `does_not_train`, runs.
    #[test]
    fn a_private_step_never_goes_where_the_pact_is_not_a_no() {
        use models::pact::DataPact;
        let dir = scratch("the-pact");
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let run = |pact: DataPact, input: serde_json::Value| {
            let action = ExternalEngineAction::resolving_with(Pacted { bin: bin.clone(), pact });
            with_price_list(None, || action.execute(&input, &shared("corsa", "passo")))
        };
        let private = json!({"tool": "motore-di-prova", "data": "private", "stdin": "ciao", "timeout_secs": 10});
        let public = json!({"tool": "motore-di-prova", "data": "public", "stdin": "ciao", "timeout_secs": 10});
        let unsaid = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let refused = run(DataPact::Trains, private.clone()).expect_err("a training engine is refused");
        assert_eq!(refused.class, "no_usable_engine");
        assert!(refused.said.contains("data pact is «trains»"), "{}", refused.said);
        let unknown = run(DataPact::Unknown, private.clone()).expect_err("unknown is not a no");
        assert!(unknown.said.contains("data pact is «unknown»"), "{}", unknown.said);

        run(DataPact::DoesNotTrain, private).expect("a pact that does not train may read it");
        run(DataPact::Trains, public).expect("a public step goes anywhere");
        run(DataPact::Unknown, unsaid).expect("a step that says nothing is public");
    }

    /// Two engines that both answer, told apart by the id on the ledger row.
    struct TwoEngines {
        bins: std::collections::BTreeMap<&'static str, String>,
    }

    impl ToolResolver for TwoEngines {
        fn resolve(&self, id: &str) -> Result<String, String> {
            self.bins.get(id).cloned().ok_or_else(|| format!("«{id}» is not here"))
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(declaring_recipe())
        }
    }

    /// An engine that answers on stdout and states its counts on stderr, the
    /// way a local model runner does: the descriptor says which pipe, and the
    /// row carries the counts; read from stdout instead, they stay unknown.
    #[test]
    fn counts_stated_on_stderr_are_read_when_the_descriptor_says_so() {
        let dir = scratch("stderr-counts");
        let bin = fake_engine(
            &dir,
            "locale",
            "cat > /dev/null\necho \"the answer\"\necho \"prompt eval count:    26 token(s)\" >&2\necho \"eval count:           298 token(s)\" >&2",
        );
        let recipe = |from: models::usage::Heard| AskRecipe {
            usage: Some(UsageRecipe {
                args: vec!["--verbose".to_owned()],
                declared: Declared {
                    read: Shape::Text,
                    from,
                    input_tokens: Some(Pointer::Pattern(r"prompt eval count:\s*(\d+)".to_owned())),
                    output_tokens: Some(Pointer::Pattern(r"(?m)^eval count:\s*(\d+)".to_owned())),
                    ..Declared::default()
                },
            }),
            ..declaring_recipe()
        };
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});
        let run = |from, ledger: &str| {
            let action = ExternalEngineAction::resolving_with(Declares { bin: bin.clone(), recipe: Some(recipe(from)) })
                .recording_to(Some(Ledger::open(dir.join(ledger)).expect("open")));
            with_price_list(None, || action.execute(&input, &shared("corsa", "passo"))).expect("answers");
            calls_in(&dir.join(ledger)).remove(0)
        };

        let heard = run(models::usage::Heard::Stderr, "deposito");
        assert_eq!((heard.input_tokens, heard.output_tokens), (Some(26), Some(298)));
        // The control: the same engine read on stdout states nothing.
        let unheard = run(models::usage::Heard::Stdout, "deposito-2");
        assert_eq!((unheard.input_tokens, unheard.output_tokens), (None, None));
    }

    /// Two engines with a subscription window each, read as fuel.
    struct Fuelled {
        bins: std::collections::BTreeMap<&'static str, String>,
        fuels: std::collections::BTreeMap<&'static str, models::fuel::Fuel>,
    }

    impl ToolResolver for Fuelled {
        fn resolve(&self, id: &str) -> Result<String, String> {
            self.bins.get(id).cloned().ok_or_else(|| format!("«{id}» is not here"))
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(declaring_recipe())
        }
        fn fuel(&self, id: &str) -> Vec<models::fuel::Fuel> {
            self.fuels.get(id).cloned().into_iter().collect()
        }
    }

    /// Under `prefer: fuel` the engine whose window expires unused soonest
    /// goes first even when the chain wrote it second; without it the chain
    /// stays as written.
    #[test]
    fn a_window_that_would_expire_unused_is_spent_first() {
        let dir = scratch("fuel");
        let long = fake_engine(&dir, "a-lungo", WRAPS_ON_DEMAND);
        let short = fake_engine(&dir, "a-breve", WRAPS_ON_DEMAND);
        let fuel = |engine: &str, left: f64, resets_in: i64| models::fuel::Fuel {
            engine: engine.to_owned(),
            unit: "five_hour".to_owned(),
            left_fraction: left,
            resets_in_secs: Some(resets_in),
        };
        let engines = || Fuelled {
            bins: [("a-lungo", long.clone()), ("a-breve", short.clone())].into_iter().collect(),
            fuels: [
                ("a-lungo", fuel("a-lungo", 0.80, 6 * 86_400)),
                ("a-breve", fuel("a-breve", 0.10, 3_600)),
            ]
            .into_iter()
            .collect(),
        };
        let by_fuel = json!({"tool": ["a-lungo", "a-breve"], "prefer": "fuel", "stdin": "ciao", "timeout_secs": 10});
        let as_written = json!({"tool": ["a-lungo", "a-breve"], "stdin": "ciao", "timeout_secs": 10});

        let action = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito")).expect("open")));
        with_price_list(None, || action.execute(&by_fuel, &shared("corsa-1", "passo")))
            .expect("the short window answers");
        assert_eq!(calls_in(&dir.join("deposito"))[0].cli, "a-breve");

        let plain = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito-2")).expect("open")));
        with_price_list(None, || plain.execute(&as_written, &shared("corsa-2", "passo")))
            .expect("the chain's first answers");
        assert_eq!(calls_in(&dir.join("deposito-2"))[0].cli, "a-lungo");

        // A word `prefer` does not know is refused by name, not read as silence.
        let by_luck = json!({"tool": ["a-lungo"], "prefer": "luck", "stdin": "ciao", "timeout_secs": 10});
        let refused = with_price_list(None, || plain.execute(&by_luck, &shared("corsa-3", "passo")))
            .expect_err("an unknown preference is refused");
        assert_eq!(refused.class, "invalid_input");
        assert!(refused.said.contains("«luck»"), "{}", refused.said);
    }

    /// A step that would start under a profile whose endpoint cannot be
    /// reached is refused before spending, with the profile's reason.
    #[test]
    fn a_profile_whose_endpoint_is_refused_holds_the_engine_back_before_spending() {
        let dir = scratch("endpoint-refused");
        let bin = fake_engine(&dir, "codex", WRAPS_ON_DEMAND);
        let store = dir.join("profili.json");
        std::fs::write(
            &store,
            format!(
                r#"{{"profiles": [{{"name": "altrove", "cli_id": "codex", "home_dir": "{}",
                    "endpoint": {{"url": "http://localhost:1/v1", "key_var": "NO_SUCH_KEY_VAR_HERE",
                    "protocol": "anthropic-messages"}}}}],
                  "active": {{"codex": "altrove"}}}}"#,
                dir.join("casa").display()
            ),
        )
        .expect("write the store");
        let action = ExternalEngineAction::resolving_with(Declares { bin, recipe: Some(declaring_recipe()) });
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let refused = with_profiles_state(&store, || action.execute(&input, &shared("corsa", "passo")));

        let refused = refused.expect_err("a profile that cannot be pointed there refuses the launch");
        assert_eq!(refused.class, "no_usable_engine");
        assert!(refused.said.contains("anthropic-messages"), "{}", refused.said);
    }

    /// A step that declares its kind goes first to the engines the strengths
    /// table names for that kind, then to the chain as written; without a row
    /// for the kind the chain's first answers. The ledger row names the kind.
    #[test]
    fn a_kind_of_work_goes_first_where_the_table_says_and_the_ledger_names_it() {
        let dir = scratch("strengths");
        let local = fake_engine(&dir, "locale", WRAPS_ON_DEMAND);
        let chained = fake_engine(&dir, "catena", WRAPS_ON_DEMAND);
        let engines = || TwoEngines {
            bins: [("locale", local.clone()), ("catena", chained.clone())].into_iter().collect(),
        };
        let table = dir.join("strengths.json");
        std::fs::write(&table, r#"{"measured_on": "a test", "rows": {"mechanical": ["locale"]}}"#)
            .expect("write the table");
        let empty = dir.join("strengths-empty.json");
        std::fs::write(&empty, r#"{"measured_on": "a test", "rows": {}}"#).expect("write the empty table");
        let input = json!({"tool": "catena", "kind": "mechanical", "stdin": "ciao", "timeout_secs": 10});

        let action = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito")).expect("open")))
            .strong_by(Some(table));
        with_price_list(None, || action.execute(&input, &shared("corsa-1", "passo")))
            .expect("the local engine answers");
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].cli, "locale", "the table's engine went first, ahead of the chain");
        assert_eq!(calls[0].work_kind.as_deref(), Some("mechanical"));

        // The control: without a row for the kind, the chain as written.
        let plain = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito-2")).expect("open")))
            .strong_by(Some(empty));
        with_price_list(None, || plain.execute(&input, &shared("corsa-2", "passo")))
            .expect("the chain's engine answers");
        assert_eq!(calls_in(&dir.join("deposito-2"))[0].cli, "catena");
    }

    /// **A PREFERRED ENGINE THAT IS NOT HERE IS WRITTEN DOWN.**
    ///
    /// The step answers on the chain it already had, and the row of whoever
    /// answered names the engine it fell back from: without that, a paid run
    /// reads like one the local engine did for free. Never started is not
    /// failed, so it goes in its own column and not in the retry chain.
    #[test]
    fn an_absent_preferred_engine_is_named_on_the_row_of_whoever_answered() {
        let dir = scratch("the-fallback-written-down");
        let chained = fake_engine(&dir, "catena", WRAPS_ON_DEMAND);
        let table = dir.join("strengths.json");
        std::fs::write(&table, r#"{"measured_on": "a test", "rows": {"mechanical": ["locale"]}}"#)
            .expect("write the table");

        let action = ExternalEngineAction::resolving_with(TwoEngines {
            bins: [("catena", chained)].into_iter().collect(),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito")).expect("open")))
        .strong_by(Some(table));
        let input = json!({"tool": "catena", "kind": "mechanical", "stdin": "ciao", "timeout_secs": 10});
        with_price_list(None, || action.execute(&input, &shared("corsa-1", "passo")))
            .expect("the chain answers when the table's engine is not here");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].cli, "catena", "the chain did the work");
        assert_eq!(
            calls[0].fell_back_from,
            ["locale"],
            "the row must say the preferred engine was not there"
        );
        assert!(calls[0].retry_chain.is_empty(), "nobody was started before it");
    }

    /// **A STEP THAT DECLARES NO KIND IS NOT MOVED, AND FELL BACK FROM NOBODY.**
    ///
    /// Both directions a mutant can break: with the table's engine here the
    /// chain must still answer, because a preference belongs to a declared
    /// kind; with it absent the column must stay empty, because a step that
    /// asked for nothing fell back from nothing.
    #[test]
    fn a_step_without_a_kind_keeps_its_chain_and_falls_back_from_nobody() {
        let dir = scratch("no-strength-declared");
        let local = fake_engine(&dir, "locale", WRAPS_ON_DEMAND);
        let chained = fake_engine(&dir, "catena", WRAPS_ON_DEMAND);
        let table = dir.join("strengths.json");
        std::fs::write(&table, r#"{"measured_on": "a test", "rows": {"mechanical": ["locale"]}}"#)
            .expect("write the table");
        let input = json!({"tool": "catena", "stdin": "ciao", "timeout_secs": 10});

        let both = ExternalEngineAction::resolving_with(TwoEngines {
            bins: [("locale", local), ("catena", chained.clone())].into_iter().collect(),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito")).expect("open")))
        .strong_by(Some(table.clone()));
        with_price_list(None, || both.execute(&input, &shared("corsa-1", "passo")))
            .expect("a step without a kind runs as it always did");
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].cli, "catena", "the table must not move a step that declared no kind");
        assert!(calls[0].fell_back_from.is_empty(), "it fell back from nobody");

        // And with the table's engine missing there is still nothing to name.
        let alone = ExternalEngineAction::resolving_with(TwoEngines {
            bins: [("catena", chained)].into_iter().collect(),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito-2")).expect("open")))
        .strong_by(Some(table));
        with_price_list(None, || alone.execute(&input, &shared("corsa-2", "passo")))
            .expect("the chain answers");
        assert!(
            calls_in(&dir.join("deposito-2"))[0].fell_back_from.is_empty(),
            "a step that declared no kind fell back from nobody"
        );
    }

    /// **AND THE LEDGER MUST SAY SO EVEN WHEN THE EXIT IS ZERO.**
    ///
    /// The half the step's behaviour does not show. The twin proofs in `tests`
    /// watch whether the fallback fires; this one watches the row left behind,
    /// which is what somebody reads tomorrow. Born with `error_type: None` it is
    /// **indistinguishable from a call that worked**: a sum mixing the two says
    /// that engine answered, and its reader goes looking for nothing. Without
    /// this proof a mutant that lets the fallback fire but writes `None` instead
    /// of `exhausted` would slip under the other two.
    #[test]
    fn a_zero_exit_refusal_is_recorded_as_exhausted_not_as_a_clean_call() {
        let dir = scratch("spent-at-zero-in-the-ledger");
        let bin = fake_engine(
            &dir,
            "motore-esaurito-a-zero",
            "cat > /dev/null\necho \"You've hit your weekly limit · resets 7am\"\nexit 0",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &shared("corsa-esaurita-a-zero", "passo-1"))
        })
        .expect_err("an engine saying it cannot work has not answered");
        assert_eq!(error.class, "engine_exhausted");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "the call was made, and goes on the record");
        assert_eq!(
            calls[0].error_type.as_deref(),
            Some("exhausted"),
            "a spent engine that exits zero is not a clean call: the row saying \
             so is the only trace left"
        );
    }

    /// **AND THE KIND DOES NOT DEPEND ON `accept`, IN EITHER BRANCH.**
    ///
    /// A step's tolerance is about **what the run does** — whether the failure
    /// is a datum the step keeps or a reason to stop — and must not touch **what
    /// stays written**. In the `ExitError` branch that always held, since
    /// `note(...)` sits ahead of the tolerance check; in the `Ok` branch it sat
    /// behind it, so with `accept: ["exit_error"]` declared the row was born
    /// `NULL` again — indistinguishable from a real answer, the defect surviving
    /// in a corner of its own remedy. The two halves sit together on purpose:
    /// they are one claim — «the kind is the same and does not depend on the
    /// tolerance» — over both exit codes.
    #[test]
    fn a_tolerated_refusal_is_recorded_as_exhausted_whatever_the_exit_code() {
        for (name, exit, script) in [
            (
                "zero",
                0,
                "cat > /dev/null\necho \"You've hit your weekly limit\"\nexit 0",
            ),
            (
                "one",
                1,
                "cat > /dev/null\necho \"You've hit your weekly limit\"\nexit 1",
            ),
        ] {
            let dir = scratch(&format!("tolerated-refusal-{name}"));
            let bin = fake_engine(&dir, "motore-esaurito", script);
            let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
            let mut recipe = declaring_recipe();
            recipe.unusable_when = vec!["weekly limit".to_owned()];
            let action = ExternalEngineAction::resolving_with(Declares {
                bin,
                recipe: Some(recipe),
            })
            .recording_to(Some(ledger));
            // The step declares it will keep this engine's failure: the run does
            // not stop, and indeed the step carries on.
            let input = json!({
                "tool": "motore-di-prova",
                "stdin": "ciao",
                "timeout_secs": 10,
                "accept": ["exit_error"]
            });

            let outcome = with_price_list(None, || {
                action.execute(&input, &shared(&format!("corsa-{name}"), "passo-1"))
            })
            .unwrap_or_else(|error| {
                panic!(
                    "the step tolerates the failure, so it must not break: {}",
                    error.said
                )
            });
            assert!(
                matches!(outcome, ActionOutcome::Went(_)),
                "the tolerance is what it always was: the step carries on (exit {exit})"
            );

            let calls = calls_in(&dir.join("deposito"));
            assert_eq!(calls.len(), 1);
            assert_eq!(
                calls[0].error_type.as_deref(),
                Some("exhausted"),
                "exit {exit}: the step tolerated the failure, but the ledger row must \
                 say all the same that this engine could not work. The tolerance \
                 decides what the run does, not what stays written"
            );
        }
    }

    /// An engine that never starts still leaves a trace, with its own cause:
    /// without this row a chain that falls back would look as if it had chosen
    /// the second engine first.
    #[test]
    fn an_engine_that_never_starts_leaves_its_own_row_too() {
        let dir = scratch("never-started");
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: "/no/binary/here-for-sure".to_owned(),
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &shared("corsa-4", "passo-4"))
        })
        .expect_err("a binary that is not there breaks the step");
        assert_eq!(error.class, "engine_spawn_failed");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].error_type.as_deref(), Some("spawn_failed"));
    }

    // ── the step's output does not change because it is measured ───────

    /// **THE CONSTRAINT NOBODY ASKED FOR, AND THAT WOULD BREAK A REAL FLOW.**
    /// A shipped flow declares `allow_extra: false` on the answer shape of an
    /// engine step. If asking for the envelope left the envelope inside
    /// `stdout`, that flow would go red over a measure it never asked for. Here
    /// the text coming out must be **identical** with and without the measure
    /// switched on.
    #[test]
    fn asking_for_a_json_envelope_does_not_change_what_the_step_receives() {
        let dir = scratch("envelope");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let without = {
            let ledger = Ledger::open(dir.join("without")).expect("the ledger");
            let action = ExternalEngineAction::resolving_with(Declares {
                bin: bin.clone(),
                recipe: Some(AskRecipe {
                    args: Vec::new(),
                    prompt: PromptVia::Stdin,
                    args_before_prompt: Vec::new(),
                    unusable_when: Vec::new(),
                    silent_without_prompt: false,
                    refuses_without_prompt: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
                    waits_for_a_person_when: Vec::new(),
                    usage: None,
                }),
            })
            .recording_to(Some(ledger));
            let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
                action.execute(&input, &shared("corsa-5", "passo-5"))
            })
            .expect("it answers") else {
                panic!("Went")
            };
            output
        };

        let with = {
            let ledger = Ledger::open(dir.join("with")).expect("the ledger");
            let action = ExternalEngineAction::resolving_with(Declares {
                bin,
                recipe: Some(declaring_recipe()),
            })
            .recording_to(Some(ledger));
            let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
                action.execute(&input, &shared("corsa-6", "passo-6"))
            })
            .expect("it answers") else {
                panic!("Went")
            };
            output
        };

        assert_eq!(
            without, with,
            "measuring must not change by one comma what the step hands downstream"
        );
        // And the measure really happened: without this, the proof would pass
        // even if the `usage` block never reached the invocation.
        assert_eq!(
            calls_in(&dir.join("with"))[0].input_tokens,
            Some(1_000_000),
            "the envelope was asked for and read"
        );
        assert_eq!(calls_in(&dir.join("without"))[0].input_tokens, None);
    }

    // ── without a place to write, nothing is written ───────────────────

    /// A row attributed to nobody would foul the sums worse than a missing row:
    /// with no ledger, or no run, nothing is recorded.
    #[test]
    fn without_a_ledger_or_without_a_run_nothing_is_written() {
        let dir = scratch("nothing-to-hold-on-to");
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let recipe = declaring_recipe();

        // With no ledger: the step works all the same.
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(recipe.clone()),
        });
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});
        assert!(action
            .execute(&input, &shared("corsa-7", "passo-7"))
            .is_ok());

        // With the ledger but no run key: no row.
        let ledger = Ledger::open(dir.join("deposito")).expect("the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let mut only_the_step = SharedState::new();
        only_the_step.insert(flow::CURRENT_STEP.to_owned(), json!("passo-8"));
        assert!(action.execute(&input, &only_the_step).is_ok());
        assert!(
            calls_in(&dir.join("deposito")).is_empty(),
            "with no run, no spending is attributed to anybody"
        );
    }

    /// A `bin` written by hand in the step is not a model call: `sh -c echo`
    /// burns no quota, and filling the ledger with it would make unreadable the
    /// view this work exists to make readable.
    #[test]
    fn a_hand_written_bin_is_not_a_model_call() {
        let dir = scratch("a-hand-written-bin");
        let ledger = Ledger::open(dir.join("deposito")).expect("the ledger");
        let action = ExternalEngineAction::new().recording_to(Some(ledger));
        let input = json!({"bin": "echo", "args": ["ciao"], "timeout_secs": 10});

        assert!(action
            .execute(&input, &shared("corsa-9", "passo-9"))
            .is_ok());
        assert!(calls_in(&dir.join("deposito")).is_empty());
    }

    /// **`cargo` AND `git` ARE NOT MODEL CALLS, AND THE LEDGER MUST NOT COUNT
    /// THEM.**
    ///
    /// Measured on this machine's ledger: of twenty-four `model_calls` rows, two
    /// are `git` and one `cargo`. None of the three burns any subscription's
    /// quota, and all three arrive with no cost — so `Spend::is_complete()` is
    /// **false on every real run**, and the cap's honesty line («the true spend
    /// is higher») lights up always, even with nothing unknown. An always-lit
    /// warning is read by nobody, and that is how the real one is lost — the
    /// codex row, which genuinely does not state its cost. **THE DESCRIPTOR
    /// DECIDES, NOT A LIST OF NAMES WRITTEN HERE**: a tool is an engine if it
    /// declares **how it is asked a question** (`ask`), which `git` and `cargo`
    /// do not, and no list of names in here would age well. Same rule as fault
    /// 3 — what the catalogue declares counts for more than what the code
    /// guesses.
    #[test]
    fn a_tool_that_cannot_be_asked_anything_is_not_a_model_call() {
        let dir = scratch("not-an-engine");
        let bin = fake_engine(&dir, "fake-cargo", "printf 'ok'");
        let ledger = Ledger::open(dir.join("deposito")).expect("the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            // No `ask` recipe: this is how `cargo` is declared in the shipped
            // catalogue, and the step writes its own options.
            recipe: None,
        })
        .recording_to(Some(ledger));
        let input = json!({
            "tool": "motore-di-prova", "args": ["test"], "timeout_secs": 10
        });

        assert!(action
            .execute(&input, &shared("corsa-cargo", "prove"))
            .is_ok());

        assert!(
            calls_in(&dir.join("deposito")).is_empty(),
            "a `cargo` row among the model calls makes false every total that sums \
             it: {:?}",
            calls_in(&dir.join("deposito"))
        );
    }

    /// The options the step writes win, and with them the usage stays unknown:
    /// appending, behind the back of whoever wrote that command line, a question
    /// they never asked would be deciding for them. The row is written all the
    /// same, and says exactly this.
    ///
    /// **THE TWIN OF THE PROOF ABOVE**, to be read together: a real engine asked
    /// with the step's options **stays** in the count, and what leaves it is
    /// what is no engine. Without this, the filter could empty the table and the
    /// proof above would stay green.
    #[test]
    fn when_the_step_writes_its_own_args_the_usage_is_not_asked_for() {
        let dir = scratch("the-steps-own-args");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({
            "tool": "motore-di-prova", "args": ["--my-own-way"],
            "stdin": "ciao", "timeout_secs": 10
        });

        let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
            action.execute(&input, &shared("corsa-10", "passo-10"))
        })
        .expect("risponde") else {
            panic!("Went")
        };

        assert_eq!(output["stdout"], "the true answer");
        // **THE ARM THAT COUNTS**: the command line is EXACTLY the one the step
        // wrote. Appending the usage options would add, behind its author's
        // back, a question they never asked, and from outside it would be
        // invisible: the difference shows up in the process argv and nowhere else.
        assert_eq!(
            argv_of(&dir),
            vec!["--my-own-way".to_owned()],
            "no option added behind anybody's back"
        );
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "the call is recorded all the same");
        assert_eq!(calls[0].input_tokens, None, "but not measured");
    }

    /// **THE MODEL-INDEPENDENCE CONSTRAINT, AT THE POINT WHERE IT BREAKS.** An
    /// engine that declares no `usage` stays unmeasured EVEN IF its output is a
    /// JSON envelope holding keys somebody would recognise. Were the code to
    /// carry a branch wired to one vendor — «if it looks like this, read here» —
    /// this proof would go red, and it must.
    #[test]
    fn output_that_merely_looks_familiar_is_not_read_without_a_declaration() {
        let dir = scratch("no-branch-wired-to-a-vendor");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", ALWAYS_WRAPS);
        let ledger = Ledger::open(dir.join("deposito")).expect("the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(AskRecipe {
                args: Vec::new(),
                prompt: PromptVia::Stdin,
                args_before_prompt: Vec::new(),
                unusable_when: Vec::new(),
                silent_without_prompt: false,
                refuses_without_prompt: Vec::new(),
                exhausted_when: Vec::new(),
                cooldown_secs: None,
                waits_for_a_person_when: Vec::new(),
                usage: None,
            }),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
            action.execute(&input, &shared("corsa-11", "passo-11"))
        })
        .expect("risponde") else {
            panic!("Went")
        };

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].input_tokens, None,
            "those numbers are there, but no descriptor said to read them"
        );
        assert_eq!(calls[0].cost_micros, None);
        assert_eq!(calls[0].actual_model, "");
        // And the step's output is the raw one: with no `answer` declared
        // nothing is unwrapped, because nobody said where to look.
        assert!(
            output["stdout"].as_str().unwrap().starts_with('{'),
            "the envelope stays exactly as it came: {}",
            output["stdout"]
        );
    }

    // ── (f) under which equipment the call ran ─────────────────────────

    /// The same lock as `with_price_list`, for the profiles state:
    /// `PROFILES_STATE_PATH` is a single variable like `SAILOR_PRICING`, and two
    /// tests writing it together would take the equipment from each other. The
    /// price list is pointed at nothing on purpose — what is watched here is the
    /// profile, not the cost, and leaning on the home file of whoever runs the
    /// tests would be a way of coming out different with nothing changed.
    fn with_profiles_state<T>(state: &std::path::Path, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::env::set_var("PROFILES_STATE_PATH", state);
        std::env::set_var(PRICING_ENV, "/no/price/list/here");
        let out = body();
        std::env::remove_var("PROFILES_STATE_PATH");
        std::env::remove_var(PRICING_ENV);
        out
    }

    /// **THE EQUIPMENT THE CALL RAN UNDER ENDS UP ON ITS ROW.**
    ///
    /// Fault 18, second half. Without it two runs of the same flow are not the
    /// same measure: one chain of steps, under two profiles, gives two different
    /// usages for a reason the row does not carry, and the column was written
    /// empty on every call. **THE HOME PATH BELONGS IN IT**, being what a
    /// diagnosis leans on: a profile name is reused, moved and deleted, a path
    /// is the place one goes to look.
    ///
    /// *Mutant run*: put `engine_identity: EngineIdentity::default()` back in
    /// `record_the_call`. This one goes red while the twin below stays green —
    /// which is why there are two.
    #[test]
    fn the_row_says_under_which_equipment_the_call_ran() {
        let dir = scratch("equipment");
        // The file name IS the link: `cli_for_executable` recognises the command
        // line from the executable, not from the descriptor's id.
        let bin = fake_engine(&dir, "codex", ALWAYS_WRAPS);
        let state = dir.join("profili.json");
        std::fs::write(
            &state,
            json!({
                "profiles": [
                    {"name": "lavoro", "cli_id": "codex", "home_dir": dir.join("casa")}
                ],
                "active": {"codex": "lavoro"}
            })
            .to_string(),
        )
        .expect("write the profiles state");

        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        with_profiles_state(&state, || {
            action.execute(&input, &shared("corsa-1", "passo-1"))
        })
        .expect("the engine answers");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "one call, one row");
        assert_eq!(
            calls[0].engine_identity,
            EngineIdentity::ProfileInForce {
                cli_id: "codex".to_owned(),
                profile_name: "lavoro".to_owned(),
                home_dir: dir.join("casa"),
                endpoint: None,
            },
            "the row does not say under which identity the call ran"
        );
    }

    /// The twin: with no profile in force the row says **inherited**, not an
    /// invented name and not an emptiness either. Without it a mutant that
    /// always wrote the same identity would pass the proof above.
    ///
    /// **«INHERITED» IS THE POINT OF THE CURE.** One empty string served four
    /// different facts — an unknown binary, a vanished profile, a home no
    /// variable moves, and this case; now this one says the process started with
    /// the home of whoever opened the terminal, and which command line it was.
    #[test]
    fn with_no_profile_in_force_the_row_says_the_identity_was_inherited() {
        let dir = scratch("no-equipment");
        let bin = fake_engine(&dir, "codex", ALWAYS_WRAPS);
        let state = dir.join("profili.json");
        std::fs::write(&state, r#"{"profiles":[],"active":{}}"#)
            .expect("write the profiles state");

        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        with_profiles_state(&state, || {
            action.execute(&input, &shared("corsa-1", "passo-1"))
        })
        .expect("the engine answers");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(
            calls[0].engine_identity,
            EngineIdentity::InheritedFromTheTerminal {
                cli_id: "codex".to_owned()
            }
        );
    }

    /// A fake `codex` that says **which home it really started in**: it writes
    /// it to a file beside itself, then answers in the envelope like the others.
    /// Without that file the proof below would watch the ledger alone, which is
    /// half the defect.
    const WRITES_DOWN_ITS_HOME: &str = r#"cat > /dev/null
printf '%s' "$CODEX_HOME" > "$(dirname "$0")/casa"
printf '{"result":"the true answer","model":"modello-di-prova","usage":{"input_tokens":1,"output_tokens":1}}'"#;

    /// **THE LEDGER RECORDS AN IDENTITY THE PROCESS NEVER USED.**
    ///
    /// That the step wins is the decision, not the defect. The defect is the row
    /// going on naming the active profile: the engine started in the home
    /// written in the step, and whoever reads the ledger to learn which
    /// credentials that process ran with reads the name of a profile never put
    /// in force. This is the case where somebody changed identity deliberately —
    /// exactly what a diagnosis or a security check exists to see — and the case
    /// where the datum lies. **THE TWO HALVES ARE WATCHED TOGETHER**: what the
    /// process received, and what the row says. Apart, each stays green with the
    /// defect inside.
    #[test]
    fn the_row_does_not_name_a_profile_the_step_replaced() {
        let dir = scratch("equipment-overridden");
        let bin = fake_engine(&dir, "codex", WRITES_DOWN_ITS_HOME);
        let state = dir.join("profili.json");
        std::fs::write(
            &state,
            json!({
                "profiles": [
                    {"name": "lavoro", "cli_id": "codex", "home_dir": dir.join("casa-del-profilo")}
                ],
                "active": {"codex": "lavoro"}
            })
            .to_string(),
        )
        .expect("write the profiles state");

        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({
            "tool": "motore-di-prova",
            "stdin": "ciao",
            "env": {"CODEX_HOME": "/a/home/written/in/the/step"},
            "timeout_secs": 10
        });

        with_profiles_state(&state, || {
            action.execute(&input, &shared("corsa-1", "passo-1"))
        })
        .expect("the engine answers");

        let home_it_started_in =
            std::fs::read_to_string(dir.join("casa")).expect("the engine wrote down its home");
        assert_eq!(
            home_it_started_in, "/a/home/written/in/the/step",
            "the overlay runs the other way now: the profile overrode the step"
        );

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "one call, one row");
        assert_eq!(
            calls[0].engine_identity,
            EngineIdentity::ChosenByTheStep {
                cli_id: "codex".to_owned(),
                home_dir: PathBuf::from("/a/home/written/in/the/step"),
            },
            "the row names an identity the process never used: it started in {home_it_started_in}"
        );
    }

    /// **NO SECRET ENTERS ANY FIELD OF THE IDENTITY.**
    ///
    /// A step may carry any variable in its environment, keys included. What
    /// ends up in the ledger is **which home** and **how it was chosen**, never
    /// what surrounded it: a ledger row is read in a diagnosis, copied into a
    /// report and sent to somebody.
    ///
    /// *Mutant run*: making the identity carry the whole environment turns this
    /// one red and no other.
    #[test]
    fn no_secret_from_the_step_ends_up_in_the_recorded_identity() {
        let dir = scratch("no-token");
        let bin = fake_engine(&dir, "codex", ALWAYS_WRAPS);
        let state = dir.join("profili.json");
        std::fs::write(
            &state,
            json!({
                "profiles": [
                    {"name": "lavoro", "cli_id": "codex", "home_dir": dir.join("casa")}
                ],
                "active": {"codex": "lavoro"}
            })
            .to_string(),
        )
        .expect("write the profiles state");

        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        // A recognisable secret: if it showed up anywhere, it would be seen.
        let input = json!({
            "tool": "motore-di-prova",
            "stdin": "ciao",
            "env": {"OPENAI_API_KEY": "sk-this-must-never-show-up"},
            "timeout_secs": 10
        });

        with_profiles_state(&state, || {
            action.execute(&input, &shared("corsa-1", "passo-1"))
        })
        .expect("the engine answers");

        let calls = calls_in(&dir.join("deposito"));
        let written = calls[0].engine_identity.to_column();
        assert!(
            !written.contains("sk-this-must-never-show-up"),
            "a token from the step ended up in the recorded identity: {written}"
        );
        assert!(
            !calls[0].engine_identity.to_string().contains("sk-"),
            "a token from the step ended up in what is printed to a person"
        );
    }
}
