//! **THE ENGINES ARE NOT WRITTEN IN RUST.** Four of them were, with their home
//! and endpoint variables: a list of providers inside a product whose first
//! constraint is that it knows none. This is what the file must say instead.

use profiles::{instruction_files, known_clis, parse_command_lines, HomeMechanism, NativeProfiles};
use std::path::{Path, PathBuf};

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

/// **A FIELD NOBODY DECLARED IS REFUSED, AND THE REFUSAL NAMES THE ENGINE.**
/// A misspelt key read as «absent» would turn a declared list of files into
/// «nobody looked», silently; a value of the wrong shape the same. Both are
/// refused as a whole, and the id is what a person greps their file for.
#[test]
fn a_field_nobody_declared_or_of_the_wrong_shape_is_refused_with_the_engines_name() {
    let refused = |entry: &str| {
        let text = format!(r#"{{"command_lines": [{entry}]}}"#);
        parse_command_lines(&text).err().unwrap_or_else(|| panic!("{entry} was accepted"))
    };
    for entry in [
        r#"{"id": "un-motore", "executable": "unmotore", "reads_instruction_from": ["AGENTS.md"]}"#,
        r#"{"id": "un-motore", "executable": "unmotore", "reads_instructions_from": "AGENTS.md"}"#,
        r#"{"id": "un-motore", "executable": "unmotore", "reads_instructions_from": ["AGENTS.md", 3]}"#,
        r#"{"id": "un-motore", "executable": "unmotore", "reads_instructions_from": ["/etc/AGENTS.md"]}"#,
        r#"{"id": "un-motore", "executable": "unmotore", "reads_instructions_from": [""]}"#,
        r#"{"id": "un-motore", "executable": "unmotore", "home": {"variabile": "X"}}"#,
    ] {
        let why = refused(entry);
        assert!(why.contains("un-motore"), "the refusal of {entry} does not name the engine: {why}");
    }
    // The absurd control: the same entry, spelt right, is taken.
    let taken = parse_command_lines(
        r#"{"command_lines": [
             {"id": "un-motore", "executable": "unmotore",
              "reads_instructions_from": ["~/.unmotore/RULES.md", "RULES.md"]}
           ]}"#,
    )
    .expect("it parses");
    assert_eq!(taken[0].reads_instructions_from, vec!["~/.unmotore/RULES.md", "RULES.md"]);
}

/// **AN ENGINE WHOSE HOME IS KNOWN SAYS WHAT IT READS AT ITS START.** It is
/// how a person learns whether the page of memories reaches it. An engine
/// nobody has looked into is allowed its silence, and is told apart by having
/// no home declared either.
#[test]
fn every_shipped_engine_with_a_home_says_what_it_reads_at_its_start() {
    let looked_into: Vec<_> = known_clis()
        .iter()
        .filter(|cli| cli.home != HomeMechanism::Unknown)
        .collect();
    assert!(!looked_into.is_empty(), "no shipped engine declares a home");
    for cli in looked_into {
        assert!(
            !cli.reads_instructions_from.is_empty(),
            "{} declares a home and not what it reads at its start",
            cli.id
        );
    }
}

/// **THE FILE IS WHERE THE ENGINE IS STARTED**: `~` is the home, a bare name
/// is under the project, and a profile that moves the engine's home by
/// variable takes the file in its usual place along. A profile that swaps a
/// symlink moves no home, and the file stays where it was.
#[test]
fn the_files_an_engine_reads_are_resolved_where_it_is_started() {
    let table = parse_command_lines(
        r#"{"command_lines": [
             {"id": "un-motore", "executable": "unmotore",
              "home": {"variable": "UNMOTORE_HOME", "already_at": ".unmotore"},
              "reads_instructions_from": ["~/.unmotore/RULES.md", "~/RULES.md", "RULES.md"]},
             {"id": "un-altro", "executable": "unaltro",
              "home": {"credential_symlink": "auth.json", "already_at": ".unaltro"},
              "reads_instructions_from": ["~/.unaltro/RULES.md"]}
           ]}"#,
    )
    .expect("it parses");
    let project = Path::new("/work/tree");
    let home = Path::new("/home/someone");
    assert_eq!(
        instruction_files(&table[0], project, home, None),
        vec![
            PathBuf::from("/home/someone/.unmotore/RULES.md"),
            PathBuf::from("/home/someone/RULES.md"),
            PathBuf::from("/work/tree/RULES.md"),
        ]
    );
    assert_eq!(
        instruction_files(&table[0], project, home, Some(Path::new("/homes/unmotore/work"))),
        vec![
            PathBuf::from("/homes/unmotore/work/RULES.md"),
            PathBuf::from("/home/someone/RULES.md"),
            PathBuf::from("/work/tree/RULES.md"),
        ]
    );
    assert_eq!(
        instruction_files(&table[1], project, home, Some(Path::new("/homes/unaltro/work"))),
        vec![PathBuf::from("/home/someone/.unaltro/RULES.md")]
    );
}
