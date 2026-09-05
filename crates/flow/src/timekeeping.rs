//! Who makes something happen again without a person asking, declared once so
//! that nothing has to assert it from a distance. See fault 74.

/// What a keeper reads to decide something is due. A shape nothing reads is
/// the honest ground for a refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reads {
    FlowSchedule,
    TriggerRecurrence,
}

/// One keeper: what it reads, how often it looks, when it is not looking, and
/// the lines its file must hold to still be it.
pub struct Keeper {
    pub name: &'static str,
    pub reads: Reads,
    pub every_seconds: Option<u64>,
    pub alive_while: &'static str,
    pub lives_in: &'static str,
    pub the_lines: &'static [&'static str],
}

/// How often the window's beat looks, read by the beat itself.
pub const WINDOW_BEAT_EVERY_SECONDS: u64 = 60;

pub const KEEPERS: &[Keeper] = &[
    Keeper {
        name: "the window's beat",
        reads: Reads::FlowSchedule,
        every_seconds: Some(WINDOW_BEAT_EVERY_SECONDS),
        alive_while: "the window is open",
        lives_in: "desktop/src-tauri/src/beat.rs",
        the_lines: &["flow::is_due(schedule", "flow::WINDOW_BEAT_EVERY_SECONDS"],
    },
    Keeper {
        name: "`sailor flow tick`",
        reads: Reads::FlowSchedule,
        every_seconds: None,
        alive_while: "something calls it",
        lives_in: "crates/sailor/src/flow_cmd/beat.rs",
        the_lines: &["flow::is_due(schedule"],
    },
];

pub fn keepers_reading(what: Reads) -> impl Iterator<Item = &'static Keeper> {
    KEEPERS.iter().filter(move |keeper| keeper.reads == what)
}

/// The keepers of `what` in a sentence; empty when nothing reads it, which
/// the caller has to say for itself.
pub fn keepers_said(what: Reads) -> String {
    keepers_reading(what)
        .map(|keeper| match keeper.every_seconds {
            Some(seconds) => format!(
                "{}, every {seconds} seconds while {}",
                keeper.name, keeper.alive_while
            ),
            None => format!("{}, {}", keeper.name, keeper.alive_while),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_in_this_tree_fires_off_a_trigger_source_own_recurrence() {
        assert_eq!(keepers_reading(Reads::TriggerRecurrence).count(), 0);
        assert!(keepers_said(Reads::TriggerRecurrence).is_empty());
    }

    #[test]
    fn the_keepers_of_a_flow_schedule_say_their_rhythm_and_their_silence() {
        let said = keepers_said(Reads::FlowSchedule);
        assert!(
            said.contains("every 60 seconds while the window is open"),
            "{said}"
        );
        assert!(
            said.contains("`sailor flow tick`, something calls it"),
            "{said}"
        );
    }
}
