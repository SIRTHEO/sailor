//! Two questions asked of an engine without spending: whether the line a
//! descriptor mounts is sound, tried without the prompt, and whether the home
//! it starts from is authenticated.

use crate::equipment::current_equipment_for;
use crate::process::{invoke_external_engine, EngineInvocation, EngineResult};
use crate::recipe::{command_line, mentions_any, says_it_cannot_work, AskRecipe, PromptVia};
use crate::{read_scalar, Pointer};
use std::collections::BTreeMap;
use std::time::Duration;

// ── the dry trial of a command line ─────────────────────────────────────────

/// How a descriptor's command line stands, tried **without the prompt**.
///
/// **WHY THERE IS NO PASS/FAIL.** Five outcomes because there are five repairs,
/// and the reader must know which is theirs: a broken line is fixed in the
/// descriptor, an exhausted engine waited out, a silent descriptor measured, a
/// mute engine investigated. One word for two sends people to the wrong work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeVerdict {
    /// The engine said "only the prompt was missing": the line is sound.
    Sound,
    /// The engine complained about **something else**: the line is malformed,
    /// and its words are the diagnosis. On fault 27 the sentence from `agy`
    /// said exactly which flag had eaten which argument — no classification of
    /// ours could have said as much.
    Broken { said: String },
    /// The engine said it cannot work right now — quota, credentials — which
    /// says nothing about the line: try again when it comes back.
    CannotWork { said: String },
    /// The descriptor does not declare how this engine refuses with no prompt.
    /// **Not "the line is sound"**: nobody looked.
    NotDeclared,
    /// No answer within the time cap, or a process that never started.
    ///
    /// The reason travels with the verdict because the two are repaired in
    /// different ways, and a report that confused them would send someone
    /// hunting a slow engine where there is an executable that does not start.
    TimedOut { why: String },
}

/// The verdict on a line tried dry, **without executing anything**.
///
/// **WHY IT IS PURE AND SEPARATE FROM THE EXECUTOR.** Judgement is the part that
/// goes wrong, and a test that had to start a real engine cannot be written: it
/// is tried against texts the engines really said, copied once and left frozen.
///
/// **THE VERDICT IS IN THE TEXT, NOT IN THE EXIT CODE**, and that is not a
/// preference. Measured on this machine, `agy` exits **2** both when it refuses
/// properly ("flag needs an argument: -print") and when the line is the
/// malformed one of fault 27 ("--print took "--output-format" as its prompt…").
/// A probe judging by exit status would see the two as identical and walk past
/// fault 27 exactly as its author did — so this function is not even handed the
/// exit code: there is no way to use it by mistake.
///
/// **THE READING ORDER IS BINDING: `unusable_when` FIRST.** An engine out of
/// quota complains about the quota, not the line; the other way round, an
/// exhausted `claude` would be declared **broken**, sending the reader to
/// correct a healthy descriptor when waiting was enough. An exhausted engine is
/// not a broken engine.
pub fn judge_dry_run(recipe: &AskRecipe, stdout: &str, stderr: &str) -> ProbeVerdict {
    // Both pipes are read together: an engine writing its refusal to stdout and
    // one writing it to stderr are the same case, and picking only one would tie
    // the verdict to a detail no descriptor declares.
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

/// What an engine said to the line mounted without a prompt, or why it said
/// nothing.
#[derive(Clone, Debug)]
pub enum DryRun {
    Answered { stdout: String, stderr: String },
    NoAnswer { why: String },
}

/// Who runs the dry trial.
///
/// **WHY A TRAIT AND NOT A DIRECT CALL.** Otherwise every test here would start
/// real `claude`, `codex` and `agy`, depending on the host's installs and that
/// day's quota — coming out different for a reason it never declares. With a
/// trait the tests inject four fake executables and get four fixed verdicts.
pub trait DryProbe: Send + Sync {
    fn run(&self, bin: &str, args: &[String], stdin: Option<Vec<u8>>) -> DryRun;
}

/// The time cap of a dry trial.
///
/// **AN EXPLICIT CAP IS NEEDED BECAUSE `timeout` AND `gtimeout` DO NOT EXIST ON
/// THIS MACHINE**: verified with `command -v`. Anyone expecting to prefix the
/// line with them would find out only when an engine waits on something and
/// blocks every other check. `invoke_external_engine` applies it; it has one.
pub const DRY_PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// The real probe: it mounts the line and runs it without the prompt.
pub struct RealDryProbe;

impl DryProbe for RealDryProbe {
    fn run(&self, bin: &str, args: &[String], stdin: Option<Vec<u8>>) -> DryRun {
        // **THE SAME EQUIPMENT AS THE REAL RUN, AND HERE LIES THE WHOLE VALUE
        // OF THE CHECK.** With an empty map the check would try the engine in
        // the home of whoever opened the terminal — authenticated — while the
        // step starts it in the active profile's home, which may hold no
        // credential at all: `flow check` closes green and the run fails, and
        // whoever read the green did nothing wrong. A check that tries a world
        // other than the one worked in is worse than none, because it reassures.
        //
        // **NOTHING FROM A STEP'S SCOPE, AND THAT IS DELIBERATE.** The check is
        // not trying a step: it is trying the line the **descriptor** mounts,
        // that one time per engine. The variables a step declares hold for that
        // one call, and slipping them in here would give a verdict that does
        // not hold for the other steps naming the same engine.
        //
        // **THIS DOES NOT SAY WHETHER THE HOME IS AUTHENTICATED, AND THAT IS A
        // LIMIT OF THE TECHNIQUE.** The check removes the prompt on purpose, so
        // the engine stops at the missing prompt and never reaches the checks
        // that would come after — the credentials are over there. Measured in
        // both homes: `codex exec < /dev/null` answers **the same thing** — "No
        // prompt provided via stdin." — and exits 1 both times.
        //
        // **THE MISSING QUESTION IS ASKED APART**: by `probe_login_status`,
        // which asks the engine with the words the descriptor declares in
        // `login_status`. It does not belong here: this probe tries *the line*,
        // and mixing the two verdicts would make it impossible to say which of
        // them said no.
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
            // A refusal exits nonzero, so the normal case lands here; but an
            // engine that exits **zero** with no prompt is all the more worth
            // looking at, and throwing it away would hide it.
            EngineResult::Ok { stdout, stderr }
            | EngineResult::ExitError { stdout, stderr, .. }
            | EngineResult::WaitingForAPerson { stdout, stderr } => {
                DryRun::Answered { stdout, stderr }
            }
            EngineResult::TimedOut => DryRun::NoAnswer {
                why: format!(
                    "no answer within {} seconds",
                    DRY_PROBE_TIMEOUT.as_secs()
                ),
            },
            EngineResult::SpawnFailed { reason } => DryRun::NoAnswer {
                why: format!("the process did not start: {reason}"),
            },
        }
    }
}

/// Mounts a recipe's line without the prompt, has it tried, and judges.
///
/// **HOW THE PROMPT IS REMOVED DEPENDS ON WHERE IT WENT**, the one mounting
/// choice this function makes: stdin-fed engines get an **empty, closed** stdin
/// (`< /dev/null`), last-argument engines get the line minus that argument. Get
/// it wrong and there is no error, only an engine that *waits* — a hung check.
pub fn probe_dry_run(probe: &dyn DryProbe, bin: &str, recipe: &AskRecipe) -> ProbeVerdict {
    probe_dry_run_with(probe, bin, recipe, &command_line(recipe))
}

/// The same trial, on a line the caller already assembled.
///
/// **WHY THE LINE COMES FROM OUTSIDE.** Since a step may name the model it
/// wants, the line no longer comes from the descriptor alone: assembling it
/// again here would try one without the model and call sound a line the run
/// will not use. Those are the two doors of fault 1.
pub fn probe_dry_run_with(
    probe: &dyn DryProbe,
    bin: &str,
    recipe: &AskRecipe,
    args: &[String],
) -> ProbeVerdict {
    let stdin = match recipe.prompt {
        PromptVia::Stdin => Some(Vec::new()),
        PromptVia::LastArg => None,
    };
    match probe.run(bin, args, stdin) {
        DryRun::Answered { stdout, stderr } => judge_dry_run(recipe, &stdout, &stderr),
        DryRun::NoAnswer { why } => ProbeVerdict::TimedOut { why },
    }
}

// ── is the home authenticated? the engine says ──────────────────────────────

/// How an engine is asked **whether the home it starts from is authenticated**,
/// and with which words it answers yes and no.
///
/// **WHY THE DISK IS NOT INSPECTED.** Hunting for `auth.json` would be a second
/// copy of the truth, rewritten per engine and kept aligned by hand as engines
/// move where they keep things. The engine knows; the descriptor declares only
/// **how to ask** and **how to recognise the answer** — the discipline of
/// `unusable_when` and `refuses_without_prompt`, applied to a third question.
///
/// **WHY A CHANNEL OF ITS OWN IS NEEDED, AND THE DRY TRIAL IS NOT ENOUGH.**
/// `flow check` tries the line **without the prompt**: the engine stops at "you
/// gave me nothing to do" and never reaches the checks that come after, where
/// the credentials are. Measured in both homes, `codex exec < /dev/null` answers
/// "No prompt provided via stdin." and exits 1 **in both**, word for word the
/// same. A limit of the technique, not a defect to repair inside it: the
/// credentials question is asked apart, and costs nothing — it is local, and no
/// provider is called.
#[derive(Clone, Debug)]
pub struct LoginRecipe {
    /// The options, or the subcommand, the question is asked with: `["login",
    /// "status"]`, `["auth", "status"]`.
    pub args: Vec<String>,
    /// Where the answer sits inside what the engine said.
    ///
    /// **IT IS `usage`'s POINTER, NOT A SECOND MECHANISM**, because the
    /// problem is the same: two engines say the same thing in two shapes.
    /// `codex` answers in prose — "Logged in using ChatGPT" — so there is
    /// nothing to point at, the subject is everything it said. `claude`
    /// answers in a JSON wrapper with a boolean field, `"loggedIn": true`,
    /// and there the key path reaches it.
    ///
    /// `None` is not "do not look": it is "the subject is the whole output",
    /// the commonest shape and the one that requires declaring nothing.
    pub answer: Option<Pointer>,
    /// The words this engine declares it **is** authenticated with.
    pub logged_in_when: Vec<String>,
    /// The words it declares it is **not**.
    ///
    /// **BOTH MUST BE DECLARED, AND ONE MISSING TURNS THE CHECK OFF.** A
    /// descriptor recognising the yes alone would call every no
    /// "unrecognised", and the reader could not tell an unauthenticated
    /// engine from one that answered something strange. Better to stay
    /// silent: see [`LoginVerdict::NotDeclared`].
    pub logged_out_when: Vec<String>,
}

/// What could be learned about a home's credentials.
///
/// **FOUR OUTCOMES AND NOT TWO, FOR THE USUAL REASON.** "Nobody looked", "it
/// answered and I did not understand it" and "it said no" are three different
/// facts, and **none of the three is a yes**. A two-state type would force a
/// choice of which side the first two fall on, and the convenient direction is
/// always the reassuring one — the one that puts the defect back.
#[derive(Clone, Debug)]
pub enum LoginVerdict {
    /// The engine declares it is authenticated in this home.
    LoggedIn { said: String },
    /// The engine declares it is **not**: calls will go out with no
    /// credentials.
    LoggedOut { said: String },
    /// The descriptor omits the block, or declares half of it. **Nobody
    /// looked**, and there is nothing to say about this home.
    NotDeclared,
    /// It answered, and the answer resembles neither declared shape. Its
    /// words are the diagnosis.
    Unrecognised { said: String },
    /// It did not answer at all: it never started, or it ran past the time cap.
    NoAnswer { why: String },
}

impl LoginVerdict {
    /// True **only** when the engine said yes. Every other outcome, doubt
    /// included, answers no: this is how the direction of the error gets
    /// written once instead of at every reading site.
    pub fn is_logged_in(&self) -> bool {
        matches!(self, LoginVerdict::LoggedIn { .. })
    }
}

/// Reads an engine's answer to "are you authenticated?".
///
/// **PURE, AND SEPARATE FROM THE EXECUTOR**, for the reason behind
/// [`judge_dry_run`]: judgement is the part that goes wrong, and a test that had
/// to launch `codex` would report the state of the runner's machine instead of
/// whether the recognition works.
///
/// **THE EXIT CODE DOES NOT ENTER HERE EITHER.** On the two engines measured it
/// *would* distinguish — `codex login status` exits 1 unauthenticated and 0
/// authenticated, `claude auth status` the same — but that is a fact about those
/// two, not a rule to write into the code: an engine answering "Not logged in"
/// while exiting zero would be declared authenticated by anyone reading the
/// status, and nobody would notice. The descriptor declares the text, not it.
///
/// **THE READING ORDER IS BINDING: THE NO FIRST.** "Not logged in" *contains*
/// "logged in", and in general the way to say no is the way to say yes with a
/// negation in front. Read the other way round, an empty home would come out
/// authenticated — precisely the silence this block exists to break. Measured
/// declared words would already avoid it; this avoids it even when the
/// descriptor's author was distracted.
pub fn judge_login_status(recipe: &LoginRecipe, stdout: &str, stderr: &str) -> LoginVerdict {
    // **BOTH PIPES TOGETHER, AND HERE IT IS NOT A DETAIL**: `codex login status`
    // writes nothing to stdout — the answer is all on stderr, measured. Reading
    // one of them alone would never find either declared shape, and would always
    // say "nobody looked".
    let said = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    let declared = |marks: &[String]| marks.iter().any(|mark| !mark.trim().is_empty());
    if !declared(&recipe.logged_in_when) || !declared(&recipe.logged_out_when) {
        return LoginVerdict::NotDeclared;
    }

    // The pointer picks the subject, nothing more: the words are looked for
    // inside it, by the rule of `unusable_when`. With no pointer the subject is
    // everything the engine said.
    let subject = match recipe.answer.as_ref() {
        None => Some(said.clone()),
        Some(pointer) => read_scalar(&said, pointer),
    };
    // A pointer that finds nothing is not a yes: the wrapper was not the one the
    // descriptor declared, and the answer stays unknown.
    let Some(subject) = subject else {
        return LoginVerdict::Unrecognised { said };
    };

    // **THE SUBJECT IS SHOWN, NOT THE WRAPPER THAT HELD IT.** The real answer
    // of `claude auth status` carries the owner's email address, the id and
    // name of their organisation and the subscription type; this text ends up
    // in `sailor profiles list` and in the `sailor flow check` report, two
    // outputs that get pasted into a delivery and poured into a ledger. **A
    // diagnosis must not drag its user along with it.**
    //
    // Nothing is lost: where a pointer exists, the value it isolated *is* the
    // answer — "false" is more precise than the wrapper, not less — and where
    // there is none, the subject is already everything the engine said. It is
    // the rule of broken lines ("the engine's words in full") applied to an
    // engine that answers with a field instead of a sentence.
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

/// Who asks the local "are you authenticated?", **inside one named home**.
///
/// **WHY A TRAIT OF ITS OWN AND NOT [`DryProbe`].** Two questions over two
/// worlds: the dry trial tries the line in the active profile's home, chosen by
/// nobody; this one is asked **in a named home** — `sailor profiles list` asks
/// it of every profile, not just the one in force, and `DryProbe` has no way to
/// say so. The environment is an argument, not something the executor reads.
pub trait LoginProbe: Send + Sync {
    fn ask(&self, bin: &str, args: &[String], env: &BTreeMap<String, String>) -> DryRun;
}

/// The two local questions an engine can be asked without spending.
///
/// They sit together because whoever checks a flow asks both at the same moment
/// and over the same world; kept apart, every call site would have to carry two
/// arguments that always mean the same thing.
pub trait EngineProbe: DryProbe + LoginProbe {}

impl<T: DryProbe + LoginProbe> EngineProbe for T {}

impl LoginProbe for RealDryProbe {
    fn ask(&self, bin: &str, args: &[String], env: &BTreeMap<String, String>) -> DryRun {
        let result = invoke_external_engine(&EngineInvocation {
            bin: bin.to_owned(),
            args: args.to_vec(),
            env: env.clone(),
            workdir: None,
            // **EMPTY, CLOSED STDIN, THAT IS `< /dev/null`.** An engine that
            // started waiting on stdin would hang the check of all the others:
            // the trap already paid for on `codex exec`, and one character
            // avoids it.
            stdin: Some(Vec::new()),
            timeout: DRY_PROBE_TIMEOUT,
        });
        match result {
            EngineResult::Ok { stdout, stderr }
            | EngineResult::ExitError { stdout, stderr, .. }
            | EngineResult::WaitingForAPerson { stdout, stderr } => {
                DryRun::Answered { stdout, stderr }
            }
            EngineResult::TimedOut => DryRun::NoAnswer {
                why: format!(
                    "no answer within {} seconds",
                    DRY_PROBE_TIMEOUT.as_secs()
                ),
            },
            EngineResult::SpawnFailed { reason } => DryRun::NoAnswer {
                why: format!("the process did not start: {reason}"),
            },
        }
    }
}

/// Asks `bin`, inside the home `env` declares, whether it is authenticated.
///
/// **IT COSTS NOTHING AND CALLS NO PROVIDER.** Measured: `codex login status`
/// and `claude auth status` read a local file and answer. They are the way to
/// know this without going to look at the disk in the engine's place.
pub fn probe_login_status(
    probe: &dyn LoginProbe,
    bin: &str,
    env: &BTreeMap<String, String>,
    recipe: &LoginRecipe,
) -> LoginVerdict {
    // A descriptor that declares nothing starts no process: asking and then
    // being unable to read the answer would be time spent for nothing.
    let declared = |marks: &[String]| marks.iter().any(|mark| !mark.trim().is_empty());
    if !declared(&recipe.logged_in_when) || !declared(&recipe.logged_out_when) {
        return LoginVerdict::NotDeclared;
    }
    match probe.ask(bin, &recipe.args, env) {
        DryRun::Answered { stdout, stderr } => judge_login_status(recipe, &stdout, &stderr),
        DryRun::NoAnswer { why } => LoginVerdict::NoAnswer { why },
    }
}
