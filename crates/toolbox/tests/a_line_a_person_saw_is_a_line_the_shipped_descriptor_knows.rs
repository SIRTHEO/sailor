//! Refusals seen on a real machine, checked against the **shipped** descriptor.
//!
//! An engine declares the words it uses to say it cannot work. A word it says
//! and nobody declared is not a quiet mistake: the chain does not fall back,
//! the step dies as an ordinary error, and the engine keeps being asked.

use actions::{probe_dry_run, DryProbe, DryRun, ProbeVerdict};
use toolbox::descriptor::{Catalog, Source};
use toolbox::probe::Machine;
use toolbox::resolver::Tools;
use actions::ToolResolver as _;

/// Lines engines really printed, with the engine that printed each. Add a row
/// whenever an engine refuses with words nobody wrote down yet.
const SEEN: &[(&str, &str)] = &[
    (
        "claude-code",
        "You've hit your session limit \u{b7} resets 7:50pm (Europe/Rome)",
    ),
    (
        "claude-code",
        "You've hit your weekly limit \u{b7} resets 7am",
    ),
];

/// Only what the product ships, with nothing of this machine around it.
fn shipped_only() -> Tools {
    Tools::new(
        Catalog::load(&[Source::Builtin]),
        Machine {
            path_dirs: Vec::new(),
            home: std::path::PathBuf::from("/home/nobody"),
            env: Default::default(),
            version_probes: false,
        },
    )
}

/// A probe that hands back the line instead of starting anything: the words are
/// the subject here, and no engine is on the machine running this.
struct Said(&'static str);

impl DryProbe for Said {
    fn run(&self, _bin: &str, _args: &[String], _stdin: Option<Vec<u8>>) -> DryRun {
        DryRun::Answered {
            stdout: String::new(),
            stderr: self.0.to_owned(),
        }
    }
}

#[test]
fn every_refusal_seen_on_a_machine_is_declared_by_the_engine_that_said_it() {
    let tools = shipped_only();
    for (engine, line) in SEEN {
        let recipe = tools
            .ask_recipe(engine)
            .unwrap_or_else(|| panic!("«{engine}» declares how a question is put to it"));

        let verdict = probe_dry_run(&Said(line), "/nowhere", &recipe);

        assert!(
            matches!(verdict, ProbeVerdict::CannotWork { .. }),
            "«{engine}» said «{line}» and its descriptor did not know: {verdict:?}"
        );
    }
}

/// Saying "I cannot work" is not saying "my quota ran out", and the two lead to
/// different rows and different waits. A spent quota must reach its own class.
#[test]
fn a_spent_quota_reaches_the_class_of_a_spent_quota() {
    let tools = shipped_only();
    for (engine, line) in SEEN {
        let recipe = tools
            .ask_recipe(engine)
            .unwrap_or_else(|| panic!("«{engine}» declares how a question is put to it"));
        let lowered = line.to_lowercase();

        assert!(
            recipe
                .exhausted_when
                .iter()
                .any(|word| lowered.contains(&word.to_lowercase())),
            "«{engine}» said «{line}», which no word of `exhausted_when` covers"
        );
    }
}
