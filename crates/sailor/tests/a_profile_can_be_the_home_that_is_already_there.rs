//! **A PROFILE MADE FROM NOTHING IS A LOGGED-OUT AGENT.** Every profile on
//! this machine was a directory Sailor had created, empty of credentials, so
//! every engine lit under one started disconnected. Adoption takes the home
//! that already holds an account — read where it is, never copied.

use std::path::{Path, PathBuf};

fn a_tree(name: &str) -> PathBuf {
    let at = std::env::temp_dir().join(format!("adozione-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&at);
    std::fs::create_dir_all(&at).expect("the test tree");
    at
}

/// One test, because it moves `HOME` and the store's path for the whole
/// process: two of these in parallel would read each other's machine.
#[test]
fn the_home_already_on_the_machine_becomes_a_profile_and_nothing_is_written_into_it() {
    let machine = a_tree("casa");
    let store = machine.join("profili.json");
    std::env::set_var("HOME", &machine);
    std::env::set_var("PROFILES_STATE_PATH", &store);

    let already = machine.join(".codex");
    std::fs::create_dir_all(&already).expect("the home that is already there");
    std::fs::write(already.join("auth.json"), "{}").expect("what makes it worth adopting");
    let before = read_dir(&already);

    sailor::profiles_cmd::adopt("codex", &"la-mia".to_owned(), None).expect("it adopts");

    let read = profiles::store_io::load_store_from(&store).expect("the store");
    let row = read
        .profiles
        .iter()
        .find(|p| p.name == "la-mia")
        .expect("the row");
    assert_eq!(row.home_dir, already, "the profile is that home, not a copy");
    assert_eq!(read_dir(&already), before, "adoption wrote inside the home");

    // A HOME THAT IS NOT THERE IS A REFUSAL, and leaves the store as it was.
    let missing = machine.join("nessuna-casa");
    let refused = sailor::profiles_cmd::adopt("codex", &"assente".to_owned(), Some(&missing))
        .expect_err("a home nobody has is not adopted");
    assert!(refused.contains("nessuna-casa"), "the refusal names it: {refused}");
    let after = profiles::store_io::load_store_from(&store).expect("the store");
    assert_eq!(after.profiles.len(), 1, "a refusal recorded a profile");
    assert!(!missing.exists(), "the refusal created the home it refused");
}

fn read_dir(at: &Path) -> Vec<String> {
    let mut seen: Vec<String> = std::fs::read_dir(at)
        .expect("the home")
        .map(|entry| entry.expect("an entry").file_name().to_string_lossy().into_owned())
        .collect();
    seen.sort();
    seen
}
