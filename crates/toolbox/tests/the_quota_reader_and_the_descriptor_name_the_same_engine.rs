//! The quota reader and the descriptor must name **the same engine**.
//!
//! Reading the quota works on one engine only, and the permanent constraint
//! "model independence" says what to do with that: declare it as **that tool's
//! capability**, and whoever lacks it keeps working, paying more. It is fault 10
//! — the same list written in two places — applied to one single name.

use toolbox::descriptor::{CapabilityState, Catalog, Source};

/// The capability's name, as the descriptors write it.
const READ_REMAINING_QUOTA: &str = "read_remaining_quota";

fn shipped() -> Catalog {
    Catalog::load(&[Source::Builtin])
}

/// A declaration nobody interrogates is decoration: the descriptor would say
/// `claude-code` and the reader would read whatever it liked, and the two would
/// diverge with nothing turning red. So the name `models::remaining` signs every
/// reading with must be the `id` of a **shipped** descriptor that declares
/// `read_remaining_quota` available. Rename one without the other and this falls.
#[test]
fn the_engine_the_reader_writes_is_a_shipped_descriptor_that_declares_the_capability() {
    let catalog = shipped();
    let found = catalog
        .descriptors
        .iter()
        .find(|loaded| loaded.descriptor.id == models::remaining::CLAUDE_CODE)
        .unwrap_or_else(|| {
            panic!(
                "«{}» is the id of no shipped descriptor: the reader would sign its \
                 own readings with an engine that does not exist",
                models::remaining::CLAUDE_CODE
            )
        });

    assert_eq!(
        found.descriptor.capability(READ_REMAINING_QUOTA),
        CapabilityState::Available,
        "the engine the reader interrogates must declare that it can say this"
    );
}

/// **AN ENGINE THAT CANNOT MUST NOT STAY SILENT.** One that cannot report its
/// own quota and does not declare so is indistinguishable from one nobody ever
/// looked at — the three states of the `capabilities` block, and all three are
/// needed. Codex was looked at and could not: it is written `false`, and the why
/// is in its note.
#[test]
fn an_engine_that_was_looked_at_and_cannot_says_so_instead_of_staying_silent() {
    let catalog = shipped();
    let codex = catalog
        .descriptors
        .iter()
        .find(|loaded| loaded.descriptor.id == "codex")
        .expect("codex is shipped");

    assert_eq!(
        codex.descriptor.capability(READ_REMAINING_QUOTA),
        CapabilityState::Absent,
        "«tried and failed» is not «nobody looked»: it is written `false`"
    );
    assert!(
        codex.descriptor.note.contains("account/rateLimits/read"),
        "whoever tries again must find how far the last attempt got, or they start over"
    );
}
