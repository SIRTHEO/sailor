//! Il nodo con cui un passo interroga un server MCP, e la verifica che viene
//! prima.
//!
//! **PERCHÉ ESISTE.** `docs/da-fare.md` lo chiamava «l'anello mancante»: Sailor
//! *riconosce* i server MCP — il rilevatore ha la famiglia `mcp_server` — ma
//! nessuna delle nove azioni registrate sapeva parlarci. Un flusso che voleva
//! chiedere a SocratiCode «cosa toccherebbe questo cambiamento» doveva uscire
//! dal grafo e diventare uno script, cioè esattamente ciò che questa casa non
//! fa.
//!
//! **COSA STA NEL CODICE E COSA NO.** Qui dentro c'è solo ciò che tocca il
//! mondo: aprire un processo, dirgli le parole della stretta di mano, leggere le
//! righe che risponde. Quale server, quale strumento, quali argomenti e quali
//! verifiche preliminari sono **dati del passo** — non c'è una sola costante che
//! nomini SocratiCode, e le prove di questo file girano contro un server finto
//! costruito in una cartella temporanea.
//!
//! **LA STRETTA DI MANO NON È UN'OFFERTA, ED È LA DISTINZIONE PRINCIPALE.**
//! Misurato il 31/08/2026: `claude mcp list` dichiarava `socraticode: ✔
//! Connected` mentre la sessione che lo interrogava non aveva affatto quello
//! strumento. Un nodo che si fida di quel segnale lavora su un indice
//! inesistente senza accorgersene. Qui il server viene aperto da Sailor e gli si
//! chiede `tools/list`: «risponde» e «offre lo strumento che mi serve» restano
//! due fatti separati, con due parole separate — `unreachable` e
//! `tool_not_offered` — e il messaggio del secondo dice perché non sono la
//! stessa cosa.
//!
//! **LO STANDARD INPUT RESTA APERTO FINCHÉ LA RISPOSTA NON ARRIVA.** Non è un
//! dettaglio di stile: misurato lo stesso giorno contro `npx -y socraticode`,
//! scrivendo tutte le richieste e chiudendo subito lo standard input — cioè
//! quello che fa `run_with_timeout_and_stdin`, la primitiva che c'era già —
//! torna **solo** la risposta a `initialize`, e le `tools/call` restano senza
//! risposta e senza errore. Il server muore sull'EOF prima di aver finito. È il
//! motivo per cui questo file ha un proprio dialogo invece di riusare quella
//! primitiva: là lo standard input si chiude per contratto.
//!
//! **QUATTRO ESITI, NON UNO.** «Il server non c'è», «il server c'è ma non offre
//! questo strumento», «una verifica preliminare dice di no», «una verifica
//! preliminare non ha potuto guardare» sono quattro fatti diversi, e il quinto è
//! «lo strumento ha risposto che non lo sa». Confonderli è il difetto che questa
//! casa chiama *«non c'è» non è sempre una misura*: dove non si è potuto
//! guardare la risposta è «non ho potuto guardare», col motivo.
//!
//! **E `could_not_look` batte `check_failed`.** Se una verifica è cieca e
//! un'altra è negativa, l'esito complessivo è la cecità. Dire `check_failed`
//! significherebbe affermare «ho guardato tutto e una cosa era sbagliata», e
//! quella frase non si può pronunciare quando una delle guardate non è
//! avvenuta: un ignoto può nascondere qualunque cosa, anche di peggio.
//!
//! **LA VERIFICA NON SI PUÒ SALTARE, E LO IMPONE IL CODICE.** Un passo dichiara
//! `project_root` — la cartella di cui pretende di parlare — e almeno una delle
//! verifiche deve legare la risposta del server a quella cartella, cioè avere
//! `project_root` dentro il proprio `proves`. Chi non ce l'ha non parte:
//! `no_preflight`. Un progetto indicizzato non è tutti i progetti, e un indice
//! giusto per un'altra cartella risponde con sicurezza su codice che qui non
//! esiste. Chi interroga un server che di cartelle non sa niente lo dichiara per
//! iscritto con `checks_waived_because`, e quella frase resta nell'uscita.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Il nome sotto cui si registra la sola verifica preliminare.
pub const MCP_READY_ACTION: &str = "mcp_ready";
/// Il nome sotto cui si registra l'interrogazione vera.
pub const MCP_ASK_ACTION: &str = "mcp_ask";

/// Registra i due nodi che parlano con un server MCP.
pub fn register_mcp(registry: &mut flow::ActionRegistry) {
    registry.register(MCP_READY_ACTION, McpReadyAction);
    registry.register(MCP_ASK_ACTION, McpAskAction);
}

/// La regola che viaggia con ogni risposta, invece di dipendere da chi scrive
/// il prompt.
///
/// **PERCHÉ STA NELL'USCITA E NON IN UN DOCUMENTO.** Sta anche in
/// `docs/2026-08-28-sailor-si-sviluppa-su-se-stesso.md`, sezione 6, che dice
/// testualmente che questa regola va nel prompt del passo che usa SocratiCode.
/// Un documento però lo legge chi c'era: chi userà questo nodo fra sei mesi
/// scriverà il proprio prompt senza averlo mai aperto. Uscendo da qui, la regola
/// entra nel prompt del passo successivo con un rinvio — `{"$from": "/caveat"}`
/// — e non dipende più dalla memoria di nessuno.
pub const CAVEAT: &str = "This answer comes from an external index: it is for finding your bearings, not for deciding. A blast radius — who depends on what, what breaks if you touch this — is decided by the tool that compiles, never by the index's dependency graph. Measured: asked about the impact of «crates/flow/src/graph.rs», the index answered «no callers, nothing depends on this», while that file has 493 lines, eight «Cargo.toml» declare the crate and 22 files use it. It was a false orphan, on the file at the centre of the flow format.";

/// Gli esiti che un passo può dichiarare di tollerare con `accept`.
///
/// `ready` e `ok` non ci sono: non sono fallimenti, e nominarli in `accept`
/// sarebbe un refuso da far vedere subito invece che una tolleranza.
const READY_FAILURES: &[&str] = &[
    "unreachable",
    "tool_not_offered",
    "could_not_look",
    "check_failed",
];

const ASK_FAILURES: &[&str] = &[
    "unreachable",
    "tool_not_offered",
    "could_not_look",
    "check_failed",
    "tool_failed",
];

// ── ciò che il passo dichiara ────────────────────────────────────────────

/// Come si avvia il server. È un comando, perché un server MCP che parla su
/// standard input e standard output è un processo figlio: chi lo dichiara scrive
/// le stesse tre cose che stanno in un `.mcp.json`.
#[derive(Debug, Clone, Deserialize)]
struct ServerSpec {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// Da quale cartella avviarlo. Un server che indicizza codice guarda spesso
    /// la cartella corrente, e lasciarla a quella di chi esegue il flusso vuol
    /// dire non sapere di quale progetto sta parlando.
    #[serde(default)]
    cwd: Option<String>,
}

/// Una verifica preliminare: una domanda al server, e cosa deve rispondere.
///
/// **`blind_if` SI GUARDA PRIMA DI DICHIARARE UN NO, E NON È UN DETTAGLIO.**
/// Misurato il 31/08/2026 contro SocratiCode: con l'archivio vettoriale spento,
/// `codebase_list_projects` risponde `{"content":[{"type":"text","text":"Could
/// not connect to Qdrant."}]}` — un risultato **riuscito**, senza `isError`.
/// Quel testo non contiene il percorso del progetto, quindi senza `blind_if` un
/// indice che non si è potuto leggere diventerebbe «il progetto non è
/// indicizzato»: un'affermazione sul mondo che nessuno ha verificato.
///
/// L'ordine dei tre casi è: la prova positiva, poi la cecità, poi il no. Il
/// primo viene prima perché una risposta che contiene ciò che si cercava l'ha
/// mostrato davvero, qualunque altra cosa dica; il no viene ultimo perché è
/// l'unico che afferma qualcosa sul mondo, e si pronuncia solo quando gli altri
/// due sono esclusi.
#[derive(Debug, Clone, Deserialize)]
struct PreflightCheck {
    /// Come si chiama il fatto che si sta verificando, per chi legge l'uscita.
    name: String,
    server_tool: String,
    #[serde(default = "empty_object")]
    arguments: Value,
    /// Il testo che la risposta deve contenere perché il fatto sia provato.
    proves: String,
    /// I testi che, se compaiono, dicono che il server **non ha potuto
    /// guardare**. Vuoto è ammesso, e allora resta solo l'errore dichiarato del
    /// server a distinguere la cecità dal no.
    #[serde(default)]
    blind_if: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ReadySpec {
    server: ServerSpec,
    /// Lo strumento che il passo userà davvero. Si chiede qui perché «il server
    /// risponde» non è «il server offre questo».
    ///
    /// **NON SI CHIAMA `tool`, E NON È UNA SCELTA DI GUSTO.** `sailor flow
    /// check` legge il campo `tool` di **ogni** passo, qualunque sia l'azione, e
    /// lo tratta come l'identificativo di uno strumento da risolvere sulla
    /// macchina — `flow_cmd::tools_wanted` lo dice per iscritto: «è il nome del
    /// campo a dire che quello è un identificativo di strumento». Uno strumento
    /// MCP non è uno strumento di Sailor: chiamandolo `tool`, ogni flusso che
    /// usa questi nodi verrebbe **rifiutato** dal controllo statico con
    /// «strumenti che nessun descrittore dichiara», e la riparazione sarebbe
    /// nell'altro crate.
    server_tool: String,
    /// La cartella di cui il passo pretende di parlare.
    project_root: String,
    #[serde(default)]
    checks: Vec<PreflightCheck>,
    #[serde(default)]
    checks_waived_because: String,
    timeout_secs: u64,
    #[serde(default)]
    accept: Vec<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct AskSpec {
    server: ServerSpec,
    /// Vedi `ReadySpec::server_tool` per il motivo per cui non si chiama `tool`.
    server_tool: String,
    #[serde(default = "empty_object")]
    arguments: Value,
    project_root: String,
    #[serde(default)]
    checks: Vec<PreflightCheck>,
    #[serde(default)]
    checks_waived_because: String,
    timeout_secs: u64,
    #[serde(default)]
    accept: Vec<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn empty_object() -> Value {
    json!({})
}

/// Nessuna verifica lega la risposta alla cartella dichiarata: il passo non
/// parte.
///
/// **PERCHÉ È UN ERRORE DI CHI SCRIVE IL FLUSSO E NON UN ESITO.** Un esito si
/// può tollerare con `accept`, e la tolleranza qui vorrebbe dire «interroga
/// pure un indice che non sai di chi sia». Un progetto indicizzato non è tutti i
/// progetti: senza questo legame la risposta è plausibile e riguarda un'altra
/// cartella, e niente lo segnala.
fn require_preflight(
    checks: &[PreflightCheck],
    waived_because: &str,
    project_root: &str,
) -> Result<(), ActionError> {
    // **UNA PROVA VUOTA NON PROVA NIENTE, E PASSEREBBE SEMPRE.** Ogni testo
    // contiene la stringa vuota: un `proves` vuoto renderebbe la verifica verde
    // qualunque cosa il server risponda, compreso «non riesco a raggiungere
    // l'archivio». È il difetto che questa casa chiama «un controllo che non
    // controlla niente», e qui si presenterebbe da solo, senza che nessuno lo
    // scriva apposta: basta un `{"$from": …}` che punta a un campo vuoto.
    for check in checks {
        if check.proves.is_empty() {
            return Err(ActionError::new(
                "invalid_input",
                format!(
                    "check «{}» does not say what it has to prove: an empty «proves» is contained in any answer, so it would always pass",
                    check.name
                ),
            ));
        }
    }
    if !waived_because.trim().is_empty() {
        return Ok(());
    }
    // Stessa ragione, un gradino più su: con un `project_root` vuoto il legame
    // fra la risposta e la cartella sarebbe soddisfatto da qualunque verifica.
    if project_root.is_empty() {
        return Err(ActionError::new(
            "no_preflight",
            "the step does not say which directory it is about: an empty «project_root» is contained in any «proves», and the tie this check enforces would become a formality",
        ));
    }
    let ties_to_the_root = checks
        .iter()
        .any(|check| check.proves.contains(project_root));
    if ties_to_the_root {
        return Ok(());
    }
    Err(ActionError::new(
        "no_preflight",
        format!(
            "no preliminary check ties the answer to «{project_root}»: it needs at least one «check» whose «proves» contains that path, or a written «checks_waived_because» saying why this server knows nothing about directories"
        ),
    ))
}

// ── il dialogo col server ────────────────────────────────────────────────

const INITIALIZE_ID: u64 = 1;
const LIST_ID: u64 = 2;
/// Da qui in su gli identificativi delle verifiche, una per numero.
const FIRST_CHECK_ID: u64 = 10;
/// L'identificativo della chiamata vera, tenuto lontano dagli altri perché si
/// riconosca a occhio in un registro.
const ERRAND_ID: u64 = 100;

/// Quanto stderr del server si tiene per il messaggio d'errore. Un server che
/// vomita un log intero non deve riempire il deposito.
const KEPT_STDERR: usize = 2000;

/// Una conversazione aperta con un server MCP.
///
/// Vive quanto il passo: si apre, si fanno le domande in due tempi — prima le
/// verifiche, poi la chiamata vera solo se le verifiche passano — e si chiude.
/// Il figlio muore in `Drop` anche uscendo per una strada d'errore.
struct Session {
    child: std::process::Child,
    /// Tenuto in vita apposta: chiuderlo fa uscire il server prima che risponda.
    stdin: Option<std::process::ChildStdin>,
    lines: mpsc::Receiver<String>,
    errors: Option<std::thread::JoinHandle<String>>,
    deadline: Instant,
}

impl Session {
    fn open(server: &ServerSpec, limit: Duration) -> Result<Session, String> {
        let mut command = Command::new(&server.command);
        command.args(&server.args);
        for (name, value) in &server.env {
            command.env(name, value);
        }
        if let Some(dir) = &server.cwd {
            command.current_dir(dir);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        in_its_own_group(&mut command);
        let mut child = command.spawn().map_err(|error| {
            // Il motivo del sistema operativo, com'è già la regola per
            // `SpawnFailed`: «non si è avviato» da solo manda a cercare un
            // binario assente quando il file c'era e non era eseguibile.
            format!("«{}» did not start: {error}", server.command)
        })?;
        let stdin = child.stdin.take();
        let out = child.stdout.take();
        let err = child.stderr.take();
        let (sender, lines) = mpsc::channel();
        if let Some(out) = out {
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines() {
                    match line {
                        Ok(line) => {
                            if sender.send(line).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
        let errors = err.map(|mut err| {
            std::thread::spawn(move || {
                let mut all = Vec::new();
                let _ = err.read_to_end(&mut all);
                all.truncate(KEPT_STDERR);
                String::from_utf8_lossy(&all).into_owned()
            })
        });
        Ok(Session {
            child,
            stdin,
            lines,
            errors,
            deadline: Instant::now() + limit,
        })
    }

    fn say(&mut self, request: &Value) -> Result<(), String> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err("the server's standard input is already closed".to_owned());
        };
        writeln!(stdin, "{request}").map_err(|error| error.to_string())?;
        stdin.flush().map_err(|error| error.to_string())
    }

    /// Aspetta le risposte agli identificativi chiesti, fino alla scadenza.
    ///
    /// Torna quello che è arrivato: chi chiama distingue «assente» da
    /// «negativa», e le due cose non si possono confondere qui dentro.
    fn listen_for(&self, wanted: &[u64]) -> BTreeMap<u64, Value> {
        let mut answers: BTreeMap<u64, Value> = BTreeMap::new();
        while answers.len() < wanted.len() {
            let left = self.deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    let Ok(value) = serde_json::from_str::<Value>(&line) else {
                        // Un server può scrivere righe che non sono JSON-RPC.
                        // Non sono un guasto: non sono una risposta.
                        continue;
                    };
                    if let Some(id) = value.get("id").and_then(Value::as_u64) {
                        if wanted.contains(&id) {
                            answers.insert(id, value);
                        }
                    }
                }
                // Scaduto, o il server ha chiuso la propria uscita: in tutti e
                // due i casi non arriverà altro.
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }
        answers
    }

    /// Chiude e restituisce ciò che il server ha scritto su stderr, che è
    /// l'unico posto dove un server rotto spiega perché.
    fn close(mut self) -> String {
        self.stdin = None;
        signal_the_whole_group(self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
        match self.errors.take() {
            Some(handle) => handle.join().unwrap_or_default(),
            None => String::new(),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stdin = None;
        signal_the_whole_group(self.child.id());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A server starts workers, so it is given a group of its own to lead.
#[cfg(unix)]
fn in_its_own_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn in_its_own_group(_command: &mut Command) {}

/// The signal goes to the group, which carries the leader's number: the minus
/// sign is what tells `kill` so. The twins live in `actions::run_with_timeout`
/// and `supervisor::child`. Known limit: a worker that calls `setsid` on its
/// own leaves the group and survives.
#[cfg(unix)]
fn signal_the_whole_group(pid: u32) {
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn signal_the_whole_group(_pid: u32) {}

fn initialize_request() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": INITIALIZE_ID,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "sailor", "version": "0.1.0"},
        },
    })
}

fn initialized_notification() -> Value {
    json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
}

fn list_request() -> Value {
    json!({"jsonrpc": "2.0", "id": LIST_ID, "method": "tools/list"})
}

fn call_request(id: u64, tool: &str, arguments: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments},
    })
}

// ── leggere una risposta ─────────────────────────────────────────────────

/// Cosa è tornato da una domanda.
///
/// `Unanswered` non è `Said` con dentro un errore: la prima è «non ho una
/// risposta», la seconda è «il server ha risposto qualcosa». Tenerle in due
/// varianti è ciò che impedisce a una domanda caduta nel vuoto di diventare una
/// risposta negativa.
enum Answer {
    Said { text: String, refused: bool },
    Unanswered(String),
}

fn read_answer(response: Option<&Value>) -> Answer {
    let Some(response) = response else {
        return Answer::Unanswered(
            "the server did not answer this question within the time given".to_owned(),
        );
    };
    if let Some(error) = response.get("error") {
        let said = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("an error with no message");
        return Answer::Unanswered(format!("the server refused the question: {said}"));
    }
    let result = response.get("result");
    let refused = result
        .and_then(|result| result.get("isError"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = result
        .and_then(|result| result.get("content"))
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    Answer::Said { text, refused }
}

/// Gli strumenti che il server dichiara di offrire.
fn offered_tools(response: Option<&Value>) -> Option<Vec<String>> {
    let tools = response?.get("result")?.get("tools")?.as_array()?;
    Some(
        tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .map(str::to_owned)
            .collect(),
    )
}

/// Il verdetto su una verifica preliminare: passata, negativa, o cieca.
fn judge(check: &PreflightCheck, answer: &Answer) -> (&'static str, String) {
    match answer {
        Answer::Unanswered(why) => ("could_not_look", why.clone()),
        Answer::Said { text, refused } => {
            if *refused {
                return (
                    "could_not_look",
                    format!("«{}» answered with an error: {text}", check.server_tool),
                );
            }
            if text.contains(&check.proves) {
                return (
                    "passed",
                    format!("«{}» risponde e contiene «{}»", check.server_tool, check.proves),
                );
            }
            if let Some(blinded) = check
                .blind_if
                .iter()
                .find(|phrase| !phrase.is_empty() && text.contains(phrase.as_str()))
            {
                return (
                    "could_not_look",
                    format!(
                        "the server could not look — «{}» in the answer of «{}»: {text}",
                        blinded, check.server_tool
                    ),
                );
            }
            (
                "failed",
                format!(
                    "«{}» answered, and «{}» does not appear: {text}",
                    check.server_tool, check.proves
                ),
            )
        }
    }
}

/// L'esito della verifica preliminare, prima che si chiami lo strumento vero.
struct Preflight {
    status: &'static str,
    said: String,
    checks: Vec<Value>,
    /// Gli strumenti offerti, elencati solo quando servono a chi ripara: cioè
    /// quando quello chiesto non c'è. Un elenco di venticinque nomi in ogni
    /// uscita riuscita è rumore che nessuno legge.
    offered: Option<Vec<String>>,
    offered_count: Option<usize>,
}

/// Apre il dialogo, fa la stretta di mano, chiede l'elenco degli strumenti e
/// esegue le verifiche dichiarate. **Non chiama lo strumento vero**: quella
/// chiamata è del chiamante, e parte solo se qui esce `ready`.
fn preflight(session: &mut Session, tool: &str, checks: &[PreflightCheck]) -> Preflight {
    let mut requests = vec![
        initialize_request(),
        initialized_notification(),
        list_request(),
    ];
    for (position, check) in checks.iter().enumerate() {
        requests.push(call_request(
            FIRST_CHECK_ID + position as u64,
            &check.server_tool,
            &check.arguments,
        ));
    }
    for request in &requests {
        if let Err(why) = session.say(request) {
            return Preflight {
                status: "unreachable",
                said: format!("the server could not be spoken to: {why}"),
                checks: Vec::new(),
                offered: None,
                offered_count: None,
            };
        }
    }
    let mut wanted = vec![INITIALIZE_ID, LIST_ID];
    for position in 0..checks.len() {
        wanted.push(FIRST_CHECK_ID + position as u64);
    }
    let answers = session.listen_for(&wanted);

    if answers.get(&INITIALIZE_ID).is_none() {
        return Preflight {
            status: "unreachable",
            said: "the server did not answer the handshake".to_owned(),
            checks: Vec::new(),
            offered: None,
            offered_count: None,
        };
    }
    let Some(offered) = offered_tools(answers.get(&LIST_ID)) else {
        return Preflight {
            // Il server c'è e risponde, ma non ha saputo dire cosa offre: non
            // si può affermare né che offra lo strumento né che non lo offra.
            status: "could_not_look",
            said: "the server answers the handshake and did not list its own tools: there is no telling whether it offers the one needed".to_owned(),
            checks: Vec::new(),
            offered: None,
            offered_count: None,
        };
    };
    let count = offered.len();
    if !offered.iter().any(|name| name == tool) {
        return Preflight {
            status: "tool_not_offered",
            said: format!(
                "the server answers, and does not offer «{tool}» to this session. Answering and offering are two different facts: an external listing that calls it connected does not prove this session has the tool. What it does offer: {}",
                if offered.is_empty() { "niente".to_owned() } else { offered.join(", ") }
            ),
            checks: Vec::new(),
            offered: Some(offered),
            offered_count: Some(count),
        };
    }

    let mut reported = Vec::new();
    let mut blind = 0usize;
    let mut refused = 0usize;
    for (position, check) in checks.iter().enumerate() {
        let answer = read_answer(answers.get(&(FIRST_CHECK_ID + position as u64)));
        let (state, said) = judge(check, &answer);
        match state {
            "could_not_look" => blind += 1,
            "failed" => refused += 1,
            _ => {}
        }
        reported.push(json!({
            "name": check.name,
            "server_tool": check.server_tool,
            "state": state,
            "said": said,
        }));
    }
    // La cecità viene prima del no: vedi il commento in testa al file.
    let (status, said) = if blind > 0 {
        (
            "could_not_look",
            format!("{blind} preliminary checks out of {} could not look: where looking was not possible the answer is «I do not know», not «no»", checks.len()),
        )
    } else if refused > 0 {
        (
            "check_failed",
            format!(
                "{refused} verifiche preliminari su {} dicono di no",
                checks.len()
            ),
        )
    } else {
        (
            "ready",
            format!(
                "the server offers «{tool}» and {} preliminary checks pass",
                checks.len()
            ),
        )
    };
    Preflight {
        status,
        said,
        checks: reported,
        offered: None,
        offered_count: Some(count),
    }
}

// ── i due nodi ───────────────────────────────────────────────────────────

/// «Posso fidarmi di questo server, adesso, per questa cartella?»
///
/// Sta separato da `mcp_ask` perché un flusso possa **ramificare prima di
/// pagare**: la chiamata vera a un motore o a un indice costa, e scoprire a metà
/// che l'indice era vecchio vuol dire aver già speso. Non è una scorciatoia per
/// saltare la verifica: `mcp_ask` la rifà comunque, nella propria conversazione.
pub struct McpReadyAction;

impl Action for McpReadyAction {
    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match serde_json::from_value::<ReadySpec>(declared.clone()) {
            Ok(spec) => spec.extra.into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }

    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        // I rinvii sono già sciolti da `step_input`: è così che la cartella
        // decisa da un passo precedente arriva qui, e che `proves` può essere
        // quel percorso invece di una costante scritta a mano.
        let spec: ReadySpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        crate::check_tolerance(&spec.accept, READY_FAILURES)?;
        require_preflight(&spec.checks, &spec.checks_waived_because, &spec.project_root)?;
        let outcome = look(
            &spec.server,
            &spec.server_tool,
            &spec.checks,
            Duration::from_secs(spec.timeout_secs),
        );
        let output = json!({
            "status": outcome.status,
            "said": outcome.said,
            "server_tool": spec.server_tool,
            "project_root": spec.project_root,
            "checks": outcome.checks,
            "checks_waived_because": spec.checks_waived_because,
            "tools_offered": outcome.offered_count,
            "offered": outcome.offered,
            "caveat": CAVEAT,
        });
        finish(outcome.status, "ready", &spec.accept, &outcome.said, output)
    }

    /// Chiedere non cambia niente: la stretta di mano, l'elenco degli strumenti
    /// e le verifiche sono domande. È lo stesso contratto che `detect_tools`
    /// dichiara per il comando di versione di un descrittore — chi ci mette
    /// dentro un gesto ha già rotto il contratto, e lo aveva rotto anche senza
    /// nessuna interruzione di mezzo.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// Interroga un server MCP, dopo aver verificato che ci si possa fidare.
pub struct McpAskAction;

impl Action for McpAskAction {
    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match serde_json::from_value::<AskSpec>(declared.clone()) {
            Ok(spec) => spec.extra.into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }

    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: AskSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        crate::check_tolerance(&spec.accept, ASK_FAILURES)?;
        require_preflight(&spec.checks, &spec.checks_waived_because, &spec.project_root)?;

        let mut session = match Session::open(&spec.server, Duration::from_secs(spec.timeout_secs))
        {
            Ok(session) => session,
            Err(why) => {
                let output = json!({
                    "status": "unreachable",
                    "said": why,
                    "server_tool": spec.server_tool,
                    "project_root": spec.project_root,
                    "checks": [],
                    "caveat": CAVEAT,
                });
                return finish("unreachable", "ok", &spec.accept, &why, output);
            }
        };
        let checked = preflight(&mut session, &spec.server_tool, &spec.checks);
        if checked.status != "ready" {
            let stderr = session.close();
            let said = with_stderr(&checked.said, &stderr);
            let output = json!({
                "status": checked.status,
                "said": said,
                "server_tool": spec.server_tool,
                "project_root": spec.project_root,
                "checks": checked.checks,
                "checks_waived_because": spec.checks_waived_because,
                "tools_offered": checked.offered_count,
                "offered": checked.offered,
                "caveat": CAVEAT,
            });
            return finish(checked.status, "ok", &spec.accept, &said, output);
        }

        // La chiamata vera parte **solo adesso**, sulla stessa conversazione:
        // fra la verifica e la domanda non c'è nessun passo in cui il flusso
        // possa saltare la prima.
        if let Err(why) = session.say(&call_request(ERRAND_ID, &spec.server_tool, &spec.arguments)) {
            let output = json!({
                "status": "unreachable",
                "said": why,
                "server_tool": spec.server_tool,
                "project_root": spec.project_root,
                "checks": checked.checks,
                "caveat": CAVEAT,
            });
            return finish("unreachable", "ok", &spec.accept, &why, output);
        }
        let answers = session.listen_for(&[ERRAND_ID]);
        let answer = read_answer(answers.get(&ERRAND_ID));
        let stderr = session.close();
        let (status, said, text) = match &answer {
            Answer::Unanswered(why) => ("tool_failed", with_stderr(why, &stderr), String::new()),
            Answer::Said { text, refused: true } => (
                "tool_failed",
                format!("«{}» answered with an error: {text}", spec.server_tool),
                text.clone(),
            ),
            Answer::Said {
                text,
                refused: false,
            } => ("ok", format!("«{}» ha risposto", spec.server_tool), text.clone()),
        };
        let output = json!({
            "status": status,
            "said": said,
            "server_tool": spec.server_tool,
            "project_root": spec.project_root,
            "checks": checked.checks,
            "checks_waived_because": spec.checks_waived_because,
            "tools_offered": checked.offered_count,
            "text": text,
            "answer": answers.get(&ERRAND_ID).and_then(|value| value.get("result")).cloned(),
            "caveat": CAVEAT,
        });
        finish(status, "ok", &spec.accept, &said, output)
    }

    /// Uno strumento MCP può indicizzare, cancellare, riscrivere: il registro
    /// non sa quale sia stato chiesto, e nessun valore predefinito può escludere
    /// al posto di chi scrive il flusso che rifarlo duplichi un effetto già
    /// avvenuto.
    fn species(&self) -> StepSpecies {
        StepSpecies::HandToHuman
    }
}

/// Apre la conversazione solo per verificarla, e la chiude.
fn look(
    server: &ServerSpec,
    tool: &str,
    checks: &[PreflightCheck],
    limit: Duration,
) -> Preflight {
    let mut session = match Session::open(server, limit) {
        Ok(session) => session,
        Err(why) => {
            return Preflight {
                status: "unreachable",
                said: why,
                checks: Vec::new(),
                offered: None,
                offered_count: None,
            }
        }
    };
    let mut outcome = preflight(&mut session, tool, checks);
    let stderr = session.close();
    outcome.said = with_stderr(&outcome.said, &stderr);
    outcome
}

/// Aggiunge al messaggio ciò che il server ha scritto su stderr, quando c'è.
/// Un server che muore all'avvio spiega perché **solo** lì, e senza questa riga
/// il passo direbbe «non ha risposto» a chi ha già la risposta sotto il naso.
fn with_stderr(said: &str, stderr: &str) -> String {
    if stderr.trim().is_empty() {
        said.to_owned()
    } else {
        format!("{said} — the server wrote: {}", stderr.trim())
    }
}

/// Rosso salvo dichiarazione contraria: la tolleranza si scrive con `accept`,
/// non si regala.
fn finish(
    status: &str,
    good: &str,
    accept: &[String],
    said: &str,
    output: Value,
) -> Result<ActionOutcome, ActionError> {
    if status == good || crate::tolerates(accept, status) {
        Ok(ActionOutcome::Went(output))
    } else {
        Err(ActionError::new(status, said.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// Una cartella tutta nostra, che si porta via quello che ci abbiamo messo.
    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new(name: &str) -> Sandbox {
            let sequence = NEXT.fetch_add(1, Ordering::SeqCst);
            let root = std::env::temp_dir().join(format!(
                "sailor-mcp-{name}-{}-{sequence}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("la cartella di prova si crea");
            Sandbox { root }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// Un server MCP finto, che risponde quello che la prova gli fa dire.
    ///
    /// **PERCHÉ FINTO E NON QUELLO VERO.** Una prova che interroga SocratiCode
    /// passa su questa macchina e cade da chiunque altro, e soprattutto non
    /// potrebbe venire diversa: proverebbe la mia installazione, non il nodo.
    /// Qui ogni caso — il server che non c'è, quello che risponde e non offre,
    /// l'indice cieco, l'indice di un'altra cartella — si costruisce.
    ///
    /// Rispetta la forma vera: legge una riga alla volta, ignora le notifiche,
    /// e rimanda indietro l'`id` che ha ricevuto.
    ///
    /// **NESSUN APOSTROFO NEI TESTI APPARECCHIATI.** Il corpo dello script sta
    /// dentro virgolette singole di shell, e un apostrofo le chiude: il server
    /// finto muore all'avvio con un errore di sintassi e il nodo lo riporta
    /// onestamente come `unreachable`. Visto succedere il 31/08/2026 con la
    /// parola «l'archivio» dentro una risposta finta.
    fn fake_server(sandbox: &Sandbox, name: &str, cases: &[(&str, &str)]) -> ServerSpec {
        let mut body = String::from(
            "#!/bin/sh\nwhile IFS= read -r line; do\n  case \"$line\" in *'\"method\":\"notifications/'*) continue;; esac\n  id=$(printf '%s' \"$line\" | sed -n 's/.*\"id\":\\([0-9][0-9]*\\).*/\\1/p')\n  case \"$line\" in\n",
        );
        for (pattern, reply) in cases {
            body.push_str(&format!(
                "    *'{pattern}'*) printf '{reply}\\n' \"$id\" ;;\n"
            ));
        }
        body.push_str("  esac\ndone\n");
        let path = sandbox.root.join(format!("{name}.sh"));
        fs::write(&path, body).expect("il server finto si scrive");
        ServerSpec {
            command: "sh".to_owned(),
            args: vec![path.to_string_lossy().into_owned()],
            env: BTreeMap::new(),
            cwd: None,
        }
    }

    /// La stretta di mano e l'elenco degli strumenti, uguali in quasi ogni caso.
    fn handshake(tools: &str) -> Vec<(&'static str, String)> {
        vec![
            (
                "\"method\":\"initialize\"",
                "{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"serverInfo\":{\"name\":\"finto\",\"version\":\"0\"}}}".to_owned(),
            ),
            (
                "\"method\":\"tools/list\"",
                format!("{{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{{\"tools\":{tools}}}}}"),
            ),
        ]
    }

    fn text_reply(text: &str) -> String {
        format!("{{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{{\"content\":[{{\"type\":\"text\",\"text\":\"{text}\"}}]}}}}")
    }

    fn cases(pairs: Vec<(&'static str, String)>) -> Vec<(String, String)> {
        pairs
            .into_iter()
            .map(|(pattern, reply)| (pattern.to_owned(), reply))
            .collect()
    }

    fn server(sandbox: &Sandbox, name: &str, pairs: Vec<(&'static str, String)>) -> ServerSpec {
        let owned = cases(pairs);
        let borrowed: Vec<(&str, &str)> = owned
            .iter()
            .map(|(pattern, reply)| (pattern.as_str(), reply.as_str()))
            .collect();
        fake_server(sandbox, name, &borrowed)
    }

    const ROOT: &str = "/casa/progetto";

    fn root_check() -> PreflightCheck {
        PreflightCheck {
            name: "l'indice è di questa cartella".to_owned(),
            server_tool: "list_projects".to_owned(),
            arguments: json!({}),
            proves: ROOT.to_owned(),
            blind_if: vec!["archivio vettoriale non raggiungibile".to_owned()],
        }
    }

    fn ask_input(server: &ServerSpec, checks: Vec<PreflightCheck>, accept: Vec<&str>) -> Value {
        json!({
            "server": {"command": server.command, "args": server.args},
            "server_tool": "impact",
            "arguments": {"target": "graph.rs"},
            "project_root": ROOT,
            "checks": checks.iter().map(|check| json!({
                "name": check.name,
                "server_tool": check.server_tool,
                "arguments": check.arguments,
                "proves": check.proves,
                "blind_if": check.blind_if,
            })).collect::<Vec<_>>(),
            "timeout_secs": 20,
            "accept": accept,
        })
    }

    fn went(outcome: ActionOutcome) -> Value {
        match outcome {
            ActionOutcome::Went(value) => value,
            ActionOutcome::Waiting(said) => panic!("nessuna attesa: {said}"),
        }
    }

    /// **UN INDICE CHE NON SI È POTUTO LEGGERE NON È UN INDICE CHE DICE DI NO.**
    ///
    /// È il caso misurato il 31/08/2026 contro SocratiCode con l'archivio
    /// vettoriale spento: `codebase_list_projects` risponde con un risultato
    /// **riuscito** il cui testo è «Could not connect to Qdrant». Quel testo non
    /// contiene il percorso del progetto, quindi una verifica che guardi
    /// `proves` per prima lo chiama `check_failed` — cioè afferma che il
    /// progetto non è indicizzato, che è un'affermazione sul mondo che nessuno
    /// ha verificato.
    ///
    /// Il mutante che la fa cadere è togliere il ramo `blind_if` da `judge`:
    /// visto rosso il 31/08/2026 con `status: "check_failed"` al posto di
    /// `could_not_look`.
    #[test]
    fn an_index_that_could_not_be_read_is_not_an_index_that_says_no() {
        let sandbox = Sandbox::new("blind");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"}]");
        pairs.push((
            "\"name\":\"list_projects\"",
            text_reply("archivio vettoriale non raggiungibile"),
        ));
        let spec = server(&sandbox, "blind", pairs);

        let outcome = McpAskAction
            .execute(
                &ask_input(&spec, vec![root_check()], vec!["could_not_look"]),
                &SharedState::new(),
            )
            .expect("l'esito è tollerato, quindi il passo va avanti col dato");
        let value = went(outcome);
        assert_eq!(
            value["status"], "could_not_look",
            "cieco non è negativo: {}",
            value["said"]
        );
        assert_eq!(value["checks"][0]["state"], "could_not_look");
    }

    /// **UN SERVER CHE RISPONDE NON È UN SERVER CHE OFFRE.**
    ///
    /// Misurato il 31/08/2026: `claude mcp list` dichiarava `socraticode: ✔
    /// Connected` mentre la sessione che lo interrogava non aveva quello
    /// strumento. Qui il server finto fa la stretta di mano e offre altro.
    ///
    /// Il mutante che la fa cadere è togliere il controllo su `tools/list` e
    /// fidarsi della stretta di mano: l'esito diventerebbe `ready`, e la
    /// chiamata partirebbe su uno strumento che non c'è.
    #[test]
    fn answering_the_handshake_is_not_offering_the_tool() {
        let sandbox = Sandbox::new("not-offered");
        let pairs = handshake("[{\"name\":\"search\"}]");
        let spec = server(&sandbox, "not-offered", pairs);

        let error = McpAskAction
            .execute(
                &ask_input(&spec, vec![root_check()], vec![]),
                &SharedState::new(),
            )
            .expect_err("uno strumento che il server non offre rompe il passo");
        assert_eq!(error.class, "tool_not_offered");
        assert!(
            error.said.contains("two different facts"),
            "il messaggio deve dire perché rispondere e offrire non sono la stessa cosa: {}",
            error.said
        );
    }

    /// Un server che non si avvia è `unreachable`, e lo dice col motivo del
    /// sistema operativo invece che con «non ha risposto».
    #[test]
    fn a_server_that_never_starts_is_unreachable() {
        let sandbox = Sandbox::new("absent");
        let spec = ServerSpec {
            command: sandbox
                .root
                .join("questo-comando-non-esiste")
                .to_string_lossy()
                .into_owned(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
        };
        let error = McpAskAction
            .execute(
                &ask_input(&spec, vec![root_check()], vec![]),
                &SharedState::new(),
            )
            .expect_err("un server che non parte rompe il passo");
        assert_eq!(error.class, "unreachable");
    }

    /// **L'INDICE DI UN ALTRO PROGETTO È UNA MISURA, E DICE DI NO.**
    ///
    /// Il server risponde, offre lo strumento, e l'elenco dei progetti non
    /// contiene questa cartella: qui si è potuto guardare, quindi la parola è
    /// `check_failed` e non `could_not_look`. È l'altra metà della prima prova:
    /// senza questa, un `judge` che dicesse sempre «cieco» passerebbe.
    #[test]
    fn an_index_of_another_project_is_a_measure_and_says_no() {
        let sandbox = Sandbox::new("other-project");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"}]");
        pairs.push((
            "\"name\":\"list_projects\"",
            text_reply("/casa/un-altro-progetto"),
        ));
        let spec = server(&sandbox, "other-project", pairs);

        let outcome = McpAskAction
            .execute(
                &ask_input(&spec, vec![root_check()], vec!["check_failed"]),
                &SharedState::new(),
            )
            .expect("tollerato");
        let value = went(outcome);
        assert_eq!(value["status"], "check_failed", "{}", value["said"]);
        assert_eq!(value["checks"][0]["state"], "failed");
        assert_eq!(
            value.get("text"),
            None,
            "una verifica fallita non lascia passare nessuna risposta dello strumento"
        );
    }

    /// **LA CECITÀ BATTE IL NO.** Due verifiche, una cieca e una negativa:
    /// l'esito è `could_not_look`. Dire `check_failed` vorrebbe dire «ho
    /// guardato tutto e una cosa era sbagliata», e quella frase non si può
    /// pronunciare quando una delle guardate non è avvenuta.
    ///
    /// Il mutante che la fa cadere è invertire i due rami in fondo a
    /// `preflight`.
    #[test]
    fn blindness_outranks_a_no() {
        let sandbox = Sandbox::new("blind-and-no");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"},{\"name\":\"status\"}]");
        pairs.push((
            "\"name\":\"list_projects\"",
            text_reply("archivio vettoriale non raggiungibile"),
        ));
        pairs.push(("\"name\":\"status\"", text_reply("indicizzato al 12 percento")));
        let spec = server(&sandbox, "blind-and-no", pairs);

        let stale = PreflightCheck {
            name: "l'indice è aggiornato".to_owned(),
            server_tool: "status".to_owned(),
            arguments: json!({}),
            proves: "100 percento".to_owned(),
            blind_if: Vec::new(),
        };
        let outcome = McpAskAction
            .execute(
                &ask_input(
                    &spec,
                    vec![root_check(), stale],
                    vec!["could_not_look", "check_failed"],
                ),
                &SharedState::new(),
            )
            .expect("tollerato");
        let value = went(outcome);
        assert_eq!(value["status"], "could_not_look", "{}", value["said"]);
        assert_eq!(value["checks"][0]["state"], "could_not_look");
        assert_eq!(value["checks"][1]["state"], "failed");
    }

    /// Il giro intero: verifiche passate, chiamata fatta, risposta consegnata —
    /// **con la regola attaccata**.
    ///
    /// Il mutante che fa cadere l'ultima asserzione è togliere `caveat`
    /// dall'uscita: chi userà questo nodo fra sei mesi non avrà letto il
    /// documento che quella regola l'ha misurata.
    #[test]
    fn a_passed_preflight_lets_the_question_through_with_the_rule_attached() {
        let sandbox = Sandbox::new("ready");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"}]");
        pairs.push(("\"name\":\"list_projects\"", text_reply(ROOT)));
        pairs.push(("\"name\":\"impact\"", text_reply("22 file usano questo crate")));
        let spec = server(&sandbox, "ready", pairs);

        let outcome = McpAskAction
            .execute(
                &ask_input(&spec, vec![root_check()], vec![]),
                &SharedState::new(),
            )
            .expect("un giro riuscito non rompe niente");
        let value = went(outcome);
        assert_eq!(value["status"], "ok", "{}", value["said"]);
        assert_eq!(value["checks"][0]["state"], "passed");
        assert_eq!(value["text"], "22 file usano questo crate");
        assert!(
            value["caveat"]
                .as_str()
                .expect("la regola viaggia con la risposta")
                .contains("not for deciding"),
            "la regola sul perimetro deve uscire insieme alla risposta"
        );
    }

    /// **UN PASSO NON PUÒ INTERROGARE UN INDICE SENZA DIRE DI CHI È.**
    ///
    /// Nessuna verifica nomina `project_root`: il passo non parte, e l'errore è
    /// di chi ha scritto il flusso — non un esito del mondo, perché tollerarlo
    /// vorrebbe dire «interroga pure un indice che non sai di chi sia».
    ///
    /// Il mutante che la fa cadere è togliere la chiamata a `require_preflight`.
    #[test]
    fn a_step_cannot_question_an_index_without_saying_whose_it_is() {
        let sandbox = Sandbox::new("no-preflight");
        let mut pairs = handshake("[{\"name\":\"impact\"}]");
        pairs.push(("\"name\":\"impact\"", text_reply("che ore sono")));
        let spec = server(&sandbox, "no-preflight", pairs);

        let error = McpAskAction
            .execute(&ask_input(&spec, vec![], vec![]), &SharedState::new())
            .expect_err("senza verifica preliminare il passo non parte");
        assert_eq!(error.class, "no_preflight");

        // E con una rinuncia scritta, invece, parte: la tolleranza è una
        // decisione dichiarata, non un valore predefinito.
        let mut waived = ask_input(&spec, vec![], vec!["check_failed", "could_not_look"]);
        waived["checks_waived_because"] =
            json!("questo server non sa niente di cartelle: risponde sull'ora del sistema");
        McpAskAction
            .execute(&waived, &SharedState::new())
            .expect("con la rinuncia scritta il passo parte");
    }

    /// **UN RINVIO ARRIVA AL SERVER RISOLTO.**
    ///
    /// La cartella e gli argomenti vengono dal passo prima. A scioglierli è
    /// `flow::step_input` — un posto solo per tutte le azioni, dal 01/09/2026 —
    /// e qui la prova lo rifà con la stessa funzione perché chiama `execute`
    /// senza passare dall'esecutore. Ciò che questa prova afferma è che l'azione
    /// **usa** ciò che riceve: il `project_root` risolto arriva davvero al
    /// server. Che ad arrivare sciolto sia l'ingresso di *ogni* azione lo prova
    /// `crates/flow/tests/a_reference_reaches_every_action.rs`, e questa non lo
    /// ripete.
    #[test]
    fn a_reference_reaches_the_server_resolved() {
        let sandbox = Sandbox::new("reference");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"}]");
        pairs.push(("\"name\":\"list_projects\"", text_reply(ROOT)));
        pairs.push(("\"name\":\"impact\"", text_reply("visto")));
        let spec = server(&sandbox, "reference", pairs);

        let input = json!({
            "repo": ROOT,
            "server": {"command": spec.command, "args": spec.args},
            "server_tool": "impact",
            "arguments": {"target": {"$from": "/repo"}},
            "project_root": {"$from": "/repo"},
            "checks": [{
                "name": "l'indice è di questa cartella",
                "server_tool": "list_projects",
                "proves": {"$from": "/repo"},
            }],
            "timeout_secs": 20,
        });
        let value = went(
            McpAskAction
                .execute(
                    &crate::tests::with_references_resolved(input),
                    &SharedState::new(),
                )
                .expect("una cartella presa con un rinvio arriva risolta"),
        );
        assert_eq!(value["status"], "ok", "{}", value["said"]);
        assert_eq!(value["project_root"], ROOT);
    }

    /// `mcp_ready` risponde senza chiamare lo strumento vero, perché un flusso
    /// possa ramificare **prima di pagare**.
    #[test]
    fn the_readiness_node_answers_without_paying_for_the_real_call() {
        let sandbox = Sandbox::new("ready-node");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"}]");
        pairs.push(("\"name\":\"list_projects\"", text_reply(ROOT)));
        // «impact» non ha nessuna risposta apparecchiata: se il nodo lo
        // chiamasse, resterebbe appeso fino alla scadenza.
        let spec = server(&sandbox, "ready-node", pairs);

        let input = json!({
            "server": {"command": spec.command, "args": spec.args},
            "server_tool": "impact",
            "project_root": ROOT,
            "checks": [{
                "name": "l'indice è di questa cartella",
                "server_tool": "list_projects",
                "proves": ROOT,
                "blind_if": ["archivio vettoriale non raggiungibile"],
            }],
            "timeout_secs": 20,
        });
        let value = went(
            McpReadyAction
                .execute(&input, &SharedState::new())
                .expect("la verifica passa"),
        );
        assert_eq!(value["status"], "ready", "{}", value["said"]);
        assert_eq!(value.get("text"), None, "la verifica non chiama lo strumento");
    }

    /// **UNA PROVA VUOTA PASSEREBBE SEMPRE, E NESSUNO LA SCRIVE APPOSTA.**
    ///
    /// Ogni testo contiene la stringa vuota: con un `proves` vuoto la verifica
    /// resta verde anche quando il server risponde che non è riuscito a
    /// guardare. Non è un caso di scuola — il modo normale di arrivarci è un
    /// `{"$from": …}` che punta a un campo vuoto, e allora la verifica smette di
    /// verificare **in silenzio**.
    ///
    /// Il mutante che la fa cadere è togliere il controllo su `proves` da
    /// `require_preflight`: l'esito diventa `ok`, cioè un passo che ha
    /// interrogato un indice cieco credendo di averlo verificato.
    #[test]
    fn an_empty_proof_proves_nothing_and_is_refused() {
        let sandbox = Sandbox::new("empty-proof");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"}]");
        pairs.push((
            "\"name\":\"list_projects\"",
            text_reply("archivio vettoriale non raggiungibile"),
        ));
        pairs.push(("\"name\":\"impact\"", text_reply("visto")));
        let spec = server(&sandbox, "empty-proof", pairs);

        let mut blank = root_check();
        blank.proves = String::new();
        let error = McpAskAction
            .execute(
                &ask_input(&spec, vec![blank], vec!["could_not_look", "check_failed"]),
                &SharedState::new(),
            )
            .expect_err("una verifica che non dice cosa prova non parte");
        assert_eq!(error.class, "invalid_input");

        // E la stessa trappola un gradino più su: senza cartella dichiarata, il
        // legame che `require_preflight` impone lo soddisferebbe chiunque.
        let mut rootless = ask_input(&spec, vec![root_check()], vec![]);
        rootless["project_root"] = json!("");
        let error = McpAskAction
            .execute(&rootless, &SharedState::new())
            .expect_err("un passo che non dice di quale cartella parla non parte");
        assert_eq!(error.class, "no_preflight");
    }

    /// Un `accept` che nomina un esito impossibile è un refuso di chi ha
    /// scritto il flusso, e si vede subito invece che alla prima corsa.
    #[test]
    fn an_accept_that_names_an_impossible_outcome_is_refused() {
        let sandbox = Sandbox::new("bad-accept");
        let pairs = handshake("[{\"name\":\"impact\"}]");
        let spec = server(&sandbox, "bad-accept", pairs);
        let error = McpAskAction
            .execute(
                &ask_input(&spec, vec![root_check()], vec!["ok"]),
                &SharedState::new(),
            )
            .expect_err("«ok» non è un fallimento e non si tollera");
        assert_eq!(error.class, "invalid_input");
        assert!(error.said.contains("accept"), "{}", error.said);
    }

    /// Asks the operating system, with a call the cure does not use: signal
    /// zero delivers nothing and only reports whether the pid is there.
    fn still_alive(pid: i32) -> bool {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    /// Closing a server takes what the server started, and returns at once.
    ///
    /// The worker inherits stderr, so while it lives the reader never sees the
    /// end of the pipe and `close` waits for it: signalling the server alone
    /// does not just leak a process, it blocks the caller for as long as the
    /// worker runs. Measured at three hundred seconds instead of a fraction.
    #[test]
    fn closing_a_server_takes_what_it_started() {
        let sandbox = Sandbox::new("orphans");
        let told = sandbox.root.join("worker.pid");
        let script = sandbox.root.join("with-a-worker.sh");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep 300 &\necho $! > {}\nwhile IFS= read -r line; do :; done\n",
                told.display()
            ),
        )
        .expect("the fake server is written");
        let spec = ServerSpec {
            command: "sh".to_owned(),
            args: vec![script.to_string_lossy().into_owned()],
            env: BTreeMap::new(),
            cwd: None,
        };

        let session = Session::open(&spec, Duration::from_secs(10)).expect("the server starts");
        let mut worker = 0i32;
        for _ in 0..100 {
            if let Ok(text) = fs::read_to_string(&told) {
                if let Ok(found) = text.trim().parse::<i32>() {
                    worker = found;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(worker > 0, "the fake server never reported its worker");
        assert!(
            still_alive(worker),
            "the worker {worker} was gone before the server closed, so this \
             test would pass without proving anything"
        );

        let began = Instant::now();
        session.close();
        let took = began.elapsed();

        assert!(
            took < Duration::from_secs(10),
            "closing took {took:?}: the worker kept the pipe open and the \
             reader waited for it, so closing a server costs as long as \
             whatever it started"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && still_alive(worker) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !still_alive(worker),
            "the server is closed and its worker {worker} is still running: \
             the signal reached the server alone, not the group it leads"
        );
    }
}
