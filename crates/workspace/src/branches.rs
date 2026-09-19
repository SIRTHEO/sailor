//! What a branch of this repository is called.

/// **ONE TRUNK, AND IT IS THE ONE A READER EXPECTS.** An archived history is a tag.
pub const TRUNK: &str = "main";

const WORK: &str = "work/";

const AGENT_TREE: &str = "worktree-agent-";

/// The names that break the convention, in the order they were given. **Pure,
/// and that is the point:** a check reading the branches of whichever machine
/// runs it goes red over a stray branch of somebody else's.
pub fn against_the_convention(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| !follows_the_convention(name))
        .cloned()
        .collect()
}

pub fn follows_the_convention(name: &str) -> bool {
    if name == TRUNK {
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
pub fn adrift(branches: &[Branch]) -> Vec<&Branch> {
    let mut adrift: Vec<&Branch> = branches
        .iter()
        .filter(|branch| {
            branch.name != TRUNK
                && !branch.in_the_trunk
                && !branch.has_a_tree
                && branch.idle_hours > ADRIFT_AFTER_HOURS
        })
        .collect();
    adrift.sort_by_key(|branch| std::cmp::Reverse(branch.idle_hours));
    adrift
}
