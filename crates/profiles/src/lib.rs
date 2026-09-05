//! Several profiles per known command line, even where that command line has
//! none of its own. Where it reads a variable for its home a profile is just a
//! directory; where it does not, the fallback is a symlink on the credentials
//! file — more fragile, and marked as such.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub mod store_io;

/// How a command line finds its own home directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeMechanism {
    /// This variable moves the whole home directory.
    EnvVar(String),
    /// No known variable: the profile swaps a symlink at this path, relative to
    /// the fixed home.
    CredentialSymlink { relative_path: String },
    /// No variable and no symlink declared: nobody looked, or somebody did and
    /// nothing moves it at the price of a variable. `home_note` says which.
    Unknown,
}

/// Whether the command line handles several profiles itself, and how sure we are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeProfiles {
    Supported,
    NotSupported,
    /// Not checked in this environment: the real command was not reachable, or
    /// was never run.
    Unverified,
}

/// A known command line: how it is invoked and how its home moves. The list is
/// **declared in a file**, not written here — see [`BUILTIN`].
#[derive(Debug, Clone)]
pub struct KnownCli {
    pub id: String,
    pub display_name: String,
    pub executable: String,
    pub native_profiles: NativeProfiles,
    /// How the judgement above was reached: what the real command says, or why
    /// it was not checked.
    pub native_profiles_note: String,
    pub home: HomeMechanism,
    pub home_note: String,
    /// Below `$HOME`, where it keeps its home when nothing moves it.
    pub home_already_here: Option<String>,
    /// How this command line is pointed at another endpoint that speaks its
    /// own protocol, unmodified and with nothing in between; `None` when no
    /// such variable is known.
    pub endpoint: Option<NativeEndpoint>,
    /// The files it reads at its start, each relative to the project root or
    /// under `~`. Empty where nobody established them, which is not «none».
    pub reads_instructions_from: Vec<String>,
}

/// The two variables a command line reads to talk to an endpoint other than
/// its maker's, and the protocol that endpoint must speak.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeEndpoint {
    pub url_var: String,
    pub key_var: String,
    pub protocol: String,
}

/// Where a profile sends its command line instead of the maker's endpoint.
/// `key_var` names the variable on this machine that holds the key: the key
/// itself is never written in the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEndpoint {
    pub url: String,
    pub key_var: String,
    /// The protocol the endpoint speaks, as the person verified it; it must
    /// be the command line's own, or the profile is refused.
    pub protocol: String,
}

/// The environment that points `cli` at the profile's endpoint, or why it
/// cannot be pointed there. Empty when the profile declares no endpoint.
pub fn endpoint_environment(
    cli: &KnownCli,
    profile: &Profile,
    key_of: &dyn Fn(&str) -> Option<String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut env = BTreeMap::new();
    let Some(endpoint) = &profile.endpoint else {
        return Ok(env);
    };
    let Some(native) = cli.endpoint.as_ref() else {
        return Err(format!(
            "profile «{}» declares an endpoint, but no variable is known that points {} elsewhere",
            profile.name, cli.display_name
        ));
    };
    if endpoint.protocol != native.protocol {
        return Err(format!(
            "profile «{}» sends {} to {} speaking «{}», and {} speaks «{}»: nothing of Sailor's translates in between",
            profile.name, cli.display_name, endpoint.url, endpoint.protocol, cli.display_name, native.protocol
        ));
    }
    let Some(key) = key_of(&endpoint.key_var) else {
        return Err(format!(
            "profile «{}» takes its key from «{}», which is not set on this machine",
            profile.name, endpoint.key_var
        ));
    };
    env.insert(native.url_var.clone(), endpoint.url.clone());
    env.insert(native.key_var.clone(), key);
    Ok(env)
}

/// The list shipped with the product, embedded as `models::pricing::BUILTIN`
/// is: a binary copied elsewhere keeps answering, with no path to guess.
pub const BUILTIN: &str = include_str!("../command-lines.default.json");

/// The variable naming a file read instead of the shipped one, whole.
pub const COMMAND_LINES_PATH_VAR: &str = "PROFILES_COMMAND_LINES";

/// The table, read once. **THE ENGINES ARE NOT WRITTEN IN RUST**: a list of
/// providers compiled into a product that claims to know none.
pub fn known_clis() -> &'static [KnownCli] {
    static TABLE: std::sync::OnceLock<Vec<KnownCli>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let declared = std::env::var_os(COMMAND_LINES_PATH_VAR)
            .and_then(|path| std::fs::read_to_string(path).ok());
        let text = declared.as_deref().unwrap_or(BUILTIN);
        // **A BROKEN LIST FALLS BACK TO THE SHIPPED ONE.** Whoever mistyped a
        // comma wants their engines back, not a product that will not start.
        parse_command_lines(text)
            .or_else(|_| parse_command_lines(BUILTIN))
            .expect("the command lines shipped with the product parse")
    })
}

/// The list as a file declares it. **A REFUSAL NAMES THE ENGINE**: an entry
/// with a field this list does not know, or a value of the wrong shape, is
/// refused as a whole, and the id is what a person greps their file for.
pub fn parse_command_lines(text: &str) -> Result<Vec<KnownCli>, String> {
    let read: CommandLinesFile =
        serde_json::from_str(text).map_err(|error| format!("the list does not parse: {error}"))?;
    read.command_lines.into_iter().map(declared).collect()
}

fn declared(entry: serde_json::Value) -> Result<KnownCli, String> {
    let id = entry
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let cli: DeclaredCli = serde_json::from_value(entry).map_err(|error| {
        catalogue::say(
            "profiles.command_line_refused",
            &[("id", &id), ("why", &error.to_string())],
        )
    })?;
    if let Some(path) = cli
        .reads_instructions_from
        .iter()
        .find(|path| path.is_empty() || Path::new(path).is_absolute())
    {
        return Err(catalogue::say(
            "profiles.instructions_path_refused",
            &[("id", &id), ("path", path)],
        ));
    }
    Ok(KnownCli::from(cli))
}

#[derive(Deserialize)]
struct CommandLinesFile {
    #[serde(default)]
    command_lines: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclaredCli {
    id: String,
    #[serde(default)]
    display_name: String,
    executable: String,
    #[serde(default)]
    native: String,
    #[serde(default)]
    native_note: String,
    #[serde(default)]
    home: Option<DeclaredHome>,
    #[serde(default)]
    home_note: String,
    #[serde(default)]
    endpoint: Option<NativeEndpoint>,
    #[serde(default)]
    reads_instructions_from: Vec<String>,
}

/// Exactly one field is written; neither means **no known way**.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeclaredHome {
    #[serde(default)]
    variable: String,
    #[serde(default)]
    credential_symlink: String,
    #[serde(default)]
    already_at: String,
}

impl From<DeclaredCli> for KnownCli {
    fn from(declared: DeclaredCli) -> KnownCli {
        let already_at = declared
            .home
            .as_ref()
            .map(|home| home.already_at.trim())
            .filter(|at| !at.is_empty())
            .map(str::to_owned);
        let display_name = if declared.display_name.is_empty() {
            declared.id.clone()
        } else {
            declared.display_name
        };
        KnownCli {
            id: declared.id,
            display_name,
            executable: declared.executable,
            // **A WORD NOBODY TAUGHT US IS «UNVERIFIED», NEVER «NO»**.
            native_profiles: match declared.native.as_str() {
                "supported" => NativeProfiles::Supported,
                "not supported" => NativeProfiles::NotSupported,
                _ => NativeProfiles::Unverified,
            },
            native_profiles_note: declared.native_note,
            home: match declared.home {
                Some(home) if !home.variable.is_empty() => HomeMechanism::EnvVar(home.variable),
                Some(home) if !home.credential_symlink.is_empty() => {
                    HomeMechanism::CredentialSymlink {
                        relative_path: home.credential_symlink,
                    }
                }
                _ => HomeMechanism::Unknown,
            },
            home_note: declared.home_note,
            home_already_here: already_at,
            endpoint: declared.endpoint,
            reads_instructions_from: declared.reads_instructions_from,
        }
    }
}

/// The command line carrying this `id`, or a readable refusal. One place, so
/// two callers cannot disagree about what «unknown» means.
pub fn find_cli(id: &str) -> Result<&'static KnownCli, String> {
    known_clis()
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("unknown command line: {id}"))
}

/// The command line about to be launched, recognised by its **executable**.
///
/// Not by the tool id: a `toolbox` descriptor calls `claude-code` what this
/// table calls `claude`, and the two lists answer different questions. What
/// reads `CLAUDE_CONFIG_DIR` is the binary `claude`, whatever anyone names it,
/// so the binary is the honest link. `None` for a binary this table does not
/// know — a hand-written `sh` in a step has no home to move.
pub fn cli_for_executable(bin: &str) -> Option<&'static KnownCli> {
    // The last segment and nothing else. A `claude-wrapper` handed Claude's home
    // would start with someone else's credentials, invisibly from the step.
    let name = Path::new(bin).file_name()?.to_str()?;
    known_clis().iter().find(|cli| cli.executable == name)
}

/// A profile: the name its owner chose, which command line it belongs to, and
/// where its home directory sits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub cli_id: String,
    pub home_dir: PathBuf,
    /// Another endpoint for this profile's command line; absent for the maker's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<ProfileEndpoint>,
}

/// The list of profiles, and which one is active for each command line.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileStore {
    #[serde(default)]
    pub profiles: Vec<Profile>,
    /// `cli_id` -> name of the active profile.
    #[serde(default)]
    pub active: BTreeMap<String, String>,
}

/// An empty string counts as an empty store: it is the shape of a file never
/// written.
pub fn parse_store(json: &str) -> Result<ProfileStore, serde_json::Error> {
    if json.trim().is_empty() {
        return Ok(ProfileStore::default());
    }
    serde_json::from_str(json)
}

pub fn serialize_store(store: &ProfileStore) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(store)
}

/// Why a profile name will not do. Any of these, uncaught, is a security fault:
/// a name that leaves the profiles directory.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProfileNameError {
    Empty,
    /// Contains `/` or `\`. That alone stops both `../escape` and an absolute
    /// name, which `Path::join` would take as a replacement for the whole path.
    PathSeparator,
    Traversal,
}

impl fmt::Debug for ProfileNameError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, out)
    }
}

impl fmt::Display for ProfileNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "the profile name is empty"),
            Self::PathSeparator => write!(f, "the profile name contains a path separator"),
            Self::Traversal => write!(f, "the profile name is '.' or '..'"),
        }
    }
}

/// No `/`, no `\`, no `.`/`..`, no empty name: a name chosen by a person becomes
/// a path segment, never a path.
pub fn validate_profile_name(name: &str) -> Result<(), ProfileNameError> {
    if name.is_empty() {
        return Err(ProfileNameError::Empty);
    }
    if name.contains('/') || name.contains('\\') {
        return Err(ProfileNameError::PathSeparator);
    }
    if name == "." || name == ".." {
        return Err(ProfileNameError::Traversal);
    }
    Ok(())
}

/// The home this command line already keeps here, when no variable moves it.
/// **READ WHERE THEY ARE, NEVER COPIED**: one rotating refresh token in two
/// homes invalidates both. `None` where nobody established where it is.
pub fn existing_home(cli: &KnownCli, home: &Path) -> Option<PathBuf> {
    cli.home_already_here
        .as_deref()
        .map(|below| home.join(below))
}

/// Where the files `cli` reads at its start sit on this machine. A path under
/// `~` is under `home`, except that a profile moving the engine's home by
/// variable takes along what sits in its usual place: the engine reads the
/// file where it is started, not where it would have been.
pub fn instruction_files(
    cli: &KnownCli,
    project_root: &Path,
    home: &Path,
    profile_home: Option<&Path>,
) -> Vec<PathBuf> {
    let moved = profile_home.filter(|_| matches!(cli.home, HomeMechanism::EnvVar(_)));
    cli.reads_instructions_from
        .iter()
        .map(|declared| {
            let Some(under_home) = declared.strip_prefix("~/") else {
                return project_root.join(declared);
            };
            let inside_the_moved_home = moved.zip(cli.home_already_here.as_deref()).and_then(
                |(profile_home, usual)| {
                    let inside = under_home.strip_prefix(usual)?.strip_prefix('/')?;
                    Some(profile_home.join(inside))
                },
            );
            inside_the_moved_home.unwrap_or_else(|| home.join(under_home))
        })
        .collect()
}

/// Where a profile's home sits inside the profiles root. Validates both
/// `cli_id` and `profile_name`: each becomes a segment.
pub fn profile_home_path(
    profiles_root: &Path,
    cli_id: &str,
    profile_name: &str,
) -> Result<PathBuf, ProfileNameError> {
    validate_profile_name(cli_id)?;
    validate_profile_name(profile_name)?;
    Ok(profiles_root.join(cli_id).join(profile_name))
}

/// The environment to overlay to launch `cli` with its home at `profile_home`.
/// Empty when the mechanism uses no variable: there the swap is a filesystem
/// operation, see [`symlink_swap`].
pub fn build_environment(cli: &KnownCli, profile_home: &Path) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    if let HomeMechanism::EnvVar(name) = &cli.home {
        env.insert(name.clone(), profile_home.to_string_lossy().into_owned());
    }
    env
}

/// The environment a terminal opens with so that every command line inside it
/// runs under its active profile: one variable per command line whose home
/// moves by variable and whose store names an active profile.
///
/// Empty for a store with nothing active, and silent about a command line
/// whose home does not move: there is no variable to set, and setting a made-up
/// one would promise a switch that does nothing.
pub fn active_environment(store: &ProfileStore) -> Vec<(String, String)> {
    active_environment_with(store, &|name| std::env::var(name).ok()).environment
}

/// What a terminal opens with, and what it could not be given: an endpoint
/// dropped in silence leaves the terminal on the subscription.
pub struct ActiveEnvironment {
    pub environment: Vec<(String, String)>,
    pub refused: Vec<String>,
}

pub fn active_environment_with(
    store: &ProfileStore,
    key_of: &dyn Fn(&str) -> Option<String>,
) -> ActiveEnvironment {
    let mut environment = Vec::new();
    let mut refused = Vec::new();
    for (cli_id, name) in &store.active {
        let Ok(cli) = find_cli(cli_id) else {
            continue;
        };
        let Some(profile) = store
            .profiles
            .iter()
            .find(|profile| &profile.cli_id == cli_id && &profile.name == name)
        else {
            continue;
        };
        environment.extend(build_environment(cli, &profile.home_dir));
        match endpoint_environment(cli, profile, key_of) {
            Ok(endpoint) => environment.extend(endpoint),
            Err(why) => refused.push(why),
        }
    }
    ActiveEnvironment { environment, refused }
}

/// The two paths a symlink swap involves: where the link sits inside the fixed
/// home, and where it must point to reach this profile's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymlinkSwap {
    pub link_path: PathBuf,
    pub target_path: PathBuf,
}

pub fn symlink_swap(fixed_home: &Path, relative_path: &str, profile_home: &Path) -> SymlinkSwap {
    SymlinkSwap {
        link_path: fixed_home.join(relative_path),
        target_path: profile_home.join(relative_path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_profile_name_rejects_empty() {
        assert_eq!(validate_profile_name(""), Err(ProfileNameError::Empty));
    }

    #[test]
    fn validate_profile_name_rejects_dot_and_dotdot() {
        assert_eq!(validate_profile_name("."), Err(ProfileNameError::Traversal));
        assert_eq!(
            validate_profile_name(".."),
            Err(ProfileNameError::Traversal)
        );
    }

    #[test]
    fn validate_profile_name_rejects_path_separators() {
        assert_eq!(
            validate_profile_name("a/b"),
            Err(ProfileNameError::PathSeparator)
        );
        assert_eq!(
            validate_profile_name("a\\b"),
            Err(ProfileNameError::PathSeparator)
        );
    }

    #[test]
    fn validate_profile_name_accepts_ordinary_names() {
        assert_eq!(validate_profile_name("work"), Ok(()));
        assert_eq!(validate_profile_name("client-1"), Ok(()));
        assert_eq!(validate_profile_name("a.b"), Ok(()));
    }

    /// Names built to leave the profiles directory — traversal and absolute —
    /// are all refused before they reach `Path::join`, where an absolute name
    /// would replace the whole path instead of extending it.
    #[test]
    fn profile_home_path_rejects_every_escape_attempt() {
        let root = Path::new("/var/profiles");
        let malicious = [
            "../../etc/passwd",
            "..",
            "/etc/passwd",
            "sub/../../escape",
            "",
        ];
        for name in malicious {
            assert!(
                profile_home_path(root, "claude", name).is_err(),
                "expected a refusal for {name:?}"
            );
        }
    }

    #[test]
    fn profile_home_path_stays_inside_the_root_for_a_valid_name() {
        let root = Path::new("/var/profiles");
        let home = profile_home_path(root, "claude", "work").unwrap();
        assert!(home.starts_with(root));
        assert_eq!(home, root.join("claude").join("work"));
    }

    #[test]
    fn build_environment_sets_the_env_var_for_the_env_mechanism() {
        let cli = known_clis().iter().find(|c| c.id == "codex").unwrap();
        let env = build_environment(cli, Path::new("/home/profiles/codex/work"));
        assert_eq!(
            env.get("CODEX_HOME").map(String::as_str),
            Some("/home/profiles/codex/work")
        );
        assert_eq!(env.len(), 1);
    }

    #[test]
    fn build_environment_is_empty_without_an_env_var_mechanism() {
        let cli = KnownCli {
            id: "una-casa".to_owned(),
            display_name: "Una Casa".to_owned(),
            executable: "unacasa".to_owned(),
            native_profiles: NativeProfiles::NotSupported,
            native_profiles_note: "a fixture".to_owned(),
            home: HomeMechanism::CredentialSymlink {
                relative_path: "credentials.json".to_owned(),
            },
            home_note: "a fixture".to_owned(),
            home_already_here: None,
            endpoint: None,
            reads_instructions_from: Vec::new(),
        };
        let env = build_environment(&cli, Path::new("/home/profiles/acme/work"));
        assert!(env.is_empty());
    }

    /// **THE TERMINAL'S ENVIRONMENT IS THE ACTIVE PROFILE'S HOME, PER COMMAND
    /// LINE.** Two command lines, two variables; a profile that exists but is
    /// not active sets nothing; an active name with no profile behind it sets
    /// nothing rather than a path invented from the name.
    #[test]
    fn the_active_profiles_become_the_variables_a_terminal_opens_with() {
        let mut store = ProfileStore::default();
        for (cli, name) in [("claude", "prove"), ("claude", "work"), ("codex", "work")] {
            store.profiles.push(Profile {
                name: name.to_owned(),
                cli_id: cli.to_owned(),
                home_dir: PathBuf::from(format!("/homes/{cli}/{name}")),
                endpoint: None,
            });
        }
        store.active.insert("claude".to_owned(), "prove".to_owned());
        store.active.insert("codex".to_owned(), "work".to_owned());
        store.active.insert("gemini".to_owned(), "nobody".to_owned());

        let mut environment = active_environment(&store);
        environment.sort();
        assert_eq!(
            environment,
            vec![
                ("CLAUDE_CONFIG_DIR".to_owned(), "/homes/claude/prove".to_owned()),
                ("CODEX_HOME".to_owned(), "/homes/codex/work".to_owned()),
            ]
        );

        // The absurd control: nothing active, nothing set.
        assert!(active_environment(&ProfileStore::default()).is_empty());
    }

    #[test]
    fn symlink_swap_composes_the_two_paths() {
        let swap = symlink_swap(
            Path::new("/home/someone/.acme"),
            "credentials.json",
            Path::new("/home/profiles/acme/work"),
        );
        assert_eq!(
            swap.link_path,
            Path::new("/home/someone/.acme/credentials.json")
        );
        assert_eq!(
            swap.target_path,
            Path::new("/home/profiles/acme/work/credentials.json")
        );
    }

    #[test]
    fn store_roundtrip_through_json() {
        let mut store = ProfileStore::default();
        store.profiles.push(Profile {
            name: "work".to_owned(),
            cli_id: "claude".to_owned(),
            home_dir: PathBuf::from("/home/profiles/claude/work"),
            endpoint: None,
        });
        store.active.insert("claude".to_owned(), "work".to_owned());

        let json = serialize_store(&store).unwrap();
        let parsed = parse_store(&json).unwrap();
        assert_eq!(parsed, store);
    }

    #[test]
    fn parse_store_treats_empty_string_as_empty_store() {
        assert_eq!(parse_store("").unwrap(), ProfileStore::default());
        assert_eq!(parse_store("   \n").unwrap(), ProfileStore::default());
    }

    #[test]
    fn find_cli_finds_a_known_id_and_rejects_an_unknown_one() {
        assert_eq!(find_cli("codex").map(|c| c.id.as_str()), Ok("codex"));
        assert!(find_cli("does-not-exist").is_err());
    }

    /// **A RESOLVED PATH LEADS BACK TO THE COMMAND LINE THAT READS THAT HOME.**
    /// All four arms count: the absolute path is the real shape the resolver
    /// returns, the bare name is what a hand-written `bin` gives, and the two
    /// refusals say nothing is guessed — `claude-code` is the **descriptor's**
    /// id, not the executable's name.
    #[test]
    fn a_resolved_path_leads_back_to_the_command_line_that_reads_that_home() {
        let named = |bin| cli_for_executable(bin).map(|cli| cli.id.as_str());
        assert_eq!(named("/opt/homebrew/bin/claude"), Some("claude"));
        assert_eq!(named("codex"), Some("codex"));
        assert_eq!(named("/bin/sh"), None);
        assert_eq!(named("claude-code"), None);
    }

    /// A prefix comparison would hand `claude-wrapper` Claude Code's home: one
    /// command line launched with another's credentials, in silence.
    #[test]
    fn a_name_that_merely_resembles_an_executable_is_not_that_executable() {
        for near in ["claude-wrapper", "myclaude", "codexx", "gemini2"] {
            assert_eq!(
                cli_for_executable(near).map(|cli| cli.id.as_str()),
                None,
                "«{near}» is not an executable in the table"
            );
        }
    }

    #[test]
    fn known_clis_have_unique_non_empty_ids() {
        let clis = known_clis();
        assert!(!clis.is_empty());
        let mut ids: Vec<&str> = clis.iter().map(|c| c.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), clis.len(), "duplicate id in the table");
        for cli in clis {
            assert!(!cli.id.is_empty());
            assert!(!cli.executable.is_empty());
        }
    }
}
