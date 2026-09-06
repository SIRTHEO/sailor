//! What an engine starts with: the environment of the profile in force under
//! the step's own, and the identity the ledger row names for it.

use ledger::EngineIdentity;
use std::collections::BTreeMap;
use std::path::PathBuf;

// ── the equipment an engine starts with ──────────────────────────────────

/// What a call to an external engine really starts with.
///
/// **WHY THE TWO FIELDS TRAVEL TOGETHER.** The environment decides *which home*
/// that engine will read; the profile name is what lands in the ledger. Apart,
/// the same profile would be resolved twice and could go wrong in one place
/// only — writing a ledger row for equipment other than the one the call ran
/// with, which is worse than writing none.
pub struct Equipment {
    /// To overlay on the inherited environment before launching.
    pub env: BTreeMap<String, String>,
    /// The identity the process starts under: **which home** and **how it was
    /// chosen**. It always answers — there is no «empty» case.
    pub identity: EngineIdentity,
    /// Why this engine must not start under this profile: an endpoint the
    /// command line cannot be pointed at, or a key the machine lacks.
    pub refused: Option<String>,
}

/// The equipment for invoking `bin`, against the given profile state.
///
/// **FAULT 18, AND THE SAME DISEASE AS 35.** Both are «Sailor holds a fact in
/// its own home and does not use it». The equipment was there — `equipment/`,
/// `flows/`, a price list, a signature — and never reached the engines, because
/// only `sailor run` called the environment overlay. An engine launched by a
/// flow step inherited the environment of whoever opened the terminal: it read
/// the neighbour's home, and two runs of one flow were not the same measure.
///
/// **THE PROFILE'S ENVIRONMENT SITS UNDER THE STEP'S, AND THE ORDER IS THE
/// DECISION.** Writing a variable inside a step says something precise about
/// *that* call — a different profile for one step, a throwaway home for a test —
/// and must not be overridden by state living elsewhere that the step does not
/// name. The other order would silently make the line written in the flow inert.
///
/// **PURE: STATE IN, EQUIPMENT OUT.** Reading the profiles file happens in
/// [`current_equipment_for`], for the same reason as `price_list_from`.
pub fn equipment_for(
    store: &profiles::ProfileStore,
    bin: &str,
    step_env: &BTreeMap<String, String>,
) -> Equipment {
    equipment_with_keys(store, bin, step_env, &|variable| std::env::var(variable).ok())
}

/// [`equipment_for`] with the machine's key variables read through `key_of`,
/// so a test hands its own.
pub fn equipment_with_keys(
    store: &profiles::ProfileStore,
    bin: &str,
    step_env: &BTreeMap<String, String>,
    key_of: &dyn Fn(&str) -> Option<String>,
) -> Equipment {
    let Some(cli) = profiles::cli_for_executable(bin) else {
        // An arbitrary command — `sh`, a script — has no home to move, and
        // handing it one would mean nothing.
        return Equipment {
            env: step_env.clone(),
            identity: EngineIdentity::NotAKnownEngine,
            refused: None,
        };
    };
    let named = store.active.get(&cli.id);
    let resolved = named.and_then(|active| {
        store
            .profiles
            .iter()
            // **STATE NAMING A VANISHED PROFILE INVENTS NO DIRECTORY.**
            // Composing the path from the name would give an empty home — one
            // with no credentials — wearing the air of an applied profile.
            .find(|profile| profile.cli_id == cli.id && &profile.name == active)
    });
    let mut from_the_profile = resolved
        .map(|profile| profiles::build_environment(cli, &profile.home_dir, key_of))
        .unwrap_or_default();
    // The endpoint, when the profile declares one: the same overlay, and a
    // refusal instead of a launch when it cannot be pointed there.
    let refused = match resolved.map(|profile| profiles::endpoint_environment(cli, profile, key_of)) {
        Some(Ok(pointed)) => {
            from_the_profile.extend(pointed);
            None
        }
        Some(Err(why)) => Some(why),
        None => None,
    };
    // Profile first, step on top: a variable written in the step wins.
    let mut env = from_the_profile;
    env.extend(
        step_env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    Equipment {
        env,
        identity: identity_of(cli, named.map(String::as_str), resolved, step_env),
        refused,
    }
}

/// The identity this invocation really starts under.
///
/// **THE STEP IS LOOKED AT FIRST, AND THAT IS THE CURE.** As a boolean — «a
/// profile was applied» — this decision stayed true even when the step had
/// written the home variable itself: the engine started in the step's home
/// while the ledger row named the active profile, so **the ledger said one
/// identity and the process used another**, exactly where somebody had changed
/// it on purpose. The order below is the real overlay's, not the state's.
fn identity_of(
    cli: &profiles::KnownCli,
    named: Option<&str>,
    resolved: Option<&profiles::Profile>,
    step_env: &BTreeMap<String, String>,
) -> EngineIdentity {
    let cli_id = cli.id.clone();
    if let profiles::HomeMechanism::EnvVar(variable) = &cli.home {
        if let Some(home) = step_env.get(variable) {
            return EngineIdentity::ChosenByTheStep {
                cli_id,
                home_dir: PathBuf::from(home),
            };
        }
    }
    match (resolved, named) {
        (Some(profile), _) => match &cli.home {
            profiles::HomeMechanism::EnvVar(_) => EngineIdentity::ProfileInForce {
                cli_id,
                profile_name: profile.name.clone(),
                home_dir: profile.home_dir.clone(),
                endpoint: profile.endpoint.as_ref().map(|endpoint| endpoint.url.clone()),
            },
            // **A DECLARED PROFILE IS NOT A PROFILE IN FORCE.** Where the home
            // moves by swapping a symlink, or where how it moves is unknown,
            // this function put nothing in the environment: the identity hangs
            // on where a file on disk points, and this code never touches disk.
            _ => EngineIdentity::NotMovedByAnEnvVar {
                cli_id,
                profile_name: profile.name.clone(),
                why: why_it_stays_where_it_is(cli),
            },
        },
        (None, Some(active)) => EngineIdentity::ProfileVanished {
            cli_id,
            profile_name: active.to_owned(),
        },
        // **«INHERITED» IS NOT «NOTHING».** The process starts in the home of
        // whoever opened the terminal, a real and nameable identity: saying so
        // beats a blank in which this case blurs into the other four.
        (None, None) => EngineIdentity::InheritedFromTheTerminal { cli_id },
    }
}

/// Why a declared profile did not reach the environment, in the words of the
/// mechanism that keeps it out — and, where no mechanism is declared, in the
/// words the command line's own entry gives: «not known» would be false for
/// an entry that was measured and found unmovable.
fn why_it_stays_where_it_is(cli: &profiles::KnownCli) -> String {
    match &cli.home {
        profiles::HomeMechanism::CredentialSymlink { .. } => {
            "this command line has no variable that moves the home: the profile swaps a symlink, and the identity depends on where that file points on the disk".to_owned()
        }
        profiles::HomeMechanism::Unknown => {
            let why = "no variable is declared to move this command line's home, so nothing was overlaid";
            match cli.home_note.trim() {
                "" => why.to_owned(),
                note => format!("{why}; its entry says: {note}"),
            }
        }
        // A variable mechanism never reaches here: the caller handled it above.
        // Were it ever to, the sentence still tells the truth.
        profiles::HomeMechanism::EnvVar(_) => {
            "the mechanism goes through a variable, and it was not overlaid".to_owned()
        }
    }
}

/// **This** machine's equipment for invoking `bin`.
///
/// **RE-READ ON EVERY CALL**, for the price list's reason: a profile changed
/// halfway through a long run applies from the next call rather than the next
/// restart, and reading a small file beside the launch of an external process
/// costs nothing.
///
/// Unreadable profile state does not stop the call: it starts with nothing
/// overlaid, which is how it always started. Stopping a step because a
/// preferences file could not be read would punish a bystander.
pub(crate) fn current_equipment_for(bin: &str, step_env: &BTreeMap<String, String>) -> Equipment {
    let store = profiles::store_io::load_store().unwrap_or_default();
    equipment_for(&store, bin, step_env)
}
