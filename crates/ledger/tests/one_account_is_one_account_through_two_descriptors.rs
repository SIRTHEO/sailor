//! A quota belongs to an account, not to the descriptor that reached it.
//! `codex` and `codex-repair` spend the same one, and a reading that splits
//! them hides how close that account is to the end.

use ledger::{EngineIdentity, Ledger, ModelCallRecord};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-accounts-{}-{serial}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create the test directory");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn a_profile(cli_id: &str, name: &str) -> EngineIdentity {
    EngineIdentity::ProfileInForce {
        cli_id: cli_id.to_owned(),
        profile_name: name.to_owned(),
        home_dir: PathBuf::from("/nowhere").join(name),
        endpoint: None,
    }
}

fn a_call(
    call_id: &str,
    cli: &str,
    identity: EngineIdentity,
    cost: i64,
    at: i64,
    error_type: Option<&str>,
) -> ModelCallRecord {
    ModelCallRecord {
        call_id: call_id.to_owned(),
        run_id: "run-1".to_owned(),
        step_id: Some("ask".to_owned()),
        purpose: "external_engine".to_owned(),
        cli: cli.to_owned(),
        requested_model: String::new(),
        actual_model: String::new(),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
        cache_write_tokens: None,
        cache_write_long_tokens: None,
        total_tokens: Some(1_000),
        turns: None,
        cost_micros: Some(cost),
        declared_cost_micros: None,
        price_currency: None,
        input_price_micros_per_million: None,
        output_price_micros_per_million: None,
        cached_price_micros_per_million: None,
        cache_write_price_micros_per_million: None,
        cache_write_long_price_micros_per_million: None,
        engine_identity: identity,
        retry_chain: vec![],
        error_type: error_type.map(str::to_owned),
        started_at: at,
        ended_at: Some(at + 1),
        session_id: None,
        work_kind: None,
        fell_back_from: Vec::new(),
        session_mode: None,
        role: None,
        role_resolved_to: Vec::new(),
    }
}

#[test]
fn two_descriptors_on_one_account_are_summed_into_one_standing() {
    let scratch = Scratch::new();
    let ledger = Ledger::open(&scratch.0).expect("open the ledger");
    for (id, cli) in [("a", "codex"), ("b", "codex-repair"), ("c", "codex")] {
        ledger
            .record_model_call(&a_call(
                id,
                cli,
                a_profile("codex", "someone@example.test"),
                1_000_000,
                1_000,
                None,
            ))
            .expect("write the call");
    }
    let found = ledger.accounts_standing(0).expect("read the standings");
    assert_eq!(found.len(), 1, "the account was split by descriptor: {found:?}");
    assert_eq!(found[0].cli, "codex");
    assert_eq!(found[0].profile.as_deref(), Some("someone@example.test"));
    assert_eq!(found[0].calls, 3);
    assert_eq!(found[0].spent_micros, 3_000_000);
    assert_eq!(found[0].tokens, 3_000);
}

/// **AN EXHAUSTED QUOTA IS THE ONE FACT WORTH A MARK.**
#[test]
fn the_last_time_a_quota_ran_out_is_kept_and_a_plain_error_is_not() {
    let scratch = Scratch::new();
    let ledger = Ledger::open(&scratch.0).expect("open the ledger");
    let who = a_profile("codex", "someone@example.test");
    ledger
        .record_model_call(&a_call("a", "codex", who.clone(), 0, 100, Some("exit_error")))
        .expect("write the failed call");
    assert_eq!(
        ledger.accounts_standing(0).expect("read")[0].ran_out_at,
        None,
        "a plain error was read as an exhausted quota"
    );
    ledger
        .record_model_call(&a_call("b", "codex", who, 0, 500, Some("quota_exhausted")))
        .expect("write the exhausted call");
    assert_eq!(
        ledger.accounts_standing(0).expect("read")[0].ran_out_at,
        Some(500)
    );
}

#[test]
fn a_call_older_than_the_window_is_not_counted() {
    let scratch = Scratch::new();
    let ledger = Ledger::open(&scratch.0).expect("open the ledger");
    let who = a_profile("claude", "someone@example.test");
    ledger
        .record_model_call(&a_call("old", "claude", who.clone(), 9, 100, None))
        .expect("write the old call");
    ledger
        .record_model_call(&a_call("new", "claude", who, 7, 900, None))
        .expect("write the new call");
    let found = ledger.accounts_standing(500).expect("read the standings");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].calls, 1, "the window did not hold: {found:?}");
    assert_eq!(found[0].spent_micros, 7);
}

/// A call that inherited the terminal's home belongs to no profile, and saying
/// so is different from attributing it to one.
#[test]
fn an_inherited_identity_is_kept_apart_from_every_profile() {
    let scratch = Scratch::new();
    let ledger = Ledger::open(&scratch.0).expect("open the ledger");
    ledger
        .record_model_call(&a_call(
            "a",
            "gemini-cli",
            EngineIdentity::InheritedFromTheTerminal {
                cli_id: "gemini".to_owned(),
            },
            5,
            100,
            None,
        ))
        .expect("write the call");
    let found = ledger.accounts_standing(0).expect("read the standings");
    assert_eq!(found[0].cli, "gemini");
    assert_eq!(found[0].profile, None);
    assert_eq!(found[0].home, None);
}
