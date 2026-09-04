//! **THE ENGINES ARE NOT WRITTEN IN RUST.** Four of them were, with their home
//! and endpoint variables: a list of providers inside a product whose first
//! constraint is that it knows none. This is what the file must say instead.

use profiles::{known_clis, parse_command_lines, HomeMechanism, NativeProfiles};

#[test]
fn the_shipped_list_parses_and_every_entry_can_be_used() {
    let table = known_clis();
    assert!(!table.is_empty(), "the shipped list declares no command line");
    let mut ids: Vec<&str> = table.iter().map(|cli| cli.id.as_str()).collect();
    let counted = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), counted, "two entries carry the same id");
    for cli in table {
        assert!(!cli.executable.is_empty(), "{} names no executable", cli.id);
        assert!(!cli.display_name.is_empty(), "{} has no label", cli.id);
    }
}

/// **THE HOME IS WHAT A PROFILE IS**: where none moves, a profile switches
/// nothing and says so nowhere.
#[test]
fn at_least_one_engine_declares_how_its_home_moves() {
    assert!(known_clis()
        .iter()
        .any(|cli| matches!(cli.home, HomeMechanism::EnvVar(_))));
}

#[test]
fn a_home_declared_neither_way_is_no_known_way_and_keeps_its_note() {
    let table = parse_command_lines(
        r#"{"command_lines": [
             {"id": "un-motore", "executable": "unmotore",
              "home_note": "nobody found one"}
           ]}"#,
    )
    .expect("it parses");
    assert_eq!(table[0].home, HomeMechanism::Unknown);
    assert_eq!(table[0].home_note, "nobody found one");
    // A label nobody wrote is the id, never an empty line in a list.
    assert_eq!(table[0].display_name, "un-motore");
}

/// **A WORD NOBODY TAUGHT US IS «UNVERIFIED», NEVER «NO»**: a typo read as
/// «this engine does not do profiles» invents a measurement.
#[test]
fn an_unknown_word_for_native_profiles_is_read_as_unverified() {
    let table = parse_command_lines(
        r#"{"command_lines": [
             {"id": "uno", "executable": "uno", "native": "supported"},
             {"id": "due", "executable": "due", "native": "not supported"},
             {"id": "tre", "executable": "tre", "native": "Supported"},
             {"id": "quattro", "executable": "quattro"}
           ]}"#,
    )
    .expect("it parses");
    let said: Vec<&NativeProfiles> = table.iter().map(|cli| &cli.native_profiles).collect();
    assert_eq!(
        said,
        vec![
            &NativeProfiles::Supported,
            &NativeProfiles::NotSupported,
            &NativeProfiles::Unverified,
            &NativeProfiles::Unverified,
        ]
    );
}

#[test]
fn a_list_that_does_not_parse_says_so_instead_of_answering_half() {
    assert!(parse_command_lines("{ not json").is_err());
    // An empty list is a legitimate answer: a machine whose owner switched
    // every engine off has no command line to make a profile of.
    assert_eq!(parse_command_lines("{}").expect("it parses").len(), 0);
}

/// **A PROFILE CAN BE THE HOME THAT IS ALREADY THERE.** A home built from
/// nothing has no credentials, and every engine lit under it starts logged
/// out; the file says where each engine already keeps one, so the account a
/// person is logged into can be taken as it is instead of copied.
#[test]
fn an_engine_can_declare_the_home_it_already_keeps() {
    let table = parse_command_lines(
        r#"{"command_lines": [
             {"id": "un-motore", "executable": "unmotore",
              "home": {"variable": "UNMOTORE_HOME", "already_at": ".unmotore"}},
             {"id": "un-altro", "executable": "unaltro",
              "home": {"variable": "UNALTRO_HOME"}}
           ]}"#,
    )
    .expect("it parses");
    assert_eq!(
        profiles::existing_home(&table[0], std::path::Path::new("/una/casa")),
        Some(std::path::PathBuf::from("/una/casa/.unmotore"))
    );
    assert_eq!(
        profiles::existing_home(&table[1], std::path::Path::new("/una/casa")),
        None,
        "an engine nobody looked into offers no home to adopt"
    );
    assert!(
        known_clis()
            .iter()
            .any(|cli| cli.home_already_here.is_some()),
        "the shipped list declares no home anybody could adopt"
    );
}
