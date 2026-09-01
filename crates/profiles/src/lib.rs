//! Several profiles per known command line, even where that command line has
//! none of its own.
//!
//! Where the command line reads an environment variable for its home, a profile
//! is just a directory: switching is setting a variable, with no copies and
//! nothing to overwrite. Where it does not, the fallback is a symlink on the
//! credentials file — more fragile, and marked as such.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub mod store_io;

/// How a command line finds its own home directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeMechanism {
    /// This variable moves the whole home directory.
    EnvVar(&'static str),
    /// No known variable: the profile swaps a symlink at this path, relative to
    /// the fixed home.
    CredentialSymlink { relative_path: &'static str },
    /// Not established: nobody has checked how this command line moves its home,
    /// or whether it can.
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

/// A known command line: how it is invoked and how its home moves. `known_clis`
/// is the declared table — extend it by adding an entry, nothing else.
#[derive(Debug, Clone, Copy)]
pub struct KnownCli {
    pub id: &'static str,
    pub display_name: &'static str,
    pub executable: &'static str,
    pub native_profiles: NativeProfiles,
    /// How the judgement above was reached: what the real command says, or why
    /// it was not checked.
    pub native_profiles_note: &'static str,
    pub home: HomeMechanism,
    pub home_note: &'static str,
}

const KNOWN_CLIS: &[KnownCli] = &[
    KnownCli {
        id: "claude",
        display_name: "Claude Code",
        executable: "claude",
        native_profiles: NativeProfiles::NotSupported,
        native_profiles_note: "checked on claude 2.1.247: `claude auth` offers only login/logout/status, and `--help` names no profile or multi-account subcommand.",
        home: HomeMechanism::EnvVar("CLAUDE_CONFIG_DIR"),
        home_note: "checked against the installed binary: the variable moves the whole directory, `.credentials.json` and `settings.json` included.",
    },
    KnownCli {
        id: "codex",
        display_name: "Codex",
        executable: "codex",
        native_profiles: NativeProfiles::Supported,
        native_profiles_note: "`-p/--profile` in `codex --help` layers `$CODEX_HOME/<name>.config.toml` over the base configuration — config profiles, not separate credentials.",
        home: HomeMechanism::EnvVar("CODEX_HOME"),
        home_note: "checked with `codex doctor`: it shows auth.json and config.toml inside the directory CODEX_HOME names.",
    },
    KnownCli {
        id: "gemini",
        display_name: "Gemini CLI",
        executable: "gemini",
        native_profiles: NativeProfiles::NotSupported,
        native_profiles_note: "no `--profile` in `gemini --help`: only sessions (`--resume`, `--session-id`), not separate identities.",
        home: HomeMechanism::EnvVar("GEMINI_CLI_HOME"),
        home_note: "checked against the installed source: `baseDir = process.env[\"GEMINI_CLI_HOME\"] || join(homedir, \".gemini\")`.",
    },
    KnownCli {
        id: "antigravity",
        display_name: "Antigravity",
        executable: "antigravity",
        native_profiles: NativeProfiles::Unverified,
        native_profiles_note: "no `antigravity` binary in PATH: the product installs as `agy`, so this entry looks for a name nobody uses. Native profiles stay unchecked — `agy --help` and its subcommands name none.",
        home: HomeMechanism::Unknown,
        home_note: "its data lives under the Gemini CLI's directory, but `GEMINI_CLI_HOME` does NOT move it: the string is absent from the binary and the home follows $HOME. So the active profile does not move this one's home the way it moves claude's and codex's — two profiles start it in the same place, silently. `Unknown` is still the right value for «no known way», but this is not «not checked yet»: it is checked, and there is no way.",
    },
];

/// The table of known command lines.
pub fn known_clis() -> &'static [KnownCli] {
    KNOWN_CLIS
}

/// The command line carrying this `id`, or a readable refusal. One place:
/// `sailor profiles` and `sailor run` both look here, not in two copies of the
/// same `.find()`.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileNameError {
    Empty,
    /// Contains `/` or `\`. That alone stops both `../escape` and an absolute
    /// name, which `Path::join` would take as a replacement for the whole path.
    PathSeparator,
    Traversal,
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
    if let HomeMechanism::EnvVar(name) = cli.home {
        env.insert(name.to_owned(), profile_home.to_string_lossy().into_owned());
    }
    env
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
            id: "acme",
            display_name: "Acme CLI",
            executable: "acme",
            native_profiles: NativeProfiles::NotSupported,
            native_profiles_note: "a fixture",
            home: HomeMechanism::CredentialSymlink {
                relative_path: "credentials.json",
            },
            home_note: "a fixture",
        };
        let env = build_environment(&cli, Path::new("/home/profiles/acme/work"));
        assert!(env.is_empty());
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
        assert_eq!(find_cli("codex").map(|c| c.id), Ok("codex"));
        assert!(find_cli("does-not-exist").is_err());
    }

    /// **A RESOLVED PATH LEADS BACK TO THE COMMAND LINE THAT READS THAT HOME.**
    /// All four arms count: the absolute path is the real shape the resolver
    /// returns, the bare name is what a hand-written `bin` gives, and the two
    /// refusals say nothing is guessed — `claude-code` is the **descriptor's**
    /// id, not the executable's name.
    #[test]
    fn a_resolved_path_leads_back_to_the_command_line_that_reads_that_home() {
        assert_eq!(
            cli_for_executable("/opt/homebrew/bin/claude").map(|c| c.id),
            Some("claude")
        );
        assert_eq!(cli_for_executable("codex").map(|c| c.id), Some("codex"));
        assert_eq!(cli_for_executable("/bin/sh").map(|c| c.id), None);
        assert_eq!(cli_for_executable("claude-code").map(|c| c.id), None);
    }

    /// A prefix comparison would hand `claude-wrapper` Claude Code's home: one
    /// command line launched with another's credentials, in silence.
    #[test]
    fn a_name_that_merely_resembles_an_executable_is_not_that_executable() {
        for near in ["claude-wrapper", "myclaude", "codexx", "gemini2"] {
            assert_eq!(
                cli_for_executable(near).map(|cli| cli.id),
                None,
                "«{near}» is not an executable in the table"
            );
        }
    }

    #[test]
    fn known_clis_have_unique_non_empty_ids() {
        let clis = known_clis();
        assert!(!clis.is_empty());
        let mut ids: Vec<&str> = clis.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), clis.len(), "duplicate id in the table");
        for cli in clis {
            assert!(!cli.id.is_empty());
            assert!(!cli.executable.is_empty());
        }
    }
}
