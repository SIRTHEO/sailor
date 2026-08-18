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
//!   4. attendi che il successore         la prova che il mandato è
//!      raccolga il segnale               arrivato; il send è il ripiego
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
/// Quanto si aspetta che il successore raccolga il mandato dal segnale.
///
/// Sovrascrivibile con `RELAY_PICKUP_TIMEOUT_SEC`, e non per gusto: le prove e
/// lo strumento di equivalenza rigenerano una decina di volte a giro, e a
/// venticinque secondi l'una il confronto durerebbe più di dieci minuti — cioè
/// non lo lancerebbe più nessuno.
const PICKUP_TIMEOUT_SEC: u64 = 25;
const PICKUP_POLL_MS: u64 = 250;

fn pickup_timeout() -> u64 {
    std::env::var("RELAY_PICKUP_TIMEOUT_SEC")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(PICKUP_TIMEOUT_SEC)
}
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
pub(crate) fn read_terminals(orca: OrcaFn) -> Option<Vec<Terminal>> {
    let (rc, out) = orca(&["terminal", "list", "--json"]);
    if rc != 0 || out.is_empty() {
        return None;
    }
    let v = serde_json::from_str::<serde_json::Value>(&out).ok()?;
    Some(Terminal::from_response(&v))
}

/// Gli handle dei terminali aperti su un albero di lavoro, adesso.
///
/// Vuoto vuol dire due cose diverse — «nessuno» e «non ho potuto leggere» — e
/// chi la usa deve trattarle uguale: qui serve a confrontare un prima con un
/// dopo, e un confronto con una lettura fallita non conclude niente. Non si
/// usi per decidere che un terminale è morto.
fn handles_on_worktree(worktree: &str, orca: OrcaFn) -> Vec<String> {
    read_terminals(orca)
        .map(|ts| {
            ts.iter()
                .filter(|t| t.worktree_id == worktree)
                .map(|t| t.handle.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Quante consegne candidate si aprono per confermarle. Si scorre per data
/// decrescente, quindi la prima che regge è anche la più recente; il tetto vale
/// per il ripiego su tutti i progetti, dove i documenti sono centinaia.
const MAX_CANDIDATES: usize = 40;

/// Quanto si legge di un documento prima di giudicarlo, come nel Python.
const MAX_READ: u64 = 64 * 1024;

/// Il repo che ospita `cwd`, se `cwd` è un albero di lavoro. Vuoto altrimenti.
///
/// In un albero secondario `.git` è un file che punta al repo principale
/// (`gitdir: /percorso/.git/worktrees/<nome>`), quindi la risposta si legge
/// invece di chiederla a `git`: nessun processo figlio in un gancio che gira a
/// ogni Stop, e nessuna dipendenza dal PATH ristretto di launchd.
pub(crate) fn canonical_root(cwd: &str) -> String {
    let mut here = PathBuf::from(cwd);
    for _ in 0..4 {
        if cwd.is_empty() || here.parent().is_none() {
            return String::new();
        }
        let marker = here.join(".git");
        if marker.is_file() {
            let Ok(text) = fs::read_to_string(&marker) else {
                return String::new();
            };
            for line in text.lines() {
                let Some(rest) = line.trim().strip_prefix("gitdir:") else {
                    continue;
                };
                let gitdir = rest.trim();
                if let Some(cut) = gitdir.find("/.git/worktrees/") {
                    if cut > 0 {
                        return gitdir[..cut].to_string();
                    }
                }
            }
            return String::new();
        }
        if marker.is_dir() {
            return String::new();
        }
        let Some(parent) = here.parent() else {
            return String::new();
        };
        here = parent.to_path_buf();
    }
    String::new()
}

/// Le cartelle dove cercare una consegna: quella del progetto, o tutte.
///
/// IL RIPIEGO CONTRADDICEVA LA RIGA SOPRA DI SÉ. Il Python dichiarava che
/// «armare un successore su una consegna altrui è peggio che non armarlo», e
/// subito dopo, non trovando la cartella del progetto, cercava ovunque. Con un
/// cwd noto la risposta giusta a «questo progetto non ha consegne» è **nessuna**:
/// `arm_successor` esce sul percorso vuoto, e la staffetta scrive un mandato
/// generico che il successore risolve leggendo il proprio `MEMORY.md`. Il
/// ripiego resta solo dove non c'è niente da restringere: cwd assente.
fn memory_roots(cwd: &str) -> Vec<PathBuf> {
    let base = home().join(".claude").join("projects");
    if cwd.is_empty() {
        return vec![base];
    }
    let canonical = canonical_root(cwd);
    for candidate in [cwd, canonical.as_str()] {
        if candidate.is_empty() {
            continue;
        }
        let p = base.join(candidate.replace('/', "-")).join("memory");
        if p.is_dir() {
            return vec![p];
        }
    }
    Vec::new()
}

/// Il documento di consegna più recente, da citare al successore.
///
/// IL RIPIEGO ERA LA STRADA NORMALE, fino al 18/08/2026. Una sessione dentro un
/// albero di lavoro non ha una cartella di memoria propria — la sua sta sotto il
/// repo che lo ospita — quindi il restringimento al progetto non si applicava
/// mai proprio dove serve. Misurato sul registro della staffetta: **5
/// rigenerazioni su 11** hanno citato la consegna di un altro progetto, e alle
/// 02:08 di quel giorno un successore sulla suite ha ricevuto il mandato della
/// configurazione.
///
/// E il nome non riconosce le consegne: la skill scrive uno slug tematico, e la
/// consegna delle 02:07 era invisibile mentre vinceva quella del giorno prima.
/// Il criterio che funziona è quello di `guards::successor`, lo stesso che arma
/// il successore: si scorre per data e si conferma leggendo.
pub(crate) fn latest_handoff(cwd: &str) -> String {
    let mut found: Vec<(std::time::SystemTime, String)> = Vec::new();
    for root in memory_roots(cwd) {
        collect_memory_docs(&root, 0, &mut found);
    }
    // Per data decrescente; a parità il percorso maggiore, come il `max()` su
    // tupla del Python.
    found.sort_by(|a, b| b.cmp(a));
    for (_, path) in found.iter().take(MAX_CANDIDATES) {
        let text = read_head(Path::new(path));
        if guards::successor::is_handoff_doc(path, text.as_deref()) {
            return path.clone();
        }
    }
    String::new()
}

/// La testa di un documento, entro il tetto di lettura. `None` se illeggibile.
fn read_head(path: &Path) -> Option<String> {
    use std::io::Read;
    let file = fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.take(MAX_READ).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Raccoglie i `.md` che stanno in `memory/`, anche un livello più sotto.
///
/// Il Python usa due glob; qui una discesa limitata copre gli stessi percorsi
/// senza dipendere da un crate di glob. Il limite di profondità non è prudenza
/// generica: scendere ovunque significherebbe passeggiare su tutta la cartella
/// dei transcript, che sono gigabyte.
fn collect_memory_docs(dir: &Path, depth: u32, found: &mut Vec<(std::time::SystemTime, String)>) {
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
                collect_memory_docs(&path, depth + 1, found);
            }
            continue;
        }
        if !name.ends_with(".md") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        found.push((mtime, path.to_string_lossy().to_string()));
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
/// Cosa ha prodotto la sessione uscente: messaggi dell'assistente e scritture.
///
/// IL §3 DEL MANDATO CHIEDE CHE OGNI ITERAZIONE REGISTRI IL PROGRESSO, e questo
/// loop non lo faceva: il registro diceva quanti token aveva la sessione e
/// perché veniva sostituita, mai cosa aveva concluso. Senza quel dato una catena
/// che gira a vuoto — nasce, non conclude niente, consegna, viene sostituita — è
/// indistinguibile da una che lavora, e i quattro freni non la fermerebbero:
/// guardano tutti il momento, nessuno la storia.
///
/// Si conta solo alla rigenerazione, che è rara (11 in 15 ore il 18/08/2026),
/// non a ogni giro di valutazione. Il transcript è JSONL compatto e si legge a
/// righe: un file da decine di MB non deve stare in memoria tutto insieme.
fn progress(transcript: &str) -> (u64, u64) {
    use std::io::BufRead;
    let Ok(file) = fs::File::open(transcript) else {
        return (0, 0);
    };
    let mut turns = 0;
    let mut writes = 0;
    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
        if line.contains(r#""type":"assistant""#) {
            turns += 1;
        }
        for tool in [r#""name":"Write""#, r#""name":"Edit""#, r#""name":"MultiEdit""#] {
            writes += line.matches(tool).count() as u64;
        }
    }
    (turns, writes)
}

pub fn regenerate(
    rec: &Record,
    title: &str,
    dry_run: bool,
    orca: OrcaFn,
    before: Option<&[Terminal]>,
) {
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
    let segnale_scritto = fs::write(
        resume_dir().join(format!("{}.txt", state_key(&rec.worktree))),
        if hpath.is_empty() {
            "ultimo handoff in memory"
        } else {
            &hpath
        },
    )
    .is_ok();

    // Chi c'era su questo albero PRIMA di creare. Non si rilegge: `step_with` ha
    // gia' fotografato i terminali all'inizio del giro, e quella e' esattamente
    // la foto giusta — lo stato prima di qualunque azione. Rileggerlo qui
    // costerebbe una chiamata a `orca` a **ogni** rigenerazione per servire un
    // ramo che scatta di rado.
    let before: Vec<String> = before
        .map(|ts| {
            ts.iter()
                .filter(|t| t.worktree_id == rec.worktree)
                .map(|t| t.handle.clone())
                .collect()
        })
        .unwrap_or_default();

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
    let mut new_handle = find_handle(&out);
    if new_handle.is_empty() {
        // IL RAMO CHE AGGIUNGE SESSIONI, e perché non basta fermarsi qui.
        //
        // «Non ho riconosciuto l'handle» non è «il terminale non è nato»: il
        // comando può essere riuscito con una risposta di forma diversa. Fino al
        // 17/08/2026 qui ci si fermava, quindi il successore restava vivo E la
        // vecchia non veniva chiusa — una sessione in più, ogni volta. È il solo
        // ramo automatico che fa crescere il totale: 47 volte in due giorni, 23
        // delle quali il 17/08, ed è la spiegazione delle 64 sessioni del 16/08,
        // giorno in cui le rigenerazioni **riuscite** sono state zero.
        //
        // Quindi si guarda invece di dedurre: se sull'albero è comparso
        // esattamente un terminale che prima non c'era, quello è il successore.
        // Uno solo: con due o più non si sa quale sia, e chiudere la vecchia
        // basandosi sul candidato sbagliato costa il lavoro di qualcun altro.
        let dopo = handles_on_worktree(&rec.worktree, &mut *orca);
        let newcomers: Vec<&String> = dopo
            .iter()
            .filter(|h| !before.contains(h) && *h != &rec.handle)
            .collect();
        match newcomers.len() {
            1 => {
                new_handle = newcomers[0].clone();
                log_line(&format!(
                    "sess={sess}: handle non riconosciuto nella risposta, ma \
                     l'elenco ne mostra uno nuovo ({new_handle}): proseguo"
                ));
            }
            0 => {
                // Qui il create davvero non ha prodotto niente: si raffredda,
                // altrimenti il giro dopo ricrea e riabbandona — è così che
                // sono nate 22 sessioni in mezz'ora.
                set_cooldown(&rec.worktree);
                log_line(&format!(
                    "sess={sess}: nessun handle dal create e nessun terminale \
                     nuovo sull'albero, NON chiudo la vecchia (cooldown \
                     {COOLDOWN_SEC}s). out={}",
                    cut(&out, 400)
                ));
                return;
            }
            n => {
                set_cooldown(&rec.worktree);
                log_line(&format!(
                    "sess={sess}: handle non riconosciuto e {n} terminali nuovi \
                     sull'albero: non so quale sia il successore, NON chiudo la \
                     vecchia (cooldown {COOLDOWN_SEC}s). out={}",
                    cut(&out, 400)
                ));
                return;
            }
        }
    }

    // 4. aspetta la PROVA che il successore ha ricevuto il mandato
    //
    // `tui-idle` NON DICE «PRONTO», dice «non sta scrivendo adesso» — vero anche
    // di un processo che non è ancora partito. Misurato il 17/08/2026 creando un
    // terminale e interrogandolo subito: `satisfied: true` dopo **0,1 secondi**,
    // con Claude Code ancora in avvio. Il `send` che segue arriva in
    // un'interfaccia che non c'è e il testo si perde: zero mandati consegnati su
    // tutte le rigenerazioni mai fatte, e su tre prove su tre. Il `send` in sé
    // funziona — provato su una shell, il comando è arrivato ed è stato
    // eseguito: il difetto è il momento, non il canale.
    //
    // Il segnale giusto c'è già: `register-session.py` **consuma** il file
    // `riprendi-da/<worktree>.txt` quando la sessione nuova parte. Se sparisce,
    // il successore è su e ha in mano il mandato. Il `send` resta come ripiego.
    if wait_for_pickup(&rec.worktree, segnale_scritto) {
        log_line(&format!(
            "sess={sess}: il successore ha raccolto il mandato dal segnale"
        ));
    } else {
        let (rc_send, _) = orca(&[
            "terminal", "send", "--terminal", &new_handle, "--text",
            "riprendi dall'ultimo handoff", "--enter",
        ]);
        log_line(&format!(
            "sess={sess}: il segnale non e' stato raccolto entro \
             {}s, provo a voce (send rc={rc_send})",
            pickup_timeout()
        ));
    }

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
    let (turns, writes) = progress(&rec.transcript);
    log_line(&format!(
        "RIGENERATA sess={sess}: vecchio={} -> nuovo={new_handle} (handoff={}, \
         prodotto: {turns} turni, {writes} scritture)",
        rec.handle,
        if hpath.is_empty() { "-" } else { &hpath }
    ));
}

/// True se il successore ha raccolto il segnale di ripresa, entro il tempo.
///
/// È l'unica conferma vera che il mandato sia arrivato: `register-session.py`
/// cancella `riprendi-da/<worktree>.txt` **dopo** averlo letto e iniettato. Un
/// `send` riuscito non prova niente — il testo può arrivare a un terminale che
/// non ha ancora un'interfaccia dove metterlo, ed è quello che succedeva.
///
/// `scritto` viene da chi il segnale l'ha scritto, e non si deduce guardando se
/// il file c'è: un successore svelto può averlo già raccolto prima che si arrivi
/// qui, e «non c'è» significherebbe allora **riuscito**, non «mai scritto». Le
/// due cose portano a righe di registro opposte, e la prima stesura le
/// confondeva — trovato da un caso, non a mente.
fn wait_for_pickup(worktree: &str, scritto: bool) -> bool {
    if !scritto {
        return false;
    }
    let signal = resume_dir().join(format!("{}.txt", state_key(worktree)));
    if !signal.exists() {
        return true; // già raccolto: il successore è stato più svelto di noi
    }
    let scadenza =
        std::time::Instant::now() + std::time::Duration::from_secs(pickup_timeout());
    while std::time::Instant::now() < scadenza {
        if !signal.exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(PICKUP_POLL_MS));
    }
    false
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

/// I primi `n` caratteri, **su una riga sola**.
///
/// L'appiattimento non è cosmesi. Il registro della staffetta è per righe, e la
/// risposta di `orca terminal create --json` è un JSON indentato: senza questo,
/// di 400 caratteri se ne leggeva **uno** — la graffa aperta. Misurato il
/// 17/08/2026 su 47 casi «nessun handle dal create», dove la domanda che conta è
/// se il terminale sia nato lo stesso, e la risposta era nella parte tagliata.
fn cut(s: &str, n: usize) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect()
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
        let handoff_deliberate = state_dir()
            .join(format!("consegna-volontaria-{}", rec.session))
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
                    handoff_deliberate,
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
                regenerate(&rec, &title, dry_run, orca, terminals.as_deref());
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

    use crate::test_home::HomeIsolata;

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
        regenerate(&record_di_prova(), "", false, &mut orca, None);
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
        regenerate(&record_di_prova(), "", false, &mut orca, None);
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
        regenerate(&record_di_prova(), "", false, &mut orca, None);
        let seq = chiamate.borrow().clone();
        assert_eq!(seq.len(), 1, "dopo un wait fallito non si fa altro: {seq:?}");
        assert!(seq[0].contains("wait"));
    }

    #[test]
    fn quando_il_successore_raccoglie_il_mandato_non_si_digita_niente() {
        let _home = HomeIsolata::nuova("mandato-raccolto");
        // È il caso buono, e va provato perché è quello che il 17/08/2026 non
        // succedeva mai: la sessione nuova parte, `register-session.py` legge
        // `riprendi-da/<worktree>.txt` e lo cancella. Sparito il file, il
        // mandato è arrivato — e digitare in una TUI diventa inutile.
        std::env::set_var("RELAY_PICKUP_TIMEOUT_SEC", "5");
        let segnale = resume_dir().join(format!("{}.txt", state_key("wt-prova")));
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            if args.contains(&"create") {
                // Il successore parte e raccoglie: è ciò che fa il gancio di
                // avvio, qui al momento giusto invece che da un altro processo.
                let _ = fs::remove_file(&segnale);
                (0, r#"{"result":{"terminal":{"handle":"term_nuovo"}}}"#.to_string())
            } else {
                (0, String::new())
            }
        };
        regenerate(&record_di_prova(), "", false, &mut orca, None);
        let seq = chiamate.borrow().clone();
        assert!(
            !seq.iter().any(|c| c.contains("send")),
            "ha digitato l'ordine a un successore che l'aveva già: {seq:?}"
        );
        let log =
            fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert!(log.contains("ha raccolto il mandato"), "{log}");
        assert!(log.contains("RIGENERATA"), "{log}");
    }

    #[test]
    fn un_ordine_di_ripresa_non_consegnato_lascia_una_riga() {
        let _home = HomeIsolata::nuova("send-fallito");
        // Il caso vero: il create riesce, la vecchia si chiude, e l'ordine a
        // voce non arriva. Finché gli esiti venivano scartati, questo era
        // indistinguibile da una staffetta perfetta — ed è andata così su
        // **tutte** le rigenerazioni mai fatte su questa macchina.
        let mut orca = |args: &[&str]| -> (i32, String) {
            if args.contains(&"create") {
                (0, r#"{"result":{"terminal":{"handle":"term_nuovo"}}}"#.to_string())
            } else if args.contains(&"send") {
                (1, String::new())
            } else {
                (0, String::new())
            }
        };
        regenerate(&record_di_prova(), "", false, &mut orca, None);
        let log = fs::read_to_string(
            home().join(".claude/state/staffetta.log"),
        )
        .unwrap_or_default();
        assert!(
            log.contains("non e' stato raccolto"),
            "il ripiego a voce non lascia traccia: {log}"
        );
        // E la staffetta va avanti lo stesso: il mandato viaggia per disco.
        assert!(log.contains("RIGENERATA"), "{log}");
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
        regenerate(&rec, "", false, &mut orca, None);

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
        regenerate(&record_di_prova(), "", true, &mut orca, None);
        assert!(chiamate.borrow().is_empty(), "a secco ha parlato con orca");
    }

    /// Il corpo che la skill `handoff` prescrive: frontmatter e le due sezioni.
    const CONSEGNA: &str = "---\nname: x\ndescription: y\nmetadata:\n  type: project\n---\n\n## Stato\n\nfatto\n\n## Prossimi passi\n\nquello dopo\n";

    /// Scrive un documento di memoria e ne fissa l'età in secondi.
    fn doc(home: &Path, progetto: &str, nome: &str, testo: &str, eta: u64) -> String {
        let dir = home
            .join(".claude")
            .join("projects")
            .join(progetto.replace('/', "-"))
            .join("memory");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(nome);
        // L'età si fissa invece di aspettare: senza, due file scritti nello
        // stesso istante lascerebbero decidere l'ordine al percorso, e il caso
        // proverebbe qualcosa d'altro.
        let f = fs::File::create(&p).unwrap();
        use std::io::Write;
        (&f).write_all(testo.as_bytes()).unwrap();
        let quando =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 - eta);
        f.set_modified(quando).unwrap();
        p.to_string_lossy().to_string()
    }

    #[test]
    fn l_albero_di_lavoro_risale_al_repo_che_lo_ospita() {
        let home = HomeIsolata::nuova("radice-canonica");
        let repo = home.dir.join("gyver").join("suite");
        let albero = home.dir.join("orca").join("tautog");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&albero).unwrap();
        fs::write(
            albero.join(".git"),
            format!("gitdir: {}/.git/worktrees/tautog\n", repo.display()),
        )
        .unwrap();
        assert_eq!(
            canonical_root(albero.to_str().unwrap()),
            repo.to_string_lossy()
        );
        // MUTANTE: un checkout principale ha `.git` cartella, e non risale a nessuno.
        assert_eq!(canonical_root(repo.to_str().unwrap()), "");
        assert_eq!(canonical_root("/non/esiste/affatto"), "");
        assert_eq!(canonical_root(""), "");
    }

    #[test]
    fn il_successore_eredita_la_consegna_del_suo_repo() {
        let home = HomeIsolata::nuova("consegna-ereditata");
        let repo = home.dir.join("gyver").join("suite");
        let albero = home.dir.join("orca").join("tautog");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&albero).unwrap();
        fs::write(
            albero.join(".git"),
            format!("gitdir: {}/.git/worktrees/tautog\n", repo.display()),
        )
        .unwrap();
        let repo_s = repo.to_string_lossy().to_string();
        // La consegna più recente del repo ha un nome tematico, la vecchia il
        // prefisso: col criterio sul nome vinceva la vecchia.
        doc(&home.dir, &repo_s, "consegna-vecchia.md", CONSEGNA, 3600);
        let fresca = doc(&home.dir, &repo_s, "guardia-e-forma.md", CONSEGNA, 60);
        // Un altro progetto ha consegnato più tardi di tutti: è il documento che
        // il ripiego globale sceglieva.
        let altrui = doc(&home.dir, "/altro/progetto", "consegna-altrui.md", CONSEGNA, 0);

        assert_eq!(latest_handoff(albero.to_str().unwrap()), fresca);
        assert_ne!(latest_handoff(albero.to_str().unwrap()), altrui);
        assert_eq!(latest_handoff(&repo_s), fresca);

        // MUTANTE: una memoria qualunque non è una consegna, nemmeno se è la più recente.
        doc(
            &home.dir,
            &repo_s,
            "nota-di-lavoro.md",
            "---\nname: n\nmetadata:\n  type: reference\n---\n\nappunto\n",
            0,
        );
        assert_eq!(latest_handoff(&repo_s), fresca);
        // MUTANTE: l'indice non è una consegna.
        doc(&home.dir, &repo_s, "MEMORY.md", "# indice\n", 0);
        assert_eq!(latest_handoff(&repo_s), fresca);
        // MUTANTE: senza cartella propria non si eredita la consegna di un altro.
        assert_eq!(latest_handoff("/percorso/senza/memoria"), "");
        // Senza cwd non c'è niente da restringere: lì il ripiego è tutto quello
        // che si può dire.
        assert_eq!(latest_handoff(""), altrui);
    }
}
