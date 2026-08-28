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

pub mod store;

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Il nome sotto cui `ExternalEngineAction` si registra in un
/// `flow::ActionRegistry`.
pub const EXTERNAL_ENGINE_ACTION: &str = "external_engine";
/// Il nome sotto cui `ShellCheckAction` si registra.
pub const SHELL_CHECK_ACTION: &str = "shell_check";

/// Registra entrambe le azioni sotto i loro nomi stabili: la scorciatoia per
/// chi vuole entrambe senza scegliere i nomi a mano.
pub fn register_default(registry: &mut flow::ActionRegistry) {
    registry.register(EXTERNAL_ENGINE_ACTION, ExternalEngineAction);
    registry.register(SHELL_CHECK_ACTION, ShellCheckAction);
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
    SpawnFailed,
}

/// Niente `timeout(1)`: non esiste su ogni macchina che esegue questi
/// binari. Il tetto è un ciclo di `try_wait` con `kill` alla scadenza, e due
/// fili drenano le pipe man mano — un figlio che le riempie prima che
/// qualcuno le legga resterebbe bloccato in scrittura per sempre.
pub fn run_with_timeout(mut cmd: Command, limit: Duration) -> RunOutcome {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return RunOutcome::SpawnFailed,
    };
    drain_and_wait(child.stdout.take(), child.stderr.take(), &mut child, limit)
}

/// Come `run_with_timeout`, ma scrive un testo sullo standard input del
/// figlio subito dopo averlo avviato, poi lo chiude — un motore che legge il
/// proprio ingresso da lì (come lo script di prova per OpenRouter) altrimenti
/// resterebbe in attesa di un EOF che non arriva mai.
pub fn run_with_timeout_and_stdin(mut cmd: Command, stdin: &[u8], limit: Duration) -> RunOutcome {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return RunOutcome::SpawnFailed,
    };
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(stdin);
        // `pipe` esce di scope qui e chiude il descrittore: il figlio vede
        // l'EOF anche se non ha altro da leggere.
    }
    drain_and_wait(child.stdout.take(), child.stderr.take(), &mut child, limit)
}

fn drain_and_wait(
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    child: &mut std::process::Child,
    limit: Duration,
) -> RunOutcome {
    let mut out_pipe = stdout.expect("stdout è piped");
    let mut err_pipe = stderr.expect("stderr è piped");
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });
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
pub enum EngineResult {
    Ok { stdout: String, stderr: String },
    ExitError { stdout: String, stderr: String },
    TimedOut,
    SpawnFailed,
}

pub fn invoke_external_engine(invocation: &EngineInvocation) -> EngineResult {
    let mut cmd = Command::new(&invocation.bin);
    cmd.args(&invocation.args);
    for (key, value) in &invocation.env {
        cmd.env(key, value);
    }
    if let Some(workdir) = &invocation.workdir {
        cmd.current_dir(workdir);
    }
    let outcome = match &invocation.stdin {
        Some(bytes) => run_with_timeout_and_stdin(cmd, bytes, invocation.timeout),
        None => {
            cmd.stdin(Stdio::null());
            run_with_timeout(cmd, invocation.timeout)
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
                EngineResult::ExitError { stdout, stderr }
            }
        }
        RunOutcome::TimedOut => EngineResult::TimedOut,
        RunOutcome::SpawnFailed => EngineResult::SpawnFailed,
    }
}

// ── eseguire una verifica con un tempo massimo ───────────────────────────

pub struct CheckInvocation {
    pub command: String,
    pub env: BTreeMap<String, String>,
    pub timeout: Duration,
}

pub enum CheckResult {
    Passed,
    Failed,
    TimedOut,
}

/// Esegue `command` con `sh -c`: la verifica di un compito è testo di shell
/// scritto da chi lo definisce, non un binario risolto a monte.
pub fn run_shell_check(invocation: &CheckInvocation) -> CheckResult {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&invocation.command).stdin(Stdio::null());
    for (key, value) in &invocation.env {
        cmd.env(key, value);
    }
    match run_with_timeout(cmd, invocation.timeout) {
        RunOutcome::Finished { status, .. } => {
            if status.success() {
                CheckResult::Passed
            } else {
                CheckResult::Failed
            }
        }
        RunOutcome::TimedOut => CheckResult::TimedOut,
        // Un binario `sh` che non parte è un guasto dell'ambiente, non della
        // verifica: si tratta come fallita, non come "passata per omissione".
        RunOutcome::SpawnFailed => CheckResult::Failed,
    }
}

// ── le due azioni registrabili in un flow::ActionRegistry ───────────────

#[derive(Debug, Deserialize)]
struct EngineSpec {
    bin: String,
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
    timeout_secs: u64,
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
/// Non fallisce mai con un `ActionError`: un'uscita diversa da zero, uno
/// scadere del tempo o un binario che non parte sono tutti dati del mondo,
/// non un guasto dell'azione — chi legge l'uscita (`status`) decide cosa
/// contano. Solo un ingresso che non si legge come `EngineSpec` è un errore
/// dell'azione stessa.
pub struct ExternalEngineAction;

impl Action for ExternalEngineAction {
    fn execute(
        &self,
        input: &Value,
        _shared: &mut SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        let spec: EngineSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let invocation = EngineInvocation {
            bin: spec.bin,
            args: spec.args,
            env: spec.env,
            workdir: spec.workdir,
            stdin: spec.stdin.map(String::into_bytes),
            timeout: Duration::from_secs(spec.timeout_secs),
        };
        let result = invoke_external_engine(&invocation);
        let outcome = match result {
            EngineResult::Ok { stdout, stderr } => EngineOutcomeJson {
                status: "ok",
                stdout,
                stderr,
            },
            EngineResult::ExitError { stdout, stderr } => EngineOutcomeJson {
                status: "exit_error",
                stdout,
                stderr,
            },
            EngineResult::TimedOut => EngineOutcomeJson {
                status: "timed_out",
                stdout: String::new(),
                stderr: String::new(),
            },
            EngineResult::SpawnFailed => EngineOutcomeJson {
                status: "spawn_failed",
                stdout: String::new(),
                stderr: String::new(),
            },
        };
        Ok(ActionOutcome::Went(json!(outcome)))
    }

    /// Non dichiara di potersi rifare, e quindi finisce a una persona.
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
    timeout_secs: u64,
}

/// Esegue una verifica di shell con un tempo massimo, leggendo comando,
/// ambiente e tetto dall'ingresso tipato del passo. Stessa regola
/// dell'azione gemella: il risultato della verifica (passata, fallita,
/// scaduta) è un dato nell'uscita, non un `ActionError`.
pub struct ShellCheckAction;

impl Action for ShellCheckAction {
    fn execute(
        &self,
        input: &Value,
        _shared: &mut SharedState,
    ) -> Result<ActionOutcome, ActionError> {
        let spec: CheckSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let invocation = CheckInvocation {
            command: spec.command,
            env: spec.env,
            timeout: Duration::from_secs(spec.timeout_secs),
        };
        let status = match run_shell_check(&invocation) {
            CheckResult::Passed => "passed",
            CheckResult::Failed => "failed",
            CheckResult::TimedOut => "timed_out",
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
            RunOutcome::SpawnFailed
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
            EngineResult::SpawnFailed
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
        assert!(matches!(run_shell_check(&invocation), CheckResult::Failed));
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
        let action = ExternalEngineAction;
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

    /// Un'uscita diversa da zero resta `Went`, con lo stato che lo dice: chi
    /// costruisce il grafo decide se questo è un rosso, non l'azione.
    #[test]
    fn the_external_engine_action_reports_exit_error_as_data_not_as_failure() {
        let action = ExternalEngineAction;
        let input = json!({"bin": "sh", "args": ["-c", "exit 3"], "timeout_secs": 5});
        let mut shared = SharedState::new();
        let ActionOutcome::Went(output) = action.execute(&input, &mut shared).unwrap() else {
            panic!("resta Went anche in errore di uscita")
        };
        assert_eq!(output["status"], "exit_error");
    }

    #[test]
    fn the_external_engine_action_rejects_an_input_without_a_binary() {
        let action = ExternalEngineAction;
        let input = json!({"timeout_secs": 5});
        let mut shared = SharedState::new();
        assert!(action.execute(&input, &mut shared).is_err());
    }

    #[test]
    fn the_shell_check_action_reads_its_json_input() {
        let action = ShellCheckAction;
        let input = json!({"command": "true", "timeout_secs": 5});
        let mut shared = SharedState::new();
        let ActionOutcome::Went(output) = action.execute(&input, &mut shared).unwrap() else {
            panic!("una verifica eseguita è sempre Went")
        };
        assert_eq!(output["status"], "passed");
    }

    #[test]
    fn the_registry_finds_both_actions_by_their_stable_names() {
        let mut registry = flow::ActionRegistry::default();
        register_default(&mut registry);
        assert!(registry.get(EXTERNAL_ENGINE_ACTION).is_some());
        assert!(registry.get(SHELL_CHECK_ACTION).is_some());
    }
}
