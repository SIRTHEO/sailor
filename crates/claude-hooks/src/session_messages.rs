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

/// Esito della colla pura, prima di ogni effetto sul mondo.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// Il payload non riguarda questo gancio: JSON non valido, non un oggetto,
    /// non `SendMessage`, o senza `tool_input`.
    NotApplicable,
    /// UN DIFETTO DELL'ORIGINALE, RIPRODOTTO. Un `to` che c'e' ma non e' testo
    /// (`{"to": 3}`) supera il controllo `if not destinatario`, non sta fra i
    /// destinatari interni, e arriva a `destinatario.split(" [")`: li' Python
    /// muore di `AttributeError`, cioe' uscita 1. Un `to` falso secondo Python
    /// — `0`, `[]`, `""` — passa invece per «destinatario assente».
    Malformed,
    /// Un destinatario reale, ma non una sessione locale.
    Denied,
    /// Da registrare e da rispondere.
    Grant { recipient: String, reason: String, chars: usize },
}

/// La colla pura: dal payload grezzo e dai nomi delle copie note (se Orca ha
/// risposto) al verdetto, senza toccare stdin, l'ambiente o il registro.
///
/// Isolata per poterla provare senza chiamare `orca`: `run()` fa solo la
/// lettura, la domanda a Orca e gli effetti; questa funzione fa tutta
/// l'estrazione (`tool_name`, `tool_input.to`, `tool_input.message`) e il
/// giudizio.
fn extract_outcome(raw: &str, copies: Option<&BTreeSet<String>>) -> Outcome {
    let Ok(payload) = serde_json::from_str::<Value>(raw) else {
        return Outcome::NotApplicable;
    };
    let Some(payload) = payload.as_object() else {
        return Outcome::NotApplicable;
    };
    if payload.get("tool_name").and_then(|v| v.as_str()) != Some("SendMessage") {
        return Outcome::NotApplicable;
    }
    let Some(input) = payload.get("tool_input").and_then(|v| v.as_object()) else {
        return Outcome::NotApplicable;
    };
    let raw_to = input.get("to");
    if let Some(v) = raw_to {
        if !v.is_string() && python_truthy(v) {
            return Outcome::Malformed;
        }
    }
    let recipient = raw_to.and_then(|v| v.as_str()).unwrap_or("");
    let verdict = is_local(recipient, copies);
    if !verdict.allow {
        return Outcome::Denied;
    }
    // La misura del messaggio e' `len(str(...))` nell'originale: un `message`
    // assente diventa la stringa vuota, non un errore.
    let chars = input
        .get("message")
        .and_then(|v| v.as_str())
        .map(|s| s.chars().count())
        .unwrap_or(0);
    Outcome::Grant { recipient: recipient.to_string(), reason: verdict.reason, chars }
}

pub fn run() -> i32 {
    if is_off() {
        return 0;
    }
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    match extract_outcome(&raw, registered_copies().as_ref()) {
        Outcome::NotApplicable | Outcome::Denied => 0,
        Outcome::Malformed => 1,
        Outcome::Grant { recipient, reason, chars } => {
            record(&recipient, &reason, chars);
            let answer = format!(
                "{{\"hookSpecificOutput\": {{\"hookEventName\": \"PermissionRequest\", \
                 \"decision\": {{\"behavior\": \"allow\"}}}}, \"systemMessage\": {}}}",
                python_json_string_with(
                    &format!(
                        "messaggio a una sessione locale: {reason}. Chi riceve resta soggetto ai propri permessi."
                    ),
                    false
                )
            );
            // Un gancio che muore scrivendo blocca il turno che doveva sbloccare.
            let _ = writeln!(std::io::stdout(), "{answer}");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copies() -> BTreeSet<String> {
        ["tautog", "general"].iter().map(|s| s.to_string()).collect()
    }

    fn payload(tool_name: &str, to: serde_json::Value, message: &str) -> String {
        serde_json::json!({
            "tool_name": tool_name,
            "tool_input": {"to": to, "message": message},
        })
        .to_string()
    }

    /// Caso nominale: colla che deve concedere, con la ragione e i caratteri
    /// giusti nel motivo di `is_local`.
    ///
    /// Rotto sostituendo `payload.get("tool_input")` con `payload.get("input")`
    /// (il nome sbagliato): il test diventa ROSSO perché `Outcome::NotApplicable`
    /// prende il posto di `Grant`. Ripristinato, torna VERDE.
    #[test]
    fn a_message_to_a_known_copy_is_granted() {
        let raw = payload("SendMessage", serde_json::json!("tautog"), "ciao");
        let outcome = extract_outcome(&raw, Some(&copies()));
        assert_eq!(
            outcome,
            Outcome::Grant {
                recipient: "tautog".to_string(),
                reason: "sessione locale sulla copia `tautog`".to_string(),
                chars: 4,
            }
        );
    }

    /// Il caso che protegge dal permesso concesso al buio: un destinatario che
    /// non sta fra le copie note non concede, anche col resto del payload a
    /// posto.
    ///
    /// Rotto sostituendo `if !verdict.allow { return Outcome::Denied; }` con
    /// un ramo che concede comunque: il test diventa ROSSO perché
    /// `qualcun-altro` riceverebbe un `Grant`. Ripristinato, torna VERDE.
    #[test]
    fn an_unrecognised_recipient_is_never_granted() {
        let raw = payload("SendMessage", serde_json::json!("qualcun-altro"), "ciao");
        assert_eq!(extract_outcome(&raw, Some(&copies())), Outcome::Denied);
    }

    /// Orca che non risponde non è un insieme vuoto: senza nomi non si concede
    /// al buio, nemmeno a un destinatario dal nome plausibile.
    ///
    /// Rotto sostituendo il `copies` ricevuto con un elenco di riserva fisso
    /// quando `None`: il test diventa ROSSO perché `tautog` viene concesso
    /// invece di negato. Ripristinato, torna VERDE.
    #[test]
    fn silence_from_orca_denies_instead_of_guessing() {
        let raw = payload("SendMessage", serde_json::json!("tautog"), "ciao");
        assert_eq!(extract_outcome(&raw, None), Outcome::Denied);
    }

    /// JSON non valido, un array al posto di un oggetto, e un tool diverso da
    /// `SendMessage`: niente panico, niente concessione per sbaglio.
    #[test]
    fn invalid_or_irrelevant_payloads_are_not_applicable() {
        assert_eq!(extract_outcome("{questo non è json", Some(&copies())), Outcome::NotApplicable);
        assert_eq!(extract_outcome("[1, 2, 3]", Some(&copies())), Outcome::NotApplicable);
        assert_eq!(extract_outcome("null", Some(&copies())), Outcome::NotApplicable);
        assert_eq!(extract_outcome("", Some(&copies())), Outcome::NotApplicable);
        let raw = payload("Edit", serde_json::json!("tautog"), "ciao");
        assert_eq!(extract_outcome(&raw, Some(&copies())), Outcome::NotApplicable);
        // `tool_input` assente del tutto.
        let raw = serde_json::json!({"tool_name": "SendMessage"}).to_string();
        assert_eq!(extract_outcome(&raw, Some(&copies())), Outcome::NotApplicable);
    }

    /// Il difetto dell'originale, riprodotto: un `to` presente ma non testo, e
    /// vero secondo Python, muore in Python — qui diventa `Malformed` e non un
    /// `Grant` travestito da destinatario vuoto.
    ///
    /// Rotto togliendo `!v.is_string() &&` dalla condizione (lasciando solo
    /// `python_truthy(v)`): il test diventa ROSSO perché un `to` **testuale**
    /// come `"tautog"` (vero secondo Python) diventerebbe `Malformed` invece di
    /// `Grant`. Ripristinato, torna VERDE.
    #[test]
    fn a_to_field_present_but_not_text_is_malformed() {
        let raw = payload("SendMessage", serde_json::json!(3), "ciao");
        assert_eq!(extract_outcome(&raw, Some(&copies())), Outcome::Malformed);

        let raw = payload("SendMessage", serde_json::json!({"x": 1}), "ciao");
        assert_eq!(extract_outcome(&raw, Some(&copies())), Outcome::Malformed);

        // Falso secondo Python: passa per «destinatario assente», non malformato.
        let raw = payload("SendMessage", serde_json::json!(0), "ciao");
        assert_eq!(extract_outcome(&raw, Some(&copies())), Outcome::Denied);

        // Un `to` testuale (vero secondo Python, ma già stringa) non entra in
        // questo ramo: la condizione `!v.is_string()` lo esclude a monte.
        let raw = payload("SendMessage", serde_json::json!("tautog"), "ciao");
        assert_ne!(extract_outcome(&raw, Some(&copies())), Outcome::Malformed);
    }

    /// L'estrazione non deforma il destinatario: il riferimento fra parentesi
    /// quadre e un suffisso di sessione arrivano a `is_local` per intero, non
    /// troncati o ripuliti in anticipo dalla colla.
    ///
    /// Rotto anticipando nella colla lo `split(" [")` che spetta a `is_local`:
    /// il test diventa ROSSO perché il destinatario registrato perde il
    /// riferimento fra parentesi. Ripristinato, torna VERDE.
    #[test]
    fn recipient_extraction_keeps_the_ref_suffix_intact() {
        let raw = payload("SendMessage", serde_json::json!("tautog-3a [c23e5e]"), "ciao");
        let outcome = extract_outcome(&raw, Some(&copies()));
        assert_eq!(
            outcome,
            Outcome::Grant {
                recipient: "tautog-3a [c23e5e]".to_string(),
                reason: "sessione locale sulla copia `tautog`".to_string(),
                chars: 4,
            }
        );
    }

    /// `message` assente diventa zero caratteri, non un errore: la misura del
    /// gancio è `len(str(...))` nell'originale, e un `message` mancante non
    /// deve far cadere l'estrazione.
    #[test]
    fn a_missing_message_counts_as_zero_characters() {
        let raw = serde_json::json!({
            "tool_name": "SendMessage",
            "tool_input": {"to": "tautog"},
        })
        .to_string();
        assert_eq!(
            extract_outcome(&raw, Some(&copies())),
            Outcome::Grant {
                recipient: "tautog".to_string(),
                reason: "sessione locale sulla copia `tautog`".to_string(),
                chars: 0,
            }
        );
    }
}
