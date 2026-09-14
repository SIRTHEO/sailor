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
/// way `sailor.pushAs` declares who pushes.
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

/// What the machine says about its index, read once by the command line and
/// handed in everywhere else.
pub struct IndexTending {
    pub server: Option<ServerSpec>,
    pub rule: IdentityRule,
    pub timeout: Duration,
}

impl IndexTending {
    pub fn of(repo: &Path) -> IndexTending {
        IndexTending {
            server: index_server_of(repo),
            rule: IdentityRule::from_environment(),
            timeout: Duration::from_secs(TWO_MINUTES),
        }
    }

    pub fn with(server: Option<ServerSpec>, rule: IdentityRule, timeout: Duration) -> IndexTending {
        IndexTending {
            server,
            rule,
            timeout,
        }
    }

    pub fn identity_of(&self, tree: &Worktree) -> Result<IndexIdentity, String> {
        identity_of(Path::new(&tree.path), tree.branch.as_deref(), &self.rule)
    }
}

fn index_server_of(repo: &Path) -> Option<ServerSpec> {
    let read = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["config", "--get", INDEX_SERVER])
        .output()
        .ok()?;
    if !read.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&read.stdout);
    let mut words = line.split_whitespace().map(str::to_owned);
    let command = words.next()?;
    Some(ServerSpec {
        command,
        args: words.collect(),
        env: Default::default(),
        cwd: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retired {
    Retired { identity: String },
    NoServerDeclared { identity: String },
    StillCarriedBy { identity: String, tree: String },
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
            Retired::StillCarriedBy { identity, tree } => catalogue::say(
                "cli.worktree.index_still_carried",
                &[("identity", identity), ("tree", tree)],
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
pub fn still_carried_by(repo: &Path, identity: &str, tending: &IndexTending) -> Option<String> {
    list(repo)
        .unwrap_or_default()
        .into_iter()
        .find(|tree| tending.identity_of(tree).is_ok_and(|held| held.id == identity))
        .map(|tree| tree.path)
}

/// Retires `identity` from the index, once no tree of `repo` carries it.
pub fn retire(repo: &Path, identity: &str, tending: &IndexTending) -> Retired {
    let identity = identity.to_owned();
    if let Some(tree) = still_carried_by(repo, &identity, tending) {
        return Retired::StillCarriedBy { identity, tree };
    }
    let Some(server) = &tending.server else {
        return Retired::NoServerDeclared { identity };
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
