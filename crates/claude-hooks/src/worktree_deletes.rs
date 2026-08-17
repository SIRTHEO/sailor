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

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return 0;
    };
    if data.get("tool_name").and_then(|v| v.as_str()) != Some("Bash") {
        return 0;
    }
    let command = data
        .get("tool_input")
        .and_then(|v| v.get("command"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let root = workspaces();
    let verdetto = decide(&Facts {
        command,
        workspaces: &root,
        // L'unica domanda che va al disco. `symlink_metadata` e non `metadata`:
        // il secondo segue il collegamento, che è esattamente ciò da cui questa
        // guardia protegge.
        is_link: &|p: &str| {
            fs::symlink_metadata(p)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
        },
    });
    let Verdict::Allow(reason) = &verdetto else {
        return 0;
    };

    log_grant(&data, reason, command);
    println!(
        "{}",
        hook_io::python_json::dumps(&serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": {"behavior": "allow"},
            },
            "systemMessage": format!("cancellazione nel worktree usa-e-getta: {reason}"),
        }))
    );
    0
}
