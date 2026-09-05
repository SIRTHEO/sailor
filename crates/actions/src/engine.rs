//! The engine action: a step asks an engine, or a chain of them, and what it
//! answers becomes the step's output or breaks it.

use crate::answer::{
    check_tolerance, how_it_exited, shape_was_asked_for, shaped_answer, tolerates, what_it_said,
    ENGINE_FAILURES,
};
use crate::candidates::{strengths_path, Candidate, Refused};
use crate::cost::{now_secs, record_the_call, recording_for, Chain, Recording, Spent};
use crate::equipment::current_equipment_for;
use crate::process::{
    invoke_external_engine_watched_until, sink_for_step, EngineInvocation, EngineResult, LiveSink,
    Pipe, StepSinks,
};
use crate::recipe::{PromptVia, ToolResolver};
use crate::session::{session_plan, SessionPlan};
use crate::spec::{EngineSpec, A_TREE_OF_ITS_OWN, TREE};
use crate::{budget, cooldown, Reading};
use flow::{Action, ActionError, ActionOutcome, Ran, SharedState, StepSpecies, ValueSchema};
use ledger::{EngineIdentity, Ledger};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

// ── le due azioni registrabili in un flow::ActionRegistry ───────────────

#[derive(Debug, Serialize)]
struct EngineOutcomeJson {
    status: &'static str,
    stdout: String,
    stderr: String,
}

/// Invoca un motore esterno leggendo la sua ricetta dall'ingresso tipato del
/// passo. Non sa nulla di quale motore o di quale coda la chiama.
///
/// La ricetta può contenere rinvii (`reference`): è così che l'incarico scritto
/// da un motore diventa l'ingresso del motore dopo.
///
/// **UN FALLIMENTO ROMPE IL PASSO.** Un'uscita diversa da zero, un tempo
/// scaduto, un binario che non parte: ognuno di questi chiude il passo come
/// rotto, con dentro il perché e le ultime righe di ciò che il motore ha detto.
/// I passi che dipendono da lui non partono — che è il punto: prima del
/// 28/08/2026 partivano, ricevendo il vuoto e spendendo una chiamata vera.
///
/// **E RESTA POSSIBILE DIRE IL CONTRARIO**, perché esiste chi esegue un comando
/// apposta per vedere se fallisce: `"accept": ["exit_error"]` nel passo rimette
/// quell'esito fra i dati, con lo `status` che lo dice, come era per tutti
/// prima. Ma va scritto, e vale solo per l'esito nominato.
pub struct ExternalEngineAction {
    pub(crate) tools: Option<Arc<dyn ToolResolver>>,
    pub(crate) watcher: Option<Arc<dyn StepSinks>>,
    pub(crate) ledger: Option<Ledger>,
    /// Where the engines set aside for a spent quota are listed; `None` when
    /// the machine has no home to keep the list in, and then nobody is aside.
    pub(crate) cooldowns: Option<PathBuf>,
    /// Where the person's spend caps per engine live; `None` means no cap.
    pub(crate) budgets: Option<PathBuf>,
    /// The person's strengths table, or `None` for the shipped one.
    pub(crate) strengths: Option<PathBuf>,
}

impl Default for ExternalEngineAction {
    fn default() -> Self {
        Self::new()
    }
}

impl ExternalEngineAction {
    /// Senza risolutore: un passo che chiede uno strumento per identificativo
    /// riceve un errore che dice come si ripara, invece di un binario indovinato.
    pub fn new() -> Self {
        Self {
            tools: None,
            watcher: None,
            ledger: None,
            cooldowns: cooldown::default_path(),
            budgets: budget::default_path(),
            strengths: strengths_path(),
        }
    }

    /// Con un risolutore: `"tool": "codex"` diventa il percorso che vale
    /// `codex` su questa macchina.
    pub fn resolving_with(resolver: impl ToolResolver + 'static) -> Self {
        Self {
            tools: Some(Arc::new(resolver)),
            watcher: None,
            ledger: None,
            cooldowns: cooldown::default_path(),
            budgets: budget::default_path(),
            strengths: strengths_path(),
        }
    }

    /// With the person's spend caps read from `path`: a test hands a scratch
    /// file, so no test reads the machine's own caps.
    pub fn budgeted_by(mut self, path: Option<PathBuf>) -> Self {
        self.budgets = path;
        self
    }

    /// With the strengths table read from `path` instead of the shipped one.
    pub fn strong_by(mut self, path: Option<PathBuf>) -> Self {
        self.strengths = path;
        self
    }

    /// With the list of engines set aside kept at `path`: a test hands a
    /// scratch file, so no test writes the machine's own list.
    pub fn cooling_down_in(mut self, path: Option<PathBuf>) -> Self {
        self.cooldowns = path;
        self
    }

    /// Con qualcuno che guarda: il testo del motore gli arriva mentre esce,
    /// marcato col passo che lo sta producendo. `None` vuol dire che nessuno
    /// guarda, ed è il valore con cui l'azione nasce — chi la registra decide,
    /// non questo crate.
    pub fn watched_by(mut self, watcher: Option<Arc<dyn StepSinks>>) -> Self {
        self.watcher = watcher;
        self
    }

    /// Con un deposito dove registrare quanto è costata ogni chiamata.
    ///
    /// **PERCHÉ IL DEPOSITO ARRIVA PER COSTRUZIONE E LA CORSA NO.** Chi
    /// costruisce il registro delle azioni ha già il deposito aperto in mano —
    /// è la stessa strada che `store::register_store` segue da sempre — ma non
    /// ha ancora la corsa: il `run_id` nasce dopo, quando si sta per partire.
    /// La corsa arriva quindi dallo stato condiviso (`flow::CURRENT_RUN`), che
    /// l'esecutore riempie passo per passo.
    ///
    /// `None` è il valore con cui l'azione nasce, e vuol dire che non si
    /// registra niente: un motore invocato senza deposito funziona esattamente
    /// come prima.
    pub fn recording_to(mut self, ledger: Option<Ledger>) -> Self {
        self.ledger = ledger;
        self
    }
}

/// The tree one step works in, taken down when that step ends however it ends.
///
/// Closed on drop and not by a line at the bottom: an engine step leaves by a
/// dozen paths, and a tree closed on one of them is a tree left open on the
/// other eleven. See fault 89.
struct OwnTree {
    repo: PathBuf,
    at: PathBuf,
    live: Option<Arc<dyn LiveSink>>,
}

impl OwnTree {
    /// A kept tree is named where the tree was announced, or the run ends with
    /// disk nobody accounted for.
    fn say(&self, sentence: String) {
        match self.live.as_deref() {
            Some(live) => live.chunk(Pipe::Stderr, format!("[sailor] {sentence}\n").as_bytes()),
            None => eprintln!("{sentence}"),
        }
    }
}

impl Drop for OwnTree {
    fn drop(&mut self) {
        let at = self.at.to_string_lossy().into_owned();
        match workspace::close_tree(&self.repo, &self.at) {
            workspace::Closing::TakenDown => {}
            workspace::Closing::GitRefused(said) => self.say(catalogue::say(
                "engine.tree_kept_over_work",
                &[("tree", &at), ("said", said.trim())],
            )),
            workspace::Closing::HoldsACommitNobodyElseHas(commit) => self.say(catalogue::say(
                "engine.tree_kept_over_a_commit",
                &[("tree", &at), ("commit", &commit)],
            )),
        }
    }
}

/// The worktree this step works in, cut now if it is not there yet.
///
/// Refused rather than run in the tree everybody shares: a step that asked to
/// be alone and silently got the shared tree writes over another engine's work.
fn tree_of_its_own(
    spec: &EngineSpec,
    shared: &SharedState,
    live: Option<Arc<dyn LiveSink>>,
) -> Result<Option<OwnTree>, ActionError> {
    let Some(asked) = spec.tree.as_deref() else {
        return Ok(None);
    };
    if asked != A_TREE_OF_ITS_OWN {
        return Err(ActionError::new(
            "invalid_input",
            format!("`{TREE}` knows only «{A_TREE_OF_ITS_OWN}», not «{asked}»"),
        ));
    }
    if spec.workdir.is_some() {
        return Err(ActionError::new(
            "invalid_input",
            format!(
                "the step asks for a `{TREE}` of its own and also names a `workdir`: \
                 the two say different places, and one of them would be ignored"
            ),
        ));
    }
    let said = |key: &str| shared.get(key).and_then(Value::as_str).map(str::to_owned);
    let (Some(root), Some(run), Some(step)) = (
        said(flow::WORKSPACE_ROOT),
        said(flow::CURRENT_RUN),
        said(flow::CURRENT_STEP),
    ) else {
        return Err(ActionError::new(
            "invalid_input",
            format!(
                "the step asks for a `{TREE}` of its own, and this run says neither which \
                 project nor which run and step it is: there is no name to cut it under"
            ),
        ));
    };
    let repo = PathBuf::from(&root);
    workspace::tree_for(&repo, &run, &step)
        .map(|at| Some(OwnTree { repo, at, live }))
        .map_err(|why| ActionError::new("tree_not_cut", why))
}

/// L'esito di una domanda a **un** motore della catena.
enum Asked {
    /// Questo motore ha risposto: quella è la risposta del passo, comunque sia
    /// andata. Nessuno dopo di lui viene provato.
    Answered(ActionOutcome),
    /// Questo motore ha dichiarato di non poter lavorare — con le parole che il
    /// suo descrittore dichiara, non con un'interpretazione nostra. Si prova il
    /// prossimo.
    CannotWork(String),
}

/// Perché ognuno è stato messo da parte, in una riga sola.
fn each_one_why(reasons: &[String]) -> String {
    if reasons.is_empty() {
        return "Nessun motivo registrato.".to_owned();
    }
    reasons.join(" · ")
}

/// Cosa succede a un passo quando il motore ha detto di **non poter lavorare**.
///
/// **STA IN UNA COPIA SOLA, E NON È PIGNOLERIA.** La *regola* — quali parole
/// contano — era già in un posto solo (`mentions_any`); la **conseguenza** no:
/// era scritta due volte, una per ramo, e le due copie erano già divergenti
/// appena nate. È il guasto 10 rientrato dalla porta di servizio, sullo stesso
/// codice che stava riparando il guasto sul ripiego.
///
/// La differenza che questa funzione tiene è quella che conta per chi legge:
/// **con una catena dietro** il lavoro passa al prossimo, e non c'è ancora
/// nessun errore da dare; **da solo** non c'è nessun prossimo, ma la diagnosi
/// resta — chi legge «esaurito» sa che deve aspettare o cambiare profilo, chi
/// legge «uscito in errore» va a cercare un guasto che non c'è.
fn engine_cannot_work(
    named: &str,
    solo: bool,
    stdout: &str,
    stderr: &str,
) -> Result<Asked, ActionError> {
    if !solo {
        return Ok(Asked::CannotWork(format!(
            "{named} could not work: {}",
            what_it_said(stdout, stderr)
        )));
    }
    Err(ActionError::new(
        "engine_exhausted",
        format!(
            "{named} ran out of its quota, it did not break: {}",
            what_it_said(stdout, stderr)
        ),
    ))
}

/// The line one engine is about to be started with, and what the record of
/// the call needs to know about how it was composed.
struct Prepared {
    invocation: EngineInvocation,
    session: SessionPlan,
    identity: EngineIdentity,
    named: String,
}

/// Composes the line for one engine: which session it continues, where the
/// prompt goes, under which equipment it starts. `set_aside` names the engines
/// already put aside, so the echo can say this one is the fallback.
fn compose(
    candidate: &Candidate,
    spec: &EngineSpec,
    live: Option<&dyn LiveSink>,
    set_aside: &[String],
    record: Option<&Recording<'_>>,
) -> Prepared {
    let bin = &candidate.bin;
    let named = match candidate.id.as_deref() {
        Some(id) => format!("«{id}» (`{bin}`)"),
        None => format!("`{bin}`"),
    };
    // Prima di montare la riga: questa chiamata continua qualcosa, o parte
    // da zero? Non può fallire — al massimo riparte da zero dicendolo.
    let session = session_plan(candidate, spec.session.as_ref(), spec.blind, record, live, &named);
    let mut args = session
        .args
        .clone()
        .unwrap_or_else(|| candidate.args.clone());
    // Il testo della domanda va dove quel motore lo vuole: sull'ingresso per
    // chi legge da lì, in coda agli argomenti per chi lo vuole scritto sulla
    // riga. È l'unica differenza fra due motori che il flusso non deve più
    // conoscere.
    let stdin = match candidate.prompt {
        PromptVia::Stdin => spec.stdin.clone(),
        PromptVia::LastArg => {
            if let Some(text) = &spec.stdin {
                args.push(text.clone());
            }
            None
        }
    };
    if let (Some(live), Some(id)) = (live, candidate.id.as_deref()) {
        if !set_aside.is_empty() {
            live.chunk(
                Pipe::Stderr,
                format!("[sailor] moving on to engine «{id}»\n").as_bytes(),
            );
        }
        if let Some(why) = &candidate.why {
            live.chunk(Pipe::Stderr, format!("[sailor] preferring {why}\n").as_bytes());
        }
    }
    // **LA DOTAZIONE DI SAILOR, NON QUELLA DEL TERMINALE.** È il guasto 18:
    // fino al 01/09/2026 questa riga era `env: spec.env.clone()`, e un
    // motore lanciato da un passo di flusso ereditava l'ambiente di chi
    // aveva aperto il terminale — cioè leggeva la casa del vicino, mentre
    // `sailor run` lo stesso motore lo portava nella propria. Il profilo sta
    // **sotto** `spec.env`: chi scrive una variabile nel passo vince.
    let equipment = current_equipment_for(bin, &spec.env);
    Prepared {
        invocation: EngineInvocation {
            bin: bin.clone(),
            args,
            env: equipment.env,
            workdir: spec.workdir.clone(),
            stdin: stdin.map(String::into_bytes),
            timeout: Duration::from_secs(spec.timeout_secs),
        },
        session,
        identity: equipment.identity,
        named,
    }
}

impl ExternalEngineAction {
    /// Asks one engine: composes its line, says it on the step's echo, starts
    /// it and judges what came back. `set_aside` are the engines already put
    /// aside, and it reaches the error messages: whoever reads a red step must
    /// see the whole chain, not only its last link. The line comes back with
    /// the answer, and on every error raised after the engine was started.
    #[allow(clippy::too_many_arguments)]
    fn ask(
        &self,
        candidate: &Candidate,
        spec: &EngineSpec,
        shape: Option<&ValueSchema>,
        live: Option<&dyn LiveSink>,
        set_aside: &[String],
        solo: bool,
        record: Option<&Recording<'_>>,
        chain: &Chain,
    ) -> Result<(Asked, Ran), ActionError> {
        let prepared = compose(candidate, spec, live, set_aside, record);
        let ran = prepared.invocation.ran();
        if let Some(live) = live {
            live.chunk(Pipe::Stderr, format!("[sailor] {}\n", ran.announce()).as_bytes());
        }
        let started = self.start(
            candidate,
            spec,
            shape,
            live,
            set_aside,
            solo,
            record,
            chain,
            &prepared,
        );
        match started {
            Ok(asked) => Ok((asked, ran)),
            Err(error) => Err(error.having_run(ran)),
        }
    }

    /// Starts the composed line and judges what came back.
    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        candidate: &Candidate,
        spec: &EngineSpec,
        shape: Option<&ValueSchema>,
        live: Option<&dyn LiveSink>,
        set_aside: &[String],
        solo: bool,
        record: Option<&Recording<'_>>,
        chain: &Chain,
        prepared: &Prepared,
    ) -> Result<Asked, ActionError> {
        let Prepared {
            invocation,
            session,
            identity,
            named,
        } = prepared;
        let seconds = spec.timeout_secs;
        // Gli istanti si prendono stretti attorno alla chiamata: è la durata di
        // *questa* invocazione, non del passo che la contiene.
        let started_at = now_secs();
        let result = invoke_external_engine_watched_until(
            invocation,
            live,
            &candidate.waits_for_a_person_when,
        );
        let ended_at = now_secs();
        // Il consumo si legge da ciò che il motore ha detto, secondo quanto il
        // suo descrittore dichiara. Chi non dichiara niente lascia tutto
        // sconosciuto — e non è un ramo `if` per fornitore: è l'assenza di un
        // dato nel descrittore.
        let read = |stdout: &str, stderr: &str| match &candidate.declared_usage {
            Some(declared) => models::usage::read_declared(&declared.from.text(stdout, stderr), declared),
            None => Reading::default(),
        };
        // Ogni ramo passa di qui: anche il fallimento e anche il silenzio, che è
        // il punto — una chiamata interrotta ha comunque bruciato la quota.
        // `said` è l'uscita **grezza**, prima che l'involucro venga tolto: è lì
        // che un motore scrive di quale sessione ha parlato, nello stesso posto
        // in cui scrive i propri token.
        let note = |reading: Reading, error_type: Option<&'static str>, said: &str| {
            if let Some(record) = record {
                record_the_call(
                    record,
                    candidate,
                    chain,
                    Spent {
                        reading,
                        error_type,
                        started_at,
                        ended_at,
                        session_id: session.session_id(said),
                        identity: identity.clone(),
                        work_kind: spec.kind.clone(),
                    },
                );
            }
        };
        let outcome = match result {
            EngineResult::Ok { stdout, stderr } => {
                let reading = read(&stdout, &stderr);
                // **DIRE DI NON POTER LAVORARE E USCIRE ZERO SONO COMPATIBILI, E
                // FINO AL 01/09/2026 QUI NON SI GUARDAVA.** La domanda «questo
                // motore ha detto di non poter lavorare?» stava solo nel ramo
                // `ExitError`: di qua la risposta era presa per buona comunque,
                // il ripiego non scattava, e la riga del deposito nasceva con
                // `error_type: None` — cioè il passo si chiudeva **verde** su una
                // non-risposta, che è peggio del ripiego perso.
                //
                // Non è un caso di scuola: `CODEX_HOME=<cartella vuota> codex
                // exec < /dev/null` risponde «No prompt provided via stdin» ed
                // esce **zero** (guasto 39, misurato su questa macchina). Con un
                // `answer_shape` dichiarato il passo moriva poi su un errore di
                // forma, cioè sul sintomo sbagliato tre gradini più in là.
                //
                // **LA SONDA A SECCO LA DISTINZIONE CE L'AVEVA GIÀ**:
                // `judge_dry_run` interroga `unusable_when` su `Ok` *e* su
                // `ExitError`. Il controllo statico e la corsa vera divergevano
                // sullo stesso motore, ed è la forma del guasto 39 su un altro
                // campo. Qui convergono: **una sola domanda, gli stessi due
                // rami**.
                //
                // **LA RIGA SI SCRIVE PRIMA DELLA TOLLERANZA, COME NELL'ALTRO
                // RAMO.** Sono due domande diverse e vanno tenute separate: la
                // specie dice **cos'è successo**, `accept` dice **cosa ne fa la
                // corsa**. Nel primo tentativo di chiudere questo guasto il
                // `note(...)` stava dentro il ramo della non-tolleranza, e un
                // passo con `accept: ["exit_error"]` tornava a scrivere una riga
                // `NULL` su un motore che aveva appena detto di non poter
                // lavorare — cioè il difetto sopravviveva dentro il proprio
                // rimedio, in un angolo. L'ha trovato un giudice che non aveva
                // scritto il lavoro.
                // **AN ANSWER IN THE DECLARED SHAPE IS WORK DONE**, and the
                // words that mean a refusal are not looked for inside it: an
                // engine reading a tree whose own documents discuss quotas
                // prints those words while working, and would refuse itself.
                let answered = reading.answer.clone().unwrap_or_else(|| stdout.clone());
                let in_shape = shape.is_some_and(|shape| shaped_answer(shape, &answered).is_ok());
                let class = if in_shape {
                    None
                } else {
                    candidate.declared_class(&stdout, &stderr)
                };
                let cannot_work = class.is_some();
                // The class is the same as the other branch's: a spent quota
                // or a door shut for another reason, never the exit code.
                note(reading.clone(), class, &stdout);
                self.set_aside_if_spent(candidate, class, ended_at, &stdout, &stderr);
                // La tolleranza viene dopo, per la stessa ragione dell'altro
                // ramo: un passo che con `accept` dichiara di volersi tenere il
                // fallimento di questo motore lo vuole come dato, e non vuole che
                // qualcun altro ci riprovi al posto suo.
                if cannot_work && !tolerates(&spec.accept, "exit_error") {
                    return engine_cannot_work(named, solo, &stdout, &stderr);
                }
                // **L'USCITA DEL PASSO NON CAMBIA PERCHÉ SI È MISURATO.** Se il
                // descrittore ha chiesto un involucro per farsi dire i token,
                // qui la risposta si tira fuori dall'involucro e `stdout` torna
                // a essere quello di prima. Misurare non deve cambiare ciò che
                // si misura: un flusso a valle che dichiara la forma della
                // propria risposta diventerebbe rosso per una misura che non ha
                // chiesto.
                let stdout = reading.answer.unwrap_or(stdout);
                match shape {
                    Some(shape) => {
                        return shaped_answer(shape, &stdout).map(|answer| {
                            Asked::Answered(ActionOutcome::Went(
                                json!({"status": "ok", "answer": answer}),
                            ))
                        })
                    }
                    None => EngineOutcomeJson {
                        status: "ok",
                        stdout,
                        stderr,
                    },
                }
            }
            EngineResult::ExitError {
                code,
                stdout,
                stderr,
            } => {
                // Il consumo si legge dall'uscita GREZZA, prima di qualunque
                // altra cosa: un motore uscito in errore può aver già speso, e
                // i suoi token vanno letti dove li ha scritti.
                let reading = read(&stdout, &stderr);
                // **ESAURITO NON È ROTTO, E SI GUARDA PRIMA DI SCRIVERE LA
                // RIGA.** Fino al 31/08/2026 questa distinzione stava dieci
                // righe più in basso e valeva solo quando c'era una catena
                // (`!solo && ...`): un passo con un motore solo che aveva finito
                // la quota veniva registrato come `exit_error`, indistinguibile
                // da un motore che si rompe. È il guasto 14, e il 29/08 è
                // costato una serata — Claude era al limite settimanale, la
                // corsa si è fermata come se fosse rotto, e `agy` era vivo.
                //
                // La specie della riga nel deposito cambia di conseguenza:
                // `exhausted` è una cosa che passa da sé alle sette del mattino,
                // `exit_error` no, e una somma che le mescola non dice niente a
                // nessuno.
                let class = candidate.declared_class(&stdout, &stderr);
                let exhausted = class.is_some();
                note(reading.clone(), Some(class.unwrap_or("exit_error")), &stdout);
                self.set_aside_if_spent(candidate, class, ended_at, &stdout, &stderr);
                if !tolerates(&spec.accept, "exit_error") {
                    // La tolleranza viene prima: un passo che si aspetta un
                    // fallimento lo vuole come dato, non vuole che qualcun altro
                    // ci riprovi al posto suo.
                    if exhausted {
                        // La conseguenza è **la stessa** dell'uscita zero, e sta
                        // in un posto solo: due copie di questo blocco erano già
                        // divergenti appena nate.
                        return engine_cannot_work(named, solo, &stdout, &stderr);
                    }
                    let before = if set_aside.is_empty() {
                        String::new()
                    } else {
                        format!(" (before: {})", each_one_why(set_aside))
                    };
                    return Err(ActionError::new(
                        "engine_exit_error",
                        format!(
                            "{named} {}; {}{before}",
                            how_it_exited(code),
                            what_it_said(&stdout, &stderr)
                        ),
                    ));
                }
                // Come nel ramo riuscito: l'involucro si toglie, l'uscita del
                // passo resta quella di prima.
                let stdout = reading.answer.unwrap_or(stdout);
                match shape {
                    // Un motore che ha parlato deve rispettare la forma anche
                    // quando il passo gli perdona l'uscita in errore: quella
                    // tolleranza riguarda il codice di uscita, non la risposta.
                    Some(shape) => {
                        return shaped_answer(shape, &stdout).map(|answer| {
                            Asked::Answered(ActionOutcome::Went(
                                json!({"status": "exit_error", "answer": answer}),
                            ))
                        })
                    }
                    None => EngineOutcomeJson {
                        status: "exit_error",
                        stdout,
                        stderr,
                    },
                }
            }
            EngineResult::WaitingForAPerson { stdout, stderr } => {
                // Stopped on the words its descriptor declares mean it only
                // waits for a person: a refusal by declaration, so the class is
                // the declared one and never a plain exit error, and the wait
                // it would have cost is the whole reason it was stopped.
                let reading = read(&stdout, &stderr);
                let class = candidate.declared_class(&stdout, &stderr).unwrap_or("exhausted");
                note(reading.clone(), Some(class), &stdout);
                self.set_aside_if_spent(candidate, Some(class), ended_at, &stdout, &stderr);
                if !tolerates(&spec.accept, "exit_error") {
                    return engine_cannot_work(named, solo, &stdout, &stderr);
                }
                let stdout = reading.answer.unwrap_or(stdout);
                match shape {
                    Some(shape) => {
                        return shaped_answer(shape, &stdout).map(|answer| {
                            Asked::Answered(ActionOutcome::Went(
                                json!({"status": "exit_error", "answer": answer}),
                            ))
                        })
                    }
                    None => EngineOutcomeJson {
                        status: "exit_error",
                        stdout,
                        stderr,
                    },
                }
            }
            EngineResult::TimedOut => {
                // Ucciso a metà: non ha detto niente, quindi non c'è niente da
                // leggere. La riga si scrive lo stesso, coi token sconosciuti —
                // il tempo che ha girato l'ha speso davvero.
                // Ucciso a metà non ha detto niente: nessun identificativo da
                // leggere, e la sessione resta ignota anche se ne aveva aperta
                // una. Riprendere quella di un passo interrotto vorrebbe dire
                // ripartire da un contesto tagliato in un punto qualunque.
                note(Reading::default(), Some("timed_out"), "");
                // Nessun ripiego su un tetto di tempo: un motore ucciso a metà
                // può aver già fatto qualcosa, e rifare quel lavoro altrove
                // sarebbe farlo due volte senza saperlo.
                if !tolerates(&spec.accept, "timed_out") {
                    return Err(ActionError::new(
                        "engine_timed_out",
                        format!("{named} did not answer within {seconds} seconds and was killed"),
                    ));
                }
                EngineOutcomeJson {
                    status: "timed_out",
                    stdout: String::new(),
                    stderr: String::new(),
                }
            }
            EngineResult::SpawnFailed { reason } => {
                // Non si è nemmeno avviato: non ha consumato niente, ma la
                // riga dice che ci si è provati — e senza di lei una catena che
                // ripiega su un secondo motore sembrerebbe averlo scelto per
                // prima, invece che per ripiego.
                note(Reading::default(), Some("spawn_failed"), "");
                if !tolerates(&spec.accept, "spawn_failed") {
                    // Non essersi avviato è il caso più netto di «non poteva
                    // lavorare»: non ha fatto niente, e non serve che il suo
                    // descrittore lo dichiari.
                    if !solo {
                        return Ok(Asked::CannotWork(format!(
                            "{named} could not be started: {reason}"
                        )));
                    }
                    return Err(ActionError::new(
                        "engine_spawn_failed",
                        format!("{named} could not be started: {reason}"),
                    ));
                }
                EngineOutcomeJson {
                    status: "spawn_failed",
                    stdout: String::new(),
                    stderr: reason,
                }
            }
        };
        Ok(Asked::Answered(ActionOutcome::Went(json!(outcome))))
    }

    /// An engine that said its quota is spent is set aside for the time its
    /// descriptor declares; without a declared time, or without a home for
    /// the list, it is tried again next time, as before.
    fn set_aside_if_spent(
        &self,
        candidate: &Candidate,
        class: Option<&'static str>,
        now: i64,
        stdout: &str,
        stderr: &str,
    ) {
        let (Some("quota_exhausted"), Some(secs), Some(id), Some(path)) =
            (class, candidate.cooldown_secs, candidate.id.as_deref(), self.cooldowns.as_deref())
        else {
            return;
        };
        // A list that cannot be written costs the next chain one knock: not
        // worth breaking this step over.
        let _ = cooldown::set_aside(path, id, now, secs, &what_it_said(stdout, stderr));
    }
}

impl Action for ExternalEngineAction {
    /// I campi che questa azione non conosce, letti dalla **struttura vera**.
    ///
    /// Non è un elenco scritto a mano accanto a `EngineSpec`: sarebbe una
    /// seconda copia della stessa verità, e le seconde copie divergono. Qui si
    /// tenta la deserializzazione e si guarda cosa è finito in `extra` — quindi
    /// aggiungere un campo alla specifica basta, non c'è nient'altro da
    /// aggiornare.
    ///
    /// Un ingresso che non si deserializza affatto non produce niente: a
    /// controllo il `with` è **parziale** per costruzione — il resto arriva
    /// dalle dipendenze — e lamentarsi di un `timeout_secs` mancante direbbe
    /// una cosa falsa.
    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match serde_json::from_value::<EngineSpec>(declared.clone()) {
            Ok(spec) => spec.extra.into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }

    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.execute_and_report(input, shared)
            .map(|(outcome, _)| outcome)
    }

    /// The line of the engine that answered travels with the answer; with a
    /// chain, the line of the last engine tried travels with the error.
    fn execute_and_report(
        &self,
        input: &Value,
        shared: &SharedState,
    ) -> Result<(ActionOutcome, Option<Ran>), ActionError> {
        let live = sink_for_step(&self.watcher, shared);
        // Dove annotare la spesa. Si costruisce qui perché `shared` più avanti
        // non c'è più, ed è `None` — cioè non si annota niente — se manca il
        // deposito o uno dei due identificativi.
        let record = recording_for(&self.ledger, shared);
        // La forma si tiene anche com'era scritta: è quel testo, non una sua
        // riscrittura, che deve comparire nel prompt.
        let written_shape = input.get("answer_shape").map(|shape| {
            serde_json::to_string(shape).expect("a value already in memory always reserialises")
        });
        let mut spec: EngineSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        check_tolerance(&spec.accept, &ENGINE_FAILURES)?;
        // Held until this step returns, and taken down then: the binding is
        // what closes the tree, so it outlives every path out of here.
        let own_tree = tree_of_its_own(&spec, shared, live.clone())?;
        if let Some(cut) = &own_tree {
            if let Some(live) = live.as_deref() {
                live.chunk(
                    Pipe::Stderr,
                    format!("[sailor] this step works in {}\n", cut.at.display()).as_bytes(),
                );
            }
            spec.workdir = Some(cut.at.to_string_lossy().into_owned());
        }
        if let Some(written) = &written_shape {
            shape_was_asked_for(written, &spec)?;
        }
        // Prima di spendere qualunque cosa: se nessuno dei motori chiesti è
        // usabile qui, il passo si ferma e dice di ognuno perché.
        let (candidates, refused) = self.candidates(&spec)?;
        if candidates.is_empty() {
            // Un motore solo che non si trova resta `tool_unavailable` col
            // motivo del risolutore: è il caso più comune, e quel messaggio è
            // già il migliore che si possa dare.
            if let [only] = refused.as_slice() {
                if only.unresolved {
                    return Err(ActionError::new("tool_unavailable", only.reason.clone()));
                }
            }
            return Err(ActionError::new(
                "no_usable_engine",
                format!(
                    "none of the engines the step asks for can be used here. {}",
                    each_one_why(&refused.iter().map(Refused::line).collect::<Vec<_>>())
                ),
            ));
        }
        let mut set_aside: Vec<String> = refused.iter().map(Refused::line).collect();
        let shape = spec.answer_shape.as_ref();
        // Un passo che chiede **un** motore solo non ha nessun ripiego da fare,
        // e deve restare identico a com'era: gli stessi esiti, gli stessi
        // messaggi. La catena cambia il comportamento solo dove c'è una catena.
        let solo = candidates.len() == 1 && refused.is_empty();
        // Gli identificativi dei motori già provati, per la catena di ripiego
        // scritta nella riga: `set_aside` porta frasi per una persona, questo
        // porta nomi che una somma può raggruppare.
        let mut chain = Chain {
            tried_before: Vec::new(),
            fell_back_from: self.fell_back_from(&spec, &candidates),
        };
        let mut last_ran = None;
        for candidate in &candidates {
            let (asked, ran) = self.ask(
                candidate,
                &spec,
                shape,
                live.as_deref(),
                &set_aside,
                solo,
                record.as_ref(),
                &chain,
            )?;
            match asked {
                Asked::Answered(outcome) => return Ok((outcome, Some(ran))),
                Asked::CannotWork(why) => {
                    last_ran = Some(ran);
                    set_aside.push(why);
                    if let Some(id) = &candidate.id {
                        chain.tried_before.push(id.clone());
                    }
                }
            }
        }
        let none_could = ActionError::new(
            "no_usable_engine",
            format!(
                "none of the engines the step asks for could work. {}",
                each_one_why(&set_aside)
            ),
        );
        Err(match last_ran {
            Some(ran) => none_could.having_run(ran),
            None => none_could,
        })
    }

    /// Non dichiara di potersi rifare, e quindi finisce a una persona.
    ///
    /// Vale anche con una catena: un motore che ha dichiarato di non poter
    /// lavorare non ha fatto niente, ma quello che ha risposto sì.
    /// È la scelta giusta per il caso generale: dietro `bin` e `args` può
    /// esserci qualunque cosa — un motore che ha già riscritto mezzo albero,
    /// una richiesta di rete già partita — e da fuori non si distingue da un
    /// comando che non ha fatto niente. Chi sa che il proprio motore è
    /// idempotente lo dichiara nella propria azione, come fa il servizio
    /// notturno: la specie appartiene a chi conosce il lavoro, non alla
    /// primitiva che lo lancia.
    fn species(&self) -> StepSpecies {
        StepSpecies::HandToHuman
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::with_references_resolved;

    /// DI QUALE PASSO È IL TESTO: l'azione chiede il destinatario alla fabbrica
    /// nominando il passo che lo `SharedState` le porta, e quello che consegna è
    /// ciò che il motore ha detto.
    #[test]
    fn the_action_asks_the_factory_for_the_step_it_is_running() {
        #[derive(Default)]
        struct Factory {
            asked: std::sync::Mutex<Vec<String>>,
            said: std::sync::Mutex<Vec<u8>>,
        }

        struct Branch(Arc<Factory>);

        impl LiveSink for Branch {
            fn chunk(&self, _pipe: Pipe, bytes: &[u8]) {
                self.0
                    .said
                    .lock()
                    .expect("nessuno panica qui")
                    .extend_from_slice(bytes);
            }
        }

        /// La fabbrica sta dietro un `Arc` perché la prova deve poter leggere
        /// ciò che ha registrato dopo che l'azione ha finito con lei.
        struct FactoryArc(Arc<Factory>);

        impl StepSinks for FactoryArc {
            fn sink_for(&self, step: &str) -> Arc<dyn LiveSink> {
                self.0
                    .asked
                    .lock()
                    .expect("nessuno panica qui")
                    .push(step.to_owned());
                Arc::new(Branch(self.0.clone()))
            }
        }

        let factory = Arc::new(Factory::default());
        let action = ExternalEngineAction::new().watched_by(Some(Arc::new(FactoryArc(
            factory.clone(),
        )) as Arc<dyn StepSinks>));
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_STEP.to_owned(), json!("il-passo-che-parla"));
        let outcome = action
            .execute(
                &json!({"bin": "sh", "args": ["-c", "echo detto-dal-motore"], "timeout_secs": 10}),
                &shared,
            )
            .expect("il passo doveva riuscire");
        assert!(matches!(outcome, ActionOutcome::Went(_)));
        assert_eq!(
            *factory.asked.lock().expect("nessuno panica qui"),
            vec!["il-passo-che-parla".to_owned()]
        );
        let said =
            String::from_utf8_lossy(&factory.said.lock().expect("nessuno panica qui")).into_owned();
        assert!(said.contains("detto-dal-motore"), "consegnato: {said:?}");
    }

    /// Senza la chiave del passo nello stato condiviso non si consegna niente:
    /// un testo che nessuno sa attribuire è peggio del silenzio.
    #[test]
    fn no_step_id_means_nobody_is_asked() {
        struct Never;

        impl StepSinks for Never {
            fn sink_for(&self, _step: &str) -> Arc<dyn LiveSink> {
                panic!("non doveva essere chiesto nessun destinatario");
            }
        }

        let action =
            ExternalEngineAction::new().watched_by(Some(Arc::new(Never) as Arc<dyn StepSinks>));
        let shared = SharedState::new();
        action
            .execute(
                &json!({"bin": "sh", "args": ["-c", "echo muto"], "timeout_secs": 10}),
                &shared,
            )
            .expect("il passo doveva riuscire lo stesso");
    }

    /// **UN PASSO CON UNA DIPENDENZA CONTINUA A GIRARE, E QUI STA IL RISCHIO
    /// DEL CONTROLLO NUOVO.**
    ///
    /// L'ingresso vero di un passo è l'uscita del passo prima, col `with`
    /// sovrapposto: `status`, `stdout`, `stderr` e qualunque altra cosa quel
    /// passo abbia prodotto arrivano qui dentro. Se `EngineSpec` li rifiutasse —
    /// la strada ovvia, `deny_unknown_fields` — nessun passo con una dipendenza
    /// partirebbe più, e il rimedio al guasto 20 sarebbe molto peggio del
    /// guasto. Questa prova tiene quella porta chiusa.
    #[test]
    fn an_input_carrying_the_previous_step_output_still_runs() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "bin": "echo",
            "args": ["fatto"],
            "timeout_secs": 10,
            // Quello che arriva dal passo prima, e che questa azione non
            // conosce né deve conoscere.
            "status": "ok",
            "stdout": "l'uscita di chi mi precede\n",
            "stderr": "",
        });

        let outcome = action
            .execute(&input, &SharedState::new())
            .expect("un ingresso con l'uscita della dipendenza deve girare");

        let ActionOutcome::Went(output) = outcome else {
            panic!("doveva riuscire")
        };
        assert_eq!(output["stdout"], "fatto\n", "e fa il proprio lavoro");
    }

    /// La gemella del controllo statico, provata **qui** dove vive la verità:
    /// gli stessi campi che a esecuzione si ignorano, a controllo si nominano.
    #[test]
    fn the_action_can_name_the_fields_it_does_not_know() {
        let action = ExternalEngineAction::new();

        let stray = action.unknown_fields(&json!({
            "tool": "claude-code",
            "prompt": "ciao",
            "timeout_secs": 10,
        }));

        assert_eq!(stray, vec!["prompt".to_owned()]);
        assert!(
            action
                .unknown_fields(
                    &json!({"tool": "claude-code", "stdin": "ciao", "timeout_secs": 10})
                )
                .is_empty(),
            "e su un ingresso scritto bene non nomina niente"
        );
    }

    // ── le azioni registrabili ─────────────────────────────────────────

    #[test]
    fn the_external_engine_action_reads_its_json_input() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "bin": "sh",
            "args": ["-c", "echo 'answer: 42'"],
            "timeout_secs": 5
        });
        let shared = SharedState::new();
        let outcome = action
            .execute(&input, &shared)
            .expect("l'azione non fallisce");
        let ActionOutcome::Went(output) = outcome else {
            panic!("un'azione motore riuscita è sempre Went")
        };
        assert_eq!(output["status"], "ok");
        assert!(output["stdout"].as_str().unwrap().contains("answer: 42"));
    }

    /// **LA MISURA CHE POTEVA VENIRE DIVERSA, ED È QUELLA CHE PRIMA VENIVA
    /// DIVERSA.** Fino al 28/08/2026 questo stesso ingresso chiudeva il passo
    /// `Went` con dentro `status: exit_error`. Il mutante che rifà cadere la
    /// prova è togliere il `return Err` dal ramo `ExitError` dell'azione.
    ///
    /// Il messaggio non è un dettaglio: un passo rotto non scrive nessuna
    /// uscita tipata, quindi il codice e ciò che il motore ha detto o stanno
    /// qui o sono persi.
    #[test]
    fn an_engine_that_exits_nonzero_breaks_its_own_step() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "bin": "sh",
            "args": ["-c", "echo dettaglio-che-serve 1>&2; exit 3"],
            "timeout_secs": 5
        });

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("un motore uscito in errore rompe il passo");

        assert_eq!(error.class, "engine_exit_error");
        assert!(error.said.contains("code 3"), "{}", error.said);
        assert!(error.said.contains("dettaglio-che-serve"), "{}", error.said);
    }

    /// L'altra metà, senza la quale la prima non proverebbe una scelta ma una
    /// mancanza: chi esegue un comando **apposta** per vederlo fallire lo
    /// dichiara, e riprende il vecchio comportamento per quell'esito solo.
    #[test]
    fn a_step_can_declare_that_a_nonzero_exit_is_an_acceptable_outcome() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "bin": "sh",
            "args": ["-c", "exit 3"],
            "accept": ["exit_error"],
            "timeout_secs": 5
        });

        let ActionOutcome::Went(output) = action
            .execute(&input, &SharedState::new())
            .expect("l'esito è dichiarato accettabile")
        else {
            panic!("un esito tollerato resta un dato")
        };

        assert_eq!(output["status"], "exit_error");
    }

    /// La tolleranza dichiarata su un esito che non esiste sarebbe un rigore
    /// che nessuno ha scelto: si scopre subito, non il giorno in cui serviva.
    #[test]
    fn a_tolerance_for_an_impossible_outcome_is_refused() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "bin": "sh",
            "args": ["-c", "exit 3"],
            "accept": ["failed"],
            "timeout_secs": 5
        });

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("«failed» non è un esito di un motore");

        assert_eq!(error.class, "invalid_input");
        assert!(error.said.contains("exit_error"), "{}", error.said);
    }

    /// Un binario che non c'è è il caso più comune di flusso scritto altrove, e
    /// deve dire **quale** binario: il messaggio è tutta la riparazione.
    #[test]
    fn a_binary_that_will_not_start_breaks_the_step_and_names_itself() {
        let action = ExternalEngineAction::new();
        let input = json!({"bin": "/nessun/binario/qui-di-sicuro", "timeout_secs": 5});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("un motore che non parte rompe il passo");

        assert_eq!(error.class, "engine_spawn_failed");
        assert!(
            error.said.contains("/nessun/binario/qui-di-sicuro"),
            "{}",
            error.said
        );
    }

    #[test]
    fn an_engine_that_never_returns_breaks_the_step_with_its_limit() {
        let action = ExternalEngineAction::new();
        let input = json!({"bin": "sh", "args": ["-c", "exec sleep 60"], "timeout_secs": 1});

        let error = action
            .execute(&input, &SharedState::new())
            .expect_err("il tempo scaduto rompe il passo");

        assert_eq!(error.class, "engine_timed_out");
        assert!(error.said.contains("within 1 seconds"), "{}", error.said);
    }

    // ── la forma dichiarata della risposta ────────────────────────────

    /// Il passo dichiara la forma **una volta**, e da quel campo escono tutte e
    /// due le cose: il testo che va nel prompt e il metro su cui si misura la
    /// risposta. Qui il motore finto risponde bene ma prolisso.
    ///
    /// Due cose insieme, e la seconda è quella che si paga a ogni chiamata a
    /// valle: la risposta è accettata, e al passo dopo arriva **solo** ciò che
    /// la forma dichiara — niente preamboli, niente campi in più, niente testo
    /// grezzo.
    #[test]
    fn a_declared_shape_is_enforced_and_only_what_it_declares_is_handed_on() {
        let said = r#"{"paths": ["src/a.rs", "src/b.rs"], "total": 2, "ragionamento": "ho guardato ovunque, e poi ancora"}"#;
        let input = json!({
            "bin": "sh",
            "args": ["-c", format!("printf '%s' '{said}'")],
            "answer_shape": {
                "type": "object",
                "properties": {
                    "paths": {"type": "array", "items": {"type": "string"}},
                    "total": {"type": "number"}
                },
                "required": ["paths", "total"],
                "allow_extra": true
            },
            "stdin": {"$join": ["Rispondi in questa forma: ", {"$json": "/answer_shape"}]},
            "timeout_secs": 5
        });

        let ActionOutcome::Went(output) = ExternalEngineAction::new()
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect("la risposta rispetta la forma")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(output["status"], "ok");
        assert_eq!(output["answer"]["total"], 2);
        assert_eq!(output["answer"]["paths"][1], "src/b.rs");
        assert!(
            output["answer"].get("ragionamento").is_none(),
            "il ragionamento del motore non deve viaggiare fino al passo dopo: {output}"
        );
        assert!(
            output.get("stdout").is_none() && output.get("stderr").is_none(),
            "col testo grezzo accanto alla risposta il risparmio non esisterebbe: {output}"
        );
    }

    /// **LA MISURA CHE POTEVA VENIRE DIVERSA**: la stessa forma, un motore che
    /// risponde con un campo del tipo sbagliato. Il mutante che la fa cadere è
    /// togliere la validazione dopo la lettura — e allora un `total` scritto a
    /// parole arriverebbe intatto al passo dopo.
    #[test]
    fn an_answer_that_does_not_fit_the_shape_breaks_the_step() {
        let said = r#"{"paths": [], "total": "parecchi"}"#;
        let input = json!({
            "bin": "sh",
            "args": ["-c", format!("printf '%s' '{said}'")],
            "answer_shape": {
                "type": "object",
                "properties": {
                    "paths": {"type": "array", "items": {"type": "string"}},
                    "total": {"type": "number"}
                },
                "required": ["paths", "total"],
                "allow_extra": false
            },
            "stdin": {"$json": "/answer_shape"},
            "timeout_secs": 5
        });

        let error = ExternalEngineAction::new()
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect_err("un motore fuori forma non ha risposto");

        assert_eq!(error.class, "answer_off_shape");
        assert!(error.said.contains("parecchi"), "{}", error.said);
    }

    #[test]
    fn an_answer_that_is_not_json_at_all_breaks_the_step() {
        let input = json!({
            "bin": "sh",
            "args": ["-c", "printf 'certo, ci penso io'"],
            "answer_shape": {"type": "object", "properties": {}, "required": [], "allow_extra": true},
            "stdin": {"$json": "/answer_shape"},
            "timeout_secs": 5
        });

        let error = ExternalEngineAction::new()
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect_err("non è JSON");

        assert_eq!(error.class, "answer_not_json");
        assert!(error.said.contains("ci penso io"), "{}", error.said);
    }

    /// I modelli incorniciano: si accetta il primo blocco recintato, anche
    /// preceduto da una riga di cortesia. Senza questa regola la forma sarebbe
    /// rispettata e il passo rosso lo stesso.
    #[test]
    fn an_answer_inside_a_fence_is_read_anyway() {
        let input = json!({
            "bin": "sh",
            // Stringa grezza: le sequenze `\n` le interpreta `printf`, non Rust.
            "args": ["-c", r#"printf 'Ecco:\n```json\n{"total": 7}\n```\n'"#],
            "answer_shape": {
                "type": "object",
                "properties": {"total": {"type": "number"}},
                "required": ["total"],
                "allow_extra": false
            },
            "stdin": {"$json": "/answer_shape"},
            "timeout_secs": 5
        });

        let ActionOutcome::Went(output) = ExternalEngineAction::new()
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect("il blocco recintato si legge")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(output["answer"]["total"], 7);
    }

    /// **CHIEDERE E VERIFICARE SONO UNA COSA SOLA.** Qui la forma è dichiarata
    /// ma non compare nel prompt: il passo si ferma prima di spendere, e lo si
    /// vede dal fatto che il binario inesistente non arriva mai a lamentarsi.
    #[test]
    fn a_shape_that_never_reaches_the_prompt_stops_the_step_before_spending() {
        let input = json!({
            "bin": "/nessun/binario/qui-di-sicuro",
            "answer_shape": {"type": "object", "properties": {}, "required": [], "allow_extra": true},
            "stdin": "elenca i percorsi",
            "timeout_secs": 5
        });

        let error = ExternalEngineAction::new()
            .execute(&input, &SharedState::new())
            .expect_err("la forma non è stata chiesta a nessuno");

        assert_eq!(
            error.class, "shape_not_in_prompt",
            "se fosse partito, il binario assente avrebbe dato un altro errore: {}",
            error.said
        );
        assert!(error.said.contains("$json"), "{}", error.said);
    }

    /// La forma vale su ciò che il motore ha detto: un passo che tollera di non
    /// sentire niente non può pretendere una forma da quel niente.
    #[test]
    fn a_shape_cannot_live_with_a_tolerance_that_leaves_no_answer() {
        let input = json!({
            "bin": "sh",
            "args": ["-c", "true"],
            "accept": ["timed_out"],
            "answer_shape": {"type": "object", "properties": {}, "required": [], "allow_extra": true},
            "stdin": {"$json": "/answer_shape"},
            "timeout_secs": 5
        });

        let error = ExternalEngineAction::new()
            .execute(&with_references_resolved(input), &SharedState::new())
            .expect_err("le due dichiarazioni non stanno insieme");

        assert_eq!(error.class, "invalid_input");
        assert!(error.said.contains("timed_out"), "{}", error.said);
    }

    /// **IL PASSAGGIO DI CONSEGNE, PROVATO SULL'AZIONE E NON SOLO SUL MODULO.**
    /// L'ingresso è quello che il motore compone davvero per un passo con una
    /// dipendenza: l'uscita del passo prima (`status`, `stdout`, `stderr`) più
    /// i valori fissi del campo `with`, coi rinvii già sciolti come li scioglie
    /// `flow::step_input`. Qui si prova che l'azione **usa** ciò che arriva; che
    /// ad arrivare sciolto sia l'ingresso di *ogni* azione lo prova
    /// l'esecutore, dov'è l'unico posto che lo fa.
    #[test]
    fn a_step_sends_the_previous_engines_answer_into_the_next_one() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "status": "ok",
            "stdout": "=== PER CODEX ===\nconta i ganci morti",
            "stderr": "",
            "bin": "cat",
            "args": [],
            "stdin": {"$join": ["Esegui solo la tua sezione.\n", {"$from": "/stdout"}]},
            "timeout_secs": 5
        });
        let shared = SharedState::new();

        let ActionOutcome::Went(output) = action
            .execute(&with_references_resolved(input), &shared)
            .unwrap()
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(output["status"], "ok");
        assert_eq!(
            output["stdout"], "Esegui solo la tua sezione.\n=== PER CODEX ===\nconta i ganci morti",
            "il motore ha ricevuto sull'ingresso ciò che il passo prima ha scritto"
        );
    }

    // **UN PUNTATORE CHE NON TROVA NIENTE FERMA IL PASSO, E NON PIÙ QUI.** La
    // prova stava in questo modulo perché la risoluzione stava in questa
    // azione. Dal 01/09/2026 sta in `flow::step_input`, quindi il passo si
    // ferma **prima che l'azione esista**: si prova dove accade, in
    // `crates/flow/tests/a_reference_reaches_every_action.rs`. Tenerla anche qui
    // vorrebbe dire due prove della stessa regola in due punti — e quella qui
    // sarebbe verde chiamando la risoluzione a mano, cioè misurando la prova.

    /// The record of an engine step carries the binary and the arguments as
    /// started — with the answer, and with the error when the engine broke.
    #[test]
    fn an_engine_step_reports_the_binary_and_the_arguments_it_started() {
        let answered = json!({"bin": "sh", "args": ["-c", "echo ok"], "timeout_secs": 5});
        let (outcome, ran) = ExternalEngineAction::new()
            .execute_and_report(&with_references_resolved(answered), &SharedState::new())
            .expect("the engine answers");
        assert!(matches!(outcome, ActionOutcome::Went(_)));
        assert_eq!(ran, Some(Ran::new("sh", ["-c", "echo ok"])));

        let broke = json!({"bin": "sh", "args": ["-c", "exit 3"], "timeout_secs": 5});
        let error = ExternalEngineAction::new()
            .execute_and_report(&with_references_resolved(broke), &SharedState::new())
            .expect_err("an engine that exits red breaks its step");
        assert_eq!(error.class, "engine_exit_error");
        assert_eq!(
            error.ran.as_deref(),
            Some(&Ran::new("sh", ["-c", "exit 3"])),
            "a broken engine step forgot the line it ran"
        );
    }

    /// Whoever watches the step reads the engine's line before the engine has
    /// spoken, on the step's own echo.
    #[test]
    fn the_engine_step_says_what_it_is_about_to_run_before_running_it() {
        struct Recorder(std::sync::Mutex<Vec<(Pipe, Vec<u8>)>>);

        impl LiveSink for Recorder {
            fn chunk(&self, pipe: Pipe, bytes: &[u8]) {
                self.0
                    .lock()
                    .expect("nobody panics here")
                    .push((pipe, bytes.to_vec()));
            }
        }

        struct OneSink(Arc<Recorder>);

        impl StepSinks for OneSink {
            fn sink_for(&self, _step: &str) -> Arc<dyn LiveSink> {
                self.0.clone()
            }
        }

        let recorder = Arc::new(Recorder(std::sync::Mutex::new(Vec::new())));
        let action = ExternalEngineAction::new()
            .watched_by(Some(Arc::new(OneSink(recorder.clone()))));
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_STEP.to_owned(), json!("ask"));
        action
            .execute(
                &json!({"bin": "sh", "args": ["-c", "echo ok"], "timeout_secs": 5}),
                &shared,
            )
            .expect("the engine answers");

        let seen = recorder.0.lock().expect("nobody panics here").clone();
        let expected = format!("[sailor] {}\n", Ran::new("sh", ["-c", "echo ok"]).announce());
        let announced = seen
            .iter()
            .position(|(pipe, bytes)| *pipe == Pipe::Stderr && bytes == expected.as_bytes());
        let answered = seen
            .iter()
            .position(|(pipe, bytes)| *pipe == Pipe::Stdout && bytes == b"ok\n");
        assert!(announced.is_some(), "the line was never said: {seen:?}");
        assert!(answered.is_some(), "the engine's own text did not arrive: {seen:?}");
        assert!(
            announced < answered,
            "the line came after the engine had spoken: {seen:?}"
        );
    }
}
