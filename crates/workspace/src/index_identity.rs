//! The identity a code index keeps for a tree, computed the way the index
//! computes it: a pin from the environment, else a pin from a file in the
//! tree, else the first twelve hex digits of the SHA-256 of the resolved path.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

/// The file a tree pins its identity in. Committed, it is shared by every
/// checkout of the repository: that is what a pin is for.
pub const PIN_FILE: &str = ".socraticode.json";
/// The variable that pins the identity of every tree this process names.
pub const PIN_VARIABLE: &str = "SOCRATICODE_PROJECT_ID";
/// When `true`, an unpinned identity carries the branch as a suffix.
pub const BRANCH_AWARE_VARIABLE: &str = "SOCRATICODE_BRANCH_AWARE";

/// What the process running the index tells it, handed in so that a test
/// arranges it instead of the machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityRule {
    pub pinned_by_environment: Option<String>,
    pub branch_aware: bool,
}

impl IdentityRule {
    pub fn from_environment() -> IdentityRule {
        let pinned = std::env::var(PIN_VARIABLE)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let branch_aware = std::env::var(BRANCH_AWARE_VARIABLE).is_ok_and(|value| value == "true");
        IdentityRule {
            pinned_by_environment: pinned,
            branch_aware,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexIdentity {
    pub id: String,
    /// A pinned identity is declared, not derived, and every tree declaring
    /// the same pin addresses the same index.
    pub pinned: bool,
}

#[derive(Deserialize)]
struct Pin {
    #[serde(rename = "projectId")]
    project_id: Option<String>,
}

/// The identity the index keeps for `tree`, given the branch it stands on.
pub fn identity_of(
    tree: &Path,
    branch: Option<&str>,
    rule: &IdentityRule,
) -> Result<IndexIdentity, String> {
    if let Some(pinned) = &rule.pinned_by_environment {
        valid(pinned, PIN_VARIABLE)?;
        return Ok(IndexIdentity {
            id: pinned.clone(),
            pinned: true,
        });
    }
    if let Some(pinned) = pinned_in(tree)? {
        return Ok(IndexIdentity {
            id: pinned,
            pinned: true,
        });
    }
    let digest = Sha256::digest(resolve_like_the_index(tree).as_bytes());
    let mut id: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    id.truncate(12);
    if rule.branch_aware {
        if let Some(suffix) = branch.map(sanitized_branch).filter(|name| !name.is_empty()) {
            id = format!("{id}__{suffix}");
        }
    }
    Ok(IndexIdentity { id, pinned: false })
}

/// The pin the tree's own file declares, if it declares one the index
/// would accept. A file that is missing, malformed or silent pins nothing.
fn pinned_in(tree: &Path) -> Result<Option<String>, String> {
    let Ok(text) = std::fs::read_to_string(tree.join(PIN_FILE)) else {
        return Ok(None);
    };
    let Ok(pin) = serde_json::from_str::<Pin>(&text) else {
        return Ok(None);
    };
    let Some(declared) = pin.project_id.map(|id| id.trim().to_owned()) else {
        return Ok(None);
    };
    if declared.is_empty() {
        return Ok(None);
    }
    valid(&declared, PIN_FILE)?;
    Ok(Some(declared))
}

fn valid(id: &str, source: &str) -> Result<(), String> {
    if id
        .chars()
        .all(|letter| letter.is_ascii_alphanumeric() || letter == '_' || letter == '-')
    {
        Ok(())
    } else {
        Err(format!("{source} pins «{id}», and the index refuses it: only letters, digits, «_» and «-» are allowed"))
    }
}

/// The path as the index resolves it: absolute, `.` and `..` folded, no
/// doubled or trailing separator, and links left exactly as written.
pub fn resolve_like_the_index(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut parts: Vec<String> = Vec::new();
    for component in absolute.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::ParentDir => {
                parts.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    format!("/{}", parts.join("/"))
}

fn sanitized_branch(branch: &str) -> String {
    let mut out = String::with_capacity(branch.len());
    for letter in branch.chars() {
        let kept = if letter.is_ascii_alphanumeric() || letter == '_' || letter == '-' {
            letter
        } else {
            '_'
        };
        if kept == '_' && out.ends_with('_') {
            continue;
        }
        out.push(kept);
    }
    out.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_scratch(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sailor-index-identity-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch");
        path
    }

    /// The vector was computed outside this crate, on the same rule the index
    /// applies: `sha256("/somewhere/project-worktrees/one")` truncated to twelve.
    #[test]
    fn an_unpinned_tree_is_the_hash_of_its_resolved_path() {
        let identity = identity_of(
            Path::new("/somewhere/project-worktrees/one"),
            Some("work/topic"),
            &IdentityRule::default(),
        )
        .expect("an identity");
        assert_eq!(identity.id, "c6e9f9d721e2");
        assert!(!identity.pinned);
    }

    #[test]
    fn a_pin_in_the_tree_wins_over_the_path() {
        let scratch = a_scratch("pinned");
        std::fs::write(scratch.join(PIN_FILE), "{\"projectId\": \" the-pin \"}\n")
            .expect("the pin");
        let identity = identity_of(&scratch, None, &IdentityRule::default()).expect("an identity");
        let _ = std::fs::remove_dir_all(&scratch);
        assert_eq!(identity.id, "the-pin");
        assert!(identity.pinned);
    }

    #[test]
    fn a_pin_in_the_environment_wins_over_the_file() {
        let scratch = a_scratch("environment");
        std::fs::write(scratch.join(PIN_FILE), "{\"projectId\": \"the-pin\"}\n").expect("the pin");
        let rule = IdentityRule {
            pinned_by_environment: Some("from-outside".to_owned()),
            branch_aware: true,
        };
        let identity = identity_of(&scratch, Some("work/topic"), &rule).expect("an identity");
        let _ = std::fs::remove_dir_all(&scratch);
        assert_eq!(identity.id, "from-outside");
    }

    /// A silent or malformed pin file pins nothing; a pin the index would
    /// refuse is refused here too, instead of naming an identity nobody holds.
    #[test]
    fn a_silent_pin_falls_to_the_path_and_a_bad_one_is_refused() {
        let scratch = a_scratch("silent");
        std::fs::write(scratch.join(PIN_FILE), "{\"projectId\": \"  \"}").expect("the pin");
        let silent = identity_of(&scratch, None, &IdentityRule::default()).expect("an identity");
        std::fs::write(scratch.join(PIN_FILE), "not json").expect("the pin");
        let broken = identity_of(&scratch, None, &IdentityRule::default()).expect("an identity");
        std::fs::write(scratch.join(PIN_FILE), "{\"projectId\": \"no spaces\"}").expect("the pin");
        let refused = identity_of(&scratch, None, &IdentityRule::default());
        let _ = std::fs::remove_dir_all(&scratch);
        assert!(!silent.pinned);
        assert_eq!(silent, broken);
        assert!(refused.is_err(), "{refused:?}");
    }

    #[test]
    fn branch_aware_appends_the_branch_the_way_the_index_writes_it() {
        let rule = IdentityRule {
            pinned_by_environment: None,
            branch_aware: true,
        };
        let on_a_branch = identity_of(
            Path::new("/somewhere/project-worktrees/one"),
            Some("work/topic"),
            &rule,
        )
        .expect("an identity");
        let detached = identity_of(Path::new("/somewhere/project-worktrees/one"), None, &rule)
            .expect("an identity");
        let unusable = identity_of(
            Path::new("/somewhere/project-worktrees/one"),
            Some("___"),
            &rule,
        )
        .expect("an identity");
        assert_eq!(on_a_branch.id, "c6e9f9d721e2__work_topic");
        assert_eq!(detached.id, "c6e9f9d721e2");
        assert_eq!(unusable.id, "c6e9f9d721e2");
        assert_eq!(sanitized_branch("feat/a b--c__"), "feat_a_b--c");
    }

    /// The index folds the path without touching the disk: a link stays a
    /// link, and the same directory written two ways is one identity.
    #[test]
    fn the_path_is_folded_and_never_read_from_the_disk() {
        assert_eq!(resolve_like_the_index(Path::new("/a/b/../c/")), "/a/c");
        assert_eq!(resolve_like_the_index(Path::new("/a//./c")), "/a/c");
        assert_eq!(resolve_like_the_index(Path::new("/")), "/");
        let folded = identity_of(Path::new("/a/b/../c"), None, &IdentityRule::default())
            .expect("an identity");
        assert_eq!(folded.id, "61f97427d954");
    }
}
