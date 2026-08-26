//! Dà alla coda di bordo un esecutore, quando nessun'altra fonte ne ha uno.
//!
//! Terza fonte del mandato di `SessionStart`, dopo `resume_message` (staffetta)
//! e `uncovered_thread::opening_notice` (filo scoperto): se tacciono tutte e
//! due, una sessione appena nata riceve come incarico la voce più vecchia della
//! coda che una sessione può davvero lavorare.
//!
//! CHI RESTA FUORI, E PERCHÉ. Il vocabolario di `per:`/`destinatario:` è quello
//! già in uso in coda (`scripts/queue-select.sh::normalize_per`): builder,
//! measurer, investigator, reviewer, theo, nessuno. Solo i primi quattro sono
//! dispacciabili qui. `theo` resta fuori perché la voce chiede una SUA
//! decisione — dispacciarla a una sessione qualunque sceglierebbe al posto
//! suo, che è esattamente il difetto che questo modulo non deve introdurre.
//! `nessuno` resta fuori perché non è lavoro di nessuno per costruzione (la
//! misura che la coda fa di sé). Una voce SENZA il campo va a Theo per la
//! stessa regola del selettore bash: dedurre il destinatario da altro (lo
//! stato, il tipo) è la porta da cui rientrano gli errori di instradamento.
//!
//! CHI BUTTA IL MARCATORE DI PRESA IN CARICO (`state/plancia/coda-mandati/`).
//! Due strade, come per `uncovered_thread`:
//!   1. la voce cambia stato — chi la lavora la chiude scrivendoci sopra
//!      `stato: chiusa`, e da quel momento non è più fra le `entries` aperte:
//!      `sweep_taken` la vede sparita dall'elenco e butta il marcatore;
//!   2. la sessione che l'aveva presa sparisce — stesso giudizio di
//!      `uncovered_thread` sul registro delle sessioni vive, con lo stesso
//!      margine di sei ore per non fidarsi di un record che non si è ancora
//!      visto smentito.
//! Senza la prima un marcatore vivrebbe finché non muore la sua sessione anche
//! su una voce già chiusa; senza la seconda, una sessione sparita terrebbe la
//! voce per sempre.
//!
//! DUE `SessionStart` QUASI SIMULTANEI SONO IL CASO NORMALE, non un caso
//! limite: succede ogni volta che si aprono più pannelli insieme. Per questo
//! `try_declare_taken` rivendica con `create_new` (atomica: fallisce se il
//! file esiste già) invece di leggere-poi-scrivere, e `opening_notice` scorre
//! la fila di `rank_mandate_with` finché una presa non riesce, così una corsa
//! persa sposta la sessione alla voce successiva invece di lasciarla a mani
//! vuote o farla vincere due volte.
//!
//! UNA VOCE PUÒ ESSERE OCCUPATA ANCHE SENZA UN NOSTRO MARCATORE: chi lavora
//! già su un file può scriverlo nel testo del campo `per:` (`cited_session_ids`,
//! `occupied_by_a_cited_session`). Non si interpreta quella prosa — si cerca
//! solo l'id di sessione che vi è citato, lo stesso segnale strutturale che
//! `queue-select.sh` già usa altrove.
//!
//! QUESTO MODULO E `scripts/queue-select.sh::holders_alive` (riga ~252, ~297)
//! LEGGONO LA STESSA CODA IN MODO DIVERSO, e nessuno dei due legge lo stato
//! dell'altro. Lo script guarda una riga scritta a mano (`presa:`), questo
//! modulo guarda `per:`/`destinatario:`: oggi non collidono sulla stessa
//! citazione, ma restano due letture che possono divergere sulla stessa voce.
//! Due scarti concreti: sui token numerici vanno in direzioni opposte — lo
//! script accetta ancora le date come id, questo modulo ora scarta anche id
//! veri che hanno la forma di una data (`looks_like_a_compact_date`); e
//! sull'astensione a più id citati lo script non scade mai (dichiaratamente),
//! mentre qui scade dopo sei ore. I marcatori in `coda-mandati/` sono
//! invisibili allo script; una riga `presa:` scritta a mano è invisibile qui.
//! Un umano che legge la coda a occhio e una sessione che la legge da questo
//! modulo possono oggi arrivare a un giudizio diverso sulla stessa voce.
//! Allinearli è un altro lavoro, fuori da questo file.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// I quattro mestieri a cui una voce si può dispacciare. `theo` e `nessuno`
/// esistono nel vocabolario di `per:` ma non qui: vedi il commento in testa al
/// file.
const DISPATCHABLE_ROLES: &[&str] = &["builder", "measurer", "investigator", "reviewer"];

/// Gli stati che contano come «voce aperta», stessa lista di
/// `queue-select.sh`. `attesa-theo` NON c'è apposta: è una voce che aspetta
/// una persona, non lavoro pronto da prendere.
const OPEN_STATES: &[&str] = &["aperta", "attesa-capitano"];

/// Oltre questa età un titolare non conta più, anche se il registro delle
/// sessioni vive non l'ha ancora smentito. Stessa soglia e stessa ragione di
/// `uncovered_thread::SHOW_ANYWAY_AFTER_SECONDS`: quel registro non si svuota
/// in modo affidabile, quindi «viva» da sola non basta per sempre.
const STALE_AFTER_SECONDS: i64 = 6 * 60 * 60;

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/home/someone".into()))
}

fn queue_dir() -> PathBuf {
    home().join(".claude").join("state").join("plancia").join("segnalazioni")
}

fn takers_dir() -> PathBuf {
    home().join(".claude").join("state").join("plancia").join("coda-mandati")
}

fn live_dir() -> PathBuf {
    home().join(".claude").join("state").join("sessioni-vive")
}

fn now_epoch() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn short(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

fn taker_path(entry_id: &str) -> PathBuf {
    takers_dir().join(format!("{entry_id}.json"))
}

/// Una voce di coda che si può dispacciare adesso: già filtrata per stato e
/// destinatario. `mtime` è l'ultima modifica del file, non `quando:` — lo
/// stesso scarto di `queue-select.sh`, che lo motiva così: `quando:` è testo
/// scritto a mano, e a volte posteriore alla propria scrittura.
///
/// `self_declared_holders` sono gli id di sessione citati dentro il testo del
/// campo `per:`/`destinatario:` — vedi `cited_session_ids`. Una voce reale in
/// coda scrive «per: builder — …NON intervenire da fuori (`790ca4e8`)»: il
/// ruolo da solo la classificherebbe dispacciabile, e sarebbe sbagliato.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueEntry {
    pub id: String,
    pub path: String,
    pub role: String,
    pub mtime: i64,
    pub self_declared_holders: Vec<String>,
}

/// Chi tiene una voce, letto dal proprio marcatore.
#[derive(Debug, Clone, PartialEq)]
struct TakenMarker {
    entry_id: String,
    session_id: String,
    taken_at_epoch: i64,
}

/// Le coppie chiave/valore del frontmatter, fra i primi due `---`. Un valore
/// che continua su righe indentate (`per: builder — ...\n  NON intervenire...`)
/// si piega in una riga sola separata da uno spazio — come YAML legge davvero
/// uno scalare piano multiriga, ed è la forma usata da tre voci reali in coda
/// oggi (`2026-08-24-due-sessioni...`, `AUTO-esame-della-forma`,
/// `AUTO-queue-health`). PRIMA SI LEGGEVA SOLO LA PRIMA RIGA: una clausola di
/// esclusività scritta sulla continuazione spariva, e la voce veniva
/// dispacciata lo stesso — riprodotto dal vivo. Un preambolo aperto e mai
/// chiuso non è un'intestazione: si torna a vuoto, come in `queue-select.sh`.
fn header_fields(text: &str) -> Vec<(String, String)> {
    let mut lines = text.lines();
    let mut started = false;
    for l in lines.by_ref() {
        let l = l.trim_end_matches('\r');
        if l.trim().is_empty() {
            continue;
        }
        started = l == "---";
        break;
    }
    if !started {
        return Vec::new();
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for l in lines {
        let l = l.trim_end_matches('\r');
        if l == "---" {
            return out;
        }
        if l.starts_with(char::is_whitespace) {
            // Continuazione: si piega sul valore della chiave precedente,
            // separata da uno spazio come fa YAML con uno scalare piano.
            if let Some((_, v)) = out.last_mut() {
                let continuation = l.trim();
                if !continuation.is_empty() {
                    if !v.is_empty() {
                        v.push(' ');
                    }
                    v.push_str(continuation);
                }
            }
            continue;
        }
        if let Some((k, v)) = l.split_once(':') {
            out.push((k.trim().to_lowercase(), v.trim().to_string()));
        }
    }
    Vec::new() // mai chiuso: come se l'intestazione non ci fosse
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn state_word(raw: &str) -> String {
    raw.split_whitespace().next().unwrap_or("").to_lowercase()
}

/// Il mestiere a cui una voce è indirizzata, dalla prima parola utile (tolti
/// gli articoli) del testo grezzo dopo `per:` o `destinatario:`. Stessa
/// normalizzazione di `normalize_per` in `scripts/queue-select.sh` — il
/// vocabolario vive lì, qui si duplica solo la lettura: un gancio Rust non può
/// chiamare uno script di shell a ogni apertura di sessione. `None` solo
/// quando il testo non contiene nessuna parola utile.
fn recipient_role(raw: &str) -> Option<&'static str> {
    for word in raw.split_whitespace() {
        let cleaned: String =
            word.chars().filter(|c| c.is_ascii_alphabetic() || *c == '\'').collect::<String>().to_lowercase();
        let cleaned = cleaned.strip_prefix("l'").unwrap_or(&cleaned);
        if cleaned.is_empty() {
            continue;
        }
        if matches!(cleaned, "il" | "lo" | "la" | "i" | "gli" | "le" | "un" | "uno" | "una") {
            continue;
        }
        return Some(match cleaned {
            "builder" => "builder",
            "measurer" => "measurer",
            "investigator" => "investigator",
            "reviewer" | "codereviewer" | "databasereviewer" | "planreviewer" | "securityreviewer" => "reviewer",
            "theo" => "theo",
            "nessuno" | "nobody" => "nessuno",
            _ => "unknown",
        });
    }
    None
}

/// Questa voce, letta dal proprio frontmatter, si può dispacciare? `Some((ruolo,
/// testo grezzo))` solo per i quattro mestieri dispacciabili — mai per `theo`,
/// `nessuno`, `unknown` o un campo assente, che valgono tutti «non qui» per la
/// stessa ragione scritta in testa al file. Il testo grezzo torna insieme al
/// ruolo perché serve un'altra volta a `cited_session_ids`, e leggere il campo
/// due volte con due copie della stessa logica diverge alla prima correzione.
fn is_dispatchable<'a>(fields: &'a [(String, String)]) -> Option<(&'static str, &'a str)> {
    let state = state_word(field(fields, "stato").unwrap_or(""));
    if !OPEN_STATES.contains(&state.as_str()) {
        return None;
    }
    let raw = field(fields, "per").or_else(|| field(fields, "destinatario")).unwrap_or("");
    let role = if raw.trim().is_empty() {
        "theo" // SENZA CAMPO SI VA A THEO, non si deduce da altro.
    } else {
        recipient_role(raw).unwrap_or("unknown")
    };
    let matched = DISPATCHABLE_ROLES.iter().find(|r| **r == role).copied()?;
    Some((matched, raw))
}

/// Una data compatta a otto cifre ha una forma riconoscibile: `20` + un anno
/// plausibile, un mese `01`-`12`, un giorno `01`-`31` (`20260824`). Un token
/// che capita per caso dentro questi limiti è raro quanto basta da scartare
/// senza rimpianti; uno fuori da questi limiti resta un candidato id di
/// sessione anche se è fatto di sole cifre.
///
/// SOSTITUISCE UN FILTRO SBAGLIATO DI CINQUE ORDINI DI GRANDEZZA: la stima che
/// aveva motivato «scarta ogni token senza una lettera esadecimale» era «1 su
/// 15 milioni»; il numero vero, calcolato e poi verificato sui dati, è
/// `(10/16)^8 ≈ 2,33%` — e sugli id di sessione reali di questa macchina è
/// **50 su 1.502 (3,3%, quasi 1 su 30)** a partire da sole cifre. Quel filtro
/// non impediva un falso positivo raro: causava un falso negativo comune,
/// perdendo il titolare vero di una voce su trenta.
fn looks_like_a_compact_date(tok: &str) -> bool {
    if tok.len() != 8 || !tok.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if &tok[0..2] != "20" {
        return false;
    }
    let month: u32 = tok[4..6].parse().unwrap_or(0);
    let day: u32 = tok[6..8].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// Gli id di sessione (otto cifre esadecimali, la stessa lunghezza di
/// `short()`) citati in un testo libero, esclusi i token che hanno la forma
/// di una data compatta (`looks_like_a_compact_date`) — la sola eccezione
/// nota, come `20260824` scritto accanto a `rivista:` in una voce reale.
///
/// NON SI INTERPRETA LA PROSA — sarebbe indovinare, e la domanda che ha
/// motivato questa funzione è esattamente «come rispetti una clausola scritta
/// in italiano senza farlo». Si cerca solo l'unico segnale STRUTTURALE che
/// questa casa già usa per nominare una sessione dentro un testo scritto a
/// mano: il suo id a otto cifre, la stessa convenzione che
/// `queue-select.sh::holders_alive` legge nella riga `presa:`. Un token che
/// non è un id di sessione (un pezzo di un commit) può ancora superare
/// entrambi i filtri, ma resta innocuo per lo stesso motivo per cui lo è là:
/// per contare occupata una voce deve anche risultare vivo in
/// `sessioni-vive/`.
fn cited_session_ids(raw: &str) -> Vec<String> {
    raw.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|tok| tok.len() == 8 && tok.chars().all(|c| c.is_ascii_hexdigit()) && !looks_like_a_compact_date(tok))
        .map(|s| s.to_lowercase())
        .collect()
}

fn read_open_entries() -> Vec<QueueEntry> {
    let Ok(rd) = fs::read_dir(queue_dir()) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if stem.eq_ignore_ascii_case("README") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let fields = header_fields(&text);
        let Some((role, raw)) = is_dispatchable(&fields) else { continue };
        let self_declared_holders = cited_session_ids(raw);
        let mtime = fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(i64::MAX); // illeggibile: in fondo alla fila, mai in testa
        out.push(QueueEntry {
            id: stem.to_string(),
            path: path.to_string_lossy().into_owned(),
            role: role.to_string(),
            mtime,
            self_declared_holders,
        });
    }
    out
}

fn read_takers() -> Vec<TakenMarker> {
    let Ok(rd) = fs::read_dir(takers_dir()) else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| {
            let text = fs::read_to_string(e.path()).ok()?;
            let v: Value = serde_json::from_str(&text).ok()?;
            Some(TakenMarker {
                entry_id: v.get("entry_id")?.as_str()?.to_string(),
                session_id: v.get("session_id")?.as_str()?.to_string(),
                taken_at_epoch: v.get("taken_at_epoch").and_then(|x| x.as_i64()).unwrap_or(0),
            })
        })
        .collect()
}

/// La tiene ancora chi ha scritto il nostro marcatore? Una sessione trovata
/// viva regge la presa finché non sta ferma da `STALE_AFTER_SECONDS`.
///
/// NON TROVATA VIVA NON VUOL DIRE MORTA, ed è una correzione, non la prima
/// stesura. Fuori da Orca la registrazione in `sessioni-vive/` è muta per
/// costruzione (`register_session.rs`), e anche dentro Orca la propria
/// scrittura del record può correre più lenta della propria chiamata a
/// `try_declare_taken`. La prima versione trattava «non trovata» come «morta»
/// e sfrattava il marcatore all'istante: **riprodotto con due thread reali**
/// (`two_concurrent_sessions_never_get_the_same_single_entry`), il secondo
/// `opening_notice` in corsa vedeva il marcatore appena scritto dal primo,
/// non trovava la sua sessione viva, lo buttava con `sweep_taken`, e
/// dispacciava la stessa voce una seconda volta. «Non trovata» vale come un
/// `Unknown`, non un `Gone` — lo stesso terzo stato di
/// `register_session::SessionLiveness` — e si aspetta lo stesso margine di
/// sei ore prima di trattarla come sparita, misurato da `since` (l'istante
/// della presa, non un'attività che per una sessione mai registrata non
/// esiste).
fn holder_still_holds(
    session_id: &str,
    alive: &BTreeSet<String>,
    since: i64,
    now: i64,
    last_activity: &dyn Fn(&str) -> Option<i64>,
) -> bool {
    if alive.contains(&short(session_id)) {
        let quiet_since = last_activity(session_id).unwrap_or(since);
        return (now - quiet_since) < STALE_AFTER_SECONDS;
    }
    (now - since) < STALE_AFTER_SECONDS
}

/// La sessione citata nel testo di una voce è viva e attiva adesso?
///
/// A DIFFERENZA DI `holder_still_holds` QUI NON C'È NESSUNA GRAZIA per «non
/// trovata viva»: quella funzione ha un `since` vero (l'istante in cui questa
/// stessa sessione ha scritto il proprio marcatore) da cui contare un margine
/// di dubbio; qui non esiste nessun istante di dichiarazione da fidarsi — la
/// citazione si rilegge dal testo a ogni chiamata — e dare lo stesso margine
/// a una sessione mai vista viva bloccherebbe una voce per sempre sulla sola
/// parola di una riga di prosa. Occupata solo con una prova positiva di vita.
fn cited_session_is_active(
    session_id: &str,
    alive: &BTreeSet<String>,
    now: i64,
    last_activity: &dyn Fn(&str) -> Option<i64>,
) -> bool {
    if !alive.contains(&short(session_id)) {
        return false;
    }
    let quiet_since = last_activity(session_id).unwrap_or(now);
    (now - quiet_since) < STALE_AFTER_SECONDS
}

/// La voce è occupata da una sessione che si è nominata dentro il proprio
/// testo, senza passare dal nostro marcatore? Con UN SOLO id citato, conta
/// occupata se `cited_session_is_active`.
///
/// CON DUE O PIÙ ID CI SI ASTIENE, la stessa prudenza di
/// `queue-select.sh::holders_alive`: una riga che nomina più sessioni
/// racconta una storia — ceduta, riassegnata — che non si riduce
/// affidabilmente a un solo titolare. MA L'ASTENSIONE HA LA STESSA GRAZIA DI
/// SEI ORE DEL MARCATORE (`holder_still_holds`), non è perpetua: `since` è
/// l'età della voce (`QueueEntry.mtime`), perché qui — a differenza del
/// marcatore — non esiste un istante di dichiarazione più preciso da cui
/// contare. Prima della correzione questo ramo restituiva sempre `true`: due
/// id citati bastavano a blindare una voce per sempre, anche a titolari già
/// spariti da ore.
///
/// L'OROLOGIO SI RIARMA A OGNI SCRITTURA SUL FILE, non solo a ogni cambio di
/// titolare, e non c'è un istante di dichiarazione migliore nella struttura
/// dati — misurato: **10 voci aperte su 54 (18,5%) hanno un `mtime` più
/// recente di oltre 24 ore rispetto alla loro data d'apertura, fino a 3,7
/// giorni dopo**. Il caso reale che oggi percorre questo ramo
/// (`2026-08-24-il-setaccio-dei-rami-scatta-alle-815-e-sara-muto.md`, i due
/// id citati nei test sotto) ha già rimesso a zero l'orologio due volte con
/// due righe `rivista:` — revisioni del contenuto, non della titolarità.
fn occupied_by_a_cited_session(
    holders: &[String],
    alive: &BTreeSet<String>,
    since: i64,
    now: i64,
    last_activity: &dyn Fn(&str) -> Option<i64>,
) -> bool {
    match holders {
        [] => false,
        [only] => cited_session_is_active(only, alive, now, last_activity),
        _ => (now - since) < STALE_AFTER_SECONDS,
    }
}

/// Il giudizio puro: l'ordine in cui offrire le voci, date quelle aperte e chi
/// le tiene già (dal nostro marcatore o da un'occupazione auto-dichiarata nel
/// testo). PURA e INIETTABILE, come `uncovered_thread::decide_uncovered_with`:
/// nessuna lettura di disco qui dentro.
///
/// RESTITUISCE UNA FILA, NON UN VINCITORE SOLO: chi rivendica la presa in
/// carico deve poter scorrere al secondo candidato quando il primo gli sfugge
/// per una corsa fra due sessioni — vedi `opening_notice`, che è dove questo
/// elenco diventa un tentativo atomico voce per voce.
///
/// L'ORDINE È LA VOCE PIÙ VECCHIA FRA QUELLE LIBERE (per `mtime`, non
/// `quando:`, per la ragione scritta sopra `QueueEntry`), a parità si sceglie
/// per nome: un criterio deterministico, non «la prima che il disco
/// restituisce».
pub fn rank_mandate_with(
    entries: &[QueueEntry],
    taken: &[(String, String, i64)], // (entry_id, session_id, taken_at_epoch)
    alive: &BTreeSet<String>,
    now: i64,
    last_activity: &dyn Fn(&str) -> Option<i64>,
) -> Vec<QueueEntry> {
    let mut free: Vec<&QueueEntry> = entries
        .iter()
        .filter(|e| match taken.iter().find(|(id, _, _)| id == &e.id) {
            None => true,
            Some((_, sess, since)) => !holder_still_holds(sess, alive, *since, now, last_activity),
        })
        .filter(|e| !occupied_by_a_cited_session(&e.self_declared_holders, alive, e.mtime, now, last_activity))
        .collect();
    free.sort_by(|a, b| a.mtime.cmp(&b.mtime).then_with(|| a.id.cmp(&b.id)));
    free.into_iter().cloned().collect()
}

/// La forma comoda per chi vuole solo sapere la prossima voce, senza
/// rivendicarla: il rapporto da riga di comando (`run_report`) e i test che
/// provano l'ordinamento.
pub fn decide_mandate_with(
    entries: &[QueueEntry],
    taken: &[(String, String, i64)],
    alive: &BTreeSet<String>,
    now: i64,
    last_activity: &dyn Fn(&str) -> Option<i64>,
) -> Option<QueueEntry> {
    rank_mandate_with(entries, taken, alive, now, last_activity).into_iter().next()
}

/// Butta i marcatori che non contano più: la voce che referenziano non è più
/// fra quelle aperte (chiusa da chi l'ha lavorata), o il titolare non la tiene
/// più secondo `holder_still_holds`.
///
/// NON GIRA A OGNI `SessionStart` — la prima stesura lo affermava, ed era
/// sbagliato: si raggiunge solo quando `opening_notice` viene interrogata, e
/// in `register_session::run` questo succede solo se tacciono sia il
/// messaggio della staffetta sia l'avviso dei fili scoperti. La staffetta è
/// il canale più usato di questa casa, quindi in pratica gira di rado. NON È
/// UN DIFETTO DI GIUDIZIO: `rank_mandate_with` ricalcola comunque la libertà
/// di ogni voce dal vivo con `holder_still_holds`, senza fidarsi della
/// presenza del file. L'unico costo di girare di rado è l'accumulo di
/// marcatori orfani su disco, non una voce data a chi non doveva riceverla.
fn sweep_taken(open_ids: &BTreeSet<String>, alive: &BTreeSet<String>, now: i64, last_activity: &dyn Fn(&str) -> Option<i64>) {
    for t in read_takers() {
        let still_held = open_ids.contains(&t.entry_id)
            && holder_still_holds(&t.session_id, alive, t.taken_at_epoch, now, last_activity);
        if !still_held {
            let _ = fs::remove_file(taker_path(&t.entry_id));
        }
    }
}

/// Rivendica una voce, e dice se ci è riuscita. ATOMICA: `create_new` è
/// un'unica chiamata di sistema che crea il file solo se non esiste ancora, e
/// fallisce altrimenti — a differenza di un `fs::write` incondizionato, non
/// c'è finestra fra «ho letto che è libera» e «l'ho scritta» in cui un'altra
/// sessione possa infilarsi. È la correzione al difetto riprodotto dal
/// revisore con due processi reali sulla stessa voce: prima si leggeva,
/// decideva, e SOLO POI si scriveva, e in mezzo non c'era nessun lucchetto.
fn try_declare_taken(entry_id: &str, session_id: &str) -> bool {
    if entry_id.is_empty() || session_id.is_empty() {
        return false;
    }
    if fs::create_dir_all(takers_dir()).is_err() {
        return false;
    }
    let body = serde_json::json!({
        "entry_id": entry_id,
        "session_id": session_id,
        "taken_at_epoch": now_epoch(),
    });
    match fs::OpenOptions::new().write(true).create_new(true).open(taker_path(entry_id)) {
        Ok(mut f) => {
            use std::io::Write as _;
            let _ = f.write_all(body.to_string().as_bytes());
            true
        }
        // Un'altra sessione l'ha rivendicata nello stesso istante: la voce
        // non è più nostra, non si sovrascrive il suo marcatore.
        Err(_) => false,
    }
}

fn alive_short_ids() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(live_dir()) {
        for e in entries.flatten() {
            if let Some(stem) = e.path().file_stem().and_then(|s| s.to_str()) {
                out.insert(stem.to_string());
            }
        }
    }
    out
}

/// Quando la sessione ha toccato il proprio transcript l'ultima volta. Stessa
/// forma di `uncovered_thread::last_activity_of`, duplicata invece che
/// condivisa: sono due giudizi isolati apposta, ciascuno provabile da solo.
fn last_activity_of(session_id: &str) -> Option<i64> {
    let raw = fs::read_to_string(live_dir().join(format!("{}.json", short(session_id)))).ok()?;
    let record = serde_json::from_str::<Value>(&raw).ok()?;
    let transcript = record.get("transcript_path").and_then(|v| v.as_str())?;
    let modified = fs::metadata(transcript).ok()?.modified().ok()?;
    modified.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

fn format_notice(entry: &QueueEntry, now: i64) -> String {
    let hours = (now - entry.mtime).max(0) / 3600;
    format!(
        "MANDATO DALLA CODA (nessuna staffetta da riprendere, nessun filo scoperto). \
C'e' una voce aperta per un {}, ferma da {hours} h: leggi `{}` per intero prima di agire \
— una clausola di esclusivita' puo' stare su una riga indentata, non solo nella prima — \
e occupatene: verifica quanto scrive, ripara se serve, e chiudila scrivendoci `stato: \
chiusa` con cosa hai trovato e come l'hai provato. Non ricominciare un lavoro che \
qualcun altro ha gia' misurato. Leggere per intero non e' un annuncio: il primo \
messaggio del turno resta il passo successivo, non una premessa. Se hai motivo di non \
prenderla, dillo in una riga e prosegui col tuo lavoro.",
        entry.role, entry.path,
    )
}

/// La riga che una sessione appena aperta si trova davanti, quando né la
/// staffetta né un filo scoperto le hanno già dato un incarico.
///
/// SCORRE LA FILA E TENTA LA PRESA ATOMICA VOCE PER VOCE, invece di decidere
/// un vincitore e poi scriverlo: due `SessionStart` quasi simultanei — normali
/// qui, capitano ogni volta che si aprono più pannelli insieme — possono
/// entrambi leggere la stessa voce come libera. `try_declare_taken` è dove la
/// corsa si chiude (una sola `create_new` può riuscire); se questa sessione la
/// perde, prova la prossima voce della fila invece di andarsene a mani vuote
/// — la fila resta valida perché nessun'altra scrittura nel frattempo cambia
/// l'ordine, solo la disponibilità della prima.
pub fn opening_notice(session_id: &str) -> String {
    let entries = read_open_entries();
    let open_ids: BTreeSet<String> = entries.iter().map(|e| e.id.clone()).collect();
    let alive = alive_short_ids();
    let now = now_epoch();
    sweep_taken(&open_ids, &alive, now, &last_activity_of);
    let taken: Vec<(String, String, i64)> =
        read_takers().into_iter().map(|t| (t.entry_id, t.session_id, t.taken_at_epoch)).collect();
    let ranked = rank_mandate_with(&entries, &taken, &alive, now, &last_activity_of);
    for candidate in ranked {
        if try_declare_taken(&candidate.id, session_id) {
            return format_notice(&candidate, now);
        }
    }
    String::new()
}

/// `claude-hooks mandato-coda`: quale voce si dispaccerebbe ora, senza
/// scrivere niente — un rapporto, come `fili-scoperti`.
pub fn run_report() -> i32 {
    let entries = read_open_entries();
    let alive = alive_short_ids();
    let now = now_epoch();
    let taken: Vec<(String, String, i64)> =
        read_takers().into_iter().map(|t| (t.entry_id, t.session_id, t.taken_at_epoch)).collect();
    match decide_mandate_with(&entries, &taken, &alive, now, &last_activity_of) {
        None => println!(
            "nessuna voce dispacciabile ora ({} aperte per un mestiere, {} già in mano)",
            entries.len(),
            taken.len()
        ),
        Some(e) => println!("{} · per un {} · ferma da {} h · {}", e.id, e.role, (now - e.mtime).max(0) / 3600, e.path),
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    // --- header_fields: pura, nessun disco ---

    #[test]
    fn header_fields_folds_a_multiline_value_into_one_line() {
        // Corretto dopo il guasto riprodotto dal vivo: la vecchia versione
        // troncava alla prima riga, e una clausola sulla continuazione
        // spariva. La forma è quella delle tre voci reali in coda oggi.
        let text = "---\nstato: aperta\nper: NESSUNO — testo lungo\n  continua qui\n---\n\n# corpo\n";
        let fields = header_fields(text);
        assert_eq!(field(&fields, "stato"), Some("aperta"));
        assert_eq!(field(&fields, "per"), Some("NESSUNO — testo lungo continua qui"));
    }

    #[test]
    fn header_fields_without_a_leading_dashes_line_is_empty() {
        assert!(header_fields("# solo un titolo\nstato: aperta\n").is_empty());
    }

    #[test]
    fn header_fields_never_closed_is_empty() {
        assert!(header_fields("---\nstato: aperta\n# nessun secondo tratto\n").is_empty());
    }

    // --- recipient_role / is_dispatchable ---

    /// La sola parte della tupla che questi test guardano.
    fn role_of(fields: &[(String, String)]) -> Option<&'static str> {
        is_dispatchable(fields).map(|(role, _)| role)
    }

    #[test]
    fn a_voice_for_theo_is_never_dispatched() {
        // IL VINCOLO DI MERITO: una voce che chiede una decisione di Theo non
        // va a una sessione qualunque, che sceglierebbe al posto suo.
        let fields = vec![("stato".into(), "aperta".into()), ("per".into(), "Theo".into())];
        assert_eq!(role_of(&fields), None);
    }

    #[test]
    fn the_old_format_field_is_read_too() {
        // Il formato vecchio (`destinatario:`) è ancora vivo in 5 voci
        // misurate il 25/08/2026, e THEO è l'unico valore osservato lì.
        let fields = vec![("stato".into(), "aperta".into()), ("destinatario".into(), "THEO".into())];
        assert_eq!(role_of(&fields), None);
        // La prova che il campo si legge davvero, non solo che THEO esclude:
        // un valore dispacciabile nello stesso campo deve passare.
        let fields = vec![("stato".into(), "aperta".into()), ("destinatario".into(), "builder".into())];
        assert_eq!(role_of(&fields), Some("builder"));
    }

    #[test]
    fn a_missing_recipient_field_defaults_to_theo_not_to_the_state() {
        let fields = vec![("stato".into(), "aperta".into())];
        assert_eq!(role_of(&fields), None);
    }

    #[test]
    fn nobody_and_unknown_words_are_not_dispatched() {
        let fields = vec![("stato".into(), "aperta".into()), ("per".into(), "NESSUNO — misura di sé".into())];
        assert_eq!(role_of(&fields), None);
        let fields = vec![("stato".into(), "aperta".into()), ("per".into(), "la sessione generale".into())];
        assert_eq!(role_of(&fields), None);
    }

    #[test]
    fn articles_are_skipped_the_way_queue_select_does() {
        let fields =
            vec![("stato".into(), "aperta".into()), ("per".into(), "un builder su ~/.claude/rust — riga 395".into())];
        assert_eq!(role_of(&fields), Some("builder"));
    }

    #[test]
    fn the_four_reviewer_synonyms_all_count_as_reviewer() {
        for word in ["reviewer", "codereviewer", "securityreviewer"] {
            let fields = vec![("stato".into(), "aperta".into()), ("per".into(), word.into())];
            assert_eq!(role_of(&fields), Some("reviewer"), "{word}");
        }
    }

    #[test]
    fn closed_or_waiting_for_theo_entries_are_never_open() {
        for state in ["chiusa", "attesa-theo"] {
            let fields = vec![("stato".into(), state.into()), ("per".into(), "builder".into())];
            assert_eq!(role_of(&fields), None, "{state}");
        }
        // Lo stato storico resta aperto, come nel selettore bash.
        let fields = vec![("stato".into(), "attesa-capitano".into()), ("per".into(), "builder".into())];
        assert_eq!(role_of(&fields), Some("builder"));
    }

    // --- cited_session_ids / occupied_by_a_cited_session: pura ---

    #[test]
    fn cited_session_ids_finds_only_eight_hex_tokens() {
        // LA VOCE REALE CHE HA RESO CONCRETO IL DIFETTO, riga per riga.
        let raw = "builder — la sessione che sta lavorando `setaccio-rami.sh` adesso \
                   (`790ca4e8`, …) — cantiere aperto, NON intervenire da fuori";
        assert_eq!(cited_session_ids(raw), vec!["790ca4e8".to_string()]);
        // Un token piu' corto o piu' lungo di otto non e' un id di sessione.
        assert!(cited_session_ids("commit a1b2c3 e poi a1b2c3d4e5").is_empty());
        assert!(cited_session_ids("nessun id qui dentro").is_empty());
        // UNA DATA COMPATTA NON E' UN IDENTIFICATIVO: struttura di data
        // riconosciuta (20 + mese 01-12 + giorno 01-31), non assenza di
        // lettere -- il difetto moderato che poteva blindare una voce.
        assert!(cited_session_ids("rivista il 20260824 e poi il 20260825").is_empty());
    }

    #[test]
    fn cited_session_ids_discards_a_real_date_from_the_queue() {
        // La data reale delle due righe `rivista:` sulla voce
        // `il-setaccio-dei-rami` (24/08/2026, 13:42 e 16:34): non inventata,
        // e' quella che ha davvero riarmato l'orologio della voce due volte.
        assert!(cited_session_ids("rivista: 2026-08-24 13:42, poi ancora 20260824 alle 16:34").is_empty());
    }

    #[test]
    fn cited_session_ids_recognizes_real_digit_only_session_ids() {
        // Il filtro vecchio (nessuna lettera esadecimale) scartava questi due
        // insieme a una sessione reale su trenta. Non inventati: sono due dei
        // 50 id di sole cifre su 1.502 misurati in `~/.claude/projects/*/`.
        assert_eq!(
            cited_session_ids("visti in `04828377` e in `74700644`"),
            vec!["04828377".to_string(), "74700644".to_string()]
        );
    }

    #[test]
    fn a_single_cited_live_session_occupies_the_entry() {
        let holders = vec!["790ca4e8".to_string()];
        let busy = |_: &str| Some(999i64);
        assert!(occupied_by_a_cited_session(&holders, &alive(&["790ca4e8"]), 0, 1000, &busy));
    }

    #[test]
    fn a_single_cited_dead_session_does_not_occupy_the_entry() {
        let holders = vec!["790ca4e8".to_string()];
        assert!(!occupied_by_a_cited_session(&holders, &alive(&[]), 0, 1000, &silent));
    }

    #[test]
    fn two_or_more_cited_sessions_abstain_and_count_as_occupied() {
        // Stessa prudenza di `queue-select.sh::holders_alive`: una riga con
        // piu' nomi racconta una storia (ceduta, riassegnata) che non si
        // riduce affidabilmente a un solo titolare. Fresca (since == now):
        // l'astensione vale ancora.
        let holders = vec!["790ca4e8".to_string(), "aaaabbbb".to_string()];
        assert!(occupied_by_a_cited_session(&holders, &alive(&[]), 1000, 1000, &silent));
    }

    #[test]
    fn two_or_more_cited_sessions_stop_abstaining_after_the_grace_period() {
        // IL DIFETTO MODERATO RIPRODOTTO: prima della correzione questo ramo
        // restituiva `true` per sempre, senza controllo di eta' -- a
        // differenza del marcatore proprio, che ha la grazia di sei ore
        // (`holder_still_holds`).
        let holders = vec!["790ca4e8".to_string(), "aaaabbbb".to_string()];
        let since = 1000;
        let now = since + STALE_AFTER_SECONDS;
        assert!(
            !occupied_by_a_cited_session(&holders, &alive(&[]), since, now, &silent),
            "due id citati hanno tenuto la voce occupata oltre le sei ore di grazia"
        );
    }

    #[test]
    fn no_cited_session_never_occupies_the_entry() {
        assert!(!occupied_by_a_cited_session(&[], &alive(&["790ca4e8"]), 0, 1000, &silent));
    }

    // --- decide_mandate_with / rank_mandate_with: puro, iniettabile ---

    fn entry(id: &str, role: &str, mtime: i64) -> QueueEntry {
        entry_holding(id, role, mtime, &[])
    }

    fn entry_holding(id: &str, role: &str, mtime: i64, holders: &[&str]) -> QueueEntry {
        QueueEntry {
            id: id.to_string(),
            path: format!("/coda/{id}.md"),
            role: role.to_string(),
            mtime,
            self_declared_holders: holders.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn alive(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn silent(_: &str) -> Option<i64> {
        None
    }

    #[test]
    fn no_entries_means_no_mandate() {
        assert!(decide_mandate_with(&[], &[], &alive(&[]), 1000, &silent).is_none());
    }

    #[test]
    fn the_oldest_free_entry_wins() {
        let entries = [entry("nuova", "builder", 900), entry("vecchia", "builder", 100)];
        let out = decide_mandate_with(&entries, &[], &alive(&[]), 1000, &silent).unwrap();
        assert_eq!(out.id, "vecchia");
    }

    #[test]
    fn an_entry_held_by_a_live_active_session_is_skipped() {
        let entries = [entry("presa", "builder", 100), entry("libera", "builder", 200)];
        let taken = [("presa".to_string(), "aaaabbbb-1111".to_string(), 50)];
        let busy = |_: &str| Some(999i64); // ha toccato il transcript un attimo fa
        let out = decide_mandate_with(&entries, &taken, &alive(&["aaaabbbb"]), 1000, &busy).unwrap();
        assert_eq!(out.id, "libera", "una voce tenuta da chi ci lavora ancora non va data a un'altra sessione");
    }

    #[test]
    fn a_freshly_taken_entry_is_not_freed_just_because_its_session_is_unregistered() {
        // IL DIFETTO CHE LA MIA STESSA PROVA DI CONCORRENZA HA TROVATO (non
        // segnalato dal revisore): la presa in carico è vecchia di un attimo
        // (`since` = `now`), e il titolare non risulta vivo — puo' non essere
        // ancora registrato in `sessioni-vive/`, non puo' essere morto un
        // istante dopo aver scritto il proprio marcatore. Prima della
        // correzione questo caso tornava libero all'istante: uno spazzino
        // concorrente avrebbe potuto sfrattare un marcatore appena scritto.
        let entries = [entry("sola", "builder", 100)];
        let taken = [("sola".to_string(), "aaaabbbb-1111".to_string(), 1000)];
        let out = decide_mandate_with(&entries, &taken, &alive(&[]), 1000, &silent);
        assert!(out.is_none(), "una presa vecchia di un istante e' stata data via subito: {out:?}");
    }

    #[test]
    fn an_entry_whose_holder_was_never_seen_alive_frees_up_after_the_grace_period() {
        // La stessa presa, ma vecchia: senza mai un segno di vita per sei ore,
        // «non trovata viva» smette di valere come un dubbio e la voce torna
        // dispacciabile — altrimenti un titolare mai registrato la terrebbe
        // per sempre, ed e' esattamente il difetto opposto.
        let entries = [entry("sola", "builder", 100)];
        let taken = [("sola".to_string(), "aaaabbbb-1111".to_string(), 1000)];
        let now = 1000 + STALE_AFTER_SECONDS;
        let out = decide_mandate_with(&entries, &taken, &alive(&[]), now, &silent);
        assert!(out.is_some(), "un titolare mai visto vivo per sei ore doveva liberare la voce");
    }

    #[test]
    fn an_entry_whose_holder_went_quiet_for_six_hours_is_free_again() {
        let entries = [entry("sola", "builder", 100)];
        let taken = [("sola".to_string(), "aaaabbbb-1111".to_string(), 1000)];
        let now = 1000 + STALE_AFTER_SECONDS;
        let quiet = |_: &str| Some(now - STALE_AFTER_SECONDS - 1);
        assert!(decide_mandate_with(&entries, &taken, &alive(&["aaaabbbb"]), now, &quiet).is_some());
        let one_second_early = now - 1;
        let busy = |_: &str| Some(one_second_early);
        assert!(
            decide_mandate_with(&entries, &taken, &alive(&["aaaabbbb"]), one_second_early, &busy).is_none(),
            "un secondo prima delle sei ore la tiene ancora chi l'ha presa"
        );
    }

    #[test]
    fn an_entry_that_cites_a_live_session_in_its_own_text_is_skipped() {
        // IL DIFETTO GRAVE #2 RIPRODOTTO: la voce reale in coda
        // (`2026-08-24-il-setaccio-dei-rami-scatta-alle-815-e-sara-muto.md`)
        // scrive «per: builder — …NON intervenire da fuori (`790ca4e8`)». Il
        // ruolo da solo la classificherebbe dispacciabile; qui non deve
        // esserlo, perché la sessione che cita è viva.
        let entries = [
            entry_holding("il-setaccio", "builder", 100, &["790ca4e8"]),
            entry("altra-voce", "builder", 200),
        ];
        let busy = |_: &str| Some(999i64);
        let out = decide_mandate_with(&entries, &[], &alive(&["790ca4e8"]), 1000, &busy).unwrap();
        assert_eq!(out.id, "altra-voce", "una voce che cita chi ci lavora ancora e' stata dispacciata lo stesso");
    }

    #[test]
    fn an_entry_citing_two_sessions_is_dispatchable_again_once_the_entry_itself_is_stale() {
        // Il collegamento fra `rank_mandate_with` e la scadenza: l'eta' che
        // conta per un'astensione a piu' id e' quella della voce (`mtime`),
        // non un now qualunque -- e' cosi' che la correzione arriva davvero a
        // `opening_notice`.
        let entries = [entry_holding("doppia", "builder", 100, &["790ca4e8", "aaaabbbb"])];
        let now = 100 + STALE_AFTER_SECONDS;
        let out = decide_mandate_with(&entries, &[], &alive(&[]), now, &silent);
        assert!(out.is_some(), "due id citati su una voce ferma da sei ore dovevano liberarla");
    }

    #[test]
    fn an_entry_that_cites_a_now_dead_session_is_dispatchable_again() {
        let entries = [entry_holding("il-setaccio", "builder", 100, &["790ca4e8"])];
        // La sessione citata non risulta piu' viva: il cantiere non ha piu' un
        // guardiano, e la voce torna dispacciabile.
        let out = decide_mandate_with(&entries, &[], &alive(&[]), 1000, &silent).unwrap();
        assert_eq!(out.id, "il-setaccio");
    }

    // --- opening_notice: I/O, con HOME isolata ---

    fn write_entry(home: &HomeIsolata, name: &str, state: &str, recipient: &str) -> PathBuf {
        let dir = home.dir.join(".claude/state/plancia/segnalazioni");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.md"));
        fs::write(&path, format!("---\nstato: {state}\nper: {recipient}\n---\n\n# {name}\n")).unwrap();
        path
    }

    #[test]
    fn opening_notice_says_nothing_when_the_queue_has_nothing_dispatchable() {
        let home = HomeIsolata::nuova("coda-mandato-vuota");
        assert_eq!(opening_notice("aaaabbbb-1111"), "");
        let _ = home;
    }

    #[test]
    fn opening_notice_hands_over_the_oldest_open_entry_and_marks_it_taken() {
        let home = HomeIsolata::nuova("coda-mandato-dispaccio");
        write_entry(&home, "2026-08-20-vecchia", "aperta", "builder");
        write_entry(&home, "2026-08-25-nuova", "aperta", "builder");
        let notice = opening_notice("aaaabbbb-1111");
        assert!(notice.contains("2026-08-20-vecchia"), "non ha dato la voce piu' vecchia: {notice}");
        assert!(notice.contains("builder"), "non dice per chi e': {notice}");
        assert!(notice.contains("occupatene"), "informa invece di incaricare: {notice}");
        assert!(notice.contains("per intero"), "non chiede di leggere la voce per intero: {notice}");
        assert!(notice.contains("Se hai motivo di non prenderla"), "non resta rifiutabile: {notice}");
        assert!(taker_path("2026-08-20-vecchia").exists(), "non ha marcato la presa in carico");
    }

    #[test]
    fn a_second_session_does_not_get_the_entry_a_live_session_already_took() {
        let home = HomeIsolata::nuova("coda-mandato-doppia-presa");
        write_entry(&home, "unica", "aperta", "measurer");
        let first = opening_notice("aaaabbbb-1111");
        assert!(first.contains("unica"));
        // Registra la prima sessione come viva, col transcript appena toccato.
        let live = home.dir.join(".claude/state/sessioni-vive");
        fs::create_dir_all(&live).unwrap();
        let transcript = home.dir.join("transcript.jsonl");
        fs::write(&transcript, "x").unwrap();
        fs::write(
            live.join("aaaabbbb.json"),
            format!(r#"{{"transcript_path": "{}"}}"#, transcript.to_string_lossy()),
        )
        .unwrap();
        let second = opening_notice("ccccdddd-2222");
        assert_eq!(second, "", "due sessioni sulla stessa voce: {second}");
    }

    #[test]
    fn a_clause_on_the_second_line_of_per_still_blocks_the_dispatch() {
        // IL GUASTO GRAVE RIPRODOTTO DAL VIVO DAL REVISORE: stessa voce del
        // caso `il-setaccio`, ma con la clausola di esclusività spostata sulla
        // riga di continuazione — la forma vera trovata nelle tre voci reali
        // che usano `per:` su più righe. Prima della correzione
        // `header_fields` troncava alla prima riga e la clausola spariva: la
        // voce veniva dispacciata anche con la sessione citata ancora viva.
        let home = HomeIsolata::nuova("coda-mandato-per-multiriga");
        let dir = home.dir.join(".claude/state/plancia/segnalazioni");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("cantiere.md"),
            "---\nstato: aperta\nper: builder — cantiere aperto\n  NON intervenire da fuori (`790ca4e8`)\n---\n\n# corpo\n",
        )
        .unwrap();
        let live = home.dir.join(".claude/state/sessioni-vive");
        fs::create_dir_all(&live).unwrap();
        let transcript = home.dir.join("transcript.jsonl");
        fs::write(&transcript, "x").unwrap();
        fs::write(
            live.join("790ca4e8.json"),
            format!(r#"{{"transcript_path": "{}"}}"#, transcript.to_string_lossy()),
        )
        .unwrap();
        let notice = opening_notice("ffffeeee-0000");
        assert_eq!(notice, "", "clausola sulla seconda riga ignorata, voce dispacciata lo stesso: {notice}");
    }

    #[test]
    fn a_closed_entry_releases_its_marker() {
        let home = HomeIsolata::nuova("coda-mandato-chiusura");
        let path = write_entry(&home, "unica", "aperta", "investigator");
        let first = opening_notice("aaaabbbb-1111");
        assert!(first.contains("unica"));
        assert!(taker_path("unica").exists());
        // Chi ha lavorato la voce la chiude.
        fs::write(&path, "---\nstato: chiusa\nper: investigator\n---\n\nfatto\n").unwrap();
        let second = opening_notice("ccccdddd-2222");
        assert_eq!(second, "", "la voce e' chiusa, non deve piu' chiamare nessuno");
        assert!(!taker_path("unica").exists(), "il marcatore di una voce chiusa doveva sparire");
    }

    // --- la corsa vera: thread reali sullo stesso disco, non simulata ---

    /// IL DIFETTO GRAVE #1 RIPRODOTTO, con thread veri invece di due processi
    /// (il revisore ha usato due processi; qui la garanzia che conta è la
    /// stessa: `create_new` è atomica a livello di sistema operativo, e la
    /// linea di confine non è il thread o il processo, è la sincronizzazione
    /// del filesystem). Due tentativi partono nello stesso istante — un
    /// `Barrier` li tiene fermi finché non sono entrambi pronti — su UNA sola
    /// voce: prima della correzione `fs::write` non incondizionato avrebbe
    /// lasciato vincere entrambi.
    #[test]
    fn only_one_of_two_concurrent_claims_wins_the_same_entry() {
        let _home = HomeIsolata::nuova("coda-mandato-corsa-marcatore");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|i| {
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    try_declare_taken("voce-contesa", &format!("sessione-{i}"))
                })
            })
            .collect();
        let results: Vec<bool> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(
            results.iter().filter(|ok| **ok).count(),
            1,
            "due prese simultanee sulla stessa voce devono lasciarne vincere una sola: {results:?}"
        );
    }

    /// Lo stesso difetto all'estremo che vede una sessione: due `SessionStart`
    /// quasi simultanei su UNA voce dispacciabile sola non devono ricevere lo
    /// stesso mandato.
    #[test]
    fn two_concurrent_sessions_never_get_the_same_single_entry() {
        let home = HomeIsolata::nuova("coda-mandato-corsa-notice");
        write_entry(&home, "unica", "aperta", "builder");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|i| {
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    opening_notice(&format!("sessione-{i}"))
                })
            })
            .collect();
        let results: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let non_empty = results.iter().filter(|s| !s.is_empty()).count();
        assert_eq!(non_empty, 1, "due sessioni concorrenti hanno ricevuto lo stesso mandato: {results:?}");
    }

    /// I due capi sono cablati davvero: `register_session` interroga questo
    /// modulo, e lo fa DOPO il controllo dei fili scoperti — non prima, non al
    /// posto suo. Stessa forma di
    /// `uncovered_thread::both_ends_of_the_marker_are_wired_where_they_belong`.
    #[test]
    fn is_wired_after_the_uncovered_thread_check() {
        let starts = include_str!("register_session.rs");
        // LA FORMA È CAMBIATA IL 25/08/2026, LA PROPRIETÀ NO. Le due fonti non
        // si chiamano più sul posto: si passano a `opening_message` **non
        // ancora eseguite**, così è lei a decidere chi interrogare e chi no, e
        // quella scelta si prova a parte con le chiusure. Qui resta da
        // sorvegliare il solo cablaggio: che a quella composizione arrivino le
        // funzioni vere, e nell'ordine giusto.
        let mandate_at = starts
            .find("crate::queue_mandate::opening_notice")
            .expect("nessuno interroga più la coda quando una sessione parte: resta con due sole fonti di mandato");
        let uncovered_at = starts
            .find("crate::uncovered_thread::opening_notice")
            .expect("il controllo dei fili scoperti non si legge più da qui");
        assert!(
            uncovered_at < mandate_at,
            "la coda va interrogata DOPO i fili scoperti: invertirle cambia quale mandato vince"
        );
    }
}
