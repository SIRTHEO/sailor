//! The public page is frozen at a commit and the store keeps moving. The page
//! is compared with the store as it stood when the page was counted, so a
//! fault opened or closed since leaves it true, and a row the store never held
//! is still found.

use faults::{Draft, Drift, Faults};
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

/// Three open faults, the first summarised for users, and the page counted.
fn a_store_and_its_page(label: &str) -> (Faults, String) {
    let store = Faults::open(scratch(label)).expect("the store");
    for what in ["the first", "the second", "the third"] {
        store.record(&an_open_fault(what)).expect("recorded");
    }
    store
        .set_public_summary(1, "The window forgets a flow.")
        .expect("a summary");
    let stood = store.stood_now().expect("the moment");
    let then = store.as_it_stood(&stood).expect("the store as it stood");
    let page = faults::stood_written_into(&faults::render_open_into(A_PAGE, &then), &stood);
    // The store's clock counts milliseconds: what follows lands after the count.
    std::thread::sleep(std::time::Duration::from_millis(5));
    (store, page)
}

fn drifts(store: &Faults, page: &str) -> Vec<Drift> {
    let stood = faults::stood_in(page).expect("the page says what it was counted from");
    faults::how_the_page_drifts(
        page,
        &store.as_it_stood(&stood).expect("the store as it stood"),
    )
}

#[test]
fn a_fault_opened_after_the_count_leaves_the_page_true() {
    let (store, page) = a_store_and_its_page("opened");
    store
        .record(&an_open_fault("the fourth"))
        .expect("recorded after the count");

    assert_eq!(
        store.still_open().expect("counted"),
        4,
        "the store moved on"
    );
    assert_eq!(
        drifts(&store, &page),
        Vec::new(),
        "a fault opened after the count moved the page"
    );
}

#[test]
fn a_fault_closed_after_the_count_is_counted_as_it_stood() {
    let (store, page) = a_store_and_its_page("closed");
    store
        .set_status(1, "**closed** on 04/09")
        .expect("closed after the count");
    store
        .set_status(2, "**closed** on 04/09")
        .expect("closed after the count");

    assert_eq!(
        store.still_open().expect("counted"),
        1,
        "the store moved on"
    );
    assert_eq!(
        drifts(&store, &page),
        Vec::new(),
        "a fault closed after the count moved the page"
    );
}

#[test]
fn a_standing_restored_after_the_count_is_counted_as_it_stood() {
    let (store, page) = a_store_and_its_page("restored");
    let mut older = store.get(3).expect("the third");
    older.status = "**closed** on 04/09".to_owned();
    store.restore(&older).expect("restored after the count");

    assert_eq!(
        drifts(&store, &page),
        Vec::new(),
        "a standing restored after the count moved the page"
    );
}

/// The drift the step was written for: a row added to the page and never to
/// the store.
#[test]
fn a_row_the_store_never_held_is_drift() {
    let (store, page) = a_store_and_its_page("never-held");
    let forged = page.replace(
        "|---|---|---|---|\n",
        "|---|---|---|---|\n| 2 | 03/09 | A row nobody recorded. | **open** |\n| 9 | 03/09 | Another. | **open** |\n",
    );

    assert_eq!(drifts(&store, &forged), vec![Drift::NotInTheStore(9)]);
}

#[test]
fn a_row_closed_before_the_count_is_drift() {
    let (store, page) = a_store_and_its_page("closed-before");
    let stale = page.replace(
        "|---|---|---|---|\n",
        "|---|---|---|---|\n| 3 | 03/09 | Closed already. | **open** |\n",
    );
    store.set_status(3, "**closed** on 04/09").expect("closed");
    let stood = faults::Stood {
        through: 3,
        at: store.stood_now().expect("now").at,
    };
    let then = store.as_it_stood(&stood).expect("the store as it stood");

    assert!(faults::how_the_page_drifts(&stale, &then).contains(&Drift::NotOpenThen(3)));
}

#[test]
fn a_count_edited_by_hand_is_drift() {
    let (store, page) = a_store_and_its_page("edited");
    let edited = page.replace("two more are kept", "three more are kept");
    assert_ne!(edited, page, "the fixture's sentence moved");

    assert_eq!(
        drifts(&store, &edited),
        vec![Drift::CountDiffers { page: 4, store: 3 }]
    );
}

#[test]
fn the_line_is_written_once_under_the_count_and_read_back() {
    let (store, page) = a_store_and_its_page("line");
    let stood = faults::stood_in(&page).expect("the line is read back");
    assert_eq!(stood.through, 3);

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
}
