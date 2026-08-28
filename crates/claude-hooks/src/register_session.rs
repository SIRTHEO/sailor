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
use hook_io::journal::{self, Field};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Un segnale di ripresa più vecchio di dieci minuti è stantio.
const FRESH_SEC: u64 = 600;

/// Le famiglie di marcatori che il presidio della consegna scrive per una
/// sessione. L'elenco è CHIUSO e sta in un posto solo: quando ne esistevano due
/// copie, la famiglia nata dopo era presente in una sola.
pub(crate) const MARKER_FAMILIES: &[&str] = &[
    "consegna-fatta",
    "consegna-fatta-ripartenze",
    "consegna-blocchi",
    "consegna-stop",
    "consegna-avvisata",
    "consegna-misura",
    "consegna-ripartenze",
    // AGGIUNTA IL 18/08/2026, e il commento qui sopra la contava già fra i 176
    // marcatori rimasti sul disco: era stata guardata e non aggiunta.
    "consegna-volontaria",
    // AGGIUNTA IL 21/08/2026, stesso difetto per la terza volta: la scrive
    // `handoff_on_stop::lockout_reference` da giorni e nessuno l'ha mai buttata.
    // Trovata da un caso di `marker_sweep`, non a occhio — ed è la ragione per
    // cui l'elenco ora ha un test che lo confronta coi sorgenti che scrivono.
    "consegna-stop-riferimento",
    // AGGIUNTA IL 21/08/2026, stesso schema del gemello qui sopra ma per il
    // presidio PostToolUse: la scrive `handoff_required::lockout_reference`,
    // il riferimento di crescita sopra il gradino di blocco (punto 1 della
    // segnalazione «il gancio della consegna non si sblocca»).
    "consegna-riferimento-lockout",
];

/// `successore-di-` fa eccezione: porta l'identificativo **intero**, non i primi
/// otto come tutti gli altri. Chi cancella per prefisso corto lo manca sempre, e
/// lo manca in silenzio.
const FULL_ID_FAMILIES: &[&str] = &["successore-di"];

/// E questa non porta l'id affatto, ma la sua impronta:
/// `successore-armato-<sha1(session)[..16]>`. Non essendo il nome leggibile,
/// nessuno aveva pensato a toglierla — 20 file il 18/08/2026, e provato lo
/// stesso giorno che due corrispondono all'impronta di sessioni ancora nominate
/// sul disco. Derivabile significa cancellabile.
const FINGERPRINT_FAMILIES: &[&str] = &["successore-armato"];

pub(crate) fn state_dir() -> PathBuf {
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

/// Le chiavi d'ambiente che mancano per poter registrare. Elenco vuoto: si
/// registra.
///
/// Sta a parte perché è la condizione che faceva sparire sessioni vive senza
/// dirlo, e una condizione che si può provare solo dal vivo non si prova. Le
/// due chiavi del pannello si contano insieme: ne basta una, ed è già scritto
/// in testa al modulo perché.
pub(crate) fn missing_panel_keys(worktree: &str, handle: &str, tab: &str) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if worktree.trim().is_empty() {
        missing.push("ORCA_WORKTREE_ID");
    }
    if handle.trim().is_empty() && tab.trim().is_empty() {
        missing.push("ORCA_TAB_ID|ORCA_TERMINAL_HANDLE");
    }
    missing
}

/// Se questa sessione gira dentro Orca. La porta del gancio di Orca è l'unico
/// segno che c'è **prima** delle chiavi che si stanno cercando: usare una di
/// quelle per dire «siamo dentro Orca» renderebbe la condizione circolare, e il
/// caso da denunciare — dentro Orca, chiavi assenti — non uscirebbe mai.
fn inside_orca() -> bool {
    !env("ORCA_AGENT_HOOK_PORT").trim().is_empty()
}

/// Il tetto ai salti quando si risale la catena dei padri: abbastanza per
/// superare una shell interposta fra il gancio e `claude` (`sh -c "..."`),
/// non tanto da rischiare di prendere un `claude` che non è il nostro, più su
/// nell'albero di Orca.
const MAX_ANCESTOR_HOPS: usize = 4;

/// Un anello della catena dei padri: pid e nome del comando così come lo dà
/// `ps -o comm=`. Misurato risalendo da questo terminale il 21/08/2026: a
/// volte è il nome corto (`claude`), a volte il percorso intero di un app
/// bundle con spazi dentro (`Orca Helper`) — mai una via di mezzo.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessLink {
    pid: u32,
    comm: String,
}

/// Il comando è il processo della sessione? Si confronta l'ultimo pezzo del
/// percorso, perché `comm` arriva ora corto ora intero.
fn is_claude_process(comm: &str) -> bool {
    comm.rsplit('/').next().unwrap_or(comm) == "claude"
}

/// Quale pid scrivere, data la catena dei padri già letta. PURA: non tocca il
/// sistema, quindi si prova senza processi veri. Si ferma al primo `claude`
/// entro `max_hops`; oltre il tetto, o se non c'è affatto, non lo sa — e
/// «non lo sa» è un esito legittimo, non un errore.
fn choose_session_pid(chain: &[ProcessLink], max_hops: usize) -> Option<u32> {
    chain.iter().take(max_hops).find(|p| is_claude_process(&p.comm)).map(|p| p.pid)
}

/// PURA: i tre campi di una riga di `ps -o pid=,ppid=,comm=`.
///
/// DUE TRAPPOLE INSIEME, e chi le prende una per volta ne lascia sempre una.
/// `ps` allinea il pid a destra su cinque caratteri, quindi sotto le cinque
/// cifre fra i campi c'è più di uno spazio e la riga comincia con spazi; e il
/// nome del comando può contenere spazi a sua volta (`Orca Helper`). Perciò
/// non si spezza a ogni spazio — `splitn(3, char::is_whitespace)` prendeva
/// `7969 claude` per nome di comando, e `split_whitespace().nth(2)` taglierebbe
/// `Orca Helper` a `Orca`. Si saltano due campi e si prende tutto il resto.
///
/// UNICO LETTORE DELLE COLONNE DI `ps` in questo modulo: il difetto del
/// 21/08/2026 è nato dall'averne avuti due, uno riparato e l'altro no.
fn ps_fields(line: &str) -> Option<(u32, u32, &str)> {
    let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
    let (ppid, rest) = rest.trim_start().split_once(char::is_whitespace)?;
    Some((pid.parse().ok()?, ppid.parse().ok()?, rest.trim()))
}

/// Pid, pid del padre e nome del comando di un processo, letti da `ps`.
/// `None` se il pid non esiste (più): la catena si legge mentre il mondo dei
/// processi cambia sotto i piedi, e un padre può essere già morto.
fn process_info(pid: u32) -> Option<(u32, u32, String)> {
    let output = std::process::Command::new("ps")
        .args(["-o", "pid=,ppid=,comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout);
    let (read_pid, ppid, comm) = ps_fields(&line)?;
    if comm.is_empty() {
        return None;
    }
    Some((read_pid, ppid, comm.to_string()))
}

/// Risale i padri di `start_pid` leggendo `ps`, fino a `max_hops` anelli o al
/// capostipite (`launchd`, ppid 0). L'IMPURITÀ VIVE SOLO QUI: `choose_session_pid`
/// decide, questa funzione si limita a guardare.
fn ancestor_chain(start_pid: u32, max_hops: usize) -> Vec<ProcessLink> {
    let mut chain = Vec::new();
    let Some((_, mut next_pid, _)) = process_info(start_pid) else {
        return chain;
    };
    for _ in 0..max_hops {
        if next_pid == 0 {
            break;
        }
        let Some((pid, ppid, comm)) = process_info(next_pid) else {
            break;
        };
        chain.push(ProcessLink { pid, comm });
        if ppid == pid {
            break; // capostipite padre di se stesso: non risalire in loop
        }
        next_pid = ppid;
    }
    chain
}

/// Il pid della sessione, risalendo dal processo del gancio fino a `claude`.
fn session_pid() -> Option<u32> {
    let chain = ancestor_chain(std::process::id(), MAX_ANCESTOR_HOPS);
    choose_session_pid(&chain, MAX_ANCESTOR_HOPS)
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
    //
    // QUI SI USCIVA MUTI, e il registro delle sessioni vive ne portava il segno:
    // la notte del 21/08/2026 conteneva quattro record contro sei sessioni vere,
    // e chi conta da lì chiude un pannello vivo credendolo morto. Dentro Orca la
    // mancanza è un guasto e va detta; fuori da Orca è il caso normale e resta
    // muta, altrimenti ogni sessione aperta da un terminale qualunque scriverebbe
    // una riga inutile.
    let missing = missing_panel_keys(&worktree, &handle, &tab);
    if !missing.is_empty() {
        if inside_orca() {
            journal::record(
                "register-session",
                "salta",
                "chiavi-del-pannello-mancanti",
                &[
                    ("session", Field::Text(sess.clone())),
                    ("mancanti", Field::Text(missing.join(" "))),
                    (
                        "cwd",
                        Field::Text(
                            data.get("cwd")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        ),
                    ),
                ],
            );
        }
        return;
    }
    let cwd = match data.get("cwd").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    // L'ordine delle chiavi era quello del dizionario Python, confrontato
    // byte per byte da un test di equivalenza: rimosso il 19/08/2026 insieme
    // all'originale Python che confrontava (commit 25fc2617), perché quel
    // Python non esiste più. Nessun lettore noto di questo file dipende
    // dall'ordine oggi; lo si tiene comunque, `serde_json` lo mantiene gratis
    // con `preserve_order`, e cambiarlo senza motivo è un rischio che non
    // rende niente.
    //
    // `session_pid`: `None` diventa `null`, distinto da qualunque pid vero —
    // «non l'ho saputo» non è «morto», ed è chi legge poi a scoprire quello.
    let record = serde_json::json!({
        "session_id": full,
        "terminal_handle": handle,
        "worktree_id": worktree,
        "tab_id": tab,
        "session_pid": session_pid(),
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
        // L'impronta si calcola sull'identificativo INTERO e non filtrato,
        // perché è esattamente ciò che il gancio passa a `hashlib.sha1`: con la
        // forma corta, o con quella ripulita, il nome uscirebbe diverso e non si
        // cancellerebbe niente. `guards::sha1` esiste per dare le stesse cifre.
        let digest = guards::duplication::sha1(full_id.as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        for f in FINGERPRINT_FAMILIES {
            paths.push(state.join(format!("{f}-{}", &hex[..16])));
        }
    }
    paths
}

/// Cosa si sa della sessione che ha appena mandato `SessionEnd`.
///
/// Tre esiti, non due: il campo del processo è nato alle 11:30 del 21/08/2026 e
/// i record scritti prima non lo portano. Un campo **assente** dice «non lo so»,
/// e leggerlo come «morta» è l'errore che ha cancellato il registro di una
/// sessione viva.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionLiveness {
    /// Il record porta un processo, e quel processo è un `claude` che gira.
    Alive,
    /// Il record porta un processo, e quel processo non c'è più.
    Gone,
    /// Non c'è il record, o non porta il processo: la domanda non ha risposta.
    Unknown,
}

/// Quanto si aspetta prima di buttare ciò che riguarda una sessione di cui non
/// si sa se è viva. Non è una stima del tempo che vive una sessione: è il tempo
/// oltre il quale la sporcizia costa più del rischio — il 18/08/2026 sul disco
/// c'erano 176 marcatori fermi da quattro giorni.
///
/// QUESTA SOGLIA MORDE UNA VOLTA SOLA, E VA SAPUTO. `forget_session` gira al
/// `SessionEnd` di quella sessione e mai più: **nessuno ripassa a ricontrollare
/// l'età**, perché in questa casa non esiste un raccoglitore — chi scrive un
/// marcatore lo butta lui. Quindi ciò che al proprio congedo è ancora fresco e
/// non si sa vivo resta sul disco per sempre, non per un giorno.
///
/// È una scelta, non una svista, ed è asimmetrica apposta: un marcatore orfano
/// costa spazio e nessuno lo interroga, mentre cancellare quello di una sessione
/// viva le toglie la consegna e ferma il ricambio.
///
/// La passata che ripassa esiste da oggi — `marker_sweep`, e usa **questo**
/// giudizio, non l'età nuda — ma NON È IN SERVIZIO: nessuna radice la invoca e
/// nessun servizio la sveglia, quindi la frase qui sopra descrive ancora il
/// presente. Chi la mette in servizio corregga questo commento nello stesso
/// turno.
pub(crate) const UNKNOWN_GRACE_SECS: u64 = 24 * 60 * 60;

/// PURA: si cancella questo file? Dipende da cosa si sa e da quanto è vecchio.
///
/// La regola in una riga: **si butta solo ciò che si sa morto, o ciò che è
/// abbastanza vecchio da non poter più servire a nessuno.** Il caso che questa
/// funzione esiste per impedire è il primo — una sessione viva che si ritrova il
/// proprio registro cancellato e sparisce da chi conta chi è di guardia.
///
/// È IL GIUDIZIO DI TUTTI E DUE. Il congedo e la passata di `marker_sweep`
/// chiamano questa e nessun'altra: una seconda regola scritta altrove sarebbe
/// libera di scendere sotto il giorno di grazia alla prima voglia di pulizia.
pub(crate) fn should_remove(liveness: SessionLiveness, file_age_secs: u64) -> bool {
    match liveness {
        SessionLiveness::Alive => false,
        SessionLiveness::Gone => true,
        SessionLiveness::Unknown => file_age_secs >= UNKNOWN_GRACE_SECS,
    }
}

/// PURA: cosa dice il record, dato il processo che ci sta scritto e l'esito
/// della verifica su quel processo. Separata dal sistema apposta, così i tre
/// esiti si provano senza processi veri.
///
/// UNA SOLA RISPOSTA È UNA MORTE, ed è `NotFound`: `ps` ha girato e quel pid
/// non esiste. Tutto il resto è un «non lo so», e il giorno di grazia decide.
///
/// `OtherProgram` valeva `Gone` fino al 21/08/2026, ed era un'inferenza — «il
/// nome non è `claude`, dunque il pid è stato riciclato» — travestita da
/// osservazione. Chi legge male la riga di `ps` finisce lì dentro, e ci è
/// finito: due sessioni vive su sei si cancellavano i propri marcatori al
/// congedo. Con la distinzione al posto giusto, lo stesso difetto sarebbe
/// costato ventiquattr'ore di polvere invece di due registri vivi.
fn liveness_from(pid: Option<u32>, lookup: ProcessLookup) -> SessionLiveness {
    match (pid, lookup) {
        (None, _) => SessionLiveness::Unknown,
        (Some(_), ProcessLookup::Unavailable) => SessionLiveness::Unknown,
        (Some(_), ProcessLookup::IsClaude) => SessionLiveness::Alive,
        (Some(_), ProcessLookup::OtherProgram) => SessionLiveness::Unknown,
        (Some(_), ProcessLookup::ExistsNameUnknown) => SessionLiveness::Unknown,
        (Some(_), ProcessLookup::NotFound) => SessionLiveness::Gone,
    }
}

/// Cosa ha risposto la domanda «questo processo è la sessione?».
///
/// I due modi di non avere risposta sono DIVERSI e per due giorni sono stati lo
/// stesso: `ps` che gira e dice che il pid non c'è è **una risposta** («morto»);
/// `ps` che non parte affatto — sandbox, risorse esaurite, `PATH` svuotato — non
/// lo è. Confonderli rimetteva in piedi il difetto che questo modulo esiste per
/// chiudere: bastava che `ps` non partisse per cancellare il registro di una
/// sessione viva, e senza nemmeno il giorno di grazia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessLookup {
    /// Il processo c'è e si chiama `claude`.
    IsClaude,
    /// Il processo c'è e non si chiama `claude`. Sembra «pid riciclato», ma è
    /// un «non lo so»: anche una riga letta storta arriva qui.
    OtherProgram,
    /// Il pid esiste — lo dice `kill`, non `ps` — ma il nome resta ignoto
    /// perché `ps` non era disponibile. Non è «un nome diverso da `claude`»:
    /// è «nessun nome letto». Stessa incertezza di `OtherProgram`, motivo
    /// diverso — tenerli distinti dice a chi legge da dove viene il dubbio.
    ExistsNameUnknown,
    /// `ps` ha risposto, e quel pid non esiste.
    NotFound,
    /// Non si è potuto chiedere: non è un «no», è un «non lo so».
    Unavailable,
}

/// Cosa risponde il kernel a «questo pid esiste?», letto da `kill(pid, 0)` —
/// che su Unix non manda nessun segnale, chiede solo l'esistenza. Misurato in
/// questo perimetro il 25/08/2026, dove `ps` non riesce nemmeno a partire:
/// `kill(1, 0)` dà `EPERM` (il pid 1 c'è, non ho il permesso di segnalarlo) e
/// `kill(999999, 0)` dà `ESRCH` (non c'è). Un comando esterno avrebbe dato
/// codice di uscita 1 in tutti e due i casi: è il motivo del fallimento che
/// li distingue, non l'esito.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessExistence {
    /// Il pid esiste. Non dice quale programma sia: quello lo sa solo `ps`.
    Exists,
    /// Il pid non esiste.
    Gone,
    /// Un errore diverso da `EPERM` ed `ESRCH`: non si sa leggere.
    Unknown,
}

/// PURA: cosa dice l'esito grezzo di `kill`. `ret == 0` è successo — il pid
/// esiste ed è nostro, o comunque abbiamo il permesso di segnalarlo — e vale
/// come `EPERM`: la sola differenza fra i due è il permesso, mai l'esistenza.
fn existence_from_kill_result(ret: i32, errno: Option<i32>) -> ProcessExistence {
    const EPERM: i32 = 1;
    const ESRCH: i32 = 3;
    if ret == 0 {
        return ProcessExistence::Exists;
    }
    match errno {
        Some(EPERM) => ProcessExistence::Exists,
        Some(ESRCH) => ProcessExistence::Gone,
        _ => ProcessExistence::Unknown,
    }
}

/// L'IMPURITÀ VIVE SOLO QUI. Nessuna crate `libc` per una firma sola — stesso
/// stile di `current_uid` in `reachability.rs`.
fn process_exists(pid: u32) -> ProcessExistence {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    let ret = unsafe { kill(pid as i32, 0) };
    let errno =
        if ret == 0 { None } else { std::io::Error::last_os_error().raw_os_error() };
    existence_from_kill_result(ret, errno)
}

/// Il processo scritto nel record è la sessione? L'IMPURITÀ VIVE SOLO QUI.
///
/// Non basta che il pid esista: un numero si ricicla, e un pid riassegnato a un
/// altro programma dichiarerebbe viva una sessione finita. Si guarda il nome,
/// con lo stesso confronto che usa chi il pid l'ha scritto — e per quello
/// serve `ps`. Quando `ps` non parte, si ripiega su `kill(pid, 0)`: dà solo
/// l'esistenza, mai il nome, ma basta a dire «morto» quando risponde `ESRCH`
/// — il ramo che il difetto di due giorni fa lasciava cieco.
fn look_up_session_process(pid: u32) -> ProcessLookup {
    let Ok(output) = std::process::Command::new("ps")
        .args(["-o", "pid=,ppid=,comm=", "-p", &pid.to_string()])
        .output()
    else {
        return match process_exists(pid) {
            ProcessExistence::Gone => ProcessLookup::NotFound,
            ProcessExistence::Exists => ProcessLookup::ExistsNameUnknown,
            ProcessExistence::Unknown => ProcessLookup::Unavailable,
        };
    };
    if !output.status.success() {
        return ProcessLookup::NotFound; // ha risposto: quel pid non c'è
    }
    let line = String::from_utf8_lossy(&output.stdout);
    classify_ps_line(&line)
}

/// PURA: cosa dice una riga di `ps` sul processo che ci sta scritto. Separata
/// da chi lancia `ps` perché il caso che conta — un `claude` con pid a meno di
/// cinque cifre — non si può fabbricare sul banco di prova: nessuno sceglie il
/// pid che il sistema gli assegna.
fn classify_ps_line(line: &str) -> ProcessLookup {
    match ps_fields(line).map(|(_, _, comm)| comm) {
        // Ha risposto ma la riga non si legge: nemmeno questo è un «no».
        None => ProcessLookup::Unavailable,
        Some(c) if c.is_empty() => ProcessLookup::Unavailable,
        Some(c) if is_claude_process(c) => ProcessLookup::IsClaude,
        Some(_) => ProcessLookup::OtherProgram,
    }
}

/// Il processo che il record attribuisce a questa sessione. `None` se il record
/// non c'è, non si legge, o non porta il campo — che è il caso di tutti quelli
/// scritti prima delle 11:30 del 21/08/2026.
pub(crate) fn recorded_pid(sess: &str) -> Option<u32> {
    let raw = fs::read_to_string(live_dir().join(format!("{sess}.json"))).ok()?;
    let record = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    record.get("session_pid").and_then(|v| v.as_u64()).map(|p| p as u32)
}

/// Quando il record dice di essere stato scritto l'ultima volta. `None` nelle
/// stesse condizioni di `recorded_pid`.
fn recorded_updated_at(sess: &str) -> Option<u64> {
    let raw = fs::read_to_string(live_dir().join(format!("{sess}.json"))).ok()?;
    let record = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    record.get("updated_at").and_then(|v| v.as_u64())
}

/// L'ora in cui questa macchina si è avviata, da `kern.boottime` (`sysctl`).
/// `None` se `sysctl` non parte — negato in questo stesso perimetro come `ps`
/// — o non risponde: chi chiama non applica lo scarto quando non sa l'orario,
/// perché un controllo che non si può fare non dice «morto».
fn boot_time_secs() -> Option<u64> {
    let output = std::process::Command::new("sysctl").args(["-n", "kern.boottime"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_boottime(&String::from_utf8_lossy(&output.stdout))
}

/// PURA: il primo intero dopo `sec = ` nella riga che dà `sysctl`, es.
/// `{ sec = 1787668740, usec = 36804 } Tue Aug 25 16:39:00 2026`.
fn parse_boottime(text: &str) -> Option<u64> {
    let after = text.split("sec = ").nth(1)?;
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// PURA: un record è morto per costruzione se è più vecchio dell'avvio della
/// macchina che lo legge — un pid di un mondo di processi che non c'è più può
/// coincidere per caso con uno vivo di adesso, e il record mentirebbe senza
/// che nessuno se ne accorga. Misurato il 25/08/2026: 2 record su 8 in
/// `sessioni-vive/` nominavano lo stesso pid di un avvio di ieri.
fn is_from_a_previous_boot(updated_at: u64, boot_time: u64) -> bool {
    updated_at < boot_time
}

/// PURA: il verdetto immediato quando si conoscono i due orari, separata da
/// chi li legge — così si prova senza `sysctl`, negato in questo stesso
/// perimetro come `ps`. `None` quando manca uno dei due: non è un'osservazione
/// a metà, è «non si sa», e chi chiama continua col giudizio sul processo.
fn boot_verdict(updated_at: Option<u64>, boot_time: Option<u64>) -> Option<SessionLiveness> {
    let (updated_at, boot_time) = (updated_at?, boot_time?);
    is_from_a_previous_boot(updated_at, boot_time).then_some(SessionLiveness::Gone)
}

/// Cosa si sa della sessione, leggendo il suo record sul disco.
///
/// Prima si chiede se il record parla di un avvio della macchina che non c'è
/// più — vedi `boot_verdict` — perché un pid di allora può coincidere per
/// caso con uno vivo adesso. Solo se questo tace si guarda il processo: i tre
/// modi di non avere il pid (record assente, JSON illeggibile, campo
/// mancante) finiscono in `None`, e `liveness_from` li legge già come «non si
/// sa niente».
pub(crate) fn liveness_of(sess: &str) -> SessionLiveness {
    if let Some(verdict) = boot_verdict(recorded_updated_at(sess), boot_time_secs()) {
        return verdict;
    }
    liveness_of_pid(recorded_pid(sess))
}

/// La stessa domanda a chi il pid ce l'ha già in mano, per non rileggere il
/// record una seconda volta solo per riottenere il numero che si aveva.
pub(crate) fn liveness_of_pid(pid: Option<u32>) -> SessionLiveness {
    liveness_from(pid, pid.map_or(ProcessLookup::Unavailable, look_up_session_process))
}

/// Il segnale afferma una rigenerazione che non è avvenuta?
///
/// LA DOMANDA NON È «QUELLA SESSIONE È VIVA», ED È QUI CHE IL DISEGNO OVVIO
/// SBAGLIA. Una sessione rigenerata resta viva: `/clear` svuota la memoria e
/// **lascia in piedi lo stesso processo**. Misurato il 25/08/2026 sui record: la
/// scheda `664b6cae` ne portava tre — `236e7b0c`, `7bee1a8f`, `c0a60027` — tutti
/// e tre con `session_pid` 18116, cioè tre generazioni della stessa postazione
/// azzerata due volte. Chiedere «è viva?» le dà tutte e tre per vive, e chi si
/// fermasse lì non riprenderebbe mai più dopo una staffetta legittima.
///
/// LA DOMANDA GIUSTA È «È UN ALTRO PROCESSO». Se la sessione nominata gira su un
/// pid **diverso dal proprio**, allora non è stata azzerata sul posto: è
/// un'altra sessione, viva, che nessuno ha sostituito — e il suo punto di
/// ripresa non è nostro. Se il pid coincide, siamo noi stessi un attimo fa, ed è
/// esattamente il caso che il segnale serve a servire.
///
/// Tutto il resto è dubbio, e il dubbio lascia passare: senza i due pid, o senza
/// una risposta piena sul processo, il mandato si consegna com'è sempre stato —
/// negarlo per un forse lascerebbe la sessione senza incarico.
pub(crate) fn signal_is_a_lie(
    named: SessionLiveness,
    named_pid: Option<u32>,
    own_pid: Option<u32>,
) -> bool {
    match (named, named_pid, own_pid) {
        (SessionLiveness::Alive, Some(theirs), Some(mine)) => theirs != mine,
        _ => false,
    }
}

/// Da quanti secondi non si tocca questo file. `None` se non esiste o se
/// l'orologio non risponde: un'età che non si sa non è un'età zero.
pub(crate) fn file_age_secs(path: &std::path::Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    SystemTime::now().duration_since(modified).ok().map(|d| d.as_secs())
}

/// `SessionEnd`: la sessione cancella il proprio record — ma solo se è davvero
/// finita.
///
/// Chi ha scritto un marcatore sa quando scade, e lo butta lui. Un raccoglitore
/// che passa dopo dovrebbe indovinare un'età massima, cioè scegliere fra
/// cancellare troppo presto e non cancellare mai.
///
/// PERCHÉ NON SI CANCELLA PIÙ A OCCHI CHIUSI. `SessionEnd` arriva anche mentre
/// la sessione continua a lavorare: misurato il 21/08/2026 su due sessioni, la
/// trascrizione dell'una proseguiva per altri dodici minuti dopo il proprio
/// congedo. Il registro delle sessioni vive ne portava il segno — cinque
/// processi `claude` vivi contro quattro record — e chi conta di lì per sapere
/// se una figura è di guardia concludeva che il posto era vuoto, aprendo un
/// secondo pannello sopra chi stava lavorando.
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
    forget_with(&sess, &full, liveness_of(&sess));
}

/// Il congedo di una sessione che **si è appena osservata morta**, per chi quel
/// fatto ce l'ha già in mano e non deve dedurlo da un record.
///
/// ESISTE PERCHÉ IL FATTO SI PERDE COL FILE. La staffetta scopre il processo
/// sparito guardando `ps`, e la cura è togliere di mezzo il record; ma se lo
/// cancellasse e basta, il `SessionEnd` che arrivasse dopo — sovrapposizione
/// plausibile, il giro della staffetta passa ogni minuto — non troverebbe più
/// niente da leggere e concluderebbe `Unknown` invece di `Gone`. Sono due
/// pulizie diverse: `Gone` butta i marcatori subito, `Unknown` li lascia lì un
/// giorno intero (`UNKNOWN_GRACE_SECS`) ad aspettare un raccoglitore che oggi
/// nessuno esegue. Chiamando qui, chi ha visto la morte la spende tutta in una
/// volta: i marcatori vanno via col record, e non resta niente da dedurre.
pub(crate) fn forget_dead_session(sess: &str, full_id: &str) {
    forget_with(sess, full_id, SessionLiveness::Gone);
}

/// La parte che decide cosa sparisce, separata da chi guarda i processi: così i
/// tre esiti si provano tutti e tre, compreso quello che sul banco di prova non
/// si può costruire — una sessione viva si chiama `claude`, e la batteria no.
fn forget_with(sess: &str, full: &str, liveness: SessionLiveness) {
    if liveness == SessionLiveness::Alive {
        // Il congedo di una sessione che sta ancora lavorando è un fatto, non
        // rumore: si registra, perché è la traccia con cui si è capito il difetto.
        journal::record(
            "register-session",
            "salta",
            "congedo-a-sessione-viva",
            &[("session", Field::Text(sess.to_string()))],
        );
        return;
    }
    for path in own_markers(sess, full) {
        // Un file senza età leggibile non esiste: `remove_file` fallirebbe
        // comunque, e saltarlo evita di buttare ciò che non si è potuto guardare.
        if let Some(age) = file_age_secs(&path) {
            if should_remove(liveness, age) {
                let _ = fs::remove_file(path);
            }
        }
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
    // IL SEGNALE È JSON, e il formato vecchio era il solo percorso su una riga.
    //
    // Si distinguono da soli: un percorso non è JSON valido. Il formato a righe
    // etichettate, provato per un'ora il 19/08/2026, non ha mai raggiunto la
    // produzione — spezzava i mandati su più righe e chi rileggeva riattaccava
    // le righe senza separatore.
    let (path, punto, mandato, incarico) = match serde_json::from_str::<serde_json::Value>(body.trim()) {
        Ok(d) => {
            let campo = |k: &str| {
                d.get(k).and_then(|v| v.as_str()).unwrap_or_default().trim().to_string()
            };
            // IL SEGNALE È DI CHI È INTESTATO. La chiave del file è l'albero, e
            // su un albero possono vivere due sessioni: senza questo controllo
            // la seconda che riparte si prende il punto di ripresa e il `/loop`
            // della prima. Si confronta la tab, che `/clear` non cambia e che
            // non invecchia come l'handle.
            //
            // Se il segnale non dice a chi va, o se l'ambiente non dice chi
            // siamo, si consuma comunque: è la forma che aveva prima, e negarsi
            // il mandato per un dubbio lascerebbe la sessione senza incarico.
            let destinatario = campo("tab");
            let io = env("ORCA_TAB_ID");
            if !destinatario.is_empty() && !io.is_empty() && destinatario != io {
                return String::new(); // non è per noi: resta a chi aspetta
            }
            // E POI SI VERIFICA CIÒ CHE IL MESSAGGIO STA PER AFFERMARE. La frase
            // che esce di qui dice «la sessione precedente ha consegnato ed è
            // stata rigenerata». Fino al 25/08/2026 non c'era modo di
            // controllarla: il segnale non nominava nessuno, e quella riga
            // veniva creduta sulla parola da chiunque la trovasse.
            let named = campo("sessione");
            if !named.is_empty() {
                let short_id: String = named.chars().take(8).collect();
                let named_pid = recorded_pid(&short_id);
                if signal_is_a_lie(liveness_of_pid(named_pid), named_pid, session_pid()) {
                    journal::record(
                        "register-session",
                        "salta",
                        "segnale-su-sessione-viva",
                        &[("session", Field::Text(short_id))],
                    );
                    // NON SI CONSUMA. Il file scade da sé dopo `FRESH_SEC`, e
                    // finché è lì il destinatario giusto può ancora prenderlo:
                    // cancellarlo qui vorrebbe dire che la sessione sbagliata,
                    // oltre a non ricevere il mandato, lo toglie anche a chi
                    // aspetta.
                    return String::new();
                }
            }
            (campo("handoff"), campo("punto"), campo("mandato"), campo("incarico"))
        }
        Err(_) => (body.trim().to_string(), String::new(), String::new(), String::new()),
    };
    let _ = fs::remove_file(&signal); // consumato: una staffetta, una ripresa
    if path.is_empty() && incarico.is_empty() {
        return String::new();
    }
    // IL MANDATO PRIMA DELLA CONSEGNA — Theo, 28/08/2026: «`/clear` →
    // autoripresa del mandato». Chi riparte deve riprendere **l'incarico**, che
    // resta vero mentre i turni passano; la consegna racconta com'è finito il
    // turno di un altro, ed è il ripiego per quando l'incarico non si sa.
    // L'ordine conta: la prima riga di un testimone è quella che viene eseguita.
    let base = if !incarico.is_empty() {
        format!(
            "RIPARTENZA AUTOMATICA (staffetta). La sessione precedente su questo \
worktree è stata azzerata per non trascinare un contesto gonfio, e **il lavoro \
non è finito**: è il suo mandato che continua, non un compito nuovo. \
Riprendilo: leggi `{incarico}` per intero e prosegui da dove quel mandato è \
arrivato. Non ricominciare da zero, non rifare ciò che risulta già fatto, e \
**non annunciare la ripartenza**: il primo messaggio del turno è già il passo \
successivo del mandato."
        )
    } else {
        format!(
            "RIPARTENZA AUTOMATICA (staffetta). La sessione precedente su questo \
worktree ha consegnato ed è stata rigenerata per non trascinare un contesto \
gonfio. Riprendi da quell'handoff: leggi `{path}` e prosegui il piano già \
autorizzato. Non ricominciare da zero, e **non annunciare la ripartenza**: il \
primo messaggio del turno è già il passo successivo del piano."
        )
    };
    if punto.is_empty() && mandato.is_empty() && (incarico.is_empty() || path.is_empty()) {
        return base;
    }
    let mut coda = String::new();
    // LA CONSEGNA SCENDE A CONTESTO quando il mandato c'è. Non si butta: dice
    // cosa è già stato fatto, ed è il modo per non rifarlo. Ma è materiale da
    // consultare, non l'ordine del giorno — che è il mandato, sopra.
    if !incarico.is_empty() && !path.is_empty() {
        coda.push_str(&format!(
            "\n\nCosa è già stato fatto su quel mandato, se ti serve saperlo \
prima di rifarlo: `{path}`. È un rendiconto, non l'incarico."
        ));
    }
    if !punto.is_empty() {
        // IL PUNTO PRIMA DEL MANDATO: è la riga che dice cosa fare adesso,
        // mentre il mandato dice a che lavoro appartiene.
        coda.push_str(&format!(
            "\n\nRIPRENDI DA QUI, e' il punto che la sessione uscente ha \
dichiarato chiudendo il suo ultimo turno:\n{punto}"
        ));
    }
    // IL MANDATO NON E' CONTESTO, E' L'INCARICO. Chi arriva qui sta sostituendo
    // una sessione che girava in `/loop`: senza queste righe legge la consegna,
    // dichiara da dove riparte e si ferma — e il loop muore alla prima
    // rigenerazione.
    if !mandato.is_empty() {
        coda.push_str(&format!(
            "\n\nLa sessione uscente stava girando su questo mandato, e tocca \
ora a te: riprendilo cosi' com'e'.\n\n{mandato}"
        ));
    }
    format!("{base}{coda}")
}

/// Siamo a fine sessione? Lo dice l'evento, non un argomento.
///
/// `--fine` restava un argomento da ricopiare in `settings.json`, e nel ramo che
/// esegue davvero non era stato ricopiato: la riga di `SessionEnd` passa
/// l'opzione al ripiego Python e **non** al binario, che esiste sempre. Esito
/// misurato il 18/08/2026: `forget_session` non era mai girata, e sul disco
/// c'erano **176 marcatori** dal 14/08 — 84 `consegna-misura-*`, 72
/// `consegna-ripartenze-*`, 16 `consegna-fatta-*`, 4 `consegna-volontaria-*`.
///
/// Il difetto non e' l'opzione dimenticata, e' che il gancio si affidava a
/// qualcuno che la ricopiasse. L'evento invece arriva sempre e da solo: chi
/// scrive la riga di configurazione non puo' piu' sbagliarla. `--fine` resta
/// riconosciuto per non rompere chi lo passa ancora.
fn is_session_end(data: &serde_json::Value, args: &[String]) -> bool {
    if args.iter().any(|a| a == "--fine") {
        return true;
    }
    data.get("hook_event_name")
        .and_then(|v| v.as_str())
        .is_some_and(|e| e == "SessionEnd")
}

/// Quale fonte apre la sessione, in ordine di priorità: la staffetta
/// (rigenerazione), poi il filo scoperto, poi la voce di coda. PURA: le tre
/// fonti arrivano come chiusure non ancora eseguite — non tre stringhe già
/// pronte — così la fonte che vince è la SOLA interrogata.
///
/// CONTA PER LA TERZA, CHE NON È INNOCUA: quando risponde,
/// `queue_mandate::opening_notice` dichiara la voce di coda presa scrivendo
/// un marcatore. Interrogarla comunque dopo che una delle prime due ha già
/// parlato la sottrarrebbe a chi la può ancora lavorare, per un mandato che
/// nessuna sessione leggerebbe mai.
///
/// `session_id` VUOTO TIENE MUTA LA CODA SENZA CHIEDERLE NIENTE:
/// `queue_mandate::try_declare_taken` rifiuta già un id vuoto (non rivendica
/// mai la voce), quindi il risultato non cambia — mancava solo il fatto che
/// nessuno lo provasse, ed evita una lettura del disco inutile.
fn opening_message(
    relay: impl FnOnce() -> String,
    uncovered: impl FnOnce() -> String,
    queued: impl FnOnce(&str) -> String,
    session_id: &str,
) -> String {
    let msg = relay();
    if !msg.is_empty() {
        return msg;
    }
    let msg = uncovered();
    if !msg.is_empty() {
        return msg;
    }
    if session_id.trim().is_empty() {
        return String::new();
    }
    queued(session_id)
}

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return 0; // stdin non è JSON: si esce muti, come l'originale
    };
    // Prima di qualunque riga di registro, altrimenti esce marcata come prova e
    // chi conta quanto un gancio morde la scarta.
    hook_io::mark_live_from_payload(&data);
    let args: Vec<String> = std::env::args().collect();
    if is_session_end(&data, &args) {
        forget_session(&data);
        return 0;
    }
    record_session(&data);
    // QUI C'È DI NUOVO QUALCUNO. Se in questo albero un successore non era mai
    // partito, il filo risultava scoperto: chiunque sia arrivato adesso — la
    // staffetta, un'automazione, Theo che apre un pannello a mano — lo tiene
    // lui. È l'unico evento che lo dice, ed è ciò che impedisce all'elenco dei
    // fili scoperti di crescere e basta: un elenco che non cala nessuno lo
    // legge dopo il terzo giorno.
    crate::uncovered_thread::clear_tree(
        data.get("cwd")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| {
                std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default()
            })
            .as_str(),
    );
    // LA COMPOSIZIONE DELLE TRE FONTI STA IN `opening_message`, PURA E PROVATA
    // LÌ CON LE CHIUSURE: qui si passa ciascuna fonte non ancora eseguita, e
    // si stampa quella che ha vinto — vedi il commento della funzione per il
    // perché conta chi viene interrogato e chi no.
    let session_id = data.get("session_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let msg = opening_message(
        resume_message,
        crate::uncovered_thread::opening_notice,
        crate::queue_mandate::opening_notice,
        &session_id,
    );
    if !msg.is_empty() {
        // E QUI CI SI FERMA. Chi riceve un mandato — di rigenerazione, di
        // filo scoperto o di coda — non è libero di narrarci sopra: il
        // messaggio dice già cosa fare. Aggiungerci un elenco di lavori
        // altrui è esattamente ciò che quel canale esclude.
        println!("{msg}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    fn event(nome: &str, sess: &str) -> serde_json::Value {
        serde_json::json!({ "hook_event_name": nome, "session_id": sess })
    }

    /// I nomi di marcatore che i sorgenti costruiscono davvero, letti a tempo di
    /// compilazione. La forma è sempre `format!("<famiglia>-{...`.
    fn families_written_in(source: &str) -> Vec<String> {
        let mut found = Vec::new();
        for piece in source.split("format!(\"consegna-").skip(1) {
            let Some(name) = piece.split('"').next() else {
                continue;
            };
            // La coda variabile comincia alla graffa: `consegna-stop-{session}`
            // e `consegna-fatta-{}` danno la stessa famiglia.
            let Some((family, _)) = name.split_once("-{") else {
                continue;
            };
            found.push(format!("consegna-{family}"));
        }
        found
    }

    /// L'ELENCO CHIUSO NON SI FIDA PIÙ DI CHI LO RICOPIA A MANO.
    ///
    /// Tre volte la stessa perdita: `consegna-volontaria` aggiunta il 18/08 dopo
    /// che 176 marcatori erano rimasti sul disco, `consegna-stop-riferimento` il
    /// 21/08 dopo che nessuno l'aveva mai buttata. Chi scrive un marcatore nuovo
    /// non ha nessun motivo di venire a leggere questa costante, e finora niente
    /// glielo diceva. Ora il confronto lo fa la batteria, sui sorgenti veri.
    #[test]
    fn every_family_a_source_writes_is_one_the_farewell_sweeps() {
        let sources = [
            include_str!("handoff_on_stop.rs"),
            include_str!("handoff_required.rs"),
            include_str!("handoff.rs"),
            include_str!("relay.rs"),
            include_str!("register_session.rs"),
        ];
        let mut seen = Vec::new();
        for source in sources {
            for family in families_written_in(source) {
                assert!(
                    MARKER_FAMILIES.contains(&family.as_str()),
                    "«{family}» viene scritta e non sta in MARKER_FAMILIES: \
nessuno la butterà mai"
                );
                if !seen.contains(&family) {
                    seen.push(family);
                }
            }
        }
        // E il caso deve poter fallire: se la ricerca non trovasse niente,
        // l'asserzione qui sopra sarebbe verde a elenco vuoto.
        assert!(
            seen.len() >= 8,
            "le famiglie trovate nei sorgenti sono troppo poche: {seen:?}"
        );
    }

    #[test]
    fn the_end_of_a_session_is_read_off_the_event() {
        assert!(is_session_end(&event("SessionEnd", "abc"), &[]));
    }

    #[test]
    fn any_other_event_is_not_the_end() {
        assert!(!is_session_end(&event("SessionStart", "abc"), &[]));
        assert!(!is_session_end(&event("", "abc"), &[]));
        assert!(!is_session_end(&serde_json::json!({ "session_id": "abc" }), &[]));
    }

    #[test]
    fn the_old_flag_still_works_for_whoever_still_passes_it() {
        // La compatibilita' e' una promessa: senza questo caso, toglierla non
        // farebbe cadere niente.
        let args = vec!["claude-hooks".to_string(), "--fine".to_string()];
        assert!(is_session_end(&event("SessionStart", "abc"), &args));
        assert!(is_session_end(&serde_json::json!({}), &args));
    }

    /// La staffetta vince su tutto: le altre due chiusure non vengono nemmeno
    /// chiamate. IL MUTANTE CHE CONTA è invertire l'ordine delle fonti dentro
    /// `opening_message`: con l'ordine scambiato questo caso torna rosso
    /// sull'uguaglianza del messaggio, ancora prima di guardare i flag.
    #[test]
    fn the_relay_wins_and_the_other_two_are_never_asked() {
        let uncovered_called = std::rc::Rc::new(std::cell::Cell::new(false));
        let queued_called = std::rc::Rc::new(std::cell::Cell::new(false));
        let (u, q) = (uncovered_called.clone(), queued_called.clone());
        let msg = opening_message(
            || "mandato della staffetta".to_string(),
            move || {
                u.set(true);
                String::new()
            },
            move |_| {
                q.set(true);
                String::new()
            },
            "sess",
        );
        assert_eq!(msg, "mandato della staffetta");
        assert!(!uncovered_called.get(), "il filo scoperto non doveva essere interrogato");
        assert!(!queued_called.get(), "la coda non doveva essere interrogata");
    }

    /// Il filo scoperto vince sulla coda quando la staffetta tace.
    #[test]
    fn the_uncovered_thread_wins_over_the_queue_when_the_relay_is_silent() {
        let queued_called = std::rc::Rc::new(std::cell::Cell::new(false));
        let q = queued_called.clone();
        let msg = opening_message(
            || String::new(),
            || "filo scoperto".to_string(),
            move |_| {
                q.set(true);
                String::new()
            },
            "sess",
        );
        assert_eq!(msg, "filo scoperto");
        assert!(!queued_called.get(), "la coda non doveva essere interrogata");
    }

    /// La terza fonte parla solo se le prime due tacciono entrambe.
    #[test]
    fn the_queue_speaks_only_when_the_first_two_are_silent() {
        let msg = opening_message(
            || String::new(),
            || String::new(),
            |sess| format!("voce di coda per {sess}"),
            "sess-42",
        );
        assert_eq!(msg, "voce di coda per sess-42");
    }

    /// Il caso del `session_id` vuoto (`unwrap_or("")` in `run()`): la coda
    /// resta muta, e non viene nemmeno interrogata.
    #[test]
    fn an_empty_session_id_keeps_the_queue_unasked() {
        let queued_called = std::rc::Rc::new(std::cell::Cell::new(false));
        let q = queued_called.clone();
        let msg = opening_message(
            || String::new(),
            || String::new(),
            move |_| {
                q.set(true);
                "non dovrebbe arrivare qui".to_string()
            },
            "",
        );
        assert_eq!(msg, "");
        assert!(!queued_called.get(), "session_id vuoto: la coda non va interrogata");
    }

    #[test]
    fn a_session_end_sweeps_its_own_markers_and_leaves_the_others() {
        // Il congedo spazza solo se sa che quella sessione e' finita: dal
        // 25/08/2026 lo sa anche senza `ps`, ripiegando su `kill(pid, 0)`.
        let casa = HomeIsolata::nuova("fine-sessione");
        let state = casa.dir.join(".claude/state");
        std::fs::create_dir_all(state.join("sessioni-vive")).unwrap();
        let miei = ["consegna-misura-11112222", "consegna-fatta-11112222"];
        let altrui = ["consegna-misura-99998888", "consegna-fatta-99998888"];
        for n in miei.iter().chain(altrui.iter()) {
            std::fs::write(state.join(n), "x").unwrap();
        }
        // Un pid che su macOS non puo' esistere (il massimo e' 99998): la
        // sessione risulta finita davvero, che e' il caso normale di un congedo.
        std::fs::write(
            state.join("sessioni-vive/11112222.json"),
            r#"{"session_pid": 999999}"#,
        )
        .unwrap();

        forget_session(&event("SessionEnd", "11112222-3333-4444-5555-666677778888"));

        for n in miei {
            assert!(!state.join(n).exists(), "{n} doveva sparire");
        }
        for n in altrui {
            assert!(state.join(n).exists(), "{n} non e' suo e doveva restare");
        }
        assert!(!state.join("sessioni-vive/11112222.json").exists());
    }

    #[test]
    fn a_farewell_from_a_session_that_is_still_working_takes_nothing_away() {
        // Il difetto vero, misurato il 21/08/2026: `SessionEnd` arriva mentre la
        // sessione lavora, e chi conta chi e' di guardia trova il posto vuoto.
        let home = HomeIsolata::nuova("fine-sessione-viva");
        let state = home.dir.join(".claude/state");
        std::fs::create_dir_all(state.join("sessioni-vive")).unwrap();
        let hers = ["consegna-misura-11112222", "consegna-fatta-11112222"];
        for n in hers {
            std::fs::write(state.join(n), "x").unwrap();
        }
        std::fs::write(state.join("sessioni-vive/11112222.json"), "{}").unwrap();

        forget_with(
            "11112222",
            "11112222-3333-4444-5555-666677778888",
            SessionLiveness::Alive,
        );

        for n in hers {
            assert!(state.join(n).exists(), "{n}: la sessione lavora ancora");
        }
        assert!(
            state.join("sessioni-vive/11112222.json").exists(),
            "il registro di una sessione viva non si cancella"
        );
        // E il fatto va lasciato scritto, altrimenti il difetto torna
        // invisibile: e' proprio dalla traccia che lo si e' capito. Senza questa
        // riga il caso non distingue il ramo, perche' la difesa e' doppia —
        // provato togliendo il ramo: la batteria restava tutta verde.
        let lines = journal_lines(&home);
        assert!(
            lines.contains("congedo-a-sessione-viva") && lines.contains("11112222"),
            "il congedo a una sessione viva deve lasciare traccia: {lines}"
        );
    }

    #[test]
    fn what_is_not_known_waits_instead_of_being_guessed() {
        // Un record scritto prima delle 11:30 del 21/08/2026 non porta il
        // processo: la domanda non ha risposta, e i suoi marcatori freschi
        // restano finche' non sono abbastanza vecchi da non servire piu'.
        let home = HomeIsolata::nuova("fine-sessione-ignota");
        let state = home.dir.join(".claude/state");
        std::fs::create_dir_all(state.join("sessioni-vive")).unwrap();
        std::fs::write(state.join("consegna-fatta-11112222"), "x").unwrap();
        std::fs::write(state.join("sessioni-vive/11112222.json"), "{}").unwrap();

        forget_session(&event("SessionEnd", "11112222-3333-4444-5555-666677778888"));

        assert!(
            state.join("consegna-fatta-11112222").exists(),
            "senza il processo non si sa niente, e non si butta niente di fresco"
        );
    }

    #[test]
    fn the_three_answers_are_three_and_a_missing_field_is_not_a_death() {
        assert_eq!(
            liveness_from(None, ProcessLookup::Unavailable),
            SessionLiveness::Unknown
        );
        assert_eq!(liveness_from(None, ProcessLookup::IsClaude), SessionLiveness::Unknown);
        assert_eq!(liveness_from(Some(42), ProcessLookup::IsClaude), SessionLiveness::Alive);
        assert_eq!(liveness_from(Some(42), ProcessLookup::NotFound), SessionLiveness::Gone);
    }

    #[test]
    fn a_regenerated_session_is_alive_on_our_own_pid_and_that_is_not_a_lie() {
        // IL CASO NORMALE DELLA STAFFETTA, e quello che un controllo ingenuo
        // romperebbe: `/clear` azzera la memoria e lascia in piedi lo stesso
        // processo, quindi la sessione uscente risulta viva e col nostro
        // stesso pid. Se questo passasse per bugia, dopo ogni rigenerazione
        // legittima nessuno riprenderebbe piu' niente.
        assert!(!signal_is_a_lie(SessionLiveness::Alive, Some(18116), Some(18116)));
    }

    #[test]
    fn a_signal_naming_another_live_process_is_a_lie() {
        // Il caso vissuto il 25/08/2026: il messaggio dichiarava rigenerata una
        // sessione che girava per conto suo su un altro processo, e chi lo ha
        // creduto le e' finito addosso sullo stesso lavoro.
        assert!(signal_is_a_lie(SessionLiveness::Alive, Some(18116), Some(85535)));
    }

    #[test]
    fn without_both_pids_the_mandate_still_goes_through() {
        // NEL DUBBIO SI CONSEGNA. I record scritti prima delle 11:30 del
        // 21/08/2026 non hanno il campo del processo, e `ps` puo' non partire
        // affatto: nessuna delle due e' una prova di bugia, e negare il mandato
        // per un forse lascerebbe la sessione senza incarico.
        assert!(!signal_is_a_lie(SessionLiveness::Alive, None, Some(85535)));
        assert!(!signal_is_a_lie(SessionLiveness::Alive, Some(18116), None));
        assert!(!signal_is_a_lie(SessionLiveness::Unknown, Some(18116), Some(85535)));
    }

    #[test]
    fn a_signal_naming_a_dead_session_is_the_honest_case() {
        // Una sessione davvero sparita e' esattamente cio' che il segnale
        // afferma: qui non c'e' niente da smentire, il mandato passa.
        assert!(!signal_is_a_lie(SessionLiveness::Gone, Some(18116), Some(85535)));
    }

    #[test]
    fn an_unrecognised_program_is_a_dont_know_not_a_death() {
        // Un nome che non riconosco non e' un'osservazione di morte: e' un pid
        // riciclato OPPURE una riga letta storta, e dal di dentro non si
        // distinguono. Valeva Gone, e il difetto della lettura di `ps` e'
        // passato di qui a cancellare i marcatori di due sessioni vive.
        assert_eq!(
            liveness_from(Some(42), ProcessLookup::OtherProgram),
            SessionLiveness::Unknown,
            "un nome che non riconosco non e' una morte osservata"
        );
        // La differenza che costa: il giorno di grazia c'e' per l'uno e non
        // per l'altro. Un errore di lettura ora costa polvere, non un registro.
        assert!(!should_remove(
            liveness_from(Some(42), ProcessLookup::OtherProgram),
            60
        ));
        assert!(should_remove(
            liveness_from(Some(42), ProcessLookup::NotFound),
            60
        ));
    }

    #[test]
    fn a_question_that_could_not_be_asked_is_not_an_answer() {
        // IL DIFETTO TROVATO DAL VERDETTO INDIPENDENTE. `ps` che risponde «quel
        // pid non c'e'» e `ps` che non parte affatto erano la stessa cosa, e
        // quella cosa era «morta»: bastava un `PATH` svuotato per cancellare il
        // registro di una sessione viva, senza nemmeno il giorno di grazia.
        assert_eq!(
            liveness_from(Some(42), ProcessLookup::Unavailable),
            SessionLiveness::Unknown,
            "non aver potuto chiedere non e' una morte"
        );
        // E il giorno di grazia protegge cio' che e' fresco, invece di buttarlo.
        assert!(!should_remove(SessionLiveness::Unknown, 60));
    }

    #[test]
    fn only_what_is_known_dead_or_old_enough_goes() {
        let one_day = UNKNOWN_GRACE_SECS;
        assert!(!should_remove(SessionLiveness::Alive, one_day * 30));
        assert!(should_remove(SessionLiveness::Gone, 0));
        assert!(!should_remove(SessionLiveness::Unknown, one_day - 1));
        assert!(should_remove(SessionLiveness::Unknown, one_day));
    }

    #[test]
    fn a_pid_that_cannot_exist_is_not_a_live_session() {
        if let Some(why) = hook_io::testing::ps_is_denied() {
            eprintln!("{why}");
            return;
        }
        // Il verso che conta: se `ps` gira e dice che quel pid non c'e', quella
        // e' una risposta — altrimenti nessun record verrebbe mai buttato.
        assert_eq!(look_up_session_process(999_999), ProcessLookup::NotFound);
        // E il processo di questa batteria esiste ma non si chiama `claude`:
        // un pid vivo non basta, il nome discrimina.
        assert_eq!(
            look_up_session_process(std::process::id()),
            ProcessLookup::OtherProgram
        );
    }

    #[test]
    fn kill_zero_reads_the_errno_not_the_exit_code() {
        // LA MISURA DEL 25/08/2026, RIPRODOTTA. `EPERM` (1) ed `ESRCH` (3) sono
        // i due soli motivi che si sanno leggere; qualunque altro resta
        // onestamente ignoto — e cosi' anche l'assenza di errno con `ret != 0`,
        // che sul sistema vero non capita mai ma qui non si suppone.
        assert_eq!(existence_from_kill_result(0, None), ProcessExistence::Exists);
        assert_eq!(existence_from_kill_result(-1, Some(1)), ProcessExistence::Exists);
        assert_eq!(existence_from_kill_result(-1, Some(3)), ProcessExistence::Gone);
        assert_eq!(existence_from_kill_result(-1, Some(13)), ProcessExistence::Unknown);
        assert_eq!(existence_from_kill_result(-1, None), ProcessExistence::Unknown);
    }

    #[test]
    fn kill_distinguishes_a_process_that_exists_from_one_that_does_not() {
        // IL RAMO CHE `ps` NON PUO' DARE QUI DENTRO. Questa batteria gira dove
        // `ps` e' negato — vedi `ps_is_denied()` — ma `kill(pid, 0)` e' una
        // system call, non un comando esterno: non dipende da quel perimetro,
        // e prova la distinzione dal vivo invece che sulla parola dell'esito.
        assert_eq!(process_exists(1), ProcessExistence::Exists, "pid 1 esiste sempre");
        assert_eq!(
            process_exists(999_999),
            ProcessExistence::Gone,
            "il massimo pid su macOS e' 99998"
        );
        assert_eq!(
            process_exists(std::process::id()),
            ProcessExistence::Exists,
            "il processo di questa batteria esiste per se stesso"
        );
    }

    #[test]
    fn without_ps_a_dead_pid_is_known_dead_and_a_live_one_is_not_claimed_alive() {
        // IL RAMO CHE PRIMA RESTAVA CIECO IN SILENZIO. Quando `ps` non parte,
        // la vecchia strada rispondeva sempre `Unavailable` — qualunque pid,
        // vivo o morto, dava lo stesso "non lo so". Qui, nello stesso
        // perimetro dove `ps_is_denied()` fa autoescludere le altre batterie,
        // si prova che ora `look_up_session_process` sa dire "morto" da solo.
        let Some(_) = hook_io::testing::ps_is_denied() else {
            eprintln!(
                "PROVA NON ESEGUITA: qui `ps` parte, il ramo di ripiego non si puo' forzare"
            );
            return;
        };
        assert_eq!(look_up_session_process(999_999), ProcessLookup::NotFound);
        // Vivo ma senza nome letto: NON diventa `IsClaude`. Sapere che il pid
        // c'e' non basta a dichiararlo la sessione — sarebbe l'errore opposto
        // a quello del 21/08/2026, un pid riciclato creduto per buono.
        assert_eq!(look_up_session_process(1), ProcessLookup::ExistsNameUnknown);
    }

    #[test]
    fn a_process_that_exists_with_an_unread_name_is_a_dont_know() {
        assert_eq!(
            liveness_from(Some(42), ProcessLookup::ExistsNameUnknown),
            SessionLiveness::Unknown,
            "esiste ma non si sa chi e': non e' una prova di vita"
        );
    }

    #[test]
    fn a_record_older_than_the_boot_is_dead_by_construction() {
        // LA MISURA DEL 25/08/2026: 2 record su 8 in `sessioni-vive/`
        // nominavano lo stesso pid di un avvio precedente. Il pid da solo non
        // lo direbbe mai — puo' essere stato riassegnato per caso a un
        // processo vivo adesso — ma l'orologio non mente.
        assert_eq!(boot_verdict(Some(100), Some(200)), Some(SessionLiveness::Gone));
        assert!(is_from_a_previous_boot(100, 200));
    }

    #[test]
    fn a_record_updated_after_the_boot_is_not_judged_here() {
        assert_eq!(boot_verdict(Some(300), Some(200)), None);
        assert!(!is_from_a_previous_boot(300, 200));
        // Lo stesso istante non e' "prima": un record scritto esattamente
        // all'avvio non e' un residuo del giro precedente.
        assert!(!is_from_a_previous_boot(200, 200));
    }

    #[test]
    fn without_one_of_the_two_clocks_the_boot_check_stays_silent() {
        // Non sapere non e' sapere che e' morto: un `sysctl` negato o un
        // record senza `updated_at` lasciano decidere al giudizio sul
        // processo, non lo anticipano con un "morto" indimostrato.
        assert_eq!(boot_verdict(None, Some(200)), None);
        assert_eq!(boot_verdict(Some(100), None), None);
        assert_eq!(boot_verdict(None, None), None);
    }

    #[test]
    fn boottime_reads_the_first_integer_after_sec() {
        // La riga vera data da `sysctl -n kern.boottime` su questa macchina,
        // il 25/08/2026: si legge il primo intero, non l'ora leggibile in
        // coda ne' i microsecondi.
        assert_eq!(
            parse_boottime("{ sec = 1787668740, usec = 36804 } Tue Aug 25 16:39:00 2026\n"),
            Some(1_787_668_740)
        );
        assert_eq!(parse_boottime("qualcosa senza il campo"), None);
    }

    /// La riga di `ps -o pid=,ppid=,comm=` e il nome del comando visto da solo,
    /// per lo stesso pid: `ps` fa da giudice a se stesso, e il nome chiesto da
    /// solo non ha colonne accanto in cui inciampare.
    fn ps_line_and_comm(pid: u32) -> (String, String) {
        let three = std::process::Command::new("ps")
            .args(["-o", "pid=,ppid=,comm=", "-p", &pid.to_string()])
            .output()
            .expect("ps must run");
        let one = std::process::Command::new("ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
            .expect("ps must run");
        assert!(three.status.success() && one.status.success(), "pid {pid} must exist");
        (
            String::from_utf8_lossy(&three.stdout).to_string(),
            String::from_utf8_lossy(&one.stdout).trim().to_string(),
        )
    }

    #[test]
    fn a_short_pid_and_a_long_one_read_the_same_command() {
        if let Some(why) = hook_io::testing::ps_is_denied() {
            eprintln!("{why}");
            return;
        }
        // IL CASO CHE MANCAVA, e senza il quale dieci prove verdi convivevano
        // col difetto: l'unica che interrogava `ps` chiedeva il pid della
        // batteria e si aspettava `OtherProgram`, cioe' la stessa risposta che
        // dava il difetto. Non poteva fallire.
        //
        // BRACCIO CORTO: il pid 1 c'e' su qualunque Unix ed e' largo una cifra.
        let (short_line, short_comm) = ps_line_and_comm(1);
        // Se questa cade, il differenziale qui sotto non prova piu' niente:
        // vuol dire che `ps` ha smesso di allineare a destra, e il caso da
        // coprire e' un altro.
        assert!(
            short_line.starts_with(char::is_whitespace),
            "ps deve allineare a destra il pid corto, altrimenti la prova e' vuota: {short_line:?}"
        );
        assert_eq!(
            ps_fields(&short_line).map(|(_, _, c)| c),
            Some(short_comm.as_str()),
            "con un pid corto i campi slittavano: il ppid finiva dentro il nome"
        );

        // BRACCIO LUNGO: il processo di questa batteria, stessa domanda.
        let me = std::process::id();
        let (long_line, long_comm) = ps_line_and_comm(me);
        assert_eq!(
            ps_fields(&long_line).map(|(_, _, c)| c),
            Some(long_comm.as_str()),
            "con un pid lungo la lettura era gia' giusta e deve restarlo"
        );
        assert_eq!(ps_fields(&long_line).map(|(p, _, _)| p), Some(me));
    }

    #[test]
    fn a_session_with_a_short_pid_is_as_alive_as_one_with_a_long_pid() {
        // Le due righe sono quelle vere del 21/08/2026, prese da `ps` su questa
        // macchina: due sessioni `claude`, una col pid a cinque cifre e una a
        // quattro. Stesso programma, stessa risposta — e non lo era: la seconda
        // dava `OtherProgram`, e al congedo la sessione viva si cancellava i
        // propri marcatori, cosi' che «ha consegnato» tornava falso.
        //
        // Un `claude` con pid corto non si fabbrica sul banco di prova: il pid
        // lo assegna il sistema. Per questo il caso vive sulle righe, e la
        // prova che le righe siano fedeli sta nel caso qui sopra, che `ps` lo
        // interroga davvero a tutte e due le larghezze.
        assert_eq!(classify_ps_line("22211 22069 claude\n"), ProcessLookup::IsClaude);
        assert_eq!(classify_ps_line(" 8027  7969 claude\n"), ProcessLookup::IsClaude);
        assert_eq!(classify_ps_line("    1     0 claude\n"), ProcessLookup::IsClaude);
        // E un nome col dentro uno spazio resta intero: spezzare a ogni spazio
        // lo taglierebbe a `/Applications/Orca` e cambierebbe il verdetto.
        assert_eq!(
            ps_fields(" 8027  7969 /Applications/Orca Helper\n").map(|(_, _, c)| c),
            Some("/Applications/Orca Helper")
        );
    }

    /// Un segnale di ripresa fresco, nella forma che scrive la staffetta.
    fn signal_json(casa: &HomeIsolata, handoff: &str, punto: &str, mandato: &str) {
        signal_for(casa, handoff, punto, mandato, "");
    }

    /// Come sopra, intestato a una tab.
    fn signal_for(
        casa: &HomeIsolata,
        handoff: &str,
        punto: &str,
        mandato: &str,
        tab: &str,
    ) {
        let corpo = serde_json::json!({
            "handoff": handoff, "punto": punto, "mandato": mandato, "tab": tab,
        })
        .to_string();
        signal(casa, &corpo);
    }

    /// Un segnale che porta anche il mandato, come dal 28/08/2026.
    fn signal_with_mandate(home: &HomeIsolata, handoff: &str, mandate: &str) {
        let corpo = serde_json::json!({
            "handoff": handoff, "punto": "", "mandato": "", "tab": "",
            "incarico": mandate,
        })
        .to_string();
        signal(home, &corpo);
    }

    #[test]
    fn the_baton_carries_the_mandate_not_the_report() {
        // Theo, 28/08/2026: «`/clear` → autoripresa del mandato». Chi riparte
        // deve ricevere l'incarico, non il rendiconto di chi c'era prima.
        let home = HomeIsolata::nuova("testimone-mandato");
        signal_with_mandate(
            &home,
            "/percorso/consegna.md",
            "/percorso/mandati/2026-08-28-il-lavoro.md",
        );
        let msg = resume_message();

        assert!(msg.contains("/percorso/mandati/2026-08-28-il-lavoro.md"), "{msg}");
        assert!(msg.contains("è il suo mandato che continua"), "{msg}");
        // La consegna non sparisce, ma scende: serve a non rifare ciò che è già
        // fatto, e il messaggio deve dire che è un rendiconto.
        assert!(msg.contains("È un rendiconto, non l'incarico."), "{msg}");
        // E il mandato viene prima: la prima riga di un testimone è quella che
        // viene eseguita.
        let mandate_at = msg.find("/percorso/mandati/").expect("il mandato è nel messaggio");
        let report_at = msg.find("/percorso/consegna.md").expect("la consegna è nel messaggio");
        assert!(mandate_at < report_at, "il mandato deve venire prima:\n{msg}");
    }

    #[test]
    fn without_a_mandate_the_baton_falls_back_to_the_report() {
        // Il ripiego, e la prova che il ramo vecchio non è stato spento: finché
        // una sessione non nomina un mandato, il testimone resta la consegna.
        let home = HomeIsolata::nuova("testimone-senza-mandato");
        signal_json(&home, "/percorso/consegna.md", "", "");
        let msg = resume_message();

        assert!(msg.contains("/percorso/consegna.md"), "{msg}");
        assert!(!msg.contains("è il suo mandato che continua"), "{msg}");
    }

    /// Un segnale col corpo dato alla lettera, per provare le forme storte.
    fn signal(casa: &HomeIsolata, corpo: &str) {
        std::env::set_var("ORCA_WORKTREE_ID", "repo::_prova");
        let dir = casa.dir.join(".claude/state/riprendi-da");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{}.txt", state_key("repo::_prova"))), corpo)
            .unwrap();
    }

    #[test]
    fn a_signal_with_only_the_path_stays_valid() {
        // E' la forma che il file ha avuto per giorni: cambiarla senza tenerla
        // buona spegnerebbe la ripresa di ogni albero gia' in attesa.
        let casa = HomeIsolata::nuova("segnale-vecchio");
        signal(&casa, "/percorso/consegna.md");
        let msg = resume_message();
        assert!(msg.contains("/percorso/consegna.md"), "{msg}");
        assert!(!msg.contains("stava girando su questo mandato"), "{msg}");
    }

    #[test]
    fn a_signal_naming_a_session_nobody_has_a_record_of_still_delivers() {
        // IL CASO NORMALE DOPO IL CABLAGGIO DEL 25/08/2026, e quello che una
        // riverifica troppo severa spegnerebbe: della sessione nominata non si
        // sa niente — nessun record, e in questo perimetro nemmeno `ps` che
        // risponda. «Non lo so» non e' «mi hanno mentito», e il mandato passa.
        let fake_home = HomeIsolata::nuova("segnale-sessione-ignota");
        let corpo = serde_json::json!({
            "handoff": "/percorso/consegna.md",
            "punto": "",
            "mandato": "",
            "tab": "",
            "sessione": "aaaabbbb-1111-2222-3333-444455556666",
        })
        .to_string();
        signal(&fake_home, &corpo);
        let msg = resume_message();
        assert!(
            msg.contains("/percorso/consegna.md"),
            "un segnale su una sessione ignota e' stato buttato via: {msg}"
        );
    }

    #[test]
    fn the_resume_point_reaches_the_successor() {
        // È la differenza fra «leggi questo documento e prosegui» e «riprendi
        // da qui»: il primo chiede di dedurre, il secondo dice.
        let casa = HomeIsolata::nuova("segnale-punto");
        signal_json(&casa, "/percorso/consegna.md", "la staffetta via /clear", "");
        let msg = resume_message();
        assert!(msg.contains("RIPRENDI DA QUI"), "{msg}");
        assert!(msg.contains("la staffetta via /clear"), "{msg}");
    }

    #[test]
    fn the_resume_point_comes_before_the_mandate() {
        let casa = HomeIsolata::nuova("segnale-ordine");
        signal_json(
            &casa,
            "/percorso/consegna.md",
            "il gate della lingua",
            "/loop Sistema tutto",
        );
        let msg = resume_message();
        let p = msg.find("il gate della lingua").expect("punto assente");
        let m = msg.find("/loop Sistema tutto").expect("mandato assente");
        assert!(p < m, "il mandato precede il punto:\n{msg}");
    }

    #[test]
    fn the_loop_mandate_reaches_the_successor() {
        // Senza queste righe il successore legge la consegna, dichiara da dove
        // riparte e si ferma: e' come e' morto il loop del 19/08/2026.
        let casa = HomeIsolata::nuova("segnale-mandato");
        signal_json(&casa, "/percorso/consegna.md", "", "/loop Sistemare la configurazione");
        let msg = resume_message();
        assert!(msg.contains("/percorso/consegna.md"), "{msg}");
        assert!(msg.contains("/loop Sistemare la configurazione"), "{msg}");
        assert!(msg.contains("tocca"), "{msg}");
    }

    #[test]
    fn a_signal_addressed_to_someone_else_is_not_consumed() {
        // Due sessioni sullo stesso albero sono un caso normale, e da quando
        // ogni rigenerazione manda un `/clear` la seconda che riparte
        // troverebbe il segnale della prima: si prenderebbe il suo punto di
        // ripresa e il suo `/loop`.
        let casa = HomeIsolata::nuova("segnale-altrui");
        signal_for(&casa, "/percorso/consegna.md", "punto altrui", "", "tab-di-un-altro");
        std::env::set_var("ORCA_TAB_ID", "tab-mia");
        let msg = resume_message();
        std::env::remove_var("ORCA_TAB_ID");
        assert!(msg.is_empty(), "ha raccolto il mandato di un'altra sessione: {msg}");
        // E resta sul disco per chi lo aspetta.
        let signal = casa
            .dir
            .join(".claude/state/riprendi-da")
            .join(format!("{}.txt", state_key("repo::_prova")));
        assert!(signal.exists(), "ha consumato un segnale non suo");
    }

    #[test]
    fn a_signal_addressed_to_us_is_consumed() {
        let casa = HomeIsolata::nuova("segnale-mio");
        signal_for(&casa, "/percorso/consegna.md", "il mio punto", "", "tab-mia");
        std::env::set_var("ORCA_TAB_ID", "tab-mia");
        let msg = resume_message();
        std::env::remove_var("ORCA_TAB_ID");
        assert!(msg.contains("il mio punto"), "{msg}");
    }

    #[test]
    fn a_multiline_mandate_arrives_intact() {
        // IL CASO PER CUI QUESTO CANALE ESISTE. Un mandato di `/loop` è spesso
        // un elenco, e il formato a righe etichettate — provato per un'ora il
        // 19/08/2026 — riattaccava le righe senza separatore: «1. leggi X» e
        // «2. fai Y» arrivavano come «1. leggi X2. fai Y».
        let casa = HomeIsolata::nuova("segnale-multiriga");
        let mandato = "/loop Sistema la configurazione:\n1. leggi X\n2. fai Y";
        signal_json(&casa, "/percorso/consegna.md", "il primo punto\ne il secondo", mandato);
        let msg = resume_message();
        assert!(msg.contains(mandato), "mandato corrotto nel viaggio:\n{msg}");
        assert!(msg.contains("il primo punto\ne il secondo"), "punto corrotto:\n{msg}");
        assert!(!msg.contains("leggi X2."), "righe riattaccate senza separatore:\n{msg}");
    }

    #[test]
    fn a_session_with_no_id_sweeps_nothing() {
        let casa = HomeIsolata::nuova("fine-senza-id");
        let state = casa.dir.join(".claude/state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("consegna-misura-77776666"), "x").unwrap();
        forget_session(&event("SessionEnd", ""));
        assert!(state.join("consegna-misura-77776666").exists());
    }

    #[test]
    fn one_panel_key_is_enough_and_the_worktree_is_never_optional() {
        assert!(missing_panel_keys("wt", "term", "tab").is_empty());
        assert!(missing_panel_keys("wt", "", "tab").is_empty());
        assert!(missing_panel_keys("wt", "term", "").is_empty());
        assert_eq!(
            missing_panel_keys("wt", "", ""),
            vec!["ORCA_TAB_ID|ORCA_TERMINAL_HANDLE"]
        );
        assert_eq!(missing_panel_keys("", "term", "tab"), vec!["ORCA_WORKTREE_ID"]);
        // Una variabile esportata piena di spazi è assente quanto una che non
        // c'è: `env()` restituisce la stringa così com'è, e il confronto con la
        // stringa vuota da solo la lascerebbe passare per buona.
        assert_eq!(missing_panel_keys("  ", "term", "tab"), vec!["ORCA_WORKTREE_ID"]);
    }

    /// Il payload di un avvio, nella forma che il gancio riceve.
    fn start(session: &str) -> serde_json::Value {
        serde_json::json!({
            "hook_event_name": "SessionStart",
            "session_id": session,
            "transcript_path": "",
            "cwd": "/prova/albero",
            "source": "startup",
        })
    }

    /// Le righe del registro dei ganci scritte finora nella casa isolata.
    fn journal_lines(home: &HomeIsolata) -> String {
        std::fs::read_to_string(home.stato().join("ganci.jsonl")).unwrap_or_default()
    }

    /// Il caso che conta: dentro Orca una chiave mancante non fa più sparire la
    /// sessione in silenzio.
    ///
    /// DIFFERENZIALE A VARIABILE UNICA — i due bracci cambiano solo per
    /// `ORCA_WORKTREE_ID`. La prova «il record c'è / non c'è» da sola non
    /// basterebbe: togliendo la riga di registro dal ramo, il record continua a
    /// mancare in entrambi i modi e la mutazione passerebbe. Quello che separa
    /// il guasto muto dal guasto detto è la riga, e qui si guarda quella.
    #[test]
    fn inside_orca_a_missing_key_leaves_a_line_instead_of_silence() {
        let home = HomeIsolata::nuova("registro-sessione-muta");
        std::env::set_var("ORCA_AGENT_HOOK_PORT", "49571");
        std::env::set_var("ORCA_TAB_ID", "tab-1");
        std::env::set_var("ORCA_TERMINAL_HANDLE", "term-1");

        std::env::set_var("ORCA_WORKTREE_ID", "repo::/prova/albero");
        record_session(&start("aaaaaaaa-1111-2222-3333-444444444444"));
        assert!(
            home.stato().join("sessioni-vive/aaaaaaaa.json").exists(),
            "con tutte le chiavi il record deve esserci"
        );
        assert!(
            !journal_lines(&home).contains("register-session"),
            "una registrazione riuscita non lascia righe: {}",
            journal_lines(&home)
        );

        std::env::remove_var("ORCA_WORKTREE_ID");
        record_session(&start("bbbbbbbb-1111-2222-3333-444444444444"));
        assert!(
            !home.stato().join("sessioni-vive/bbbbbbbb.json").exists(),
            "senza la chiave dell'albero non si registra, e resta così"
        );
        let lines = journal_lines(&home);
        assert!(lines.contains("\"gancio\":\"register-session\""), "{lines}");
        assert!(lines.contains("chiavi-del-pannello-mancanti"), "{lines}");
        assert!(lines.contains("ORCA_WORKTREE_ID"), "{lines}");
        assert!(lines.contains("\"session\":\"bbbbbbbb\""), "{lines}");
    }

    /// Fuori da Orca la stessa mancanza è il caso normale e resta muta:
    /// altrimenti ogni sessione aperta da un terminale qualunque scriverebbe una
    /// riga, e il registro che serve a contare i guasti conterebbe il normale.
    #[test]
    fn outside_orca_the_same_absence_stays_silent() {
        let home = HomeIsolata::nuova("registro-fuori-da-orca");
        std::env::remove_var("ORCA_AGENT_HOOK_PORT");
        std::env::remove_var("ORCA_WORKTREE_ID");
        std::env::remove_var("ORCA_TAB_ID");
        std::env::remove_var("ORCA_TERMINAL_HANDLE");

        record_session(&start("cccccccc-1111-2222-3333-444444444444"));

        assert!(!home.stato().join("sessioni-vive/cccccccc.json").exists());
        assert!(
            !journal_lines(&home).contains("register-session"),
            "fuori da Orca non c'è niente da denunciare: {}",
            journal_lines(&home)
        );
    }

    /// Un anello di prova: solo `pid` e `comm` contano per la scelta.
    fn link(pid: u32, comm: &str) -> ProcessLink {
        ProcessLink { pid, comm: comm.to_string() }
    }

    #[test]
    fn the_direct_parent_already_being_claude_is_chosen() {
        let chain = [link(111, "claude"), link(222, "/bin/zsh")];
        assert_eq!(choose_session_pid(&chain, MAX_ANCESTOR_HOPS), Some(111));
    }

    #[test]
    fn a_shell_interposed_before_claude_is_skipped_over() {
        // Il gancio può essere lanciato via `sh -c "..."`: il padre diretto
        // muore subito, `claude` è un anello più su.
        let chain = [link(333, "/bin/sh"), link(111, "claude"), link(222, "/bin/zsh")];
        assert_eq!(choose_session_pid(&chain, MAX_ANCESTOR_HOPS), Some(111));
    }

    #[test]
    fn a_chain_without_claude_at_all_says_it_does_not_know() {
        let chain = [link(333, "/bin/sh"), link(222, "/bin/zsh"), link(1, "/sbin/launchd")];
        assert_eq!(choose_session_pid(&chain, MAX_ANCESTOR_HOPS), None);
    }

    #[test]
    fn claude_beyond_the_cap_is_not_found() {
        // `claude` c'è, ma al quinto anello: con un tetto di quattro non lo si
        // deve trovare, altrimenti il tetto non tetta niente.
        let chain = [
            link(9, "a"),
            link(8, "b"),
            link(7, "c"),
            link(6, "d"),
            link(111, "claude"),
        ];
        assert_eq!(choose_session_pid(&chain, 4), None);
    }

    #[test]
    fn a_full_bundle_path_is_matched_on_its_last_component() {
        // `comm` arriva come percorso intero per gli app bundle: si confronta
        // l'ultimo pezzo, non la stringa intera.
        assert!(is_claude_process("claude"));
        assert!(!is_claude_process("/Applications/Orca.app/Contents/MacOS/Orca"));
        assert!(!is_claude_process("/bin/zsh"));
    }
}
