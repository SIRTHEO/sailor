//! Retiring the identity a code index keeps for a tree that was taken down.
//!
//! The index is asked for its inventory first and told to delete second, with
//! the token that inventory handed out: a stale token, an identity mid-write
//! or one held for a person's inspection is refused by the index itself.

use actions::mcp::{ask_once, Asked, ServerSpec};
use serde_json::json;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use workspace::index_identity::{identity_of, IdentityRule, IndexIdentity};
use workspace::{list, Worktree};

/// Where a tree declares the command line of the server that indexes it, the
/// way `sailor.pushAs` declares who pushes. The value is argv-shaped: words
/// split on whitespace, the first being the command.
pub const INDEX_SERVER: &str = "sailor.indexServer";
/// The tool the index offers over its own inventory: a report with no
/// arguments, a deletion with a fresh token.
pub const PRUNE_TOOL: &str = "codebase_prune";
const TWO_MINUTES: u64 = 120;

const IDENTITY_LINE: &str = "Identity: ";
const TOKEN_LINE: &str = "Confirmation token: ";
const IN_PROGRESS_LINE: &str = "Indexing in progress";
const MANUAL_LINE: &str = "Manual inspection required:";
const REMOVED_ALL: &str = "Removed all";

/// What the repository says about its index server. Three answers, because
/// a `git config` that fails is not a repository that declares nothing.
#[derive(Debug, Clone)]
pub enum IndexServer {
    Declared(ServerSpec),
    NotDeclared,
    CouldNotRead(String),
}

/// What the machine says about its index, read once by the command line and
/// handed in everywhere else.
#[derive(Debug, Clone)]
pub struct IndexTending {
    pub server: IndexServer,
    pub rule: IdentityRule,
    /// Why the repository's own declaration could not be read, when it could
    /// not. Carried rather than thrown away: falling back to «no convention»
    /// would go on naming trees, by a rule nobody chose.
    pub convention_refused: Option<String>,
    pub timeout: Duration,
}

impl IndexTending {
    pub fn of(repo: &Path) -> IndexTending {
        let (rule, refused) = match IdentityRule::declared_by(repo) {
            Ok(rule) => (rule, None),
            Err(why) => (IdentityRule::default(), Some(why)),
        };
        IndexTending {
            server: index_server_of(repo),
            rule,
            convention_refused: refused,
            timeout: Duration::from_secs(TWO_MINUTES),
        }
    }

    pub fn with(server: IndexServer, rule: IdentityRule, timeout: Duration) -> IndexTending {
        IndexTending {
            server,
            rule,
            convention_refused: None,
            timeout,
        }
    }

    pub fn identity_of(&self, tree: &Worktree) -> Result<IndexIdentity, String> {
        if let Some(why) = &self.convention_refused {
            return Err(why.clone());
        }
        identity_of(Path::new(&tree.path), tree.branch.as_deref(), &self.rule)
    }
}

/// `git config --get` exits 1 for a key nobody set and otherwise for a
/// configuration it could not read: the two are kept apart.
fn index_server_of(repo: &Path) -> IndexServer {
    let read = match Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["config", "--get", INDEX_SERVER])
        .output()
    {
        Ok(read) => read,
        Err(error) => return IndexServer::CouldNotRead(error.to_string()),
    };
    if read.status.code() == Some(1) {
        return IndexServer::NotDeclared;
    }
    if !read.status.success() {
        return IndexServer::CouldNotRead(String::from_utf8_lossy(&read.stderr).trim().to_owned());
    }
    let line = String::from_utf8_lossy(&read.stdout);
    let mut words = line.split_whitespace().map(str::to_owned);
    match words.next() {
        Some(command) => IndexServer::Declared(ServerSpec {
            command,
            args: words.collect(),
            env: Default::default(),
            cwd: None,
        }),
        None => IndexServer::NotDeclared,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retired {
    Retired { identity: String },
    NoServerDeclared { identity: String },
    ServerUnreadable { identity: String, why: String },
    StillCarriedBy { identity: String, tree: String },
    CouldNotLook { identity: String, repo: String, why: String },
    NoEntry { identity: String },
    Refused { identity: String, why: String },
    CouldNotAsk { identity: String, why: String },
}

impl Retired {
    pub fn render(&self) -> String {
        match self {
            Retired::Retired { identity } => {
                catalogue::say("cli.worktree.index_retired", &[("identity", identity)])
            }
            Retired::NoServerDeclared { identity } => catalogue::say(
                "cli.worktree.index_no_server",
                &[("identity", identity), ("key", INDEX_SERVER)],
            ),
            Retired::ServerUnreadable { identity, why } => catalogue::say(
                "cli.worktree.index_server_unreadable",
                &[("identity", identity), ("key", INDEX_SERVER), ("why", why.trim())],
            ),
            Retired::StillCarriedBy { identity, tree } => catalogue::say(
                "cli.worktree.index_still_carried",
                &[("identity", identity), ("tree", tree)],
            ),
            Retired::CouldNotLook {
                identity,
                repo,
                why,
            } => catalogue::say(
                "cli.worktree.index_could_not_list",
                &[("identity", identity), ("repo", repo), ("why", why.trim())],
            ),
            Retired::NoEntry { identity } => {
                catalogue::say("cli.worktree.index_no_entry", &[("identity", identity)])
            }
            Retired::Refused { identity, why } => catalogue::say(
                "cli.worktree.index_refused",
                &[("identity", identity), ("why", why.trim())],
            ),
            Retired::CouldNotAsk { identity, why } => catalogue::say(
                "cli.worktree.index_could_not_ask",
                &[("identity", identity), ("why", why.trim())],
            ),
        }
    }
}

/// A tree of `repo` still standing under the same identity, if any: a pin is
/// shared by design, and retiring it would empty the index of the trees left.
/// A git that cannot list is an error, never a repository with no trees.
pub fn still_carried_by(
    repo: &Path,
    identity: &str,
    tending: &IndexTending,
) -> Result<Option<String>, String> {
    Ok(list(repo)?
        .into_iter()
        .find(|tree| tending.identity_of(tree).is_ok_and(|held| held.id == identity))
        .map(|tree| tree.path))
}

/// Retires `identity` from the index, once no tree of `repo` carries it.
pub fn retire(repo: &Path, identity: &str, tending: &IndexTending) -> Retired {
    let identity = identity.to_owned();
    match still_carried_by(repo, &identity, tending) {
        Ok(Some(tree)) => return Retired::StillCarriedBy { identity, tree },
        Ok(None) => {}
        Err(why) => {
            return Retired::CouldNotLook {
                identity,
                repo: repo.to_string_lossy().into_owned(),
                why,
            }
        }
    }
    let server = match &tending.server {
        IndexServer::Declared(server) => server,
        IndexServer::NotDeclared => return Retired::NoServerDeclared { identity },
        IndexServer::CouldNotRead(why) => {
            return Retired::ServerUnreadable {
                identity,
                why: why.clone(),
            }
        }
    };
    let report = match ask_once(server, PRUNE_TOOL, &json!({}), tending.timeout) {
        Asked::Said {
            text,
            refused: false,
        } => text,
        Asked::Said { text, refused: true } => {
            return Retired::CouldNotAsk {
                identity,
                why: text,
            }
        }
        Asked::Unreachable(why)
        | Asked::NotOffered(why)
        | Asked::CouldNotLook(why)
        | Asked::Unanswered(why) => return Retired::CouldNotAsk { identity, why },
    };
    let Some(entry) = entry_of(&report, &identity) else {
        return Retired::NoEntry { identity };
    };
    if let Some(why) = entry.held_back {
        return Retired::Refused { identity, why };
    }
    let apply = json!({
        "apply": true,
        "identity": identity,
        "confirmationToken": entry.token,
        "acknowledgeNoRemoteWriters": true,
    });
    match ask_once(server, PRUNE_TOOL, &apply, tending.timeout) {
        Asked::Said {
            text,
            refused: false,
        } if text.starts_with(REMOVED_ALL) => Retired::Retired { identity },
        Asked::Said { text, .. } => Retired::Refused {
            identity,
            why: text,
        },
        Asked::Unreachable(why)
        | Asked::NotOffered(why)
        | Asked::CouldNotLook(why)
        | Asked::Unanswered(why) => Retired::CouldNotAsk { identity, why },
    }
}

struct Entry {
    token: String,
    held_back: Option<String>,
}

/// The inventory's entry for one identity: its token, and what holds it back.
fn entry_of(report: &str, identity: &str) -> Option<Entry> {
    let mut inside = false;
    let mut token = None;
    let mut held_back = None;
    for line in report.lines() {
        if let Some(named) = line.strip_prefix(IDENTITY_LINE) {
            inside = named.trim() == identity;
            continue;
        }
        if !inside {
            continue;
        }
        let line = line.trim();
        if let Some(found) = line.strip_prefix(TOKEN_LINE) {
            token = Some(found.trim().to_owned());
        } else if line.starts_with(IN_PROGRESS_LINE) || line.starts_with(MANUAL_LINE) {
            held_back.get_or_insert_with(|| line.to_owned());
        }
    }
    Some(Entry {
        token: token?,
        held_back,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = "Stored project inventory:\n\nIdentity: aaaa\n  Path: /gone\n  Confirmation token: tok-a\n\nIdentity: bbbb\n  Path: /busy\n  Indexing in progress: a metadata record is mid-write\n  Confirmation token: tok-b\n\nIdentity: cccc\n  Confirmation token: tok-c\n  Manual inspection required: two collections claim it\n";

    #[test]
    fn the_entry_is_read_with_its_own_token_and_nobody_elses() {
        let a = entry_of(REPORT, "aaaa").expect("the entry");
        let b = entry_of(REPORT, "bbbb").expect("the entry");
        let c = entry_of(REPORT, "cccc").expect("the entry");
        assert_eq!(a.token, "tok-a");
        assert!(a.held_back.is_none());
        assert_eq!(b.token, "tok-b");
        assert!(b.held_back.is_some_and(|why| why.starts_with(IN_PROGRESS_LINE)));
        assert!(c.held_back.is_some_and(|why| why.starts_with(MANUAL_LINE)));
        assert!(entry_of(REPORT, "dddd").is_none());
        assert!(entry_of("No stored project identities found.", "aaaa").is_none());
    }
}
