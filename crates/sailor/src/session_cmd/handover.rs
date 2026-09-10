//! What the relay leaves at the edges of a session, and what it takes there.
//!
//! Two moments and no more: the ask that stands until a mandate answers it, and
//! the handover that arrives with a greeting and is taken once. Both are read
//! by a keyed lookup, because measuring on every hook would read a transcript
//! of hundreds of megabytes in front of a person waiting to type.

use super::{Arrival, Request, TheDeposit};

/// Whoever opened this terminal, read from the variables their descriptor says
/// they leave in a session of theirs.
///
/// **NOTHING IS GUESSED.** A variable named in this crate would be one
/// product's fact in the code; the descriptors say which, and a machine with
/// none of them simply has no keeper.
pub(super) fn kept_by(tty: &str) -> Option<sessions::Kept> {
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    kept_from(&catalog, &machine.env, tty)
}

/// The same reading with the environment named, so it can be put to a session
/// this machine does not have.
fn kept_from(
    catalog: &toolbox::Catalog,
    env: &std::collections::BTreeMap<String, String>,
    tty: &str,
) -> Option<sessions::Kept> {
    for loaded in catalog.live() {
        let Some(keeps) = &loaded.descriptor.keeps_terminals else {
            continue;
        };
        for name in &keeps.known_by {
            let Some(handle) = env.get(name).filter(|it| !it.is_empty()) else {
                continue;
            };
            return Some(sessions::Kept {
                tty: tty.to_owned(),
                keeper: loaded.descriptor.id.clone(),
                handle: handle.clone(),
                named_by: name.clone(),
                seen_at: sessions::now(),
            });
        }
    }
    None
}

/// The mandate the session before left for this terminal, taken as it is read.
///
/// **THE GREETING IS THE DELIVERY.** It is the one channel whose text reaches
/// whoever is starting, so a mandate that arrived anywhere else would be a
/// mandate nobody was handed. Taken here, and marked with the successor's own
/// name: a second reading of the same handover would do the work twice.
pub(super) fn handed_on(request: &Request<'_>, arrival: &Arrival) -> Option<String> {
    let TheDeposit::Open(ledger) = request.deposit else {
        return None;
    };
    the_mandate_of(
        ledger.directory(),
        &arrival.anchor.tty,
        &arrival.session_id.clone().unwrap_or_default(),
    )
}

/// The same handover with everything it reads named, so it can be taken from a
/// store this machine does not keep.
fn the_mandate_of(store: &std::path::Path, tty: &str, session: &str) -> Option<String> {
    let path = sessions::mandate::address_in(store, tty);
    let left = sessions::mandate::read(&path)?;
    if left.taken.is_some() {
        return None;
    }
    if sessions::mandate::consume(&path, session, sessions::now()).is_err() {
        return None;
    }
    Some(catalogue::say(
        "cli.session.the_mandate_is_yours",
        &[
            ("goal", &left.work.goal),
            ("asked", &left.work.asked),
            ("next", &left.work.next),
            ("never", &left.work.never.join(" · ")),
            ("path", &path.display().to_string()),
        ],
    ))
}

/// The request for a mandate, while it is still unanswered.
///
/// Written by a run that measured this session out of band, and read here by a
/// keyed lookup: measuring on every hook would read a transcript of hundreds of
/// megabytes in front of a person waiting to type.
pub(super) fn the_ask_still_standing(request: &Request<'_>, tty: &str) -> Option<String> {
    let TheDeposit::Open(ledger) = request.deposit else {
        return None;
    };
    // The same store the ask was read from, so a run pointed elsewhere is
    // answered by the mandate that belongs to it.
    the_ask_of(ledger, ledger.directory(), tty)
}

/// The same question with everything it reads named, so it can be answered
/// about a store this machine does not keep.
fn the_ask_of(ledger: &ledger::Ledger, store: &std::path::Path, tty: &str) -> Option<String> {
    let asked = ledger.read_record(ASKS, tty).ok().flatten()?;
    if asked.value.get("state").and_then(serde_json::Value::as_str) != Some(OBLIGE) {
        return None;
    }
    // **A MANDATE NOBODY HAS TAKEN IS AN ANSWER, NOT A GAP.** The session goes
    // on filling and the run writes a fresher request each time: asking again
    // for what is already on disk teaches whoever reads it to stop reading.
    match sessions::mandate::read(&sessions::mandate::address_in(store, tty)) {
        Some(left) if left.taken.is_none() => {
            return Some(catalogue::say("cli.session.the_mandate_is_waiting", &[]));
        }
        // Taken, and asked for again since: the successor filled up in its turn.
        Some(left) if left.written.at >= asked.written_at => return None,
        _ => {}
    }
    let tokens = asked
        .value
        .get("tokens")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    Some(catalogue::say(
        "cli.session.the_mandate_is_asked_for",
        &[("tokens", &tokens.to_string())],
    ))
}

/// Where a run leaves the request, and the standing that makes one.
const ASKS: &str = "mandate_asks";
const OBLIGE: &str = "oblige";

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> toolbox::Catalog {
        toolbox::Catalog::load(&[toolbox::Source::Builtin])
    }

    fn env_of(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// **THE SESSION NAMES ITSELF TO WHOEVER KEEPS IT.** A hook runs inside the
    /// session, so the name is read from its own variables; guessed from
    /// outside it would be a guess about somebody else's terminal.
    #[test]
    fn a_session_opened_by_a_keeper_says_so_in_its_own_variables() {
        let kept = kept_from(
            &shipped(),
            &env_of(&[("ORCA_PANE_KEY", "pane-7"), ("PATH", "/usr/bin")]),
            "ttys004",
        )
        .expect("a session that carries the variable has a keeper");

        assert_eq!(kept.keeper, "orca");
        assert_eq!(kept.handle, "pane-7");
        assert_eq!(kept.named_by, "ORCA_PANE_KEY");
        assert_eq!(kept.tty, "ttys004");
    }

    /// An empty variable is not a handle: written that way it would name a
    /// terminal nobody can reach, and a road that refuses is worth more.
    #[test]
    fn a_variable_that_is_there_and_empty_names_nobody() {
        assert!(kept_from(&shipped(), &env_of(&[("ORCA_PANE_KEY", "")]), "ttys004").is_none());
    }

    /// A terminal nobody else opened has no keeper, and that is an answer.
    #[test]
    fn a_session_nobody_else_opened_has_no_keeper() {
        assert!(kept_from(&shipped(), &env_of(&[("PATH", "/usr/bin")]), "ttys004").is_none());
    }

    /// **THE GREETING IS THE DELIVERY, AND IT IS TAKEN ONCE.** A handover read
    /// twice sends two sessions off to do the same work, and neither of them
    /// can tell.
    #[test]
    fn a_mandate_left_for_this_terminal_arrives_with_the_greeting_and_only_once() {
        let directory = std::env::temp_dir().join(format!("sailor-handed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory of this test's own");
        let mut mandate = sessions::mandate::Mandate::default();
        mandate.written.tty = "ttys001".to_owned();
        mandate.written.at = 100;
        mandate.work.goal = "carry the relay to the end".to_owned();
        mandate.work.next = "read the screen of a held terminal".to_owned();
        mandate.work.never = vec!["do not type into what nobody holds".to_owned()];
        sessions::mandate::deposit(&directory, &mandate).expect("the mandate is deposited");

        let handed =
            the_mandate_of(&directory, "ttys001", "the-successor").expect("a mandate arrives");
        assert!(handed.contains("carry the relay to the end"), "{handed}");
        assert!(
            handed.contains("read the screen of a held terminal"),
            "{handed}"
        );
        assert!(
            handed.contains("do not type into what nobody holds"),
            "{handed}"
        );

        assert_eq!(
            the_mandate_of(&directory, "ttys001", "another-successor"),
            None,
            "a mandate already taken is not handed on a second time"
        );
        let taken = sessions::mandate::read(&sessions::mandate::address_in(&directory, "ttys001"))
            .expect("it is still on disk")
            .taken
            .expect("marked with whoever took it");
        assert_eq!(taken.by, "the-successor");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A terminal nobody left anything for is greeted and told nothing.
    #[test]
    fn a_terminal_with_no_mandate_waiting_is_handed_nothing() {
        let directory =
            std::env::temp_dir().join(format!("sailor-unhanded-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory of this test's own");

        assert_eq!(the_mandate_of(&directory, "ttys009", "whoever"), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// **THE ASK STANDS UNTIL IT IS ANSWERED, AND THE ANSWER IS THE MANDATE.**
    /// A request repeated after the mandate was written is a request nobody can
    /// satisfy: whoever reads it has already done the thing.
    #[test]
    fn the_ask_stands_until_a_mandate_answers_it() {
        let directory = std::env::temp_dir().join(format!("sailor-ask-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory of this test's own");
        let ledger = ledger::Ledger::open(&directory).expect("a store of this test's own");
        ledger
            .put_record(&ledger::StoreRecord {
                collection: ASKS.to_owned(),
                key: "ttys001".to_owned(),
                value: serde_json::json!({"state": OBLIGE, "tokens": 260_000u64}),
                written_by: "a-run".to_owned(),
                written_at: 100,
            })
            .expect("the ask is written");

        let asked = the_ask_of(&ledger, &directory, "ttys001");
        assert!(
            asked.is_some_and(|said| said.contains("260000")),
            "unanswered, it is asked"
        );

        let mut mandate = sessions::mandate::Mandate::default();
        mandate.written.tty = "ttys001".to_owned();
        mandate.written.at = 101;
        sessions::mandate::deposit(&directory, &mandate).expect("the mandate is deposited");
        let waiting = the_ask_of(&ledger, &directory, "ttys001").expect("it says what stands now");
        assert!(
            !waiting.contains("260000"),
            "deposited, it is no longer asked for: {waiting}"
        );

        // **AND A REQUEST FRESHER THAN A MANDATE ALREADY TAKEN ASKS AGAIN**:
        // that is the successor, which filled up in its turn.
        sessions::mandate::consume(
            &sessions::mandate::address_in(&directory, "ttys001"),
            "the-successor",
            102,
        )
        .expect("the successor takes it");
        ledger
            .put_record(&ledger::StoreRecord {
                collection: ASKS.to_owned(),
                key: "ttys001".to_owned(),
                value: serde_json::json!({"state": OBLIGE, "tokens": 300_000u64}),
                written_by: "a-later-run".to_owned(),
                written_at: 200,
            })
            .expect("the later ask is written");
        let again = the_ask_of(&ledger, &directory, "ttys001").expect("it is asked again");
        assert!(again.contains("300000"), "{again}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A standing below the one that obliges is not asked about at all.
    #[test]
    fn a_session_that_is_only_filling_up_is_asked_nothing() {
        let directory =
            std::env::temp_dir().join(format!("sailor-ask-below-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory of this test's own");
        let ledger = ledger::Ledger::open(&directory).expect("a store of this test's own");
        ledger
            .put_record(&ledger::StoreRecord {
                collection: ASKS.to_owned(),
                key: "ttys002".to_owned(),
                value: serde_json::json!({"state": "warn", "tokens": 160_000u64}),
                written_by: "a-run".to_owned(),
                written_at: 100,
            })
            .expect("the ask is written");

        assert_eq!(the_ask_of(&ledger, &directory, "ttys002"), None);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
