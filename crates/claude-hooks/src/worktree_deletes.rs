//! Il gancio PermissionRequest che concede le cancellazioni usa-e-getta.
//!
//! Il giudizio sta in `guards::worktree_deletes`; qui c'è ciò che tocca il
//! mondo: il payload su stdin, la domanda al disco sui collegamenti, il registro
//! delle concessioni e la forma esatta della risposta.
//!
//! DUE FORME DI JSON, ed è la stessa distinzione già pagata altrove: la risposta
//! esce con `ensure_ascii=True` (il predefinito di `json.dumps`), il registro con
//! `ensure_ascii=False`. Sono due chiamate diverse nell'originale, e uniformarle
//! cambierebbe il byte che finisce sul disco per ogni percorso accentato.
//!
//! FAIL-OPEN, MA NON MUTO. Il silenzio qui è anche l'esito normale — questo
//! gancio concede e basta — quindi senza una riga di registro un guasto non si
//! distinguerebbe mai dal funzionamento.

use guards::worktree_deletes::{decide, Facts, Verdict};
use std::fs;
use std::io::{Read, Write};

fn home() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn workspaces() -> String {
    home().join("orca").join("workspaces").display().to_string()
}

fn log_path() -> std::path::PathBuf {
    home()
        .join(".claude")
        .join("state")
        .join("cancellazioni-concesse.jsonl")
}

/// La riga del registro: una per concessione, col comando per intero.
///
/// Senza, «il gancio è partito» resta un'impressione, e una decisione sbagliata
/// resterebbe invisibile invece di trovarsi dopo. Non deve mai far fallire il
/// gancio: se il disco non risponde, si concede lo stesso e si tace.
fn log_grant(data: &serde_json::Value, reason: &str, command: &str) {
    let riga = hook_io::python_json::dumps_unicode(&serde_json::json!({
        "quando": hook_io::journal::now_iso8601_seconds(),
        "sessione": tronca(data.get("session_id").and_then(|v| v.as_str()).unwrap_or(""), 36),
        "cartella": tronca(data.get("cwd").and_then(|v| v.as_str()).unwrap_or(""), 200),
        "motivo": reason,
        "comando": tronca(command, 600),
    }));
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{riga}");
    }
}

/// `s[:n]` del Python: taglia **caratteri**, non byte.
fn tronca(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Ciò che serve per registrare e rispondere a una concessione.
struct Grant {
    data: serde_json::Value,
    command: String,
    reason: String,
}

/// La colla pura: dal payload grezzo al verdetto, senza toccare disco né stdout.
///
/// Isolata per poterla provare senza stdin: `run()` fa solo la lettura e gli
/// effetti, questa funzione fa tutta l'estrazione (`tool_name`, `tool_input`,
/// `command`) e il giudizio. `None` copre sia il payload che non riguarda
/// questo gancio sia il verdetto che non concede: chi chiama non deve
/// distinguerli, deve solo sapere se c'è qualcosa da concedere.
fn extract_grant(raw: &str, workspaces: &str, is_link: &dyn Fn(&str) -> bool) -> Option<Grant> {
    let data = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    if data.get("tool_name").and_then(|v| v.as_str()) != Some("Bash") {
        return None;
    }
    // Convertito subito in `String`: `command` altrimenti resta un prestito su
    // `data`, e `Grant` vuole spostare `data` per intero nella stessa espressione.
    let command = data
        .get("tool_input")
        .and_then(|v| v.get("command"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let verdetto = decide(&Facts { command: &command, workspaces, is_link });
    let Verdict::Allow(reason) = verdetto else {
        return None;
    };
    Some(Grant { data, command, reason })
}

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    // Chi legge stdin da sé deve dichiararlo, o ogni sua riga di registro esce
    // marcata come prova. Qui il payload si rilegge — `extract_grant` lo analizza
    // per conto suo e non torna indietro — ma è un oggetto piccolo, e la
    // dichiarazione deve precedere qualunque riga scritta.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
        hook_io::mark_live_from_payload(&v);
    }
    let root = workspaces();
    // L'unica domanda che va al disco. `symlink_metadata` e non `metadata`: il
    // secondo segue il collegamento, che è esattamente ciò da cui questa
    // guardia protegge.
    let is_link = |p: &str| {
        fs::symlink_metadata(p)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    };
    let Some(grant) = extract_grant(&raw, &root, &is_link) else {
        return 0;
    };

    log_grant(&grant.data, &grant.reason, &grant.command);
    println!(
        "{}",
        hook_io::python_json::dumps(&serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": {"behavior": "allow"},
            },
            "systemMessage": format!("cancellazione nel worktree usa-e-getta: {}", grant.reason),
        }))
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: &str = "/Users/theo/orca/workspaces";

    fn payload(tool_name: &str, command: serde_json::Value) -> String {
        serde_json::json!({
            "tool_name": tool_name,
            "tool_input": {"command": command},
            "session_id": "abc123",
            "cwd": "/Users/theo/orca/workspaces/repo/wt",
        })
        .to_string()
    }

    fn no_link(_: &str) -> bool {
        false
    }

    /// Caso nominale: colla che deve concedere.
    ///
    /// Rotto togliendo `data.get("tool_input")` e leggendo `data.get("command")`
    /// (un livello più in alto, come se il payload non fosse annidato): il test
    /// diventa ROSSO perché il comando estratto è vuoto e `decide` risponde
    /// "comando vuoto", non un `Allow`. Ripristinato, torna VERDE.
    #[test]
    fn a_grantable_bash_deletion_is_granted_with_its_reason() {
        let raw = payload("Bash", serde_json::json!(format!("rm -rf {W}/repo/wt/dist")));
        let grant = extract_grant(&raw, W, &no_link).expect("doveva concedere");
        assert_eq!(grant.command, format!("rm -rf {W}/repo/wt/dist"));
        assert!(grant.reason.contains("1 bersaglio/i"), "{}", grant.reason);
    }

    /// Il caso che protegge il lavoro: un bersaglio fuori dai worktree usa-e-getta
    /// non concede, a prescindere da quanto il resto del payload sembri normale.
    ///
    /// Rotto sostituendo `Verdict::Allow(reason) = verdetto` con un'estrazione
    /// che concede su qualunque verdetto (es. `let reason = verdetto.reason();`
    /// incondizionato): il test diventa ROSSO perché `/etc/passwd` viene
    /// concesso. Ripristinato, torna VERDE.
    #[test]
    fn a_target_outside_the_workspaces_is_never_granted() {
        let raw = payload("Bash", serde_json::json!("rm -rf /etc/passwd"));
        assert!(extract_grant(&raw, W, &no_link).is_none());
    }

    #[test]
    fn a_non_bash_tool_is_ignored() {
        let raw = payload("Edit", serde_json::json!(format!("rm -rf {W}/repo/wt/dist")));
        assert!(extract_grant(&raw, W, &no_link).is_none());
    }

    /// JSON non valido: niente panico, niente concessione per sbaglio.
    #[test]
    fn invalid_json_does_not_panic_and_grants_nothing() {
        assert!(extract_grant("{questo non è json", W, &no_link).is_none());
        assert!(extract_grant("", W, &no_link).is_none());
        assert!(extract_grant("null", W, &no_link).is_none());
        // Un array valido come JSON, ma senza campi da leggere.
        assert!(extract_grant("[1, 2, 3]", W, &no_link).is_none());
    }

    /// Campi mancanti o del tipo sbagliato: si tace, non si indovina.
    ///
    /// Rotto sostituendo `.unwrap_or("")` con un valore che sarebbe dentro i
    /// worktree usa-e-getta (`rm -rf /Users/theo/orca/workspaces/repo/wt/dist`):
    /// il test diventa ROSSO perché un `tool_input` senza `command` concede lo
    /// stesso. Ripristinato, torna VERDE.
    #[test]
    fn missing_or_mistyped_fields_grant_nothing() {
        // Manca `tool_input` del tutto.
        let raw = serde_json::json!({"tool_name": "Bash"}).to_string();
        assert!(extract_grant(&raw, W, &no_link).is_none());

        // `tool_input` c'è ma senza `command`.
        let raw = serde_json::json!({"tool_name": "Bash", "tool_input": {}}).to_string();
        assert!(extract_grant(&raw, W, &no_link).is_none());

        // `command` c'è ma non è testo.
        let raw = payload("Bash", serde_json::json!(42));
        assert!(extract_grant(&raw, W, &no_link).is_none());

        // Manca `tool_name`.
        let raw = serde_json::json!({
            "tool_input": {"command": format!("rm -rf {W}/repo/wt/dist")}
        })
        .to_string();
        assert!(extract_grant(&raw, W, &no_link).is_none());
    }

    /// L'estrazione non deforma il comando: opzioni e virgolette davanti al
    /// bersaglio arrivano al giudice esattamente come nel payload.
    ///
    /// Rotto aggiungendo `.replace('"', "")` sul comando estratto, come farebbe
    /// una "sanificazione" ingenua: il test diventa ROSSO perché le virgolette
    /// spariscono prima di raggiungere `decide`. Ripristinato, torna VERDE.
    #[test]
    fn quoting_and_leading_options_survive_extraction() {
        let raw = payload(
            "Bash",
            serde_json::json!(format!(r#"rm -rf -- "{W}/repo/wt/dist""#)),
        );
        let grant = extract_grant(&raw, W, &no_link).expect("doveva concedere");
        assert!(grant.command.contains("--"));
        assert!(grant.command.contains('"'));
    }
}
