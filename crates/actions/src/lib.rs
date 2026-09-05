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

pub mod apply;
pub mod budget;
pub mod cooldown;
pub mod faults;
pub mod handoff;
pub mod history;
pub mod mcp;
pub mod draft;
pub mod presence;
pub mod search;
pub mod store;
pub mod terminals;

/// I tipi puri con cui un descrittore dichiara dove stanno i suoi numeri,
/// ri-esportati da qui.
///
/// **PERCHÉ RI-ESPORTATI E NON RIDEFINITI.** `toolbox` deve poter costruire una
/// ricetta senza dipendere a sua volta da `models`, e una copia di questi tipi
/// da questa parte del confine sarebbe una seconda definizione della stessa
/// cosa: due strutture gemelle divergono al primo campo che qualcuno aggiunge a
/// una sola delle due.
pub use models::usage::{read_declared, read_scalar, read_text, Declared, Pointer, Reading, Shape};

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies, ValueSchema};
use ledger::{EngineIdentity, Ledger, ModelCallRecord};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
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
    apply::register_apply_patch(registry);
    mcp::register_mcp(registry);
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

/// Accende il figlio in un **gruppo di processi suo**, per uccidere alla
/// scadenza lui e la sua discendenza in un colpo solo.
///
/// Prezzo dichiarato: fuori dal nostro gruppo non riceve il Ctrl-C del
/// terminale. A rimetterlo in riga sono il tetto, che ora tronca davvero, e il
/// sorvegliante che raccoglie i rimasti in piedi.
#[cfg(unix)]
fn spawn_in_its_own_group(cmd: &mut Command) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0).spawn()
}

#[cfg(not(unix))]
fn spawn_in_its_own_group(cmd: &mut Command) -> std::io::Result<std::process::Child> {
    cmd.spawn()
}

/// Uccide il figlio **e chi ha acceso lui**, poi lo raccoglie.
///
/// Il gruppo porta il numero del capogruppo: il segno meno lo dice a `kill`.
/// Limite noto: un nipote che si è staccato da solo con `setsid` esce dal
/// gruppo e sopravvive — lì non arriva nessun segnale nostro.
fn kill_the_whole_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
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
    let mut child = match spawn_in_its_own_group(&mut cmd) {
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
    let mut child = match spawn_in_its_own_group(&mut cmd) {
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

/// La prima pausa fra due `try_wait`. Piccola apposta: un comando di shell che
/// dura cinque millisecondi veniva comunque atteso cinquanta, e in un flusso di
/// molti passi brevi quella era latenza pura, pagata a ogni passo.
const FIRST_POLL_PAUSE: Duration = Duration::from_millis(1);

/// Dove la crescita si ferma. **Non un millisecondo sopra i cinquanta di
/// prima**: su un figlio che dura minuti il numero di risvegli resta quello di
/// sempre, e lo scarto massimo fra la scadenza del tetto di tempo e il `kill`
/// che ne discende non peggiora di niente rispetto a ieri.
const MAX_POLL_PAUSE: Duration = Duration::from_millis(50);

/// Raddoppia fino al tetto e lì resta.
///
/// **STA FUORI DAL CICLO PERCHÉ SI POSSA GUARDARE DA SOLA.** Dentro allo
/// `scope`, in mezzo al `try_wait` e all'uccisione, sarebbe una riga che nessuno
/// può interrogare senza avviare un processo.
fn next_poll_pause(current: Duration) -> Duration {
    (current * 2).min(MAX_POLL_PAUSE)
}

fn drain_and_wait(
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    child: &mut std::process::Child,
    limit: Duration,
    sink: Option<&dyn LiveSink>,
) -> RunOutcome {
    drain_and_wait_paced(stdout, stderr, child, limit, sink, &mut |how_long| {
        std::thread::sleep(how_long)
    })
}

/// Il corpo vero, con la pausa passata da fuori.
///
/// **PERCHÉ LA PAUSA È UN PARAMETRO E NON UNA `sleep` CABLATA.** La sola cosa
/// che distingue questo ciclo da quello di prima è la *sequenza* delle durate
/// che chiede: `1ms, 2ms, 4ms…` invece di `50ms, 50ms…`. Cronometrare da fuori
/// per vederla non funziona — una macchina carica può solo allungare i tempi,
/// mai accorciarli, quindi la prova o mente quando la macchina è occupata o si
/// dà un margine così largo da non distinguere più i due codici. È la stessa
/// trappola che il guasto 7 ha già respinto. Qui invece chi prova osserva la
/// sequenza, che il codice decide e l'orologio non tocca.
fn drain_and_wait_paced(
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    child: &mut std::process::Child,
    limit: Duration,
    sink: Option<&dyn LiveSink>,
    pause: &mut dyn FnMut(Duration),
) -> RunOutcome {
    let mut out_pipe = stdout.expect("stdout is piped");
    let mut err_pipe = stderr.expect("stderr is piped");
    // `scope` e non `spawn`: i fili prendono in prestito il destinatario, che
    // vive nello stack di chi ha chiamato. Con fili staccati l'API pretenderebbe
    // un `'static` — cioè un `Arc` — da chiunque voglia guardare, compresa una
    // prova che cattura una variabile locale.
    std::thread::scope(|scope| {
        let out_thread = scope.spawn(move || drain(&mut out_pipe, Pipe::Stdout, sink));
        let err_thread = scope.spawn(move || drain(&mut err_pipe, Pipe::Stderr, sink));
        let start = Instant::now();
        let mut poll_pause = FIRST_POLL_PAUSE;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if start.elapsed() >= limit {
                        kill_the_whole_group(child);
                        break None;
                    }
                    pause(poll_pause);
                    poll_pause = next_poll_pause(poll_pause);
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
    /// Dove gira la verifica.
    ///
    /// **PRIMA DEL 31/08/2026 NON C'ERA, E IL DIFETTO ERA INVISIBILE**: una
    /// verifica girava sempre dove sta il processo, cioè dove capita che sia
    /// stata lanciata la finestra o il terminale. Un `cargo test` che passa
    /// perché è stato eseguito nell'albero sbagliato non fallisce: dice di sì.
    /// La gemella `EngineInvocation` questo campo ce l'aveva già, e la
    /// differenza fra le due non era una scelta.
    pub workdir: Option<String>,
}

/// Asimmetrico di proposito: chi passa non ha niente da spiegare, chi fallisce
/// sì — e senza queste due righe un passo rosso non lascia in mano a nessuno il
/// motivo, perché l'uscita tipata di un passo rotto non si scrive.
pub enum CheckResult {
    /// **L'USCITA VIAGGIA CON L'ESITO.** Prima qui non c'era: `run_with_timeout`
    /// la catturava e questa conversione la scartava con un `..`, quindi un
    /// comando poteva dire qualcosa e nessuno poteva riceverlo. La porta solo
    /// il ramo riuscito: un comando fallito non ha prodotto la lettura che gli
    /// è stata chiesta, e offrirla lì vorrebbe dire leggere da uno strumento
    /// rotto.
    Passed {
        stdout: String,
    },
    Failed {
        code: Option<i32>,
        stderr: String,
    },
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
    if let Some(workdir) = &invocation.workdir {
        cmd.current_dir(workdir);
    }
    match run_with_timeout_watched(cmd, invocation.timeout, sink) {
        RunOutcome::Finished {
            status,
            stdout,
            stderr,
        } => {
            if status.success() {
                CheckResult::Passed {
                    stdout: String::from_utf8_lossy(&stdout).into_owned(),
                }
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

    /// Whether what is sent to `id` trains its provider's next model. The
    /// default is what nobody measured, and a private step reads it as a no.
    fn data_pact(&self, _id: &str) -> models::pact::DataPact {
        models::pact::DataPact::Unknown
    }

    /// The subscription windows of `id` as fuel, read now; empty when it
    /// declares no channel or the reading failed.
    fn fuel(&self, _id: &str) -> Vec<models::fuel::Fuel> {
        Vec::new()
    }

    /// Come **questo** motore apre, riprende e ramifica una sessione, se lo sa
    /// fare.
    ///
    /// **IL PREDEFINITO È `None`, E QUEL `None` È IL VINCOLO PERMANENTE.** Un
    /// motore che non sa riprendere non diventa un errore e non diventa un ramo
    /// `if` scritto per lui: riceve la riga di comando di sempre, riparte da
    /// zero, e paga di più. È l'unica forma che «indipendenza dal modello»
    /// può prendere qui — la capacità è un dato di chi la dichiara, non una
    /// costante scritta accanto al codice che la userebbe.
    fn session_recipe(&self, _id: &str) -> Option<SessionRecipe> {
        None
    }

    /// Come si chiede a `id` se la casa da cui parte è autenticata.
    ///
    /// **`None` VUOL DIRE «NESSUNO HA GUARDATO», MAI «È AUTENTICATO».** Chi non
    /// la dichiara non fa scattare nessun avviso e non ne fa scattare nemmeno
    /// uno tranquillizzante: il controllo tace su quel motore, e chi legge sa
    /// che tace. È la stessa regola di `refuses_without_prompt`, e il verso
    /// conta — un predefinito che dicesse di sì renderebbe silenziosa proprio la
    /// condizione che questo canale esiste per rendere visibile.
    fn login_recipe(&self, _id: &str) -> Option<LoginRecipe> {
        None
    }
}

/// Il segnaposto che, dentro le opzioni di una ricetta di sessione, prende il
/// posto dell'identificativo della sessione.
///
/// Sta qui e non in `toolbox` perché è **il contratto fra i due**: chi scrive
/// un file di capacità e chi monta la riga di comando devono nominare la stessa
/// cosa, e due costanti gemelle in due crate divergono al primo che la cambia.
pub const SESSION_PLACEHOLDER: &str = "{session}";

/// Cosa un motore sa fare con le proprie sessioni, in opzioni già scritte.
///
/// **OGNI MODO PORTA LA RIGA INTERA, NON LE OPZIONI IN PIÙ.** Sembra una
/// duplicazione di `AskRecipe::args` e non lo è: su `codex` riprendere non è
/// un'opzione aggiunta, è **un sottocomando diverso** — `codex exec resume
/// <id>` contro `codex exec` — e su `codex` ramificare è un terzo sottocomando
/// ancora, `codex exec fork <id>`. Un modello «aggiungi queste opzioni» non
/// saprebbe esprimere nessuno dei due, e li escluderebbe entrambi per sempre.
/// Verificato il 31/08/2026 con `codex exec --help` su questa macchina.
///
/// Ciò che resta condiviso con la ricetta della domanda resta condiviso: le
/// opzioni del consumo e quelle che devono stare attaccate alla domanda si
/// accodano qui come si accodano là, perché **misurare non deve smettere di
/// funzionare quando si riprende** — sarebbe il modo più elegante di perdere
/// proprio i numeri che dicono se la ripresa conviene.
///
/// `None` su un modo vuol dire che quel motore non lo sa fare: si riparte da
/// zero, e si paga di più.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionRecipe {
    /// Apre una sessione. Se la riga contiene il segnaposto, l'identificativo
    /// lo scegliamo noi; se non lo contiene, lo conia il motore e lo si va a
    /// leggere con `id_from`.
    pub open: Option<Vec<String>>,
    /// Riprende una sessione esistente, che resta la stessa.
    pub resume: Option<Vec<String>>,
    /// Ramifica una sessione esistente: il tronco resta dov'è, e il lavoro di
    /// questo passo non lo tocca.
    pub fork: Option<Vec<String>>,
    /// Dove, in ciò che il motore ha detto, sta l'identificativo della sessione
    /// **che ha appena usato**.
    ///
    /// **SERVE PERCHÉ NON TUTTI LASCIANO SCEGLIERE IL NOME, ED È LA MAGGIORANZA.**
    /// Verificato il 31/08/2026: `codex` non ha nessuna opzione per imporre un
    /// identificativo, ma lo **stampa** — `session id: <uuid>` — nello stesso
    /// flusso di testo da cui il suo descrittore legge già i token. Senza
    /// questa via, i motori che coniano da sé sarebbero esclusi per sempre da
    /// una capacità che hanno.
    ///
    /// **E VALE ANCHE DOPO UNA RAMIFICAZIONE**, che è dove rende di più: un
    /// ramo nasce con un identificativo nuovo che nessuno ci ha chiesto, e
    /// leggerlo è l'unico modo perché un passo ancora più avanti possa
    /// continuare **quel ramo** invece del tronco.
    pub id_from: Option<Pointer>,
}

/// Dove va a finire il testo della domanda quando si interroga un motore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptVia {
    /// Sull'ingresso standard.
    Stdin,
    /// Come ultimo argomento della riga di comando.
    LastArg,
}

/// La riga di comando di una ricetta, **senza** il testo della domanda.
///
/// L'ordine è: le opzioni della domanda, quelle che servono a farsi dire il
/// consumo, e per ultime quelle che devono restare attaccate alla domanda.
///
/// **STA FUORI DAL PUNTO CHE LA USA PERCHÉ SI POSSA GUARDARE SENZA ESEGUIRE
/// NIENTE.** Un ordine sbagliato qui non rompe la compilazione e non rompe
/// nessuna prova sui singoli blocchi: si vede solo lanciando il motore giusto,
/// che è come il guasto 1 è arrivato in produzione e come ci è tornato il
/// 31/08/2026 da un'altra porta.
pub fn command_line(recipe: &AskRecipe) -> Vec<String> {
    command_line_with(recipe, &recipe.args)
}

/// La stessa riga, con le opzioni della domanda sostituite da altre.
///
/// Serve alle sessioni: `codex exec resume <id>` non è `codex exec` con
/// qualcosa in coda, è un'altra riga. Ciò che sta **dopo** le opzioni della
/// domanda — il consumo e ciò che deve restare attaccato al testo — non cambia,
/// ed è il motivo per cui questa funzione esiste invece di lasciar montare la
/// riga a chi chiama: un motore ripreso deve continuare a dire quanto consuma.
pub fn command_line_with(recipe: &AskRecipe, ask_args: &[String]) -> Vec<String> {
    let mut args = ask_args.to_vec();
    if let Some(usage) = &recipe.usage {
        args.extend(usage.args.iter().cloned());
    }
    args.extend(recipe.args_before_prompt.iter().cloned());
    args
}

// ── la prova a secco di una riga di comando ─────────────────────────────

/// Come sta messa una riga di comando montata da un descrittore, provata
/// **senza dare la domanda**.
///
/// **PERCHÉ NON C'È UN «PASSATO/FALLITO».** Cinque esiti perché ci sono cinque
/// riparazioni diverse, e chi legge deve sapere quale gli tocca: una riga rotta
/// si corregge nel descrittore, un motore esaurito si aspetta, un descrittore
/// che tace si misura, un motore che non risponde si indaga. Metterne due sotto
/// la stessa parola manda a fare il lavoro sbagliato.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeVerdict {
    /// Il motore ha detto «mancava solo la domanda»: la riga è montata bene.
    Sound,
    /// Il motore si è lamentato di **qualcos'altro**: la riga è malformata, e
    /// le sue parole sono la diagnosi. Sul guasto 27 la frase di `agy` diceva
    /// esattamente quale bandiera aveva mangiato quale argomento — nessuna
    /// classificazione nostra avrebbe potuto dire altrettanto.
    Broken { said: String },
    /// Il motore ha detto di non poter lavorare adesso — quota, credenziali —
    /// e questo non dice niente sulla riga: si riprova quando torna.
    CannotWork { said: String },
    /// Il descrittore non dichiara come questo motore rifiuta senza domanda.
    /// **Non è «la riga è sana»**: è che nessuno ha guardato.
    NotDeclared,
    /// Nessuna risposta dentro il tetto di tempo, o processo che non è partito.
    ///
    /// Il motivo viaggia col verdetto perché le due cose si riparano in modi
    /// diversi, e un rapporto che le confondesse manderebbe a cercare un motore
    /// lento dove c'è un eseguibile che non parte.
    TimedOut { why: String },
}

/// Il verdetto su una riga provata a secco, **senza eseguire niente**.
///
/// **PERCHÉ È UNA FUNZIONE PURA E SEPARATA DA CHI ESEGUE.** Perché il giudizio
/// è la parte che si sbaglia, e una prova che debba avviare un motore vero per
/// interrogarlo non si scrive: si prova con i testi che i motori hanno detto
/// davvero, copiati una volta e poi fermi lì.
///
/// **IL VERDETTO STA NEL TESTO, NON NEL CODICE D'USCITA**, e non è una
/// preferenza. Misurato il 31/08/2026 su questa macchina: `agy` esce **2** sia
/// quando rifiuta bene («flag needs an argument: -print») sia quando la riga è
/// quella malformata del guasto 27 («--print took "--output-format" as its
/// prompt…»). Una sonda che giudicasse dall'esito vedrebbe i due casi identici,
/// e passerebbe sopra al guasto 27 esattamente come ci è passato sopra chi
/// l'ha scritto. Per questo questa funzione non riceve nemmeno il codice
/// d'uscita: non c'è modo di usarlo per sbaglio.
///
/// **L'ORDINE DI LETTURA È VINCOLANTE: PRIMA `unusable_when`.** Un motore che
/// ha finito la quota si lamenta di quello, non della riga; letto nell'ordine
/// opposto, un `claude` esaurito verrebbe dichiarato **rotto** — e chi legge
/// andrebbe a correggere un descrittore sano mentre bastava aspettare. Un
/// motore esaurito non è un motore rotto.
pub fn judge_dry_run(recipe: &AskRecipe, stdout: &str, stderr: &str) -> ProbeVerdict {
    // Le due pipe si guardano insieme: chi scrive il rifiuto su stdout e chi lo
    // scrive su stderr sono lo stesso caso, e sceglierne una sola avrebbe reso
    // il verdetto dipendente da un dettaglio che nessun descrittore dichiara.
    let said = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if says_it_cannot_work(&recipe.unusable_when, &said) {
        return ProbeVerdict::CannotWork { said };
    }
    // An engine measured to answer nothing without a question is sound when
    // stdout is empty; stderr may carry a spinner and is not read here.
    if recipe.silent_without_prompt {
        return if stdout.trim().is_empty() {
            ProbeVerdict::Sound
        } else {
            ProbeVerdict::Broken { said }
        };
    }
    if recipe
        .refuses_without_prompt
        .iter()
        .all(|mark| mark.trim().is_empty())
    {
        return ProbeVerdict::NotDeclared;
    }
    if mentions_any(&recipe.refuses_without_prompt, &said) {
        return ProbeVerdict::Sound;
    }
    ProbeVerdict::Broken { said }
}

/// Cosa ha detto un motore alla riga montata senza domanda, o perché non ha
/// detto niente.
#[derive(Clone, Debug)]
pub enum DryRun {
    Answered { stdout: String, stderr: String },
    NoAnswer { why: String },
}

/// Chi esegue la prova a secco.
///
/// **PERCHÉ UN TRATTO E NON UNA CHIAMATA DIRETTA.** Perché altrimenti ogni
/// prova su questo codice dovrebbe avviare `claude`, `codex` e `agy` veri: la
/// batteria dipenderebbe da cosa è installato su chi la esegue e da come sta
/// messa la quota di quel giorno — cioè non potrebbe venire diversa per la
/// ragione che dichiara. Con un tratto le prove iniettano quattro finti
/// eseguibili e ottengono quattro verdetti, sempre gli stessi.
pub trait DryProbe: Send + Sync {
    fn run(&self, bin: &str, args: &[String], stdin: Option<Vec<u8>>) -> DryRun;
}

/// Il tetto di tempo di una prova a secco.
///
/// **SERVE UN TETTO ESPLICITO PERCHÉ SU QUESTA MACCHINA `timeout` E `gtimeout`
/// NON ESISTONO**: verificato il 31/08/2026 con `command -v`. Chi si aspettasse
/// di poterli mettere davanti alla riga scoprirebbe il contrario solo quando un
/// motore si mette ad aspettare qualcosa e blocca il controllo di tutti gli
/// altri. Il tetto lo mette `invoke_external_engine`, che ce l'ha già.
pub const DRY_PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// La sonda vera: monta la riga e la esegue senza dare la domanda.
pub struct RealDryProbe;

impl DryProbe for RealDryProbe {
    fn run(&self, bin: &str, args: &[String], stdin: Option<Vec<u8>>) -> DryRun {
        // **LA STESSA DOTAZIONE DELLA CORSA VERA, E QUI STA TUTTO IL VALORE DEL
        // VAGLIO.** Fino al 01/09/2026 questa riga era `BTreeMap::new()`: il
        // vaglio provava il motore nella casa di chi aveva aperto il terminale —
        // autenticata — e il passo lo faceva partire in quella del profilo
        // attivo, che può non avere nessuna credenziale. `flow check` chiudeva
        // in verde e la corsa falliva, e chi aveva letto il verde non aveva
        // sbagliato niente. Un controllo che prova un mondo diverso da quello in
        // cui si lavora è peggio di nessun controllo, perché rassicura.
        //
        // **NIENTE DALLO SPAZIO DI UN PASSO, ED È DELIBERATO.** Il vaglio non
        // sta provando un passo: sta provando la riga che il **descrittore**
        // monta, quella sola volta per motore. Le variabili che un passo
        // dichiara valgono per quella chiamata lì, e infilarle qui darebbe un
        // verdetto che non vale per gli altri passi che nominano lo stesso
        // motore.
        //
        // **QUESTO NON DICE SE LA CASA È AUTENTICATA, ED È UN LIMITE DELLA
        // TECNICA.** Il vaglio toglie la domanda apposta, quindi il motore si
        // ferma sulla domanda mancante e non arriva mai ai controlli che
        // verrebbero dopo — le credenziali stanno di là. Rimisurato il
        // 01/09/2026 nelle due case: `codex exec < /dev/null` risponde **la
        // stessa cosa** — «No prompt provided via stdin.» — e esce 1 tutte e due
        // le volte. (Fino a quella misura questa riga diceva «esce zero»: il
        // numero era falso, l'identità delle due risposte no, ed è quella che
        // porta la conclusione.)
        //
        // **LA DOMANDA CHE MANCA SI FA A PARTE, E ADESSO ESISTE**: è
        // `probe_login_status`, che chiede al motore con le parole che il
        // descrittore dichiara in `login_status`. Non va infilata qui: questa
        // sonda prova *la riga*, e mescolare i due verdetti renderebbe
        // impossibile dire quale dei due ha detto di no.
        let equipment = current_equipment_for(bin, &BTreeMap::new());
        let result = invoke_external_engine(&EngineInvocation {
            bin: bin.to_owned(),
            args: args.to_vec(),
            env: equipment.env,
            workdir: None,
            stdin,
            timeout: DRY_PROBE_TIMEOUT,
        });
        match result {
            // Un rifiuto è un'uscita non-zero, quindi il caso normale sta qui;
            // ma un motore che esce **zero** senza domanda è a maggior ragione
            // qualcosa da guardare, e buttarlo via lo nasconderebbe.
            EngineResult::Ok { stdout, stderr }
            | EngineResult::ExitError { stdout, stderr, .. } => DryRun::Answered { stdout, stderr },
            EngineResult::TimedOut => DryRun::NoAnswer {
                why: format!(
                    "nessuna risposta entro {} secondi",
                    DRY_PROBE_TIMEOUT.as_secs()
                ),
            },
            EngineResult::SpawnFailed { reason } => DryRun::NoAnswer {
                why: format!("the process did not start: {reason}"),
            },
        }
    }
}

/// Monta la riga di una ricetta senza la domanda, la fa provare, e giudica.
///
/// **COME SI TOGLIE LA DOMANDA DIPENDE DA DOVE ANDAVA**, ed è la sola parte del
/// montaggio che questa funzione decide: a chi la vuole sull'ingresso si dà un
/// ingresso **vuoto e chiuso** — che è ciò che fa `< /dev/null` — e a chi la
/// vuole come ultimo argomento si dà la riga senza quell'argomento. Sbagliare
/// qui non darebbe un errore: darebbe un motore che *aspetta*, e la prova a
/// secco diventerebbe un modo per appendere il controllo.
pub fn probe_dry_run(probe: &dyn DryProbe, bin: &str, recipe: &AskRecipe) -> ProbeVerdict {
    let args = command_line(recipe);
    let stdin = match recipe.prompt {
        PromptVia::Stdin => Some(Vec::new()),
        PromptVia::LastArg => None,
    };
    match probe.run(bin, &args, stdin) {
        DryRun::Answered { stdout, stderr } => judge_dry_run(recipe, &stdout, &stderr),
        DryRun::NoAnswer { why } => ProbeVerdict::TimedOut { why },
    }
}

// ── la casa è autenticata? lo dice il motore ─────────────────────────────

/// Come si chiede a un motore **se la casa da cui parte è autenticata**, e con
/// quali parole risponde di sì e di no.
///
/// **PERCHÉ NON SI GUARDA IL DISCO.** Cercare `auth.json` sarebbe una seconda
/// copia della verità, da riscrivere per ogni motore e da tenere allineata a
/// mano mentre i motori cambiano dove mettono le cose. Chi sa rispondere è il
/// motore; il descrittore dichiara soltanto **come si chiede** e **come si
/// riconosce la risposta** — la stessa disciplina di `unusable_when` e
/// `refuses_without_prompt`, applicata a una terza domanda.
///
/// **PERCHÉ SERVE UN CANALE A SÉ, E IL VAGLIO A SECCO NON BASTA.** `flow check`
/// prova la riga **senza la domanda**: il motore si ferma su «non mi hai dato
/// niente da fare» e non arriva mai ai controlli che vengono dopo, dove stanno
/// le credenziali. Misurato il 01/09/2026 nelle due case: `codex exec <
/// /dev/null` risponde «No prompt provided via stdin.» ed esce 1 **in tutte e
/// due**, parola per parola la stessa cosa. È un limite della tecnica, non un
/// difetto da riparare in essa: la domanda sulle credenziali si fa a parte, e
/// costa zero perché è locale — nessun fornitore viene chiamato.
#[derive(Clone, Debug)]
pub struct LoginRecipe {
    /// Le opzioni, o il sottocomando, con cui si fa la domanda: `["login",
    /// "status"]`, `["auth", "status"]`.
    pub args: Vec<String>,
    /// Dove sta la risposta dentro ciò che il motore ha detto.
    ///
    /// **È IL PUNTATORE DI `usage`, NON UN SECONDO MECCANISMO**, e la ragione è
    /// che il problema è lo stesso: due motori dicono la stessa cosa in due
    /// forme diverse. `codex` risponde in prosa — «Logged in using ChatGPT» — e
    /// allora non c'è niente da puntare, il soggetto è tutto ciò che ha detto.
    /// `claude` risponde con un involucro JSON e mette la risposta in un campo
    /// booleano, `"loggedIn": true`, e allora il cammino di chiavi la raggiunge.
    ///
    /// `None` non è «non guardare»: è «il soggetto è l'uscita intera», che è la
    /// forma più comune e quella che non richiede di dichiarare niente.
    pub answer: Option<Pointer>,
    /// Le parole con cui questo motore dichiara di **essere** autenticato.
    pub logged_in_when: Vec<String>,
    /// Le parole con cui dichiara di **non** esserlo.
    ///
    /// **VANNO DICHIARATE TUTTE E DUE, E LA MANCANZA DI UNA SPEGNE IL
    /// CONTROLLO.** Un descrittore che sapesse riconoscere solo il sì
    /// chiamerebbe «non riconosciuto» ogni no, e chi legge non saprebbe
    /// distinguere un motore non autenticato da uno che ha risposto qualcosa di
    /// strano. Meglio tacere: vedi [`LoginVerdict::NotDeclared`].
    pub logged_out_when: Vec<String>,
}

/// Che cosa si è potuto sapere sulle credenziali di una casa.
///
/// **QUATTRO ESITI E NON DUE, PER LA RAGIONE DI SEMPRE.** «Nessuno ha guardato»,
/// «ha risposto e non l'ho capito» e «ha detto di no» sono tre fatti diversi, e
/// **nessuno dei tre è un sì**. Un tipo a due stati costringerebbe a scegliere
/// da che parte far cadere i primi due, e la direzione comoda è sempre quella
/// che tranquillizza — cioè quella che rimette il difetto.
#[derive(Clone, Debug)]
pub enum LoginVerdict {
    /// Il motore dichiara di essere autenticato in questa casa.
    LoggedIn { said: String },
    /// Il motore dichiara di **non** esserlo: le chiamate partiranno senza
    /// credenziali.
    LoggedOut { said: String },
    /// Il descrittore non dichiara il blocco, o lo dichiara a metà. **Nessuno
    /// ha guardato**, e non c'è niente da dire su questa casa.
    NotDeclared,
    /// Ha risposto, e la risposta non somiglia a nessuna delle due forme
    /// dichiarate. Le sue parole sono la diagnosi.
    Unrecognised { said: String },
    /// Non ha risposto affatto: non è partito, o ha superato il tetto di tempo.
    NoAnswer { why: String },
}

impl LoginVerdict {
    /// Vero **solo** quando il motore ha detto di sì. Ogni altro esito, dubbio
    /// compreso, risponde di no: è la forma in cui il verso dell'errore si
    /// scrive una volta sola invece che a ogni luogo di lettura.
    pub fn is_logged_in(&self) -> bool {
        matches!(self, LoginVerdict::LoggedIn { .. })
    }
}

/// Legge la risposta di un motore alla domanda «sei autenticato?».
///
/// **PURA, E SEPARATA DA CHI ESEGUE**, per la stessa ragione di
/// [`judge_dry_run`]: il giudizio è la parte che si sbaglia, e una prova che
/// dovesse lanciare `codex` direbbe com'è messa la macchina di chi la esegue
/// invece che se il riconoscimento funziona.
///
/// **IL CODICE D'USCITA NON ENTRA NEMMENO QUI.** Sui due motori misurati il
/// 01/09/2026 l'esito *distinguerebbe* — `codex login status` esce 1 non
/// autenticato e 0 autenticato, e `claude auth status` fa lo stesso — ma è un
/// fatto di quei due e non una regola che si possa scrivere nel codice: un
/// motore che rispondesse «Not logged in» uscendo zero verrebbe dichiarato
/// autenticato da chiunque leggesse l'esito, e nessuno se ne accorgerebbe. Il
/// testo lo dichiara il descrittore, l'esito no.
///
/// **L'ORDINE DI LETTURA È VINCOLANTE: PRIMA IL NO.** «Not logged in»
/// *contiene* «logged in», e in generale il modo di dire di no è il modo di dire
/// di sì con una negazione davanti. Letto nell'ordine opposto, una casa vuota
/// risulterebbe autenticata — che è precisamente il silenzio che questo blocco
/// esiste per rompere. Le parole dichiarate misurate lo eviterebbero già; questo
/// lo evita anche quando chi scrive il descrittore è stato distratto.
pub fn judge_login_status(recipe: &LoginRecipe, stdout: &str, stderr: &str) -> LoginVerdict {
    // **LE DUE PIPE INSIEME, E QUI NON È UN DETTAGLIO**: `codex login status`
    // non scrive niente su stdout — la risposta è tutta su stderr, misurato il
    // 01/09/2026. Chi ne leggesse una sola non troverebbe mai nessuna delle due
    // forme e direbbe sempre «nessuno ha guardato».
    let said = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    let declared = |marks: &[String]| marks.iter().any(|mark| !mark.trim().is_empty());
    if !declared(&recipe.logged_in_when) || !declared(&recipe.logged_out_when) {
        return LoginVerdict::NotDeclared;
    }

    // Il puntatore sceglie il soggetto, e basta: le parole si cercano dentro
    // quello, con la stessa regola di `unusable_when`. Senza puntatore il
    // soggetto è ciò che il motore ha detto per intero.
    let subject = match recipe.answer.as_ref() {
        None => Some(said.clone()),
        Some(pointer) => read_scalar(&said, pointer),
    };
    // Un puntatore che non trova niente non è un sì: l'involucro non era quello
    // che il descrittore dichiarava, e la risposta resta sconosciuta.
    let Some(subject) = subject else {
        return LoginVerdict::Unrecognised { said };
    };

    // **SI MOSTRA IL SOGGETTO, NON L'INVOLUCRO CHE LO CONTENEVA.** La risposta
    // vera di `claude auth status` porta con sé l'indirizzo di posta del
    // proprietario, l'identificativo e il nome della sua organizzazione e il
    // tipo di abbonamento; questo testo finisce in `sailor profiles list` e nel
    // rapporto di `sailor flow check`, cioè in due uscite che si incollano in
    // una consegna e si versano in un registro. **Una diagnosi non deve
    // portarsi dietro chi la usa.**
    //
    // Non si perde niente: dove un puntatore c'è, il valore che ha isolato *è*
    // la risposta — «false» è più preciso dell'involucro, non meno — e dove non
    // c'è, il soggetto è già tutto ciò che il motore ha detto. È la stessa
    // regola delle righe rotte («le parole del motore per intero») applicata a
    // un motore che risponde con un campo invece che con una frase.
    let shown = if recipe.answer.is_some() {
        subject.clone()
    } else {
        said
    };

    if mentions_any(&recipe.logged_out_when, &subject) {
        return LoginVerdict::LoggedOut { said: shown };
    }
    if mentions_any(&recipe.logged_in_when, &subject) {
        return LoginVerdict::LoggedIn { said: shown };
    }
    LoginVerdict::Unrecognised { said: shown }
}

/// Chi fa la domanda locale «sei autenticato?», **dentro una casa precisa**.
///
/// **PERCHÉ UN TRATTO A SÉ E NON [`DryProbe`].** Sono due domande diverse su due
/// mondi diversi: il vaglio a secco prova la riga nella casa del profilo attivo,
/// e chi la compone non la sceglie; questa domanda va fatta **in una casa
/// nominata** — `sailor profiles list` la fa a ogni profilo, non solo a quello
/// in forza, e con `DryProbe` non avrebbe modo di dirlo. L'ambiente è quindi un
/// argomento, non una cosa che l'esecutore va a leggersi da solo.
pub trait LoginProbe: Send + Sync {
    fn ask(&self, bin: &str, args: &[String], env: &BTreeMap<String, String>) -> DryRun;
}

/// Le due domande locali che si possono fare a un motore senza spendere.
///
/// Sta insieme perché chi controlla un flusso le fa tutte e due nello stesso
/// momento e sullo stesso mondo; separate, ogni luogo di chiamata dovrebbe
/// portarsi due argomenti che valgono sempre la stessa cosa.
pub trait EngineProbe: DryProbe + LoginProbe {}

impl<T: DryProbe + LoginProbe> EngineProbe for T {}

impl LoginProbe for RealDryProbe {
    fn ask(&self, bin: &str, args: &[String], env: &BTreeMap<String, String>) -> DryRun {
        let result = invoke_external_engine(&EngineInvocation {
            bin: bin.to_owned(),
            args: args.to_vec(),
            env: env.clone(),
            workdir: None,
            // **L'INGRESSO VUOTO E CHIUSO, CIOÈ `< /dev/null`.** Un motore che
            // si mettesse ad aspettare qualcosa dall'ingresso appenderebbe il
            // controllo di tutti gli altri: è la trappola già pagata su `codex
            // exec`, e costa un carattere evitarla.
            stdin: Some(Vec::new()),
            timeout: DRY_PROBE_TIMEOUT,
        });
        match result {
            EngineResult::Ok { stdout, stderr }
            | EngineResult::ExitError { stdout, stderr, .. } => DryRun::Answered { stdout, stderr },
            EngineResult::TimedOut => DryRun::NoAnswer {
                why: format!(
                    "nessuna risposta entro {} secondi",
                    DRY_PROBE_TIMEOUT.as_secs()
                ),
            },
            EngineResult::SpawnFailed { reason } => DryRun::NoAnswer {
                why: format!("the process did not start: {reason}"),
            },
        }
    }
}

/// Chiede a `bin`, dentro la casa che `env` dichiara, se è autenticato.
///
/// **NON COSTA NIENTE E NON CHIAMA NESSUN FORNITORE.** Misurato il 01/09/2026:
/// `codex login status` e `claude auth status` leggono un file locale e
/// rispondono. Sono l'unico modo di sapere la cosa senza andare a guardare il
/// disco al posto del motore.
pub fn probe_login_status(
    probe: &dyn LoginProbe,
    bin: &str,
    env: &BTreeMap<String, String>,
    recipe: &LoginRecipe,
) -> LoginVerdict {
    // Un descrittore che non dichiara non fa partire nessun processo: chiedere
    // per poi non saper leggere la risposta sarebbe tempo speso per niente.
    let declared = |marks: &[String]| marks.iter().any(|mark| !mark.trim().is_empty());
    if !declared(&recipe.logged_in_when) || !declared(&recipe.logged_out_when) {
        return LoginVerdict::NotDeclared;
    }
    match probe.ask(bin, &recipe.args, env) {
        DryRun::Answered { stdout, stderr } => judge_login_status(recipe, &stdout, &stderr),
        DryRun::NoAnswer { why } => LoginVerdict::NoAnswer { why },
    }
}

/// Come si interroga un motore in un colpo solo, e come quel motore dice di
/// **non poter lavorare**.
#[derive(Clone, Debug)]
pub struct AskRecipe {
    /// Le opzioni che vogliono una domanda secca, senza il testo della domanda.
    pub args: Vec<String>,
    /// Dove va il testo della domanda.
    pub prompt: PromptVia,
    /// Le opzioni che devono restare **attaccate alla domanda**, dopo quelle
    /// del consumo. Vuoto per quasi tutti; vedi `Ask::args_before_prompt`.
    pub args_before_prompt: Vec<String>,
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
    /// The words that mean the quota is spent, and how long to set the engine
    /// aside when they appear. Empty and `None` when the descriptor does not
    /// tell a spent quota from a missing credential.
    pub exhausted_when: Vec<String>,
    pub cooldown_secs: Option<u64>,
    /// Measured: without a question it exits quietly with an empty stdout
    /// instead of refusing in words.
    pub silent_without_prompt: bool,
    /// I frammenti con cui questo motore rifiuta la riga **montata senza la
    /// domanda**: «la riga andava bene, mancava solo il testo».
    ///
    /// Viaggia con la ricetta e non accanto, perché serve esattamente dove
    /// serve la riga: chi monta `command_line` per provarla a secco deve poter
    /// giudicare la risposta senza tornare a chiedere niente al catalogo.
    /// Vuoto vuol dire «nessuno ha guardato», mai «la riga è sana».
    pub refuses_without_prompt: Vec<String>,
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

/// Se questa uscita contiene una delle parole dichiarate. Il confronto ignora
/// maiuscole e minuscole: nessun fornitore promette di non cambiarle. Un
/// frammento vuoto non conta — combacerebbe con tutto, e trasformerebbe
/// qualunque uscita in una corrispondenza.
///
/// **STA QUI IN UNA COPIA SOLA** perché i due elenchi che un descrittore
/// dichiara — «non posso lavorare» e «mancava la domanda» — si leggono nello
/// stesso identico modo. Due funzioni gemelle divergerebbero sul primo
/// dettaglio che qualcuno cambia a una sola delle due, ed è il guasto 10.
fn mentions_any(marks: &[String], output: &str) -> bool {
    let output = output.to_lowercase();
    marks
        .iter()
        .any(|mark| !mark.trim().is_empty() && output.contains(&mark.to_lowercase()))
}

/// Se questa uscita è il modo in cui un motore dice di non poter lavorare.
fn says_it_cannot_work(marks: &[String], output: &str) -> bool {
    mentions_any(marks, output)
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
                    "`accept` names «{name}», which this step cannot produce; the possible values are: {}",
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
                "the step declares a shape for the answer and at the same time tolerates «{silent}», which leaves no answer at all: the two do not go together"
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
            "the step demands an answer in a declared shape, and that shape does not appear in what it sends the engine: put it in the prompt with a {} reference to /answer_shape, so it is written once. The shape is: {written}",
            flow::reference::JSON_KEY
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
        return "it said nothing, on stdout or on stderr".to_owned();
    }
    parts.join("\n")
}

/// `None` non è «uscito con zero»: è un processo ucciso da un segnale, e
/// confonderli manda a cercare un guasto nel posto sbagliato.
fn how_it_exited(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("it exited with code {code}"),
        None => "it was killed by a signal".to_owned(),
    }
}

/// Cosa un passo chiede alla sessione del motore.
///
/// **UN PASSO NOMINA UN PASSO, NON UN IDENTIFICATIVO.** L'identificativo di una
/// sessione nasce mentre la corsa gira; chi scrive un flusso non lo può
/// conoscere, e un flusso che lo contenesse varrebbe per una corsa sola. Si
/// scrive quindi da chi si continua — `{"fork": "scopri"}` — e l'identificativo
/// lo va a cercare il deposito, che è il posto dove il passo `scopri` l'ha
/// posato.
///
/// **RIPRENDERE E RAMIFICARE NON SONO LA STESSA COSA, E CONFONDERLE COSTA.**
/// Chi riprende continua la sessione: due passi che riprendessero lo stesso
/// tronco si scriverebbero addosso a vicenda, e in un fronte parallelo
/// l'ordine con cui lo fanno non è deciso da nessuno. Chi ramifica parte dallo
/// stesso contesto e prosegue per conto suo: è il modo giusto per tre passi
/// indipendenti che guardano lo stesso albero, ed è il caso che rende di più —
/// la scoperta si paga una volta invece di tre.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SessionUse {
    /// `"session": "open"` — apre una sessione nuova e la registra, così i
    /// passi dopo possono continuarla.
    Open,
    /// `"session": {"resume": "scopri"}` — continua la sessione del passo
    /// nominato.
    Resume(String),
    /// `"session": {"fork": "scopri"}` — parte dal contesto del passo nominato
    /// senza toccarlo.
    Fork(String),
}

impl SessionUse {
    /// Il modo, detto a parole per chi guarda.
    fn word(&self) -> &'static str {
        match self {
            SessionUse::Open => "open a session",
            SessionUse::Resume(_) => "resume a session",
            SessionUse::Fork(_) => "fork a session",
        }
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

/// **PERCHÉ QUESTA STRUTTURA RACCOGLIE CIÒ CHE NON CONOSCE INVECE DI SCARTARLO.**
///
/// Il 30/08/2026 un flusso di prova scriveva `"prompt"` dove va `"stdin"`. Il
/// passo è partito lo stesso, il motore ha ricevuto una riga di comando monca,
/// e l'errore che è tornato era suo: «Input must be provided either through
/// stdin». Una chiamata a pagamento spesa per un refuso, e nessuno che potesse
/// dirlo prima. È il guasto 20.
///
/// **`deny_unknown_fields` NON È LA RISPOSTA, E ROMPEREBBE TUTTO.** A tempo di
/// esecuzione l'ingresso di un passo *è* l'uscita della sua dipendenza, col
/// `with` sovrapposto: arriva quindi ogni campo che il passo prima ha prodotto.
/// Rifiutarli renderebbe impossibile ogni passo con una dipendenza — lo stesso
/// motivo per cui `toolbox::needs::NeedsSpec` lo dichiara e lo rifiuta.
///
/// Qui i campi non riconosciuti finiscono in `extra` e vengono ignorati come
/// prima. La differenza è che adesso **si possono chiedere**, e `flow check` li
/// chiede sul `with` — che è testo scritto a mano, dove un campo di troppo non è
/// l'uscita di nessuno: è un refuso.
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
    /// What the text of this step is: `private` never resolves to an engine
    /// whose data pact is `trains` or `unknown`. Absent is `public`.
    #[serde(default)]
    data: Option<DataClass>,
    /// The kind of work (`mechanical`, `research`, `implementation`,
    /// `judgement`, `writing`): the strengths table puts its engines first.
    #[serde(default)]
    kind: Option<String>,
    /// This step is given what it is handed and nothing else: no session of
    /// another step is continued, whatever it asks. Whoever writes the flow
    /// declares it — a step is not read as a judge by the words in it.
    #[serde(default)]
    blind: bool,
    /// `fuel`: among the chain, the engine whose subscription window would
    /// otherwise expire unused goes first, and the why is said.
    #[serde(default)]
    prefer: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    workdir: Option<String>,
    /// `"tree": "own"` gives this step a git worktree of the project to
    /// itself, named after the run and the step. Two steps of one front then
    /// write over each other's work only if somebody wrote the same name
    /// twice, which the graph does not allow.
    #[serde(default)]
    tree: Option<String>,
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
    /// Le capacità che questo passo chiede al motore: `response_shape`,
    /// `resume_session`, e qualunque altro nome un descrittore dichiari.
    ///
    /// **DICHIARATO QUI E NON ANCORA USATO, DI PROPOSITO.** Chi lo legge oggi è
    /// `sailor flow check`, che avvisa prima di spendere quando il motore
    /// scelto non dichiara quella capacità. L'esecuzione non cambia: chi non sa
    /// imporre una forma alla risposta continua a farsela chiedere nel prompt
    /// con `answer_shape`, e paga più token — è il vincolo permanente
    /// «indipendenza dal modello», e quel ripiego resta il ripiego.
    ///
    /// **E STA NELLA SPECIFICA PER NON DIVENTARE UN REFUSO.** I campi che
    /// questa azione non riconosce finiscono in `extra`, e il controllo li
    /// nomina come «campi che l'azione non conosce»: un passo che dichiara
    /// onestamente ciò che gli serve si vedrebbe accusare di un errore di
    /// battitura.
    ///
    /// Nessuno lo legge da qui dentro finché le azioni non useranno le
    /// capacità: il permesso è sulla riga sopra e non su tutta la struttura,
    /// così il giorno che qualcuno lo usa il permesso sparisce con lui.
    #[allow(dead_code)]
    #[serde(default)]
    needs_capabilities: Vec<String>,
    /// Se questo passo apre una sessione, ne riprende una, o ne ramifica una.
    ///
    /// Assente — il valore predefinito — vuol dire che il passo apre un
    /// processo che non sa niente di ciò che è già stato letto: è come ha
    /// sempre funzionato, ed è ciò che il 31/08/2026 è stato misurato costare
    /// 2,79 volte un prompt solo, perché quattro passi hanno riscoperto lo
    /// stesso albero quattro volte.
    #[serde(default)]
    session: Option<SessionUse>,
    timeout_secs: u64,
    /// Tutto ciò che questa azione non riconosce.
    ///
    /// A tempo di esecuzione è l'uscita della dipendenza e si ignora; a tempo di
    /// controllo, sul solo `with`, è l'elenco dei refusi. Vedi il commento sopra
    /// la struttura.
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

/// What the text of a step is, for the pact an engine must hold to read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DataClass {
    Private,
    Public,
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
        // No field declared and extras allowed: the shape says «an object,
        // whatever it holds», and pruning it would forward `{}` every time.
        (
            ValueSchema::Object {
                properties,
                allow_extra: true,
                ..
            },
            value @ Value::Object(_),
        ) if properties.is_empty() => value,
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
                "the step demands an answer in a declared shape, and what arrived is not JSON: {error}; it said: {}",
                tail(said)
            ),
        )
    })?;
    shape.validate(&value).map_err(|error| {
        ActionError::new(
            "answer_off_shape",
            format!(
                "the answer does not respect the shape the step declared ({error}); it said: {}",
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
    /// Where the engines set aside for a spent quota are listed; `None` when
    /// the machine has no home to keep the list in, and then nobody is aside.
    cooldowns: Option<PathBuf>,
    /// Where the person's spend caps per engine live; `None` means no cap.
    budgets: Option<PathBuf>,
    /// The person's strengths table, or `None` for the shipped one.
    strengths: Option<PathBuf>,
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

    /// The chain for this step: the strengths table's engines for its kind
    /// first, then the chain as the flow wrote it; a kind without a row, or
    /// a step without a kind, is the chain as written. Then, under
    /// `prefer: fuel`, the engine whose window expires unused soonest moves
    /// to the front, with the why.
    fn ordered(
        &self,
        tools: &dyn ToolResolver,
        spec: &EngineSpec,
        chain: &[String],
    ) -> (Vec<String>, Option<models::fuel::Preference>) {
        let mut ordered: Vec<String> = match spec.kind.as_deref() {
            Some(kind) => self.strengths_table().first_for(kind).to_vec(),
            None => Vec::new(),
        };
        for id in chain {
            if !ordered.contains(id) {
                ordered.push(id.clone());
            }
        }
        if spec.prefer.as_deref() != Some("fuel") {
            return (ordered, None);
        }
        let fuels: Vec<models::fuel::Fuel> = ordered.iter().flat_map(|id| tools.fuel(id)).collect();
        let preferred = models::fuel::prefer(&fuels);
        if let Some(preference) = &preferred {
            if let Some(at) = ordered.iter().position(|id| *id == preference.engine) {
                let first = ordered.remove(at);
                ordered.insert(0, first);
            }
        }
        (ordered, preferred)
    }

    fn strengths_table(&self) -> models::strengths::Strengths {
        self.strengths
            .as_deref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| models::strengths::Strengths::parse(&text).ok())
            .unwrap_or_else(models::strengths::Strengths::shipped)
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
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
                    declared_usage: None,
                    // Un comando scritto a mano non ha un descrittore: non c'è
                    // niente che dichiari che sia un motore, e infatti la riga
                    // non si scrive già per via dell'`id` assente.
                    can_be_asked: false,
                    why: None,
                    session: SessionRecipe::default(),
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
                if let Some(other) = spec.prefer.as_deref().filter(|word| *word != "fuel") {
                    return Err(ActionError::new(
                        "invalid_input",
                        format!("`prefer` knows only «fuel», not «{other}»"),
                    ));
                }
                let (ids, preferred) = self.ordered(tools.as_ref(), spec, choice.ids());
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
                for id in &ids {
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
                    // An engine set aside for a spent quota is not knocked on
                    // again before its time: the refusal says until when, and
                    // what it said, so the chain goes on with the others.
                    if let Some(aside) = self
                        .cooldowns
                        .as_deref()
                        .and_then(|path| cooldown::set_aside_until(path, id, now_secs()))
                    {
                        refused.push(Refused {
                            id: id.clone(),
                            reason: format!(
                                "set aside until {} after saying its quota was spent: «{}»",
                                aside.until, aside.said
                            ),
                            unresolved: false,
                        });
                        continue;
                    }
                    // The pact first: it is permanent, and a cap that would
                    // be named instead suggests raising it would help.
                    let pact = tools.data_pact(id);
                    if spec.data == Some(DataClass::Private) && pact != models::pact::DataPact::DoesNotTrain {
                        refused.push(Refused {
                            id: id.clone(),
                            reason: format!(
                                "a private step does not go to an engine whose data pact is «{pact}»"
                            ),
                            unresolved: false,
                        });
                        continue;
                    }
                    // A cap on a window excludes, and never reorders: the sum
                    // is the ledger's, over every run of this engine.
                    if let Some(why) = self.over_budget(id) {
                        refused.push(Refused {
                            id: id.clone(),
                            reason: why,
                            unresolved: false,
                        });
                        continue;
                    }
                    if let Some(why) = current_equipment_for(&bin, &spec.env).refused {
                        refused.push(Refused {
                            id: id.clone(),
                            reason: why,
                            unresolved: false,
                        });
                        continue;
                    }
                    // Le opzioni scritte nel passo vincono sulla ricetta: chi le
                    // ha scritte sta dicendo qualcosa di preciso su *questa*
                    // chiamata, e sovrascriverle sarebbe decidere al posto suo.
                    if step_said_args {
                        let declared = tools.ask_recipe(id);
                        usable.push(Candidate {
                            id: Some(id.clone()),
                            bin,
                            args: spec.args.clone(),
                            prompt: PromptVia::Stdin,
                            // Il descrittore dice se questo strumento è un
                            // motore, anche quando le opzioni non vengono da
                            // lui: `git` e `cargo` non dichiarano `ask`, e le
                            // loro esecuzioni non sono chiamate a un modello.
                            can_be_asked: declared.is_some(),
                            why: preferred.as_ref().filter(|p| p.engine == *id).map(|p| p.why.clone()),
                            exhausted_when: declared
                                .as_ref()
                                .map(|recipe| recipe.exhausted_when.clone())
                                .unwrap_or_default(),
                            cooldown_secs: declared.as_ref().and_then(|recipe| recipe.cooldown_secs),
                            unusable_when: declared
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
                            session: SessionRecipe::default(),
                        });
                        continue;
                    }
                    match tools.ask_recipe(id) {
                        Some(recipe) => usable.push(Candidate {
                            id: Some(id.clone()),
                            bin,
                            args: command_line(&recipe),
                            prompt: recipe.prompt,
                            session: session_lines(&recipe, tools.session_recipe(id)),
                            unusable_when: recipe.unusable_when,
                            exhausted_when: recipe.exhausted_when,
                            cooldown_secs: recipe.cooldown_secs,
                            declared_usage: recipe.usage.map(|usage| usage.declared),
                            // Siamo dentro il ramo che ha trovato una ricetta
                            // `ask`: questo strumento è per definizione un
                            // motore.
                            can_be_asked: true,
                            why: preferred.as_ref().filter(|p| p.engine == *id).map(|p| p.why.clone()),
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
                "the step declares both `bin` and `tool`: only one of the two says what to run",
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
    /// The words that mean the quota is spent, and how long to set the engine
    /// aside when they appear; the descriptor's, or empty.
    exhausted_when: Vec<String>,
    cooldown_secs: Option<u64>,
    /// Dove leggere il consumo nell'uscita di questo motore. `None` quando il
    /// descrittore non lo dichiara, o quando le opzioni le ha scritte il passo.
    declared_usage: Option<Declared>,
    /// **QUESTO STRUMENTO È UN MOTORE**, cioè il suo descrittore dichiara come
    /// gli si fa una domanda (`ask`).
    ///
    /// Serve a decidere se la sua invocazione va in `model_calls`. `git` e
    /// `cargo` stanno nel catalogo e si eseguono da un passo come tutti gli
    /// altri, ma non si interrogano: non consumano quota di nessun
    /// abbonamento, e contarli fra le chiamate ai modelli falsa ogni totale che
    /// le somma. **Il criterio è del descrittore e non di un elenco di nomi
    /// scritto qui**: un elenco a mano invecchia al primo strumento nuovo, e
    /// nessun controllo lo direbbe.
    ///
    /// Resta vero anche quando le opzioni le scrive il passo: un motore
    /// interrogato a modo proprio è sempre un motore, e la sua riga si scrive —
    /// col consumo sconosciuto, che è l'informazione giusta.
    can_be_asked: bool,
    /// Why this engine was moved to the front, when the fuel said so.
    why: Option<String>,
    /// Le righe di comando alternative con cui questo motore apre, riprende o
    /// ramifica una sessione — già montate col resto della ricetta, e ancora
    /// col segnaposto al posto dell'identificativo.
    ///
    /// Tutta vuota per chi non lo sa fare, e per chi si è scritto le opzioni
    /// nel passo: chi scrive la propria riga di comando la sta decidendo lui, e
    /// infilarci dentro un'opzione che non ha chiesto sarebbe decidere al posto
    /// suo — la stessa regola che vale già per le opzioni del consumo.
    session: SessionRecipe,
}

// ── riprendere invece di riscoprire ──────────────────────────────────────

/// Le opzioni di sessione dichiarate dal motore, montate col resto della sua
/// ricetta. Quello che il motore non dichiara resta `None` fin qui.
fn session_lines(recipe: &AskRecipe, declared: Option<SessionRecipe>) -> SessionRecipe {
    let Some(declared) = declared else {
        return SessionRecipe::default();
    };
    let line = |args: Option<Vec<String>>| args.map(|args| command_line_with(recipe, &args));
    SessionRecipe {
        open: line(declared.open),
        resume: line(declared.resume),
        fork: line(declared.fork),
        id_from: declared.id_from,
    }
}

/// Le opzioni col segnaposto sostituito dall'identificativo vero.
///
/// La sostituzione è **dentro** l'opzione, non al posto suo: `codex` vuole
/// l'identificativo come argomento a sé, `claude` pure, ma niente vieta a un
/// motore futuro di volerlo attaccato a un `--session=`.
fn with_session_id(args: &[String], id: &str) -> Vec<String> {
    args.iter()
        .map(|arg| arg.replace(SESSION_PLACEHOLDER, id))
        .collect()
}

/// Un identificativo di sessione nuovo, nella forma che le righe di comando
/// chiedono (un UUID).
///
/// **NON SERVE CHE SIA IMPREVEDIBILE, SERVE CHE SIA UNICO.** Non protegge
/// niente: nomina una conversazione sul disco di chi la esegue. Dentro un
/// processo il contatore basta da solo; fra processi diversi il seme casuale di
/// `RandomState` — che il sistema operativo dà a ogni processo — separa le
/// serie. Tirarsi dentro una dipendenza per questo violerebbe la scelta scritta
/// nel `Cargo.toml` del workspace, che di dipendenze ne tiene tre.
fn fresh_session_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    static MINTED_SO_FAR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seed = std::collections::hash_map::RandomState::new();
    let mut halves = [0u64; 2];
    for (round, half) in halves.iter_mut().enumerate() {
        let mut hasher = seed.build_hasher();
        hasher.write_u64(MINTED_SO_FAR.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        hasher.write_u64(round as u64);
        hasher.write_u32(std::process::id());
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default(),
        );
        *half = hasher.finish();
    }
    let [high, low] = halves;
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (high >> 32) as u32,
        (high >> 16) as u16,
        (high & 0x0fff) as u16,
        // La variante che un UUID deve dichiarare: due bit fissi in cima.
        ((low >> 48) as u16 & 0x3fff) | 0x8000,
        low & 0xffff_ffff_ffff
    )
}

/// The field a step declares to be given nothing it was not handed.
pub const BLIND: &str = "blind";

/// The field a step declares to work in a tree nobody else touches, and the
/// only word it takes.
pub const TREE: &str = "tree";
pub const A_TREE_OF_ITS_OWN: &str = "own";

/// The worktree this step works in, cut now if it is not there yet.
///
/// Refused rather than run in the tree everybody shares: a step that asked to
/// be alone and silently got the shared tree writes over another engine's work.
fn tree_of_its_own(
    spec: &EngineSpec,
    shared: &SharedState,
) -> Result<Option<std::path::PathBuf>, ActionError> {
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
    workspace::tree_for(std::path::Path::new(&root), &run, &step)
        .map(Some)
        .map_err(|why| ActionError::new("tree_not_cut", why))
}

/// Con quali opzioni girare, e sotto quale identificativo di sessione questa
/// chiamata risulta essere girata.
struct SessionPlan {
    /// La riga di comando della sessione. `None` vuol dire «quella di sempre»,
    /// cioè si riparte da zero.
    args: Option<Vec<String>>,
    /// Cosa scrivere nella colonna `session_id` del deposito **se il motore non
    /// dice il proprio**. Vedi il commento su `ModelCallRecord::session_id`.
    recorded: Option<String>,
    /// Dove leggere, in ciò che il motore dirà, l'identificativo vero. Quando
    /// c'è **vince su `recorded`**: la parola del motore su quale sessione ha
    /// usato batte la nostra su quale gli avevamo chiesto.
    read_id_from: Option<Pointer>,
}

impl SessionPlan {
    /// Da zero, come è sempre stato.
    fn from_scratch() -> Self {
        Self {
            args: None,
            recorded: None,
            read_id_from: None,
        }
    }

    /// L'identificativo da registrare, dopo che il motore ha parlato.
    fn session_id(&self, said: &str) -> Option<String> {
        match &self.read_id_from {
            Some(pointer) => read_text(said, pointer),
            None => self.recorded.clone(),
        }
    }
}

/// Lo dice a chi guarda mentre succede, non solo al deposito dopo.
///
/// **UN RIPIEGO MUTO È LA PEGGIORE DELLE DUE COSE**: si paga il prezzo della
/// riscoperta e non si sa di averlo pagato, e chi legge il flusso continuerà a
/// credere che quel passo riprenda. È il vincolo «chiarezza per chi guarda»
/// applicato al caso in cui l'ottimizzazione **non** scatta.
fn say_it_starts_over(live: Option<&dyn LiveSink>, named: &str, why: &str) {
    if let Some(live) = live {
        live.chunk(
            Pipe::Stderr,
            format!("[sailor] {named} riparte da zero: {why}\n").as_bytes(),
        );
    }
}

/// Decide se questa chiamata apre, riprende, ramifica, o riparte da zero.
///
/// **NON FALLISCE MAI, E LA SCELTA È IL VINCOLO.** Ogni impedimento — il motore
/// non sa riprendere, il passo prima non ha lasciato nessuna sessione, non c'è
/// un deposito dove cercarla — porta alla riga di comando di sempre. Un flusso
/// scritto su una macchina dove `claude-code` c'è deve girare su una macchina
/// dove c'è solo un motore che non sa riprendere: gira peggio, non gira meno.
fn session_plan(
    candidate: &Candidate,
    asked: Option<&SessionUse>,
    blind: bool,
    record: Option<&Recording<'_>>,
    live: Option<&dyn LiveSink>,
    named: &str,
) -> SessionPlan {
    let Some(asked) = asked else {
        return SessionPlan::from_scratch();
    };
    // Declared by whoever wrote the step, never inferred from what the step
    // looks like: a session carried in would hand it what it asked not to see.
    if blind {
        say_it_starts_over(live, named, "the step is declared blind");
        return SessionPlan::from_scratch();
    }
    let Some(record) = record else {
        // Il deposito è il posto dove una sessione si posa e si ritrova: senza,
        // non c'è niente da aprire perché non ci sarebbe niente da riprendere.
        say_it_starts_over(
            live,
            named,
            &format!(
                "the step asks to {}, and this run has no store to put it in",
                asked.word()
            ),
        );
        return SessionPlan::from_scratch();
    };
    match asked {
        SessionUse::Open => {
            let Some(line) = &candidate.session.open else {
                say_it_starts_over(live, named, "cannot open a session that can be found again");
                return SessionPlan::from_scratch();
            };
            // **SI CONIA UN IDENTIFICATIVO SOLO SE SI HA DOVE METTERLO.** Una
            // riga senza segnaposto è quella di un motore che il nome se lo dà
            // da sé: registrare lì il nostro scriverebbe nel deposito una
            // sessione che su quella macchina non esiste, e il passo dopo
            // andrebbe a riprendere il nulla — dopo aver speso.
            let ours = line
                .iter()
                .any(|arg| arg.contains(SESSION_PLACEHOLDER))
                .then(fresh_session_id);
            SessionPlan {
                args: Some(match &ours {
                    Some(id) => with_session_id(line, id),
                    None => line.clone(),
                }),
                recorded: ours,
                read_id_from: candidate.session.id_from.clone(),
            }
        }
        SessionUse::Resume(step) | SessionUse::Fork(step) => {
            let forking = matches!(asked, SessionUse::Fork(_));
            let line = if forking {
                &candidate.session.fork
            } else {
                &candidate.session.resume
            };
            let Some(line) = line else {
                say_it_starts_over(live, named, &format!("cannot {}", asked.word()));
                return SessionPlan::from_scratch();
            };
            // Senza identificativo di strumento non c'è nessun motore a cui
            // attribuire una sessione: è un `bin` scritto a mano nel passo.
            let Some(cli) = candidate.id.as_deref() else {
                return SessionPlan::from_scratch();
            };
            let found = record
                .ledger
                .session_opened_by(&record.run_id, step, cli)
                .ok()
                .flatten();
            let Some(id) = found else {
                say_it_starts_over(
                    live,
                    named,
                    &format!("step «{step}» left no session of «{cli}» to continue"),
                );
                return SessionPlan::from_scratch();
            };
            SessionPlan {
                args: Some(with_session_id(line, &id)),
                // Ramificare conia un identificativo nuovo: se il motore non lo
                // dice, questo ramo resta senza nome, e nessuno potrà
                // continuarlo. Se lo dice, `read_id_from` lo raccoglie e il
                // ramo diventa continuabile come il tronco.
                recorded: if forking { None } else { Some(id) },
                read_id_from: candidate.session.id_from.clone(),
            }
        }
    }
}

// ── la dotazione con cui un motore parte ─────────────────────────────────

/// Con che cosa una chiamata a un motore esterno parte davvero.
///
/// **PERCHÉ I DUE CAMPI STANNO INSIEME.** L'ambiente decide *quale casa* quel
/// motore leggerà; il nome del profilo è ciò che finisce nel deposito. Separarli
/// vorrebbe dire risolvere due volte lo stesso profilo e poter sbagliare in un
/// posto solo — cioè scrivere nel deposito una dotazione diversa da quella con
/// cui la chiamata è girata, che è peggio di non scriverla.
pub struct Equipment {
    /// Da sovrapporre all'ambiente ereditato prima di lanciare.
    pub env: BTreeMap<String, String>,
    /// Con quale identità il processo parte: **quale casa** e **come è stata
    /// scelta**. Risponde sempre — non esiste il caso «vuoto».
    pub identity: EngineIdentity,
    /// Why this engine must not start under this profile: an endpoint the
    /// command line cannot be pointed at, or a key the machine lacks.
    pub refused: Option<String>,
}

/// La dotazione per invocare `bin`, secondo lo stato dei profili dato.
///
/// **IL GUASTO 18, ED È LA STESSA MALATTIA DEL 35.** Tutte e due sono «Sailor ha
/// un dato in casa propria e non lo usa». Il listino c'era e non viaggiava col
/// prodotto; la dotazione c'era — `~/.config/sailor/` ha `equipment/`, `flows/`,
/// un listino, una firma — e non arrivava ai motori, perché la sovrapposizione
/// d'ambiente la chiamava solo `sailor run`. Un motore lanciato da un passo di
/// flusso ereditava l'ambiente di chi aveva aperto il terminale: leggeva la casa
/// del vicino, e due corse dello stesso flusso non erano la stessa misura.
///
/// **L'AMBIENTE DEL PROFILO STA SOTTO QUELLO DEL PASSO, E IL VERSO È LA
/// DECISIONE.** Chi scrive una variabile dentro un passo sta dicendo qualcosa di
/// preciso su *quella* chiamata — un profilo diverso per un solo passo, una casa
/// usa-e-getta per una prova — e non deve poter essere scavalcato da uno stato
/// che vive altrove e che quel passo non nomina. Il verso opposto renderebbe la
/// riga scritta nel flusso inerte, in silenzio.
///
/// **PURO: LO STATO ENTRA, LA DOTAZIONE ESCE.** Chi legge il file dei profili sta
/// in [`current_equipment_for`], per la stessa ragione di `price_list_from`.
pub fn equipment_for(
    store: &profiles::ProfileStore,
    bin: &str,
    step_env: &BTreeMap<String, String>,
) -> Equipment {
    equipment_with_keys(store, bin, step_env, &|variable| std::env::var(variable).ok())
}

/// [`equipment_for`] with the machine's key variables read through `key_of`,
/// so a test hands its own.
pub fn equipment_with_keys(
    store: &profiles::ProfileStore,
    bin: &str,
    step_env: &BTreeMap<String, String>,
    key_of: &dyn Fn(&str) -> Option<String>,
) -> Equipment {
    let Some(cli) = profiles::cli_for_executable(bin) else {
        // Un comando qualunque — `sh`, uno script — non ha nessuna casa da
        // spostare, e dargliene una non vorrebbe dire niente.
        return Equipment {
            env: step_env.clone(),
            identity: EngineIdentity::NotAKnownEngine,
            refused: None,
        };
    };
    let named = store.active.get(&cli.id);
    let resolved = named.and_then(|active| {
        store
            .profiles
            .iter()
            // **UNO STATO CHE NOMINA UN PROFILO SPARITO NON INVENTA UNA
            // CARTELLA.** Comporre il percorso dal nome darebbe una casa vuota,
            // cioè senza credenziali, con l'aria di aver applicato un profilo.
            .find(|profile| profile.cli_id == cli.id && &profile.name == active)
    });
    let mut from_the_profile = resolved
        .map(|profile| profiles::build_environment(cli, &profile.home_dir))
        .unwrap_or_default();
    // The endpoint, when the profile declares one: the same overlay, and a
    // refusal instead of a launch when it cannot be pointed there.
    let refused = match resolved.map(|profile| profiles::endpoint_environment(cli, profile, key_of)) {
        Some(Ok(pointed)) => {
            from_the_profile.extend(pointed);
            None
        }
        Some(Err(why)) => Some(why),
        None => None,
    };
    // Il profilo prima, il passo sopra: chi scrive una variabile nel passo vince.
    let mut env = from_the_profile;
    env.extend(
        step_env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    Equipment {
        env,
        identity: identity_of(cli, named.map(String::as_str), resolved, step_env),
        refused,
    }
}

/// Con quale identità questa invocazione parte davvero.
///
/// **IL PASSO SI GUARDA PER PRIMO, ED È LA CURA DEL DIFETTO.** Fino al
/// 01/09/2026 questa decisione era un booleano — «un profilo è stato applicato»
/// — che restava vero anche quando il passo aveva scritto da sé la variabile di
/// casa. Il motore partiva nella casa del passo e la riga nel deposito nominava
/// il profilo attivo: **il registro diceva un'identità e il processo ne aveva
/// usata un'altra**, proprio nel caso in cui qualcuno l'aveva cambiata apposta.
/// L'ordine qui sotto è quello della sovrapposizione vera, non quello dello
/// stato: si registra ciò che accade.
fn identity_of(
    cli: &profiles::KnownCli,
    named: Option<&str>,
    resolved: Option<&profiles::Profile>,
    step_env: &BTreeMap<String, String>,
) -> EngineIdentity {
    let cli_id = cli.id.clone();
    if let profiles::HomeMechanism::EnvVar(variable) = &cli.home {
        if let Some(home) = step_env.get(variable) {
            return EngineIdentity::ChosenByTheStep {
                cli_id,
                home_dir: PathBuf::from(home),
            };
        }
    }
    match (resolved, named) {
        (Some(profile), _) => match &cli.home {
            profiles::HomeMechanism::EnvVar(_) => EngineIdentity::ProfileInForce {
                cli_id,
                profile_name: profile.name.clone(),
                home_dir: profile.home_dir.clone(),
                endpoint: profile.endpoint.as_ref().map(|endpoint| endpoint.url.clone()),
            },
            // **UN PROFILO DICHIARATO NON È UN PROFILO IN FORZA.** Dove la casa
            // si sposta scambiando un collegamento simbolico, o dove non si sa
            // come si sposti, questa funzione non ha messo niente
            // nell'ambiente: l'identità dipende da dove punta un file sul disco,
            // e questo codice il disco non lo tocca.
            mechanism => EngineIdentity::NotMovedByAnEnvVar {
                cli_id,
                profile_name: profile.name.clone(),
                why: why_it_stays_where_it_is(mechanism).to_owned(),
            },
        },
        (None, Some(active)) => EngineIdentity::ProfileVanished {
            cli_id,
            profile_name: active.to_owned(),
        },
        // **«EREDITATA» NON È «NIENTE».** Il processo parte con la casa di chi ha
        // aperto il terminale, che è un'identità vera e nominabile: dirlo è più
        // utile che lasciare un vuoto in cui questo caso si confonde con gli
        // altri quattro.
        (None, None) => EngineIdentity::InheritedFromTheTerminal { cli_id },
    }
}

/// Perché un profilo dichiarato non è finito nell'ambiente, con le parole del
/// meccanismo che lo impedisce.
fn why_it_stays_where_it_is(mechanism: &profiles::HomeMechanism) -> &'static str {
    match mechanism {
        profiles::HomeMechanism::CredentialSymlink { .. } => {
            "this command line has no variable that moves the home: the profile swaps a symlink, and the identity depends on where that file points on the disk"
        }
        profiles::HomeMechanism::Unknown => {
            "how this command line moves its own home is not known, so nothing was overlaid"
        }
        // Un meccanismo a variabile qui non ci arriva: chi chiama lo ha già
        // trattato sopra. Se un giorno ci arrivasse, la frase dice il vero.
        profiles::HomeMechanism::EnvVar(_) => {
            "the mechanism goes through a variable, and it was not overlaid"
        }
    }
}

/// La dotazione di **questa** macchina per invocare `bin`.
///
/// **RILETTA A OGNI CHIAMATA**, per la stessa ragione del listino: un profilo
/// cambiato a metà di una corsa lunga vale dalla chiamata dopo, invece che dal
/// prossimo riavvio, e leggere un file piccolo accanto all'avvio di un processo
/// esterno non costa niente.
///
/// Uno stato dei profili illeggibile non ferma la chiamata: si parte senza
/// sovrapporre niente, che è come si è sempre partiti. Fermare un passo perché
/// non si è potuto leggere un file di preferenze punirebbe chi non c'entra.
fn current_equipment_for(bin: &str, step_env: &BTreeMap<String, String>) -> Equipment {
    let store = profiles::store_io::load_store().unwrap_or_default();
    equipment_for(&store, bin, step_env)
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

/// The person's strengths table: `SAILOR_STRENGTHS`, or `strengths.json` in the home.
fn strengths_path() -> Option<PathBuf> {
    match std::env::var_os("SAILOR_STRENGTHS").filter(|value| !value.is_empty()) {
        Some(declared) => Some(PathBuf::from(declared)),
        None => ledger::sailor_home().map(|home| home.join("strengths.json")),
    }
}

/// Il listino da applicare: quello spedito col prodotto, sovrascritto da quello
/// scritto in casa.
///
/// **PURO, E NON È UN VEZZO.** Il testo di casa entra come argomento e il
/// listino esce: così la regola — «senza niente in casa il costo si sa lo
/// stesso» — si interroga senza toccare il disco e senza scrivere una variabile
/// d'ambiente, che è di **processo** e rovinerebbe le prove che girano in
/// parallelo nello stesso. Chi legge il file sta in [`load_pricing`].
///
/// **UN FILE DI CASA ILLEGGIBILE NON TOGLIE IL LISTINO A TUTTI.** Si torna a
/// quello spedito: prima del 01/09/2026 un JSON scritto male lasciava il costo
/// sconosciuto per l'intera corsa, che è il guasto 35 nella sua forma più
/// silenziosa — un errore di battitura che spegne un tetto di spesa.
pub fn price_list_from(home_text: Option<&str>) -> models::pricing::PriceList {
    let shipped = models::pricing::shipped();
    match home_text.and_then(|text| models::pricing::PriceList::parse(text).ok()) {
        Some(home) => shipped.overridden_by(home),
        None => shipped,
    }
}

/// Il listino di questa macchina: quello spedito, sovrascritto dal file di casa.
///
/// **RILETTO A OGNI CHIAMATA, NON TENUTO IN MEMORIA**: un prezzo cambiato a metà
/// di una corsa lunga vale dalla chiamata dopo, invece che dal prossimo riavvio.
/// Il costo è una lettura di un file piccolo accanto all'avvio di un processo
/// esterno — cioè niente, in confronto a ciò che sta per succedere.
///
/// **PUBBLICA PERCHÉ `sailor flow check` DEVE POTER DIRE COSA NON SA PREZZARE.**
/// Un freno che non frena si deve vedere prima di lanciare, e chi lo mostra è un
/// comando, non questo crate.
pub fn current_price_list() -> models::pricing::PriceList {
    let path = match std::env::var_os(PRICING_ENV).filter(|value| !value.is_empty()) {
        Some(declared) => Some(std::path::PathBuf::from(declared)),
        None => ledger::sailor_home().map(|home| home.join(PRICING_FILE)),
    };
    let text = path.and_then(|path| std::fs::read_to_string(path).ok());
    price_list_from(text.as_deref())
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
    /// La sessione sotto cui questa chiamata è girata, quando si sa qual è.
    session_id: Option<String>,
    /// Con quale identità il processo è partito: quale casa, e come è stata
    /// scelta. Senza, due corse dello stesso flusso non sono la stessa misura —
    /// e la riga non porta la ragione per cui i due consumi differiscono.
    identity: EngineIdentity,
    /// The kind of work the step declared, for the sum per kind.
    work_kind: Option<String>,
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
fn record_the_call(
    record: &Recording<'_>,
    candidate: &Candidate,
    tried_before: &[String],
    spent: Spent,
) {
    let Some(cli) = candidate.id.as_deref() else {
        // Un `bin` scritto a mano nel passo non è una chiamata a un modello:
        // `sh -c echo` non consuma nessuna quota, e riempirne il deposito
        // renderebbe illeggibile proprio la vista che questo lavoro esiste per
        // rendere leggibile.
        return;
    };
    if !candidate.can_be_asked {
        // **E NON LO È NEMMENO UNO STRUMENTO CHE NON SI PUÒ INTERROGARE.**
        // `git` e `cargo` stanno nel catalogo, si eseguono da un passo, e non
        // consumano quota di nessun abbonamento. Contarli fra le chiamate ai
        // modelli non è solo rumore: arrivano senza costo, quindi rendono
        // `Spend::is_complete()` falso su **ogni** corsa vera — misurato il
        // 31/08/2026 sul deposito di questa macchina, tre righe su ventiquattro
        // — e la frase d'onestà del tetto («la spesa vera è più alta») si
        // accende sempre, anche quando non c'è niente di ignoto. Un avviso
        // sempre acceso non lo legge nessuno, e a perdersi è quello vero.
        return;
    }
    let reading = spent.reading;
    let price_list = current_price_list();
    // Il legame col listino passa dal nome che il motore stesso dichiara, non
    // da un'ipotesi: un modello presunto sarebbe un numero inventato con la
    // faccia di una misura, creduto per sempre da chiunque lo legga.
    let entry = reading
        .model
        .as_deref()
        .and_then(|name| price_list.find(name));
    let prices = entry
        .map(models::pricing::Price::micros)
        .unwrap_or_default();
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
        // I turni arrivano dalla stessa uscita da cui arrivano i token, e
        // finora venivano buttati via. Sono la quantita' che spiega perche' una
        // catena di passi costa piu' di una sessione sola.
        turns: reading.turns,
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
        price_currency: cost_micros.map(|_| price_list.currency.clone()),
        input_price_micros_per_million: prices.input,
        output_price_micros_per_million: prices.output,
        cached_price_micros_per_million: prices.cached,
        cache_write_price_micros_per_million: prices.cache_write,
        cache_write_long_price_micros_per_million: prices.cache_write_long,
        // **CON QUALE IDENTITÀ QUESTO PROCESSO È PARTITO.** Non «sotto quale
        // profilo»: quale casa, e come è stata scelta. La differenza è il difetto
        // che questa riga esisteva per avere e non aveva — un passo che scriveva
        // da sé la variabile di casa faceva partire il motore altrove, e qui
        // finiva scritto il nome del profilo attivo.
        engine_identity: spent.identity,
        retry_chain: tried_before.to_vec(),
        error_type: spent.error_type.map(str::to_owned),
        started_at: spent.started_at,
        ended_at: Some(spent.ended_at),
        session_id: spent.session_id,
        work_kind: spent.work_kind,
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
        let invocation = EngineInvocation {
            bin: bin.clone(),
            args,
            env: equipment.env,
            workdir: spec.workdir.clone(),
            stdin: stdin.map(String::into_bytes),
            timeout: Duration::from_secs(seconds),
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
                    tried_before,
                    Spent {
                        reading,
                        error_type,
                        started_at,
                        ended_at,
                        session_id: session.session_id(said),
                        identity: equipment.identity.clone(),
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
                let class = candidate.declared_class(&stdout, &stderr);
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
                    return engine_cannot_work(&named, solo, &stdout, &stderr);
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
                        return engine_cannot_work(&named, solo, &stdout, &stderr);
                    }
                    let chain = if set_aside.is_empty() {
                        String::new()
                    } else {
                        format!(" (before: {})", each_one_why(set_aside))
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
}

impl ExternalEngineAction {
    /// Why `id` is over the cap the person declared for it, if it is: no
    /// file, no cap for it, or no ledger to sum from means it fits. A caps
    /// file that does not read, or a sum that fails, refuses with the reason:
    /// a cap the person wrote is never lifted by a typo.
    fn over_budget(&self, id: &str) -> Option<String> {
        let budgets = match budget::declared(self.budgets.as_deref()?) {
            Ok(budgets) => budgets,
            Err(why) => return Some(format!("its caps cannot be read: {why}")),
        };
        let declared = budgets.get(id)?;
        let now = now_secs();
        let spent = match self.ledger.as_ref()?.spent_by_cli_since(id, now - declared.window_secs) {
            Ok(spent) => spent,
            Err(error) => return Some(format!("its spend cannot be summed: {error}")),
        };
        budget::over(declared, &spent)
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

impl Candidate {
    fn says_it_cannot_work(&self, stdout: &str, stderr: &str) -> bool {
        says_it_cannot_work(&self.unusable_when, stdout)
            || says_it_cannot_work(&self.unusable_when, stderr)
    }

    /// The class of a failure this engine declared: a spent quota is its own
    /// class, anything else it cannot work with is `exhausted` as before, and
    /// an output that says neither is `None`.
    fn declared_class(&self, stdout: &str, stderr: &str) -> Option<&'static str> {
        if mentions_any(&self.exhausted_when, stdout) || mentions_any(&self.exhausted_when, stderr) {
            return Some("quota_exhausted");
        }
        self.says_it_cannot_work(stdout, stderr).then_some("exhausted")
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
        if let Some(cut) = tree_of_its_own(&spec, shared)? {
            if let Some(live) = live.as_deref() {
                live.chunk(
                    Pipe::Stderr,
                    format!("[sailor] this step works in {}\n", cut.display()).as_bytes(),
                );
            }
            spec.workdir = Some(cut.to_string_lossy().into_owned());
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
                "none of the engines the step asks for could work. {}",
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
    /// Dove gira la verifica. Non lo scrive quasi mai una persona: ce lo mette
    /// l'esecutore quando compone l'ingresso, prendendolo dalla radice del
    /// progetto. Un percorso assoluto scritto qui a mano non arriva mai fin
    /// qui — `step_input` lo rifiuta prima.
    #[serde(default)]
    workdir: Option<String>,
    /// La forma della lettura, quando questo passo non verifica soltanto ma
    /// **legge**. Assente vuol dire come prima: a valle va solo l'esito.
    ///
    /// Il controllo gemello del motore — `shape_was_asked_for`, che si rifiuta
    /// di spendere se la forma non compare nel prompt — qui non ha analogo, e
    /// fingerlo sarebbe peggio che non averlo: `git` non riceve la tua forma e
    /// non può conformarsi. Perciò il patto è l'altro: **il comando deve già
    /// emettere JSON**. Se non lo fa il passo va rosso dicendo esattamente
    /// cosa aggiungere — `--json`, `--format=json`, `| jq` — invece di
    /// indovinare come si legge un testo che un giorno cambierà formato.
    #[serde(default)]
    answer_shape: Option<ValueSchema>,
    /// Ciò che non è riconosciuto, per la stessa ragione di `EngineSpec::extra`.
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

/// **IL PRIMO TETTO SUL VOLUME CHE QUESTO PROGETTO ABBIA.** L'unico tetto che
/// c'era è sul tempo: un comando lento viene ucciso, un comando logorroico no.
/// Un motore ha un freno naturale perché paga a token; un comando stampa
/// gratis, e senza un limite ciò che stampa finirebbe nel deposito.
///
/// Un milione di caratteri — un libro di seicento pagine — è largo per un uso
/// vero e stretto abbastanza da prendere gli incidenti. Sopra il tetto si va in
/// rosso e **non si tronca**: un valore mozzato sembra intero, e chi lo legge a
/// valle non ha modo di sapere che manca un pezzo.
const MAX_ANSWER_BYTES: usize = 1_000_000;

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
    /// Come per il motore, e dalla stessa struttura.
    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match serde_json::from_value::<CheckSpec>(declared.clone()) {
            Ok(spec) => spec.extra.into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }

    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let live = sink_for_step(&self.watcher, shared);
        let spec: CheckSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        check_tolerance(&spec.accept, &CHECK_FAILURES)?;
        let seconds = spec.timeout_secs;
        let command = spec.command.clone();
        let invocation = CheckInvocation {
            command: spec.command,
            env: spec.env,
            timeout: Duration::from_secs(seconds),
            workdir: spec.workdir,
        };
        let (status, said) = match run_shell_check_watched(&invocation, live.as_deref()) {
            CheckResult::Passed { stdout } => ("passed", Some(stdout)),
            CheckResult::Failed { code, stderr } => {
                if !tolerates(&spec.accept, "failed") {
                    return Err(ActionError::new(
                        "check_failed",
                        format!(
                            "the check `{command}` {}; {}",
                            how_it_exited(code),
                            what_it_said("", &stderr)
                        ),
                    ));
                }
                ("failed", None)
            }
            CheckResult::TimedOut => {
                if !tolerates(&spec.accept, "timed_out") {
                    return Err(ActionError::new(
                        "check_timed_out",
                        format!(
                            "the check `{command}` did not finish within {seconds} seconds and was killed"
                        ),
                    ));
                }
                ("timed_out", None)
            }
        };
        // **LA FORMA SI APPLICA SOLO A UN COMANDO RIUSCITO**, e qui il comando
        // si separa dal motore di proposito. Il motore pretende la forma anche
        // in `exit_error`, perché un motore che fallisce ha comunque parlato;
        // un comando fallito non ha prodotto la lettura richiesta. Chi ha
        // scritto `accept` ramifica già sullo stato, altrimenti non l'avrebbe
        // scritto.
        let Some((shape, said)) = spec.answer_shape.as_ref().zip(said) else {
            return Ok(ActionOutcome::Went(json!({ "status": status })));
        };
        if said.len() > MAX_ANSWER_BYTES {
            return Err(ActionError::new(
                "answer_too_large",
                format!(
                    "the reading of `{command}` weighs {} characters, past the cap of {MAX_ANSWER_BYTES}.                      The cap does not truncate: a cut value looks whole. Narrow what the                      command prints — with a filter, or by asking it for fewer fields.",
                    said.len()
                ),
            ));
        }
        // **IL TESTO GREZZO NON ESCE DAL PASSO**: consegna `answer`, o niente.
        // È la stessa scelta che `an_engine_step_declares_what_it_can_return_and_what_it_hands_on`
        // pretende già dal motore, e lasciarlo passare accanto al valore
        // renderebbe la forma un ornamento.
        let answer = shaped_answer(shape, &said)?;
        Ok(ActionOutcome::Went(
            json!({ "status": status, "answer": answer }),
        ))
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

    /// L'ingresso come lo riceve un'azione **quando gira davvero**: coi rinvii
    /// già sciolti.
    ///
    /// **PERCHÉ UNA PROVA DI QUESTO CRATE NE HA BISOGNO.** Dal 01/09/2026 i
    /// rinvii li scioglie `flow::step_input`, una volta sola dove l'ingresso si
    /// compone — è la cura del guasto 28, e la ragione per cui nel codice di
    /// questo crate non c'è più nessuna chiamata a `resolve_references`. Una
    /// prova che invochi `execute` direttamente salta quel passaggio: senza
    /// questa riga proverebbe l'azione in un mondo in cui non gira mai, che è
    /// il guasto 39.
    ///
    /// **NON È UNA SECONDA COPIA DELLA REGOLA**: chiama la funzione vera. E
    /// non prova niente da sola — ciò che i rinvii arrivino sciolti a **ogni**
    /// azione lo prova `crates/flow/tests/a_reference_reaches_every_action.rs`,
    /// che passa dall'esecutore invece di chiamare la risoluzione a mano.
    pub(crate) fn with_references_resolved(input: Value) -> Value {
        flow::reference::resolve_references(&input).expect("i rinvii della prova si sciolgono")
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

    /// E il nipote, che è il caso vero: un motore che accende un figlio suo.
    ///
    /// `sleep & wait` costringe la shell a forkare, così il nipote esiste su
    /// qualunque shell. Uccidere il figlio lo lascia vivo col capo scrivente
    /// della pipe in mano, e chi la svuota resta fermo fino alla sua morte
    /// naturale. L'orologio qui può dare un rosso falso, mai un verde falso.
    #[test]
    fn a_grandchild_does_not_keep_the_cap_waiting() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("sleep 20 & wait");
        let start = Instant::now();
        let outcome = run_with_timeout(cmd, secs(1));
        assert!(matches!(outcome, RunOutcome::TimedOut));
        assert!(
            start.elapsed() < secs(10),
            "il nipote ha tenuto aperta la pipe fino alla fine: {:?}",
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
            self.chunks.lock().expect("nessuno panica qui").push((
                self.start.elapsed(),
                pipe,
                bytes.to_vec(),
            ));
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
        assert!(
            !out.contains("di-errore"),
            "stderr finito su stdout: {out:?}"
        );
        assert!(
            !err.contains("di-fuori"),
            "stdout finito su stderr: {err:?}"
        );
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
        assert!(seen.iter().any(|(pipe, bytes)| *pipe == Pipe::Stdout
            && String::from_utf8_lossy(bytes).contains("per-la-closure")));
    }

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
                &mut shared,
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
    fn engine_stdin_reaches_an_engine_that_reads_it() {
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
            .execute(&input, &mut SharedState::new())
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

    // ── run_shell_check ───────────────────────────────────────────────

    #[test]
    fn a_true_check_passes() {
        let invocation = CheckInvocation {
            command: "true".to_string(),
            env: BTreeMap::new(),
            timeout: secs(5),
            workdir: None,
        };
        assert!(matches!(
            run_shell_check(&invocation),
            CheckResult::Passed { .. }
        ));
    }

    #[test]
    fn a_false_check_fails() {
        let invocation = CheckInvocation {
            command: "false".to_string(),
            env: BTreeMap::new(),
            timeout: secs(5),
            workdir: None,
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
            workdir: None,
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
            workdir: None,
        };
        assert!(matches!(
            run_shell_check(&invocation),
            CheckResult::Passed { .. }
        ));
    }

    /// **UNA VERIFICA GIRA DOVE LE SI DICE**, e prima del 31/08/2026 non c'era
    /// modo di dirglielo: girava dove sta il processo. Un `cargo test` che
    /// passa perché eseguito nell'albero sbagliato non fallisce — dice di sì,
    /// ed è il difetto peggiore che una verifica possa avere.
    #[test]
    fn a_check_runs_where_it_is_told() {
        let elsewhere =
            std::env::temp_dir().join(format!("sailor-verifica-altrove-{}", std::process::id()));
        std::fs::create_dir_all(&elsewhere).expect("cartella di prova");
        std::fs::write(elsewhere.join("il-testimone"), "x").expect("testimone");
        let invocation = CheckInvocation {
            command: "test -f il-testimone".to_string(),
            env: BTreeMap::new(),
            timeout: secs(5),
            workdir: Some(elsewhere.display().to_string()),
        };

        assert!(matches!(
            run_shell_check(&invocation),
            CheckResult::Passed { .. }
        ));

        let _ = std::fs::remove_dir_all(&elsewhere);
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
        let outcome = action
            .execute(&input, &mut shared)
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
            .execute(&input, &mut SharedState::new())
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
            .execute(&input, &mut SharedState::new())
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
            .execute(&with_references_resolved(input), &mut SharedState::new())
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
            .execute(&with_references_resolved(input), &mut SharedState::new())
            .expect_err("un motore fuori forma non ha risposto");

        assert_eq!(error.class, "answer_off_shape");
        assert!(error.said.contains("parecchi"), "{}", error.said);
    }

    /// **A SHAPE WITH NO FIELD AND EXTRAS ALLOWED IS «ANY OBJECT»**, not «no
    /// field»: pruned to its declaration it came out `{}`, and the flow a model
    /// had drafted whole reached the next step empty.
    #[test]
    fn an_object_shape_declaring_no_field_hands_the_whole_object_on() {
        let shape: ValueSchema = serde_json::from_value(json!({
            "type": "object", "properties": {}, "required": [], "allow_extra": true
        }))
        .expect("a shape");
        let whole = json!({"id": "una-bozza", "graph": {"steps": []}});
        assert_eq!(pruned(&shape, whole.clone()), whole);
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
            .execute(&with_references_resolved(input), &mut SharedState::new())
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
            .execute(&with_references_resolved(input), &mut SharedState::new())
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
            .execute(&with_references_resolved(input), &mut SharedState::new())
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
                    args_before_prompt: Vec::new(),
                    unusable_when: vec!["weekly limit".to_owned()],
                    silent_without_prompt: false,
                    refuses_without_prompt: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
                    usage: None,
                }),
                "vivo" => Some(AskRecipe {
                    args: vec!["ha-risposto-il-secondo".to_owned()],
                    prompt: PromptVia::LastArg,
                    args_before_prompt: Vec::new(),
                    unusable_when: vec!["weekly limit".to_owned()],
                    silent_without_prompt: false,
                    refuses_without_prompt: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
                    usage: None,
                }),
                "rotto" => Some(AskRecipe {
                    args: Vec::new(),
                    prompt: PromptVia::Stdin,
                    args_before_prompt: Vec::new(),
                    unusable_when: vec!["weekly limit".to_owned()],
                    silent_without_prompt: false,
                    refuses_without_prompt: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
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

    /// Un eseguibile finto che dice di essere esaurito **ed esce zero**.
    ///
    /// **PERCHÉ SERVE UN SECONDO FINTO MOTORE.** Il gemello qui sopra esce 1, e
    /// tutte le prove ermetiche su questa catena facevano così: il motore
    /// esaurito usciva **sempre** in errore, quindi nessuna di esse guardava mai
    /// il ramo riuscito. Un difetto che vive solo di là non poteva diventare
    /// rosso.
    fn engine_that_says_it_is_out_and_exits_zero(dir: &std::path::Path) -> String {
        let path = dir.join("zero-dopo-aver-parlato");
        std::fs::write(
            &path,
            "#!/bin/sh\necho \"You've hit your weekly limit · resets 7am\"\nexit 0\n",
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

    /// **DIRLO E USCIRE ZERO.**
    ///
    /// **IL GUASTO NON HA ANCORA UN NUMERO, E NON GLIENE DO UNO.** Sta in
    /// `docs/da-fare.md` in attesa della fusione: due rami stanno numerando
    /// righe nuove nello stesso momento, e il numero che questo lavoro si
    /// aspettava di prendere è già stato preso mentre era in corso. Un numero
    /// sbagliato in un commento manda a leggere il guasto di qualcun altro.
    ///
    /// Un motore che dichiara con le proprie parole di non poter lavorare, e
    /// **esce zero**. Fino al 01/09/2026 `says_it_cannot_work` veniva
    /// interrogato solo dentro il ramo `ExitError`: nel ramo riuscito la
    /// risposta era presa per buona, il ripiego non scattava, e la riga del
    /// deposito nasceva con `error_type: None` — cioè il passo risultava
    /// riuscito, e il motore dopo di lui non partiva mai.
    ///
    /// **NON È IPOTETICO SU QUESTA MACCHINA.** È la forma del guasto 39:
    /// `CODEX_HOME=<cartella vuota> codex exec < /dev/null` risponde «No prompt
    /// provided via stdin» ed esce **zero**. E la sonda a secco la distinzione
    /// ce l'aveva già — `judge_dry_run` è applicata a `Ok` *e* a `ExitError` —
    /// quindi il controllo statico e la corsa vera dicevano cose diverse sullo
    /// stesso motore.
    ///
    /// La coppia con `an_engine_that_says_it_is_out_hands_the_work_to_the_next_one`
    /// è tutta la dimostrazione: le stesse parole, l'unica differenza è il
    /// codice d'uscita, e il ripiego deve scattare in tutti e due i casi.
    #[test]
    fn an_engine_that_says_it_is_out_while_exiting_zero_still_hands_the_work_over() {
        let dir = scratch("esaurito-a-uscita-zero");
        let action = ExternalEngineAction::resolving_with(ChainIn(
            engine_that_says_it_is_out_and_exits_zero(&dir),
        ));
        let input = json!({"tool": ["esaurito", "vivo"], "timeout_secs": 10});

        let ActionOutcome::Went(output) = action
            .execute(&input, &mut SharedState::new())
            .expect("il secondo motore risponde")
        else {
            panic!("un motore che risponde è sempre Went")
        };

        assert_eq!(
            output["stdout"], "ha-risposto-il-secondo\n",
            "il primo motore ha detto di non poter lavorare ed è uscito zero: il \
             lavoro doveva passare al secondo, non fermarsi sulla sua non-risposta"
        );
    }

    /// **E DA SOLO LO DICE, INVECE DI FINGERE DI AVER RISPOSTO.**
    ///
    /// Senza nessun ripiego dietro non c'è niente da salvare, ma resta la
    /// diagnosi: chi legge «esaurito» sa che deve aspettare o cambiare profilo,
    /// chi legge un passo **verde** va a cercare la risposta che non c'è. Era la
    /// seconda metà del difetto, e la peggiore: il passo si chiudeva riuscito.
    #[test]
    fn alone_an_engine_that_says_it_is_out_while_exiting_zero_does_not_pass_for_answered() {
        let dir = scratch("esaurito-a-uscita-zero-da-solo");
        let action = ExternalEngineAction::resolving_with(ChainIn(
            engine_that_says_it_is_out_and_exits_zero(&dir),
        ));
        let input = json!({"tool": ["esaurito"], "timeout_secs": 10});

        let error = action
            .execute(&input, &mut SharedState::new())
            .expect_err("un motore che dice di non poter lavorare non ha risposto");

        assert_eq!(
            error.class, "engine_exhausted",
            "esaurito non è rotto, e a uscita zero non è nemmeno «riuscito»: {}",
            error.said
        );
        assert!(
            error.said.contains("weekly limit"),
            "il motivo deve portare le parole con cui il motore l'ha detto: {}",
            error.said
        );
    }

    /// **IL GUASTO 31, RESO UN FATTO INVECE DI UNA LETTURA.**
    ///
    /// Lo stesso motore esaurito di qui sopra, con la sola differenza che
    /// conta: il suo descrittore **non dichiara nessuna parola** di
    /// `unusable_when`. `says_it_cannot_work` su un elenco vuoto è `false`,
    /// quindi il suo esaurirsi passa per un fallimento qualunque, il passo
    /// muore lì, e il motore successivo **non parte mai**. È il descrittore di
    /// `agy` così com'è spedito il 31/08/2026, ed è la ragione per cui nella
    /// catena `claude-code → agy → codex` un `agy` esaurito uccide il passo e
    /// `codex` non viene nemmeno provato.
    ///
    /// **PERCHÉ NON BASTA LA GEMELLA SUI FRAMMENTI VUOTI.** Quella prova un
    /// descrittore scritto male; questa prova un descrittore che **tace**, che
    /// è il caso vero e quello che nessuno legge come un difetto: un campo
    /// assente sembra una scelta, un campo pieno di stringhe vuote sembra un
    /// errore. Il comportamento è lo stesso, e la differenza è che al primo
    /// nessuno guarda.
    ///
    /// La coppia con la prova qui sopra è tutta la dimostrazione: elenco
    /// popolato, il secondo parte; elenco vuoto, il secondo non parte.
    #[test]
    fn an_engine_that_declares_no_exhaustion_words_kills_the_chain() {
        /// Come `ChainIn`, ma al primo motore si toglie ciò che `agy` non ha.
        struct NoMarks(String);
        impl ToolResolver for NoMarks {
            fn resolve(&self, id: &str) -> Result<String, String> {
                match id {
                    "esaurito" => Ok(self.0.clone()),
                    other => Chain.resolve(other),
                }
            }
            fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
                let recipe = Chain.ask_recipe(id)?;
                if id == "esaurito" {
                    return Some(AskRecipe {
                        unusable_when: Vec::new(),
                        ..recipe
                    });
                }
                Some(recipe)
            }
        }

        let dir = scratch("catena-senza-parole");
        let action =
            ExternalEngineAction::resolving_with(NoMarks(engine_that_says_it_is_out(&dir)));
        let input = json!({"tool": ["esaurito", "vivo"], "timeout_secs": 10});

        let error = action
            .execute(&input, &mut SharedState::new())
            .expect_err("il passo muore sul primo motore");

        assert_eq!(
            error.class, "engine_exit_error",
            "un esaurimento non dichiarato passa per un fallimento qualunque"
        );
        assert!(
            !error.said.contains("ha-risposto-il-secondo"),
            "il secondo motore non doveva nemmeno partire: {}",
            error.said
        );
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
                        args_before_prompt: Vec::new(),
                        unusable_when: vec![String::new(), "   ".to_owned()],
                        silent_without_prompt: false,
                        refuses_without_prompt: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
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
        let mut shared = SharedState::new();

        let ActionOutcome::Went(output) = action
            .execute(&with_references_resolved(input), &mut shared)
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
                .execute(&with_references_resolved(input), &mut SharedState::new())
                .map(|outcome| {
                    let ActionOutcome::Went(output) = outcome else {
                        panic!("una verifica accettata è sempre Went")
                    };
                    output["status"].as_str().unwrap().to_owned()
                })
                .map_err(|error| error.class)
        };

        assert_eq!(
            verdict("ho guardato i file\nVERDETTO: APPROVATO\n"),
            Ok("passed".to_owned())
        );
        assert_eq!(
            verdict("mancano due sezioni\nVERDETTO: RESPINTO\n"),
            Err("check_failed".to_owned())
        );
        assert_eq!(
            verdict(""),
            Err("check_failed".to_owned()),
            "un motore muto non approva"
        );
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
        assert!(error.said.contains("code 2"), "{}", error.said);
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

    /// **UNA VERIFICA CHE LEGGE, NON SOLO CHE GIUDICA.** Oggi `shell_check`
    /// consegna a valle una cosa sola — se è andata bene — e ciò che il comando
    /// ha detto muore dentro il passo. Il macchinario per non buttarlo esiste
    /// già novanta righe più su: `shaped_answer` valida contro la forma
    /// dichiarata e `pruned` taglia ciò che la forma non ha promesso.
    ///
    /// LA MISURA CHE POTEVA VENIRE DIVERSA: `spurio` esce dal comando ma **non**
    /// dalla forma. Se la potatura non venisse applicata, l'asserzione su
    /// `answer.spurio` lo troverebbe e questa prova diventerebbe rossa. E se il
    /// testo grezzo venisse inoltrato accanto al valore, `stdout` comparirebbe
    /// nell'uscita: è la scorciatoia che renderebbe inutile la forma, e
    /// `an_engine_step_declares_what_it_can_return_and_what_it_hands_on` la
    /// vieta già per il motore.
    #[test]
    fn a_check_that_declares_a_shape_hands_on_a_value_not_only_a_verdict() {
        let input = json!({
            "command": r#"echo '{"conta": 3, "spurio": "non promesso"}'"#,
            // **`allow_extra` VERO È IL PUNTO DELLA PROVA, NON UNA SVISTA.** Con
            // `false` un campo in più è un rifiuto e la potatura non entra mai
            // in gioco; con `true` il campo è tollerato dalla validazione, e
            // ciò che lo toglie è `pruned`. Metterlo a `false` qui renderebbe
            // questa prova verde per il motivo sbagliato.
            "answer_shape": {
                "type": "object",
                "properties": {"conta": {"type": "number"}},
                "required": ["conta"],
                "allow_extra": true
            },
            "timeout_secs": 5
        });

        let ActionOutcome::Went(output) = ShellCheckAction::new()
            .execute(&input, &mut SharedState::new())
            .expect("il comando riesce e risponde nella forma dichiarata")
        else {
            panic!("una verifica eseguita è sempre Went")
        };

        assert_eq!(output["status"], "passed");
        assert_eq!(output["answer"]["conta"], 3);
        assert!(
            output["answer"].get("spurio").is_none(),
            "a valle passa solo ciò che la forma ha promesso: {}",
            output["answer"]
        );
        assert!(
            output.get("stdout").is_none(),
            "il testo grezzo non esce dal passo: consegna «answer», o niente — {output}"
        );
    }

    /// I due modi di sbagliare, con lo stesso nome che usa già il motore.
    /// Scartata l'interpretazione del testo a righe: un pavimento che cede in
    /// silenzio il giorno che il comando cambia formato. Chi scrive il flusso
    /// aggiunge `--json` o `| jq`, e il rosso glielo dice.
    #[test]
    fn a_reading_that_is_not_json_or_not_in_shape_breaks_the_step() {
        let forma = json!({
            "type": "object",
            "properties": {"conta": {"type": "number"}},
            "required": ["conta"],
            "allow_extra": false
        });

        let non_json = json!({
            "command": "echo non sono json",
            "answer_shape": forma.clone(),
            "timeout_secs": 5
        });
        let error = ShellCheckAction::new()
            .execute(&non_json, &mut SharedState::new())
            .expect_err("un comando che non emette JSON non ha prodotto una lettura");
        assert_eq!(error.class, "answer_not_json");

        let fuori_forma = json!({
            "command": r#"echo '{"conta": "tre"}'"#,
            "answer_shape": forma,
            "timeout_secs": 5
        });
        let error = ShellCheckAction::new()
            .execute(&fuori_forma, &mut SharedState::new())
            .expect_err("JSON valido ma fuori dalla forma dichiarata");
        assert_eq!(error.class, "answer_off_shape");
    }

    /// **QUI IL COMANDO SI SEPARA DAL MOTORE, E NON PER SVISTA.** Il motore
    /// pretende la forma anche in `exit_error`, perché un motore che fallisce ha
    /// comunque parlato. Un comando fallito non ha prodotto la lettura che gli
    /// è stata chiesta: lasciar passare un valore lì dentro vorrebbe dire
    /// leggere da uno strumento rotto. Chi ha scritto `accept` ramifica già
    /// sullo stato, altrimenti non l'avrebbe scritto.
    #[test]
    fn a_tolerated_failure_hands_on_no_value_at_all() {
        let input = json!({
            "command": r#"echo '{"conta": 3}'; exit 2"#,
            "accept": ["failed"],
            "answer_shape": {
                "type": "object",
                "properties": {"conta": {"type": "number"}},
                "required": ["conta"],
                "allow_extra": false
            },
            "timeout_secs": 5
        });

        let ActionOutcome::Went(output) = ShellCheckAction::new()
            .execute(&input, &mut SharedState::new())
            .expect("l'esito è dichiarato accettabile")
        else {
            panic!("un esito tollerato resta un dato")
        };

        assert_eq!(output["status"], "failed");
        assert!(
            output.get("answer").is_none(),
            "un comando fallito non ha prodotto la lettura richiesta: {output}"
        );
    }

    /// **IL PRIMO TETTO SUL VOLUME CHE SAILOR ABBIA.** Cercato in tutto
    /// `crates/`: non ce n'è nessuno, né nelle azioni né nel deposito né nel
    /// registro. L'unico tetto esistente è sul *tempo* — un comando lento viene
    /// ucciso, un comando logorroico no. Un motore ha un freno naturale perché
    /// paga a token; un comando stampa gratis.
    ///
    /// Rosso e non troncamento: un valore mozzato sembra intero, e chi lo legge
    /// a valle non ha modo di sapere che manca un pezzo.
    #[test]
    fn a_reading_above_the_ceiling_is_refused_instead_of_being_cut() {
        let input = json!({
            // Due milioni di caratteri: il doppio della soglia.
            "command": "printf '\"a\": \"'; head -c 2000000 /dev/zero | tr '\\0' 'a'",
            "answer_shape": {
                "type": "object",
                "properties": {"a": {"type": "string"}},
                "required": ["a"],
                "allow_extra": false
            },
            "timeout_secs": 30
        });

        let error = ShellCheckAction::new()
            .execute(&input, &mut SharedState::new())
            .expect_err("sopra il tetto il passo si ferma invece di tagliare");
        assert_eq!(error.class, "answer_too_large");
    }

    // **UN PUNTATORE CHE NON TROVA NIENTE FERMA IL PASSO, E NON PIÙ QUI.** La
    // prova stava in questo modulo perché la risoluzione stava in questa
    // azione. Dal 01/09/2026 sta in `flow::step_input`, quindi il passo si
    // ferma **prima che l'azione esista**: si prova dove accade, in
    // `crates/flow/tests/a_reference_reaches_every_action.rs`. Tenerla anche qui
    // vorrebbe dire due prove della stessa regola in due punti — e quella qui
    // sarebbe verde chiamando la risoluzione a mano, cioè misurando la prova.

    #[test]
    fn the_registry_finds_both_actions_by_their_stable_names() {
        let mut registry = flow::ActionRegistry::default();
        register_default(&mut registry);
        assert!(registry.get(EXTERNAL_ENGINE_ACTION).is_some());
        assert!(registry.get(SHELL_CHECK_ACTION).is_some());
    }
    // ── la pausa fra due `try_wait` ──────────────────────────────────

    /// Avvia un figlio con le pipe collegate e registra ogni durata che il
    /// ciclo chiede di aspettare, dormendola davvero.
    ///
    /// **QUESTO È IL PUNTO DI INIEZIONE, ED È IL MOTIVO PER CUI QUESTE DUE
    /// PROVE NON CRONOMETRANO.** La sequenza delle durate la decide il codice:
    /// è la stessa a macchina ferma e a macchina in ginocchio. Il carico può
    /// cambiare solo *quanti* elementi ha, e su quello non si afferma niente.
    fn pauses_asked_while_running(script: &str) -> Vec<Duration> {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("sh esiste");
        let mut asked = Vec::new();
        let outcome = drain_and_wait_paced(
            child.stdout.take(),
            child.stderr.take(),
            &mut child,
            secs(30),
            None,
            &mut |how_long| {
                asked.push(how_long);
                std::thread::sleep(how_long);
            },
        );
        assert!(
            matches!(outcome, RunOutcome::Finished { .. }),
            "il figlio doveva finire da solo: il tetto di tempo non c'entra"
        );
        asked
    }

    /// LA MISURA CHE POTEVA VENIRE DIVERSA: col codice di prima la prima durata
    /// chiesta era 50 ms, e un comando finito in cinque millisecondi restava
    /// comunque appeso per quarantacinque.
    #[test]
    fn the_first_poll_pause_is_short_not_fifty_milliseconds() {
        let asked = pauses_asked_while_running("sleep 0.1");
        assert!(
            !asked.is_empty(),
            "il ciclo deve aver aspettato almeno una volta, o non prova niente"
        );
        assert!(
            asked[0] <= Duration::from_millis(2),
            "la prima pausa dev'essere dell'ordine del millisecondo, non di cinquanta: {:?}",
            asked[0]
        );
    }

    /// L'altra metà: la crescita si ferma. Senza tetto un figlio di dieci
    /// minuti verrebbe raccolto minuti dopo essere morto.
    ///
    /// Nessuna affermazione sul *numero* di risvegli — quello dipende dalla
    /// durata reale del figlio, cioè dal carico della macchina. Solo sulla
    /// forma: prima sale davvero, poi si ferma sul tetto e lì resta.
    ///
    /// **«NON DECRESCENTE» NON BASTAVA, ed è il difetto con cui questa prova è
    /// nata**: la sequenza `[50, 50, 50…]` del polling fisso è non decrescente,
    /// non supera il tetto e finisce sul tetto — passava tutte e tre le
    /// asserzioni di prima. Serve chiedere che ci sia una salita *prima* del
    /// tetto, perché è esattamente quella che il difetto vecchio non ha.
    /// La regola di crescita interrogata da sola, **senza avviare niente**: è
    /// quello che il commento su `next_poll_pause` promette a chi legge, e
    /// finché nessuno lo faceva era una promessa scritta e non mantenuta.
    ///
    /// Da sola non prova niente sul ciclo — una `sleep` fissa rimessa dentro al
    /// `loop` lascerebbe questa verde. È la prova qui sotto che lega la regola
    /// al ciclo; questa fissa la regola.
    #[test]
    fn the_growth_rule_doubles_and_saturates_without_running_anything() {
        let mut pause = FIRST_POLL_PAUSE;
        let mut seen = vec![pause];
        for _ in 0..16 {
            pause = next_poll_pause(pause);
            seen.push(pause);
        }
        assert_eq!(
            &seen[..4],
            &[
                Duration::from_millis(1),
                Duration::from_millis(2),
                Duration::from_millis(4),
                Duration::from_millis(8),
            ],
            "la crescita è un raddoppio: {seen:?}"
        );
        assert_eq!(
            seen.last().copied(),
            Some(MAX_POLL_PAUSE),
            "sedici raddoppi devono aver saturato sul tetto: {seen:?}"
        );
    }

    #[test]
    fn the_poll_pause_grows_up_to_the_cap_and_stays_there() {
        assert!(
            MAX_POLL_PAUSE <= Duration::from_millis(50),
            "il tetto non può superare i 50 ms che il ciclo già pagava"
        );
        let asked = pauses_asked_while_running("sleep 0.6");
        let reached = asked
            .iter()
            .position(|&one| one == MAX_POLL_PAUSE)
            .unwrap_or_else(|| {
                panic!("mezzo secondo deve bastare per arrivare al tetto: {asked:?}")
            });
        let (climbing, at_cap) = asked.split_at(reached);
        assert!(
            !climbing.is_empty(),
            "la prima pausa è già il tetto: qui non cresce niente, è l'attesa fissa di prima — {asked:?}"
        );
        assert!(
            climbing.windows(2).all(|pair| pair[0] < pair[1]),
            "finché non tocca il tetto ogni pausa dev'essere più lunga della precedente: {asked:?}"
        );
        assert!(
            at_cap.iter().all(|&one| one == MAX_POLL_PAUSE),
            "arrivata al tetto la pausa non deve più muoversi: {asked:?}"
        );
    }
}

#[cfg(test)]
mod what_it_cost {
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
        let dir =
            std::env::temp_dir().join(format!("sailor-consumo-{}-{name}", std::process::id()));
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
    const PRICE_LIST: &str = r#"{
      "currency": "USD",
      "dated": "2026-08-29",
      "models": [
        { "id": "modello-di-prova", "aliases": ["prova"],
          "input_per_million": 3.0, "output_per_million": 15.0,
          "cached_per_million": 0.3 }
      ]
    }"#;


    /// A step declared blind is handed what the flow hands it and nothing else.
    /// The word is the flow's, not ours: no kind of work is read as a judge.
    #[test]
    fn a_step_declared_blind_carries_no_option_that_would_continue_a_session() {
        let dir = scratch("cieco");
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        ledger
            .record_model_call(&a_call_that_opened("s-1"))
            .expect("una sessione lasciata dal passo di prima");
        let candidate = a_candidate_that_can_resume();
        let record = Recording {
            ledger: &ledger,
            run_id: "corsa".to_owned(),
            step_id: "verifica".to_owned(),
        };
        let asked = SessionUse::Resume("implementa".to_owned());

        // The control: without the declaration the step resumes, which is what
        // makes the blind case a difference and not a coincidence.
        let seeing = session_plan(&candidate, Some(&asked), false, Some(&record), None, "«motore»");
        assert_eq!(
            seeing.args,
            Some(vec!["--resume".to_owned(), "s-1".to_owned()]),
            "a step that did not ask to be blind continues the session it named"
        );

        let blind = session_plan(&candidate, Some(&asked), true, Some(&record), None, "«motore»");
        assert!(
            blind.args.is_none() && blind.recorded.is_none(),
            "a blind step starts from scratch: {:?}",
            blind.args
        );
    }

    fn a_candidate_that_can_resume() -> Candidate {
        Candidate {
            id: Some("motore".to_owned()),
            bin: "eco".to_owned(),
            args: vec!["ask".to_owned()],
            prompt: PromptVia::Stdin,
            unusable_when: Vec::new(),
            exhausted_when: Vec::new(),
            cooldown_secs: None,
            declared_usage: None,
            can_be_asked: true,
            why: None,
            session: SessionRecipe {
                open: Some(vec!["--session".to_owned(), SESSION_PLACEHOLDER.to_owned()]),
                resume: Some(vec!["--resume".to_owned(), SESSION_PLACEHOLDER.to_owned()]),
                fork: None,
                id_from: None,
            },
        }
    }

    fn a_call_that_opened(session: &str) -> ledger::ModelCallRecord {
        ledger::ModelCallRecord {
            call_id: format!("corsa:implementa:{session}"),
            run_id: "corsa".to_owned(),
            step_id: Some("implementa".to_owned()),
            purpose: EXTERNAL_ENGINE_ACTION.to_owned(),
            cli: "motore".to_owned(),
            requested_model: String::new(),
            actual_model: String::new(),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost_micros: None,
            declared_cost_micros: None,
            price_currency: None,
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: ledger::EngineIdentity::NotAKnownEngine,
            retry_chain: Vec::new(),
            error_type: None,
            started_at: 1,
            ended_at: Some(2),
            session_id: Some(session.to_owned()),
            work_kind: None,
        }
    }

    fn write_price_list(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("pricing.json");
        std::fs::write(&path, PRICE_LIST).expect("scrivere il listino");
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
        Some(Pointer::Path(
            keys.iter().map(|k| (*k).to_owned()).collect(),
        ))
    }

    /// La ricetta di un motore che sa dire quanto ha consumato: chiede
    /// l'involucro e dichiara dove stanno i numeri, il modello e la risposta.
    fn declaring_recipe() -> AskRecipe {
        AskRecipe {
            args: Vec::new(),
            prompt: PromptVia::Stdin,
            args_before_prompt: Vec::new(),
            unusable_when: Vec::new(),
            silent_without_prompt: false,
            refuses_without_prompt: Vec::new(),
            exhausted_when: Vec::new(),
            cooldown_secs: None,
            usage: Some(UsageRecipe {
                args: vec!["--output-format".to_owned(), "json".to_owned()],
                declared: Declared {
                    read: Shape::Json,
                    from: models::usage::Heard::Stdout,
                    input_tokens: path(&["usage", "input_tokens"]),
                    output_tokens: path(&["usage", "output_tokens"]),
                    cached_tokens: path(&["usage", "cache_read_input_tokens"]),
                    cache_write_tokens: path(&["usage", "cache_creation_input_tokens"]),
                    cache_write_long_tokens: None,
                    total_tokens: None,
                    turns: None,
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
        // **UNA SOLA LETTURA DELLA PROIEZIONE, E NON È PIÙ QUI.**
        //
        // Fino al 01/09/2026 questo modulo teneva una copia privata di
        // `ui::parse::parse_model_call_row` — ventotto indici scritti a mano,
        // uguali a quelli dell'originale — perché «`actions` non dipende da
        // `ui`, e la dipendenza inversa sarebbe un ciclo». Non lo era: `ui` non
        // ha mai dipeso da `actions`, e comunque un ciclo di sole prove cargo lo
        // ammette apposta, com'è scritto nel `Cargo.toml` di `flow`.
        //
        // Il costo di quella copia era preciso: una colonna spostata avrebbe
        // fatto sbagliare **le due letture allo stesso modo**, e le prove che
        // confrontano l'una con l'altra sarebbero rimaste verdi. Adesso a
        // leggere è una sola, e a tenerla onesta c'è
        // `ledger::MODEL_CALL_DUMP_COLUMNS`, che non è né la lettura né la
        // scrittura.
        ui::parse::parse_model_calls(&dump)
    }

    /// Il listino vive in un file, e le prove non devono contendersi la casa di
    /// chi le esegue: `SAILOR_PRICING` lo sposta. Una serratura perché le prove
    /// girano in parallelo nello stesso processo e la variabile d'ambiente è una
    /// sola — senza, due prove si toglierebbero il listino a vicenda.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_price_list<T>(price_list: Option<&std::path::Path>, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match price_list {
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
    fn a_declaring_engine_writes_a_row_with_true_tokens_and_a_cost_from_the_price_list() {
        let dir = scratch("dichiara");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let outcome = with_price_list(Some(&price_list), || {
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
            models::pricing::PriceList::parse(PRICE_LIST)
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
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(AskRecipe {
                args: Vec::new(),
                prompt: PromptVia::Stdin,
                args_before_prompt: Vec::new(),
                unusable_when: Vec::new(),
                silent_without_prompt: false,
                refuses_without_prompt: Vec::new(),
                exhausted_when: Vec::new(),
                cooldown_secs: None,
                usage: None,
            }),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let outcome = with_price_list(Some(&price_list), || {
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
        let bin = fake_engine(
            &dir,
            "motore",
            "cat > /dev/null\necho 'è andata male' >&2\nexit 3",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
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

    /// **ESAURITO E ROTTO SONO DUE COSE, ANCHE QUANDO IL MOTORE È UNO SOLO.**
    ///
    /// Il guasto 14, per esteso. Il 29/08/2026 Claude era al limite settimanale
    /// e il passo si è fermato con un errore che diceva «uscito in errore»: chi
    /// l'ha letto è andato a cercare un guasto che non c'era, mentre la cosa da
    /// fare era aspettare le sette o cambiare motore. La distinzione esisteva
    /// già nel codice ma valeva **solo con una catena** (`!solo && ...`), cioè
    /// mai nel caso in cui è capitato.
    ///
    /// Si guarda in due posti perché sono due lettori diversi: la classe
    /// dell'errore la legge una persona adesso, `error_type` nel deposito la
    /// legge una somma fra un mese — e una somma che mescola le quote finite coi
    /// guasti veri non dice niente a nessuno.
    #[test]
    fn a_single_engine_that_ran_out_is_not_reported_as_broken() {
        let dir = scratch("esaurito-da-solo");
        let bin = fake_engine(
            &dir,
            "motore-esaurito",
            "cat > /dev/null\necho \"You've hit your weekly limit · resets 7am\"\nexit 1",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-esaurita", "passo-1"))
        })
        .expect_err("un motore esaurito e solo non può fare il lavoro");

        assert_eq!(
            error.class, "engine_exhausted",
            "non «engine_exit_error»: chi legge deve sapere che è finita la quota"
        );
        assert!(
            error.said.contains("quota"),
            "e il messaggio lo dice a parole: {}",
            error.said
        );

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "la chiamata ha bruciato quota: la riga c'è");
        assert_eq!(
            calls[0].error_type.as_deref(),
            Some("exhausted"),
            "e la riga distingue la quota finita da un guasto"
        );
    }

    /// **UN GUASTO VERO RESTA UN GUASTO.** La gemella della prova sopra: stesso
    /// motore solo, stessa ricetta con le stesse parole di esaurimento, ma
    /// un'uscita che quelle parole non le contiene. Senza questa, far dire
    /// «esaurito» a *qualunque* fallimento passerebbe verde.
    #[test]
    fn a_single_engine_that_truly_broke_is_still_reported_as_broken() {
        let dir = scratch("rotto-da-solo");
        let bin = fake_engine(
            &dir,
            "motore-rotto",
            "cat > /dev/null\necho 'errore: il mandato non ha senso' >&2\nexit 3",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-rotta", "passo-1"))
        })
        .expect_err("un'uscita diversa da zero rompe il passo");

        assert_eq!(error.class, "engine_exit_error");
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].error_type.as_deref(), Some("exit_error"));
    }

    /// A spent quota is its own class, and the engine is set aside for the
    /// time its descriptor declares: the second step in the same window does
    /// not knock on it. Without `exhausted_when` the same output stays the
    /// plain `exhausted` of before, and nobody is set aside.
    #[test]
    fn a_spent_quota_is_its_own_class_and_sets_the_engine_aside() {
        let dir = scratch("quota-spesa");
        let bin = fake_engine(
            &dir,
            "motore-a-secco",
            "cat > /dev/null\necho \"You've hit your weekly limit · resets 7am\"\nexit 1",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        recipe.exhausted_when = vec!["weekly limit".to_owned()];
        recipe.cooldown_secs = Some(1800);
        let aside = dir.join("cooldowns.json");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(recipe.clone()),
        })
        .recording_to(Some(ledger))
        .cooling_down_in(Some(aside.clone()));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-a-secco", "passo-1"))
        })
        .expect_err("a spent engine alone cannot do the work");
        assert_eq!(error.class, "engine_exhausted");
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].error_type.as_deref(), Some("quota_exhausted"));
        let set = cooldown::set_aside_until(&aside, "motore-di-prova", now_secs()).expect("set aside");
        assert!(set.said.contains("weekly limit"), "{set:?}");

        // The second knock is refused before spending, and says until when.
        let again = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-a-secco", "passo-2"))
        })
        .expect_err("an engine set aside is not tried");
        assert_eq!(again.class, "no_usable_engine");
        assert!(again.said.contains("set aside until"), "{}", again.said);
        assert_eq!(calls_in(&dir.join("deposito")).len(), 1, "nothing was spent on the second knock");

        // The control: the same words without `exhausted_when` are the old class, and nobody is aside.
        let plain = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(AskRecipe { exhausted_when: Vec::new(), cooldown_secs: None, ..recipe }),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito-2")).expect("second ledger")))
        .cooling_down_in(Some(dir.join("cooldowns-2.json")));
        with_price_list(None, || plain.execute(&input, &mut shared("corsa-piana", "passo-1")))
            .expect_err("still cannot work");
        assert_eq!(calls_in(&dir.join("deposito-2"))[0].error_type.as_deref(), Some("exhausted"));
        assert!(cooldown::set_aside_until(&dir.join("cooldowns-2.json"), "motore-di-prova", now_secs()).is_none());
    }

    /// A cap per engine on a window, declared by the person in a file: the
    /// first priced call fits, the second finds the window full and is refused
    /// before spending, naming the sum. A cap on another engine changes nothing.
    #[test]
    fn an_engine_over_its_budget_is_refused_before_spending() {
        let dir = scratch("tetto");
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let budgets = dir.join("budgets.json");
        // One priced call costs 18.30 $ and is checked before it is made: under
        // a cap of 10 $ the first goes through, and fills the window.
        std::fs::write(
            &budgets,
            r#"{"motore-di-prova": {"cap_micros": 10000000, "window_secs": 3600}}"#,
        )
        .expect("write the caps");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger))
        .budgeted_by(Some(budgets));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        with_price_list(Some(&price_list), || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("the first call fits under the cap");
        let refused = with_price_list(Some(&price_list), || {
            action.execute(&input, &mut shared("corsa-2", "passo-1"))
        })
        .expect_err("the second call finds the window full");
        assert_eq!(refused.class, "no_usable_engine");
        assert!(refused.said.contains("over its budget: spent 18.3000 $ of 10.0000 $"), "{}", refused.said);
        assert_eq!(calls_in(&dir.join("deposito")).len(), 1, "the refusal spent nothing");

        // The control: a cap declared for some other engine does not bind this one.
        let others = dir.join("budgets-others.json");
        std::fs::write(&others, r#"{"another-engine": {"cap_micros": 1, "window_secs": 3600}}"#)
            .expect("write the other caps");
        let unbound = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito")).expect("reopen")))
        .budgeted_by(Some(others));
        with_price_list(Some(&price_list), || {
            unbound.execute(&input, &mut shared("corsa-3", "passo-1"))
        })
        .expect("a cap on another engine is not this engine's");
        assert_eq!(calls_in(&dir.join("deposito")).len(), 2);
    }

    /// **A DOOR KNOWN TO BE SHUT IS NOT KNOCKED ON AGAIN.** The file's
    /// arithmetic was proved; the chain that fills it was not. Here a real
    /// engine says its quota is spent, and the next chain refuses it without
    /// starting it — naming until when, and what it said.
    #[test]
    fn an_engine_that_said_its_quota_was_spent_is_not_started_again() {
        let dir = scratch("da-parte");
        let bin = fake_engine(
            &dir,
            "motore-esaurito",
            "cat > /dev/null\necho \"You've hit your weekly limit\"\nexit 0",
        );
        let aside = dir.join("cooldowns.json");
        let mut recipe = declaring_recipe();
        recipe.exhausted_when = vec!["weekly limit".to_owned()];
        recipe.cooldown_secs = Some(3_600);
        let action = ExternalEngineAction::resolving_with(Declares {
            bin: bin.clone(),
            recipe: Some(recipe.clone()),
        })
        .recording_to(Some(Ledger::open(dir.join("deposito")).expect("aprire il deposito")))
        .cooling_down_in(Some(aside.clone()));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        // The first chain runs it, and it says the quota is spent.
        let broke = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect_err("a spent quota is not a step that went");
        assert_eq!(broke.class, "engine_exhausted");
        assert!(aside.exists(), "nothing was set aside: {}", broke.said);

        // The second chain does not start it at all: the refusal is the list's.
        let refused = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-2", "passo-1"))
        })
        .expect_err("the door is known to be shut");
        assert!(
            refused.said.contains("set aside until") && refused.said.contains("weekly limit"),
            "the refusal says neither until when nor what it said: {}",
            refused.said
        );
        assert_eq!(
            calls_in(&dir.join("deposito")).len(),
            1,
            "the second chain started the engine again"
        );

        // THE CONTROL: past its time the same engine is knocked on again, or
        // the code could set one aside for ever and pass. The instant is read
        // from the list and never recomputed: guessing it as `now + 3600`
        // guesses what the clock said when the file was written, and one second
        // of load left the file untouched and this arm red for nothing.
        let past = std::fs::read_to_string(&aside).expect("the list");
        let mut written: serde_json::Value = serde_json::from_str(&past).expect("the list is JSON");
        let now = now_secs();
        for (_, aside) in written.as_object_mut().expect("one entry per engine") {
            aside["until"] = json!(now - 1);
        }
        std::fs::write(&aside, serde_json::to_string(&written).expect("write it back"))
            .expect("bring its time forward");
        let again = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-3", "passo-1"))
        })
        .expect_err("it is spent again, but it was asked");
        assert!(
            !again.said.contains("set aside until"),
            "past its time it was still refused from the list: {}",
            again.said
        );
        assert_eq!(calls_in(&dir.join("deposito")).len(), 2);
    }

    /// An engine that resolves, with the pact its descriptor would declare.
    struct Pacted {
        bin: String,
        pact: models::pact::DataPact,
    }

    impl ToolResolver for Pacted {
        fn resolve(&self, _id: &str) -> Result<String, String> {
            Ok(self.bin.clone())
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(declaring_recipe())
        }
        fn data_pact(&self, _id: &str) -> models::pact::DataPact {
            self.pact
        }
    }

    /// A step that says its text is private never resolves to an engine whose
    /// pact is `trains` or `unknown`, and the refusal names the pact; the same
    /// step said public, or the same engine under `does_not_train`, runs.
    #[test]
    fn a_private_step_never_goes_where_the_pact_is_not_a_no() {
        use models::pact::DataPact;
        let dir = scratch("patto");
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let run = |pact: DataPact, input: serde_json::Value| {
            let action = ExternalEngineAction::resolving_with(Pacted { bin: bin.clone(), pact });
            with_price_list(None, || action.execute(&input, &mut shared("corsa", "passo")))
        };
        let private = json!({"tool": "motore-di-prova", "data": "private", "stdin": "ciao", "timeout_secs": 10});
        let public = json!({"tool": "motore-di-prova", "data": "public", "stdin": "ciao", "timeout_secs": 10});
        let unsaid = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let refused = run(DataPact::Trains, private.clone()).expect_err("a training engine is refused");
        assert_eq!(refused.class, "no_usable_engine");
        assert!(refused.said.contains("data pact is «trains»"), "{}", refused.said);
        let unknown = run(DataPact::Unknown, private.clone()).expect_err("unknown is not a no");
        assert!(unknown.said.contains("data pact is «unknown»"), "{}", unknown.said);

        run(DataPact::DoesNotTrain, private).expect("a pact that does not train may read it");
        run(DataPact::Trains, public).expect("a public step goes anywhere");
        run(DataPact::Unknown, unsaid).expect("a step that says nothing is public");
    }

    /// Two engines that both answer, told apart by the id on the ledger row.
    struct TwoEngines {
        bins: std::collections::BTreeMap<&'static str, String>,
    }

    impl ToolResolver for TwoEngines {
        fn resolve(&self, id: &str) -> Result<String, String> {
            self.bins.get(id).cloned().ok_or_else(|| format!("«{id}» is not here"))
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(declaring_recipe())
        }
    }

    /// An engine that answers on stdout and states its counts on stderr, the
    /// way a local model runner does: the descriptor says which pipe, and the
    /// row carries the counts; read from stdout instead, they stay unknown.
    #[test]
    fn counts_stated_on_stderr_are_read_when_the_descriptor_says_so() {
        let dir = scratch("stderr-counts");
        let bin = fake_engine(
            &dir,
            "locale",
            "cat > /dev/null\necho \"the answer\"\necho \"prompt eval count:    26 token(s)\" >&2\necho \"eval count:           298 token(s)\" >&2",
        );
        let recipe = |from: models::usage::Heard| AskRecipe {
            usage: Some(UsageRecipe {
                args: vec!["--verbose".to_owned()],
                declared: Declared {
                    read: Shape::Text,
                    from,
                    input_tokens: Some(Pointer::Pattern(r"prompt eval count:\s*(\d+)".to_owned())),
                    output_tokens: Some(Pointer::Pattern(r"(?m)^eval count:\s*(\d+)".to_owned())),
                    ..Declared::default()
                },
            }),
            ..declaring_recipe()
        };
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});
        let run = |from, ledger: &str| {
            let action = ExternalEngineAction::resolving_with(Declares { bin: bin.clone(), recipe: Some(recipe(from)) })
                .recording_to(Some(Ledger::open(dir.join(ledger)).expect("open")));
            with_price_list(None, || action.execute(&input, &mut shared("corsa", "passo"))).expect("answers");
            calls_in(&dir.join(ledger)).remove(0)
        };

        let heard = run(models::usage::Heard::Stderr, "deposito");
        assert_eq!((heard.input_tokens, heard.output_tokens), (Some(26), Some(298)));
        // The control: the same engine read on stdout states nothing.
        let unheard = run(models::usage::Heard::Stdout, "deposito-2");
        assert_eq!((unheard.input_tokens, unheard.output_tokens), (None, None));
    }

    /// Two engines with a subscription window each, read as fuel.
    struct Fuelled {
        bins: std::collections::BTreeMap<&'static str, String>,
        fuels: std::collections::BTreeMap<&'static str, models::fuel::Fuel>,
    }

    impl ToolResolver for Fuelled {
        fn resolve(&self, id: &str) -> Result<String, String> {
            self.bins.get(id).cloned().ok_or_else(|| format!("«{id}» is not here"))
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(declaring_recipe())
        }
        fn fuel(&self, id: &str) -> Vec<models::fuel::Fuel> {
            self.fuels.get(id).cloned().into_iter().collect()
        }
    }

    /// Under `prefer: fuel` the engine whose window expires unused soonest
    /// goes first even when the chain wrote it second; without it the chain
    /// stays as written.
    #[test]
    fn a_window_that_would_expire_unused_is_spent_first() {
        let dir = scratch("carburante");
        let long = fake_engine(&dir, "a-lungo", WRAPS_ON_DEMAND);
        let short = fake_engine(&dir, "a-breve", WRAPS_ON_DEMAND);
        let fuel = |engine: &str, left: f64, resets_in: i64| models::fuel::Fuel {
            engine: engine.to_owned(),
            unit: "five_hour".to_owned(),
            left_fraction: left,
            resets_in_secs: Some(resets_in),
        };
        let engines = || Fuelled {
            bins: [("a-lungo", long.clone()), ("a-breve", short.clone())].into_iter().collect(),
            fuels: [
                ("a-lungo", fuel("a-lungo", 0.80, 6 * 86_400)),
                ("a-breve", fuel("a-breve", 0.10, 3_600)),
            ]
            .into_iter()
            .collect(),
        };
        let by_fuel = json!({"tool": ["a-lungo", "a-breve"], "prefer": "fuel", "stdin": "ciao", "timeout_secs": 10});
        let as_written = json!({"tool": ["a-lungo", "a-breve"], "stdin": "ciao", "timeout_secs": 10});

        let action = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito")).expect("open")));
        with_price_list(None, || action.execute(&by_fuel, &mut shared("corsa-1", "passo")))
            .expect("the short window answers");
        assert_eq!(calls_in(&dir.join("deposito"))[0].cli, "a-breve");

        let plain = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito-2")).expect("open")));
        with_price_list(None, || plain.execute(&as_written, &mut shared("corsa-2", "passo")))
            .expect("the chain's first answers");
        assert_eq!(calls_in(&dir.join("deposito-2"))[0].cli, "a-lungo");

        // A word `prefer` does not know is refused by name, not read as silence.
        let by_luck = json!({"tool": ["a-lungo"], "prefer": "luck", "stdin": "ciao", "timeout_secs": 10});
        let refused = with_price_list(None, || plain.execute(&by_luck, &mut shared("corsa-3", "passo")))
            .expect_err("an unknown preference is refused");
        assert_eq!(refused.class, "invalid_input");
        assert!(refused.said.contains("«luck»"), "{}", refused.said);
    }

    /// A step that would start under a profile whose endpoint cannot be
    /// reached is refused before spending, with the profile's reason.
    #[test]
    fn a_profile_whose_endpoint_is_refused_holds_the_engine_back_before_spending() {
        let dir = scratch("endpoint-rifiutato");
        let bin = fake_engine(&dir, "codex", WRAPS_ON_DEMAND);
        let store = dir.join("profili.json");
        std::fs::write(
            &store,
            format!(
                r#"{{"profiles": [{{"name": "altrove", "cli_id": "codex", "home_dir": "{}",
                    "endpoint": {{"url": "http://localhost:1/v1", "key_var": "NO_SUCH_KEY_VAR_HERE",
                    "protocol": "anthropic-messages"}}}}],
                  "active": {{"codex": "altrove"}}}}"#,
                dir.join("casa").display()
            ),
        )
        .expect("write the store");
        let action = ExternalEngineAction::resolving_with(Declares { bin, recipe: Some(declaring_recipe()) });
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        std::env::set_var("PROFILES_STATE_PATH", &store);
        let refused = with_price_list(None, || action.execute(&input, &mut shared("corsa", "passo")));
        std::env::remove_var("PROFILES_STATE_PATH");

        let refused = refused.expect_err("a profile that cannot be pointed there refuses the launch");
        assert_eq!(refused.class, "no_usable_engine");
        assert!(refused.said.contains("anthropic-messages"), "{}", refused.said);
    }

    /// A step that declares its kind goes first to the engines the strengths
    /// table names for that kind, then to the chain as written; without a row
    /// for the kind the chain's first answers. The ledger row names the kind.
    #[test]
    fn a_kind_of_work_goes_first_where_the_table_says_and_the_ledger_names_it() {
        let dir = scratch("forze");
        let local = fake_engine(&dir, "locale", WRAPS_ON_DEMAND);
        let chained = fake_engine(&dir, "catena", WRAPS_ON_DEMAND);
        let engines = || TwoEngines {
            bins: [("locale", local.clone()), ("catena", chained.clone())].into_iter().collect(),
        };
        let table = dir.join("strengths.json");
        std::fs::write(&table, r#"{"measured_on": "a test", "rows": {"mechanical": ["locale"]}}"#)
            .expect("write the table");
        let empty = dir.join("strengths-empty.json");
        std::fs::write(&empty, r#"{"measured_on": "a test", "rows": {}}"#).expect("write the empty table");
        let input = json!({"tool": "catena", "kind": "mechanical", "stdin": "ciao", "timeout_secs": 10});

        let action = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito")).expect("open")))
            .strong_by(Some(table));
        with_price_list(None, || action.execute(&input, &mut shared("corsa-1", "passo")))
            .expect("the local engine answers");
        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls[0].cli, "locale", "the table's engine went first, ahead of the chain");
        assert_eq!(calls[0].work_kind.as_deref(), Some("mechanical"));

        // The control: without a row for the kind, the chain as written.
        let plain = ExternalEngineAction::resolving_with(engines())
            .recording_to(Some(Ledger::open(dir.join("deposito-2")).expect("open")))
            .strong_by(Some(empty));
        with_price_list(None, || plain.execute(&input, &mut shared("corsa-2", "passo")))
            .expect("the chain's engine answers");
        assert_eq!(calls_in(&dir.join("deposito-2"))[0].cli, "catena");
    }

    /// **E IL DEPOSITO DEVE DIRLO ANCHE QUANDO L'USCITA È ZERO.**
    ///
    /// La metà che non si vede dal comportamento del passo. Le due
    /// prove gemelle in `tests` guardano se il ripiego scatta; questa guarda la
    /// riga che resta scritta, ed è quella che qualcuno leggerà domani. Fino al
    /// 01/09/2026 nasceva con `error_type: None`, cioè **indistinguibile da una
    /// chiamata riuscita**: una somma che le mescola dice che quel motore ha
    /// risposto, e chi la legge non va a cercare niente.
    ///
    /// Senza questa prova un mutante che lascia scattare il ripiego ma scrive
    /// `None` invece di `exhausted` passerebbe sotto alle altre due.
    #[test]
    fn a_zero_exit_refusal_is_recorded_as_exhausted_not_as_a_clean_call() {
        let dir = scratch("esaurito-a-zero-nel-deposito");
        let bin = fake_engine(
            &dir,
            "motore-esaurito-a-zero",
            "cat > /dev/null\necho \"You've hit your weekly limit · resets 7am\"\nexit 0",
        );
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let mut recipe = declaring_recipe();
        recipe.unusable_when = vec!["weekly limit".to_owned()];
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(recipe),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let error = with_price_list(None, || {
            action.execute(&input, &mut shared("corsa-esaurita-a-zero", "passo-1"))
        })
        .expect_err("un motore che dice di non poter lavorare non ha risposto");
        assert_eq!(error.class, "engine_exhausted");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "la chiamata è stata fatta, e va registrata");
        assert_eq!(
            calls[0].error_type.as_deref(),
            Some("exhausted"),
            "un motore esaurito che esce zero non è una chiamata pulita: la riga \
             che lo dice è l'unica traccia che resta"
        );
    }

    /// **E LA SPECIE NON DIPENDE DA `accept`, IN NESSUNO DEI DUE RAMI.**
    ///
    /// La tolleranza di un passo riguarda **cosa fa la corsa** — se il
    /// fallimento è un dato che il passo si tiene, o una ragione per fermarsi —
    /// e non deve toccare **cosa resta scritto**. Nel ramo `ExitError` questo
    /// vale da sempre, perché lì il `note(...)` sta prima del controllo di
    /// tolleranza; nel primo tentativo di chiudere questo guasto, nel ramo `Ok`
    /// stava **dopo**, e con `accept: ["exit_error"]` dichiarato la riga
    /// nasceva di nuovo `NULL` — cioè indistinguibile da una risposta vera.
    /// Il difetto sopravviveva in un angolo del proprio rimedio, e l'ha trovato
    /// un giudice che non aveva scritto il lavoro.
    ///
    /// Le due metà stanno insieme apposta: sono la stessa affermazione — «la
    /// specie è la stessa e non dipende dalla tolleranza» — su tutti e due i
    /// codici d'uscita, ed è quella che il commento accanto al codice dichiara.
    #[test]
    fn a_tolerated_refusal_is_recorded_as_exhausted_whatever_the_exit_code() {
        for (name, exit, script) in [
            (
                "zero",
                0,
                "cat > /dev/null\necho \"You've hit your weekly limit\"\nexit 0",
            ),
            (
                "uno",
                1,
                "cat > /dev/null\necho \"You've hit your weekly limit\"\nexit 1",
            ),
        ] {
            let dir = scratch(&format!("esaurito-tollerato-{name}"));
            let bin = fake_engine(&dir, "motore-esaurito", script);
            let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
            let mut recipe = declaring_recipe();
            recipe.unusable_when = vec!["weekly limit".to_owned()];
            let action = ExternalEngineAction::resolving_with(Declares {
                bin,
                recipe: Some(recipe),
            })
            .recording_to(Some(ledger));
            // Il passo dichiara di volersi tenere il fallimento di questo
            // motore: la corsa non si ferma, e infatti il passo va avanti.
            let input = json!({
                "tool": "motore-di-prova",
                "stdin": "ciao",
                "timeout_secs": 10,
                "accept": ["exit_error"]
            });

            let outcome = with_price_list(None, || {
                action.execute(&input, &mut shared(&format!("corsa-{name}"), "passo-1"))
            })
            .unwrap_or_else(|error| {
                panic!(
                    "il passo tollera il fallimento, non deve rompersi: {}",
                    error.said
                )
            });
            assert!(
                matches!(outcome, ActionOutcome::Went(_)),
                "la tolleranza resta quella di prima: il passo prosegue (uscita {exit})"
            );

            let calls = calls_in(&dir.join("deposito"));
            assert_eq!(calls.len(), 1);
            assert_eq!(
                calls[0].error_type.as_deref(),
                Some("exhausted"),
                "uscita {exit}: il passo ha tollerato il fallimento, ma la riga del \
                 deposito deve dire lo stesso che quel motore non poteva lavorare. \
                 La tolleranza decide cosa fa la corsa, non cosa resta scritto"
            );
        }
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

        let error = with_price_list(None, || {
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
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", WRAPS_ON_DEMAND);
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let without = {
            let ledger = Ledger::open(dir.join("senza")).expect("deposito");
            let action = ExternalEngineAction::resolving_with(Declares {
                bin: bin.clone(),
                recipe: Some(AskRecipe {
                    args: Vec::new(),
                    prompt: PromptVia::Stdin,
                    args_before_prompt: Vec::new(),
                    unusable_when: Vec::new(),
                    silent_without_prompt: false,
                    refuses_without_prompt: Vec::new(),
                    exhausted_when: Vec::new(),
                    cooldown_secs: None,
                    usage: None,
                }),
            })
            .recording_to(Some(ledger));
            let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
                action.execute(&input, &mut shared("corsa-5", "passo-5"))
            })
            .expect("risponde") else {
                panic!("Went")
            };
            output
        };

        let with = {
            let ledger = Ledger::open(dir.join("con")).expect("deposito");
            let action = ExternalEngineAction::resolving_with(Declares {
                bin,
                recipe: Some(declaring_recipe()),
            })
            .recording_to(Some(ledger));
            let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
                action.execute(&input, &mut shared("corsa-6", "passo-6"))
            })
            .expect("risponde") else {
                panic!("Went")
            };
            output
        };

        assert_eq!(
            without, with,
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
        let mut only_the_step = SharedState::new();
        only_the_step.insert(flow::CURRENT_STEP.to_owned(), json!("passo-8"));
        assert!(action.execute(&input, &mut only_the_step).is_ok());
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

    /// **`cargo` E `git` NON SONO CHIAMATE A UN MODELLO, E IL DEPOSITO NON DEVE
    /// CONTARLE.**
    ///
    /// Misurato sul deposito di questa macchina il 31/08/2026: su ventiquattro
    /// righe di `model_calls`, due sono `git` e una `cargo`. Nessuna delle tre
    /// consuma quota di nessun abbonamento, e tutte e tre arrivano senza costo —
    /// quindi `Spend::is_complete()` è **falso su ogni corsa vera**, e la frase
    /// d'onestà del tetto («la spesa vera è più alta») si accende sempre, anche
    /// quando non c'è niente di ignoto. Un avviso sempre acceso non lo legge
    /// nessuno, ed è così che si perde quello vero — la riga di codex, che il
    /// costo davvero non lo dichiara.
    ///
    /// **CHI DECIDE È IL DESCRITTORE, NON UN ELENCO DI NOMI SCRITTO QUI.** Uno
    /// strumento è un motore se dichiara **come gli si fa una domanda**
    /// (`ask`): `git` e `cargo` non lo dichiarano, e nessun elenco di nomi qui
    /// dentro invecchierebbe bene. È la stessa regola del guasto 3 — quello che
    /// il catalogo dichiara vale più di quello che il codice indovina.
    #[test]
    fn a_tool_that_cannot_be_asked_anything_is_not_a_model_call() {
        let dir = scratch("non-e-un-motore");
        let bin = fake_engine(&dir, "finto-cargo", "printf 'ok'");
        let ledger = Ledger::open(dir.join("deposito")).expect("deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            // Nessuna ricetta `ask`: è com'è dichiarato `cargo` nel catalogo
            // spedito, e il passo si scrive le proprie opzioni.
            recipe: None,
        })
        .recording_to(Some(ledger));
        let input = json!({
            "tool": "motore-di-prova", "args": ["test"], "timeout_secs": 10
        });

        assert!(action
            .execute(&input, &mut shared("corsa-cargo", "prove"))
            .is_ok());

        assert!(
            calls_in(&dir.join("deposito")).is_empty(),
            "una riga di `cargo` nel conto delle chiamate ai modelli rende falso \
             ogni totale che la somma: {:?}",
            calls_in(&dir.join("deposito"))
        );
    }

    /// Le opzioni scritte dal passo vincono, e con loro il consumo resta
    /// sconosciuto: allungare alle spalle di chi ha scritto quella riga di
    /// comando una domanda che non ha fatto sarebbe decidere al posto suo. La
    /// riga però si scrive, e dice proprio questo.
    ///
    /// **È LA GEMELLA DELLA PROVA QUI SOPRA**, e le due vanno lette insieme: un
    /// motore vero interrogato con le opzioni del passo **resta** nel conto, e
    /// solo chi non è un motore ne esce. Senza questa, il filtro potrebbe
    /// svuotare la tabella e la prova sopra sarebbe verde lo stesso.
    #[test]
    fn when_the_step_writes_its_own_args_the_usage_is_not_asked_for() {
        let dir = scratch("args-del-passo");
        let price_list = write_price_list(&dir);
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

        let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
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
        let price_list = write_price_list(&dir);
        let bin = fake_engine(&dir, "motore", ALWAYS_WRAPS);
        let ledger = Ledger::open(dir.join("deposito")).expect("deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(AskRecipe {
                args: Vec::new(),
                prompt: PromptVia::Stdin,
                args_before_prompt: Vec::new(),
                unusable_when: Vec::new(),
                silent_without_prompt: false,
                refuses_without_prompt: Vec::new(),
                exhausted_when: Vec::new(),
                cooldown_secs: None,
                usage: None,
            }),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        let ActionOutcome::Went(output) = with_price_list(Some(&price_list), || {
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

    // ── (f) sotto quale dotazione la chiamata è girata ─────────────────

    /// La stessa serratura di `with_price_list`, per lo stato dei profili:
    /// `PROFILES_STATE_PATH` è una variabile sola come `SAILOR_PRICING`, e due
    /// prove che la scrivessero insieme si toglierebbero la dotazione a vicenda.
    /// Il listino si punta al vuoto apposta — qui si guarda il profilo, non il
    /// costo, e dipendere dal file di casa di chi esegue le prove sarebbe un
    /// modo di venire diversi senza che niente sia cambiato.
    fn with_profiles_state<T>(state: &std::path::Path, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::env::set_var("PROFILES_STATE_PATH", state);
        std::env::set_var(PRICING_ENV, "/nessun/listino/qui");
        let out = body();
        std::env::remove_var("PROFILES_STATE_PATH");
        std::env::remove_var(PRICING_ENV);
        out
    }

    /// **LA DOTAZIONE SOTTO CUI LA CHIAMATA È GIRATA FINISCE NELLA SUA RIGA.**
    ///
    /// Guasto 18, seconda metà. Senza, due corse dello stesso flusso non sono la
    /// stessa misura: la stessa catena di passi, sotto due profili, dà due
    /// consumi diversi per una ragione che la riga non porta. Fino al
    /// 01/09/2026 questa colonna la scriveva vuota ogni chiamata.
    ///
    /// **E IL PERCORSO DELLA CASA CI STA DENTRO**, che è il dato su cui una
    /// diagnostica si appoggia: un nome di profilo si riusa, si sposta e si
    /// cancella, un percorso è il posto dove si va a guardare.
    ///
    /// *Mutante eseguito*: rimettere `engine_identity: EngineIdentity::default()`
    /// in `record_the_call`. Questa diventa rossa e la gemella qui sotto resta
    /// verde — ed è per questo che ci sono tutte e due.
    #[test]
    fn the_row_says_under_which_equipment_the_call_ran() {
        let dir = scratch("dotazione");
        // Il nome del file È il legame: `cli_for_executable` riconosce la riga
        // di comando dall'eseguibile, non dall'identificativo del descrittore.
        let bin = fake_engine(&dir, "codex", ALWAYS_WRAPS);
        let state = dir.join("profili.json");
        std::fs::write(
            &state,
            json!({
                "profiles": [
                    {"name": "lavoro", "cli_id": "codex", "home_dir": dir.join("casa")}
                ],
                "active": {"codex": "lavoro"}
            })
            .to_string(),
        )
        .expect("scrivere lo stato dei profili");

        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        with_profiles_state(&state, || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("il motore risponde");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "una chiamata, una riga");
        assert_eq!(
            calls[0].engine_identity,
            EngineIdentity::ProfileInForce {
                cli_id: "codex".to_owned(),
                profile_name: "lavoro".to_owned(),
                home_dir: dir.join("casa"),
                endpoint: None,
            },
            "la riga non dice con quale identità la chiamata è girata"
        );
    }

    /// La gemella: senza nessun profilo attivo la riga dice **ereditata**, non un
    /// nome inventato e nemmeno un vuoto. Senza di lei un mutante che scrivesse
    /// sempre la stessa identità passerebbe la prova qui sopra.
    ///
    /// **«EREDITATA» È IL PUNTO DELLA CURA.** Prima qui c'era la stringa vuota,
    /// la stessa che usciva quando il binario non era un motore conosciuto,
    /// quando il profilo era sparito, e quando la casa non si sposta con una
    /// variabile. Quattro fatti diversi e un vuoto solo: adesso questo dice che
    /// il processo è partito con la casa di chi ha aperto il terminale, e quale
    /// riga di comando era.
    #[test]
    fn with_no_profile_in_force_the_row_says_the_identity_was_inherited() {
        let dir = scratch("nessuna-dotazione");
        let bin = fake_engine(&dir, "codex", ALWAYS_WRAPS);
        let state = dir.join("profili.json");
        std::fs::write(&state, r#"{"profiles":[],"active":{}}"#)
            .expect("scrivere lo stato dei profili");

        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({"tool": "motore-di-prova", "stdin": "ciao", "timeout_secs": 10});

        with_profiles_state(&state, || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("il motore risponde");

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(
            calls[0].engine_identity,
            EngineIdentity::InheritedFromTheTerminal {
                cli_id: "codex".to_owned()
            }
        );
    }

    /// Un finto `codex` che dice **con quale casa è partito davvero**: la scrive
    /// su un file accanto a sé, e poi risponde nell'involucro come gli altri.
    /// Senza questo file la prova qui sotto guarderebbe solo il deposito, cioè
    /// solo metà del difetto.
    const WRITES_DOWN_ITS_HOME: &str = r#"cat > /dev/null
printf '%s' "$CODEX_HOME" > "$(dirname "$0")/casa"
printf '{"result":"la risposta vera","model":"modello-di-prova","usage":{"input_tokens":1,"output_tokens":1}}'"#;

    /// **IL DEPOSITO REGISTRA UN'IDENTITÀ CHE IL PROCESSO NON HA USATO.**
    ///
    /// Che il passo vinca è la decisione, non il difetto. Il difetto è che la
    /// riga continua a nominare il profilo attivo: il motore è partito nella
    /// casa scritta nel passo, e chi legge il deposito per sapere con quali
    /// credenziali quel processo ha girato legge il nome di un profilo che non è
    /// mai stato messo in forza. È il caso in cui qualcuno ha cambiato identità
    /// apposta — cioè esattamente quello che una diagnostica o un controllo di
    /// sicurezza esiste per vedere — ed è il caso in cui il dato mente.
    ///
    /// **LE DUE METÀ SI GUARDANO INSIEME.** Cosa ha ricevuto il processo, e cosa
    /// dice la riga. Separate, ognuna delle due resta verde col difetto dentro.
    #[test]
    fn the_row_does_not_name_a_profile_the_step_replaced() {
        let dir = scratch("dotazione-scavalcata");
        let bin = fake_engine(&dir, "codex", WRITES_DOWN_ITS_HOME);
        let state = dir.join("profili.json");
        std::fs::write(
            &state,
            json!({
                "profiles": [
                    {"name": "lavoro", "cli_id": "codex", "home_dir": dir.join("casa-del-profilo")}
                ],
                "active": {"codex": "lavoro"}
            })
            .to_string(),
        )
        .expect("scrivere lo stato dei profili");

        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        let input = json!({
            "tool": "motore-di-prova",
            "stdin": "ciao",
            "env": {"CODEX_HOME": "/una/casa/scritta/nel/passo"},
            "timeout_secs": 10
        });

        with_profiles_state(&state, || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("il motore risponde");

        let home_it_started_in =
            std::fs::read_to_string(dir.join("casa")).expect("il motore ha scritto la sua casa");
        assert_eq!(
            home_it_started_in, "/una/casa/scritta/nel/passo",
            "il verso della sovrapposizione è cambiato: il profilo ha scavalcato il passo"
        );

        let calls = calls_in(&dir.join("deposito"));
        assert_eq!(calls.len(), 1, "una chiamata, una riga");
        assert_eq!(
            calls[0].engine_identity,
            EngineIdentity::ChosenByTheStep {
                cli_id: "codex".to_owned(),
                home_dir: PathBuf::from("/una/casa/scritta/nel/passo"),
            },
            "la riga nomina un'identità che il processo non ha usato: è partito in {home_it_started_in}"
        );
    }

    /// **IL GETTONE NON ENTRA IN NESSUN CAMPO DELL'IDENTITÀ.**
    ///
    /// Un passo può portare nel proprio ambiente qualunque variabile, chiavi
    /// comprese. Ciò che finisce nel deposito è **quale casa** e **come è stata
    /// scelta**, mai cosa c'era intorno: una riga di registro si legge in una
    /// diagnostica, si copia in un rapporto e si manda a qualcuno.
    ///
    /// *Mutante eseguito*: vedi la consegna — far portare all'identità l'intero
    /// ambiente rende rossa questa e nessun'altra.
    #[test]
    fn no_secret_from_the_step_ends_up_in_the_recorded_identity() {
        let dir = scratch("nessun-gettone");
        let bin = fake_engine(&dir, "codex", ALWAYS_WRAPS);
        let state = dir.join("profili.json");
        std::fs::write(
            &state,
            json!({
                "profiles": [
                    {"name": "lavoro", "cli_id": "codex", "home_dir": dir.join("casa")}
                ],
                "active": {"codex": "lavoro"}
            })
            .to_string(),
        )
        .expect("scrivere lo stato dei profili");

        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            recipe: Some(declaring_recipe()),
        })
        .recording_to(Some(ledger));
        // Un gettone riconoscibile: se comparisse da qualche parte, si vede.
        let input = json!({
            "tool": "motore-di-prova",
            "stdin": "ciao",
            "env": {"OPENAI_API_KEY": "sk-questo-non-deve-comparire"},
            "timeout_secs": 10
        });

        with_profiles_state(&state, || {
            action.execute(&input, &mut shared("corsa-1", "passo-1"))
        })
        .expect("il motore risponde");

        let calls = calls_in(&dir.join("deposito"));
        let written = calls[0].engine_identity.to_column();
        assert!(
            !written.contains("sk-questo-non-deve-comparire"),
            "un gettone del passo è finito nell'identità registrata: {written}"
        );
        assert!(
            !calls[0].engine_identity.to_string().contains("sk-"),
            "un gettone del passo è finito in ciò che si stampa a una persona"
        );
    }
}

#[cfg(test)]
mod resuming_instead_of_rediscovering {
    //! Le prove della ripresa: un passo continua la sessione di un altro invece
    //! di riaprire un processo che non sa niente.
    //!
    //! **NESSUN MOTORE VERO.** I motori qui dentro sono script di shell che
    //! scrivono la propria riga di comando su un file: quello che si prova è
    //! **cosa arriva al motore** e **cosa resta nel deposito**, che sono le due
    //! cose su cui questo lavoro sta o cade. Quanto si risparmi in token lo
    //! dice una corsa vera, non una prova: qui non si può misurare e non si
    //! finge di farlo.

    use super::*;
    use ledger::Ledger;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sailor-sessione-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("cartella di lavoro");
        dir
    }

    /// Un motore che scrive **in coda** la riga di comando con cui è stato
    /// invocato: in coda perché una prova sola lo chiama quattro volte, e
    /// sovrascrivere terrebbe solo l'ultima.
    const LOGS_ITS_ARGUMENTS: &str = r#"cat > /dev/null
printf '%s\n' "$*" >> "$(dirname "$0")/invocations"
printf 'ok'"#;

    /// Un motore che, oltre a registrare la riga, **annuncia** la sessione con
    /// cui sta parlando — e ne annuncia una diversa a ogni invocazione, come fa
    /// un motore vero quando ramifica.
    const ANNOUNCES_ITS_SESSION: &str = r#"cat > /dev/null
here="$(dirname "$0")"
printf '%s\n' "$*" >> "$here/invocations"
n=$(cat "$here/counter" 2>/dev/null || echo 0)
n=$((n + 1))
printf '%s' "$n" > "$here/counter"
printf 'session id: sessione-%s\nok\n' "$n""#;

    fn fake_engine(dir: &std::path::Path) -> String {
        engine_that(dir, LOGS_ITS_ARGUMENTS)
    }

    fn engine_that(dir: &std::path::Path, body: &str) -> String {
        let path = dir.join("engine");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("scrivere il finto motore");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("renderlo eseguibile");
        path.to_string_lossy().into_owned()
    }

    fn invocations(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("invocations"))
            .expect("il motore finto ha scritto le proprie invocazioni")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn shared(run: &str, step: &str) -> SharedState {
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_RUN.to_owned(), json!(run));
        shared.insert(flow::CURRENT_STEP.to_owned(), json!(step));
        shared
    }

    const TOOL: &str = "motore-di-prova";

    /// Un risolutore che dichiara la ricetta della domanda e — separatamente —
    /// cosa quel motore sa fare con le proprie sessioni. Le due cose viaggiano
    /// separate anche nella vita vera.
    struct Declares {
        bin: String,
        sessions: Option<SessionRecipe>,
    }

    impl ToolResolver for Declares {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                TOOL => Ok(self.bin.clone()),
                other => Err(format!("«{other}» non è su questa macchina")),
            }
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(AskRecipe {
                args: vec!["--ask".to_owned()],
                prompt: PromptVia::Stdin,
                args_before_prompt: Vec::new(),
                unusable_when: Vec::new(),
                silent_without_prompt: false,
                refuses_without_prompt: Vec::new(),
                exhausted_when: Vec::new(),
                cooldown_secs: None,
                usage: None,
            })
        }
        fn session_recipe(&self, _id: &str) -> Option<SessionRecipe> {
            self.sessions.clone()
        }
    }

    /// Un motore che sa tutti e tre i modi, come `claude-code`.
    fn knows_all_three() -> SessionRecipe {
        SessionRecipe {
            open: Some(vec![
                "--ask".to_owned(),
                "--session-id".to_owned(),
                SESSION_PLACEHOLDER.to_owned(),
            ]),
            resume: Some(vec![
                "--ask".to_owned(),
                "--resume".to_owned(),
                SESSION_PLACEHOLDER.to_owned(),
            ]),
            fork: Some(vec![
                "--ask".to_owned(),
                "--resume".to_owned(),
                SESSION_PLACEHOLDER.to_owned(),
                "--fork-session".to_owned(),
            ]),
            id_from: None,
        }
    }

    /// Un motore che l'identificativo se lo conia da sé e lo **stampa**, come
    /// `codex`: apre con la riga di sempre, e il nome si va a leggere.
    fn mints_its_own() -> SessionRecipe {
        SessionRecipe {
            open: Some(vec!["--ask".to_owned()]),
            resume: Some(vec![
                "--ask".to_owned(),
                "resume".to_owned(),
                SESSION_PLACEHOLDER.to_owned(),
            ]),
            fork: Some(vec![
                "--ask".to_owned(),
                "fork".to_owned(),
                SESSION_PLACEHOLDER.to_owned(),
            ]),
            id_from: Some(Pointer::Pattern("session id: ([0-9a-z-]+)".to_owned())),
        }
    }

    fn step_that(session: Value) -> Value {
        json!({
            "tool": TOOL,
            "stdin": "guarda l'albero",
            "timeout_secs": 20,
            "session": session,
        })
    }

    fn ran(action: &ExternalEngineAction, input: &Value, run: &str, step: &str) {
        match action.execute(input, &shared(run, step)) {
            Ok(ActionOutcome::Went(_)) => {}
            other => panic!("il passo «{step}» doveva andare: {other:?}"),
        }
    }

    /// **CHI APRE POSA L'IDENTIFICATIVO NEL DEPOSITO, O NESSUNO POTRÀ
    /// RIPRENDERLO.** Le due metà si provano insieme di proposito: un
    /// identificativo passato al motore e non registrato è indistinguibile,
    /// dal passo dopo, da una sessione mai aperta.
    #[test]
    fn a_step_that_opens_a_session_hands_it_to_the_engine_and_writes_it_down() {
        let dir = scratch("apre");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-1", "scopri");

        let line = invocations(&dir).remove(0);
        let written = ledger
            .session_opened_by("corsa-1", "scopri", TOOL)
            .expect("il deposito risponde")
            .expect("e ha registrato la sessione");
        assert!(
            line.contains("--session-id") && line.contains(&written),
            "il motore deve ricevere lo stesso identificativo che il deposito conserva: \
             riga «{line}», registrato «{written}»"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **IL CASO CHE RENDE DI PIÙ, ED È IL MOTIVO DI TUTTO IL LAVORO.** Tre
    /// passi indipendenti guardano lo stesso albero nello stesso momento: senza
    /// ramificazione fanno tre scoperte identiche e le pagano tre volte. Qui
    /// devono ricevere tutti e tre lo stesso tronco, e ognuno il proprio ramo.
    ///
    /// **E OGNUNO DEI TRE DEVE REGISTRARE UNA SESSIONE IGNOTA.** Ramificare
    /// conia un identificativo che il motore non ci dice: scrivere lì quello
    /// del padre farebbe riprendere il tronco a chi crede di stare sul proprio
    /// ramo — in silenzio, che è il modo peggiore.
    #[test]
    fn three_independent_steps_fork_one_discovery_instead_of_doing_it_three_times() {
        let dir = scratch("ramifica");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-2", "scopri");
        for step in ["struttura", "rischi", "attrito"] {
            ran(
                &action,
                &step_that(json!({ "fork": "scopri" })),
                "corsa-2",
                step,
            );
        }

        let trunk = ledger
            .session_opened_by("corsa-2", "scopri", TOOL)
            .expect("il deposito risponde")
            .expect("il tronco è registrato");
        let lines = invocations(&dir);
        assert_eq!(lines.len(), 4, "una scoperta e tre rami");
        for line in &lines[1..] {
            assert!(
                line.contains(&trunk) && line.contains("--fork-session"),
                "ogni ramo parte dal tronco senza continuarlo: «{line}»"
            );
        }
        for step in ["struttura", "rischi", "attrito"] {
            assert_eq!(
                ledger
                    .session_opened_by("corsa-2", step, TOOL)
                    .expect("il deposito risponde"),
                None,
                "il ramo di «{step}» ha un identificativo che il motore non ci ha detto"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Chi riprende continua la **stessa** sessione, e la lascia in eredità:
    /// tre passi in fila devono poter continuare l'uno dall'altro.
    #[test]
    fn resuming_keeps_the_same_session_so_the_next_step_can_take_it_too() {
        let dir = scratch("riprende");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-3", "scopri");
        ran(
            &action,
            &step_that(json!({ "resume": "scopri" })),
            "corsa-3",
            "piano",
        );
        ran(
            &action,
            &step_that(json!({ "resume": "piano" })),
            "corsa-3",
            "implementa",
        );

        let trunk = ledger
            .session_opened_by("corsa-3", "scopri", TOOL)
            .expect("il deposito risponde")
            .expect("il tronco è registrato");
        assert_eq!(
            ledger
                .session_opened_by("corsa-3", "piano", TOOL)
                .expect("risponde"),
            Some(trunk.clone()),
            "chi riprende non cambia sessione, e per questo la può passare avanti"
        );
        let lines = invocations(&dir);
        assert!(
            lines[2].contains(&trunk) && !lines[2].contains("--fork-session"),
            "il terzo passo continua lo stesso tronco: «{}»",
            lines[2]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **IL VINCOLO PERMANENTE, PROVATO.** Un motore che non sa ramificare non
    /// diventa rosso e non diventa un caso speciale: riceve la riga di sempre,
    /// riparte da zero e paga di più. Se un giorno qualcuno facesse fallire il
    /// passo «per non nascondere il problema», questa prova lo prenderebbe.
    #[test]
    fn an_engine_that_cannot_fork_starts_over_instead_of_breaking() {
        let dir = scratch("non-sa-ramificare");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(SessionRecipe {
                open: knows_all_three().open,
                resume: None,
                fork: None,
                id_from: None,
            }),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-4", "scopri");
        ran(
            &action,
            &step_that(json!({ "fork": "scopri" })),
            "corsa-4",
            "rischi",
        );

        let lines = invocations(&dir);
        assert_eq!(
            lines[1], "--ask",
            "chi non sa ramificare riceve la riga di sempre, non una riga monca"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Un motore che non dichiara **niente** sulle sessioni funziona come
    /// prima, anche quando il passo chiede di aprirne una: è il caso di tre dei
    /// quattro motori installati su questa macchina.
    #[test]
    fn an_engine_that_declares_no_sessions_works_exactly_as_before() {
        let dir = scratch("muto");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: None,
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-5", "scopri");

        assert_eq!(invocations(&dir), vec!["--ask".to_owned()]);
        assert_eq!(
            ledger
                .session_opened_by("corsa-5", "scopri", TOOL)
                .expect("risponde"),
            None,
            "non c'è nessuna sessione da registrare, e non se ne inventa una"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ramificare da un passo che non ha lasciato niente riparte da zero. È il
    /// caso di chi scrive `{"fork": "un-passo-che-non-c-e"}` — un refuso — e di
    /// chi ramifica da un passo che quel giorno è finito su un altro motore.
    #[test]
    fn forking_from_a_step_that_left_no_session_starts_over() {
        let dir = scratch("nessun-tronco");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger));

        ran(
            &action,
            &step_that(json!({ "fork": "un-passo-che-non-c-e" })),
            "corsa-6",
            "rischi",
        );

        assert_eq!(invocations(&dir), vec!["--ask".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **UN MOTORE CHE IL NOME SE LO DÀ DA SÉ NON È UN MOTORE ESCLUSO.**
    /// Verificato il 31/08/2026 su `codex`, che non ha nessuna opzione per
    /// imporre un identificativo e lo **stampa**: senza questa via i motori che
    /// coniano da sé sarebbero fuori da una capacità che hanno.
    ///
    /// E il ramo diventa **continuabile a sua volta**: il terzo passo ramifica
    /// dal secondo, non dal primo. Senza leggere l'identificativo del ramo, una
    /// catena di tre passi tornerebbe di colpo alla scoperta iniziale, e nessun
    /// errore lo direbbe — arriverebbe solo un contesto sbagliato.
    #[test]
    fn an_engine_that_names_its_own_session_is_read_and_its_branch_is_continuable() {
        let dir = scratch("si-nomina-da-se");
        let bin = engine_that(&dir, ANNOUNCES_ITS_SESSION);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(mints_its_own()),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-7", "scopri");
        ran(
            &action,
            &step_that(json!({ "fork": "scopri" })),
            "corsa-7",
            "rischi",
        );
        ran(
            &action,
            &step_that(json!({ "fork": "rischi" })),
            "corsa-7",
            "dettaglio",
        );

        assert_eq!(
            ledger
                .session_opened_by("corsa-7", "scopri", TOOL)
                .expect("risponde"),
            Some("sessione-1".to_owned()),
            "l'identificativo lo dice il motore, non lo decidiamo noi"
        );
        assert_eq!(
            ledger
                .session_opened_by("corsa-7", "rischi", TOOL)
                .expect("risponde"),
            Some("sessione-2".to_owned()),
            "e il ramo ha il proprio, non quello del tronco"
        );
        let lines = invocations(&dir);
        assert_eq!(
            lines[1], "--ask fork sessione-1",
            "il ramo parte dal tronco"
        );
        assert_eq!(
            lines[2], "--ask fork sessione-2",
            "e il ramo dopo parte dal ramo"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **NON SI CONIA UN NOME CHE NON SI PUÒ CONSEGNARE.** Un motore che apre
    /// con la riga di sempre non riceve nessun identificativo: scriverne uno
    /// nostro nel deposito farebbe riprendere al passo dopo una sessione che su
    /// quella macchina non esiste — e se ne accorgerebbe dopo aver speso.
    #[test]
    fn a_session_we_cannot_name_is_not_named_by_us() {
        let dir = scratch("nome-non-consegnabile");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("aprire il deposito");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            // Apre con la riga di sempre e non dice dove scrive il proprio
            // nome: è il caso di chi non ha nessuna delle due vie.
            sessions: Some(SessionRecipe {
                open: Some(vec!["--ask".to_owned()]),
                ..SessionRecipe::default()
            }),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-8", "scopri");

        assert_eq!(invocations(&dir), vec!["--ask".to_owned()]);
        assert_eq!(
            ledger
                .session_opened_by("corsa-8", "scopri", TOOL)
                .expect("risponde"),
            None,
            "una sessione che non si sa nominare resta senza nome nel deposito"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Due chiamate non ricevono mai lo stesso identificativo: se lo
    /// ricevessero, due sessioni diverse si scriverebbero addosso sul disco di
    /// chi esegue, e il passo dopo riprenderebbe un miscuglio.
    #[test]
    fn two_sessions_never_get_the_same_identifier() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..1000 {
            assert!(
                seen.insert(fresh_session_id()),
                "un identificativo ripetuto"
            );
        }
        // E la forma è quella che le righe di comando chiedono: cinque gruppi
        // separati da trattini, la versione al posto giusto.
        let one = fresh_session_id();
        let groups: Vec<&str> = one.split('-').collect();
        assert_eq!(
            groups.iter().map(|group| group.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12],
            "«{one}» non ha la forma di un UUID"
        );
        assert!(one.starts_with(|c: char| c.is_ascii_hexdigit()));
        assert!(groups[2].starts_with('4'), "la versione: «{one}»");
    }
}
