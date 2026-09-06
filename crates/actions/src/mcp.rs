//! The node with which a step questions an MCP server, and the check that
//! comes first.
//!
//! **WHY IT EXISTS.** The `da-fare` note called it «the missing link»: Sailor
//! *recognises* MCP servers — the detector has the `mcp_server` family — but
//! none of the nine registered actions could talk to one. A flow that wanted
//! to ask SocratiCode «what would this change touch» had to leave the graph
//! and become a script, which is exactly what this house does not do.
//!
//! **WHAT IS IN THE CODE AND WHAT IS NOT.** Only what touches the world:
//! opening a process, saying the words of the handshake, reading the lines it
//! answers. Which server, which tool, which arguments and which preliminary
//! checks are **step data** — not one constant names SocratiCode, and this
//! file's tests run against a fake server built in a temporary directory.
//!
//! **THE HANDSHAKE IS NOT AN OFFER, AND THAT IS THE MAIN DISTINCTION.**
//! Measured: `claude mcp list` declared `socraticode: ✔ Connected` while the
//! session questioning it did not have that tool at all. A node that trusts
//! that signal works on an index that is not there without noticing. Here
//! Sailor opens the server itself and asks it `tools/list`: «it answers» and
//! «it offers the tool I need» stay two separate facts, with two separate
//! words — `unreachable` and `tool_not_offered` — and the message of the
//! second says why they are not the same thing.
//!
//! **STANDARD INPUT STAYS OPEN UNTIL THE ANSWER ARRIVES.** Not a matter of
//! style: measured against `npx -y socraticode`, writing every request and
//! closing standard input straight away — what `run_with_timeout_and_stdin`,
//! the primitive already there, does — brings back **only** the answer to
//! `initialize`, and the `tools/call` requests stay without answer and
//! without error. The server dies on EOF before it has finished. That is why
//! this file has a dialogue of its own instead of reusing that primitive:
//! there standard input closes by contract.
//!
//! **FOUR OUTCOMES, NOT ONE.** «The server is not there», «the server is
//! there but does not offer this tool», «a preliminary check says no», «a
//! preliminary check could not look» are four different facts, and the fifth
//! is «the tool answered that it does not know». Confusing them is the fault
//! this house calls *«it is not there» is not always a measurement*: where
//! looking was not possible the answer is «I could not look», with the reason.
//!
//! **AND `could_not_look` OUTRANKS `check_failed`.** If one check is blind
//! and another is negative, the overall outcome is the blindness. Saying
//! `check_failed` would assert «I looked at everything and one thing was
//! wrong», and that sentence cannot be spoken when one of the looks never
//! happened: an unknown can hide anything, including worse.
//!
//! **THE CHECK CANNOT BE SKIPPED, AND THE CODE ENFORCES IT.** A step declares
//! `project_root` — the directory it claims to speak about — and at least one
//! check must tie the server's answer to that directory, that is, have
//! `project_root` inside its own `proves`. One that does not have it does not
//! start: `no_preflight`. An indexed project is not every project, and an
//! index that is right for another directory answers confidently about code
//! that does not exist here. Whoever questions a server that knows nothing of
//! directories declares it in writing with `checks_waived_because`, and that
//! sentence stays in the output.

use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// The name under which the preliminary check alone registers.
pub const MCP_READY_ACTION: &str = "mcp_ready";
/// The name under which the real questioning registers.
pub const MCP_ASK_ACTION: &str = "mcp_ask";

/// Registers the two nodes that talk to an MCP server.
pub fn register_mcp(registry: &mut flow::ActionRegistry) {
    registry.register(MCP_READY_ACTION, McpReadyAction);
    registry.register(MCP_ASK_ACTION, McpAskAction);
}

/// The rule that travels with every answer, instead of depending on whoever
/// writes the prompt.
///
/// **WHY IT IS IN THE OUTPUT AND NOT IN A DOCUMENT.** The note
/// `2026-08-28-sailor-si-sviluppa-su-se-stesso`, section 6, says in so many
/// words that this rule belongs in the prompt of the step that uses
/// SocratiCode. But a document is read by whoever was there: whoever uses this
/// node in six months writes their own prompt without ever opening it. Coming
/// out of here, the rule enters the next step's prompt through a reference —
/// `{"$from": "/caveat"}` — and depends on nobody's memory.
pub const CAVEAT: &str = "This answer comes from an external index: it is for finding your bearings, not for deciding. A blast radius — who depends on what, what breaks if you touch this — is decided by the tool that compiles, never by the index's dependency graph. Measured: asked about the impact of «crates/flow/src/graph.rs», the index answered «no callers, nothing depends on this», while that file has 493 lines, eight «Cargo.toml» declare the crate and 22 files use it. It was a false orphan, on the file at the centre of the flow format.";

/// The outcomes a step may declare it tolerates with `accept`.
///
/// `ready` and `ok` are absent: they are not failures, and naming them in
/// `accept` would be a typo to show at once rather than a tolerance.
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

// ── what the step declares ───────────────────────────────────────────────

/// How the server is started. It is a command, because an MCP server that
/// talks over standard input and standard output is a child process: whoever
/// declares it writes the same three things that live in a `.mcp.json`.
#[derive(Debug, Clone, Deserialize)]
struct ServerSpec {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// Which directory to start it from. A server that indexes code often
    /// looks at the current directory, and leaving it as that of whoever runs
    /// the flow means not knowing which project it is talking about.
    #[serde(default)]
    cwd: Option<String>,
}

/// A preliminary check: a question to the server, and what it must answer.
///
/// **`blind_if` IS LOOKED AT BEFORE DECLARING A NO, AND IT IS NO DETAIL.**
/// Measured against SocratiCode: with the vector store down,
/// `codebase_list_projects` answers `{"content":[{"type":"text","text":"Could
/// not connect to Qdrant."}]}` — a **successful** result, with no `isError`.
/// That text does not contain the project's path, so without `blind_if` an
/// index that could not be read would become «the project is not indexed»: a
/// claim about the world that nobody verified.
///
/// The order of the three cases is: the positive proof, then the blindness,
/// then the no. The first comes first because an answer containing what was
/// sought really did show it, whatever else it says; the no comes last because
/// it is the only one asserting something about the world, and it is spoken
/// once the other two are ruled out.
#[derive(Debug, Clone, Deserialize)]
struct PreflightCheck {
    /// What the fact under check is called, for whoever reads the output.
    name: String,
    server_tool: String,
    #[serde(default = "empty_object")]
    arguments: Value,
    /// The text the answer must contain for the fact to be proved.
    proves: String,
    /// The texts that, if they appear, say the server **could not look**.
    /// Empty is allowed, and then only the server's declared error separates
    /// blindness from a no.
    #[serde(default)]
    blind_if: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ReadySpec {
    server: ServerSpec,
    /// The tool the step will actually use. It is asked here because «the
    /// server answers» is not «the server offers this».
    ///
    /// **IT IS NOT CALLED `tool`, AND THAT IS NOT A MATTER OF TASTE.** `sailor
    /// flow check` reads the `tool` field of **every** step, whatever the
    /// action, and treats it as the identifier of a tool to resolve on the
    /// machine — `flow_cmd::tools_wanted` says so in writing: «it is the name
    /// of the field that says this is a tool identifier». An MCP tool is not a
    /// Sailor tool: calling it `tool` would get every flow using these nodes
    /// **rejected** by the static check with «tools no descriptor declares»,
    /// and the repair would be in the other crate.
    server_tool: String,
    /// The directory the step claims to speak about.
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
    /// See `ReadySpec::server_tool` for why it is not called `tool`.
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

/// No check ties the answer to the declared directory: the step does not
/// start.
///
/// **WHY IT IS AN ERROR OF WHOEVER WRITES THE FLOW AND NOT AN OUTCOME.** An
/// outcome can be tolerated with `accept`, and tolerance here would mean «go
/// ahead and question an index you do not know the owner of». An indexed
/// project is not every project: without this tie the answer is plausible and
/// is about another directory, and nothing flags it.
fn require_preflight(
    checks: &[PreflightCheck],
    waived_because: &str,
    project_root: &str,
) -> Result<(), ActionError> {
    // **AN EMPTY PROOF PROVES NOTHING, AND WOULD ALWAYS PASS.** Every text
    // contains the empty string: an empty `proves` would leave the check green
    // whatever the server answers, «I cannot reach the store» included. It is
    // the fault this house calls «a check that checks nothing», and here it
    // would turn up on its own, with nobody writing it on purpose: one
    // `{"$from": …}` pointing at an empty field is enough.
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
    // Same reason, one rung up: with an empty `project_root` the tie between
    // the answer and the directory would be met by any check at all.
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

// ── the dialogue with the server ─────────────────────────────────────────

const INITIALIZE_ID: u64 = 1;
const LIST_ID: u64 = 2;
/// From here up, the check identifiers, one per number.
const FIRST_CHECK_ID: u64 = 10;
/// The identifier of the real call, kept far from the others so it stands out
/// at a glance in a ledger.
const ERRAND_ID: u64 = 100;

/// How much of the server's stderr is kept for the error message. A server
/// that spews a whole log must not fill the ledger.
const KEPT_STDERR: usize = 2000;

/// A conversation open with an MCP server.
///
/// It lives as long as the step: it opens, the questions are asked in two
/// rounds — first the checks, then the real call if the checks pass — and it
/// closes. The child dies in `Drop` even when leaving by an error path.
struct Session {
    child: std::process::Child,
    /// Kept alive on purpose: closing it makes the server exit before answering.
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
            // The operating system's reason, as is already the rule for
            // `SpawnFailed`: «it did not start» on its own sends you hunting a
            // missing binary when the file was there and not executable.
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

    /// Waits for the answers to the identifiers asked for, until the deadline.
    ///
    /// It returns what arrived: the caller tells «absent» from «negative», and
    /// the two cannot be confused in here.
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
                        // A server may write lines that are not JSON-RPC.
                        // They are not a fault: they are not an answer.
                        continue;
                    };
                    if let Some(id) = value.get("id").and_then(Value::as_u64) {
                        if wanted.contains(&id) {
                            answers.insert(id, value);
                        }
                    }
                }
                // Timed out, or the server closed its own output: either way
                // nothing more will arrive.
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }
        answers
    }

    /// Closes and returns what the server wrote on stderr, the one place where
    /// a broken server explains why.
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

// ── reading an answer ────────────────────────────────────────────────────

/// What came back from a question.
///
/// `Unanswered` is not `Said` with an error inside: the first is «I have no
/// answer», the second is «the server answered something». Keeping them in two
/// variants is what stops a question fallen into the void from becoming a
/// negative answer.
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

/// The tools the server declares it offers.
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

/// The verdict on a preliminary check: passed, negative, or blind.
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
                    format!(
                        "«{}» risponde e contiene «{}»",
                        check.server_tool, check.proves
                    ),
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

/// The outcome of the preliminary check, before the real tool is called.
struct Preflight {
    status: &'static str,
    said: String,
    checks: Vec<Value>,
    /// The tools offered, listed when they help whoever repairs: that is, when
    /// the one asked for is absent. A list of twenty-five names in every
    /// successful output is noise nobody reads.
    offered: Option<Vec<String>>,
    offered_count: Option<usize>,
}

/// Opens the dialogue, does the handshake, asks for the tool list and runs the
/// declared checks. **It does not call the real tool**: that call belongs to
/// the caller, and starts only if `ready` comes out of here.
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

    if !answers.contains_key(&INITIALIZE_ID) {
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
            // The server is there and answers, but could not say what it
            // offers: neither that it has the tool nor that it lacks it.
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
    // Blindness comes before the no: see the comment at the head of the file.
    let (status, said) = if blind > 0 {
        (
            "could_not_look",
            format!("{blind} preliminary checks out of {} could not look: where looking was not possible the answer is «I do not know», not «no»", checks.len()),
        )
    } else if refused > 0 {
        (
            "check_failed",
            format!(
                "{refused} preliminary checks out of {} say no",
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

// ── the two nodes ────────────────────────────────────────────────────────

/// «Can I trust this server, right now, for this directory?»
///
/// It is kept apart from `mcp_ask` so a flow can **branch before paying**: the
/// real call to an engine or an index costs, and finding out halfway that the
/// index was stale means having spent. It is no shortcut past the check:
/// `mcp_ask` does it again anyway, in its own conversation.
pub struct McpReadyAction;

impl Action for McpReadyAction {
    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match serde_json::from_value::<ReadySpec>(declared.clone()) {
            Ok(spec) => spec.extra.into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }

    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        // References are already resolved by `step_input`: that is how the
        // directory decided by an earlier step gets here, and how `proves` can
        // be that path instead of a constant written by hand.
        let spec: ReadySpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        crate::check_tolerance(&spec.accept, READY_FAILURES)?;
        require_preflight(
            &spec.checks,
            &spec.checks_waived_because,
            &spec.project_root,
        )?;
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

    /// Asking changes nothing: the handshake, the tool list and the checks are
    /// questions. It is the same contract `detect_tools` declares for a
    /// descriptor's version command — whoever puts a gesture in there has
    /// already broken the contract, and had broken it with no interruption in
    /// the middle either.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// Questions an MCP server, after checking that it can be trusted.
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
        require_preflight(
            &spec.checks,
            &spec.checks_waived_because,
            &spec.project_root,
        )?;

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

        // The real call starts **only now**, on the same conversation: between
        // the check and the question there is no step at which the flow could
        // skip the first.
        if let Err(why) = session.say(&call_request(ERRAND_ID, &spec.server_tool, &spec.arguments))
        {
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
            Answer::Said {
                text,
                refused: true,
            } => (
                "tool_failed",
                format!("«{}» answered with an error: {text}", spec.server_tool),
                text.clone(),
            ),
            Answer::Said {
                text,
                refused: false,
            } => (
                "ok",
                format!("«{}» ha risposto", spec.server_tool),
                text.clone(),
            ),
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

    /// An MCP tool can index, delete, rewrite: the registry does not know which
    /// was asked for, and no default can rule out, in place of whoever writes
    /// the flow, that doing it again duplicates an effect already had.
    fn species(&self) -> StepSpecies {
        StepSpecies::HandToHuman
    }
}

/// Opens the conversation only to check it, and closes it.
fn look(server: &ServerSpec, tool: &str, checks: &[PreflightCheck], limit: Duration) -> Preflight {
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

/// Adds to the message what the server wrote on stderr, when there is any. A
/// server that dies at startup explains why **only** there, and without this
/// line the step would say «it did not answer» to who has it under their nose.
fn with_stderr(said: &str, stderr: &str) -> String {
    if stderr.trim().is_empty() {
        said.to_owned()
    } else {
        format!("{said} — the server wrote: {}", stderr.trim())
    }
}

/// Red unless declared otherwise: tolerance is written with `accept`, it is
/// not given away.
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

    /// A directory all our own, that takes away whatever we put in it.
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
            fs::create_dir_all(&root).expect("the scratch directory is created");
            Sandbox { root }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// A fake MCP server, answering whatever the test makes it say.
    ///
    /// **WHY FAKE AND NOT THE REAL ONE.** A test questioning SocratiCode passes
    /// on this machine and falls over on anybody else's, and above all it could
    /// not come out otherwise: it would prove my installation, not the node.
    /// Here every case — the server that is not there, the one that answers and
    /// does not offer, the blind index, the index of another directory — is
    /// built.
    ///
    /// It respects the real shape: it reads one line at a time, ignores
    /// notifications, and sends back the `id` it received.
    ///
    /// **NO APOSTROPHES IN THE PREPARED TEXTS.** The script's body sits inside
    /// shell single quotes, and an apostrophe closes them: the fake server dies
    /// at startup with a syntax error and the node reports it honestly as
    /// `unreachable`. Seen with the word «l'archivio» inside a fake answer.
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
        fs::write(&path, body).expect("the fake server is written");
        ServerSpec {
            command: "sh".to_owned(),
            args: vec![path.to_string_lossy().into_owned()],
            env: BTreeMap::new(),
            cwd: None,
        }
    }

    /// The handshake and the tool list, the same in nearly every case.
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

    const ROOT: &str = "/home/project";

    fn root_check() -> PreflightCheck {
        PreflightCheck {
            name: "the index is of this directory".to_owned(),
            server_tool: "list_projects".to_owned(),
            arguments: json!({}),
            proves: ROOT.to_owned(),
            blind_if: vec!["vector store not reachable".to_owned()],
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
            ActionOutcome::Waiting(said) => panic!("nothing waits here: {said}"),
            ActionOutcome::NotYet(said) => panic!("nothing is postponed here: {said}"),
        }
    }

    /// **AN INDEX THAT COULD NOT BE READ IS NOT AN INDEX THAT SAYS NO.**
    ///
    /// It is the case measured against SocratiCode with the vector store down:
    /// `codebase_list_projects` answers with a **successful** result whose text
    /// is «Could not connect to Qdrant». That text does not contain the
    /// project's path, so a check looking at `proves` first calls it
    /// `check_failed` — that is, asserts the project is not indexed, which is a
    /// claim about the world that nobody verified.
    ///
    /// The mutant that makes it fall is removing the `blind_if` branch from
    /// `judge`: seen red with `status: "check_failed"` in place of
    /// `could_not_look`.
    #[test]
    fn an_index_that_could_not_be_read_is_not_an_index_that_says_no() {
        let sandbox = Sandbox::new("blind");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"}]");
        pairs.push((
            "\"name\":\"list_projects\"",
            text_reply("vector store not reachable"),
        ));
        let spec = server(&sandbox, "blind", pairs);

        let outcome = McpAskAction
            .execute(
                &ask_input(&spec, vec![root_check()], vec!["could_not_look"]),
                &SharedState::new(),
            )
            .expect("the outcome is tolerated, so the step carries on with the datum");
        let value = went(outcome);
        assert_eq!(
            value["status"], "could_not_look",
            "blind is not negative: {}",
            value["said"]
        );
        assert_eq!(value["checks"][0]["state"], "could_not_look");
    }

    /// **A SERVER THAT ANSWERS IS NOT A SERVER THAT OFFERS.**
    ///
    /// Measured: `claude mcp list` declared `socraticode: ✔ Connected` while
    /// the session questioning it did not have that tool. Here the fake server
    /// does the handshake and offers something else.
    ///
    /// The mutant that makes it fall is removing the check on `tools/list` and
    /// trusting the handshake: the outcome would become `ready`, and the call
    /// would start on a tool that is not there.
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
            .expect_err("a tool the server does not offer breaks the step");
        assert_eq!(error.class, "tool_not_offered");
        assert!(
            error.said.contains("two different facts"),
            "the message has to say why answering and offering are not the same thing: {}",
            error.said
        );
    }

    /// A server that never starts is `unreachable`, and says so with the
    /// operating system's reason instead of «it did not answer».
    #[test]
    fn a_server_that_never_starts_is_unreachable() {
        let sandbox = Sandbox::new("absent");
        let spec = ServerSpec {
            command: sandbox
                .root
                .join("this-command-does-not-exist")
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
            .expect_err("a server that will not start breaks the step");
        assert_eq!(error.class, "unreachable");
    }

    /// **THE INDEX OF ANOTHER PROJECT IS A MEASUREMENT, AND IT SAYS NO.**
    ///
    /// The server answers, offers the tool, and the project list does not
    /// contain this directory: here looking was possible, so the word is
    /// `check_failed` and not `could_not_look`. It is the other half of the
    /// first test: without it, a `judge` always saying «blind» would pass.
    #[test]
    fn an_index_of_another_project_is_a_measure_and_says_no() {
        let sandbox = Sandbox::new("other-project");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"}]");
        pairs.push((
            "\"name\":\"list_projects\"",
            text_reply("/home/another-project"),
        ));
        let spec = server(&sandbox, "other-project", pairs);

        let outcome = McpAskAction
            .execute(
                &ask_input(&spec, vec![root_check()], vec!["check_failed"]),
                &SharedState::new(),
            )
            .expect("tolerated");
        let value = went(outcome);
        assert_eq!(value["status"], "check_failed", "{}", value["said"]);
        assert_eq!(value["checks"][0]["state"], "failed");
        assert_eq!(
            value.get("text"),
            None,
            "a failed check lets no answer of the tool through"
        );
    }

    /// **BLINDNESS OUTRANKS A NO.** Two checks, one blind and one negative:
    /// the outcome is `could_not_look`. Saying `check_failed` would mean «I
    /// looked at everything and one thing was wrong», and that sentence cannot
    /// be spoken when one of the looks never happened.
    ///
    /// The mutant that makes it fall is swapping the two branches at the foot
    /// of `preflight`.
    #[test]
    fn blindness_outranks_a_no() {
        let sandbox = Sandbox::new("blind-and-no");
        let mut pairs =
            handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"},{\"name\":\"status\"}]");
        pairs.push((
            "\"name\":\"list_projects\"",
            text_reply("vector store not reachable"),
        ));
        pairs.push((
            "\"name\":\"status\"",
            text_reply("indexed to 12 percent"),
        ));
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
            .expect("tolerated");
        let value = went(outcome);
        assert_eq!(value["status"], "could_not_look", "{}", value["said"]);
        assert_eq!(value["checks"][0]["state"], "could_not_look");
        assert_eq!(value["checks"][1]["state"], "failed");
    }

    /// The whole round: checks passed, call made, answer delivered — **with the
    /// rule attached**.
    ///
    /// The mutant that makes the last assertion fall is removing `caveat` from
    /// the output: whoever uses this node in six months will not have read the
    /// document that measured that rule.
    #[test]
    fn a_passed_preflight_lets_the_question_through_with_the_rule_attached() {
        let sandbox = Sandbox::new("ready");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"}]");
        pairs.push(("\"name\":\"list_projects\"", text_reply(ROOT)));
        pairs.push((
            "\"name\":\"impact\"",
            text_reply("22 files use this crate"),
        ));
        let spec = server(&sandbox, "ready", pairs);

        let outcome = McpAskAction
            .execute(
                &ask_input(&spec, vec![root_check()], vec![]),
                &SharedState::new(),
            )
            .expect("a round that went through breaks nothing");
        let value = went(outcome);
        assert_eq!(value["status"], "ok", "{}", value["said"]);
        assert_eq!(value["checks"][0]["state"], "passed");
        assert_eq!(value["text"], "22 files use this crate");
        assert!(
            value["caveat"]
                .as_str()
                .expect("the rule travels with the answer")
                .contains("not for deciding"),
            "the rule about the perimeter has to come out beside the answer"
        );
    }

    /// **A STEP CANNOT QUESTION AN INDEX WITHOUT SAYING WHOSE IT IS.**
    ///
    /// No check names `project_root`: the step does not start, and the error is
    /// of whoever wrote the flow — not an outcome of the world, since
    /// tolerating it would mean «go ahead and question an index you do not know
    /// the owner of». The mutant that makes it fall is dropping the call to
    /// `require_preflight`.
    #[test]
    fn a_step_cannot_question_an_index_without_saying_whose_it_is() {
        let sandbox = Sandbox::new("no-preflight");
        let mut pairs = handshake("[{\"name\":\"impact\"}]");
        pairs.push(("\"name\":\"impact\"", text_reply("what time is it")));
        let spec = server(&sandbox, "no-preflight", pairs);

        let error = McpAskAction
            .execute(&ask_input(&spec, vec![], vec![]), &SharedState::new())
            .expect_err("with no preflight check the step does not start");
        assert_eq!(error.class, "no_preflight");

        // With a written waiver, though, it does start: tolerance is a
        // declared decision, not a default.
        let mut waived = ask_input(&spec, vec![], vec!["check_failed", "could_not_look"]);
        waived["checks_waived_because"] =
            json!("this server knows nothing of directories: it answers about the system clock");
        McpAskAction
            .execute(&waived, &SharedState::new())
            .expect("with the waiver written down the step does start");
    }

    /// **A REFERENCE REACHES THE SERVER RESOLVED.**
    ///
    /// The directory and the arguments come from the previous step. Resolving
    /// them is `flow::step_input`'s job — one place for every action — and here
    /// the test redoes it with the same function because it calls `execute`
    /// without going through the executor. What this test asserts is that the
    /// action **uses** what it receives: the resolved `project_root` really does
    /// reach the server. That the input of *every* action arrives resolved is
    /// proved by `crates/flow/tests/a_reference_reaches_every_action.rs`, and
    /// this one does not repeat it.
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
                "name": "the index is of this directory",
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
                .expect("a directory taken through a reference arrives resolved"),
        );
        assert_eq!(value["status"], "ok", "{}", value["said"]);
        assert_eq!(value["project_root"], ROOT);
    }

    /// `mcp_ready` answers without calling the real tool, so a flow can branch
    /// **before paying**.
    #[test]
    fn the_readiness_node_answers_without_paying_for_the_real_call() {
        let sandbox = Sandbox::new("ready-node");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"}]");
        pairs.push(("\"name\":\"list_projects\"", text_reply(ROOT)));
        // «impact» has no prepared answer: were the node to call it, it would
        // hang until the deadline.
        let spec = server(&sandbox, "ready-node", pairs);

        let input = json!({
            "server": {"command": spec.command, "args": spec.args},
            "server_tool": "impact",
            "project_root": ROOT,
            "checks": [{
                "name": "the index is of this directory",
                "server_tool": "list_projects",
                "proves": ROOT,
                "blind_if": ["vector store not reachable"],
            }],
            "timeout_secs": 20,
        });
        let value = went(
            McpReadyAction
                .execute(&input, &SharedState::new())
                .expect("the check passes"),
        );
        assert_eq!(value["status"], "ready", "{}", value["said"]);
        assert_eq!(
            value.get("text"),
            None,
            "the check does not call the tool"
        );
    }

    /// **AN EMPTY PROOF WOULD ALWAYS PASS, AND NOBODY WRITES ONE ON PURPOSE.**
    ///
    /// Every text contains the empty string: with an empty `proves` the check
    /// stays green even when the server answers that it could not look. This is
    /// no textbook case — the normal way to get there is a `{"$from": …}`
    /// pointing at an empty field, and then the check stops checking **in
    /// silence**.
    ///
    /// The mutant that makes it fall is removing the check on `proves` from
    /// `require_preflight`: the outcome becomes `ok`, that is, a step that
    /// questioned a blind index believing it had verified it.
    #[test]
    fn an_empty_proof_proves_nothing_and_is_refused() {
        let sandbox = Sandbox::new("empty-proof");
        let mut pairs = handshake("[{\"name\":\"impact\"},{\"name\":\"list_projects\"}]");
        pairs.push((
            "\"name\":\"list_projects\"",
            text_reply("vector store not reachable"),
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
            .expect_err("a check that does not say what it proves never starts");
        assert_eq!(error.class, "invalid_input");

        // The same trap one rung up: with no declared directory, the tie
        // `require_preflight` imposes would be met by anything at all.
        let mut rootless = ask_input(&spec, vec![root_check()], vec![]);
        rootless["project_root"] = json!("");
        let error = McpAskAction
            .execute(&rootless, &SharedState::new())
            .expect_err("a step that does not say which directory it speaks of never starts");
        assert_eq!(error.class, "no_preflight");
    }

    /// An `accept` naming an impossible outcome is a typo of whoever wrote the
    /// flow, and shows at once instead of on the first run.
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
            .expect_err("«ok» is no failure, and nothing tolerates it");
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
