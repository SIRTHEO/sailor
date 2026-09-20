//! The name a provider gives a quota window does not say what the window is:
//! one names lengths, another names an order, and the same `primary_window`
//! was measured at a week on one account and thirty days on another. Proved
//! here: every shipped channel declares where the length comes from, and a
//! length nobody declared is never dressed up as a week.

use models::remaining::HowLong;
use toolbox::descriptor::{Descriptor, BUILTIN};

fn shipped() -> Vec<Descriptor> {
    let parsed: serde_json::Value = serde_json::from_str(BUILTIN).expect("the catalogue parses");
    parsed["tools"]
        .as_array()
        .expect("the catalogue holds tools")
        .iter()
        .map(|tool| serde_json::from_value(tool.clone()).expect("a tool of the catalogue"))
        .collect()
}

/// **A CHANNEL THAT READS A QUOTA READS ITS LENGTH TOO.** Without it the
/// window reaches the screen under the provider's own word, and the two
/// windows a person actually asks about cannot be told apart.
#[test]
fn every_shipped_quota_channel_says_where_the_length_comes_from() {
    let mut mute = Vec::new();
    for descriptor in shipped() {
        let Some(quota) = descriptor
            .quota
            .as_ref()
            .filter(|quota| quota.reader == "oauth_usage")
        else {
            continue;
        };
        let says = quota
            .shape
            .as_ref()
            .is_some_and(|shape| !shape.lasts.is_empty() || !shape.lasts_by_name.is_empty());
        if !says {
            mute.push(descriptor.id.clone());
        }
    }

    assert!(
        mute.is_empty(),
        "these read a quota and never say how long a window lasts: {mute:?}. \
         Declare `shape.lasts` where the provider sends the length, or \
         `shape.lasts_by_name` where it was measured off its answer."
    );
}

/// The two lengths that were measured, read back from the catalogue: a
/// rewrite that quietly drops one leaves this saying which.
#[test]
fn the_lengths_the_catalogue_declares_are_a_session_and_a_week() {
    let claude = shipped()
        .into_iter()
        .find(|descriptor| descriptor.id == "claude-code")
        .expect("the catalogue ships claude-code");
    let by_name = claude
        .quota
        .and_then(|quota| quota.shape)
        .expect("its shape")
        .lasts_by_name;

    assert_eq!(
        HowLong::of(by_name.get("five_hour").copied()),
        HowLong::Session
    );
    assert_eq!(
        HowLong::of(by_name.get("seven_day").copied()),
        HowLong::Week
    );
}

/// **A LENGTH NOBODY DECLARED STAYS UNNAMED.** The bands are what a screen
/// reads, so a window outside them keeps its seconds rather than borrowing
/// the nearest name: nineteen days called a week sends somebody back to a
/// door that stays shut for twelve more.
#[test]
fn a_length_outside_the_two_bands_is_never_called_one_of_them() {
    let nineteen_days = 19 * 24 * 60 * 60;

    assert_eq!(HowLong::of(None), HowLong::NotSaid);
    assert_eq!(
        HowLong::of(Some(nineteen_days)),
        HowLong::Other(nineteen_days)
    );
    assert_eq!(HowLong::of(Some(18_000)).to_string(), "session");
    assert_eq!(HowLong::of(Some(604_800)).to_string(), "week");
}
