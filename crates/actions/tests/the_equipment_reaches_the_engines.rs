//! **FAULT 18, PROVED WHERE IT LIVED.**
//!
//! The environment overlay that takes an engine into Sailor's home —
//! `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `GEMINI_CLI_HOME` — was called from **one
//! place**: `sailor run`, the quick exchange from a terminal. An engine launched
//! by a **flow step** never went through it: it inherited the environment of
//! whoever opened the terminal and read the neighbour's home. Two runs of one
//! flow, launched from two terminals, were not the same measure — and nothing
//! said so.
//!
//! **THE SAME DISEASE AS FAULT 35**, and the two proofs read together: Sailor
//! held the fact in its own home and did not use it. The price list was there
//! and did not travel with the product; the equipment was there and never
//! reached the engines.

use actions::{equipment_asking_for, equipment_for, equipment_with_keys_and_disk};
use ledger::EngineIdentity;
use profiles::{Profile, ProfileEndpoint, ProfileStore};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// State with one active profile for `codex` and a dormant one for `claude`:
/// the second proves that *existing* is not enough, a profile has to be active.
fn a_store_with_one_active_profile() -> ProfileStore {
    let mut store = ProfileStore::default();
    store.profiles.push(Profile {
        name: "lavoro".to_owned(),
        cli_id: "codex".to_owned(),
        home_dir: PathBuf::from("/case/codex/lavoro"),
        endpoint: None,
    });
    store.profiles.push(Profile {
        name: "riposo".to_owned(),
        cli_id: "claude".to_owned(),
        home_dir: PathBuf::from("/case/claude/riposo"),
        endpoint: None,
    });
    store.profiles.push(Profile {
        name: "riposo-codex".to_owned(),
        cli_id: "codex".to_owned(),
        home_dir: PathBuf::from("/case/codex/riposo"),
        endpoint: None,
    });
    store.active.insert("codex".to_owned(), "lavoro".to_owned());
    store
}

fn step_env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

/// **A CALL CAN NAME THE ACCOUNT IT RUNS ON, AND IT IS OBEYED.** Fault 165:
/// five accounts out of seven had never answered anything, because the active
/// profile is one per command line and nothing could ask for another.
#[test]
fn a_call_that_names_an_account_runs_in_that_account_and_not_the_active_one() {
    let store = a_store_with_one_active_profile();

    let signed_in = |_: &std::path::Path| true;
    let active =
        equipment_with_keys_and_disk(&store, "codex", &BTreeMap::new(), &|_| None, &signed_in, None);
    let asked = equipment_with_keys_and_disk(
        &store,
        "codex",
        &BTreeMap::new(),
        &|_| None,
        &signed_in,
        Some("riposo-codex"),
    );

    assert!(
        matches!(active.identity, EngineIdentity::ProfileInForce { ref profile_name, .. } if profile_name == "lavoro"),
        "the control: with nothing asked the active profile answers: {:?}",
        active.identity
    );
    assert!(
        matches!(asked.identity, EngineIdentity::ProfileInForce { ref profile_name, .. } if profile_name == "riposo-codex"),
        "the account asked for is the one that answers: {:?}",
        asked.identity
    );
    assert_eq!(asked.refused, None, "it is a real account of this machine");
    assert_ne!(
        active.env.get("CODEX_HOME"),
        asked.env.get("CODEX_HOME"),
        "and the two do not share a home, or nothing moved"
    );
}

/// **AN ACCOUNT THAT IS NOT HERE REFUSES THE CALL.** Falling back to whichever
/// profile happens to be active would spend another person's quota under the
/// name of the one asked for.
#[test]
fn a_call_naming_an_account_this_machine_does_not_have_is_refused() {
    let store = a_store_with_one_active_profile();

    let asked = equipment_asking_for(&store, "codex", &BTreeMap::new(), Some("nessuno"));

    let why = asked.refused.expect("a name that resolves to nothing is refused");
    assert!(why.contains("nessuno"), "the refusal names it: {why}");
}

/// **A HOME NOTHING MOVES IS NOT A HOME NOBODY LOOKED AT.** The shipped list
/// declares one command line with no way to move its home and says why in its
/// note; a profile declared for it reaches no environment — there is nothing
/// to set — and the identity the ledger keeps carries that measured note
/// instead of «not known», which would be false.
#[test]
fn a_profile_for_a_home_nothing_moves_reaches_no_environment_and_says_why() {
    let cli = profiles::known_clis()
        .iter()
        .find(|cli| cli.home == profiles::HomeMechanism::Unknown)
        .expect("the shipped list declares a command line whose home nothing moves");
    assert!(
        !cli.home_note.trim().is_empty(),
        "the entry must say why, since that is what a person reads"
    );
    let mut store = ProfileStore::default();
    store.profiles.push(Profile {
        name: "work".to_owned(),
        cli_id: cli.id.clone(),
        home_dir: PathBuf::from("/case/work"),
        endpoint: None,
    });
    store.active.insert(cli.id.clone(), "work".to_owned());

    let equipment = equipment_for(&store, &cli.executable, &BTreeMap::new());

    assert!(
        equipment.env.is_empty(),
        "nothing moves this home, so nothing is overlaid: {:?}",
        equipment.env
    );
    let EngineIdentity::NotMovedByAnEnvVar {
        why, profile_name, ..
    } = equipment.identity
    else {
        panic!("a declared profile that is not in force")
    };
    assert_eq!(profile_name, "work");
    assert!(
        why.contains(cli.home_note.trim()),
        "the why carries the entry's own measured note: {why}"
    );
}

/// A profile with a native endpoint launches the unmodified command line
/// pointed there, with the key read from the machine, and the identity names
/// the endpoint; a profile whose endpoint speaks another protocol refuses the
/// launch, naming both protocols.
#[test]
fn a_profile_with_a_native_endpoint_points_the_engine_there_and_says_so() {
    let mut store = a_store_with_one_active_profile();
    let keys = |variable: &str| (variable == "A_KEY_VAR").then(|| "the-key".to_owned());
    let mut pointed = |protocol: &str| {
        store.profiles[0].endpoint = Some(ProfileEndpoint {
            url: "http://localhost:11434/v1".to_owned(),
            key_var: "A_KEY_VAR".to_owned(),
            protocol: protocol.to_owned(),
        });
        equipment_with_keys_and_disk(
            &store,
            "/opt/homebrew/bin/codex",
            &BTreeMap::new(),
            &keys,
            &|path: &std::path::Path| path.ends_with("auth.json"),
            None,
        )
    };

    let native = pointed("openai-responses");
    assert_eq!(native.refused, None);
    assert_eq!(native.env.get("OPENAI_BASE_URL").map(String::as_str), Some("http://localhost:11434/v1"));
    assert_eq!(native.env.get("OPENAI_API_KEY").map(String::as_str), Some("the-key"));
    assert_eq!(native.env.get("CODEX_HOME").map(String::as_str), Some("/case/codex/lavoro"));
    let EngineIdentity::ProfileInForce { endpoint, .. } = native.identity else {
        panic!("a profile in force")
    };
    assert_eq!(endpoint.as_deref(), Some("http://localhost:11434/v1"));

    let foreign = pointed("anthropic-messages");
    let why = foreign.refused.expect("another protocol is refused");
    assert!(why.contains("anthropic-messages") && why.contains("openai-responses"), "{why}");
    assert!(!foreign.env.contains_key("OPENAI_BASE_URL"), "nothing points a refused launch");
}

/// **THE PROOF THAT CLOSES FAULT 18.** A step invoking `codex` must start with
/// the active profile's home, not the terminal's.
///
/// Put `equipment_for` back to returning `step_env` alone and this turns red: it
/// is the original defect, not an imitation of it.
#[test]
fn a_flow_step_launches_the_engine_inside_the_profiles_home() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "/opt/homebrew/bin/codex",
        &BTreeMap::new(),
    );

    assert_eq!(
        equipment.env.get("CODEX_HOME").map(String::as_str),
        Some("/case/codex/lavoro"),
        "the step still inherits the home of whoever opened the terminal"
    );
}

/// **A VARIABLE WRITTEN IN THE STEP WINS, AND THE ORDER IS THE DECISION.**
///
/// A variable written inside a step says something precise about *that* call — a
/// different profile for one step, a throwaway home for a test — and must not be
/// overridden by state living elsewhere that the step does not name. The other
/// order would silently make a line written on purpose in the flow inert.
///
/// *Mutant run*: reverse the overlay order, letting the profile win. This test
/// turns red and the one above stays green — precisely why both are needed.
#[test]
fn what_the_step_declares_beats_the_profile_never_the_other_way_round() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "/opt/homebrew/bin/codex",
        &step_env(&[("CODEX_HOME", "/a/home/written/in/the/step")]),
    );

    assert_eq!(
        equipment.env.get("CODEX_HOME").map(String::as_str),
        Some("/a/home/written/in/the/step")
    );
}

/// The variables a step declares that have nothing to do with the profile
/// arrive untouched: the equipment **adds**, it does not replace.
#[test]
fn the_rest_of_what_the_step_declares_arrives_untouched() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "codex",
        &step_env(&[("RUST_LOG", "debug")]),
    );

    assert_eq!(
        equipment.env.get("RUST_LOG").map(String::as_str),
        Some("debug")
    );
    assert_eq!(
        equipment.env.get("CODEX_HOME").map(String::as_str),
        Some("/case/codex/lavoro")
    );
}

/// **A PROFILE THAT EXISTS BUT IS NOT ACTIVE CHANGES NOTHING.** `claude` has a
/// profile in the table and nobody switched it on: overlaying a home would move
/// the identity of a command line nobody asked to move, and it would be
/// discovered through a lost login.
#[test]
fn a_profile_that_exists_but_is_not_active_moves_nothing() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "/usr/local/bin/claude",
        &BTreeMap::new(),
    );

    assert!(equipment.env.is_empty(), "{:?}", equipment.env);
    // **AND IT IS NOT A BLANK: IT IS «INHERITED».** The process starts in the
    // home of whoever opened the terminal, a real and nameable identity.
    assert_eq!(
        equipment.identity,
        EngineIdentity::InheritedFromTheTerminal {
            cli_id: "claude".to_owned()
        }
    );
}

/// A command that is not a known command line — an `sh` hand-written in a step
/// — has no home to move, and must not be handed one.
#[test]
fn a_plain_command_gets_no_home_of_anyones() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "/bin/sh",
        &BTreeMap::new(),
    );

    assert!(equipment.env.is_empty(), "{:?}", equipment.env);
    assert_eq!(equipment.identity, EngineIdentity::NotAKnownEngine);
}

/// **THE RESOLVED PROFILE IS WRITTEN DOWN, OR TWO RUNS ARE NOT ONE MEASURE.**
///
/// Reading a ledger row without knowing which identity that call ran under, you
/// can compare it with nothing: the same chain of steps under two profiles gives
/// two different consumptions for a reason the row does not carry. **And the
/// home's path belongs in it**: that is the ground a diagnosis stands on, while
/// a name gets reused and moved.
#[test]
fn the_resolved_profile_is_written_down_not_left_to_be_guessed() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "codex",
        &BTreeMap::new(),
    );

    assert_eq!(
        equipment.identity,
        EngineIdentity::ProfileInForce {
            cli_id: "codex".to_owned(),
            profile_name: "lavoro".to_owned(),
            home_dir: PathBuf::from("/case/codex/lavoro"),
            endpoint: None,
        }
    );
}

/// **A STEP THAT OVERRIDES SAYS SO, AND THAT IS THE CURE.**
///
/// The ledger row used to say `codex/lavoro` here too: the engine started in the
/// home written in the step while the record named the active profile. **The
/// record said one identity and the process had used another**, exactly where
/// somebody had changed it on purpose — the case a diagnosis or a security check
/// exists to see.
///
/// *Mutant run*: remove from `identity_of` the branch that looks at `step_env`
/// first. This turns red and the others stay green.
#[test]
fn a_step_that_writes_the_home_variable_is_recorded_as_the_one_who_chose() {
    let equipment = equipment_for(
        &a_store_with_one_active_profile(),
        "codex",
        &step_env(&[("CODEX_HOME", "/a/home/written/in/the/step")]),
    );

    assert_eq!(
        equipment.identity,
        EngineIdentity::ChosenByTheStep {
            cli_id: "codex".to_owned(),
            home_dir: PathBuf::from("/a/home/written/in/the/step"),
        },
        "the row would name a profile the process never used"
    );
}

/// **A DECLARED PROFILE IS NOT A PROFILE IN FORCE.** `antigravity` has no
/// variable that moves the home: there the identity hangs on where a file on
/// disk points, and this function never touches disk. The same empty string as
/// «no profile» would blur the two cases; the row says **why** as well.
#[test]
fn a_cli_whose_home_no_variable_moves_says_so_with_its_reason() {
    let mut store = ProfileStore::default();
    store.profiles.push(Profile {
        name: "lavoro".to_owned(),
        cli_id: "antigravity".to_owned(),
        home_dir: PathBuf::from("/case/antigravity/lavoro"),
        endpoint: None,
    });
    store
        .active
        .insert("antigravity".to_owned(), "lavoro".to_owned());

    // The binary is not the id: the list declares `agy`, which is what the
    // vendor installs, and it is the binary that a step names.
    let equipment = equipment_for(&store, "agy", &BTreeMap::new());

    assert!(equipment.env.is_empty(), "{:?}", equipment.env);
    let EngineIdentity::NotMovedByAnEnvVar {
        cli_id,
        profile_name,
        why,
    } = equipment.identity
    else {
        panic!("a profile never put in force reads as something else");
    };
    assert_eq!(cli_id, "antigravity");
    assert_eq!(profile_name, "lavoro");
    assert!(!why.is_empty(), "the reason is missing, and it is half the datum");
}

/// **STATE NAMING A VANISHED PROFILE INVENTS NO HOME.**
///
/// `sailor run` refuses to start in this case, and rightly: there the whole
/// invocation *is* that profile. Here the step still has an engine to call, and
/// stopping it over stale state would punish a bystander — but inventing a
/// directory from the profile's name would be worse: it would start in an empty
/// home, with no credentials, wearing the air of an applied profile. Nothing is
/// overlaid, and the ledger **says so** rather than keeping quiet: of the five
/// cases that all ended in one empty string, this is the one asking somebody to
/// act, because there is state to repair.
#[test]
fn a_stale_active_name_that_matches_no_profile_moves_nothing() {
    let mut store = a_store_with_one_active_profile();
    store
        .active
        .insert("codex".to_owned(), "sparito".to_owned());

    let equipment = equipment_for(&store, "codex", &BTreeMap::new());

    assert!(equipment.env.is_empty(), "{:?}", equipment.env);
    assert_eq!(
        equipment.identity,
        EngineIdentity::ProfileVanished {
            cli_id: "codex".to_owned(),
            profile_name: "sparito".to_owned(),
        }
    );
}

/// **A PROFILE NOBODY IS SIGNED IN TO IS PAID FOR BEFORE IT FAILS.** The chain
/// tried it, the engine started, the call went out and came back unauthorised:
/// the refusal belongs before the launch, where every other one already is.
#[test]
fn a_profile_whose_home_carries_no_credentials_is_refused_before_it_is_launched() {
    let store = a_store_with_one_active_profile();
    let nothing_on_this_disk = |_: &std::path::Path| false;

    let equipment = equipment_with_keys_and_disk(
        &store,
        "codex",
        &step_env(&[]),
        &|_| None,
        &nothing_on_this_disk,
        None,
    );

    let why = equipment.refused.expect("a signed-out profile was let through");
    assert!(why.contains("lavoro"), "the refusal does not name the profile: {why}");
    assert!(
        why.contains("/case/codex/lavoro"),
        "the refusal does not name the home to go and look at: {why}"
    );
}

/// The same profile, with its credentials where the command line declares them:
/// nothing is refused, and the environment still reaches the engine.
#[test]
fn a_profile_signed_in_where_its_command_line_says_is_not_refused() {
    let store = a_store_with_one_active_profile();
    let signed_in = |path: &std::path::Path| path.ends_with("auth.json");

    let equipment =
        equipment_with_keys_and_disk(&store, "codex", &step_env(&[]), &|_| None, &signed_in, None);

    assert_eq!(equipment.refused, None, "a signed-in profile was refused");
    assert_eq!(
        equipment.env.get("CODEX_HOME"),
        Some(&"/case/codex/lavoro".to_owned()),
        "the home stopped reaching the engine"
    );
}

/// **UNKNOWN IS NOT SIGNED OUT.** Where nobody established where a command line
/// keeps its credentials, an empty disk says nothing about it: the engine is
/// tried and answers for itself, rather than being excluded on a guess.
#[test]
fn a_command_line_that_declares_no_credentials_file_is_never_refused_for_one() {
    let mut store = ProfileStore::default();
    store.profiles.push(Profile {
        name: "sola".to_owned(),
        cli_id: "gemini".to_owned(),
        home_dir: PathBuf::from("/case/gemini/sola"),
        endpoint: None,
    });
    store.active.insert("gemini".to_owned(), "sola".to_owned());

    let equipment = equipment_with_keys_and_disk(
        &store,
        "gemini",
        &step_env(&[]),
        &|_| None,
        &|_: &std::path::Path| false,
        None,
    );

    assert_eq!(
        equipment.refused, None,
        "an engine was excluded because nobody had established where it keeps credentials"
    );
}
