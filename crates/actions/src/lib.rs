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

pub mod reference;
pub mod store;

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies, ValueSchema};
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
    /// Col motivo del sistema operativo. «Non si è avviato» da solo manda a
    /// cercare un binario assente quando il file c'era e non era eseguibile:
    /// sono due riparazioni diverse, e chi legge deve poterle distinguere.
    SpawnFailed(String),
}

/// Niente `timeout(1)`: non esiste su ogni macchina che esegue questi
/// binari. Il tetto è un ciclo di `try_wait` con `kill` alla scadenza, e due
/// fili drenano le pipe man mano — un figlio che le riempie prima che
/// qualcuno le legga resterebbe bloccato in scrittura per sempre.
pub fn run_with_timeout(mut cmd: Command, limit: Duration) -> RunOutcome {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(error) => return RunOutcome::SpawnFailed(error.to_string()),
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
        Err(error) => return RunOutcome::SpawnFailed(error.to_string()),
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
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&invocation.command).stdin(Stdio::null());
    for (key, value) in &invocation.env {
        cmd.env(key, value);
    }
    match run_with_timeout(cmd, invocation.timeout) {
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

#[derive(Debug, Deserialize)]
struct EngineSpec {
    /// Il comando così com'è. Resta per un comando qualunque — `sh`, `cat`, uno
    /// script — non per un motore: un motore si chiede per identificativo, o il
    /// flusso gira solo dove quel nome è nel percorso di chi esegue.
    #[serde(default)]
    bin: Option<String>,
    /// L'identificativo dello strumento voluto, lo stesso che il rilevatore
    /// della macchina restituisce.
    #[serde(default)]
    tool: Option<String>,
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
        Self { tools: None }
    }

    /// Con un risolutore: `"tool": "codex"` diventa il percorso che vale
    /// `codex` su questa macchina.
    pub fn resolving_with(resolver: impl ToolResolver + 'static) -> Self {
        Self {
            tools: Some(Arc::new(resolver)),
        }
    }

    /// Chi eseguire. `bin` e `tool` non convivono: due risposte alla stessa
    /// domanda vorrebbero una precedenza, e una precedenza fra «il nome che ho
    /// scritto» e «quello che c'è sulla macchina» sarebbe una regola che nessuno
    /// ricorda al momento giusto.
    fn executable(&self, spec: &EngineSpec) -> Result<String, ActionError> {
        match (spec.bin.as_deref(), spec.tool.as_deref()) {
            (Some(bin), None) => Ok(bin.to_owned()),
            (None, Some(id)) => match &self.tools {
                Some(tools) => tools
                    .resolve(id)
                    .map_err(|reason| ActionError::new("tool_unavailable", reason)),
                None => Err(ActionError::new(
                    "no_tool_resolver",
                    format!(
                        "il passo chiede lo strumento «{id}», ma questo motore è stato registrato \
                         senza un modo per risolverlo: chi costruisce il registro deve registrare \
                         `{EXTERNAL_ENGINE_ACTION}` con `ExternalEngineAction::resolving_with(...)`"
                    ),
                )),
            },
            (Some(_), Some(_)) => Err(ActionError::new(
                "invalid_input",
                "il passo dichiara sia `bin` sia `tool`: uno solo dei due dice chi eseguire",
            )),
            (None, None) => Err(ActionError::new(
                "invalid_input",
                "il passo non dice chi eseguire: serve `tool` (l'identificativo di uno strumento) \
                 oppure `bin` (un comando così com'è)",
            )),
        }
    }
}

impl Action for ExternalEngineAction {
    fn execute(
        &self,
        input: &Value,
        _shared: &mut SharedState,
    ) -> Result<ActionOutcome, ActionError> {
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
        // Prima di spendere qualunque cosa: se lo strumento non c'è, il passo si
        // ferma qui e dice quale mancava.
        let bin = self.executable(&spec)?;
        let seconds = spec.timeout_secs;
        let invocation = EngineInvocation {
            bin: bin.clone(),
            args: spec.args,
            env: spec.env,
            workdir: spec.workdir,
            stdin: spec.stdin.map(String::into_bytes),
            timeout: Duration::from_secs(seconds),
        };
        let shape = spec.answer_shape.as_ref();
        let outcome = match invoke_external_engine(&invocation) {
            EngineResult::Ok { stdout, stderr } => match shape {
                Some(shape) => return shaped_answer(shape, &stdout)
                    .map(|answer| ActionOutcome::Went(json!({"status": "ok", "answer": answer}))),
                None => EngineOutcomeJson {
                    status: "ok",
                    stdout,
                    stderr,
                },
            },
            EngineResult::ExitError {
                code,
                stdout,
                stderr,
            } => {
                if !tolerates(&spec.accept, "exit_error") {
                    return Err(ActionError::new(
                        "engine_exit_error",
                        format!(
                            "`{bin}` {}; {}",
                            how_it_exited(code),
                            what_it_said(&stdout, &stderr)
                        ),
                    ));
                }
                match shape {
                    // Un motore che ha parlato deve rispettare la forma anche
                    // quando il passo gli perdona l'uscita in errore: quella
                    // tolleranza riguarda il codice di uscita, non la risposta.
                    Some(shape) => {
                        return shaped_answer(shape, &stdout).map(|answer| {
                            ActionOutcome::Went(json!({"status": "exit_error", "answer": answer}))
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
                if !tolerates(&spec.accept, "timed_out") {
                    return Err(ActionError::new(
                        "engine_timed_out",
                        format!("`{bin}` non ha risposto entro {seconds} secondi ed è stato ucciso"),
                    ));
                }
                EngineOutcomeJson {
                    status: "timed_out",
                    stdout: String::new(),
                    stderr: String::new(),
                }
            }
            EngineResult::SpawnFailed { reason } => {
                if !tolerates(&spec.accept, "spawn_failed") {
                    return Err(ActionError::new(
                        "engine_spawn_failed",
                        format!("`{bin}` non si è potuto avviare: {reason}"),
                    ));
                }
                EngineOutcomeJson {
                    status: "spawn_failed",
                    stdout: String::new(),
                    stderr: reason,
                }
            }
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
pub struct ShellCheckAction;

impl Action for ShellCheckAction {
    fn execute(
        &self,
        input: &Value,
        _shared: &mut SharedState,
    ) -> Result<ActionOutcome, ActionError> {
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
        let status = match run_shell_check(&invocation) {
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
        let action = ShellCheckAction;
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
            ShellCheckAction
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
        let error = ShellCheckAction
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
        let ActionOutcome::Went(output) = ShellCheckAction
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
