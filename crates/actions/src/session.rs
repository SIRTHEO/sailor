//! Resuming instead of rediscovering: how a step opens, resumes or forks the
//! session another step left, and starts over when it cannot.

use crate::candidates::Candidate;
use crate::cost::Recording;
use crate::process::{LiveSink, Pipe};
use crate::recipe::{command_line_with, AskRecipe, SessionRecipe, SESSION_PLACEHOLDER};
use crate::spec::SessionUse;
use crate::{read_text, Pointer, Reading, Reports};
use ledger::SessionMode;

// ── resuming instead of rediscovering ────────────────────────────────────

/// The session options the engine declares, assembled with the rest of its
/// recipe. What the engine leaves undeclared stays `None` all the way here.
pub(crate) fn session_lines(recipe: &AskRecipe, declared: Option<SessionRecipe>) -> SessionRecipe {
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

/// The options with the placeholder replaced by the real identifier.
///
/// The substitution happens **inside** the option, not in place of it: `codex`
/// wants the identifier as an argument of its own and so does `claude`, but
/// nothing stops a future engine from wanting it glued to a `--session=`.
fn with_session_id(args: &[String], id: &str) -> Vec<String> {
    args.iter()
        .map(|arg| arg.replace(SESSION_PLACEHOLDER, id))
        .collect()
}

/// A fresh session identifier, in the shape the command lines ask for (a UUID).
///
/// **IT NEED NOT BE UNPREDICTABLE, IT MUST BE UNIQUE.** It guards nothing: it
/// names a conversation on the disk of whoever runs it. Within one process the
/// counter suffices on its own; across processes the random seed of
/// `RandomState` — which the operating system hands to each process — separates
/// the series. Pulling in a dependency for this would break the choice written
/// in the workspace `Cargo.toml`, which keeps three of them.
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
        // The variant a UUID must declare: two fixed bits at the top.
        ((low >> 48) as u16 & 0x3fff) | 0x8000,
        low & 0xffff_ffff_ffff
    )
}

/// Which options to run with, and under which session identifier this call
/// counts as having run.
pub(crate) struct SessionPlan {
    /// The session's command line. `None` means «the usual one», that is,
    /// starting over from scratch.
    pub(crate) args: Option<Vec<String>>,
    /// What to write in the store's `session_id` column **if the engine does
    /// not state its own**. See the comment on `ModelCallRecord::session_id`.
    recorded: Option<String>,
    /// Where to read the real identifier in what the engine will say. When it
    /// is there it **beats `recorded`**: the engine's word on which session it
    /// used outranks ours on which one we had asked for.
    read_id_from: Option<Pointer>,
    /// What the ledger will say this call did with the session. `None` is the
    /// step that asked for nothing at all.
    pub(crate) mode: Option<SessionMode>,
}

impl SessionPlan {
    /// From scratch, as it always was.
    fn from_scratch() -> Self {
        Self {
            args: None,
            recorded: None,
            read_id_from: None,
            mode: None,
        }
    }

    /// From nothing, on a step that had asked for something else.
    ///
    /// **THE ROW SAYS SO, NOT ONLY THE LIVE TEXT.** The line on the terminal is
    /// gone by morning; the bill is not, and a run of cold calls keeping no
    /// trace of having asked reads afterwards like a run that resumed.
    fn fell_back() -> Self {
        Self {
            mode: Some(SessionMode::ColdFallback),
            ..Self::from_scratch()
        }
    }

    /// The identifier to record, once the engine has spoken.
    pub(crate) fn session_id(&self, said: &str) -> Option<String> {
        match &self.read_id_from {
            Some(pointer) => read_text(said, pointer),
            None => self.recorded.clone(),
        }
    }
}

/// Says it to whoever watches as it happens, not to the store afterwards.
///
/// **A SILENT FALLBACK IS THE WORST OF BOTH**: the price of rediscovery is paid
/// and nobody knows it was paid, and whoever reads the flow will go on
/// believing that step resumes. It is the «clarity for whoever watches»
/// constraint applied to the case where the optimisation does **not** fire.
fn say_it_starts_over(live: Option<&dyn LiveSink>, named: &str, why: &str) {
    if let Some(live) = live {
        live.chunk(
            Pipe::Stderr,
            format!("[sailor] {named} riparte da zero: {why}\n").as_bytes(),
        );
    }
}

/// Decides whether this call opens, resumes, forks, or starts from scratch.
///
/// **IT NEVER FAILS, AND THAT CHOICE IS THE CONSTRAINT.** Every obstacle — the
/// engine cannot resume, the preceding step left no session behind, there is no
/// store to look in — leads to the usual command line. A flow written on a
/// machine that has `claude-code` must run on a machine whose engine cannot
/// resume: it runs worse, it does not run less.
pub(crate) fn session_plan(
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
        return SessionPlan::fell_back();
    }
    let Some(record) = record else {
        // The store is where a session settles and is found again: without one
        // there is nothing to open, because there would be nothing to resume.
        say_it_starts_over(
            live,
            named,
            &format!(
                "the step asks to {}, and this run has no store to put it in",
                asked.word()
            ),
        );
        return SessionPlan::fell_back();
    };
    match asked {
        SessionUse::Open => {
            let Some(line) = &candidate.session.open else {
                say_it_starts_over(live, named, "cannot open a session that can be found again");
                return SessionPlan::fell_back();
            };
            // **AN IDENTIFIER IS MINTED ONLY WITH SOMEWHERE TO PUT IT.** A line
            // with no placeholder belongs to an engine that names itself:
            // recording ours would write into the store a session that does not
            // exist on that machine, and the following step would go resume
            // nothing — having spent first.
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
                mode: Some(SessionMode::Opened),
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
                return SessionPlan::fell_back();
            };
            // With no tool identifier there is no engine to attribute a session
            // to: this is a `bin` written by hand in the step.
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
                return SessionPlan::fell_back();
            };
            SessionPlan {
                args: Some(with_session_id(line, &id)),
                // Forking mints a fresh identifier: if the engine stays quiet
                // the branch has no name and nobody will be able to continue
                // it. If it speaks, `read_id_from` picks the name up and the
                // branch becomes as continuable as the trunk.
                recorded: if forking { None } else { Some(id) },
                read_id_from: candidate.session.id_from.clone(),
                mode: Some(if forking {
                    SessionMode::Forked
                } else {
                    SessionMode::Resumed
                }),
            }
        }
    }
}

/// The part of a reading that belongs to **this** step.
///
/// **AN ENGINE THAT COUNTS PER CALL IS ALREADY ANSWERED**, and its reading is
/// returned untouched: nothing is subtracted from a number that was never a
/// running total. Only `per_session` takes the other road, against what this
/// run has already attributed to that session.
pub(crate) fn this_step_share(
    record: &Recording<'_>,
    candidate: &Candidate,
    session_id: Option<&str>,
    reading: Reading,
) -> Reading {
    let cumulative = candidate
        .declared_usage
        .as_ref()
        .is_some_and(|declared| declared.reports == Reports::PerSession);
    if !cumulative {
        return reading;
    }
    // **NO SESSION, NO BASELINE, NO SHARE.** A cumulative engine called outside
    // a session we can name states what the session has spent, and there is no
    // honest way to cut this call out of it: the row says unknown.
    let before = session_id
        .zip(candidate.id.as_deref())
        .and_then(|(session, cli)| {
            record
                .ledger
                .attributed_to_session(&record.run_id, session, cli)
                .ok()
        })
        .map(what_the_session_carried)
        .unwrap_or_default();
    models::usage::share_after(reading, &before)
}

/// The ledger's totals in the shape the subtraction speaks.
fn what_the_session_carried(so_far: ledger::SessionSoFar) -> Reading {
    Reading {
        input_tokens: so_far.input_tokens,
        output_tokens: so_far.output_tokens,
        cached_tokens: so_far.cached_tokens,
        cache_write_tokens: so_far.cache_write_tokens,
        cache_write_long_tokens: so_far.cache_write_long_tokens,
        total_tokens: so_far.total_tokens,
        turns: so_far.turns,
        declared_cost: so_far
            .declared_cost_micros
            .map(|micros| micros as f64 / 1_000_000.0),
        ..Reading::default()
    }
}

#[cfg(test)]
mod resuming_instead_of_rediscovering {
    //! The resume tests: a step continues another step's session rather than
    //! reopening a process that knows nothing.
    //!
    //! **NO REAL ENGINE.** The engines in here are shell scripts that write
    //! their own command line to a file: what is tested is **what reaches the
    //! engine** and **what stays in the store**, the two things this work
    //! stands or falls on. How many tokens are saved is told by a real run and
    //! never by a test: here it cannot be measured, and we do not pretend to
    //! measure it.

    use super::*;
    use crate::engine::ExternalEngineAction;
    use crate::recipe::{PromptVia, ToolResolver};
    use crate::EXTERNAL_ENGINE_ACTION;
    use flow::{Action, ActionOutcome, SharedState};
    use ledger::Ledger;
    use serde_json::{json, Value};
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sailor-sessione-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch directory");
        dir
    }

    /// A step declared blind is handed what the flow hands it and nothing else.
    /// The word is the flow's, not ours: no kind of work is read as a judge.
    #[test]
    fn a_step_declared_blind_carries_no_option_that_would_continue_a_session() {
        let dir = scratch("blind");
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        ledger
            .record_model_call(&a_call_that_opened("s-1"))
            .expect("a session left behind by the step before");
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
            account: None,
            id: Some("motore".to_owned()),
            bin: "eco".to_owned(),
            args: vec!["ask".to_owned()],
            prompt: PromptVia::Stdin,
            unusable_when: Vec::new(),
            exhausted_when: Vec::new(),
            cooldown_secs: None,
            waits_for_a_person_when: Vec::new(),
            declared_usage: None,
            can_be_asked: true,
            why: None,
            ceiling: None,
            no_ceiling_because: "this fixture declares none".to_owned(),
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
            fell_back_from: Vec::new(),
            session_mode: Some(SessionMode::Opened),
        }
    }

    /// An engine that **appends** the command line it was invoked with:
    /// appends, because one test calls it four times, and overwriting would
    /// keep the last of them alone.
    const LOGS_ITS_ARGUMENTS: &str = r#"cat > /dev/null
printf '%s\n' "$*" >> "$(dirname "$0")/invocations"
printf 'ok'"#;

    /// An engine that, besides logging the line, **announces** the session it
    /// is speaking through — announcing a different one per invocation, as a
    /// real engine does when it forks.
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
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write the fake engine");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make it executable");
        path.to_string_lossy().into_owned()
    }

    fn invocations(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("invocations"))
            .expect("the fake engine wrote down its own invocations")
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

    /// A resolver that declares the ask recipe and — separately — what that
    /// engine can do with its own sessions. The two travel apart in real life
    /// as well.
    struct Declares {
        bin: String,
        sessions: Option<SessionRecipe>,
    }

    impl ToolResolver for Declares {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                TOOL => Ok(self.bin.clone()),
                other => Err(format!("«{other}» is not on this machine")),
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
                waits_for_a_person_when: Vec::new(),
                usage: None,
            })
        }
        fn session_recipe(&self, _id: &str) -> Option<SessionRecipe> {
            self.sessions.clone()
        }
    }

    /// An engine that knows all three ways, like `claude-code`.
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

    /// An engine that mints its own identifier and **prints** it, like `codex`:
    /// it opens on the usual line, and the name is read back out.
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
            other => panic!("the step «{step}» had to go: {other:?}"),
        }
    }

    /// **WHOEVER OPENS LAYS THE IDENTIFIER IN THE STORE, OR NOBODY WILL BE
    /// ABLE TO RESUME IT.** The two halves are tested together on purpose: an
    /// identifier handed to the engine and left unrecorded is, to the following
    /// step, indistinguishable from a session that was never opened.
    #[test]
    fn a_step_that_opens_a_session_hands_it_to_the_engine_and_writes_it_down() {
        let dir = scratch("opens");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger.clone()));

        ran(&action, &step_that(json!("open")), "corsa-1", "scopri");

        let line = invocations(&dir).remove(0);
        let written = ledger
            .session_opened_by("corsa-1", "scopri", TOOL)
            .expect("the ledger answers")
            .expect("and it recorded the session");
        assert!(
            line.contains("--session-id") && line.contains(&written),
            "the engine has to receive the very identifier the ledger keeps: \
             line «{line}», recorded «{written}»"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **THE CASE THAT PAYS MOST, AND THE REASON FOR ALL THIS WORK.** Three
    /// independent steps look at the same tree at the same moment: with no fork
    /// they make three identical discoveries and pay for them three times. Here
    /// all three must get the same trunk, and each its own branch.
    ///
    /// **AND ALL THREE MUST RECORD AN UNKNOWN SESSION.** Forking mints an
    /// identifier the engine does not tell us: writing the parent's there would
    /// have whoever believes they are on their own branch resume the trunk — in
    /// silence, which is the worst way.
    #[test]
    fn three_independent_steps_fork_one_discovery_instead_of_doing_it_three_times() {
        let dir = scratch("forks");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
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
            .expect("the ledger answers")
            .expect("the trunk is recorded");
        let lines = invocations(&dir);
        assert_eq!(lines.len(), 4, "one discovery and three branches");
        for line in &lines[1..] {
            assert!(
                line.contains(&trunk) && line.contains("--fork-session"),
                "every branch starts from the trunk without continuing it: «{line}»"
            );
        }
        for step in ["struttura", "rischi", "attrito"] {
            assert_eq!(
                ledger
                    .session_opened_by("corsa-2", step, TOOL)
                    .expect("the ledger answers"),
                None,
                "the branch of «{step}» carries an identifier the engine never told us"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Whoever resumes continues the **same** session and passes it on: three
    /// steps in a row must be able to continue from one another.
    #[test]
    fn resuming_keeps_the_same_session_so_the_next_step_can_take_it_too() {
        let dir = scratch("resumes");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
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
            .expect("the ledger answers")
            .expect("the trunk is recorded");
        assert_eq!(
            ledger
                .session_opened_by("corsa-3", "piano", TOOL)
                .expect("it answers"),
            Some(trunk.clone()),
            "whoever resumes does not change session, and can therefore pass it on"
        );
        let lines = invocations(&dir);
        assert!(
            lines[2].contains(&trunk) && !lines[2].contains("--fork-session"),
            "the third step continues the very same trunk: «{}»",
            lines[2]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **THE STANDING CONSTRAINT, TESTED.** An engine that cannot fork turns
    /// neither red nor into a special case: it gets the usual line, starts from
    /// scratch and pays more. Were someone to make the step fail «so as to hide
    /// no problem», this test would catch them.
    #[test]
    fn an_engine_that_cannot_fork_starts_over_instead_of_breaking() {
        let dir = scratch("cannot-fork");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
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
            "an engine that cannot fork gets the usual line, not a truncated one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An engine that declares **nothing** about sessions works as it did,
    /// even where the step asks to open one: the case of three of the four
    /// engines installed on this machine.
    #[test]
    fn an_engine_that_declares_no_sessions_works_exactly_as_before() {
        let dir = scratch("mute");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
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
                .expect("it answers"),
            None,
            "there is no session to record, and none is invented"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Forking from a step that left nothing behind starts from scratch. The
    /// case of whoever forks from a step name that is not there — a typo — and
    /// of whoever forks from a step that landed on another engine that day.
    #[test]
    fn forking_from_a_step_that_left_no_session_starts_over() {
        let dir = scratch("no-trunk");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger));

        ran(
            &action,
            &step_that(json!({ "fork": "a-step-that-is-not-there" })),
            "corsa-6",
            "rischi",
        );

        assert_eq!(invocations(&dir), vec!["--ask".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **AN ENGINE THAT NAMES ITSELF IS NOT AN EXCLUDED ENGINE.** Verified on
    /// `codex`, which has no option to impose an identifier and **prints** it
    /// instead: without this road, engines that mint their own would be shut
    /// out of a capability they have.
    ///
    /// And the branch becomes **continuable in turn**: the third step forks
    /// from the second, not from the first. Without reading the branch's
    /// identifier, a chain of three steps would snap back to the first
    /// discovery, and no error would say so — a wrong context would just land.
    #[test]
    fn an_engine_that_names_its_own_session_is_read_and_its_branch_is_continuable() {
        let dir = scratch("names-itself");
        let bin = engine_that(&dir, ANNOUNCES_ITS_SESSION);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
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
                .expect("it answers"),
            Some("sessione-1".to_owned()),
            "the identifier is the engine's word, not ours to decide"
        );
        assert_eq!(
            ledger
                .session_opened_by("corsa-7", "rischi", TOOL)
                .expect("it answers"),
            Some("sessione-2".to_owned()),
            "and the branch has one of its own, not the trunk's"
        );
        let lines = invocations(&dir);
        assert_eq!(
            lines[1], "--ask fork sessione-1",
            "the branch starts from the trunk"
        );
        assert_eq!(
            lines[2], "--ask fork sessione-2",
            "and the next branch starts from that branch"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A NAME THAT CANNOT BE DELIVERED IS NOT MINTED.** An engine that opens
    /// on the usual line receives no identifier: writing one of ours into the
    /// store would have the following step resume a session that does not exist
    /// on that machine — and it would find out having spent first.
    #[test]
    fn a_session_we_cannot_name_is_not_named_by_us() {
        let dir = scratch("a-name-that-cannot-be-delivered");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            // Opens on the usual line and does not say where it writes its own
            // name: the case of an engine with neither road.
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
                .expect("it answers"),
            None,
            "a session nobody knows how to name stays nameless in the ledger"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two calls never get the same identifier: were they to, two different
    /// sessions would write over each other on the runner's disk, and the
    /// following step would resume a mixture.
    #[test]
    fn two_sessions_never_get_the_same_identifier() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..1000 {
            assert!(
                seen.insert(fresh_session_id()),
                "a repeated identifier"
            );
        }
        // And the shape is the one the command lines ask for: five groups
        // parted by hyphens, the version in its right place.
        let one = fresh_session_id();
        let groups: Vec<&str> = one.split('-').collect();
        assert_eq!(
            groups.iter().map(|group| group.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12],
            "«{one}» has not the shape of a UUID"
        );
        assert!(one.starts_with(|c: char| c.is_ascii_hexdigit()));
        assert!(groups[2].starts_with('4'), "the version: «{one}»");
    }

    // ── what the ledger says a call did with the session ────────────────

    fn calls_in(dir: &std::path::Path) -> Vec<ledger::ModelCallRecord> {
        let ledger = Ledger::open(dir).expect("reopen the store");
        let dump = ledger.projection_dump().expect("read the projection");
        ui::parse::parse_model_calls(&dump)
    }

    fn only_call(dir: &std::path::Path) -> ledger::ModelCallRecord {
        let mut calls = calls_in(dir);
        assert_eq!(calls.len(), 1, "one call only: {calls:?}");
        calls.remove(0)
    }

    /// A step with no `session` key at all: the line the engine gets and the
    /// row the store keeps must be the ones they were before any of this.
    fn plain_step() -> Value {
        json!({ "tool": TOOL, "stdin": "guarda l'albero", "timeout_secs": 20 })
    }

    /// **A FLOW THAT DECLARES NOTHING BEHAVES AS IT DID.** All of this is
    /// opt-in: with no declaration the engine is invoked on its own recipe and
    /// nothing is written in the session columns.
    #[test]
    fn a_step_that_declares_no_session_is_invoked_and_recorded_as_before() {
        let dir = scratch("declares-nothing");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger));

        ran(&action, &plain_step(), "corsa-9", "scopri");

        assert_eq!(
            invocations(&dir),
            vec!["--ask".to_owned()],
            "no session option on a line that asked for none"
        );
        let call = only_call(&dir.join("deposito"));
        assert_eq!(call.session_id, None);
        assert_eq!(
            call.session_mode, None,
            "whoever asked for nothing has nothing to confess"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **THE FALLBACK IS ON THE ROW, NOT ONLY ON THE TERMINAL.** An engine that
    /// cannot resume gives the step a cold call; without this column that run's
    /// bill would read exactly like a run where every step resumed.
    #[test]
    fn a_step_that_had_to_start_over_says_so_in_the_ledger() {
        let dir = scratch("fallback-recorded");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            // Opens, cannot resume: three of the four engines on this machine.
            sessions: Some(SessionRecipe {
                open: knows_all_three().open,
                ..SessionRecipe::default()
            }),
        })
        .recording_to(Some(ledger));

        ran(
            &action,
            &step_that(json!({ "resume": "scopri" })),
            "corsa-10",
            "piano",
        );

        let call = only_call(&dir.join("deposito"));
        assert_eq!(
            call.session_mode,
            Some(SessionMode::ColdFallback),
            "the step had asked to resume, and the call started from nothing"
        );
        assert_eq!(call.session_id, None, "and there is no session to name");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A BLIND STEP IS COLD, AND THE ROW SAYS IT WAS ASKED TO BE OTHERWISE.**
    /// The check refuses the pair before a run; a step reached by a road the
    /// check never walked still leaves a trace of what it asked for.
    #[test]
    fn a_blind_step_gets_a_cold_call_and_the_ledger_records_the_fallback() {
        let dir = scratch("blind-recorded");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger));

        // A session really is left behind, or the blind step would start over
        // for want of one and the test would pass without blindness.
        ran(&action, &step_that(json!("open")), "corsa-11", "scopri");
        let mut step = step_that(json!({ "resume": "scopri" }));
        step["blind"] = json!(true);
        ran(&action, &step, "corsa-11", "giudica");

        assert_eq!(
            invocations(&dir)[1],
            "--ask",
            "no option that would carry an earlier context in"
        );
        let judging = calls_in(&dir.join("deposito"))
            .into_iter()
            .find(|call| call.step_id.as_deref() == Some("giudica"))
            .expect("the blind step wrote its row");
        assert_eq!(judging.session_mode, Some(SessionMode::ColdFallback));
        assert_eq!(judging.session_id, None, "and it continued nobody");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// And a call that really did continue says the other thing, so the two are
    /// told apart by the row and not by whoever remembers the run.
    #[test]
    fn a_call_that_really_resumed_is_written_down_as_resumed() {
        let dir = scratch("resume-recorded");
        let bin = fake_engine(&dir);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Declares {
            bin,
            sessions: Some(knows_all_three()),
        })
        .recording_to(Some(ledger));

        ran(&action, &step_that(json!("open")), "corsa-12", "scopri");
        ran(
            &action,
            &step_that(json!({ "resume": "scopri" })),
            "corsa-12",
            "piano",
        );

        let modes: Vec<Option<SessionMode>> = calls_in(&dir.join("deposito"))
            .iter()
            .map(|call| call.session_mode)
            .collect();
        assert!(
            modes.contains(&Some(SessionMode::Opened))
                && modes.contains(&Some(SessionMode::Resumed)),
            "one row opens and the other continues: {modes:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── whose numbers those are: this call's, or the session's ──────────

    /// An engine that keeps counting from the moment its session opened: every
    /// answer states a thousand more than the one before it.
    const COUNTS_THE_WHOLE_SESSION: &str = r#"cat > /dev/null
here="$(dirname "$0")"
printf '%s\n' "$*" >> "$here/invocations"
n=$(cat "$here/counter" 2>/dev/null || echo 0)
n=$((n + 1))
printf '%s' "$n" > "$here/counter"
printf '{"result":"ok","usage":{"input_tokens":%s}}' "$((n * 1000))""#;

    /// The same engine, declared one way or the other. Nothing but the
    /// declaration separates the two runs.
    struct Counting {
        bin: String,
        reports: Reports,
    }

    impl ToolResolver for Counting {
        fn resolve(&self, id: &str) -> Result<String, String> {
            match id {
                TOOL => Ok(self.bin.clone()),
                other => Err(format!("«{other}» is not on this machine")),
            }
        }
        fn ask_recipe(&self, _id: &str) -> Option<AskRecipe> {
            Some(AskRecipe {
                args: vec!["--ask".to_owned()],
                prompt: PromptVia::Stdin,
                args_before_prompt: Vec::new(),
                unusable_when: Vec::new(),
                exhausted_when: Vec::new(),
                cooldown_secs: None,
                waits_for_a_person_when: Vec::new(),
                silent_without_prompt: false,
                refuses_without_prompt: Vec::new(),
                usage: Some(crate::recipe::UsageRecipe {
                    args: Vec::new(),
                    declared: crate::Declared {
                        read: crate::Shape::Json,
                        from: models::usage::Heard::Stdout,
                        reports: self.reports,
                        input_tokens: Some(Pointer::Path(vec![
                            "usage".to_owned(),
                            "input_tokens".to_owned(),
                        ])),
                        answer: Some(Pointer::Path(vec!["result".to_owned()])),
                        ..crate::Declared::default()
                    },
                }),
            })
        }
        fn session_recipe(&self, _id: &str) -> Option<SessionRecipe> {
            Some(knows_all_three())
        }
    }

    /// Runs open-then-resume against the counting engine and returns what each
    /// row was charged, in the order the calls were made.
    fn charged_to_each_step(label: &str, reports: Reports) -> Vec<Option<u64>> {
        let dir = scratch(label);
        let bin = engine_that(&dir, COUNTS_THE_WHOLE_SESSION);
        let ledger = Ledger::open(dir.join("deposito")).expect("open the ledger");
        let action = ExternalEngineAction::resolving_with(Counting { bin, reports })
            .recording_to(Some(ledger));

        ran(&action, &step_that(json!("open")), "corsa-13", "scopri");
        ran(
            &action,
            &step_that(json!({ "resume": "scopri" })),
            "corsa-13",
            "piano",
        );

        let mut calls = calls_in(&dir.join("deposito"));
        // In the order they were made: the counter at the end of `call_id` is
        // the only field that keeps it — the step names sort the other way.
        calls.sort_by_key(|call| {
            call.call_id
                .rsplit(':')
                .next()
                .and_then(|tail| tail.parse::<u64>().ok())
                .expect("every call carries its own sequence number")
        });
        let _ = std::fs::remove_dir_all(&dir);
        calls.iter().map(|call| call.input_tokens).collect()
    }

    /// **THE DIFFERENCE BETWEEN TWO READINGS IS THE SECOND STEP'S SHARE.** The
    /// engine says 1000 then 2000; charging the second step 2000 would count
    /// the first step's thousand twice and make the resumed call look dearer
    /// than the cold one it replaced.
    #[test]
    fn a_cumulative_engine_charges_each_step_only_what_it_added() {
        assert_eq!(
            charged_to_each_step("consumo-cumulativo", Reports::PerSession),
            vec![Some(1_000), Some(1_000)]
        );
    }

    /// **AND WHICH ONE IT IS, IS DECLARED.** The same engine read as per-call
    /// charges the second step the whole 2000: the two readings are identical,
    /// so only the descriptor can tell them apart.
    #[test]
    fn the_same_numbers_read_as_per_call_are_not_touched() {
        assert_eq!(
            charged_to_each_step("consumo-per-chiamata", Reports::PerCall),
            vec![Some(1_000), Some(2_000)]
        );
    }
}
