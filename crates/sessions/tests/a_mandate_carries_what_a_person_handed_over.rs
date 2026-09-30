//! What a person handed to a session travels with the work.
//!
//! A link the person sent was lost across a reset: the mandate had a place for
//! the goal, the last instruction and the next step, and none for the material
//! itself, and the deposit dropped any field it did not know without a word.

use serde_json::{json, Value};
use sessions::mandate::{blank_fields, deposit, pass_on, read, address_in, Mandate};
use std::path::PathBuf;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sailor-material-{}-{name}", std::process::id()));
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
