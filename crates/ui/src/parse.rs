//! Interpreta l'uscita di `Ledger::projection_dump`: ogni riga è un array di
//! colonne, nell'ordine fissato da `ledger::dump_table`. Pura — prende un
//! `serde_json::Value` già in memoria, mai un file — perché è l'unico modo
//! pubblico per leggere `runs` e `model_calls` senza conoscerne già i
//! `run_id`. I token nel deposito sono colonne di tipo testo (per non
//! perdere precisione oltre 2^53), quindi qui si accetta sia stringa sia
//! numero.

use ledger::{ModelCallRecord, RunRecord};
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
        // Da qui in giù una colonna NULL è un «non lo so», non un valore
        // assente da un record malformato: si legge come `None` invece di far
        // cadere l'intera riga con `?`. La differenza conta — una chiamata non
        // misurata deve comparire nell'elenco, altrimenti sparisce dai conti
        // proprio come se fosse costata zero.
        input_tokens: u64_at(cols, 7),
        output_tokens: u64_at(cols, 8),
        cached_tokens: u64_at(cols, 9),
        cost_micros: opt_i64_at(cols, 10),
        price_currency: opt_str_at(cols, 11),
        input_price_micros_per_million: opt_i64_at(cols, 12),
        output_price_micros_per_million: opt_i64_at(cols, 13),
        cached_price_micros_per_million: opt_i64_at(cols, 14),
        mandate_name: str_at(cols, 15)?,
        mandate_version: str_at(cols, 16)?,
        retry_chain: retry_chain_at(cols, 17),
        error_type: opt_str_at(cols, 18),
        started_at: i64_at(cols, 19)?,
        ended_at: opt_i64_at(cols, 20),
        // Le colonne nate dopo stanno in coda, nell'ordine in cui sono nate: un
        // deposito più vecchio non le ha, e la riga si legge lo stesso.
        // Versione 4:
        total_tokens: u64_at(cols, 21),
        declared_cost_micros: opt_i64_at(cols, 22),
        // Versione 5, la cache scritta — la voce che mancava e che su una
        // chiamata misurata valeva il 96% della spesa:
        cache_write_tokens: u64_at(cols, 23),
        cache_write_long_tokens: u64_at(cols, 24),
        cache_write_price_micros_per_million: opt_i64_at(cols, 25),
        cache_write_long_price_micros_per_million: opt_i64_at(cols, 26),
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

/// Un conteggio, se c'è. Un NULL, una colonna che non esiste, o un testo che
/// non è un numero danno `None` — mai `0`.
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
                "run-1", "sweep", "marker-sweep", Value::Null, "prova", "running",
                1200, Value::Null, 1000, Value::Null
            ]],
            "model_calls": [[
                "call-1", "run-1", "scan_markers", "classifica", "claude",
                "sonnet", "claude-sonnet-5", "100", "50", "10", 500, "USD",
                3_000_000, 15_000_000, 300_000, "prova", "1",
                "[\"call-0\"]", Value::Null, 1001, 1009, Value::Null, Value::Null
            ]],
            "steps": [],
            "snapshots": []
        })
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

    #[test]
    fn token_columns_also_accept_a_plain_json_number() {
        let mut dump = dump_with_one_run_and_one_call();
        dump["model_calls"][0][7] = json!(100);
        let calls = parse_model_calls(&dump);
        assert_eq!(calls[0].input_tokens, Some(100));
    }

    /// IL BRACCIO CHE CONTA per il criterio 4 del mandato: una colonna NULL
    /// torna `None`, mai `Some(0)`. Uno zero letto qui si sommerebbe nel
    /// cruscotto e nessuno potrebbe più distinguerlo da una chiamata gratuita.
    #[test]
    fn a_null_token_column_is_unknown_not_zero() {
        let mut dump = dump_with_one_run_and_one_call();
        for column in [7, 8, 9, 10] {
            dump["model_calls"][0][column] = Value::Null;
        }
        let calls = parse_model_calls(&dump);
        assert_eq!(calls.len(), 1, "una riga non misurata resta nell'elenco");
        assert_eq!(calls[0].input_tokens, None);
        assert_eq!(calls[0].output_tokens, None);
        assert_eq!(calls[0].cached_tokens, None);
        assert_eq!(calls[0].cost_micros, None);
    }

    /// Le due colonne nate con la versione 4 della proiezione si leggono, e un
    /// dump più corto (un deposito che non le ha ancora) non fa cadere la riga.
    #[test]
    fn the_two_newest_columns_are_read_and_their_absence_is_not_fatal() {
        let mut dump = dump_with_one_run_and_one_call();
        dump["model_calls"][0][21] = json!("13910");
        dump["model_calls"][0][22] = json!(42_000);
        let calls = parse_model_calls(&dump);
        assert_eq!(calls[0].total_tokens, Some(13_910));
        assert_eq!(calls[0].declared_cost_micros, Some(42_000));

        let mut older = dump_with_one_run_and_one_call();
        older["model_calls"][0].as_array_mut().unwrap().truncate(21);
        let calls = parse_model_calls(&older);
        assert_eq!(calls.len(), 1, "un dump più vecchio si legge lo stesso");
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
