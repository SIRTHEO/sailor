//! `sailor run <cli> [arguments...]`: the quick swap. It finds the active
//! profile of `cli` and **replaces** this process with its executable — never a
//! child, since signals, exit code and interactive terminal must behave as if
//! the command line had been invoked by hand. With no active profile, or a home
//! mechanism still unknown, it refuses: launching under the wrong identity is
//! the worst fault this command could commit.

use ledger::{EngineIdentity, Ledger, ModelCallRecord};
use profiles::{
    build_environment, find_cli, store_io, symlink_swap, HomeMechanism, ProfileStore, SymlinkSwap,
};
use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

/// What to launch for `cli_id`, according to the given state: pure — it touches
/// no process — so the test that matters (the swap reaches the right
/// environment) need not replace itself to check it.
#[derive(Debug)]
struct Launch {
    executable: String,
    env: BTreeMap<String, String>,
    args: Vec<String>,
    /// For command lines that do not move the home with a variable but swap a
    /// link over the credentials, the link that must be standing for the launch
    /// to carry the right identity. `None` for all the others. The launcher
    /// checks it **before** replacing the process; here it stays data, so this
    /// function touches no disk.
    expected_link: Option<SymlinkSwap>,
    /// Which home this launch carries and how it was chosen, so the swap can be
    /// written down before it happens.
    identity: EngineIdentity,
}

fn resolve(
    cli_id: &str,
    store: &ProfileStore,
    rest: &[String],
    fixed_home: &std::path::Path,
) -> Result<Launch, String> {
    resolve_with(cli_id, store, rest, fixed_home, &key_of_the_environment)
}

/// The same, reading the endpoint's key from wherever the caller says. The
/// proof needs a machine whose variables it decides.
fn resolve_with(
    cli_id: &str,
    store: &ProfileStore,
    rest: &[String],
    fixed_home: &std::path::Path,
    key_of: &dyn Fn(&str) -> Option<String>,
) -> Result<Launch, String> {
    let cli = find_cli(cli_id)?;
    if matches!(cli.home, HomeMechanism::Unknown) {
        return Err(catalogue::say(
            "cli.run.home_mechanism_unknown",
            &[("cli", &cli.display_name), ("note", &cli.home_note)],
        ));
    }
    let Some(active_name) = store.active.get(&cli.id) else {
        return Err(catalogue::say(
            "cli.run.no_active_profile",
            &[("cli", &cli.display_name), ("id", &cli.id)],
        ));
    };
    let profile = store
        .profiles
        .iter()
        .find(|p| p.cli_id == cli.id && &p.name == active_name)
        .ok_or_else(|| {
            catalogue::say(
                "cli.run.active_profile_gone",
                &[("profile", active_name), ("id", &cli.id)],
            )
        })?;

    // The link mechanism goes through no variable: the launch's identity depends
    // **only** on where a link on disk points. If the state says «X is active»
    // but the link still points at Y, the command line starts with Y's
    // credentials and nobody notices. Here the expected link is computed; the
    // launcher compares it against the real one.
    let expected_link = match &cli.home {
        HomeMechanism::CredentialSymlink { relative_path } => {
            Some(symlink_swap(fixed_home, relative_path, &profile.home_dir))
        }
        _ => None,
    };

    // A profile that points its command line at another endpoint points it
    // there in a terminal too. Refused rather than launched without it: an
    // engine that quietly falls back to the subscription spends money the
    // person had said should be spent elsewhere.
    let mut env = build_environment(cli, &profile.home_dir, key_of);
    env.extend(profiles::endpoint_environment(cli, profile, key_of)?);

    Ok(Launch {
        executable: cli.executable.to_owned(),
        env,
        args: rest.to_vec(),
        expected_link,
        identity: EngineIdentity::ProfileInForce {
            cli_id: cli.id.clone(),
            profile_name: profile.name.clone(),
            home_dir: profile.home_dir.clone(),
            endpoint: profile.endpoint.as_ref().map(|it| it.url.clone()),
        },
    })
}

/// The run_id of an engine nobody started for a flow. A real id would tie the
/// spend to a run that never existed; the empty string would be a run.
pub const BY_HAND: &str = "by-hand";

/// The invocation about to happen, written **before** the swap: past `exec`
/// there is no process left to write anything down.
fn the_invocation(launch: &Launch, cli_id: &str, pid: u32, now: i64) -> ModelCallRecord {
    ModelCallRecord {
        call_id: format!("{BY_HAND}:{cli_id}:{pid}:{now}"),
        run_id: BY_HAND.to_owned(),
        step_id: None,
        purpose: "run_by_hand".to_owned(),
        cli: cli_id.to_owned(),
        // `sailor run` chooses no model: the engine picks its own, and this
        // process is gone before it says which.
        requested_model: String::new(),
        actual_model: String::new(),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
        cache_write_tokens: None,
        cache_write_long_tokens: None,
        total_tokens: None,
        turns: None,
        // **A CALL WITHOUT A COST IS NOT A FREE CALL.** Nobody reports what an
        // engine driven by hand spends, so the sum a budget reads is a floor
        // and already says so. Writing a zero here would make it a lie.
        cost_micros: None,
        declared_cost_micros: None,
        price_currency: None,
        input_price_micros_per_million: None,
        output_price_micros_per_million: None,
        cached_price_micros_per_million: None,
        cache_write_price_micros_per_million: None,
        cache_write_long_price_micros_per_million: None,
        engine_identity: launch.identity.clone(),
        retry_chain: Vec::new(),
        error_type: None,
        started_at: now,
        // The swap leaves nobody behind to write an ending.
        ended_at: None,
        session_id: None,
        session_mode: None,
        work_kind: None,
        fell_back_from: Vec::new(),
    }
}

/// The key itself is never in the store: the profile names the variable, the
/// machine holds the value.
fn key_of_the_environment(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|key| !key.is_empty())
}

/// Does the link on disk really point at the active profile? It refuses rather
/// than repairing: `sailor run` must have no side effects on disk, and an
/// explicit refusal is safer than a silent repair — whoever reads the message
/// knows state and disk had drifted apart, and the swap has a command of its own.
fn link_points_at_the_active_profile(expected: &SymlinkSwap) -> Result<(), String> {
    match std::fs::read_link(&expected.link_path) {
        Ok(actual) if actual == expected.target_path => Ok(()),
        Ok(actual) => Err(catalogue::say(
            "cli.run.link_points_elsewhere",
            &[
                ("link", &expected.link_path.display().to_string()),
                ("actual", &actual.display().to_string()),
                ("wanted", &expected.target_path.display().to_string()),
            ],
        )),
        Err(error) => Err(catalogue::say(
            "cli.run.link_unreadable",
            &[
                ("link", &expected.link_path.display().to_string()),
                ("error", &error.to_string()),
            ],
        )),
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The shape of `sailor run`. See `flow_cmd::USAGE`.
pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor run <cli> [arguments...]",
    says_key: "",
}];

pub fn run(args: &[String]) -> i32 {
    let [cli_id, rest @ ..] = args else {
        eprintln!(
            "{} {}",
            catalogue::say("cli.usage_heading", &[]),
            USAGE[0].form
        );
        return 2;
    };
    let store = match store_io::load_store() {
        Ok(store) => store,
        Err(message) => {
            eprintln!("sailor run: {message}");
            return 1;
        }
    };
    let launch = match resolve(cli_id, &store, rest, &home_dir()) {
        Ok(launch) => launch,
        Err(message) => {
            eprintln!("sailor run: {message}");
            return 1;
        }
    };
    // Before `exec`, never after: past it there is no coming back.
    if let Some(expected) = &launch.expected_link {
        if let Err(message) = link_points_at_the_active_profile(expected) {
            eprintln!("sailor run: {message}");
            return 1;
        }
    }
    write_down_the_invocation(&launch, cli_id);
    // `exec` replaces this process's image: on success the code below never
    // runs. It returns only to say the launch failed.
    let error = Command::new(&launch.executable)
        .args(&launch.args)
        .envs(&launch.env)
        .exec();
    eprintln!(
        "{}",
        catalogue::say(
            "cli.run.cannot_start",
            &[
                ("executable", &launch.executable),
                ("error", &error.to_string())
            ],
        )
    );
    1
}

/// **THE LEDGER IS NOT A REASON TO REFUSE A LAUNCH.** An engine a person asked
/// for starts whether or not the store opened; what is lost is the record, and
/// that is not paid for with the person's work.
fn write_down_the_invocation(launch: &Launch, cli_id: &str) {
    let Some(directory) = ledger::default_directory() else {
        return;
    };
    let Ok(store) = Ledger::open(&directory) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let _ = store.record_model_call(&the_invocation(launch, cli_id, std::process::id(), now));
}

#[cfg(test)]
mod tests {
    use super::*;
    use profiles::Profile;
    use std::path::{Path, PathBuf};

    fn two_profile_store() -> ProfileStore {
        let mut store = ProfileStore::default();
        store.profiles.push(Profile {
            name: "primo".to_owned(),
            cli_id: "codex".to_owned(),
            home_dir: PathBuf::from("/prova/codex/primo"),
            endpoint: None,
        });
        store.profiles.push(Profile {
            name: "secondo".to_owned(),
            cli_id: "codex".to_owned(),
            home_dir: PathBuf::from("/prova/codex/secondo"),
            endpoint: None,
        });
        store
    }

    /// The test that matters: the swap must reach the environment that would be
    /// handed to the process, not stay written in the state alone. If the same
    /// value showed up here before and after, the swap would not work: that is
    /// why the first and second profiles have different homes and the test
    /// compares both.
    #[test]
    fn switching_the_active_profile_changes_what_the_launch_would_see() {
        let mut store = two_profile_store();
        store.active.insert("codex".to_owned(), "primo".to_owned());
        let launch = resolve("codex", &store, &[], Path::new("/casa")).unwrap();
        assert_eq!(
            launch.env.get("CODEX_HOME"),
            Some(&"/prova/codex/primo".to_owned())
        );

        store
            .active
            .insert("codex".to_owned(), "secondo".to_owned());
        let launch = resolve("codex", &store, &[], Path::new("/casa")).unwrap();
        assert_eq!(
            launch.env.get("CODEX_HOME"),
            Some(&"/prova/codex/secondo".to_owned())
        );
    }

    /// A terminal is where an engine is actually worked in, and until now the
    /// endpoint reached only the steps of a flow: the same profile sent a run
    /// to another provider and a terminal to the subscription.
    #[test]
    fn a_profile_with_an_endpoint_points_the_terminal_there_too() {
        let mut store = two_profile_store();
        store.profiles[0].endpoint = Some(profiles::ProfileEndpoint {
            url: "https://altrove.example/v1".to_owned(),
            key_var: "CHIAVE_ALTROVE".to_owned(),
            protocol: "openai-responses".to_owned(),
        });
        store.active.insert("codex".to_owned(), "primo".to_owned());
        let with_the_key = |name: &str| (name == "CHIAVE_ALTROVE").then(|| "una-chiave".to_owned());

        let launch =
            resolve_with("codex", &store, &[], Path::new("/casa"), &with_the_key).unwrap();
        assert_eq!(
            launch.env.get("OPENAI_BASE_URL"),
            Some(&"https://altrove.example/v1".to_owned()),
            "the terminal starts pointed at the endpoint the profile declares"
        );
        assert_eq!(launch.env.get("OPENAI_API_KEY"), Some(&"una-chiave".to_owned()));
        assert_eq!(
            launch.env.get("CODEX_HOME"),
            Some(&"/prova/codex/primo".to_owned()),
            "and still in the home of the profile in force"
        );

        let nowhere = |_: &str| None;
        let refused =
            resolve_with("codex", &store, &[], Path::new("/casa"), &nowhere).unwrap_err();
        assert!(
            refused.contains("CHIAVE_ALTROVE"),
            "a key that is not on this machine refuses the launch instead of \
             starting it against the subscription: {refused}"
        );
    }

    #[test]
    fn without_an_active_profile_the_launch_is_refused() {
        let store = two_profile_store();
        let error = resolve("codex", &store, &[], Path::new("/casa")).unwrap_err();
        assert!(error.contains("no active profile"), "{error}");
    }

    #[test]
    fn a_stale_active_name_that_matches_no_profile_is_refused() {
        let mut store = two_profile_store();
        store
            .active
            .insert("codex".to_owned(), "sparito".to_owned());
        let error = resolve("codex", &store, &[], Path::new("/casa")).unwrap_err();
        assert!(error.contains("no longer in the state"), "{error}");
    }

    #[test]
    fn an_unverified_home_mechanism_is_refused_with_the_reason() {
        let mut store = ProfileStore::default();
        store.profiles.push(Profile {
            name: "prova".to_owned(),
            cli_id: "antigravity".to_owned(),
            home_dir: PathBuf::from("/prova/antigravity/prova"),
            endpoint: None,
        });
        store
            .active
            .insert("antigravity".to_owned(), "prova".to_owned());
        let error = resolve("antigravity", &store, &[], Path::new("/casa")).unwrap_err();
        assert!(error.contains("is not known yet"), "{error}");
    }

    #[test]
    fn an_unknown_cli_is_refused() {
        let store = ProfileStore::default();
        assert!(resolve("non-esiste", &store, &[], Path::new("/casa")).is_err());
    }

    #[test]
    fn no_cli_named_is_a_usage_error() {
        assert_eq!(run(&[]), 2);
    }

    /// A defect found by an independent reviewer who had not written this code:
    /// for a command line that swaps a link instead of reading a variable, the
    /// built environment is **empty** — the launch's identity depends only on
    /// where a file on disk points. No command line in the table uses that
    /// today, so it was not exploitable; the first one to use it would have
    /// started under another profile's credentials, silently. All three
    /// situations count and the test can fail on each: the right link passes,
    /// one pointing elsewhere is refused, and an absent one too — «I cannot
    /// read it» is not «it is fine».
    #[test]
    fn a_link_pointing_at_another_profile_stops_the_launch() {
        let dir = std::env::temp_dir().join(format!("sailor-run-link-{}", std::process::id()));
        let fixed_home = dir.join("casa-fissa");
        let wanted = dir.join("profili").join("voluto");
        let other = dir.join("profili").join("altro");
        std::fs::create_dir_all(&fixed_home).unwrap();
        std::fs::create_dir_all(&wanted).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(wanted.join("credentials.json"), "the right ones").unwrap();
        std::fs::write(other.join("credentials.json"), "somebody else's").unwrap();

        let expected = symlink_swap(&fixed_home, "credentials.json", &wanted);

        // No link at all: it refuses rather than launching blind.
        let error = link_points_at_the_active_profile(&expected).unwrap_err();
        assert!(error.contains("cannot read the link"), "{error}");

        // A link to another profile: it refuses, and names which.
        std::os::unix::fs::symlink(other.join("credentials.json"), &expected.link_path).unwrap();
        let error = link_points_at_the_active_profile(&expected).unwrap_err();
        assert!(error.contains("have come apart"), "{error}");
        assert!(error.contains("altro"), "{error}");

        // A link to the active profile: it passes.
        std::fs::remove_file(&expected.link_path).unwrap();
        std::os::unix::fs::symlink(&expected.target_path, &expected.link_path).unwrap();
        assert!(link_points_at_the_active_profile(&expected).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod invocations {
    use super::*;
    use profiles::Profile;
    use std::path::{Path, PathBuf};

    fn a_store_with_a_profile() -> ProfileStore {
        let mut store = ProfileStore::default();
        store.profiles.push(Profile {
            name: "quello-di-casa".to_owned(),
            cli_id: "codex".to_owned(),
            home_dir: PathBuf::from("/prova/codex/quello-di-casa"),
            endpoint: None,
        });
        store
            .active
            .insert("codex".to_owned(), "quello-di-casa".to_owned());
        store
    }

    fn a_scratch(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sailor-run-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    /// **AN ENGINE DRIVEN BY HAND SPENT NOTHING THE LEDGER COULD SEE.** Every
    /// window of spend per engine read only what a flow had opened, so a person
    /// working all afternoon in `sailor run codex` was, to the budget, an
    /// engine nobody had used.
    #[test]
    fn an_engine_started_by_hand_appears_in_what_that_engine_has_been_asked_for() {
        let root = a_scratch("by-hand");
        let store = Ledger::open(&root).expect("a store opens");
        let launch = resolve("codex", &a_store_with_a_profile(), &[], Path::new("/casa"))
            .expect("the launch resolves");

        let before = store.spent_by_cli_since("codex", 0).expect("read the window");
        store
            .record_model_call(&the_invocation(&launch, "codex", 4242, 1_700_000_000))
            .expect("the invocation goes in");
        let after = store.spent_by_cli_since("codex", 0).expect("read the window");

        assert_eq!(before.calls, 0, "the window was not empty to begin with");
        assert_eq!(after.calls, 1, "the engine a person started is in no window");
        assert_eq!(
            after.calls_without_cost, 1,
            "an invocation nobody priced was counted as priced"
        );
        assert_eq!(after.micros, 0, "a cost was invented for a call nobody measured");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The row must say which home the swap carried: a launch under the wrong
    /// identity is the worst fault this command can commit, and a row that does
    /// not name the identity cannot show it happened.
    #[test]
    fn the_row_names_the_profile_the_swap_carried() {
        let launch = resolve("codex", &a_store_with_a_profile(), &[], Path::new("/casa"))
            .expect("the launch resolves");

        let written = the_invocation(&launch, "codex", 4242, 1_700_000_000);

        assert_eq!(
            written.engine_identity,
            EngineIdentity::ProfileInForce {
                cli_id: "codex".to_owned(),
                profile_name: "quello-di-casa".to_owned(),
                home_dir: PathBuf::from("/prova/codex/quello-di-casa"),
                endpoint: None,
            },
            "the row does not name the home the process started with"
        );
        assert_eq!(written.run_id, BY_HAND, "a run that never existed was named");
        assert!(written.ended_at.is_none(), "an ending was written for a swap");
    }
}
