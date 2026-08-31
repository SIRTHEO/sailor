//! **Tiene accesa la finestra mentre si aggiusta la macchina di sotto.**
//!
//! PERCHÉ ESISTE, IN UNA RIGA: `cargo tauri dev` ferma il programma acceso
//! **prima** di ricompilare, quindi ogni file toccato spegne la finestra e una
//! compilazione fallita è soltanto il motivo per cui non ne ritorna una. Qui
//! l'ordine è rovesciato — si costruisce, e si tocca ciò che è acceso solo se
//! la costruzione è riuscita. Il meccanismo di `tauri-cli` è citato per esteso
//! in `crates/supervisor/tests/a_broken_build_keeps_the_window.rs`.
//!
//! L'ALTRA METÀ È IL GUASTO 4. Un supervisore che accende processi e non li
//! scrive da nessuna parte fabbrica esattamente gli orfani che hanno bloccato
//! l'avvio due volte in una notte: quello che accende qui va nel deposito
//! (`crates/ledger`), che sopravvive alla finestra, alla sessione e al riavvio.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod child;

/// Qualcosa che è acceso e che si può spegnere.
///
/// **È un tratto e non un processo concreto perché la regola che questo crate
/// difende è una sola riga di sequenza**, e una riga di sequenza si prova senza
/// accendere niente. Il processo vero è `child::Process`, e le prove usano
/// tutti e due: il finto per l'ordine, quello vero per credere all'ordine.
pub trait Running {
    fn stop(&mut self) -> Result<(), String>;
}

/// Com'è andata la costruzione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildOutcome {
    Succeeded,
    /// Il testo che il compilatore ha stampato. **Va portato intero fino a chi
    /// guarda**: una modalità viva che dice «costruzione fallita» e basta
    /// obbliga a cercare l'errore in un terminale, che è il posto da cui questa
    /// finestra dovrebbe liberare.
    Failed { message: String },
}

/// Cosa è successo a un giro di ricostruzione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rebuild {
    /// Costruito, il vecchio fermato, il nuovo acceso.
    Replaced,
    /// Costruzione fallita: **quello di prima è ancora acceso**, e questo è il
    /// comportamento che il guasto 11 chiedeva.
    KeptRunning { message: String },
    /// Costruito, ma il programma nuovo non è partito. Va distinto da
    /// `KeptRunning`: lì c'è ancora qualcosa da guardare, qui no, e chiamarli
    /// con lo stesso nome farebbe dire «tutto bene, uso la versione vecchia» a
    /// chi non ha più nessuna versione.
    StartFailed { message: String },
}

/// **PRIMA SI COSTRUISCE, POI SI SOSTITUISCE.**
///
/// L'ordine è tutto il contenuto di questa funzione, ed è l'inverso di quello
/// di `tauri-cli`. Invertirlo di nuovo — fermare `running` prima di chiamare
/// `build` — rimette il guasto 11 esattamente com'era, e le prove di questo
/// crate diventano rosse su quella mutazione: è così che sono state verificate.
///
/// `start` non viene nemmeno chiamata quando la costruzione fallisce: non c'è
/// niente di nuovo da accendere, e chiamarla vorrebbe dire far ripartire il
/// binario **vecchio** facendolo passare per nuovo.
pub fn rebuild_then_swap<R: Running>(
    running: &mut Option<R>,
    build: impl FnOnce() -> BuildOutcome,
    start: impl FnOnce() -> Result<R, String>,
) -> Rebuild {
    match build() {
        BuildOutcome::Failed { message } => Rebuild::KeptRunning { message },
        BuildOutcome::Succeeded => {
            // Da qui in poi si tocca ciò che è acceso, e si può farlo solo
            // perché il binario nuovo esiste già sul disco.
            if let Some(previous) = running.as_mut() {
                if let Err(error) = previous.stop() {
                    // Non si torna indietro: il binario è cambiato sotto, e
                    // tenere acceso qualcosa che non si riesce a fermare
                    // lascerebbe due programmi sulla stessa porta.
                    return Rebuild::StartFailed {
                        message: format!("il programma acceso non si è fermato: {error}"),
                    };
                }
            }
            *running = None;
            match start() {
                Ok(fresh) => {
                    *running = Some(fresh);
                    Rebuild::Replaced
                }
                Err(message) => Rebuild::StartFailed { message },
            }
        }
    }
}

/// In che stato è la modalità viva, per chi la guarda da dentro la finestra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveState {
    /// Si sta ricostruendo. Quello che si vede è ancora la versione di prima.
    Building,
    /// Quello che si vede è l'ultima versione costruita.
    Running,
    /// La ricostruzione è fallita. **Quello che si vede è vecchio**, e chi
    /// guarda deve saperlo: senza questo stato la finestra mentirebbe per
    /// omissione, che il guasto 30 ha già pagato una volta.
    BuildFailed,
}

/// Lo stato che il supervisore pubblica e la finestra legge.
///
/// **PERCHÉ UN FILE E NON UN CANALE.** Chi deve leggere questo messaggio è il
/// programma **già acceso** — quello costruito *prima* che il supervisore
/// partisse, o prima ancora. Non esiste nessun canale che il supervisore possa
/// aprire verso un processo che è nato senza saperlo; un file in un posto
/// concordato invece lo leggono tutti e due, e sopravvive al fatto che uno dei
/// due muoia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveStatus {
    pub state: LiveState,
    /// Vuoto quando va tutto bene; l'uscita del compilatore quando no.
    pub message: String,
    pub changed_at: i64,
    /// Da quando è acceso il programma che si sta guardando. Con
    /// `state: build_failed` è la risposta a «quanto è vecchio quello che
    /// vedo», che è la sola cosa che chi guarda vuole sapere in quel momento.
    pub running_since: Option<i64>,
}

/// Come si chiama il file, sotto la casa di Sailor.
pub const STATUS_FILE: &str = "live-status.json";

/// La porta del servitore di sviluppo della finestra.
///
/// **È la porta del guasto 4**, quella che un orfano teneva occupata due volte
/// in una notte. Sta scritta anche in `desktop/src-tauri/tauri.conf.json` come
/// `devUrl`, e due copie di un numero divergono: `the_dev_port_matches_the_tauri_config`
/// le confronta, perché un registro che dichiara la porta sbagliata manda chi
/// cerca l'orfano a guardare altrove.
pub const DEV_PORT: u16 = 5183;

impl LiveStatus {
    /// Dove sta il file di stato, data la casa di Sailor.
    pub fn path_in(home: &Path) -> PathBuf {
        home.join(STATUS_FILE)
    }

    /// **SCRITTURA INTERA O NIENTE.** Il lettore è un altro processo che legge
    /// quando vuole: scrivendo sul posto lo si sorprenderebbe a metà file, e un
    /// JSON troncato lo farebbe sembrare assente proprio nell'istante in cui
    /// c'è un errore da mostrare.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("creare {}: {error}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("comporre lo stato: {error}"))?;
        let temporary = path.with_extension("json.partial");
        std::fs::write(&temporary, text)
            .map_err(|error| format!("scrivere {}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("spostare su {}: {error}", path.display()))?;
        Ok(())
    }

    /// **UN FILE ASSENTE O ROTTO NON È UN ERRORE, È UN «NON SO».** Chi legge è
    /// la finestra, e una finestra che muore perché il file di stato è a metà
    /// sarebbe il guasto 11 rifatto da questa parte.
    pub fn read(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }
}

/// Un processo che il deposito dà per acceso, e cosa ne dice il sistema.
#[derive(Debug, Clone)]
pub struct LeftRunning {
    pub record: ledger::ProcessRecord,
    /// **Due domande diverse, tenute separate apposta.** Il deposito dice cosa
    /// è stato avviato; questo campo dice se quel pid respira ancora. Fonderle
    /// vorrebbe dire chiedere al sistema operativo l'elenco, che è la strada su
    /// cui aspetta il guasto 12.
    pub still_alive: bool,
}

/// Cosa è rimasto acceso dall'ultima volta, con la conferma pid per pid.
pub fn left_running(store: &ledger::Ledger) -> Result<Vec<LeftRunning>, ledger::LedgerError> {
    Ok(store
        .processes_left_running()?
        .into_iter()
        .map(|record| LeftRunning {
            still_alive: ledger::pid_is_alive(record.pid),
            record,
        })
        .collect())
}

/// Chiude nel deposito le voci di processi che non respirano più.
///
/// **SERVE PERCHÉ IL DEPOSITO NON PUÒ VEDERE UNA MORTE VIOLENTA.** Un processo
/// ucciso da fuori — o insieme al terminale che lo teneva — non scrive la
/// propria chiusura, e resta «acceso» per sempre. Senza questa passata l'elenco
/// si riempie di fantasmi, e un elenco pieno di fantasmi lo si smette di
/// leggere: è il modo esatto in cui un registro dei processi smette di
/// impedire il guasto 4.
///
/// Restituisce quanti ne ha chiusi.
pub fn close_the_ones_that_stopped_breathing(
    store: &ledger::Ledger,
    now: i64,
) -> Result<usize, ledger::LedgerError> {
    let mut closed = 0;
    for gone in left_running(store)?.into_iter().filter(|item| !item.still_alive) {
        store.record_process_ended(&ledger::ProcessEndRecord {
            process_id: gone.record.process_id,
            // Non si inventa un codice d'uscita: nessuno l'ha visto uscire.
            exit_code: None,
            ended_at: now,
        })?;
        closed += 1;
    }
    Ok(closed)
}

/// Adesso, in secondi dall'epoca.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}
