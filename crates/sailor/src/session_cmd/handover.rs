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
        &arrival.anchor.worktree,
    )
}

/// The same handover with everything it reads named, so it can be taken from a
/// store this machine does not keep.
fn the_mandate_of(
    store: &std::path::Path,
    tty: &str,
    session: &str,
    tree: &str,
) -> Option<String> {
    let path = sessions::mandate::address_in(store, tty);
    let left = sessions::mandate::read(&path)?;
    if left.taken.is_some() {
        return None;
    }
    // **THE TTY IS THE ADDRESS, AND THE ADDRESS IS REUSED.** A different
    // session is what a mandate expects; the tree is the part that must still
    // hold. Taken once, it would be gone for the successor it was left for. A
    // mandate naming no tree is the older shape, and silence is not a tree.
    if !left.written.tree.is_empty() && !tree.is_empty() && left.written.tree != tree {
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

/// Files a mandate the session left in its own home, and says what became of it.
///
/// **THE DEPOSIT MUST NOT NEED A PERSON.** The session's own shell cannot write
/// the store, so a mandate typed there is refused and the relay stops at the one
/// session that filled up. The hook runs outside that sandbox: what the session
/// could only drop, this files.
pub(super) fn filed_what_was_dropped(request: &Request<'_>, tty: &str) -> Option<String> {
    let TheDeposit::Open(ledger) = request.deposit else {
        return None;
    };
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    filing_from(&catalog, &machine.env, ledger.directory(), tty)
}

/// The same filing with the environment named, so it can be put to a machine
/// this one does not have.
fn filing_from(
    catalog: &toolbox::Catalog,
    env: &std::collections::BTreeMap<String, String>,
    store: &std::path::Path,
    tty: &str,
) -> Option<String> {
    for home in homes_in(catalog, env) {
        let dropped = sessions::mandate::dropped_in(&home, tty);
        let Ok(text) = std::fs::read_to_string(&dropped) else {
            continue;
        };
        let at = dropped.display().to_string();
        return Some(match filed(store, tty, &text) {
            Ok(head) => {
                // Gone once filed: a drop left behind would be filed again at
                // the next hook, archiving the mandate over itself every turn.
                let _ = std::fs::remove_file(&dropped);
                catalogue::say(
                    "cli.session.the_drop_was_filed",
                    &[("tty", tty), ("head", &head)],
                )
            }
            // **THE DROP STAYS WHERE IT IS.** Its author is still here to be
            // asked, and a refusal nobody reads is a handover silently lost.
            Err(why) => catalogue::say(
                "cli.session.the_drop_was_refused",
                &[("path", &at), ("why", &why)],
            ),
        });
    }
    None
}

/// The home of every command line this machine declares, read the way the
/// grafting reads it: the descriptor says where, never this crate.
fn homes_in(
    catalog: &toolbox::Catalog,
    env: &std::collections::BTreeMap<String, String>,
) -> Vec<std::path::PathBuf> {
    let home = env.get("HOME").cloned().unwrap_or_default();
    catalog
        .live()
        .into_iter()
        .filter_map(|loaded| {
            let hooks = loaded.descriptor.session_hooks.as_ref()?;
            let root = env.get(&hooks.file.root_var).map(String::as_str);
            let file = hooks.file.path(root, &home)?;
            file.parent().map(std::path::Path::to_path_buf)
        })
        .collect()
}

/// Where this session is told to leave what its shell cannot file. The first
/// home is the one a session of that line writes to; with no line declared,
/// the ask names nothing and the typed form is the only way.
fn the_drop_for(
    catalog: &toolbox::Catalog,
    env: &std::collections::BTreeMap<String, String>,
    tty: &str,
) -> String {
    homes_in(catalog, env)
        .first()
        .map(|home| sessions::mandate::dropped_in(home, tty).display().to_string())
        .unwrap_or_default()
}

/// The deposit itself, from the shape the session dropped.
///
/// Whatever the session did not have to know is filled in here, exactly as the
/// typed form fills it: which terminal, which tree, which store.
fn filed(store: &std::path::Path, tty: &str, text: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err(catalogue::say("cli.terminal.nothing_to_hand_on", &[]));
    }
    let mut written: serde_json::Value = serde_json::from_str(text).map_err(|error| {
        catalogue::say("cli.terminal.mandate_shape", &[("why", &error.to_string())])
    })?;
    let object = written
        .as_object_mut()
        .ok_or_else(|| catalogue::say("cli.terminal.mandate_shape", &[("why", "not an object")]))?;
    object.insert("tty".to_owned(), serde_json::Value::String(tty.to_owned()));
    let at = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let tree = flow::workspace::find_root(&at).unwrap_or(at);
    object
        .entry("tree")
        .or_insert_with(|| serde_json::Value::String(tree.display().to_string()));
    object.insert(
        "store".to_owned(),
        serde_json::Value::String(store.display().to_string()),
    );
    let answer = actions::mandate::deposited(&written).map_err(|error| error.said)?;
    Ok(answer["head"].as_str().unwrap_or_default().to_owned())
}

/// The request for a mandate, while it is still unanswered.
///
/// Written by a run that measured this session out of band, and read here by a
/// keyed lookup: measuring on every hook would read a transcript of hundreds of
/// megabytes in front of a person waiting to type.
pub(super) fn the_ask_still_standing(
    request: &Request<'_>,
    tty: &str,
    session: &str,
) -> Option<String> {
    let TheDeposit::Open(ledger) = request.deposit else {
        return None;
    };
    // The same store the ask was read from, so a run pointed elsewhere is
    // answered by the mandate that belongs to it.
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    let drop = the_drop_for(&catalog, &machine.env, tty);
    the_ask_of(ledger, ledger.directory(), tty, session, &drop)
}

/// The same question with everything it reads named, so it can be answered
/// about a store this machine does not keep.
fn the_ask_of(
    ledger: &ledger::Ledger,
    store: &std::path::Path,
    tty: &str,
    session: &str,
    drop: &str,
) -> Option<String> {
    let asked = ledger.read_record(ASKS, tty).ok().flatten()?;
    if asked.value.get("state").and_then(serde_json::Value::as_str) != Some(OBLIGE) {
        return None;
    }
    // **A TTY NUMBER OUTLIVES THE SESSION THAT HELD IT.** The row names the
    // session it was written for; one naming another is not this session's to
    // answer. A row naming nobody is the older shape on disk, and silence is
    // not a different session.
    if let Some(asked_for) = asked.value.get("session").and_then(serde_json::Value::as_str) {
        if !asked_for.is_empty() && !session.is_empty() && asked_for != session {
            return None;
        }
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
        &[("tokens", &tokens.to_string()), ("drop", drop)],
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

    /// A mandate with nothing left blank: the deposit refuses a gap, and this
    /// test is about the road, not about the shape.
    fn a_whole_mandate() -> String {
        serde_json::json!({
            "session": "the-one-that-filled-up",
            "engine": "claude",
            "work": {
                "goal": "hand on without a person",
                "asked": "make depositing a mandate stop needing one",
                "next": "read the drop back from the store"
            }
        })
        .to_string()
    }

    fn env_of(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    /// **A DEPOSIT THAT NEEDS A PERSON IS NOT A RELAY.** The session's own shell
    /// is refused the store by the sandbox it runs in, so the one thing the
    /// relay is built on — a session handing on when it fills — was the one
    /// thing it could not do alone. What the session drops in its own home,
    /// the hook files from outside that sandbox.
    #[test]
    fn a_mandate_the_shell_could_only_drop_is_filed_by_the_hook() {
        let directory =
            std::env::temp_dir().join(format!("sailor-dropped-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let home = directory.join("home");
        let store = directory.join("store");
        std::fs::create_dir_all(&home).expect("a home of this test's own");
        std::fs::create_dir_all(&store).expect("a store of this test's own");

        let dropped = sessions::mandate::dropped_in(&home.join(".claude"), "ttys009");
        std::fs::create_dir_all(dropped.parent().expect("the letterbox has a parent"))
            .expect("the letterbox");
        std::fs::write(&dropped, a_whole_mandate()).expect("what the shell could write");

        let said = filing_from(
            &shipped(),
            &env_of(&[("HOME", &home.display().to_string())]),
            &store,
            "ttys009",
        )
        .expect("a drop waiting for this terminal is filed");

        assert!(said.contains("ttys009"), "the filing names the terminal: {said}");
        assert!(
            sessions::mandate::read(&sessions::mandate::address_in(&store, "ttys009")).is_some(),
            "the mandate is in the store, where a successor is handed it"
        );
        assert!(
            !dropped.exists(),
            "the drop is gone, or the next hook files it over itself"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// **A REFUSAL THE AUTHOR NEVER READS IS A HANDOVER LOST.** A drop that
    /// cannot be filed stays where it was left: whoever wrote it is still here
    /// to be told why, and to write it again.
    #[test]
    fn a_drop_that_cannot_be_filed_stays_where_its_author_can_still_fix_it() {
        let directory =
            std::env::temp_dir().join(format!("sailor-dropped-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let home = directory.join("home");
        let store = directory.join("store");
        std::fs::create_dir_all(&home).expect("a home of this test's own");
        std::fs::create_dir_all(&store).expect("a store of this test's own");

        let dropped = sessions::mandate::dropped_in(&home.join(".claude"), "ttys009");
        std::fs::create_dir_all(dropped.parent().expect("the letterbox has a parent"))
            .expect("the letterbox");
        std::fs::write(&dropped, "carry the conduit on").expect("prose where a mandate belongs");

        let said = filing_from(
            &shipped(),
            &env_of(&[("HOME", &home.display().to_string())]),
            &store,
            "ttys009",
        )
        .expect("a drop that cannot be filed is still answered");

        assert!(
            said.contains(&dropped.display().to_string()),
            "the refusal names the file to correct: {said}"
        );
        assert!(dropped.exists(), "the drop is still there to be corrected");
        assert!(
            sessions::mandate::read(&sessions::mandate::address_in(&store, "ttys009")).is_none(),
            "nothing was deposited from prose"
        );
        let _ = std::fs::remove_dir_all(&directory);
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
            the_mandate_of(&directory, "ttys001", "the-successor", "").expect("a mandate arrives");
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
            the_mandate_of(&directory, "ttys001", "another-successor", ""),
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

        assert_eq!(the_mandate_of(&directory, "ttys009", "whoever", ""), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// **A TTY NUMBER IS RECYCLED AND A SESSION IS NOT.** The lookup is keyed
    /// by the terminal, so a session handed that number later is told it is
    /// full on its predecessor's measurement. Seen in the store: a row of
    /// 345,732 tokens for a session of another tree, read by one holding 78k.
    #[test]
    fn a_session_that_takes_a_recycled_tty_does_not_inherit_the_ask_of_the_one_before() {
        let directory = std::env::temp_dir().join(format!(
            "sailor-recycled-tty-{}-{}",
            std::process::id(),
            sessions::now()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory of this test's own");
        let ledger = ledger::Ledger::open(&directory).expect("a store of this test's own");
        ledger
            .put_record(&ledger::StoreRecord {
                collection: ASKS.to_owned(),
                key: "ttys015".to_owned(),
                value: serde_json::json!({
                    "state": OBLIGE,
                    "tokens": 345_732,
                    "session": "the-session-that-was-measured",
                }),
                written_by: "the-run-that-measured-it".to_owned(),
                written_at: sessions::now(),
            })
            .expect("the ask is written");

        let mine = the_ask_of(&ledger, &directory, "ttys015", "the-session-that-was-measured", "")
            .expect("the session that was measured is told it is full");
        assert!(mine.contains("345732"), "{mine}");

        assert_eq!(
            the_ask_of(&ledger, &directory, "ttys015", "the-one-that-took-the-number-after", ""),
            None,
            "a later session on the same tty number inherits nothing of the one before"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// **AND THE MANDATE IS KEYED BY THE SAME RECYCLED NUMBER.** A mandate
    /// expects a session that is not its author, so the session cannot tell
    /// them apart — the tree can. Taken once, a mandate handed across trees is
    /// gone for the successor it was left for.
    #[test]
    fn a_mandate_is_not_handed_to_a_session_that_took_the_tty_in_another_tree() {
        let directory = std::env::temp_dir().join(format!(
            "sailor-mandate-tree-{}-{}",
            std::process::id(),
            sessions::now()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory of this test's own");
        let mut mandate = sessions::mandate::Mandate::default();
        mandate.written.tty = "ttys015".to_owned();
        mandate.written.tree = "/the/tree/it/was/written/in".to_owned();
        mandate.written.at = 100;
        mandate.work.goal = "swap the profile of a live session".to_owned();
        mandate.work.next = "consult the strong model before deciding".to_owned();
        sessions::mandate::deposit(&directory, &mandate).expect("the mandate is deposited");

        assert_eq!(
            the_mandate_of(&directory, "ttys015", "a-stranger", "/a/different/tree"),
            None,
            "a session that took the tty number in another tree is handed nothing"
        );
        let path = sessions::mandate::address_in(&directory, "ttys015");
        assert!(
            sessions::mandate::read(&path)
                .expect("the mandate is still there")
                .taken
                .is_none(),
            "refusing to hand it on must not consume it: it is still owed to its own tree"
        );

        let handed =
            the_mandate_of(&directory, "ttys015", "the-successor", "/the/tree/it/was/written/in")
                .expect("the successor in the mandate's own tree is handed it");
        assert!(handed.contains("swap the profile of a live session"), "{handed}");
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

        let asked = the_ask_of(&ledger, &directory, "ttys001", "whoever-is-here", "");
        assert!(
            asked.is_some_and(|said| said.contains("260000")),
            "unanswered, it is asked"
        );

        let mut mandate = sessions::mandate::Mandate::default();
        mandate.written.tty = "ttys001".to_owned();
        mandate.written.at = 101;
        sessions::mandate::deposit(&directory, &mandate).expect("the mandate is deposited");
        let waiting = the_ask_of(&ledger, &directory, "ttys001", "whoever-is-here", "")
            .expect("it says what stands now");
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
        let again = the_ask_of(&ledger, &directory, "ttys001", "whoever-is-here", "")
            .expect("it is asked again");
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

        assert_eq!(
            the_ask_of(&ledger, &directory, "ttys002", "whoever-is-here", ""),
            None
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}
