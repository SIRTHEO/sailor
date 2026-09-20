//! The suite of `aside_cmd`, in a file of its own (ADR-022).

use super::*;

fn a_scratch(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("sailor-aside-{}-{name}", std::process::id()))
}

fn one(until: i64, said: &str) -> SetAside {
    SetAside {
        since: 0,
        until,
        said: said.to_owned(),
    }
}

#[test]
fn an_empty_list_says_nobody_is_aside_instead_of_printing_nothing() {
    let said = render(&BTreeMap::new(), 100);
    assert!(!said.trim().is_empty());
    assert!(said.contains("no engine"), "{said}");
}

#[test]
fn every_parked_engine_is_named_with_what_it_said_and_how_long_is_left() {
    let aside = BTreeMap::from([
        ("a-later-one".to_owned(), one(25_000, "weekly limit")),
        ("second@other".to_owned(), one(1_000, "quota spent")),
    ]);
    let said = render(&aside, 400);
    let rows: Vec<&str> = said.lines().collect();
    assert!(
        rows[1].contains("second@other"),
        "soonest back comes first: {said}"
    );
    assert!(
        rows[2].contains("a-later-one"),
        "soonest back comes first: {said}"
    );
    assert!(
        said.contains("quota spent") && said.contains("weekly limit"),
        "{said}"
    );
    assert!(
        said.contains("10m"),
        "under an hour is said in minutes: {said}"
    );
    assert!(said.contains("6h"), "over an hour is said in hours: {said}");
}

#[test]
fn bringing_one_back_answers_for_the_key_it_was_given() {
    let dir = a_scratch("cmd");
    let path = dir.join("cooldowns.json");
    cooldown::set_aside(&path, "first", 100, 600, "quota spent").expect("written");
    let said = bring_back(&path, "first").expect("it was aside");
    assert!(said.contains("first"), "{said}");
    let refused = bring_back(&path, "first").expect_err("no longer aside");
    assert!(refused.contains("first"), "{refused}");
    assert!(cooldown::all_set_aside(&path, 200).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
