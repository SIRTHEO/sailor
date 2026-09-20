//! The identity a code index keeps for a tree, computed the way the index
//! computes it: a pin from the environment, else a pin from a file in the
//! tree, else the first twelve hex digits of the SHA-256 of the resolved path.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

/// Where a repository declares the names its index uses (ADR-021). Nothing
/// declared, no pin read: the tree is named by its path.
pub const CONVENTION_FILE: &str = ".sailor/index.json";

/// The pin file is committed, so every checkout addresses the same index.
/// `branch_aware_variable` reading `true` suffixes an unpinned identity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Convention {
    pub pin_file: Option<String>,
    pub pin_key: Option<String>,
    pub pin_variable: Option<String>,
    pub branch_aware_variable: Option<String>,
}

#[derive(Deserialize)]
struct Declaration {
    #[serde(default)]
    identity: Convention,
}

impl Convention {
    /// No file declares nothing, which is an answer; an unreadable one is a
    /// refusal, because a silent no-pin would address a different index.
    pub fn declared_by(repo: &Path) -> Result<Convention, String> {
        let Ok(text) = std::fs::read_to_string(repo.join(CONVENTION_FILE)) else {
            return Ok(Convention::default());
        };
        serde_json::from_str::<Declaration>(&text)
            .map(|declared| declared.identity)
            .map_err(|why| format!("{CONVENTION_FILE} cannot be read: {why}"))
    }

    /// Half a declaration pins nothing, said here so the caller need not guess.
    fn pin(&self) -> Option<(&str, &str)> {
        Some((self.pin_file.as_deref()?, self.pin_key.as_deref()?))
    }
}

/// What the process running the index tells it, handed in so that a test
/// arranges it instead of the machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityRule {
    pub pinned_by_environment: Option<String>,
    pub branch_aware: bool,
    pub convention: Convention,
}

impl IdentityRule {
    /// No declaration, no variable to read.
    pub fn declared_by(repo: &Path) -> Result<IdentityRule, String> {
        let convention = Convention::declared_by(repo)?;
        let pinned = convention
            .pin_variable
            .as_deref()
            .and_then(|name| std::env::var(name).ok())
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let branch_aware = convention
            .branch_aware_variable
            .as_deref()
            .is_some_and(|name| std::env::var(name).is_ok_and(|value| value == "true"));
        Ok(IdentityRule {
            pinned_by_environment: pinned,
            branch_aware,
            convention,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexIdentity {
    pub id: String,
    /// A pinned identity is declared, not derived, and every tree declaring
    /// the same pin addresses the same index.
    pub pinned: bool,
}

/// The identity the index keeps for `tree`, given the branch it stands on.
pub fn identity_of(
    tree: &Path,
    branch: Option<&str>,
    rule: &IdentityRule,
) -> Result<IndexIdentity, String> {
    if let Some(pinned) = &rule.pinned_by_environment {
        let named = rule.convention.pin_variable.as_deref().unwrap_or("the pin");
        valid(pinned, named)?;
        return Ok(IndexIdentity {
            id: pinned.clone(),
            pinned: true,
        });
    }
    if let Some(pinned) = pinned_in(tree, &rule.convention)? {
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

/// No convention names no file; a file missing, malformed or silent pins nothing.
fn pinned_in(tree: &Path, convention: &Convention) -> Result<Option<String>, String> {
    let Some((file, key)) = convention.pin() else {
        return Ok(None);
    };
    let Ok(text) = std::fs::read_to_string(tree.join(file)) else {
        return Ok(None);
    };
    let Ok(pin) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok(None);
    };
    let Some(declared) = pin.get(key).and_then(|id| id.as_str()).map(str::trim) else {
        return Ok(None);
    };
    if declared.is_empty() {
        return Ok(None);
    }
    valid(declared, file)?;
    Ok(Some(declared.to_owned()))
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

    /// A made-up index: no test here borrows a name from a real machine.
    fn a_convention() -> Convention {
        Convention {
            pin_file: Some(".an-index.json".to_owned()),
            pin_key: Some("projectId".to_owned()),
            pin_variable: Some("AN_INDEX_PROJECT_ID".to_owned()),
            branch_aware_variable: Some("AN_INDEX_BRANCH_AWARE".to_owned()),
        }
    }

    fn reading(convention: Convention) -> IdentityRule {
        IdentityRule {
            convention,
            ..IdentityRule::default()
        }
    }

    #[test]
    fn a_pin_in_the_tree_wins_over_the_path() {
        let scratch = a_scratch("pinned");
        std::fs::write(scratch.join(".an-index.json"), "{\"projectId\": \" the-pin \"}\n")
            .expect("the pin");
        let identity =
            identity_of(&scratch, None, &reading(a_convention())).expect("an identity");
        let _ = std::fs::remove_dir_all(&scratch);
        assert_eq!(identity.id, "the-pin");
        assert!(identity.pinned);
    }

    /// **A NAME NOBODY DECLARED IS A FILE NOBODY OPENS**: it was a constant
    /// here, so one index's pin was read by every repository.
    #[test]
    fn a_pin_file_nobody_declared_is_never_opened() {
        let scratch = a_scratch("undeclared");
        std::fs::write(scratch.join(".an-index.json"), "{\"projectId\": \"the-pin\"}\n")
            .expect("the pin");
        let identity = identity_of(&scratch, None, &IdentityRule::default()).expect("an identity");
        let half = Convention {
            pin_key: None,
            ..a_convention()
        };
        let halfway = identity_of(&scratch, None, &reading(half)).expect("an identity");
        let _ = std::fs::remove_dir_all(&scratch);
        assert!(!identity.pinned, "{identity:?}");
        assert!(!halfway.pinned, "half a declaration pins nothing");
    }

    /// A broken declaration is a refusal, never a quiet no-pin.
    #[test]
    fn the_convention_is_read_from_the_repository_or_refused() {
        let scratch = a_scratch("declared");
        std::fs::create_dir_all(scratch.join(".sailor")).expect("the directory");
        assert_eq!(
            Convention::declared_by(&scratch),
            Ok(Convention::default()),
            "a repository declaring nothing declares nothing"
        );
        std::fs::write(
            scratch.join(CONVENTION_FILE),
            "{\"identity\": {\"pin_file\": \".an-index.json\", \"pin_key\": \"projectId\"}}",
        )
        .expect("the declaration");
        let declared = Convention::declared_by(&scratch).expect("a convention");
        std::fs::write(scratch.join(CONVENTION_FILE), "not json").expect("the declaration");
        let broken = Convention::declared_by(&scratch);
        let _ = std::fs::remove_dir_all(&scratch);
        assert_eq!(declared.pin_file.as_deref(), Some(".an-index.json"));
        assert!(broken.is_err(), "{broken:?}");
    }

    #[test]
    fn a_pin_in_the_environment_wins_over_the_file() {
        let scratch = a_scratch("environment");
        std::fs::write(scratch.join(".an-index.json"), "{\"projectId\": \"the-pin\"}\n")
            .expect("the pin");
        let rule = IdentityRule {
            pinned_by_environment: Some("from-outside".to_owned()),
            branch_aware: true,
            convention: a_convention(),
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
        let rule = reading(a_convention());
        std::fs::write(scratch.join(".an-index.json"), "{\"projectId\": \"  \"}").expect("the pin");
        let silent = identity_of(&scratch, None, &rule).expect("an identity");
        std::fs::write(scratch.join(".an-index.json"), "not json").expect("the pin");
        let broken = identity_of(&scratch, None, &rule).expect("an identity");
        std::fs::write(scratch.join(".an-index.json"), "{\"projectId\": \"no spaces\"}")
            .expect("the pin");
        let refused = identity_of(&scratch, None, &rule);
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
            convention: a_convention(),
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
