//! The mandate is a contract, not a paragraph.
//!
//! Refused at deposit while its author is still alive; taken once; and when
//! the tree has moved under it, handed on with the difference beside it
//! instead of a refusal nobody can act on.

use actions::mandate::{MANDATE_DEPOSIT_ACTION, MANDATE_RESUME_ACTION, MANDATE_WAITING_ACTION};
use flow::{ActionOutcome, SharedState};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

fn registry() -> flow::ActionRegistry {
    let mut registry = flow::ActionRegistry::default();
    actions::mandate::register_mandate(&mut registry);
    registry
}

fn run(action: &str, input: Value) -> Result<ActionOutcome, flow::ActionError> {
    let registry = registry();
    registry
        .get(action)
        .expect("the action is registered")
        .execute(&input, &SharedState::new())
}

fn went(action: &str, input: Value) -> Value {
    match run(action, input).expect("the step does not break") {
        ActionOutcome::Went(value) => value,
        other => panic!("it did not go: {other:?}"),
    }
}

/// A tree and a store of this test's own, taken down with it.
struct Scratch {
    tree: PathBuf,
    store: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "sailor-mandate-contract-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let tree = root.join("tree");
        let store = root.join("store");
        std::fs::create_dir_all(&tree).expect("a tree to work in");
        std::fs::create_dir_all(&store).expect("a store to write in");
        git(&tree, &["init", "--quiet"]);
        git(&tree, &["config", "user.email", "a@test"]);
        git(&tree, &["config", "user.name", "A Test"]);
        let scratch = Scratch { tree, store };
        scratch.commit("one.txt", "first");
        scratch
    }

    fn commit(&self, name: &str, text: &str) {
        std::fs::write(self.tree.join(name), text).expect("a file to commit");
        git(&self.tree, &["add", name]);
        git(&self.tree, &["commit", "--quiet", "-m", name]);
    }

    fn store(&self) -> String {
        self.store.to_string_lossy().into_owned()
    }

    fn tree(&self) -> String {
        self.tree.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(root) = self.tree.parent() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

fn git(tree: &Path, args: &[&str]) {
    let done = Command::new("git")
        .arg("-C")
        .arg(tree)
        .args(args)
        .output()
        .expect("git runs");
    assert!(done.status.success(), "git {args:?}: {done:?}");
}

fn a_constraint() -> Value {
    json!({
        "holds": "nothing is pushed",
        "prerequisite": "the whole suite is green",
        "authority": "the person, in this session",
        "fallback": "leave the commits local and say so",
        "consequence": "a trunk nobody can release from",
    })
}

fn work() -> Value {
    json!({
        "goal": "prove the round before reporting it",
        "asked": "procedi pure in ordine in autonomia",
        "state": [{"said": "the sensor is in service", "verified": true}],
        "decisions": [{"decided": "thresholds in tokens", "authorised_by": "the measurement"}],
        "constraints": [a_constraint()],
        "failed": ["a phrase in the transcript as the resume point"],
        "next": "write the event source",
        "questions": [],
        "never": ["typing into a terminal Sailor does not hold"],
    })
}

fn deposit(scratch: &Scratch, work: Value) -> Result<ActionOutcome, flow::ActionError> {
    run(
        MANDATE_DEPOSIT_ACTION,
        json!({
            "tree": scratch.tree(),
            "tty": "ttys001",
            "session": "the-predecessor",
            "engine": "a-command-line",
            "tokens": 260_000,
            "reread": ["crates/actions/src/mandate.rs"],
            "work": work,
            "store": scratch.store(),
        }),
    )
}

fn resume(scratch: &Scratch, session: &str) -> Result<ActionOutcome, flow::ActionError> {
    run(
        MANDATE_RESUME_ACTION,
        json!({
            "tree": scratch.tree(),
            "tty": "ttys001",
            "session": session,
            "store": scratch.store(),
        }),
    )
}

/// **THE MECHANICAL HALF IS READ, NEVER ASKED FOR.** A session's claim about
/// the commit it worked on is exactly the claim a successor must not have to
/// take on trust, so the deposit reads the tree itself.
#[test]
fn the_deposit_reads_the_tree_instead_of_believing_the_session() {
    let scratch = Scratch::new("mechanical");

    let answer = match deposit(&scratch, work()).expect("the deposit goes") {
        ActionOutcome::Went(value) => value,
        other => panic!("it did not go: {other:?}"),
    };

    assert_eq!(answer["head"].as_str().map(str::len), Some(40), "{answer}");
    assert!(!answer["branch"].as_str().unwrap_or_default().is_empty());
    assert_eq!(
        answer["archived"],
        Value::Null,
        "the first one archives none"
    );
}

/// **A GAP IS FOUND WHERE IT CAN STILL BE FILLED.** Discovered at resume, the
/// author is gone and nobody can answer for it.
#[test]
fn a_constraint_missing_a_field_is_refused_at_deposit() {
    let scratch = Scratch::new("incomplete");
    let mut short = a_constraint();
    short.as_object_mut().expect("an object").remove("fallback");
    let mut work = work();
    work["constraints"] = json!([short]);

    let refusal = deposit(&scratch, work).expect_err("a mandate with a hole is refused");

    assert_eq!(refusal.class, "mandate_incomplete", "{refusal:?}");
}

/// And a field written blank is the same gap with a value in it.
#[test]
fn a_constraint_whose_field_is_blank_is_refused_too() {
    let scratch = Scratch::new("blank");
    let mut blank = a_constraint();
    blank["fallback"] = json!("   ");
    let mut work = work();
    work["constraints"] = json!([blank]);

    let refusal = deposit(&scratch, work).expect_err("a blank field is a hole");

    assert_eq!(refusal.class, "mandate_incomplete", "{refusal:?}");
    assert!(
        refusal.said.contains("work.constraints[0].fallback"),
        "and it says which one: {refusal:?}"
    );
}

/// **STALE IS NOT A REFUSAL.** The work is still the work; what moved under it
/// is a fact the successor can read, and reading it is cheaper than starting
/// again.
#[test]
fn a_tree_that_moved_hands_the_mandate_on_with_the_difference_beside_it() {
    let scratch = Scratch::new("stale");
    deposit(&scratch, work()).expect("the deposit goes");
    scratch.commit("two.txt", "landed after the mandate");

    let answer = went(
        MANDATE_RESUME_ACTION,
        json!({
            "tree": scratch.tree(),
            "tty": "ttys001",
            "session": "the-successor",
            "store": scratch.store(),
        }),
    );

    assert_eq!(answer["stale"], json!(true), "{answer}");
    assert_eq!(answer["head_moved"], json!(true));
    assert!(
        answer["since"]
            .as_str()
            .unwrap_or_default()
            .contains("two.txt"),
        "the difference travels with it: {answer}"
    );
    assert_eq!(answer["work"]["next"], json!("write the event source"));
}

/// A tree that did not move hands on a mandate that is not stale.
#[test]
fn a_tree_that_stood_still_hands_on_a_fresh_mandate() {
    let scratch = Scratch::new("fresh");
    deposit(&scratch, work()).expect("the deposit goes");

    let answer = went(
        MANDATE_RESUME_ACTION,
        json!({
            "tree": scratch.tree(),
            "tty": "ttys001",
            "session": "the-successor",
            "store": scratch.store(),
        }),
    );

    assert_eq!(answer["stale"], json!(false), "{answer}");
    assert_eq!(answer["written"]["tokens"], json!(260_000));
    assert_eq!(
        answer["written"]["reread"],
        json!(["crates/actions/src/mandate.rs"]),
        "the list of files to read again is what actually travels"
    );
}

/// **ONE MANDATE, ONE SUCCESSOR.** Taken twice it is the same work done twice,
/// by two sessions neither of which knows about the other.
#[test]
fn a_mandate_is_taken_once_and_says_who_took_it() {
    let scratch = Scratch::new("once");
    deposit(&scratch, work()).expect("the deposit goes");
    resume(&scratch, "the-successor").expect("the first taking goes");

    let refusal = resume(&scratch, "another-successor").expect_err("the second is refused");

    assert_eq!(refusal.class, "mandate_already_taken", "{refusal:?}");
    assert!(
        refusal.said.contains("the-successor"),
        "and it names who holds it: {refusal:?}"
    );
}

/// **NOT YET IS NOT BROKEN.** Between the ask and the writing the agent is
/// still typing, and a red step there parks the relay for good.
#[test]
fn a_resume_with_no_mandate_deposited_is_not_yet() {
    let scratch = Scratch::new("empty");

    let outcome = resume(&scratch, "the-successor").expect("it does not break");

    assert!(
        matches!(outcome, ActionOutcome::NotYet(_)),
        "{outcome:?}: nothing deposited is not a failure"
    );
}

/// A second deposit does not write over the first: what a predecessor believed
/// is the only place a bad handover can be read back from.
#[test]
fn a_second_deposit_moves_the_first_aside() {
    let scratch = Scratch::new("archive");
    deposit(&scratch, work()).expect("the first deposit goes");

    let answer = match deposit(&scratch, work()).expect("the second deposit goes") {
        ActionOutcome::Went(value) => value,
        other => panic!("it did not go: {other:?}"),
    };

    let aside = answer["archived"].as_str().expect("the first was kept");
    assert!(Path::new(aside).is_file(), "{answer}");
}

/// The four fields of a constraint are the contract: a schema that dropped
/// them would take a fixture like this one and call it complete.
#[test]
fn the_four_fields_of_a_constraint_are_all_required() {
    let scratch = Scratch::new("four");
    for field in ["prerequisite", "authority", "fallback", "consequence"] {
        let mut short = a_constraint();
        short.as_object_mut().expect("an object").remove(field);
        let mut work = work();
        work["constraints"] = json!([short]);

        let refusal = deposit(&scratch, work)
            .expect_err(&format!("a constraint without «{field}» is refused"));

        assert_eq!(
            refusal.class, "mandate_incomplete",
            "«{field}»: {refusal:?}"
        );
    }
}

fn waiting(scratch: &Scratch) -> Result<ActionOutcome, flow::ActionError> {
    run(
        MANDATE_WAITING_ACTION,
        json!({"tty": "ttys001", "store": scratch.store()}),
    )
}

fn not_yet(outcome: Result<ActionOutcome, flow::ActionError>) -> String {
    match outcome.expect("the step does not break") {
        ActionOutcome::NotYet(why) => why,
        other => panic!("it should have said not yet: {other:?}"),
    }
}

/// **NOTHING DESTRUCTIVE STANDS ON A SILENCE.** A terminal with no handover on
/// disk has written nothing down, and emptying it would throw the work away.
#[test]
fn a_terminal_that_handed_nothing_on_is_not_ready_to_be_emptied() {
    let scratch = Scratch::new("nothing-waiting");

    let why = not_yet(waiting(&scratch));

    assert!(why.contains("no mandate has been deposited"), "{why}");
}

/// And a handover already taken belongs to a session that has come and gone.
#[test]
fn a_handover_already_taken_is_not_one_waiting() {
    let scratch = Scratch::new("already-taken");
    deposit(&scratch, work()).expect("the deposit goes");
    resume(&scratch, "the-successor").expect("the successor takes it");

    let why = not_yet(waiting(&scratch));

    assert!(why.contains("the-successor"), "{why}");
}

/// The handover names the command line that wrote it, so whoever empties the
/// session reads the product's name from the mandate and not from a flow file.
#[test]
fn a_handover_waiting_names_the_line_that_wrote_it() {
    let scratch = Scratch::new("waiting");
    deposit(&scratch, work()).expect("the deposit goes");

    let answer = match waiting(&scratch).expect("the step does not break") {
        ActionOutcome::Went(value) => value,
        other => panic!("it did not go: {other:?}"),
    };

    assert_eq!(answer["engine"], json!("a-command-line"), "{answer}");
    assert_eq!(answer["session"], json!("the-predecessor"), "{answer}");
}
