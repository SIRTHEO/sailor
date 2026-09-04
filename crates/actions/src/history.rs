//! The node a flow asks **how it went** with, the times before.
//!
//! **SURFACE: `gate`. POWERS CLAIMED: none.** It does not read the world, does
//! not touch it, does not write to the deposit of its own: it offers a mandate
//! and waits. Declared here because the four surfaces do not exist in the code
//! yet, and a new action that stays quiet while the criterion is being written
//! becomes the first unwritten exception.
//!
//! **WHY IT EXISTS.** The deposit has always recorded how every step of every
//! run ended, with which failure class and how long it took, and no flow could
//! read it back. The three `store` nodes do not serve: they read a keyed
//! archive — the facts a flow decided to remember — not what happened to it.
//! Without it a flow runs the same steps the same way however the last twenty
//! went: a system that repeats instead of one that accumulates experience.
//!
//! **IT IS ASKED IN NAMED QUESTIONS, NOT IN SQL**, for two measured reasons.
//! SQL would tie every flow file to the shape of today's `steps` table, which
//! has already gained columns; with four named questions the schema can change
//! and the flows stay valid, because the SQL lives where the schema lives. And
//! a closed question **has no arbitrary projection**: there is no syntax for
//! «give me that column», so the secret is not defended by a blacklist somebody
//! forgets to update but by the absence of a way to ask.
//!
//! The price is declared: an unforeseen question needs a new variant in Rust,
//! not a line in a data file — the same choice `reference.rs` made for
//! references, three closed forms instead of a language, and for the same
//! reason.
//!
//! **WHAT COMES OUT AND WHAT DOES NOT.** Always the structural facts and the
//! measures — step, run, flow, outcome, failure class, attempt, instants,
//! durations, counts, bytes. Never `input` and `output`, the typed data channel
//! where prompts, environments and model answers travel: returning them would
//! put arbitrary structured values back into a step's input, the gap
//! `reference.rs` declares open for `store_read` and which does not widen. The
//! ban is structural — the types returned on this path have no such fields.
//!
//! **ONLY IF ASKED**: `said`, with `include_said: true`, admitted on `last_run`
//! alone, on that run's broken steps only, at most [`SAID_MAX_STEPS`] steps and
//! [`SAID_MAX_BYTES`] bytes each, with `said_truncated` declaring the cut.
//! `record.rs` describes it as raw text for a person when something goes wrong,
//! not as data to decide on: reachable, because without it a diagnosis cannot
//! be made, and narrow, because none of the four questions needs it.
//!
//! **AN EMPTY DEPOSIT IS AN ANSWER, NOT A FAULT.** The envelope always carries
//! `deposit`: `absent`, `empty` or `present`. The key `answer` exists **only**
//! in the last case and never appears set to `null`, and the reason is in the
//! engine: `Condition::PointerExists` leans on `Value::pointer`, which answers
//! `Some` on a `null` — so `answer: null` would fire the «I have an answer»
//! branch on a freshly installed machine, the very case to be told apart.
//!
//! Omitting the key separates «I do not know» from «zero», and `PointerEquals`
//! on `/deposit` separates an absent deposit from an empty one. Inside
//! `answer`, no faults is the number zero. In none of the three cases does the
//! step fail: as with `store_read`, a first run is not born red.

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
    /// Which runs are still open: a recorded intent and no outcome, or a step
    /// waiting for somebody. The two are kept apart because the repair is not
    /// the same — one is resumed, the other is taken up by a person.
    OpenRuns {},
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
        "open_runs" => Some(&["ask"]),
        "step_duration" => Some(&["ask", "step_id", "flow", "within_last_runs"]),
        _ => None,
    }
}

const KNOWN_ASKS: &str = "step_failures, failure_classes, last_run, open_runs, step_duration";

fn parse_ask(input: &Value) -> Result<Ask, ActionError> {
    let object = input.as_object().ok_or_else(|| {
        ActionError::new(
            "invalid_input",
            format!(
                "a question to the history is an object with an `ask` field among: {KNOWN_ASKS}"
            ),
        )
    })?;
    let name = object
        .get("ask")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ActionError::new(
                "invalid_input",
                format!("the `ask` field is missing. The possible questions are: {KNOWN_ASKS}"),
            )
        })?
        .to_owned();
    let allowed = allowed_fields(&name).ok_or_else(|| {
        ActionError::new(
            "invalid_input",
            format!("unknown question `{name}`. The possible questions are: {KNOWN_ASKS}"),
        )
    })?;
    // **THE TYPO IS CAUGHT AT CHECK TIME, WHERE THE TEXT IS A PERSON'S.** Here
    // the input is also the output of the step before, where foreign fields are
    // the norm: refusing them made this node unusable after any dependency, and
    // the contract on `Action::unknown_fields` says so in as many words.
    let pruned: serde_json::Map<String, Value> = object
        .iter()
        .filter(|(key, _)| allowed.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let ask: Ask = serde_json::from_value(Value::Object(pruned))
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
        // `last_run` looks at one run by definition, and what is still open is
        // open now: neither has a window to declare.
        Ask::LastRun { .. } | Ask::OpenRuns {} => None,
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
            format!("`within_last_runs` goes between 1 and {MAX_WINDOW}, not {value}"),
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
        Ask::OpenRuns {} => "open_runs",
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

    fn answer(
        &self,
        ledger: &Ledger,
        ask: &Ask,
        asking: Option<&str>,
    ) -> Result<(i64, Value), ActionError> {
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
            Ask::OpenRuns {} => {
                let halfway = ledger.unfinished_runs().map_err(unreadable)?;
                // **THE RUN THAT ASKS IS NOT ONE OF THE OPEN ONES.** While
                // this question answers, its own run has open steps by
                // construction: counting it means whoever watches at every beat
                // always reads one open, and learns to stop reading it.
                let halfway: Vec<_> = halfway
                    .into_iter()
                    .filter(|run| Some(run.run_id.as_str()) != asking)
                    .collect();
                let waiting = ledger.waiting_runs().map_err(unreadable)?;
                let considered = ledger.recorded_runs().map_err(unreadable)?;
                Ok((
                    considered,
                    json!({
                        "halfway": halfway
                            .iter()
                            .map(|run| json!({
                                "run_id": run.run_id,
                                "flow": run.entity,
                                "open_steps": run.open_steps,
                                "oldest_started_at": run.oldest_started_at,
                            }))
                            .collect::<Vec<Value>>(),
                        "waiting": waiting
                            .iter()
                            .map(|run| json!({
                                "run_id": run.run_id,
                                "flow": run.entity,
                                "waiting_since": run.waiting_since,
                            }))
                            .collect::<Vec<Value>>(),
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
    /// What a hand-written `with` says that this question does not know: a
    /// `step-id` for `step_id` would otherwise be answered about every step.
    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        let Some(object) = declared.as_object() else {
            return Vec::new();
        };
        let Some(allowed) = object
            .get("ask")
            .and_then(Value::as_str)
            .and_then(allowed_fields)
        else {
            return Vec::new();
        };
        object
            .keys()
            .filter(|key| !allowed.contains(&key.as_str()))
            .cloned()
            .collect()
    }

    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
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
        let asking = shared.get(flow::CURRENT_RUN).and_then(Value::as_str);
        let (considered, answer) = self.answer(ledger, &ask, asking)?;
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
                worktree: None,
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

    /// What is open now, kept apart by what closing it takes: a run left
    /// halfway is resumed, a run waiting is taken up by a person. One list
    /// would make the count right and the next gesture unknown.
    #[test]
    fn what_is_still_open_says_which_runs_are_resumed_and_which_are_waited_on() {
        let (ledger, _kept) = store("aperte");
        a_run(&ledger, "finita", "un-flusso", 10, Some(20));
        a_step(&ledger, "finita", "passo", 10, Outcome::Went, None, 20);
        a_run(&ledger, "a-meta", "un-flusso", 30, None);
        a_step_left_open(&ledger, "a-meta", "passo", 30);
        a_waiting_run(&ledger, "in-attesa", "un-altro", 40);

        let action = HistoryAskAction::new(Some(ledger));
        let value = went(&action, json!({"ask": "open_runs"}));

        assert_eq!(value["deposit"], json!("present"));
        let halfway: Vec<&str> = value["answer"]["halfway"]
            .as_array()
            .expect("the runs left halfway")
            .iter()
            .map(|run| run["run_id"].as_str().expect("a run id"))
            .collect();
        assert_eq!(halfway, vec!["a-meta"], "a closed run is not open, {value}");
        let waiting: Vec<&str> = value["answer"]["waiting"]
            .as_array()
            .expect("the runs waiting")
            .iter()
            .map(|run| run["run_id"].as_str().expect("a run id"))
            .collect();
        assert_eq!(waiting, vec!["in-attesa"], "{value}");
        assert!(
            !serde_json::to_string(&value).expect("it serialises").contains(SECRET),
            "what passed through the flow does not come back out: {value}"
        );
    }

    /// **A WATCH THAT COUNTS ITSELF TEACHES PEOPLE TO IGNORE IT.** While this
    /// question answers, its own run has open steps by construction: found on
    /// the first real run of `watch-the-crew`, which reported itself.
    #[test]
    fn the_run_that_asks_is_not_among_the_ones_left_open() {
        let (ledger, _kept) = store("chiede");
        a_run(&ledger, "chi-chiede", "la-guardia", 30, None);
        a_step_left_open(&ledger, "chi-chiede", "passo", 30);
        a_run(&ledger, "un-altra", "un-flusso", 40, None);
        a_step_left_open(&ledger, "un-altra", "passo", 40);

        let action = HistoryAskAction::new(Some(ledger));
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_RUN.to_owned(), json!("chi-chiede"));
        let ActionOutcome::Went(value) = action
            .execute(&json!({"ask": "open_runs"}), &mut shared)
            .expect("the question answers")
        else {
            panic!("a local reading waits for nobody");
        };

        let halfway: Vec<&str> = value["answer"]["halfway"]
            .as_array()
            .expect("the runs left halfway")
            .iter()
            .map(|run| run["run_id"].as_str().expect("a run id"))
            .collect();
        assert_eq!(
            halfway,
            vec!["un-altra"],
            "the asking run counted itself: {value}"
        );
    }

    /// A run whose status is what the engine writes when it hands over: the
    /// store finds it by that word, not by an open step.
    fn a_waiting_run(ledger: &Ledger, run_id: &str, flow: &str, started_at: i64) {
        ledger
            .record_run(&RunRecord {
                run_id: run_id.to_owned(),
                kind: "flow".to_owned(),
                entity: flow.to_owned(),
                parent_run_id: None,
                started_by: "prova".to_owned(),
                status: "waiting".to_owned(),
                total_cost_micros: 0,
                error: None,
                started_at,
                ended_at: None,
                worktree: None,
            })
            .expect("registrare la corsa in attesa");
    }

    /// A step whose intent is recorded and whose outcome never was.
    fn a_step_left_open(ledger: &Ledger, run_id: &str, step_id: &str, started_at: i64) {
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

    /// The typo is named where the text is a person's: `step-id` for `step_id`
    /// would be answered about every step at once — a plausible wrong number.
    /// At run time it is not refused: the input is also the output of the step
    /// before, and refusing a field somebody else wrote made this node
    /// unusable after any dependency.
    #[test]
    fn a_typo_in_the_written_question_is_named_before_the_run() {
        let action = HistoryAskAction::new(None);
        let written = json!({"ask": "step_failures", "step_id": "compile", "step-id": "compile"});

        assert_eq!(action.unknown_fields(&written), vec!["step-id".to_owned()]);
        assert!(
            action
                .unknown_fields(&json!({"ask": "step_failures", "step_id": "compile"}))
                .is_empty(),
            "a question written right accuses nobody"
        );

        let mut shared = SharedState::new();
        let carried = action
            .execute(
                &json!({"ask": "last_run", "flow": "a-flow", "text": "from the step before"}),
                &mut shared,
            )
            .expect("a field from the step before is not a typo of ours");
        let ActionOutcome::Went(answer) = carried else {
            panic!("a question always answers")
        };
        assert_eq!(answer["ask"], json!("last_run"), "and the question stays the one asked");
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
