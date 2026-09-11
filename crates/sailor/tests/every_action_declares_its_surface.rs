//! **A NAME IS NOT A SURFACE.** An agent handed `apply_patch`, `for_each`,
//! `wait_free` guesses what they do and works the rest of the run on the guess.
//! So every action this machine registers carries one sentence of what it does,
//! what it takes, what it answers, what it refuses, and what it costs — and an
//! action added without one turns this red.
//!
//! **THE ANCHOR IS THE REGISTRY, RUNNING.** The names come from
//! `registry_in(House::empty(), ...)` and are never transcribed: a list written
//! beside the registry is the defect this test exists to stop.

use actions::surface::{declared_in, SURFACES};
use registry::{registry_in, House};

fn every_action() -> Vec<actions::surface::Declared> {
    declared_in(&registry_in(House::empty(), None, None))
}

#[test]
fn no_action_is_registered_without_a_surface() {
    let declared = every_action();
    assert!(
        !declared.is_empty(),
        "the registry answered no names at all: this test measured nothing"
    );
    let bare: Vec<&str> = declared
        .iter()
        .filter(|action| action.surface.is_none())
        .map(|action| action.action.as_str())
        .collect();
    assert!(
        bare.is_empty(),
        "{} of {} registered actions carry no surface, so whoever asks what \
         Sailor can do gets a bare name: {}. Declare each one in \
         `actions::surface::SURFACES`",
        bare.len(),
        declared.len(),
        bare.join(", ")
    );
}

/// The other direction: a surface for an action nobody registers is a sentence
/// no reader can act on, and it is how a table survives the code it described.
#[test]
fn no_surface_describes_an_action_that_is_not_registered() {
    let registered: Vec<String> = every_action()
        .into_iter()
        .map(|action| action.action)
        .collect();
    let orphans: Vec<&str> = SURFACES
        .iter()
        .map(|surface| surface.action)
        .filter(|action| !registered.iter().any(|known| known == action))
        .collect();
    assert!(
        orphans.is_empty(),
        "the table describes actions the registry does not hold: {}",
        orphans.join(", ")
    );
}

/// What «good» means here, in the only part of it a machine can read: the three
/// mandatory fields are said, and none of them restates the name.
#[test]
fn a_surface_says_something_the_name_does_not() {
    let mut complaints = Vec::new();
    for surface in SURFACES {
        for (field, text) in [
            ("does", surface.does),
            ("takes", surface.takes),
            ("answers", surface.answers),
        ] {
            if text.trim().is_empty() {
                complaints.push(format!("«{}» says nothing under {field}", surface.action));
            }
        }
        if surface.does.to_lowercase().starts_with("this action") {
            complaints.push(format!(
                "«{}» opens with «this action» instead of saying what it does",
                surface.action
            ));
        }
    }
    assert!(complaints.is_empty(), "{}", complaints.join("; "));
}
