//! `marker-sweep`: ripassa i marcatori che il congedo di una sessione non ha
//! potuto buttare.
//!
//! PERCHÉ ESISTE. `register-session` dimentica una sessione al suo `SessionEnd`
//! e mai più. Ciò che in quell'istante non si sa vivo, ed è ancora fresco, resta
//! sul disco **per sempre** e non per un giorno: la soglia del congedo morde una
//! volta sola, e nessuno ripassa. Misurato il 21/08/2026 alle 13:40 sul disco
//! vero: 83 marcatori, 17 di sessioni vive, 66 di sessioni di cui non si sa
//! niente — e 28 di questi avevano già passato il giorno di grazia in solitudine,
//! accumulati in tre giorni dopo che il 18/08 una figura ne aveva spostati 149 a
//! mano in una cartella d'archivio.
//!
//! NON È UN RACCOGLITORE PER ETÀ, e la differenza è tutta qui. Il giudizio è
//! `register_session::should_remove`, LO STESSO che usa il congedo: chi si sa
//! vivo non si tocca a nessuna età, chi non si sa aspetta il giorno di grazia,
//! solo chi si sa morto va via subito. Nessuna soglia nuova e nessuna soglia più
//! bassa, perché i due errori non costano uguale: un marcatore orfano costa
//! spazio e nessuno lo interroga, mentre cancellare quello di una sessione viva
//! le toglie la consegna e ferma il ricambio delle figure.
//!
//! NON CANCELLA SE NON GLIELO SI CHIEDE. Senza `--delete` racconta e basta, e il
//! racconto è la forma in cui va letto la prima volta. I due nomi che in questa
//! casa hanno già ingannato qualcuno — `--secco`, che chiudeva davvero, e
//! `--esegui`, che ha smontato cinque alberi altrui — non si riusano.
//!
//! `successore-armato-<impronta>`: DUE FORMATI, DUE VIE. Il nome porta il
//! digest della sessione e mai il suo identificativo, quindi non si risale a
//! un record come per le altre famiglie. Chi arma il marcatore da questa
//! consegna in poi (21/08/2026, decisione del capitano delle 15:55) scrive
//! anche l'identificativo dentro: quei marcatori entrano nel censimento
//! normale, con lo stesso criterio a tre esiti degli altri. I venti nati
//! prima non lo portano: per loro il ricalcolo confronta l'impronta con ogni
//! sessione viva di adesso, senza soglia d'età — l'età non dice niente
//! sull'appartenenza quando la si può ricostruire.
//!
//! DUE CASI CHE IL RICALCOLO NON PUÒ RISOLVERE (riserve del revisore
//! indipendente, 21/08/2026), e per quelli sì che conta l'età: un'impronta
//! nata da sessione vuota non potrà mai combaciare con l'id di una sessione
//! viva (nessun id è vuoto), e un contenuto troncato non porta né percorso né
//! sessione. Dichiararli orfani a vista li cancellerebbe anche appena
//! scritti; ignorarli li lascerebbe sul disco per sempre. Prendono la stessa
//! grazia di 24 ore delle altre famiglie — vedi `LegacyVerdict::Unmatched`.

use crate::register_session::{
    file_age_secs, liveness_of, should_remove, state_dir, SessionLiveness, MARKER_FAMILIES,
    UNKNOWN_GRACE_SECS,
};
use hook_io::journal::{self, Field};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Oltre questo, un lucchetto rimasto in piedi è di un processo morto.
///
/// Una passata guarda un centinaio di file e chiama `ps` una volta per sessione:
/// dura meno di un secondo. Dieci minuti sono tre ordini di grandezza di margine,
/// e servono a non lasciare la raccolta spenta per sempre dopo un singolo
/// processo ucciso a metà — che è il modo in cui un lucchetto smette di
/// proteggere e comincia a bloccare.
const LOCK_STALE_SECS: u64 = 10 * 60;

/// Il nome del marcatore spezzato in famiglia e sessione. PURA.
///
/// LE FAMIGLIE SI PROVANO DALLA PIÙ LUNGA. `consegna-fatta` è prefisso di
/// `consegna-fatta-ripartenze`: prendendo la prima che combacia, la sessione di
/// `consegna-fatta-ripartenze-d9fed018` uscirebbe come `ripartenze-d9fed018`, e
/// per quel nome nessun record esiste — il marcatore di una sessione viva
/// diventerebbe di una sessione ignota, cioè cancellabile.
pub(crate) fn split_marker(name: &str) -> Option<(&'static str, &str)> {
    let mut best: Option<(&'static str, &str)> = None;
    for family in MARKER_FAMILIES {
        let Some(rest) = name.strip_prefix(family).and_then(|r| r.strip_prefix('-')) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        if best.map_or(true, |(chosen, _)| family.len() > chosen.len()) {
            best = Some((family, rest));
        }
    }
    best
}

/// Il prefisso della famiglia che porta l'impronta invece del nome.
const ARMED_PREFIX: &str = "successore-armato-";

/// Cosa c'è scritto in un marcatore `successore-armato-*`, nelle due forme
/// che convivono sul disco.
///
/// IL FORMATO VECCHIO (Python, 18/08/2026) porta l'orario e poi il percorso;
/// un marcatore con una sola riga porta solo il percorso. Quello nuovo — da
/// questa consegna in poi (decisione del capitano, 21/08/2026 15:55) — porta
/// il percorso e poi l'identificativo intero della sessione. Si distinguono
/// dalla prima riga: un orario comincia con quattro cifre e un trattino, un
/// percorso no.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ArmedContent {
    New { path: String, session: String },
    Legacy { path: String },
    Unreadable,
}

fn parse_armed_content(raw: &str) -> ArmedContent {
    let lines: Vec<&str> = raw.lines().filter(|l| !l.is_empty()).collect();
    match lines.as_slice() {
        [] => ArmedContent::Unreadable,
        [only] => ArmedContent::Legacy { path: only.to_string() },
        [first, second, ..] => {
            let looks_like_timestamp = first.len() >= 5
                && first.as_bytes()[..4].iter().all(u8::is_ascii_digit)
                && first.as_bytes()[4] == b'-';
            if looks_like_timestamp {
                ArmedContent::Legacy { path: second.to_string() }
            } else {
                ArmedContent::New {
                    path: first.to_string(),
                    session: second.to_string(),
                }
            }
        }
    }
}

/// Un marcatore censito: com'è fatto, di chi è, e cosa se ne sa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Marker {
    pub name: String,
    pub session: String,
    pub liveness: SessionLiveness,
    pub age_secs: u64,
}

/// Il censimento: ogni marcatore della cartella, con quel che se ne sa.
///
/// La vivezza si chiede **una volta per sessione** e non una per file: la
/// domanda costa un `ps`, e una sessione lascia fino a nove marcatori. Chiederla
/// per file darebbe anche risposte diverse dentro la stessa passata, e allora
/// due marcatori della stessa sessione finirebbero uno tenuto e uno buttato.
///
/// `successore-armato-*` col formato nuovo entra qui come tutti gli altri,
/// leggendo la sessione dal contenuto invece che dal nome; col formato
/// vecchio resta fuori, ed è compito di `legacy_armed_markers`.
pub(crate) fn census(dir: &Path) -> Vec<Marker> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut known: HashMap<String, SessionLiveness> = HashMap::new();
    let mut markers = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Una cartella che si chiama come un marcatore non è un marcatore: la
        // passata cancella file, e `remove_file` su una cartella fallirebbe
        // lasciando il conteggio bugiardo.
        if !entry.path().is_file() {
            continue;
        }
        let session = if name.starts_with(ARMED_PREFIX) {
            let raw = fs::read_to_string(entry.path()).unwrap_or_default();
            match parse_armed_content(&raw) {
                ArmedContent::New { session, .. } if !session.is_empty() => {
                    session.chars().take(8).collect()
                }
                // Formato vecchio o illeggibile: non c'è sessione da leggere
                // dal nome né dal contenuto, se ne occupa il ricalcolo.
                _ => continue,
            }
        } else {
            let Some((_, session)) = split_marker(&name) else {
                continue;
            };
            session.to_string()
        };
        let Some(age_secs) = file_age_secs(&entry.path()) else {
            continue; // età illeggibile: non si giudica ciò che non si è guardato
        };
        let liveness = *known
            .entry(session.clone())
            .or_insert_with(|| liveness_of(&session));
        markers.push(Marker { name, session, liveness, age_secs });
    }
    markers.sort_by(|a, b| a.name.cmp(&b.name));
    markers
}

// ─── Il ricalcolo in avanti, per i marcatori nati senza identificativo ─────
//
// Decisione del capitano, 21/08/2026 15:55: i venti marcatori `successore-
// armato-<impronta>` scritti prima di questa consegna non portano la
// sessione, né nel nome né nel contenuto. L'unica via è ricalcolare
// l'impronta per ogni sessione viva di adesso — senza guardare l'età, perché
// per questa famiglia l'età non dice niente sull'appartenenza.

/// Un marcatore armato nel formato vecchio: impronta e, quando c'è, il
/// percorso. Solo questi passano dal ricalcolo — i nuovi li legge già il
/// censimento normale.
///
/// `path: None` è un file troncato o vuoto (riserva b, revisore indipendente
/// 21/08/2026): niente da rileggere, ma il nome resta un marcatore vero e va
/// giudicato lo stesso — non ignorato per sempre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyArmedMarker {
    pub name: String,
    pub hex: String,
    pub path: Option<String>,
}

/// Un marcatore armato del formato vecchio, letto **da un nome solo**.
///
/// `None` se non c'è, se non è un file, se il nome non è di questa famiglia, o
/// se il contenuto è già nel formato nuovo — che il censimento normale giudica
/// da sé.
fn legacy_marker_at(dir: &Path, name: &str) -> Option<LegacyArmedMarker> {
    let hex = name.strip_prefix(ARMED_PREFIX)?.to_string();
    let file = dir.join(name);
    if !file.is_file() {
        return None;
    }
    let raw = fs::read_to_string(&file).unwrap_or_default();
    let path = match parse_armed_content(&raw) {
        ArmedContent::Legacy { path } => Some(path),
        ArmedContent::Unreadable => None,
        ArmedContent::New { .. } => return None,
    };
    Some(LegacyArmedMarker { name: name.to_string(), hex, path })
}

/// I marcatori armati che non portano l'identificativo.
pub(crate) fn legacy_armed_markers(dir: &Path) -> Vec<LegacyArmedMarker> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<LegacyArmedMarker> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            legacy_marker_at(dir, &name)
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// PURA: gli identificativi interi delle sole sessioni il cui record dice
/// "viva". Separata da chi legge il disco e chiama `ps`, per lo stesso
/// motivo per cui il resto di questo modulo tiene il giudizio lontano dal
/// sistema: una sessione viva si chiama `claude`, e la batteria no.
fn alive_full_ids(records: &[(SessionLiveness, serde_json::Value)]) -> Vec<String> {
    records
        .iter()
        .filter(|(liveness, _)| *liveness == SessionLiveness::Alive)
        .filter_map(|(_, v)| v.get("session_id").and_then(|x| x.as_str()))
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect()
}

/// Gli identificativi interi delle sessioni vive adesso: la materia prima
/// del ricalcolo in avanti. `None` se la cartella non si è potuta leggere —
/// un elenco vuoto perché non risponde e uno vuoto perché non c'è nessuno
/// restano distinguibili, perché confonderli costerebbe la consegna di una
/// sessione viva (la stessa trappola del registro delle autorizzazioni).
fn live_full_session_ids(state: &Path) -> Option<Vec<String>> {
    let live_dir = state.join("sessioni-vive");
    let entries = match fs::read_dir(&live_dir) {
        Ok(e) => e,
        // LA CARTELLA CHE NON C'È È «NON SO», NON «NESSUNO».
        //
        // Qui rispondeva `Some(vec![])`, cioè lo stesso valore che dà una
        // cartella vuota — e il commento sopra prometteva il contrario. La
        // differenza non è teorica: con zero sessioni vive nessuna impronta
        // combacia, quindi **ogni** marcatore del formato vecchio risulta
        // orfano, e un orfano si cancella senza grazia, a età zero. Il
        // verdetto del 21/08/2026 l'ha riprodotto: cartella assente, marcatore
        // scritto un secondo prima, `1 orphan` e con `--delete` sparisce.
        //
        // Una cartella che non c'è non è un caso esotico: è l'ambiente nuovo,
        // la macchina diversa, o qualcuno che sposta a mano una cartella di
        // stato durante una manutenzione — già successo il 18/08/2026. Ed è
        // meno grave della cartella illeggibile per altra via, che invece
        // rispondeva già «non so»: lo stesso nulla dava due esiti opposti a
        // seconda della forma dell'errore, e quello trattato con fiducia era
        // il più probabile dei due.
        Err(_) => return None,
    };
    let mut records = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(sess) = name.strip_suffix(".json") else {
            continue;
        };
        let Ok(raw) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        records.push((liveness_of(sess), v));
    }
    Some(alive_full_ids(&records))
}

/// Il giudizio su un marcatore del formato vecchio. Due bracci in più
/// rispetto a `FingerprintOwner`, per le due riserve del revisore
/// indipendente (21/08/2026):
///
/// - RISERVA (a): un'impronta calcolata su sessione vuota (`armed_fingerprint
///   (path, "")`) non può MAI combaciare con l'id di una sessione viva — la
///   formula usa il percorso solo quando la sessione è vuota, e nessuna
///   sessione viva ha un id vuoto. Dichiararla `Orphan` la cancellerebbe
///   sempre, anche appena scritta: non è un giudizio sulla sua appartenenza,
///   è una domanda a cui la sessione non può rispondere.
/// - RISERVA (b): un contenuto troncato o vuoto non porta né percorso né
///   sessione: non c'è niente da ricalcolare, ma il file esiste e va
///   giudicato — non ignorato per sempre.
///
/// Per tutti e due, `Unmatched` giudica sull'età con la stessa grazia delle
/// altre famiglie (`UNKNOWN_GRACE_SECS`): non è "orfano", è "non so".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyVerdict {
    Alive,
    Orphan,
    Unknown,
    Unmatched { stale: bool },
}

/// A chi appartiene ciascun marcatore, ricalcolando l'impronta — o,
/// quando il ricalcolo non può rispondere per costruzione, giudicando
/// sull'età.
pub(crate) fn fingerprint_owners(
    dir: &Path,
    markers: &[LegacyArmedMarker],
    live: Option<&[String]>,
) -> Vec<(String, LegacyVerdict)> {
    markers
        .iter()
        .map(|m| {
            let verdict = match &m.path {
                // Riserva (b): niente percorso da ricalcolare.
                None => {
                    let age = file_age_secs(&dir.join(&m.name)).unwrap_or(0);
                    LegacyVerdict::Unmatched { stale: age >= UNKNOWN_GRACE_SECS }
                }
                // Riserva (a): l'impronta è quella di una sessione vuota, e
                // nessuna sessione viva potrà mai riprodurla.
                Some(path) if guards::successor::armed_fingerprint(path, "") == m.hex => {
                    let age = file_age_secs(&dir.join(&m.name)).unwrap_or(0);
                    LegacyVerdict::Unmatched { stale: age >= UNKNOWN_GRACE_SECS }
                }
                Some(path) => {
                    match guards::successor::recalculate_fingerprint_owner(&m.hex, path, live) {
                        guards::successor::FingerprintOwner::Alive => LegacyVerdict::Alive,
                        guards::successor::FingerprintOwner::Orphan => LegacyVerdict::Orphan,
                        guards::successor::FingerprintOwner::Unknown => LegacyVerdict::Unknown,
                    }
                }
            };
            (m.name.clone(), verdict)
        })
        .collect()
}

/// Quanti marcatori per ciascun esito del ricalcolo. Un conteggio a parte
/// dal `Tally` delle altre famiglie: la parte ricalcolata per impronta non
/// conosce età, quella "non abbinabile" (riserve a/b) sì.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FingerprintTally {
    pub seen: usize,
    pub alive: usize,
    pub orphan: usize,
    pub unknown: usize,
    pub unmatched_fresh: usize,
    pub unmatched_stale: usize,
    pub removed: usize,
    /// Condannati al censimento e risparmiati alla rilettura, **perché il
    /// verdetto è cambiato**. Zero è il caso normale; un numero qui dice che
    /// fra il censimento e la cancellazione il mondo è cambiato, o che l'elenco
    /// dei vivi ha smesso di rispondere.
    pub spared_on_recheck: usize,
    /// Condannati che alla rilettura **non c'erano più**. Tenuto separato dai
    /// risparmiati perché non è la stessa cosa: qui non è stato salvato niente,
    /// il file se n'era già andato per conto suo. Sommarli farebbe salire un
    /// numero che si legge come «la passata sta girando addosso a qualcuno».
    pub vanished: usize,
    /// Condannati che la passata **non è riuscita** a togliere, e che sono
    /// ancora sul disco. Zero è il caso normale; qualunque altro numero è un
    /// guasto da guardare, non una statistica.
    pub remove_failed: usize,
}

pub(crate) fn tally_fingerprint(owners: &[(String, LegacyVerdict)]) -> FingerprintTally {
    let mut t = FingerprintTally { seen: owners.len(), ..FingerprintTally::default() };
    for (_, verdict) in owners {
        match verdict {
            LegacyVerdict::Alive => t.alive += 1,
            LegacyVerdict::Orphan => t.orphan += 1,
            LegacyVerdict::Unknown => t.unknown += 1,
            LegacyVerdict::Unmatched { stale: false } => t.unmatched_fresh += 1,
            LegacyVerdict::Unmatched { stale: true } => t.unmatched_stale += 1,
        }
    }
    t
}

/// Cancella i marcatori orfani e quelli "non abbinabili" ormai vecchi.
///
/// LA RILETTURA DI SICUREZZA C'È ANCHE QUI, dal 21/08/2026. Prima non c'era, e
/// la motivazione scritta qui — «questi marcatori li scrive una volta sola
/// `already_armed`, che non riscrive mai un file esistente» — era vera e
/// insufficiente: descriveva l'unica corsa che `apply` teme, non l'unica che
/// esiste. **Quello che cambia sotto i piedi della passata non è il file: è
/// l'elenco delle sessioni vive.** Un elenco che al censimento si legge e un
/// istante dopo non risponde più fa risultare orfano ogni marcatore del
/// formato vecchio — e un orfano si cancella senza grazia, a età zero.
///
/// Quindi i vivi si rileggono **adesso** — `live` è la lettura fresca, non
/// quella del censimento — e se la risposta è «non so» non si cancella niente:
/// tutti i condannati risultano risparmiati alla rilettura, e il rapporto lo
/// dice. È lo stesso terzo esito che regge tutto il modulo: «zero perché non
/// c'è nessuno» e «zero perché non ho potuto guardare» non sono la stessa cosa.
///
/// L'elenco ARRIVA DA FUORI e non si legge qui dentro, come ovunque in questo
/// modulo: chi giudica resta separato da chi tocca il sistema, altrimenti un
/// caso di prova non può più dire *quale* mondo sta provando.
pub(crate) fn apply_fingerprint(
    dir: &Path,
    owners: &[(String, LegacyVerdict)],
    live: Option<&[String]>,
    t: &mut FingerprintTally,
) {
    for (name, verdict) in owners {
        let remove = matches!(
            verdict,
            LegacyVerdict::Orphan | LegacyVerdict::Unmatched { stale: true }
        );
        if !remove {
            continue;
        }
        match remove_if_still_condemned(dir, name, live.as_deref()) {
            RecheckOutcome::Removed => t.removed += 1,
            RecheckOutcome::Spared => t.spared_on_recheck += 1,
            RecheckOutcome::Vanished => t.vanished += 1,
            RecheckOutcome::RemoveFailed => t.remove_failed += 1,
        }
    }
}

/// Cancella un marcatore del formato vecchio solo se **adesso** è ancora
/// condannato.
///
/// LE ALTRE FAMIGLIE LO FACEVANO GIÀ, QUESTA NO. `apply` rilegge l'età un
/// istante prima di togliere il file, perché fra il censimento e la
/// cancellazione la sessione può aver riscritto il proprio marcatore. Qui si
/// cancellava sulla fotografia del censimento, e la differenza fra le due
/// famiglie non era difendibile (verdetto del 21/08/2026).
///
/// SI RILEGGE IL VERDETTO, NON L'ETÀ, e la differenza sta in cosa può essere
/// cambiato: l'elenco delle sessioni vive, che qui arriva riletto. Il giudizio
/// si rifà con la funzione del censimento, non con una copia — due letture
/// dello stesso dato divergono alla prima correzione.
///
/// SI RILEGGE UN FILE SOLO, non tutta la cartella. La prima versione richiamava
/// `legacy_armed_markers`, cioè leggeva M file per giudicarne uno, N volte
/// (riserva del verdetto del 21/08/2026). Su venti marcatori era invisibile, ma
/// è un disegno che non regge da nessun'altra parte.
///
/// `Vanished` non è un risparmio, ed è la seconda riserva dello stesso verdetto:
/// un file già sparito non è stato salvato da niente, e contarlo insieme ai
/// salvataggi veri farebbe salire un numero che qualcuno leggerà come «la
/// passata sta girando addosso a qualcuno».
/// UNA CANCELLAZIONE FALLITA NON È UN FILE SPARITO, ed è la rifinitura del
/// secondo verdetto: un `remove_file` che non riesce su un file **ancora
/// presente** è un guasto — permessi, disco in sola lettura durante una
/// manutenzione — e il marcatore resta lì, orfano. Chiamarlo «già sparito»
/// sarebbe la stessa bugia tranquillizzante che questo modulo evita altrove.
enum RecheckOutcome {
    Removed,
    Spared,
    Vanished,
    RemoveFailed,
}

fn remove_if_still_condemned(dir: &Path, name: &str, live: Option<&[String]>) -> RecheckOutcome {
    let Some(still) = legacy_marker_at(dir, name) else {
        return RecheckOutcome::Vanished;
    };
    // `fingerprint_owners` mappa uno a uno e non filtra mai, quindi su un
    // elemento solo la risposta c'è sempre: nessun ramo per il vuoto.
    let (_, verdict) = fingerprint_owners(dir, &[still], live)
        .into_iter()
        .next()
        .expect("fingerprint_owners maps one to one");
    if !matches!(
        verdict,
        LegacyVerdict::Orphan | LegacyVerdict::Unmatched { stale: true }
    ) {
        return RecheckOutcome::Spared;
    }
    let file = dir.join(name);
    if fs::remove_file(&file).is_ok() {
        return RecheckOutcome::Removed;
    }
    // Si chiede al disco quale dei due casi è: il file portato via da qualcun
    // altro fra la rilettura e adesso, oppure una cancellazione che non riesce.
    if file.exists() {
        RecheckOutcome::RemoveFailed
    } else {
        RecheckOutcome::Vanished
    }
}

/// Il rapporto del ricalcolo in avanti, separato dal resto.
pub(crate) fn report_fingerprint(t: &FingerprintTally, deleting: bool) -> String {
    let head = format!(
        "marker-sweep (successore-armato, ricalcolo): {} markers, {} held (session alive), \
{} orphan, {} unknown (live list unreadable), {} unmatched fresh, {} unmatched stale \
(grace {}h)",
        t.seen,
        t.alive,
        t.orphan,
        t.unknown,
        t.unmatched_fresh,
        t.unmatched_stale,
        UNKNOWN_GRACE_SECS / 3600
    );
    if !deleting {
        return format!("{head}\n  report only -- pass --delete to remove orphans and stale unmatched ones");
    }
    format!(
        "{head}\n  removed {}, spared on recheck {}, already gone {}, could not remove {}",
        t.removed, t.spared_on_recheck, t.vanished, t.remove_failed
    )
}

/// Quanti marcatori per ciascun esito. Serve al rapporto e al registro.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tally {
    pub seen: usize,
    pub held_alive: usize,
    pub held_fresh: usize,
    pub stale: usize,
    pub removed: usize,
    /// Giudicati da buttare e poi risparmiati: il file è tornato fresco fra il
    /// censimento e la cancellazione. Un numero che sale è il segnale che la
    /// passata sta girando addosso a qualcuno.
    pub spared_on_recheck: usize,
}

pub(crate) fn tally(markers: &[Marker]) -> Tally {
    let mut t = Tally { seen: markers.len(), ..Tally::default() };
    for m in markers {
        if should_remove(m.liveness, m.age_secs) {
            t.stale += 1;
        } else if m.liveness == SessionLiveness::Alive {
            t.held_alive += 1;
        } else {
            t.held_fresh += 1;
        }
    }
    t
}

/// Cancella, ma solo se il file è ANCORA quello che si è giudicato.
///
/// Fra il censimento e la cancellazione passa del tempo, e in quel tempo la
/// sessione che non si sapeva viva può aver riscritto il proprio marcatore: si
/// rilegge l'età un istante prima di togliere il file, e se è tornata fresca non
/// si tocca niente. Senza questa rilettura la passata porta via un marcatore
/// appena riscritto — due processi sullo stesso file di stato, che è la trappola
/// che il 21/08/2026 aveva già dato un rosso altrove.
fn remove_if_still_stale(path: &Path, liveness: SessionLiveness) -> bool {
    let Some(age) = file_age_secs(path) else {
        return false; // sparito o illeggibile: non è roba nostra da togliere
    };
    if !should_remove(liveness, age) {
        return false;
    }
    fs::remove_file(path).is_ok()
}

/// Esegue le cancellazioni del censimento, con la rilettura di sicurezza.
pub(crate) fn apply(dir: &Path, markers: &[Marker], t: &mut Tally) {
    for m in markers {
        if !should_remove(m.liveness, m.age_secs) {
            continue;
        }
        if remove_if_still_stale(&dir.join(&m.name), m.liveness) {
            t.removed += 1;
        } else {
            t.spared_on_recheck += 1;
        }
    }
}

/// Il lucchetto della passata: un file creato con `O_EXCL`, tolto all'uscita.
///
/// PERCHÉ. Due passate insieme censiscono la stessa cartella e poi si contendono
/// gli stessi file: la seconda giudica su una fotografia che la prima sta già
/// smontando, e il rapporto che ne esce conta cancellazioni che non ha fatto.
/// Il file porta il processo, così un lucchetto rimasto in piedi si sa di chi era.
pub(crate) struct SweepLock {
    path: PathBuf,
}

impl SweepLock {
    /// Prende il lucchetto, oppure `None` se un'altra passata è in corso.
    ///
    /// Un lucchetto più vecchio di `LOCK_STALE_SECS` è di un processo che non
    /// esiste più e si scavalca: altrimenti un solo processo ucciso a metà
    /// spegnerebbe la raccolta per sempre, in silenzio.
    pub(crate) fn take(dir: &Path) -> Option<Self> {
        let path = dir.join("marker-sweep.lock");
        if Self::create(&path) {
            return Some(Self { path });
        }
        match file_age_secs(&path) {
            Some(age) if age >= LOCK_STALE_SECS => {
                let _ = fs::remove_file(&path);
                Self::create(&path).then_some(Self { path })
            }
            // Il lucchetto è sparito fra il tentativo e la lettura: si riprova
            // una volta sola, e se anche questa perde la corsa qualcuno l'ha
            // preso nel frattempo.
            None => Self::create(&path).then_some(Self { path }),
            Some(_) => None,
        }
    }

    fn create(path: &Path) -> bool {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let Ok(mut file) = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        else {
            return false;
        };
        let _ = writeln!(file, "{}", std::process::id());
        true
    }
}

impl Drop for SweepLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Il rapporto di una passata. In inglese perché finisce in un registro.
pub(crate) fn report(t: &Tally, deleting: bool) -> String {
    let hours = UNKNOWN_GRACE_SECS / 3600;
    let head = format!(
        "marker-sweep: {} markers, {} held (session alive), {} held (fresh, grace {}h), {} stale",
        t.seen, t.held_alive, t.held_fresh, hours, t.stale
    );
    if !deleting {
        return format!("{head}\n  report only -- pass --delete to remove the stale ones");
    }
    format!(
        "{head}\n  removed {}, spared on recheck {}",
        t.removed, t.spared_on_recheck
    )
}

pub fn run() -> i32 {
    let deleting = std::env::args().any(|a| a == "--delete");
    let dir = state_dir();
    let Some(_lock) = SweepLock::take(&dir) else {
        println!("marker-sweep: another sweep holds the lock, nothing done");
        return 0;
    };
    let markers = census(&dir);
    let mut t = tally(&markers);
    if deleting {
        apply(&dir, &markers, &mut t);
    }
    journal::record(
        "marker-sweep",
        if deleting { "passata" } else { "rapporto" },
        "marcatori-orfani",
        &[
            ("visti", Field::Number(t.seen as i64)),
            ("tenuti_vivi", Field::Number(t.held_alive as i64)),
            ("tenuti_freschi", Field::Number(t.held_fresh as i64)),
            ("scaduti", Field::Number(t.stale as i64)),
            ("cancellati", Field::Number(t.removed as i64)),
            ("risparmiati", Field::Number(t.spared_on_recheck as i64)),
        ],
    );
    println!("{}", report(&t, deleting));

    let legacy = legacy_armed_markers(&dir);
    let live = live_full_session_ids(&dir);
    let owners = fingerprint_owners(&dir, &legacy, live.as_deref());
    let mut ft = tally_fingerprint(&owners);
    if deleting {
        // I VIVI SI RILEGGONO QUI, e non si riusa `live` del censimento: fra
        // il giudizio e la cancellazione la cartella può aver smesso di
        // rispondere, e cancellare sulla fotografia di prima è il difetto che
        // il verdetto del 21/08/2026 ha trovato.
        let live_now = live_full_session_ids(&dir);
        apply_fingerprint(&dir, &owners, live_now.as_deref(), &mut ft);
    }
    journal::record(
        "marker-sweep",
        if deleting { "passata" } else { "rapporto" },
        "successore-armato-ricalcolo",
        &[
            ("visti", Field::Number(ft.seen as i64)),
            ("vivi", Field::Number(ft.alive as i64)),
            ("orfani", Field::Number(ft.orphan as i64)),
            ("ignoti", Field::Number(ft.unknown as i64)),
            ("non_abbinabili_freschi", Field::Number(ft.unmatched_fresh as i64)),
            ("non_abbinabili_scaduti", Field::Number(ft.unmatched_stale as i64)),
            ("cancellati", Field::Number(ft.removed as i64)),
            ("risparmiati", Field::Number(ft.spared_on_recheck as i64)),
            ("gia_spariti", Field::Number(ft.vanished as i64)),
            ("non_rimossi", Field::Number(ft.remove_failed as i64)),
        ],
    );
    println!("{}", report_fingerprint(&ft, deleting));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;
    use std::time::{Duration, SystemTime};

    /// Un marcatore col nome dato e l'età voluta, sul disco della casa isolata.
    ///
    /// L'età si impone all'indietro sull'orologio invece di aspettare: un caso
    /// che dovesse attendere un giorno di grazia non si scriverebbe.
    fn marker(dir: &Path, name: &str, age_secs: u64) {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, "x").unwrap();
        let when = SystemTime::now() - Duration::from_secs(age_secs);
        let file = fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(when).unwrap();
    }

    /// Un record di sessione con quel processo dentro. Senza `pid` il record
    /// c'è ma non sa: è il caso «non lo so», che non autorizza a cancellare.
    fn record(state: &Path, session: &str, pid: Option<u32>) {
        fs::create_dir_all(state.join("sessioni-vive")).unwrap();
        let body = match pid {
            Some(p) => format!("{{\"session_pid\": {p}}}"),
            None => "{}".to_string(),
        };
        fs::write(state.join(format!("sessioni-vive/{session}.json")), body).unwrap();
    }

    const A_DAY: u64 = UNKNOWN_GRACE_SECS;
    /// Un pid che su macOS non può esistere: il massimo è 99998.
    const IMPOSSIBLE_PID: u32 = 999_999;

    #[test]
    fn the_longest_family_wins_so_a_session_is_not_read_as_a_family() {
        assert_eq!(
            split_marker("consegna-fatta-ripartenze-d9fed018"),
            Some(("consegna-fatta-ripartenze", "d9fed018"))
        );
        assert_eq!(
            split_marker("consegna-fatta-d9fed018"),
            Some(("consegna-fatta", "d9fed018"))
        );
        assert_eq!(
            split_marker("consegna-stop-riferimento-d9fed018"),
            Some(("consegna-stop-riferimento", "d9fed018"))
        );
        assert_eq!(
            split_marker("consegna-stop-d9fed018"),
            Some(("consegna-stop", "d9fed018"))
        );
    }

    #[test]
    fn what_is_not_a_marker_is_not_touched() {
        assert_eq!(split_marker("consegna-fatta"), None); // manca la sessione
        assert_eq!(split_marker("consegna-fatta-"), None);
        assert_eq!(split_marker("sessioni-vive"), None);
        assert_eq!(split_marker("successore-armato-0fd0bb405406ad0e"), None);
        assert_eq!(split_marker("ganci.jsonl"), None);
    }

    /// PROVA 1. Vecchio oltre soglia, sessione che non è viva: sparisce.
    #[test]
    fn old_markers_of_a_session_that_is_gone_are_swept() {
        let home = HomeIsolata::nuova("passata-morta");
        let state = home.stato();
        record(&state, "11112222", Some(IMPOSSIBLE_PID));
        marker(&state, "consegna-misura-11112222", A_DAY * 4);
        marker(&state, "consegna-fatta-ripartenze-11112222", A_DAY * 4);

        let markers = census(&state);
        let mut t = tally(&markers);
        assert_eq!(t.stale, 2, "{markers:?}");
        apply(&state, &markers, &mut t);

        assert_eq!(t.removed, 2);
        assert!(!state.join("consegna-misura-11112222").exists());
        assert!(!state.join("consegna-fatta-ripartenze-11112222").exists());
    }

    /// PROVA 2, QUELLA CHE DEVE SAPER FALLIRE. Vecchio oltre soglia, ma la
    /// sessione è VIVA: resta. Un raccoglitore che guardasse solo l'età
    /// diventerebbe rosso qui, ed è il caso per cui questa passata non ha una
    /// soglia propria.
    ///
    /// La vivezza si dichiara invece di costruirla: una sessione viva si chiama
    /// `claude`, e la batteria no — è la stessa ragione per cui il caso gemello
    /// del congedo chiama `forget_with` e non `forget_session`.
    #[test]
    fn old_markers_of_a_live_session_survive_the_sweep() {
        let home = HomeIsolata::nuova("passata-viva");
        let state = home.stato();
        let names = ["consegna-misura-33334444", "consegna-volontaria-33334444"];
        for n in names {
            marker(&state, n, A_DAY * 30);
        }
        let markers: Vec<Marker> = names
            .iter()
            .map(|n| Marker {
                name: (*n).to_string(),
                session: "33334444".to_string(),
                liveness: SessionLiveness::Alive,
                age_secs: A_DAY * 30,
            })
            .collect();

        let mut t = tally(&markers);
        assert_eq!(t.stale, 0, "una sessione viva non ha marcatori scaduti");
        assert_eq!(t.held_alive, 2);
        apply(&state, &markers, &mut t);

        assert_eq!(t.removed, 0);
        for n in names {
            assert!(state.join(n).exists(), "{n}: la sessione lavora ancora");
        }
    }

    /// PROVA 3. Marcatori freschi: restano in tutti e due i casi — sessione
    /// ignota per record muto e sessione ignota per record assente.
    #[test]
    fn fresh_markers_stay_whatever_is_known_about_the_session() {
        let home = HomeIsolata::nuova("passata-fresca");
        let state = home.stato();
        // Il record c'è ma non porta il processo: è ogni record scritto prima
        // delle 11:30 del 21/08/2026. L'altro non ce l'ha affatto: è ogni
        // sessione fuori da Orca.
        record(&state, "55556666", None);
        marker(&state, "consegna-misura-55556666", 60);
        marker(&state, "consegna-misura-77778888", 60);

        let markers = census(&state);
        let mut t = tally(&markers);
        assert_eq!(t.stale, 0, "{markers:?}");
        assert_eq!(t.held_fresh, 2);
        apply(&state, &markers, &mut t);

        assert_eq!(t.removed, 0);
        assert!(state.join("consegna-misura-55556666").exists());
        assert!(state.join("consegna-misura-77778888").exists());
        // E il freno è l'età, non un caso che non poteva fallire: lo stesso
        // marcatore, un giorno più vecchio, esce scaduto.
        marker(&state, "consegna-misura-55556666", A_DAY);
        assert_eq!(tally(&census(&state)).stale, 1);
    }

    /// PROVA 4a. La passata non gira mentre un'altra passata gira.
    #[test]
    fn two_sweeps_do_not_run_on_the_same_state() {
        let home = HomeIsolata::nuova("passata-lucchetto");
        let state = home.stato();
        let first = SweepLock::take(&state).expect("il primo lucchetto si prende");
        assert!(
            SweepLock::take(&state).is_none(),
            "due passate insieme sullo stesso stato"
        );
        drop(first);
        assert!(
            SweepLock::take(&state).is_some(),
            "il lucchetto non si rilascia all'uscita"
        );
    }

    /// PROVA 4b. Un lucchetto di un processo morto non spegne la raccolta per
    /// sempre; uno fresco la ferma davvero.
    #[test]
    fn a_stale_lock_is_taken_over_but_a_fresh_one_still_holds() {
        let home = HomeIsolata::nuova("passata-lucchetto-vecchio");
        let state = home.stato();
        marker(&state, "marker-sweep.lock", LOCK_STALE_SECS + 1);
        assert!(SweepLock::take(&state).is_some());
        marker(&state, "marker-sweep.lock", 1);
        assert!(SweepLock::take(&state).is_none());
    }

    /// PROVA 4c, LA TRAPPOLA VERA. Il marcatore è stato riscritto fra il
    /// censimento e la cancellazione: la sessione che si credeva ferma sta
    /// lavorando, e il file non si tocca.
    #[test]
    fn a_marker_rewritten_after_the_census_is_not_removed() {
        let home = HomeIsolata::nuova("passata-corsa");
        let state = home.stato();
        marker(&state, "consegna-misura-99990000", A_DAY * 3);
        let markers = census(&state);
        let mut t = tally(&markers);
        assert_eq!(t.stale, 1);

        // La sessione riscrive il proprio marcatore: il censimento in mano è
        // vecchio di un istante e dice ancora «scaduto».
        marker(&state, "consegna-misura-99990000", 0);
        apply(&state, &markers, &mut t);

        assert_eq!(t.removed, 0);
        assert_eq!(t.spared_on_recheck, 1);
        assert!(
            state.join("consegna-misura-99990000").exists(),
            "portato via un marcatore riscritto un istante prima"
        );
    }

    /// Il censimento guarda le famiglie dichiarate e nient'altro.
    #[test]
    fn the_census_reads_the_families_and_nothing_else() {
        let home = HomeIsolata::nuova("passata-censimento");
        let state = home.stato();
        marker(&state, "consegna-misura-aaaabbbb", A_DAY * 2);
        marker(&state, "successore-armato-0fd0bb405406ad0e", A_DAY * 2);
        marker(&state, "ganci.jsonl", A_DAY * 2);
        fs::create_dir_all(state.join("consegna-misura-ccccdddd")).unwrap();

        let markers = census(&state);
        assert_eq!(markers.len(), 1, "{markers:?}");
        assert_eq!(markers[0].name, "consegna-misura-aaaabbbb");
        assert_eq!(markers[0].session, "aaaabbbb");
    }

    /// Il rapporto dice cosa farebbe, e senza `--delete` dichiara di non averlo
    /// fatto.
    #[test]
    fn the_report_names_what_a_pass_would_take() {
        let t = Tally {
            seen: 83,
            held_alive: 17,
            held_fresh: 38,
            stale: 28,
            ..Tally::default()
        };
        let text = report(&t, false);
        assert!(text.contains("83 markers"), "{text}");
        assert!(text.contains("17 held (session alive)"), "{text}");
        assert!(text.contains("28 stale"), "{text}");
        assert!(text.contains("--delete"), "{text}");
        assert!(!report(&t, true).contains("report only"));
    }

    // ─── Il ricalcolo in avanti: `successore-armato-*` ─────────────────────
    // Decisione del capitano, 21/08/2026 15:55.

    #[test]
    fn parse_armed_content_reads_the_three_shapes() {
        assert_eq!(
            parse_armed_content("/x/consegna.md\n"),
            ArmedContent::Legacy { path: "/x/consegna.md".to_string() }
        );
        assert_eq!(
            parse_armed_content("2026-08-18T02:53:35.905854\n/x/consegna.md\n"),
            ArmedContent::Legacy { path: "/x/consegna.md".to_string() }
        );
        assert_eq!(
            parse_armed_content("/x/consegna.md\n11112222-3333-4444-5555-666677778888\n"),
            ArmedContent::New {
                path: "/x/consegna.md".to_string(),
                session: "11112222-3333-4444-5555-666677778888".to_string(),
            }
        );
        assert_eq!(parse_armed_content(""), ArmedContent::Unreadable);
    }

    #[test]
    fn alive_full_ids_keeps_only_the_live_records() {
        let alive = serde_json::json!({"session_id": "11112222-aaaa"});
        let gone = serde_json::json!({"session_id": "99998888-bbbb"});
        let unknown = serde_json::json!({"session_id": "77776666-cccc"});
        let records = vec![
            (SessionLiveness::Alive, alive),
            (SessionLiveness::Gone, gone),
            (SessionLiveness::Unknown, unknown),
        ];
        assert_eq!(alive_full_ids(&records), vec!["11112222-aaaa".to_string()]);
    }

    #[test]
    fn live_full_session_ids_on_a_missing_directory_is_unknown() {
        let home = HomeIsolata::nuova("ricalcolo-cartella-assente");
        // IL CONTRATTO È CAMBIATO IL 21/08/2026, e questo caso diceva il
        // contrario: la cartella assente valeva «letta, nessuno». Ma quel
        // valore fa risultare orfano ogni marcatore del formato vecchio, e un
        // orfano si cancella senza grazia. Una cartella che non c'è è
        // l'ambiente nuovo o una manutenzione in corso, non una macchina
        // deserta: è il terzo esito.
        assert_eq!(live_full_session_ids(&home.stato()), None);
    }

    #[test]
    fn a_missing_live_directory_spares_a_fresh_legacy_marker() {
        let home = HomeIsolata::nuova("cartella-assente-non-cancella");
        let state = home.stato();
        // IL CASO DEL VERDETTO DEL 21/08/2026, dal principio alla fine invece
        // che sulla sola funzione che legge: cartella delle sessioni vive
        // assente, un marcatore del formato vecchio scritto adesso. Prima
        // usciva `1 orphan` e con `--delete` spariva a età zero, perché
        // «cartella che non c'è» valeva «nessuno è vivo».
        let hex = guards::successor::armed_fingerprint("/x/consegna.md", "1111-viva");
        let name = format!("successore-armato-{hex}");
        fs::write(state.join(&name), "2026-08-21T19:00:00.000000\n/x/consegna.md\n").unwrap();
        assert!(!state.join("sessioni-vive").exists(), "la casa isolata non deve avere l'elenco");

        let legacy = legacy_armed_markers(&state);
        let live = live_full_session_ids(&state);
        let owners = fingerprint_owners(&state, &legacy, live.as_deref());
        let mut t = tally_fingerprint(&owners);

        assert_eq!(t.unknown, 1, "senza elenco dei vivi il verdetto deve essere «non so»");
        assert_eq!(t.orphan, 0, "un marcatore è risultato orfano senza che nessuno abbia guardato");

        apply_fingerprint(&state, &owners, live.as_deref(), &mut t);

        assert_eq!(t.removed, 0);
        assert!(
            state.join(&name).exists(),
            "cancellato un marcatore fresco perché l'elenco dei vivi non c'era"
        );
    }

    #[test]
    fn an_orphan_whose_session_shows_up_again_is_spared() {
        let home = HomeIsolata::nuova("orfano-che-torna-vivo");
        let state = home.stato();
        // IL CASO PER CUI LA RILETTURA ESISTE, e mancava: al censimento la
        // sessione non risulta viva, quindi il marcatore è orfano — e un orfano
        // si cancella senza grazia. Un istante dopo la stessa sessione risulta
        // viva, e la rilettura deve salvarlo. Gli altri due casi provano solo
        // l'esito «non so»; questo prova il passaggio da condannato ad assolto.
        let owner = "11112222-3333-4444-5555-666677778888".to_string();
        let hex = guards::successor::armed_fingerprint("/x/consegna.md", &owner);
        let name = format!("successore-armato-{hex}");
        fs::write(state.join(&name), "2026-08-18T02:53:41.589486\n/x/consegna.md\n").unwrap();

        let legacy = legacy_armed_markers(&state);
        let owners = fingerprint_owners(&state, &legacy, Some(&[]));
        let mut t = tally_fingerprint(&owners);
        assert_eq!(t.orphan, 1, "{owners:?}");

        // La rilettura vede la sessione che al censimento non c'era.
        let live_now = vec![owner];
        apply_fingerprint(&state, &owners, Some(&live_now), &mut t);

        assert_eq!(t.removed, 0);
        assert_eq!(t.spared_on_recheck, 1, "il salvataggio non è stato contato");
        assert_eq!(t.vanished, 0, "un salvataggio è stato scambiato per un file sparito");
        assert!(state.join(&name).exists(), "tolto a una sessione tornata viva");
    }

    #[test]
    fn a_marker_that_vanished_is_not_counted_as_a_rescue() {
        let home = HomeIsolata::nuova("sparito-non-e-risparmiato");
        let state = home.stato();
        // I DUE NUMERI NON DICONO LA STESSA COSA. Un file già sparito non è
        // stato salvato da niente: contarlo insieme ai salvataggi veri fa
        // salire un numero che si legge come «la passata sta girando addosso a
        // qualcuno» (riserva del verdetto, 21/08/2026).
        let hex = guards::successor::armed_fingerprint("/x/morta.md", "9999-morta");
        let name = format!("successore-armato-{hex}");
        fs::write(state.join(&name), "2026-08-18T02:53:41.589486\n/x/morta.md\n").unwrap();

        let legacy = legacy_armed_markers(&state);
        let owners = fingerprint_owners(&state, &legacy, Some(&["1111-viva".to_string()]));
        let mut t = tally_fingerprint(&owners);
        assert_eq!(t.orphan, 1, "{owners:?}");

        // Fra il giudizio e la cancellazione il file se ne va per conto suo.
        fs::remove_file(state.join(&name)).unwrap();
        apply_fingerprint(&state, &owners, Some(&["1111-viva".to_string()]), &mut t);

        assert_eq!(t.removed, 0);
        assert_eq!(t.vanished, 1, "il file sparito non è stato contato per quello che è");
        assert_eq!(t.spared_on_recheck, 0, "un file sparito è stato contato come salvataggio");
    }

    #[test]
    fn a_removal_that_fails_is_not_called_a_vanished_file() {
        use std::os::unix::fs::PermissionsExt;
        let home = HomeIsolata::nuova("cancellazione-fallita-non-e-sparizione");
        let state = home.stato();
        // IL TERZO RAMO, che il secondo verdetto ha trovato scoperto: il file
        // c'è ancora e la cancellazione non riesce. Chiamarlo «già sparito»
        // sarebbe tranquillizzante e falso — il marcatore orfano resta lì.
        let hex = guards::successor::armed_fingerprint("/x/morta.md", "9999-morta");
        let name = format!("successore-armato-{hex}");
        fs::write(state.join(&name), "2026-08-18T02:53:41.589486\n/x/morta.md\n").unwrap();

        let legacy = legacy_armed_markers(&state);
        let live = vec!["1111-viva".to_string()];
        let owners = fingerprint_owners(&state, &legacy, Some(&live));
        let mut t = tally_fingerprint(&owners);
        assert_eq!(t.orphan, 1, "{owners:?}");

        // Una cartella in sola lettura fa fallire la rimozione senza toccare
        // il file: è la forma che questa casa ha già incontrato durante una
        // manutenzione del disco.
        let before = fs::metadata(&state).unwrap().permissions();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o555)).unwrap();
        apply_fingerprint(&state, &owners, Some(&live), &mut t);
        fs::set_permissions(&state, before).unwrap();

        assert_eq!(t.removed, 0);
        assert_eq!(t.vanished, 0, "una cancellazione fallita è stata contata come sparizione");
        assert_eq!(t.remove_failed, 1, "il guasto di cancellazione non è stato contato");
        assert!(state.join(&name).exists(), "il file doveva restare: la rimozione non è riuscita");
    }

    #[test]
    fn a_live_list_that_stops_answering_spares_the_condemned() {
        let home = HomeIsolata::nuova("elenco-sparito-fra-giudizio-e-taglio");
        let state = home.stato();
        // LA CORSA CHE `apply` COPRIVA E QUESTA FAMIGLIA NO: il censimento
        // legge l'elenco e giudica orfano, poi fra il giudizio e la
        // cancellazione l'elenco smette di rispondere. Cancellare sulla
        // fotografia di prima è il difetto; qui la rilettura fresca dice «non
        // so» e il file resta.
        let orphan_hex = guards::successor::armed_fingerprint("/x/morta.md", "9999-morta");
        let name = format!("successore-armato-{orphan_hex}");
        fs::write(state.join(&name), "2026-08-18T02:53:41.589486\n/x/morta.md\n").unwrap();

        let legacy = legacy_armed_markers(&state);
        let live_at_census = vec!["1111-viva".to_string()];
        let owners = fingerprint_owners(&state, &legacy, Some(&live_at_census));
        let mut t = tally_fingerprint(&owners);
        assert_eq!(t.orphan, 1, "{owners:?}");

        // La rilettura di adesso non risponde più.
        apply_fingerprint(&state, &owners, None, &mut t);

        assert_eq!(t.removed, 0);
        assert_eq!(t.spared_on_recheck, 1, "il risparmio non è stato contato");
        assert!(state.join(&name).exists(), "cancellato mentre l'elenco non rispondeva");
    }

    #[test]
    fn live_full_session_ids_on_an_unreadable_directory_is_unknown() {
        let home = HomeIsolata::nuova("ricalcolo-cartella-illeggibile");
        let state = home.stato();
        // Un file al posto della cartella: `read_dir` fallisce, ma non con
        // `NotFound` — è il terzo esito, non l'elenco vuoto.
        fs::write(state.join("sessioni-vive"), "non e' una cartella").unwrap();
        assert_eq!(live_full_session_ids(&state), None);
    }

    /// PROVA A DUE BRACCI (capitano, 21/08/2026 15:55). Un marcatore del
    /// vecchio formato la cui impronta combacia con una sessione viva
    /// sopravvive; uno la cui impronta non combacia con nessuna sparisce —
    /// nello stesso censimento, senza guardare l'età di nessuno dei due.
    #[test]
    fn the_forward_recalculation_keeps_the_live_one_and_drops_the_orphan() {
        let home = HomeIsolata::nuova("ricalcolo-due-bracci");
        let state = home.stato();
        let live_full = "11112222-3333-4444-5555-666677778888".to_string();

        let live_hex = guards::successor::armed_fingerprint("/x/consegna-viva.md", &live_full);
        let orphan_hex =
            guards::successor::armed_fingerprint("/x/consegna-morta.md", "99998888-morta");

        // Il formato vecchio vero: una riga di orario e poi il percorso —
        // quello dei venti marcatori trovati sul disco il 21/08/2026.
        fs::write(
            state.join(format!("successore-armato-{live_hex}")),
            "2026-08-18T02:53:35.905854\n/x/consegna-viva.md\n",
        )
        .unwrap();
        fs::write(
            state.join(format!("successore-armato-{orphan_hex}")),
            "2026-08-18T02:53:41.589486\n/x/consegna-morta.md\n",
        )
        .unwrap();

        let legacy = legacy_armed_markers(&state);
        assert_eq!(legacy.len(), 2, "{legacy:?}");

        let live = vec![live_full];
        let owners = fingerprint_owners(&state, &legacy, Some(&live));
        let mut t = tally_fingerprint(&owners);
        assert_eq!(t.alive, 1, "{owners:?}");
        assert_eq!(t.orphan, 1, "{owners:?}");

        apply_fingerprint(&state, &owners, Some(&live), &mut t);

        assert_eq!(t.removed, 1);
        assert!(
            state.join(format!("successore-armato-{live_hex}")).exists(),
            "il marcatore della sessione viva doveva restare"
        );
        assert!(
            !state.join(format!("successore-armato-{orphan_hex}")).exists(),
            "il marcatore orfano doveva sparire"
        );
    }

    /// Lo stesso caso, ma l'elenco delle sessioni vive non si è potuto
    /// leggere: il terzo esito protegge anche l'orfano, e nessuno dei due
    /// sparisce — è il vincolo che non si tocca.
    #[test]
    fn an_unreadable_live_list_removes_nothing_at_all() {
        let live_full = "11112222-3333-4444-5555-666677778888".to_string();
        let orphan_hex =
            guards::successor::armed_fingerprint("/x/consegna-morta.md", "99998888-morta");
        let legacy = vec![LegacyArmedMarker {
            name: format!("successore-armato-{orphan_hex}"),
            hex: orphan_hex,
            path: Some("/x/consegna-morta.md".to_string()),
        }];
        let owners = fingerprint_owners(Path::new("/inesistente"), &legacy, None);
        let t = tally_fingerprint(&owners);
        assert_eq!(t.unknown, 1);
        assert_eq!(t.orphan, 0);
        let _ = live_full; // solo a mostrare che non serve nemmeno passarlo
    }

    /// RISERVA (a) del revisore indipendente, 21/08/2026: `already_armed`
    /// chiamato con una sessione vuota — `input.session_id` è `None` — arma
    /// un marcatore la cui impronta è quella del solo percorso. Nessuna
    /// sessione viva potrà mai riprodurla (nessun id è vuoto): il ricalcolo
    /// non deve dichiararlo orfano a vista, o il freno anti-doppione
    /// sparirebbe appena scritto — il cui difetto è riarmare un secondo
    /// pannello. Prende la stessa grazia di 24 ore delle altre famiglie.
    #[test]
    fn a_session_less_marker_survives_fresh_and_falls_after_the_grace() {
        let home = HomeIsolata::nuova("armato-sessione-vuota");
        let state = home.stato();

        // La chiamata vera, con la sessione vuota che genera la riserva.
        assert!(!crate::successor::already_armed("/x/consegna-empty-session.md", ""));

        let hex = guards::successor::armed_fingerprint("/x/consegna-empty-session.md", "");
        let name = format!("successore-armato-{hex}");
        assert!(state.join(&name).exists());

        let legacy = legacy_armed_markers(&state);
        assert_eq!(legacy.len(), 1, "{legacy:?}");
        assert_eq!(legacy[0].path.as_deref(), Some("/x/consegna-empty-session.md"));

        // Appena scritto: nessuna sessione viva può mai riprodurre questa
        // impronta, ma è fresco — deve sopravvivere, non sparire a vista.
        let live = vec!["11112222-3333-4444-5555-666677778888".to_string()];
        let owners = fingerprint_owners(&state, &legacy, Some(&live));
        let mut t = tally_fingerprint(&owners);
        assert_eq!(t.unmatched_fresh, 1, "{owners:?}");
        assert_eq!(t.orphan, 0, "{owners:?}");
        apply_fingerprint(&state, &owners, Some(&live), &mut t);
        assert_eq!(t.removed, 0);
        assert!(state.join(&name).exists(), "il freno appena armato non deve sparire");

        // Invecchiato oltre la grazia: stessa impronta, adesso si toglie —
        // la stessa sorte di "non so" in tutte le altre famiglie.
        let old = SystemTime::now() - Duration::from_secs(UNKNOWN_GRACE_SECS + 1);
        let file = fs::File::options().write(true).open(state.join(&name)).unwrap();
        file.set_modified(old).unwrap();

        let legacy = legacy_armed_markers(&state);
        let owners = fingerprint_owners(&state, &legacy, Some(&live));
        let mut t = tally_fingerprint(&owners);
        assert_eq!(t.unmatched_stale, 1, "{owners:?}");
        apply_fingerprint(&state, &owners, Some(&live), &mut t);
        assert_eq!(t.removed, 1);
        assert!(!state.join(&name).exists(), "scaduta la grazia doveva sparire");
    }

    /// RISERVA (b) del revisore indipendente, 21/08/2026: un marcatore
    /// troncato — scrittura interrotta a metà — non porta niente da
    /// ricalcolare, ma esiste ed è un marcatore vero: prima veniva ignorato
    /// per sempre da tutte e due le passate.
    #[test]
    fn a_truncated_marker_is_no_longer_invisible() {
        let home = HomeIsolata::nuova("armato-troncato");
        let state = home.stato();
        let name = "successore-armato-0000000000000000";
        fs::write(state.join(name), "").unwrap(); // scrittura interrotta: niente dentro

        let legacy = legacy_armed_markers(&state);
        assert_eq!(legacy.len(), 1, "{legacy:?}");
        assert_eq!(legacy[0].path, None);

        let owners = fingerprint_owners(&state, &legacy, Some(&[]));
        let mut t = tally_fingerprint(&owners);
        assert_eq!(t.unmatched_fresh, 1, "{owners:?}");
        apply_fingerprint(&state, &owners, Some(&[]), &mut t);
        assert_eq!(t.removed, 0, "appena scritto, non ancora scaduto");
        assert!(state.join(name).exists());

        // Invecchiato oltre la grazia: sparisce, non resta lì per sempre.
        let old = SystemTime::now() - Duration::from_secs(UNKNOWN_GRACE_SECS + 1);
        let file = fs::File::options().write(true).open(state.join(name)).unwrap();
        file.set_modified(old).unwrap();
        let legacy = legacy_armed_markers(&state);
        let owners = fingerprint_owners(&state, &legacy, Some(&[]));
        let mut t = tally_fingerprint(&owners);
        apply_fingerprint(&state, &owners, Some(&[]), &mut t);
        assert_eq!(t.removed, 1);
        assert!(!state.join(name).exists());
    }

    /// Il formato nuovo scritto dal vero `already_armed`, non da un
    /// contenuto fabbricato a mano: prova l'aggancio fra chi scrive e chi
    /// legge, non solo la meccanica di lettura.
    #[test]
    fn a_marker_from_the_real_writer_is_read_directly_by_the_census() {
        let home = HomeIsolata::nuova("armato-formato-nuovo-scrittura-vera");
        let state = home.stato();
        let full = "aaaa1111-2222-3333-4444-555566667777";

        assert!(!crate::successor::already_armed("/x/consegna.md", full));

        let hex = guards::successor::armed_fingerprint("/x/consegna.md", full);
        let name = format!("successore-armato-{hex}");
        assert!(state.join(&name).exists(), "il marcatore vero deve esistere");

        // Il ricalcolo non ha niente da fare: il formato nuovo non è suo.
        assert!(legacy_armed_markers(&state).is_empty());

        // Il censimento normale lo legge diretto, sessione compresa.
        let markers = census(&state);
        let found = markers.iter().find(|m| m.name == name);
        assert_eq!(
            found.map(|m| m.session.as_str()),
            Some(&full[..8]),
            "{markers:?}"
        );
    }
}
