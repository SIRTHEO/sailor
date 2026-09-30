//! What a person handed to a session travels with the work.
//!
//! A link the person sent was lost across a reset: the mandate had a place for
//! the goal, the last instruction and the next step, and none for the material
//! itself, and the deposit dropped any field it did not know without a word.

use serde_json::{Value, json};
use sessions::mandate::{Mandate, address_in, blank_fields, deposit, pass_on, read, reserve};
use std::path::PathBuf;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("sailor-material-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory to write in");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn a_mandate(tty: &str, references: Value) -> Mandate {
    let mut mandate: Value = json!({
        "written": {"tree": "/t", "tty": tty, "session": "author", "engine": "claude-code",
                    "tokens": 1, "at": 1, "branch": "b", "head": "h", "uncommitted": "u"},
        "work": {"goal": "g", "asked": "a", "next": "n"},
    });
    if !references.is_null() {
        mandate["work"]["references"] = references;
    }
    serde_json::from_value(mandate).expect("a mandate of this shape")
}

fn material() -> Value {
    json!([{"what": "the method this session applies", "at": "https://example.org/method"}])
}

/// **THE DEPOSIT DOES NOT DROP WHAT IT WAS GIVEN.**
#[test]
fn material_left_in_a_mandate_is_the_material_read_back() {
    let scratch = Scratch::new("round-trip");
    deposit(&scratch.0, &a_mandate("ttys001", material())).expect("the deposit goes");

    let read_back = read(&address_in(&scratch.0, "ttys001")).expect("it is on disk");

    assert_eq!(
        serde_json::to_value(&read_back).expect("a value")["work"]["references"],
        material()
    );
}

/// **A MANDATE WRITTEN BEFORE THE FIELD STILL READS, AND IS WRITTEN BACK AS IT WAS.**
/// Taking and reserving rewrite the file; a rewrite that added an empty list to
/// every old mandate would change what nobody asked to change.
#[test]
fn a_mandate_without_material_reads_and_is_written_back_unchanged() {
    let scratch = Scratch::new("old");
    let before = a_mandate("ttys002", Value::Null);
    deposit(&scratch.0, &before).expect("the deposit goes");

    let read_back = read(&address_in(&scratch.0, "ttys002")).expect("it is on disk");

    assert_eq!(read_back.work, before.work);
    assert!(
        serde_json::to_value(&read_back).expect("a value")["work"]
            .get("references")
            .is_none(),
        "an old mandate gains no field by being read"
    );
}

/// **A REFERENCE WITH A BLANK HALF IS REFUSED WHILE ITS AUTHOR IS STILL HERE.**
#[test]
fn material_with_a_blank_half_is_named_at_deposit() {
    let blank = a_mandate(
        "ttys003",
        json!([{"what": "", "at": "https://example.org/m"}, {"what": "the note", "at": " "}]),
    );

    assert_eq!(
        blank_fields(&blank),
        vec![
            "work.references[0].what".to_owned(),
            "work.references[1].at".to_owned()
        ]
    );
}

/// **A MANDATE SENT ON KEEPS WHAT THE PERSON HANDED OVER.**
#[test]
fn material_survives_a_mandate_being_passed_to_another_terminal() {
    let scratch = Scratch::new("passed");
    deposit(&scratch.0, &a_mandate("ttys004", material())).expect("the deposit goes");

    pass_on(&scratch.0, "ttys004", "ttys005", 50).expect("pass it on");

    let arrived = read(&address_in(&scratch.0, "ttys005")).expect("it waits for the new one");
    assert_eq!(
        serde_json::to_value(&arrived).expect("a value")["work"]["references"],
        material()
    );
}

/// **A KEY THIS BINARY DOES NOT KNOW SURVIVES BEING REWRITTEN.** Reserving and
/// taking rewrite the file, and an older or newer binary in service must not
/// strip what only the other one understands.
#[test]
fn a_key_this_binary_does_not_know_survives_a_rewrite() {
    let scratch = Scratch::new("later-field");
    let mut value = serde_json::to_value(a_mandate("ttys006", Value::Null)).expect("a value");
    value["work"]["a_field_of_a_later_version"] = json!(["kept"]);
    let mandate: Mandate = serde_json::from_value(value).expect("it reads");
    deposit(&scratch.0, &mandate).expect("the deposit goes");

    reserve(&address_in(&scratch.0, "ttys006"), "the-successor", 100).expect("it is held");

    let read_back = read(&address_in(&scratch.0, "ttys006")).expect("it is on disk");
    assert_eq!(
        serde_json::to_value(&read_back).expect("a value")["work"]["a_field_of_a_later_version"],
        json!(["kept"])
    );
}

/// **HALF A REFERENCE STILL READS, SO IT CAN BE NAMED.**
#[test]
fn half_a_reference_reads_and_is_named_with_its_place() {
    let half: Mandate = serde_json::from_value(
        serde_json::to_value(a_mandate("ttys007", json!([{"what": "a note", "at": "x"}])))
            .map(|mut value| {
                value["work"]["references"] = json!([{"what": "a note"}]);
                value
            })
            .expect("a value"),
    )
    .expect("half a reference still reads");

    assert_eq!(
        blank_fields(&half),
        vec!["work.references[0].at".to_owned()]
    );
}

/// **THE SAME ONE LEVEL DOWN.** A claim, a decision, a constraint or a reference
/// from another version keeps its extra key through a rewrite as well.
#[test]
fn a_key_one_level_down_survives_a_rewrite_too() {
    let scratch = Scratch::new("nested-later-field");
    let mut value = serde_json::to_value(a_mandate("ttys008", Value::Null)).expect("a value");
    value["work"]["state"] = json!([{"said": "x", "verified": true, "a_later_key": "kept"}]);
    let mandate: Mandate = serde_json::from_value(value).expect("it reads");
    deposit(&scratch.0, &mandate).expect("the deposit goes");

    reserve(&address_in(&scratch.0, "ttys008"), "the-successor", 100).expect("it is held");

    let read_back = read(&address_in(&scratch.0, "ttys008")).expect("it is on disk");
    assert_eq!(
        serde_json::to_value(&read_back).expect("a value")["work"]["state"][0]["a_later_key"],
        json!("kept")
    );
}

/// **THE LISTED KEYS ARE THE KEYS OF A WORK**, so the refusal cannot go stale.
#[test]
fn the_keys_a_refusal_lists_are_the_keys_a_work_has() {
    let full = a_mandate("ttys009", material());
    let value = serde_json::to_value(&full.work).expect("a value");
    let mut has: Vec<&str> = value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    let mut listed: Vec<&str> = sessions::mandate::WORK_KEYS.to_vec();
    has.sort_unstable();
    listed.sort_unstable();
    assert_eq!(has, listed);
}

/// **A LATER KEY SURVIVES IN EVERY KIND THAT CARRIES ONE.**
#[test]
fn a_later_key_survives_in_every_nested_kind() {
    let scratch = Scratch::new("every-kind");
    let mut value = serde_json::to_value(a_mandate("ttys010", material())).expect("a value");
    value["work"]["state"] = json!([{"said": "x", "verified": true, "later": 1}]);
    value["work"]["decisions"] = json!([{"decided": "x", "authorised_by": "y", "later": 2}]);
    value["work"]["constraints"] = json!([{"holds": "a", "prerequisite": "b", "authority": "c",
        "fallback": "d", "consequence": "e", "later": 3}]);
    value["work"]["references"][0]["later"] = json!(4);
    let mandate: Mandate = serde_json::from_value(value).expect("it reads");
    deposit(&scratch.0, &mandate).expect("the deposit goes");

    reserve(&address_in(&scratch.0, "ttys010"), "the-successor", 100).expect("it is held");

    let work = serde_json::to_value(read(&address_in(&scratch.0, "ttys010")).expect("on disk"))
        .expect("a value")["work"]
        .clone();
    assert_eq!(work["state"][0]["later"], json!(1));
    assert_eq!(work["decisions"][0]["later"], json!(2));
    assert_eq!(work["constraints"][0]["later"], json!(3));
    assert_eq!(work["references"][0]["later"], json!(4));
}
