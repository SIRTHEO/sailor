//! Scrive un deposito finto per guardare la pagina senza aspettare che un
//! flusso vero giri. Uso: `cargo run --example seed_fake_ledger -- CARTELLA`.

use flow::{Completion, Outcome, StepRecord};
use ledger::{Ledger, ModelCallRecord, RunRecord};
use serde_json::json;

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| panic!("uso: seed_fake_ledger CARTELLA"));
    let ledger = Ledger::open(&dir).expect("apertura del deposito");

    ledger
        .record_run(&RunRecord {
            run_id: "marker-sweep-demo-1".into(),
            kind: "sweep".into(),
            entity: "marker-sweep".into(),
            parent_run_id: None,
            started_by: "prova-manuale".into(),
            status: "succeeded".into(),
            total_cost_micros: 1_250_000,
            error: None,
            started_at: 1_756_000_000,
            ended_at: Some(1_756_000_042),
        })
        .expect("registrare la corsa conclusa");
    close_step(&ledger, "marker-sweep-demo-1", "scan_markers", 1_756_000_000, 1_756_000_010);
    close_step(&ledger, "marker-sweep-demo-1", "classify_standard", 1_756_000_010, 1_756_000_020);
    close_step(&ledger, "marker-sweep-demo-1", "plan_removals", 1_756_000_020, 1_756_000_042);
    ledger
        .record_model_call(&fake_call("marker-sweep-demo-1", "classify_standard", "claude-sonnet-5", 40_000, 900))
        .expect("registrare la chiamata");

    ledger
        .record_run(&RunRecord {
            run_id: "marker-sweep-demo-2".into(),
            kind: "sweep".into(),
            entity: "marker-sweep".into(),
            parent_run_id: None,
            started_by: "prova-manuale".into(),
            status: "running".into(),
            total_cost_micros: 300_000,
            error: None,
            started_at: 1_756_000_100,
            ended_at: None,
        })
        .expect("registrare la corsa in corso");
    close_step(&ledger, "marker-sweep-demo-2", "scan_markers", 1_756_000_100, 1_756_000_105);
    ledger
        .append_step_started(&StepRecord::started(
            "marker-sweep-demo-2",
            "classify_standard",
            1,
            1,
            vec!["scan_markers".into()],
            json!({}),
            vec![],
            1_756_000_105,
        ))
        .expect("passo lasciato aperto di proposito");
    ledger
        .record_model_call(&fake_call("marker-sweep-demo-2", "classify_standard", "claude-haiku-5", 12_000, 220))
        .expect("registrare la chiamata");

    println!("deposito finto scritto in {dir}");
}

fn close_step(ledger: &Ledger, run_id: &str, step_id: &str, started_at: i64, ended_at: i64) {
    ledger
        .append_step_started(&StepRecord::started(
            run_id,
            step_id,
            1,
            1,
            vec![],
            json!({}),
            vec![],
            started_at,
        ))
        .expect("passo avviato");
    ledger
        .close_step(
            run_id,
            step_id,
            1,
            1,
            Completion {
                outcome: Outcome::Went,
                output: Some(json!({"ok": true})),
                said: None,
                failure_class: None,
                ended_at,
                bytes_seen: None,
                bytes_discarded: None,
            },
        )
        .expect("passo chiuso");
}

/// Una chiamata **inventata**, e marcata come tale in ogni riga che produce.
///
/// **PERCHÉ LA MARCATURA È OBBLIGATORIA.** Fino al 29/08/2026 questo esempio
/// era l'unico scrittore di `model_calls` insieme alle prove: il cruscotto
/// sommava un costo per modello, e quel costo era interamente finzione. Un
/// cruscotto alimentato da dati finti sembra funzionare, ed è peggio del non
/// averlo — perché chi lo guarda crede di sapere quanto ha speso. Adesso il
/// deposito lo riempie il motore vero; questo esempio resta per provare la
/// pagina senza spendere, e ogni riga che scrive dice a chiare lettere di non
/// essere una misura.
fn fake_call(run_id: &str, step_id: &str, model: &str, input_tokens: u64, cost_micros: i64) -> ModelCallRecord {
    ModelCallRecord {
        call_id: format!("call-{run_id}-{step_id}"),
        run_id: run_id.to_owned(),
        step_id: Some(step_id.to_owned()),
        purpose: "FINTO — seminato da seed_fake_ledger, non è una misura".into(),
        cli: "claude".into(),
        requested_model: "sonnet".into(),
        actual_model: model.to_owned(),
        input_tokens: Some(input_tokens),
        output_tokens: Some(input_tokens / 8),
        cached_tokens: Some(input_tokens / 4),
        cache_write_tokens: None,
        cache_write_long_tokens: None,
        total_tokens: None,
        turns: None,
        cost_micros: Some(cost_micros),
        declared_cost_micros: None,
        price_currency: Some("USD".into()),
        input_price_micros_per_million: Some(3_000_000),
        output_price_micros_per_million: Some(15_000_000),
        cached_price_micros_per_million: Some(300_000),
        cache_write_price_micros_per_million: None,
        cache_write_long_price_micros_per_million: None,
        mandate_name: "prova-manuale".into(),
        mandate_version: "1".into(),
        retry_chain: vec![],
        error_type: None,
        started_at: 1_756_000_001,
        ended_at: Some(1_756_000_009),
        session_id: None,
    }
}
