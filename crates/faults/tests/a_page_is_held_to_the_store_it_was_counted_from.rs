//! The public page is frozen at a commit and the store keeps moving. The page
//! is compared with a fresh render of the store as it stood when the page was
//! counted, so a fault opened or closed since leaves it true, any line the
//! store would not give is named, and a moment the history does not reach is
//! never guessed.

use faults::{Draft, Faults, Held};
use std::path::PathBuf;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "faults-stood-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    dir.join(faults::FAULTS_FILE)
}

fn an_open_fault(what: &str) -> Draft {
    Draft {
        happened_on: "03/09".to_owned(),
        what_happened: what.to_owned(),
        how_it_showed: "reading it".to_owned(),
        what_would_prevent: "a check".to_owned(),
        status: "**open**".to_owned(),
        standing: None,
    }
}

fn counted(store: &Faults) -> String {
    let stood = store.stood_now().expect("the moment");
    let then = store
        .as_it_stood(&stood)
        .expect("the store as it stood")
        .expect("a store this binary opened keeps its history");
    faults::the_page(&then, &stood)
}

/// Three open faults, the first two summarised for users, and the page counted.
fn a_store_and_its_page(label: &str) -> (Faults, String, PathBuf) {
    let path = scratch(label);
    let store = Faults::open(&path).expect("the store");
    for what in ["the first", "the second", "the third"] {
        store.record(&an_open_fault(what)).expect("recorded");
    }
    store
        .set_public_summary(1, "The window forgets a flow.")
        .expect("a summary");
    store
        .set_public_summary(2, "A run stops without a reason.")
        .expect("a summary");
    let page = counted(&store);
    (store, page, path)
}

fn held(store: &Faults, page: &str) -> Held {
    let stood = faults::stood_in(page).expect("the page says what it was counted from");
    store
        .hold_the_page(page, &stood)
        .expect("held against the store")
}

fn differs_at(store: &Faults, page: &str, forged: &str, what: &str) {
    assert_ne!(forged, page, "the fixture's line moved: {what}");
    match held(store, forged) {
        Held::Differs(difference) => assert!(difference.line > 0, "{what}: {difference:?}"),
        other => panic!("{what} passed as {other:?}\n{forged}"),
    }
}

#[test]
fn a_fault_opened_after_the_count_leaves_the_page_true() {
    let (store, page, _) = a_store_and_its_page("opened");
    let fourth = store
        .record(&an_open_fault("the fourth"))
        .expect("recorded after the count");
    store
        .set_public_summary(fourth.number, "A fourth.")
        .expect("a summary");

    assert_eq!(
        store.still_open().expect("counted"),
        4,
        "the store moved on"
    );
    assert_eq!(held(&store, &page), Held::Agrees { open_then: 3 });
}

#[test]
fn a_fault_closed_after_the_count_is_counted_as_it_stood() {
    let (store, page, _) = a_store_and_its_page("closed");
    store
        .set_status(1, "**closed** on 04/09")
        .expect("closed after the count");
    store
        .set_status(3, "**closed** on 04/09")
        .expect("closed after the count");

    assert_eq!(
        store.still_open().expect("counted"),
        1,
        "the store moved on"
    );
    assert_eq!(held(&store, &page), Held::Agrees { open_then: 3 });
}

#[test]
fn a_standing_restored_after_the_count_is_counted_as_it_stood() {
    let (store, page, _) = a_store_and_its_page("restored");
    let mut older = store.get(2).expect("the second");
    older.status = "**closed** on 04/09".to_owned();
    store.restore(&older).expect("restored after the count");

    assert_eq!(held(&store, &page), Held::Agrees { open_then: 3 });
}

/// What a binary older than the history writes: plain SQL, through no road
/// of this crate. The store's own triggers write it down all the same.
#[test]
fn a_close_written_past_this_crate_is_counted_as_it_stood() {
    let (store, page, path) = a_store_and_its_page("older-binary");
    let older = rusqlite::Connection::open(&path).expect("a second writer");
    older
        .execute(
            "UPDATE faults SET status = '**closed** on 04/09', standing = 'closed' WHERE number = 1",
            [],
        )
        .expect("closed by plain SQL");
    older
        .execute(
            "INSERT OR REPLACE INTO public_summaries (number, summary) VALUES (2, 'Reworded.')",
            [],
        )
        .expect("reworded by plain SQL");

    assert_eq!(
        store.still_open().expect("counted"),
        2,
        "the store moved on"
    );
    assert_eq!(held(&store, &page), Held::Agrees { open_then: 3 });
}

#[test]
fn a_summary_rewritten_after_the_count_is_counted_as_it_stood() {
    let (store, page, _) = a_store_and_its_page("reworded");
    store
        .set_public_summary(1, "Worded otherwise.")
        .expect("reworded");
    store
        .set_public_summary(3, "Put on the page later.")
        .expect("summarised");

    assert_eq!(held(&store, &page), Held::Agrees { open_then: 3 });
}

/// `restore` and `import` put numbers back that are no higher than the stamp.
#[test]
fn a_fault_put_back_after_the_count_did_not_stand_then() {
    let path = scratch("put-back");
    let store = Faults::open(&path).expect("the store");
    let first = store.record(&an_open_fault("the first")).expect("recorded");
    store
        .restore(&faults::Fault {
            number: 3,
            ..first.clone()
        })
        .expect("a third");
    store
        .set_public_summary(1, "The first.")
        .expect("a summary");
    let page = counted(&store);

    store
        .restore(&faults::Fault { number: 2, ..first })
        .expect("put back under 2");
    store.set_public_summary(2, "Put back.").expect("a summary");

    assert_eq!(held(&store, &page), Held::Agrees { open_then: 2 });
}

#[test]
fn a_row_the_store_never_held_is_named() {
    let (store, page, _) = a_store_and_its_page("never-held");
    let forged = page.replace(
        "|---|---|---|---|\n",
        "|---|---|---|---|\n| 9 | 03/09 | Another. | **open** |\n",
    );
    differs_at(
        &store,
        &page,
        &forged,
        "a row under a number the store never gave",
    );
}

/// Fault 3 is open and summarised nowhere, so the render never puts it there.
#[test]
fn a_row_invented_under_a_number_the_store_holds_is_named() {
    let (store, page, _) = a_store_and_its_page("invented");
    let forged = page.replace(
        "|---|---|---|---|\n",
        "|---|---|---|---|\n| 3 | 03/09 | A row nobody recorded. | **open** |\n",
    );
    differs_at(&store, &page, &forged, "a row invented under 3");
}

#[test]
fn a_renumbered_row_is_named() {
    let (store, page, _) = a_store_and_its_page("renumbered");
    differs_at(
        &store,
        &page,
        &page.replace("| 2 | 03/09", "| 3 | 03/09"),
        "row 2 renumbered 3",
    );
}

#[test]
fn a_row_whose_text_was_replaced_is_named() {
    let (store, page, _) = a_store_and_its_page("replaced");
    let forged = page.replace(
        "A run stops without a reason.",
        "Invented text nobody wrote.",
    );
    differs_at(&store, &page, &forged, "row 2 with invented text");
}

#[test]
fn a_deleted_row_is_named() {
    let (store, page, _) = a_store_and_its_page("deleted");
    let forged: String = page
        .lines()
        .filter(|line| !line.starts_with("| 2 |"))
        .map(|line| format!("{line}\n"))
        .collect();
    differs_at(&store, &page, &forged, "row 2 deleted");
}

#[test]
fn a_count_edited_by_hand_is_named() {
    let (store, page, _) = a_store_and_its_page("edited");
    let forged = page.replace("one more is kept", "two more are kept");
    differs_at(&store, &page, &forged, "the count edited");
}

#[test]
fn a_row_closed_before_the_count_is_named() {
    let (store, _, _) = a_store_and_its_page("closed-before");
    store.set_status(2, "**closed** on 04/09").expect("closed");
    let page = counted(&store);
    let forged = page.replace(
        "|---|---|---|---|\n",
        "|---|---|---|---|\n| 2 | 03/09 | A run stops without a reason. | **open** |\n",
    );
    differs_at(&store, &page, &forged, "a row closed before the count");
}

/// A binary that writes no stamp refreshes the rows and keeps the old line.
#[test]
fn a_render_that_kept_an_older_stamp_is_named() {
    let (store, page, _) = a_store_and_its_page("stale-stamp");
    store
        .set_status(1, "**closed** on 04/09")
        .expect("closed after the count");
    let rendered_again = faults::render_open_into(&page, &store.all().expect("today"));
    differs_at(
        &store,
        &page,
        &rendered_again,
        "a fresh count under an old stamp",
    );
}

/// A store an older binary made: no history, and no trigger to write one.
fn without_its_history(path: &PathBuf) {
    rusqlite::Connection::open(path)
        .expect("a second writer")
        .execute_batch(
            "DROP TRIGGER a_fault_is_written; DROP TRIGGER a_fault_is_changed;
             DROP TRIGGER a_fault_is_taken_out; DROP TRIGGER a_summary_is_written;
             DROP TRIGGER a_summary_is_changed; DROP TRIGGER a_summary_is_taken_out;
             DROP TRIGGER a_fault_is_renumbered; DROP TRIGGER a_summary_is_renumbered;
             DROP TABLE store_history;",
        )
        .expect("the history taken away");
}

#[test]
fn a_page_counted_before_the_history_began_cannot_be_told() {
    let (store, _, path) = a_store_and_its_page("before-history");
    drop(store);
    without_its_history(&path);
    let reader = Faults::open_for_reading(&path).expect("read as an older store");
    let stood = reader.stood_now().expect("the moment");
    assert_eq!(
        stood.change, None,
        "a store with no history has no change to name"
    );
    let page = faults::the_page(&reader.all().expect("the faults"), &stood);
    assert_eq!(
        reader.hold_the_page(&page, &stood).expect("held"),
        Held::CannotTell {
            kept_since: None,
            rows: Vec::new()
        }
    );

    rusqlite::Connection::open(&path)
        .expect("an older binary")
        .execute("UPDATE faults SET standing = 'closed' WHERE number = 1", [])
        .expect("closed where nothing writes it down");
    let store = Faults::open(&path).expect("a binary that keeps the history");
    match store.hold_the_page(&page, &stood).expect("held") {
        Held::CannotTell {
            kept_since: Some(_),
            rows,
        } => assert_eq!(rows, vec![1]),
        other => panic!("a moment before the history was answered: {other:?}"),
    }
}

#[test]
fn the_page_ends_with_its_count_and_one_stamp() {
    let (_store, page, _) = a_store_and_its_page("line");
    let stood = faults::stood_in(&page).expect("the line is read back");
    assert_eq!(stood.through, 3);
    assert!(stood.change.is_some(), "{page}");
    assert!(page.starts_with(faults::PAGE_OPENING), "{page}");
    let lines: Vec<&str> = page.lines().collect();
    assert!(
        faults::is_the_count_sentence(lines[lines.len() - 3]),
        "{page}"
    );
    assert_eq!(
        (lines[lines.len() - 2], lines[lines.len() - 1]),
        ("", faults::stood_line(&stood).as_str())
    );
    assert!(page.ends_with(".\n"), "{page}");
    assert_eq!(page.matches("Counted from the fault store").count(), 1);
    let older =
        "Counted from the fault store through fault 7, as it stood at 2026-09-01T00:00:00.000Z.";
    assert_eq!(faults::stood_in(older).map(|it| it.change), Some(None));
}

/// Close, reword, reopen: only the first change after the count says what stood.
#[test]
fn a_fault_changed_again_after_the_count_is_counted_as_it_first_stood() {
    let (store, page, _) = a_store_and_its_page("changed-again");
    store.set_status(1, "**closed** on 04/09").expect("closed");
    let mut reworded = store.get(1).expect("the first");
    reworded.what_happened = "Worded otherwise.".to_owned();
    store.restore(&reworded).expect("reworded");
    store.set_status(1, "**open**").expect("reopened");
    for summary in ["Said once more.", "Said a third time."] {
        store.set_public_summary(2, summary).expect("resummarised");
    }

    assert_eq!(held(&store, &page), Held::Agrees { open_then: 3 });
}

/// No road of this crate deletes a fault; plain SQL can.
#[test]
fn a_fault_deleted_after_the_count_comes_back_with_its_summary() {
    let (store, page, path) = a_store_and_its_page("deleted-after");
    rusqlite::Connection::open(&path)
        .expect("a second writer")
        .execute_batch(
            "DELETE FROM faults WHERE number = 1;
             DELETE FROM public_summaries WHERE number = 2;
             DELETE FROM faults WHERE number = 2;",
        )
        .expect("deleted by plain SQL");

    assert_eq!(held(&store, &page), Held::Agrees { open_then: 3 });
}

/// No road of this crate renumbers a fault or a summary; plain SQL can.
#[test]
fn a_fault_renumbered_after_the_count_stood_under_its_old_number() {
    let (store, page, path) = a_store_and_its_page("renumbered-after");
    rusqlite::Connection::open(&path)
        .expect("a second writer")
        .execute("UPDATE faults SET number = 400 WHERE number = 2", [])
        .expect("renumbered by plain SQL");

    assert_eq!(held(&store, &page), Held::Agrees { open_then: 3 });
}

#[test]
fn a_summary_renumbered_after_the_count_stood_under_its_old_number() {
    let (store, page, path) = a_store_and_its_page("summary-renumbered-after");
    rusqlite::Connection::open(&path)
        .expect("a second writer")
        .execute(
            "UPDATE public_summaries SET number = 3 WHERE number = 1",
            [],
        )
        .expect("renumbered by plain SQL");

    assert_eq!(held(&store, &page), Held::Agrees { open_then: 3 });
}

/// `OR REPLACE` deletes the row under the new number and fires no delete.
#[test]
fn a_fault_renumbered_onto_a_held_number_after_the_count_stood_as_both() {
    let (store, _, path) = a_store_and_its_page("replaced-after");
    let writer = rusqlite::Connection::open(&path).expect("a second writer");
    writer
        .execute(
            "UPDATE faults SET happened_on = '04/09' WHERE number = 3",
            [],
        )
        .expect("the mover dated apart from the fault it lands on");
    let page = counted(&store);
    writer
        .execute(
            "UPDATE OR REPLACE faults SET number = 1 WHERE number = 3",
            [],
        )
        .expect("renumbered onto fault 1 by plain SQL");

    assert_eq!(held(&store, &page), Held::Agrees { open_then: 3 });
}

#[test]
fn a_summary_renumbered_onto_a_held_number_after_the_count_stood_as_both() {
    let (store, page, path) = a_store_and_its_page("summary-replaced-after");
    rusqlite::Connection::open(&path)
        .expect("a second writer")
        .execute(
            "UPDATE OR REPLACE public_summaries SET number = 1 WHERE number = 2",
            [],
        )
        .expect("renumbered onto summary 1 by plain SQL");

    assert_eq!(held(&store, &page), Held::Agrees { open_then: 3 });
}

fn changes_kept(path: &PathBuf) -> i64 {
    rusqlite::Connection::open(path)
        .expect("a reader")
        .query_row("SELECT COUNT(*) FROM store_history", [], |row| row.get(0))
        .expect("the history counted")
}

#[test]
fn an_update_that_changes_nothing_writes_no_history() {
    let (_store, _, path) = a_store_and_its_page("no-change");
    let before = changes_kept(&path);
    let writer = rusqlite::Connection::open(&path).expect("a second writer");
    writer
        .execute_batch(
            "UPDATE faults SET status = status, standing = standing;
             UPDATE public_summaries SET summary = summary;",
        )
        .expect("updated to what it was");
    assert_eq!(
        changes_kept(&path),
        before,
        "an update that changed nothing"
    );

    writer
        .execute_batch(
            "UPDATE faults SET status = '**closed** on 04/09' WHERE number = 1;
             UPDATE public_summaries SET summary = 'Otherwise.' WHERE number = 2;",
        )
        .expect("changed");
    assert_eq!(changes_kept(&path), before + 2, "two changes");
}

fn restamped(page: &str, stamp: impl FnOnce(faults::Stood) -> faults::Stood) -> String {
    let stood = faults::stood_in(page).expect("the page's stamp");
    page.replace(
        &faults::stood_line(&stood),
        &faults::stood_line(&stamp(stood.clone())),
    )
}

fn cannot_tell(store: &Faults, page: &str, what: &str) {
    match held(store, page) {
        Held::CannotTell { .. } => {}
        other => panic!("{what} was answered as {other:?}"),
    }
}

/// Rows written in the same millisecond as a stamp read as written at it.
fn a_millisecond_passes() {
    std::thread::sleep(std::time::Duration::from_millis(3));
}

/// The page was counted from a copy that kept a history; the store it is
/// held to began its own later, and its numbers mean other changes.
#[test]
fn a_history_begun_after_the_count_cannot_tell() {
    let (store, page, path) = a_store_and_its_page("begun-after");
    let stamped = faults::stood_in(&page)
        .and_then(|it| it.change)
        .expect("a change");
    drop(store);
    without_its_history(&path);
    a_millisecond_passes();
    let store = Faults::open(&path).expect("a newer binary opens it later");
    while changes_kept(&path) <= stamped {
        store.set_status(1, "**closed** on 04/09").expect("closed");
        store.set_status(1, "**open**").expect("reopened");
    }

    cannot_tell(&store, &page, "a history begun after the stamp");
}

#[test]
fn a_stamp_moved_into_the_past_cannot_tell() {
    let (store, page, _) = a_store_and_its_page("moved-back");
    let forged = restamped(&page, |stood| faults::Stood {
        at: "2020-01-01T00:00:00.000Z".to_owned(),
        ..stood
    });
    cannot_tell(&store, &forged, "a stamp moved to 2020");
}

/// A clock set back makes a history whose start is later than its rows.
#[test]
fn a_history_that_says_it_began_after_the_stamp_is_not_read() {
    let (store, page, path) = a_store_and_its_page("kept-later");
    rusqlite::Connection::open(&path)
        .expect("a second writer")
        .execute(
            "UPDATE store_history SET at = '2999-01-01T00:00:00.000Z' WHERE what = 'kept'",
            [],
        )
        .expect("the start moved on");
    cannot_tell(
        &store,
        &page,
        "a history begun after the stamp by its own word",
    );
}

#[test]
fn a_stamp_naming_a_change_the_history_never_held_cannot_tell() {
    let (store, page, _) = a_store_and_its_page("never-changed");
    let forged = restamped(&page, |stood| faults::Stood {
        change: Some(999),
        ..stood
    });
    cannot_tell(&store, &forged, "change 999");
}

#[test]
fn a_stamp_naming_a_change_written_after_it_cannot_tell() {
    let (store, page, _) = a_store_and_its_page("later-change");
    a_millisecond_passes();
    store.set_status(1, "**closed** on 04/09").expect("closed");
    let forged = restamped(&page, |stood| faults::Stood {
        change: stood.change.map(|change| change + 1),
        ..stood
    });
    cannot_tell(&store, &forged, "a change written after the stamp");
}

#[test]
fn a_stamp_naming_an_earlier_change_cannot_tell() {
    let (store, _, _) = a_store_and_its_page("earlier-change");
    a_millisecond_passes();
    let page = counted(&store);
    let forged = restamped(&page, |stood| faults::Stood {
        change: stood.change.map(|change| change - 1),
        ..stood
    });
    cannot_tell(
        &store,
        &forged,
        "a change followed by another before the stamp",
    );
}

/// Each shape a page took that a reader of rows let through, so that GFM
/// showed a reader a fault the store never held, or hid the ones it holds.
fn forgeries(page: &str) -> Vec<(&'static str, String)> {
    let invented = "999 | 23/09 | Invented. | **open** |";
    let last_row = page
        .lines()
        .rfind(|line| line.starts_with("| 2 |"))
        .expect("the last row");
    let under_the_rows =
        |row: &str| page.replace(&format!("{last_row}\n"), &format!("{last_row}\n{row}\n"));
    let count = page
        .lines()
        .find(|line| faults::is_the_count_sentence(line))
        .expect("the count");
    let stamp = page
        .lines()
        .find(|line| faults::stood_in(line).is_some())
        .expect("the stamp");
    vec![
        ("A1 a row with no leading bar", under_the_rows(invented)),
        (
            "A2 and no trailing bar",
            under_the_rows(invented.trim_end_matches(" |")),
        ),
        (
            "A3 a bar after an invisible letter",
            under_the_rows(&format!("\u{200b}| {invented}")),
        ),
        (
            "A14 an invisible letter before the number",
            under_the_rows(&format!("\u{200b}{invented}")),
        ),
        (
            "A7 a second table in a quote",
            format!(
                "{page}\n> {}\n> |---|---|---|---|\n> | {invented}\n",
                faults::PUBLIC_HEADER
            ),
        ),
        (
            "A8 a second table with no bars at its ends",
            format!("{page}\n# | since | what goes wrong | status\n---|---|---|---\n{invented}\n"),
        ),
        (
            "A11 a table in HTML",
            format!("{page}\n<table><tr><td>999</td><td>Invented.</td></tr></table>\n"),
        ),
        (
            "B1 a second count sentence",
            format!(
                "{page}\n**Thirty open faults are described on this page; zero more are kept \
                 only in the fault store**.\n"
            ),
        ),
        (
            "B2 the count and stamp in a comment, others shown",
            page.replace(&format!("{count}\n"), &format!("<!--\n{count}\n"))
                + &format!(
                    "-->\n**Twenty open faults are described on this page; ninety more are \
                     kept only in the fault store.**\n\n{}\n",
                    stamp.trim_end_matches('.')
                ),
        ),
        (
            "C1 the whole table in a comment, a forged one shown",
            page.replace(
                &format!("{}\n", faults::PUBLIC_HEADER),
                &format!("<!--\n{}\n", faults::PUBLIC_HEADER),
            ) + &format!(
                "-->\n# | since | what goes wrong | status\n---|---|---|---\n{invented}\n\n\
                 **One open fault is described on this page; zero more are kept only in the \
                 fault store.**\n"
            ),
        ),
        (
            "a row with a space after it",
            page.replace(&format!("{last_row}\n"), &format!("{last_row} \n")),
        ),
        (
            "a row written without spaces",
            page.replace(last_row, &last_row.replace(" | ", "|")),
        ),
        (
            "a status the render never writes",
            page.replace(
                last_row,
                &last_row.replace("**open**", "**open** since the start"),
            ),
        ),
        (
            "a closed row",
            page.replace(last_row, &last_row.replace("**open**", "**closed**")),
        ),
        (
            "a count of the rows that is wrong",
            page.replace(
                count,
                &count.replace("Two open faults", "Three open faults"),
            ),
        ),
        ("a page whose lines end in CRLF", page.replace('\n', "\r\n")),
    ]
}

/// The first line, counted from one, where the forged page's bytes leave the page's.
fn where_it_parts(page: &str, forged: &str) -> usize {
    let (page, forged): (Vec<&str>, Vec<&str>) = (
        page.split_inclusive('\n').collect(),
        forged.split_inclusive('\n').collect(),
    );
    (0..)
        .find(|&at| page.get(at) != forged.get(at))
        .expect("the two differ")
        + 1
}

#[test]
fn every_forgery_is_named_at_the_line_it_parts_from_the_render() {
    let (store, page, _) = a_store_and_its_page("forgeries");
    for (what, forged) in forgeries(&page) {
        assert_ne!(forged, page, "the fixture moved: {what}");
        match held(&store, &forged) {
            Held::Differs(difference) => {
                assert_eq!(difference.line, where_it_parts(&page, &forged), "{what}")
            }
            other => panic!("{what} passed as {other:?}\n{forged}"),
        }
    }
}

#[test]
fn a_stamp_moved_into_the_future_cannot_tell() {
    let (store, page, _) = a_store_and_its_page("future");
    let forged = restamped(&page, |stood| faults::Stood {
        at: "2099-01-01T00:00:00.000Z".to_owned(),
        ..stood
    });
    cannot_tell(&store, &forged, "a stamp moved to 2099");
}

#[test]
fn a_stamp_with_words_after_its_instant_cannot_tell() {
    let (store, page, _) = a_store_and_its_page("trailing");
    let forged = restamped(&page, |stood| faults::Stood {
        at: format!("{} and later", stood.at),
        ..stood
    });
    a_millisecond_passes();
    cannot_tell(&store, &forged, "an instant with words after it");
}

fn named_where_it_parts(store: &Faults, page: &str, forged: &str, what: &str) {
    assert_ne!(forged, page, "the fixture moved: {what}");
    match held(store, forged) {
        Held::Differs(difference) => {
            assert_eq!(difference.line, where_it_parts(page, forged), "{what}")
        }
        other => panic!("{what} passed as {other:?}\n{forged}"),
    }
}

fn under_the_last_row(page: &str, rows: &str) -> String {
    let last_row = page
        .lines()
        .rfind(|line| line.starts_with("| 2 |"))
        .expect("the last row");
    page.replace(&format!("{last_row}\n"), &format!("{last_row}\n{rows}"))
}

#[test]
fn a_row_after_a_spacer_row_is_named() {
    let (store, page, _) = a_store_and_its_page("spacer");
    let forged = under_the_last_row(
        &page,
        "| — | | | |\n| 998 | 23/09 | Invented. | **open** |\n",
    );
    named_where_it_parts(&store, &page, &forged, "a spacer row");
}

#[test]
fn a_row_whose_number_is_not_a_number_is_named() {
    let (store, page, _) = a_store_and_its_page("hashed");
    let forged = under_the_last_row(&page, "| #999 | 23/09 | Invented. | **open** |\n");
    named_where_it_parts(&store, &page, &forged, "a row numbered #999");
}

#[test]
fn a_row_written_without_spaces_after_an_empty_row_is_named() {
    let (store, page, _) = a_store_and_its_page("packed");
    let forged = under_the_last_row(
        &page,
        "| | | | |\n|999|23/09|Invented, written without spaces.|**open**|\n",
    );
    named_where_it_parts(&store, &page, &forged, "an empty row");
}

#[test]
fn a_second_table_after_the_stamp_is_named() {
    let (store, page, _) = a_store_and_its_page("second-table");
    let forged = format!(
        "{page}\n| # | since | what goes wrong | status |\n|---|---|---|---|\n\
         | 999 | 23/09 | Invented. | **open** |\n"
    );
    named_where_it_parts(&store, &page, &forged, "a second table");
}

#[test]
fn a_second_count_sentence_is_named() {
    let (store, page, _) = a_store_and_its_page("second-count");
    let forged = format!(
        "{page}\n**Thirty open faults are described on this page; zero more are kept only in \
         the fault store.**\n"
    );
    named_where_it_parts(&store, &page, &forged, "a second count sentence");
}

/// A page the crate would write, over a stamp that names a lower `through`
/// than the store held: the stamp line is where it is named.
fn named_at_the_stamp(store: &Faults, forged: &str, what: &str) {
    let stamp = forged
        .lines()
        .position(|line| faults::stood_in(line).is_some())
        .expect("the stamp")
        + 1;
    assert!(
        faults::held_to_the_template(forged).is_ok(),
        "{what}: {forged}"
    );
    match held(store, forged) {
        Held::Differs(difference) => assert_eq!(difference.line, stamp, "{what}"),
        other => panic!("{what} passed as {other:?}\n{forged}"),
    }
}

fn the_store_up_to(store: &Faults, through: i64) -> Vec<faults::Fault> {
    store
        .all()
        .expect("the faults")
        .into_iter()
        .filter(|fault| fault.number <= through)
        .collect()
}

#[test]
fn a_page_with_no_row_stamped_through_fault_zero_is_named() {
    let (store, page, _) = a_store_and_its_page("through-zero");
    let stood = faults::stood_in(&page).expect("the stamp");
    let forged = faults::the_page(
        &[],
        &faults::Stood {
            through: 0,
            ..stood
        },
    );
    match held(&store, &forged) {
        Held::Differs(difference) => assert_eq!(
            difference.line,
            forged.lines().count(),
            "named at the stamp"
        ),
        other => panic!("a page with nothing open passed as {other:?}\n{forged}"),
    }
}

/// Faults 1 and 2 are on the page and 3 is kept only in the store.
#[test]
fn a_page_stamped_through_a_lower_fault_is_named() {
    let (store, page, _) = a_store_and_its_page("through-lower");
    let stood = faults::stood_in(&page).expect("the stamp");
    let lower = faults::Stood {
        through: 2,
        ..stood
    };
    let forged = faults::the_page(&the_store_up_to(&store, 2), &lower);
    assert!(forged.contains("zero more"), "{forged}");
    named_at_the_stamp(&store, &forged, "a stamp through 2 hiding fault 3");
}

#[test]
fn a_wrong_more_under_a_lower_through_is_named() {
    let (store, page, _) = a_store_and_its_page("wrong-more");
    let stood = faults::stood_in(&page).expect("the stamp");
    let forged = page
        .replace("one more is kept", "zero more are kept")
        .replace(
            &faults::stood_line(&stood),
            &faults::stood_line(&faults::Stood {
                through: 2,
                ..stood.clone()
            }),
        );
    named_at_the_stamp(
        &store,
        &forged,
        "the rows kept, the more and through lowered",
    );
}
