//! Reads the output of `Ledger::projection_dump`: every row is an array of
//! columns in the order `ledger::dump_table` fixes. Pure — it takes a
//! `serde_json::Value` already in memory, never a file — because that is the
//! only public way to read `runs` and `model_calls` without already knowing
//! their `run_id`. Token columns are stored as text so precision survives past
//! 2^53, so both a string and a number are accepted here.

use ledger::{EngineIdentity, ModelCallRecord, RunRecord};
use serde_json::Value;

pub fn parse_runs(dump: &Value) -> Vec<RunRecord> {
    rows_of(dump, "runs").filter_map(parse_run_row).collect()
}

pub fn parse_model_calls(dump: &Value) -> Vec<ModelCallRecord> {
    rows_of(dump, "model_calls")
        .filter_map(parse_model_call_row)
        .collect()
}

fn rows_of<'a>(dump: &'a Value, table: &str) -> impl Iterator<Item = &'a Value> {
    dump.get(table)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn parse_run_row(row: &Value) -> Option<RunRecord> {
    let cols = row.as_array()?;
    Some(RunRecord {
        run_id: str_at(cols, 0)?,
        kind: str_at(cols, 1)?,
        entity: str_at(cols, 2)?,
        parent_run_id: opt_str_at(cols, 3),
        started_by: str_at(cols, 4)?,
        status: str_at(cols, 5)?,
        total_cost_micros: i64_at(cols, 6)?,
        error: opt_str_at(cols, 7),
        started_at: i64_at(cols, 8)?,
        ended_at: opt_i64_at(cols, 9),
        // A dump from before the column has no tenth cell.
        worktree: opt_str_at(cols, 10),
    })
}

fn parse_model_call_row(row: &Value) -> Option<ModelCallRecord> {
    let cols = row.as_array()?;
    Some(ModelCallRecord {
        call_id: str_at(cols, 0)?,
        run_id: str_at(cols, 1)?,
        step_id: opt_str_at(cols, 2),
        purpose: str_at(cols, 3)?,
        cli: str_at(cols, 4)?,
        requested_model: str_at(cols, 5)?,
        actual_model: str_at(cols, 6)?,
        // From here down a NULL column means «unknown», never a value missing
        // from a malformed record: it reads as `None` instead of dropping the
        // whole row with `?`. The difference matters — an unmeasured call must
        // appear in the list, or it vanishes from the counts exactly as though
        // it had cost zero.
        input_tokens: u64_at(cols, 7),
        output_tokens: u64_at(cols, 8),
        cached_tokens: u64_at(cols, 9),
        cost_micros: opt_i64_at(cols, 10),
        price_currency: opt_str_at(cols, 11),
        input_price_micros_per_million: opt_i64_at(cols, 12),
        output_price_micros_per_million: opt_i64_at(cols, 13),
        cached_price_micros_per_million: opt_i64_at(cols, 14),
        // Version 8: which identity the process started under. Text that is not
        // our JSON — that is, a row written earlier — never drops the row: it
        // becomes `Unrecorded`, which is what that row is.
        engine_identity: opt_str_at(cols, 15)
            .map(|text| EngineIdentity::from_column(&text))
            .unwrap_or_default(),
        retry_chain: retry_chain_at(cols, 16),
        error_type: opt_str_at(cols, 17),
        started_at: i64_at(cols, 18)?,
        ended_at: opt_i64_at(cols, 19),
        // Columns born later sit at the tail, in birth order: an older ledger
        // lacks them, and the row still reads.
        // Version 4:
        total_tokens: u64_at(cols, 20),
        declared_cost_micros: opt_i64_at(cols, 21),
        // Version 5, the written cache — the entry that was missing, and that
        // on one measured call was 96% of the spend:
        cache_write_tokens: u64_at(cols, 22),
        cache_write_long_tokens: u64_at(cols, 23),
        cache_write_price_micros_per_million: opt_i64_at(cols, 24),
        cache_write_long_price_micros_per_million: opt_i64_at(cols, 25),
        // Version 6, the turns: the quantity that explains why a chain of steps
        // costs more than a single session.
        turns: u64_at(cols, 26),
        // Version 7, the session: what lets a step resume instead of
        // rediscovering.
        session_id: opt_str_at(cols, 27),
        work_kind: opt_str_at(cols, 28),
    })
}

fn str_at(cols: &[Value], index: usize) -> Option<String> {
    cols.get(index)?.as_str().map(str::to_owned)
}

fn opt_str_at(cols: &[Value], index: usize) -> Option<String> {
    cols.get(index).and_then(Value::as_str).map(str::to_owned)
}

fn i64_at(cols: &[Value], index: usize) -> Option<i64> {
    cols.get(index)?.as_i64()
}

fn opt_i64_at(cols: &[Value], index: usize) -> Option<i64> {
    cols.get(index).and_then(Value::as_i64)
}

/// A count, when there is one. A NULL, a column that does not exist, or text
/// that is not a number all give `None` — never `0`.
fn u64_at(cols: &[Value], index: usize) -> Option<u64> {
    let value = cols.get(index)?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn retry_chain_at(cols: &[Value], index: usize) -> Vec<String> {
    cols.get(index)
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dump_with_one_run_and_one_call() -> Value {
        json!({
            "runs": [[
                "run-1", "sweep", "marker-sweep", Value::Null, "test", "running",
                1200, Value::Null, 1000, Value::Null
            ]],
            "model_calls": [[
                "call-1", "run-1", "scan_markers", "classify", "claude",
                "sonnet", "claude-sonnet-5", "100", "50", "10", 500, "USD",
                3_000_000, 15_000_000, 300_000,
                "{\"kind\":\"inherited_from_the_terminal\",\"cli_id\":\"claude\"}",
                "[\"call-0\"]", Value::Null, 1001, 1009, Value::Null, Value::Null
            ]],
            "steps": [],
            "snapshots": []
        })
    }

    /// **THE ANCHOR OUTSIDE EVERYTHING THAT READS BY POSITION.** Every index
    /// below is a hand-written number in `parse_model_call_row`, and it shifts
    /// whenever a column is born or dies. While **two** copies read the dump —
    /// one here, one inside `actions` — a moved column stayed green in both:
    /// wrong together, confirming each other. That copy is gone; this measures
    /// what is left against `ledger::MODEL_CALL_DUMP_COLUMNS`, neither of them.
    #[test]
    fn every_position_this_file_reads_is_the_column_the_ledger_dumps() {
        // *Mutant run*: moving one index here turns this test red before a
        // price can appear in place of a token. The anchor was watched firing,
        // not merely believed to fire.
        let dumped: Vec<&str> = ledger::MODEL_CALL_DUMP_COLUMNS.split(',').collect();
        for (index, name) in [
            (0, "call_id"),
            (1, "run_id"),
            (2, "step_id"),
            (3, "purpose"),
            (4, "cli"),
            (5, "requested_model"),
            (6, "actual_model"),
            (7, "input_tokens"),
            (8, "output_tokens"),
            (9, "cached_tokens"),
            (10, "cost_micros"),
            (11, "price_currency"),
            (12, "input_price_micros_per_million"),
            (13, "output_price_micros_per_million"),
            (14, "cached_price_micros_per_million"),
            (15, "engine_identity"),
            (16, "retry_chain"),
            (17, "error_type"),
            (18, "started_at"),
            (19, "ended_at"),
            (20, "total_tokens"),
            (21, "declared_cost_micros"),
            (22, "cache_write_tokens"),
            (23, "cache_write_long_tokens"),
            (24, "cache_write_price_micros_per_million"),
            (25, "cache_write_long_price_micros_per_million"),
            (26, "turns"),
            (27, "session_id"),
            (28, "work_kind"),
        ] {
            assert_eq!(
                dumped.get(index).copied(),
                Some(name),
                "position {index} is no longer «{name}»: this file's indices read another column"
            );
        }
        assert_eq!(
            dumped.len(),
            29,
            "the ledger dumps a column this file never reads"
        );
    }

    #[test]
    fn a_run_row_is_read_by_position_not_by_name() {
        let runs = parse_runs(&dump_with_one_run_and_one_call());
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.run_id, "run-1");
        assert_eq!(run.kind, "sweep");
        assert_eq!(run.parent_run_id, None);
        assert_eq!(run.total_cost_micros, 1200);
        assert_eq!(run.started_at, 1000);
        assert_eq!(run.ended_at, None);
    }

    #[test]
    fn model_call_token_columns_come_back_as_text_and_are_parsed_to_numbers() {
        let calls = parse_model_calls(&dump_with_one_run_and_one_call());
        assert_eq!(calls.len(), 1);
        let call = &calls[0];
        assert_eq!(call.input_tokens, Some(100));
        assert_eq!(call.output_tokens, Some(50));
        assert_eq!(call.cached_tokens, Some(10));
        assert_eq!(call.cost_micros, Some(500));
        assert_eq!(call.retry_chain, vec!["call-0".to_owned()]);
        assert_eq!(call.step_id, Some("scan_markers".to_owned()));
        assert_eq!(call.error_type, None);
    }

    /// The identity column comes back as the shape it carries, never as text.
    #[test]
    fn the_identity_column_comes_back_as_the_shape_it_carries() {
        let calls = parse_model_calls(&dump_with_one_run_and_one_call());
        assert_eq!(
            calls[0].engine_identity,
            EngineIdentity::InheritedFromTheTerminal {
                cli_id: "claude".to_owned()
            }
        );
    }

    /// **AN OLDER ROW NEVER BECOMES A DECLARED PROFILE.** That column held
    /// `<cli>/<profile>`, and the text named the active profile even where the
    /// step had overridden it: promoting it to `ProfileInForce` now would give
    /// an old lie the face of a fresh measurement. The text is kept, the claim
    /// is not.
    #[test]
    fn an_old_identity_column_is_kept_as_text_and_not_promoted() {
        let mut dump = dump_with_one_run_and_one_call();
        dump["model_calls"][0][15] = json!("codex/lavoro");
        let calls = parse_model_calls(&dump);
        assert_eq!(
            calls[0].engine_identity,
            EngineIdentity::Unrecorded {
                legacy: "codex/lavoro".to_owned()
            }
        );
    }

    #[test]
    fn token_columns_also_accept_a_plain_json_number() {
        let mut dump = dump_with_one_run_and_one_call();
        dump["model_calls"][0][7] = json!(100);
        let calls = parse_model_calls(&dump);
        assert_eq!(calls[0].input_tokens, Some(100));
    }

    /// A NULL column comes back `None`, never `Some(0)`. A zero read here would
    /// be summed into the dashboard, and nobody could tell it apart from a call
    /// that really cost nothing.
    #[test]
    fn a_null_token_column_is_unknown_not_zero() {
        let mut dump = dump_with_one_run_and_one_call();
        for column in [7, 8, 9, 10] {
            dump["model_calls"][0][column] = Value::Null;
        }
        let calls = parse_model_calls(&dump);
        assert_eq!(calls.len(), 1, "an unmeasured row stays in the list");
        assert_eq!(calls[0].input_tokens, None);
        assert_eq!(calls[0].output_tokens, None);
        assert_eq!(calls[0].cached_tokens, None);
        assert_eq!(calls[0].cost_micros, None);
    }

    /// The two columns born with projection version 4 are read, and a shorter
    /// dump (a ledger that lacks them yet) never drops the row.
    #[test]
    fn the_two_newest_columns_are_read_and_their_absence_is_not_fatal() {
        let mut dump = dump_with_one_run_and_one_call();
        dump["model_calls"][0][20] = json!("13910");
        dump["model_calls"][0][21] = json!(42_000);
        let calls = parse_model_calls(&dump);
        assert_eq!(calls[0].total_tokens, Some(13_910));
        assert_eq!(calls[0].declared_cost_micros, Some(42_000));

        let mut older = dump_with_one_run_and_one_call();
        older["model_calls"][0].as_array_mut().unwrap().truncate(20);
        let calls = parse_model_calls(&older);
        assert_eq!(calls.len(), 1, "an older dump still reads");
        assert_eq!(calls[0].total_tokens, None);
        assert_eq!(calls[0].declared_cost_micros, None);
    }

    #[test]
    fn a_row_missing_a_required_column_is_skipped_not_panicked_on() {
        let mut dump = dump_with_one_run_and_one_call();
        dump["runs"][0] = json!(["only", "two"]);
        assert_eq!(parse_runs(&dump).len(), 0);
    }

    #[test]
    fn a_missing_table_yields_an_empty_list() {
        let runs = parse_runs(&json!({}));
        assert!(runs.is_empty());
    }
}
