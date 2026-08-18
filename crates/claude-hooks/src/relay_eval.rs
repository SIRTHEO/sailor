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

use guards::chain::{chain_verdict, ChainLimits, ChainLink, ChainVerdict};
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

/// Lo stesso ponte per il freno della catena, che `guards::chain` decide con la
/// storia come dato: qui la storia arriva dal caso invece che dal disco, così il
/// confronto col Python non deve fabbricare un `state/catene/` per ogni caso.
pub fn run_chain() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 1;
    }
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(case) = serde_json::from_str::<serde_json::Value>(line) else {
            println!("{}", serde_json::json!({"error": "caso illeggibile"}));
            continue;
        };
        let links: Vec<ChainLink> = case
            .get("links")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    // Le stesse funzioni di `relay::read_chain`, e non una copia:
                    // finché erano due scritture, la correzione su una lasciava
                    // l'altra indietro — e questa è quella che risponde ai 1009
                    // casi del confronto, cioè alla cifra più vistosa che si
                    // citi. Un porto con due letture ha due comportamenti.
                    .map(|l| ChainLink {
                        session: l.get("session").and_then(|v| v.as_str()).unwrap_or("").into(),
                        at: crate::relay::link_time(l.get("at")),
                        turns: crate::relay::link_count(l.get("turns")),
                        writes: crate::relay::link_count(l.get("writes")),
                        handoff: l.get("handoff").and_then(|v| v.as_str()).unwrap_or("").into(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let d = ChainLimits::default();
        let lim = case.get("limits");
        let num = |name: &str, fallback: f64| -> f64 {
            lim.and_then(|l| l.get(name)).and_then(|v| v.as_f64()).unwrap_or(fallback)
        };
        let limits = ChainLimits {
            max_links: num("max_links", d.max_links as f64) as usize,
            max_age_sec: num("max_age_sec", d.max_age_sec),
            idle_reset_sec: num("idle_reset_sec", d.idle_reset_sec),
            stall_links: num("stall_links", d.stall_links as f64) as usize,
        };
        let now = case.get("now").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let (verdict, reason) = chain_verdict(&links, now, &limits);
        // I nomi sono quelli del Python, che resta l'oracolo: tradurli qui
        // nasconderebbe una divergenza sul verdetto.
        let verdict = match verdict {
            ChainVerdict::Go => "go",
            ChainVerdict::Reset => "reset",
            ChainVerdict::Stop => "stop",
        };
        println!(
            "{}",
            serde_json::json!({
                "verdict": verdict,
                "reason": reason,
                "sterile": guards::chain::sterile_tail(&links),
            })
        );
    }
    0
}

/// Il quarto ponte: quali file di stato la staffetta considera orfani.
///
/// A SECCO PER COSTRUZIONE — risponde l'elenco e non cancella niente. Un ponte
/// di confronto che cancellasse dovrebbe fabbricare lo stato due volte, una per
/// lato, e due fabbriche sono due occasioni di divergere su cose che non
/// c'entrano con la decisione.
pub fn run_sweep() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 1;
    }
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(case) = serde_json::from_str::<serde_json::Value>(line) else {
            println!("{}", serde_json::json!({"error": "caso illeggibile"}));
            continue;
        };
        // `live` assente o `null` = elenco **illeggibile**, che non è «nessun
        // albero è vivo»: la distinzione è il cuore di ciò che si confronta.
        //
        // Con `orca_out` si parte invece dalla risposta GREZZA, e si passa da
        // `live_worktree_keys`: era l'unico pezzo della catena a restare fuori
        // dal confronto, ed è quello che decide chi è vivo — cioè quali file
        // sopravvivono. Il primo caso grezzo provato ha trovato subito una
        // divergenza: su un array nudo il porto accettava, il Python sollevava.
        let live: Option<Vec<String>> = match case.get("orca_out").and_then(|v| v.as_str()) {
            Some(raw) => {
                let rc = case.get("orca_rc").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let raw = raw.to_string();
                let mut fake = move |_args: &[&str]| (rc, raw.clone());
                crate::relay::live_worktree_keys(&mut fake)
            }
            None => case.get("live").and_then(|v| v.as_array()).map(|a| {
                a.iter()
                    .map(|x| x.as_str().unwrap_or_default().to_string())
                    .collect()
            }),
        };
        let known = live.is_some();
        let root = std::path::PathBuf::from(text(&case, "root"));
        let now = case.get("now").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mut stale: Vec<String> = crate::relay::orphan_tree_state(live.as_deref(), now, &root)
            .iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        // L'ordine di `read_dir` non è quello di `glob`, e un elenco di file
        // orfani è un insieme: ordinarlo qui evita di confrontare il capriccio
        // del filesystem invece della decisione.
        stale.sort();
        // Le chiavi vive escono anche loro: una divergenza su «chi è vivo» e una
        // su «cosa si butta» hanno cause diverse, e stampandone una sola si
        // guarderebbe il sintomo invece del punto in cui i due lati si separano.
        let mut keys = live.unwrap_or_default();
        keys.sort();
        println!(
            "{}",
            serde_json::json!({ "stale": stale, "keys": keys, "known": known })
        );
    }
    0
}

/// Il terzo ponte, e qui il disco serve: la guardia sull'albero ricreato sta in
/// `relay::read_chain`, e ciò che distingue una copia rifatta dalla sua omonima
/// è la data di nascita di una cartella vera. Il chiamante prepara una `HOME`
/// finta con dentro le catene e gli alberi, e i due lati la leggono entrambi.
///
/// SI RISPONDE ANCHE COL VERDETTO, non con i soli anelli. Confrontare la sola
/// lettura certifica una normalizzazione, non un comportamento: un campo può
/// tornare identico da entrambe le parti e portare comunque a una decisione
/// diversa più avanti — è successo con `writes`, dove la stringa `"0"` è falsa
/// per il porto e vera per Python. Qui la catena letta arriva fino a
/// `chain_verdict`, che è ciò che ferma o lascia passare una staffetta.
pub fn run_read_chain() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 1;
    }
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(case) = serde_json::from_str::<serde_json::Value>(line) else {
            println!("{}", serde_json::json!({"error": "caso illeggibile"}));
            continue;
        };
        let links = crate::relay::read_chain(text(&case, "worktree"));
        let items: Vec<serde_json::Value> = links
            .iter()
            .map(|l| {
                serde_json::json!({
                    "session": l.session,
                    "at": l.at,
                    "turns": l.turns,
                    "writes": l.writes,
                    "handoff": l.handoff,
                })
            })
            .collect();
        let now = case.get("now").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let (verdict, reason) = chain_verdict(&links, now, &ChainLimits::default());
        let verdict = match verdict {
            ChainVerdict::Go => "go",
            ChainVerdict::Reset => "reset",
            ChainVerdict::Stop => "stop",
        };
        println!(
            "{}",
            serde_json::json!({
                "links": items,
                "verdict": verdict,
                "reason": reason,
                "sterile": guards::chain::sterile_tail(&links),
            })
        );
    }
    0
}
