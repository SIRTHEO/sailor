//! La staffetta: chiude una sessione piena e ne apre una che riprende da lì.
//!
//! Porta del corpo di `skills/hooks/relay.py`. Il giudizio — rigenera, salta,
//! pulisci — sta in `guards::handoff::evaluate` ed è già portato; qui c'è ciò
//! che tocca disco e `orca`.
//!
//! L'ORDINE DELLE CHIAMATE A ORCA È IL COMPORTAMENTO, e non è negoziabile:
//!
//! ```text
//!   1. wait  tui-idle sulla vecchia   non si tronca un turno a metà
//!   2. write riprendi-da/<worktree>   il segnale di ripresa, prima di tutto
//!   3. create il successore            PRIMA di chiudere: il worktree non resta
//!                                      mai senza sessione
//!   4. wait + send al successore       l'ordine di riprendere
//!   5. close la vecchia, pulisci       solo ORA, e solo se il resto è riuscito
//! ```
//!
//! Invertire 3 e 5 lascerebbe un albero scoperto ogni volta che il create
//! fallisce. Un errore nel mezzo lascia due sessioni, mai zero: è il verso
//! giusto in cui sbagliare, e vale più dell'eleganza.
//!
//! PERCHÉ `orca` SI INIETTA. `regenerate` non ha un'uscita da confrontare, ha
//! **effetti**: cinque chiamate in un ordine preciso. Chiamando `orca` davvero,
//! una prova su un handle finto diventa verde perché la chiamata è fallita, non
//! perché la guardia ha funzionato — è già successo in questa configurazione,
//! con due mutanti che passavano su 36 prove su 36. Qui il chiamante entra come
//! parametro: le prove registrano la sequenza e la confrontano.

use guards::handoff::{
    evaluate, resolve_terminal_handle, state_key, Action, SessionFacts, Terminal,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Dopo una staffetta, cinque minuti di tregua sul worktree.
const COOLDOWN_SEC: u64 = 300;
/// Se non è idle entro quattro secondi sta lavorando: si riprova dopo.
const IDLE_TIMEOUT_MS: u64 = 4000;
/// Oltre questa dimensione il registro si tronca. La memoria «i log in
/// background crescono senza limite» è un precedente: 5 GB in sette minuti.
const LOG_MAX_BYTES: u64 = 1_000_000;
const LOG_KEEP_LINES: usize = 2000;

/// Chi parla con Orca. Iniettabile perché le prove possano registrarlo.
pub type OrcaFn<'a> = &'a mut dyn FnMut(&[&str]) -> (i32, String);

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn state_dir() -> PathBuf {
    home().join(".claude").join("state")
}

fn live_dir() -> PathBuf {
    state_dir().join("sessioni-vive")
}

fn resume_dir() -> PathBuf {
    state_dir().join("riprendi-da")
}

/// Una riga nel registro, con la data locale davanti.
///
/// Muto in caso di errore, come l'originale: gira da launchd, dove un errore di
/// scrittura del registro non deve fermare la staffetta.
fn log_line(line: &str) {
    let _ = fs::create_dir_all(state_dir());
    let path = state_dir().join("staffetta.log");
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > LOG_MAX_BYTES {
            if let Ok(text) = fs::read_to_string(&path) {
                let lines: Vec<&str> = text.lines().collect();
                let start = lines.len().saturating_sub(LOG_KEEP_LINES);
                let _ = fs::write(&path, lines[start..].join("\n") + "\n");
            }
        }
    }
    // `time.strftime('%Y-%m-%d %H:%M:%S')`: sono i primi 19 caratteri dell'ISO
    // locale, con la T al posto dello spazio.
    let stamp: String = hook_io::local_time::now_local_iso8601()
        .chars()
        .take(19)
        .collect();
    use std::io::Write;
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}  {line}", stamp.replace('T', " "));
    }
}

fn is_off() -> bool {
    state_dir().join("staffetta-off").exists()
}

fn opt_out(sess: &str, worktree: &str) -> bool {
    [
        "non-rigenerare".to_string(),
        format!("non-rigenerare-{worktree}"),
        format!("non-rigenerare-{sess}"),
    ]
    .iter()
    .any(|n| state_dir().join(n).exists())
}

/// `state_key`: un worktree_id vero contiene le barre di un percorso, e un nome
/// di file non le regge. Perché fosse un difetto e non un dettaglio, vedi
/// `guards::handoff::state_key` — fino al 17/08/2026 nessuna tregua è mai stata
/// scritta su questa macchina.
fn in_cooldown(worktree: &str, now: f64) -> bool {
    fs::read_to_string(
        state_dir().join(format!("staffetta-cooldown-{}", state_key(worktree))),
    )
    .ok()
    .and_then(|t| t.trim().parse::<f64>().ok())
    .map(|then| now - then < COOLDOWN_SEC as f64)
    .unwrap_or(false)
}

fn set_cooldown(worktree: &str) {
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(
        state_dir().join(format!("staffetta-cooldown-{}", state_key(worktree))),
        format!("{}", now_epoch()),
    );
}

fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// L'handle del successore che un altro meccanismo ha già aperto, se ancora vivo.
///
/// Si risolve dalla `tabId`; l'handle è la ricaduta per i marcatori scritti
/// prima del 17/08/2026. L'handle registrato all'apertura invecchia al primo
/// riattacco, e un freno che legge un identificatore scaduto non frena mai.
fn armed_successor(session_id: &str, terminals: &[Terminal]) -> String {
    if session_id.is_empty() {
        return String::new();
    }
    let Ok(text) = fs::read_to_string(state_dir().join(format!("successore-di-{session_id}")))
    else {
        return String::new();
    };
    let Ok(d) = serde_json::from_str::<serde_json::Value>(&text) else {
        return String::new();
    };
    let get = |k: &str| d.get(k).and_then(|v| v.as_str()).unwrap_or("");
    resolve_terminal_handle(get("tabId"), "", get("handle"), terminals)
}

/// Il primo `handle` che somiglia a un terminale, ovunque sia nella risposta.
///
/// Non si fissa il percorso di proposito. La versione precedente leggeva
/// `result.handle` mentre Orca risponde `result.terminal.handle`: un livello di
/// scarto, e ogni creazione riuscita veniva letta come fallita — 24 sessioni in
/// mezz'ora, una al minuto.
pub fn find_handle(out: &str) -> String {
    fn walk(node: &serde_json::Value) -> String {
        match node {
            serde_json::Value::Object(map) => {
                if let Some(h) = map.get("handle").and_then(|v| v.as_str()) {
                    if h.starts_with("term_") {
                        return h.to_string();
                    }
                }
                map.values().map(walk).find(|s| !s.is_empty()).unwrap_or_default()
            }
            serde_json::Value::Array(items) => items
                .iter()
                .map(walk)
                .find(|s| !s.is_empty())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }
    serde_json::from_str::<serde_json::Value>(out)
        .map(|v| walk(&v))
        .unwrap_or_default()
}

/// Il chiamante vero, quello che parla con `orca` sul serio.
fn real_orca(args: &[&str]) -> (i32, String) {
    match Command::new("orca").args(args).output() {
        Ok(o) => (
            o.status.code().unwrap_or(1),
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
        ),
        Err(e) => (1, e.to_string()),
    }
}

/// L'elenco dei pannelli vivi, o `None` se **non si è potuto leggere**.
///
/// La differenza è tutto. Prima si tornava una lista vuota in ogni caso storto,
/// e a valle «insieme vuoto» significa «sono morti tutti», con la cancellazione
/// come risposta: una lettura fallita era indistinguibile da una strage, e
/// questo gira ogni minuto — 276 giri tutti «riusciti» mentre cancellava i
/// record di sessioni vive.
fn read_terminals(orca: OrcaFn) -> Option<Vec<Terminal>> {
    let (rc, out) = orca(&["terminal", "list", "--json"]);
    if rc != 0 || out.is_empty() {
        return None;
    }
    let v = serde_json::from_str::<serde_json::Value>(&out).ok()?;
    Some(Terminal::from_response(&v))
}

/// Il documento di consegna più recente, da citare al successore.
fn latest_handoff(cwd: &str) -> String {
    let base = home().join(".claude").join("projects");
    let mut roots: Vec<PathBuf> = Vec::new();
    if !cwd.is_empty() {
        let p = base.join(cwd.replace('/', "-")).join("memory");
        if p.is_dir() {
            roots.push(p);
        }
    }
    if roots.is_empty() {
        roots.push(base);
    }
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for root in roots {
        collect_handoffs(&root, 0, &mut best);
    }
    best.map(|(_, p)| p).unwrap_or_default()
}

/// Cerca `handoff-*.md` e `consegna-*.md`, anche un livello sotto in `memory/`.
///
/// Il Python usa quattro glob; qui una discesa limitata copre gli stessi
/// percorsi senza dipendere da un crate di glob. Il limite di profondità non è
/// prudenza generica: scendere ovunque significherebbe passeggiare su tutta la
/// cartella dei transcript, che sono gigabyte.
fn collect_handoffs(dir: &Path, depth: u32, best: &mut Option<(std::time::SystemTime, String)>) {
    if depth > 2 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if path.is_dir() {
            if depth == 0 || name == "memory" {
                collect_handoffs(&path, depth + 1, best);
            }
            continue;
        }
        if !name.ends_with(".md") {
            continue;
        }
        if !(name.starts_with("handoff-") || name.starts_with("consegna-")) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        let candidate = path.to_string_lossy().to_string();
        // A parità di mtime il Python sceglie il percorso maggiore: `max()` su
        // una tupla confronta il secondo campo quando il primo è uguale.
        let better = match best {
            None => true,
            Some((t, p)) => mtime > *t || (mtime == *t && candidate > *p),
        };
        if better {
            *best = Some((mtime, candidate));
        }
    }
}

/// Un record di sessione viva, per quel che serve alla staffetta.
pub struct Record {
    pub session_id: String,
    pub session: String,
    pub handle: String,
    pub worktree: String,
    pub tab_id: String,
    pub transcript: String,
    pub cwd: String,
}

fn read_record(path: &Path) -> Option<Record> {
    let text = fs::read_to_string(path).ok()?;
    let d: serde_json::Value = serde_json::from_str(&text).ok()?;
    let get = |k: &str| d.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let session_id = get("session_id");
    Some(Record {
        session: session_id.chars().take(8).collect(),
        session_id,
        handle: get("terminal_handle"),
        worktree: get("worktree_id"),
        tab_id: get("tab_id"),
        transcript: get("transcript_path"),
        cwd: get("cwd"),
    })
}

/// Chiude la vecchia e apre il successore, nell'ordine che non lascia scoperti.
pub fn regenerate(rec: &Record, title: &str, dry_run: bool, orca: OrcaFn) {
    let sess = &rec.session;
    if dry_run {
        log_line(&format!(
            "[SECCO] rigenererei sess={sess} handle={} worktree={} cwd={}",
            rec.handle, rec.worktree, rec.cwd
        ));
        return;
    }

    // 1. non troncare un turno: attendi che la vecchia sia idle
    let timeout = IDLE_TIMEOUT_MS.to_string();
    let (rc, _) = orca(&[
        "terminal", "wait", "--terminal", &rec.handle, "--for", "tui-idle",
        "--timeout-ms", &timeout,
    ]);
    if rc != 0 {
        log_line(&format!("sess={sess}: non idle (rc={rc}), rimando"));
        return;
    }

    // 2. lascia il segnale di ripresa per il successore
    let hpath = latest_handoff(&rec.cwd);
    let _ = fs::create_dir_all(resume_dir());
    let _ = fs::write(
        resume_dir().join(format!("{}.txt", state_key(&rec.worktree))),
        if hpath.is_empty() {
            "ultimo handoff in memory"
        } else {
            &hpath
        },
    );

    // 3. CREA il successore PRIMA di chiudere
    let selector = if rec.cwd.is_empty() {
        "active".to_string()
    } else {
        format!("path:{}", rec.cwd)
    };
    let mut args: Vec<&str> = vec![
        "terminal", "create", "--worktree", &selector,
        "--command", "claude", "--json",
    ];
    if !title.is_empty() {
        args.push("--title");
        args.push(title);
    }
    let (rc, out) = orca(&args);
    if rc != 0 {
        set_cooldown(&rec.worktree);
        log_line(&format!(
            "sess={sess}: create fallito (rc={rc}), NON chiudo la vecchia \
             (cooldown {COOLDOWN_SEC}s). out={}",
            cut(&out, 400)
        ));
        return;
    }
    let new_handle = find_handle(&out);
    if new_handle.is_empty() {
        // Raffredda anche qui: senza, il giro dopo ricrea un terminale e lo
        // riabbandona — è così che sono nate 22 sessioni in mezz'ora.
        set_cooldown(&rec.worktree);
        log_line(&format!(
            "sess={sess}: nessun handle dal create, NON chiudo la vecchia \
             (cooldown {COOLDOWN_SEC}s). out={}",
            cut(&out, 400)
        ));
        return;
    }

    // 4. attendi che il successore sia su, poi mandagli l'ordine di ripresa
    orca(&[
        "terminal", "wait", "--terminal", &new_handle, "--for", "tui-idle",
        "--timeout-ms", "15000",
    ]);
    orca(&[
        "terminal", "send", "--terminal", &new_handle, "--text",
        "riprendi dall'ultimo handoff", "--enter",
    ]);

    // 5. ORA chiudi la vecchia e pulisci
    orca(&["terminal", "close", "--terminal", &rec.handle]);
    let _ = fs::remove_file(live_dir().join(format!("{sess}.json")));
    for family in [
        "consegna-fatta", "consegna-blocchi", "consegna-stop",
        "consegna-avvisata", "consegna-misura",
    ] {
        let _ = fs::remove_file(state_dir().join(format!("{family}-{sess}")));
    }
    set_cooldown(&rec.worktree);
    log_line(&format!(
        "RIGENERATA sess={sess}: vecchio={} -> nuovo={new_handle} (handoff={})",
        rec.handle,
        if hpath.is_empty() { "-" } else { &hpath }
    ));
}

/// Chiude la vecchia senza aprirne un'altra: il successore c'è già.
///
/// Stessa prudenza di `regenerate` sul primo passo — si attende che la sessione
/// sia ferma, e se non lo è si rimanda. Troncare un turno a metà costa il lavoro
/// in corso, e qui il rischio è più concreto che altrove: la sessione da
/// chiudere è per definizione una che ha consegnato **e ha continuato a
/// lavorare**, altrimenti la staffetta l'avrebbe già rigenerata.
///
/// Non si scrive il segnale di ripresa: il successore è già partito col suo
/// mandato, e riscriverlo adesso non lo raggiunge.
fn retire(rec: &Record, orca: OrcaFn) {
    let sess = &rec.session;
    let timeout = IDLE_TIMEOUT_MS.to_string();
    let (rc, _) = orca(&[
        "terminal", "wait", "--terminal", &rec.handle, "--for", "tui-idle",
        "--timeout-ms", &timeout,
    ]);
    if rc != 0 {
        log_line(&format!("sess={sess}: non idle (rc={rc}), rimando la chiusura"));
        return;
    }
    orca(&["terminal", "close", "--terminal", &rec.handle]);
    let _ = fs::remove_file(live_dir().join(format!("{sess}.json")));
    for family in [
        "consegna-fatta", "consegna-blocchi", "consegna-stop",
        "consegna-avvisata", "consegna-misura",
    ] {
        let _ = fs::remove_file(state_dir().join(format!("{family}-{sess}")));
    }
    set_cooldown(&rec.worktree);
    log_line(&format!(
        "CONGEDATA sess={sess}: chiusa {} senza aprirne un'altra",
        rec.handle
    ));
}

/// Taglia a `n` **caratteri**, come lo slicing di Python su una stringa.
fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Un passo della staffetta: guarda ogni sessione registrata e agisce.
pub fn step_with(dry_run: bool, orca: OrcaFn) -> i32 {
    if is_off() || !live_dir().is_dir() {
        return 0;
    }
    let now = now_epoch();
    let terminals = read_terminals(orca);
    let live: Option<Vec<String>> = terminals
        .as_ref()
        .map(|ts| ts.iter().map(|t| t.handle.clone()).collect());

    let mut files: Vec<PathBuf> = fs::read_dir(live_dir())
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    files.sort();

    for file in files {
        let Some(mut rec) = read_record(&file) else {
            // Un record illeggibile si butta, come nell'originale: l'alternativa
            // è riprovare a leggerlo ogni minuto per sempre.
            let _ = fs::remove_file(&file);
            continue;
        };
        // L'handle salvato all'avvio invecchia a ogni riattacco: si rilegge
        // quello di adesso PRIMA di decidere, altrimenti si giudica una sessione
        // viva sulla base di un identificatore morto.
        if let Some(ts) = &terminals {
            let attuale = resolve_terminal_handle(&rec.tab_id, "", &rec.handle, ts);
            if !attuale.is_empty() {
                rec.handle = attuale;
            }
        }
        let armed = armed_successor(&rec.session_id, terminals.as_deref().unwrap_or(&[]));
        // PRIMA i fatti che costano un `exists()`, POI la misura.
        //
        // Misurare subito sarebbe più leggibile e sbagliato: `context_used`
        // **scrive** il memo `consegna-misura-*`, e il Python non arriva mai a
        // chiamarla quando un controllo precedente ha già deciso. Farlo qui
        // lasciava un memo per ogni sessione in opt-out, in raffreddamento o col
        // pannello già morto — sette scenari su venti, trovati confrontando i
        // file rimasti sul disco invece delle risposte.
        let opted_out = opt_out(&rec.session, &rec.worktree);
        let in_cooldown = in_cooldown(&rec.worktree, now);
        let handoff_done = state_dir()
            .join(format!("consegna-fatta-{}", rec.session))
            .exists();
        let transcript_exists =
            !rec.transcript.is_empty() && Path::new(&rec.transcript).exists();
        macro_rules! fatti {
            ($thresholds:expr, $used:expr, $lavorato:expr) => {
                SessionFacts {
                    session: &rec.session,
                    handle: &rec.handle,
                    worktree: &rec.worktree,
                    live_handles: live.as_deref(),
                    opted_out,
                    in_cooldown,
                    armed_successor: &armed,
                    handoff_done,
                    transcript_exists,
                    worked_after_handoff: $lavorato,
                    used: $used,
                    thresholds: $thresholds,
                }
            };
        }
        // Senza soglie `evaluate` arriva in fondo e risponde «soglie non
        // calcolabili»: è il segnale che i controlli economici sono tutti
        // passati e che adesso la misura serve davvero. Quel ramo esisteva senza
        // scopo — un vaglio indipendente l'aveva segnalato come irraggiungibile
        // dall'oracolo — e questo è lo scopo.
        let t;
        let (action, reason) = match evaluate(&fatti!(None, 0, false)) {
            (Action::Skip, r) if r == "soglie non calcolabili" => {
                t = crate::handoff::thresholds(&rec.transcript);
                let used = crate::handoff::context_used(&rec.transcript, &rec.session);
                // Si scorre la coda una seconda volta, e solo qui: è la stessa
                // ragione per cui la misura sta in questo ramo e non sopra.
                let lavorato =
                    crate::handoff::worked_after_handoff(&rec.transcript, &rec.session);
                evaluate(&fatti!(Some(&t), used, lavorato))
            }
            altro => altro,
        };
        match action {
            Action::Clean => {
                // Si scrive PRIMA di cancellare. Una cancellazione muta non
                // lascia niente da leggere quando si sbaglia — ed è andata così
                // per 276 giri.
                log_line(&format!(
                    "pulisco sess={}: {reason} (handle={})",
                    rec.session, rec.handle
                ));
                let _ = fs::remove_file(&file);
            }
            Action::Regenerate => {
                log_line(&format!("candidato sess={}: {reason}", rec.session));
                let title = terminals
                    .as_ref()
                    .and_then(|ts| ts.iter().find(|t| t.handle == rec.handle))
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                regenerate(&rec, &title, dry_run, orca);
            }
            Action::Retire => {
                log_line(&format!("congedo sess={}: {reason}", rec.session));
                if dry_run {
                    log_line(&format!(
                        "[SECCO] chiuderei sess={} handle={} senza aprirne un'altra",
                        rec.session, rec.handle
                    ));
                } else {
                    retire(&rec, orca);
                }
            }
            // 'salta': silenzio, o il registro si riempie a ogni giro.
            Action::Skip => {}
        }
    }
    0
}

/// Il passo vero, quello che parla con `orca` sul serio.
pub fn step(dry_run: bool) -> i32 {
    step_with(dry_run, &mut real_orca)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializza i casi che scrivono stato, e li porta in una HOME usa-e-getta.
    ///
    /// PERCHÉ. `log_line` e `set_cooldown` scrivono sotto `$HOME/.claude/state`,
    /// e i test girano col `HOME` vero: il 17/08/2026 quattro righe
    /// `sess=provarel` — fra cui una «RIGENERATA» — sono finite nel registro di
    /// produzione della staffetta, dove chi indaga un guasto le legge come fatti.
    /// Una batteria che sporca ciò che osserva è la stessa trappola già vista
    /// quattro volte in questa configurazione, qui presa dal lato opposto.
    ///
    /// `HOME` è globale al processo e i test Rust girano in parallelo, quindi il
    /// lucchetto non è prudenza: senza, un caso porterebbe via il `HOME` a un
    /// altro mentre scrive.
    struct HomeIsolata {
        _lock: std::sync::MutexGuard<'static, ()>,
        precedente: Option<String>,
    }

    static LUCCHETTO: std::sync::Mutex<()> = std::sync::Mutex::new(());

    impl HomeIsolata {
        fn nuova(nome: &str) -> Self {
            let lock = LUCCHETTO.lock().unwrap_or_else(|e| e.into_inner());
            let precedente = std::env::var("HOME").ok();
            let dir = std::env::temp_dir().join(format!("relay-prove-{nome}"));
            let _ = fs::remove_dir_all(&dir);
            let _ = fs::create_dir_all(dir.join(".claude").join("state"));
            std::env::set_var("HOME", &dir);
            Self { _lock: lock, precedente }
        }
    }

    impl Drop for HomeIsolata {
        fn drop(&mut self) {
            match &self.precedente {
                Some(h) => std::env::set_var("HOME", h),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn l_handle_si_trova_ovunque_sia_annidato() {
        // La riga esatta del registro del 17/08/2026: forma riuscita, lettura
        // fallita, e 24 sessioni nate da lì.
        assert_eq!(
            find_handle(
                r#"{"ok":true,"result":{"terminal":{"handle":"term_dc752551","tabId":"x"}}}"#
            ),
            "term_dc752551"
        );
        assert_eq!(find_handle(r#"{"result":{"handle":"term_a"}}"#), "term_a");
        assert_eq!(find_handle(r#"{"handle":"term_b"}"#), "term_b");
        assert_eq!(
            find_handle(r#"{"result":{"terminals":[{"handle":"term_c"}]}}"#),
            "term_c"
        );
    }

    #[test]
    fn un_handle_che_non_e_un_terminale_non_conta() {
        // La vecchia espressione restituiva volentieri un id di worktree.
        assert_eq!(find_handle(r#"{"result":{"handle":"wt-123"}}"#), "");
        assert_eq!(find_handle(r#"{"ok":false,"error":"no worktree"}"#), "");
        assert_eq!(find_handle("non e' json"), "");
        assert_eq!(find_handle(""), "");
    }

    #[test]
    fn il_taglio_e_a_caratteri_non_a_byte() {
        // Lo slicing di Python conta caratteri: su un messaggio accentato un
        // taglio a byte spezzerebbe una lettera a metà.
        assert_eq!(cut("perché", 5), "perch");
        assert_eq!(cut("abc", 10), "abc");
    }

    fn record_di_prova() -> Record {
        Record {
            session_id: "prova-relay-0000".into(),
            session: "provarel".into(),
            handle: "term_vecchio".into(),
            worktree: "wt-prova".into(),
            tab_id: "tab-1".into(),
            transcript: String::new(),
            cwd: "/x".into(),
        }
    }

    #[test]
    fn il_successore_si_crea_prima_di_chiudere_la_vecchia() {
        let _home = HomeIsolata::nuova("ordine");
        // L'invariante che vale più di ogni altra qui: se questo ordine si
        // inverte, un create fallito lascia il worktree senza sessione.
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            if args.contains(&"create") {
                (0, r#"{"result":{"terminal":{"handle":"term_nuovo"}}}"#.to_string())
            } else {
                (0, String::new())
            }
        };
        regenerate(&record_di_prova(), "", false, &mut orca);
        let seq = chiamate.borrow().clone();
        let pos = |frammento: &str| seq.iter().position(|c| c.contains(frammento));
        let create = pos("create").expect("il create deve esserci");
        let close = pos("close").expect("il close deve esserci");
        assert!(create < close, "create dopo close: {seq:?}");
        // E il send arriva al NUOVO handle, non al vecchio.
        let send = seq.iter().find(|c| c.contains("send")).expect("send assente");
        assert!(send.contains("term_nuovo"), "send al terminale sbagliato: {send}");
    }

    #[test]
    fn senza_handle_dal_create_la_vecchia_non_si_chiude() {
        let _home = HomeIsolata::nuova("senza-handle");
        // Il difetto del 17/08: risposta riuscita, handle non trovato. Il verso
        // giusto è tenersi la vecchia, mai chiuderla al buio.
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            if args.contains(&"create") {
                (0, r#"{"ok":true,"result":{}}"#.to_string())
            } else {
                (0, String::new())
            }
        };
        regenerate(&record_di_prova(), "", false, &mut orca);
        let seq = chiamate.borrow().clone();
        assert!(
            !seq.iter().any(|c| c.contains("close")),
            "ha chiuso la vecchia senza avere il successore: {seq:?}"
        );
    }

    #[test]
    fn una_sessione_che_non_e_idle_non_si_tocca() {
        let _home = HomeIsolata::nuova("non-idle");
        // Primo comando: `wait`. Se fallisce, non deve seguire nient'altro —
        // troncare un turno a metà costa il lavoro in corso.
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            (1, String::new())
        };
        regenerate(&record_di_prova(), "", false, &mut orca);
        let seq = chiamate.borrow().clone();
        assert_eq!(seq.len(), 1, "dopo un wait fallito non si fa altro: {seq:?}");
        assert!(seq[0].contains("wait"));
    }

    #[test]
    fn un_worktree_col_percorso_dentro_scrive_i_suoi_file() {
        let _home = HomeIsolata::nuova("worktree-con-barre");
        // Il caso normale, non un caso limite: ogni identificativo di copia di
        // Orca è `<uuid>::/percorso/assoluto`. Finché quelle barre finivano nel
        // nome del file, la tregua e il segnale di ripresa non venivano scritti
        // mai — sul disco del 17/08/2026, zero e zero su sei sessioni vive.
        let mut rec = record_di_prova();
        rec.worktree = "9591c8dd-9b12::/Users/theo/gyver/work/suite".into();
        let mut orca = |args: &[&str]| -> (i32, String) {
            if args.contains(&"create") {
                (0, r#"{"result":{"terminal":{"handle":"term_nuovo"}}}"#.to_string())
            } else {
                (0, String::new())
            }
        };
        regenerate(&rec, "", false, &mut orca);

        let state = home().join(".claude").join("state");
        let ripresa: Vec<_> = fs::read_dir(state.join("riprendi-da"))
            .map(|d| d.flatten().map(|e| e.file_name()).collect())
            .unwrap_or_default();
        assert_eq!(ripresa.len(), 1, "il segnale di ripresa non è stato scritto");
        let tregua = fs::read_dir(&state)
            .map(|d| {
                d.flatten()
                    .filter(|e| {
                        e.file_name().to_string_lossy().starts_with("staffetta-cooldown-")
                    })
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(tregua, 1, "la tregua non è stata scritta");
    }

    #[test]
    fn a_secco_non_si_chiama_orca_nemmeno_una_volta() {
        let _home = HomeIsolata::nuova("a-secco");
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            (0, String::new())
        };
        regenerate(&record_di_prova(), "", true, &mut orca);
        assert!(chiamate.borrow().is_empty(), "a secco ha parlato con orca");
    }
}
