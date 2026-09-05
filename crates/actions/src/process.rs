//! Running a child within a time limit, and handing its output to whoever
//! watches while it runs: the primitive both actions stand on.

use flow::{Ran, SharedState};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// Il destinatario del passo in corso, se qualcuno sta guardando.
///
/// **PERCHÉ L'IDENTIFICATIVO ARRIVA DALLO STATO CONDIVISO.** `Action::execute`
/// non riceve il passo, e cambiargli la firma toccherebbe ogni implementatore in
/// cinque crate per un dato che serve a uno solo. L'esecutore lo scrive in
/// `SharedState` sotto una chiave riservata (`flow::CURRENT_STEP`) prima di ogni
/// azione. Senza guardiano, o senza quella chiave, non si guarda: un testo
/// consegnato senza sapere di chi è sarebbe peggio del silenzio, perché in un
/// grafo con due passi vivi nessuno saprebbe attribuirlo.
pub(crate) fn sink_for_step(
    watcher: &Option<Arc<dyn StepSinks>>,
    shared: &SharedState,
) -> Option<Arc<dyn LiveSink>> {
    let watcher = watcher.as_ref()?;
    let step = shared.get(flow::CURRENT_STEP)?.as_str()?;
    Some(watcher.sink_for(step))
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

impl EngineInvocation {
    /// The line as `invoke_external_engine` starts it, for the record.
    pub fn ran(&self) -> Ran {
        Ran::new(&self.bin, &self.args)
    }
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

impl CheckInvocation {
    /// The program and the arguments a check is started with: the shell, told
    /// to read the command as text. One place, read by the spawn and by the
    /// record alike, so the two cannot drift apart.
    fn command_line(&self) -> (&'static str, [&str; 2]) {
        ("sh", ["-c", self.command.as_str()])
    }

    /// The line as `run_shell_check` starts it, for the record.
    pub fn ran(&self) -> Ran {
        let (program, args) = self.command_line();
        Ran::new(program, args)
    }
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
    let (program, args) = invocation.command_line();
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null());
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
