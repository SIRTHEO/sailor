//! What empties a running session is a fact about one command line, and it is
//! declared or it is unknown.
//!
//! There is no default here on purpose. A default would be one product's line
//! typed into every other, and a wrong line typed into a working session is
//! the mistake that cannot be taken back.

use toolbox::{builtin_catalog, Catalog, Descriptor, Source};

fn shipped() -> Catalog {
    Catalog::load(&[Source::Builtin])
}

fn read(json: &str) -> Descriptor {
    serde_json::from_str(json).expect("the descriptor loads")
}

#[test]
fn a_declared_line_comes_back() {
    let described = read(r#"{"id":"a","family":"ai_cli","reset_context":{"line":"/wipe"}}"#);
    assert_eq!(described.reset_line(), Some("/wipe"));
}

#[test]
fn a_command_line_that_declares_nothing_answers_nothing() {
    let described = read(r#"{"id":"a","family":"ai_cli"}"#);
    assert_eq!(
        described.reset_line(),
        None,
        "silence must stay silence: there is no line to fall back on"
    );
}

/// An empty line is not a line. Typed as it stands it would send a bare Enter
/// into a live session, which is an action and not a refusal.
#[test]
fn a_declaration_with_nothing_in_it_answers_nothing() {
    let described = read(r#"{"id":"a","family":"ai_cli","reset_context":{"line":""}}"#);
    assert_eq!(described.reset_line(), None);
}

/// A descriptor written for a newer Sailor keeps loading, and the field it
/// brought is named rather than swallowed.
#[test]
fn a_field_this_version_does_not_know_does_not_drop_the_declaration() {
    let described = read(
        r#"{"id":"a","family":"ai_cli","reset_context":{"line":"/wipe","confirm":"yes"}}"#,
    );
    assert_eq!(described.reset_line(), Some("/wipe"));
    assert!(
        described
            .unknown_fields()
            .contains(&"reset_context.confirm".to_owned()),
        "{:?}",
        described.unknown_fields()
    );
}

/// The shipped descriptors never carry a declaration that says nothing: an
/// empty one reads as «declared» to a human skimming the file and as «unknown»
/// to the code, and those two must not disagree.
#[test]
fn no_shipped_descriptor_declares_an_empty_line() {
    for loaded in shipped().live() {
        if let Some(reset) = &loaded.descriptor.reset_context {
            assert!(
                !reset.line.trim().is_empty(),
                "«{}» declares a reset with no line in it",
                loaded.descriptor.id
            );
        }
    }
}

/// Whoever measures gets measured. With nothing shipped declaring a line, every
/// test above would pass against a reader nobody ever calls.
#[test]
fn at_least_one_shipped_command_line_says_how_it_is_emptied() {
    let catalog = shipped();
    let declaring: Vec<&str> = catalog
        .live()
        .into_iter()
        .filter(|loaded| loaded.descriptor.reset_line().is_some())
        .map(|loaded| loaded.descriptor.id.as_str())
        .collect();
    assert!(
        !declaring.is_empty(),
        "no shipped descriptor declares how a session of it is emptied"
    );
}

/// And the shipped catalog is really the file, not an empty list that would let
/// the check above pass by having nothing to look at.
#[test]
fn the_shipped_catalog_is_the_one_that_ships() {
    assert!(
        builtin_catalog("tools").is_some_and(|text| text.contains("ai_cli")),
        "the builtin catalog must be the shipped file"
    );
    assert!(shipped().live().len() > 10, "the catalog loaded almost nothing");
}
