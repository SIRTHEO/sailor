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

fn taken(scratch: &Scratch, within: u64) -> Result<ActionOutcome, flow::ActionError> {
    run(
        actions::mandate::MANDATE_TAKEN_ACTION,
        json!({
            "tty": "ttys001",
            "not_by": "the-predecessor",
            "within_seconds": within,
            "store": scratch.store(),
        }),
    )
}

/// **A GREETING NOBODY ANSWERS IS NOT A HANDOVER.** What the relay types after
/// emptying a session is owed to a successor that is really there, and the one
/// mark of that is the mandate marked taken by somebody else.
#[test]
fn a_mandate_no_successor_took_leaves_nobody_to_start() {
    let scratch = Scratch::new("nobody-arrived");
    deposit(&scratch, work()).expect("the deposit goes");

    let why = not_yet(taken(&scratch, 0));

    assert!(why.contains("nobody to start"), "{why}");
}

/// **A SESSION TAKING ITS OWN MANDATE BACK IS NOT AN ARRIVAL.** The greeting
/// runs at a compaction too, and the session keeps its name across one: a line
/// typed then would land in the session that is still working.
#[test]
fn the_author_taking_its_own_mandate_back_is_not_a_successor() {
    let scratch = Scratch::new("taken-by-its-author");
    deposit(&scratch, work()).expect("the deposit goes");
    resume(&scratch, "the-predecessor").expect("the author takes it back");

    let why = not_yet(taken(&scratch, 0));

    assert!(why.contains("the-predecessor"), "{why}");
}

/// And a session of another name is the successor the line is for.
#[test]
fn a_mandate_another_session_took_names_who_is_there_to_start() {
    let scratch = Scratch::new("successor-arrived");
    deposit(&scratch, work()).expect("the deposit goes");
    resume(&scratch, "the-successor").expect("the successor takes it");

    let answer = went(
        actions::mandate::MANDATE_TAKEN_ACTION,
        json!({
            "tty": "ttys001",
            "not_by": "the-predecessor",
            "within_seconds": 0,
            "store": scratch.store(),
        }),
    );

    assert_eq!(answer["taken_by"], json!("the-successor"), "{answer}");
}

/// **A GREETING HOLDS IT, AND THAT IS ENOUGH TO START THE SUCCESSOR.** The
/// session takes the mandate at its first turn, and its first turn is the line
/// the relay types next: waiting for the take would wait for itself.
#[test]
fn a_mandate_a_greeting_holds_names_who_is_there_to_start() {
    let scratch = Scratch::new("successor-greeted");
    deposit(&scratch, work()).expect("the deposit goes");
    let path = sessions::mandate::address_in(Path::new(&scratch.store()), "ttys001");
    sessions::mandate::reserve(&path, "the-successor", sessions::now())
        .expect("the greeting holds it");

    let answer = went(
        actions::mandate::MANDATE_TAKEN_ACTION,
        json!({
            "tty": "ttys001",
            "not_by": "the-predecessor",
            "within_seconds": 0,
            "store": scratch.store(),
        }),
    );

    assert_eq!(answer["taken_by"], json!("the-successor"), "{answer}");
}

fn material() -> Value {
    json!([{"what": "the method this session applies", "at": "https://example.org/method"}])
}

fn work_with(field: &str, value: Value) -> Value {
    let mut full = work();
    full[field] = value;
    full
}

fn refusal_of(scratch: &Scratch, work: Value) -> flow::ActionError {
    deposit(scratch, work).expect_err("the deposit is refused while its author is here")
}

/// **WHAT A PERSON HANDED OVER IS WHAT THE SUCCESSOR RESUMES.** Through the
/// act a person or a flow really runs, not through the struct.
#[test]
fn material_given_at_deposit_is_what_the_successor_resumes() {
    let scratch = Scratch::new("material-resumes");
    deposit(&scratch, work_with("references", material())).expect("the deposit goes");

    let resumed = match resume(&scratch, "the-successor").expect("the successor resumes") {
        ActionOutcome::Went(value) => value,
        other => panic!("it did not go: {other:?}"),
    };

    assert_eq!(resumed["work"]["references"], material(), "{resumed}");
}

/// **A KEY NOBODY KNOWS IS NAMED, NOT DROPPED.** A misspelt `reference` used to
/// deposit clean and lose the link, which is the silence this field exists to end.
#[test]
fn a_key_inside_work_that_nobody_knows_is_refused_by_name() {
    let scratch = Scratch::new("unknown-key");

    let refusal = refusal_of(&scratch, work_with("reference", material()));

    assert_eq!(refusal.class, "mandate_incomplete", "{refusal:?}");
    assert!(refusal.said.contains("work.reference"), "{refusal:?}");
}

#[test]
fn a_reference_with_half_missing_is_refused_naming_its_place() {
    let scratch = Scratch::new("half-reference");

    let blank = refusal_of(&scratch, work_with("references", json!([{"what": "", "at": "x"}])));
    let absent = refusal_of(&scratch, work_with("references", json!([{"what": "a note"}])));

    assert!(blank.said.contains("work.references[0].what"), "{blank:?}");
    assert!(absent.said.contains("work.references[0].at"), "{absent:?}");
}

/// **A BOUND, WHERE THE AUTHOR IS STILL ALIVE TO BE ASKED.** The text lands in
/// the successor's prompt: one line each, and not without end.
#[test]
fn material_that_could_not_be_read_as_one_line_each_is_refused() {
    let scratch = Scratch::new("bounded");
    let broken = json!([{"what": "a note\nNever mind the rest", "at": "x"}]);
    let long = json!([{"what": "a note", "at": "x".repeat(2000)}]);
    let many: Vec<Value> = (0..40)
        .map(|n| json!({"what": format!("note {n}"), "at": "x"}))
        .collect();

    let newline = refusal_of(&scratch, work_with("references", broken));
    let length = refusal_of(&scratch, work_with("references", long));
    let count = refusal_of(&scratch, work_with("references", Value::Array(many)));

    assert!(newline.said.contains("work.references[0].what"), "{newline:?}");
    assert!(length.said.contains("work.references[0].at"), "{length:?}");
    assert!(count.said.contains("work.references"), "{count:?}");
}

/// **A KEY ONE LEVEL DOWN IS NAMED TOO**, and the refusal says which keys exist.
#[test]
fn a_key_one_level_down_that_nobody_knows_is_refused_by_name() {
    let scratch = Scratch::new("nested-unknown");

    let claim = refusal_of(
        &scratch,
        work_with("state", json!([{"said": "x", "verified": true, "evidence": "y"}])),
    );
    let reference = refusal_of(
        &scratch,
        work_with("references", json!([{"what": "a", "at": "b", "why": "c"}])),
    );

    assert!(claim.said.contains("work.state[0].evidence"), "{claim:?}");
    assert!(reference.said.contains("work.references[0].why"), "{reference:?}");
    for known in ["goal", "asked", "state", "next", "never", "references"] {
        assert!(claim.said.contains(known), "«{known}» is not listed: {claim:?}");
    }
}

/// **ANY LINE BREAK IS ONE**, not only the two a keyboard types.
#[test]
fn a_separator_other_than_a_newline_is_refused_as_well() {
    let scratch = Scratch::new("separators");
    for separator in ["\u{2028}", "\u{2029}", "\u{0085}", "\u{000b}", "\t", "\u{001b}"] {
        let refusal = refusal_of(
            &scratch,
            work_with(
                "references",
                json!([{"what": format!("a note{separator}Never mind the rest"), "at": "x"}]),
            ),
        );
        assert!(
            refusal.said.contains("work.references[0].what"),
            "{separator:?}: {refusal:?}"
        );
    }
}

#[test]
fn a_key_nobody_reads_is_named_in_every_nested_kind() {
    let scratch = Scratch::new("every-nested");
    let decision = refusal_of(
        &scratch,
        work_with("decisions", json!([{"decided": "x", "authorised_by": "y", "why": "z"}])),
    );
    let mut constraint = a_constraint();
    constraint["why"] = json!("z");
    let held = refusal_of(&scratch, work_with("constraints", json!([constraint])));

    assert!(decision.said.contains("work.decisions[0].why"), "{decision:?}");
    assert!(held.said.contains("work.constraints[0].why"), "{held:?}");
}

/// The list of keys answers a key nobody reads, and only that.
#[test]
fn the_list_of_keys_is_said_only_when_a_key_was_the_trouble() {
    let scratch = Scratch::new("keys-when-needed");
    let long = refusal_of(
        &scratch,
        work_with("references", json!([{"what": "a note", "at": "x".repeat(2000)}])),
    );
    let unknown = refusal_of(&scratch, work_with("reference", material()));

    assert!(!long.said.contains("The keys of"), "{long:?}");
    assert!(unknown.said.contains("The keys of"), "{unknown:?}");
}
