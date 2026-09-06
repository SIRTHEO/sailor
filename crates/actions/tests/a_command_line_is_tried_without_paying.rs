//! The dry probe of a command line: it is assembled, run **without handing over
//! the question**, and judged by what the engine says.
//!
//! **WHY IT EXISTS.** Fault 1 and fault 27 are one defect twice in the same
//! week: a line assembled from pieces each right apart, wrong together, and
//! never run before landing in a flow that costs money. The cure written beside
//! fault 1 — «really run every command line before it lands in a flow» — stayed
//! unimplemented because running it looked like spending. It is not: with no
//! question no provider is called, and the argument parsing is the same.
//!
//! **WHY THE ENGINES HERE ARE FAKE.** A test starting the real `claude`, `codex`
//! and `agy` depends on what the runner has installed and on how its quota
//! stands that day: it could not come out differently for the reason it claims.
//! Here the four cases are built, and the texts the fakes print are the real
//! ones, measured on this machine.

use actions::{
    judge_dry_run, probe_dry_run, AskRecipe, DryProbe, DryRun, ProbeVerdict, PromptVia,
    RealDryProbe,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("actions-{name}-{}-{n}", std::process::id()));
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

/// A fake executable printing what we tell it to print, with the exit code we
/// tell it to have.
fn fake_binary(dir: &Path, name: &str, script: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("writing the fake executable");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("bit di esecuzione");
    path
}

/// `agy`'s recipe as shipped, the one fault 27 was lived on: the question goes
/// last, and `--print` must stay attached to it.
fn agy_recipe(refuses: &[&str]) -> AskRecipe {
    AskRecipe {
        args: vec!["--mode".to_owned(), "plan".to_owned()],
        prompt: PromptVia::LastArg,
        args_before_prompt: vec!["--print".to_owned()],
        unusable_when: Vec::new(),
        silent_without_prompt: false,
        refuses_without_prompt: refuses.iter().map(|s| (*s).to_owned()).collect(),
        exhausted_when: Vec::new(),
        cooldown_secs: None,
        waits_for_a_person_when: Vec::new(),
        usage: None,
    }
}

// ── the four fake engines, four verdicts ────────────────────────────────

/// **THE SOUND CASE.** The engine says «the question was all that was missing»:
/// the line is well assembled, and nobody paid a thing to learn it.
#[test]
fn an_engine_that_only_misses_the_prompt_is_declared_sound() {
    let sandbox = Sandbox::new("sound");
    let bin = fake_binary(
        &sandbox.root,
        "agy-sound",
        "echo 'flag needs an argument: -print' >&2\nexit 2",
    );
    let verdict = probe_dry_run(
        &RealDryProbe,
        &bin.to_string_lossy(),
        &agy_recipe(&["flag needs an argument: -print"]),
    );
    assert_eq!(verdict, ProbeVerdict::Sound, "{verdict:?}");
}

/// **FAULT 27'S CASE.** The same engine, the same exit code, and a line
/// complaining about something else entirely: the text is the diagnosis, and it
/// reaches the reader word for word.
#[test]
fn an_engine_that_complains_about_something_else_is_declared_broken_with_its_own_words() {
    let sandbox = Sandbox::new("broken");
    let bin = fake_binary(
        &sandbox.root,
        "agy-broken",
        "echo 'Error: --print took \"--output-format\" as its prompt, so the intended prompt was left as an argument and ignored.' >&2\nexit 2",
    );
    let verdict = probe_dry_run(
        &RealDryProbe,
        &bin.to_string_lossy(),
        &agy_recipe(&["flag needs an argument: -print"]),
    );
    match verdict {
        ProbeVerdict::Broken { said } => {
            // THE ENGINE'S WORDS ARE THE PRODUCT. On fault 27 the sentence said
            // which flag had eaten which argument: no classification of ours
            // could have said as much, and a «broken» without them sends the
            // reader off guessing.
            assert!(said.contains("--print took"), "{said}");
            assert!(said.contains("--output-format"), "{said}");
        }
        other => panic!("it had to be broken, and it is {other:?}"),
    }
}

/// **AN EXHAUSTED ENGINE IS NOT A BROKEN ENGINE**, and the two resemble each
/// other in the wrong reading order alone: `claude`'s line is sound, it is the
/// quota that ran out.
#[test]
fn an_exhausted_engine_is_not_called_broken() {
    let sandbox = Sandbox::new("spent");
    let bin = fake_binary(
        &sandbox.root,
        "claude-spent",
        "echo \"You've hit your weekly limit · resets 7am\" >&2\nexit 1",
    );
    let recipe = AskRecipe {
        args: vec!["-p".to_owned()],
        prompt: PromptVia::Stdin,
        args_before_prompt: Vec::new(),
        unusable_when: vec!["weekly limit".to_owned()],
        silent_without_prompt: false,
        refuses_without_prompt: vec!["input must be provided either through stdin".to_owned()],
        exhausted_when: Vec::new(),
        cooldown_secs: None,
        waits_for_a_person_when: Vec::new(),
        usage: None,
    };
    let verdict = probe_dry_run(&RealDryProbe, &bin.to_string_lossy(), &recipe);
    match verdict {
        ProbeVerdict::CannotWork { said } => assert!(said.contains("weekly limit"), "{said}"),
        other => panic!("it had to be «cannot work right now», and it is {other:?}"),
    }
}

/// **SILENCE IS NOT SOUNDNESS.** A descriptor with no `refuses_without_prompt`
/// does not say the line is fine: it says nobody looked at it. Calling that
/// sound would be the quietest way of giving up checking — the same distinction
/// the `capabilities` block keeps between «it lacks it» and «nobody looked».
#[test]
fn an_engine_whose_descriptor_says_nothing_is_not_declared_sound() {
    let sandbox = Sandbox::new("unspoken");
    let bin = fake_binary(
        &sandbox.root,
        "agy-mute",
        "echo 'flag needs an argument: -print' >&2\nexit 2",
    );
    let verdict = probe_dry_run(&RealDryProbe, &bin.to_string_lossy(), &agy_recipe(&[]));
    assert_eq!(
        verdict,
        ProbeVerdict::NotDeclared,
        "the engine did refuse properly, but nobody had declared how it says so"
    );
}

// ── the line that really starts ─────────────────────────────────────────

type Seen = (String, Vec<String>, Option<Vec<u8>>);

/// Watches what was run, without running anything.
#[derive(Default)]
struct RecordingProbe {
    seen: Mutex<Vec<Seen>>,
}

impl DryProbe for RecordingProbe {
    fn run(&self, bin: &str, args: &[String], stdin: Option<Vec<u8>>) -> DryRun {
        self.seen
            .lock()
            .expect("the fake probe's record")
            .push((bin.to_owned(), args.to_vec(), stdin));
        DryRun::Answered {
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

/// **THE LINE TRIED IS THE REAL LINE, LESS THE QUESTION.** Were the probe to
/// assemble a line of its own, it would try something no run will ever execute —
/// a green on an object other than the one being paid for.
#[test]
fn the_line_that_is_tried_is_the_real_one_without_the_prompt() {
    let probe = RecordingProbe::default();
    let _ = probe_dry_run(&probe, "agy", &agy_recipe(&["flag needs an argument"]));

    let seen = probe.seen.lock().expect("the record");
    let (bin, args, stdin) = &seen[0];
    assert_eq!(bin, "agy");
    assert_eq!(args, &["--mode", "plan", "--print"]);
    // An engine wanting the question last gets no stdin at all: an empty one
    // would be harmless, but an open one would make it wait — and the dry probe
    // would become a way of hanging the check, on a machine with no `timeout`.
    assert!(
        stdin.is_none(),
        "the question went at the tail, not on the input"
    );
}

/// And an engine wanting it on stdin gets an **empty, closed** stdin, which is
/// what `< /dev/null` does — the one shape in which `claude` and `codex` answer
/// rather than wait.
#[test]
fn an_engine_that_reads_the_prompt_from_stdin_gets_an_empty_closed_one() {
    let probe = RecordingProbe::default();
    let recipe = AskRecipe {
        args: vec!["-p".to_owned()],
        prompt: PromptVia::Stdin,
        args_before_prompt: Vec::new(),
        unusable_when: Vec::new(),
        silent_without_prompt: false,
        refuses_without_prompt: vec!["input must be provided".to_owned()],
        exhausted_when: Vec::new(),
        cooldown_secs: None,
        waits_for_a_person_when: Vec::new(),
        usage: None,
    };
    let _ = probe_dry_run(&probe, "claude", &recipe);

    let seen = probe.seen.lock().expect("the record");
    assert_eq!(seen[0].2, Some(Vec::new()));
}

/// An engine that does not answer within the cap is neither sound nor broken:
/// it is unknown, and the reason travels with the verdict, because a process
/// that fails to start and one that fails to answer are repaired differently.
#[test]
fn an_engine_that_never_answers_is_neither_sound_nor_broken() {
    let verdict = probe_dry_run(
        &RealDryProbe,
        "/this/path/does/not/exist/anywhere",
        &agy_recipe(&["flag needs an argument"]),
    );
    match verdict {
        ProbeVerdict::TimedOut { why } => assert!(!why.is_empty(), "the reason is not empty"),
        other => panic!("it had to be «no answer», and it is {other:?}"),
    }
}

// ── the reading order, which is the part people get wrong ───────────────

/// **`unusable_when` IS READ FIRST.** An exhausted engine complains about its
/// quota, not about the line; read the other way round it would be declared
/// broken, and the reader would go and correct a healthy descriptor when waiting
/// was enough.
///
/// The output here holds **both** things: the one case in which the order can be
/// observed, and so the one that turns red on an inversion.
#[test]
fn the_exhausted_reading_comes_first_when_the_output_says_both() {
    let recipe = AskRecipe {
        args: vec!["-p".to_owned()],
        prompt: PromptVia::Stdin,
        args_before_prompt: Vec::new(),
        unusable_when: vec!["weekly limit".to_owned()],
        silent_without_prompt: false,
        refuses_without_prompt: vec!["input must be provided".to_owned()],
        exhausted_when: Vec::new(),
        cooldown_secs: None,
        waits_for_a_person_when: Vec::new(),
        usage: None,
    };
    let verdict = judge_dry_run(
        &recipe,
        "",
        "Input must be provided either through stdin\nYou've hit your weekly limit",
    );
    match verdict {
        ProbeVerdict::CannotWork { .. } => {}
        other => {
            panic!("the reading order is inverted: a spent engine became {other:?}")
        }
    }
}

/// The comparison ignores case, because no provider promises not to change it.
#[test]
fn the_words_are_matched_whatever_case_the_engine_shouts_them_in() {
    let verdict = judge_dry_run(
        &agy_recipe(&["flag needs an argument: -print"]),
        "",
        "FLAG NEEDS AN ARGUMENT: -PRINT",
    );
    assert_eq!(verdict, ProbeVerdict::Sound);
}

/// An empty fragment makes nothing sound: it would match any output, and a
/// badly written descriptor would turn every broken line into a sound one —
/// the worst defect this check could possibly have.
#[test]
fn an_empty_fragment_declares_nothing() {
    let verdict = judge_dry_run(&agy_recipe(&["", "   "]), "", "an error like any other");
    assert_eq!(verdict, ProbeVerdict::NotDeclared);
}

/// **THE VERDICT DOES NOT LOOK AT THE EXIT CODE, AND CANNOT.** `judge_dry_run`
/// never even receives it: both measured `agy` cases exit **2** — the healthy
/// refusal and fault 27's malformed line — so a probe judging by outcome would
/// see them as identical. The two real texts sit side by side here, and this is
/// the one test that compares them.
#[test]
fn two_failures_with_the_same_exit_code_get_two_different_verdicts() {
    let recipe = agy_recipe(&["flag needs an argument: -print"]);
    let sound = judge_dry_run(&recipe, "", "flag needs an argument: -print");
    let broken = judge_dry_run(
        &recipe,
        "",
        "Error: --print took \"--output-format\" as its prompt, so the intended \
         prompt was left as an argument and ignored.",
    );
    assert_eq!(sound, ProbeVerdict::Sound);
    assert!(
        matches!(broken, ProbeVerdict::Broken { .. }),
        "the same exit code, a different verdict: {broken:?}"
    );
    assert_ne!(sound, broken);
}

/// An engine exiting **zero** with no question is not sound for it: `agy
/// --mode nonsense-value --not-a-real-flag --help` exits 0 on this machine and
/// prints the help. That is why `--help` cannot try a line, and why this verdict
/// looks at the text and nothing else.
#[test]
fn an_engine_that_exits_zero_with_a_help_screen_is_not_sound() {
    let sandbox = Sandbox::new("help-screen");
    let bin = fake_binary(&sandbox.root, "agy-help", "echo 'Usage of agy:'\nexit 0");
    let verdict = probe_dry_run(
        &RealDryProbe,
        &bin.to_string_lossy(),
        &agy_recipe(&["flag needs an argument: -print"]),
    );
    match verdict {
        ProbeVerdict::Broken { said } => assert!(said.contains("Usage of agy"), "{said}"),
        other => panic!("exiting zero with no question is no sound line: {other:?}"),
    }
}

/// An engine measured to answer nothing without a question is sound when its
/// stdout is empty, whatever spinner it drew on stderr; text on stdout, a
/// help screen, is still a broken line. Without the declaration the same
/// silence is «nobody looked».
#[test]
fn an_engine_declared_silent_is_sound_on_an_empty_stdout_and_nothing_else() {
    let mut silent = agy_recipe(&[]);
    silent.silent_without_prompt = true;
    assert!(matches!(judge_dry_run(&silent, "", "⠙ ⠹ ⠸"), ProbeVerdict::Sound));
    assert!(matches!(judge_dry_run(&silent, "Usage of agy:", ""), ProbeVerdict::Broken { .. }));
    // The control: the same silence, undeclared, proves nothing.
    assert!(matches!(judge_dry_run(&agy_recipe(&[]), "", ""), ProbeVerdict::NotDeclared));
}
