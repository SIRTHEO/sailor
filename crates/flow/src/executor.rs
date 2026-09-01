use crate::record::truncate_said;
use crate::{AttemptRelation, Graph, Outcome, SchemaError, Step, StepRecord, StepSpecies};
use serde_json::Value;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub type SharedState = BTreeMap<String, Value>;

/// La chiave sotto cui l'esecutore scrive l'identificativo del passo che sta
/// per partire, prima di ogni `Action::execute`.
///
/// **PERCHÉ QUI E NON NELLA FIRMA DEL TRATTO.** Un'azione che produce testo
/// mentre gira deve poter dire di chi è quel testo, altrimenti in un grafo con
/// due passi vivi nessuno lo attribuisce. `execute` non riceve il passo, e
/// aggiungerglielo toccherebbe ogni implementatore in cinque crate per un dato
/// che serve a pochi. Il prefisso `flow.` è dell'esecutore: un flusso non ci
/// scrive, e chi lo facesse si vedrebbe il valore sovrascritto a ogni passo.
pub const CURRENT_STEP: &str = "flow.step";

/// La chiave sotto cui l'esecutore scrive l'identificativo della **corsa**,
/// accanto a quello del passo e nello stesso punto.
///
/// **PERCHÉ NON ARRIVA PER COSTRUZIONE COME IL DEPOSITO.** Chi registra le
/// azioni ha in mano il deposito prima di costruire il registro, ma non ancora
/// la corsa: il `run_id` nasce dopo, quando si sta per partire. Un'azione che
/// deve attribuire a una corsa ciò che ha speso lo scopre qui, sulla stessa
/// strada e per la stessa ragione di `CURRENT_STEP` — `execute` non riceve né
/// l'uno né l'altro, e allargare la firma del tratto toccherebbe ogni
/// implementatore in cinque crate per un dato che serve a uno solo.
///
/// **E NON LO DICHIARA IL FLUSSO.** Farlo scrivere nell'ingresso di un passo
/// darebbe a un file di dati il potere di attribuire una spesa a una corsa
/// qualunque, su una misura che esiste proprio per non doversi fidare. Il
/// prefisso `flow.` è dell'esecutore: chi ci scrive si vede il valore
/// sovrascritto a ogni passo.
pub const CURRENT_RUN: &str = "flow.run";

/// La chiave sotto cui **chi lancia** scrive la radice del progetto.
///
/// **PERCHÉ LO STATO CONDIVISO E NON UN RINVIO NEL FLUSSO.** Un `{"$root": …}`
/// dovrebbe passare da `resolve_references`, che il guasto 28 ha misurato
/// essere chiamata da due azioni su nove: le altre sette riceverebbero la
/// radice come un oggetto e morirebbero, o peggio la scriverebbero letterale.
/// Lo stato condiviso arriva a **ogni** `Action::execute` per costruzione,
/// comprese le azioni che nessuno ha ancora scritto.
///
/// **PERCHÉ IL PREFISSO NON È `flow.`.** Quello è dell'esecutore, che scrive
/// passo e corsa a ogni giro. Questa non la scrive l'esecutore: la porta chi
/// lancia, prima che la corsa cominci, e resta la stessa per tutta la corsa.
/// Due prefissi diversi dicono a chi legge chi è il proprietario del dato.
///
/// **ASSENTE VUOL DIRE ASSENTE.** Nessun ripiego sulla cartella del processo:
/// è il guasto 25: un flusso che lavora dove capita senza dirlo fa danno
/// invece di fallire.
pub const WORKSPACE_ROOT: &str = "workspace.root";

/// La chiave sotto cui l'esecutore scrive il **tetto di spesa** della corsa,
/// quando la corsa ne ha uno. Assente vuol dire «nessun tetto», non zero.
///
/// **PERCHÉ UN'AZIONE DEVE POTERLO LEGGERE.** Il tetto lo fa rispettare
/// l'esecutore, e nessuna azione ha motivo di conoscerlo — tranne una: quella
/// che fa partire **un'altra corsa**. Un sotto-flusso senza tetto lanciato da
/// una corsa che ne ha uno lo annullerebbe, e basterebbe spostare la spesa
/// dentro il figlio per spendere quanto si vuole. Perché il figlio possa
/// ereditare il residuo, il residuo deve essere leggibile da dove si decide, e
/// si decide dentro l'azione.
///
/// **E NON LO DICHIARA IL FLUSSO**, per la stessa ragione di [`CURRENT_RUN`]:
/// il prefisso `flow.` è dell'esecutore, e un file di dati che potesse scrivere
/// qui alzerebbe da solo il proprio tetto.
pub const CURRENT_CAP: &str = "flow.cap_micros";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionError {
    pub class: String,
    pub said: String,
}

impl ActionError {
    pub fn new(class: impl Into<String>, said: impl Into<String>) -> Self {
        Self {
            class: class.into(),
            said: said.into(),
        }
    }
}

impl Display for ActionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.class, self.said)
    }
}

impl Error for ActionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectStatus {
    Applied(Value),
    NotApplied,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionOutcome {
    /// L'azione conosce il proprio risultato tipato.
    Went(Value),
    /// L'azione non può conoscere il risultato; non è un fallimento ritentabile.
    Waiting(String),
}

pub trait Action: Send + Sync {
    fn execute(
        &self,
        input: &Value,
        shared: &SharedState,
    ) -> Result<ActionOutcome, ActionError>;

    /// I campi di un `with` scritto a mano che questa azione **non**
    /// riconosce.
    ///
    /// **SI CHIEDE SOLO A TEMPO DI CONTROLLO, MAI MENTRE SI ESEGUE.** A tempo
    /// di esecuzione l'ingresso di un passo è l'uscita della sua dipendenza,
    /// dove i campi estranei sono la norma; nel `with` no — quello lo scrive una
    /// persona, e un campo che l'azione non conosce lì dentro è un refuso che
    /// costa una chiamata a pagamento per essere scoperto.
    ///
    /// Chi non risponde non fa dire niente a `flow check`: il valore
    /// predefinito è il silenzio, perché un'azione che non sa elencare i propri
    /// campi non deve poter accusare chi la usa.
    fn unknown_fields(&self, _declared: &Value) -> Vec<String> {
        Vec::new()
    }

    /// Un'azione senza una prova positiva non è rilanciabile automaticamente:
    /// `Unknown` conserva l'ambiguità invece di duplicare un effetto esterno.
    fn inspect_effect(
        &self,
        _record: &StepRecord,
        _shared: &SharedState,
    ) -> Result<EffectStatus, ActionError> {
        Ok(EffectStatus::Unknown("effect_not_inspectable".to_owned()))
    }

    /// Se rifare questa azione è sicuro. Chi non risponde viene consegnato a
    /// una persona: il difetto da cui si difende è duplicare un effetto già
    /// avvenuto sul mondo, e nessun valore predefinito lo può escludere al
    /// posto di chi ha scritto l'azione.
    fn species(&self) -> StepSpecies {
        StepSpecies::HandToHuman
    }

    /// Disfa l'effetto già prodotto, perché il passo possa essere rifatto.
    /// Ha senso solo per un'azione che si dichiara `Compensable`: una che lo
    /// dichiara senza scrivere questo metodo fallisce la compensazione e
    /// finisce a una persona, che è il modo giusto di far vedere l'errore.
    fn compensate(
        &self,
        _record: &StepRecord,
        _shared: &SharedState,
    ) -> Result<(), ActionError> {
        Err(ActionError::new(
            "no_compensation",
            "l'azione si dichiara compensabile ma non sa disfare il proprio effetto",
        ))
    }
}

#[derive(Default)]
pub struct ActionRegistry {
    actions: BTreeMap<String, Box<dyn Action>>,
}

impl ActionRegistry {
    pub fn register(
        &mut self,
        name: impl Into<String>,
        action: impl Action + 'static,
    ) -> Option<Box<dyn Action>> {
        self.actions.insert(name.into(), Box::new(action))
    }

    pub fn get(&self, name: &str) -> Option<&dyn Action> {
        self.actions.get(name).map(Box::as_ref)
    }

    /// I nomi registrati, in ordine.
    ///
    /// **CHI VUOLE ELENCARE LE AZIONI CHIEDE QUI, E NON TIENE UNA COPIA.** Un
    /// elenco scritto a mano accanto al registro diverge al primo che aggiunge
    /// un'azione, e nessun controllo locale lo mostra: il rapporto continua a
    /// stampare una riga plausibile e vecchia.
    pub fn names(&self) -> Vec<&str> {
        self.actions.keys().map(String::as_str).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub outcome: Outcome,
    pub output: Option<Value>,
    pub said: Option<String>,
    pub failure_class: Option<String>,
    pub ended_at: i64,
    pub bytes_seen: Option<u64>,
    pub bytes_discarded: Option<u64>,
}

/// Quanto una corsa ha speso, e quanto di quella spesa resta sconosciuto.
///
/// **STA QUI E NON NEL DEPOSITO PERCHÉ SERVE A DECIDERE, NON SOLO A MOSTRARE.**
/// Chi ferma una corsa al tetto è l'esecutore, che di depositi non sa niente:
/// chiede al proprio `RecordStore`. Il deposito vero è solo una delle risposte
/// possibili.
///
/// **NON È UN `Option<i64>`, E IL MOTIVO È IL TERZO CASO.** «Il totale, oppure
/// non lo so» ne copre due; i casi veri sono tre — non ho speso niente, ho speso
/// questo e lo so tutto, ho speso **almeno** questo. Un `Option` collassa il
/// terzo su uno degli altri, e in tutti e due i modi la cifra che resta è più
/// bassa del vero: cioè un tetto che lascia passare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Spend {
    /// La somma dei costi noti, in micro-unità di valuta.
    pub micros: i64,
    /// Le chiamate registrate per quella corsa, comunque siano andate.
    pub calls: i64,
    /// Quante di quelle non portano un costo. `micros` le esclude.
    pub calls_without_cost: i64,
    /// La più cara osservata, se se ne conosce almeno una.
    ///
    /// Serve a chi deve decidere **quanti passi aprire insieme**: con N in volo
    /// lo sforamento peggiore è N volte questa. `None` quando nessuna chiamata
    /// ha dichiarato un costo — e allora quel calcolo non si può fare, e chi lo
    /// facesse lo starebbe inventando.
    pub dearest_micros: Option<i64>,
}

impl Spend {
    /// Il totale è completo: ogni chiamata ha detto quanto è costata.
    ///
    /// Serve a chi deve **dichiarare** su cosa sta decidendo. Un tetto
    /// rispettato con questo a `false` è rispettato solo per quanto si sa.
    pub fn is_complete(&self) -> bool {
        self.calls_without_cost == 0
    }

    /// I tre casi, nella forma in cui si mostrano a una persona.
    ///
    /// **ESISTE PERCHÉ LA DISTINZIONE C'ERA GIÀ E NON ARRIVAVA A CHI LEGGE.**
    /// `Spend` documenta tre casi da quando è nato, ma il solo modo di
    /// interrogarli era `is_complete()` — un booleano che chi stampa poteva
    /// scrivere **accanto** al numero invece che **al posto** del numero. È
    /// esattamente quello che `sailor flow cost` faceva: la corsa dell'A/B del
    /// 31/08/2026 stampava «1,6674» ed era costata 7,2080, con la nota
    /// «parziale: 3 chiamate senza costo noto» una riga più sotto. Chi legge un
    /// totale legge il numero. Restituire il caso invece del booleano toglie a
    /// chi mostra la possibilità di sbagliarsi.
    pub fn reading(&self) -> CostReading {
        if !self.is_complete() {
            return CostReading::AtLeast {
                known_micros: self.micros,
                calls: self.calls,
                calls_without_cost: self.calls_without_cost,
            };
        }
        if self.micros == 0 {
            CostReading::Nothing
        } else {
            CostReading::Exact(self.micros)
        }
    }
}

/// Come si legge il totale di una spesa.
///
/// **TRE CASI, GLI STESSI CHE `Spend` DICHIARA.** «Non ho speso niente», «ho
/// speso questo e lo so tutto», «ho speso **almeno** questo». Il terzo è il
/// motivo per cui questo tipo esiste: collassarlo su uno degli altri due — un
/// `Option<i64>`, o un numero con una nota accanto — lascia in mano a chi legge
/// una cifra più bassa del vero, e su una corsa con passi consegnati «più bassa»
/// è stata 4,3 volte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostReading {
    /// Niente da mostrare: nessuna chiamata ha speso, e nessuna tace.
    Nothing,
    /// Il totale, e **è** il totale: ogni chiamata ha dichiarato il suo costo.
    Exact(i64),
    /// Un pavimento, non una somma. `known_micros` è quanto si sa; le altre due
    /// cifre dicono su quanto lavoro quel numero non ha visto niente — senza,
    /// «almeno 1,67» non si distingue da «1,67 e manca un centesimo».
    AtLeast {
        known_micros: i64,
        calls: i64,
        calls_without_cost: i64,
    },
}

/// Dove si scrive che un passo è partito e com'è finito.
///
/// **PRENDE `&self`, E NON È UN DETTAGLIO DI STILE.** Con `&mut self` un fronte
/// di passi indipendenti non si può eseguire insieme: il deposito è uno solo, e
/// un solo filo per volta potrebbe tenerlo. Chi implementa questo tratto si
/// procura da sé la mutabilità che gli serve — `Ledger` ha già la sua connessione
/// dietro un lucchetto, e le sue scritture sono già transazioni. Il tratto chiede
/// `Sync` per la stessa ragione: senza, il fronte resta una fila indiana.
pub trait RecordStore: Sync {
    /// Deve rendere durevole l'intenzione prima di restituire al chiamante.
    fn append_started(&self, record: StepRecord) -> Result<(), FlowError>;
    fn close(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), FlowError>;
    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, FlowError>;

    /// Quanto quella corsa ha speso finora.
    ///
    /// **NON HA UNA RISPOSTA PREDEFINITA, ED È VOLUTO.** La tentazione era dare
    /// al tratto un corpo che risponde `Spend::default()` — zero — così le
    /// implementazioni esistenti non cambiavano. Ma zero vuol dire «questa
    /// corsa non ha ancora speso niente», ed è un'affermazione: un tetto che la
    /// riceve da un deposito che semplicemente non tiene i costi **non scatta
    /// mai**, e non lo dice a nessuno. Chi implementa questo tratto dichiara
    /// cosa sa, anche quando la risposta onesta è «niente, perché non registro
    /// le chiamate».
    fn spent(&self, run_id: &str) -> Result<Spend, FlowError>;
}

/// Il deposito che vive in memoria, per le prove e per chi non vuole un file.
///
/// **IL LUCCHETTO C'È PERCHÉ IL FRONTE GIRA INSIEME.** Prima qui c'era un `Vec`
/// nudo e il tratto chiedeva `&mut self`: bastava, perché i passi si eseguivano
/// in fila. Da quando un fronte parte tutto insieme, due fili scrivono qui
/// dentro nello stesso istante, e la mutabilità se la procura la struttura
/// invece di chiederla al chiamante — che non potrebbe darla a entrambi.
#[derive(Debug, Default)]
pub struct InMemoryRecordStore {
    records: Mutex<Vec<StepRecord>>,
}

impl InMemoryRecordStore {
    pub fn from_records(records: Vec<StepRecord>) -> Self {
        Self {
            records: Mutex::new(records),
        }
    }

    /// Una copia di ciò che c'è dentro adesso.
    ///
    /// **RESTITUISCE UNA COPIA E NON UN RIFERIMENTO**: dietro il lucchetto non
    /// si può prestare niente che sopravviva alla presa, e prestarlo comunque
    /// vorrebbe dire lasciar leggere mentre un altro filo scrive.
    pub fn all(&self) -> Vec<StepRecord> {
        self.held().clone()
    }

    /// La presa sul contenuto. Un lucchetto avvelenato è un filo morto mentre
    /// scriveva: si prende quello che c'è invece di propagare il panico, perché
    /// qui dentro non ci sono invarianti a metà — ogni scrittura è un `push` o
    /// un campo assegnato.
    fn held(&self) -> std::sync::MutexGuard<'_, Vec<StepRecord>> {
        self.records.lock().unwrap_or_else(|held| held.into_inner())
    }
}

impl RecordStore for InMemoryRecordStore {
    fn append_started(&self, record: StepRecord) -> Result<(), FlowError> {
        let mut records = self.held();
        if record.outcome.is_some()
            || record.output.is_some()
            || record.said.is_some()
            || record.failure_class.is_some()
            || record.ended_at.is_some()
            || record.bytes_seen.is_some()
            || record.bytes_discarded.is_some()
        {
            return Err(FlowError::InvalidRecord(
                "a started record already contains closing fields".to_owned(),
            ));
        }
        let duplicate = records.iter().any(|found| {
            found.run_id == record.run_id
                && found.step_id == record.step_id
                && found.attempt == record.attempt
        });
        if duplicate {
            return Err(FlowError::DuplicateAttempt {
                step: record.step_id,
                attempt: record.attempt,
            });
        }
        let greatest_epoch = records
            .iter()
            .filter(|found| found.run_id == record.run_id && found.step_id == record.step_id)
            .map(|found| found.epoch)
            .max();
        if greatest_epoch.is_some_and(|epoch| record.epoch <= epoch) {
            return Err(FlowError::StaleEpoch {
                step: record.step_id,
                epoch: record.epoch,
            });
        }
        records.push(record);
        Ok(())
    }

    fn close(
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        mut completion: Completion,
    ) -> Result<(), FlowError> {
        let mut records = self.held();
        let greatest_epoch = records
            .iter()
            .filter(|found| found.run_id == run_id && found.step_id == step_id)
            .map(|found| found.epoch)
            .max();
        if greatest_epoch != Some(epoch) {
            return Err(FlowError::StaleEpoch {
                step: step_id.to_owned(),
                epoch,
            });
        }
        let record = records
            .iter_mut()
            .find(|found| {
                found.run_id == run_id
                    && found.step_id == step_id
                    && found.attempt == attempt
                    && found.epoch == epoch
            })
            .ok_or_else(|| FlowError::MissingAttempt {
                step: step_id.to_owned(),
                attempt,
            })?;
        if record.outcome.is_some() {
            return Err(FlowError::AlreadyClosed {
                step: step_id.to_owned(),
                attempt,
            });
        }
        if let Some(said) = completion.said.take() {
            completion.said = Some(truncate_said(said));
        }
        record.outcome = Some(completion.outcome);
        record.output = completion.output;
        record.said = completion.said;
        record.failure_class = completion.failure_class;
        record.ended_at = Some(completion.ended_at);
        record.bytes_seen = completion.bytes_seen;
        record.bytes_discarded = completion.bytes_discarded;
        Ok(())
    }

    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, FlowError> {
        Ok(self
            .held()
            .iter()
            .filter(|record| record.run_id == run_id)
            .cloned()
            .collect())
    }

    /// **QUESTO DEPOSITO NON REGISTRA LE CHIAMATE, QUINDI NON SA NIENTE DELLA
    /// SPESA.** E qui lo zero è la risposta vera, non un ripiego: nessuna
    /// chiamata scritta, nessun costo, niente di ignoto. Chi usa questo deposito
    /// e dichiara un tetto ottiene un tetto che non scatta — ed è corretto, però
    /// va saputo: le prove che misurano il tetto usano un deposito che i costi
    /// li tiene, non questo.
    fn spent(&self, _run_id: &str) -> Result<Spend, FlowError> {
        Ok(Spend::default())
    }
}

/// Che ora è. `&self` e `Sync` per la stessa ragione del deposito: due passi che
/// girano insieme chiedono l'ora insieme.
pub trait Clock: Sync {
    fn now(&self) -> Result<i64, FlowError>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Result<i64, FlowError> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .map_err(|error| FlowError::Clock(error.to_string()))
    }
}

pub trait ProcessProbe {
    fn is_running(&self, record: &StepRecord) -> Result<bool, FlowError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Ready(Vec<String>),
    Running(Vec<String>),
    Waiting(Vec<String>),
    Stopped(Vec<String>),
    Failed(Vec<String>),
    /// La corsa si è fermata da sé per non superare il tetto di spesa.
    ///
    /// **PERCHÉ UNA PAROLA SUA E NON `Stopped` NÉ `Failed`.** `Failed` direbbe
    /// che qualcosa si è rotto, e un flusso notturno che tocca il proprio tetto
    /// ogni notte apparirebbe guasto ogni notte: chi guarda smetterebbe di
    /// guardare. `Stopped` esiste già e vuol dire un'altra cosa — un passo che
    /// il deposito porta fermo. Qui non si è fermato un passo: si è fermata la
    /// corsa, e per una ragione che si può leggere in soldi.
    CapReached(SpendStop),
    Complete,
}

/// Perché la corsa si è fermata, con i numeri per giudicarlo.
///
/// **PORTA I DATI, NON LA FRASE.** La frase la compone chi mostra — il
/// terminale in una riga, la finestra in un riquadro — e in due lingue diverse
/// se un giorno servirà. Un messaggio già formattato qui dentro obbligherebbe
/// tutti e due a disfarlo per rifarlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendStop {
    /// Il tetto dichiarato per questa corsa, in micro-unità.
    pub cap_micros: i64,
    /// Quanto risulta speso, e quanto di quello resta ignoto.
    pub spent: Spend,
    /// I passi che erano pronti e non sono partiti. Non sono guasti: sono da
    /// fare, e una ripresa con un tetto più alto li trova lì.
    pub not_started: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub decisions: Vec<Decision>,
    pub shared: SharedState,
}

/// Com'è finita una corsa, e se chi l'ha lanciata può dirsi soddisfatto.
///
/// **STA QUI PERCHÉ `Decision` STA QUI.** Questa traduzione è nata in
/// `sailor::flow_cmd` e ne esisteva una copia nel guscio della finestra; il
/// 30/08/2026 le due sono state riunite in `registry::execution_status`.
/// Adesso ne serve una terza a chi esegue un **sotto-flusso**, che vive nel
/// crate del flusso e non può guardare in `registry`. La regola vale come le
/// altre volte: invece di ricopiarla si sposta dove tutti la vedono, cioè
/// accanto al tipo che traduce. `registry::execution_status` resta il nome
/// pubblico che i due chiamanti già usano, e chiama questa.
///
/// Il booleano è la seconda metà della risposta: `cap_reached` e `waiting` non
/// sono guasti, ma nemmeno «è andata».
pub fn run_status(execution: &Execution) -> (&'static str, bool) {
    match execution.decisions.last() {
        Some(Decision::Complete) => ("complete", true),
        Some(Decision::Waiting(_)) => ("waiting", false),
        Some(Decision::Stopped(_)) => ("stopped", false),
        Some(Decision::Failed(_)) => ("failed", false),
        Some(Decision::CapReached(_)) => ("cap_reached", false),
        Some(Decision::Ready(_)) | Some(Decision::Running(_)) | None => ("incomplete", false),
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    pub run_id: String,
    pub root_inputs: BTreeMap<String, Value>,
    pub gates: Vec<String>,
    pub shared: SharedState,
    /// Quanto questa corsa può spendere, in micro-unità di valuta.
    ///
    /// **`None` VUOL DIRE «NESSUN TETTO DICHIARATO», E NON ZERO.** Sono due
    /// cose opposte: `Some(0)` è un flusso che non deve spendere niente — e si
    /// ferma prima della prima chiamata a pagamento, il che è un modo legittimo
    /// di provarlo — mentre `None` è un flusso a cui nessuno ha messo un limite.
    /// Il valore predefinito è `None`, cioè come si è sempre comportato: un
    /// tetto che comparisse da sé fermerebbe corse che nessuno ha chiesto di
    /// fermare.
    pub spend_cap_micros: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reconciliation {
    pub closed_as_went: Vec<String>,
    pub closed_as_broke: Vec<String>,
    pub closed_as_waiting: Vec<String>,
    pub still_running: Vec<String>,
    /// I passi il cui effetto è stato disfatto prima di riaprirli. Non è un
    /// secchio a sé rispetto agli altri: sono anche in `closed_as_broke`,
    /// che è dove si legge «torna pronto». Qui si legge un'altra cosa —
    /// qualcosa è stato annullato sul mondo, e chi guarda deve saperlo.
    pub compensated: Vec<String>,
}

pub struct ReconciliationRequest<'a> {
    pub graph: &'a Graph,
    pub run_id: &'a str,
    pub store: &'a mut dyn RecordStore,
    pub actions: &'a ActionRegistry,
    pub shared: &'a SharedState,
    pub processes: &'a dyn ProcessProbe,
    pub clock: &'a mut dyn Clock,
}

pub trait Executor {
    fn execute(
        &self,
        graph: &Graph,
        request: ExecutionRequest,
        store: &dyn RecordStore,
        actions: &ActionRegistry,
        clock: &dyn Clock,
    ) -> Result<Execution, FlowError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct InProcessExecutor;

impl InProcessExecutor {
    pub fn decision(
        &self,
        graph: &Graph,
        run_id: &str,
        store: &dyn RecordStore,
    ) -> Result<Decision, FlowError> {
        let records = store.records(run_id)?;
        decision_from(graph, &records)
    }

    pub fn reconcile(
        &self,
        request: ReconciliationRequest<'_>,
    ) -> Result<Reconciliation, FlowError> {
        let ReconciliationRequest {
            graph,
            run_id,
            store,
            actions,
            shared,
            processes,
            clock,
        } = request;
        let records = store.records(run_id)?;
        let mut report = Reconciliation::default();
        for record in records.iter().filter(|record| record.outcome.is_none()) {
            if processes.is_running(record)? {
                report.still_running.push(record.step_id.clone());
                continue;
            }
            let resolved = match graph.step(&record.step_id) {
                None => Err(ActionError::new("unknown_step", &record.step_id)),
                Some(step) => match actions.get(&step.action) {
                    None => Err(ActionError::new("unknown_action", &step.action)),
                    Some(action) => Ok((step, action)),
                },
            };
            let action = resolved.as_ref().ok().map(|(_, action)| *action);
            let inspected = match resolved {
                Err(error) => Err(error),
                Ok((step, action)) => action.inspect_effect(record, shared).and_then(|status| {
                    if let EffectStatus::Applied(output) = &status {
                        step.output_schema.validate(output).map_err(|error| {
                            ActionError::new("invalid_recovered_output", error.to_string())
                        })?;
                    }
                    Ok(status)
                }),
            };
            let now = clock.now()?;
            let (completion, bucket) = match inspected {
                Ok(EffectStatus::Applied(output)) => (
                    closed(Outcome::Went, Some(output), None, None, now),
                    &mut report.closed_as_went,
                ),
                Ok(EffectStatus::NotApplied) => (
                    closed(Outcome::Broke, None, None, Some("process_disappeared"), now),
                    &mut report.closed_as_broke,
                ),
                // L'effetto non si sa: qui, e solo qui, decide la specie del
                // passo. Senza di lei l'unica scelta sicura era «in attesa» —
                // e un passo in attesa non torna mai pronto (`decision_from`),
                // cioè la ripresa vedeva l'interrotto e non lo rilanciava mai.
                Ok(EffectStatus::Unknown(reason)) => {
                    match species_for(record, action) {
                        StepSpecies::Repeatable => (
                            closed(
                                Outcome::Broke,
                                None,
                                Some(reason),
                                Some("repeatable_after_unknown_effect"),
                                now,
                            ),
                            &mut report.closed_as_broke,
                        ),
                        StepSpecies::Compensable => {
                            let compensation = action.map_or_else(
                                || {
                                    Err(ActionError::new(
                                        "unknown_action",
                                        "nessuna azione da cui disfare l'effetto",
                                    ))
                                },
                                |action| action.compensate(record, shared),
                            );
                            match compensation {
                                Ok(()) => {
                                    report.compensated.push(record.step_id.clone());
                                    (
                                        closed(
                                            Outcome::Broke,
                                            None,
                                            Some(reason),
                                            Some("compensated_then_retry"),
                                            now,
                                        ),
                                        &mut report.closed_as_broke,
                                    )
                                }
                                // La compensazione dichiarata che non riesce
                                // lascia il mondo a metà: è il caso in cui una
                                // persona serve davvero.
                                Err(error) => (
                                    closed(
                                        Outcome::Waiting,
                                        None,
                                        Some(error.said),
                                        Some(error.class.as_str()),
                                        now,
                                    ),
                                    &mut report.closed_as_waiting,
                                ),
                            }
                        }
                        StepSpecies::HandToHuman => (
                            closed(
                                Outcome::Waiting,
                                None,
                                Some(reason),
                                Some("effect_unknown"),
                                now,
                            ),
                            &mut report.closed_as_waiting,
                        ),
                    }
                }
                Err(error) => (
                    closed(
                        Outcome::Waiting,
                        None,
                        Some(error.said),
                        Some(error.class.as_str()),
                        now,
                    ),
                    &mut report.closed_as_waiting,
                ),
            };
            store.close(
                run_id,
                &record.step_id,
                record.attempt,
                record.epoch,
                completion,
            )?;
            bucket.push(record.step_id.clone());
        }
        Ok(report)
    }
}

impl Executor for InProcessExecutor {
    fn execute(
        &self,
        graph: &Graph,
        mut request: ExecutionRequest,
        store: &dyn RecordStore,
        actions: &ActionRegistry,
        clock: &dyn Clock,
    ) -> Result<Execution, FlowError> {
        let mut decisions = Vec::new();
        // La radice si legge una volta sola e si tiene per valore: resta la
        // stessa per tutta la corsa, e un riferimento dentro `request.shared`
        // impedirebbe all'esecutore di scriverci la corsa poco più sotto.
        let root: Option<PathBuf> = request
            .shared
            .get(WORKSPACE_ROOT)
            .and_then(Value::as_str)
            .map(PathBuf::from);
        loop {
            let records = store.records(&request.run_id)?;
            let decision = decision_from(graph, &records)?;
            decisions.push(decision.clone());
            let Decision::Ready(front) = decision else {
                return Ok(Execution {
                    decisions,
                    shared: request.shared,
                });
            };

            // IL TETTO SI GUARDA PRIMA DI APRIRE, NON DOPO AVER SPESO.
            //
            // Qui, e non dentro l'azione che chiama il motore: un passo che
            // scopre a metà di aver sforato ha già pagato. L'unico momento in
            // cui fermarsi costa zero è prima di aprire il fronte.
            //
            // **IL CONFRONTO È `>=`, NON `>`.** Con `>` un tetto di zero
            // lascerebbe passare la prima chiamata — cioè proprio il caso in
            // cui qualcuno sta dicendo «questo flusso non deve spendere
            // niente».
            let mut at_once = AT_ONCE;
            if let Some(cap) = request.spend_cap_micros {
                let spent = store.spent(&request.run_id)?;
                if spent.micros >= cap {
                    decisions.push(Decision::CapReached(SpendStop {
                        cap_micros: cap,
                        spent,
                        not_started: front,
                    }));
                    return Ok(Execution {
                        decisions,
                        shared: request.shared,
                    });
                }
                at_once = how_many_fit(cap - spent.micros, spent.dearest_micros);
            }

            // IL FRONTE PARTE INSIEME.
            //
            // **PERCHÉ PRIMA NO, E QUANTO COSTAVA.** Qui c'era un `for` che
            // percorreva i passi pronti uno dopo l'altro, con un commento che lo
            // ammetteva: «il fronte è una decisione unica anche se questo
            // esecutore lo percorre in ordine». Misurato il 30/08/2026: due
            // passi indipendenti da sei secondi ne impiegavano dodici, tre ne
            // impiegavano diciotto — lineare, con la macchina ferma allo 0% di
            // processore per tutto il tempo. È il guasto 7, documentato da due
            // giorni e mai riparato, e regge in piedi il terzo blocco di lavoro:
            // «sfruttare la macchina» non ha dove appoggiarsi su una fila
            // indiana.
            //
            // **L'EPOCA È DEL FRONTE, NON DEL PASSO.** Si calcola una volta qui,
            // prima di aprire qualunque passo, e vale per tutti quelli
            // dell'ondata. Prima la calcolava ciascuno dalla stessa fotografia
            // dei record e usciva comunque uguale per tutti: la differenza è che
            // adesso è dichiarata invece che coincidente, e chi legge una corsa
            // vede che quei passi sono partiti insieme perché portano la stessa
            // epoca.
            //
            // **PRIMA SI APRONO TUTTI, POI SI ESEGUONO.** L'apertura è breve e
            // ordinata; l'esecuzione è lunga e concorrente. Tenerle separate
            // rende deterministico l'ordine in cui i passi compaiono nel
            // deposito — che è quello del grafo, non quello in cui i fili
            // vincono la corsa — e lascia la chiusura di ciascuno nel proprio
            // filo, appena finisce, così chi guarda la vede arrivare quando
            // accade.
            let epoch = records.iter().map(|record| record.epoch).max().unwrap_or(0) + 1;
            let mut opened: Vec<Opened<'_>> = Vec::with_capacity(front.len());
            for step_id in front {
                let step = graph
                    .step(&step_id)
                    .ok_or_else(|| FlowError::UnknownStep(step_id.clone()))?;
                let input =
                    step_input(graph, step, &request.root_inputs, &records, root.as_deref())?;
                step.input_schema.validate(&input)?;
                let condition_met = step
                    .when
                    .as_ref()
                    .is_none_or(|condition| condition.matches(&input));
                let action = if condition_met {
                    Some(
                        actions
                            .get(&step.action)
                            .ok_or_else(|| FlowError::UnknownAction(step.action.clone()))?,
                    )
                } else {
                    None
                };
                let previous = latest_for(step, &records);
                let attempt = previous.map_or(1, |record| record.attempt + 1);
                let mut started = StepRecord::started(
                    &request.run_id,
                    &step.id,
                    attempt,
                    epoch,
                    step.deps.clone(),
                    input.clone(),
                    request.gates.clone(),
                    clock.now()?,
                );
                started.attempt_relation = attempt_relation(&records, &started);
                // Chi tiene il passo è questo processo, per definizione di
                // esecutore in processo: il pid si scrive PRIMA dell'effetto,
                // insieme all'intenzione, o alla ripresa non serve a nulla.
                started.held_by_pid = Some(std::process::id());
                started.species = action.map(|action| action.species());
                store.append_started(started)?;
                opened.push(Opened {
                    step,
                    input,
                    attempt,
                    action,
                });
            }

            // La corsa entra nello stato condiviso una volta per tutte: è la
            // stessa per ogni passo. Il passo, invece, è di ciascuno, e ognuno lo
            // riceve nella propria copia — vedi `run_one`.
            request.shared.insert(
                CURRENT_RUN.to_owned(),
                Value::String(request.run_id.clone()),
            );
            // Il tetto entra accanto alla corsa, e solo se c'è: la chiave
            // assente è «nessun tetto dichiarato», che non è `Some(0)`. Serve a
            // un'azione sola — quella che lancia un altro flusso — e senza di
            // questa riga quel figlio girerebbe senza limite sotto un padre che
            // ne ha uno.
            if let Some(cap) = request.spend_cap_micros {
                request
                    .shared
                    .insert(CURRENT_CAP.to_owned(), Value::from(cap));
            }

            // A GRUPPI, E LA LARGHEZZA VIENE DAI SOLDI.
            //
            // Un fronte largo è raro in un grafo scritto a mano, ma quando
            // capita i passi non sono conti: sono agenti. Venti insieme
            // vorrebbero dire venti processi e venti chiamate a pagamento, e
            // nessuno l'avrebbe chiesto.
            //
            // **QUANTI, ADESSO, LO DECIDE IL RESIDUO.** `AT_ONCE` non è più il
            // numero: è il soffitto. Sotto un tetto di spesa la larghezza si
            // stringe man mano che il residuo cala, fino a uno — vedi
            // `how_many_fit`. Senza tetto resta quella di sempre.
            let mut failure: Option<FlowError> = None;
            for group in opened.chunks(at_once) {
                let outcomes: Vec<Result<(), FlowError>> = std::thread::scope(|scope| {
                    let handles: Vec<_> = group
                        .iter()
                        .map(|work| {
                            let shared = &request.shared;
                            let run_id = request.run_id.as_str();
                            scope.spawn(move || run_one(work, run_id, epoch, shared, store, clock))
                        })
                        .collect();
                    handles
                        .into_iter()
                        .map(|handle| match handle.join() {
                            Ok(result) => result,
                            // Un filo che va in panico non deve portarsi via la
                            // corsa in silenzio: diventa un guasto del passo,
                            // con scritto che è successo qui.
                            Err(_) => Err(FlowError::Store(
                                "un passo del fronte è morto mentre girava".to_owned(),
                            )),
                        })
                        .collect()
                });
                for outcome in outcomes {
                    if let Err(error) = outcome {
                        // **SI TIENE IL PRIMO E SI VA AVANTI FINO IN FONDO AL
                        // GRUPPO.** Uscire subito lascerebbe i passi già aperti
                        // dell'ondata senza chiusura, e alla ripresa
                        // sembrerebbero tenuti da un processo vivo.
                        failure.get_or_insert(error);
                    }
                }
                if failure.is_some() {
                    break;
                }
            }
            if let Some(error) = failure {
                return Err(error);
            }
        }
    }
}

/// Il **soffitto** di quanti passi girano insieme, non il numero.
///
/// **NON È UN LIMITE TECNICO.** La macchina ne reggerebbe di più; a non
/// reggerne di più sono le quote dei motori e la pazienza di chi guarda.
/// Quattro: abbastanza da far sparire l'attesa di un fronte normale — che nei
/// flussi scritti finora è di due o tre passi — e poco abbastanza da non aprire
/// una decina di conversazioni a pagamento per una corsa che nessuno sorveglia.
///
/// Dal 31/08/2026 è un massimo e non più la scelta: sotto un tetto di spesa il
/// numero vero lo calcola `how_many_fit`, e questa costante dice solo fin dove
/// può salire.
const AT_ONCE: usize = 4;

/// Quanti passi si possono aprire insieme con questo residuo.
///
/// **IL PROBLEMA CHE RISOLVE, DETTO IN UNA RIGA:** un tetto non si può
/// rispettare con un fronte largo. Quattro chiamate partono nello stesso
/// istante, nessuna delle quattro sa delle altre, e quando la prima registra il
/// proprio costo le altre tre hanno già speso. Lo sforamento peggiore non è di
/// una chiamata: è di quante ne sono in volo. Quindi la larghezza del fronte è
/// una **conseguenza aritmetica** del residuo, non una preferenza.
///
/// **CON CHE MISURA.** La chiamata più cara vista *in questa corsa*: è il caso
/// peggiore osservato, non una media — una media lascerebbe sforare ogni volta
/// che la prossima è sopra la media, cioè in metà dei casi. Non si guarda alla
/// storia di altre corse apposta: un flusso che chiama un modello piccolo non
/// deve stringersi perché ieri un altro flusso ne ha chiamato uno grande.
///
/// **QUANDO NON SI SA, NON SI STRINGE.** Senza nessuna chiamata dichiarata —
/// primo fronte di una corsa, oppure motori che il costo non lo dicono — questa
/// divisione non si può fare. Restituire 1 «per prudenza» sembrerebbe la scelta
/// sicura ed è invece una scelta arbitraria travestita: renderebbe seriale ogni
/// corsa con un tetto, per sempre, sulla base di un numero che non esiste. Si
/// resta al soffitto, e la corsa si ferma al controllo del fronte dopo — che è
/// dove il tetto lavora davvero.
fn how_many_fit(remaining_micros: i64, dearest_micros: Option<i64>) -> usize {
    let Some(dearest) = dearest_micros.filter(|dearest| *dearest > 0) else {
        return AT_ONCE;
    };
    // Il residuo è positivo per costruzione: chi chiama ha già verificato che la
    // spesa non abbia raggiunto il tetto. La divisione intera tronca verso il
    // basso, che è il verso giusto — tre chiamate e mezzo di margine sono tre.
    let fit = (remaining_micros / dearest).clamp(1, AT_ONCE as i64);
    fit as usize
}

/// Un passo già aperto nel deposito, in attesa di essere eseguito.
struct Opened<'a> {
    step: &'a Step,
    input: Value,
    attempt: u32,
    action: Option<&'a dyn Action>,
}

/// Esegue un passo e lo chiude. Gira nel proprio filo.
///
/// **LO STATO CONDIVISO È UNA COPIA, E QUI STA IL PUNTO DELICATO DI TUTTO IL
/// LAVORO.** Un'azione che produce testo, o che registra una spesa, chiede allo
/// stato condiviso di chi è il passo corrente (`CURRENT_STEP`). Finché i passi
/// giravano in fila, una chiave sola bastava: c'era un solo passo vivo. Con due
/// passi vivi quella chiave avrebbe un valore solo, e il testo e i **costi** di
/// uno finirebbero attribuiti all'altro — in silenzio, senza che niente diventi
/// rosso. Per questo ogni filo riceve la propria copia con dentro il proprio
/// passo: la chiave resta una, ma la mappa è di ciascuno.
fn run_one(
    work: &Opened<'_>,
    run_id: &str,
    epoch: u64,
    shared: &SharedState,
    store: &dyn RecordStore,
    clock: &dyn Clock,
) -> Result<(), FlowError> {
    let step = work.step;
    let mut mine = shared.clone();
    mine.insert(CURRENT_STEP.to_owned(), Value::String(step.id.clone()));

    let completion = match work.action {
        None => Completion {
            outcome: Outcome::Skipped,
            output: None,
            said: None,
            failure_class: None,
            ended_at: clock.now()?,
            bytes_seen: None,
            bytes_discarded: None,
        },
        Some(action) => match action.execute(&work.input, &mine) {
            Ok(ActionOutcome::Went(output)) => match step.output_schema.validate(&output) {
                Ok(()) => Completion {
                    outcome: Outcome::Went,
                    output: Some(output),
                    said: None,
                    failure_class: None,
                    ended_at: clock.now()?,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
                Err(error) => Completion {
                    outcome: Outcome::Broke,
                    output: None,
                    said: Some(error.to_string()),
                    failure_class: Some("invalid_output".to_owned()),
                    ended_at: clock.now()?,
                    bytes_seen: None,
                    bytes_discarded: None,
                },
            },
            Ok(ActionOutcome::Waiting(reason)) => Completion {
                outcome: Outcome::Waiting,
                output: None,
                said: Some(reason),
                failure_class: None,
                ended_at: clock.now()?,
                bytes_seen: None,
                bytes_discarded: None,
            },
            Err(error) => Completion {
                outcome: Outcome::Broke,
                output: None,
                said: Some(error.said),
                failure_class: Some(error.class),
                ended_at: clock.now()?,
                bytes_seen: None,
                bytes_discarded: None,
            },
        },
    };
    store.close(run_id, &step.id, work.attempt, epoch, completion)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowError {
    Store(String),
    Clock(String),
    InvalidRecord(String),
    DuplicateAttempt { step: String, attempt: u32 },
    MissingAttempt { step: String, attempt: u32 },
    AlreadyClosed { step: String, attempt: u32 },
    StaleEpoch { step: String, epoch: u64 },
    UnknownStep(String),
    UnknownAction(String),
    MissingOutput(String),
    Schema(SchemaError),
    Action(ActionError),
    /// Un campo di posizione con un percorso assoluto scritto dentro il flusso.
    AbsolutePath {
        step: String,
        field: String,
        value: String,
    },
    /// Un passo ha bisogno della radice del progetto e nessuno l'ha portata.
    NoWorkspaceRoot {
        step: String,
        field: String,
        value: String,
    },
}

impl From<SchemaError> for FlowError {
    fn from(value: SchemaError) -> Self {
        Self::Schema(value)
    }
}

impl From<ActionError> for FlowError {
    fn from(value: ActionError) -> Self {
        Self::Action(value)
    }
}

impl Display for FlowError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            FlowError::Store(error) => write!(formatter, "record store: {error}"),
            FlowError::Clock(error) => write!(formatter, "clock: {error}"),
            FlowError::InvalidRecord(error) => write!(formatter, "invalid record: {error}"),
            FlowError::DuplicateAttempt { step, attempt } => {
                write!(formatter, "step {step} attempt {attempt} already exists")
            }
            FlowError::MissingAttempt { step, attempt } => {
                write!(formatter, "step {step} attempt {attempt} does not exist")
            }
            FlowError::AlreadyClosed { step, attempt } => {
                write!(formatter, "step {step} attempt {attempt} is already closed")
            }
            FlowError::StaleEpoch { step, epoch } => {
                write!(formatter, "step {step} epoch {epoch} is stale")
            }
            FlowError::UnknownStep(step) => write!(formatter, "unknown step {step}"),
            FlowError::UnknownAction(action) => write!(formatter, "unknown action {action}"),
            FlowError::MissingOutput(step) => write!(formatter, "step {step} has no typed output"),
            FlowError::Schema(error) => Display::fmt(error, formatter),
            FlowError::Action(error) => Display::fmt(error, formatter),
            // In italiano, come dice `AGENTS.md`. Le righe qui sopra sono in
            // inglese: sono più vecchie della regola, e tradurle tutte è un
            // lavoro a sé che cambierebbe messaggi già visti da chi usa il
            // programma. Non si scrive un messaggio nuovo sbagliato per
            // assomigliare a quelli vecchi.
            FlowError::AbsolutePath { step, field, value } => write!(
                formatter,
                "il passo {step} dichiara «{field}» con un percorso assoluto ({value}): \
                 un flusso non deve sapere dove sta il progetto, o gira in un posto solo. \
                 Si toglie con «sailor flow relocate»"
            ),
            FlowError::NoWorkspaceRoot { step, field, value } => write!(
                formatter,
                "il passo {step} dichiara «{field}» relativo ({value}) ma non c'è nessuna \
                 radice di progetto: manca un {} risalendo da dove hai lanciato. \
                 Si crea con «sailor workspace init»",
                crate::workspace::MARKER
            ),
        }
    }
}

impl Error for FlowError {}

fn closed(
    outcome: Outcome,
    output: Option<Value>,
    said: Option<String>,
    failure_class: Option<&str>,
    ended_at: i64,
) -> Completion {
    Completion {
        outcome,
        output,
        said,
        failure_class: failure_class.map(str::to_owned),
        ended_at,
        bytes_seen: None,
        bytes_discarded: None,
    }
}

/// La specie di un passo aperto. **Il record vince sull'azione**: è ciò che
/// valeva quando il passo è partito, e un'azione riscritta nel frattempo non
/// può cambiare il giudizio su un effetto prodotto dalla versione di prima.
/// L'azione risponde solo per i record scritti prima che la specie esistesse;
/// se non c'è nemmeno quella, si consegna a una persona.
fn species_for(record: &StepRecord, action: Option<&dyn Action>) -> StepSpecies {
    record
        .species
        .or_else(|| action.map(|action| action.species()))
        .unwrap_or(StepSpecies::HandToHuman)
}

fn decision_from(graph: &Graph, records: &[StepRecord]) -> Result<Decision, FlowError> {
    let mut ready = Vec::new();
    let mut running = Vec::new();
    let mut waiting = Vec::new();
    let mut stopped = Vec::new();
    let mut failed = Vec::new();
    for step in graph.steps() {
        let latest = latest_for(step, records);
        match latest.and_then(|record| record.outcome) {
            Some(Outcome::Went) => continue,
            None if latest.is_some() => running.push(step.id.clone()),
            Some(Outcome::Waiting) => waiting.push(step.id.clone()),
            Some(Outcome::Stopped) => stopped.push(step.id.clone()),
            Some(Outcome::Skipped) => continue,
            Some(Outcome::Broke)
                if latest.is_some_and(|record| record.attempt >= step.max_attempts) =>
            {
                failed.push(step.id.clone());
            }
            Some(Outcome::Broke) | None => {
                if dependencies_satisfied(graph, step, records) {
                    ready.push(step.id.clone());
                }
            }
        }
    }
    if !failed.is_empty() {
        Ok(Decision::Failed(failed))
    } else if !ready.is_empty() {
        Ok(Decision::Ready(ready))
    } else if !running.is_empty() {
        Ok(Decision::Running(running))
    } else if !waiting.is_empty() {
        Ok(Decision::Waiting(waiting))
    } else if !stopped.is_empty() {
        Ok(Decision::Stopped(stopped))
    } else {
        Ok(Decision::Complete)
    }
}

fn dependencies_satisfied(graph: &Graph, step: &Step, records: &[StepRecord]) -> bool {
    step.deps.iter().all(|dependency| {
        let outcome = records
            .iter()
            .filter(|record| record.step_id == *dependency)
            .max_by_key(|record| (record.attempt, record.epoch))
            .and_then(|record| record.outcome);
        outcome == Some(Outcome::Went)
            || (outcome == Some(Outcome::Skipped)
                && graph.dependency_is_skippable(&step.id, dependency))
    })
}

#[cfg(test)]
mod workdir_tests {
    use super::*;
    use crate::schema::ValueSchema;

    fn step_named(id: &str, with: Value, schema: ValueSchema) -> Step {
        let json = serde_json::json!({
            "id": id, "deps": [], "action": "qualunque", "max_attempts": 1,
            "when": null,
            "input_schema": schema,
            "output_schema": {"type": "any"},
            "with": with
        });
        serde_json::from_value(json).expect("un passo valido")
    }

    fn open_object() -> ValueSchema {
        serde_json::from_value(serde_json::json!({
            "type": "object", "properties": {}, "required": [], "allow_extra": true
        }))
        .expect("schema aperto")
    }

    fn resolved(with: Value, root: Option<&str>) -> Result<Value, FlowError> {
        let step = step_named("passo", with, open_object());
        let input = step.with.clone().expect("il with c'è");
        resolve_workdir(&step, input, root.map(Path::new))
    }

    /// **UN PERCORSO RELATIVO SI ATTACCA ALLA RADICE**, ed è tutto il punto:
    /// lo stesso flusso lavora in due cloni diversi senza cambiare una riga.
    #[test]
    fn a_relative_workdir_hangs_off_the_root() {
        let out = resolved(serde_json::json!({"workdir": "crates/flow"}), Some("/qui"))
            .expect("si risolve");

        assert_eq!(out["workdir"], "/qui/crates/flow");
    }

    /// Assoluto: errore che nomina passo e valore. Non «gira altrove»: gira nel
    /// posto sbagliato, ed è il modo in cui il guasto 25 è passato inosservato.
    #[test]
    fn an_absolute_workdir_is_refused_by_name() {
        let refused = resolved(
            serde_json::json!({"workdir": "/home/someone/personal/sailor"}),
            Some("/qui"),
        )
        .expect_err("non deve risolversi");

        match refused {
            FlowError::AbsolutePath { step, value, .. } => {
                assert_eq!(step, "passo");
                assert_eq!(value, "/home/someone/personal/sailor");
            }
            altro => panic!("errore sbagliato: {altro}"),
        }
    }

    /// Assente: eredita la radice. È ciò che rende possibile togliere i sette
    /// `workdir` dal flusso di sviluppo senza che i passi cambino posto.
    #[test]
    fn an_absent_workdir_inherits_the_root() {
        let out = resolved(serde_json::json!({"command": "true"}), Some("/qui"))
            .expect("si risolve");

        assert_eq!(out["workdir"], "/qui");
    }

    /// **MA SOLO A CHI PUÒ RICEVERLA.** Il passo d'innesco di `sviluppa-sailor`
    /// ha uno schema chiuso e niente a che fare con una cartella: offrirgliela
    /// lo farebbe morire su un campo che non ha chiesto.
    #[test]
    fn a_closed_schema_is_not_given_a_workdir_it_never_asked_for() {
        let closed: ValueSchema = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {"source": {"type": "string"}},
            "required": [],
            "allow_extra": false
        }))
        .expect("schema chiuso");
        let step = step_named("innesco", serde_json::json!({"source": "manual"}), closed);
        let input = step.with.clone().expect("il with c'è");

        let out = resolve_workdir(&step, input, Some(Path::new("/qui"))).expect("si risolve");

        assert!(out.get("workdir").is_none(), "niente campi non richiesti");
        step.input_schema.validate(&out).expect("lo schema regge");
    }

    /// **SENZA RADICE SI FALLISCE DICENDOLO, MAI SUL `cwd`.** Un ripiego
    /// silenzioso sulla cartella del processo è esattamente il guasto 25:
    /// lavorare dove capita senza che nessuno lo veda scritto.
    #[test]
    fn a_relative_workdir_without_a_root_fails_out_loud() {
        let refused = resolved(serde_json::json!({"workdir": "crates/flow"}), None)
            .expect_err("non deve ripiegare sul cwd");

        match refused {
            FlowError::NoWorkspaceRoot { step, value, .. } => {
                assert_eq!(step, "passo");
                assert_eq!(value, "crates/flow");
            }
            altro => panic!("errore sbagliato: {altro}"),
        }
        assert!(
            refused_says_how_to_fix(&resolved(
                serde_json::json!({"workdir": "crates/flow"}),
                None
            )),
            "il messaggio deve dire cosa fare"
        );
    }

    fn refused_says_how_to_fix(outcome: &Result<Value, FlowError>) -> bool {
        match outcome {
            Err(error) => error.to_string().contains(crate::workspace::MARKER),
            Ok(_) => false,
        }
    }
}

/// Il campo con cui un passo dice **dove** lavora.
///
/// **IL CRATE DEL FLUSSO CONOSCE QUESTA PAROLA, E SI PAGA APPOSTA.** Finché la
/// risoluzione stava dentro le azioni, ogni azione nuova nasceva senza — è il
/// guasto 28 sulla stessa dimensione: `resolve_references` è chiamata da due
/// azioni su nove, e le altre sette non lo sanno. Qui la risoluzione avviene
/// **una volta sola dove l'ingresso si compone**, quindi ogni azione
/// registrata la eredita, comprese quelle che nessuno ha ancora scritto.
pub const WORKDIR_FIELD: &str = "workdir";

pub fn step_input(
    graph: &Graph,
    step: &Step,
    root_inputs: &BTreeMap<String, Value>,
    records: &[StepRecord],
    root: Option<&Path>,
) -> Result<Value, FlowError> {
    let input = match step.deps.as_slice() {
        [] => Ok(root_inputs.get(&step.id).cloned().unwrap_or(Value::Null)),
        [only] if !graph.dependency_is_skippable(&step.id, only) => {
            successful_output(only, records)
        }
        many => {
            let mut values = serde_json::Map::new();
            for dependency in many {
                if let Some(output) = dependency_output(
                    dependency,
                    graph.dependency_is_skippable(&step.id, dependency),
                    records,
                )? {
                    values.insert(dependency.clone(), output);
                }
            }
            Ok(Value::Object(values))
        }
    }?;
    resolve_workdir(step, overlay_input(input, step.with.as_ref()), root)
}

/// Dove il passo lavorerà, deciso qui e non dentro l'azione.
///
/// Quattro casi, e nessuno di essi è un ripiego silenzioso:
/// - **assoluto** → errore che nomina passo e valore: il flusso girerebbe in un
///   posto solo, e altrove non fallirebbe, lavorerebbe nel posto sbagliato;
/// - **relativo** → si attacca alla radice;
/// - **assente** → la radice, ma **solo a chi può riceverla** (vedi
///   `accepts_property`): offrirla a uno schema chiuso lo farebbe morire su un
///   campo che non ha chiesto;
/// - **radice assente e passo che ne ha bisogno** → errore leggibile. **Mai il
///   `cwd`**: lavorare dove sta il processo senza dirlo è il guasto 25.
fn resolve_workdir(step: &Step, input: Value, root: Option<&Path>) -> Result<Value, FlowError> {
    let Value::Object(mut fields) = input else {
        return Ok(input);
    };
    match fields.get(WORKDIR_FIELD).cloned() {
        Some(Value::String(declared)) => {
            if declared.starts_with('/') || declared.starts_with("~/") {
                return Err(FlowError::AbsolutePath {
                    step: step.id.clone(),
                    field: WORKDIR_FIELD.to_owned(),
                    value: declared,
                });
            }
            let Some(root) = root else {
                return Err(FlowError::NoWorkspaceRoot {
                    step: step.id.clone(),
                    field: WORKDIR_FIELD.to_owned(),
                    value: declared,
                });
            };
            fields.insert(
                WORKDIR_FIELD.to_owned(),
                root.join(declared).display().to_string().into(),
            );
        }
        // Dichiarato ma non come testo: non è un percorso, e inventarne uno
        // sarebbe peggio che lasciarlo passare a chi sa cosa farne.
        Some(_) => {}
        None => {
            if let Some(root) = root.filter(|_| step.input_schema.accepts_property(WORKDIR_FIELD)) {
                fields.insert(
                    WORKDIR_FIELD.to_owned(),
                    root.display().to_string().into(),
                );
            }
        }
    }
    Ok(Value::Object(fields))
}

fn overlay_input(input: Value, with: Option<&Value>) -> Value {
    let Some(with) = with else {
        return input;
    };
    let Value::Object(with) = with else {
        return with.clone();
    };
    let Value::Object(mut input) = input else {
        return Value::Object(with.clone());
    };
    input.extend(with.clone());
    Value::Object(input)
}

pub fn attempt_relation(
    records: &[StepRecord],
    started: &StepRecord,
) -> Option<AttemptRelation> {
    let previous = records
        .iter()
        .filter(|record| {
            record.step_id == started.step_id
                && (record.attempt < started.attempt || record.epoch < started.epoch)
        })
        .max_by_key(|record| (record.attempt, record.epoch))?;
    if previous.input_digest != started.input_digest {
        Some(AttemptRelation::DifferentInput)
    } else {
        let origin = records
            .iter()
            .filter(|record| {
                record.step_id == started.step_id
                    && record.input_digest == started.input_digest
            })
            .min_by_key(|record| (record.attempt, record.epoch))
            .unwrap_or(previous);
        if same_gates(&origin.gates, &started.gates) {
            Some(AttemptRelation::SameInput)
        } else {
            Some(AttemptRelation::SameInputGatesChanged)
        }
    }
}

pub fn latest_for<'a>(step: &Step, records: &'a [StepRecord]) -> Option<&'a StepRecord> {
    records
        .iter()
        .filter(|record| record.step_id == step.id)
        .max_by_key(|record| (record.attempt, record.epoch))
}

pub fn same_gates(left: &[String], right: &[String]) -> bool {
    let left: std::collections::BTreeSet<_> = left.iter().collect();
    let right: std::collections::BTreeSet<_> = right.iter().collect();
    left == right
}

fn successful_output(step_id: &str, records: &[StepRecord]) -> Result<Value, FlowError> {
    records
        .iter()
        .filter(|record| record.step_id == step_id && record.outcome == Some(Outcome::Went))
        .max_by_key(|record| (record.attempt, record.epoch))
        .and_then(|record| record.output.clone())
        .ok_or_else(|| FlowError::MissingOutput(step_id.to_owned()))
}

fn dependency_output(
    step_id: &str,
    skippable: bool,
    records: &[StepRecord],
) -> Result<Option<Value>, FlowError> {
    let latest = records
        .iter()
        .filter(|record| record.step_id == step_id)
        .max_by_key(|record| (record.attempt, record.epoch));
    match latest.and_then(|record| record.outcome) {
        Some(Outcome::Went) => latest
            .and_then(|record| record.output.clone())
            .map(Some)
            .ok_or_else(|| FlowError::MissingOutput(step_id.to_owned())),
        Some(Outcome::Skipped) if skippable => Ok(None),
        _ => Err(FlowError::MissingOutput(step_id.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Condition, ValueSchema};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Un orologio finto che avanza di uno a ogni domanda, con un contatore
    /// atomico: ora l'orologio lo condividono i fili di un fronte.
    struct Tick(std::sync::atomic::AtomicI64);

    impl Tick {
        fn new(start: i64) -> Self {
            Tick(std::sync::atomic::AtomicI64::new(start))
        }
    }

    impl Clock for Tick {
        fn now(&self) -> Result<i64, FlowError> {
            Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
        }
    }

    struct Echo;

    impl Action for Echo {
        fn execute(
            &self,
            input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            Ok(ActionOutcome::Went(input.clone()))
        }
    }

    struct FailOnce(Arc<AtomicUsize>);

    impl Action for FailOnce {
        fn execute(
            &self,
            input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(ActionError::new("temporary", "try again"))
            } else {
                Ok(ActionOutcome::Went(input.clone()))
            }
        }
    }

    struct Wait;

    impl Action for Wait {
        fn execute(
            &self,
            _input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            Ok(ActionOutcome::Waiting("source unreadable".to_owned()))
        }
    }

    struct Empty;

    impl Action for Empty {
        fn execute(
            &self,
            _input: &Value,
            _shared: &SharedState,
        ) -> Result<ActionOutcome, ActionError> {
            Ok(ActionOutcome::Went(json!([])))
        }
    }

    fn step(id: &str, deps: &[&str], action: &str, max_attempts: u32) -> Step {
        Step {
            id: id.to_owned(),
            deps: deps.iter().map(|id| (*id).to_owned()).collect(),
            input_schema: ValueSchema::Any,
            output_schema: ValueSchema::Any,
            with: None,
            when: None,
            action: action.to_owned(),
            max_attempts,
        }
    }

    /// L'ESECUTORE DICE ALL'AZIONE DI QUALE PASSO È IL LAVORO CHE STA
    /// FACENDO, e lo dice a ogni passo: senza, un'azione che produce testo
    /// mentre gira non saprebbe attribuirlo, e in un grafo con due passi vivi
    /// chi guarda leggerebbe due voci mescolate senza nome.
    #[test]
    fn each_action_sees_the_id_of_the_step_it_is_running() {
        struct WhoAmI(Arc<std::sync::Mutex<Vec<String>>>);

        impl Action for WhoAmI {
            fn execute(
                &self,
                _input: &Value,
                shared: &SharedState,
            ) -> Result<ActionOutcome, ActionError> {
                let seen = shared
                    .get(CURRENT_STEP)
                    .and_then(Value::as_str)
                    .unwrap_or("nessuno")
                    .to_owned();
                self.0.lock().expect("nessuno panica qui").push(seen);
                Ok(ActionOutcome::Went(json!({})))
            }
        }

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let graph = Graph::new(vec![
            step("primo", &[], "chi-sono", 1),
            step("secondo", &["primo"], "chi-sono", 1),
        ])
        .expect("grafo valido");
        let mut actions = ActionRegistry::default();
        actions.register("chi-sono", WhoAmI(seen.clone()));
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: BTreeMap::new(),
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        };
        let mut store = InMemoryRecordStore::default();
        InProcessExecutor
            .execute(&graph, request, &mut store, &actions, &mut Tick::new(0))
            .expect("esecuzione riuscita");
        assert_eq!(
            *seen.lock().expect("nessuno panica qui"),
            vec!["primo".to_owned(), "secondo".to_owned()]
        );
    }

    #[test]
    fn branch_and_join_use_ready_fronts_and_typed_values() {
        let graph = Graph::new(vec![
            step("root", &[], "echo", 1),
            step("left", &["root"], "echo", 1),
            step("right", &["root"], "echo", 1),
            step("join", &["left", "right"], "echo", 1),
        ])
        .expect("grafo valido");
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: [("root".to_owned(), json!({"value": "said is not data"}))]
                .into_iter()
                .collect(),
            gates: vec!["filesystem".to_owned()],
            shared: [("budget".to_owned(), json!(10))].into_iter().collect(),
            spend_cap_micros: None,
        };
        let mut store = InMemoryRecordStore::default();
        let result = InProcessExecutor
            .execute(&graph, request, &mut store, &actions, &mut Tick::new(0))
            .expect("esecuzione riuscita");
        assert_eq!(
            result.decisions,
            vec![
                Decision::Ready(vec!["root".to_owned()]),
                Decision::Ready(vec!["left".to_owned(), "right".to_owned()]),
                Decision::Ready(vec!["join".to_owned()]),
                Decision::Complete,
            ]
        );
        let records = store.all();
        let join = records
            .iter()
            .find(|record| record.step_id == "join")
            .expect("record della giunzione");
        assert_eq!(join.input["left"]["value"], "said is not data");
        assert_eq!(join.input["right"]["value"], "said is not data");
    }

    #[test]
    fn dependent_step_merges_its_values_over_predecessor_output() {
        let mut send = step("send", &["panel"], "echo", 1);
        send.with = Some(json!({"text": "/clear", "mode": "declared"}));
        let graph = Graph::new(vec![step("panel", &[], "echo", 1), send])
            .expect("grafo valido");
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: [(
                "panel".to_owned(),
                json!({"panel": "p-7", "mode": "predecessor"}),
            )]
            .into_iter()
            .collect(),
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        };
        let mut store = InMemoryRecordStore::default();

        InProcessExecutor
            .execute(&graph, request, &mut store, &actions, &mut Tick::new(0))
            .expect("esecuzione riuscita");

        let records = store.all();
        let send = records
            .iter()
            .find(|record| record.step_id == "send")
            .expect("record dell'invio");
        assert_eq!(
            send.input,
            json!({"panel": "p-7", "mode": "declared", "text": "/clear"})
        );
    }

    #[test]
    fn action_can_wait_without_failure_or_retry() {
        let graph = Graph::new(vec![
            step("uncertain", &[], "wait", 3),
            step("later", &["uncertain"], "echo", 1),
        ])
        .expect("grafo valido");
        let mut actions = ActionRegistry::default();
        actions.register("wait", Wait);
        actions.register("echo", Echo);
        let mut store = InMemoryRecordStore::default();
        let execution = InProcessExecutor
            .execute(
                &graph,
                ExecutionRequest {
                    run_id: "run".to_owned(),
                    root_inputs: BTreeMap::new(),
                    gates: vec![],
                    shared: SharedState::new(),
                    spend_cap_micros: None,
                },
                &mut store,
                &actions,
                &mut Tick::new(0),
            )
            .expect("l'attesa è un esito legittimo");

        assert_eq!(
            execution.decisions,
            vec![
                Decision::Ready(vec!["uncertain".to_owned()]),
                Decision::Waiting(vec!["uncertain".to_owned()]),
            ]
        );
        assert_eq!(store.all().len(), 1);
        assert_eq!(store.all()[0].outcome, Some(Outcome::Waiting));
        assert_eq!(store.all()[0].failure_class, None);
    }

    #[test]
    fn conditional_join_omits_skipped_input_but_keeps_present_empty_input() {
        let mut skipped = step("skipped", &["root"], "empty", 1);
        skipped.when = Some(Condition::PointerEquals {
            pointer: "/take_skipped".to_owned(),
            value: json!(true),
        });
        let graph = Graph::with_skippable_dependencies(
            vec![
                step("root", &[], "echo", 1),
                skipped,
                step("present_empty", &["root"], "empty", 1),
                step("join", &["skipped", "present_empty"], "echo", 1),
            ],
            [crate::DependencyEdge::new("join", "skipped")],
        )
        .expect("grafo valido");
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        actions.register("empty", Empty);
        let mut store = InMemoryRecordStore::default();
        InProcessExecutor
            .execute(
                &graph,
                ExecutionRequest {
                    run_id: "run".to_owned(),
                    root_inputs: [("root".to_owned(), json!({"take_skipped": false}))]
                        .into_iter()
                        .collect(),
                    gates: vec![],
                    shared: SharedState::new(),
                    spend_cap_micros: None,
                },
                &mut store,
                &actions,
                &mut Tick::new(0),
            )
            .expect("la giunzione parte");

        let records = store.all();
        let join = records
            .iter()
            .find(|record| record.step_id == "join")
            .expect("record della giunzione");
        let input = join.input.as_object().expect("ingresso composto");
        assert!(!input.contains_key("skipped"));
        assert_eq!(input.get("present_empty"), Some(&json!([])));
        assert_eq!(join.outcome, Some(Outcome::Went));
    }

    #[test]
    fn retry_repeats_only_the_failed_step() {
        let graph = Graph::new(vec![
            step("first", &[], "echo", 1),
            step("retry", &["first"], "flaky", 2),
        ])
        .expect("grafo valido");
        let count = Arc::new(AtomicUsize::new(0));
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);
        actions.register("flaky", FailOnce(count));
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: [("first".to_owned(), json!("input"))].into_iter().collect(),
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        };
        let mut store = InMemoryRecordStore::default();
        InProcessExecutor
            .execute(&graph, request, &mut store, &actions, &mut Tick::new(0))
            .expect("il secondo tentativo riesce");
        assert_eq!(
            store
                .all()
                .iter()
                .filter(|record| record.step_id == "first")
                .count(),
            1
        );
        assert_eq!(
            store
                .all()
                .iter()
                .filter(|record| record.step_id == "retry")
                .count(),
            2
        );
    }

    #[test]
    fn retry_with_same_input_and_changed_gates_is_explicit() {
        let graph = Graph::new(vec![step("work", &[], "echo", 2)]).expect("grafo valido");
        let input = json!({"payload": 7});
        let mut first = StepRecord::started(
            "run",
            "work",
            1,
            1,
            vec![],
            input.clone(),
            vec!["filesystem".to_owned()],
            1,
        );
        first.outcome = Some(Outcome::Broke);
        first.failure_class = Some("temporary".to_owned());
        first.ended_at = Some(2);
        let mut store = InMemoryRecordStore::from_records(vec![first]);
        let mut actions = ActionRegistry::default();
        actions.register("echo", Echo);

        InProcessExecutor
            .execute(
                &graph,
                ExecutionRequest {
                    run_id: "run".to_owned(),
                    root_inputs: [("work".to_owned(), input)].into_iter().collect(),
                    gates: vec!["network".to_owned(), "filesystem".to_owned()],
                    shared: SharedState::new(),
                    spend_cap_micros: None,
                },
                &mut store,
                &actions,
                &mut Tick::new(2),
            )
            .expect("ripresa riuscita");

        let attempts = store.all();
        assert_eq!(attempts[0].input_digest, attempts[1].input_digest);
        assert_eq!(
            attempts[1].attempt_relation,
            Some(AttemptRelation::SameInputGatesChanged)
        );
        assert_eq!(attempts[1].said, None);
    }

    #[test]
    fn later_epoch_fences_a_returning_attempt() {
        let store = InMemoryRecordStore::default();
        let mut first = StepRecord::started("run", "step", 1, 4, vec![], json!(null), vec![], 1);
        first.outcome = Some(Outcome::Broke);
        first.failure_class = Some("dead".to_owned());
        first.ended_at = Some(2);
        store.held().push(first);
        store
            .append_started(StepRecord::started(
                "run",
                "step",
                2,
                5,
                vec![],
                json!(null),
                vec![],
                3,
            ))
            .expect("epoca successiva");
        let result = store.close(
            "run",
            "step",
            1,
            4,
            Completion {
                outcome: Outcome::Went,
                output: Some(json!("late")),
                said: None,
                failure_class: None,
                ended_at: 4,
                bytes_seen: None,
                bytes_discarded: None,
            },
        );
        assert_eq!(
            result,
            Err(FlowError::StaleEpoch {
                step: "step".to_owned(),
                epoch: 4
            })
        );
    }

    #[test]
    fn condition_reads_typed_input_and_never_said() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut conditional = step("conditional", &[], "action", 1);
        conditional.when = Some(Condition::PointerEquals {
            pointer: "/approved".to_owned(),
            value: json!(true),
        });
        let graph = Graph::new(vec![conditional]).expect("grafo valido");
        let mut actions = ActionRegistry::default();
        actions.register("action", FailOnce(Arc::clone(&count)));
        let request = ExecutionRequest {
            run_id: "run".to_owned(),
            root_inputs: [(
                "conditional".to_owned(),
                json!({"approved": false, "said": "approved"}),
            )]
            .into_iter()
            .collect(),
            gates: vec![],
            shared: SharedState::new(),
            spend_cap_micros: None,
        };
        let mut store = InMemoryRecordStore::default();
        let execution = InProcessExecutor
            .execute(&graph, request, &mut store, &actions, &mut Tick::new(0))
            .expect("condizione valutata");
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(store.all()[0].outcome, Some(Outcome::Skipped));
        assert_eq!(
            execution.decisions,
            vec![
                Decision::Ready(vec!["conditional".to_owned()]),
                Decision::Complete,
            ]
        );
    }
}
