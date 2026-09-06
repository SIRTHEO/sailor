//! The shell check action: a command run under a time limit, whose verdict,
//! and optionally its reading, becomes the step's output.

use crate::answer::{
    check_tolerance, how_it_exited, shaped_answer, tolerates, what_it_said, ANSWER_SHAPE_CHECK,
    CHECK_FAILURES,
};
use crate::process::{
    run_shell_check_watched, sink_for_step, CheckInvocation, CheckResult, LiveSink, Pipe,
    StepSinks,
};
use flow::{
    Action, ActionError, ActionOutcome, Ran, Refusal, RefusalRule, SharedState, StepSpecies,
    ValueSchema,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct CheckSpec {
    command: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// As for the engine: empty means a failed check is a broken step.
    /// `["failed"]` puts it back among the data, for whoever wants to branch on
    /// the result rather than stop.
    #[serde(default)]
    accept: Vec<String>,
    timeout_secs: u64,
    /// Where the check runs. A person hardly ever writes it: the executor puts
    /// it there as it composes the input, taking it from the project root. An
    /// absolute path written here by hand never gets this far — `step_input`
    /// refuses it first.
    #[serde(default)]
    workdir: Option<String>,
    /// The shape of the reading, when this step does not only check but
    /// **reads**. Absent means as before: only the verdict goes downstream.
    ///
    /// The engine's twin control — `shape_was_asked_for`, refusing to spend
    /// when the shape is absent from the prompt — has no analogue here, and
    /// faking one would be worse than having none: `git` never receives your
    /// shape and cannot conform. So the pact is the other one: **the command
    /// must already emit JSON**, and if it does not the step goes red naming
    /// what to add — `--json`, `--format=json`, `| jq` — rather than guessing
    /// at a text whose format will change one day.
    #[serde(default)]
    answer_shape: Option<ValueSchema>,
    /// What is not recognised, for the same reason as `EngineSpec::extra`.
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

/// **THE FIRST CEILING ON VOLUME THIS PROJECT HAS.** The only ceiling there
/// was is on time: a slow command is killed, a talkative one is not. An engine
/// has a natural brake because it pays by token; a command prints for free,
/// and with no limit what it prints would end up in the store.
///
/// A million characters — a six-hundred-page book — is wide for real use and
/// narrow enough to catch the accidents. Past the ceiling the step goes red
/// and **does not truncate**: a lopped value looks whole, and whoever reads it
/// downstream has no way to know a piece is missing.
const MAX_ANSWER_BYTES: usize = 1_000_000;

/// Runs a shell check under a time limit, reading command, environment and
/// ceiling from the step's typed input. Same rule as the twin action, and for
/// the same reason: a check that fails breaks its own step, unless the step
/// declares `"accept": ["failed"]`.
///
/// **A reference to what an engine said goes in `env`, never in `command`.**
/// The command is shell text and gets executed; a model's answer pasted in
/// there is a command written by whoever answered. Inside an environment
/// variable it stays data, and the command reads it between quotes.
#[derive(Default)]
pub struct ShellCheckAction {
    watcher: Option<Arc<dyn StepSinks>>,
}

impl ShellCheckAction {
    /// With nobody watching: the check's text is seen at the end, as it has
    /// always been.
    pub fn new() -> Self {
        Self { watcher: None }
    }

    /// With somebody watching. It holds for a check as for an engine: a test
    /// suite running ten minutes is exactly as blind as one.
    pub fn watched_by(mut self, watcher: Option<Arc<dyn StepSinks>>) -> Self {
        self.watcher = watcher;
        self
    }
}

impl Action for ShellCheckAction {
    /// A command line of this machine reaches no engine and buys nothing, so
    /// it takes no room from a paid call in the same front.
    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }

    /// As for the engine, and out of the same struct.
    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        match serde_json::from_value::<CheckSpec>(declared.clone()) {
            Ok(spec) => spec.extra.into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }

    fn execute(&self, input: &Value, shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        self.execute_and_report(input, shared)
            .map(|(outcome, _)| outcome)
    }

    /// The line is said on the step's echo before the shell starts, so a check
    /// that hangs is still seen for what it is; and it travels with the
    /// outcome, or with the error, from the one place both leave.
    fn execute_and_report(
        &self,
        input: &Value,
        shared: &SharedState,
    ) -> Result<(ActionOutcome, Option<Ran>), ActionError> {
        let live = sink_for_step(&self.watcher, shared);
        let spec: CheckSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        check_tolerance(&spec.accept, &CHECK_FAILURES)?;
        let invocation = CheckInvocation {
            command: spec.command.clone(),
            env: spec.env.clone(),
            timeout: Duration::from_secs(spec.timeout_secs),
            workdir: spec.workdir.clone(),
        };
        let ran = invocation.ran();
        if let Some(live) = live.as_deref() {
            live.chunk(Pipe::Stderr, format!("[sailor] {}\n", ran.announce()).as_bytes());
        }
        match run_and_read(&spec, &invocation, live.as_deref()) {
            Ok(outcome) => Ok((outcome, Some(ran))),
            Err(error) => Err(error.having_run(ran)),
        }
    }

    /// An interrupted check is simply redone: its trade is to reread the world
    /// and say how it is, not to change it. Whoever slips a command that
    /// modifies inside it has broken this action's contract, and had broken it
    /// before: the engine reruns the check on every attempt, interruption or
    /// no interruption.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }

    /// It writes a verdict, so a flow may hang `decides_done` on it.
    fn is_a_check(&self) -> bool {
        true
    }
}

/// Runs the check and reads what it said, by the step's tolerances and the
/// shape it declared.
fn run_and_read(
    spec: &CheckSpec,
    invocation: &CheckInvocation,
    live: Option<&dyn LiveSink>,
) -> Result<ActionOutcome, ActionError> {
    let command = &invocation.command;
    let seconds = spec.timeout_secs;
    // **WHAT IS STILL OPEN LEAVES THE STEP, AND NOTHING ELSE DOES.** A step
    // that comes after a failed check receives this: that is how a second call
    // carries the unresolved part instead of the whole mandate again.
    let (status, said, unresolved) = match run_shell_check_watched(invocation, live) {
        CheckResult::Passed { stdout } => ("passed", Some(stdout), None),
        CheckResult::Failed {
            code,
            stdout,
            stderr,
        } => {
            let named = format!("the check `{command}` {}; {}", how_it_exited(code), what_it_said(&stdout, &stderr));
            if !tolerates(&spec.accept, "failed") {
                return Err(ActionError::new("check_failed", named).refused(Refusal::new(
                    "command",
                    "",
                    RefusalRule::ExitCode,
                    &stderr,
                )));
            }
            ("failed", None, Some(named))
        }
        CheckResult::TimedOut => {
            let named = format!(
                "the check `{command}` did not finish within {seconds} seconds and was killed"
            );
            if !tolerates(&spec.accept, "timed_out") {
                return Err(ActionError::new("check_timed_out", named));
            }
            ("timed_out", None, Some(named))
        }
    };
    let unresolved = unresolved.map(Value::String);
    // **THE SHAPE APPLIES ONLY TO A COMMAND THAT PASSED**, and here the
    // command parts from the engine on purpose. The engine demands the shape
    // even in `exit_error`, because an engine that fails has spoken anyway; a
    // failed command did not produce the reading asked of it. Whoever wrote
    // `accept` branches on the status, or would not have written it.
    let Some((shape, said)) = spec.answer_shape.as_ref().zip(said) else {
        return Ok(ActionOutcome::Went(verdict(status, unresolved, None)));
    };
    if said.len() > MAX_ANSWER_BYTES {
        return Err(ActionError::new(
            "answer_too_large",
            format!(
                "the reading of `{command}` weighs {} characters, past the cap of {MAX_ANSWER_BYTES}.                      The cap does not truncate: a cut value looks whole. Narrow what the                      command prints — with a filter, or by asking it for fewer fields.",
                said.len()
            ),
        )
        .refused(Refusal::new(
            ANSWER_SHAPE_CHECK,
            "",
            RefusalRule::TooLong,
            &said,
        )));
    }
    // **THE RAW TEXT DOES NOT LEAVE THE STEP**: `answer`, or nothing. It is
    // the same choice `an_engine_step_declares_what_it_can_return_and_what_it_hands_on`
    // demands of the engine, and letting it through beside the value would
    // make the shape an ornament.
    let answer = shaped_answer(shape, &said)?;
    Ok(ActionOutcome::Went(verdict(status, unresolved, Some(answer))))
}

/// The verdict as the run reads it. `status` is always there, and the two
/// optional fields are written only when they exist: a key present and null
/// would look to a downstream step like something it can work from.
fn verdict(status: &str, unresolved: Option<Value>, answer: Option<Value>) -> Value {
    let mut output = json!({ "status": status });
    for (key, value) in [("unresolved", unresolved), ("answer", answer)] {
        if let Some(value) = value {
            output[key] = value;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::with_references_resolved;
    use std::sync::Mutex;

    #[test]
    fn the_shell_check_action_reads_its_json_input() {
        let action = ShellCheckAction::new();
        let input = json!({"command": "true", "timeout_secs": 5});
        let shared = SharedState::new();
        let ActionOutcome::Went(output) = action.execute(&input, &shared).unwrap() else {
            panic!("una verifica eseguita è sempre Went")
        };
        assert_eq!(output["status"], "passed");
    }

    /// The final check reads the model's verdict from an environment variable.
    /// **Three cases, because one alone would prove nothing**: the same command
    /// must pass on one verdict and break the step on the other two — the
    /// opposite verdict, and the mute engine.
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
                .execute(&with_references_resolved(input), &SharedState::new())
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

    /// A failed check breaks its step, and whoever wants to branch on it says
    /// so. Without this second half, «red» would be the one thing a check knows
    /// how to do, not a choice.
    #[test]
    fn a_failing_check_breaks_its_step_unless_the_step_says_otherwise() {
        let strict = json!({"command": "echo perche 1>&2; exit 2", "timeout_secs": 5});
        let error = ShellCheckAction::new()
            .execute(&strict, &SharedState::new())
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
            .execute(&tolerant, &SharedState::new())
            .expect("l'esito è dichiarato accettabile")
        else {
            panic!("un esito tollerato resta un dato")
        };
        assert_eq!(output["status"], "failed");
    }

    /// **A CHECK THAT READS, NOT ONLY ONE THAT JUDGES.** The machinery lives
    /// ninety lines above: `shaped_answer` validates against the declared
    /// shape, and `pruned` cuts what the shape did not promise.
    ///
    /// THE MEASURE THAT COULD HAVE COME OUT DIFFERENTLY: `spurio` comes out of
    /// the command but **not** of the shape. Were the pruning not applied, the
    /// assertion on `answer.spurio` would find it and this test would go red.
    /// And were the raw text forwarded beside the value, `stdout` would appear
    /// in the output: the shortcut that would make the shape useless, and
    /// `an_engine_step_declares_what_it_can_return_and_what_it_hands_on`
    /// forbids it for the engine already.
    #[test]
    fn a_check_that_declares_a_shape_hands_on_a_value_not_only_a_verdict() {
        let input = json!({
            "command": r#"echo '{"conta": 3, "spurio": "non promesso"}'"#,
            // **`allow_extra` TRUE IS THE POINT OF THE TEST, NOT AN OVERSIGHT.**
            // With `false` an extra field is a refusal and the pruning never
            // comes into play; with `true` validation tolerates the field, and
            // what removes it is `pruned`. Setting it `false` here would make
            // this test green for the wrong reason.
            "answer_shape": {
                "type": "object",
                "properties": {"conta": {"type": "number"}},
                "required": ["conta"],
                "allow_extra": true
            },
            "timeout_secs": 5
        });

        let ActionOutcome::Went(output) = ShellCheckAction::new()
            .execute(&input, &SharedState::new())
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

    /// The two ways to be wrong, under the name the engine uses already.
    /// Line-wise reading of the text was discarded: a floor that gives way in
    /// silence the day the command changes format. Whoever writes the flow adds
    /// `--json` or `| jq`, and the red tells them so.
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
            .execute(&non_json, &SharedState::new())
            .expect_err("un comando che non emette JSON non ha prodotto una lettura");
        assert_eq!(error.class, "answer_not_json");

        let fuori_forma = json!({
            "command": r#"echo '{"conta": "tre"}'"#,
            "answer_shape": forma,
            "timeout_secs": 5
        });
        let error = ShellCheckAction::new()
            .execute(&fuori_forma, &SharedState::new())
            .expect_err("JSON valido ma fuori dalla forma dichiarata");
        assert_eq!(error.class, "answer_off_shape");
    }

    /// Beside the class, the error names the check that refused and what it
    /// saw: the field and its value for a shape, the text for an answer that
    /// is not JSON, the stderr for a command that exited red.
    #[test]
    fn a_refusal_names_the_check_the_field_and_what_it_saw() {
        let shape = json!({
            "type": "object",
            "properties": {"conta": {"type": "number"}},
            "required": ["conta"],
            "allow_extra": false
        });
        let off_shape = json!({
            "command": r#"echo '{"conta": "tre"}'"#,
            "answer_shape": shape.clone(),
            "timeout_secs": 5
        });
        let refusal = ShellCheckAction::new()
            .execute(&off_shape, &SharedState::new())
            .expect_err("off shape")
            .refusal
            .expect("a shape that refuses says so");
        assert_eq!(refusal.check, "answer_shape");
        assert_eq!(refusal.path, "$.conta");
        assert_eq!(refusal.rule, RefusalRule::WrongType);
        assert_eq!(refusal.seen, "\"tre\"");

        let not_json = json!({
            "command": "echo non sono json",
            "answer_shape": shape,
            "timeout_secs": 5
        });
        let refusal = ShellCheckAction::new()
            .execute(&not_json, &SharedState::new())
            .expect_err("not json")
            .refusal
            .expect("a text that is not JSON is refused by the shape");
        assert_eq!(refusal.check, "answer_shape");
        assert_eq!(refusal.rule, RefusalRule::NotJson);
        assert_eq!(refusal.seen, "non sono json");

        let red = json!({"command": "echo perche 1>&2; exit 2", "timeout_secs": 5});
        let refusal = ShellCheckAction::new()
            .execute(&red, &SharedState::new())
            .expect_err("a red command")
            .refusal
            .expect("a command that exits red is a check that refused");
        assert_eq!(refusal.check, "command");
        assert_eq!(refusal.rule, RefusalRule::ExitCode);
        assert_eq!(refusal.seen, "perche");
    }

    /// **WHAT IS OPEN COMES FROM BOTH PIPES.** A step after this one receives
    /// `unresolved` and nothing else, which is how the second call carries the
    /// open piece instead of the whole mandate. A suite's complaint lands on
    /// stdout far more often than on stderr, and a reading that took only
    /// stderr would hand an empty line downstream.
    #[test]
    fn a_failed_check_hands_on_what_is_still_unresolved() {
        let input = json!({
            "command": "echo 'section tre is empty'; echo 'exit 1' >&2; exit 1",
            "accept": ["failed"],
            "timeout_secs": 5
        });

        let ActionOutcome::Went(output) = ShellCheckAction::new()
            .execute(&input, &SharedState::new())
            .expect("l'esito è dichiarato accettabile")
        else {
            panic!("un esito tollerato resta un dato")
        };

        assert_eq!(output["status"], "failed");
        let unresolved = output["unresolved"]
            .as_str()
            .unwrap_or_else(|| panic!("a failing check names what is left: {output}"));
        assert!(unresolved.contains("section tre is empty"), "{unresolved}");
        assert!(unresolved.contains("exit 1"), "{unresolved}");
    }

    /// A check that passed has nothing open to hand on, and an empty key there
    /// would read downstream as work still to do.
    #[test]
    fn a_passing_check_hands_on_nothing_unresolved() {
        let input = json!({"command": "true", "timeout_secs": 5});

        let ActionOutcome::Went(output) = ShellCheckAction::new()
            .execute(&input, &SharedState::new())
            .expect("il controllo passa")
        else {
            panic!("un controllo passato è un dato")
        };

        assert_eq!(output["status"], "passed");
        assert!(output.get("unresolved").is_none(), "{output}");
    }

    /// A check the clock killed says so, and says how long it had: downstream
    /// it would otherwise read like a check that measured something.
    #[test]
    fn a_check_killed_by_the_clock_names_the_limit_it_hit() {
        let input = json!({
            "command": "sleep 5",
            "accept": ["timed_out"],
            "timeout_secs": 1
        });

        let ActionOutcome::Went(output) = ShellCheckAction::new()
            .execute(&input, &SharedState::new())
            .expect("l'esito è dichiarato accettabile")
        else {
            panic!("un esito tollerato resta un dato")
        };

        assert_eq!(output["status"], "timed_out");
        let unresolved = output["unresolved"].as_str().expect("dice cosa è successo");
        assert!(unresolved.contains("1 seconds"), "{unresolved}");
    }

    /// **HERE THE COMMAND PARTS FROM THE ENGINE, AND NOT BY OVERSIGHT.** The
    /// engine demands the shape even in `exit_error`, because an engine that
    /// fails has spoken anyway. A failed command did not produce the reading
    /// asked of it: letting a value through there would mean reading off a
    /// broken instrument. Whoever wrote `accept` branches on the status, or
    /// would not have written it.
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
            .execute(&input, &SharedState::new())
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

    /// **THE FIRST CEILING ON VOLUME SAILOR HAS.** Searched across all of
    /// `crates/`: there is none, not in the actions, the store or the registry.
    /// The only ceiling is on *time* — a slow command is killed, a talkative
    /// one is not; an engine has a natural brake because it pays by token, a
    /// command prints for free. Red and no truncation: a lopped value looks
    /// whole, and whoever reads it downstream cannot know a piece is missing.
    #[test]
    fn a_reading_above_the_ceiling_is_refused_instead_of_being_cut() {
        let input = json!({
            // Two million characters: twice the ceiling.
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
            .execute(&input, &SharedState::new())
            .expect_err("sopra il tetto il passo si ferma invece di tagliare");
        assert_eq!(error.class, "answer_too_large");
    }

    /// The record of a shell step carries the shell and the text as they were
    /// started, on the step that passed and on the one that broke alike.
    #[test]
    fn a_shell_step_reports_the_shell_and_the_text_it_ran() {
        let passed = json!({"command": "echo hi", "timeout_secs": 5});
        let (outcome, ran) = ShellCheckAction::new()
            .execute_and_report(&passed, &SharedState::new())
            .expect("the check passes");
        assert!(matches!(outcome, ActionOutcome::Went(_)));
        assert_eq!(ran, Some(Ran::new("sh", ["-c", "echo hi"])));

        let broke = json!({"command": "exit 2", "timeout_secs": 5});
        let error = ShellCheckAction::new()
            .execute_and_report(&broke, &SharedState::new())
            .expect_err("a failing check breaks its step");
        assert_eq!(error.class, "check_failed");
        assert_eq!(
            error.ran.as_deref(),
            Some(&Ran::new("sh", ["-c", "exit 2"])),
            "a broken check forgot the line it ran"
        );
    }

    /// Whoever watches the step reads the line before the shell starts, on the
    /// step's own echo: a check that hangs is then seen for what it is.
    #[test]
    fn the_step_says_what_it_is_about_to_run_before_running_it() {
        struct Recorder(Mutex<Vec<(Pipe, Vec<u8>)>>);

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

        let recorder = Arc::new(Recorder(Mutex::new(Vec::new())));
        let action =
            ShellCheckAction::new().watched_by(Some(Arc::new(OneSink(recorder.clone()))));
        let mut shared = SharedState::new();
        shared.insert(flow::CURRENT_STEP.to_owned(), json!("check"));
        action
            .execute(&json!({"command": "echo out", "timeout_secs": 5}), &shared)
            .expect("the check passes");

        let seen = recorder.0.lock().expect("nobody panics here").clone();
        let expected = format!("[sailor] {}\n", Ran::new("sh", ["-c", "echo out"]).announce());
        assert_eq!(
            seen.first()
                .map(|(pipe, bytes)| (*pipe, String::from_utf8_lossy(bytes).into_owned())),
            Some((Pipe::Stderr, expected)),
            "the line is not the first thing the watcher reads: {seen:?}"
        );
        assert!(
            seen.iter()
                .any(|(pipe, bytes)| *pipe == Pipe::Stdout && bytes == b"out\n"),
            "the command's own text still arrives: {seen:?}"
        );
    }
}
