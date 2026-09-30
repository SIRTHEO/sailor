//! The page every command line starts with carries the rules whole and only
//! points at the notes: a `reference` or `project` body past a line's length is
//! cut at its first sentence, and says which search finds the rest.

use actions::memory::{page, whole, Memory};

fn memory(kind: &str, label: &str, value: &str) -> Memory {
    Memory {
        kind: kind.to_owned(),
        label: label.to_owned(),
        value: value.to_owned(),
        provenance: "a test".to_owned(),
        modified: 0,
        valid_from: 0,
        valid_until: None,
        tree: None,
    }
}

fn long_body(first: &str) -> String {
    format!(
        "{first} {}",
        "A later sentence that only the search needs. ".repeat(40)
    )
}

fn line_of<'a>(page: &'a str, label: &str) -> &'a str {
    page.lines()
        .find(|line| line.contains(&format!("**{label}**")))
        .unwrap_or_else(|| panic!("the page does not list {label}: {page}"))
}

#[test]
fn a_long_reference_is_cut_at_its_first_sentence_and_names_its_search() {
    let body = long_body("The lab answers on one port.");
    let text = page(&[memory("reference", "lab-port", &body)], "any");
    let line = line_of(&text, "lab-port");
    assert!(line.contains("The lab answers on one port."), "{line}");
    assert!(!line.contains("A later sentence"), "{line}");
    assert!(line.contains("sailor memory show lab-port"), "{line}");
}

#[test]
fn a_long_project_note_is_indexed_like_a_reference() {
    let body = long_body("The window ships in three columns.");
    let text = page(&[memory("project", "window-columns", &body)], "any");
    let line = line_of(&text, "window-columns");
    assert!(!line.contains("A later sentence"), "{line}");
    assert!(line.contains("sailor memory show window-columns"), "{line}");
}

#[test]
fn a_rule_is_never_cut() {
    let body = long_body("Never commit with add -A.");
    for kind in ["feedback", "user"] {
        let text = page(&[memory(kind, "a-rule", &body)], "any");
        assert!(
            line_of(&text, "a-rule").contains(body.trim()),
            "{kind} was cut"
        );
    }
}

#[test]
fn a_short_note_stays_whole_and_points_nowhere() {
    let text = page(
        &[memory("reference", "short", "One line. And a second.")],
        "any",
    );
    let line = line_of(&text, "short");
    assert!(line.contains("One line. And a second."), "{line}");
    assert!(!line.contains("memory show"), "{line}");
}

#[test]
fn a_body_without_a_sentence_end_is_cut_on_a_character_boundary() {
    let body = "è".repeat(400);
    let text = page(&[memory("reference", "accents", &body)], "any");
    let line = line_of(&text, "accents");
    assert!(line.len() < 300, "{} bytes", line.len());
    assert!(std::str::from_utf8(line.as_bytes()).is_ok());
    assert!(line.contains("sailor memory show accents"), "{line}");
}

#[test]
fn the_page_of_a_hundred_long_notes_stays_small() {
    let notes: Vec<Memory> = (0..100)
        .map(|n| memory("reference", &format!("note-{n}"), &long_body("A summary.")))
        .collect();
    let text = page(&notes, "any");
    assert!(text.len() < 40_000, "{} bytes", text.len());
}

#[test]
fn an_abbreviation_does_not_end_the_first_sentence() {
    let body = long_body("Use e.g. port 8080 only when the door is closed.");
    let text = page(&[memory("project", "the-door", &body)], "any");
    let line = line_of(&text, "the-door");
    assert!(line.contains("only when the door is closed."), "{line}");
    assert!(!line.contains("A later sentence"), "{line}");
}

#[test]
fn a_note_is_whole_at_the_limit_and_cut_one_byte_past_it() {
    let at = "x".repeat(240);
    let past = "x".repeat(241);
    let text = page(
        &[
            memory("reference", "at-limit", &at),
            memory("reference", "past-limit", &past),
        ],
        "any",
    );
    assert!(!line_of(&text, "at-limit").contains("memory show"));
    assert!(line_of(&text, "past-limit").contains("sailor memory show past-limit"));
}

#[test]
fn the_whole_note_is_found_by_its_label() {
    let body = long_body("A summary.");
    let held = [
        memory("reference", "wanted", &body),
        memory("reference", "other", "Short."),
    ];
    let found = whole(&held, "wanted");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].value, body);
    assert!(whole(&held, "unknown").is_empty());
}

#[test]
fn a_full_stop_is_a_sentence_end_only_before_a_capital() {
    for first in [
        "Use v1. 2 of the tool only when the door is closed.",
        "Ask Dr. Smith about the door when it is closed.",
        "Ask the owner (e.g. Bob) about the door when it is closed.",
        "Read section 3. 4 of the note about the door when closed.",
    ] {
        let text = page(&[memory("project", "the-door", &long_body(first))], "any");
        let line = line_of(&text, "the-door");
        assert!(line.contains("when"), "cut inside «{first}»: {line}");
        assert!(!line.contains("A later sentence"), "{line}");
    }
}

#[test]
fn a_sentence_that_ends_in_no_is_cut_there() {
    let text = page(
        &[memory(
            "project",
            "the-answer",
            &long_body("The answer is no. Then the rest."),
        )],
        "any",
    );
    let line = line_of(&text, "the-answer");
    assert!(line.contains("The answer is no. …"), "{line}");
}

#[test]
fn a_note_with_windows_line_ends_is_cut_like_any_other() {
    for (label, joint) in [
        ("crlf", "\r\n"),
        ("lone-cr", "\r"),
        ("blank-line", "\n\n"),
        ("crlf-blank", "\r\n\r\n"),
    ] {
        let body = long_body(&format!("Line one.{joint}Line two."));
        let text = page(&[memory("reference", label, &body)], "any");
        let line = line_of(&text, label);
        assert!(!line.contains('\r'), "{line:?}");
        assert_eq!(summary_of(line), "Line one.", "{label}");
    }
}

#[test]
fn the_pointer_is_the_key_and_the_key_finds_the_note() {
    let body = long_body("A summary.");
    let held = [memory("reference", "Lab Port", &body)];
    let text = page(&held, "any");
    let line = line_of(&text, "Lab Port");
    assert!(line.contains("`sailor memory show lab-port`"), "{line}");
    assert_eq!(whole(&held, "lab-port").len(), 1);
    assert_eq!(whole(&held, "Lab Port").len(), 1);
}

fn summary_of(line: &str) -> &str {
    line.split_once("): ")
        .expect("a page line")
        .1
        .split(" … (")
        .next()
        .expect("a summary")
}

#[test]
fn a_sentence_may_open_on_markup_a_quote_or_a_year() {
    for next in [
        "«Bar» follows.",
        "`bar` follows.",
        "**Bar** follows.",
        "(Bar) follows.",
        "\"Bar\" follows.",
        "2026 follows.",
    ] {
        let text = page(
            &[memory(
                "project",
                "opening",
                &long_body(&format!("Foo. {next}")),
            )],
            "any",
        );
        assert_eq!(
            summary_of(line_of(&text, "opening")),
            "Foo.",
            "next: {next}"
        );
    }
}

#[test]
fn an_abbreviation_or_an_initial_is_not_a_sentence_end() {
    for first in [
        "Ask Dott. Rossi about the door when it is closed.",
        "Ask the U.S. Army about the door when it is closed.",
        "Ask A. Rossi about the door when it is closed.",
        "See art. 5 and Cfr. Rossi about the door when closed.",
    ] {
        let text = page(&[memory("project", "the-door", &long_body(first))], "any");
        assert_eq!(summary_of(line_of(&text, "the-door")), first);
    }
}

#[test]
fn a_summary_is_at_most_two_hundred_bytes_and_may_end_on_the_last_of_them() {
    let none = page(&[memory("reference", "no-stop", &"x ".repeat(300))], "any");
    assert!(summary_of(line_of(&none, "no-stop")).len() <= 200);
    let body = format!(
        "{}. Then the rest, which is long. {}",
        "x".repeat(198),
        "y ".repeat(50)
    );
    let edge = page(&[memory("reference", "edge", &body)], "any");
    assert!(
        line_of(&edge, "edge").contains(". … (`sailor memory show edge`)"),
        "{edge}"
    );
}

#[test]
fn the_key_is_lowercase_dashed_and_keeps_accents() {
    let held = [memory(
        "reference",
        "Café  Nord_Port",
        &long_body("A summary."),
    )];
    assert!(line_of(&page(&held, "any"), "Café  Nord_Port")
        .contains("`sailor memory show café-nord-port`"));
    assert_eq!(whole(&held, "CAFÉ nord port").len(), 1);
    assert!(whole(&held, "cafe-nord-port").is_empty());
}
