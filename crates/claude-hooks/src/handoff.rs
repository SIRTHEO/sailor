//! La parte con disco e stato della misura di consegna.
//!
//! Il giudizio puro sta in `guards::handoff`; qui c'è solo ciò che ha bisogno
//! del filesystem: leggere la coda di un transcript, e il memo per sessione che
//! evita di riscorrerla a ogni chiamata.
//!
//! IL MEMO NON È UNA CACHE QUALSIASI. Fra due misure vale l'ultima, ed è un
//! **limite inferiore**: il contesto dentro un turno non cala mai. Per questo si
//! può rispondere con un valore vecchio senza sbagliare direzione — al più si
//! consegna un turno più tardi, mai un turno troppo presto.

use guards::handoff::{context_used_from_lines, thresholds_from_lines, Thresholds, MIN_GROWTH,
                      TAIL_BYTES};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

fn state_dir() -> PathBuf {
    dirs_home().join(".claude").join("state")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
}

/// Le ultime righe del transcript, vuoto in caso di errore.
///
/// Si legge dalla coda perché un transcript di sessione lunga arriva a centinaia
/// di MB. Il taglio cade a metà di una riga e quella riga risulta illeggibile:
/// va bene, chi la usa scorre all'indietro e la salta — ma è il motivo per cui
/// un parse fallito non deve mai interrompere la ricerca.
pub fn transcript_tail(transcript: &str) -> String {
    let Ok(meta) = fs::metadata(transcript) else {
        return String::new();
    };
    let size = meta.len();
    let Ok(mut f) = fs::File::open(transcript) else {
        return String::new();
    };
    if f.seek(SeekFrom::Start(size.saturating_sub(TAIL_BYTES))).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Modello, budget e soglie per la sessione che sta scrivendo questo transcript.
pub fn thresholds(transcript: &str) -> Thresholds {
    let tail = transcript_tail(transcript);
    thresholds_from_lines(&tail.lines().collect::<Vec<_>>())
}

/// I token in contesto, rimisurati solo se il transcript è cresciuto abbastanza.
///
/// Il memo replica il formato del Python — `<byte> <token>` separati da uno
/// spazio — perché lo stesso file lo scrivono e lo leggono entrambe le
/// implementazioni finché convivono. Un formato diverso qui farebbe rimisurare
/// il Python a ogni giro senza che nessuno se ne accorga.
pub fn context_used(transcript: &str, session: &str) -> u64 {
    let memo = if session.is_empty() {
        None
    } else {
        Some(state_dir().join(format!("consegna-misura-{session}")))
    };
    let size = fs::metadata(transcript).map(|m| m.len()).unwrap_or(0);

    if let Some(path) = &memo {
        if let Ok(text) = fs::read_to_string(path) {
            let mut parts = text.split_whitespace();
            if let (Some(old_size), Some(old_tokens)) = (parts.next(), parts.next()) {
                if let (Ok(old_size), Ok(old_tokens)) =
                    (old_size.parse::<u64>(), old_tokens.parse::<u64>())
                {
                    if size.saturating_sub(old_size) < MIN_GROWTH {
                        return old_tokens;
                    }
                }
            }
        }
    }

    let tail = transcript_tail(transcript);
    let tokens = context_used_from_lines(&tail.lines().collect::<Vec<_>>());
    // Il Python scrive il memo solo quando ha trovato una misura: uno zero
    // memorizzato terrebbe la sessione sotto soglia per i 400 KB successivi.
    if tokens > 0 {
        if let Some(path) = &memo {
            let _ = fs::create_dir_all(state_dir());
            let _ = fs::write(path, format!("{size} {tokens}"));
        }
    }
    tokens
}

/// Risolve un handle leggendo l'elenco dei pannelli da stdin, e lo stampa.
///
/// Legge da stdin invece di chiamare `orca` perché le due implementazioni devono
/// vedere **lo stesso** elenco: chiamandolo ognuna per conto suo, due letture a
/// un secondo di distanza possono già non concordare, e una divergenza così
/// verrebbe letta come un difetto del porting.
pub fn resolve(tab_id: &str, worktree_id: &str, known_handle: &str) -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        println!();
        return 0;
    }
    let terminals = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => guards::handoff::Terminal::from_response(&v),
        Err(_) => Vec::new(),
    };
    println!(
        "{}",
        guards::handoff::resolve_terminal_handle(tab_id, worktree_id, known_handle, &terminals)
    );
    0
}

/// Stampa la misura in JSON, per il confronto con l'implementazione Python.
///
/// Non è un gancio: è il punto d'aggancio dello strumento di equivalenza, che
/// chiama le due implementazioni sullo stesso transcript e pretende lo stesso
/// oggetto. Senza un modo di interrogare il Rust dall'esterno, il confronto su
/// materiale vero non si può fare — e i casi scritti a mano non bastano.
pub fn measure(transcript: &str, session: &str) -> i32 {
    let t = thresholds(transcript);
    let used = context_used(transcript, session);
    println!(
        "{}",
        serde_json::json!({
            "model": t.model,
            "budget": t.budget,
            "warn": t.warn,
            "require": t.require,
            "used": used,
        })
    );
    0
}
