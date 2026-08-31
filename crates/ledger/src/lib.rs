//! Il deposito durevole di Sailor.
//!
//! `events.db` contiene la verità append-only; `state.db` contiene quattro
//! proiezioni interrogabili e un segno dell'ultimo evento incorporato. I due
//! file sono collegati, ma il nuovo evento e la sua proiezione vengono commessi
//! in due fasi perché WAL non offre atomicità fra database collegati.

use flow::{AttemptRelation, Completion, Outcome, RecordStore, Spend, StepRecord, StepSpecies};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

const STATE_FILE: &str = "state.db";
const EVENTS_FILE: &str = "events.db";

/// Dove vive il deposito di questa macchina.
///
/// **STA QUI PERCHÉ È UNA SOLA.** Era una funzione privata di `sailor flow`, e
/// finché a leggere il deposito c'era un comando solo bastava. Adesso lo legge
/// anche la staffetta, per sapere su che lavoro si è: due copie di questo
/// percorso divergerebbero al primo che cambia idea, e nessuna delle due
/// direbbe di essere sbagliata — semplicemente una delle due aprirebbe un
/// deposito vuoto e risponderebbe «non lo so».
///
/// `None` quando `HOME` non è definita: chi chiama ha sempre un ripiego, e
/// nessuno deve dedurre una casa che l'ambiente non dichiara.
///
/// IL 28/08/2026 QUESTA FUNZIONE HA QUASI AVUTO UNA GEMELLA, e il commento qui
/// sopra aveva previsto cosa sarebbe successo. Una modifica aveva insegnato alla
/// finestra a cercare il deposito altrove, lasciando qui il percorso vecchio:
/// chi esegue i flussi avrebbe scritto in una casa e chi li guarda avrebbe letto
/// nell'altra, **senza che nessuna delle due dicesse di sbagliare**. Adesso la
/// scoperta della casa vive qui, dove tutti già passano.
pub fn default_directory() -> Option<PathBuf> {
    if let Some(declared) = env_path("SAILOR_LEDGER") {
        return Some(declared);
    }
    // LA CASA DI CHI C'ERA PRIMA. Su una macchina dove Sailor ha già girato, il
    // deposito sta dove lo metteva la versione vecchia, e spostare il
    // predefinito lo renderebbe invisibile: le corse ci sono, la finestra
    // direbbe «nessuna». Si riconosce dai due file, non dalla cartella: una
    // cartella vuota rimasta lì non è un'installazione.
    //
    // Questo gradino è una migrazione, non una casa: si toglie quando il
    // deposito vecchio sarà stato spostato, e chi lo toglie deve prima
    // spostarlo.
    if let Some(home) = env_path("HOME") {
        let previous = home.join(".claude/state/flussi");
        if previous.join(STATE_FILE).exists() && previous.join(EVENTS_FILE).exists() {
            return Some(previous);
        }
    }
    Some(sailor_home()?.join("ledger"))
}

/// La casa di Sailor: dove vivono deposito, flussi e configurazione.
///
/// **Nessun percorso di una persona sola.** Si scopre come la scopre qualunque
/// programma su questo sistema: `SAILOR_HOME` se dichiarata, altrimenti la
/// cartella di configurazione standard, altrimenti quella dell'utente che
/// esegue. `None` se l'ambiente non dichiara nemmeno quella — una casa dedotta
/// senza fondamento manderebbe a scrivere nel posto di qualcun altro.
pub fn sailor_home() -> Option<PathBuf> {
    Some(sailor_home_in(
        env_path("SAILOR_HOME"),
        env_path("XDG_CONFIG_HOME"),
        env_path("HOME")?,
    ))
}

/// La stessa regola, applicata a un ambiente dichiarato invece che a quello di
/// questo processo.
///
/// **Esiste perché la casa era in due posti.** Fino al 30/08/2026 questa regola
/// stava scritta due volte: qui, e dentro chi cerca i descrittori su una
/// macchina *descritta* (`toolbox::default_sources`, `trigger::default_sources`).
/// La seconda copia ignorava `XDG_CONFIG_HOME` e cadeva su `~/.sailor` invece
/// che su `~/.config/sailor`, così il listino dei prezzi e i descrittori
/// dell'utente finivano in due case diverse — e la documentazione mandava tutti
/// nella casa che il codice del listino non guarda. Chi cerca la casa la chiede
/// qui, chiunque sia.
pub fn sailor_home_in(
    declared: Option<PathBuf>,
    xdg_config: Option<PathBuf>,
    home: PathBuf,
) -> PathBuf {
    if let Some(declared) = declared {
        return declared;
    }
    if let Some(config) = xdg_config {
        return config.join("sailor");
    }
    home.join(".config").join("sailor")
}

/// Una variabile d'ambiente come percorso. La stringa vuota vale come «non
/// impostata»: è quello che lascia dietro uno script che esporta una variabile
/// senza valore, e prenderla alla lettera manderebbe a scrivere nella radice.
fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// La forma delle proiezioni che questo codice si aspetta.
///
/// **VA ALZATA INSIEME ALLE COLONNE, E IL 30/08/2026 NON LO È STATA.** Quel
/// giorno `add_missing_projection_columns` ha imparato le quattro colonne della
/// cache scritta, e questa costante è rimasta a 4: un deposito già esistente
/// resta registrato alla 4, `4 < 4` è falso, la migrazione non parte, e ogni
/// lettura muore con «no such column: cache_write_tokens».
///
/// **NESSUNA DELLE 517 PROVE L'HA PRESO**, perché un deposito creato in una
/// prova nasce dal `CREATE TABLE` completo e non passa mai dalla migrazione. Si
/// vede solo su una macchina che Sailor l'aveva già usato — cioè su quella di
/// chi lo sviluppa, il giorno dopo.
const PROJECTION_SCHEMA_VERSION: i64 = 7;

#[derive(Debug)]
pub enum LedgerError {
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    InvalidRecord(String),
    DuplicateAttempt { step: String, attempt: u32 },
    MissingAttempt { step: String, attempt: u32 },
    AlreadyClosed { step: String, attempt: u32 },
    StaleEpoch { step: String, epoch: u64 },
    Poisoned,
}

impl fmt::Display for LedgerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "sqlite: {error}"),
            Self::Json(error) => write!(formatter, "json: {error}"),
            Self::InvalidRecord(message) => write!(formatter, "invalid record: {message}"),
            Self::DuplicateAttempt { step, attempt } => {
                write!(formatter, "duplicate attempt {attempt} for step {step}")
            }
            Self::MissingAttempt { step, attempt } => {
                write!(formatter, "missing attempt {attempt} for step {step}")
            }
            Self::AlreadyClosed { step, attempt } => {
                write!(
                    formatter,
                    "attempt {attempt} for step {step} is already closed"
                )
            }
            Self::StaleEpoch { step, epoch } => {
                write!(formatter, "epoch {epoch} for step {step} is stale")
            }
            Self::Poisoned => write!(formatter, "ledger mutex poisoned"),
        }
    }
}

impl std::error::Error for LedgerError {}

impl From<rusqlite::Error> for LedgerError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for LedgerError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub kind: String,
    pub entity: String,
    pub parent_run_id: Option<String>,
    pub started_by: String,
    pub status: String,
    pub total_cost_micros: i64,
    pub error: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

/// Una voce vista da una scansione dell'inventario.
///
/// I campi sono testo e basta: il deposito **non** dipende dal crate che li
/// produce, e non deve. Se un giorno l'inventario impara a riconoscere una
/// famiglia nuova, qui non cambia niente — mentre un `enum` condiviso
/// obbligherebbe a migrare il deposito per ogni parola nuova.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryItem {
    pub kind: String,
    pub name: String,
    pub origin: String,
    pub path: String,
    /// `active`, `inactive` o `unknown`.
    pub reach: String,
    /// Perché non è raggiungibile, quando non lo è.
    pub reason: Option<String>,
}

/// Una scansione intera, con il suo istante.
///
/// SI DEPOSITA LA SCANSIONE, NON LA SINGOLA VOCE, e la differenza è tutto ciò
/// che rende utile il deposito: **da un elenco completo si sa anche che cosa
/// non c'è più**. Registrando voce per voce si saprebbe solo che cosa è stato
/// visto, e «sparito» resterebbe indistinguibile da «non ancora guardato» —
/// cioè proprio la domanda per cui questo deposito esiste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryScan {
    pub taken_at: i64,
    pub items: Vec<InventoryItem>,
}

/// Che cosa è cambiato per una voce fra due scansioni.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryChange {
    pub kind: String,
    pub name: String,
    pub path: String,
    pub origin: String,
    pub reach: String,
    pub reason: Option<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    /// L'istante della scansione in cui è sparita, se è sparita.
    pub gone_at: Option<i64>,
}

/// Una chiamata a un modello, con quanto ha consumato e quanto è costata.
///
/// **QUI `None` VUOL DIRE «NON LO SO», E NON ESISTE UNO ZERO DI RIPIEGO.** I
/// conteggi e i prezzi erano numeri secchi finché a scrivere questa riga erano
/// solo le prove, che il numero se lo inventavano. Da quando lo scrive il
/// motore che invoca davvero una riga di comando, la differenza fra «zero
/// token» e «quel motore non dice quanti token ha usato» è la differenza fra
/// una misura e una bugia: uno zero scritto al posto di «non lo so» si somma, e
/// nessuna vista a valle può più correggerlo. Chi legge una riga con i
/// conteggi a `None` sa di avere una chiamata non misurata, e quello è un
/// fatto su cui si può agire.
///
/// I campi `Option` portano `serde(default)` perché il deposito è a eventi: un
/// evento scritto quando erano numeri secchi continua a leggersi — `10`
/// diventa `Some(10)` — e uno scritto dopo, senza il campo, diventa `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallRecord {
    pub call_id: String,
    pub run_id: String,
    pub step_id: Option<String>,
    pub purpose: String,
    pub cli: String,
    pub requested_model: String,
    pub actual_model: String,
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    /// I token d'ingresso letti dalla cache, in una colonna loro: hanno un
    /// prezzo per milione tutto loro, spesso un ordine di grandezza sotto
    /// quello dell'ingresso fresco.
    #[serde(default)]
    pub cached_tokens: Option<u64>,
    /// I token d'ingresso **scritti** nella cache, che non sono quelli letti e
    /// non costano come loro: scrivere costa più dell'ingresso normale.
    ///
    /// **QUESTA COLONNA È NATA DA UNA MISURA, IL 30/08/2026.** Una chiamata con
    /// due token d'ingresso e quattro d'uscita è costata 0,1285 dollari
    /// dichiarati dal motore: 12.347 token scritti in cache erano il 96% di
    /// quella cifra. Senza una colonna dove metterli, ogni riga di questa
    /// tabella sottostimava la spesa di 24 volte — e sempre verso il basso.
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    /// I token scritti in una cache **a lunga durata**, dove il fornitore ne
    /// offre più d'una e le fa pagare diversamente.
    #[serde(default)]
    pub cache_write_long_tokens: Option<u64>,
    /// Il totale, per i motori che dicono **solo** quello senza separare i due
    /// lati. Senza questo campo l'unica misura vera che quei motori danno
    /// verrebbe buttata via per non saperla spezzare in tre.
    #[serde(default)]
    pub total_tokens: Option<u64>,
    /// **QUANTI TURNI HA FATTO QUESTA CHIAMATA.** Non e' una curiosita': su una
    /// misura del 31/08/2026 una catena di quattro passi ha letto per turno
    /// l'8% in piu' di una sessione sola che faceva lo stesso lavoro, e ha
    /// consumato il doppio -- perche' ha fatto il doppio dei turni. Il numero
    /// che spiega il conto di un flusso e' questo, e fino a ora non era in
    /// nessuna colonna: chi voleva far costare meno una catena stava lavorando
    /// su una quantita' che nessuno misurava.
    #[serde(default)]
    pub turns: Option<u64>,
    pub cost_micros: Option<i64>,
    /// Il costo che il motore ha dichiarato di suo, tenuto **accanto** a
    /// quello del listino e mai al posto suo: se un giorno i due divergono
    /// sistematicamente, quella divergenza è essa stessa l'informazione. Un
    /// costo che arriva dallo stesso posto da cui arriva la spesa non è una
    /// verifica di niente.
    #[serde(default)]
    pub declared_cost_micros: Option<i64>,
    #[serde(default)]
    pub price_currency: Option<String>,
    #[serde(default)]
    pub input_price_micros_per_million: Option<i64>,
    #[serde(default)]
    pub output_price_micros_per_million: Option<i64>,
    #[serde(default)]
    pub cached_price_micros_per_million: Option<i64>,
    /// Il prezzo applicato ai token **scritti** in cache, e quello della cache a
    /// lunga durata. Stanno sulla riga come gli altri: un costo si deve poter
    /// rifare a mano leggendo la riga, senza sapere quale listino c'era.
    #[serde(default)]
    pub cache_write_price_micros_per_million: Option<i64>,
    #[serde(default)]
    pub cache_write_long_price_micros_per_million: Option<i64>,
    pub mandate_name: String,
    pub mandate_version: String,
    pub retry_chain: Vec<String>,
    pub error_type: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    /// **LA SESSIONE SOTTO CUI QUESTA CHIAMATA È GIRATA, QUANDO SI SA QUAL È.**
    ///
    /// Non è un dato decorativo: è la sola cosa che permette a un passo dopo di
    /// **riprendere** invece di riscoprire. Il 31/08/2026 una catena di quattro
    /// passi ha letto 2.545.109 token dalla cache per guardare lo stesso albero
    /// quattro volte; il rimedio è che il secondo passo continui la sessione del
    /// primo, e per continuarla bisogna sapere come si chiama.
    ///
    /// **PERCHÉ NEL DEPOSITO E NON IN MEMORIA.** Una variabile in memoria muore
    /// col processo, e una corsa sospesa — il ramo «aspettare» di un motore
    /// esaurito, che `docs/piano-consumo-e-profili.md` lascia scoperto — deve
    /// poter riprendere domani mattina da un altro processo. Se lo stato che
    /// permette la ripresa non è registrato, non c'è nessuna ripresa: c'è un
    /// rifacimento con un altro nome.
    ///
    /// **`None` VUOL DIRE «NON LO SO», COME OVUNQUE QUI DENTRO**, e ne esistono
    /// tre casi diversi che portano tutti allo stesso valore perché a valle si
    /// comportano allo stesso modo: il motore non sa aprire sessioni; il passo
    /// non ne ha chiesta una; oppure il passo ha **ramificato**, e il motore ha
    /// coniato per il ramo un identificativo che non ci ha detto. L'ultimo è il
    /// più insidioso, e scrivere lì l'identificativo del padre sarebbe una
    /// bugia: chi lo riprendesse ripartirebbe dal tronco credendo di essere sul
    /// ramo, in silenzio.
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRecord {
    pub snapshot_id: String,
    pub run_id: String,
    pub step_id: Option<String>,
    pub phase: String,
    pub before: Value,
    pub after: Value,
    pub created_at: i64,
}

/// Un fatto che un flusso vuole ricordare, in una collezione che ha nominato lui.
///
/// **LO SPAZIO È DEL FLUSSO, NON DEL MOTORE.** La prima stesura di questo pezzo,
/// il 28/08/2026, era una tabella `current_mandate` con le sue colonne: un
/// concetto di dominio scolpito in Rust, cioè lo stesso difetto per cui `notte`
/// è condannata — un flusso di quattro passi diventato un programma di 2.562
/// righe perché ogni cosa che doveva ricordare si è fatta la sua struttura.
/// Theo l'ha fermata: *«dovrebbe esistere disegnato, non hardcodato»*.
///
/// Qui invece il motore offre **lo spazio**, e chi lo riempie decide cosa
/// significa: `collection` è un nome che sceglie il flusso, `key` la voce
/// dentro quel nome, `value` un JSON qualunque. Il motore non sa e non deve
/// sapere che esiste una cosa chiamata «mandato».
///
/// **Perché non tabelle SQL vere, create dal file di flusso.** Sarebbe DDL
/// arbitrario preso da un file di dati, dentro il processo che tiene i freni —
/// la stessa porta che la pietra miliare §2 chiude quando vieta un interprete
/// qui dentro. Una collezione dà la stessa libertà senza aprirla: chi legge
/// vede le sue voci e nessun'altra, e nessuno può cambiare la forma del
/// deposito scrivendo un file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreRecord {
    /// Lo spazio dei nomi, scelto da chi scrive il flusso.
    pub collection: String,
    /// La voce dentro quella collezione.
    pub key: String,
    /// Cosa vale, nella forma che il flusso ha deciso.
    pub value: Value,
    /// Chi l'ha scritto: il flusso, la corsa, o una persona.
    pub written_by: String,
    pub written_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedStep {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub epoch: u64,
    pub failure_class: String,
    pub ended_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatesChangedStep {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub epoch: u64,
}

/// Una corsa con almeno un passo aperto, come la vede chi riprende.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnfinishedRun {
    pub run_id: String,
    /// Su cosa lavorava la corsa. Vuota se nessuno l'ha mai registrata.
    pub entity: String,
    pub open_steps: usize,
    pub oldest_started_at: i64,
}

/// Una corsa ferma perché aspetta qualcuno.
///
/// **NON PORTA `open_steps`, E L'ASSENZA È UN'AFFERMAZIONE.** Una corsa in
/// attesa non ha passi aperti: quello consegnato è chiuso con esito `Waiting`.
/// Un campo che dicesse sempre zero farebbe credere a chi legge che la corsa sia
/// stata abbandonata a metà, che è la storia sbagliata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingRun {
    pub run_id: String,
    /// Su quale flusso. Vuota se nessuno l'ha mai registrata.
    pub entity: String,
    /// Da quando aspetta: l'istante in cui la corsa si è fermata, o quello in
    /// cui è partita se non si è ancora fermata.
    pub waiting_since: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardedOutputStep {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    pub epoch: u64,
    pub bytes_seen: u64,
    pub bytes_discarded: u64,
}

// ── com'è andata: le risposte che un flusso può ricevere sul proprio storico ──
//
// **NESSUNA DI QUESTE STRUTTURE PORTA `input` O `output`, E NON È UNA
// DIMENTICANZA.** Da qui passa lo storico verso un'azione che qualunque flusso
// può nominare, e `input`/`output` sono il canale dati tipato: ci transitano
// prompt, ambienti e risposte di modelli. Tenerli fuori dai *tipi* invece che
// da una proiezione fa sì che nessuna distrazione futura in `actions` possa
// farli uscire: non c'è un campo da dimenticare di togliere. `said` esce da un
// varco solo, `said_of_failed_steps`, legato a una corsa nominata.

/// Quante volte un passo si è rotto, e con quale classe di guasto.
///
/// `attempts` è il denominatore: tre guasti su tre tentativi e tre su duecento
/// sono la stessa cifra e non la stessa cosa.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepFailureTally {
    pub attempts: i64,
    pub failures: i64,
    /// Le corse toccate, che non sono i guasti: un passo può rompersi più
    /// volte nella stessa corsa, un tentativo per volta.
    pub runs_affected: i64,
    pub by_class: Vec<FailureClassCount>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureClassCount {
    /// `None` è un passo rotto che il motore non ha saputo classificare, e va
    /// distinto da una classe che si chiama «sconosciuta»: qui manca il dato.
    pub failure_class: Option<String>,
    pub failures: i64,
    pub runs_affected: i64,
}

/// Una corsa **chiusa**, passo per passo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedRun {
    pub run_id: String,
    pub entity: String,
    pub status: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub steps: Vec<StepOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    pub step_id: String,
    pub attempt: u32,
    /// `None` è un passo rimasto aperto dentro una corsa già chiusa.
    pub outcome: Option<String>,
    pub failure_class: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub bytes_seen: Option<i64>,
    pub bytes_discarded: Option<i64>,
}

/// Quanto ci mette un passo, misurato sui soli tentativi riusciti.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepDurations {
    /// Secondi interi, già ordinati: chi riassume non deve riordinare, e chi
    /// legge la mediana non deve fidarsi che qualcuno l'abbia fatto.
    pub seconds_sorted: Vec<i64>,
    pub last_seconds: Option<i64>,
    /// I tentativi rotti, contati ma **non** misurati: un guasto veloce
    /// abbasserebbe la mediana e farebbe sembrare rapido un passo lento.
    pub failed_samples: i64,
}

/// Il testo grezzo di un passo rotto, come lo si consegna a chi diagnostica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaidExcerpt {
    pub step_id: String,
    pub attempt: u32,
    pub said: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "record", rename_all = "snake_case")]
enum StoredEvent {
    RunRecorded(RunRecord),
    StepStarted(StepRecord),
    StepClosed(StepRecord),
    ModelCallRecorded(ModelCallRecord),
    SnapshotRecorded(SnapshotRecord),
    InventoryScanned(InventoryScan),
    RecordWritten(StoreRecord),
    Trace(TraceRecord),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraceRecord {
    level: String,
    target: String,
    fields: Value,
    occurred_at: i64,
}

#[derive(Clone)]
pub struct Ledger {
    connection: Arc<Mutex<Connection>>,
    directory: Arc<PathBuf>,
}

impl Ledger {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, LedgerError> {
        let directory = directory.as_ref();
        std::fs::create_dir_all(directory).map_err(|error| {
            LedgerError::InvalidRecord(format!("create ledger directory: {error}"))
        })?;
        let state_path = directory.join(STATE_FILE);
        let events_path = directory.join(EVENTS_FILE);
        let connection = Connection::open(state_path)?;
        connection.busy_timeout(BUSY_TIMEOUT)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.execute(
            "ATTACH DATABASE ?1 AS events",
            [events_path.to_string_lossy().as_ref()],
        )?;
        connection.pragma_update(Some("events"), "journal_mode", "WAL")?;
        connection.pragma_update(Some("events"), "synchronous", "NORMAL")?;
        let mut connection = connection;
        {
            let transaction = immediate(&mut connection)?;
            create_event_schema(&transaction)?;
            let projection_schema_version: i64 =
                transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if !(0..=PROJECTION_SCHEMA_VERSION).contains(&projection_schema_version) {
                return Err(LedgerError::InvalidRecord(format!(
                    "unsupported projection schema version {projection_schema_version}"
                )));
            }
            // La creazione delle tabelle, l'adeguamento delle colonne e la nascita
            // degli indici sono tre fasi distinte: un deposito nuovo crea tutto subito,
            // un deposito vecchio deve prima aggiungere le colonne mancanti affinché
            // gli indici possano agganciarsi, e un deposito aggiornato non tocca nulla.
            create_projection_tables(&transaction)?;
            initialize_projection_watermark(&transaction)?;
            if projection_schema_version < PROJECTION_SCHEMA_VERSION {
                add_missing_projection_columns(&transaction)?;
                transaction.pragma_update(None, "user_version", PROJECTION_SCHEMA_VERSION)?;
            }
            create_projection_indexes(&transaction)?;
            apply_pending_events(&transaction)?;
            transaction.commit()?;
        }
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            directory: Arc::new(directory.to_path_buf()),
        })
    }

    pub fn directory(&self) -> &Path {
        self.directory.as_path()
    }

    pub fn record_run(&self, record: &RunRecord) -> Result<(), LedgerError> {
        self.write_event(StoredEvent::RunRecorded(record.clone()))
    }

    pub fn record_model_call(&self, record: &ModelCallRecord) -> Result<(), LedgerError> {
        self.write_event(StoredEvent::ModelCallRecorded(record.clone()))
    }

    pub fn record_snapshot(&self, record: &SnapshotRecord) -> Result<(), LedgerError> {
        self.write_event(StoredEvent::SnapshotRecorded(record.clone()))
    }

    /// Deposita una scansione dell'inventario.
    pub fn record_inventory(&self, scan: &InventoryScan) -> Result<(), LedgerError> {
        self.write_event(StoredEvent::InventoryScanned(scan.clone()))
    }

    /// Scrive una voce nella collezione che il flusso ha nominato.
    ///
    /// Collezione e chiave non possono essere vuote: sono l'indirizzo, e una
    /// voce senza indirizzo la ritrova solo chi già sa dov'è.
    pub fn put_record(&self, record: &StoreRecord) -> Result<(), LedgerError> {
        if record.collection.trim().is_empty() {
            return Err(LedgerError::InvalidRecord("record collection is empty".into()));
        }
        if record.key.trim().is_empty() {
            return Err(LedgerError::InvalidRecord("record key is empty".into()));
        }
        self.write_event(StoredEvent::RecordWritten(record.clone()))
    }

    /// Che cosa vale una voce, se qualcuno l'ha scritta.
    ///
    /// `None` non è un guasto: è una voce che nessuno ha ancora scritto, e chi
    /// legge deve avere un ripiego invece di fermarsi. Un deposito che
    /// inventasse un valore plausibile sarebbe peggio del non sapere.
    pub fn read_record(
        &self,
        collection: &str,
        key: &str,
    ) -> Result<Option<StoreRecord>, LedgerError> {
        let connection = self.lock()?;
        let found = connection
            .query_row(
                "SELECT collection, key, value, written_by, written_at
                 FROM store WHERE collection = ?1 AND key = ?2",
                params![collection, key],
                read_store_row,
            )
            .optional()?;
        Ok(found)
    }

    /// Tutte le voci di una collezione, per chi la vuole mostrare intera.
    pub fn records_in(&self, collection: &str) -> Result<Vec<StoreRecord>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT collection, key, value, written_by, written_at
             FROM store WHERE collection = ?1 ORDER BY key",
        )?;
        let rows = statement.query_map(params![collection], read_store_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Le voci sparite: c'erano, e l'ultima scansione non le ha più viste.
    ///
    /// È la domanda che un elenco calcolato ogni volta non sa porsi. Serve
    /// prima di cancellare qualunque cosa: una voce sparita da ieri è un
    /// cambiamento da capire, una sparita da un mese è spazzatura già morta.
    pub fn inventory_gone(&self) -> Result<Vec<InventoryChange>, LedgerError> {
        self.inventory_where("gone_at IS NOT NULL", "gone_at DESC, kind, name")
    }

    /// Le voci apparse dopo un certo istante — «che cosa è cambiato da ieri».
    pub fn inventory_new_since(&self, since: i64) -> Result<Vec<InventoryChange>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT kind, name, path, origin, reach, reason, first_seen, last_seen, gone_at
             FROM inventory_items WHERE first_seen >= ?1 AND gone_at IS NULL
             ORDER BY first_seen DESC, kind, name",
        )?;
        let rows = statement.query_map(params![since], read_inventory_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Tutto ciò che c'è adesso, come lo ha visto l'ultima scansione.
    pub fn inventory_present(&self) -> Result<Vec<InventoryChange>, LedgerError> {
        self.inventory_where("gone_at IS NULL", "kind, name")
    }

    fn inventory_where(
        &self,
        condition: &str,
        order: &str,
    ) -> Result<Vec<InventoryChange>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT kind, name, path, origin, reach, reason, first_seen, last_seen, gone_at
             FROM inventory_items WHERE {condition} ORDER BY {order}"
        ))?;
        let rows = statement.query_map([], read_inventory_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn append_step_started(&self, record: &StepRecord) -> Result<(), LedgerError> {
        validate_started(record)?;
        let mut connection = self.lock()?;
        let transaction = immediate(&mut connection)?;
        apply_pending_events(&transaction)?;
        let existing: Option<(u32, String)> = transaction
            .query_row(
                "SELECT attempt, epoch FROM steps
                 WHERE run_id = ?1 AND step_id = ?2
                 ORDER BY epoch DESC LIMIT 1",
                params![record.run_id, record.step_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if existing.is_some_and(|(_, epoch)| padded_u64(record.epoch) <= epoch) {
            return Err(LedgerError::StaleEpoch {
                step: record.step_id.clone(),
                epoch: record.epoch,
            });
        }
        let duplicate: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM steps
             WHERE run_id = ?1 AND step_id = ?2 AND attempt = ?3)",
            params![record.run_id, record.step_id, record.attempt],
            |row| row.get(0),
        )?;
        if duplicate {
            return Err(LedgerError::DuplicateAttempt {
                step: record.step_id.clone(),
                attempt: record.attempt,
            });
        }
        test_pause_after_step_read();
        append_event(&transaction, &StoredEvent::StepStarted(record.clone()))?;
        transaction.commit()?;
        apply_pending(&mut connection)?;
        Ok(())
    }

    pub fn close_step(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), LedgerError> {
        let mut connection = self.lock()?;
        let transaction = immediate(&mut connection)?;
        apply_pending_events(&transaction)?;
        let greatest_epoch: Option<String> = transaction.query_row(
            "SELECT MAX(epoch) FROM steps WHERE run_id = ?1 AND step_id = ?2",
            params![run_id, step_id],
            |row| row.get(0),
        )?;
        if greatest_epoch.as_deref() != Some(padded_u64(epoch).as_str()) {
            return Err(LedgerError::StaleEpoch {
                step: step_id.to_owned(),
                epoch,
            });
        }
        let mut record =
            read_step(&transaction, run_id, step_id, attempt, epoch)?.ok_or_else(|| {
                LedgerError::MissingAttempt {
                    step: step_id.to_owned(),
                    attempt,
                }
            })?;
        if record.outcome.is_some() {
            return Err(LedgerError::AlreadyClosed {
                step: step_id.to_owned(),
                attempt,
            });
        }
        record.outcome = Some(completion.outcome);
        record.output = completion.output;
        record.said = completion.said.map(flow::truncate_said);
        record.failure_class = completion.failure_class;
        record.ended_at = Some(completion.ended_at);
        record.bytes_seen = completion.bytes_seen;
        record.bytes_discarded = completion.bytes_discarded;
        append_event(&transaction, &StoredEvent::StepClosed(record.clone()))?;
        transaction.commit()?;
        test_crash_after_close_event();
        apply_pending(&mut connection)?;
        Ok(())
    }

    pub fn steps(&self, run_id: &str) -> Result<Vec<StepRecord>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT run_id, step_id, attempt, epoch, deps, input_digest, input,
                    gates, attempt_relation, started_at, outcome, output, said,
                    failure_class, ended_at, bytes_seen, bytes_discarded,
                    held_by_pid, species
             FROM steps WHERE run_id = ?1 ORDER BY started_at, step_id, attempt",
        )?;
        let records = statement
            .query_map([run_id], step_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn steps_resumed_with_changed_gates(&self) -> Result<Vec<GatesChangedStep>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT run_id, step_id, attempt, epoch FROM steps
             WHERE attempt_relation = 'same_input_gates_changed'
             ORDER BY started_at, run_id, step_id, attempt",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(GatesChangedStep {
                run_id: row.get(0)?,
                step_id: row.get(1)?,
                attempt: row.get(2)?,
                epoch: u64_column(row, 3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Le corse rimaste a metà: hanno un'intenzione scritta e nessun esito.
    ///
    /// È LA DOMANDA DELLA RIPRESA, e non nomina nessuna lavorazione: chi
    /// riprende non sa quali corse esistano, sa solo di voler chiudere ciò che
    /// è rimasto aperto. Prima di questo metodo il nome della corsa andava
    /// ricostruito da fuori — nel servizio notturno, con la data di oggi — e
    /// chi si era interrotto prima di mezzanotte non veniva ritrovato mai.
    ///
    /// L'ordine è quello di apertura: si riprende ciò che è fermo da più tempo.
    pub fn unfinished_runs(&self) -> Result<Vec<UnfinishedRun>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT steps.run_id, COALESCE(runs.entity, ''), COUNT(*), MIN(steps.started_at)
             FROM steps LEFT JOIN runs ON runs.run_id = steps.run_id
             WHERE steps.outcome IS NULL
             GROUP BY steps.run_id
             ORDER BY MIN(steps.started_at), steps.run_id",
        )?;
        let rows = statement.query_map([], |row| {
            let open: i64 = row.get(2)?;
            Ok(UnfinishedRun {
                run_id: row.get(0)?,
                entity: row.get(1)?,
                open_steps: open as usize,
                oldest_started_at: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// L'intestazione di una corsa, se ne esiste una con quel nome.
    ///
    /// **SERVE A RITROVARE IL FLUSSO DA CUI UNA CORSA È NATA.** Chi riprende una
    /// corsa ha in mano il suo identificativo e nient'altro: senza `entity` non
    /// sa quale grafo caricare, e con il grafo sbagliato validerebbe l'uscita di
    /// un passo contro lo schema di un altro. `None` non è un guasto — è una
    /// corsa che nessuno ha registrato, e chi chiede deve poterlo distinguere da
    /// un deposito rotto.
    pub fn run_header(&self, run_id: &str) -> Result<Option<RunRecord>, LedgerError> {
        let connection = self.lock()?;
        let found = connection
            .query_row(
                "SELECT run_id, kind, entity, parent_run_id, started_by, status,
                        total_cost_micros, error, started_at, ended_at
                 FROM runs WHERE run_id = ?1",
                params![run_id],
                |row| {
                    Ok(RunRecord {
                        run_id: row.get(0)?,
                        kind: row.get(1)?,
                        entity: row.get(2)?,
                        parent_run_id: row.get(3)?,
                        started_by: row.get(4)?,
                        status: row.get(5)?,
                        total_cost_micros: row.get(6)?,
                        error: row.get(7)?,
                        started_at: row.get(8)?,
                        ended_at: row.get(9)?,
                    })
                },
            )
            .optional()?;
        Ok(found)
    }

    /// Le corse ferme in attesa di una persona o di un agente.
    ///
    /// **NON È `unfinished_runs` CON UN ALTRO FILTRO, ED È IL PUNTO DI TUTTO.**
    /// Quella domanda cerca i passi **aperti** — `steps.outcome IS NULL` — cioè
    /// un'intenzione scritta senza esito. Un passo consegnato non è così: è
    /// **chiuso**, con esito `Waiting`, perché chi doveva eseguirlo non è un
    /// processo di cui si aspetta la morte. Fino al 31/08/2026 nessuna
    /// interrogazione trovava quelle corse: una consegna che nessuno raccoglieva
    /// spariva, e l'unico modo di ritrovarla era ricordarsene.
    ///
    /// Niente migrazione: `runs.status` è testo libero e `waiting` ci viene già
    /// scritto da `execution_status`.
    ///
    /// L'ordine è quello dell'attesa: si guarda per prima quella ferma da più
    /// tempo.
    pub fn waiting_runs(&self) -> Result<Vec<WaitingRun>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT run_id, entity, COALESCE(ended_at, started_at)
             FROM runs WHERE status = 'waiting'
             ORDER BY COALESCE(ended_at, started_at), run_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(WaitingRun {
                run_id: row.get(0)?,
                entity: row.get(1)?,
                waiting_since: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Quando ciascuna entità ha cominciato l'ultima volta.
    ///
    /// Serve a sapere se un flusso pianificato è dovuto adesso, e la domanda è
    /// **quando è partito**, non quando è finito: una corsa ancora in volo ha
    /// già consumato il suo turno, e contarla come «mai girata» la farebbe
    /// ripartire sopra se stessa.
    pub fn last_started_at(&self) -> Result<BTreeMap<String, i64>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT entity, MAX(started_at) FROM runs WHERE entity <> '' GROUP BY entity",
        )?;
        let rows = statement.query_map([], |row| {
            let entity: String = row.get(0)?;
            let started: i64 = row.get(1)?;
            Ok((entity, started))
        })?;
        Ok(rows.collect::<Result<BTreeMap<_, _>, _>>()?)
    }

    pub fn steps_with_discarded_output(&self) -> Result<Vec<DiscardedOutputStep>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT run_id, step_id, attempt, epoch, bytes_seen, bytes_discarded
             FROM steps
             WHERE bytes_discarded > 0
             ORDER BY started_at, run_id, step_id, attempt",
        )?;
        let rows = statement.query_map([], |row| {
            let seen: i64 = row.get(4)?;
            let discarded: i64 = row.get(5)?;
            Ok(DiscardedOutputStep {
                run_id: row.get(0)?,
                step_id: row.get(1)?,
                attempt: row.get(2)?,
                epoch: u64_column(row, 3)?,
                bytes_seen: seen as u64,
                bytes_discarded: discarded as u64,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn is_checkpointed(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
    ) -> Result<bool, LedgerError> {
        let connection = self.lock()?;
        Ok(connection
            .query_row(
                "SELECT checkpointed FROM steps
                 WHERE run_id = ?1 AND step_id = ?2 AND attempt = ?3",
                params![run_id, step_id, attempt],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    pub fn failed_steps_in_recent_runs(
        &self,
        failure_class: &str,
        run_limit: usize,
    ) -> Result<Vec<FailedStep>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "WITH recent AS (
                 SELECT run_id FROM runs ORDER BY started_at DESC LIMIT ?1
             )
             SELECT s.run_id, s.step_id, s.attempt, s.epoch,
                    s.failure_class, s.ended_at
             FROM steps s JOIN recent r ON r.run_id = s.run_id
             WHERE s.failure_class = ?2 AND s.outcome = 'Broke'
             ORDER BY s.ended_at DESC",
        )?;
        let rows = statement.query_map(params![run_limit as i64, failure_class], |row| {
            Ok(FailedStep {
                run_id: row.get(0)?,
                step_id: row.get(1)?,
                attempt: row.get(2)?,
                epoch: u64_column(row, 3)?,
                failure_class: row.get(4)?,
                ended_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Quante corse il deposito conosce, comunque le conosca.
    ///
    /// **SERVE A DISTINGUERE «NON C'È NIENTE» DA «ZERO GUASTI».** Su una
    /// macchina appena installata ogni conteggio è zero, e uno zero senza
    /// questo numero accanto è indistinguibile da una macchina che gira da
    /// mesi senza rompere niente — cioè da una bugia. Conta l'unione delle due
    /// tabelle apposta: una corsa i cui passi sono registrati ma la cui
    /// intestazione non lo è resta una corsa avvenuta, e dire «nessuna» a chi
    /// ha lo storico sotto gli occhi sarebbe la stessa bugia al contrario.
    pub fn recorded_runs(&self) -> Result<i64, LedgerError> {
        let connection = self.lock()?;
        Ok(connection.query_row(
            "SELECT COUNT(*) FROM (SELECT run_id FROM runs UNION SELECT run_id FROM steps)",
            [],
            |row| row.get(0),
        )?)
    }

    /// Quanto una corsa ha speso finora, e su quante chiamate non si sa.
    ///
    /// **IL SECONDO NUMERO NON È UN ORNAMENTO: È CIÒ CHE RENDE IL PRIMO
    /// LEGGIBILE.** Un motore che non dichiara i propri token lascia la riga
    /// col costo a `NULL` — codex fa esattamente questo, dichiara un totale e
    /// non i due lati, e da un totale non si ricava un costo senza inventare la
    /// proporzione. Sommare solo i costi noti dà quindi una **sottostima
    /// sistematica**, e un tetto di spesa che si fidasse di quella somma da
    /// sola lascerebbe passare corse che hanno speso il doppio. Chi legge questa
    /// struttura ha entrambi i numeri e può decidere; chi ne guardasse uno solo
    /// starebbe leggendo una rassicurazione.
    ///
    /// Una corsa senza nessuna chiamata risponde con tutti zeri, ed è la
    /// risposta giusta: non ha speso niente **e** non c'è niente che non si sa.
    pub fn spent_in_run(&self, run_id: &str) -> Result<Spend, LedgerError> {
        let connection = self.lock()?;
        // `MAX` su una colonna dove ogni riga è `NULL` risponde `NULL`, e
        // `Option<i64>` lo porta fino a chi decide: «la più cara è sconosciuta»
        // non è «la più cara è zero».
        let (micros, calls, calls_without_cost, dearest_micros) = connection.query_row(
            "SELECT COALESCE(SUM(cost_micros), 0),
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN cost_micros IS NULL THEN 1 ELSE 0 END), 0),
                    MAX(cost_micros)
             FROM model_calls WHERE run_id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        Ok(Spend {
            micros,
            calls,
            calls_without_cost,
            dearest_micros,
        })
    }

    /// La sessione che un passo di questa corsa ha aperto **su quel motore**.
    ///
    /// È la domanda che rende possibile «riprendi la sessione del passo prima»:
    /// il passo dopo nomina il passo, non l'identificativo, perché
    /// l'identificativo nasce a tempo di esecuzione e chi scrive il flusso non
    /// lo può conoscere.
    ///
    /// **IL MOTORE FA PARTE DELLA DOMANDA, E TOGLIERLO SAREBBE UN GUASTO
    /// SILENZIOSO.** Un passo con una catena di motori può essere finito su
    /// `codex` perché `claude-code` aveva esaurito la quota: dare al passo dopo
    /// una sessione di `claude-code` da riprendere con `codex` gli farebbe
    /// passare un identificativo che quel motore non conosce, e la chiamata
    /// morirebbe **dopo** essere partita — cioè dopo aver speso.
    ///
    /// L'ultima per inizio, non la prima: un passo rifatto ne ha aperte due, e
    /// quella buona è la più recente.
    pub fn session_opened_by(
        &self,
        run_id: &str,
        step_id: &str,
        cli: &str,
    ) -> Result<Option<String>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT session_id FROM model_calls
             WHERE run_id = ?1 AND step_id = ?2 AND cli = ?3 AND session_id IS NOT NULL
             ORDER BY started_at DESC LIMIT 1",
        )?;
        let mut rows = statement.query(params![run_id, step_id, cli])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Quante corse cadono davvero nella finestra chiesta.
    ///
    /// Chi riceve un conteggio deve sapere su quanto è stato calcolato: una
    /// finestra di cinquanta corse su un deposito che ne ha tre non è una
    /// finestra di cinquanta, e senza questo numero «zero guasti nelle ultime
    /// cinquanta» suonerebbe come una rassicurazione che nessuno ha misurato.
    pub fn runs_in_window(&self, flow: Option<&str>, limit: usize) -> Result<i64, LedgerError> {
        let connection = self.lock()?;
        Ok(connection.query_row(
            "SELECT COUNT(*) FROM (SELECT run_id FROM runs
             WHERE (?1 IS NULL OR entity = ?1)
             ORDER BY started_at DESC LIMIT ?2)",
            params![flow, limit as i64],
            |row| row.get(0),
        )?)
    }

    /// Quante volte un passo si è rotto nella finestra, e come.
    ///
    /// **IL FILTRO PER FLUSSO PASSA DALLA GIUNZIONE CON `runs`**: `steps` non
    /// sa a quale flusso appartiene, lo sa solo l'intestazione della corsa. Il
    /// prezzo è dichiarato — i passi di corse mai registrate in `runs` restano
    /// fuori dalla finestra — e il prezzo opposto sarebbe peggio: rispondere
    /// sulla somma di tutti i flussi a chi ne ha nominato uno.
    pub fn step_failure_tally(
        &self,
        step_id: &str,
        flow: Option<&str>,
        within_last_runs: usize,
    ) -> Result<StepFailureTally, LedgerError> {
        let connection = self.lock()?;
        let limit = within_last_runs as i64;
        let (attempts, failures, runs_affected) = connection.query_row(
            "WITH recent AS (
                 SELECT run_id FROM runs
                 WHERE (?1 IS NULL OR entity = ?1)
                 ORDER BY started_at DESC LIMIT ?2
             )
             SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN s.outcome = 'Broke' THEN 1 ELSE 0 END), 0),
                    COUNT(DISTINCT CASE WHEN s.outcome = 'Broke' THEN s.run_id END)
             FROM steps s JOIN recent r ON r.run_id = s.run_id
             WHERE s.step_id = ?3",
            params![flow, limit, step_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let mut statement = connection.prepare(
            "WITH recent AS (
                 SELECT run_id FROM runs
                 WHERE (?1 IS NULL OR entity = ?1)
                 ORDER BY started_at DESC LIMIT ?2
             )
             SELECT s.failure_class, COUNT(*), COUNT(DISTINCT s.run_id)
             FROM steps s JOIN recent r ON r.run_id = s.run_id
             WHERE s.step_id = ?3 AND s.outcome = 'Broke'
             GROUP BY s.failure_class
             ORDER BY COUNT(*) DESC, s.failure_class",
        )?;
        let by_class = statement
            .query_map(params![flow, limit, step_id], read_failure_class_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(StepFailureTally {
            attempts,
            failures,
            runs_affected,
            by_class,
        })
    }

    /// Le classi di guasto più frequenti, dalla più frequente in giù.
    pub fn failure_class_tally(
        &self,
        flow: Option<&str>,
        within_last_runs: usize,
    ) -> Result<Vec<FailureClassCount>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "WITH recent AS (
                 SELECT run_id FROM runs
                 WHERE (?1 IS NULL OR entity = ?1)
                 ORDER BY started_at DESC LIMIT ?2
             )
             SELECT s.failure_class, COUNT(*), COUNT(DISTINCT s.run_id)
             FROM steps s JOIN recent r ON r.run_id = s.run_id
             WHERE s.outcome = 'Broke'
             GROUP BY s.failure_class
             ORDER BY COUNT(*) DESC, s.failure_class",
        )?;
        let rows = statement.query_map(
            params![flow, within_last_runs as i64],
            read_failure_class_row,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// L'ultima corsa **chiusa** di un flusso, passo per passo.
    ///
    /// **CHIUSA, NON RECENTE**, e la differenza non è di gusto: un flusso che
    /// interroga il proprio storico mentre gira è lui stesso la corsa più
    /// recente, e rispondergli con se stesso a metà gli darebbe un esito che
    /// non è ancora successo. `ended_at IS NOT NULL` esclude chi sta chiedendo
    /// per costruzione, senza che l'azione debba sapere il proprio nome.
    pub fn last_finished_run(&self, flow: &str) -> Result<Option<FinishedRun>, LedgerError> {
        let connection = self.lock()?;
        let head: Option<(String, String, String, i64, i64)> = connection
            .query_row(
                "SELECT run_id, entity, status, started_at, ended_at FROM runs
                 WHERE entity = ?1 AND ended_at IS NOT NULL
                 ORDER BY started_at DESC, run_id DESC LIMIT 1",
                params![flow],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((run_id, entity, status, started_at, ended_at)) = head else {
            return Ok(None);
        };
        let mut statement = connection.prepare(
            "SELECT step_id, attempt, outcome, failure_class, started_at, ended_at,
                    bytes_seen, bytes_discarded
             FROM steps WHERE run_id = ?1
             ORDER BY started_at, step_id, attempt",
        )?;
        let steps = statement
            .query_map(params![run_id], |row| {
                Ok(StepOutcome {
                    step_id: row.get(0)?,
                    attempt: row.get(1)?,
                    outcome: row.get(2)?,
                    failure_class: row.get(3)?,
                    started_at: row.get(4)?,
                    ended_at: row.get(5)?,
                    bytes_seen: row.get(6)?,
                    bytes_discarded: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(FinishedRun {
            run_id,
            entity,
            status,
            started_at,
            ended_at,
            steps,
        }))
    }

    /// Quanto ci ha messo un passo, tentativo riuscito per tentativo riuscito.
    ///
    /// I tentativi rotti si contano a parte invece di entrare nelle durate:
    /// un guasto immediato è veloce, e mescolarlo alle riuscite risponderebbe
    /// «va più svelto del solito» a un passo che ha smesso di funzionare.
    pub fn step_durations(
        &self,
        step_id: &str,
        flow: Option<&str>,
        within_last_runs: usize,
    ) -> Result<StepDurations, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "WITH recent AS (
                 SELECT run_id FROM runs
                 WHERE (?1 IS NULL OR entity = ?1)
                 ORDER BY started_at DESC LIMIT ?2
             )
             SELECT s.outcome, s.ended_at - s.started_at, s.ended_at
             FROM steps s JOIN recent r ON r.run_id = s.run_id
             WHERE s.step_id = ?3 AND s.ended_at IS NOT NULL
             ORDER BY s.ended_at DESC",
        )?;
        let rows = statement.query_map(params![flow, within_last_runs as i64, step_id], |row| {
            let outcome: Option<String> = row.get(0)?;
            let seconds: i64 = row.get(1)?;
            Ok((outcome, seconds))
        })?;
        let mut durations = StepDurations::default();
        for row in rows {
            let (outcome, seconds) = row?;
            match outcome.as_deref() {
                Some("Went") => {
                    // Le righe arrivano dalla più recente: la prima riuscita è
                    // «l'ultima volta», ed è quella che chi chiede confronta.
                    if durations.last_seconds.is_none() {
                        durations.last_seconds = Some(seconds);
                    }
                    durations.seconds_sorted.push(seconds);
                }
                Some("Broke") => durations.failed_samples += 1,
                // Saltato, fermato o in attesa: né una riuscita da misurare né
                // un guasto da contare. Tacerne è più onesto che classificarli.
                _ => {}
            }
        }
        durations.seconds_sorted.sort_unstable();
        Ok(durations)
    }

    /// Il testo grezzo dei passi rotti di **una** corsa nominata.
    ///
    /// **È UN VARCO, ED È SCRITTO COME UN VARCO.** `said` è l'unica cosa che
    /// esce di ciò che è passato dentro un flusso, e potrebbe contenere
    /// qualunque cosa un modello abbia detto. Accetta una corsa sola, un tetto
    /// di passi e un tetto di byte proprio perché nessuna sequenza di domande
    /// possa rastrellare lo storico un pezzo per volta: un metodo che
    /// accettasse una finestra di corse sarebbe la fuga di dati che
    /// l'interrogazione dello storico deve evitare, con l'aspetto di una
    /// comodità.
    pub fn said_of_failed_steps(
        &self,
        run_id: &str,
        max_steps: usize,
        max_bytes: usize,
    ) -> Result<Vec<SaidExcerpt>, LedgerError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT step_id, attempt, said FROM steps
             WHERE run_id = ?1 AND outcome = 'Broke' AND said IS NOT NULL
             ORDER BY ended_at, step_id, attempt
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![run_id, max_steps as i64], |row| {
            let step_id: String = row.get(0)?;
            let attempt: u32 = row.get(1)?;
            let said: String = row.get(2)?;
            Ok((step_id, attempt, said))
        })?;
        let mut excerpts = Vec::new();
        for row in rows {
            let (step_id, attempt, said) = row?;
            let (said, truncated) = clip_to_bytes(said, max_bytes);
            excerpts.push(SaidExcerpt {
                step_id,
                attempt,
                said,
                truncated,
            });
        }
        Ok(excerpts)
    }

    pub fn rebuild_projections(&self) -> Result<(), LedgerError> {
        let mut connection = self.lock()?;
        let transaction = immediate(&mut connection)?;
        rebuild_projections_in(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn projection_dump(&self) -> Result<Value, LedgerError> {
        let connection = self.lock()?;
        let tables = ["runs", "steps", "model_calls", "snapshots"];
        let mut dump = serde_json::Map::new();
        for table in tables {
            dump.insert(table.to_owned(), dump_table(&connection, table)?);
        }
        Ok(Value::Object(dump))
    }

    fn write_event(&self, event: StoredEvent) -> Result<(), LedgerError> {
        let mut connection = self.lock()?;
        let transaction = immediate(&mut connection)?;
        apply_pending_events(&transaction)?;
        append_event(&transaction, &event)?;
        transaction.commit()?;
        apply_pending(&mut connection)?;
        Ok(())
    }

    fn append_trace(&self, trace: TraceRecord) -> Result<(), LedgerError> {
        self.write_event(StoredEvent::Trace(trace))
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, LedgerError> {
        self.connection.lock().map_err(|_| LedgerError::Poisoned)
    }
}

/// **PRENDE `&self` PERCHÉ IL DEPOSITO ERA GIÀ PRONTO A RICEVERE PIÙ FILI.** La
/// connessione sta dietro un `Arc<Mutex<_>>` da sempre, `append_step_started` e
/// `close_step` lavorano già su `&self`, e ogni scrittura è già una transazione
/// `BEGIN IMMEDIATE` con cinque secondi di attesa se un altro la sta tenendo. A
/// bloccare l'esecuzione insieme di due passi non era il deposito: era la firma
/// del tratto, che chiedeva una mutabilità che nessuno usava.
impl RecordStore for Ledger {
    fn append_started(&self, record: StepRecord) -> Result<(), flow::FlowError> {
        self.append_step_started(&record)
            .map_err(|error| flow::FlowError::Store(error.to_string()))
    }

    fn close(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), flow::FlowError> {
        self.close_step(run_id, step_id, attempt, epoch, completion)
            .map_err(|error| flow::FlowError::Store(error.to_string()))
    }

    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, flow::FlowError> {
        self.steps(run_id)
            .map_err(|error| flow::FlowError::Store(error.to_string()))
    }

    /// Il deposito le chiamate le tiene, quindi risponde per davvero.
    fn spent(&self, run_id: &str) -> Result<Spend, flow::FlowError> {
        self.spent_in_run(run_id)
            .map_err(|error| flow::FlowError::Store(error.to_string()))
    }
}

#[derive(Clone)]
pub struct SqliteLayer {
    ledger: Ledger,
    clock: Arc<dyn Fn() -> i64 + Send + Sync>,
}

impl SqliteLayer {
    pub fn new(ledger: Ledger) -> Self {
        Self::with_clock(ledger, || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs() as i64)
        })
    }

    pub fn with_clock(ledger: Ledger, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        Self {
            ledger,
            clock: Arc::new(clock),
        }
    }
}

impl<S> Layer<S> for SqliteLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut visitor = JsonVisitor::default();
        event.record(&mut visitor);
        let metadata = event.metadata();
        // `Layer` non può restituire l'errore al chiamante; un guasto del ponte
        // resta fuori dal percorso critico e non deve fermare il lavoro tracciato.
        let _ = self.ledger.append_trace(TraceRecord {
            level: metadata.level().to_string(),
            target: metadata.target().to_owned(),
            fields: Value::Object(visitor.fields),
            occurred_at: (self.clock)(),
        });
    }
}

#[derive(Default)]
struct JsonVisitor {
    fields: serde_json::Map<String, Value>,
}

impl Visit for JsonVisitor {
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(field.name().to_owned(), value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(field.name().to_owned(), value.into());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.insert(field.name().to_owned(), value.into());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_owned(), value.to_owned().into());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}").into());
    }
}

fn immediate(connection: &mut Connection) -> Result<Transaction<'_>, LedgerError> {
    Ok(connection.transaction_with_behavior(TransactionBehavior::Immediate)?)
}

fn apply_pending(connection: &mut Connection) -> Result<(), LedgerError> {
    let transaction = immediate(connection)?;
    apply_pending_events(&transaction)?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
fn test_pause_after_step_read() {
    let Some(marker) = std::env::var_os("LEDGER_TEST_STEP_READ_MARKER") else {
        return;
    };
    std::fs::write(marker, b"ready").expect("scrivere il segnale della prova");
    let hold = std::env::var("LEDGER_TEST_STEP_READ_HOLD_MILLIS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    std::thread::sleep(Duration::from_millis(hold));
}

#[cfg(not(test))]
fn test_pause_after_step_read() {}

#[cfg(test)]
fn test_crash_after_close_event() {
    if std::env::var_os("LEDGER_TEST_CRASH_AFTER_CLOSE_EVENT").is_some() {
        // L'evento è durevole, il watermark no: l'apertura deve incorporarlo
        // prima che il passo possa apparire rilanciabile.
        std::process::exit(86);
    }
}

#[cfg(not(test))]
fn test_crash_after_close_event() {}

fn create_event_schema(connection: &Connection) -> Result<(), LedgerError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS events.events (
             seq INTEGER PRIMARY KEY AUTOINCREMENT,
             kind TEXT NOT NULL,
             run_id TEXT,
             step_id TEXT,
             attempt INTEGER,
             epoch TEXT,
             occurred_at INTEGER,
             payload TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS events.events_run_idx
             ON events(run_id, seq);
         CREATE INDEX IF NOT EXISTS events.events_step_idx
             ON events(run_id, step_id, attempt, seq);
         -- «Che cosa è successo fra le due e le tre» era l'unica delle domande
         -- previste che nessun indice serviva: si leggeva il registro intero.
         -- Misurato il 28/08/2026 su un registro finto da un milione di eventi,
         -- accendendo e spegnendo questo indice: 81,93 ms di scansione contro
         -- 0,05 ms, cioè **1.640 volte**, per 2,8% di spazio in più.
         -- Alle 112 voci di oggi non si sente; si sentirà, e allora l'indice
         -- c'è già — aggiungerlo dopo vuol dire aggiungerlo quando qualcuno si
         -- è già chiesto perché il cruscotto ci mette.
         CREATE INDEX IF NOT EXISTS events.events_time_idx
             ON events(occurred_at);
         CREATE TRIGGER IF NOT EXISTS events.events_append_only_update
             BEFORE UPDATE ON events
             BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;
         CREATE TRIGGER IF NOT EXISTS events.events_append_only_delete
             BEFORE DELETE ON events
             BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;",
    )?;
    Ok(())
}

fn create_projection_schema(connection: &Connection) -> Result<(), LedgerError> {
    create_projection_tables(connection)?;
    create_projection_indexes(connection)?;
    Ok(())
}

fn create_projection_tables(connection: &Connection) -> Result<(), LedgerError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS runs (
             run_id TEXT PRIMARY KEY,
             kind TEXT NOT NULL,
             entity TEXT NOT NULL,
             parent_run_id TEXT,
             started_by TEXT NOT NULL,
             status TEXT NOT NULL,
             total_cost_micros INTEGER NOT NULL,
             error TEXT,
             started_at INTEGER NOT NULL,
             ended_at INTEGER
         );
         CREATE TABLE IF NOT EXISTS steps (
             run_id TEXT NOT NULL,
             step_id TEXT NOT NULL,
             attempt INTEGER NOT NULL,
             epoch TEXT NOT NULL,
             deps TEXT NOT NULL,
             input_digest TEXT NOT NULL,
             input TEXT NOT NULL,
             gates TEXT NOT NULL,
             attempt_relation TEXT,
             held_by_pid INTEGER,
             species TEXT,
             started_at INTEGER NOT NULL,
             outcome TEXT,
             output TEXT,
             said TEXT,
             failure_class TEXT,
             ended_at INTEGER,
             bytes_seen INTEGER,
             bytes_discarded INTEGER,
             checkpointed INTEGER NOT NULL DEFAULT 0 CHECK(checkpointed IN (0, 1)),
             PRIMARY KEY (run_id, step_id, attempt)
         );
         CREATE TABLE IF NOT EXISTS model_calls (
             call_id TEXT PRIMARY KEY,
             run_id TEXT NOT NULL,
             step_id TEXT,
             purpose TEXT NOT NULL,
             cli TEXT NOT NULL,
             requested_model TEXT NOT NULL,
             actual_model TEXT NOT NULL,
             input_tokens TEXT,
             output_tokens TEXT,
             cached_tokens TEXT,
             cost_micros INTEGER,
             price_currency TEXT,
             input_price_micros_per_million INTEGER,
             output_price_micros_per_million INTEGER,
             cached_price_micros_per_million INTEGER,
             mandate_name TEXT NOT NULL,
             mandate_version TEXT NOT NULL,
             retry_chain TEXT NOT NULL,
             error_type TEXT,
             started_at INTEGER NOT NULL,
             ended_at INTEGER,
             total_tokens TEXT,
             declared_cost_micros INTEGER,
             cache_write_tokens TEXT,
             cache_write_long_tokens TEXT,
             cache_write_price_micros_per_million INTEGER,
             cache_write_long_price_micros_per_million INTEGER,
             turns TEXT,
             session_id TEXT
         );
         CREATE TABLE IF NOT EXISTS snapshots (
             snapshot_id TEXT PRIMARY KEY,
             run_id TEXT NOT NULL,
             step_id TEXT,
             phase TEXT NOT NULL,
             before_state TEXT NOT NULL,
             after_state TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS inventory_items (
             kind TEXT NOT NULL,
             name TEXT NOT NULL,
             path TEXT NOT NULL,
             origin TEXT NOT NULL,
             reach TEXT NOT NULL,
             reason TEXT,
             first_seen INTEGER NOT NULL,
             last_seen INTEGER NOT NULL,
             gone_at INTEGER,
             PRIMARY KEY (kind, name, path)
         );
         CREATE TABLE IF NOT EXISTS store (
             collection TEXT NOT NULL,
             key TEXT NOT NULL,
             value TEXT NOT NULL,
             written_by TEXT NOT NULL,
             written_at INTEGER NOT NULL,
             PRIMARY KEY (collection, key)
         );
         CREATE TABLE IF NOT EXISTS projection_watermark (
             singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
             last_applied_seq INTEGER NOT NULL CHECK(last_applied_seq >= 0)
         );",
    )?;
    Ok(())
}

fn create_projection_indexes(connection: &Connection) -> Result<(), LedgerError> {
    connection.execute_batch(
        "CREATE INDEX IF NOT EXISTS runs_started_idx ON runs(started_at DESC);
         CREATE UNIQUE INDEX IF NOT EXISTS steps_epoch_idx
             ON steps(run_id, step_id, epoch);
         CREATE INDEX IF NOT EXISTS steps_failure_idx
             ON steps(failure_class, outcome, ended_at DESC);
         CREATE INDEX IF NOT EXISTS steps_attempt_relation_idx
             ON steps(attempt_relation, started_at);
         CREATE INDEX IF NOT EXISTS steps_discarded_idx
             ON steps(bytes_discarded, started_at);
         CREATE INDEX IF NOT EXISTS model_calls_run_idx
             ON model_calls(run_id, step_id, started_at);
         CREATE INDEX IF NOT EXISTS snapshots_run_idx
             ON snapshots(run_id, step_id, phase);
         CREATE UNIQUE INDEX IF NOT EXISTS snapshots_phase_idx
             ON snapshots(run_id, IFNULL(step_id, ''), phase);",
    )?;
    Ok(())
}

/// Porta la proiezione di un deposito già esistente alla versione corrente
/// aggiungendo le colonne opzionali nate dopo di lui. Non è una catena di
/// migrazioni numerate: ogni colonna si aggiunge se manca, quindi la stessa
/// funzione porta a destinazione un deposito di qualunque versione passata,
/// e rieseguirla non fa niente. Nessuna invalida le proiezioni esistenti né
/// obbliga a rileggere il registro degli eventi — i valori dei record già
/// scritti restano nulli, che è esattamente ciò che erano.
fn add_missing_projection_columns(transaction: &Transaction<'_>) -> Result<(), LedgerError> {
    for (column, kind) in [
        // versione 2
        ("bytes_seen", "INTEGER"),
        ("bytes_discarded", "INTEGER"),
        // versione 3: chi teneva il passo, e se rifarlo è sicuro
        ("held_by_pid", "INTEGER"),
        ("species", "TEXT"),
    ] {
        if !column_exists(transaction, "steps", column)? {
            transaction.execute(&format!("ALTER TABLE steps ADD COLUMN {column} {kind}"), [])?;
        }
    }
    // versione 4: i conteggi e i prezzi di una chiamata possono essere ignoti.
    relax_model_calls(transaction)?;
    // versione 5: la cache non è una sola voce. Leggerla e scriverla sono due
    // gesti con due prezzi, e quello che mancava — la scrittura — è il più caro.
    // Vanno in coda, nello stesso ordine in cui stanno nel `CREATE TABLE`: le
    // righe si scrivono per posizione.
    for (column, kind) in [
        ("cache_write_tokens", "TEXT"),
        ("cache_write_long_tokens", "TEXT"),
        ("cache_write_price_micros_per_million", "INTEGER"),
        ("cache_write_long_price_micros_per_million", "INTEGER"),
    ] {
        if !column_exists(transaction, "model_calls", column)? {
            transaction.execute(
                &format!("ALTER TABLE model_calls ADD COLUMN {column} {kind}"),
                [],
            )?;
        }
    }
    // versione 6: i turni. Si paga per turno, e nessuna colonna li contava.
    if !column_exists(transaction, "model_calls", "turns")? {
        transaction.execute("ALTER TABLE model_calls ADD COLUMN turns TEXT", [])?;
    }
    // versione 7: la sessione. Senza una colonna dove posarla, «riprendi la
    // sessione del passo prima» non si può nemmeno formulare — e la costante
    // qui sopra va alzata insieme, o su un deposito già esistente questa riga
    // non gira mai (è il guasto che il commento di `PROJECTION_SCHEMA_VERSION`
    // racconta, ed è costato una mattina il 30/08/2026).
    if !column_exists(transaction, "model_calls", "session_id")? {
        transaction.execute("ALTER TABLE model_calls ADD COLUMN session_id TEXT", [])?;
    }
    Ok(())
}

/// Rifà `model_calls` nella forma in cui i conteggi e i prezzi ammettono NULL,
/// conservando le righe già scritte.
///
/// **PERCHÉ UN RIFACIMENTO E NON UN `ALTER`.** SQLite non sa togliere un
/// `NOT NULL` da una colonna esistente: l'unica strada è creare la forma nuova,
/// copiarci dentro le righe, e sostituire la vecchia. E non si passa dalla
/// ricostruzione da eventi — che pure esiste — perché `rebuild_projections_in`
/// rifiuta un registro potato: su un deposito a cui qualcuno ha già tagliato la
/// coda degli eventi quella strada non arriva in fondo, e si porterebbe via
/// anche le righe che si stanno cercando di salvare.
///
/// Il riconoscimento passa da `total_tokens`, che nasce con questa versione:
/// se c'è, il rifacimento è già stato fatto, e rieseguire questa funzione non
/// fa niente.
fn relax_model_calls(transaction: &Transaction<'_>) -> Result<(), LedgerError> {
    if column_exists(transaction, "model_calls", "total_tokens")? {
        return Ok(());
    }
    transaction.execute_batch(
        "CREATE TABLE model_calls_relaxed (
             call_id TEXT PRIMARY KEY,
             run_id TEXT NOT NULL,
             step_id TEXT,
             purpose TEXT NOT NULL,
             cli TEXT NOT NULL,
             requested_model TEXT NOT NULL,
             actual_model TEXT NOT NULL,
             input_tokens TEXT,
             output_tokens TEXT,
             cached_tokens TEXT,
             cost_micros INTEGER,
             price_currency TEXT,
             input_price_micros_per_million INTEGER,
             output_price_micros_per_million INTEGER,
             cached_price_micros_per_million INTEGER,
             mandate_name TEXT NOT NULL,
             mandate_version TEXT NOT NULL,
             retry_chain TEXT NOT NULL,
             error_type TEXT,
             started_at INTEGER NOT NULL,
             ended_at INTEGER,
             total_tokens TEXT,
             declared_cost_micros INTEGER
         );
         INSERT INTO model_calls_relaxed (
             call_id, run_id, step_id, purpose, cli, requested_model, actual_model,
             input_tokens, output_tokens, cached_tokens, cost_micros, price_currency,
             input_price_micros_per_million, output_price_micros_per_million,
             cached_price_micros_per_million, mandate_name, mandate_version,
             retry_chain, error_type, started_at, ended_at)
         SELECT
             call_id, run_id, step_id, purpose, cli, requested_model, actual_model,
             input_tokens, output_tokens, cached_tokens, cost_micros, price_currency,
             input_price_micros_per_million, output_price_micros_per_million,
             cached_price_micros_per_million, mandate_name, mandate_version,
             retry_chain, error_type, started_at, ended_at
         FROM model_calls;
         DROP TABLE model_calls;
         ALTER TABLE model_calls_relaxed RENAME TO model_calls;",
    )?;
    Ok(())
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool, LedgerError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn initialize_projection_watermark(connection: &Connection) -> Result<(), LedgerError> {
    connection.execute(
        "INSERT OR IGNORE INTO projection_watermark (singleton, last_applied_seq) VALUES (1, 0)",
        [],
    )?;
    Ok(())
}

fn drop_projection_schema(transaction: &Transaction<'_>) -> Result<(), LedgerError> {
    transaction.execute_batch(
        "DROP TABLE IF EXISTS snapshots;
         DROP TABLE IF EXISTS model_calls;
         DROP TABLE IF EXISTS steps;
         DROP TABLE IF EXISTS runs;",
    )?;
    Ok(())
}

fn rebuild_projections_in(transaction: &Transaction<'_>) -> Result<(), LedgerError> {
    let (event_count, log_high_water): (i64, i64) = transaction.query_row(
        "SELECT COUNT(*), COALESCE((SELECT seq FROM events.sqlite_sequence
                                    WHERE name = 'events'), 0)
         FROM events.events",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if event_count != log_high_water {
        return Err(LedgerError::InvalidRecord(
            "cannot rebuild projections from a pruned event log".to_owned(),
        ));
    }
    drop_projection_schema(transaction)?;
    create_projection_schema(transaction)?;
    transaction.execute("DELETE FROM projection_watermark", [])?;
    initialize_projection_watermark(transaction)?;
    let events = {
        let mut statement =
            transaction.prepare("SELECT seq, payload FROM events.events ORDER BY seq")?;
        let events = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        events
    };
    for (seq, payload) in events {
        let event: StoredEvent = serde_json::from_str(&payload)?;
        project_event(transaction, &event)?;
        set_projection_watermark(transaction, seq)?;
    }
    transaction.pragma_update(None, "user_version", PROJECTION_SCHEMA_VERSION)?;
    Ok(())
}

fn apply_pending_events(transaction: &Transaction<'_>) -> Result<(), LedgerError> {
    let watermark: Option<i64> = transaction
        .query_row(
            "SELECT last_applied_seq FROM projection_watermark WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let watermark = watermark
        .ok_or_else(|| LedgerError::InvalidRecord("projection watermark is missing".to_owned()))?;
    let log_high_water: i64 = transaction.query_row(
        "SELECT COALESCE((SELECT seq FROM events.sqlite_sequence
                              WHERE name = 'events'), 0)",
        [],
        |row| row.get(0),
    )?;
    if watermark > log_high_water {
        return Err(LedgerError::InvalidRecord(format!(
            "projection watermark {watermark} is ahead of event log {log_high_water}"
        )));
    }
    let events = {
        let mut statement = transaction
            .prepare("SELECT seq, payload FROM events.events WHERE seq > ?1 ORDER BY seq")?;
        let events = statement
            .query_map([watermark], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        events
    };
    for (seq, payload) in events {
        test_count_applied_event_read();
        let event: StoredEvent = serde_json::from_str(&payload)?;
        project_event(transaction, &event)?;
        set_projection_watermark(transaction, seq)?;
    }
    Ok(())
}

fn set_projection_watermark(transaction: &Transaction<'_>, seq: i64) -> Result<(), LedgerError> {
    transaction.execute(
        "UPDATE projection_watermark SET last_applied_seq = ?1 WHERE singleton = 1",
        [seq],
    )?;
    Ok(())
}

fn append_event(transaction: &Transaction<'_>, event: &StoredEvent) -> Result<(), LedgerError> {
    // Da qui l'evento va in `events.db`, mentre la successiva proiezione va in
    // `state.db`. Con WAL SQLite non rende atomico il commit dei due database
    // collegati. Il nuovo evento non viene mai proiettato nella transazione che
    // lo inserisce; il watermark avanza solo nella transazione seguente. Uno
    // schianto può lasciare la proiezione indietro, mai davanti al registro, e
    // l'apertura applica soltanto la coda che manca.
    let (kind, run_id, step_id, attempt, epoch, occurred_at) = event_metadata(event);
    let payload = serde_json::to_string(event)?;
    transaction.execute(
        "INSERT INTO events.events
         (kind, run_id, step_id, attempt, epoch, occurred_at, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![kind, run_id, step_id, attempt, epoch, occurred_at, payload],
    )?;
    Ok(())
}

type EventMetadata<'a> = (
    &'static str,
    Option<&'a str>,
    Option<&'a str>,
    Option<u32>,
    Option<String>,
    Option<i64>,
);

fn event_metadata(event: &StoredEvent) -> EventMetadata<'_> {
    match event {
        StoredEvent::RunRecorded(record) => (
            "run_recorded",
            Some(&record.run_id),
            None,
            None,
            None,
            Some(record.started_at),
        ),
        StoredEvent::StepStarted(record) => (
            "step_started",
            Some(&record.run_id),
            Some(&record.step_id),
            Some(record.attempt),
            Some(padded_u64(record.epoch)),
            Some(record.started_at),
        ),
        StoredEvent::StepClosed(record) => (
            "step_closed",
            Some(&record.run_id),
            Some(&record.step_id),
            Some(record.attempt),
            Some(padded_u64(record.epoch)),
            record.ended_at,
        ),
        StoredEvent::ModelCallRecorded(record) => (
            "model_call_recorded",
            Some(&record.run_id),
            record.step_id.as_deref(),
            None,
            None,
            Some(record.started_at),
        ),
        StoredEvent::SnapshotRecorded(record) => (
            "snapshot_recorded",
            Some(&record.run_id),
            record.step_id.as_deref(),
            None,
            None,
            Some(record.created_at),
        ),
        StoredEvent::InventoryScanned(scan) => {
            ("inventory_scanned", None, None, None, None, Some(scan.taken_at))
        }
        StoredEvent::RecordWritten(record) => {
            ("record_written", None, None, None, None, Some(record.written_at))
        }
        StoredEvent::Trace(record) => ("trace", None, None, None, None, Some(record.occurred_at)),
    }
}

fn project_event(transaction: &Transaction<'_>, event: &StoredEvent) -> Result<(), LedgerError> {
    match event {
        StoredEvent::RunRecorded(record) => project_run(transaction, record),
        StoredEvent::StepStarted(record) => project_step(transaction, record, false),
        StoredEvent::StepClosed(record) => project_step(transaction, record, true),
        StoredEvent::ModelCallRecorded(record) => project_model_call(transaction, record),
        StoredEvent::SnapshotRecorded(record) => project_snapshot(transaction, record),
        StoredEvent::InventoryScanned(scan) => project_inventory(transaction, scan),
        StoredEvent::RecordWritten(record) => project_record(transaction, record),
        StoredEvent::Trace(_) => Ok(()),
    }
}

/// Una voce, un valore: l'ultima scrittura sostituisce la precedente.
///
/// La storia non si perde e non va qui: sta nel registro, dove ogni
/// `record_written` resta con la sua data e con chi l'ha scritto. Questa
/// tabella risponde a una domanda sola — *adesso*, quanto vale questa voce — e
/// una tabella che risponde a una domanda sola non può dare due risposte in
/// disaccordo.
fn project_record(
    transaction: &Transaction<'_>,
    record: &StoreRecord,
) -> Result<(), LedgerError> {
    transaction.execute(
        "INSERT INTO store (collection, key, value, written_by, written_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(collection, key) DO UPDATE SET
          value=excluded.value, written_by=excluded.written_by,
          written_at=excluded.written_at",
        params![
            record.collection,
            record.key,
            serde_json::to_string(&record.value)?,
            record.written_by,
            record.written_at,
        ],
    )?;
    Ok(())
}

fn read_failure_class_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FailureClassCount> {
    Ok(FailureClassCount {
        failure_class: row.get(0)?,
        failures: row.get(1)?,
        runs_affected: row.get(2)?,
    })
}

/// Taglia un testo a un tetto di byte senza spezzare un carattere, e dice se
/// ha tagliato. Il «se» va restituito, non dedotto dalla lunghezza: chi legge
/// una diagnosi troncata senza saperlo la legge come completa.
fn clip_to_bytes(value: String, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_owned(), true)
}

fn read_store_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoreRecord> {
    let raw: String = row.get(2)?;
    Ok(StoreRecord {
        collection: row.get(0)?,
        key: row.get(1)?,
        // Il valore è entrato come JSON e deve uscire com'è entrato. Se il testo
        // sul disco non si rilegge — un deposito toccato a mano, un file
        // troncato — si restituisce la stringa grezza invece di far cadere la
        // lettura: chi legge vede qualcosa di sbagliato e se ne accorge, mentre
        // un errore qui spegnerebbe la riga per una voce sola.
        value: serde_json::from_str(&raw).unwrap_or(Value::String(raw)),
        written_by: row.get(3)?,
        written_at: row.get(4)?,
    })
}

fn project_run(transaction: &Transaction<'_>, record: &RunRecord) -> Result<(), LedgerError> {
    transaction.execute(
        "INSERT INTO runs
         (run_id, kind, entity, parent_run_id, started_by, status,
          total_cost_micros, error, started_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(run_id) DO UPDATE SET
          kind=excluded.kind, entity=excluded.entity,
          parent_run_id=excluded.parent_run_id, started_by=excluded.started_by,
          status=excluded.status, total_cost_micros=excluded.total_cost_micros,
          error=excluded.error, started_at=excluded.started_at,
          ended_at=excluded.ended_at",
        params![
            record.run_id,
            record.kind,
            record.entity,
            record.parent_run_id,
            record.started_by,
            record.status,
            record.total_cost_micros,
            record.error,
            record.started_at,
            record.ended_at,
        ],
    )?;
    Ok(())
}

fn project_step(
    transaction: &Transaction<'_>,
    record: &StepRecord,
    checkpointed: bool,
) -> Result<(), LedgerError> {
    transaction.execute(
        "INSERT INTO steps
         (run_id, step_id, attempt, epoch, deps, input_digest, input, gates,
          attempt_relation, started_at, outcome, output, said, failure_class,
          ended_at, bytes_seen, bytes_discarded, held_by_pid, species,
          checkpointed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 ?15, ?16, ?17, ?18, ?19, ?20)
         ON CONFLICT(run_id, step_id, attempt) DO UPDATE SET
          epoch=excluded.epoch, deps=excluded.deps,
          input_digest=excluded.input_digest, input=excluded.input,
          gates=excluded.gates, attempt_relation=excluded.attempt_relation,
          started_at=excluded.started_at,
          outcome=excluded.outcome, output=excluded.output, said=excluded.said,
          failure_class=excluded.failure_class, ended_at=excluded.ended_at,
          bytes_seen=excluded.bytes_seen, bytes_discarded=excluded.bytes_discarded,
          held_by_pid=excluded.held_by_pid, species=excluded.species,
          checkpointed=excluded.checkpointed",
        params![
            record.run_id,
            record.step_id,
            record.attempt,
            padded_u64(record.epoch),
            serde_json::to_string(&record.deps)?,
            record.input_digest,
            serde_json::to_string(&record.input)?,
            serde_json::to_string(&record.gates)?,
            record.attempt_relation.map(attempt_relation_name),
            record.started_at,
            record.outcome.map(outcome_name),
            record
                .output
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            record.said,
            record.failure_class,
            record.ended_at,
            record.bytes_seen.map(|bytes| bytes as i64),
            record.bytes_discarded.map(|bytes| bytes as i64),
            record.held_by_pid.map(|pid| pid as i64),
            record.species.map(species_name),
            checkpointed,
        ],
    )?;
    Ok(())
}

fn project_model_call(
    transaction: &Transaction<'_>,
    record: &ModelCallRecord,
) -> Result<(), LedgerError> {
    transaction.execute(
        // Le colonne sono nominate una per una di proposito: un `VALUES` nudo si
        // regge sull'ordine della tabella, e la colonna aggiunta dopo — che
        // arriva sempre — la sposta senza che niente diventi rosso.
        "INSERT INTO model_calls (
             call_id, run_id, step_id, purpose, cli, requested_model, actual_model,
             input_tokens, output_tokens, cached_tokens, cost_micros, price_currency,
             input_price_micros_per_million, output_price_micros_per_million,
             cached_price_micros_per_million, mandate_name, mandate_version,
             retry_chain, error_type, started_at, ended_at, total_tokens,
             declared_cost_micros, cache_write_tokens, cache_write_long_tokens,
             cache_write_price_micros_per_million,
             cache_write_long_price_micros_per_million, turns, session_id)
         VALUES
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
          ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29)
         ON CONFLICT(call_id) DO UPDATE SET
          run_id=excluded.run_id, step_id=excluded.step_id,
          purpose=excluded.purpose, cli=excluded.cli,
          requested_model=excluded.requested_model, actual_model=excluded.actual_model,
          input_tokens=excluded.input_tokens, output_tokens=excluded.output_tokens,
          cached_tokens=excluded.cached_tokens, cost_micros=excluded.cost_micros,
          price_currency=excluded.price_currency,
          input_price_micros_per_million=excluded.input_price_micros_per_million,
          output_price_micros_per_million=excluded.output_price_micros_per_million,
          cached_price_micros_per_million=excluded.cached_price_micros_per_million,
          mandate_name=excluded.mandate_name, mandate_version=excluded.mandate_version,
          retry_chain=excluded.retry_chain, error_type=excluded.error_type,
          started_at=excluded.started_at, ended_at=excluded.ended_at,
          total_tokens=excluded.total_tokens,
          declared_cost_micros=excluded.declared_cost_micros,
          cache_write_tokens=excluded.cache_write_tokens,
          cache_write_long_tokens=excluded.cache_write_long_tokens,
          cache_write_price_micros_per_million=excluded.cache_write_price_micros_per_million,
          cache_write_long_price_micros_per_million=excluded.cache_write_long_price_micros_per_million,
          turns=excluded.turns,
          session_id=excluded.session_id",
        params![
            record.call_id,
            record.run_id,
            record.step_id,
            record.purpose,
            record.cli,
            record.requested_model,
            record.actual_model,
            // I conteggi restano colonne di testo per non perdere precisione
            // oltre 2^53; un conteggio ignoto è un NULL, non la stringa "0".
            record.input_tokens.map(|n| n.to_string()),
            record.output_tokens.map(|n| n.to_string()),
            record.cached_tokens.map(|n| n.to_string()),
            record.cost_micros,
            record.price_currency,
            record.input_price_micros_per_million,
            record.output_price_micros_per_million,
            record.cached_price_micros_per_million,
            record.mandate_name,
            record.mandate_version,
            serde_json::to_string(&record.retry_chain)?,
            record.error_type,
            record.started_at,
            record.ended_at,
            record.total_tokens.map(|n| n.to_string()),
            record.declared_cost_micros,
            record.cache_write_tokens.map(|n| n.to_string()),
            record.cache_write_long_tokens.map(|n| n.to_string()),
            record.cache_write_price_micros_per_million,
            record.cache_write_long_price_micros_per_million,
            record.turns.map(|n| n.to_string()),
            record.session_id,
        ],
    )?;
    Ok(())
}

/// Una scansione dell'inventario diventa lo stato di ciò che c'è, ciò che è
/// tornato e ciò che non c'è più.
///
/// TRE GESTI, IN QUEST'ORDINE, e l'ordine è il punto:
/// 1. ogni voce vista aggiorna `last_seen` e cancella un'eventuale sparizione —
///    una cosa che ricompare non è più sparita, e tenerne il segno la
///    mostrerebbe morta per sempre;
/// 2. le voci **non** viste in questa scansione, e non ancora marcate, prendono
///    l'istante di questa scansione come momento della sparizione;
/// 3. `first_seen` non si tocca mai dopo la prima volta: è l'unica data che
///    risponde a «da quando ce l'abbiamo», e riscriverla la perderebbe.
///
/// SI CANCELLA SOLO DOPO AVER SCRITTO, non prima: una proiezione che azzera e
/// riempie mostra un istante in cui l'inventario è vuoto, e chi legge in quel
/// momento — la pagina, un flusso — vede una macchina senza niente installato.
fn read_inventory_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InventoryChange> {
    Ok(InventoryChange {
        kind: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        origin: row.get(3)?,
        reach: row.get(4)?,
        reason: row.get(5)?,
        first_seen: row.get(6)?,
        last_seen: row.get(7)?,
        gone_at: row.get(8)?,
    })
}

fn project_inventory(
    transaction: &Transaction<'_>,
    scan: &InventoryScan,
) -> Result<(), LedgerError> {
    for item in &scan.items {
        transaction.execute(
            "INSERT INTO inventory_items
             (kind, name, path, origin, reach, reason, first_seen, last_seen, gone_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, NULL)
             ON CONFLICT(kind, name, path) DO UPDATE SET
              origin=excluded.origin, reach=excluded.reach, reason=excluded.reason,
              last_seen=excluded.last_seen, gone_at=NULL",
            params![
                item.kind,
                item.name,
                item.path,
                item.origin,
                item.reach,
                item.reason,
                scan.taken_at,
            ],
        )?;
    }
    transaction.execute(
        "UPDATE inventory_items SET gone_at = ?1
         WHERE last_seen < ?1 AND gone_at IS NULL",
        params![scan.taken_at],
    )?;
    Ok(())
}

fn project_snapshot(
    transaction: &Transaction<'_>,
    record: &SnapshotRecord,
) -> Result<(), LedgerError> {
    transaction.execute(
        "INSERT INTO snapshots
         (snapshot_id, run_id, step_id, phase, before_state, after_state, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(snapshot_id) DO UPDATE SET
          run_id=excluded.run_id, step_id=excluded.step_id, phase=excluded.phase,
          before_state=excluded.before_state, after_state=excluded.after_state,
          created_at=excluded.created_at",
        params![
            record.snapshot_id,
            record.run_id,
            record.step_id,
            record.phase,
            serde_json::to_string(&record.before)?,
            serde_json::to_string(&record.after)?,
            record.created_at,
        ],
    )?;
    Ok(())
}

fn validate_started(record: &StepRecord) -> Result<(), LedgerError> {
    if record.outcome.is_some()
        || record.output.is_some()
        || record.said.is_some()
        || record.failure_class.is_some()
        || record.ended_at.is_some()
        || record.bytes_seen.is_some()
        || record.bytes_discarded.is_some()
    {
        return Err(LedgerError::InvalidRecord(
            "a started record contains closing fields".to_owned(),
        ));
    }
    Ok(())
}

fn read_step(
    connection: &Connection,
    run_id: &str,
    step_id: &str,
    attempt: u32,
    epoch: u64,
) -> Result<Option<StepRecord>, LedgerError> {
    Ok(connection
        .query_row(
            "SELECT run_id, step_id, attempt, epoch, deps, input_digest, input,
                    gates, attempt_relation, started_at, outcome, output, said,
                    failure_class, ended_at, bytes_seen, bytes_discarded,
                    held_by_pid, species
             FROM steps
             WHERE run_id = ?1 AND step_id = ?2 AND attempt = ?3 AND epoch = ?4",
            params![run_id, step_id, attempt, padded_u64(epoch)],
            step_from_row,
        )
        .optional()?)
}

#[cfg(test)]
thread_local! {
    static APPLIED_EVENT_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn test_count_applied_event_read() {
    APPLIED_EVENT_READS.set(APPLIED_EVENT_READS.get() + 1);
}

#[cfg(not(test))]
fn test_count_applied_event_read() {}

fn step_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StepRecord> {
    let deps: String = row.get(4)?;
    let input: String = row.get(6)?;
    let gates: String = row.get(7)?;
    let attempt_relation: Option<String> = row.get(8)?;
    let outcome: Option<String> = row.get(10)?;
    let output: Option<String> = row.get(11)?;
    let bytes_seen: Option<i64> = row.get(15)?;
    let bytes_discarded: Option<i64> = row.get(16)?;
    let held_by_pid: Option<i64> = row.get(17)?;
    let species: Option<String> = row.get(18)?;
    Ok(StepRecord {
        run_id: row.get(0)?,
        step_id: row.get(1)?,
        attempt: row.get(2)?,
        epoch: u64_column(row, 3)?,
        deps: json_column(&deps, 4)?,
        input_digest: row.get(5)?,
        input: json_column(&input, 6)?,
        gates: json_column(&gates, 7)?,
        attempt_relation: attempt_relation
            .as_deref()
            .map(parse_attempt_relation)
            .transpose()?,
        started_at: row.get(9)?,
        outcome: outcome
            .as_deref()
            .map(|value| parse_outcome(value, 10))
            .transpose()?,
        output: output
            .as_deref()
            .map(|value| json_column(value, 11))
            .transpose()?,
        said: row.get(12)?,
        failure_class: row.get(13)?,
        ended_at: row.get(14)?,
        bytes_seen: bytes_seen.map(|b| b as u64),
        bytes_discarded: bytes_discarded.map(|b| b as u64),
        held_by_pid: held_by_pid.map(|pid| pid as u32),
        species: species.as_deref().map(parse_species).transpose()?,
    })
}

fn json_column<T: serde::de::DeserializeOwned>(value: &str, column: usize) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn u64_column(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    let value: String = row.get(column)?;
    value.parse().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn padded_u64(value: u64) -> String {
    format!("{value:020}")
}

fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Went => "Went",
        Outcome::Broke => "Broke",
        Outcome::Waiting => "Waiting",
        Outcome::Stopped => "Stopped",
        Outcome::Skipped => "Skipped",
    }
}

fn parse_outcome(value: &str, column: usize) -> rusqlite::Result<Outcome> {
    match value {
        "Went" => Ok(Outcome::Went),
        "Broke" => Ok(Outcome::Broke),
        "Waiting" => Ok(Outcome::Waiting),
        "Stopped" => Ok(Outcome::Stopped),
        "Skipped" => Ok(Outcome::Skipped),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown outcome {other}"),
            )
            .into(),
        )),
    }
}

fn species_name(species: StepSpecies) -> &'static str {
    match species {
        StepSpecies::Repeatable => "repeatable",
        StepSpecies::Compensable => "compensable",
        StepSpecies::HandToHuman => "hand_to_human",
    }
}

/// Una specie che il deposito non riconosce è un errore, non un ripiego
/// silenzioso: leggerla come `hand_to_human` sarebbe prudente per il singolo
/// passo e falso per chi legge la storia.
fn parse_species(value: &str) -> rusqlite::Result<StepSpecies> {
    match value {
        "repeatable" => Ok(StepSpecies::Repeatable),
        "compensable" => Ok(StepSpecies::Compensable),
        "hand_to_human" => Ok(StepSpecies::HandToHuman),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            18,
            rusqlite::types::Type::Text,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown step species {other}"),
            )
            .into(),
        )),
    }
}

fn attempt_relation_name(relation: AttemptRelation) -> &'static str {
    match relation {
        AttemptRelation::SameInput => "same_input",
        AttemptRelation::SameInputGatesChanged => "same_input_gates_changed",
        AttemptRelation::DifferentInput => "different_input",
    }
}

fn parse_attempt_relation(value: &str) -> rusqlite::Result<AttemptRelation> {
    match value {
        "same_input" => Ok(AttemptRelation::SameInput),
        "same_input_gates_changed" => Ok(AttemptRelation::SameInputGatesChanged),
        "different_input" => Ok(AttemptRelation::DifferentInput),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            8,
            rusqlite::types::Type::Text,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown attempt relation {other}"),
            )
            .into(),
        )),
    }
}

fn dump_table(connection: &Connection, table: &str) -> Result<Value, LedgerError> {
    let columns = match table {
        "runs" => "run_id,kind,entity,parent_run_id,started_by,status,total_cost_micros,error,started_at,ended_at",
        "steps" => "run_id,step_id,attempt,epoch,deps,input_digest,input,gates,attempt_relation,started_at,outcome,output,said,failure_class,ended_at,bytes_seen,bytes_discarded,held_by_pid,species,checkpointed",
        // Le due colonne nate con la versione 4 stanno in coda, e non è un
        // disordine: chi legge questo dump lo fa per posizione, e infilarle in
        // mezzo sposterebbe ogni indice a valle senza che niente se ne accorga
        // finché un token non compare al posto di un prezzo.
        "model_calls" => "call_id,run_id,step_id,purpose,cli,requested_model,actual_model,input_tokens,output_tokens,cached_tokens,cost_micros,price_currency,input_price_micros_per_million,output_price_micros_per_million,cached_price_micros_per_million,mandate_name,mandate_version,retry_chain,error_type,started_at,ended_at,total_tokens,declared_cost_micros,cache_write_tokens,cache_write_long_tokens,cache_write_price_micros_per_million,cache_write_long_price_micros_per_million,turns,session_id",
        "snapshots" => "snapshot_id,run_id,step_id,phase,before_state,after_state,created_at",
        _ => return Err(LedgerError::InvalidRecord("unknown projection".to_owned())),
    };
    let order = match table {
        "runs" => "run_id",
        "steps" => "run_id,step_id,attempt",
        "model_calls" => "call_id",
        "snapshots" => "snapshot_id",
        _ => unreachable!(),
    };
    let sql = format!("SELECT json_array({columns}) FROM {table} ORDER BY {order}");
    let mut statement = connection.prepare(&sql)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|row| row.and_then(|value| json_column(&value, 0)))
        .collect::<Result<Vec<Value>, _>>()?;
    Ok(Value::Array(rows))
}

#[cfg(test)]
mod tests;
