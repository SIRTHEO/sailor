//! What a branch of this repository is called.

/// The git configuration key a repository declares its trunk in.
pub const TRUNK_KEY: &str = "sailor.trunk";

const WORK: &str = "work/";

const AGENT_TREE: &str = "worktree-agent-";

pub const THE_CONVENTION: &str = "work/<topic>, in lowercase letters, digits and hyphens";

/// A branch already standing keeps the name it was given; a new one does not.
pub fn may_be_cut(branch: &str, the_branch_exists: bool, trunk: &str) -> Result<(), String> {
    if the_branch_exists || follows_the_convention(branch, trunk) {
        return Ok(());
    }
    Err(format!(
        "«{branch}» is not a name new work is given here: {THE_CONVENTION}"
    ))
}

/// The names that break the convention, in the order they were given. **Pure,
/// and that is the point:** a check reading the branches of whichever machine
/// runs it goes red over a stray branch of somebody else's.
pub fn against_the_convention(names: &[String], trunk: &str) -> Vec<String> {
    names
        .iter()
        .filter(|name| !follows_the_convention(name, trunk))
        .cloned()
        .collect()
}

pub fn follows_the_convention(name: &str, trunk: &str) -> bool {
    if name == trunk {
        return true;
    }
    if let Some(id) = name.strip_prefix(AGENT_TREE) {
        return !id.is_empty();
    }
    match name.strip_prefix(WORK) {
        Some(topic) => is_a_topic(topic),
        None => false,
    }
}

fn is_a_topic(topic: &str) -> bool {
    !topic.is_empty()
        && !topic.starts_with('-')
        && !topic.ends_with('-')
        && topic
            .chars()
            .all(|letter| letter.is_ascii_lowercase() || letter.is_ascii_digit() || letter == '-')
}

/// A branch, and the three facts that say whether anything still watches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub in_the_trunk: bool,
    pub has_a_tree: bool,
    pub idle_hours: i64,
}

/// Measured over the forty branches that reached this trunk: the longest
/// lived 117 hours.
pub const ADRIFT_AFTER_HOURS: i64 = 120;

/// The branches carrying work nothing watches. **Pure, for the reason
/// `against_the_convention` is**: the facts are handed in, never read here.
pub fn adrift<'a>(branches: &'a [Branch], trunk: &str) -> Vec<&'a Branch> {
    let mut adrift: Vec<&Branch> = branches
        .iter()
        .filter(|branch| {
            branch.name != trunk
                && !branch.in_the_trunk
                && !branch.has_a_tree
                && branch.idle_hours > ADRIFT_AFTER_HOURS
        })
        .collect();
    adrift.sort_by_key(|branch| std::cmp::Reverse(branch.idle_hours));
    adrift
}
