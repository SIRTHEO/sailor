//! Il gancio `PermissionRequest` che concede i messaggi fra sessioni locali.
//!
//! Porto di `skills/hooks/allow-session-messages.py`. Il giudizio sta in
//! `guards::session_messages`; qui c'e' cio' che tocca il mondo: il payload su
//! stdin, la domanda a Orca, la riga di registro, la risposta.
//!
//! IL PROBLEMA CHE CHIUDE, misurato il 18/08/2026. Due sessioni sullo stesso
//! ambito non possono avvisarsi: `SendMessage` chiede il permesso e la domanda
//! resta ferma finche' una persona non passa davanti allo schermo. Quel giorno
//! un messaggio e' arrivato molto dopo, e nel frattempo chi lo aspettava aveva
//! riprogettato casi di prova che esistevano gia'.
//!
//! NON APRE UNA PORTA AI PERMESSI. Concede l'invio di un testo, non cio' che il
//! testo chiede: chi riceve resta soggetto ai propri permessi, e un messaggio da
//! un pari non e' l'approvazione dell'utente.
//!
//! LE DUE USCITE NON HANNO LO STESSO ESCAPE, e non e' una svista: l'originale
//! scrive registro e risposta con `ensure_ascii=False`, quindi gli accenti
//! restano leggibili. Il confronto e' byte a byte, e con il default di serde
//! divergerebbe su ogni riga che contiene una parola italiana.
//!
//! Valvola: `MESSAGGI_FRA_SESSIONI=off`.

use crate::json_tool::python_json_string_with;
use guards::session_messages::is_local;
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;

const ORCA: &str = "/usr/local/bin/orca";

/// Il percorso di `orca`, sovrascrivibile solo per il confronto d'equivalenza.
///
/// L'originale ha il percorso scritto in chiaro, quindi non si puo' spostare da
/// fuori: `tools/compare-session-messages.py` ne fa una copia temporanea con
/// quella riga sostituita, e da questo lato serve la variabile. Esiste per
/// quello e per niente altro — in esercizio non e' mai impostata.
fn orca_bin() -> String {
    std::env::var("CLAUDE_ORCA_BIN").unwrap_or_else(|_| ORCA.to_string())
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn log_path() -> PathBuf {
    home().join(".claude").join("state").join("messaggi-concessi.jsonl")
}

/// Cosa Python considera vero. Serve solo a riprodurre il ramo in cui
/// l'originale muore: `if not destinatario` lascia passare `3`, ma non `0`.
fn python_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn is_off() -> bool {
    std::env::var("MESSAGGI_FRA_SESSIONI")
        .map(|v| v.to_lowercase() == "off")
        .unwrap_or(false)
}

/// I nomi delle copie di lavoro che Orca conosce. `None` se non risponde.
///
/// `None` non e' un insieme vuoto: e' «non si sa», e un gancio che concede non
/// deve concedere al buio. Ogni copia entra con tre nomi possibili — quello
/// visualizzato, quello interno, e l'ultimo pezzo del percorso — perche' e' da
/// uno qualunque dei tre che una sessione prende il proprio.
fn registered_copies() -> Option<BTreeSet<String>> {
    let out = Command::new(orca_bin())
        .args(["worktree", "list", "--json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let d: Value = serde_json::from_slice(&out.stdout).ok()?;
    let root = d.get("result").unwrap_or(&d);
    let items = root.get("worktrees")?.as_array()?;
    let mut names = BTreeSet::new();
    for w in items {
        for key in ["displayName", "name"] {
            if let Some(v) = w.get(key).and_then(|v| v.as_str()) {
                let v = v.trim();
                if !v.is_empty() {
                    names.insert(v.to_string());
                }
            }
        }
        if let Some(p) = w.get("path").and_then(|v| v.as_str()) {
            let p = p.trim_end_matches('/');
            if let Some(base) = p.rsplit('/').next() {
                if !base.is_empty() {
                    names.insert(base.to_string());
                }
            }
        }
    }
    Some(names)
}

/// Una riga per concessione. Senza, «il gancio e' partito» resta un'impressione
/// e una concessione sbagliata non si ritrova piu'.
///
/// IL TESTO NON SI REGISTRA, e basta la misura: un messaggio fra sessioni puo'
/// contenere qualunque cosa, e un registro non e' il posto dove farla finire.
fn record(recipient: &str, reason: &str, chars: usize) {
    let Some(dir) = log_path().parent().map(|p| p.to_path_buf()) else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let line = format!(
        "{{\"t\": {}, \"gancio\": {}, \"destinatario\": {}, \"motivo\": {}, \"caratteri\": {}}}\n",
        python_json_string_with(&hook_io::local_time::now_local_iso8601(), false),
        python_json_string_with("allow-session-messages", false),
        python_json_string_with(recipient, false),
        python_json_string_with(reason, false),
        chars
    );
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path()) {
        let _ = f.write_all(line.as_bytes());
    }
}

pub fn run() -> i32 {
    if is_off() {
        return 0;
    }
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
        return 0;
    };
    let Some(payload) = payload.as_object() else {
        return 0;
    };
    if payload.get("tool_name").and_then(|v| v.as_str()) != Some("SendMessage") {
        return 0;
    }
    let Some(input) = payload.get("tool_input").and_then(|v| v.as_object()) else {
        return 0;
    };
    // UN DIFETTO DELL'ORIGINALE, RIPRODOTTO. Un `to` che c'e' ma non e' testo
    // (`{"to": 3}`) supera il controllo `if not destinatario`, non sta fra i
    // destinatari interni, e arriva a `destinatario.split(" [")`: li' Python
    // muore di `AttributeError`, cioe' uscita 1 e niente su stdout. Un `to`
    // falso secondo Python — `0`, `[]`, `""` — passa invece per «destinatario
    // assente» e l'uscita resta 0. Il confronto ha trovato proprio questo caso,
    // ed e' l'unica differenza che restava su 24.
    let raw_to = input.get("to");
    if let Some(v) = raw_to {
        if !v.is_string() && python_truthy(v) {
            return 1;
        }
    }
    let recipient = raw_to.and_then(|v| v.as_str()).unwrap_or("");
    let verdict = is_local(recipient, registered_copies().as_ref());
    if !verdict.allow {
        return 0;
    }
    // La misura del messaggio e' `len(str(...))` nell'originale: un `message`
    // assente diventa la stringa vuota, non un errore.
    let chars = input
        .get("message")
        .and_then(|v| v.as_str())
        .map(|s| s.chars().count())
        .unwrap_or(0);
    record(recipient, &verdict.reason, chars);
    let answer = format!(
        "{{\"hookSpecificOutput\": {{\"hookEventName\": \"PermissionRequest\", \
         \"decision\": {{\"behavior\": \"allow\"}}}}, \"systemMessage\": {}}}",
        python_json_string_with(
            &format!(
                "messaggio a una sessione locale: {}. Chi riceve resta soggetto ai propri permessi.",
                verdict.reason
            ),
            false
        )
    );
    // Un gancio che muore scrivendo blocca il turno che doveva sbloccare.
    let _ = writeln!(std::io::stdout(), "{answer}");
    0
}
