//! Far partire un flusso dalla finestra, e guardarlo correre.
//!
//! ## PERCHÉ IL GUSCIO ESEGUE, INVECE DI LANCIARE IL BINARIO
//!
//! La via ovvia sarebbe lanciare `sailor flow run <nome>` come processo figlio
//! e leggerne l'uscita. Misurato il 28/08/2026, quel comando dà **una riga
//! sola, a corsa finita**: dodici secondi di silenzio e poi `flusso X terminato
//! con stato Y`. Una vista costruita su quello resterebbe vuota per tutta la
//! durata del lavoro, che è il difetto peggiore di una vista d'esecuzione —
//! peggiore di non averla, perché sembra rotta.
//!
//! E c'è un secondo motivo, che da solo basterebbe: **il binario non accetta un
//! testo**. Il suo dispatch è un `match` su due elementi esatti
//! (`crates/sailor/src/flow_cmd.rs`), senza `--input`, senza stdin, senza
//! `--json`. Un mandato si passa solo cambiando il campo `inputs` del file. Un
//! innesco che, per portare la consegna di chi preme, dovesse **riscrivere il
//! flusso sul disco** cambierebbe il documento a ogni corsa: la consegna di
//! stamattina resterebbe scritta nel file di domani.
//!
//! Il guscio dipende già da `flow`, `actions`, `ledger` e `toolbox`, e
//! `InProcessExecutor` è sincrono. Eseguire qui costa un thread e restituisce
//! le due cose che servono: il mandato passa in memoria, e ogni passo si vede
//! aprirsi e chiudersi nell'istante in cui accade.
//!
//! ## QUESTO MODULO RICALCA `flow_cmd::run_flow`, E VA TENUTO IN PARI
//!
//! L'ordine deposito-poi-registro, la forma del `run_id`, i due `record_run`
//! attorno all'esecuzione, la traduzione da `Decision` a stato: sono le stesse
//! quaranta righe di `crates/sailor/src/flow_cmd.rs`, che sono funzioni private
//! di un binario e non si possono richiamare da qui. **Chi cambia quelle cambia
//! anche queste.** La duplicazione è dichiarata invece che nascosta perché
//! l'alternativa — rendere pubblico mezzo `flow_cmd` per un guscio che vive
//! fuori dal workspace — sposterebbe il problema senza chiuderlo.
//!
//! ## COSA SCORRE DAVVERO, E COSA NO
//!
//! Scorre lo **stato dei passi**: `append_started` e `close` passano di qui nel
//! momento esatto in cui il deposito li rende durevoli, e da lì l'evento arriva
//! alla finestra. Chi guarda vede un passo aprirsi e sa da quanto gira.
//!
//! Non scorre il **testo** che un passo produce, e non è una scelta di questo
//! modulo: `crates/actions/src/lib.rs` legge stdout del processo con
//! `read_to_end` su un thread, e quel buffer diventa leggibile solo al `join`,
//! cioè a processo finito. Il testo compare quindi tutto insieme alla chiusura
//! del passo. La finestra lo dichiara invece di far finta; qui resta scritto
//! **dove si cambierebbe**: `drain_and_wait`, sostituendo `read_to_end` con un
//! `BufReader::lines()` che spinga ogni riga in un canale.

use flow::{
    ActionRegistry, Completion, Decision, Execution, ExecutionRequest, Executor, FlowError,
    FlowFile, InProcessExecutor, RecordStore, SharedState, StepRecord, SystemClock,
};
use ledger::{Ledger, RunRecord};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

use ui::gather::default_ledger_dir;

/// Il canale su cui la finestra riceve quello che succede in una corsa.
pub const RUN_EVENT: &str = "sailor://run";

// ── quello che la finestra riceve ───────────────────────────────────────

/// Un fatto della corsa, numerato.
///
/// **IL NUMERO NON È DECORAZIONE.** Chi apre la vista chiede prima l'elenco di
/// quello che è già successo, poi si mette in ascolto: fra le due cose la corsa
/// continua, e un evento può arrivare due volte o cadere nel mezzo. Con `seq`
/// monotono per corsa chi ascolta scarta quello che ha già — senza, la vista
/// mostrerebbe due volte lo stesso passo e non avrebbe modo di accorgersene.
#[derive(Debug, Clone, Serialize)]
pub struct RunEvent {
    pub run_id: String,
    pub seq: u64,
    /// `step_started` | `step_closed` | `run_ended` | `note`
    pub kind: String,
    pub at: i64,
    pub step_id: Option<String>,
    pub payload: Value,
}

/// Lo stato di una corsa come lo vede chi si affaccia adesso.
#[derive(Debug, Clone, Serialize)]
pub struct RunSnapshot {
    pub run_id: String,
    pub flow: String,
    pub started_at: i64,
    /// `running` finché il thread lavora, poi lo stato finale del motore.
    pub status: String,
    pub events: Vec<RunEvent>,
}

/// Quello che il pulsante riceve indietro quando la corsa parte.
#[derive(Debug, Clone, Serialize)]
pub struct StartedRun {
    pub run_id: String,
    pub flow: String,
    pub started_at: i64,
}

/// Dove finisce il testo di chi preme il pulsante — o perché non c'è posto.
///
/// Si risponde **prima** di eseguire, perché una consegna che non ha dove
/// andare va detta mentre la si scrive, non dopo che il flusso è partito
/// ignorandola.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MandateTarget {
    /// Il testo entra in quel campo degli ingressi di quel passo.
    Field { step: String, field: String },
    /// Nessun posto dove metterlo, col motivo in chiaro.
    None { why: String },
}

/// Il nome dell'azione di innesco, e il campo in cui porta la consegna.
///
/// **LETTI DAL FLUSSO CHE IL MOTORE SCRIVE, NON DECISI QUI.** Il contratto è
/// nato il 28/08/2026 in `flows/smista-il-lavoro.flow.json`: un passo senza
/// dipendenze con `"action": "trigger"`, `"with": {"source": "manual"}`, e la
/// consegna in `inputs.<passo>.text`. Se quei nomi cambiano di là, cambiano
/// qui — e la finestra dirà «non c'è posto» invece di scrivere in un campo che
/// nessuno legge, che è il modo giusto di sbagliare.
const TRIGGER_ACTION: &str = "trigger";
const TRIGGER_FIELD: &str = "text";
const MANUAL_SOURCE: &str = "manual";

/// Il campo di testo dell'azione di un passo, quando ne ha uno.
///
/// Un motore esterno riceve la consegna sullo stdin del programma che invoca;
/// un nodo di innesco la porta nel proprio testo. Una verifica di shell riceve
/// un comando e non una consegna: infilarci dentro del testo scritto da una
/// persona sarebbe un'iniezione di shell, non una funzione.
fn text_field_of(action: &str) -> Option<&'static str> {
    match action {
        TRIGGER_ACTION => Some(TRIGGER_FIELD),
        "external_engine" => Some("stdin"),
        _ => None,
    }
}

/// Come si innesca un flusso: da dove parte, e se accetta una consegna.
#[derive(Debug, Clone, Serialize)]
pub struct FlowTrigger {
    pub flow: String,
    /// I passi senza dipendenze: da lì comincia il grafo.
    pub roots: Vec<String>,
    pub mandate: MandateTarget,
    /// Vero se il flusso ha una pianificazione propria: allora il pulsante non
    /// è l'unico modo in cui parte, e chi guarda deve saperlo.
    pub scheduled: bool,
}

// ── il registro delle corse vive nel guscio ─────────────────────────────

/// Una corsa in vita, con tutto quello che ha detto finora.
struct RunState {
    flow: String,
    started_at: i64,
    status: String,
    events: Vec<RunEvent>,
    next_seq: u64,
}

/// **LE CORSE NON APPARTENGONO ALLA PAGINA.** Vivono qui, nel guscio, per la
/// ragione che le rende utili: chi chiude il pannello della vista, cambia
/// flusso a fuoco o ricarica la pagina non deve fermare un lavoro che sta
/// girando, né perdere quello che è già stato detto. Il thread continua, gli
/// eventi si accumulano in questa mappa, e chi si riaffaccia li ritrova tutti.
#[derive(Default)]
pub struct Runs(Mutex<HashMap<String, RunState>>);

impl Runs {
    /// Aggiunge un fatto alla corsa e lo manda alla finestra, in quest'ordine.
    ///
    /// Prima si scrive nel registro, poi si annuncia: al contrario, chi
    /// ricevesse l'annuncio e chiedesse subito l'elenco potrebbe non trovarci
    /// dentro il fatto appena annunciato.
    fn publish(&self, app: &AppHandle, run_id: &str, kind: &str, step_id: Option<String>, payload: Value) {
        let event = {
            let mut runs = self.0.lock().expect("il registro delle corse non è avvelenato");
            let Some(state) = runs.get_mut(run_id) else {
                return;
            };
            let event = RunEvent {
                run_id: run_id.to_owned(),
                seq: state.next_seq,
                kind: kind.to_owned(),
                at: now_secs(),
                step_id,
                payload,
            };
            state.next_seq += 1;
            state.events.push(event.clone());
            event
        };
        // Fuori dal lucchetto: `emit` attraversa il ponte verso la finestra, e
        // tenerlo preso mentre lo fa bloccherebbe il thread della corsa.
        let _ = app.emit(RUN_EVENT, event);
    }

    fn set_status(&self, run_id: &str, status: &str) {
        if let Ok(mut runs) = self.0.lock() {
            if let Some(state) = runs.get_mut(run_id) {
                state.status = status.to_owned();
            }
        }
    }
}

// ── il deposito, guardato mentre scrive ─────────────────────────────────

/// Il deposito vero, con un testimone accanto.
///
/// **PERCHÉ UN DECORATORE E NON UN CONTROLLO A INTERVALLI.** La strada
/// alternativa era interrogare il deposito ogni tot: funzionerebbe — SQLite in
/// WAL regge le letture concorrenti — ma introdurrebbe un ritardo scelto a caso
/// e mostrerebbe un passo «partito» fino a mezzo secondo dopo che è partito.
/// `RecordStore` ha tre metodi: avvolgerlo costa venti righe e l'evento parte
/// nello stesso istante in cui il fatto diventa durevole.
///
/// **Il deposito prima, l'annuncio dopo.** Se la scrittura fallisce non si
/// annuncia niente: una finestra che mostra un passo che il deposito non ha mai
/// registrato racconta una corsa che non esiste.
struct WatchedStore {
    inner: Ledger,
    app: AppHandle,
    runs: Arc<Runs>,
    run_id: String,
}

impl RecordStore for WatchedStore {
    fn append_started(&mut self, record: StepRecord) -> Result<(), FlowError> {
        let announced = record.clone();
        self.inner.append_started(record)?;
        self.runs.publish(
            &self.app,
            &self.run_id,
            "step_started",
            Some(announced.step_id.clone()),
            serde_json::to_value(&announced).unwrap_or(Value::Null),
        );
        Ok(())
    }

    fn close(
        &mut self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), FlowError> {
        // `Completion` non è serializzabile: si compone a mano il fatto da
        // annunciare, con i campi che servono a chi guarda. `said` è il testo
        // grezzo del passo — l'unico testo che questo motore conserva.
        let announced = json!({
            "step_id": step_id,
            "attempt": attempt,
            "epoch": epoch,
            "outcome": format!("{:?}", completion.outcome),
            "output": completion.output,
            "said": completion.said,
            "failure_class": completion.failure_class,
            "ended_at": completion.ended_at,
            "bytes_seen": completion.bytes_seen,
            "bytes_discarded": completion.bytes_discarded,
        });
        self.inner.close(run_id, step_id, attempt, epoch, completion)?;
        self.runs.publish(
            &self.app,
            &self.run_id,
            "step_closed",
            Some(step_id.to_owned()),
            announced,
        );
        Ok(())
    }

    fn records(&self, run_id: &str) -> Result<Vec<StepRecord>, FlowError> {
        self.inner.records(run_id)
    }
}

// ── i comandi che la finestra chiama ────────────────────────────────────

/// Come si innesca questo flusso, e se accetta una consegna scritta a mano.
#[tauri::command]
pub(crate) fn flow_trigger(name: String) -> Result<FlowTrigger, String> {
    let flow = load_flow(&name)?;
    Ok(trigger_of(&flow))
}

/// Fa partire un flusso. Torna appena la corsa è avviata, non quando finisce.
#[tauri::command]
pub(crate) fn start_run(
    app: AppHandle,
    runs: State<'_, Arc<Runs>>,
    name: String,
    mandate: Option<String>,
) -> Result<StartedRun, String> {
    let flow = load_flow(&name)?;

    // IL DEPOSITO PRIMA DEL REGISTRO: `store_write` e `store_read` lo
    // possiedono, e un registro costruito prima dichiarerebbe mancanti due
    // azioni che esistono. È la stessa nota di `flow_cmd::run_flow`.
    let ledger_dir = default_ledger_dir();
    let ledger = Ledger::open(&ledger_dir).map_err(|error| {
        format!(
            "non riesco ad aprire il deposito {}: {error}",
            ledger_dir.display()
        )
    })?;
    // Il testimone resta `None`: la finestra vede i passi aprirsi e chiudersi
    // dal deposito (`WatchedStore`), non dal testo che esce dal motore mentre
    // gira. È il limite già dichiarato a chi guarda nella console — il testo
    // arriva tutto insieme alla chiusura — e si toglie da qui il giorno che si
    // toglie di là.
    let registry = default_registry(&ledger, None);
    let missing: Vec<&str> = flow
        .graph
        .steps()
        .iter()
        .filter(|step| registry.get(&step.action).is_none())
        .map(|step| step.action.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "il flusso {} nomina azioni che il motore non conosce: {}",
            flow.id,
            missing.join(", ")
        ));
    }

    // La consegna entra qui, in memoria, e non tocca il file sul disco: il
    // documento del flusso descrive il lavoro, non l'ultima volta che qualcuno
    // ha premuto il pulsante.
    let inputs = inputs_with_mandate(&flow, mandate.as_deref())?;

    let started_at = now_secs();
    let run_id = format!("{}-{}", flow.id, nanos());
    // DA DOVE È PARTITA LA CHIAMATA, scritto dal sistema nel momento in cui
    // parte. Non è un racconto di un agente: è il guscio che dichiara la
    // propria provenienza prima che qualunque passo giri, e resta nel deposito
    // append-only anche se la corsa si schianta al primo passo.
    let origin = origin_label(mandate.as_deref());
    record_run(&ledger, &flow, &run_id, "running", started_at, None, None, &origin)?;

    {
        let mut registro = runs.lock_map();
        registro.insert(
            run_id.clone(),
            RunState {
                flow: flow.id.clone(),
                started_at,
                status: "running".to_owned(),
                events: Vec::new(),
                next_seq: 0,
            },
        );
    }

    let handle = runs.inner().clone();
    let started = StartedRun {
        run_id: run_id.clone(),
        flow: flow.id.clone(),
        started_at,
    };

    // IL LAVORO NON STA SUL FILO DELLA FINESTRA. `execute` è bloccante e non
    // riporta niente finché non ha finito: lasciarlo sul thread che serve i
    // comandi congelerebbe l'interfaccia per tutta la durata della corsa —
    // mezz'ora, sui flussi che chiamano un agente.
    std::thread::spawn(move || {
        let mut store = WatchedStore {
            inner: ledger.clone(),
            app: app.clone(),
            runs: handle.clone(),
            run_id: run_id.clone(),
        };
        let request = ExecutionRequest {
            run_id: run_id.clone(),
            root_inputs: inputs,
            gates: Vec::new(),
            shared: SharedState::new(),
        };
        let result = InProcessExecutor.execute(
            &flow.graph,
            request,
            &mut store,
            &registry,
            &mut SystemClock,
        );

        let ended_at = now_secs();
        let (status, error) = match &result {
            Ok(execution) => (execution_status(execution).to_owned(), None),
            Err(failure) => ("failed".to_owned(), Some(failure.to_string())),
        };
        let _ = record_run(
            &ledger,
            &flow,
            &run_id,
            &status,
            started_at,
            Some(ended_at),
            error.clone(),
            &origin,
        );
        handle.set_status(&run_id, &status);
        handle.publish(
            &app,
            &run_id,
            "run_ended",
            None,
            json!({ "status": status, "error": error, "ended_at": ended_at }),
        );
    });

    Ok(started)
}

/// Tutto quello che una corsa ha detto finora, per chi si affaccia adesso.
#[tauri::command]
pub(crate) fn run_snapshot(runs: State<'_, Arc<Runs>>, run_id: String) -> Result<RunSnapshot, String> {
    let registro = runs.lock_map();
    let state = registro
        .get(&run_id)
        .ok_or_else(|| format!("la corsa {run_id} non è nota a questa finestra"))?;
    Ok(RunSnapshot {
        run_id: run_id.clone(),
        flow: state.flow.clone(),
        started_at: state.started_at,
        status: state.status.clone(),
        events: state.events.clone(),
    })
}

/// Le corse che questa finestra ha avviato, la più recente per ultima.
///
/// Serve a chi ricarica la pagina mentre un flusso gira: senza questo elenco la
/// vista ripartirebbe vuota e la corsa continuerebbe senza nessuno che la
/// guarda.
#[tauri::command]
pub(crate) fn known_runs(runs: State<'_, Arc<Runs>>) -> Vec<RunSnapshot> {
    let registro = runs.lock_map();
    let mut all: Vec<RunSnapshot> = registro
        .iter()
        .map(|(run_id, state)| RunSnapshot {
            run_id: run_id.clone(),
            flow: state.flow.clone(),
            started_at: state.started_at,
            status: state.status.clone(),
            events: state.events.clone(),
        })
        .collect();
    all.sort_by_key(|snapshot| snapshot.started_at);
    all
}

impl Runs {
    fn lock_map(&self) -> std::sync::MutexGuard<'_, HashMap<String, RunState>> {
        self.0.lock().expect("il registro delle corse non è avvelenato")
    }
}

// ── cosa è entrato in un nodo, nel tempo ────────────────────────────────

/// Una volta in cui un passo è stato attraversato.
///
/// **VIENE TUTTO DAL DEPOSITO, NIENTE DALLA MEMORIA DELLA FINESTRA.** Le corse
/// che questa finestra ha avviato sono una manciata; quelle che quel nodo ha
/// visto passare possono essere centinaia, avviate da riga di comando, da una
/// pianificazione o da una finestra chiusa mesi fa. Leggere dalla propria
/// memoria darebbe una storia che comincia all'apertura del programma — cioè
/// una storia che sembra completa e non lo è.
#[derive(Debug, Clone, Serialize)]
pub struct StepPassage {
    pub run_id: String,
    pub attempt: u32,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub outcome: Option<String>,
    pub failure_class: Option<String>,
    /// Da dove è partita la corsa: la provenienza, scritta dal sistema.
    pub started_by: String,
    /// Che cosa è entrato in **questo** nodo, quella volta.
    pub input: Value,
    /// La consegna con cui è partita la corsa, se ne portava una.
    pub mandate: Option<String>,
    /// Chi ha mandato il segnale, per come la sorgente lo sapeva. Vuoto quando
    /// non lo sapeva: è un fatto, non un campo da riempire.
    pub signal_who: Option<String>,
    /// Da dove è arrivato il segnale: la finestra, un pannello, una sessione.
    pub signal_where: Option<String>,
    pub said: Option<String>,
    pub output: Option<Value>,
}

/// Tutto quello che è passato per un nodo, dal più recente.
///
/// Quanto è costata una corsa: token, cache e denaro, dal deposito.
///
/// **NON RIFÀ NESSUN CONTO.** I totali li calcola `ui::dashboard`, che è già
/// puro e già provato, ed è lo stesso codice che serve la pagina di `sailor ui`.
/// Due viste della stessa spesa che si sommano da sole darebbero due cifre, e la
/// domanda «quale delle due è giusta» non avrebbe risposta.
///
/// **UN TOTALE PARZIALE SI DICHIARA.** `TokenTotals::is_partial` dice se qualche
/// chiamata non ha detto i propri conteggi o non ha un prezzo: chi mostra questi
/// numeri deve mostrare anche quello, o sta presentando una somma che nasconde
/// ciò che non sa. Sul deposito assente si torna `None`, non un errore: un
/// programma che non ha ancora eseguito niente non è guasto.
///
/// **SI LEGGE A RICHIESTA, A CORSA FINITA O QUANDO QUALCUNO GUARDA.** Aprire il
/// deposito e scorrere le corse non è lavoro da fare a ogni battito.
#[tauri::command]
pub(crate) fn run_usage(run_id: String) -> Result<Option<ui::dashboard::ExecutionView>, String> {
    let ledger_dir = default_ledger_dir();
    let Some(data) = ui::gather::gather(&ledger_dir)
        .map_err(|error| format!("non riesco a leggere il deposito: {error}"))?
    else {
        return Ok(None);
    };
    let Some(run) = data.runs.iter().find(|run| run.run_id == run_id) else {
        // Una corsa appena avviata può non essere ancora nella proiezione: non è
        // un errore, è «non ancora», e la finestra riprova al battito dopo.
        return Ok(None);
    };
    let steps = data.steps_by_run.get(&run_id).cloned().unwrap_or_default();
    let calls = data.calls_by_run.get(&run_id).cloned().unwrap_or_default();
    Ok(Some(ui::dashboard::summarize_run(
        run,
        &steps,
        &calls,
        now_secs(),
    )))
}

/// **SI LEGGE A RICHIESTA, NON DI CONTINUO.** Ricostruire questa storia
/// significa aprire il deposito e scorrere le corse: è il genere di lavoro che
/// si fa quando qualcuno clicca un nodo, non a ogni battito di una corsa che
/// sta girando.
#[tauri::command]
pub(crate) fn step_history(
    flow: String,
    step: String,
    limit: Option<usize>,
) -> Result<Vec<StepPassage>, String> {
    let ledger_dir = default_ledger_dir();
    let Some(data) = ui::gather::gather(&ledger_dir)
        .map_err(|error| format!("non riesco a leggere il deposito: {error}"))?
    else {
        // Nessun deposito non è un guasto: è un programma che non ha ancora
        // eseguito niente, e dirlo come errore manderebbe a cercare un guasto
        // che non c'è.
        return Ok(Vec::new());
    };

    let mut passages = Vec::new();
    for run in &data.runs {
        if run.entity != flow {
            continue;
        }
        let Some(steps) = data.steps_by_run.get(&run.run_id) else {
            continue;
        };
        let signal = signal_of_run(steps);
        for record in steps {
            if record.step_id != step {
                continue;
            }
            passages.push(StepPassage {
                run_id: record.run_id.clone(),
                attempt: record.attempt,
                started_at: record.started_at,
                ended_at: record.ended_at,
                outcome: record.outcome.map(|outcome| format!("{outcome:?}")),
                failure_class: record.failure_class.clone(),
                started_by: run.started_by.clone(),
                input: record.input.clone(),
                mandate: signal.text.clone(),
                signal_who: signal.who.clone(),
                signal_where: signal.where_from.clone(),
                said: record.said.clone(),
                output: record.output.clone(),
            });
        }
    }

    // Dal più recente: chi apre questo elenco cerca quasi sempre l'ultima volta.
    passages.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then(b.attempt.cmp(&a.attempt))
    });
    passages.truncate(limit.unwrap_or(25));
    Ok(passages)
}

/// Che cosa portava il segnale con cui è partita una corsa.
///
/// Il deposito registra l'ingresso di ogni passo nel momento in cui si apre,
/// quindi **il segnale è già scritto dal sistema** e non dipende dal fatto che
/// un modello abbia deciso di raccontarlo. Qui non si registra niente di nuovo:
/// si legge quello che c'era.
///
/// `who` e `where` sono i campi che un nodo di innesco porta con sé. Un campo
/// vuoto resta `None` e non una stringa vuota: la finestra deve poter tacere su
/// quello che il segnale non sapeva, invece di mostrare un'etichetta senza
/// valore accanto.
#[derive(Debug, Default, Clone)]
struct RunSignal {
    text: Option<String>,
    who: Option<String>,
    where_from: Option<String>,
}

fn signal_of_run(steps: &[StepRecord]) -> RunSignal {
    let Some(root) = steps.iter().find(|record| record.deps.is_empty()) else {
        return RunSignal::default();
    };
    let Value::Object(input) = &root.input else {
        return RunSignal::default();
    };

    fn text_at(input: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
        match input.get(key) {
            Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
            _ => None,
        }
    }

    RunSignal {
        // I due campi in cui una consegna può essere entrata, nello stesso
        // ordine in cui `text_field_of` li sceglie.
        text: text_at(input, TRIGGER_FIELD).or_else(|| text_at(input, "stdin")),
        who: text_at(input, "who"),
        where_from: text_at(input, "where"),
    }
}

// ── le regole, tenute fuori dai comandi per poterle provare ─────────────

/// I passi da cui il grafo comincia: quelli che non aspettano nessuno.
fn roots_of(flow: &FlowFile) -> Vec<String> {
    flow.graph
        .steps()
        .iter()
        .filter(|step| step.deps.is_empty())
        .map(|step| step.id.clone())
        .collect()
}

/// Dove va a finire il testo di chi preme il pulsante.
///
/// **SI NEGA INVECE DI INDOVINARE.** Un mandato messo nel posto sbagliato non
/// dà errore: il flusso parte e lavora sulla consegna di ieri, e chi ha
/// premuto crede di aver dato la sua. I casi in cui non c'è un posto certo
/// tornano il motivo, che la finestra mostra accanto al campo prima ancora che
/// qualcuno ci scriva dentro.
fn mandate_target(flow: &FlowFile) -> MandateTarget {
    let roots = roots_of(flow);
    let [root] = roots.as_slice() else {
        return MandateTarget::None {
            why: if roots.is_empty() {
                "il flusso non ha un passo di partenza: ogni passo aspetta qualcun altro".to_owned()
            } else {
                format!(
                    "il flusso parte da {} passi ({}), e una consegna sola non dice a quale va",
                    roots.len(),
                    roots.join(", ")
                )
            },
        };
    };

    let Some(step) = flow.graph.steps().iter().find(|step| &step.id == root) else {
        return MandateTarget::None {
            why: "il passo di partenza non si trova nel grafo".to_owned(),
        };
    };

    let Some(field) = text_field_of(&step.action) else {
        return MandateTarget::None {
            why: format!(
                "il passo di partenza «{root}» esegue «{}», che non ha un ingresso di testo",
                step.action
            ),
        };
    };

    // Un innesco dichiara da dove arriva il segnale. Se non è il gesto di una
    // persona, un pulsante che promette di darglielo prometterebbe una cosa
    // che quel passo non aspetta da qui.
    if step.action == TRIGGER_ACTION {
        if let Some(Value::Object(fixed)) = &step.with {
            match fixed.get("source") {
                Some(Value::String(source)) if source == MANUAL_SOURCE => {}
                Some(Value::String(source)) => {
                    return MandateTarget::None {
                        why: format!(
                            "l'innesco «{root}» aspetta un segnale di tipo «{source}», non il gesto di una persona"
                        ),
                    };
                }
                _ => {}
            }
        }
    }

    // `with` vince sulle chiavi ricevute in ingresso: se dichiara già quel
    // campo, la consegna verrebbe scritta e poi scavalcata senza un errore. È
    // il modo peggiore di perdere un testo, e si nega prima.
    if let Some(Value::Object(fixed)) = &step.with {
        if fixed.contains_key(field) {
            return MandateTarget::None {
                why: format!(
                    "il passo «{root}» dichiara già il proprio «{field}» nei parametri fissi: \
                     una consegna scritta qui verrebbe scavalcata senza dirlo"
                ),
            };
        }
    }

    MandateTarget::Field {
        step: root.clone(),
        field: field.to_owned(),
    }
}

fn trigger_of(flow: &FlowFile) -> FlowTrigger {
    FlowTrigger {
        flow: flow.id.clone(),
        roots: roots_of(flow),
        mandate: mandate_target(flow),
        scheduled: flow.schedule.is_some(),
    }
}

/// Gli ingressi della corsa, con la consegna dentro se ce n'è una.
///
/// Una consegna che non ha dove andare **ferma la partenza**: eseguire
/// ignorandola darebbe una corsa che sembra aver ricevuto il testo e ha
/// lavorato su altro.
fn inputs_with_mandate(
    flow: &FlowFile,
    mandate: Option<&str>,
) -> Result<std::collections::BTreeMap<String, Value>, String> {
    let mut inputs = flow.inputs.clone();
    let Some(text) = mandate.filter(|text| !text.trim().is_empty()) else {
        return Ok(inputs);
    };

    match mandate_target(flow) {
        MandateTarget::Field { step, field } => {
            let entry = inputs.entry(step).or_insert_with(|| json!({}));
            match entry {
                Value::Object(map) => {
                    let is_trigger = field == TRIGGER_FIELD;
                    map.insert(field, Value::String(text.to_owned()));
                    // CHI E DA DOVE, per come questa sorgente lo sa. Un innesco
                    // registra `who` e `where` insieme al testo, e riempirli qui
                    // è ciò che rende il segnale rintracciabile a distanza di
                    // mesi: senza, resta scritto *cosa* è arrivato e non da chi.
                    //
                    // Solo per un nodo di innesco: un motore esterno non ha
                    // quei campi, e scriverglieli dentro sarebbe inventare un
                    // parametro che la sua azione non legge.
                    if is_trigger {
                        map.entry("who".to_owned()).or_insert(Value::String(who()));
                        map.entry("where".to_owned())
                            .or_insert(Value::String(WHERE.to_owned()));
                    }
                    Ok(inputs)
                }
                _ => Err(
                    "gli ingressi del passo di partenza non sono un oggetto: non c'è dove \
                     scrivere la consegna"
                        .to_owned(),
                ),
            }
        }
        MandateTarget::None { why } => Err(format!("questo flusso non accetta una consegna: {why}")),
    }
}

/// La stessa traduzione di `flow_cmd::execution_status`, senza il booleano che
/// lì serve al codice d'uscita del processo.
fn execution_status(execution: &Execution) -> &'static str {
    match execution.decisions.last() {
        Some(Decision::Complete) => "complete",
        Some(Decision::Waiting(_)) => "waiting",
        Some(Decision::Stopped(_)) => "stopped",
        Some(Decision::Failed(_)) => "failed",
        Some(Decision::Ready(_)) | Some(Decision::Running(_)) | None => "incomplete",
    }
}

/// Le azioni che il motore sa eseguire: **la stessa lista del terminale**.
///
/// **QUESTA LISTA SI DISALLINEAVA, E L'AVEVA GIÀ FATTO TRE VOLTE.** Qui c'era
/// una copia a mano di quella del comando `sailor flow run`, tenute allineate
/// dalla buona volontà. Il 28/08/2026 nacque il crate `trigger`, registrato di
/// là e non di qua: il pulsante rispondeva «azione sconosciuta: trigger» su un
/// flusso che dal terminale partiva. Il 30/08/2026 alle 09:05 è successo di
/// nuovo con la misura del consumo, e stavolta in silenzio — la finestra
/// costruiva un motore **senza risolutore di strumenti** (ogni passo che nomina
/// `claude-code` invece di un percorso cadeva con `no_tool_resolver`) e
/// **senza deposito** (nessuna riga di costo per le corse lanciate da qui).
///
/// Ora la lista è una sola e sta in `crates/registry`. Il commento che stava
/// qui diceva «chi registra un'azione nuova la registra in tutti e due i
/// posti»: era l'istruzione giusta per un difetto che andava tolto, non
/// rispettato.
fn default_registry(ledger: &Ledger, watcher: Option<Arc<dyn actions::StepSinks>>) -> ActionRegistry {
    registry::default_registry(Some(ledger.clone()), watcher)
}

/// Da dove arriva il segnale che questo guscio manda. È la finestra, sempre:
/// non è un dato da indovinare, è quello che questo programma è.
const WHERE: &str = "finestra di Sailor";

/// Chi ha premuto, **per come questa sorgente lo sa**, che è poco: la finestra
/// non ha un'identità di persona — non c'è login, non c'è account — e l'unica
/// cosa vera a disposizione è l'utente con cui il programma gira.
///
/// **SI TORNA VUOTO PIUTTOSTO CHE INVENTARE.** `Signal` dichiara che un segnale
/// che non sa chi l'ha mandato lo dice con una stringa vuota; scrivere lì un
/// nome plausibile ma non verificato sarebbe peggio che ammettere di non
/// saperlo, perché nessuno andrebbe più a controllare.
fn who() -> String {
    std::env::var("USER").unwrap_or_default()
}

/// Da dove è partita una corsa, in una riga che si legge senza decodificarla.
///
/// **PORTA LA PROVENIENZA, NON IL CONTENUTO.** Dice che qualcuno ha premuto in
/// questa finestra e se ha allegato una consegna; il testo della consegna non
/// entra qui. Il testo è già registrato dove deve stare — negli ingressi del
/// passo che lo riceve — e ricopiarlo anche nell'etichetta della corsa
/// significherebbe scriverlo due volte in posti con regole diverse.
fn origin_label(mandate: Option<&str>) -> String {
    let carried = mandate.is_some_and(|text| !text.trim().is_empty());
    if carried {
        "finestra · innesco manuale, con consegna".to_owned()
    } else {
        "finestra · innesco manuale".to_owned()
    }
}

#[allow(clippy::too_many_arguments)]
fn record_run(
    ledger: &Ledger,
    flow: &FlowFile,
    run_id: &str,
    status: &str,
    started_at: i64,
    ended_at: Option<i64>,
    error: Option<String>,
    started_by: &str,
) -> Result<(), String> {
    ledger
        .record_run(&RunRecord {
            run_id: run_id.to_owned(),
            kind: "flow".to_owned(),
            entity: flow.id.clone(),
            parent_run_id: None,
            // Chi ha avviato questa corsa si legge nel deposito: una corsa
            // partita dal pulsante non si confonde con una partita dalla riga
            // di comando o da una pianificazione.
            started_by: started_by.to_owned(),
            status: status.to_owned(),
            total_cost_micros: 0,
            error,
            started_at,
            ended_at,
        })
        .map_err(|error| format!("non riesco a registrare la corsa {run_id}: {error}"))
}

/// Il flusso che si chiama così, cercato **dove la tela lo ha trovato**.
///
/// **IL DIFETTO CHE QUESTA FUNZIONE AVEVA, E COSA SI VEDEVA DA FUORI.** Fino al
/// 30/08/2026 qui il nome diventava un percorso dentro `default_flows_dir()` —
/// `~/.config/sailor/flows`, una cartella sola — mentre l'elenco che la finestra
/// disegna viene da tre sorgenti: quelli spediti dentro il binario, quelli di
/// casa e quelli del progetto. Su questa macchina i sette flussi esistenti
/// stanno nelle altre due, e quella cartella non esiste nemmeno. Risultato:
/// `flow_trigger` falliva su ognuno, ogni innesco restava `mute`, e **il
/// pulsante ▶ Esegui era grigio su tutti i nodi**. Da fuori era indistinguibile
/// da un pulsante non collegato a niente — che è come è stato descritto per due
/// giorni, mentre il collegamento c'era ed era intero.
///
/// **E IL NOME NON DIVENTA PIÙ UN PERCORSO.** Cercandolo in un elenco già
/// costruito, un nome che quell'elenco non contiene non apre niente: non c'è
/// nessun posto da cui scappare, e il controllo che serviva prima — `safe_name`
/// — se ne va con la ragione che lo teneva in vita. È la stessa scelta già
/// motivata in `flow_cmd::known_flows`, e ora le due strade la condividono.
fn load_flow(name: &str) -> Result<FlowFile, String> {
    let known = ui::gather::load_all_flows(&ui::gather::flow_sources());
    match known.iter().find(|(known, _, _)| known == name) {
        Some((_, _, Ok(flow))) => Ok(flow.clone()),
        Some((_, origin, Err(reason))) => {
            Err(format!("il flusso «{name}» ({origin}) non si carica: {reason}"))
        }
        None => {
            let names: Vec<&str> = known.iter().map(|(name, _, _)| name.as_str()).collect();
            Err(format!(
                "nessun flusso si chiama «{name}»; quelli che vedo sono: {}",
                if names.is_empty() {
                    "nessuno".to_owned()
                } else {
                    names.join(", ")
                }
            ))
        }
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn flow_with(steps: Value, inputs: Value) -> FlowFile {
        serde_json::from_value(json!({
            "id": "prova",
            "description": "",
            "graph": { "steps": steps },
            "inputs": inputs,
        }))
        .expect("il flusso di prova si carica")
    }

    fn engine_step(id: &str, deps: Vec<&str>, with: Value) -> Value {
        json!({
            "id": id,
            "deps": deps,
            "action": "external_engine",
            "max_attempts": 1,
            "when": null,
            "with": with,
            "input_schema": { "type": "any" },
            "output_schema": { "type": "any" }
        })
    }

    #[test]
    fn a_single_engine_root_takes_the_mandate_on_its_stdin() {
        let flow = flow_with(
            json!([engine_step("dispatch", vec![], json!({ "bin": "true", "timeout_secs": 5 }))]),
            json!({ "dispatch": { "bin": "true", "timeout_secs": 5 } }),
        );
        match mandate_target(&flow) {
            MandateTarget::Field { step, field } => {
                assert_eq!(step, "dispatch");
                assert_eq!(field, "stdin");
            }
            other => panic!("atteso un bersaglio, trovato {other:?}"),
        }

        let inputs = inputs_with_mandate(&flow, Some("fai questa cosa")).expect("consegna accolta");
        assert_eq!(
            inputs["dispatch"]["stdin"],
            json!("fai questa cosa"),
            "la consegna deve entrare nello stdin del passo di partenza"
        );
    }

    /// LA MISURA CHE POTEVA VENIRE DIVERSA: il passo dichiara già `stdin` nei
    /// parametri fissi, che vincono sugli ingressi. Senza il controllo in
    /// `mandate_target` la consegna verrebbe scritta negli ingressi, scavalcata
    /// da `with` all'esecuzione, e persa senza un errore: chi ha premuto
    /// crederebbe di aver dato il proprio testo. Tolto quel controllo, questa
    /// prova diventa rossa.
    #[test]
    fn a_root_that_fixes_its_own_stdin_refuses_the_mandate_instead_of_losing_it() {
        let flow = flow_with(
            json!([engine_step(
                "dispatch",
                vec![],
                json!({ "bin": "true", "stdin": "gia' deciso", "timeout_secs": 5 })
            )]),
            json!({}),
        );
        match mandate_target(&flow) {
            MandateTarget::None { why } => assert!(why.contains("scavalcata"), "{why}"),
            other => panic!("atteso un rifiuto, trovato {other:?}"),
        }
        let error = inputs_with_mandate(&flow, Some("la mia consegna"))
            .expect_err("una consegna che verrebbe persa ferma la partenza");
        assert!(error.contains("scavalcata"), "{error}");
    }

    /// Una verifica di shell riceve un comando, non un testo scritto da una
    /// persona: infilarcelo dentro sarebbe un'iniezione di shell.
    #[test]
    fn a_shell_root_has_no_place_for_a_mandate() {
        let flow = flow_with(
            json!([{
                "id": "solo", "deps": [], "action": "shell_check", "max_attempts": 1,
                "when": null, "input_schema": { "type": "any" }, "output_schema": { "type": "any" }
            }]),
            json!({ "solo": { "command": "true", "timeout_secs": 5 } }),
        );
        match mandate_target(&flow) {
            MandateTarget::None { why } => assert!(why.contains("ingresso di testo"), "{why}"),
            other => panic!("atteso un rifiuto, trovato {other:?}"),
        }
    }

    #[test]
    fn two_roots_leave_the_mandate_without_an_address() {
        let flow = flow_with(
            json!([
                engine_step("uno", vec![], json!({ "bin": "true", "timeout_secs": 5 })),
                engine_step("due", vec![], json!({ "bin": "true", "timeout_secs": 5 })),
            ]),
            json!({}),
        );
        match mandate_target(&flow) {
            MandateTarget::None { why } => assert!(why.contains("2 passi"), "{why}"),
            other => panic!("atteso un rifiuto, trovato {other:?}"),
        }
    }

    /// Nessuna consegna scritta: il flusso parte con i propri ingressi tali e
    /// quali, anche quando non avrebbe dove metterne una.
    #[test]
    fn no_mandate_leaves_the_declared_inputs_untouched() {
        let flow = flow_with(
            json!([{
                "id": "solo", "deps": [], "action": "shell_check", "max_attempts": 1,
                "when": null, "input_schema": { "type": "any" }, "output_schema": { "type": "any" }
            }]),
            json!({ "solo": { "command": "true", "timeout_secs": 5 } }),
        );
        let inputs = inputs_with_mandate(&flow, None).expect("nessuna consegna, nessun problema");
        assert_eq!(inputs["solo"]["command"], json!("true"));
        // Uno spazio bianco non è una consegna: sarebbe un rifiuto per un testo
        // che nessuno ha davvero scritto.
        let blank = inputs_with_mandate(&flow, Some("   ")).expect("il bianco non è una consegna");
        assert_eq!(blank["solo"]["command"], json!("true"));
    }

    fn trigger_step(id: &str, with: Value) -> Value {
        json!({
            "id": id,
            "deps": [],
            "action": "trigger",
            "max_attempts": 1,
            "when": null,
            "with": with,
            "input_schema": { "type": "any" },
            "output_schema": { "type": "any" }
        })
    }

    /// IL CONTRATTO VERO, letto da `flows/smista-il-lavoro.flow.json` il
    /// 28/08/2026: un passo `trigger` di sorgente manuale porta la consegna nel
    /// proprio `text`, non in uno `stdin`.
    ///
    /// LA MISURA CHE POTEVA VENIRE DIVERSA: se `text_field_of` non conoscesse
    /// l'azione `trigger`, la consegna finirebbe rifiutata su un flusso che la
    /// aspetta — e il pulsante direbbe «non c'è posto» davanti a un nodo nato
    /// apposta per riceverla.
    #[test]
    fn a_manual_trigger_root_takes_the_mandate_in_its_own_text() {
        let flow = flow_with(
            json!([
                trigger_step("trigger", json!({ "source": "manual" })),
                engine_step("dispatch", vec!["trigger"], json!({ "bin": "true", "timeout_secs": 5 })),
            ]),
            json!({ "trigger": { "text": "la consegna di ieri" } }),
        );
        match mandate_target(&flow) {
            MandateTarget::Field { step, field } => {
                assert_eq!(step, "trigger");
                assert_eq!(field, "text");
            }
            other => panic!("atteso il campo dell'innesco, trovato {other:?}"),
        }

        let inputs = inputs_with_mandate(&flow, Some("la consegna di oggi")).expect("accolta");
        assert_eq!(
            inputs["trigger"]["text"],
            json!("la consegna di oggi"),
            "la consegna di chi preme deve sostituire quella scritta nel file"
        );
    }

    /// Un innesco che aspetta un segnale che non è il gesto di una persona non
    /// riceve una consegna scritta a mano: il pulsante prometterebbe una cosa
    /// che quel passo non aspetta da lì.
    #[test]
    fn a_trigger_waiting_for_another_kind_of_signal_refuses_the_mandate() {
        let flow = flow_with(
            json!([trigger_step("trigger", json!({ "source": "schedule" }))]),
            json!({}),
        );
        match mandate_target(&flow) {
            MandateTarget::None { why } => assert!(why.contains("schedule"), "{why}"),
            other => panic!("atteso un rifiuto, trovato {other:?}"),
        }
    }

    /// **OGNI AZIONE DEI FLUSSI SPEDITI DEV'ESSERE NEL REGISTRO DELLA FINESTRA.**
    ///
    /// È lo stesso controllo che `start_run` fa prima di eseguire (e che
    /// risponde «il flusso nomina azioni che il motore non conosce»), qui fatto
    /// sui flussi che stanno dentro il binario. Prima del 30/08/2026 il guscio
    /// costruiva una lista sua, più corta di quella del terminale: mancavano
    /// `tool_needs` e il risolutore degli strumenti, quindi un flusso spedito
    /// col prodotto veniva rifiutato dalla finestra e accettato dal terminale.
    ///
    /// Il deposito qui non serve: le azioni che mancavano non sono quelle che
    /// scrivono.
    #[test]
    fn every_action_of_a_shipped_flow_is_known_to_the_window() {
        let registro = registry::default_registry(None, None);
        for nome in ["strumenti-di-questa-macchina", "migrazione-a-sailor"] {
            let flow = load_flow(nome).expect("i flussi di sistema si caricano");
            for step in flow.graph.steps() {
                assert!(
                    registro.get(&step.action).is_some(),
                    "«{}» nomina l'azione «{}», che la finestra non conosce",
                    nome,
                    step.action
                );
            }
        }
    }

    /// **LA PROVA DELLA RIPARAZIONE, E NON LEGGE LA MACCHINA DI NESSUNO.**
    ///
    /// I flussi di sistema stanno **dentro il binario**: ci sono su qualunque
    /// macchina, anche su una appena installata, anche dove `~/.config/sailor`
    /// non esiste. Prima del 30/08/2026 questa `load_flow` non ne trovava
    /// nemmeno uno — cercava in una cartella sola, e non era quella — quindi
    /// `flow_trigger` falliva, l'innesco restava muto e il pulsante ▶ Esegui era
    /// grigio su ogni nodo della tela.
    ///
    /// Rimettendo `default_flows_dir()` al posto dell'elenco, questa prova
    /// diventa rossa (provato).
    #[test]
    fn a_flow_shipped_inside_the_binary_is_loadable_from_the_window() {
        let flow = load_flow("strumenti-di-questa-macchina")
            .expect("un flusso di sistema si carica ovunque, senza niente sul disco");
        assert!(
            !flow.graph.steps().is_empty(),
            "e arriva col suo grafo, non come guscio vuoto"
        );
    }

    /// **LA STESSA GARANZIA DI PRIMA, OTTENUTA IN UN ALTRO MODO.** Qui c'era una
    /// prova su `safe_name`, il controllo che impediva a un nome di uscire dalla
    /// cartella quando il nome diventava un percorso. Adesso il nome si cerca in
    /// un elenco: non apre niente per costruzione, e `safe_name` non esiste più.
    /// La prova resta, perché la cosa da garantire è la stessa — un nome storto
    /// non deve leggere niente — ed è il comportamento che si prova, non la
    /// funzione che lo otteneva.
    #[test]
    fn a_flow_name_that_climbs_out_of_the_directory_opens_nothing() {
        for storto in ["../evaso", "sotto/cartella", "", "/etc/passwd"] {
            let esito = load_flow(storto);
            assert!(
                esito.is_err(),
                "«{storto}» non è il nome di nessun flusso: non deve caricare niente"
            );
            let why = esito.unwrap_err();
            assert!(
                why.contains("nessun flusso si chiama"),
                "e il motivo dev'essere che non è in elenco, non un errore di lettura: {why}"
            );
        }
    }
}
