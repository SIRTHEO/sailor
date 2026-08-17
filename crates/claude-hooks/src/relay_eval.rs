//! Il punto d'aggancio con cui lo strumento di equivalenza interroga la decisione
//! della staffetta. Non è un gancio: nessuna configurazione lo chiama.
//!
//! PERCHÉ SERVE. `guards::handoff::evaluate` è pura e riceve i fatti già
//! raccolti, mentre il Python li legge da disco mentre decide. Confrontare le due
//! senza un punto d'ingresso significherebbe riscrivere qui la raccolta — cioè
//! confrontare il porting con una terza copia scritta apposta. Qui invece si
//! raccoglie con le stesse funzioni che il gancio userebbe (`handoff::thresholds`
//! e `handoff::context_used`, che leggono il transcript e il memo per sessione) e
//! si decide con la funzione sotto esame.
//!
//! I casi arrivano da stdin come JSONL e le risposte escono nello stesso ordine:
//! un processo per l'intera batteria invece di uno per caso, perché duecento
//! avvii di processo per parte trasformano un confronto da secondi in minuti, e
//! un confronto che nessuno aspetta non viene lanciato.

use guards::handoff::{evaluate, Action, SessionFacts};
use std::io::Read;

/// `live_handles` assente o `null` = elenco **illeggibile**, che non è «sono
/// morti tutti». La distinzione è il cuore di ciò che si sta confrontando, e
/// `null` deve arrivare fin qui senza diventare una lista vuota per strada.
fn live_handles(case: &serde_json::Value) -> Option<Vec<String>> {
    let v = case.get("live")?;
    let items = v.as_array()?;
    Some(
        items
            .iter()
            .map(|x| x.as_str().unwrap_or_default().to_string())
            .collect(),
    )
}

fn flag(case: &serde_json::Value, name: &str) -> bool {
    case.get(name).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn text<'a>(case: &'a serde_json::Value, name: &str) -> &'a str {
    case.get(name).and_then(|v| v.as_str()).unwrap_or_default()
}

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 1;
    }
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(case) = serde_json::from_str::<serde_json::Value>(line) else {
            println!("{}", serde_json::json!({"error": "caso illeggibile"}));
            continue;
        };
        let transcript = text(&case, "transcript");
        let exists = !transcript.is_empty() && std::path::Path::new(transcript).exists();
        let t = crate::handoff::thresholds(transcript);
        let used = crate::handoff::context_used(transcript, text(&case, "memo_session"));
        let live = live_handles(&case);
        let facts = SessionFacts {
            session: text(&case, "session"),
            handle: text(&case, "handle"),
            worktree: text(&case, "worktree"),
            live_handles: live.as_deref(),
            opted_out: flag(&case, "opted_out"),
            in_cooldown: flag(&case, "in_cooldown"),
            armed_successor: text(&case, "armed"),
            handoff_done: flag(&case, "handoff_done"),
            handoff_deliberate: flag(&case, "handoff_deliberate"),
            transcript_exists: exists,
            // Si legge dal disco invece di prenderlo dal caso, come tutto il
            // resto di questa riga: il Python lo calcola da sé mentre decide, e
            // passarglielo cotto qui confronterebbe due domande diverse.
            worked_after_handoff: crate::handoff::worked_after_handoff(
                transcript,
                text(&case, "session"),
            ),
            used,
            thresholds: Some(&t),
        };
        let (action, reason) = evaluate(&facts);
        // I nomi sono quelli del Python: è lui l'oracolo finché non lo si
        // cancella, e tradurli qui nasconderebbe una divergenza sull'azione.
        let action = match action {
            Action::Regenerate => "rigenera",
            Action::Skip => "salta",
            Action::Clean => "pulisci",
            // Nome nuovo su entrambi i lati: il Python lo prende con la stessa
            // correzione, quindi il confronto resta un confronto e non una
            // traduzione che nasconde la differenza.
            Action::Retire => "congeda",
        };
        println!(
            "{}",
            serde_json::json!({
                "action": action,
                "reason": reason,
                "model": t.model,
                "budget": t.budget,
                "require": t.require,
                "used": used,
            })
        );
    }
    0
}
