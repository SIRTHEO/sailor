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

    /// Una verifica interrotta si rifà: il suo mestiere è rileggere il mondo
    /// e dire com'è, non cambiarlo. Chi ci infila dentro un comando che
    /// modifica ha già rotto il contratto di questa azione, e lo aveva già
    /// rotto prima: il motore riesegue la verifica a ogni tentativo anche
    /// senza nessuna interruzione di mezzo.
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
    // **LA FORMA SI APPLICA SOLO A UN COMANDO RIUSCITO**, e qui il comando
    // si separa dal motore di proposito. Il motore pretende la forma anche
    // in `exit_error`, perché un motore che fallisce ha comunque parlato;
    // un comando fallito non ha prodotto la lettura richiesta. Chi ha
    // scritto `accept` ramifica già sullo stato, altrimenti non l'avrebbe
    // scritto.
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
    // **IL TESTO GREZZO NON ESCE DAL PASSO**: consegna `answer`, o niente.
    // È la stessa scelta che `an_engine_step_declares_what_it_can_return_and_what_it_hands_on`
    // pretende già dal motore, e lasciarlo passare accanto al valore
    // renderebbe la forma un ornamento.
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

    /// Una verifica fallita rompe il passo, e chi vuole ramificarci sopra lo
    /// dichiara. Senza questa seconda metà, «rosso» sarebbe l'unica cosa che
    /// una verifica sa fare, non una scelta.
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
