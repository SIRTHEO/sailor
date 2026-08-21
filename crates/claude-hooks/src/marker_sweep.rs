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
//! COSA NON GUARDA, E PERCHÉ. `successore-armato-<impronta>` porta il digest
//! della sessione, non il suo nome: da lì non si risale a nessun record, quindi
//! l'unico criterio possibile sarebbe l'età nuda — cioè proprio la cosa che
//! questo modulo esiste per non fare. Restano al congedo, che l'impronta la sa
//! calcolare.

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
pub(crate) fn census(dir: &Path) -> Vec<Marker> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut known: HashMap<String, SessionLiveness> = HashMap::new();
    let mut markers = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((_, session)) = split_marker(&name) else {
            continue;
        };
        // Una cartella che si chiama come un marcatore non è un marcatore: la
        // passata cancella file, e `remove_file` su una cartella fallirebbe
        // lasciando il conteggio bugiardo.
        if !entry.path().is_file() {
            continue;
        }
        let Some(age_secs) = file_age_secs(&entry.path()) else {
            continue; // età illeggibile: non si giudica ciò che non si è guardato
        };
        let session = session.to_string();
        let liveness = *known
            .entry(session.clone())
            .or_insert_with(|| liveness_of(&session));
        markers.push(Marker { name, session, liveness, age_secs });
    }
    markers.sort_by(|a, b| a.name.cmp(&b.name));
    markers
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
}
