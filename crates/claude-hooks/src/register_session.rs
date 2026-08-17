//! `SessionStart`: registra la sessione viva per la staffetta, e la fa ripartire.
//!
//! Porta di `skills/hooks/register-session.py`. I due compiti restano quelli:
//! 1. scrivere `state/sessioni-vive/<sess>.json` con la tupla che il demone
//!    della staffetta non può dedurre da solo — manico del terminale, tab,
//!    worktree, trascrizione, cartella. È il ponte sessione↔terminale: senza,
//!    si sa che una sessione ha consegnato ma non quale terminale chiudere;
//! 2. se la staffetta ha appena rigenerato questo worktree, consumare il
//!    segnale `state/riprendi-da/<chiave>.txt` e iniettarne il mandato su
//!    stdout, così la sessione nuova riprende invece di aprire a vuoto.
//!
//! IL MANICO È UNA FOTOGRAFIA, NON UN'IDENTITÀ. `ORCA_TERMINAL_HANDLE` vale per
//! l'incarnazione del terminale che c'era all'avvio; dopo un riattacco Orca ne
//! conia un altro. La chiave che sopravvive è `ORCA_TAB_ID`, e basta una delle
//! due per registrare: pretendere il manico lasciava fuori proprio le sessioni
//! più facili da ritrovare.
//!
//! `state_key` NON SI RISCRIVE QUI. Sta in `guards::handoff`, ed è la stessa
//! che usa chi il segnale lo scrive: due copie della stessa trasformazione
//! divergono alla prima correzione, e chi scrive con una chiave e legge con
//! l'altra lascia il successore orfano — che è il difetto già visto il 17/08.
//!
//! FAIL-OPEN OVUNQUE: qualunque errore → stdout vuoto, uscita 0. Un gancio che
//! rompe l'avvio di una sessione fa più danno del problema che risolve.

use guards::handoff::state_key;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Un segnale di ripresa più vecchio di dieci minuti è stantio.
const FRESH_SEC: u64 = 600;

/// Le famiglie di marcatori che il presidio della consegna scrive per una
/// sessione. L'elenco è CHIUSO e sta in un posto solo: quando ne esistevano due
/// copie, la famiglia nata dopo era presente in una sola.
const MARKER_FAMILIES: &[&str] = &[
    "consegna-fatta",
    "consegna-fatta-ripartenze",
    "consegna-blocchi",
    "consegna-stop",
    "consegna-avvisata",
    "consegna-misura",
    "consegna-ripartenze",
];

/// `successore-di-` fa eccezione: porta l'identificativo **intero**, non i primi
/// otto come tutti gli altri. Chi cancella per prefisso corto lo manca sempre, e
/// lo manca in silenzio.
const FULL_ID_FAMILIES: &[&str] = &["successore-di"];

fn state_dir() -> PathBuf {
    // La HOME si legge dall'ambiente come nell'originale (`Path.home()`), così
    // il confronto di equivalenza può spostarla senza toccare nessuno dei due.
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/theo".into());
    PathBuf::from(home).join(".claude").join("state")
}

fn live_dir() -> PathBuf {
    state_dir().join("sessioni-vive")
}

fn resume_dir() -> PathBuf {
    state_dir().join("riprendi-da")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

fn record_session(data: &serde_json::Value) {
    let full = data
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let sess: String = full.chars().take(8).collect();
    if sess.is_empty() {
        return;
    }
    let handle = env("ORCA_TERMINAL_HANDLE");
    let worktree = env("ORCA_WORKTREE_ID");
    let tab = env("ORCA_TAB_ID");
    // Basta UNA delle due chiavi; fuori da Orca non c'è niente da rigenerare.
    if worktree.is_empty() || (handle.is_empty() && tab.is_empty()) {
        return;
    }
    let cwd = match data.get("cwd").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    // L'ordine delle chiavi è quello del dizionario Python, e conta: il file è
    // confrontato byte per byte dal test di equivalenza. `serde_json` lo tiene
    // grazie a `preserve_order`.
    let record = serde_json::json!({
        "session_id": full,
        "terminal_handle": handle,
        "worktree_id": worktree,
        "tab_id": tab,
        "transcript_path": data.get("transcript_path").and_then(|v| v.as_str()).unwrap_or(""),
        "cwd": cwd,
        "source": data.get("source").and_then(|v| v.as_str()).unwrap_or(""),
        "updated_at": now(),
    });
    let dir = live_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    // `json.dumps(..., ensure_ascii=False)`: separatori con lo spazio e unicode
    // vero, non `\uXXXX`.
    let _ = fs::write(
        dir.join(format!("{sess}.json")),
        hook_io::python_json::dumps_unicode(&record),
    );
}

/// I file che questa sessione ha scritto, il proprio record compreso. Solo i suoi.
fn own_markers(sess: &str, full_id: &str) -> Vec<PathBuf> {
    if sess.is_empty() {
        return Vec::new();
    }
    let state = state_dir();
    let mut paths = vec![live_dir().join(format!("{sess}.json"))];
    for f in MARKER_FAMILIES {
        paths.push(state.join(format!("{f}-{sess}")));
    }
    if !full_id.is_empty() {
        let safe: String = full_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .take(64)
            .collect();
        for f in FULL_ID_FAMILIES {
            paths.push(state.join(format!("{f}-{safe}")));
        }
    }
    paths
}

/// `SessionEnd`: la sessione cancella il proprio record, e nessuno indovina.
///
/// Chi ha scritto un marcatore sa quando scade, e lo butta lui. Un raccoglitore
/// che passa dopo dovrebbe indovinare un'età massima, cioè scegliere fra
/// cancellare troppo presto e non cancellare mai.
fn forget_session(data: &serde_json::Value) {
    let full = data
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let sess: String = full.chars().take(8).collect();
    if sess.is_empty() {
        return;
    }
    for path in own_markers(&sess, &full) {
        let _ = fs::remove_file(path);
    }
}

/// Il mandato lasciato dalla staffetta, se c'è ed è fresco. Lo consuma.
fn resume_message() -> String {
    let worktree = env("ORCA_WORKTREE_ID");
    if worktree.is_empty() {
        return String::new();
    }
    let signal = resume_dir().join(format!("{}.txt", state_key(&worktree)));
    let Ok(meta) = fs::metadata(&signal) else {
        return String::new();
    };
    let age = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| now().saturating_sub(d.as_secs()))
        .unwrap_or(u64::MAX);
    if age > FRESH_SEC {
        let _ = fs::remove_file(&signal); // stantio: si butta senza agire
        return String::new();
    }
    let Ok(body) = fs::read_to_string(&signal) else {
        return String::new();
    };
    let path = body.trim().to_string();
    let _ = fs::remove_file(&signal); // consumato: una staffetta, una ripresa
    if path.is_empty() {
        return String::new();
    }
    format!(
        "RIPARTENZA AUTOMATICA (staffetta). La sessione precedente su questo \
worktree ha consegnato ed e' stata rigenerata per non trascinare un contesto \
gonfio. Riprendi da quell'handoff: leggi `{path}` e prosegui il piano gia' \
autorizzato, dichiarando in una riga da cosa riparti. Non ricominciare da zero."
    )
}

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return 0; // stdin non è JSON: si esce muti, come l'originale
    };
    if std::env::args().any(|a| a == "--fine") {
        forget_session(&data);
        return 0;
    }
    record_session(&data);
    let msg = resume_message();
    if !msg.is_empty() {
        println!("{msg}");
    }
    0
}
