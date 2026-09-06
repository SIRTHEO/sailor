//! **FAULT 35, PROVED WHERE IT LIVED.**
//!
//! Reading `~/.config/sailor/pricing.json` and nothing else, on a freshly
//! installed machine there is no such file: every `cost_micros` stayed `None`,
//! every run's recorded spend stayed zero, and a flow with `spend_cap_micros`
//! ran to the end without the cap ever tripping — no error, no warning. A brake
//! that does not brake.
//!
//! The scene here is that one: **nothing at home**, and the cost must be known
//! all the same. The counts are not invented — they are what
//! `claude -p --output-format json` really declared on this machine. That the
//! shipped descriptor can *read* them out of that output is proved by
//! `toolbox/tests/the_shipped_descriptor_prices_a_real_call.rs`, which prices
//! against the shipped list rather than a hand-written one: the chain is whole
//! there, and here is the one link this fault is about.
//!
//! **WHY IT IS NOT PROVED BY POINTING `SAILOR_PRICING` SOMEWHERE.** An
//! environment variable belongs to the **process**: `cargo test` runs one
//! binary's tests on several threads of the same process, so a test writing it
//! would decide the price list of all the others while they run. Hence the rule
//! lives in `price_list_from`, which takes the home text as an argument.

use models::pricing::{cost_micros, Known, Price, TokenCounts};

/// The real counts of that call: 2 input tokens, 4 output, 9,922 read from the
/// cache and 12,347 written to a long-lived one. The engine declared
/// 0.128541 dollars.
const MEASURED: TokenCounts = TokenCounts {
    input: Some(2),
    output: Some(4),
    cached: Some(9_922),
    cache_write: None,
    cache_write_long: Some(12_347),
};

/// The name that engine declared itself under. Not the price list's `id`: an
/// alias, and were the shipped list to lose it the cost would go back to
/// unknown with no price having changed.
const AS_THE_ENGINE_NAMED_IT: &str = "claude-opus-5[1m]";

/// **THE PROOF THAT CLOSES FAULT 35.** No file at home, and a real call's cost
/// is known all the same — to the micro-unit.
///
/// Put `price_list_from` back to returning the home list alone and this turns
/// red: it is the original defect, not an imitation of it.
#[test]
fn with_nothing_in_the_users_home_a_real_call_still_gets_a_cost() {
    let prices = actions::price_list_from(None);
    let entry = prices
        .find(AS_THE_ENGINE_NAMED_IT)
        .unwrap_or_else(|| panic!("il listino spedito non conosce «{AS_THE_ENGINE_NAMED_IT}»"));
    assert_eq!(
        cost_micros(MEASURED, entry.micros()),
        Some(128_541),
        "senza un listino spedito questa chiamata costava zero, e nessun tetto scattava"
    );
    assert_eq!(prices.currency, "USD");
}

/// The home file still wins: that is the point of the cure, not a side effect.
/// A price is corrected with a text editor, and holds from the next call with
/// nothing recompiled.
#[test]
fn what_the_user_writes_at_home_still_beats_what_is_shipped() {
    let home = r#"{"currency":"USD","models":[
        {"id":"claude-opus-5","aliases":["claude-opus-5[1m]"],
         "input_per_million":1.0,"output_per_million":1.0,
         "cached_per_million":1.0,"cache_write_long_per_million":1.0}
    ]}"#;
    let prices = actions::price_list_from(Some(home));
    let entry = prices
        .find(AS_THE_ENGINE_NAMED_IT)
        .expect("la voce di casa");
    assert_eq!(
        entry.input_per_million,
        Some(1.0),
        "ha vinto quella spedita"
    );
    // And what the home file does not name still comes from the shipped one.
    assert_eq!(prices.knows("claude-haiku-4-5"), Known::Priced);
}

/// **A MALFORMED HOME FILE MUST NOT SWITCH THE PRICE LIST OFF.** It is fault 35
/// in its quietest form: a typo in a JSON file that switches off a spending cap
/// without telling anybody.
#[test]
fn a_broken_file_at_home_falls_back_to_what_is_shipped() {
    let prices = actions::price_list_from(Some("questo non è JSON"));
    assert_eq!(prices.knows("claude-opus-5"), Known::Priced);
}

/// Having no price for a model has to be knowable. OpenAI's and Google's models
/// are out of the shipped list **on purpose** — nobody has verified their
/// prices — and stay declaredly unknown instead of becoming zero.
#[test]
fn a_model_nobody_priced_is_reported_as_absent_not_as_free() {
    let prices = actions::price_list_from(None);
    assert_eq!(prices.knows("gpt-5-codex"), Known::Absent);
    assert_eq!(
        cost_micros(
            TokenCounts {
                input: Some(1_000_000),
                output: Some(1_000_000),
                ..TokenCounts::default()
            },
            prices
                .find("gpt-5-codex")
                .map(Price::micros)
                .unwrap_or_default()
        ),
        None,
        "un modello senza prezzo non costa zero: non si sa"
    );
}
