//! Starting a flow from the window, and watching it run.
//!
//! **WHY THE SHELL EXECUTES INSTEAD OF LAUNCHING THE BINARY.** Measured:
//! `sailor flow run` gives one line, at the end — twelve seconds of silence
//! and then a verdict, which is the worst thing an execution view can be,
//! because it looks broken. And the binary takes no mandate: a delivery would
//! mean rewriting the flow on disk, so this morning's would stay in tomorrow's
//! file. Executing here costs a thread and gives both.
//!
//! **IT MIRRORS `flow_cmd::run_flow`, AND THE TWO ARE KEPT IN STEP.** The
//! ledger-then-registry order, the shape of a `run_id`, the two `record_run`
//! around the execution: the same forty lines, private to a binary and
//! unreachable from here. The duplication is declared rather than hidden.
//!
//! What streams is the **state** of the steps, at the instant the ledger makes
//! it durable. The **text** a step produces does not, and not by a choice made
//! here: `actions` reads stdout with `read_to_end` on a thread, and that buffer
//! is readable only at the join. Where it would change: `drain_and_wait`.

// `Decision` non compare più: la traduzione da decisione a stato è passata in
// `registry` insieme alla sua gemella della riga di comando, e l'importazione
// era rimasta. Nessuno l'aveva vista perché questo guscio sta fuori dal
// workspace, quindi i suoi avvisi non li stampa `cargo test --workspace`.
use actions::{LiveSink, Pipe, StepSinks};
use flow::{
    ActionRegistry, Completion, Execution, Executor, FlowError, FlowFile, InProcessExecutor,
    Ran, RecordStore, Refusal, StepRecord, SystemClock,
};
use ledger::Ledger;
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
    /// `step_started` | `step_text` | `step_closed` | `run_ended` | `note`
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
/// nato il 28/08/2026 in `flows/dispatch-the-work.flow.json`: un passo senza
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
    /// Somebody pressed stop: the executor reads it before the next front.
    halt: bool,
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
    fn publish(
        &self,
        app: &AppHandle,
        run_id: &str,
        kind: &str,
        step_id: Option<String>,
        payload: Value,
    ) {
        let event = {
            let mut runs = self
                .0
                .lock()
                .expect("il registro delle corse non è avvelenato");
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
        let _ = app.emit(RUN_EVENT, &event);
        crate::events::emit(app, "run", &event);
    }

    fn set_status(&self, run_id: &str, status: &str) {
        if let Ok(mut runs) = self.0.lock() {
            if let Some(state) = runs.get_mut(run_id) {
                state.status = status.to_owned();
            }
        }
    }

    /// Marks the run to stop before its next front. Refused, with the reason,
    /// for a run this window does not hold or one that has already ended.
    pub(crate) fn request_halt(&self, run_id: &str) -> Result<(), String> {
        let mut runs = self.lock_map();
        let Some(state) = runs.get_mut(run_id) else {
            return Err(format!("no run {run_id} in this window"));
        };
        if state.status != "running" {
            return Err(format!("run {run_id} is not running: {}", state.status));
        }
        state.halt = true;
        Ok(())
    }

    pub(crate) fn halt_requested(&self, run_id: &str) -> bool {
        self.lock_map().get(run_id).is_some_and(|state| state.halt)
    }
}

// ── the text of a running step ──────────────────────────────────────────

/// One step's bytes on their way to the window.
///
/// A pipe breaks wherever it happens to break — sometimes halfway through a
/// character. The tail of an incomplete one is held until the rest arrives, so
/// an accented letter never reaches the window as a replacement mark; past four
/// bytes it is not a split character, and goes out as it is.
struct StepText {
    step: String,
    emit: Arc<dyn Fn(&str, Pipe, String) + Send + Sync>,
    /// One per pipe: stdout and stderr are drained by two threads, and a single
    /// buffer would splice one's tail onto the other's head.
    tails: Mutex<(Vec<u8>, Vec<u8>)>,
}

impl LiveSink for StepText {
    fn chunk(&self, pipe: Pipe, bytes: &[u8]) {
        let text = {
            let mut tails = self.tails.lock().expect("the tails are not poisoned");
            let buffer = match pipe {
                Pipe::Stdout => &mut tails.0,
                Pipe::Stderr => &mut tails.1,
            };
            buffer.extend_from_slice(bytes);
            take_whole_characters(buffer)
        };
        if !text.is_empty() {
            (self.emit)(&self.step, pipe, text);
        }
    }
}

/// The longest prefix of `buffer` that is whole text; the rest stays behind.
fn take_whole_characters(buffer: &mut Vec<u8>) -> String {
    let whole = match std::str::from_utf8(buffer) {
        Ok(_) => buffer.len(),
        Err(error) => error.valid_up_to(),
    };
    // Four bytes is the longest a character can be, so a longer tail is not one
    // waiting to be completed: holding it would silence the pipe for good.
    let cut = if buffer.len() - whole > 4 {
        buffer.len()
    } else {
        whole
    };
    let rest = buffer.split_off(cut);
    let text = String::from_utf8_lossy(buffer).into_owned();
    *buffer = rest;
    text
}

/// Hands each step somewhere to put what it says while it runs.
struct LiveText {
    emit: Arc<dyn Fn(&str, Pipe, String) + Send + Sync>,
}

impl StepSinks for LiveText {
    fn sink_for(&self, step: &str) -> Arc<dyn LiveSink> {
        Arc::new(StepText {
            step: step.to_owned(),
            emit: self.emit.clone(),
            tails: Mutex::new((Vec::new(), Vec::new())),
        })
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
    fn append_started(&self, record: StepRecord) -> Result<(), FlowError> {
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
        &self,
        run_id: &str,
        step_id: &str,
        attempt: u32,
        epoch: u64,
        completion: Completion,
    ) -> Result<(), FlowError> {
        let announced = announced_close(step_id, attempt, epoch, &completion);
        self.inner
            .close(run_id, step_id, attempt, epoch, completion)?;
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

    /// La spesa la sa il deposito sotto, e questo decoratore non la commenta:
    /// **un guscio che rispondesse zero renderebbe il tetto muto solo nella
    /// finestra**, cioè proprio dove qualcuno sta guardando una corsa partire.
    fn spent(&self, run_id: &str) -> Result<flow::Spend, FlowError> {
        self.inner.spent(run_id)
    }

    /// The stop lives in the window's registry of runs, where the button
    /// wrote it: this is the only store that can carry it to the executor.
    fn halt_requested(&self, run_id: &str) -> Result<bool, FlowError> {
        Ok(self.runs.halt_requested(run_id))
    }
}

/// The `step_closed` fact as the window receives it. `Completion` does not
/// serialise, so the fields a watcher needs are picked by hand: `said` is the
/// raw text of the step, `refusal` the check that refused and what it saw,
/// `ran` the program and the arguments the step actually started.
fn announced_close(step_id: &str, attempt: u32, epoch: u64, completion: &Completion) -> Value {
    json!({
        "step_id": step_id,
        "attempt": attempt,
        "epoch": epoch,
        "outcome": format!("{:?}", completion.outcome),
        "output": completion.output,
        "said": completion.said,
        "failure_class": completion.failure_class,
        "refusal": completion.refusal,
        "ran": completion.ran,
        "ended_at": completion.ended_at,
        "bytes_seen": completion.bytes_seen,
        "bytes_discarded": completion.bytes_discarded,
    })
}

/// Resumes a run the ledger holds — one parked on a person and just closed —
/// through this window: the run joins the registry, every step reaches the
/// console, and Stop applies to it. The root is the one the window stands
/// in, and the answer says so, because the ledger keeps no root of a run's own.
pub(crate) fn resume(
    app: &AppHandle,
    runs: &Arc<Runs>,
    ledger: Ledger,
    flow: FlowFile,
    run_id: String,
) -> Result<String, String> {
    let header = ledger
        .run_header(&run_id)
        .map_err(|error| format!("cannot read run {run_id}: {error}"))?
        .ok_or_else(|| format!("no run {run_id} in the ledger"))?;
    let root = std::env::current_dir()
        .ok()
        .and_then(|working| flow::workspace::find_root(&working));
    {
        let mut known = runs.lock_map();
        if known.get(&run_id).is_some_and(|state| state.status == "running") {
            return Err(format!("run {run_id} is already running in this window"));
        }
        known.insert(
            run_id.clone(),
            RunState {
                flow: flow.id.clone(),
                started_at: header.started_at,
                status: "running".to_owned(),
                events: Vec::new(),
                next_seq: 0,
                halt: false,
            },
        );
    }
    let where_it_runs = root
        .as_ref()
        .map_or("no project root: steps that declare a workdir will fail".to_owned(), |root| {
            format!("in {}", root.display())
        });
    let handle = runs.clone();
    let app = app.clone();
    let id = run_id.clone();
    std::thread::spawn(move || {
        let mut store = WatchedStore {
            inner: ledger.clone(),
            app: app.clone(),
            runs: handle.clone(),
            run_id: id.clone(),
        };
        let outcome = sailor::flow_cmd::resume_run_with(&ledger, &flow, &id, &mut store, root.as_deref());
        // The status the resume recorded is the ledger's word for it; the
        // report, right or wrong, reaches the console as the run's last line.
        let status = ledger
            .run_header(&id)
            .ok()
            .flatten()
            .map_or("incomplete".to_owned(), |header| header.status);
        let (report, error) = match outcome {
            Ok(report) => (Some(report), None),
            Err(error) => (None, Some(error)),
        };
        handle.set_status(&id, &status);
        handle.publish(
            &app,
            &id,
            "run_ended",
            None,
            json!({ "status": status, "error": error, "report": report, "ended_at": now_secs() }),
        );
    });
    Ok(format!("run {run_id} is resuming {where_it_runs}; follow it in the console"))
}

/// Asks a run held by this window to stop before its next front. The step
/// running now finishes: the engine cannot take a step back from an agent
/// already at work, and the window says so instead of pretending.
#[tauri::command]
pub(crate) fn stop_run(
    app: AppHandle,
    runs: State<'_, Arc<Runs>>,
    run_id: String,
) -> Result<(), String> {
    runs.request_halt(&run_id)?;
    runs.publish(
        &app,
        &run_id,
        "stop_requested",
        None,
        json!({ "by": who(), "at": now_secs() }),
    );
    Ok(())
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
    let origin = origin_label(mandate.as_deref());
    start(&app, runs.inner(), &name, mandate.as_deref(), origin)
}

/// One road for the button and for the beat: whoever starts a run comes
/// through here, and `origin` is what tells the two apart in the ledger.
pub(crate) fn start(
    app: &AppHandle,
    runs: &Arc<Runs>,
    name: &str,
    mandate: Option<&str>,
    origin: String,
) -> Result<StartedRun, String> {
    let flow = load_flow(name)?;

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
    // THE NAME BEFORE THE REGISTRY. The witness carries the run it belongs to,
    // so the run has to have a name before the registry that holds it is built.
    // Nothing is written yet: a name spent on a flow that turns out to name a
    // missing action costs nothing.
    let started_at = now_secs();
    let run_id = format!("{}-{}", flow.id, nanos());
    // What a step says while it runs reaches the window through here. The
    // ledger tells the window when a step opens and closes; this tells it what
    // the step is saying in between, which the ledger only learns at the end.
    let watcher: Arc<dyn StepSinks> = Arc::new(LiveText {
        emit: Arc::new({
            let app = app.clone();
            let runs = runs.clone();
            let run_id = run_id.clone();
            move |step: &str, pipe: Pipe, text: String| {
                runs.publish(
                    &app,
                    &run_id,
                    "step_text",
                    Some(step.to_owned()),
                    json!({ "pipe": pipe.name(), "text": text }),
                );
            }
        }),
    });
    let registry = default_registry(&ledger, Some(watcher));
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
    let inputs = inputs_with_mandate(&flow, mandate)?;

    // DA DOVE È PARTITA LA CHIAMATA, scritto dal sistema nel momento in cui
    // parte. Non è un racconto di un agente: è il guscio che dichiara la
    // propria provenienza prima che qualunque passo giri, e resta nel deposito
    // append-only anche se la corsa si schianta al primo passo.
    record_run(
        &ledger, &flow, &run_id, "running", started_at, None, None, &origin,
    )?;

    {
        let mut known = runs.lock_map();
        known.insert(
            run_id.clone(),
            RunState {
                flow: flow.id.clone(),
                started_at,
                status: "running".to_owned(),
                events: Vec::new(),
                next_seq: 0,
                halt: false,
            },
        );
    }

    let handle = runs.clone();
    let app = app.clone();
    let started = StartedRun {
        run_id: run_id.clone(),
        flow: flow.id.clone(),
        started_at,
    };

    // **CHI LANCIA DICE DOVE HA DECISO DI LAVORARE, PRIMA DI PARTIRE**, e vale
    // per il pulsante quanto per il terminale: senza questa riga il piano ha un
    // modo silenzioso di sbagliare, che è lo stesso del guasto 25. Si risolve
    // qui e non dentro il filo, così la riga esce prima che la corsa cominci.
    let root = std::env::current_dir()
        .ok()
        .and_then(|working| flow::workspace::find_root(&working));
    match root.as_deref() {
        Some(root) => println!("radice del progetto: {}", root.display()),
        None => println!(
            "radice del progetto: nessuna (nessun {} risalendo da qui); \
             i passi che dichiarano «workdir» falliranno",
            flow::workspace::MARKER
        ),
    }

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
        // **LA RICHIESTA LA COSTRUISCE `registry`, NON QUESTO FILE.** Era
        // scritta anche qui, ed è il guasto 10 in posizione: le due copie si
        // sono già disallineate tre volte. Con la radice del progetto in mezzo
        // la prossima divergenza sarebbe stata una corsa dalla finestra che
        // lavora dove sta il processo mentre la stessa corsa dal terminale
        // lavora nella radice giusta — e nessuna delle due lo direbbe.
        //
        // L'ingresso resta quello che il pulsante ha in mano: la finestra può
        // lanciare lo stesso flusso con un mandato diverso.
        let mut request = registry::execution_request(&flow, &run_id, root.as_deref());
        request.root_inputs = inputs;
        let result = InProcessExecutor.execute(
            &flow.graph,
            request,
            &mut store,
            &registry,
            &mut SystemClock,
        );

        let ended_at = now_secs();
        let (status, error) = match &result {
            // Una corsa fermata dal tetto porta i numeri con sé: quanto era il
            // tetto, quanto risultava speso, e quali passi non sono partiti.
            // Senza, nella finestra resterebbe una parola sola e nessun motivo.
            Ok(execution) => (
                execution_status(execution).to_owned(),
                registry::stopped_by_cap(execution)
                    .or_else(|| registry::halted_by_hand(execution)),
            ),
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
pub(crate) fn run_snapshot(
    runs: State<'_, Arc<Runs>>,
    run_id: String,
) -> Result<RunSnapshot, String> {
    let known = runs.lock_map();
    let state = known
        .get(&run_id)
        .ok_or_else(|| format!("run {run_id} is not known to this window"))?;
    Ok(RunSnapshot {
        run_id: run_id.clone(),
        flow: state.flow.clone(),
        started_at: state.started_at,
        status: state.status.clone(),
        events: state.events.clone(),
    })
}

/// In che modo una corsa è aperta.
///
/// **DUE MODI, NON UNO, E IL DEPOSITO LI TIENE IN DUE POSTI DIVERSI.** Una
/// corsa al lavoro ha un passo **senza esito**; una corsa consegnata a una
/// persona ha il passo **chiuso** con esito `Waiting`, perché chi deve
/// eseguirlo non è un processo di cui si aspetta la morte. Chiedere una sola
/// delle due domande fa sparire l'altra metà — è il guasto che
/// `waiting_runs` documenta al 31/08/2026: una consegna che nessuno raccoglieva
/// spariva, e l'unico modo di ritrovarla era ricordarsene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OpenState {
    /// Qualcuno o qualcosa ci sta lavorando adesso.
    Working,
    /// È ferma, e riparte solo se una persona fa qualcosa.
    Waiting,
}

/// Un passo che è aperto adesso, e da quanto.
///
/// **«TRE PASSI APERTI» NON È UNA RISPOSTA.** La domanda vera è *quale*, e da
/// quanto: un passo aperto da sei minuti sta lavorando, lo stesso passo aperto
/// da tre ore è appeso. Nella ricognizione del 31/08/2026 il modello è la
/// sezione «Pending Activities» di Temporal — tipo dell'attività, tentativo
/// corrente, tentativi rimasti, battito — costruita apposta perché la
/// cronologia degli eventi da sola non basta: `ActivityTaskStarted` non compare
/// finché l'attività non è finita o non ha esaurito i tentativi. Nessuno degli
/// strumenti per agenti confrontati ha un equivalente.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OpenStep {
    pub step_id: String,
    /// Quale tentativo: `2` su un passo aperto vuol dire che il primo è caduto.
    pub attempt: u32,
    pub open_for_secs: i64,
}

/// Una corsa aperta, chiunque l'abbia avviata.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OpenRun {
    pub run_id: String,
    /// Su cosa lavorava: il flusso, o la cosa che la corsa nomina.
    pub entity: String,
    /// Al lavoro, o ferma ad aspettare una persona.
    pub state: OpenState,
    /// Quanti passi hanno ancora l'esito aperto. Zero per chi aspetta.
    pub open_steps: usize,
    /// **Quali** passi, e da quanto. Vuoto per chi aspetta.
    pub open_now: Vec<OpenStep>,
    /// Da quando dura questo stato: l'apertura del passo più vecchio per chi
    /// lavora, l'inizio dell'attesa per chi aspetta.
    pub since: i64,
    /// Vero se questa finestra è quella che l'ha avviata.
    pub started_here: bool,
    /// Steps with an outcome already, counted once each.
    pub steps_done: usize,
    /// Steps the flow declares; `None` when the flow cannot be read back.
    pub steps_total: Option<usize>,
}

/// «4 of 7»: how far the run is, from the ledger and the flow it names.
///
/// The flow is read back through the same door `step close` uses; a run whose
/// flow cannot be found still shows how many steps it has done, so the count
/// never invents a total.
fn progress_of(ledger: &Ledger, run_id: &str) -> (usize, Option<usize>) {
    let done = ledger
        .steps(run_id)
        .map(|steps| {
            steps
                .iter()
                .filter(|step| step.outcome.is_some())
                .map(|step| step.step_id.clone())
                .collect::<std::collections::HashSet<String>>()
                .len()
        })
        .unwrap_or(0);
    let total = sailor::step_cmd::flow_of_run(ledger, run_id)
        .ok()
        .map(|file| file.graph.steps().len());
    (done, total)
}

/// **Tutte** le corse che hanno almeno un passo aperto, non solo le nostre.
///
/// **PERCHÉ NON BASTAVA `known_runs`.** Quella legge una mappa in memoria di
/// questo processo: una corsa lanciata dal terminale, da un'altra finestra o da
/// un demone non compare, e la prima schermata di Sailor — che deve dire «cosa
/// sta succedendo adesso» — direbbe il falso a chiunque lavori in più di un
/// posto. È il vincolo permanente «chiarezza per chi guarda»: un'interfaccia
/// che mostra solo il proprio angolo è peggio di una che non mostra niente,
/// perché sembra completa.
///
/// **IL DEPOSITO È L'ORACOLO, E LA MEMORIA È SOLO UN'ETICHETTA.** L'elenco
/// viene da due domande al deposito — `unfinished_runs` per chi lavora,
/// `waiting_runs` per chi aspetta una persona; ciò che questo processo sa in
/// più serve solo a dire *quali* sono sue, perché una corsa avviata qui si può
/// seguire dal vivo e una avviata altrove no. Se il deposito non c'è, l'elenco
/// è vuoto: non è un errore, è una macchina su cui non è ancora girato niente.
///
/// **CHI ASPETTA VINCE SU CHI LAVORA** quando una corsa comparisse in tutte e
/// due le risposte. Non è una preferenza estetica: dei due stati uno solo
/// chiede qualcosa a chi guarda, e mostrarlo come «al lavoro» lo farebbe
/// aspettare in eterno un processo che non tornerà.
#[tauri::command]
pub(crate) fn open_runs(runs: State<'_, Arc<Runs>>) -> Result<Vec<OpenRun>, String> {
    let ledger_dir = default_ledger_dir();
    if !ledger_dir.exists() {
        return Ok(Vec::new());
    }
    let ledger = Ledger::open(&ledger_dir)
        .map_err(|error| format!("cannot open the ledger: {error}"))?;
    let unfinished = ledger
        .unfinished_runs()
        .map_err(|error| format!("cannot read the open runs: {error}"))?;
    let waiting = ledger
        .waiting_runs()
        .map_err(|error| format!("cannot read the waiting runs: {error}"))?;
    let known = runs.lock_map();

    let now = now_secs();
    let mut all: Vec<OpenRun> = waiting
        .into_iter()
        .map(|run| {
            let (steps_done, steps_total) = progress_of(&ledger, &run.run_id);
            OpenRun {
                started_here: known.contains_key(&run.run_id),
                run_id: run.run_id,
                entity: run.entity,
                state: OpenState::Waiting,
                open_steps: 0,
                open_now: Vec::new(),
                since: run.waiting_since,
                steps_done,
                steps_total,
            }
        })
        .collect();
    let held: std::collections::HashSet<String> =
        all.iter().map(|run| run.run_id.clone()).collect();
    for run in unfinished
        .into_iter()
        .filter(|run| !held.contains(&run.run_id))
    {
        // UNA DOMANDA IN PIÙ PER CORSA APERTA, e sono poche per costruzione:
        // qui ci finisce solo ciò che è in volo adesso, non la storia. Se un
        // giorno fossero tante, il posto dove si ripara è il deposito con una
        // sola interrogazione che porti anche i passi — non qui, saltando il
        // dettaglio, perché è il dettaglio la risposta.
        let open_now = ledger
            .steps(&run.run_id)
            .map(|steps| {
                steps
                    .into_iter()
                    .filter(|step| step.outcome.is_none())
                    .map(|step| OpenStep {
                        step_id: step.step_id,
                        attempt: step.attempt,
                        open_for_secs: now - step.started_at,
                    })
                    .collect()
            })
            // Un passo che non si riesce a leggere non fa sparire la corsa:
            // il conteggio resta, il dettaglio manca, e la riga si vede lo
            // stesso. Perdere la riga sarebbe il danno grosso.
            .unwrap_or_default();
        let (steps_done, steps_total) = progress_of(&ledger, &run.run_id);
        all.push(OpenRun {
            started_here: known.contains_key(&run.run_id),
            run_id: run.run_id,
            entity: run.entity,
            state: OpenState::Working,
            open_steps: run.open_steps,
            open_now,
            since: run.oldest_started_at,
            steps_done,
            steps_total,
        });
    }

    // La più vecchia in cima: chi guarda cerca prima ciò che è fermo da più
    // tempo, non ciò che è appena partito. Qui l'ordine è solo sul tempo —
    // raggruppare per stato è una scelta di chi disegna, e questo comando serve
    // anche a chi non disegna niente.
    all.sort_by_key(|run| run.since);
    Ok(all)
}

/// Le corse che questa finestra ha avviato, la più recente per ultima.
///
/// Serve a chi ricarica la pagina mentre un flusso gira: senza questo elenco la
/// vista ripartirebbe vuota e la corsa continuerebbe senza nessuno che la
/// guarda.
#[tauri::command]
pub(crate) fn known_runs(runs: State<'_, Arc<Runs>>) -> Vec<RunSnapshot> {
    let known = runs.lock_map();
    let mut all: Vec<RunSnapshot> = known
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
        self.0
            .lock()
            .expect("il registro delle corse non è avvelenato")
    }

    /// The flows this window is running right now, by flow id.
    pub(crate) fn running_flows(&self) -> Vec<String> {
        self.lock_map()
            .values()
            .filter(|state| state.status == "running")
            .map(|state| state.flow.clone())
            .collect()
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
    /// Which check refused, where, by which rule and what it saw: the
    /// structure beside the class, so the window need not parse `said`.
    pub refusal: Option<Refusal>,
    /// The program and the arguments the step started, after resolution: what
    /// a person would have to type to reach the same outcome by hand.
    pub ran: Option<Ran>,
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
        .map_err(|error| format!("cannot read the ledger: {error}"))?
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
        .map_err(|error| format!("cannot read the ledger: {error}"))?
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
            passages.push(passage_of(record, &run.started_by, &signal));
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

/// One record of the ledger, as the history panel reads it.
fn passage_of(record: &StepRecord, started_by: &str, signal: &RunSignal) -> StepPassage {
    StepPassage {
        run_id: record.run_id.clone(),
        attempt: record.attempt,
        started_at: record.started_at,
        ended_at: record.ended_at,
        outcome: record.outcome.map(|outcome| format!("{outcome:?}")),
        failure_class: record.failure_class.clone(),
        refusal: record.refusal.clone(),
        ran: record.ran.clone(),
        started_by: started_by.to_owned(),
        input: record.input.clone(),
        mandate: signal.text.clone(),
        signal_who: signal.who.clone(),
        signal_where: signal.where_from.clone(),
        said: record.said.clone(),
        output: record.output.clone(),
    }
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
                "the flow has no starting step: every step waits on another".to_owned()
            } else {
                format!(
                    "the flow starts from {} steps ({}), and one mandate does not say which it goes to",
                    roots.len(),
                    roots.join(", ")
                )
            },
        };
    };

    let Some(step) = flow.graph.steps().iter().find(|step| &step.id == root) else {
        return MandateTarget::None {
            why: "the starting step is not in the graph".to_owned(),
        };
    };

    let Some(field) = text_field_of(&step.action) else {
        return MandateTarget::None {
            why: format!(
                "the starting step «{root}» runs «{}», which has no text input",
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
                            "the trigger «{root}» waits for a signal of kind «{source}», not a person's gesture"
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
        MandateTarget::None { why } => {
            Err(format!("this flow takes no mandate: {why}"))
        }
    }
}

/// Com'è finita la corsa. **Non è più una copia**: era scritta anche qui, con
/// sopra un commento che diceva di essere la stessa di `flow_cmd`. Lo era, per
/// buona volontà; adesso lo è per costruzione. Il booleano serve solo al codice
/// d'uscita di un processo, e qui non c'è nessun processo che esce.
fn execution_status(execution: &Execution) -> &'static str {
    registry::execution_status(execution).0
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
fn default_registry(
    ledger: &Ledger,
    watcher: Option<Arc<dyn actions::StepSinks>>,
) -> ActionRegistry {
    registry::default_registry(Some(ledger.clone()), watcher)
}

/// Da dove arriva il segnale che questo guscio manda. È la finestra, sempre:
/// non è un dato da indovinare, è quello che questo programma è.
const WHERE: &str = "the Sailor window";

/// Chi ha premuto, **per come questa sorgente lo sa**, che è poco: la finestra
/// non ha un'identità di persona — non c'è login, non c'è account — e l'unica
/// cosa vera a disposizione è l'utente con cui il programma gira.
///
/// **SI TORNA VUOTO PIUTTOSTO CHE INVENTARE.** `Signal` dichiara che un segnale
/// che non sa chi l'ha mandato lo dice con una stringa vuota; scrivere lì un
/// nome plausibile ma non verificato sarebbe peggio che ammettere di non
/// saperlo, perché nessuno andrebbe più a controllare.
pub(crate) fn who() -> String {
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

/// Registra l'intestazione della corsa.
///
/// **QUESTA COPIA NON ESISTE PIÙ, ED È IL PUNTO.** Qui c'erano le stesse venti
/// righe di `flow_cmd`, con sotto un commento che dichiarava la duplicazione e
/// diceva perché non si poteva chiudere. Si poteva: dal 30/08/2026 c'è un crate
/// che le due strade condividono. Il 31/08 tutte e due scrivevano il totale a
/// zero a mano, e riparare solo una avrebbe dato due cifre diverse per la stessa
/// corsa a seconda del pulsante premuto.
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
    registry::record_flow_run(
        ledger,
        flow,
        registry::FlowRun {
            run_id,
            status,
            started_at,
            ended_at,
            error,
            started_by,
        },
    )
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
        Some((_, origin, Err(reason))) => Err(format!(
            "flow «{name}» ({origin}) does not load: {reason}"
        )),
        None => {
            let names: Vec<&str> = known.iter().map(|(name, _, _)| name.as_str()).collect();
            Err(format!(
                "no flow is called «{name}»; the ones I see are: {}",
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

    /// A run as the registry holds it while it runs.
    fn held_run(runs: &Runs, run_id: &str, status: &str) {
        runs.lock_map().insert(
            run_id.to_owned(),
            RunState {
                flow: "prova".to_owned(),
                started_at: 0,
                status: status.to_owned(),
                events: Vec::new(),
                next_seq: 0,
                halt: false,
            },
        );
    }

    /// **STOP IS A FACT THE EXECUTOR READS, AND ONLY A RUNNING RUN CAN CARRY
    /// IT.** An unknown run and an ended one are refused with the reason, so
    /// a button pressed late does not report a stop nobody will honour.
    #[test]
    fn a_stop_is_held_for_a_running_run_and_refused_otherwise() {
        let runs = Runs::default();
        held_run(&runs, "live", "running");
        held_run(&runs, "done", "complete");

        assert!(!runs.halt_requested("live"));
        runs.request_halt("live").expect("a running run takes the stop");
        assert!(runs.halt_requested("live"));

        let ended = runs.request_halt("done").expect_err("an ended run refuses");
        assert!(ended.contains("is not running: complete"), "{ended}");
        assert!(!runs.halt_requested("done"));

        let unknown = runs.request_halt("nobody").expect_err("an unknown run refuses");
        assert!(unknown.contains("no run nobody"), "{unknown}");
    }

    fn refused_by_shape() -> Refusal {
        Refusal::new("answer_shape", "$.verdict", flow::RefusalRule::NotAllowed, "\"remvoe\"")
    }

    /// The window shows which rule refused and at which path: the check
    /// travels as structure in the closing fact, beside the class it explains.
    #[test]
    fn a_closing_fact_carries_the_refusal_as_structure() {
        let completion = Completion {
            outcome: flow::Outcome::Broke,
            output: None,
            said: Some("off shape".to_owned()),
            failure_class: Some("answer_off_shape".to_owned()),
            refusal: Some(refused_by_shape()),
            ran: None,
            ended_at: 7,
            bytes_seen: None,
            bytes_discarded: None,
        };
        let announced = announced_close("verdict", 1, 0, &completion);
        assert_eq!(announced["refusal"]["check"], "answer_shape");
        assert_eq!(announced["refusal"]["path"], "$.verdict");
        assert_eq!(announced["refusal"]["rule"], "not_allowed");
        assert_eq!(announced["refusal"]["seen"], "\"remvoe\"");

        let plain = Completion { refusal: None, ..completion };
        assert!(announced_close("verdict", 1, 0, &plain)["refusal"].is_null());
    }

    fn ran_a_shell_line() -> Ran {
        Ran::new("sh", ["-c", "echo hi"])
    }

    /// The line a step started travels in the closing fact, so whoever is
    /// watching sees the command instead of guessing it from the outcome.
    #[test]
    fn a_closing_fact_carries_the_line_the_step_ran() {
        let completion = Completion {
            outcome: flow::Outcome::Went,
            output: None,
            said: None,
            failure_class: None,
            refusal: None,
            ran: Some(ran_a_shell_line()),
            ended_at: 7,
            bytes_seen: None,
            bytes_discarded: None,
        };
        let announced = announced_close("verdict", 1, 0, &completion);
        assert_eq!(announced["ran"]["program"], "sh");
        assert_eq!(announced["ran"]["args"][0], "-c");
        assert_eq!(announced["ran"]["args"][1], "echo hi");

        let quiet = Completion { ran: None, ..completion };
        assert!(announced_close("verdict", 1, 0, &quiet)["ran"].is_null());
    }

    /// The same structure reaches the history of a step, read back from the
    /// ledger, so an old refusal is as legible as the one just made.
    #[test]
    fn a_passage_carries_the_refusal_of_its_record() {
        let mut record =
            StepRecord::started("r1", "verdict", 1, 0, Vec::new(), json!({}), Vec::new(), 1);
        record.refusal = Some(refused_by_shape());
        let passage = passage_of(&record, "window", &RunSignal::default());
        assert_eq!(passage.refusal, Some(refused_by_shape()));

        record.refusal = None;
        assert_eq!(passage_of(&record, "window", &RunSignal::default()).refusal, None);
    }

    /// The same line reaches the history of a step, read back from the ledger,
    /// so a run from months ago says what it started as plainly as this one.
    #[test]
    fn a_passage_carries_the_line_its_record_ran() {
        let mut record =
            StepRecord::started("r1", "verdict", 1, 0, Vec::new(), json!({}), Vec::new(), 1);
        record.ran = Some(ran_a_shell_line());
        let passage = passage_of(&record, "window", &RunSignal::default());
        assert_eq!(passage.ran, Some(ran_a_shell_line()));

        record.ran = None;
        assert_eq!(passage_of(&record, "window", &RunSignal::default()).ran, None);
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

    /// Collects what a step says, the way the window would receive it.
    fn heard(step: &str) -> (Arc<dyn LiveSink>, Arc<Mutex<Vec<(String, String)>>>) {
        let said: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let sinks = LiveText {
            emit: Arc::new({
                let said = said.clone();
                move |_step: &str, pipe: Pipe, text: String| {
                    said.lock()
                        .expect("not poisoned")
                        .push((pipe.name().to_owned(), text));
                }
            }),
        };
        (sinks.sink_for(step), said)
    }

    /// THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY. A pipe breaks where it
    /// breaks, and half of this project's output is accented: decoding each
    /// chunk on its own turns every letter unlucky enough to straddle a break
    /// into a replacement mark. Emitting the tail regardless makes this red.
    #[test]
    fn a_character_split_across_two_chunks_arrives_whole() {
        let (sink, said) = heard("engine");
        let text = "perché".as_bytes();
        let split = text.len() - 1;
        sink.chunk(Pipe::Stdout, &text[..split]);
        sink.chunk(Pipe::Stdout, &text[split..]);

        let said = said.lock().expect("not poisoned");
        let whole: String = said.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(whole, "perché");
        assert!(
            !whole.contains('\u{fffd}'),
            "a letter reached the window broken"
        );
    }

    /// Two threads drain the two pipes: one buffer between them would splice
    /// the tail of one onto the head of the other.
    #[test]
    fn the_two_pipes_do_not_splice_into_each_other() {
        let (sink, said) = heard("engine");
        let out = "è".as_bytes();
        sink.chunk(Pipe::Stdout, &out[..1]);
        sink.chunk(Pipe::Stderr, b"broke\n");
        sink.chunk(Pipe::Stdout, &out[1..]);

        let said = said.lock().expect("not poisoned");
        assert_eq!(
            said.as_slice(),
            [
                ("err".to_owned(), "broke\n".to_owned()),
                ("out".to_owned(), "è".to_owned())
            ]
        );
    }

    /// A tail that will never be completed must not silence the pipe for good.
    #[test]
    fn bytes_that_are_not_a_split_character_stop_being_waited_for() {
        let (sink, said) = heard("engine");
        sink.chunk(Pipe::Stdout, &[0xff, 0xfe, 0xfd, 0xfc, 0xfb]);

        let said = said.lock().expect("not poisoned");
        assert_eq!(
            said.len(),
            1,
            "five bytes that no character starts were held back"
        );
    }

    /// THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY. While the shell hands
    /// the engine no witness, a running step says nothing until it closes, and
    /// nothing in the window is red about it: the panel simply stays empty.
    /// This reads the line that decides it.
    #[test]
    fn the_shell_hands_the_engine_somewhere_to_put_a_running_step_s_text() {
        const SOURCE: &str = include_str!("run.rs");
        // Spelled in two halves on purpose: written whole, the assertion would
        // find itself in the file it reads and pass on its own text.
        let no_witness = format!("default_registry(&ledger, {})", "None");
        assert!(
            !SOURCE.contains(&no_witness),
            "the shell builds the action registry with no witness, so the text \
             of a running step reaches nobody"
        );
    }

    #[test]
    fn a_single_engine_root_takes_the_mandate_on_its_stdin() {
        let flow = flow_with(
            json!([engine_step(
                "dispatch",
                vec![],
                json!({ "bin": "true", "timeout_secs": 5 })
            )]),
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
            MandateTarget::None { why } => assert!(why.contains("no text input"), "{why}"),
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
            MandateTarget::None { why } => assert!(why.contains("2 steps"), "{why}"),
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

    /// IL CONTRATTO VERO, letto da `flows/dispatch-the-work.flow.json` il
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
                engine_step(
                    "dispatch",
                    vec!["trigger"],
                    json!({ "bin": "true", "timeout_secs": 5 })
                ),
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
        let known = registry::default_registry(None, None);
        for name in ["what-this-machine-has", "migrate-to-sailor"] {
            let flow = load_flow(name).expect("i flussi di sistema si caricano");
            for step in flow.graph.steps() {
                assert!(
                    known.get(&step.action).is_some(),
                    "«{}» nomina l'azione «{}», che la finestra non conosce",
                    name,
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
        let flow = load_flow("what-this-machine-has")
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
        for malformed in ["../evaso", "sotto/cartella", "", "/etc/passwd"] {
            let outcome = load_flow(malformed);
            assert!(
                outcome.is_err(),
                "«{malformed}» non è il nome di nessun flusso: non deve caricare niente"
            );
            let why = outcome.unwrap_err();
            assert!(
                why.contains("no flow is called"),
                "e il motivo dev'essere che non è in elenco, non un errore di lettura: {why}"
            );
        }
    }
}
