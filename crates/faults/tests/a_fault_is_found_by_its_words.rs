//! A fault is one document of the search, found by a word written in any of
//! its four columns and by nothing else.

use faults::{Draft, Faults};
use std::path::PathBuf;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "faults-search-{label}-{}-{}",
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

fn draft(what: &str, how: &str, prevent: &str, status: &str) -> Draft {
    Draft {
        happened_on: "01/01/2000".to_owned(),
        what_happened: what.to_owned(),
        how_it_showed: how.to_owned(),
        what_would_prevent: prevent.to_owned(),
        status: status.to_owned(),
    }
}

#[test]
fn a_word_in_any_column_of_one_fault_finds_that_fault_and_no_other() {
    let path = scratch("columns");
    let store = Faults::open(&path).expect("opening");
    let found = store
        .record(&draft(
            "the quokka got in",
            "seen as zwieback in the log",
            "a marzipan check at the door",
            "**aperto** capybara pending",
        ))
        .expect("recording");
    store
        .record(&draft(
            "a different fault",
            "seen by a person",
            "a test",
            "**chiuso** since the same day",
        ))
        .expect("recording the other");
    let documents = store.documents_to_search().expect("the documents");
    let ids = |word: &str| -> Vec<String> {
        ledger::search::rank_texts(&documents, word)
            .expect("a ranking")
            .into_iter()
            .map(|hit| hit.id)
            .collect()
    };
    let expected = vec![format!("fault:{}", found.number)];
    assert_eq!(ids("quokka"), expected, "what happened");
    assert_eq!(ids("zwieback"), expected, "how it showed");
    assert_eq!(ids("marzipan"), expected, "what would prevent");
    assert_eq!(ids("capybara"), expected, "status");
    assert!(ids("okapi").is_empty());
    let _ = std::fs::remove_dir_all(path.parent().expect("a parent"));
}
