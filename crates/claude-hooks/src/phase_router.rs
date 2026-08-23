//! Il gancio `PreToolUse` su `Agent` che riscrive il modello dal mestiere.
//!
//! Il giudizio sta in `guards::phase_router`; qui c'è ciò che tocca il mondo:
//! il payload su stdin, la riscrittura di `tool_input` in `updatedInput` e il
//! registro — una riga per ogni decisione, anche quella che lascia tutto
//! com'è (senza il negativo il registro non direbbe quanto morde davvero).
//!
//! NON NEGA MAI, NON BLOCCA MAI: o riscrive `model`, o non stampa niente. Un
//! `PreToolUse` che nega qui vieterebbe di lanciare il subagent, non solo di
//! scegliergli il modello — il danno peggiore che questo gancio può fare.
//! Fail-open su stdin illeggibile o fuori contesto, come ogni altro gancio.

use guards::phase_router::{route_with, Routing, Row, TRADE_MODEL};
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};

use crate::handoff::state_dir;

const CEILING_BYTES: u64 = 5 * 1024 * 1024;

/// Una riga in `ganci.jsonl`: mestiere, modello scelto (o `null`), motivo,
/// sessione se c'è. Formato compatto, come i ganci nati in Rust senza un
/// originale Python da ricopiare byte per byte.
fn log_routing(routing: &Routing, session: Option<&str>) {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "t".into(),
        Value::String(hook_io::journal::now_iso8601_python()),
    );
    obj.insert("gancio".into(), Value::String("phase-router".into()));
    obj.insert("mestiere".into(), Value::String(routing.trade.clone()));
    obj.insert(
        "modello".into(),
        routing
            .model
            .map(|m| Value::String(m.to_string()))
            .unwrap_or(Value::Null),
    );
    obj.insert("motivo".into(), Value::String(routing.reason.to_string()));
    if let Some(s) = session.filter(|s| !s.is_empty()) {
        obj.insert("session".into(), Value::String(s.chars().take(8).collect()));
    }
    let line = Value::Object(obj).to_string();

    let dir = state_dir();
    let path = dir.join("ganci.jsonl");
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > CEILING_BYTES {
            let _ = fs::rename(&path, path.with_extension("jsonl.1"));
        }
    }
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

/// Dal payload grezzo alla riscrittura da stampare, se c'è. Registra ogni
/// decisione tranne quella della valvola, che è pensata per non lasciare
/// traccia — come `Mode::Off` altrove in questa configurazione, dove un
/// gancio spento non tocca né stato né registro. Isolata da stdin/stdout per
/// potersi provare due volte di fila senza toccare il terminale.
fn process_with(table: &[Row], payload: &Value) -> Option<Value> {
    if payload.get("tool_name").and_then(|v| v.as_str()) != Some("Agent") {
        return None; // invocato fuori dal proprio matcher: non è affar suo
    }
    let tool_input = payload.get("tool_input").filter(|v| v.is_object())?;
    let trade = tool_input.get("subagent_type").and_then(|v| v.as_str());
    let model = tool_input.get("model").and_then(|v| v.as_str());
    let prompt = tool_input
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let valve_off = hook_io::Mode::from_env("PHASE_ROUTER") == hook_io::Mode::Off;

    let routing = route_with(table, trade, model, prompt, valve_off);
    if valve_off {
        return None; // la valvola lascia passare tutto senza toccare il registro
    }
    let session = payload.get("session_id").and_then(|v| v.as_str());
    log_routing(&routing, session);

    let chosen = routing.model?;
    let mut rewritten = tool_input.clone();
    rewritten
        .as_object_mut()
        .expect("filtrato sopra: e' un oggetto")
        .insert("model".to_string(), Value::String(chosen.to_string()));
    Some(serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "updatedInput": rewritten,
        }
    }))
}

pub fn run() -> i32 {
    run_from(TRADE_MODEL, &mut std::io::stdin(), &mut std::io::stdout())
}

/// Lo stesso giro di `run`, con tabella, stdin e stdout passati da fuori: è
/// l'unico modo di provare «non nega mai» su un payload rotto senza un processo.
fn run_from(table: &[Row], input: &mut dyn Read, output: &mut dyn Write) -> i32 {
    let mut raw = String::new();
    if input.read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
        return 0;
    };
    // Chi legge stdin da sé deve dichiararlo, o ogni sua riga di registro esce
    // marcata come prova.
    hook_io::mark_live_from_payload(&payload);
    if let Some(out) = process_with(table, &payload) {
        let _ = writeln!(output, "{out}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;
    use guards::phase_router::CANDIDATE_TABLE;

    fn agent_payload(trade: &str, model: Option<&str>, prompt: &str) -> Value {
        let mut tool_input = serde_json::json!({
            "subagent_type": trade,
            "prompt": prompt,
        });
        if let Some(m) = model {
            tool_input["model"] = Value::String(m.to_string());
        }
        serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Agent",
            "tool_input": tool_input,
            "session_id": "abcdef01-2345",
        })
    }

    fn hook_journal_lines(home: &HomeIsolata) -> Vec<Value> {
        fs::read_to_string(home.stato().join("ganci.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// Un `measurer` senza `model` viene riscritto su haiku, e la riga finisce
    /// nel registro con il mestiere e il modello scelto.
    #[test]
    fn a_measurer_without_a_model_is_rewritten_to_haiku() {
        let home = HomeIsolata::nuova("phase-router-riscrive");
        let out = process_with(
            CANDIDATE_TABLE,
            &agent_payload("measurer", None, "conta i file"),
        )
        .expect("doveva riscrivere");
        assert_eq!(out["hookSpecificOutput"]["updatedInput"]["model"], "haiku");
        assert_eq!(
            out["hookSpecificOutput"]["updatedInput"]["subagent_type"],
            "measurer"
        );
        let lines = hook_journal_lines(&home);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["gancio"], "phase-router");
        assert_eq!(lines[0]["mestiere"], "measurer");
        assert_eq!(lines[0]["modello"], "haiku");
    }

    /// Un mestiere fuori tabella non produce nessun output, ma la riga «non
    /// riscrivo» resta nel registro con `modello: null`.
    #[test]
    fn a_builder_is_passed_through_but_still_logged() {
        let home = HomeIsolata::nuova("phase-router-passa");
        assert_eq!(
            process_with(
                CANDIDATE_TABLE,
                &agent_payload("builder", None, "scrivi il codice")
            ),
            None
        );
        let lines = hook_journal_lines(&home);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["mestiere"], "builder");
        assert!(lines[0]["modello"].is_null());
    }

    /// `model` già scritto non si tocca: nessun output.
    #[test]
    fn an_explicit_model_produces_no_output() {
        let _home = HomeIsolata::nuova("phase-router-esplicito");
        assert_eq!(
            process_with(
                CANDIDATE_TABLE,
                &agent_payload("measurer", Some("opus"), "conta i file")
            ),
            None
        );
    }

    /// La valvola spegne tutto: nessuna riscrittura, nessuna riga.
    #[test]
    fn the_valve_silences_the_hook_entirely() {
        let home = HomeIsolata::nuova("phase-router-valvola");
        std::env::set_var("PHASE_ROUTER", "off");
        let outcome = process_with(
            CANDIDATE_TABLE,
            &agent_payload("measurer", None, "conta i file"),
        );
        std::env::remove_var("PHASE_ROUTER");
        assert_eq!(outcome, None);
        assert!(!home.stato().join("ganci.jsonl").exists());
    }

    /// L'invariante che conta di più: su qualunque stdin rotto il gancio esce
    /// 0 e non stampa niente — mai un `deny`, mai un panico. Un lettore che
    /// fallisce copre anche lo stdin chiuso male.
    #[test]
    fn a_broken_stdin_never_denies() {
        let _home = HomeIsolata::nuova("phase-router-stdin-rotto");
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("stdin chiuso"))
            }
        }
        let mut out = Vec::new();
        assert_eq!(run_from(CANDIDATE_TABLE, &mut Broken, &mut out), 0);
        for raw in [
            "",
            "non è json",
            "[1,2]",
            "\"stringa\"",
            r#"{"tool_name":"Agent"}"#,
            r#"{"tool_name":"Agent","tool_input":"non un oggetto"}"#,
            r#"{"tool_name":"Agent","tool_input":{"subagent_type":7,"model":[],"prompt":null}}"#,
        ] {
            let mut out = Vec::new();
            assert_eq!(
                run_from(CANDIDATE_TABLE, &mut raw.as_bytes(), &mut out),
                0,
                "{raw:?}"
            );
            assert!(out.is_empty(), "{raw:?} non doveva stampare: {out:?}");
        }
        // Il caso sano, dallo stesso ingresso: stampa la riscrittura e basta.
        let ok = agent_payload("measurer", None, "conta i file").to_string();
        let mut out = Vec::new();
        assert_eq!(run_from(CANDIDATE_TABLE, &mut ok.as_bytes(), &mut out), 0);
        let printed: Value = serde_json::from_slice(&out).expect("una riga JSON");
        assert_eq!(
            printed["hookSpecificOutput"]["updatedInput"]["model"],
            "haiku"
        );
    }

    /// Uno strumento diverso da `Agent` non è affar suo: nessun output, nessuna
    /// riga — è il caso di un gancio invocato fuori dal proprio matcher.
    #[test]
    fn a_non_agent_tool_is_ignored_and_not_logged() {
        let home = HomeIsolata::nuova("phase-router-altro-strumento");
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "echo ciao"},
        });
        assert_eq!(process_with(CANDIDATE_TABLE, &payload), None);
        assert!(!home.stato().join("ganci.jsonl").exists());
    }
}
