//! An engine that only waits for a person is not waited for: the words it
//! prints before the wait are declared in its descriptor, and the step stops
//! it on them. The fake here prints the two lines a real command line printed
//! when started without credentials — before waiting sixty seconds for a code
//! nobody will type, at every step of every chain — and then sleeps; what is
//! measured is that the step is done long before the sleep is.

use actions::{AskRecipe, ExternalEngineAction, PromptVia, ToolResolver};
use flow::{Action, ActionOutcome, SharedState};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!("actions-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the scratch directory is created");
        Scratch(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fake_engine(dir: &Path, name: &str, script: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("the fake engine is written");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("and made executable");
    path.to_string_lossy().into_owned()
}

/// The two lines measured on the real engine, then the wait it imposes.
fn engine_that_waits_for_a_person(dir: &Path) -> String {
    fake_engine(
        dir,
        "aspetta",
        "echo 'Authentication required. Please visit the URL to log in:' 1>&2\n\
         echo 'Waiting for authentication (timeout 60s)...' 1>&2\n\
         sleep 20\n\
         exit 1",
    )
}

/// Two engines: one that waits for a person, one that answers.
struct TwoEngines {
    waits: String,
    answers: String,
}

impl ToolResolver for TwoEngines {
    fn resolve(&self, id: &str) -> Result<String, String> {
        match id {
            "aspetta" => Ok(self.waits.clone()),
            "risponde" => Ok(self.answers.clone()),
            other => Err(format!("«{other}» is not on this machine")),
        }
    }

    fn ask_recipe(&self, id: &str) -> Option<AskRecipe> {
        let recipe = |waits_for_a_person_when: Vec<&str>| AskRecipe {
            args: Vec::new(),
            prompt: PromptVia::LastArg,
            args_before_prompt: Vec::new(),
            unusable_when: vec![
                "Authentication required. Please visit the URL to log in".to_owned(),
                "authentication timed out".to_owned(),
            ],
            exhausted_when: Vec::new(),
            cooldown_secs: None,
            waits_for_a_person_when: waits_for_a_person_when
                .into_iter()
                .map(str::to_owned)
                .collect(),
            silent_without_prompt: false,
            refuses_without_prompt: Vec::new(),
            usage: None,
        };
        match id {
            "aspetta" => Some(recipe(vec!["Waiting for authentication"])),
            "risponde" => Some(recipe(Vec::new())),
            _ => None,
        }
    }
}

fn action_over(scratch: &Scratch) -> ExternalEngineAction {
    ExternalEngineAction::resolving_with(TwoEngines {
        waits: engine_that_waits_for_a_person(&scratch.0),
        answers: fake_engine(&scratch.0, "risponde", "echo answered-by-the-second"),
    })
    .cooling_down_in(None)
    .budgeted_by(None)
}

/// A short wait is the whole claim: the mutant that never stops the engine
/// still moves on, twenty seconds later, and this bound turns it red.
const AT_ONCE: Duration = Duration::from_secs(10);

/// **THE CHAIN MOVES ON THE MOMENT THE FIRST ENGINE STARTS WAITING.** The
/// second one answers, and the step closes on its answer well inside the
/// twenty seconds the first would have made everybody wait.
#[test]
fn a_chain_moves_on_the_moment_an_engine_starts_waiting_for_a_person() {
    let scratch = Scratch::new("waits-in-a-chain");
    let action = action_over(&scratch);
    let input = json!({
        "tool": ["aspetta", "risponde"],
        "stdin": "the question",
        "timeout_secs": 60
    });

    let started = Instant::now();
    let outcome = action
        .execute(&input, &SharedState::new())
        .expect("the second engine answers");
    let took = started.elapsed();

    let ActionOutcome::Went(output) = outcome else {
        panic!("an engine that answers is always Went")
    };
    assert!(
        output["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("answered-by-the-second"),
        "{output}"
    );
    assert!(took < AT_ONCE, "the step paid the first engine's wait: {took:?}");
}

/// **ALONE, THE STEP CLOSES AT ONCE AND WITH THE ENGINE'S OWN WORDS.** The
/// class is the one a declared refusal gets, never a plain exit error: the
/// words that stopped it are the descriptor's, and they say why.
#[test]
fn alone_an_engine_waiting_for_a_person_closes_the_step_at_once_with_its_words() {
    let scratch = Scratch::new("waits-alone");
    let action = action_over(&scratch);
    let input = json!({"tool": ["aspetta"], "stdin": "the question", "timeout_secs": 60});

    let started = Instant::now();
    let error = action
        .execute(&input, &SharedState::new())
        .expect_err("an engine waiting for a person has not answered");
    let took = started.elapsed();

    assert_eq!(error.class, "engine_exhausted", "{}", error.said);
    assert!(
        error.said.contains("Waiting for authentication"),
        "the reason carries the words it was stopped on: {}",
        error.said
    );
    assert!(took < AT_ONCE, "the step paid the wait: {took:?}");
}
