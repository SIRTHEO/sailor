//! The quota channel and the capability that announces it must agree, engine
//! by engine, with **no engine named in code**: the constraint "model
//! independence" makes a reading that works on some engines only **that
//! tool's** declared channel, and whoever lacks it keeps working, paying more.
//! It is fault 10, one list in two places, applied to one block and one word.

use toolbox::descriptor::{CapabilityState, Catalog, Source};
use toolbox::probe::Machine;

/// The capability's name, as the descriptors write it.
const READ_REMAINING_QUOTA: &str = "read_remaining_quota";

fn shipped() -> Catalog {
    Catalog::load(&[Source::Builtin])
}

/// A `quota` block nobody announces is a channel no dispatcher will consider;
/// an announced capability with no block is a promise the reader cannot keep.
/// Either way the two diverge with nothing turning red — unless this does.
#[test]
fn every_declared_channel_is_announced_and_every_announcement_has_its_channel() {
    let catalog = shipped();
    let machine = Machine::current();
    let mut with_channel = 0;
    for loaded in catalog.live() {
        let descriptor = &loaded.descriptor;
        let announced = descriptor.capability(READ_REMAINING_QUOTA) == CapabilityState::Available;
        let declared = toolbox::quota::channel_of(descriptor, &machine);
        match (announced, declared) {
            (true, Some(Ok(channel))) => {
                assert_eq!(channel.engine, descriptor.id, "the channel is signed with its own engine");
                with_channel += 1;
            }
            (true, Some(Err(why))) => panic!("«{}» announces a channel it cannot make: {why}", descriptor.id),
            (true, None) => panic!("«{}» announces `read_remaining_quota` and declares no `quota` block", descriptor.id),
            (false, Some(_)) => panic!("«{}» declares a `quota` block and does not announce it", descriptor.id),
            (false, None) => {}
        }
    }
    assert!(with_channel >= 1, "the shipped list declares no quota channel at all: the reader has nothing to read");
}

/// **AN ENGINE THAT CANNOT MUST NOT STAY SILENT.** One that cannot report its
/// own quota and does not declare so is indistinguishable from one nobody ever
/// looked at — the three states of the `capabilities` block, and all three are
/// needed. At least one shipped engine was looked at and could not: it is
/// written `false`, and the why is in its note.
#[test]
fn an_engine_that_was_looked_at_and_cannot_says_so_instead_of_staying_silent() {
    let catalog = shipped();
    let looked_at_and_cannot = catalog
        .live()
        .into_iter()
        .filter(|loaded| loaded.descriptor.family == "ai_cli")
        .filter(|loaded| loaded.descriptor.capability(READ_REMAINING_QUOTA) == CapabilityState::Absent)
        .count();
    assert!(looked_at_and_cannot >= 1, "no shipped engine says «I cannot»: absence and silence are being confused");
}
