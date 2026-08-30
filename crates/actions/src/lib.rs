//! Azioni riusabili da qualunque `flow::Graph`: invocare un motore esterno,
//! eseguire una verifica con un tempo massimo, e la primitiva che impone il
//! limite di durata a entrambe. Agnostiche a qualunque coda, servizio o
//! percorso: chi le usa passa binario, argomenti, ambiente e percorsi
//! nell'ingresso tipato del passo — niente è cablato qui dentro.
//!
//! CONSOLIDA (non duplica) la logica che prima viveva solo in
//! `notte::main::run_with_timeout`: quel file la richiama da qui adesso.
//!
//! Le due azioni registrabili (`ExternalEngineAction`, `ShellCheckAction`)
//! parlano JSON con `flow::ActionRegistry`. Chi compone un passo unico da più
//! azioni (motore poi verifica, come fa `notte`) può anche chiamare
//! direttamente `invoke_external_engine`/`run_shell_check`: sono funzioni
//! semplici, non solo azioni registrate.
//!
//! Tutte e due leggono il proprio ingresso **dopo** che i rinvii sono stati
//! risolti (`reference`): è così che il lavoro deciso da un passo arriva al
//! passo dopo senza uscire dal grafo.
//!
//! **UN PASSO CHE FALLISCE È ROSSO, E NON PER GENTILEZZA DI CHI VIENE DOPO.**
//! Fino al 28/08/2026 un motore uscito in errore lasciava il passo `Went` con
//! dentro un campo `status: exit_error`: la corsa diventava rossa solo se
//! qualcuno, più avanti nel grafo, guardava quel campo. Adesso un esito di
//! fallimento rompe il proprio passo, e i passi che ne dipendono non partono.
//! Chi vuole il contrario lo dichiara nel passo, esito per esito, col campo
//! `accept` — c'è chi esegue un comando apposta per vedere se fallisce. La
//! tolleranza è una decisione scritta; il rigore è il valore predefinito.
//!
//! **UN PASSO NOMINA LO STRUMENTO, NON IL BINARIO.** `bin` resta per un comando
//! qualunque, ma un motore si chiede per identificativo (`tool`) e chi compone
//! il registro delle azioni decide come si risolve — su questa macchina lo fa
//! `toolbox` leggendo i suoi descrittori. Un flusso che scrive `"bin": "claude"`
//! gira solo dove quel nome è nel percorso di chi esegue; uno che scrive
//! `"tool": "claude-code"` gira ovunque quel descrittore trovi qualcosa, e si
//! ferma con un messaggio utile dove non lo trova.

pub mod history;
pub mod reference;
pub mod store;

/// I tipi puri con cui un descrittore dichiara dove stanno i suoi numeri,
/// ri-esportati da qui.
///
/// **PERCHÉ RI-ESPORTATI E NON RIDEFINITI.** `toolbox` deve poter costruire una
/// ricetta senza dipendere a sua volta da `models`, e una copia di questi tipi
/// da questa parte del confine sarebbe una seconda definizione della stessa
/// cosa: due strutture gemelle divergono al primo campo che qualcuno aggiunge a
/// una sola delle due.
pub use models::usage::{read_declared, Declared, Pointer, Reading, Shape};

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies, ValueSchema};
use ledger::{Ledger, ModelCallRecord};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Il nome sotto cui `ExternalEngineAction` si registra in un
/// `flow::ActionRegistry`.
pub const EXTERNAL_ENGINE_ACTION: &str = "external_engine";
/// Il nome sotto cui `ShellCheckAction` si registra.
pub const SHELL_CHECK_ACTION: &str = "shell_check";

/// Registra entrambe le azioni sotto i loro nomi stabili: la scorciatoia per
/// chi vuole entrambe senza scegliere i nomi a mano.
/// Registra entrambe le azioni sotto i loro nomi stabili.
///
/// Il motore registrato qui **non sa risolvere uno strumento per
/// identificativo**: un passo che scrive `tool` riceve un errore che dice come
/// si ripara. Chi vuole quella capacità registra `EXTERNAL_ENGINE_ACTION` con
/// `ExternalEngineAction::resolving_with(...)` dopo questa chiamata — lo fa
/// `sailor flow`, che è l'unico punto dove `toolbox` e le azioni si incontrano.
pub fn register_default(registry: &mut flow::ActionRegistry) {
    registry.register(EXTERNAL_ENGINE_ACTION, ExternalEngineAction::new());
    registry.register(SHELL_CHECK_ACTION, ShellCheckAction::new());
}

// ── chi guarda il testo mentre esce ─────────────────────────────────────

/// Da quale delle due pipe del figlio viene un pezzo di testo.
///
/// La primitiva non le tratta diversamente — accumula l'una e l'altra allo
/// stesso modo — ma consegnarle a chi guarda senza dire quale sia quale
/// rimetterebbe l'opacità da un'altra parte: un errore mescolato all'uscita
/// normale e indistinguibile da lei non è più visibile di prima.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pipe {
    Stdout,
    Stderr,
}

impl Pipe {
    /// Il nome breve della cosa, non la sua presentazione: chi stampa decide
    /// come mostrarlo, ma non deve reinventare come si chiama.
    pub fn name(self) -> &'static str {
        match self {
            Pipe::Stdout => "out",
            Pipe::Stderr => "err",
        }
    }
}

/// Chi riceve i pezzi di uscita di un figlio **mentre** escono, invece che
/// quando il figlio è morto.
///
/// **SI CONSEGNANO BYTE GREZZI, E LA SCELTA È DICHIARATA.** Una lettura si
/// ferma dove capita, anche a metà di una sequenza UTF-8 multibyte. Decodificare
/// qui sostituirebbe l'accento spezzato al bordo con un carattere di
/// sostituzione — un guasto invisibile e permanente — oppure obbligherebbe
/// questo crate a trattenere i byte incompleti fino al pezzo dopo, cioè a
/// reintrodurre in piccolo il ritardo che il meccanismo esiste per togliere. I
/// byte passano di peso, nel loro ordine e integri: chi guarda li riversa su un
/// descrittore e la sequenza si ricompone da sé, oppure li accumula e decodifica
/// quando gli serve. La decodifica è di chi guarda, che è l'unico a sapere cosa
/// vuole farne.
///
/// `chunk` non deve bloccare a lungo né panicare: lo chiamano i due fili che
/// drenano le pipe, e un filo fermo è un figlio bloccato in scrittura. E non
/// riceve mai un pezzo vuoto: «zero byte» è la fine della pipe, non qualcosa
/// che il figlio ha detto, e consegnarlo farebbe scrivere una riga a chi guarda
/// per un fatto che non è accaduto.
pub trait LiveSink: Send + Sync {
    fn chunk(&self, pipe: Pipe, bytes: &[u8]);
}

/// Una closure basta: un destinatario semplice non deve costare un tipo.
impl<F> LiveSink for F
where
    F: Fn(Pipe, &[u8]) + Send + Sync,
{
    fn chunk(&self, pipe: Pipe, bytes: &[u8]) {
        self(pipe, bytes)
    }
}

/// Dato l'identificativo di un passo, a chi consegnare i suoi pezzi.
///
/// **PERCHÉ DUE LIVELLI E NON UNO.** La primitiva che legge le pipe non sa cosa
/// sia un passo e non deve saperlo: sarebbe politica dentro il crate che tocca
/// il mondo. Chi compone il programma sa entrambe le cose e fa da giunto — è lì
/// che si decide dove il testo va a finire, non qui. Ed è il punto dove un
/// secondo consumatore potrà attaccarsi domani, con una fabbrica che ne alimenta
/// due, senza che una riga di questo file cambi.
pub trait StepSinks: Send + Sync {
    fn sink_for(&self, step: &str) -> Arc<dyn LiveSink>;
}

// ── la primitiva: imporre un limite di durata ───────────────────────────

/// L'esito grezzo di un comando entro un tempo massimo. Chi lo consuma
/// decide se un'uscita diversa da zero conta come un fallimento vero o come
/// un dato da riportare: questa primitiva non lo sa e non deve saperlo.
pub enum RunOutcome {
    Finished {
        status: std::process::ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    TimedOut,
    /// Col motivo del sistema operativo. «Non si è avviato» da solo manda a
    /// cercare un binario assente quando il file c'era e non era eseguibile:
    /// sono due riparazioni diverse, e chi legge deve poterle distinguere.
    SpawnFailed(String),
}

/// Niente `timeout(1)`: non esiste su ogni macchina che esegue questi
/// binari. Il tetto è un ciclo di `try_wait` con `kill` alla scadenza, e due
/// fili drenano le pipe man mano — un figlio che le riempie prima che
/// qualcuno le legga resterebbe bloccato in scrittura per sempre.
pub fn run_with_timeout(cmd: Command, limit: Duration) -> RunOutcome {
    run_with_timeout_watched(cmd, limit, None)
}

/// Come `run_with_timeout`, ma consegna a `sink` ogni pezzo di stdout e di
/// stderr **appena arriva**, senza aspettare che il figlio muoia. Con `None` il
/// comportamento è quello di sempre, byte per byte.
///
/// **AFFIANCATA, NON UN PARAMETRO IN PIÙ SU QUELLA DI PRIMA.** Altri crate
/// chiamano `run_with_timeout`: cambiarle la firma per un dato che a loro non
/// serve li costringerebbe a scrivere `None` per non chiedere niente, e una
/// promessa additiva che rompe i chiamanti non è additiva.
pub fn run_with_timeout_watched(
    mut cmd: Command,
    limit: Duration,
    sink: Option<&dyn LiveSink>,
) -> RunOutcome {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(error) => return RunOutcome::SpawnFailed(error.to_string()),
    };
    drain_and_wait(
        child.stdout.take(),
        child.stderr.take(),
        &mut child,
        limit,
        sink,
    )
}

/// Come `run_with_timeout`, ma scrive un testo sullo standard input del
/// figlio subito dopo averlo avviato, poi lo chiude — un motore che legge il
/// proprio ingresso da lì (come lo script di prova per OpenRouter) altrimenti
/// resterebbe in attesa di un EOF che non arriva mai.
pub fn run_with_timeout_and_stdin(cmd: Command, stdin: &[u8], limit: Duration) -> RunOutcome {
    run_with_timeout_and_stdin_watched(cmd, stdin, limit, None)
}

/// La gemella guardata di `run_with_timeout_and_stdin`, per la stessa ragione.
pub fn run_with_timeout_and_stdin_watched(
    mut cmd: Command,
    stdin: &[u8],
    limit: Duration,
    sink: Option<&dyn LiveSink>,
) -> RunOutcome {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(error) => return RunOutcome::SpawnFailed(error.to_string()),
    };
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(stdin);
        // `pipe` esce di scope qui e chiude il descrittore: il figlio vede
        // l'EOF anche se non ha altro da leggere.
    }
    drain_and_wait(
        child.stdout.take(),
        child.stderr.take(),
        &mut child,
        limit,
        sink,
    )
}

/// Svuota una pipe fino a EOF, accumulando tutto e consegnando ogni pezzo a chi
/// guarda **una volta sola**.
///
/// **A PEZZI E NON `read_to_end`, ED È TUTTA LA DIFFERENZA.** Con `read_to_end`
/// i byte esistevano in memoria appena arrivati ma nessuno poteva vederli prima
/// del `join`, cioè prima della morte del figlio: non c'era un buffer cattivo da
/// togliere, mancava il destinatario. L'accumulo resta identico e **non dipende
/// dalla consegna**: chi guarda non può far mancare né raddoppiare ciò che
/// l'esito riporta.
fn drain(pipe: &mut impl Read, which: Pipe, sink: Option<&dyn LiveSink>) -> Vec<u8> {
    let mut all = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => {
                all.extend_from_slice(&buf[..read]);
                if let Some(sink) = sink {
                    sink.chunk(which, &buf[..read]);
                }
            }
            // Come faceva `read_to_end`: un segnale arrivato durante la lettura
            // non è la fine dell'uscita, e trattarlo così troncherebbe il testo
            // di un figlio sano.
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    all
}

fn drain_and_wait(
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    child: &mut std::process::Child,
    limit: Duration,
    sink: Option<&dyn LiveSink>,
) -> RunOutcome {
    let mut out_pipe = stdout.expect("stdout è piped");
    let mut err_pipe = stderr.expect("stderr è piped");
    // `scope` e non `spawn`: i fili prendono in prestito il destinatario, che
    // vive nello stack di chi ha chiamato. Con fili staccati l'API pretenderebbe
    // un `'static` — cioè un `Arc` — da chiunque voglia guardare, compresa una
    // prova che cattura una variabile locale.
    std::thread::scope(|scope| {
        let out_thread = scope.spawn(move || drain(&mut out_pipe, Pipe::Stdout, sink));
        let err_thread = scope.spawn(move || drain(&mut err_pipe, Pipe::Stderr, sink));
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if start.elapsed() >= limit {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break None,
            }
        };
        // I `join` restano dopo la morte del figlio: è così che le pipe si
        // chiudono e i fili finiscono. Ciò che era già stato consegnato prima
        // dell'uccisione è già arrivato a chi guarda, e resta anche qui dentro.
        let stdout = out_thread.join().unwrap_or_default();
        let stderr = err_thread.join().unwrap_or_default();
        match status {
            Some(status) => RunOutcome::Finished {
                status,
                stdout,
                stderr,
            },
            None => RunOutcome::TimedOut,
        }
    })
}

// ── invocare un motore esterno ───────────────────────────────────────────

/// Cosa serve per invocare un motore esterno: un binario già risolto — la
/// ricerca sul percorso, coi suoi fallback per un servizio senza la shell di
/// chi lo installa, resta a chi chiama: qui non si cablano posti — i suoi
/// argomenti, l'ambiente, la cartella di lavoro, un testo opzionale
/// sull'ingresso, e il tetto di tempo.
pub struct EngineInvocation {
    pub bin: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub workdir: Option<String>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
}

/// L'esito di un'invocazione: successo con l'uscita catturata, o una delle
/// forme di fallimento che un motore esterno può dare — mai un panico, e mai
/// un giudizio su cosa quel fallimento significhi per chi ha chiamato.
///
/// I fallimenti portano con sé di che spiegarsi: il codice di uscita (`None`
/// quando il processo è stato ucciso da un segnale, che non è la stessa cosa di
/// «uscito con zero») e il motivo del sistema operativo quando non è partito.
/// Chi trasforma questo esito in un passo rosso perde l'uscita tipata — è per
/// questo che il perché deve stare qui dentro, non solo nei byte catturati.
pub enum EngineResult {
    Ok {
        stdout: String,
        stderr: String,
    },
    ExitError {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    },
    TimedOut,
    SpawnFailed {
        reason: String,
    },
}

pub fn invoke_external_engine(invocation: &EngineInvocation) -> EngineResult {
    invoke_external_engine_watched(invocation, None)
}

/// Come `invoke_external_engine`, ma passa il destinatario alla primitiva.
///
/// **NESSUN CAMPO NUOVO IN `EngineInvocation`**: chi la costruisce lo fa con un
/// letterale completo (lo fa `notte`), e un campo in più romperebbe quei
/// letterali. Il destinatario è un argomento della chiamata, non un pezzo della
/// ricetta: non descrive *cosa* eseguire, descrive chi sta guardando.
pub fn invoke_external_engine_watched(
    invocation: &EngineInvocation,
    sink: Option<&dyn LiveSink>,
) -> EngineResult {
    let mut cmd = Command::new(&invocation.bin);
    cmd.args(&invocation.args);
    for (key, value) in &invocation.env {
        cmd.env(key, value);
    }
    if let Some(workdir) = &invocation.workdir {
        cmd.current_dir(workdir);
    }
    let outcome = match &invocation.stdin {
        Some(bytes) => run_with_timeout_and_stdin_watched(cmd, bytes, invocation.timeout, sink),
        None => {
            cmd.stdin(Stdio::null());
            run_with_timeout_watched(cmd, invocation.timeout, sink)
        }
    };
    match outcome {
        RunOutcome::Finished {
            status,
            stdout,
            stderr,
        } => {
            let stdout = String::from_utf8_lossy(&stdout).into_owned();
            let stderr = String::from_utf8_lossy(&stderr).into_owned();
            if status.success() {
                EngineResult::Ok { stdout, stderr }
            } else {
                EngineResult::ExitError {
                    code: status.code(),
                    stdout,
                    stderr,
                }
            }
        }
        RunOutcome::TimedOut => EngineResult::TimedOut,
        RunOutcome::SpawnFailed(reason) => EngineResult::SpawnFailed { reason },
    }
}

// ── eseguire una verifica con un tempo massimo ───────────────────────────

pub struct CheckInvocation {
    pub command: String,
    pub env: BTreeMap<String, String>,
    pub timeout: Duration,
}

/// Asimmetrico di proposito: chi passa non ha niente da spiegare, chi fallisce
/// sì — e senza queste due righe un passo rosso non lascia in mano a nessuno il
/// motivo, perché l'uscita tipata di un passo rotto non si scrive.
pub enum CheckResult {
    Passed,
    Failed { code: Option<i32>, stderr: String },
    TimedOut,
}

/// Esegue `command` con `sh -c`: la verifica di un compito è testo di shell
/// scritto da chi lo definisce, non un binario risolto a monte.
pub fn run_shell_check(invocation: &CheckInvocation) -> CheckResult {
    run_shell_check_watched(invocation, None)
}

/// Come `run_shell_check`, ma passa il destinatario alla primitiva. Vale la
/// stessa ragione della gemella: `CheckInvocation` non guadagna campi.
pub fn run_shell_check_watched(
    invocation: &CheckInvocation,
    sink: Option<&dyn LiveSink>,
) -> CheckResult {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&invocation.command).stdin(Stdio::null());
    for (key, value) in &invocation.env {
        cmd.env(key, value);
    }
    match run_with_timeout_watched(cmd, invocation.timeout, sink) {
        RunOutcome::Finished { status, stderr, .. } => {
            if status.success() {
                CheckResult::Passed
            } else {
                CheckResult::Failed {
                    code: status.code(),
                    stderr: String::from_utf8_lossy(&stderr).into_owned(),
                }
            }
        }
        RunOutcome::TimedOut => CheckResult::TimedOut,
        // Un binario `sh` che non parte è un guasto dell'ambiente, non della
        // verifica: si tratta come fallita, non come "passata per omissione".
        RunOutcome::SpawnFailed(reason) => CheckResult::Failed {
            code: None,
            stderr: reason,
        },
    }
}

// ── le due azioni registrabili in un flow::ActionRegistry ───────────────

/// Come si passa da «voglio *questo* strumento» all'eseguibile che lo è qui.
///
/// **PERCHÉ UN TRATTO E NON UNA CHIAMATA.** Chi sa quali strumenti esistono su
/// una macchina è `toolbox`, e `toolbox` dipende da questo crate: chiamarlo da
/// qui chiuderebbe un anello. Ma la ragione vera viene prima dell'anello: un
/// flusso non deve sapere *come* si cerca uno strumento. Chi compone il registro
/// delle azioni sceglie — dove Sailor gira si leggono i descrittori, in una
/// prova si risponde senza toccare il disco — e il flusso resta lo stesso file.
pub trait ToolResolver: Send + Sync {
    /// Il percorso dell'eseguibile che vale `id` su questa macchina, oppure il
    /// motivo per cui non si può usare, scritto per una persona: quel testo
    /// finisce dentro il passo rosso, ed è tutto ciò che chi legge avrà.
    fn resolve(&self, id: &str) -> Result<String, String>;

    /// Come si fa una domanda secca a `id`, se il suo descrittore lo dichiara.
    ///
    /// **PERCHÉ IL PASSO NON DEVE SAPERLO.** Finché le opzioni di un motore
    /// stanno scritte dentro un passo — `-p` per uno, `--mode plan --print` per
    /// un altro — quel passo è legato a quel motore, e un flusso «indipendente
    /// dal modello» lo è solo nel nome. Il 29/08/2026 sei passi su sei di un
    /// flusso nominavano lo stesso motore: quando quello ha esaurito la quota,
    /// il flusso è morto mentre un altro motore, installato e vivo, non è stato
    /// nemmeno provato.
    ///
    /// Chi non la dichiara restituisce `None`, e il passo dovrà dire le opzioni
    /// da sé: si funziona peggio, non in silenzio.
    fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
        None
    }
}

/// Dove va a finire il testo della domanda quando si interroga un motore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptVia {
    /// Sull'ingresso standard.
    Stdin,
    /// Come ultimo argomento della riga di comando.
    LastArg,
}

/// Come si interroga un motore in un colpo solo, e come quel motore dice di
/// **non poter lavorare**.
#[derive(Clone, Debug)]
pub struct AskRecipe {
    /// Le opzioni che vogliono una domanda secca, senza il testo della domanda.
    pub args: Vec<String>,
    /// Dove va il testo della domanda.
    pub prompt: PromptVia,
    /// I frammenti che, comparendo nell'uscita di un fallimento, dicono che
    /// **questo motore non poteva lavorare** — quota esaurita, credenziali
    /// mancanti — e non che il lavoro fosse sbagliato.
    ///
    /// **PERCHÉ LA DISTINZIONE È TUTTO.** Passare al motore successivo a ogni
    /// fallimento sarebbe la cosa peggiore: un mandato scritto male
    /// scenderebbe la catena fino a un modello che risponde comunque, e la
    /// risposta sbagliata arriverebbe senza che nessuno sappia perché. Si passa
    /// oltre **solo** quando il motore ha dichiarato di non poter lavorare, e
    /// solo con le parole che il suo descrittore dichiara: chi non le dichiara
    /// non fa scattare nessun ripiego.
    pub unusable_when: Vec<String>,
    /// Come si legge **quanto ha consumato**, se il suo descrittore lo dichiara.
    ///
    /// Viaggia sulla stessa strada di tutto il resto della ricetta: chi scrive
    /// un descrittore lo dichiara una volta, e nessun flusso deve conoscerlo.
    /// `None` è la risposta di chi non lo dichiara, e non è un guasto: quel
    /// motore si invoca come prima e i suoi token restano sconosciuti.
    pub usage: Option<UsageRecipe>,
}

/// Le opzioni da aggiungere per farsi dire il consumo, e dove leggerlo.
#[derive(Clone, Debug)]
pub struct UsageRecipe {
    pub args: Vec<String>,
    pub declared: Declared,
}

/// Se questa uscita è il modo in cui un motore dice di non poter lavorare. Il
/// confronto ignora maiuscole e minuscole: nessun fornitore promette di non
/// cambiarle. Un frammento vuoto non conta — combacerebbe con tutto, e
/// trasformerebbe ogni fallimento in un ripiego.
fn says_it_cannot_work(marks: &[String], output: &str) -> bool {
    let output = output.to_lowercase();
    marks
        .iter()
        .any(|mark| !mark.trim().is_empty() && output.contains(&mark.to_lowercase()))
}

/// Il destinatario del passo in corso, se qualcuno sta guardando.
///
/// **PERCHÉ L'IDENTIFICATIVO ARRIVA DALLO STATO CONDIVISO.** `Action::execute`
/// non riceve il passo, e cambiargli la firma toccherebbe ogni implementatore in
/// cinque crate per un dato che serve a uno solo. L'esecutore lo scrive in
/// `SharedState` sotto una chiave riservata (`flow::CURRENT_STEP`) prima di ogni
/// azione. Senza guardiano, o senza quella chiave, non si guarda: un testo
/// consegnato senza sapere di chi è sarebbe peggio del silenzio, perché in un
/// grafo con due passi vivi nessuno saprebbe attribuirlo.
fn sink_for_step(
    watcher: &Option<Arc<dyn StepSinks>>,
    shared: &SharedState,
) -> Option<Arc<dyn LiveSink>> {
    let watcher = watcher.as_ref()?;
    let step = shared.get(flow::CURRENT_STEP)?.as_str()?;
    Some(watcher.sink_for(step))
}

/// Gli esiti di fallimento che un motore esterno può produrre.
const ENGINE_FAILURES: [&str; 3] = ["exit_error", "timed_out", "spawn_failed"];
/// Quelli di una verifica di shell.
const CHECK_FAILURES: [&str; 2] = ["failed", "timed_out"];

/// Un `accept` che nomina un esito impossibile è un errore di chi ha scritto il
/// passo, non un silenzio: darebbe una tolleranza che non si applica mai, e il
/// passo diventerebbe rosso il giorno in cui serviva che non lo fosse.
fn check_tolerance(accept: &[String], known: &[&str]) -> Result<(), ActionError> {
    for name in accept {
        if !known.contains(&name.as_str()) {
            return Err(ActionError::new(
                "invalid_input",
                format!(
                    "`accept` nomina «{name}», che questo passo non può produrre; i valori possibili sono: {}",
                    known.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

fn tolerates(accept: &[String], status: &str) -> bool {
    accept.iter().any(|name| name == status)
}

/// Gli esiti che non lasciano nessuna risposta da mettere in forma.
const SILENT_FAILURES: [&str; 2] = ["timed_out", "spawn_failed"];

/// **CHIEDERE SENZA VERIFICARE E VERIFICARE SENZA CHIEDERE SONO LO STESSO
/// DIFETTO**, e questo controllo chiude il cerchio dalla parte che di solito
/// resta aperta: un motore non rispetta una forma perché qualcuno l'ha
/// dichiarata in un campo, la rispetta se gliel'hanno detta. Qui si guarda che
/// il testo della forma compaia davvero in ciò che sta per partire — sia esso
/// l'ingresso o un argomento — e se non c'è il passo si ferma **prima** di
/// spendere una chiamata che fallirebbe di sicuro.
fn shape_was_asked_for(written: &str, spec: &EngineSpec) -> Result<(), ActionError> {
    if let Some(silent) = SILENT_FAILURES
        .iter()
        .find(|status| tolerates(&spec.accept, status))
    {
        return Err(ActionError::new(
            "invalid_input",
            format!(
                "il passo dichiara una forma per la risposta e insieme tollera «{silent}», che non lascia nessuna risposta: le due cose non stanno insieme"
            ),
        ));
    }
    let mut sent = spec.stdin.clone().unwrap_or_default();
    for arg in &spec.args {
        sent.push('\n');
        sent.push_str(arg);
    }
    if sent.contains(written) {
        return Ok(());
    }
    Err(ActionError::new(
        "shape_not_in_prompt",
        format!(
            "il passo pretende una risposta in una forma dichiarata, ma quella forma non compare in ciò che manda al motore: mettila nel prompt con un rinvio {} a /answer_shape, così è scritta una volta sola. La forma è: {written}",
            reference::JSON_KEY
        ),
    ))
}

/// Quanto di ciò che ha detto un comando entra nel messaggio di un passo rotto.
const SAID_TAIL: usize = 1200;

/// **LE ULTIME RIGHE, NON LE PRIME.** Un motore che fallisce scrive l'errore in
/// fondo, dopo pagine di avvio. E servono davvero qui dentro: un passo rotto non
/// scrive nessuna uscita tipata, quindi senza questo testo stdout e stderr
/// muoiono col processo e chi guarda il deposito trova un rosso senza motivo.
fn tail(text: &str) -> &str {
    let text = text.trim_end();
    if text.len() <= SAID_TAIL {
        return text;
    }
    let mut start = text.len() - SAID_TAIL;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

fn what_it_said(stdout: &str, stderr: &str) -> String {
    let mut parts = Vec::new();
    if !stderr.trim().is_empty() {
        parts.push(format!("stderr: {}", tail(stderr)));
    }
    if !stdout.trim().is_empty() {
        parts.push(format!("stdout: {}", tail(stdout)));
    }
    if parts.is_empty() {
        return "non ha detto niente, né su stdout né su stderr".to_owned();
    }
    parts.join("\n")
}

/// `None` non è «uscito con zero»: è un processo ucciso da un segnale, e
/// confonderli manda a cercare un guasto nel posto sbagliato.
fn how_it_exited(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("è uscito con codice {code}"),
        None => "è stato ucciso da un segnale".to_owned(),
    }
}

/// Chi eseguire: un motore, o una catena di motori da provare in ordine.
///
/// **PERCHÉ UNA CATENA E NON UN RIPIEGO SOLO.** Un ripiego singolo copre il
/// caso di stanotte e non quello di domani: i motori esauriscono a scaglioni,
/// e chi ne ha tre installati vuole che il lavoro trovi il primo che può
/// farlo. La catena si legge nell'ordine in cui è scritta, e quell'ordine è
/// una scelta di chi ha scritto il flusso — il migliore per primo, non il più
/// economico.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ToolChoice {
    One(String),
    Chain(Vec<String>),
}

impl ToolChoice {
    fn ids(&self) -> &[String] {
        match self {
            ToolChoice::One(id) => std::slice::from_ref(id),
            ToolChoice::Chain(ids) => ids,
        }
    }
}

#[derive(Debug, Deserialize)]
struct EngineSpec {
    /// Il comando così com'è. Resta per un comando qualunque — `sh`, `cat`, uno
    /// script — non per un motore: un motore si chiede per identificativo, o il
    /// flusso gira solo dove quel nome è nel percorso di chi esegue.
    #[serde(default)]
    bin: Option<String>,
    /// L'identificativo dello strumento voluto — lo stesso che il rilevatore
    /// della macchina restituisce — oppure una **catena** di identificativi da
    /// provare in ordine.
    #[serde(default)]
    tool: Option<ToolChoice>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    workdir: Option<String>,
    /// Il testo dell'ingresso, se il motore lo legge da lì invece che da un
    /// argomento: JSON non porta byte grezzi, un motore binario sull'ingresso
    /// non è un caso che questa azione copre.
    #[serde(default)]
    stdin: Option<String>,
    /// Gli esiti di fallimento che questo passo dichiara accettabili invece che
    /// rossi. Vuoto — il valore predefinito — significa che ogni fallimento
    /// rompe il passo.
    #[serde(default)]
    accept: Vec<String>,
    /// La forma che questo passo pretende dalla propria risposta.
    ///
    /// **PERCHÉ NON È UN CONTROLLO IN PIÙ MA UN CONTRATTO.** Senza, un passo
    /// restituisce un blocco di testo libero e il passo dopo ci pesca dentro con
    /// un rinvio sperando che la forma sia quella: un motore che un giorno
    /// risponde più prolisso rompe la catena in silenzio. Con, la forma è
    /// scritta una volta, viene chiesta al motore (deve comparire nel prompt, e
    /// qui si controlla che ci sia) e viene fatta rispettare sulla risposta.
    ///
    /// **E PASSA SOLO CIÒ CHE LA FORMA DICHIARA.** I preamboli, i ragionamenti
    /// e i saluti non entrano nell'uscita del passo: al passo dopo arriva
    /// l'oggetto potato sui campi dichiarati. È il risparmio che si paga a ogni
    /// chiamata a valle, ed è la ragione per cui la potatura avviene anche
    /// quando la forma tollererebbe campi in più.
    #[serde(default)]
    answer_shape: Option<ValueSchema>,
    timeout_secs: u64,
}

/// Il testo da leggere come JSON dentro ciò che un motore ha detto.
///
/// Un modello incornicia spesso la risposta in un blocco recintato, a volte
/// dopo una riga di cortesia: si accetta **il primo blocco recintato**, e se non
/// ce n'è nessuno tutto il testo. Non si cercano le parentesi più esterne dentro
/// una frase: quella regola accetterebbe anche mezza risposta, o un esempio
/// citato nel discorso, e un dato sbagliato che passa è peggio di un rosso.
fn json_body(said: &str) -> &str {
    let trimmed = said.trim();
    let Some(open) = trimmed.find("```") else {
        return trimmed;
    };
    let after = &trimmed[open + 3..];
    // La riga della recinzione può portare il nome del linguaggio: si scarta.
    let body = match after.find('\n') {
        Some(end) => &after[end + 1..],
        None => return trimmed,
    };
    match body.find("```") {
        Some(close) => body[..close].trim(),
        None => body.trim(),
    }
}

/// Tiene solo i campi che la forma dichiara. `allow_extra` dice cosa si
/// **tollera** nella risposta; questa potatura dice cosa si **inoltra**, e sono
/// due domande diverse: la prima difende dal motore prolisso, la seconda dal
/// costo di portarselo dietro per tutta la catena.
fn pruned(shape: &ValueSchema, value: Value) -> Value {
    match (shape, value) {
        (ValueSchema::Object { properties, .. }, Value::Object(fields)) => {
            let mut kept = serde_json::Map::new();
            for (name, item) in fields {
                if let Some(inner) = properties.get(&name) {
                    kept.insert(name, pruned(inner, item));
                }
            }
            Value::Object(kept)
        }
        (ValueSchema::Array { items }, Value::Array(values)) => Value::Array(
            values
                .into_iter()
                .map(|value| pruned(items, value))
                .collect(),
        ),
        (_, value) => value,
    }
}

/// Legge la risposta di un motore secondo la forma che il passo ha dichiarato.
fn shaped_answer(shape: &ValueSchema, said: &str) -> Result<Value, ActionError> {
    let body = json_body(said);
    let value: Value = serde_json::from_str(body).map_err(|error| {
        ActionError::new(
            "answer_not_json",
            format!(
                "il passo pretende una risposta in una forma dichiarata, e ciò che è arrivato non è JSON: {error}; ha detto: {}",
                tail(said)
            ),
        )
    })?;
    shape.validate(&value).map_err(|error| {
        ActionError::new(
            "answer_off_shape",
            format!(
                "la risposta non rispetta la forma dichiarata dal passo ({error}); ha detto: {}",
                tail(said)
            ),
        )
    })?;
    Ok(pruned(shape, value))
}

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
    tools: Option<Arc<dyn ToolResolver>>,
    watcher: Option<Arc<dyn StepSinks>>,
    ledger: Option<Ledger>,
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
        }
    }

    /// Con un risolutore: `"tool": "codex"` diventa il percorso che vale
    /// `codex` su questa macchina.
    pub fn resolving_with(resolver: impl ToolResolver + 'static) -> Self {
        Self {
            tools: Some(Arc::new(resolver)),
            watcher: None,
            ledger: None,
        }
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

    /// Chi eseguire, in ordine di preferenza. `bin` e `tool` non convivono: due
    /// risposte alla stessa domanda vorrebbero una precedenza, e una precedenza
    /// fra «il nome che ho scritto» e «quello che c'è sulla macchina» sarebbe una
    /// regola che nessuno ricorda al momento giusto.
    ///
    /// Restituisce anche i motori che **non** si possono usare qui, col motivo:
    /// se nessuno resta, quel motivo è tutto ciò che chi legge avrà.
    fn candidates(&self, spec: &EngineSpec) -> Result<(Vec<Candidate>, Vec<Refused>), ActionError> {
        match (spec.bin.as_deref(), spec.tool.as_ref()) {
            (Some(bin), None) => Ok((
                vec![Candidate {
                    id: None,
                    bin: bin.to_owned(),
                    args: spec.args.clone(),
                    prompt: PromptVia::Stdin,
                    unusable_when: Vec::new(),
                    declared_usage: None,
                }],
                Vec::new(),
            )),
            (None, Some(choice)) => {
                let Some(tools) = &self.tools else {
                    let first = choice.ids().first().map(String::as_str).unwrap_or("");
                    return Err(ActionError::new(
                        "no_tool_resolver",
                        format!(
                            "il passo chiede lo strumento «{first}», ma questo motore è stato \
                             registrato senza un modo per risolverlo: chi costruisce il registro \
                             deve registrare `{EXTERNAL_ENGINE_ACTION}` con \
                             `ExternalEngineAction::resolving_with(...)`"
                        ),
                    ));
                };
                let ids = choice.ids();
                if ids.is_empty() {
                    return Err(ActionError::new(
                        "invalid_input",
                        "il passo dichiara una catena di motori vuota: serve almeno un \
                         identificativo, o `tool` va tolto del tutto",
                    ));
                }
                let step_said_args = !spec.args.is_empty();
                let mut usable = Vec::new();
                let mut refused = Vec::new();
                for id in ids {
                    let bin = match tools.resolve(id) {
                        Ok(bin) => bin,
                        Err(reason) => {
                            refused.push(Refused {
                                id: id.clone(),
                                reason,
                                unresolved: true,
                            });
                            continue;
                        }
                    };
                    // Le opzioni scritte nel passo vincono sulla ricetta: chi le
                    // ha scritte sta dicendo qualcosa di preciso su *questa*
                    // chiamata, e sovrascriverle sarebbe decidere al posto suo.
                    if step_said_args {
                        usable.push(Candidate {
                            id: Some(id.clone()),
                            bin,
                            args: spec.args.clone(),
                            prompt: PromptVia::Stdin,
                            unusable_when: tools
                                .ask_recipe(id)
                                .map(|recipe| recipe.unusable_when)
                                .unwrap_or_default(),
                            // **NIENTE CONSUMO QUANDO LE OPZIONI LE SCRIVE IL
                            // PASSO**, ed è la stessa regola di due righe più
                            // su applicata al dato nuovo: le opzioni del
                            // consumo si accodano a quelle della ricetta, e qui
                            // la ricetta non detta niente. Accodarle lo stesso
                            // vorrebbe dire allungare alle spalle di chi ha
                            // scritto quella riga di comando una domanda che
                            // non ha fatto. Il consumo resta sconosciuto — la
                            // riga nel deposito si scrive comunque, e dice
                            // proprio questo.
                            declared_usage: None,
                        });
                        continue;
                    }
                    match tools.ask_recipe(id) {
                        Some(recipe) => usable.push(Candidate {
                            id: Some(id.clone()),
                            bin,
                            args: match &recipe.usage {
                                Some(usage) => {
                                    let mut args = recipe.args;
                                    args.extend(usage.args.iter().cloned());
                                    args
                                }
                                None => recipe.args,
                            },
                            prompt: recipe.prompt,
                            unusable_when: recipe.unusable_when,
                            declared_usage: recipe.usage.map(|usage| usage.declared),
                        }),
                        None => refused.push(Refused {
                            id: id.clone(),
                            reason: "il passo non dice con quali opzioni interrogarlo e il suo \
                                     descrittore non dichiara come gli si fa una domanda (`ask`)"
                                .to_owned(),
                            unresolved: false,
                        }),
                    }
                }
                Ok((usable, refused))
            }
            (Some(_), Some(_)) => Err(ActionError::new(
                "invalid_input",
                "il passo dichiara sia `bin` sia `tool`: uno solo dei due dice chi eseguire",
            )),
            (None, None) => Err(ActionError::new(
                "invalid_input",
                "il passo non dice chi eseguire: serve `tool` (l'identificativo di uno strumento, \
                 o una catena di identificativi) oppure `bin` (un comando così com'è)",
            )),
        }
    }
}

/// Un motore chiesto dal passo che qui non si può nemmeno provare.
struct Refused {
    id: String,
    reason: String,
    /// Vero quando il risolutore non ha saputo dire quale eseguibile sia — la
    /// distinzione conta: un passo che chiede **un** motore solo e non lo trova
    /// deve dare `tool_unavailable` col motivo del risolutore, come ha sempre
    /// fatto. La catena non deve peggiorare il caso più comune.
    unresolved: bool,
}

impl Refused {
    fn line(&self) -> String {
        format!("«{}»: {}", self.id, self.reason)
    }
}

/// Un motore che si può provare: già risolto in un eseguibile, con le opzioni
/// con cui interrogarlo e le parole con cui dichiara di non poter lavorare.
struct Candidate {
    /// L'identificativo, se è stato chiesto per identificativo. `None` quando il
    /// passo ha scritto un comando così com'è.
    id: Option<String>,
    bin: String,
    args: Vec<String>,
    prompt: PromptVia,
    unusable_when: Vec<String>,
    /// Dove leggere il consumo nell'uscita di questo motore. `None` quando il
    /// descrittore non lo dichiara, o quando le opzioni le ha scritte il passo.
    declared_usage: Option<Declared>,
}

// ── quanto è costata una chiamata ────────────────────────────────────────

/// Dove sta il listino locale su questa macchina.
///
/// Risposta a «il listino deve essere modificabile senza ricompilare»: è un
/// file JSON nella casa di Sailor, accanto al deposito e ai flussi, e si
/// riscrive con un editor di testo. `SAILOR_PRICING` lo sposta altrove — serve
/// alle prove, e a chi tiene più listini.
///
/// **NON STA IN `modelli.json`**, che è la *scelta* dell'utente su quale
/// modello usare: mescolare «cosa voglio» e «quanto costa» farebbe sì che
/// cambiare una preferenza tocchi un listino, e viceversa.
const PRICING_ENV: &str = "SAILOR_PRICING";
const PRICING_FILE: &str = "pricing.json";

/// Il listino, riletto a ogni chiamata.
///
/// **RILETTO, NON TENUTO IN MEMORIA**: un prezzo cambiato a metà di una corsa
/// lunga vale dalla chiamata dopo, invece che dal prossimo riavvio. Il costo è
/// una lettura di un file piccolo accanto all'avvio di un processo esterno —
/// cioè niente, in confronto a ciò che sta per succedere.
///
/// Un listino assente, illeggibile o scritto male non è un guasto: lascia il
/// costo sconosciuto. Fermare una chiamata a un motore perché non si sa quanto
/// costerà sarebbe un tetto di spesa, che qui non c'è ed è un lavoro separato.
fn load_pricing() -> Option<models::pricing::PriceList> {
    let path = match std::env::var_os(PRICING_ENV).filter(|value| !value.is_empty()) {
        Some(declared) => std::path::PathBuf::from(declared),
        None => ledger::sailor_home()?.join(PRICING_FILE),
    };
    let text = std::fs::read_to_string(path).ok()?;
    models::pricing::PriceList::parse(&text).ok()
}

/// Dove registrare quanto si è speso: deposito, corsa e passo.
///
/// Servono tutti e tre. Senza uno solo **non si scrive nessuna riga**, invece
/// di scriverne una attribuita a nessuno: una riga senza corsa non si somma con
/// nessun'altra e sporcherebbe i conti peggio di una riga mancante. È la stessa
/// regola che `sink_for_step` applica già al testo dal vivo.
struct Recording<'a> {
    ledger: &'a Ledger,
    run_id: String,
    step_id: String,
}

fn recording_for<'a>(ledger: &'a Option<Ledger>, shared: &SharedState) -> Option<Recording<'a>> {
    Some(Recording {
        ledger: ledger.as_ref()?,
        run_id: shared.get(flow::CURRENT_RUN)?.as_str()?.to_owned(),
        step_id: shared.get(flow::CURRENT_STEP)?.as_str()?.to_owned(),
    })
}

/// Un contatore di processo, perché due chiamate nello stesso secondo dentro lo
/// stesso passo non si sovrascrivano a vicenda: `call_id` è chiave primaria, e
/// una collisione farebbe sparire una spesa invece di sommarla.
static CALLS_SO_FAR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// Ciò che si sa di una chiamata appena finita, prima di darle un prezzo.
struct Spent {
    reading: Reading,
    error_type: Option<&'static str>,
    started_at: i64,
    ended_at: i64,
}

/// Scrive nel deposito la riga di **questa** chiamata.
///
/// **SI SCRIVE ANCHE QUANDO È ANDATA MALE**, ed è una scelta deliberata: un
/// turno interrotto brucia comunque la quota, e azzerarne il costo
/// sottostimerebbe la spesa esattamente nei minuti che precedono un
/// esaurimento — cioè quando la misura serve. Separare «lavoro utile» da
/// «quota consumata» è compito di chi legge le righe, non di chi le scrive.
///
/// **E SI SCRIVE ANCHE QUANDO I TOKEN SONO SCONOSCIUTI.** «Questo motore è
/// stato chiamato quaranta volte, token non dichiarati» è un'informazione su
/// cui si può agire; il silenzio nasconde il buco, e un totale che si presenta
/// come completo mentre è parziale è la bugia da cui questo lavoro nasce.
///
/// Un fallimento del deposito non rompe il passo: la misura è al servizio del
/// lavoro, non il contrario, e far fallire una chiamata già riuscita perché non
/// si è potuto annotarla sarebbe il contrario di ciò che si sta costruendo.
fn record_the_call(record: &Recording<'_>, candidate: &Candidate, tried_before: &[String], spent: Spent) {
    let Some(cli) = candidate.id.as_deref() else {
        // Un `bin` scritto a mano nel passo non è una chiamata a un modello:
        // `sh -c echo` non consuma nessuna quota, e riempirne il deposito
        // renderebbe illeggibile proprio la vista che questo lavoro esiste per
        // rendere leggibile.
        return;
    };
    let reading = spent.reading;
    let listino = load_pricing();
    // Il legame col listino passa dal nome che il motore stesso dichiara, non
    // da un'ipotesi: un modello presunto sarebbe un numero inventato con la
    // faccia di una misura, creduto per sempre da chiunque lo legga.
    let voce = listino
        .as_ref()
        .zip(reading.model.as_deref())
        .and_then(|(listino, name)| listino.find(name));
    let prices = voce.map(models::pricing::Price::micros).unwrap_or_default();
    let cost_micros = models::pricing::cost_micros(
        models::pricing::TokenCounts {
            input: reading.input_tokens,
            output: reading.output_tokens,
            cached: reading.cached_tokens,
            cache_write: reading.cache_write_tokens,
            cache_write_long: reading.cache_write_long_tokens,
        },
        prices,
    );
    let sequence = CALLS_SO_FAR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let written = ModelCallRecord {
        call_id: format!(
            "{}:{}:{}:{sequence}",
            record.run_id, record.step_id, spent.started_at
        ),
        run_id: record.run_id.clone(),
        step_id: Some(record.step_id.clone()),
        purpose: EXTERNAL_ENGINE_ACTION.to_owned(),
        cli: cli.to_owned(),
        // Un passo nomina lo strumento, non il modello: nessuno qui *chiede* un
        // modello, e scriverne uno sarebbe inventarlo. Vuoto vuol dire «non
        // dichiarato», e la finestra lo mostra come tale.
        requested_model: String::new(),
        actual_model: reading.model.clone().unwrap_or_default(),
        input_tokens: reading.input_tokens,
        output_tokens: reading.output_tokens,
        cached_tokens: reading.cached_tokens,
        cache_write_tokens: reading.cache_write_tokens,
        cache_write_long_tokens: reading.cache_write_long_tokens,
        total_tokens: reading.total_tokens,
        cost_micros,
        // Il costo del motore accanto al nostro, mai al posto suo.
        declared_cost_micros: reading
            .declared_cost
            .map(|usd| (usd * 1_000_000.0).round() as i64),
        // La valuta è quella del listino con cui si è calcolato: senza un conto
        // fatto non c'è nessuna valuta da dichiarare.
        price_currency: cost_micros
            .and(listino.as_ref())
            .map(|listino| listino.currency.clone()),
        input_price_micros_per_million: prices.input,
        output_price_micros_per_million: prices.output,
        cached_price_micros_per_million: prices.cached,
        cache_write_price_micros_per_million: prices.cache_write,
        cache_write_long_price_micros_per_million: prices.cache_write_long,
        mandate_name: String::new(),
        mandate_version: String::new(),
        retry_chain: tried_before.to_vec(),
        error_type: spent.error_type.map(str::to_owned),
        started_at: spent.started_at,
        ended_at: Some(spent.ended_at),
    };
    let _ = record.ledger.record_model_call(&written);
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

impl ExternalEngineAction {
    /// Interroga un motore. `set_aside` sono quelli già scartati, e finisce nei
    /// messaggi d'errore: chi legge un passo rosso deve vedere l'intera catena,
    /// non solo l'ultimo anello.
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
        tried_before: &[String],
    ) -> Result<Asked, ActionError> {
        let bin = &candidate.bin;
        let seconds = spec.timeout_secs;
        let mut args = candidate.args.clone();
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
                    format!("[sailor] passo al motore «{id}»\n").as_bytes(),
                );
            }
        }
        let invocation = EngineInvocation {
            bin: bin.clone(),
            args,
            env: spec.env.clone(),
            workdir: spec.workdir.clone(),
            stdin: stdin.map(String::into_bytes),
            timeout: Duration::from_secs(seconds),
        };
        let named = match candidate.id.as_deref() {
            Some(id) => format!("«{id}» (`{bin}`)"),
            None => format!("`{bin}`"),
        };
        // Gli istanti si prendono stretti attorno alla chiamata: è la durata di
        // *questa* invocazione, non del passo che la contiene.
        let started_at = now_secs();
        let result = invoke_external_engine_watched(&invocation, live);
        let ended_at = now_secs();
        // Il consumo si legge da ciò che il motore ha detto, secondo quanto il
        // suo descrittore dichiara. Chi non dichiara niente lascia tutto
        // sconosciuto — e non è un ramo `if` per fornitore: è l'assenza di un
        // dato nel descrittore.
        let read = |said: &str| match &candidate.declared_usage {
            Some(declared) => models::usage::read_declared(said, declared),
            None => Reading::default(),
        };
        // Ogni ramo passa di qui: anche il fallimento e anche il silenzio, che è
        // il punto — una chiamata interrotta ha comunque bruciato la quota.
        let note = |reading: Reading, error_type: Option<&'static str>| {
            if let Some(record) = record {
                record_the_call(
                    record,
                    candidate,
                    tried_before,
                    Spent {
                        reading,
                        error_type,
                        started_at,
                        ended_at,
                    },
                );
            }
        };
        let outcome = match result {
            EngineResult::Ok { stdout, stderr } => {
                let reading = read(&stdout);
                note(reading.clone(), None);
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
                let reading = read(&stdout);
                note(reading.clone(), Some("exit_error"));
                if !tolerates(&spec.accept, "exit_error") {
                    // La tolleranza viene prima: un passo che si aspetta un
                    // fallimento lo vuole come dato, non vuole che qualcun altro
                    // ci riprovi al posto suo.
                    if !solo && candidate.says_it_cannot_work(&stdout, &stderr) {
                        return Ok(Asked::CannotWork(format!(
                            "{named} non poteva lavorare: {}",
                            what_it_said(&stdout, &stderr)
                        )));
                    }
                    let chain = if set_aside.is_empty() {
                        String::new()
                    } else {
                        format!(" (prima: {})", each_one_why(set_aside))
                    };
                    return Err(ActionError::new(
                        "engine_exit_error",
                        format!(
                            "{named} {}; {}{chain}",
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
            EngineResult::TimedOut => {
                // Ucciso a metà: non ha detto niente, quindi non c'è niente da
                // leggere. La riga si scrive lo stesso, coi token sconosciuti —
                // il tempo che ha girato l'ha speso davvero.
                note(Reading::default(), Some("timed_out"));
                // Nessun ripiego su un tetto di tempo: un motore ucciso a metà
                // può aver già fatto qualcosa, e rifare quel lavoro altrove
                // sarebbe farlo due volte senza saperlo.
                if !tolerates(&spec.accept, "timed_out") {
                    return Err(ActionError::new(
                        "engine_timed_out",
                        format!("{named} non ha risposto entro {seconds} secondi ed è stato ucciso"),
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
                note(Reading::default(), Some("spawn_failed"));
                if !tolerates(&spec.accept, "spawn_failed") {
                    // Non essersi avviato è il caso più netto di «non poteva
                    // lavorare»: non ha fatto niente, e non serve che il suo
                    // descrittore lo dichiari.
                    if !solo {
                        return Ok(Asked::CannotWork(format!(
                            "{named} non si è potuto avviare: {reason}"
                        )));
                    }
                    return Err(ActionError::new(
                        "engine_spawn_failed",
                        format!("{named} non si è potuto avviare: {reason}"),
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
}

impl Candidate {
    fn says_it_cannot_work(&self, stdout: &str, stderr: &str) -> bool {
        says_it_cannot_work(&self.unusable_when, stdout)
            || says_it_cannot_work(&self.unusable_when, stderr)
    }
}

impl Action for ExternalEngineAction {
    fn execute(
        &self,
        input: &Value,
        shared: &SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        let live = sink_for_step(&self.watcher, shared);
        // Dove annotare la spesa. Si costruisce qui perché `shared` più avanti
        // non c'è più, ed è `None` — cioè non si annota niente — se manca il
        // deposito o uno dei due identificativi.
        let record = recording_for(&self.ledger, shared);
        let input = reference::resolve_references(input)?;
        // La forma si tiene anche com'era scritta: è quel testo, non una sua
        // riscrittura, che deve comparire nel prompt.
        let written_shape = input.get("answer_shape").map(|shape| {
            serde_json::to_string(shape).expect("un valore già in memoria si riserializza sempre")
        });
        let spec: EngineSpec = serde_json::from_value(input)
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        check_tolerance(&spec.accept, &ENGINE_FAILURES)?;
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
                    "nessuno dei motori che il passo chiede si può usare qui. {}",
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
        let mut tried_before: Vec<String> = Vec::new();
        for candidate in &candidates {
            match self.ask(
                candidate,
                &spec,
                shape,
                live.as_deref(),
                &set_aside,
                solo,
                record.as_ref(),
                &tried_before,
            )? {
                Asked::Answered(outcome) => return Ok(outcome),
                Asked::CannotWork(why) => {
                    set_aside.push(why);
                    if let Some(id) = &candidate.id {
                        tried_before.push(id.clone());
                    }
                }
            }
        }
        Err(ActionError::new(
            "no_usable_engine",
            format!(
                "nessuno dei motori che il passo chiede ha potuto lavorare. {}",
                each_one_why(&set_aside)
            ),
        ))
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

#[derive(Debug, Deserialize)]
struct CheckSpec {
    command: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// Come per il motore: vuoto vuol dire che una verifica fallita è un passo
    /// rotto. `["failed"]` la rimette fra i dati, per chi sul risultato ci vuole
    /// ramificare invece di fermarsi.
    #[serde(default)]
    accept: Vec<String>,
    timeout_secs: u64,
}

/// Esegue una verifica di shell con un tempo massimo, leggendo comando,
/// ambiente e tetto dall'ingresso tipato del passo. Stessa regola dell'azione
/// gemella, e per la stessa ragione: una verifica che fallisce rompe il proprio
/// passo, salvo che il passo dichiari `"accept": ["failed"]`.
///
/// **Un rinvio a ciò che ha detto un motore va in `env`, mai in `command`.**
/// Il comando è testo di shell e viene eseguito; una risposta di modello
/// incollata lì dentro è un comando scritto da chi ha risposto. Dentro una
/// variabile d'ambiente resta un dato, e il comando la legge fra virgolette.
#[derive(Default)]
pub struct ShellCheckAction {
    watcher: Option<Arc<dyn StepSinks>>,
}

impl ShellCheckAction {
    /// Senza nessuno che guarda: il testo della verifica si vede solo alla fine,
    /// come è sempre stato.
    pub fn new() -> Self {
        Self { watcher: None }
    }

    /// Con qualcuno che guarda. Vale per una verifica quanto per un motore: una
    /// suite di prove che gira dieci minuti è cieca esattamente come lui.
    pub fn watched_by(mut self, watcher: Option<Arc<dyn StepSinks>>) -> Self {
        self.watcher = watcher;
        self
    }
}

impl Action for ShellCheckAction {
    fn execute(
        &self,
        input: &Value,
        shared: &SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        let live = sink_for_step(&self.watcher, shared);
        let input = reference::resolve_references(input)?;
        let spec: CheckSpec = serde_json::from_value(input)
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        check_tolerance(&spec.accept, &CHECK_FAILURES)?;
        let seconds = spec.timeout_secs;
        let command = spec.command.clone();
        let invocation = CheckInvocation {
            command: spec.command,
            env: spec.env,
            timeout: Duration::from_secs(seconds),
        };
        let status = match run_shell_check_watched(&invocation, live.as_deref()) {
            CheckResult::Passed => "passed",
            CheckResult::Failed { code, stderr } => {
                if !tolerates(&spec.accept, "failed") {
                    return Err(ActionError::new(
                        "check_failed",
                        format!(
                            "la verifica `{command}` {}; {}",
                            how_it_exited(code),
                            what_it_said("", &stderr)
                        ),
                    ));
                }
                "failed"
            }
            CheckResult::TimedOut => {
                if !tolerates(&spec.accept, "timed_out") {
                    return Err(ActionError::new(
                        "check_timed_out",
                        format!(
                            "la verifica `{command}` non è finita entro {seconds} secondi ed è stata uccisa"
                        ),
                    ));
                }
                "timed_out"
            }
        };
        Ok(ActionOutcome::Went(json!({ "status": status })))
    }

    /// Una verifica interrotta si rifà: il suo mestiere è rileggere il mondo
    /// e dire com'è, non cambiarlo. Chi ci infila dentro un comando che
    /// modifica ha già rotto il contratto di questa azione, e lo aveva già
    /// rotto prima: il motore riesegue la verifica a ogni tentativo anche
    /// senza nessuna interruzione di mezzo.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    // ── run_with_timeout ─────────────────────────────────────────────

    #[test]
    fn a_quick_command_finishes_within_the_limit() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo ciao");
        match run_with_timeout(cmd, secs(5)) {
            RunOutcome::Finished { status, stdout, .. } => {
                assert!(status.success());
                assert_eq!(String::from_utf8_lossy(&stdout).trim(), "ciao");
            }
            _ => panic!("doveva finire in tempo"),
        }
    }

    /// LA MISURA CHE POTEVA VENIRE DIVERSA: un comando che dorme più a lungo
    /// del tetto viene ucciso, non aspettato. Un tetto largo (60s) con un
    /// limite stretto (1s) renderebbe questa prova rossa se il tetto non
    /// venisse davvero applicato.
    #[test]
    fn a_slow_command_is_killed_at_the_limit() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 60");
        let start = Instant::now();
        let outcome = run_with_timeout(cmd, secs(1));
        assert!(matches!(outcome, RunOutcome::TimedOut));
        assert!(
            start.elapsed() < secs(10),
            "il tetto deve troncare, non solo essere misurato: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn a_missing_binary_reports_spawn_failed() {
        let cmd = Command::new("/nessun/binario/qui-di-sicuro");
        assert!(matches!(
            run_with_timeout(cmd, secs(1)),
            RunOutcome::SpawnFailed(_)
        ));
    }

    #[test]
    fn stdin_reaches_the_child_and_gets_echoed_back() {
        let cmd = Command::new("cat");
        match run_with_timeout_and_stdin(cmd, b"un segreto pubblico\n", secs(5)) {
            RunOutcome::Finished { stdout, .. } => {
                assert_eq!(String::from_utf8_lossy(&stdout), "un segreto pubblico\n");
            }
            _ => panic!("cat doveva rispondere"),
        }
    }

    // ── il testo consegnato mentre il figlio è vivo ──────────────────

    /// Un destinatario che segna **quando** ha ricevuto ogni pezzo: è l'istante,
    /// non il contenuto, la cosa che distingue «consegnato mentre girava» da
    /// «consegnato tutto alla fine».
    struct Recorder {
        start: Instant,
        chunks: std::sync::Mutex<Vec<(Duration, Pipe, Vec<u8>)>>,
    }

    impl Recorder {
        fn new() -> Self {
            Self {
                start: Instant::now(),
                chunks: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<(Duration, Pipe, Vec<u8>)> {
            self.chunks.lock().expect("nessuno panica qui").clone()
        }

        /// Tutti i byte di una pipe, riattaccati nell'ordine di consegna.
        fn joined(&self, want: Pipe) -> Vec<u8> {
            self.seen()
                .into_iter()
                .filter(|(_, pipe, _)| *pipe == want)
                .flat_map(|(_, _, bytes)| bytes)
                .collect()
        }
    }

    impl LiveSink for Recorder {
        fn chunk(&self, pipe: Pipe, bytes: &[u8]) {
            self.chunks
                .lock()
                .expect("nessuno panica qui")
                .push((self.start.elapsed(), pipe, bytes.to_vec()));
        }
    }

    /// LA PROVA CHE CONTA, E CHE COL CODICE DI PRIMA SAREBBE ROSSA: il comando
    /// stampa, dorme quattro secondi, stampa ancora. Non si guarda che alla
    /// fine il testo ci sia — sarebbe verde anche consegnando tutto in blocco
    /// alla morte del figlio — si guarda **quando** è arrivato il primo pezzo.
    ///
    /// Margini larghi di proposito: quattro secondi di sonno contro una soglia
    /// di due, perché su una macchina carica la prova non diventi rossa a caso.
    #[test]
    fn the_first_chunk_arrives_while_the_child_is_still_alive() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo primo; sleep 4; echo secondo");
        let recorder = Recorder::new();
        let start = Instant::now();
        let outcome = run_with_timeout_watched(cmd, secs(30), Some(&recorder));
        let whole = start.elapsed();
        assert!(
            whole >= secs(4),
            "il comando doveva davvero durare quattro secondi, altrimenti la \
             misura non distingue niente: {whole:?}"
        );
        let seen = recorder.seen();
        let (when, pipe, bytes) = seen.first().cloned().expect("qualcosa doveva arrivare");
        // IL TEMPO PRIMA DI TUTTO IL RESTO: è l'istante la cosa che questa prova
        // misura, e leggerlo per ultimo nasconderebbe il motivo vero di un
        // rosso. Vale anche contro l'asserzione sul pezzo vuoto qui sotto:
        // rimettendo la consegna in blocco scatta *anche* quella, perché su una
        // pipe muta `read_to_end` produce zero byte, e chi legge il rosso
        // troverebbe il difetto minore al posto di quello grosso.
        assert!(
            when < secs(2),
            "il primo pezzo è arrivato dopo {when:?}, cioè con la fine del \
             comando e non mentre girava"
        );
        assert!(
            seen.iter().all(|(_, _, bytes)| !bytes.is_empty()),
            "un pezzo vuoto non è qualcosa che il figlio ha detto: {seen:?}"
        );
        assert_eq!(pipe, Pipe::Stdout);
        assert!(
            String::from_utf8_lossy(&bytes).contains("primo"),
            "il primo pezzo doveva essere «primo»: {:?}",
            String::from_utf8_lossy(&bytes)
        );
        match outcome {
            RunOutcome::Finished { stdout, .. } => {
                let all = String::from_utf8_lossy(&stdout).into_owned();
                assert!(all.contains("primo") && all.contains("secondo"), "{all}");
            }
            _ => panic!("doveva finire in tempo"),
        }
    }

    /// Chi guarda deve sapere da quale delle due pipe viene il testo: un errore
    /// indistinguibile dall'uscita normale non è più visibile di prima.
    #[test]
    fn each_chunk_says_which_pipe_produced_it() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo di-fuori; echo di-errore 1>&2");
        let recorder = Recorder::new();
        let _ = run_with_timeout_watched(cmd, secs(10), Some(&recorder));
        let out = String::from_utf8_lossy(&recorder.joined(Pipe::Stdout)).into_owned();
        let err = String::from_utf8_lossy(&recorder.joined(Pipe::Stderr)).into_owned();
        assert!(out.contains("di-fuori"), "stdout consegnato: {out:?}");
        assert!(err.contains("di-errore"), "stderr consegnato: {err:?}");
        assert!(!out.contains("di-errore"), "stderr finito su stdout: {out:?}");
        assert!(!err.contains("di-fuori"), "stdout finito su stderr: {err:?}");
    }

    /// NIENTE PERSO E NIENTE DOPPIO: la somma dei pezzi consegnati è, byte per
    /// byte, l'uscita che l'esito riporta. Molte righe apposta, perché con una
    /// sola la pipe si svuoterebbe in una lettura e la prova non direbbe nulla
    /// su cosa succede quando i pezzi sono tanti.
    #[test]
    fn the_delivered_chunks_add_up_to_the_accumulated_output() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("i=0; while [ $i -lt 500 ]; do echo \"riga $i di uscita normale\"; i=$((i+1)); done; echo problema 1>&2");
        let recorder = Recorder::new();
        match run_with_timeout_watched(cmd, secs(30), Some(&recorder)) {
            RunOutcome::Finished { stdout, stderr, .. } => {
                assert_eq!(recorder.joined(Pipe::Stdout), stdout);
                assert_eq!(recorder.joined(Pipe::Stderr), stderr);
                assert!(!stdout.is_empty(), "l'uscita non doveva essere vuota");
            }
            _ => panic!("doveva finire in tempo"),
        }
    }

    /// IL TETTO CONTINUA A UCCIDERE, e il testo già consegnato prima
    /// dell'uccisione non sparisce né arriva due volte.
    #[test]
    fn what_was_said_before_the_kill_is_delivered_once() {
        let mut cmd = Command::new("sh");
        // `exec` non è ornamento: senza, `sh` resta il figlio e `sleep` diventa
        // un nipote che tiene aperta la pipe anche dopo l'uccisione del padre —
        // e allora si aspetta il nipote, non il tetto. È una proprietà della
        // shell che c'era già prima di questo lavoro e che questa prova non ha
        // il compito di giudicare: qui si misura il tetto, quindi si fa in modo
        // che il processo ucciso sia davvero l'unico che scrive.
        cmd.arg("-c").arg("echo vivo; exec sleep 60");
        let recorder = Recorder::new();
        let start = Instant::now();
        let outcome = run_with_timeout_watched(cmd, secs(3), Some(&recorder));
        assert!(matches!(outcome, RunOutcome::TimedOut));
        assert!(
            start.elapsed() < secs(30),
            "il tetto deve troncare: {:?}",
            start.elapsed()
        );
        let out = String::from_utf8_lossy(&recorder.joined(Pipe::Stdout)).into_owned();
        assert_eq!(
            out.matches("vivo").count(),
            1,
            "«vivo» doveva arrivare una volta sola: {out:?}"
        );
    }

    /// CHI NON GUARDA OTTIENE ESATTAMENTE QUELLO DI PRIMA: stesso comando, una
    /// volta col destinatario e una senza, e le due uscite accumulate coincidono
    /// byte per byte. La consegna in diretta non è un ramo che cambia l'esito.
    #[test]
    fn without_a_watcher_the_outcome_is_byte_for_byte_the_same() {
        let command = "echo prima; echo dopo; echo lamentela 1>&2";
        let mut watched_cmd = Command::new("sh");
        watched_cmd.arg("-c").arg(command);
        let recorder = Recorder::new();
        let watched = run_with_timeout_watched(watched_cmd, secs(10), Some(&recorder));
        let mut plain_cmd = Command::new("sh");
        plain_cmd.arg("-c").arg(command);
        let plain = run_with_timeout(plain_cmd, secs(10));
        match (watched, plain) {
            (
                RunOutcome::Finished {
                    status: watched_status,
                    stdout: watched_out,
                    stderr: watched_err,
                },
                RunOutcome::Finished {
                    status: plain_status,
                    stdout: plain_out,
                    stderr: plain_err,
                },
            ) => {
                assert_eq!(watched_status.code(), plain_status.code());
                assert_eq!(watched_out, plain_out);
                assert_eq!(watched_err, plain_err);
                assert_eq!(
                    String::from_utf8_lossy(&plain_out).trim(),
                    "prima\ndopo",
                    "l'uscita intera deve restare nell'esito"
                );
                assert_eq!(String::from_utf8_lossy(&plain_err).trim(), "lamentela");
            }
            _ => panic!("tutti e due dovevano finire in tempo"),
        }
    }

    /// Il destinatario può essere una closure: chi ne ha uno semplice non deve
    /// dichiarare un tipo per averlo.
    #[test]
    fn a_closure_is_a_watcher_too() {
        let seen = std::sync::Mutex::new(Vec::new());
        let sink = |pipe: Pipe, bytes: &[u8]| {
            seen.lock()
                .expect("nessuno panica qui")
                .push((pipe, bytes.to_vec()));
        };
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo per-la-closure");
        let _ = run_with_timeout_watched(cmd, secs(10), Some(&sink));
        let seen = seen.into_inner().expect("nessuno panica qui");
        assert!(seen
            .iter()
            .any(|(pipe, bytes)| *pipe == Pipe::Stdout
                && String::from_utf8_lossy(bytes).contains("per-la-closure")));
    }

    /// DI QUALE PASSO È IL TESTO: l'azione chiede il destinatario alla fabbrica
    /// nominando il passo che lo `SharedState` le porta, e quello che consegna è
    /// ciò che il motore ha detto.
    #[test]
    fn the_action_asks_the_factory_for_the_step_it_is_running() {
        #[derive(Default)]
        struct Fabbrica {
            asked: std::sync::Mutex<Vec<String>>,
            said: std::sync::Mutex<Vec<u8>>,
        }

        struct Ramo(Arc<Fabbrica>);

        impl LiveSink for Ramo {
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
        struct FabbricaArc(Arc<Fabbrica>);

        impl StepSinks for FabbricaArc {
            fn sink_for(&self, step: &str) -> Arc<dyn LiveSink> {
                self.0
                    .asked
                    .lock()
                    .expect("nessuno panica qui")
                    .push(step.to_owned());
                Arc::new(Ramo(self.0.clone()))
            }
        }

        let fabbrica = Arc::new(Fabbrica::default());
        let action = ExternalEngineAction::new()
            .watched_by(Some(Arc::new(FabbricaArc(fabbrica.clone())) as Arc<dyn StepSinks>));
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_STEP.to_owned(), json!("il-passo-che-parla"));
        let outcome = action
            .execute(
                &json!({"bin": "sh", "args": ["-c", "echo detto-dal-motore"], "timeout_secs": 10}),
                &mut shared,
            )
            .expect("il passo doveva riuscire");
        assert!(matches!(outcome, ActionOutcome::Went(_)));
        assert_eq!(
            *fabbrica.asked.lock().expect("nessuno panica qui"),
            vec!["il-passo-che-parla".to_owned()]
        );
        let said = String::from_utf8_lossy(&fabbrica.said.lock().expect("nessuno panica qui"))
            .into_owned();
        assert!(said.contains("detto-dal-motore"), "consegnato: {said:?}");
    }

    /// Senza la chiave del passo nello stato condiviso non si consegna niente:
    /// un testo che nessuno sa attribuire è peggio del silenzio.
    #[test]
    fn no_step_id_means_nobody_is_asked() {
        struct Mai;

        impl StepSinks for Mai {
            fn sink_for(&self, _step: &str) -> Arc<dyn LiveSink> {
                panic!("non doveva essere chiesto nessun destinatario");
            }
        }

        let action = ExternalEngineAction::new().watched_by(Some(Arc::new(Mai) as Arc<dyn StepSinks>));
        let mut shared = SharedState::new();
        action
            .execute(
                &json!({"bin": "sh", "args": ["-c", "echo muto"], "timeout_secs": 10}),
                &mut shared,
            )
            .expect("il passo doveva riuscire lo stesso");
    }

    // ── invoke_external_engine ───────────────────────────────────────

    #[test]
    fn a_successful_engine_call_is_ok() {
        let invocation = EngineInvocation {
            bin: "sh".to_string(),
            args: vec!["-c".to_string(), "echo 'answer: 42'".to_string()],
            env: BTreeMap::new(),
            workdir: None,
            stdin: None,
            timeout: secs(5),
        };
        match invoke_external_engine(&invocation) {
            EngineResult::Ok { stdout, .. } => assert!(stdout.contains("answer: 42"), "{stdout}"),
            _ => panic!("doveva riuscire"),
        }
    }

    #[test]
    fn a_nonzero_exit_is_exit_error_not_ok() {
        let invocation = EngineInvocation {
            bin: "sh".to_string(),
            args: vec!["-c".to_string(), "echo boom 1>&2; exit 1".to_string()],
            env: BTreeMap::new(),
            workdir: None,
            stdin: None,
            timeout: secs(5),
        };
        match invoke_external_engine(&invocation) {
            EngineResult::ExitError { stderr, .. } => assert!(stderr.contains("boom"), "{stderr}"),
            _ => panic!("un'uscita diversa da zero è un errore di uscita, non un successo"),
        }
    }

    #[test]
    fn an_engine_env_var_is_visible_to_the_child() {
        let mut env = BTreeMap::new();
        env.insert("PROVA_ACTIONS".to_string(), "c'è".to_string());
        let invocation = EngineInvocation {
            bin: "sh".to_string(),
            args: vec!["-c".to_string(), "echo \"$PROVA_ACTIONS\"".to_string()],
            env,
            workdir: None,
            stdin: None,
            timeout: secs(5),
        };
        match invoke_external_engine(&invocation) {
            EngineResult::Ok { stdout, .. } => assert_eq!(stdout.trim(), "c'è"),
            _ => panic!("doveva riuscire"),
        }
    }

    #[test]
    fn a_missing_engine_binary_is_spawn_failed() {
        let invocation = EngineInvocation {
            bin: "/nessun/binario/qui-di-sicuro".to_string(),
            args: vec![],
            env: BTreeMap::new(),
            workdir: None,
            stdin: None,
            timeout: secs(5),
        };
        assert!(matches!(
            invoke_external_engine(&invocation),
            EngineResult::SpawnFailed { .. }
        ));
    }

    #[test]
    fn an_engine_that_never_returns_times_out() {
        let invocation = EngineInvocation {
            bin: "sh".to_string(),
            args: vec!["-c".to_string(), "exec sleep 60".to_string()],
            env: BTreeMap::new(),
            workdir: None,
            stdin: None,
            timeout: secs(1),
        };
        assert!(matches!(
            invoke_external_engine(&invocation),
            EngineResult::TimedOut
        ));
    }

    #[test]
    fn engine_stdin_reaches_a_motore_that_reads_it() {
        let invocation = EngineInvocation {
            bin: "cat".to_string(),
            args: vec![],
            env: BTreeMap::new(),
            workdir: None,
            stdin: Some(b"prompt dall'ingresso\n".to_vec()),
            timeout: secs(5),
        };
        match invoke_external_engine(&invocation) {
            EngineResult::Ok { stdout, .. } => {
                assert_eq!(stdout, "prompt dall'ingresso\n");
            }
            _ => panic!("doveva riuscire"),
        }
    }

    // ── run_shell_check ───────────────────────────────────────────────

    #[test]
    fn a_true_check_passes() {
        let invocation = CheckInvocation {
            command: "true".to_string(),
            env: BTreeMap::new(),
            timeout: secs(5),
        };
        assert!(matches!(run_shell_check(&invocation), CheckResult::Passed));
    }

    #[test]
    fn a_false_check_fails() {
        let invocation = CheckInvocation {
            command: "false".to_string(),
            env: BTreeMap::new(),
            timeout: secs(5),
        };
        assert!(matches!(
            run_shell_check(&invocation),
            CheckResult::Failed { code: Some(1), .. }
        ));
    }

    #[test]
    fn a_hanging_check_times_out() {
        let invocation = CheckInvocation {
            command: "sleep 60".to_string(),
            env: BTreeMap::new(),
            timeout: secs(1),
        };
        assert!(matches!(
            run_shell_check(&invocation),
            CheckResult::TimedOut
        ));
    }

    #[test]
    fn a_check_reads_its_own_env_var() {
        let mut env = BTreeMap::new();
        env.insert("NOTTE_OUTPUT_FILE".to_string(), "/dev/null".to_string());
        let invocation = CheckInvocation {
            command: "test -n \"$NOTTE_OUTPUT_FILE\"".to_string(),
            env,
            timeout: secs(5),
        };
        assert!(matches!(run_shell_check(&invocation), CheckResult::Passed));
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
        let mut shared = SharedState::new();
        let outcome = action.execute(&input, &mut shared).expect("l'azione non fallisce");
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
            .execute(&input, &mut SharedState::new())
            .expect_err("un motore uscito in errore rompe il passo");

        assert_eq!(error.class, "engine_exit_error");
        assert!(error.said.contains("codice 3"), "{}", error.said);
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
            .execute(&input, &mut SharedState::new())
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
            .execute(&input, &mut SharedState::new())
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
            .execute(&input, &mut SharedState::new())
            .expect_err("un motore che non parte rompe il passo");

        assert_eq!(error.class, "engine_spawn_failed");
        assert!(error.said.contains("/nessun/binario/qui-di-sicuro"), "{}", error.said);
    }

    #[test]
    fn an_engine_that_never_returns_breaks_the_step_with_its_limit() {
        let action = ExternalEngineAction::new();
        let input = json!({"bin": "sh", "args": ["-c", "exec sleep 60"], "timeout_secs": 1});

        let error = action
            .execute(&input, &mut SharedState::new())
            .expect_err("il tempo scaduto rompe il passo");

        assert_eq!(error.class, "engine_timed_out");
        assert!(error.said.contains("1 secondi"), "{}", error.said);
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
            .execute(&input, &mut SharedState::new())
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
            .execute(&input, &mut SharedState::new())
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
            .execute(&input, &mut SharedState::new())
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
            .execute(&input, &mut SharedState::new())
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
            .execute(&input, &mut SharedState::new())
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
            .execute(&input, &mut SharedState::new())
            .expect_err("le due dichiarazioni non stanno insieme");

        assert_eq!(error.class, "invalid_input");
        assert!(error.said.contains("timed_out"), "{}", error.said);
    }

    // ── chiedere uno strumento per identificativo ─────────────────────

    struct FixedTools(&'static str);

    impl ToolResolver for FixedTools {
        fn resolve(&self, id: &str) -> Result<String, String> {
            if id == "il-motore" {
                Ok(self.0.to_owned())
            } else {
                Err(format!("«{id}» non è dichiarato da nessun descrittore"))
            }
        }
    }

    /// Il passo nomina uno strumento; chi eseguirlo lo decide la macchina.
    #[test]
    fn a_tool_id_becomes_the_executable_the_resolver_names() {
        let action = ExternalEngineAction::resolving_with(FixedTools("echo"));
        let input = json!({"tool": "il-motore", "args": ["risolto"], "timeout_secs": 5});

        let ActionOutcome::Went(output) = action
            .execute(&input, &mut SharedState::new())
            .expect("lo strumento si risolve")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(output["stdout"], "risolto\n");
    }

    /// Lo strumento che qui non c'è: il passo si ferma **prima** di spendere
    /// qualunque cosa, e porta con sé il motivo di chi ha guardato la macchina.
    #[test]
    fn a_tool_that_is_not_here_stops_the_step_with_the_resolvers_reason() {
        let action = ExternalEngineAction::resolving_with(FixedTools("echo"));
        let input = json!({"tool": "un-altro", "timeout_secs": 5});

        let error = action
            .execute(&input, &mut SharedState::new())
            .expect_err("lo strumento non c'è");

        assert_eq!(error.class, "tool_unavailable");
        assert!(error.said.contains("un-altro"), "{}", error.said);
    }

    /// Un motore registrato senza risolutore non indovina un binario dal nome
    /// dello strumento: dice come si ripara il registro.
    #[test]
    fn without_a_resolver_a_tool_step_says_how_to_repair_the_registry() {
        let action = ExternalEngineAction::new();
        let input = json!({"tool": "claude-code", "timeout_secs": 5});

        let error = action
            .execute(&input, &mut SharedState::new())
            .expect_err("nessuno sa risolvere gli strumenti");

        assert_eq!(error.class, "no_tool_resolver");
        assert!(error.said.contains("resolving_with"), "{}", error.said);
    }

    // ── la catena di motori ───────────────────────────────────────────

    /// Una macchina finta con tre motori: uno che dichiara di essere esaurito,
    /// uno che risponde, uno che non è installato.
    struct Chain;

    impl ToolResolver for Chain {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                // Stampa il messaggio di un motore esaurito ed esce 1.
                "esaurito" => Ok("false-dopo-aver-parlato".to_owned()),
                "vivo" => Ok("echo".to_owned()),
                "rotto" => Ok("false".to_owned()),
                "senza-ricetta" => Ok("echo".to_owned()),
                _ => Err(format!("«{id}» non è su questa macchina")),
            }
        }

        fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
            match id {
                "esaurito" => Some(AskRecipe {
                    args: Vec::new(),
                    prompt: PromptVia::Stdin,
                    unusable_when: vec!["weekly limit".to_owned()],
                    usage: None,
                }),
                "vivo" => Some(AskRecipe {
                    args: vec!["ha-risposto-il-secondo".to_owned()],
                    prompt: PromptVia::LastArg,
                    unusable_when: vec!["weekly limit".to_owned()],
                    usage: None,
                }),
                "rotto" => Some(AskRecipe {
                    args: Vec::new(),
                    prompt: PromptVia::Stdin,
                    unusable_when: vec!["weekly limit".to_owned()],
                    usage: None,
                }),
                // Risolvibile ma senza ricetta: un passo che non scrive le
                // opzioni non sa come interrogarlo.
                _ => None,
            }
        }
    }

    /// Un eseguibile finto che dice di essere esaurito ed esce in errore.
    fn engine_that_says_it_is_out(dir: &std::path::Path) -> String {
        let path = dir.join("false-dopo-aver-parlato");
        std::fs::write(
            &path,
            "#!/bin/sh\necho \"You've hit your weekly limit · resets 7am\"\nexit 1\n",
        )
        .expect("scrivere il finto motore");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("renderlo eseguibile");
        }
        path.to_string_lossy().into_owned()
    }

    struct ChainIn(String);

    impl ToolResolver for ChainIn {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                "esaurito" => Ok(self.0.clone()),
                other => Chain.resolve(other),
            }
        }
        fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
            Chain.ask_recipe(id)
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sailor-catena-{name}"));
        std::fs::create_dir_all(&dir).expect("cartella di lavoro");
        dir
    }

    /// **Il caso del 29/08/2026.** Il primo motore dichiara di essere esaurito;
    /// il lavoro non muore, passa al secondo, e il secondo risponde.
    #[test]
    fn an_engine_that_says_it_is_out_hands_the_work_to_the_next_one() {
        let dir = scratch("passa-al-secondo");
        let action =
            ExternalEngineAction::resolving_with(ChainIn(engine_that_says_it_is_out(&dir)));
        let input = json!({"tool": ["esaurito", "vivo"], "timeout_secs": 10});

        let ActionOutcome::Went(output) = action
            .execute(&input, &mut SharedState::new())
            .expect("il secondo motore risponde")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(output["status"], "ok");
        assert_eq!(output["stdout"], "ha-risposto-il-secondo\n");
    }

    /// Un eseguibile finto che fallisce **parlando**, ma non con le parole con
    /// cui quel motore dichiarerebbe di essere esaurito.
    ///
    /// **PERCHÉ NON BASTA UN COMANDO CHE FALLISCE MUTO.** La prima versione
    /// della prova qui sotto usava `false`, che esce 1 senza dire niente, e un
    /// mutante che faceva scattare il ripiego su *qualunque* uscita le è
    /// passato sotto: con l'uscita vuota, «qualunque uscita» e «quelle parole»
    /// si comportano uguale. Un fallimento vero parla, ed è quello il caso che
    /// questa prova deve tenere.
    fn engine_that_fails_loudly(dir: &std::path::Path) -> String {
        let path = dir.join("fallisce-parlando");
        std::fs::write(
            &path,
            "#!/bin/sh\necho 'errore: il mandato non ha senso' >&2\nexit 1\n",
        )
        .expect("scrivere il finto motore");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("renderlo eseguibile");
        }
        path.to_string_lossy().into_owned()
    }

    struct LoudFailure(String);

    impl ToolResolver for LoudFailure {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                "rotto" => Ok(self.0.clone()),
                other => Chain.resolve(other),
            }
        }
        fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
            Chain.ask_recipe(id)
        }
    }

    /// **La metà che conta di più.** Un fallimento qualunque NON scende la
    /// catena: un mandato scritto male deve fermarsi lì, non trovare più in
    /// basso un motore che risponde comunque — quella sarebbe una risposta
    /// sbagliata con la faccia di una buona.
    #[test]
    fn an_ordinary_failure_does_not_walk_down_the_chain() {
        let dir = scratch("fallimento-qualunque");
        let action =
            ExternalEngineAction::resolving_with(LoudFailure(engine_that_fails_loudly(&dir)));
        let input = json!({"tool": ["rotto", "vivo"], "timeout_secs": 10});

        let error = action
            .execute(&input, &mut SharedState::new())
            .expect_err("il primo è fallito senza dire di non poter lavorare");

        assert_eq!(error.class, "engine_exit_error");
        assert!(
            error.said.contains("il mandato non ha senso"),
            "{}",
            error.said
        );
    }

    /// Un descrittore scritto a mano con un frammento **vuoto** fra le parole
    /// di `unusable_when`: quel frammento è contenuto in qualunque testo, e
    /// senza una guardia farebbe scendere la catena a **ogni** fallimento —
    /// cioè esattamente il guasto che la catena esiste per non introdurre. Chi
    /// ha scritto quel descrittore non se ne accorgerebbe: funzionerebbe, e
    /// darebbe risposte sbagliate.
    #[test]
    fn an_empty_mark_in_a_descriptor_does_not_make_everything_a_fallback() {
        struct EmptyMark(String);
        impl ToolResolver for EmptyMark {
            fn resolve(&self, id: &str) -> Result<String, String> {
                match id {
                    "rotto" => Ok(self.0.clone()),
                    other => Chain.resolve(other),
                }
            }
            fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
                match id {
                    "rotto" => Some(AskRecipe {
                        args: Vec::new(),
                        prompt: PromptVia::Stdin,
                        unusable_when: vec![String::new(), "   ".to_owned()],
                        usage: None,
                    }),
                    other => Chain.ask_recipe(other),
                }
            }
        }

        let dir = scratch("frammento-vuoto");
        let action =
            ExternalEngineAction::resolving_with(EmptyMark(engine_that_fails_loudly(&dir)));
        let input = json!({"tool": ["rotto", "vivo"], "timeout_secs": 10});

        let error = action
            .execute(&input, &mut SharedState::new())
            .expect_err("un frammento vuoto non è una dichiarazione di esaurimento");

        assert_eq!(error.class, "engine_exit_error");
    }

    /// Quando ogni motore della catena dichiara di non poter lavorare, il passo
    /// è rosso con il motivo di **ognuno**: chi legge deve vedere l'intera
    /// catena, non solo l'ultimo anello.
    #[test]
    fn a_chain_that_is_entirely_out_names_every_engine() {
        let dir = scratch("tutti-esauriti");
        let action =
            ExternalEngineAction::resolving_with(ChainIn(engine_that_says_it_is_out(&dir)));
        let input = json!({"tool": ["esaurito", "non-installato"], "timeout_secs": 10});

        let error = action
            .execute(&input, &mut SharedState::new())
            .expect_err("nessuno dei due può lavorare");

        assert_eq!(error.class, "no_usable_engine");
        assert!(error.said.contains("esaurito"), "{}", error.said);
        assert!(error.said.contains("non-installato"), "{}", error.said);
    }

    /// Il descrittore decide dove va il testo della domanda. Senza questo, un
    /// flusso dovrebbe conoscere le opzioni di ogni motore — ed è la ragione per
    /// cui i flussi erano legati a uno solo.
    #[test]
    fn the_descriptor_decides_where_the_question_goes() {
        let action = ExternalEngineAction::resolving_with(Chain);
        let input = json!({"tool": "vivo", "stdin": "la-domanda", "timeout_secs": 10});

        let ActionOutcome::Went(output) = action
            .execute(&input, &mut SharedState::new())
            .expect("risponde")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        // `echo` stampa i propri argomenti: se la domanda fosse finita
        // sull'ingresso invece che in coda agli argomenti, qui non ci sarebbe.
        assert_eq!(output["stdout"], "ha-risposto-il-secondo la-domanda\n");
    }

    /// Un motore che c'è ma non dichiara come lo si interroga non viene
    /// indovinato: si mette da parte col motivo, e si prova il prossimo.
    #[test]
    fn an_engine_without_a_recipe_is_set_aside_with_the_reason() {
        let action = ExternalEngineAction::resolving_with(Chain);
        let input = json!({"tool": ["senza-ricetta"], "timeout_secs": 10});

        let error = action
            .execute(&input, &mut SharedState::new())
            .expect_err("non si sa come interrogarlo");

        assert_eq!(error.class, "no_usable_engine");
        assert!(error.said.contains("ask"), "{}", error.said);
    }

    /// Le opzioni scritte nel passo vincono sulla ricetta: chi le ha scritte sta
    /// dicendo qualcosa di preciso su questa chiamata.
    #[test]
    fn options_written_in_the_step_win_over_the_recipe() {
        let action = ExternalEngineAction::resolving_with(Chain);
        let input = json!({"tool": "vivo", "args": ["scritte-nel-passo"], "timeout_secs": 10});

        let ActionOutcome::Went(output) = action
            .execute(&input, &mut SharedState::new())
            .expect("risponde")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(output["stdout"], "scritte-nel-passo\n");
    }

    #[test]
    fn a_step_cannot_declare_both_a_binary_and_a_tool() {
        let action = ExternalEngineAction::resolving_with(FixedTools("echo"));
        let input = json!({"bin": "sh", "tool": "il-motore", "timeout_secs": 5});

        let error = action
            .execute(&input, &mut SharedState::new())
            .expect_err("due risposte alla stessa domanda");

        assert_eq!(error.class, "invalid_input");
    }

    #[test]
    fn the_external_engine_action_rejects_an_input_without_a_binary() {
        let action = ExternalEngineAction::new();
        let input = json!({"timeout_secs": 5});
        let mut shared = SharedState::new();
        assert!(action.execute(&input, &mut shared).is_err());
    }

    #[test]
    fn the_shell_check_action_reads_its_json_input() {
        let action = ShellCheckAction::new();
        let input = json!({"command": "true", "timeout_secs": 5});
        let mut shared = SharedState::new();
        let ActionOutcome::Went(output) = action.execute(&input, &mut shared).unwrap() else {
            panic!("una verifica eseguita è sempre Went")
        };
        assert_eq!(output["status"], "passed");
    }

    /// **IL PASSAGGIO DI CONSEGNE, PROVATO SULL'AZIONE E NON SOLO SUL MODULO.**
    /// L'ingresso è quello che il motore compone davvero per un passo con una
    /// dipendenza: l'uscita del passo prima (`status`, `stdout`, `stderr`) più
    /// i valori fissi del campo `with`. Il mutante che la fa cadere è togliere
    /// la risoluzione dei rinvii: `stdin` resta un oggetto e l'azione rifiuta
    /// l'ingresso.
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
        let mut shared = SharedState::new();

        let ActionOutcome::Went(output) = action.execute(&input, &mut shared).unwrap() else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(output["status"], "ok");
        assert_eq!(
            output["stdout"],
            "Esegui solo la tua sezione.\n=== PER CODEX ===\nconta i ganci morti",
            "il motore ha ricevuto sull'ingresso ciò che il passo prima ha scritto"
        );
    }

    /// La verifica finale legge il verdetto del modello da una variabile
    /// d'ambiente. **Tre casi, perché uno solo non proverebbe niente**: lo
    /// stesso comando deve passare su un verdetto e rompere il passo sugli
    /// altri due — il verdetto contrario e il motore muto.
    #[test]
    fn the_verdict_check_reads_the_models_answer_and_can_say_no() {
        let command =
            "printf '%s' \"$VERDICT\" | grep -v '^[[:space:]]*$' | tail -n 1 | grep -q 'VERDETTO: APPROVATO'";

        let verdict = |said: &str| {
            let input = json!({
                "status": "ok",
                "stdout": said,
                "stderr": "",
                "command": command,
                "env": {"VERDICT": {"$from": "/stdout"}},
                "timeout_secs": 5
            });
            ShellCheckAction::new()
                .execute(&input, &mut SharedState::new())
                .map(|outcome| {
                    let ActionOutcome::Went(output) = outcome else {
                        panic!("una verifica accettata è sempre Went")
                    };
                    output["status"].as_str().unwrap().to_owned()
                })
                .map_err(|error| error.class)
        };

        assert_eq!(verdict("ho guardato i file\nVERDETTO: APPROVATO\n"), Ok("passed".to_owned()));
        assert_eq!(
            verdict("mancano due sezioni\nVERDETTO: RESPINTO\n"),
            Err("check_failed".to_owned())
        );
        assert_eq!(verdict(""), Err("check_failed".to_owned()), "un motore muto non approva");
    }

    /// Una verifica fallita rompe il passo, e chi vuole ramificarci sopra lo
    /// dichiara. Senza questa seconda metà, «rosso» sarebbe l'unica cosa che
    /// una verifica sa fare, non una scelta.
    #[test]
    fn a_failing_check_breaks_its_step_unless_the_step_says_otherwise() {
        let strict = json!({"command": "echo perche 1>&2; exit 2", "timeout_secs": 5});
        let error = ShellCheckAction::new()
            .execute(&strict, &mut SharedState::new())
            .expect_err("una verifica fallita è un passo rotto");
        assert_eq!(error.class, "check_failed");
        assert!(error.said.contains("codice 2"), "{}", error.said);
        assert!(error.said.contains("perche"), "{}", error.said);

        let tolerant = json!({
            "command": "exit 2",
            "accept": ["failed"],
            "timeout_secs": 5
        });
        let ActionOutcome::Went(output) = ShellCheckAction::new()
            .execute(&tolerant, &mut SharedState::new())
            .expect("l'esito è dichiarato accettabile")
        else {
            panic!("un esito tollerato resta un dato")
        };
        assert_eq!(output["status"], "failed");
    }

    /// Un puntatore che non trova niente è un errore dell'azione, non un
    /// ingresso vuoto passato al motore: costerebbe una chiamata vera.
    #[test]
    fn a_dangling_reference_stops_the_engine_before_it_costs_anything() {
        let action = ExternalEngineAction::new();
        let input = json!({
            "bin": "cat",
            "stdin": {"$from": "/dispatch/stdout"},
            "timeout_secs": 5
        });
        let mut shared = SharedState::new();

        let error = action
            .execute(&input, &mut shared)
            .expect_err("il puntatore non trova niente");

        assert_eq!(error.class, "unresolved_reference");
    }

    #[test]
    fn the_registry_finds_both_actions_by_their_stable_names() {
        let mut registry = flow::ActionRegistry::default();
        register_default(&mut registry);
        assert!(registry.get(EXTERNAL_ENGINE_ACTION).is_some());
        assert!(registry.get(SHELL_CHECK_ACTION).is_some());
    }
}

#[cfg(test)]
mod quanto_e_costata {
    //! Le prove della misura: quanto ha consumato una chiamata, dove finisce
    //! scritta, e che cosa succede a chi non lo dichiara.
    //!
    //! **NESSUN MOTORE VERO E NESSUNA CHIAMATA A PAGAMENTO.** I motori qui
    //! dentro sono script di shell scritti al volo, come quelli che il resto di
    //! questo file usa già: sono l'unico modo di provare una misura senza
    //! comprarla.

    use super::*;
    use ledger::Ledger;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-consumo-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("cartella di lavoro");
        dir
    }

    /// Uno script eseguibile che si comporta come gli si dice.
    fn fake_engine(dir: &std::path::Path, name: &str, body: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("scrivere il finto motore");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("renderlo eseguibile");
        path.to_string_lossy().into_owned()
    }

    /// Il listino di prova, con la cache a un decimo dell'ingresso: è la
    /// differenza che il criterio 3 del mandato esiste per non perdere.
    const LISTINO: &str = r#"{
      "currency": "USD",
      "dated": "2026-08-29",
      "models": [
        { "id": "modello-di-prova", "aliases": ["prova"],
          "input_per_million": 3.0, "output_per_million": 15.0,
          "cached_per_million": 0.3 }
      ]
    }"#;

    fn write_listino(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("pricing.json");
        std::fs::write(&path, LISTINO).expect("scrivere il listino");
        path
    }

    /// Uno stato condiviso come quello che l'esecutore prepara prima di ogni
    /// azione: corsa e passo, sotto le chiavi riservate di `flow`.
    fn shared(run: &str, step: &str) -> SharedState {
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_RUN.to_owned(), json!(run));
        shared.insert(flow::CURRENT_STEP.to_owned(), json!(step));
        shared
    }

    /// Un risolutore che punta a uno script e gli attacca la ricetta che gli si
    /// dà: è il posto dove, nella vita vera, arriva un descrittore.
    struct Declares {
        bin: String,
        recipe: Option<AskRecipe>,
    }

    impl ToolResolver for Declares {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                "motore-di-prova" => Ok(self.bin.clone()),
                other => Err(format!("«{other}» non è su questa macchina")),
            }
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            self.recipe.clone()
        }
    }

    fn path(keys: &[&str]) -> Option<Pointer> {
        Some(Pointer::Path(keys.iter().map(|k| (*k).to_owned()).collect()))
    }

    /// La ricetta di un motore che sa dire quanto ha consumato: chiede
    /// l'involucro e dichiara dove stanno i numeri, il modello e la risposta.
    fn declaring_recipe() -> AskRecipe {
        AskRecipe {
            args: Vec::new(),
            prompt: PromptVia::Stdin,
            unusable_when: Vec::new(),
            usage: Some(UsageRecipe {
                args: vec!["--output-format".to_owned(), "json".to_owned()],
                declared: Declared {
                    read: Shape::Json,
                    input_tokens: path(&["usage", "input_tokens"]),
                    output_tokens: path(&["usage", "output_tokens"]),
                    cached_tokens: path(&["usage", "cache_read_input_tokens"]),
                    cache_write_tokens: path(&["usage", "cache_creation_input_tokens"]),
                    cache_write_long_tokens: None,
                    total_tokens: None,
                    cost: path(&["total_cost_usd"]),
                    model: path(&["model"]),
                    answer: path(&["result"]),
                },
            }),
        }
    }

    /// Un motore che risponde con l'involucro **solo** se gli si è chiesto
    /// `--output-format json`, e in chiaro altrimenti: è il comportamento vero
    /// di una riga di comando, e senza di lui la prova sull'uscita invariata
    /// non proverebbe niente.
    const WRAPS_ON_DEMAND: &str = r#"cat > /dev/null
printf '%s\n' "$@" > "$(dirname "$0")/argv"
if [ "$1" = "--output-format" ] && [ "$2" = "json" ]; then
  printf '{"result":"la risposta vera","model":"modello-di-prova","total_cost_usd":0.5,"usage":{"input_tokens":1000000,"output_tokens":1000000,"cache_read_input_tokens":1000000}}'
else
  printf 'la risposta vera'
fi"#;

    /// **UN MOTORE CHE RISPONDE IN JSON SENZA CHE NESSUNO GLIEL'ABBIA CHIESTO.**
    /// Serve a provare che il consumo si legge perché un DESCRITTORE lo
    /// dichiara, non perché l'uscita per caso somiglia a un formato noto: se
    /// qui dentro comparisse un ramo cablato su chiavi di un fornitore, i suoi
    /// token verrebbero letti lo stesso, ed è esattamente ciò che il vincolo di
    /// indipendenza dal modello vieta.
    const ALWAYS_WRAPS: &str = r#"cat > /dev/null
printf '{"result":"la risposta vera","model":"modello-di-prova","total_cost_usd":0.5,"usage":{"input_tokens":1000000,"output_tokens":1000000,"cache_read_input_tokens":1000000}}'"#;

    /// La riga di comando con cui il finto motore è stato davvero invocato.
    fn argv_of(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("argv"))
            .expect("il motore finto ha scritto la propria riga di comando")
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn calls_in(dir: &std::path::Path) -> Vec<ledger::ModelCallRecord> {
        let ledger = Ledger::open(dir).expect("riaprire il deposito");
        let dump = ledger
            .projection_dump()
            .expect("il deposito sa dire cosa contiene");
        ui_free_parse(&dump)
    }

    /// Legge le righe di `model_calls` dal dump, per posizione. `actions` non
    /// dipende da `ui`, quindi la lettura sta qui: è poca, e la dipendenza
    /// inversa sarebbe un ciclo.
    fn ui_free_parse(dump: &Value) -> Vec<ledger::ModelCallRecord> {
        dump.get("model_calls")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        let cols = row.as_array()?;
                        let count = |i: usize| -> Option<u64> {
                            let value = cols.get(i)?;
                            value.as_u64().or_else(|| value.as_str()?.parse().ok())
                        };
                        Some(ledger::ModelCallRecord {
                            call_id: cols.first()?.as_str()?.to_owned(),
                            run_id: cols.get(1)?.as_str()?.to_owned(),
                            step_id: cols.get(2)?.as_str().map(str::to_owned),
                            purpose: cols.get(3)?.as_str()?.to_owned(),
                            cli: cols.get(4)?.as_str()?.to_owned(),
                            requested_model: cols.get(5)?.as_str()?.to_owned(),
                            actual_model: cols.get(6)?.as_str()?.to_owned(),
                            input_tokens: count(7),
                            output_tokens: count(8),
                            cached_tokens: count(9),
                            cache_write_tokens: count(23),
                            cache_write_long_tokens: count(24),
                            cost_micros: cols.get(10)?.as_i64(),
                            price_currency: cols.get(11)?.as_str().map(str::to_owned),
                            input_price_micros_per_million: cols.get(12)?.as_i64(),
                            output_price_micros_per_million: cols.get(13)?.as_i64(),
                            cached_price_micros_per_million: cols.get(14)?.as_i64(),
                            cache_write_price_micros_per_million: cols.get(25).and_then(Value::as_i64),
                            cache_write_long_price_micros_per_million: cols
                                .get(26)
                                .and_then(Value::as_i64),
                            mandate_name: cols.get(15)?.as_str()?.to_owned(),
                            mandate_version: cols.get(16)?.as_str()?.to_owned(),
                            retry_chain: cols
                                .get(17)
                                .and_then(Value::as_str)
                                .and_then(|text| serde_json::from_str(text).ok())
                                .unwrap_or_default(),
                            error_type: cols.get(18)?.as_str().map(str::to_owned),
                            started_at: cols.get(19)?.as_i64()?,
                            ended_at: cols.get(20)?.as_i64(),
                            total_tokens: count(21),
                            declared_cost_micros: cols.get(22)?.as_i64(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Il listino vive in un file, e le prove non devono contendersi la casa di
    /// chi le esegue: `SAILOR_PRICING` lo sposta. Una serratura perché le prove
    /// girano in parallelo nello stesso processo e la variabile d'ambiente è una
    /// sola — senza, due prove si toglierebbero il listino a vicenda.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_listino<T>(listino: Option<&std::path::Path>, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match listino {
            Some(path) => std::env::set_var(PRICING_ENV, path),
            None => std::env::set_var(PRICING_ENV, "/nessun/listino/qui"),
        }
        let out = body();
        std::env::remove_var(PRICING_ENV);
        out
    }

    // ── (a) chi dichiara: token veri, cache a parte, costo dal listino ─

    /// **IL CRITERIO 2 E IL CRITERIO 3 INSIEME.** Un motore che dichiara come si
    /// legge il suo consumo produce una riga nel deposito con i token veri, la
    /// cache in una colonna sua, e il costo calcolato dal listino locale — non
    /// quello che il motore stesso dichiara.
    #[test]
    fn a_declaring_engine_writes_a_row_with_true_tokens_and_a_cost_from_the_listino() {
        let dir = scratch("dichiara");
        let listino = write_listino(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let outcome = with_listino(Some(&listino), || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("il motore risponde");
        let ActionOutcome::Went(output) = outcome else {
            panic!("un motore che risponde è sempre Went")
        };

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "una chiamata, una riga");
        let call = &calls[0];
        assert_eq!(call.run_id, "corsa-1");
        assert_eq!(call.step_id.as_deref(), Some("passo-1"));
        assert_eq!(call.cli, "motore-di-prova");
        assert_eq!(call.actual_model, "modello-di-prova");
        assert_eq!(call.input_tokens, Some(1_000_000));
        assert_eq!(call.output_tokens, Some(1_000_000));
        assert_eq!(
            call.cached_tokens,
            Some(1_000_000),
            "la cache ha una colonna sua e non finisce dentro l'ingresso"
        );
        // 1M a 3 $ + 1M a 15 $ + 1M di cache a 0,30 $ = 18,30 $ = 18 300 000 micro.
        assert_eq!(call.cost_micros, Some(18_300_000));
        assert_eq!(call.price_currency.as_deref(), Some("USD"));
        assert_eq!(call.cached_price_micros_per_million, Some(300_000));
        // Il costo che il motore dichiara di suo sta accanto, mai al posto:
        // 0,5 $ è volutamente diverso dal conto del listino.
        assert_eq!(call.declared_cost_micros, Some(500_000));
        assert_eq!(call.error_type, None);
        assert!(call.ended_at.is_some());

        // E l'uscita del passo è il testo, non l'involucro.
        assert_eq!(output["stdout"], "la risposta vera");
    }

    /// **IL CRITERIO 3, DALLA PARTE IN CUI SI ROMPE.** Se la cache fosse contata
    /// al prezzo dell'ingresso invece che al suo, questo costo verrebbe dieci
    /// volte più caro sulla parte della cache. La prova sopra fissa il numero;
    /// questa dice perché quel numero e non un altro.
    #[test]
    fn cache_priced_as_input_would_cost_ten_times_more() {
        let solo_cache = models::pricing::cost_micros(
            models::pricing::TokenCounts {
                input: Some(0),
                output: Some(0),
                cached: Some(1_000_000),
                ..models::pricing::TokenCounts::default()
            },
            models::pricing::PriceList::parse(LISTINO)
                .unwrap()
                .find("prova")
                .unwrap()
                .micros(),
        );
        assert_eq!(solo_cache, Some(300_000), "1M di cache costa 0,30 $");
        assert!(
            solo_cache.unwrap() * 5 < 3_000_000,
            "e non i 3,00 $ che costerebbe come ingresso fresco"
        );
    }

    // ── (b) chi non dichiara niente: identico, e sconosciuto ───────────

    /// **IL CRITERIO 4.** Un motore senza blocco `usage` produce la stessa
    /// identica uscita di prima, e la sua riga porta i token a SCONOSCIUTO.
    /// Mai zero: uno zero si somma, e nessuna vista a valle può correggerlo.
    #[test]
    fn an_engine_that_declares_nothing_is_unchanged_and_leaves_the_tokens_unknown() {
        let dir = scratch("non-dichiara");
        let listino = write_listino(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(AskRecipe {
                args: Vec::new(),
                prompt: PromptVia::Stdin,
                unusable_when: Vec::new(),
                usage: None,
            }),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let outcome = with_listino(Some(&listino), || {
            action.execute(&input, &mut shared("corsa-2", "passo-2"))
        })
        .expect("il motore risponde");
        let ActionOutcome::Went(output) = outcome else {
            panic!("un motore che risponde è sempre Went")
        };

        // Stessa uscita di sempre: nessun campo in più, nessun involucro.
        assert_eq!(output["status"], "ok");
        assert_eq!(output["stdout"], "la risposta vera");
        assert_eq!(
            output.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["status", "stdout", "stderr"],
            "l'uscita del passo non guadagna campi perché qualcuno misura"
        );

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "la chiamata si registra comunque");
        let call = &calls[0];
        assert_eq!(call.input_tokens, None, "sconosciuto, non zero");
        assert_eq!(call.output_tokens, None, "sconosciuto, non zero");
        assert_eq!(call.cached_tokens, None, "sconosciuto, non zero");
        assert_eq!(call.total_tokens, None);
        assert_eq!(call.cost_micros, None, "senza token non c'è nessun costo");
        assert_eq!(call.actual_model, "", "nessun modello dichiarato");
    }

    // ── (c) una chiamata fallita scrive comunque la sua riga ───────────

    /// **IL CRITERIO 5.** Un motore uscito in errore scrive la sua riga con la
    /// causa: un turno interrotto brucia comunque la quota, e azzerarne il
    /// costo sottostimerebbe la spesa proprio nei minuti che precedono un
    /// esaurimento.
    #[test]
    fn a_failed_call_still_writes_its_row_with_the_cause() {
        let dir = scratch("fallita");
        let bin = fake_engine(&dir, "motore", "cat > /dev/null\necho 'è andata male' >&2\nexit 3");
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_listino(None, || {
            action.execute(&input, &mut shared("corsa-3", "passo-3"))
        })
        .expect_err("un'uscita diversa da zero rompe il passo");
        assert_eq!(error.class, "engine_exit_error");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "anche un fallimento lascia la sua riga");
        assert_eq!(calls[0].error_type.as_deref(), Some("exit_error"));
        assert_eq!(calls[0].cli, "motore-di-prova");
        assert_eq!(calls[0].input_tokens, None, "non ha fatto in tempo a dirlo");
    }

    /// Un motore che non parte lascia comunque traccia, con la causa sua: senza
    /// questa riga una catena che ripiega sembrerebbe aver scelto il secondo
    /// motore per primo.
    #[test]
    fn an_engine_that_never_starts_leaves_its_own_row_too() {
        let dir = scratch("mai-partito");
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: "/nessun/binario/qui-di-sicuro".to_owned(),
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_listino(None, || {
            action.execute(&input, &mut shared("corsa-4", "passo-4"))
        })
        .expect_err("un binario che non c'è rompe il passo");
        assert_eq!(error.class, "engine_spawn_failed");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].error_type.as_deref(), Some("spawn_failed"));
    }

    // ── l'uscita del passo non cambia perché si misura ─────────────────

    /// **IL VINCOLO CHE NESSUNO HA CHIESTO E CHE ROMPEREBBE UN FLUSSO VERO.**
    /// `flows/come-lo-risolvono-gli-altri.flow.json` dichiara `allow_extra: false`
    /// sulla forma della risposta di un passo motore. Se chiedere l'involucro
    /// lasciasse l'involucro dentro `stdout`, quel flusso diventerebbe rosso per
    /// una misura che non ha chiesto. Qui si guarda che il testo che esce sia
    /// **identico** con e senza la misura accesa.
    #[test]
    fn asking_for_a_json_envelope_does_not_change_what_the_step_receives() {
        let dir = scratch("involucro");
        let listino = write_listino(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let senza = {
            let ledger = Ledger::open(dir.join("senza")).expect("deposito");
            let action = ExternalEngineAction::resolving_with(Declares {
                bin: bin.clone(),
                recipe: Some(AskRecipe {
                    args: Vec::new(),
                    prompt: PromptVia::Stdin,
                    unusable_when: Vec::new(),
                    usage: None,
                }),
            })
            .recording_to(Some(ledger));
            let ActionOutcome::Went(output) = with_listino(Some(&listino), || {
                action.execute(&input, &mut shared("corsa-5", "passo-5"))
            })
            .expect("risponde") else {
                panic!("Went")
            };
            output
        };

        let con = {
            let ledger = Ledger::open(dir.join("con")).expect("deposito");
            let action = ExternalEngineAction::resolving_with(Declares {
                bin,
                recipe: Some(declaring_recipe()),
            })
            .recording_to(Some(ledger));
            let ActionOutcome::Went(output) = with_listino(Some(&listino), || {
                action.execute(&input, &mut shared("corsa-6", "passo-6"))
            })
            .expect("risponde") else {
                panic!("Went")
            };
            output
        };

        assert_eq!(
            senza, con,
            "misurare non deve cambiare di una virgola ciò che il passo consegna a valle"
        );
        // E la misura c'è stata davvero: senza questo, la prova passerebbe anche
        // se il blocco `usage` non fosse mai arrivato al punto di invocazione.
        assert_eq!(
            calls_in(&dir.join("con"))[0].input_tokens,
            Some(1_000_000),
            "l'involucro è stato chiesto e letto"
        );
        assert_eq!(calls_in(&dir.join("senza"))[0].input_tokens, None);
    }

    // ── senza il posto dove scrivere, non si scrive niente ─────────────

    /// Una riga attribuita a nessuno sporcherebbe le somme peggio di una riga
    /// mancante: senza deposito, o senza corsa, non si registra.
    #[test]
    fn without_a_ledger_or_without_a_run_nothing_is_written() {
        let dir = scratch("senza-appigli");
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let recipe = declaring_recipe();

        // Senza deposito: il passo funziona lo stesso.
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(recipe.clone()),
        });
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});
        assert!(action
            .execute(&input, &mut shared("corsa-7", "passo-7"))
            .is_ok());

        // Col deposito ma senza la chiave della corsa: nessuna riga.
        let ledger = Ledger::open(dir.join("deposito")).expect("deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let mut solo_il_passo = SharedState::new();
        solo_il_passo.insert(flow::CURRENT_STEP.to_owned(), json!("passo-8"));
        assert!(action.execute(&input, &mut solo_il_passo).is_ok());
        assert!(
            calls_in(&dir.join("deposito")).is_empty(),
            "senza corsa non si attribuisce nessuna spesa a nessuno"
        );
    }

    /// Un `bin` scritto a mano nel passo non è una chiamata a un modello:
    /// `sh -c echo` non consuma nessuna quota, e riempirne il deposito
    /// renderebbe illeggibile la vista che questo lavoro esiste per rendere
    /// leggibile.
    #[test]
    fn a_hand_written_bin_is_not_a_model_call() {
        let dir = scratch("bin-a-mano");
        let ledger = Ledger::open(dir.join("deposito")).expect("deposito");
        let action = ExternalEngineAction::new().recording_to(Some(ledger));
        let input = json!({"bin": "echo", "args": ["ciao"], "timeout_secs": 10});

        assert!(action
            .execute(&input, &mut shared("corsa-9", "passo-9"))
            .is_ok());
        assert!(calls_in(&dir.join("deposito")).is_empty());
    }

    /// Le opzioni scritte dal passo vincono, e con loro il consumo resta
    /// sconosciuto: allungare alle spalle di chi ha scritto quella riga di
    /// comando una domanda che non ha fatto sarebbe decidere al posto suo. La
    /// riga però si scrive, e dice proprio questo.
    #[test]
    fn when_the_step_writes_its_own_args_the_usage_is_not_asked_for() {
        let dir = scratch("args-del-passo");
        let listino = write_listino(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({
            "tool": "motore-di-prova", "args": ["--a-modo-mio"],
            "stdin": "ciao", "timeout_secs": 10
        });

        let ActionOutcome::Went(output) = with_listino(Some(&listino), || {
            action.execute(&input, &mut shared("corsa-10", "passo-10"))
        })
        .expect("risponde") else {
            panic!("Went")
        };

        assert_eq!(output["stdout"], "la risposta vera");
        // **IL BRACCIO CHE CONTA**: la riga di comando è ESATTAMENTE quella che
        // il passo ha scritto. Accodarci le opzioni del consumo sarebbe
        // allungare alle spalle di chi l'ha scritta una domanda che non ha
        // fatto, e da fuori non si vedrebbe: solo guardando l'argv del processo
        // la differenza salta fuori.
        assert_eq!(
            argv_of(&dir),
            vec!["--a-modo-mio".to_owned()],
            "nessuna opzione aggiunta di nascosto"
        );
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "la chiamata si registra lo stesso");
        assert_eq!(calls[0].input_tokens, None, "ma non misurata");
    }

    /// **IL VINCOLO DI INDIPENDENZA DAL MODELLO, NEL PUNTO IN CUI SI ROMPE.**
    /// Un motore che non dichiara `usage` resta non misurato ANCHE SE la sua
    /// uscita è un involucro JSON con dentro chiavi che qualcuno riconoscerebbe.
    /// Se il codice avesse un ramo cablato su un fornitore — «se somiglia a
    /// questo, leggi qui» — questa prova diventerebbe rossa, e deve.
    #[test]
    fn output_that_merely_looks_familiar_is_not_read_without_a_declaration() {
        let dir = scratch("nessun-ramo-cablato");
        let listino = write_listino(&dir);
        let bin = fake_engine(&dir, "motore", ALWAYS_WRAPS);
        let ledger = Ledger::open(dir.join("deposito")).expect("deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(AskRecipe {
                args: Vec::new(),
                prompt: PromptVia::Stdin,
                unusable_when: Vec::new(),
                usage: None,
            }),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let ActionOutcome::Went(output) = with_listino(Some(&listino), || {
            action.execute(&input, &mut shared("corsa-11", "passo-11"))
        })
        .expect("risponde") else {
            panic!("Went")
        };

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].input_tokens, None,
            "quei numeri ci sono, ma nessun descrittore ha detto di leggerli"
        );
        assert_eq!(calls[0].cost_micros, None);
        assert_eq!(calls[0].actual_model, "");
        // E l'uscita del passo è quella grezza: senza `answer` dichiarato non
        // si spacchetta niente, perché nessuno ha detto dove guardare.
        assert!(
            output["stdout"].as_str().unwrap().starts_with('{'),
            "l'involucro resta tale e quale: {}",
            output["stdout"]
        );
    }
}
