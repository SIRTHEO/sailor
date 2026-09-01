//! Il nodo con cui un flusso chiede **com'è andata** le volte prima.
//!
//! **PERCHÉ ESISTE.** Il deposito registra da sempre l'esito di ogni passo di
//! ogni corsa — com'è finito, con che classe di guasto, quanto ci ha messo — e
//! fino a oggi nessun flusso poteva rileggerlo. I tre nodi di `store` non
//! servono: leggono un archivio a chiave, cioè i fatti che un flusso ha deciso
//! di ricordare, non quello che gli è successo. Senza questo nodo un flusso
//! esegue gli stessi passi nello stesso modo indipendentemente da come sono
//! andati le venti volte precedenti: è la differenza fra un sistema che ripete
//! e uno che accumula esperienza.
//!
//! **SI CHIEDE PER DOMANDE NOMINATE, NON IN SQL.** Un nodo che accettasse SQL
//! sarebbe potente e sbagliato per due motivi misurati su questo albero. Primo:
//! legherebbe ogni file di flusso alla forma della tabella `steps` di *oggi*, e
//! quella tabella ha già ricevuto colonne nuove (`species`, `held_by_pid`,
//! `bytes_discarded`, aggiunte da `add_missing_projection_columns`); ogni
//! aggiunta futura sarebbe una rottura silenziosa per chi ha scritto una
//! `SELECT`. Con quattro domande nominate lo schema può cambiare e i flussi
//! restano validi, perché il SQL vive dove vive lo schema — in `ledger` —
//! mentre qui si parla solo JSON. Secondo: una domanda chiusa **non ha
//! proiezione arbitraria**. Non esiste una sintassi per dire «dammi questa
//! colonna», quindi il segreto non si difende con una lista nera, che qualcuno
//! prima o poi dimentica di aggiornare, ma con l'assenza del modo di chiederlo.
//! Il prezzo è dichiarato: una domanda che nessuno ha previsto richiede una
//! variante nuova in Rust, non una riga in un file di dati. È la stessa scelta
//! già presa per i rinvii in `reference.rs` — tre forme chiuse invece di un
//! linguaggio — e per la stessa ragione.
//!
//! **COSA ESCE E COSA NO.**
//! - **Sempre**: i fatti strutturali e le misure — passo, corsa, flusso,
//!   esito, classe di guasto, tentativo, istanti, durate, conteggi, byte. Sono
//!   fatti *sul* lavoro, non il lavoro.
//! - **Mai**: `input` e `output`. Sono il canale dati tipato: ci passano
//!   prompt, ambienti e risposte di modelli. Restituirli rimetterebbe valori
//!   strutturati arbitrari dentro l'ingresso di un passo — il varco che
//!   `reference.rs` dichiara già aperto per `store_read` e che non si allarga.
//!   Il divieto è strutturale, non disciplinare: i tipi che il deposito
//!   restituisce su questo percorso non hanno quei campi.
//! - **Solo se chiesto**: `said`, con `include_said: true`, ammesso unicamente
//!   sulla domanda `last_run`, sui soli passi rotti di quella corsa, al massimo
//!   [`SAID_MAX_STEPS`] passi e [`SAID_MAX_BYTES`] byte ciascuno, con
//!   `said_truncated` che dichiara il taglio. `record.rs` lo descrive come
//!   testo grezzo per una persona quando qualcosa va storto, non come dato su
//!   cui si decide: resta raggiungibile perché senza di lui una diagnosi non si
//!   fa, ma resta un varco stretto — nessuna delle quattro domande ne ha
//!   bisogno, e il valore predefinito è che non esca.
//!
//! **IL DEPOSITO VUOTO È UNA RISPOSTA, NON UN GUASTO.** La busta porta sempre
//! `deposit`, che vale `absent` (su questa macchina non c'è nessun deposito),
//! `empty` (c'è, e non ha mai visto una corsa) o `present`. La chiave `answer`
//! esiste **solo** nell'ultimo caso, e non compare mai valorizzata a `null`: la
//! ragione sta nel motore, non nel gusto. `Condition::PointerExists` si
//! appoggia a `Value::pointer`, che su un `null` risponde `Some`, quindi un
//! `answer: null` farebbe scattare il ramo «ho una risposta» proprio sulla
//! macchina appena installata — cioè nel caso che va distinto. Omettendo la
//! chiave, un flusso separa «non lo so» da «zero», e con `PointerEquals` su
//! `/deposit` separa «deposito assente» da «deposito vuoto». Dentro `answer`,
//! zero guasti è il numero zero. In nessuno dei tre casi il passo fallisce:
//! come per `store_read`, un primo giro non nasce rosso.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use ledger::{Ledger, StepDurations};
use serde::Deserialize;
use serde_json::{json, Map, Value};

/// Il nome sotto cui `HistoryAskAction` si registra.
pub const HISTORY_ASK_ACTION: &str = "history_ask";

/// Quante corse si guardano quando il flusso non lo dice.
const DEFAULT_WINDOW: u32 = 50;
/// Il tetto della finestra. Limita insieme il costo della lettura e quanto
/// storico una domanda sola può attraversare.
const MAX_WINDOW: u32 = 500;
/// Quanti passi rotti possono portare il proprio testo grezzo.
pub const SAID_MAX_STEPS: usize = 5;
/// Quanto testo grezzo esce da ciascuno.
pub const SAID_MAX_BYTES: usize = 512;

/// Registra il nodo che interroga lo storico.
///
/// **IL DEPOSITO È FACOLTATIVO, E IL NODO C'È COMUNQUE.** I nodi di `store`
/// scrivono, e senza deposito non hanno niente da fare: restano fuori dal
/// registro. Questo legge, e «su questa macchina non c'è nessuna corsa» è la
/// risposta giusta, non un'azione mancante. Registrarlo sotto la stessa
/// condizione farebbe dire a `sailor flow check` che l'azione non esiste
/// esattamente sulla macchina appena installata, dove serve che risponda.
pub fn register_history(registry: &mut flow::ActionRegistry, ledger: Option<Ledger>) {
    registry.register(HISTORY_ASK_ACTION, HistoryAskAction::new(ledger));
}

/// Le quattro domande, e nient'altro.
#[derive(Debug, Deserialize)]
#[serde(tag = "ask", rename_all = "snake_case")]
enum Ask {
    /// Quante volte questo passo è fallito, e con quale classe di guasto.
    StepFailures {
        step_id: String,
        #[serde(default)]
        flow: Option<String>,
        #[serde(default)]
        within_last_runs: Option<u32>,
    },
    /// Quali guasti sono i più frequenti.
    FailureClasses {
        #[serde(default)]
        flow: Option<String>,
        #[serde(default)]
        within_last_runs: Option<u32>,
    },
    /// Com'è andata l'ultima corsa chiusa di questo flusso, passo per passo.
    LastRun {
        flow: String,
        #[serde(default)]
        include_said: Option<bool>,
    },
    /// Quanto ci mette di solito questo passo.
    StepDuration {
        step_id: String,
        #[serde(default)]
        flow: Option<String>,
        #[serde(default)]
        within_last_runs: Option<u32>,
    },
}

/// I campi ammessi da ciascuna domanda, `None` se la domanda non esiste.
///
/// **SI RIFIUTANO A MANO PERCHÉ SERDE NON PUÒ FARLO QUI**: `deny_unknown_fields`
/// non si applica dentro un enum a tag interno. Senza questo controllo un flusso
/// che scrive `step-id` invece di `step_id` riceverebbe la risposta su *tutti*
/// i passi e la scambierebbe per la propria — un numero plausibile e sbagliato,
/// cioè il modo peggiore di sbagliare.
fn allowed_fields(ask: &str) -> Option<&'static [&'static str]> {
    match ask {
        "step_failures" => Some(&["ask", "step_id", "flow", "within_last_runs"]),
        "failure_classes" => Some(&["ask", "flow", "within_last_runs"]),
        "last_run" => Some(&["ask", "flow", "include_said"]),
        "step_duration" => Some(&["ask", "step_id", "flow", "within_last_runs"]),
        _ => None,
    }
}

const KNOWN_ASKS: &str = "step_failures, failure_classes, last_run, step_duration";

fn parse_ask(input: &Value) -> Result<Ask, ActionError> {
    let object = input.as_object().ok_or_else(|| {
        ActionError::new(
            "invalid_input",
            format!("una domanda allo storico è un oggetto con un campo `ask` fra: {KNOWN_ASKS}"),
        )
    })?;
    let name = object
        .get("ask")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ActionError::new(
                "invalid_input",
                format!("manca il campo `ask`. Le domande possibili sono: {KNOWN_ASKS}"),
            )
        })?
        .to_owned();
    let allowed = allowed_fields(&name).ok_or_else(|| {
        ActionError::new(
            "invalid_input",
            format!("domanda sconosciuta `{name}`. Le domande possibili sono: {KNOWN_ASKS}"),
        )
    })?;
    if let Some(unexpected) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(ActionError::new(
            "invalid_input",
            format!(
                "la domanda `{name}` non conosce il campo `{unexpected}`. \
                 Conosce: {}",
                allowed.join(", ")
            ),
        ));
    }
    let ask: Ask = serde_json::from_value(input.clone())
        .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
    // La finestra si controlla qui, con il resto della domanda, e non al
    // momento di leggere: un limite assurdo è un errore di chi ha scritto il
    // flusso, e su una macchina senza deposito passerebbe inosservato.
    window(declared_window(&ask))?;
    Ok(ask)
}

fn declared_window(ask: &Ask) -> Option<u32> {
    match ask {
        Ask::StepFailures {
            within_last_runs, ..
        }
        | Ask::FailureClasses {
            within_last_runs, ..
        }
        | Ask::StepDuration {
            within_last_runs, ..
        } => *within_last_runs,
        // `last_run` guarda una corsa sola per definizione.
        Ask::LastRun { .. } => None,
    }
}

/// La finestra chiesta, in corse. Fuori dai limiti è un errore invece di un
/// taglio silenzioso: una finestra ridotta di nascosto restituirebbe una
/// risposta su cinquanta corse a chi crede di averne guardate cinquemila.
fn window(declared: Option<u32>) -> Result<usize, ActionError> {
    let value = declared.unwrap_or(DEFAULT_WINDOW);
    if value == 0 || value > MAX_WINDOW {
        return Err(ActionError::new(
            "invalid_input",
            format!("`within_last_runs` va fra 1 e {MAX_WINDOW}, non {value}"),
        ));
    }
    Ok(value as usize)
}

/// Lo stato del deposito su questa macchina.
const DEPOSIT_ABSENT: &str = "absent";
const DEPOSIT_EMPTY: &str = "empty";
const DEPOSIT_PRESENT: &str = "present";

/// La busta. `answer` entra solo se c'è davvero una risposta da dare.
fn envelope(ask: &str, deposit: &str, runs_considered: i64, answer: Option<Value>) -> Value {
    let mut object = Map::new();
    object.insert("ask".to_owned(), json!(ask));
    object.insert("deposit".to_owned(), json!(deposit));
    object.insert("runs_considered".to_owned(), json!(runs_considered));
    if let Some(answer) = answer {
        object.insert("answer".to_owned(), answer);
    }
    Value::Object(object)
}

/// Il nome della domanda, per l'eco nella busta.
fn ask_name(ask: &Ask) -> &'static str {
    match ask {
        Ask::StepFailures { .. } => "step_failures",
        Ask::FailureClasses { .. } => "failure_classes",
        Ask::LastRun { .. } => "last_run",
        Ask::StepDuration { .. } => "step_duration",
    }
}

/// Il campione a un percentile, per **rango**: nessuna interpolazione, quindi
/// ogni numero restituito è una durata davvero misurata e non una media che
/// non è mai successa a nessuno.
fn percentile(sorted: &[i64], percent: usize) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (percent * sorted.len()).div_ceil(100).max(1);
    sorted.get(rank - 1).or_else(|| sorted.last()).copied()
}

/// Il riassunto delle durate, come lo legge un flusso.
///
/// **NESSUNA SOGLIA DI «TROPPO LENTO» SI DECIDE QUI.** Mediana e ultima escono
/// affiancate perché sia il flusso a confrontarle: scolpire in Rust quanto è
/// «molto di più» sarebbe una decisione di dominio dentro il motore, lo stesso
/// difetto per cui `notte` è condannata. `samples` esce in chiaro per la stessa
/// ragione: su tre campioni un novantesimo percentile è aritmetica, non una
/// misura, e chi legge deve poterlo vedere.
///
/// **L'UNITÀ È IL SECONDO INTERO, ED È UN LIMITE VERO.** L'orologio del motore
/// conta secondi, quindi un passo veloce misura zero. Sta scritto nella
/// risposta (`unit`) invece che qui soltanto: chi confronta due zeri deve
/// sapere che non sta confrontando niente.
fn duration_summary(durations: &StepDurations) -> Value {
    let sorted = &durations.seconds_sorted;
    let mut object = Map::new();
    object.insert("unit".to_owned(), json!("seconds"));
    object.insert("samples".to_owned(), json!(sorted.len()));
    object.insert("failed_samples".to_owned(), json!(durations.failed_samples));
    // Le misure escono solo se qualcosa è stato misurato: un minimo a zero su
    // zero campioni sarebbe un numero inventato con l'aspetto di un dato.
    if let (Some(min), Some(max)) = (sorted.first(), sorted.last()) {
        object.insert("min".to_owned(), json!(min));
        object.insert("max".to_owned(), json!(max));
        object.insert("median".to_owned(), json!(percentile(sorted, 50)));
        object.insert("p90".to_owned(), json!(percentile(sorted, 90)));
    }
    if let Some(last) = durations.last_seconds {
        object.insert("last".to_owned(), json!(last));
    }
    Value::Object(object)
}

fn classes_to_json(classes: &[ledger::FailureClassCount]) -> Value {
    Value::Array(
        classes
            .iter()
            .map(|count| {
                json!({
                    // `null` è una rottura che il motore non ha classificato, e
                    // non una classe che si chiama così: qui manca il dato.
                    "failure_class": count.failure_class,
                    "failures": count.failures,
                    "runs_affected": count.runs_affected,
                })
            })
            .collect(),
    )
}

/// Interroga lo storico delle corse.
pub struct HistoryAskAction {
    /// `None` quando su questa macchina il deposito non c'è o non si apre. Il
    /// nodo resta registrato e lo dichiara: vedi `register_history`.
    ledger: Option<Ledger>,
}

impl HistoryAskAction {
    pub fn new(ledger: Option<Ledger>) -> Self {
        Self { ledger }
    }

    fn answer(&self, ledger: &Ledger, ask: &Ask) -> Result<(i64, Value), ActionError> {
        match ask {
            Ask::StepFailures {
                step_id,
                flow,
                within_last_runs,
            } => {
                let limit = window(*within_last_runs)?;
                let flow = flow.as_deref();
                let tally = ledger
                    .step_failure_tally(step_id, flow, limit)
                    .map_err(unreadable)?;
                let considered = ledger.runs_in_window(flow, limit).map_err(unreadable)?;
                Ok((
                    considered,
                    json!({
                        "step_id": step_id,
                        "attempts": tally.attempts,
                        "failures": tally.failures,
                        "runs_affected": tally.runs_affected,
                        "by_class": classes_to_json(&tally.by_class),
                    }),
                ))
            }
            Ask::FailureClasses {
                flow,
                within_last_runs,
            } => {
                let limit = window(*within_last_runs)?;
                let flow = flow.as_deref();
                let classes = ledger
                    .failure_class_tally(flow, limit)
                    .map_err(unreadable)?;
                let considered = ledger.runs_in_window(flow, limit).map_err(unreadable)?;
                Ok((
                    considered,
                    json!({
                        "failures": classes.iter().map(|c| c.failures).sum::<i64>(),
                        "classes": classes_to_json(&classes),
                    }),
                ))
            }
            Ask::LastRun { flow, include_said } => {
                let found = ledger.last_finished_run(flow).map_err(unreadable)?;
                let Some(run) = found else {
                    // Un flusso che non ha ancora chiuso nessuna corsa non è un
                    // guasto: è il primo giro, e chi chiede ha un ramo per
                    // questo esattamente come per una voce mai scritta.
                    return Ok((0, json!({"found": false, "flow": flow})));
                };
                let steps: Vec<Value> = run
                    .steps
                    .iter()
                    .map(|step| {
                        json!({
                            "step_id": step.step_id,
                            "attempt": step.attempt,
                            "outcome": step.outcome,
                            "failure_class": step.failure_class,
                            "started_at": step.started_at,
                            "ended_at": step.ended_at,
                            "seconds": step.ended_at.map(|end| end - step.started_at),
                            "bytes_seen": step.bytes_seen,
                            "bytes_discarded": step.bytes_discarded,
                        })
                    })
                    .collect();
                let broke = run
                    .steps
                    .iter()
                    .filter(|step| step.outcome.as_deref() == Some("Broke"))
                    .count();
                let mut answer = json!({
                    "found": true,
                    "flow": run.entity,
                    "run_id": run.run_id,
                    "status": run.status,
                    "started_at": run.started_at,
                    "ended_at": run.ended_at,
                    "seconds": run.ended_at - run.started_at,
                    "steps": steps,
                    "broke": broke,
                });
                if include_said.unwrap_or(false) {
                    let excerpts = ledger
                        .said_of_failed_steps(&run.run_id, SAID_MAX_STEPS, SAID_MAX_BYTES)
                        .map_err(unreadable)?;
                    let said: Vec<Value> = excerpts
                        .iter()
                        .map(|excerpt| {
                            json!({
                                "step_id": excerpt.step_id,
                                "attempt": excerpt.attempt,
                                "said": excerpt.said,
                                "said_truncated": excerpt.truncated,
                            })
                        })
                        .collect();
                    answer["said"] = Value::Array(said);
                }
                Ok((1, answer))
            }
            Ask::StepDuration {
                step_id,
                flow,
                within_last_runs,
            } => {
                let limit = window(*within_last_runs)?;
                let flow = flow.as_deref();
                let durations = ledger
                    .step_durations(step_id, flow, limit)
                    .map_err(unreadable)?;
                let considered = ledger.runs_in_window(flow, limit).map_err(unreadable)?;
                let mut answer = duration_summary(&durations);
                answer["step_id"] = json!(step_id);
                Ok((considered, answer))
            }
        }
    }
}

fn unreadable(error: ledger::LedgerError) -> ActionError {
    ActionError::new("history_unreadable", error.to_string())
}

impl Action for HistoryAskAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        // La domanda si valida **prima** di guardare il deposito: un campo
        // sbagliato è sbagliato su qualunque macchina, e scoprirlo solo dove il
        // deposito esiste renderebbe il difetto invisibile a chi prova altrove.
        let ask = parse_ask(input)?;
        let name = ask_name(&ask);
        let Some(ledger) = self.ledger.as_ref() else {
            return Ok(ActionOutcome::Went(envelope(name, DEPOSIT_ABSENT, 0, None)));
        };
        let recorded = ledger.recorded_runs().map_err(unreadable)?;
        if recorded == 0 {
            return Ok(ActionOutcome::Went(envelope(name, DEPOSIT_EMPTY, 0, None)));
        }
        let (considered, answer) = self.answer(ledger, &ask)?;
        Ok(ActionOutcome::Went(envelope(
            name,
            DEPOSIT_PRESENT,
            considered,
            Some(answer),
        )))
    }

    fn species(&self) -> StepSpecies {
        // Legge e basta: rilanciarlo non tocca niente del mondo.
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flow::{Completion, Outcome, StepRecord};
    use ledger::RunRecord;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TestStore(std::path::PathBuf);

    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn store(label: &str) -> (Ledger, TestStore) {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sailor-actions-history-{label}-{}-{sequence}",
            std::process::id()
        ));
        let ledger = Ledger::open(&path).expect("aprire il deposito");
        (ledger, TestStore(path))
    }

    /// Il testo che non deve mai uscire: entra come ingresso e come uscita di
    /// ogni passo di prova, così una fuga si vede a occhio nella risposta.
    const SECRET: &str = "PROMPT-SEGRETO-CHE-NON-DEVE-USCIRE";

    fn a_run(ledger: &Ledger, run_id: &str, flow: &str, started_at: i64, ended_at: Option<i64>) {
        ledger
            .record_run(&RunRecord {
                run_id: run_id.to_owned(),
                kind: "flow".to_owned(),
                entity: flow.to_owned(),
                parent_run_id: None,
                started_by: "prova".to_owned(),
                status: "done".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at,
                ended_at,
            })
            .expect("registrare la corsa");
    }

    fn a_step(
        ledger: &Ledger,
        run_id: &str,
        step_id: &str,
        started_at: i64,
        outcome: Outcome,
        failure_class: Option<&str>,
        ended_at: i64,
    ) {
        let record = StepRecord::started(
            run_id,
            step_id,
            1,
            1,
            vec![],
            json!({"prompt": SECRET}),
            vec![],
            started_at,
        );
        ledger
            .append_step_started(&record)
            .expect("registrare l'intenzione");
        ledger
            .close_step(
                run_id,
                step_id,
                1,
                1,
                Completion {
                    outcome,
                    output: Some(json!({"risposta": SECRET})),
                    said: Some(format!("{SECRET} detto da {step_id}")),
                    failure_class: failure_class.map(str::to_owned),
                    ended_at,
                    bytes_seen: Some(64),
                    bytes_discarded: Some(0),
                },
            )
            .expect("chiudere il passo");
    }

    fn went(action: &HistoryAskAction, question: Value) -> Value {
        let mut shared = SharedState::new();
        let outcome = action
            .execute(&question, &mut shared)
            .expect("la domanda risponde");
        let ActionOutcome::Went(value) = outcome else {
            panic!("una lettura locale non aspetta nessuno");
        };
        value
    }

    /// Senza deposito la domanda riceve comunque una risposta, e la risposta
    /// dice che deposito non ce n'è.
    ///
    /// Il mutante che la fa cadere è far fallire il passo quando il deposito
    /// manca: ogni flusso che chiede com'è andata nascerebbe rosso su una
    /// macchina appena installata, cioè proprio dove il ramo «non lo so» serve.
    #[test]
    fn without_a_deposit_the_question_still_gets_an_answer() {
        let action = HistoryAskAction::new(None);

        let value = went(&action, json!({"ask": "failure_classes"}));

        assert_eq!(value["deposit"], json!("absent"));
        assert_eq!(value["ask"], json!("failure_classes"));
        assert_eq!(
            value.get("answer"),
            None,
            "senza deposito non c'è nessuna risposta da dare"
        );
    }

    /// «Nessuna corsa registrata» e «zero guasti» sono due cose diverse, e si
    /// distinguono dalla **presenza** della chiave `answer`.
    ///
    /// Il mutante che la fa cadere è restituire `answer: null` sul deposito
    /// vuoto: `Condition::PointerExists` si appoggia a `Value::pointer`, che su
    /// un `null` risponde `Some`, quindi il flusso prenderebbe il ramo «ho una
    /// risposta» proprio dove non ce n'è nessuna.
    #[test]
    fn an_empty_deposit_is_not_the_same_as_zero_failures() {
        let (ledger, _guard) = store("vuoto");
        let action = HistoryAskAction::new(Some(ledger.clone()));

        let empty = went(
            &action,
            json!({"ask": "step_failures", "step_id": "compile"}),
        );
        assert_eq!(empty["deposit"], json!("empty"));
        assert_eq!(
            empty.get("answer"),
            None,
            "sul deposito vuoto la chiave non c'è, nemmeno a null: {empty}"
        );

        a_run(&ledger, "run-1", "alpha", 100, Some(200));
        a_step(&ledger, "run-1", "compile", 100, Outcome::Went, None, 130);

        let quiet = went(
            &action,
            json!({"ask": "step_failures", "step_id": "compile"}),
        );
        assert_eq!(quiet["deposit"], json!("present"));
        assert_eq!(
            quiet["answer"]["failures"],
            json!(0),
            "zero guasti è il numero zero"
        );
        assert_eq!(quiet["answer"]["attempts"], json!(1));
        assert_eq!(quiet["runs_considered"], json!(1));
    }

    /// Un campo che la domanda non conosce si rifiuta, dicendo quale.
    ///
    /// Cade se il controllo dei campi ignoti sparisce: `step-id` verrebbe
    /// ignorato, `step_id` mancherebbe, e la domanda risponderebbe su tutti i
    /// passi insieme — un numero plausibile e sbagliato.
    #[test]
    fn an_unknown_field_is_refused_by_name() {
        let action = HistoryAskAction::new(None);
        let mut shared = SharedState::new();

        let error = action
            .execute(
                &json!({"ask": "step_failures", "step_id": "compile", "step-id": "compile"}),
                &mut shared,
            )
            .expect_err("un campo ignoto non si ignora");

        assert_eq!(error.class, "invalid_input");
        assert!(
            error.said.contains("step-id"),
            "va nominato: {}",
            error.said
        );
    }

    /// Una domanda che nessuno ha previsto si rifiuta dicendo quali esistono.
    #[test]
    fn an_unknown_question_names_the_ones_that_exist() {
        let action = HistoryAskAction::new(None);
        let mut shared = SharedState::new();

        let error = action
            .execute(&json!({"ask": "select_star_from_steps"}), &mut shared)
            .expect_err("non esiste");

        assert_eq!(error.class, "invalid_input");
        assert!(error.said.contains("last_run"), "{}", error.said);
    }

    /// **NIENTE DI CIÒ CHE È PASSATO NEL FLUSSO ESCE SENZA CHE SIA CHIESTO.**
    ///
    /// La prova guarda il testo intero della risposta, non i campi che si
    /// ricorda di controllare: cade il giorno in cui qualcuno aggiunge `output`
    /// «per comodità», e cade anche se il segreto arriva dentro un campo che
    /// oggi non esiste.
    #[test]
    fn a_last_run_answer_never_carries_what_passed_through_the_flow() {
        let (ledger, _guard) = store("senza-detto");
        a_run(&ledger, "run-1", "alpha", 100, Some(400));
        a_step(
            &ledger,
            "run-1",
            "compile",
            100,
            Outcome::Broke,
            Some("timeout"),
            150,
        );
        let action = HistoryAskAction::new(Some(ledger));

        let value = went(&action, json!({"ask": "last_run", "flow": "alpha"}));
        let text = value.to_string();

        assert_eq!(value["answer"]["found"], json!(true));
        assert_eq!(value["answer"]["steps"][0]["outcome"], json!("Broke"));
        assert_eq!(
            value["answer"]["steps"][0]["failure_class"],
            json!("timeout")
        );
        assert!(!text.contains(SECRET), "il canale dati non esce: {text}");
        assert!(
            !text.contains("\"said\""),
            "il testo grezzo non esce senza che sia chiesto: {text}"
        );
        assert!(!text.contains("\"input\""), "{text}");
        assert!(!text.contains("\"output\""), "{text}");
    }

    /// Chiesto esplicitamente, il testo grezzo esce — dai soli passi rotti,
    /// della sola corsa nominata, e troncato.
    ///
    /// Il mutante che la fa cadere è far uscire `said` sempre: la prova qui
    /// sopra diventerebbe rossa, e questa resterebbe verde. Servono tutte e
    /// due, e una sola delle due non prova la politica.
    #[test]
    fn said_comes_out_only_when_asked_and_only_from_broken_steps() {
        let (ledger, _guard) = store("con-detto");
        a_run(&ledger, "run-1", "alpha", 100, Some(400));
        a_step(&ledger, "run-1", "riuscito", 100, Outcome::Went, None, 150);
        a_step(
            &ledger,
            "run-1",
            "rotto",
            200,
            Outcome::Broke,
            Some("timeout"),
            250,
        );
        let action = HistoryAskAction::new(Some(ledger));

        let value = went(
            &action,
            json!({"ask": "last_run", "flow": "alpha", "include_said": true}),
        );
        let said = value["answer"]["said"]
            .as_array()
            .expect("l'elenco c'è")
            .clone();

        assert_eq!(said.len(), 1, "solo il passo rotto: {said:?}");
        assert_eq!(said[0]["step_id"], json!("rotto"));
        assert!(said[0]["said"].as_str().expect("testo").contains("rotto"));
        assert_eq!(said[0]["said_truncated"], json!(false));
        // Il passo riuscito ha un `said` nel deposito e non esce: il varco è
        // per la diagnosi di un guasto, non per leggere le corse riuscite.
        assert!(!value.to_string().contains("detto da riuscito"), "{value}");
    }

    /// La domanda sulle durate misura le riuscite e conta i guasti a parte, e
    /// dice su quanti campioni sta parlando.
    ///
    /// Cade se il riassunto smette di dichiarare `samples`: un novantesimo
    /// percentile su due misure sembrerebbe una legge.
    #[test]
    fn a_duration_answer_says_how_few_samples_it_stands_on() {
        let (ledger, _guard) = store("durate");
        a_run(&ledger, "run-1", "alpha", 100, Some(900));
        a_step(&ledger, "run-1", "compile", 100, Outcome::Went, None, 110);
        let action = HistoryAskAction::new(Some(ledger));

        let value = went(
            &action,
            json!({"ask": "step_duration", "step_id": "compile", "flow": "alpha"}),
        );
        let answer = &value["answer"];

        assert_eq!(answer["samples"], json!(1));
        assert_eq!(answer["median"], json!(10));
        assert_eq!(answer["last"], json!(10));
        assert_eq!(
            answer["unit"],
            json!("seconds"),
            "l'unità si dichiara: l'orologio conta secondi"
        );
    }

    /// Su zero campioni non esce nessuna misura inventata.
    #[test]
    fn no_samples_means_no_numbers_pretending_to_be_measures() {
        let summary = duration_summary(&StepDurations::default());

        assert_eq!(summary["samples"], json!(0));
        assert_eq!(
            summary.get("median"),
            None,
            "una mediana di niente non si scrive: {summary}"
        );
        assert_eq!(summary.get("last"), None);
    }

    /// La mediana e il novantesimo percentile sono campioni veri, presi per
    /// rango: nessun numero che non sia mai stato misurato.
    #[test]
    fn percentiles_are_measured_samples_not_averages() {
        let sorted = vec![10, 20, 30, 40];

        assert_eq!(percentile(&sorted, 50), Some(20));
        assert_eq!(percentile(&sorted, 90), Some(40));
        assert_eq!(percentile(&[], 50), None);
    }

    /// Una finestra fuori dai limiti si rifiuta invece di essere ridotta di
    /// nascosto: chi crede di aver guardato cinquemila corse deve saperlo.
    #[test]
    fn a_window_beyond_the_ceiling_is_refused_instead_of_silently_shrunk() {
        let action = HistoryAskAction::new(None);
        let mut shared = SharedState::new();

        let error = action
            .execute(
                &json!({"ask": "failure_classes", "within_last_runs": 5000}),
                &mut shared,
            )
            .expect_err("oltre il tetto");

        assert_eq!(error.class, "invalid_input");
        assert!(error.said.contains("500"), "{}", error.said);
    }

    /// Un flusso che non ha ancora chiuso nessuna corsa riceve `found: false`,
    /// e il passo riesce: è il primo giro, non un guasto.
    #[test]
    fn a_flow_with_no_finished_run_is_told_so_without_breaking() {
        let (ledger, _guard) = store("primo-giro");
        a_run(&ledger, "run-1", "altro-flusso", 100, Some(200));
        a_step(&ledger, "run-1", "compile", 100, Outcome::Went, None, 130);
        let action = HistoryAskAction::new(Some(ledger));

        let value = went(&action, json!({"ask": "last_run", "flow": "mai-girato"}));

        assert_eq!(value["deposit"], json!("present"));
        assert_eq!(value["answer"]["found"], json!(false));
    }
}
