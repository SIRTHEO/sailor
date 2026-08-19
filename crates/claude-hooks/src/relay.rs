//! La staffetta: azzera una sessione piena e le rimette in mano il lavoro.
//!
//! Il giudizio — rigenera, salta, congeda, pulisci — sta in
//! `guards::handoff::evaluate`; qui c'è ciò che tocca disco e `orca`.
//!
//! L'ORDINE DELLE CHIAMATE A ORCA È IL COMPORTAMENTO, e non è negoziabile:
//!
//! ```text
//!   0. il freno della catena          la storia, prima del momento: guarda
//!                                     `guards::chain`
//!   1. wait  tui-idle sulla vecchia   non si tronca un turno a metà
//!   2. write riprendi-da/<worktree>   il testimone — consegna, punto di
//!                                     ripresa e mandato — prima di agire
//!   3. send  /clear allo stesso       la sessione riparte vuota sul posto:
//!      pannello                       stesso handle, stessa tab
//!   4. attendi che il segnale         la prova che il mandato è arrivato:
//!      sparisca                       lo consuma il gancio di avvio
//!   5. send  l'avvio                  un turno non parte da solo, e senza
//!                                     questo tutto il resto non produce nulla
//! ```
//!
//! NON SI CREA E NON SI CHIUDE PIÙ NIENTE, ed è la differenza che conta rispetto
//! a com'era fino al 19/08/2026. La vecchia via creava un pannello, ne
//! riconosceva l'handle nella risposta, e se non lo riconosceva lo deduceva
//! dalla differenza fra due elenchi; poi ririsolveva l'handle della vecchia,
//! la chiudeva, e verificava che si fosse chiusa davvero. Cinque punti in cui
//! sbagliare, e hanno sbagliato: 47 sessioni in più in due giorni dal ramo che
//! non riconosceva l'handle, due sessioni sullo stesso albero da una chiusura
//! che colpiva un handle già morto. `/clear` li toglie tutti — misurato sul
//! vivo: la memoria si azzera, il pannello resta, il contesto riparte da 65k.
//!
//! IL VERSO IN CUI SBAGLIARE. Se il `/clear` non parte non si è perso niente: la
//! sessione ha ancora contesto e consegna, si raffredda e si riprova. Se parte
//! ma il segnale non viene raccolto, la sessione è già vuota — e allora il
//! testo dell'avvio **diventa** il mandato, perché una sessione azzerata e
//! lasciata senza incarico è l'unico esito davvero distruttivo di tutta la
//! staffetta.
//!
//! `retire` è l'altro percorso: chiude una sessione senza aprirne una, quando il
//! successore l'ha già aperto qualcun altro. Lì si chiude davvero un pannello, ed
//! è l'unico posto rimasto dove si fa.
//!
//! PERCHÉ `orca` SI INIETTA. `regenerate` non ha un'uscita da confrontare, ha
//! **effetti**: chiamate in un ordine preciso. Chiamando `orca` davvero, una
//! prova su un handle finto diventa verde perché la chiamata è fallita, non
//! perché la guardia ha funzionato — è già successo in questa configurazione,
//! con due mutanti che passavano su 36 prove su 36. Qui il chiamante entra come
//! parametro: le prove registrano la sequenza e la confrontano.

use guards::chain::{chain_verdict, ChainLimits, ChainLink, ChainVerdict};
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

// ─── La storia della catena: la parte che tocca il disco ─────────────────────
//
// La decisione sta in `guards::chain`, pura. Qui c'è dove si scrive, quanto se
// ne tiene, e come si spegne. L'albero di lavoro è la chiave perché è l'unico
// ponte che sopravvive alla sostituzione: la sessione cambia identità a ogni
// anello, il suo pannello pure.

/// Quanti anelli si conservano. Il verdetto ne guarda molti meno; il resto è la
/// storia che si legge quando un freno morde e si vuole sapere perché.
const CHAIN_KEEP: usize = 50;

fn chain_dir() -> PathBuf {
    state_dir().join("catene")
}

fn chain_path(worktree: &str) -> PathBuf {
    chain_dir().join(format!("{}.json", state_key(worktree)))
}

/// Quanti `/clear` di fila senza prova di sostituzione prima di smettere.
///
/// SERVE PERCHÉ UN TENTATIVO NON CONFERMATO NON È PIÙ UN ANELLO DI CATENA, e il
/// freno della catena li contava: finché la pulizia girava comunque, una serie
/// di rigenerazioni mai riuscite arrivava al tetto delle dieci e si fermava da
/// sé. Adesso che non si contano — giustamente, perché non hanno sostituito
/// niente — quel tipo di guasto sarebbe l'unico esente da ogni freno, e la
/// staffetta manderebbe `/clear` allo stesso pannello ogni cinque minuti per
/// sempre, anche a una persona che nel frattempo se l'è ripreso.
const MAX_BLIND_ATTEMPTS: u32 = 3;

/// Il conto dei tentativi di fila rimasti senza prova. Sparisce al primo
/// successo: è una serie, non un totale storico.
fn blind_attempts_path(worktree: &str) -> PathBuf {
    state_dir().join(format!("staffetta-tentativi-ciechi-{}", state_key(worktree)))
}

/// Il marcatore che dice «qui si è smesso di provare, ed ecco perché». Stesso
/// mestiere doppio del marcatore del freno: traccia per Theo e memoria di
/// averlo già detto.
fn blind_stop_path(worktree: &str) -> PathBuf {
    state_dir().join(format!("staffetta-cieca-{}", state_key(worktree)))
}

/// Il marcatore che dice «questa catena è stata fermata, ed ecco perché».
///
/// Fa due mestieri di proposito: è la traccia che Theo legge e insieme la
/// memoria di «l'ho già detto», senza la quale il registro prenderebbe una riga
/// identica ogni sessanta secondi finché qualcuno non interviene.
fn chain_blocked_path(worktree: &str) -> PathBuf {
    state_dir().join(format!("catena-bloccata-{}", state_key(worktree)))
}

/// Il freno si spegne con un file, come `staffetta-off` accanto a cui vive:
/// sotto launchd l'ambiente lo fissa il `.plist`, e una valvola che si accende
/// solo riscrivendo un plist non la usa nessuno.
fn brake_is_off() -> bool {
    state_dir().join("freno-catena-off").exists()
}

fn env_num<T: std::str::FromStr>(name: &str, fallback: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(fallback)
}

fn chain_limits() -> ChainLimits {
    let d = ChainLimits::default();
    ChainLimits {
        max_links: env_num("RELAY_CHAIN_MAX_LINKS", d.max_links),
        max_age_sec: env_num("RELAY_CHAIN_MAX_AGE_SEC", d.max_age_sec),
        idle_reset_sec: env_num("RELAY_CHAIN_IDLE_RESET_SEC", d.idle_reset_sec),
        stall_links: env_num("RELAY_CHAIN_STALL_LINKS", d.stall_links),
    }
}

/// La cartella della copia, presa dall'id e non dal nome del file di stato.
///
/// Un id è `<repoId>::<percorso>`, quindi il percorso sta lì intero. Nel nome
/// del file di stato le barre sono già diventate underscore e tornare indietro è
/// ambiguo: `orca_general` può essere `orca/general` o `orca_general`.
fn worktree_dir(worktree: &str) -> &str {
    match worktree.split_once("::") {
        Some((_, dir)) => dir,
        None => "",
    }
}

/// Quanto la nascita deve superare il primo anello perché l'albero conti come
/// rifatto. NON è prudenza generica: i due istanti vengono dallo stesso orologio
/// letti in momenti diversi, e una correzione all'indietro (NTP) sposterebbe il
/// confine dalla parte sbagliata. Dei due errori possibili, l'unico caro è
/// tagliare una catena viva; due minuti coprono ogni correzione plausibile e non
/// salvano nessun albero davvero rifatto, perché fra lo smontaggio e la
/// ricreazione ne passano molti di più.
const REBORN_MARGIN_SEC: f64 = 120.0;

/// Quando un percorso è nato, come lo legge il Python: secondi dall'epoca.
fn birth_of(path: &Path) -> Option<f64> {
    let born = fs::metadata(path).and_then(|m| m.created()).ok()?;
    Some(born.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64())
}

/// Quando questo albero è stato messo su — non quando è nata la cartella.
///
/// La cartella non basta da sola. Uno smontaggio può lasciarla in piedi
/// (`close-finished.py` stampa `STILL THERE` proprio in quel caso), e se la
/// copia nuova ci viene ricreata dentro la data di nascita non cambia mai: la
/// catena morta verrebbe ereditata per sempre. Il file `.git` di un worktree
/// invece lo riscrive ogni `git worktree add`, quindi vede anche quel caso.
///
/// Si guarda solo se `.git` è un FILE, cioè un worktree registrato: in un
/// checkout principale è una cartella con vita propria — su `other-repo/work` è nata
/// 126 giorni dopo il checkout — e prenderla taglierebbe catene vive. Fra i due
/// si tiene il più recente: un albero rifatto sposta almeno uno dei due segni, e
/// nessuno dei due torna indietro da solo (`worktree repair` e `worktree move`
/// lasciano la nascita del `.git` dov'era, misurato).
fn tree_birth(dir: &str) -> Option<f64> {
    let mut born = birth_of(Path::new(dir))?;
    let g = Path::new(dir).join(".git");
    if fs::metadata(&g).map(|m| m.is_file()).unwrap_or(false) {
        if let Some(gborn) = birth_of(&g) {
            born = born.max(gborn);
        }
    }
    Some(born)
}

/// Vero se l'albero è nato DOPO il primo anello, cioè è un altro albero.
///
/// L'id di una copia è deterministico: smontarla e rifarla con lo stesso nome
/// restituisce lo stesso id, e con esso `catene/<id>.json`. Entro le sei ore
/// della scadenza per inattività la copia nuova erediterebbe i tetti di una
/// lavorazione morta e verrebbe frenata al primo giro — il primo morso del freno
/// si leggerebbe come «funziona» invece che come guasto.
///
/// Senza il segno non si taglia: cartella assente, data di nascita che il
/// sistema non tiene, o un `at` non plausibile lasciano la catena com'è.
fn tree_reborn(worktree: &str, first_at: f64) -> bool {
    let dir = worktree_dir(worktree);
    if dir.is_empty() || !(first_at > 0.0) {
        return false;
    }
    let Some(born) = tree_birth(dir) else {
        return false;
    };
    born > first_at + REBORN_MARGIN_SEC
}

/// Un numero come lo leggerebbe il Python, che chiama `float()` e accetta anche
/// la stringa che lo contiene.
///
/// IL PORTO NON PUÒ ESSERE PIÙ SEVERO DELL'ORACOLO. Con `as_f64()` soltanto, un
/// `"at":"1787053675.22"` diventava zero: l'albero non risultava mai rinato, la
/// catena morta veniva ereditata, e l'età si calcolava dall'epoca — cioè il
/// difetto opposto a quello che la guardia chiude, proprio nel campo su cui
/// decide.
pub(crate) fn link_time(v: Option<&serde_json::Value>) -> f64 {
    match v {
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(serde_json::Value::String(s)) => s.trim().parse::<f64>().unwrap_or(0.0),
        // `float(True)` vale 1.0 anche in Python: un istante assurdo, ma lo
        // stesso assurdo da entrambe le parti.
        Some(serde_json::Value::Bool(b)) => *b as u8 as f64,
        _ => 0.0,
    }
}

/// I conteggi, con la **verità** del Python e non col solo numero.
///
/// `_is_sterile` non guarda quanto vale `writes`: guarda se è falsy, e in Python
/// la stringa `"0"` non lo è — è una stringa non vuota. Un porto che la leggesse
/// come zero conterebbe sterile un anello che l'oracolo dichiara produttivo, e a
/// tre di fila fermerebbe una catena viva. Quindi: il numero quando c'è un
/// numero, altrimenti 1 se il valore sarebbe vero per Python e 0 se sarebbe
/// falso.
///
/// Resta fuori il solo giro completo: riscrivendo la catena, un `"0"` diventa
/// `1`, mentre il Python lo ricopierebbe tale e quale. Nessuno dei due scrive
/// conteggi che non siano interi — li produce `progress()` — e allineare anche
/// quello vorrebbe dire tenere il JSON grezzo dentro `ChainLink`.
pub(crate) fn link_count(v: Option<&serde_json::Value>) -> u64 {
    match v {
        // `as_u64()` fallisce su `5.0` e su `-5`, e non perché valgano zero: non
        // stanno in un `u64`. Ricadere lì su zero direbbe «non ha prodotto» di un
        // anello che per Python ha prodotto eccome — `not 5.0` e `not -5` sono
        // entrambi falsi. Prima la verità, il numero solo quando c'è.
        Some(serde_json::Value::Number(n)) => match n.as_f64().unwrap_or(0.0) {
            f if f == 0.0 => 0,
            _ => n.as_u64().unwrap_or(1).max(1),
        },
        Some(serde_json::Value::String(s)) => match s.trim().parse::<u64>() {
            Ok(n) if n > 0 => n,
            // `"0"`, `"abc"`, `"1.5"`, `"  "`: stringhe non vuote, quindi vere.
            _ if !s.is_empty() => 1,
            _ => 0,
        },
        Some(serde_json::Value::Bool(b)) => *b as u64,
        Some(serde_json::Value::Array(a)) => !a.is_empty() as u64,
        Some(serde_json::Value::Object(o)) => !o.is_empty() as u64,
        _ => 0,
    }
}

/// Gli anelli già percorsi su questo albero. Vuoto anche quando il file è
/// illeggibile: una storia che non si riesce a leggere non deve fermare niente,
/// perché il costo dell'errore qui è una catena viva bloccata al buio.
pub(crate) fn read_chain(worktree: &str) -> Vec<ChainLink> {
    let Ok(text) = fs::read_to_string(chain_path(worktree)) else {
        return Vec::new();
    };
    let Ok(d) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(items) = d.get("links").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let links: Vec<ChainLink> = items
        .iter()
        // Ciò che non è un oggetto non è un anello, e il Python lo scarta.
        // Convertirlo darebbe un anello a `at` zero in testa: una storia più
        // lunga di quella vera, con un'età che sfonda ogni tetto.
        .filter(|l| l.is_object())
        .map(|l| {
            let s = |k: &str| l.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            ChainLink {
                session: s("session"),
                at: link_time(l.get("at")),
                turns: link_count(l.get("turns")),
                writes: link_count(l.get("writes")),
                handoff: s("handoff"),
            }
        })
        .collect();
    match links.first() {
        Some(primo) if tree_reborn(worktree, primo.at) => Vec::new(),
        _ => links,
    }
}

fn write_chain(worktree: &str, links: &[ChainLink]) {
    let _ = fs::create_dir_all(chain_dir());
    let start = links.len().saturating_sub(CHAIN_KEEP);
    let items: Vec<serde_json::Value> = links[start..]
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
    let _ = fs::write(
        chain_path(worktree),
        serde_json::json!({ "links": items }).to_string(),
    );
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
    // `?` e non `Some(...)`: una forma che non si riconosce deve arrivare qui
    // come «non si è potuto leggere», che è il contratto scritto qui sopra.
    // Appiattirla su una lista vuota è esattamente il difetto dei 276 giri.
    Terminal::from_response(&v)
}

// ─── Lo stato che vive quanto l'albero, e chi lo butta ────────────────────────
//
// IL CRITERIO DI CASA È «chi scrive un marcatore sa quando scade, e lo butta
// lui» (`register-session.py::forget_session`), e vale per ciò che è legato a
// una SESSIONE: la sessione finisce, emette un evento, si porta via i suoi file.
//
// Queste quattro famiglie sono legate a un ALBERO, e un albero non emette
// nessuna fine: nessuno dei tre smontatori tocca lo stato, quindi smontare una
// copia lascia dietro la sua catena, la sua tregua e il suo mandato di ripresa.
// La staffetta è l'unica che li scrive tutti, quindi tocca a lei buttarli.

/// Cartella (dentro `state/`), prefisso, suffisso. La chiave sta in mezzo.
const TREE_STATE: &[(&str, &str, &str)] = &[
    ("catene", "", ".json"),
    ("riprendi-da", "", ".txt"),
    ("", "staffetta-cooldown-", ""),
    ("", "catena-bloccata-", ""),
];

/// Quanto dev'essere vecchio un file perché la sua assenza dall'elenco valga
/// come «l'albero non c'è più». Non è prudenza generica: un albero appena creato
/// che per un istante non compare nella risposta di Orca perderebbe la sua
/// catena, e con essa il freno. A un'ora di distanza quel dubbio non esiste più,
/// e tenere quattro file in più per un'ora non costa niente.
const TREE_STATE_GRACE_SEC: f64 = 3600.0;

/// Le chiavi di stato delle copie che Orca conosce, o **None** se non lo so.
///
/// Vale la stessa distinzione di `read_terminals`, e per la stessa ragione: un
/// elenco vuoto qui significherebbe «nessuna copia esiste», cioè butta tutto.
pub(crate) fn live_worktree_keys(orca: OrcaFn) -> Option<Vec<String>> {
    let (rc, out) = orca(&["worktree", "list", "--json"]);
    if rc != 0 || out.is_empty() {
        return None;
    }
    let v = serde_json::from_str::<serde_json::Value>(&out).ok()?;
    // Una risposta che non è un oggetto è una forma che non conosco, e «non lo
    // so» vale più di un'ipotesi. Qui il porto era più permissivo dell'oracolo:
    // su un array nudo prendeva gli elementi come copie di lavoro, mentre il
    // Python sollevava — due comportamenti diversi sulla funzione che decide
    // quali file sopravvivono.
    if !v.is_object() {
        return None;
    }
    let items = v.get("result").unwrap_or(&v).clone();
    let list = match &items {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(_) => items
            .get("worktrees")
            .or_else(|| items.get("items"))
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => return None,
    };
    let keys: Vec<String> = list
        .iter()
        .filter_map(|x| x.get("id").and_then(|i| i.as_str()))
        .filter(|id| !id.is_empty())
        .map(state_key)
        .collect();
    if keys.is_empty() {
        return None;
    }
    Some(keys)
}

/// I file di stato il cui albero non esiste più. Vuoto se non si sa.
///
/// La radice entra come parametro perché le prove non giudichino `state/` di
/// produzione, dove i file sono di chi sta lavorando adesso.
pub(crate) fn orphan_tree_state(
    live_keys: Option<&[String]>,
    now: f64,
    root: &Path,
) -> Vec<PathBuf> {
    let Some(live) = live_keys.filter(|k| !k.is_empty()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (folder, prefix, suffix) in TREE_STATE {
        let base = if folder.is_empty() {
            root.to_path_buf()
        } else {
            root.join(folder)
        };
        let Ok(dir) = fs::read_dir(&base) else {
            continue;
        };
        let mut found: Vec<PathBuf> = dir
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        found.sort();
        for f in found {
            let Some(name) = f.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // `strip_*` invece di tagliare per indice: con `starts_with` e
            // `ends_with` in un `if` a parte, il taglio qui sotto resta corretto
            // solo finché quel controllo esiste — e un mutante che lo toglieva ha
            // fatto **panicare** il binario su `staffetta.log`, cioè un giro di
            // staffetta perso invece di un file saltato. Qui il caso storto è
            // impossibile per costruzione.
            let Some(key) = name.strip_prefix(prefix).and_then(|r| r.strip_suffix(suffix)) else {
                continue;
            };
            if key.is_empty() || live.iter().any(|k| k == key) {
                continue;
            }
            let Ok(when) = fs::metadata(&f).and_then(|m| m.modified()) else {
                continue;
            };
            let Ok(since) = when.duration_since(std::time::UNIX_EPOCH) else {
                continue;
            };
            if now - since.as_secs_f64() < TREE_STATE_GRACE_SEC {
                continue;
            }
            out.push(f);
        }
    }
    out
}

/// Butta lo stato degli alberi che non ci sono più. Ritorna quanti file.
///
/// Si scrive PRIMA di cancellare, come per i record: una cancellazione muta non
/// lascia niente da leggere quando si sbaglia.
pub(crate) fn sweep_tree_state(
    live_keys: Option<&[String]>,
    now: f64,
    dry_run: bool,
    root: &Path,
) -> usize {
    let stale = orphan_tree_state(live_keys, now, root);
    for f in &stale {
        let parent = f
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        log_line(&format!(
            "{} lo stato orfano {parent}/{name}: nessun albero con questa chiave",
            if dry_run { "direi di buttare" } else { "butto" }
        ));
        if !dry_run {
            let _ = fs::remove_file(f);
        }
    }
    stale.len()
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
/// indistinguibile da una che lavora.
///
/// Dal 18/08/2026 questa misura non finisce solo nel registro: è metà del
/// segnale che `guards::chain` usa per riconoscere un anello sterile. L'altra
/// metà è la consegna citata, perché le sole scritture mentirebbero — in
/// modalità automatica i file si toccano da Bash e qui risulterebbero zero.
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

pub fn regenerate(rec: &Record, dry_run: bool, orca: OrcaFn) {
    let sess = &rec.session;

    // 0. IL FRENO, prima di ogni altra cosa e prima di spendere una chiamata a
    //    `orca`. Gli altri controlli guardano questa sessione adesso; questo
    //    guarda le rigenerazioni già fatte su questo albero, che è l'unica cosa
    //    che dice se la catena sta convergendo o girando a vuoto.
    let mut chain = read_chain(&rec.worktree);
    if !brake_is_off() {
        let (verdict, why) = chain_verdict(&chain, now_epoch(), &chain_limits());
        match verdict {
            ChainVerdict::Stop => {
                let marker = chain_blocked_path(&rec.worktree);
                if dry_run {
                    log_line(&format!("[SECCO] FRENO sess={sess}: {why}"));
                    return;
                }
                // Si parla una volta sola: il marcatore è insieme la traccia per
                // Theo e la memoria di averlo già detto. Senza, questa riga
                // tornerebbe ogni sessanta secondi finché non interviene
                // qualcuno, e un registro saturo è un registro cieco.
                if !marker.exists() {
                    let _ = fs::create_dir_all(state_dir());
                    let _ = fs::write(
                        &marker,
                        format!(
                            "{}\n{why}\nalbero: {}\nsessione: {sess}\n",
                            hook_io::local_time::now_local_iso8601(),
                            rec.worktree
                        ),
                    );
                    log_line(&format!(
                        "FRENO sess={sess}: {why}. NON rigenero. Per ripartire: \
                         rm {} {}",
                        chain_path(&rec.worktree).display(),
                        marker.display()
                    ));
                }
                return;
            }
            ChainVerdict::Reset => {
                // La catena precedente è finita da un pezzo: il lavoro di oggi
                // non si giudica coi numeri di ieri.
                chain.clear();
                if !dry_run {
                    let _ = fs::remove_file(chain_blocked_path(&rec.worktree));
                }
                log_line(&format!("sess={sess}: {why}"));
            }
            ChainVerdict::Go => {}
        }
    }

    // 0-bis. si è già provato tre volte senza che nessuno rispondesse: da qui
    //        in poi il `/clear` è solo disturbo a un pannello che magari una
    //        persona ha ripreso in mano.
    if blind_stop_path(&rec.worktree).exists() {
        return;
    }

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
    //
    // IL MANDATO DEL LOOP VIAGGIA COL TESTIMONE. Si arriva qui con un risveglio
    // ancora armato solo quando il contesto è oltre `require` — sotto, la
    // sessione non si tocca proprio — e in quel caso la consegna da sola non
    // basta: dice da dove riprendere, non che si stava girando in `/loop` né su
    // che mandato. Il 19/08/2026 il successore ha ricevuto solo la consegna e
    // il loop è finito lì.
    //
    // IL PUNTO DI RIPRESA, non un documento da cui dedurlo. Il `CLAUDE.md`
    // prescrive che ogni turno chiuda con «**Procedo con** — <il passo
    // successivo>»: è il punto scritto da chi lavorava, nel momento in cui lo
    // sapeva. Fino al 19/08/2026 il successore riceveva solo «leggi la consegna
    // e prosegui», cioè gli si chiedeva di ricavarsi da un riassunto una cosa
    // che era già scritta in chiaro tre righe più in là.
    //
    // SI SCRIVE IN JSON, e non a righe etichettate. Un mandato di `/loop` è
    // spesso un elenco su più righe: un formato a righe lo spezza, e chi lo
    // rilegge riga per riga o lo tronca o lo riattacca senza separatori —
    // «1. leggi X» e «2. fai Y» diventano «1. leggi X2. fai Y». Trovato da un
    // vaglio indipendente il 19/08/2026, ed è il caso per cui questo canale
    // esiste. Il formato vecchio — il solo percorso, senza newline — non è JSON
    // valido e continua a essere letto come tale.
    let hpath = latest_handoff(&rec.cwd);
    let punto = crate::handoff::resume_point(&rec.transcript).unwrap_or_default();
    let mandato = crate::handoff::wakeup_prompt(&rec.transcript).unwrap_or_default();
    if !mandato.is_empty() {
        log_line(&format!(
            "sess={sess}: il mandato del loop viaggia col testimone ({} caratteri)",
            mandato.chars().count()
        ));
    }
    // IL SEGNALE È INDIRIZZATO, non lasciato sull'albero per chiunque. La chiave
    // resta il worktree — la sessione che riparte non ha ancora un nome — ma il
    // corpo dice a quale tab è destinato, e chi non è quella tab non lo consuma.
    //
    // Serviva poco finché il testimone passava da un pannello appena **creato**,
    // evento raro. Adesso ogni rigenerazione manda un `/clear`, e un `/clear`
    // può anche digitarlo una persona: due sessioni sullo stesso albero sono un
    // caso normale — lo dicono `retire` e il tetto delle sessioni — e senza
    // indirizzo la seconda si prenderebbe il punto di ripresa e il `/loop` della
    // prima. La tab è l'identità giusta: `ORCA_TERMINAL_HANDLE` invecchia a ogni
    // riattacco, `ORCA_TAB_ID` no, e `/clear` non la cambia (misurato).
    let corpo = serde_json::json!({
        "handoff": if hpath.is_empty() { "ultimo handoff in memory" } else { &hpath },
        "punto": punto,
        "mandato": mandato,
        "tab": rec.tab_id,
    })
    .to_string();
    let _ = fs::create_dir_all(resume_dir());
    let segnale_scritto =
        fs::write(resume_dir().join(format!("{}.txt", state_key(&rec.worktree))), &corpo)
            .is_ok();

    // 3. AZZERA LA SESSIONE SUL POSTO, invece di crearne un'altra e chiudere
    //    questa.
    //
    // `/clear` svuota la memoria del modello e lascia **il pannello, la tab e
    // l'handle dov'erano**: misurato sul vivo il 19/08/2026 facendo memorizzare
    // un codice a una sessione di prova, mandando `/clear` e richiedendoglielo —
    // risponde di non saperlo, e `terminal list` mostra lo stesso `handle`,
    // `tabId` e `leaf`. Il contesto riparte da 65k, il 13% del budget.
    //
    // COSA SPARISCE DA QUI. La vecchia via era: crea un terminale, riconosci il
    // suo handle nella risposta, e se non lo riconosci deducilo dalla differenza
    // fra due elenchi, poi ririsolvi l'handle della vecchia perché nel frattempo
    // è invecchiato, poi chiudila e guarda se si è chiusa davvero. Cinque punti
    // in cui sbagliare, e tutti e cinque hanno sbagliato almeno una volta: 47
    // sessioni in più in due giorni dal ramo che non riconosceva l'handle, due
    // sessioni sullo stesso albero quando la chiusura colpiva un handle già
    // morto. Nessuno di quei rami esiste più: non c'è niente da creare e niente
    // da chiudere.
    //
    // L'HANDLE SI RIRISOLVE COMUNQUE, ed è l'unica cautela che resta: fra la
    // lettura di inizio giro e questo istante Orca può aver riattaccato il
    // pannello, e mandare `/clear` all'handle sbagliato azzera la sessione di
    // qualcun altro.
    let handle = handle_di_adesso(rec, orca);
    if handle.is_empty() {
        // Il pannello non c'è più: non è un guasto, è una sessione finita per
        // conto suo. Il record lo raccoglie `Action::Clean` al giro dopo.
        log_line(&format!(
            "sess={sess}: il pannello non c'e' piu', niente da azzerare"
        ));
        return;
    }
    // L'istante del `/clear` separa il record del successore da quello di una
    // sessione che viveva su questa tab prima d'ora: i record vecchi restano
    // sul disco finché non li raccoglie qualcuno, e senza data un predecessore
    // passerebbe per prova di sostituzione.
    let clear_at = now_epoch();
    let (rc_clear, out) = orca(&[
        "terminal", "send", "--terminal", &handle, "--text", "/clear", "--enter",
    ]);
    if rc_clear != 0 {
        // NON si è perso niente: la sessione vecchia è ancora lì col suo
        // contesto e la sua consegna già scritta. Si raffredda e si riprova.
        set_cooldown(&rec.worktree);
        log_line(&format!(
            "sess={sess}: /clear non inviato (rc={rc_clear}), la sessione resta \
             com'era (cooldown {COOLDOWN_SEC}s). out={}",
            cut(&out, 400)
        ));
        return;
    }

    // 4. aspetta la PROVA che la sessione azzerata ha in mano il mandato
    //
    // `register-session` è agganciato a `SessionStart` anche per la sorgente
    // `clear`, e **consuma** `riprendi-da/<worktree>.txt` dopo averlo iniettato.
    // Se il file sparisce, il mandato è arrivato: misurato sul vivo, il segnale
    // è stato raccolto pochi secondi dopo il `/clear`.
    let raccolto = wait_for_pickup(&rec.worktree, segnale_scritto);
    // IL SEGNALE CONSUMATO NON È L'UNICA PROVA, ed è una prova negativa: dice
    // che un file è sparito. Quella positiva la scrive già il gancio
    // `register-session` a ogni `SessionStart` — dopo un `/clear` compare un
    // `sessioni-vive/<nuovo>.json` con `source: "clear"` sulla stessa tab — e
    // fino al 19/08/2026 non la leggeva nessuno.
    let heir = registered_heir(rec, clear_at);
    let swapped = raccolto || heir.is_some();

    // 5. FALLA PARTIRE. Il contesto iniettato da un gancio non avvia nessun
    //    turno: dopo il `/clear` la sessione resta al prompt vuoto, e lì
    //    resterebbe finché non le scrive una persona. È il difetto che il
    //    19/08/2026 ha lasciato una ripresa ferma ad aspettare un mandato che
    //    aveva già in mano — nove byte bastano ad avviarla, e senza quei nove
    //    byte tutto il resto di questa funzione non produce lavoro.
    //
    // Quando il segnale NON è stato raccolto il testo diventa il mandato
    // stesso: è l'ultimo canale rimasto, e a quel punto la sessione è già
    // azzerata — lasciarla senza incarico sarebbe l'unico esito davvero
    // distruttivo di tutta la staffetta.
    let avvio = if raccolto {
        "riprendi dal punto di ripresa che hai ricevuto".to_string()
    } else {
        // A voce si dà il testo, non il JSON: chi legge è un modello in una
        // TUI, e le newline dentro un `--text` le mangia il terminale.
        let mut voce = format!(
            "RIPARTENZA (staffetta). Il segnale non e' stato raccolto, quindi te \
             lo do a voce. Leggi la consegna: {}.",
            if hpath.is_empty() { "ultimo handoff in memory" } else { &hpath }
        );
        if !punto.is_empty() {
            voce.push_str(&format!(" RIPRENDI DA QUI: {}", punto.replace('\n', " · ")));
        }
        if !mandato.is_empty() {
            voce.push_str(&format!(" MANDATO: {}", mandato.replace('\n', " · ")));
        }
        voce
    };
    let (rc_send, _) = orca(&[
        "terminal", "send", "--terminal", &handle, "--text", &avvio, "--enter",
    ]);
    log_line(&format!(
        "sess={sess}: azzerata sul posto, mandato {} (avvio rc={rc_send}, \
         sostituzione {})",
        if raccolto { "raccolto dal segnale" } else { "dato a voce" },
        match (&heir, raccolto) {
            (Some(nuova), _) => format!("confermata dal record di {nuova}"),
            (None, true) => "confermata dal segnale raccolto".to_string(),
            (None, false) => "NON confermata".to_string(),
        }
    ));

    // 6. pulisci lo stato della sessione sostituita — SOLO se è stata davvero
    //    sostituita. Il pannello non si tocca: è lo stesso, e adesso ci vive la
    //    sessione nuova.
    //
    // NEL VERSO GIUSTO IN CUI SBAGLIARE. Fino al 19/08/2026 questa pulizia
    // girava comunque: `raccolto` sceglieva una parola nel registro e nient'altro,
    // e alle 14:30 `cbcded8a` è stata annotata `RIGENERATA` mentre continuava a
    // lavorare. Un record che resta fa riprovare la staffetta dopo la tregua; un
    // record cancellato per errore rende la sessione invisibile per sempre,
    // perché `sessioni-vive/<sess>.json` si riscrive solo a `SessionStart` — e se
    // la sessione non è ripartita, quel momento non arriva più.
    if !swapped {
        // Niente anello di catena: un tentativo fallito non è un giro, e
        // contarlo farebbe mordere il freno mentre non si sostituisce niente.
        // Il conto lo tiene un contatore suo, che ha il proprio tetto.
        set_cooldown(&rec.worktree);
        let n = mark_blind_attempt(&rec.worktree);
        log_line(&format!(
            "RIGENERAZIONE NON CONFERMATA sess={sess} ({n}/{MAX_BLIND_ATTEMPTS}): \
             /clear inviato a {handle}, ma il segnale non e' stato raccolto e \
             nessuna sessione nuova si e' registrata qui. Stato e marcatori \
             restano dov'erano, si riprova fra {COOLDOWN_SEC}s."
        ));
        if n >= MAX_BLIND_ATTEMPTS {
            let marker = blind_stop_path(&rec.worktree);
            let _ = fs::create_dir_all(state_dir());
            let _ = fs::write(
                &marker,
                format!(
                    "{}\n{n} tentativi di sostituzione senza prova, di fila.\nalbero: {}\n\
                     ultima sessione: {sess}\npannello: {handle}\n",
                    hook_io::local_time::now_local_iso8601(),
                    rec.worktree
                ),
            );
            log_line(&format!(
                "STAFFETTA CIECA su {}: {n} tentativi di fila senza prova che la \
                 sostituzione sia avvenuta. NON provo piu'. Per ripartire: rm {} {}",
                rec.worktree,
                marker.display(),
                blind_attempts_path(&rec.worktree).display()
            ));
        }
        return;
    }
    // La serie si azzera al primo successo: tre tentativi ciechi sparsi in una
    // giornata non sono il guasto che questo tetto cerca.
    let _ = fs::remove_file(blind_attempts_path(&rec.worktree));
    let _ = fs::remove_file(live_dir().join(format!("{sess}.json")));
    // LA LISTA È UNA SOLA, e vive in `register_session`. Qui ne esistevano due
    // copie da cinque famiglie contro otto: mancavano `consegna-volontaria`,
    // `consegna-fatta-ripartenze` e `consegna-ripartenze`. La prima costa cara —
    // dice «questa consegna è stata una scelta, non un ordine della soglia», e
    // `guards::handoff` la lascia scavalcare la guardia del «sotto soglia».
    // Sopravvivendo alla rigenerazione restava vera per sempre: il 19/08/2026
    // alle 15:38 ha fatto azzerare una sessione al 21% del budget (104.724 token).
    // È lo stesso difetto che il commento di quella lista dichiarava chiuso.
    for family in crate::register_session::MARKER_FAMILIES {
        let _ = fs::remove_file(state_dir().join(format!("{family}-{sess}")));
    }
    set_cooldown(&rec.worktree);
    let (turns, writes) = progress(&rec.transcript);
    // L'anello si scrive solo qui, dove la rigenerazione è davvero avvenuta: un
    // tentativo fallito non è un giro di catena, e contarlo farebbe mordere il
    // tetto proprio mentre la staffetta non sta sostituendo niente.
    chain.push(ChainLink {
        session: sess.clone(),
        at: now_epoch(),
        turns,
        writes,
        handoff: hpath.clone(),
    });
    write_chain(&rec.worktree, &chain);
    let _ = fs::remove_file(chain_blocked_path(&rec.worktree));
    log_line(&format!(
        "RIGENERATA sess={sess}: azzerata sul posto su {handle} (handoff={}, \
         prodotto: {turns} turni, {writes} scritture, anello {} della catena)",
        if hpath.is_empty() { "-" } else { &hpath },
        chain.len()
    ));
}

/// Segna un tentativo rimasto senza prova e ritorna a quanti si è arrivati.
///
/// Un file di stato e non la memoria del processo: la staffetta è un comando che
/// launchd rilancia ogni sessanta secondi, e fra un giro e l'altro non
/// sopravvive niente.
fn mark_blind_attempt(worktree: &str) -> u32 {
    let path = blind_attempts_path(worktree);
    let n = fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0)
        + 1;
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(&path, n.to_string());
    n
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

/// Il record di una sessione **nuova** comparso qui dopo il `/clear`: la prova
/// positiva che la sostituzione è avvenuta. Ritorna il suo nome corto.
///
/// SI GUARDA LA TAB, non l'handle: `/clear` non la cambia, mentre l'handle
/// invecchia a ogni riattacco del pannello.
///
/// E SENZA TAB NON SI RIPIEGA SUL WORKTREE, che pure è la chiave con cui la
/// staffetta ragiona ovunque: `register_session` scrive un record valido con la
/// sola delle due chiavi che ha (`register_session.rs:103`), e due sessioni
/// sullo stesso albero sono un caso normale — lo dicono `retire` e il tetto
/// delle sessioni. Col worktree per identità basterebbe che una persona
/// digitasse `/clear` su un'altra tab dello stesso albero nello stesso minuto
/// per far passare per erede una sessione che non c'entra, cioè per riaprire il
/// falso positivo che questa funzione esiste per chiudere. Senza tab la prova
/// positiva non si dà: si ricade sul segnale, e nel dubbio non si cancella
/// niente.
///
/// `source == "clear"` esclude una sessione aperta a mano sullo stesso posto
/// nello stesso momento, e `updated_at` esclude i predecessori: è in secondi
/// interi, quindi si confronta col secondo del `/clear` troncato — un record
/// scritto mezzo secondo dopo porta lo stesso numero, e pretenderlo maggiore
/// stretto lo scarterebbe.
fn registered_heir(rec: &Record, clear_at: f64) -> Option<String> {
    if rec.tab_id.is_empty() {
        return None;
    }
    let threshold = clear_at.max(0.0).floor() as u64;
    for entry in fs::read_dir(live_dir()).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(d) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let field = |k: &str| d.get(k).and_then(|v| v.as_str()).unwrap_or("");
        if field("session_id") == rec.session_id || field("source") != "clear" {
            continue;
        }
        if field("tab_id") != rec.tab_id {
            continue;
        }
        if d.get("updated_at").and_then(|v| v.as_u64()).unwrap_or(0) >= threshold {
            return Some(field("session_id").chars().take(8).collect());
        }
    }
    None
}

/// L'handle su cui vive **adesso** la sessione del record, o vuoto se non c'è.
///
/// NON SI RIUSA L'HANDLE DEL RECORD. Invecchia a ogni riattacco del pannello, e
/// fra la lettura di inizio giro e il momento di agire passa l'attesa dell'idle.
/// Misurato il 18/08/2026 sulla sessione `6bd74afa`: la staffetta ha chiuso
/// `term_49e690ff`, già morto, mentre quella sessione viveva su `term_ca8f3dab`
/// e ha continuato a girare — due sessioni sullo stesso albero.
///
/// Se l'elenco non si legge si risponde con l'handle del record: è la scelta
/// meno peggio, perché «non lo so» qui significherebbe non agire mai.
fn handle_di_adesso(rec: &Record, orca: OrcaFn) -> String {
    match read_terminals(orca) {
        Some(ts) => resolve_terminal_handle(&rec.tab_id, &rec.worktree, &rec.handle, &ts),
        None => rec.handle.clone(),
    }
}

/// Chiude la scheda della sessione sostituita, e guarda se si è chiusa davvero.
///
/// L'HANDLE VA RIRISOLTO QUI, non riusato. Invecchia a ogni riattacco del
/// pannello, e fra la lettura di inizio giro e questo momento passa l'attesa del
/// pickup — venticinque secondi in cui Orca può aver riattaccato la scheda.
/// Misurato il 18/08/2026 sulla sessione `6bd74afa`: la staffetta ha chiuso
/// `term_49e690ff`, già morto, mentre quella sessione viveva su `term_ca8f3dab`
/// e ha continuato a girare — due sessioni sullo stesso albero, e
/// `register-session.py` ha riscritto il record col nuovo handle, rimettendola in
/// fila per la prossima rigenerazione.
///
/// E L'ESITO SI GUARDA. Prima si chiamava `orca terminal close` scartando la
/// risposta: «ho chiuso una scheda morta» era indistinguibile da «ho chiuso la
/// vecchia», ed è lo stesso difetto già corretto per il `send` due passi più su.
/// La prova è rileggere l'elenco: se l'handle è ancora fra i vivi, la chiusura
/// non è avvenuta e lo si scrive.
fn close_old_panel(rec: &Record, sess: &str, orca: OrcaFn) -> Chiusura {
    let handle = handle_di_adesso(rec, orca);
    if handle.is_empty() {
        log_line(&format!(
            "sess={sess}: la scheda vecchia non c'è già più, niente da chiudere"
        ));
        return Chiusura::NonCera;
    }
    let (rc, _) = orca(&["terminal", "close", "--terminal", &handle]);
    match read_terminals(orca) {
        None => {
            // L'elenco non si rilegge: resta la parola del comando. È l'unico
            // ramo in cui si crede a un `rc`, e si crede solo a quello — un
            // `rc` diverso da zero qui è un fallimento dichiarato, non un
            // dubbio.
            log_line(&format!(
                "sess={sess}: chiusa la scheda {handle} (rc={rc}), esito non verificabile"
            ));
            if rc == 0 { Chiusura::Fatta } else { Chiusura::Fallita }
        }
        Some(ts) if ts.iter().any(|t| t.handle == handle) => {
            log_line(&format!(
                "sess={sess}: LA SCHEDA {handle} NON SI E' CHIUSA (rc={rc}): \
                 restano due sessioni su {}",
                rec.worktree
            ));
            Chiusura::Fallita
        }
        Some(_) => Chiusura::Fatta,
    }
}

/// Cos'è successo alla scheda che si voleva chiudere.
///
/// TRE ESITI, NON DUE. «Non c'era più» e «l'ho chiusa» portano entrambi avanti
/// il congedo, ma non sono la stessa cosa da scrivere nel registro: fondendoli
/// in un `bool`, una riga affermava «chiusa `term_x`» per una chiamata a `close`
/// mai fatta. Chi indaga un guasto legge quella riga come un fatto.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Chiusura {
    /// Il `close` è partito e la scheda non è più nell'elenco.
    Fatta,
    /// Non c'era niente da chiudere: il pannello era già sparito.
    NonCera,
    /// La scheda è ancora viva, o il comando ha dichiarato di non averla chiusa.
    Fallita,
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
    // L'ESITO DELLA CHIUSURA DECIDE, invece di essere calcolato e buttato.
    //
    // `close_old_panel` ririsolve l'handle e rilegge l'elenco apposta per
    // rispondere «si è chiusa davvero?», ed è nata dall'incidente `6bd74afa` —
    // una scheda stantia chiusa mentre la sessione viveva altrove. Proseguire su
    // un `false` cancella il record di una sessione ancora viva: da lì in poi
    // gira **non tracciata**, accanto a chi ha preso il suo posto, e nessun giro
    // successivo la guarda più. Trovato da un vaglio indipendente il
    // 19/08/2026: il controllo c'era, il verdetto pure, e nessuno lo leggeva.
    let esito = close_old_panel(rec, sess, orca);
    if esito == Chiusura::Fallita {
        set_cooldown(&rec.worktree);
        log_line(&format!(
            "sess={sess}: congedo annullato, la sessione e' ancora viva \
             (cooldown {COOLDOWN_SEC}s)"
        ));
        return;
    }
    let _ = fs::remove_file(live_dir().join(format!("{sess}.json")));
    // LA LISTA È UNA SOLA, e vive in `register_session`. Qui ne esistevano due
    // copie da cinque famiglie contro otto: mancavano `consegna-volontaria`,
    // `consegna-fatta-ripartenze` e `consegna-ripartenze`. La prima costa cara —
    // dice «questa consegna è stata una scelta, non un ordine della soglia», e
    // `guards::handoff` la lascia scavalcare la guardia del «sotto soglia».
    // Sopravvivendo alla rigenerazione restava vera per sempre: il 19/08/2026
    // alle 15:38 ha fatto azzerare una sessione al 21% del budget (104.724 token).
    // È lo stesso difetto che il commento di quella lista dichiarava chiuso.
    for family in crate::register_session::MARKER_FAMILIES {
        let _ = fs::remove_file(state_dir().join(format!("{family}-{sess}")));
    }
    set_cooldown(&rec.worktree);
    // ANCHE QUESTO È UN ANELLO, e ometterlo aprirebbe la porta di servizio: il
    // congedo sostituisce una sessione esattamente come la rigenerazione, solo
    // che il successore l'ha aperto qualcun altro. Contando i soli `Regenerate`,
    // una catena che passa di qui cresce senza che nessun tetto la veda — ed è
    // un percorso vivo, non teorico: una sostituzione su tredici nel registro
    // del 17-18/08/2026, e in crescita da quando esiste il congedo.
    //
    // Non si frena, si conta. Frenare un congedo lascerebbe due sessioni vive
    // sullo stesso albero, cioè più di quante ce n'erano prima: il freno esiste
    // per chi apre, non per chi chiude.
    let (turns, writes) = progress(&rec.transcript);
    let mut chain = read_chain(&rec.worktree);
    chain.push(ChainLink {
        session: sess.clone(),
        at: now_epoch(),
        turns,
        writes,
        // Senza questa lettura ogni anello da congedo avrebbe consegna vuota, e
        // una consegna vuota non è mai sterile: il rilevamento dello stallo si
        // spegnerebbe proprio sul percorso che non ha freni.
        handoff: latest_handoff(&rec.cwd),
    });
    write_chain(&rec.worktree, &chain);
    log_line(&format!(
        "CONGEDATA sess={sess}: {} senza aprirne un'altra \
         (prodotto: {turns} turni, {writes} scritture, anello {} della catena)",
        match esito {
            Chiusura::Fatta => format!("chiusa {}", rec.handle),
            Chiusura::NonCera => "il pannello era gia' sparito".to_string(),
            Chiusura::Fallita => unreachable!("il ramo fallito esce sopra"),
        },
        chain.len()
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
            ($thresholds:expr, $used:expr, $lavorato:expr, $sveglia:expr) => {
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
                    wakeup_in: $sveglia,
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
        let (action, reason) = match evaluate(&fatti!(None, 0, false, None)) {
            (Action::Skip, r) if r == "soglie non calcolabili" => {
                t = crate::handoff::thresholds(&rec.transcript);
                let used = crate::handoff::context_used(&rec.transcript, &rec.session);
                // Si scorre la coda una seconda volta, e solo qui: è la stessa
                // ragione per cui la misura sta in questo ramo e non sopra.
                let lavorato =
                    crate::handoff::worked_after_handoff(&rec.transcript, &rec.session);
                // Stessa coda, stessa passata: il risveglio armato si legge qui
                // e non prima, per la ragione già scritta sopra.
                let sveglia =
                    crate::handoff::wakeup_pending(&rec.transcript, now as f64);
                evaluate(&fatti!(Some(&t), used, lavorato, sveglia))
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
                regenerate(&rec, dry_run, orca);
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

    // Alla fine, e non all'inizio: se un giro rigenera, la copia nuova è già
    // nell'elenco quando si guarda chi è rimasto senza albero.
    let keys = live_worktree_keys(orca);
    sweep_tree_state(keys.as_deref(), now, dry_run, &state_dir());
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
    fn the_cut_counts_characters_not_bytes() {
        // Lo slicing di Python conta caratteri: su un messaggio accentato un
        // taglio a byte spezzerebbe una lettera a metà.
        assert_eq!(cut("perché", 5), "perch");
        assert_eq!(cut("abc", 10), "abc");
    }

    fn test_record() -> Record {
        Record {
            // Il nome corto sono i primi otto caratteri del lungo, come lo
            // ricava `read_record`: tenerli scollegati faceva scrivere il record
            // di prova sotto un nome che poi nessuno andava a cercare.
            session_id: "provarel-0000-0000".into(),
            session: "provarel".into(),
            handle: "term_vecchio".into(),
            worktree: "wt-prova".into(),
            tab_id: "tab-1".into(),
            transcript: String::new(),
            cwd: "/x".into(),
        }
    }

    #[test]
    fn the_session_is_cleared_in_place() {
        let _home = HomeIsolata::nuova("azzera-sul-posto");
        // L'invariante che ha sostituito «crea prima di chiudere»: non si crea
        // e non si chiude niente. Erano i due gesti che sbagliavano — 47
        // sessioni in più in due giorni da un create riuscito e non
        // riconosciuto, e due sessioni sullo stesso albero da una chiusura che
        // colpiva un handle già morto.
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            (0, String::new())
        };
        regenerate(&test_record(), false, &mut orca);
        let seq = chiamate.borrow().clone();
        assert!(!seq.iter().any(|c| c.contains("create")), "ha creato: {seq:?}");
        assert!(!seq.iter().any(|c| c.contains("close")), "ha chiuso: {seq:?}");
        let clear = seq.iter().find(|c| c.contains("/clear")).expect("nessun /clear");
        assert!(clear.contains("term_vecchio"), "azzerata la sessione sbagliata: {clear}");
    }

    #[test]
    fn a_clear_that_never_went_out_leaves_the_session_as_it_was() {
        let _home = HomeIsolata::nuova("clear-fallito");
        // Il verso sicuro: se il comando non parte non si è perso niente — la
        // sessione ha ancora il suo contesto e la sua consegna già scritta. Si
        // raffredda e si riprova, invece di insistere sullo stesso giro.
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            if args.contains(&"/clear") { (1, "boom".into()) } else { (0, String::new()) }
        };
        regenerate(&test_record(), false, &mut orca);
        let seq = chiamate.borrow().clone();
        let send = seq.iter().filter(|c| c.contains("send")).count();
        assert_eq!(send, 1, "ha insistito dopo un /clear non inviato: {seq:?}");
        let log = fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert!(log.contains("/clear non inviato"), "{log}");
        assert!(!log.contains("RIGENERATA"), "ha contato una rigenerazione mai avvenuta: {log}");
    }

    #[test]
    fn a_session_that_is_not_idle_is_left_alone() {
        let _home = HomeIsolata::nuova("non-idle");
        // Primo comando: `wait`. Se fallisce, non deve seguire nient'altro —
        // troncare un turno a metà costa il lavoro in corso.
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            (1, String::new())
        };
        regenerate(&test_record(), false, &mut orca);
        let seq = chiamate.borrow().clone();
        assert_eq!(seq.len(), 1, "dopo un wait fallito non si fa altro: {seq:?}");
        assert!(seq[0].contains("wait"));
    }

    #[test]
    fn once_the_signal_is_picked_up_only_the_start_is_sent() {
        let _home = HomeIsolata::nuova("mandato-raccolto");
        // Il caso buono. `register-session` è agganciato al `SessionStart` della
        // sorgente `clear` e consuma `riprendi-da/<worktree>.txt` dopo averlo
        // iniettato: sparito il file, il mandato è in mano alla sessione nuova.
        //
        // MA UN TURNO VA AVVIATO LO STESSO. Il contesto iniettato da un gancio
        // non fa partire niente: misurato sul vivo il 19/08/2026, dopo il
        // `/clear` la sessione resta al prompt vuoto con il mandato già dentro.
        // Sono nove byte, e senza di loro tutta la staffetta non produce lavoro.
        std::env::set_var("RELAY_PICKUP_TIMEOUT_SEC", "5");
        let segnale = resume_dir().join(format!("{}.txt", state_key("wt-prova")));
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            if args.contains(&"/clear") {
                // La sessione riparte e raccoglie: è ciò che fa il gancio di
                // avvio, qui al momento giusto invece che da un altro processo.
                let _ = fs::remove_file(&segnale);
            }
            (0, String::new())
        };
        regenerate(&test_record(), false, &mut orca);
        let seq = chiamate.borrow().clone();
        // Due `send`: il `/clear` e l'avvio. Contarne uno solo lascerebbe
        // passare la versione che azzera e non fa ripartire — e «l'ultimo
        // comando contiene send» è vero anche del `/clear`.
        let send: Vec<&String> = seq.iter().filter(|c| c.contains("send")).collect();
        assert_eq!(send.len(), 2, "non ha avviato il turno dopo il /clear: {seq:?}");
        assert!(send[0].contains("/clear"), "{seq:?}");
        assert!(
            !send[1].contains("RIPARTENZA"),
            "ha ridettato a voce un mandato già raccolto: {}",
            send[1]
        );
        let log =
            fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert!(log.contains("raccolto dal segnale"), "{log}");
        assert!(log.contains("RIGENERATA"), "{log}");
    }

    #[test]
    fn the_signal_carries_both_the_resume_point_and_the_mandate() {
        let _home = HomeIsolata::nuova("segnale-completo");
        // Il contenuto del segnale non lo guardava nessuno: toglierne il punto
        // di ripresa non faceva cadere niente, e il successore sarebbe tornato
        // a dedurlo da un documento — cioè al difetto del 19/08/2026.
        std::env::set_var("RELAY_PICKUP_TIMEOUT_SEC", "0");
        let transcript = _home.dir.join("t.jsonl");
        let righe = [
            serde_json::json!({
                "type": "assistant",
                "timestamp": "2026-08-18T23:30:15.814Z",
                "message": {"content": [{"type": "tool_use", "name": "ScheduleWakeup",
                                         "input": {"delaySeconds": 1500,
                                                   "prompt": "/loop Sistema la configurazione"}}]}
            })
            .to_string(),
            serde_json::json!({
                "type": "assistant",
                "timestamp": "2026-08-18T23:30:23.608Z",
                "message": {"content": [{"type": "text",
                                         "text": "**Procedo con** — la staffetta via /clear"}]}
            })
            .to_string(),
        ];
        fs::write(&transcript, righe.join("\n")).unwrap();
        let rec = Record {
            transcript: transcript.to_string_lossy().into_owned(),
            ..test_record()
        };
        let mut orca = |_: &[&str]| -> (i32, String) { (0, String::new()) };
        regenerate(&rec, false, &mut orca);
        // Il segnale resta sul disco: nessuno l'ha raccolto in questo caso.
        let corpo = fs::read_to_string(
            resume_dir().join(format!("{}.txt", state_key("wt-prova"))),
        )
        .unwrap_or_default();
        let d: serde_json::Value = serde_json::from_str(&corpo)
            .unwrap_or_else(|e| panic!("segnale non è JSON ({e}):\n{corpo}"));
        assert_eq!(d["punto"].as_str().unwrap_or(""), "**Procedo con** — la staffetta via /clear");
        assert_eq!(d["mandato"].as_str().unwrap_or(""), "/loop Sistema la configurazione");
        assert!(d["handoff"].as_str().unwrap_or("").ends_with(".md")
            || d["handoff"] == "ultimo handoff in memory", "{corpo}");
    }

    #[test]
    fn a_signal_left_unread_makes_the_mandate_said_out_loud() {
        let _home = HomeIsolata::nuova("mandato-a-voce");
        // Qui la sessione è GIÀ azzerata: lasciarla senza incarico sarebbe
        // l'unico esito davvero distruttivo della staffetta — un pannello vuoto
        // che ha buttato il contesto e non sa cosa fare. Il testo dell'avvio
        // diventa allora il mandato stesso.
        std::env::set_var("RELAY_PICKUP_TIMEOUT_SEC", "0");
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            (0, String::new())
        };
        regenerate(&test_record(), false, &mut orca);
        let seq = chiamate.borrow().clone();
        let avvio = seq.last().expect("nessun comando");
        assert!(
            avvio.contains("RIPARTENZA"),
            "sessione azzerata e lasciata senza incarico: {seq:?}"
        );
        let log =
            fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert!(log.contains("dato a voce"), "{log}");
        // E NON «RIGENERATA»: qui nessuno ha raccolto il segnale e nessuna
        // sessione nuova si è registrata, quindi la sostituzione non è provata.
        // Il mandato a voce parte lo stesso — è l'ultimo canale rimasto — ma lo
        // stato della sessione resta dov'era.
        assert!(log.contains("RIGENERAZIONE NON CONFERMATA"), "{log}");
        assert!(!log.contains("RIGENERATA sess="), "{log}");
    }

    #[test]
    fn a_regeneration_nobody_confirms_leaves_the_session_visible() {
        let _home = HomeIsolata::nuova("sostituzione-non-provata");
        // IL 19/08/2026 ALLE 14:30 `cbcded8a` È STATA DICHIARATA SOSTITUITA
        // MENTRE CONTINUAVA A LAVORARE: la pulizia girava comunque, e il record
        // cancellato non torna più — si riscrive solo a `SessionStart`, che per
        // una sessione mai ripartita non arriva mai. Meglio riprovare fra cinque
        // minuti che perdere di vista una sessione per sempre.
        std::env::set_var("RELAY_PICKUP_TIMEOUT_SEC", "0");
        write_live_record("provarel-0000-0000", "tab-1", "startup", now_epoch() as u64);
        let marker = state_dir().join("consegna-volontaria-provarel");
        let _ = fs::create_dir_all(state_dir());
        fs::write(&marker, "").unwrap();
        let mut orca = |_args: &[&str]| -> (i32, String) { (0, String::new()) };
        regenerate(&test_record(), false, &mut orca);
        assert!(
            live_dir().join("provarel.json").exists(),
            "la sessione è stata resa invisibile senza prova che fosse ripartita"
        );
        assert!(marker.exists(), "i marcatori sono stati raccolti a vuoto");
        assert!(read_chain("wt-prova").is_empty(), "un tentativo fallito ha contato come anello");
    }

    #[test]
    fn a_new_session_on_the_same_tab_proves_the_swap() {
        let _home = HomeIsolata::nuova("prova-positiva");
        // La prova positiva, quando quella negativa non c'è: il segnale è
        // rimasto lì (magari il successore l'ha letto senza consumarlo, magari
        // non è mai stato scritto), ma un `SessionStart` con `source: "clear"`
        // si è registrato su questa tab dopo il `/clear`. La sostituzione è
        // avvenuta, e lo stato vecchio si può raccogliere.
        std::env::set_var("RELAY_PICKUP_TIMEOUT_SEC", "0");
        write_live_record("provarel-0000-0000", "tab-1", "startup", now_epoch() as u64);
        let mut orca = |args: &[&str]| -> (i32, String) {
            if args.contains(&"/clear") {
                write_live_record("erede-000-0000", "tab-1", "clear", now_epoch() as u64);
            }
            (0, String::new())
        };
        regenerate(&test_record(), false, &mut orca);
        assert!(
            !live_dir().join("provarel.json").exists(),
            "sostituzione provata dal record dell'erede, e lo stato vecchio è rimasto"
        );
        assert_eq!(read_chain("wt-prova").len(), 1, "l'anello non è stato registrato");
        let log =
            fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert!(log.contains("confermata dal record di erede-00"), "{log}");
    }

    #[test]
    fn without_a_tab_there_is_no_positive_proof() {
        let _home = HomeIsolata::nuova("senza-tab-niente-prova");
        // `register_session` scrive un record valido anche con la sola delle due
        // chiavi (`register_session.rs:103`), quindi una sessione senza tab
        // esiste. Ripiegare sul worktree per riconoscerne l'erede riaprirebbe il
        // falso positivo: due sessioni sullo stesso albero sono un caso normale,
        // e basterebbe un `/clear` digitato a mano su un'altra tab dello stesso
        // albero per far passare per erede una sessione che non c'entra.
        let mut rec = test_record();
        rec.tab_id = String::new();
        write_live_record("altra-tab-0000", "tab-9", "clear", now_epoch() as u64 + 5);
        assert!(
            registered_heir(&rec, now_epoch()).is_none(),
            "una sessione di un'altra tab è passata per erede solo perché condivide l'albero"
        );
    }

    #[test]
    fn a_record_written_in_the_same_second_still_counts() {
        let _home = HomeIsolata::nuova("stesso-secondo");
        // `updated_at` è in secondi interi: un erede registrato mezzo secondo
        // dopo il `/clear` porta lo stesso numero della soglia. Pretenderlo
        // maggiore stretto lo scarterebbe, e sarebbe un falso negativo a ogni
        // rigenerazione svelta.
        let now = now_epoch();
        write_live_record("erede-000-0000", "tab-1", "clear", now as u64);
        assert_eq!(
            registered_heir(&test_record(), now).as_deref(),
            Some("erede-00")
        );
    }

    #[test]
    fn a_broken_record_does_not_stop_the_search() {
        let _home = HomeIsolata::nuova("record-illeggibile");
        // Un file troncato a metà scrittura non deve né far cadere la staffetta
        // né nascondere l'erede che sta nel file dopo.
        let _ = fs::create_dir_all(live_dir());
        fs::write(live_dir().join("aaa-rotto.json"), "{\"session_id\": \"tron").unwrap();
        fs::write(live_dir().join("bbb-vuoto.json"), "").unwrap();
        write_live_record("erede-000-0000", "tab-1", "clear", now_epoch() as u64 + 5);
        assert_eq!(
            registered_heir(&test_record(), now_epoch()).as_deref(),
            Some("erede-00")
        );
    }

    #[test]
    fn three_blind_attempts_stop_the_relay() {
        let _home = HomeIsolata::nuova("staffetta-cieca");
        // IL FRENO DELLA CATENA NON LI VEDE PIÙ, ed è la conseguenza di non
        // contarli come anelli: senza un tetto proprio, un guasto che non
        // conferma mai — il gancio che non parte, un pannello che non torna —
        // farebbe piovere un `/clear` ogni cinque minuti su un terminale che nel
        // frattempo può essere tornato in mano a una persona.
        std::env::set_var("RELAY_PICKUP_TIMEOUT_SEC", "0");
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            (0, String::new())
        };
        for _ in 0..5 {
            regenerate(&test_record(), false, &mut orca);
        }
        let clear = chiamate.borrow().iter().filter(|c| c.contains("/clear")).count();
        assert_eq!(clear, 3, "la staffetta ha continuato a provare: {clear} volte");
        assert!(blind_stop_path("wt-prova").exists(), "manca la traccia per Theo");
        let log =
            fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert_eq!(log.matches("STAFFETTA CIECA").count(), 1, "l'ha ripetuto: {log}");
        assert!(log.contains("Per ripartire: rm "), "non dice come si riparte: {log}");
    }

    #[test]
    fn one_confirmed_swap_clears_the_blind_count() {
        let _home = HomeIsolata::nuova("serie-azzerata");
        // È una serie, non un totale storico: due tentativi andati a vuoto in
        // giornata, poi uno riuscito, non devono lasciare la staffetta a un
        // passo dal proprio tetto.
        std::env::set_var("RELAY_PICKUP_TIMEOUT_SEC", "0");
        let mut unanswered = |_args: &[&str]| -> (i32, String) { (0, String::new()) };
        regenerate(&test_record(), false, &mut unanswered);
        regenerate(&test_record(), false, &mut unanswered);
        assert!(blind_attempts_path("wt-prova").exists());
        let (_, mut orca) = orca_that_records();
        regenerate(&test_record(), false, &mut orca);
        assert!(
            !blind_attempts_path("wt-prova").exists(),
            "la serie non si è azzerata dopo una sostituzione provata"
        );
    }

    #[test]
    fn only_a_session_born_after_the_clear_counts_as_the_heir() {
        let _home = HomeIsolata::nuova("erede-solo-se-nuovo");
        // I record non si cancellano da soli: su una tab che ha già ospitato
        // cinque sessioni ce ne sono cinque, tutti con `source: "clear"`. Senza
        // la data, il primo predecessore letto dalla cartella basterebbe a
        // dichiarare avvenuta una sostituzione che non è avvenuta.
        let now = now_epoch() as u64;
        let rec = test_record();
        // MUTANTE: stessa tab, `source` giusto, ma di prima del `/clear`.
        write_live_record("vecchia-0-0000", "tab-1", "clear", now - 600);
        assert!(registered_heir(&rec, now_epoch()).is_none());
        // MUTANTE: nuovo e sulla tab giusta, ma aperto a mano — non è un erede.
        write_live_record("a-mano-00-0000", "tab-1", "startup", now + 5);
        assert!(registered_heir(&rec, now_epoch()).is_none());
        // MUTANTE: nuovo e azzerato, ma su un'altra tab.
        write_live_record("altrove-0-0000", "tab-9", "clear", now + 5);
        assert!(registered_heir(&rec, now_epoch()).is_none());
        // MUTANTE: è il record della sessione stessa, non del suo erede.
        write_live_record("provarel-0000-0000", "tab-1", "clear", now + 5);
        assert!(registered_heir(&rec, now_epoch()).is_none());
        // E questo invece è l'erede.
        write_live_record("erede-000-0000", "tab-1", "clear", now + 5);
        assert_eq!(registered_heir(&rec, now_epoch()).as_deref(), Some("erede-00"));
    }

    #[test]
    fn a_worktree_id_carrying_a_path_still_writes_its_files() {
        let _home = HomeIsolata::nuova("worktree-con-barre");
        // Il caso normale, non un caso limite: ogni identificativo di copia di
        // Orca è `<uuid>::/percorso/assoluto`. Finché quelle barre finivano nel
        // nome del file, la tregua e il segnale di ripresa non venivano scritti
        // mai — sul disco del 17/08/2026, zero e zero su sei sessioni vive.
        let mut rec = test_record();
        rec.worktree = "9591c8dd-9b12::/home/someone/other-repo/work/suite".into();
        let mut orca = |args: &[&str]| -> (i32, String) {
            if args.contains(&"create") {
                (0, r#"{"result":{"terminal":{"handle":"term_nuovo"}}}"#.to_string())
            } else {
                (0, String::new())
            }
        };
        regenerate(&rec, false, &mut orca);

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
    fn a_dry_run_never_calls_orca_even_once() {
        let _home = HomeIsolata::nuova("a-secco");
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            (0, String::new())
        };
        regenerate(&test_record(), true, &mut orca);
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
    fn a_worktree_traces_back_to_the_repo_that_hosts_it() {
        let home = HomeIsolata::nuova("radice-canonica");
        let repo = home.dir.join("other-repo").join("suite");
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
    fn the_successor_inherits_the_handoff_of_its_own_repo() {
        let home = HomeIsolata::nuova("consegna-ereditata");
        let repo = home.dir.join("other-repo").join("suite");
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

    // ─── Il freno della catena ────────────────────────────────────────────────

    /// Scrive una storia finta di `n` anelli, l'ultimo a `eta` secondi fa.
    fn fake_chain(worktree: &str, n: usize, writes: u64, handoff: &str, eta: f64) {
        let now = now_epoch();
        let links: Vec<ChainLink> = (0..n)
            .map(|i| ChainLink {
                session: format!("sess{i}"),
                // Un anello al minuto, come la fuga vera del 17/08/2026.
                at: now - eta - ((n - 1 - i) as f64 * 60.0),
                turns: 10,
                writes,
                handoff: handoff.to_string(),
            })
            .collect();
        write_chain(worktree, &links);
    }

    /// Un `orca` che registra le chiamate e finge un create riuscito.
    ///
    /// E FINGE ANCHE IL SUCCESSORE: al `/clear` fa quello che fa davvero il
    /// gancio `register-session` — consuma il segnale di ripresa e si registra
    /// fra le sessioni vive. Senza, ogni prova di questa batteria descriverebbe
    /// un `/clear` a cui non risponde nessuno, cioè il caso che la staffetta
    /// deve rifiutarsi di dichiarare riuscito.
    fn orca_that_records(
    ) -> (std::rc::Rc<std::cell::RefCell<Vec<String>>>, impl FnMut(&[&str]) -> (i32, String)) {
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let orca = move |args: &[&str]| -> (i32, String) {
            c.borrow_mut().push(args.join(" "));
            if args.contains(&"/clear") {
                the_heir_registers_itself();
            }
            if args.contains(&"create") {
                (0, r#"{"result":{"terminal":{"handle":"term_nuovo"}}}"#.to_string())
            } else {
                (0, String::new())
            }
        };
        (chiamate, orca)
    }

    /// Quello che fa `register-session` a `SessionStart` dopo un `/clear`:
    /// consuma il segnale indirizzato alla propria tab e scrive il proprio
    /// record fra le sessioni vive.
    fn the_heir_registers_itself() {
        let Ok(segnali) = fs::read_dir(resume_dir()) else {
            return;
        };
        for entry in segnali.flatten() {
            let corpo = fs::read_to_string(entry.path()).unwrap_or_default();
            let tab = serde_json::from_str::<serde_json::Value>(&corpo)
                .ok()
                .and_then(|d| d.get("tab").and_then(|v| v.as_str()).map(String::from))
                .unwrap_or_default();
            let _ = fs::remove_file(entry.path());
            write_live_record("nuova-dopo-il-clear", &tab, "clear", now_epoch() as u64);
        }
    }

    /// Un record fra le sessioni vive, come lo scrive `register_session`.
    fn write_live_record(session_id: &str, tab: &str, source: &str, updated_at: u64) {
        let corto: String = session_id.chars().take(8).collect();
        let _ = fs::create_dir_all(live_dir());
        let _ = fs::write(
            live_dir().join(format!("{corto}.json")),
            serde_json::json!({
                "session_id": session_id,
                "terminal_handle": "term_vecchio",
                "worktree_id": "wt-prova",
                "tab_id": tab,
                "transcript_path": "",
                "cwd": "/x",
                "source": source,
                "updated_at": updated_at,
            })
            .to_string(),
        );
    }

    // ─── La chiusura della scheda vecchia ────────────────────────────────────

    /// Un `orca` che risponde a `terminal list` con l'elenco dato, e registra
    /// tutto. `dopo` è l'elenco che risponde DOPO il primo `close`: è così che si
    /// finge una scheda che si chiude davvero, o una che resiste.
    fn orca_with_panels(
        prima: &'static str,
        dopo: &'static str,
    ) -> (std::rc::Rc<std::cell::RefCell<Vec<String>>>, impl FnMut(&[&str]) -> (i32, String)) {
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let orca = move |args: &[&str]| -> (i32, String) {
            let chiuso = c.borrow().iter().any(|x| x.contains("close"));
            c.borrow_mut().push(args.join(" "));
            if args.contains(&"list") {
                (0, if chiuso { dopo.to_string() } else { prima.to_string() })
            } else if args.contains(&"create") {
                (0, r#"{"result":{"terminal":{"handle":"term_nuovo"}}}"#.to_string())
            } else {
                (0, String::new())
            }
        };
        (chiamate, orca)
    }

    #[test]
    fn the_clear_goes_to_the_current_handle_not_the_recorded_one() {
        let _home = HomeIsolata::nuova("clear-handle-fresco");
        // Lo stesso caso del 18/08/2026, ma sul gesto nuovo: mandare `/clear`
        // all'handle scaduto azzererebbe la sessione di qualcun altro — o
        // nessuna — e questa lascerebbe il posto senza averlo mai lasciato.
        // Fino al 19/08/2026 nessun caso di `regenerate` leggeva una lista di
        // schede vera: la ririsoluzione era scritta e non provata.
        let rec = Record {
            handle: "term_scaduto".into(),
            tab_id: "tab-1".into(),
            ..test_record()
        };
        let (chiamate, mut orca) = orca_with_panels(
            r#"{"result":{"terminals":[{"handle":"term_attuale","tabId":"tab-1"}]}}"#,
            r#"{"result":{"terminals":[]}}"#,
        );
        regenerate(&rec, false, &mut orca);
        let seq = chiamate.borrow().clone();
        let clear = seq.iter().find(|c| c.contains("/clear")).expect("nessun /clear");
        assert!(clear.contains("term_attuale"), "azzerata la scheda sbagliata: {clear}");
        assert!(!clear.contains("term_scaduto"), "{clear}");
    }

    #[test]
    fn a_retirement_that_does_not_close_does_not_forget_the_session() {
        let _home = HomeIsolata::nuova("congedo-fallito");
        // Il controllo c'era, il verdetto pure, e nessuno lo leggeva: il congedo
        // proseguiva a cancellare il record di una sessione ancora viva, che da
        // lì in poi girava non tracciata accanto a chi aveva preso il suo posto.
        let rec = Record { tab_id: "tab-1".into(), ..test_record() };
        let vive = r#"{"result":{"terminals":[{"handle":"term_vecchio","tabId":"tab-1"}]}}"#;
        // La scheda resta nell'elenco anche dopo il `close`: non si è chiusa.
        let (_, mut orca) = orca_with_panels(vive, vive);
        let vivo = live_dir().join("provarel.json");
        fs::create_dir_all(live_dir()).unwrap();
        fs::write(&vivo, "{}").unwrap();
        retire(&rec, &mut orca);
        assert!(vivo.exists(), "ha smesso di tracciare una sessione ancora viva");
        assert!(read_chain("wt-prova").is_empty(), "ha contato un congedo mai avvenuto");
        let log = fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert!(log.contains("congedo annullato"), "{log}");
    }

    #[test]
    fn the_panel_is_resolved_again_before_closing_it() {
        let _home = HomeIsolata::nuova("chiusura-handle-fresco");
        // IL CASO VERO DEL 18/08/2026. Il record porta l'handle letto a inizio
        // giro; nel frattempo il pannello si è riattaccato e la stessa tab vive
        // su un handle nuovo. Chiudendo quello vecchio si chiude una scheda già
        // morta, e la sessione continua a girare accanto al successore.
        let rec = Record {
            handle: "term_scaduto".into(),
            tab_id: "tab-1".into(),
            ..test_record()
        };
        let (chiamate, mut orca) = orca_with_panels(
            r#"{"result":{"terminals":[{"handle":"term_attuale","tabId":"tab-1"}]}}"#,
            r#"{"result":{"terminals":[]}}"#,
        );
        assert_eq!(close_old_panel(&rec, "provarel", &mut orca), Chiusura::Fatta);
        let seq = chiamate.borrow().clone();
        assert!(
            seq.iter().any(|c| c.contains("close") && c.contains("term_attuale")),
            "ha chiuso l'handle scaduto invece di quello di adesso: {seq:?}"
        );
        assert!(
            !seq.iter().any(|c| c.contains("close") && c.contains("term_scaduto")),
            "ha speso una chiusura sulla scheda morta: {seq:?}"
        );
    }

    #[test]
    fn a_panel_that_does_not_close_is_said_out_loud() {
        let _home = HomeIsolata::nuova("chiusura-fallita");
        // Prima si scartava la risposta di `close`: una scheda che resta aperta
        // era indistinguibile da una chiusa, e il registro diceva RIGENERATA
        // mentre restavano due sessioni sullo stesso albero.
        let rec = Record { handle: "term_x".into(), tab_id: "tab-1".into(), ..test_record() };
        let vivo = r#"{"result":{"terminals":[{"handle":"term_x","tabId":"tab-1"}]}}"#;
        let (_, mut orca) = orca_with_panels(vivo, vivo);
        assert_eq!(
            close_old_panel(&rec, "provarel", &mut orca),
            Chiusura::Fallita,
            "ha dichiarato chiusa una scheda viva"
        );
        let log = fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        // La riga si cita come la scrive il codice: `E'` con l'apostrofo, non
        // `È`. Con l'accento la prova non poteva passare mai, e il rosso si
        // leggeva come una corsa fra prove parallele invece che come un
        // confronto di stringhe che non combacia.
        assert!(log.contains("NON SI E' CHIUSA"), "{log}");
    }

    #[test]
    fn when_the_list_cannot_be_reread_the_command_is_taken_at_its_word() {
        let _home = HomeIsolata::nuova("verifica-impossibile");
        // L'unico ramo in cui si crede a un codice d'uscita, e nessun caso lo
        // toccava: un mutante che ci scrivesse `true` fisso — cioè «chiusa» per
        // un comando fallito — sopravviveva a tutta la batteria, e il congedo
        // avrebbe smesso di tracciare una sessione ancora viva.
        let rec = Record { handle: "term_x".into(), tab_id: "tab-1".into(), ..test_record() };
        let prima = r#"{"result":{"terminals":[{"handle":"term_x","tabId":"tab-1"}]}}"#;
        let chiamate = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let c = chiamate.clone();
        let mut orca = move |args: &[&str]| -> (i32, String) {
            let gia_chiuso = c.borrow().iter().any(|x| x.contains("close"));
            c.borrow_mut().push(args.join(" "));
            if args.contains(&"list") {
                // Dopo il `close` l'elenco non si legge più: è il ramo `None`.
                if gia_chiuso { (1, String::new()) } else { (0, prima.to_string()) }
            } else if args.contains(&"close") {
                (1, "boom".to_string()) // il comando dichiara di non aver chiuso
            } else {
                (0, String::new())
            }
        };
        assert_eq!(close_old_panel(&rec, "provarel", &mut orca), Chiusura::Fallita);
    }

    #[test]
    fn a_panel_already_gone_costs_no_close() {
        let _home = HomeIsolata::nuova("scheda-gia-sparita");
        let rec = Record { handle: "term_x".into(), tab_id: "tab-1".into(), ..test_record() };
        let (chiamate, mut orca) = orca_with_panels(
            r#"{"result":{"terminals":[{"handle":"term_altro","tabId":"tab-9"}]}}"#,
            r#"{"result":{"terminals":[]}}"#,
        );
        // NON «Fatta»: non si è chiuso niente, e la riga di registro del
        // congedo deve poterlo dire.
        assert_eq!(close_old_panel(&rec, "provarel", &mut orca), Chiusura::NonCera);
        assert!(
            !chiamate.borrow().iter().any(|c| c.contains("close")),
            "ha chiuso qualcosa che non era suo: {:?}",
            chiamate.borrow()
        );
    }

    #[test]
    fn a_worn_out_chain_is_not_regenerated() {
        let _home = HomeIsolata::nuova("catena-esausta");
        // Dieci rigenerazioni ravvicinate: il tetto morde PRIMA di spendere una
        // sola chiamata a orca, che è il punto — il freno non deve costare un
        // giro per accorgersi di essere un freno.
        fake_chain("wt-prova", 10, 5, "/qualcosa.md", 30.0);
        let (chiamate, mut orca) = orca_that_records();
        regenerate(&test_record(), false, &mut orca);
        assert!(
            chiamate.borrow().is_empty(),
            "il freno non ha fermato niente: {:?}",
            chiamate.borrow()
        );
        let log = fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert!(log.contains("FRENO"), "{log}");
        assert!(!log.contains("RIGENERATA"), "{log}");
        assert!(chain_blocked_path("wt-prova").exists(), "manca il marcatore");
    }

    #[test]
    fn a_stalled_chain_is_not_regenerated() {
        let _home = HomeIsolata::nuova("catena-in-stallo");
        // Quattro anelli sotto il tetto, ma gli ultimi tre non hanno scritto
        // niente e citano tutti la stessa consegna.
        fake_chain("wt-prova", 4, 0, "/ferma.md", 30.0);
        let (chiamate, mut orca) = orca_that_records();
        regenerate(&test_record(), false, &mut orca);
        assert!(chiamate.borrow().is_empty(), "{:?}", chiamate.borrow());
        let log = fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert!(log.contains("gira a vuoto"), "{log}");
    }

    #[test]
    fn the_brake_speaks_once_not_every_minute() {
        let _home = HomeIsolata::nuova("freno-silenzioso");
        // Gira ogni sessanta secondi: se parlasse a ogni giro, il registro
        // diventerebbe illeggibile in un pomeriggio — ed è il difetto che questa
        // configurazione ha già pagato altrove, con 16 falsi allarmi su 18.
        fake_chain("wt-prova", 10, 5, "/qualcosa.md", 30.0);
        let (_, mut orca) = orca_that_records();
        regenerate(&test_record(), false, &mut orca);
        regenerate(&test_record(), false, &mut orca);
        regenerate(&test_record(), false, &mut orca);
        let log = fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert_eq!(
            log.matches("FRENO").count(),
            1,
            "il freno ha ripetuto la stessa riga: {log}"
        );
    }

    #[test]
    fn a_chain_gone_quiet_starts_over() {
        let _home = HomeIsolata::nuova("catena-scaduta");
        // Dieci anelli, ma l'ultimo è di otto ore fa: quella catena è finita, e
        // il lavoro di adesso non si giudica coi suoi numeri.
        fake_chain("wt-prova", 10, 0, "/vecchia.md", 8.0 * 3600.0);
        let (chiamate, mut orca) = orca_that_records();
        regenerate(&test_record(), false, &mut orca);
        let seq = chiamate.borrow().clone();
        assert!(seq.iter().any(|c| c.contains("/clear")), "non ha rigenerato: {seq:?}");
        let anelli = read_chain("wt-prova");
        assert_eq!(anelli.len(), 1, "la catena vecchia non è stata azzerata");
    }

    #[test]
    fn a_successful_regeneration_records_its_link() {
        let _home = HomeIsolata::nuova("anello-registrato");
        let (_, mut orca) = orca_that_records();
        regenerate(&test_record(), false, &mut orca);
        let anelli = read_chain("wt-prova");
        assert_eq!(anelli.len(), 1, "l'anello non è stato registrato");
        assert_eq!(anelli[0].session, "provarel");
        let log = fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert!(log.contains("anello 1 della catena"), "{log}");
    }

    #[test]
    fn the_brake_has_a_valve() {
        let _home = HomeIsolata::nuova("freno-spento");
        fake_chain("wt-prova", 10, 0, "/ferma.md", 30.0);
        fs::write(home().join(".claude/state/freno-catena-off"), "").unwrap();
        let (chiamate, mut orca) = orca_that_records();
        regenerate(&test_record(), false, &mut orca);
        assert!(
            chiamate.borrow().iter().any(|c| c.contains("/clear")),
            "la valvola non spegne il freno: {:?}",
            chiamate.borrow()
        );
        // E la storia si continua a scrivere anche a freno spento: serve a
        // capire cosa è successo quando qualcuno riaccende.
        assert_eq!(read_chain("wt-prova").len(), 11);
    }

    #[test]
    fn a_retirement_is_a_link_too() {
        let _home = HomeIsolata::nuova("congedo-e-un-anello");
        // La porta di servizio: il congedo sostituisce una sessione come la
        // rigenerazione, e se non contasse, una catena che passa di qui
        // crescerebbe senza che nessun tetto la veda.
        let mut orca = |_args: &[&str]| -> (i32, String) { (0, String::new()) };
        retire(&test_record(), &mut orca);
        let anelli = read_chain("wt-prova");
        assert_eq!(anelli.len(), 1, "il congedo non ha lasciato un anello");
        let log = fs::read_to_string(home().join(".claude/state/staffetta.log")).unwrap_or_default();
        assert!(log.contains("CONGEDATA"), "{log}");
        assert!(log.contains("anello 1 della catena"), "{log}");
    }

    #[test]
    fn a_failed_regeneration_is_not_a_link() {
        let _home = HomeIsolata::nuova("anello-solo-se-riuscita");
        // Il `/clear` non parte: la catena non è avanzata, e contarlo farebbe
        // mordere il tetto proprio mentre non si sostituisce niente.
        let mut orca = |args: &[&str]| -> (i32, String) {
            if args.contains(&"/clear") {
                (1, "boom".to_string())
            } else {
                (0, String::new())
            }
        };
        regenerate(&test_record(), false, &mut orca);
        assert!(read_chain("wt-prova").is_empty(), "un tentativo fallito ha contato");
    }

    // ─── L'albero ricreato ───────────────────────────────────────────────────
    //
    // Le date si prendono dal filesystem, mai dall'orologio: fra il `mkdir` e la
    // riga che scrive gli anelli passano millisecondi, e un confine calcolato su
    // `now_epoch()` cadrebbe dalla parte sbagliata su una macchina occupata.

    /// Una cartella vera dentro la casa isolata, e l'id di copia che la nomina.
    fn real_tree(home: &HomeIsolata, nome: &str) -> (PathBuf, String) {
        let dir = home.dir.join(nome);
        fs::create_dir_all(&dir).unwrap();
        let wt = format!("repo-di-prova::{}", dir.display());
        (dir, wt)
    }

    fn birth(dir: &Path) -> f64 {
        fs::metadata(dir)
            .and_then(|m| m.created())
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    }

    /// Una catena i cui anelli stanno agli scarti dati dalla nascita della cartella.
    fn chain_at_offsets(worktree: &str, dir: &Path, scarti: &[f64]) {
        let nato = birth(dir);
        let links: Vec<ChainLink> = scarti
            .iter()
            .enumerate()
            .map(|(i, s)| ChainLink {
                session: format!("sess{i}"),
                at: nato + s,
                turns: 10,
                writes: 5,
                handoff: "/a.md".to_string(),
            })
            .collect();
        write_chain(worktree, &links);
    }

    #[test]
    fn a_recreated_tree_does_not_inherit_the_brake() {
        let _home = HomeIsolata::nuova("albero-ricreato");
        let (dir, wt) = real_tree(&_home, "copia-rifatta");
        // Dieci anelli, tutti prima che la cartella nascesse: è la copia di
        // prima, smontata e rifatta con lo stesso nome — e quindi con lo stesso
        // id, perché l'id è `<repoId>::<percorso>`. Senza la guardia il freno
        // morderebbe al primo giro della lavorazione nuova, e quel morso si
        // leggerebbe come «funziona».
        chain_at_offsets(&wt, &dir, &[-600.0, -540.0, -480.0, -420.0, -360.0,
                                        -300.0, -240.0, -180.0, -120.0, -60.0]);
        let (chiamate, mut orca) = orca_that_records();
        let rec = Record { worktree: wt.clone(), ..test_record() };
        regenerate(&rec, false, &mut orca);
        assert!(
            chiamate.borrow().iter().any(|c| c.contains("/clear")),
            "la copia nuova è stata frenata dalla storia di quella vecchia: {:?}",
            chiamate.borrow()
        );
        assert_eq!(read_chain(&wt).len(), 1, "la catena non è ripartita da capo");
    }

    #[test]
    fn a_tree_older_than_its_chain_keeps_it() {
        let _home = HomeIsolata::nuova("albero-fermo");
        let (dir, wt) = real_tree(&_home, "copia-vissuta");
        // Il gemello del caso sopra: qui la cartella c'era già quando la catena
        // è partita, quindi la storia è sua e il tetto deve mordere.
        let scarti: Vec<f64> = (1..=10).map(|i| i as f64 * 60.0).collect();
        chain_at_offsets(&wt, &dir, &scarti);
        let (chiamate, mut orca) = orca_that_records();
        let rec = Record { worktree: wt.clone(), ..test_record() };
        regenerate(&rec, false, &mut orca);
        assert!(
            chiamate.borrow().is_empty(),
            "il freno non ha fermato niente: {:?}",
            chiamate.borrow()
        );
        // E la storia è ancora tutta lì: senza questa riga la prova resterebbe
        // verde anche per un taglio parziale che lasciasse abbastanza anelli.
        assert_eq!(read_chain(&wt).len(), 10, "la catena è stata toccata");
    }

    #[test]
    fn the_margin_covers_a_clock_correction() {
        let _home = HomeIsolata::nuova("margine-orologio");
        let (dir, dentro) = real_tree(&_home, "dentro-il-margine");
        chain_at_offsets(&dentro, &dir, &[-REBORN_MARGIN_SEC]);
        assert_eq!(
            read_chain(&dentro).len(),
            1,
            "un salto d'orologio ha tagliato una catena viva"
        );
        let (dir, oltre) = real_tree(&_home, "oltre-il-margine");
        chain_at_offsets(&oltre, &dir, &[-REBORN_MARGIN_SEC - 1.0]);
        assert!(read_chain(&oltre).is_empty(), "oltre il margine non ha tagliato");
        // Il numero scritto per esteso: i due casi qui sopra si muovono con la
        // costante e resterebbero verdi anche a margine zero, cioè proprio
        // quando la protezione sparisce.
        let (dir, un_minuto) = real_tree(&_home, "un-minuto-prima");
        chain_at_offsets(&un_minuto, &dir, &[-60.0]);
        assert_eq!(read_chain(&un_minuto).len(), 1, "un minuto di scarto ha tagliato");
        // E il numero dall'altro lato: gonfiando il margine «per prudenza» la
        // guardia smetterebbe di prendere gli alberi rifatti, e i due casi qui
        // sopra resterebbero verdi lo stesso.
        let (dir, cinque_minuti) = real_tree(&_home, "cinque-minuti-prima");
        chain_at_offsets(&cinque_minuti, &dir, &[-300.0]);
        assert!(
            read_chain(&cinque_minuti).is_empty(),
            "cinque minuti di scarto non hanno tagliato: il margine è troppo largo"
        );
    }

    /// Sposta indietro la NASCITA di un percorso, e dice se ci è riuscito.
    ///
    /// Il divario fra cartella e `.git` deve superare i due minuti del margine,
    /// e una prova che dorme due minuti non la lancia più nessuno. `SetFile` è
    /// l'unico attrezzo che sposta la nascita su APFS: se manca, i casi non si
    /// eseguono e la prova lo dice invece di passare muta.
    fn age_birth(path: &Path, seconds_ago: u64) -> bool {
        let target = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds_ago);
        let secs = target
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // `date -r` stampa l'istante nel formato che `SetFile` accetta, e non
        // richiede una libreria di date nel crate.
        let Ok(stamped) = Command::new("date")
            .args(["-r", &secs.to_string(), "+%m/%d/%Y %H:%M:%S"])
            .output()
        else {
            return false;
        };
        let arg = String::from_utf8_lossy(&stamped.stdout).trim().to_string();
        if arg.is_empty() {
            return false;
        }
        Command::new("SetFile")
            .args(["-d", &arg])
            .arg(path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn a_surviving_directory_does_not_hide_a_rebuilt_worktree() {
        let _home = HomeIsolata::nuova("cartella-sopravvissuta");
        let (dir, wt) = real_tree(&_home, "copia-ricreata-dentro");
        // Il `.git` di un worktree è un FILE, e `git worktree add` lo riscrive
        // ogni volta: è il segno che distingue la cartella lasciata in piedi da
        // uno smontaggio (`close-finished.py` stampa `STILL THERE`) dalla
        // cartella di un albero mai toccato.
        fs::write(dir.join(".git"), "gitdir: /altrove\n").unwrap();
        assert!(
            age_birth(&dir, 3600),
            "SetFile non ha spostato la nascita: il caso non è stato provato"
        );
        let nato = birth(&dir);
        // Gli anelli stanno un minuto DOPO la cartella e cinquantanove minuti
        // PRIMA del `.git`: guardando la sola cartella la catena morta
        // resterebbe, ed è esattamente il buco che questa guardia chiude.
        chain_at_offsets(&wt, &dir, &[60.0, 120.0]);
        assert!(
            read_chain(&wt).is_empty(),
            "la catena della lavorazione morta è stata ereditata: nato={nato}"
        );
    }

    #[test]
    fn a_main_checkout_keeps_its_chain() {
        let _home = HomeIsolata::nuova("checkout-principale");
        let (dir, wt) = real_tree(&_home, "copia-principale");
        // Stesso identico divario, ma `.git` è una CARTELLA: è un checkout
        // principale, dove `.git` ha vita propria — su `other-repo/work` è nata 126
        // giorni dopo il checkout. Prenderla taglierebbe una catena viva.
        fs::create_dir_all(dir.join(".git")).unwrap();
        assert!(age_birth(&dir, 3600), "SetFile non ha spostato la nascita");
        chain_at_offsets(&wt, &dir, &[60.0, 120.0]);
        assert_eq!(
            read_chain(&wt).len(),
            2,
            "una catena viva è stata tagliata da un `.git` di cartella"
        );
    }

    #[test]
    fn an_old_git_file_cuts_nothing() {
        let _home = HomeIsolata::nuova("git-vecchio");
        let (dir, wt) = real_tree(&_home, "copia-vissuta-con-git");
        fs::write(dir.join(".git"), "gitdir: /altrove\n").unwrap();
        // Il gemello che deve restare fermo: `.git` file come nel primo caso,
        // ma vecchio quanto la cartella. Senza di lui i due casi sopra
        // proverebbero solo che il segno taglia sempre.
        assert!(age_birth(&dir, 3600), "SetFile non ha spostato la nascita");
        assert!(age_birth(&dir.join(".git"), 3600), "SetFile sul `.git`");
        chain_at_offsets(&wt, &dir, &[60.0, 120.0]);
        assert_eq!(read_chain(&wt).len(), 2, "il segno ha tagliato una catena viva");
    }

    #[test]
    fn tree_birth_reads_the_two_signs() {
        let _home = HomeIsolata::nuova("due-segni");
        let (dir, _wt) = real_tree(&_home, "segni");
        // Senza `.git` resta la cartella, e non c'è modo di sbagliarsi.
        assert_eq!(tree_birth(dir.to_str().unwrap()), Some(birth(&dir)));
        fs::write(dir.join(".git"), "gitdir: /altrove\n").unwrap();
        assert!(age_birth(&dir, 3600), "SetFile non ha spostato la nascita");
        let git_born = birth(&dir.join(".git"));
        assert_eq!(
            tree_birth(dir.to_str().unwrap()),
            Some(git_born),
            "il `.git` più giovane non ha vinto"
        );
        // Una cartella che non c'è non risponde: «non lo so» vale più di
        // un'ipotesi quando dopo c'è un taglio.
        assert_eq!(tree_birth(&format!("{}/mai-esistita", dir.display())), None);
        assert_eq!(tree_birth(""), None);
    }

    #[test]
    fn a_count_written_as_text_is_a_truth_not_a_zero() {
        let _home = HomeIsolata::nuova("writes-stringa");
        let (dir, wt) = real_tree(&_home, "writes-di-testo");
        let nato = birth(&dir);
        let _ = fs::create_dir_all(chain_dir());
        // `_is_sterile` del Python scrive `not link.get('writes')`, e `"0"` è una
        // stringa non vuota: vera. Leggendola come zero, quattro anelli con la
        // stessa consegna diventerebbero uno stallo e fermerebbero una catena
        // che l'oracolo lascia correre.
        let anelli: Vec<String> = (0..4)
            .map(|i| {
                format!(
                    r#"{{"session":"s{i}","at":{},"turns":3,"writes":"0","handoff":"/ferma.md"}}"#,
                    nato + 60.0 * (i as f64 + 1.0)
                )
            })
            .collect();
        fs::write(chain_path(&wt), format!(r#"{{"links":[{}]}}"#, anelli.join(","))).unwrap();
        let letti = read_chain(&wt);
        assert_eq!(letti.len(), 4);
        assert_eq!(
            guards::chain::sterile_tail(&letti),
            0,
            "una catena viva è stata contata come stallo"
        );
    }

    // ─── Lo stato che resta quando l'albero se ne va ─────────────────────────

    /// `touch -t` vuole `[[CC]YY]MMDDhhmm[.ss]`, e qui si passa da `date`.
    fn touch_timestamp(epoch_secs: u64) -> String {
        let out = std::process::Command::new("date")
            .args(["-r", &epoch_secs.to_string(), "+%Y%m%d%H%M.%S"])
            .output()
            .expect("date");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Un file di stato con l'età voluta. `filetime` non è fra le dipendenze —
    /// sono tenute al minimo di proposito — e per invecchiare un file basta il
    /// comando di sistema.
    fn fake_state_file(root: &Path, rel: &str, older_by: Option<f64>) -> PathBuf {
        let f = root.join(rel);
        let _ = fs::create_dir_all(f.parent().unwrap());
        fs::write(&f, "x").unwrap();
        if let Some(sec) = older_by {
            let when = (now_epoch() - sec) as u64;
            let _ = std::process::Command::new("touch")
                .arg("-t")
                .arg(touch_timestamp(when))
                .arg(&f)
                .status();
        }
        f
    }

    #[test]
    fn who_is_alive_says_i_do_not_know_when_the_shape_is_unknown() {
        // Questa funzione decide l'insieme dei vivi, quindi decide quali file
        // NON si cancellano: era l'unico anello della catena senza prove e fuori
        // dal confronto, e proprio lì i due lati divergevano — su un array nudo
        // il porto prendeva gli elementi come copie, il Python sollevava.
        for (nome, rc, out, atteso) in [
            (
                "la forma vera di oggi",
                0,
                r#"{"id":"x","result":{"worktrees":[{"id":"repo::/a/b"}]}}"#,
                Some(vec!["repo::_a_b".to_string()]),
            ),
            (
                "result con items",
                0,
                r#"{"result":{"items":[{"id":"repo::/a/b"}]}}"#,
                Some(vec!["repo::_a_b".to_string()]),
            ),
            (
                "result è una lista",
                0,
                r#"{"result":[{"id":"repo::/a/b"}]}"#,
                Some(vec!["repo::_a_b".to_string()]),
            ),
            ("array nudo: forma che non conosco", 0, r#"[{"id":"repo::/a/b"}]"#, None),
            ("numero al posto della risposta", 0, "42", None),
            ("null", 0, "null", None),
            ("testo non JSON", 0, "boh", None),
            ("uscita vuota", 0, "", None),
            ("elenco vuoto: non lo so", 0, r#"{"result":{"worktrees":[]}}"#, None),
            ("id vuoti o assenti", 0, r#"{"result":{"worktrees":[{"id":""},{}]}}"#, None),
            (
                "orca ha risposto male",
                1,
                r#"{"result":{"worktrees":[{"id":"repo::/a/b"}]}}"#,
                None,
            ),
        ] {
            let mut orca = |_args: &[&str]| (rc, out.to_string());
            assert_eq!(live_worktree_keys(&mut orca), atteso, "caso: {nome}");
        }
    }

    #[test]
    fn the_state_of_a_vanished_tree_is_swept() {
        let _home = HomeIsolata::nuova("stato-orfano");
        let root = _home.stato();
        let alive = "repo::_Users_theo_orca_viva".to_string();
        let gone = "repo::_Users_theo_orca_morta";
        let old_enough = Some(TREE_STATE_GRACE_SEC + 60.0);
        for key in [alive.as_str(), gone] {
            fake_state_file(&root, &format!("catene/{key}.json"), old_enough);
            fake_state_file(&root, &format!("riprendi-da/{key}.txt"), old_enough);
            fake_state_file(&root, &format!("staffetta-cooldown-{key}"), old_enough);
            fake_state_file(&root, &format!("catena-bloccata-{key}"), old_enough);
        }
        let live = vec![alive.clone()];
        let now = now_epoch();

        let stale = orphan_tree_state(Some(&live), now, &root);
        assert_eq!(stale.len(), 4, "quattro famiglie, un albero morto: {stale:?}");
        assert!(
            stale.iter().all(|f| f.to_string_lossy().contains(gone)),
            "ha preso anche l'albero vivo: {stale:?}"
        );

        // LA GUARDIA CHE CONTA: un elenco che non si è potuto leggere non è
        // «sono spariti tutti». Né lo è un elenco vuoto.
        assert!(orphan_tree_state(None, now, &root).is_empty());
        assert!(orphan_tree_state(Some(&[]), now, &root).is_empty());

        // Il periodo di grazia: un file di un minuto fa è nuovo, non orfano.
        //
        // UN MINUTO, NON «adesso». Scritto senza età, il file nasce dopo
        // l'istante `now` preso qui sopra: la sua età risulta negativa e sta
        // dentro qualunque soglia, quindi la prova resterebbe verde anche a
        // grazia azzerata — cioè proprio quando la protezione sparisce.
        // Misurato mutando la costante: 123 verdi con la guardia spenta.
        fake_state_file(
            &root,
            "catene/repo::_Users_theo_orca_appena_nata.json",
            Some(60.0),
        );
        assert!(
            !orphan_tree_state(Some(&live), now, &root)
                .iter()
                .any(|f| f.to_string_lossy().contains("appena_nata")),
            "un file fresco è stato contato come orfano"
        );

        let before = fs::read_dir(root.join("catene")).unwrap().count();
        assert_eq!(sweep_tree_state(Some(&live), now, true, &root), 4);
        assert_eq!(
            fs::read_dir(root.join("catene")).unwrap().count(),
            before,
            "a secco ha cancellato"
        );
        assert_eq!(sweep_tree_state(Some(&live), now, false, &root), 4);
        assert!(
            root.join(format!("catene/{alive}.json")).exists(),
            "l'albero vivo ha perso la catena"
        );
        assert!(!root.join(format!("catene/{gone}.json")).exists());
        assert_eq!(
            sweep_tree_state(Some(&live), now, false, &root),
            0,
            "la spazzata non è idempotente"
        );
    }

    #[test]
    fn a_count_that_does_not_fit_a_u64_is_still_a_truth() {
        let _home = HomeIsolata::nuova("writes-non-interi");
        // `5.0` e `-5` non stanno in un `u64`, e `as_u64()` ci ricadeva sopra
        // con uno zero: «non ha prodotto» detto di un anello che per Python ha
        // prodotto (`not 5.0` e `not -5` sono entrambi falsi). Quattro anelli
        // così, con la stessa consegna, diventavano uno stallo.
        for (nome, valore) in [("float", "5.0"), ("negativo", "-5")] {
            let (dir, wt) = real_tree(&_home, &format!("writes-{nome}"));
            let nato = birth(&dir);
            let _ = fs::create_dir_all(chain_dir());
            let anelli: Vec<String> = (0..4)
                .map(|i| {
                    format!(
                        r#"{{"session":"s{i}","at":{},"turns":3,"writes":{valore},"handoff":"/ferma.md"}}"#,
                        nato + 60.0 * (i as f64 + 1.0)
                    )
                })
                .collect();
            fs::write(chain_path(&wt), format!(r#"{{"links":[{}]}}"#, anelli.join(","))).unwrap();
            let letti = read_chain(&wt);
            assert_eq!(letti.len(), 4, "writes {nome}");
            assert_eq!(
                guards::chain::sterile_tail(&letti),
                0,
                "writes {nome}: una catena viva è stata contata come stallo"
            );
        }
        // E lo zero resta zero, altrimenti il caso sopra proverebbe solo che
        // nessun valore conta più.
        let (dir, wt) = real_tree(&_home, "writes-zero");
        chain_at_offsets(&wt, &dir, &[60.0, 120.0, 180.0, 240.0]);
        let nato = birth(&dir);
        let anelli: Vec<String> = (0..4)
            .map(|i| {
                format!(
                    r#"{{"session":"s{i}","at":{},"turns":3,"writes":0,"handoff":"/ferma.md"}}"#,
                    nato + 60.0 * (i as f64 + 1.0)
                )
            })
            .collect();
        fs::write(chain_path(&wt), format!(r#"{{"links":[{}]}}"#, anelli.join(","))).unwrap();
        assert_eq!(
            guards::chain::sterile_tail(&read_chain(&wt)),
            3,
            "lo zero vero non conta più come «non ha prodotto»"
        );
    }

    #[test]
    fn a_time_written_as_text_is_still_a_time() {
        let _home = HomeIsolata::nuova("at-stringa");
        let (dir, wt) = real_tree(&_home, "at-di-testo");
        let nato = birth(&dir);
        let _ = fs::create_dir_all(chain_dir());
        // MUTANTE STORICO: con `as_f64()` soltanto questo `at` valeva zero, la
        // guardia non scattava mai e la catena morta veniva ereditata — mentre
        // il Python, che passa da `float()`, la tagliava.
        fs::write(
            chain_path(&wt),
            format!(
                r#"{{"links":[{{"session":"s0","at":"{}","turns":"3","writes":"5","handoff":"/a.md"}}]}}"#,
                nato - 600.0
            ),
        )
        .unwrap();
        assert!(read_chain(&wt).is_empty(), "la stringa numerica non è stata letta");
    }

    #[test]
    fn the_guard_reads_the_first_link_not_the_last() {
        let _home = HomeIsolata::nuova("guardia-primo-anello");
        let (dir, wt) = real_tree(&_home, "a-cavallo");
        // MUTANTE: guardando l'ultimo anello, proprio la copia rifatta che sta
        // già lavorando passerebbe per «stesso albero».
        chain_at_offsets(&wt, &dir, &[-300.0, 300.0]);
        assert!(read_chain(&wt).is_empty(), "la guardia ha guardato l'ultimo anello");
    }

    #[test]
    fn without_a_sign_the_chain_stays() {
        let _home = HomeIsolata::nuova("nessun-segno");
        let (dir, wt) = real_tree(&_home, "cartella-viva");
        // Un `at` che non è un istante non è una prova di niente: si tiene.
        write_chain(
            &wt,
            &[ChainLink { session: "s0".into(), at: 0.0, turns: 1, writes: 1,
                          handoff: "/a.md".into() }],
        );
        assert_eq!(read_chain(&wt).len(), 1, "un `at` a zero ha azzerato la catena");
        // Cartella mai creata e id senza percorso: nessun segno, nessun taglio.
        assert!(!tree_reborn(&format!("repo::{}/mai-esistito", dir.display()), 1.0));
        assert!(!tree_reborn("solo-un-nome", 1.0));
        assert_eq!(worktree_dir("repo::/home/someone/orca/general"), "/home/someone/orca/general");
    }

    #[test]
    fn what_is_not_an_object_is_not_a_link() {
        let _home = HomeIsolata::nuova("anelli-misti");
        let (dir, wt) = real_tree(&_home, "elenco-misto");
        let nato = birth(&dir);
        let _ = fs::create_dir_all(chain_dir());
        // Il Python scarta ciò che non è un oggetto. Convertirlo darebbe un
        // anello a `at` zero in testa: una storia più lunga di quella vera, con
        // un'età che sfonda ogni tetto — e una guardia che non scatta mai.
        fs::write(
            chain_path(&wt),
            format!(
                r#"{{"links":["x",{{"session":"s0","at":{},"turns":1,"writes":1,"handoff":"/a.md"}},7]}}"#,
                nato + 60.0
            ),
        )
        .unwrap();
        let anelli = read_chain(&wt);
        assert_eq!(anelli.len(), 1, "un elemento non oggetto è diventato un anello");
        assert_eq!(anelli[0].session, "s0");
    }
}
