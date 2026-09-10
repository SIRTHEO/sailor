//! What a flow asks of a session event, and the single reason it is told when
//! the answer is no.

use trigger::{deferral, Happened, Kind, On};

fn happened() -> Happened {
    Happened {
        event: "UserPromptSubmit".to_owned(),
        tree: "/a/tree".to_owned(),
        tty: "ttys001".to_owned(),
        session: "a-session".to_owned(),
        prompt: Some("carry on with the relay".to_owned()),
    }
}

fn on(event: &str) -> On {
    On {
        event: event.to_owned(),
        phrase: None,
        tree: None,
    }
}

#[test]
fn a_flow_watching_this_event_in_any_tree_is_started() {
    assert_eq!(deferral(&on("UserPromptSubmit"), &happened()), None);
}

#[test]
fn a_flow_watching_another_event_is_told_which_condition_said_no() {
    assert_eq!(
        deferral(&on("Stop"), &happened()),
        Some("another event"),
        "a guard that will not say which condition refused cannot be told from a broken one"
    );
}

#[test]
fn a_flow_bound_to_another_tree_is_deferred() {
    let mut asks = on("UserPromptSubmit");
    asks.tree = Some("/somewhere/else".to_owned());

    assert_eq!(deferral(&asks, &happened()), Some("another tree"));
}

#[test]
fn a_phrase_is_read_without_regard_to_case() {
    let mut asks = on("UserPromptSubmit");
    asks.phrase = Some("RELAY".to_owned());

    assert_eq!(deferral(&asks, &happened()), None);
}

/// **THE PHRASE IS MATCHED ON WHAT A PERSON TYPED AND ON NOTHING ELSE.** An
/// agent writing the phrase in its own answer would start the flow that
/// watches for it, and a flow that starts itself has no brake left.
#[test]
fn an_event_carrying_no_prompt_of_a_person_matches_no_phrase() {
    let mut asks = on("Stop");
    asks.phrase = Some("relay".to_owned());
    let ended = Happened {
        event: "Stop".to_owned(),
        prompt: None,
        ..happened()
    };

    assert_eq!(deferral(&asks, &ended), Some("no prompt of a person"));
}

#[test]
fn a_prompt_without_the_phrase_is_deferred_for_that_reason() {
    let mut asks = on("UserPromptSubmit");
    asks.phrase = Some("release".to_owned());

    assert_eq!(
        deferral(&asks, &happened()),
        Some("the phrase is not in the prompt")
    );
}

/// The source is shipped, switched on, and carries the signal with it: there
/// is no file to watch, because the call that writes the row is the source.
#[test]
fn the_shipped_source_is_loaded_and_declares_no_place_to_listen() {
    let catalog = trigger::Catalog::load(&[trigger::Source::Builtin]);

    let found = catalog
        .find("session-event")
        .expect("the source is shipped and switched on");

    assert_eq!(found.descriptor.kind, Kind::SessionEvent);
    assert!(found.descriptor.listen.is_none());
    assert!(found.descriptor.periodic.is_none());
}
