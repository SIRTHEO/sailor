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

const A_PAGE: &str =
    "# Faults still open\n\n| # | since | what goes wrong | status |\n|---|---|---|---|\n\n\
    **Zero open faults are described on this page; zero more are kept only in the fault store.**\n";

fn counted(store: &Faults) -> String {
    let stood = store.stood_now().expect("the moment");
    let then = store
        .as_it_stood(&stood)
        .expect("the store as it stood")
        .expect("a store this binary opened keeps its history");
    faults::the_page_from(A_PAGE, &then, &stood)
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
    let page = faults::the_page_from(A_PAGE, &reader.all().expect("the faults"), &stood);
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
fn the_line_is_written_once_under_the_count_and_read_back() {
    let (store, page, _) = a_store_and_its_page("line");
    let stood = faults::stood_in(&page).expect("the line is read back");
    assert_eq!(stood.through, 3);
    assert!(stood.change.is_some(), "{page}");

    let again = faults::stood_written_into(
        &format!("{page}\nAfter the count.\n"),
        &store.stood_now().expect("now"),
    );

    assert_eq!(
        again.matches("Counted from the fault store").count(),
        1,
        "{again}"
    );
    let lines: Vec<&str> = again.lines().collect();
    let sentence = lines
        .iter()
        .position(|line| faults::is_the_count_sentence(line))
        .expect("the count");
    assert_eq!(
        &lines[sentence + 1..sentence + 5],
        &["", lines[sentence + 2], "", "After the count."],
        "{again}"
    );
    assert!(faults::stood_in(lines[sentence + 2]).is_some(), "{again}");
    let older =
        "Counted from the fault store through fault 7, as it stood at 2026-09-01T00:00:00.000Z.";
    assert_eq!(faults::stood_in(older).map(|it| it.change), Some(None));
}
