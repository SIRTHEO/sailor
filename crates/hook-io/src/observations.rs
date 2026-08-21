//! La raccolta delle osservazioni: una riga per ogni chiamata a strumento.
//!
//! Porta di `skills/hooks/observe.sh`, che è il gancio più caldo di tutti —
//! gira su `PreToolUse` **e** `PostToolUse` con matcher `*`, quindi due volte
//! per ogni singola chiamata, e dopo il porting dei cinque freni era lui a
//! dettare il muro (30 ms).
//!
//! Non decide niente e non blocca mai: registra e basta. Per questo sta qui
//! accanto al registro dei ganci e non fra le guardie.
//!
//! LA STORIA CHE SPIEGA LA FORMA. La versione precedente avviava **tre**
//! processi Python per fase — uno per interpretare l'ingresso, uno per
//! rileggere il proprio esito, uno per scrivere la riga — più `du` e `date`:
//! 142,6 ms per chiamata su un gancio che non prende nessuna decisione. Ridotti
//! a uno solo il 12/08/2026. Qui diventano zero.
//!
//! Il formato dei record non cambia: stessi campi, stesso ordine, stesso file.

use crate::journal::Field;
use serde_json::Value;
use std::fs::{create_dir_all, rename, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const MAX_BODY: usize = 5000;
const MAX_ERROR: usize = 300;

fn folder() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    home.join(".claude").join("homunculus")
}

/// Quale fase: `pre` produce `tool_start`, `permission` la richiesta di
/// permesso, tutto il resto `tool_complete`.
///
/// LA FASE DEL PERMESSO HA UN NOME PROPRIO, e non è cosmesi. Con due sole fasi
/// una richiesta di permesso finiva registrata come `tool_complete`, cioè
/// **indistinguibile dal traffico normale** — e il file ne conta a migliaia.
/// Chi poi contasse «quanti prompt di permesso e per quale comando» leggerebbe
/// zero: non perché i prompt manchino, ma perché nessuno li riconosce. Misurato
/// il 21/08/2026 dando in pasto al binario in servizio un payload di
/// `PermissionRequest` vero.
/// IL NOME DELL'EVENTO VALE QUANTO LA PAROLA CORTA, e non è tolleranza gratuita:
/// il cablaggio di questa fase non è ancora stato scritto, e l'altro gancio che
/// riceve una fase (`comment-refs`) passa `PreToolUse`/`PostToolUse` così come
/// arrivano. Chi cablerà seguendo quello schema scriverebbe `PermissionRequest`,
/// cadrebbe nel ripiego qui sotto **senza un errore**, e il registro conterebbe
/// zero prompt: un silenzio che si legge come «i prompt non ci sono».
pub fn event_name(phase: &str) -> &'static str {
    match phase {
        "pre" | "PreToolUse" => "tool_start",
        "permission" | "PermissionRequest" => "permission_request",
        _ => "tool_complete",
    }
}

/// Se la chiamata è andata bene. È il segnale che sblocca i modi «comando
/// fallito → comando che ha funzionato», quindi vale la pena guardarlo bene.
pub fn succeeded(response: &Value) -> bool {
    if let Some(map) = response.as_object() {
        if truthy(map.get("is_error")) || truthy(map.get("isError")) {
            return false;
        }
        for key in ["error", "stderr"] {
            if truthy(map.get(key)) {
                return false;
            }
        }
        if truthy(map.get("interrupted")) {
            return false;
        }
        return true;
    }
    if let Some(text) = response.as_str() {
        if text.trim().is_empty() {
            return true;
        }
        let head: String = text.chars().take(400).collect::<String>().to_lowercase();
        if head.starts_with("error")
            || head.contains("is not allowed")
            || head.contains("permission denied")
            || head.contains("command not found")
        {
            return false;
        }
    }
    true
}

/// La verità di Python: `""`, `0`, `[]`, `{}` e `null` sono falsi. Il default
/// di `serde_json` direbbe che la stringa vuota è un valore, e un `stderr: ""`
/// — che è il caso normale di un comando riuscito — diventerebbe un errore.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Il `tronca()` del Python: una stringa resta sé stessa, tutto il resto passa
/// da `json.dumps`. **L'ordine conta**: si serializza e poi si taglia, quindi
/// la lunghezza dipende dalla serializzazione — con `ensure_ascii` un accento
/// occupa sei caratteri e il taglio arriva molto prima.
fn truncate(v: &Value, max: usize) -> String {
    let text = match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => crate::python_json::dumps(other),
    };
    text.chars().take(max).collect()
}

/// Quale corpo porta la riga: l'ingresso o l'esito, mai tutti e due.
///
/// Il corpo non si registra su entrambe le fasi: gonfierebbe il file di un
/// centinaio di megabyte al mese senza aggiungere niente.
///
/// LA RICHIESTA DI PERMESSO PORTA L'INGRESSO, NON L'ESITO, e senza quello la
/// riga non serve a niente: l'esito lì è vuoto per forza — il permesso non è
/// ancora stato dato — mentre la domanda che si vuole contare è **quale
/// comando** ha fatto comparire il prompt. Misurato il 21/08/2026: la riga
/// usciva con l'esito vuoto e il comando da nessuna parte.
///
/// Sta fuori da `record` per una ragione sola: `record` scrive su disco sotto
/// `HOME` e una prova che lo chiami dovrebbe spostare `HOME` a tutti i casi che
/// girano insieme. Così la scelta si prova per quello che è — una decisione su
/// due valori — e il caso che la copre può diventare rosso.
fn body_field(event: &str, input: &Value, output: &Value) -> (&'static str, Field) {
    if event == "tool_start" || event == "permission_request" {
        ("input", Field::Text(truncate(input, MAX_BODY)))
    } else {
        ("output", Field::Text(truncate(output, MAX_BODY)))
    }
}

/// Registra la chiamata. Un ingresso illeggibile non si perde: si scrive come
/// `parse_error`, perché un buco nella raccolta è indistinguibile da un periodo
/// in cui non è successo niente.
pub fn record(phase: &str, raw: &str) {
    if raw.trim().is_empty() {
        return;
    }
    let dir = folder();
    if dir.join("disabled").exists() {
        return; // l'interruttore: spegne la raccolta senza toccare i ganci
    }
    let event = event_name(phase);
    let when = crate::journal::now_iso8601_seconds();

    let Ok(data) = serde_json::from_str::<Value>(raw) else {
        write_line(
            &dir,
            &[
                ("timestamp", Field::Text(when)),
                ("event", Field::Text("parse_error".into())),
                ("raw", Field::Text(raw.chars().take(2000).collect())),
            ],
        );
        return;
    };

    let get = |a: &str, b: &str| data.get(a).or_else(|| data.get(b)).cloned();
    let tool = get("tool_name", "tool")
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into());
    let input = get("tool_input", "input").unwrap_or(Value::Object(Default::default()));
    // Claude Code passa il risultato in `tool_response`: leggere `tool_output`
    // dava sempre stringa vuota, ed è il motivo per cui in un mese di raccolta
    // (128.000 chiamate) nessun `tool_complete` ha mai avuto un esito.
    let output = data
        .get("tool_response")
        .or_else(|| data.get("tool_output"))
        .or_else(|| data.get("output"))
        .cloned()
        .unwrap_or(Value::String(String::new()));
    let session = data
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let ok = if event == "tool_complete" {
        Some(succeeded(&output))
    } else {
        None
    };

    let mut fields = vec![
        ("timestamp", Field::Text(when)),
        ("event", Field::Text(event.into())),
        ("tool", Field::Text(tool)),
        ("session", Field::Text(session)),
    ];
    fields.push(body_field(event, &input, &output));
    if let Some(ok) = ok {
        fields.push(("ok", Field::Bool(ok)));
    }
    if ok == Some(false) {
        // Dell'errore si tiene un moncone: basta a riconoscere la famiglia.
        fields.push(("err", Field::Text(truncate(&output, MAX_ERROR))));
    }
    write_line(&dir, &fields);
}

fn write_line(dir: &std::path::Path, fields: &[(&str, Field)]) {
    if create_dir_all(dir).is_err() {
        return;
    }
    let path = dir.join("observations.jsonl");
    rotate(&path, dir);

    let mut line = String::with_capacity(256);
    line.push('{');
    for (i, (key, value)) in fields.iter().enumerate() {
        if i > 0 {
            line.push_str(", ");
        }
        line.push_str(&serde_json::to_string(key).unwrap_or_default());
        line.push_str(": ");
        value.write_to(&mut line);
    }
    line.push_str("}\n");

    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn rotate(path: &std::path::Path, dir: &std::path::Path) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() < MAX_BYTES {
        return;
    }
    let archive = dir.join("observations.archive");
    if create_dir_all(&archive).is_err() {
        return;
    }
    let stamp = crate::journal::now_iso8601_seconds()
        .replace(['-', ':'], "")
        .replace('T', "-");
    let stamp = stamp.trim_end_matches('Z').to_string();
    let _ = rename(path, archive.join(format!("observations-{stamp}.jsonl")));
}

/// Sveglia l'osservatore, se è in ascolto. Nessun processo in più quando il
/// file del pid non c'è, che è il caso normale.
pub fn wake_observer() {
    let pid_file = folder().join(".observer.pid");
    let Ok(text) = std::fs::read_to_string(&pid_file) else {
        return;
    };
    let pid = text.trim();
    if pid.is_empty() {
        return;
    }
    let _ = std::process::Command::new("kill")
        .args(["-USR1", pid])
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_phase_decides_the_event_name() {
        assert_eq!(event_name("pre"), "tool_start");
        assert_eq!(event_name("post"), "tool_complete");
        assert_eq!(event_name(""), "tool_complete");
    }

    #[test]
    fn an_empty_stderr_is_a_success_not_a_failure() {
        // il caso normale di un comando riuscito, e quello su cui una verità
        // non-Python avrebbe sbagliato
        assert!(succeeded(&json!({"stdout": "ciao", "stderr": ""})));
        assert!(succeeded(&json!({})));
    }

    #[test]
    fn it_reads_the_failure_from_any_of_the_flags() {
        assert!(!succeeded(&json!({"is_error": true})));
        assert!(!succeeded(&json!({"isError": true})));
        assert!(!succeeded(&json!({"stderr": "boom"})));
        assert!(!succeeded(&json!({"error": "boom"})));
        assert!(!succeeded(&json!({"interrupted": true})));
    }

    #[test]
    fn a_string_response_is_judged_on_how_it_starts() {
        assert!(!succeeded(&json!("Error: no such file")));
        assert!(!succeeded(&json!("bash: command not found")));
        assert!(!succeeded(&json!("zsh: permission denied")));
        assert!(succeeded(&json!("tutto bene")));
        assert!(succeeded(&json!("")));
    }

    #[test]
    fn it_truncates_on_characters_not_bytes() {
        let accented = Value::String("è".repeat(10));
        assert_eq!(truncate(&accented, 4).chars().count(), 4);
    }

    /// La fase del permesso ha un nome suo, e le altre due non cambiano.
    ///
    /// MUTANTE: rimettendo `permission` nel ramo di scarto, la sua riga torna a
    /// chiamarsi `tool_complete` e questo caso va rosso. Senza, una richiesta di
    /// permesso resta indistinguibile dal traffico normale e chi conta i prompt
    /// legge zero.
    #[test]
    fn a_permission_request_is_not_a_completed_tool() {
        assert_eq!(event_name("permission"), "permission_request");
        assert_eq!(event_name("pre"), "tool_start");
        assert_eq!(event_name("post"), "tool_complete");
        // Una fase che nessuno conosce resta il completamento, com'era.
        assert_eq!(event_name("qualunque-altra"), "tool_complete");
    }

    /// Il nome dell'evento vale quanto la parola corta.
    ///
    /// MUTANTE: togliendo `PermissionRequest` dal suo ramo, la riga torna a
    /// chiamarsi `tool_complete` e questo caso va rosso. Serve perché il
    /// cablaggio di questa fase non è ancora scritto, e l'altro gancio che
    /// riceve una fase passa il nome dell'evento tale e quale: chi cablasse così
    /// avrebbe un registro muto **senza nessun errore**.
    #[test]
    fn the_event_name_works_as_well_as_the_short_word() {
        assert_eq!(event_name("PermissionRequest"), "permission_request");
        assert_eq!(event_name("PreToolUse"), "tool_start");
        assert_eq!(event_name("PostToolUse"), "tool_complete");
    }

    /// La riga del permesso porta il comando, quella del completamento l'esito.
    ///
    /// MUTANTE: togliendo `|| event == "permission_request"` da `body_field`, la
    /// richiesta di permesso finisce nel ramo dell'esito — che lì è vuoto per
    /// forza — e questo caso va rosso. Senza di lui quella riga non era coperta
    /// da niente: si poteva disfare la metà utile della modifica e la batteria
    /// restava tutta verde.
    #[test]
    fn the_permission_line_carries_the_command_not_the_outcome() {
        let input = json!({"command": "scw instance server list"});
        let output = json!("ok");

        let (name, value) = body_field("permission_request", &input, &output);
        assert_eq!(name, "input");
        let mut written = String::new();
        value.write_to(&mut written);
        assert!(
            written.contains("scw instance server list"),
            "il comando deve stare nella riga: {written}"
        );

        assert_eq!(body_field("tool_complete", &input, &output).0, "output");
        assert_eq!(body_field("tool_start", &input, &output).0, "input");
    }
}
