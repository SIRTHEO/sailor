//! What a branch of this repository is called.

pub const TRUNK: &str = "sorgenti";

pub const KEPT_HISTORY: &str = "main";

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
    if name == TRUNK || name == KEPT_HISTORY {
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
