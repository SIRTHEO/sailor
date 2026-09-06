//! Reading the store asks for no right to write it: every verb opened the
//! ledger read-write, so an agent in a sandbox that grants no writes was
//! answered «attempt to write a readonly database» instead of the memory.

use std::path::{Path, PathBuf};

/// Removed at the end even when the test fails: a read-only directory left
/// behind would refuse the next run.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("sailor-reader-{name}-{}", std::process::id()));
        let _ = make_writable(&path);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch directory");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = make_writable(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn permissions(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

fn make_writable(path: &Path) -> std::io::Result<()> {
    permissions(path, 0o755)
}

/// **THE MEASURE IS THE DIRECTORY, NOT THE FILE**: taking its write bit off is
/// what a sandbox does to a reader.
#[test]
fn a_store_whose_file_beside_it_cannot_be_made_says_which_half_refused() {
    let scratch = Scratch::new("read-only");
    let directory = scratch.0.join("ledger");
    {
        let ledger = ledger::Ledger::open(&directory).expect("a ledger to read afterwards");
        ledger.put_record(&entry("una-voce")).expect("one entry");
    }
    permissions(&directory, 0o555).expect("the directory refuses writes from here on");

    let refused = ledger::Ledger::open(&directory);
    assert!(
        refused.is_err(),
        "the read-write open must refuse here, or this test proves nothing"
    );

    // **WHAT IS CLOSED, AND WHAT IS NOT.** The reader no longer asks for the
    // right to write, which the next test measures; it still cannot conjure
    // the file a WAL store wants beside it. That half says so by name.
    let said = ledger::Ledger::open_for_reading(&directory)
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        said.contains("may not write the directory"),
        "the refusal must name the perimeter, not the store: {said}"
    );
}

/// The half the fault was about: **a reader asks for no right to write**, and
/// the handle is refused the moment it tries to record anything.
#[test]
fn a_reader_is_given_no_way_to_record() {
    let scratch = Scratch::new("no-writes");
    let directory = scratch.0.join("ledger");
    {
        let ledger = ledger::Ledger::open(&directory).expect("a ledger to read afterwards");
        ledger.put_record(&entry("una-voce")).expect("one entry");
    }

    let reading = ledger::Ledger::open_for_reading(&directory).expect("a reader is let in");
    let held = reading
        .records_in("una-collezione")
        .expect("the entries come back");
    assert_eq!(held.len(), 1, "what was written is read back: {held:?}");
    assert!(
        reading.put_record(&entry("un-altra")).is_err(),
        "a reader that could write would be the read-write handle under another name"
    );
}

fn entry(key: &str) -> ledger::StoreRecord {
    ledger::StoreRecord {
        collection: "una-collezione".to_owned(),
        key: key.to_owned(),
        value: serde_json::json!("written before the door was shut"),
        written_by: "this test".to_owned(),
        written_at: 0,
    }
}
