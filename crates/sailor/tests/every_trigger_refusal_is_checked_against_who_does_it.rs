//! A refusal is a statement about the tree: «nobody listens», «the clock is
//! kept on the flow's schedule». Every shape of trigger source is fired here,
//! and what each refusal claims is looked for in the sources: an absence has
//! to stay absent, a keeper has to be there. A statement that ages with nobody
//! re-reading it lies exactly like a sensor that confuses «zero» with «I did
//! not look». See fault 74.

use flow::{Action, ActionError, ActionOutcome, SharedState};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use trigger::TriggerAction;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

/// One shape of source: its `kind` as a descriptor writes it, a descriptor
/// that loads, and what the step has to add for the shape to carry a signal.
struct Shape {
    kind: &'static str,
    descriptor: &'static str,
    input: &'static str,
}

const SHAPES: &[Shape] = &[
    Shape {
        kind: "manual",
        descriptor: r#"{"id": "by-hand", "kind": "manual"}"#,
        input: r#"{"text": "go"}"#,
    },
    Shape {
        kind: "terminal",
        descriptor: r#"{"id": "a-pane", "kind": "terminal", "listen": {
            "kind": "appended_lines", "files": ["~/pane.jsonl"], "text_pointer": ["text"]}}"#,
        input: "{}",
    },
    Shape {
        kind: "periodic",
        descriptor: r#"{"id": "every-hour", "kind": "periodic", "periodic": {
            "every": {"kind": "every_seconds", "seconds": 3600},
            "missed_run": "once_for_all_of_them", "at_most_at_once": 1}}"#,
        input: "{}",
    },
];

/// What a refusal class states about the tree, and where the tree would show
/// the statement to have aged.
struct Claim {
    class: &'static str,
    /// The thing the refusal is about; its sentence in the catalogue and the
    /// refusal itself both have to name it.
    names: &'static [&'static str],
    /// Marks of somebody doing what the class says nobody does, looked for in
    /// the code that ships outside the trigger crate. None may be there.
    signs_somebody_does_it: &'static [&'static str],
    /// The keeper the class points at instead: a file, and the line in it that
    /// does the keeping. Every one has to be there.
    who_does_it_instead: &'static [(&'static str, &'static str)],
}

const CLAIMS: &[Claim] = &[
    Claim {
        class: "listening_not_built",
        names: &["listen"],
        signs_somebody_does_it: &[
            "Listen::AppendedLines",
            "Listen::CursorCommand",
            "trigger::Listen",
        ],
        who_does_it_instead: &[],
    },
    Claim {
        class: "periodic_source_not_read",
        names: &["schedule"],
        signs_somebody_does_it: &[".periodic", "MissedRun", "Kind::Periodic"],
        who_does_it_instead: &[
            (
                "crates/sailor/src/flow_cmd/beat.rs",
                "flow::is_due(schedule",
            ),
            ("desktop/src-tauri/src/beat.rs", "flow::is_due(schedule"),
        ],
    },
];

/// Tests run side by side and fire the same shapes: a fixture named after the
/// shape alone is swept by one while the other reads it.
static FIRED: AtomicUsize = AtomicUsize::new(0);

fn fire(shape: &Shape) -> Result<Value, ActionError> {
    let dir = std::env::temp_dir().join(format!(
        "sailor-refusals-{}-{}-{}",
        std::process::id(),
        FIRED.fetch_add(1, Ordering::Relaxed),
        shape.kind
    ));
    std::fs::create_dir_all(&dir).expect("digging the fixture directory");
    let file = dir.join("triggers.json");
    std::fs::write(&file, format!("[{}]", shape.descriptor))
        .expect("writing the fixture descriptor");
    let mut input: Value = serde_json::from_str(shape.input).expect("the step input is JSON");
    let descriptor: Value =
        serde_json::from_str(shape.descriptor).expect("the fixture descriptor is JSON");
    input["source"] = descriptor["id"].clone();
    input["descriptor_paths"] = json!([file.to_string_lossy()]);
    input["include_defaults"] = json!(false);
    let outcome = TriggerAction.execute(&input, &SharedState::new());
    let _ = std::fs::remove_dir_all(&dir);
    match outcome? {
        ActionOutcome::Went(signal) => Ok(signal),
        ActionOutcome::Waiting(why) | ActionOutcome::NotYet(why) => {
            panic!("a trigger answers or refuses, it does not postpone: {why}")
        }
    }
}

/// The shapes that stop, each with the class it stopped with.
fn refusals() -> Vec<(&'static str, ActionError)> {
    SHAPES
        .iter()
        .filter_map(|shape| fire(shape).err().map(|error| (shape.kind, error)))
        .collect()
}

/// The shapes `Kind` declares, read off its source: a shape added there and
/// not to `SHAPES` would be a refusal nobody asked about.
fn kinds_declared(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("crates/trigger/src/descriptor.rs"))
        .expect("the descriptor source is where the shapes are declared");
    let body = text
        .split_once("pub enum Kind {")
        .and_then(|(_, rest)| rest.split_once('}'))
        .map(|(body, _)| body)
        .expect("`Kind` is declared in the descriptor source");
    let mut found: Vec<String> = body
        .lines()
        .map(str::trim)
        .filter(|line| line.ends_with(',') && line.starts_with(|c: char| c.is_ascii_uppercase()))
        .map(|line| snake_case(line.trim_end_matches(',')))
        .collect();
    found.sort();
    found
}

fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for (at, letter) in name.chars().enumerate() {
        if letter.is_ascii_uppercase() && at > 0 {
            out.push('_');
        }
        out.push(letter.to_ascii_lowercase());
    }
    out
}

/// The code that ships outside the trigger crate, cut at its unit tests and
/// stripped of its comments: where somebody doing what a refusal says nobody
/// does would have to be, and where prose about it is not doing it.
fn shipped_code_outside_trigger(root: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    let crates = std::fs::read_dir(root.join("crates")).expect("the crates directory is there");
    for entry in crates.flatten() {
        if entry.file_name().to_string_lossy() != "trigger" {
            walk(&entry.path().join("src"), &mut files);
        }
    }
    walk(&root.join("desktop/src-tauri/src"), &mut files);
    files
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let shipped = text
                .split("#[cfg(test)]")
                .next()
                .unwrap_or_default()
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            Some((path, shipped))
        })
        .collect()
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
}

/// The absurd control first: the shapes fired here are the shapes declared,
/// so a fourth one born in `Kind` is judged from the day it is born.
#[test]
fn every_shape_kind_declares_is_fired_here() {
    let declared = kinds_declared(&root());
    let mut fired: Vec<String> = SHAPES.iter().map(|shape| shape.kind.to_owned()).collect();
    fired.sort();
    assert!(
        declared.len() >= 3,
        "the reader of `Kind` found {declared:?}: it stopped reading"
    );
    assert_eq!(
        declared, fired,
        "a shape `Kind` declares is not fired here: add it to SHAPES, with a claim for its refusal"
    );
}

/// A refusal without a claim is a statement nobody will re-read; a claim
/// nothing refuses with any more has already aged.
#[test]
fn every_refusal_carries_a_claim_and_every_claim_is_still_refused_with() {
    let refused = refusals();
    assert!(
        !refused.is_empty(),
        "no shape refused: the shapes that stop have to be fired here"
    );
    for (kind, error) in &refused {
        assert!(
            CLAIMS.iter().any(|claim| claim.class == error.class),
            "the {kind} shape refuses with «{}», and no claim says what that class states \
             about the tree: write one",
            error.class
        );
    }
    for claim in CLAIMS {
        assert!(
            refused.iter().any(|(_, error)| error.class == claim.class),
            "no shape refuses with «{}» any more: the claim is stale, take it out",
            claim.class
        );
    }
}

/// «Nobody does it» is checked against the code, not believed: the marks of
/// somebody doing it are looked for in everything that ships outside the
/// trigger crate, and finding one means the refusal has aged.
#[test]
fn a_refusal_that_says_nobody_does_it_is_right_about_the_tree() {
    let shipped = shipped_code_outside_trigger(&root());
    assert!(
        shipped.len() > 50,
        "the walk opened {} files: it is not looking at the tree",
        shipped.len()
    );
    assert!(
        shipped
            .iter()
            .any(|(_, text)| text.contains("flow::is_due(schedule")),
        "the walk cannot see the beat, so its silence would mean nothing"
    );
    let mut aged = Vec::new();
    for claim in CLAIMS {
        for sign in claim.signs_somebody_does_it {
            for (path, text) in &shipped {
                if text.contains(sign) {
                    aged.push(format!(
                        "«{}» says nobody does it, and {} carries `{sign}`",
                        claim.class,
                        path.display()
                    ));
                }
            }
        }
    }
    assert!(
        aged.is_empty(),
        "somebody now does what a refusal says nobody does; reword the refusal, its \
         sentence and its claim:\n{}",
        aged.join("\n")
    );
}

/// The keeper a refusal points at has to be there doing the keeping, and the
/// sentence a person reads has to name it, in both languages and in the
/// refusal itself.
#[test]
fn a_refusal_that_names_a_keeper_finds_the_keeper_in_the_tree() {
    let root = root();
    let refused = refusals();
    for claim in CLAIMS {
        for (file, keeping) in claim.who_does_it_instead {
            let text = std::fs::read_to_string(root.join(file)).unwrap_or_else(|error| {
                panic!(
                    "«{}» points at {file}, which cannot be read: {error}",
                    claim.class
                )
            });
            assert!(
                text.contains(keeping),
                "«{}» points at {file} as the keeper, and it no longer holds `{keeping}`",
                claim.class
            );
        }
        let key = format!("run.failure.{}", claim.class);
        let english = catalogue::look("en", &key, &[])
            .unwrap_or_else(|| panic!("no english sentence answers for {key}"));
        assert!(
            catalogue::entries("it").is_some_and(|italian| italian.contains_key(&key)),
            "no italian sentence answers for {key}"
        );
        let (_, error) = refused
            .iter()
            .find(|(_, error)| error.class == claim.class)
            .unwrap_or_else(|| {
                let seen: Vec<&str> = refused
                    .iter()
                    .map(|(_, error)| error.class.as_str())
                    .collect();
                panic!(
                    "nothing refuses with «{}»; the refusals seen were {seen:?}",
                    claim.class
                )
            });
        for name in claim.names {
            assert!(
                english.contains(name),
                "the sentence for «{}» does not name `{name}`: {english}",
                claim.class
            );
            assert!(
                error.said.contains(name),
                "the refusal «{}» does not name `{name}`: {}",
                claim.class,
                error.said
            );
        }
    }
}

/// The row that asked for this file: in a tree where a beat starts what is
/// due, a periodic trigger may be refused, but never as if nobody kept the time.
#[test]
fn a_periodic_trigger_in_a_tree_with_a_beat_is_not_refused_as_if_nobody_kept_the_time() {
    let root = root();
    for beat in [
        "crates/sailor/src/flow_cmd/beat.rs",
        "desktop/src-tauri/src/beat.rs",
    ] {
        let text = std::fs::read_to_string(root.join(beat)).expect("the beat is there");
        assert!(
            text.contains("flow::is_due(schedule"),
            "{beat} no longer judges a schedule: the beat is gone, and every claim about it \
             has to be re-read"
        );
    }
    let periodic = SHAPES
        .iter()
        .find(|shape| shape.kind == "periodic")
        .expect("the periodic shape is fired here");
    let error = fire(periodic).expect_err("nothing reads the source's own recurrence yet");
    assert_ne!(
        error.class, "nobody_keeps_the_time",
        "the refusal denies a clock that two beats keep"
    );
    assert_eq!(error.class, "periodic_source_not_read");
    assert!(
        error.said.contains("`schedule`") && error.said.contains("flow tick"),
        "{}",
        error.said
    );
}
