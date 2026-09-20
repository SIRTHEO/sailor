//! `sailor aside`: which engines are set aside after saying their quota was
//! spent, and the one gesture that brings one back early.
//!
//! The time a park lasts is the one the descriptor declares, and no provider
//! promised it. Where the window really reopened sooner, or the account was
//! topped up, this is what ends the park instead of waiting out the clock.

use actions::cooldown::{self, SetAside};
use std::collections::BTreeMap;
use std::path::Path;

pub const USAGE: &[crate::Form] = &[
    crate::Form {
        form: "sailor aside list [--json]",
        says_key: "cli.aside.form.list",
    },
    crate::Form {
        form: "sailor aside bring-back <key>",
        says_key: "cli.aside.form.bring_back",
    },
];

const AN_HOUR: i64 = 3_600;

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("sailor aside: {message}");
            1
        }
    }
}

fn usage_text() -> String {
    format!(
        "{} {}",
        catalogue::say("cli.usage_heading", &[]),
        crate::forms_as_lines(USAGE).join("\n       ")
    )
}

fn dispatch(args: &[String]) -> Result<String, String> {
    let path =
        cooldown::default_path().ok_or_else(|| catalogue::say("cli.aside.no_home_no_list", &[]))?;
    let now = machine::now();
    match args {
        [verb] if verb == "list" => Ok(render(&cooldown::all_set_aside(&path, now), now)),
        [verb, option] if verb == "list" && option == "--json" => {
            serde_json::to_string_pretty(&cooldown::all_set_aside(&path, now))
                .map_err(|error| error.to_string())
        }
        [verb, key] if verb == "bring-back" => bring_back(&path, key),
        _ => Err(usage_text()),
    }
}

/// **PUBLIC SO A TEST READS THE WORDS THE PERSON READS.** What an engine said
/// is printed beside its key: the key alone says which door is shut and never
/// why, which is the question somebody looking at this list actually has.
pub fn render(aside: &BTreeMap<String, SetAside>, now: i64) -> String {
    if aside.is_empty() {
        return catalogue::say("cli.aside.nothing_is_set_aside", &[]);
    }
    let mut lines = vec![catalogue::say(
        "cli.aside.set_aside_now",
        &[("count", &aside.len().to_string())],
    )];
    let mut soonest_back_first: Vec<(&String, &SetAside)> = aside.iter().collect();
    soonest_back_first.sort_by_key(|(key, one)| (one.until, *key));
    for (key, one) in soonest_back_first {
        lines.push(catalogue::say(
            "cli.aside.one_engine",
            &[
                ("key", key),
                ("left", &how_long_is_left(one.until - now)),
                ("said", &one.said),
            ],
        ));
    }
    lines.push(catalogue::say("cli.aside.a_park_is_not_a_verdict", &[]));
    lines.join("\n")
}

fn how_long_is_left(secs: i64) -> String {
    let secs = secs.max(0);
    if secs >= AN_HOUR {
        return catalogue::say("cli.aside.hours_left", &[("hours", &(secs / AN_HOUR).to_string())]);
    }
    catalogue::say("cli.aside.minutes_left", &[("minutes", &(secs / 60).to_string())])
}

/// **PUBLIC SO A TEST CAN HAND IT A PATH.** Which list is read is decided by
/// the environment, and a test that sets a variable decides it for whatever
/// runs beside it.
pub fn bring_back(path: &Path, key: &str) -> Result<String, String> {
    if cooldown::bring_back(path, key)? {
        return Ok(catalogue::say("cli.aside.brought_back", &[("key", key)]));
    }
    Err(catalogue::say("cli.aside.was_not_set_aside", &[("key", key)]))
}

#[cfg(test)]
mod tests {
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
        assert!(rows[1].contains("second@other"), "soonest back comes first: {said}");
        assert!(rows[2].contains("a-later-one"), "soonest back comes first: {said}");
        assert!(said.contains("quota spent") && said.contains("weekly limit"), "{said}");
        assert!(said.contains("10m"), "under an hour is said in minutes: {said}");
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
}
