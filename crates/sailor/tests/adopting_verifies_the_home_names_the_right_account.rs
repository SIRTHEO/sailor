//! **THE FAULT THIS TEST HOLDS SHUT.** A home adopted under a name is a home
//! nobody checked against what it actually says about itself. `adopt` is the
//! one moment a login and a registration meet, and refusing here costs
//! nothing — no row is written yet.

use std::path::PathBuf;

fn a_tree(name: &str) -> PathBuf {
    let at = std::env::temp_dir().join(format!("adozione-identita-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&at);
    std::fs::create_dir_all(&at).expect("the test tree");
    at
}

/// One test per process, for the reason the sibling file already gives: `HOME`
/// and the store's path are moved for the whole process.
#[test]
fn a_home_naming_another_account_is_refused_and_a_matching_one_is_adopted() {
    let machine = a_tree("casa");
    let store = machine.join("profili.json");
    std::env::set_var("HOME", &machine);
    std::env::set_var("PROFILES_STATE_PATH", &store);

    let mismatched = machine.join("mismatched-home");
    std::fs::create_dir_all(&mismatched).expect("the home");
    std::fs::write(
        mismatched.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"tools@example.com"}}"#,
    )
    .expect("the identity file");

    let refused = sailor::profiles_cmd::adopt(
        "claude",
        &"matteo19@example.com".to_owned(),
        Some(&mismatched),
    )
    .expect_err("a home naming another account is not adopted under this name");
    assert!(refused.contains("tools@example.com"), "{refused}");
    assert!(refused.contains("matteo19@example.com"), "{refused}");

    let after_refusal = profiles::store_io::load_store_from(&store).expect("the store");
    assert!(
        after_refusal.profiles.is_empty(),
        "a refusal registered a profile"
    );

    let matching = machine.join("matching-home");
    std::fs::create_dir_all(&matching).expect("the home");
    std::fs::write(
        matching.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"matteo19@example.com"}}"#,
    )
    .expect("the identity file");

    sailor::profiles_cmd::adopt("claude", &"matteo19@example.com".to_owned(), Some(&matching))
        .expect("a home naming the very account it is adopted under goes through");
    let after_adoption = profiles::store_io::load_store_from(&store).expect("the store");
    assert_eq!(after_adoption.profiles.len(), 1, "the matching home was not adopted");
}
