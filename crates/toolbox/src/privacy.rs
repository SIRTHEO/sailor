//! What a machine declares must never leave it.
//!
//! **THE LIST WAS READ BY A JUDGE AND BY NOBODY ELSE.** A gate that only runs
//! at commit catches the leak in the file, long after the words were written
//! into a store that never asked. One rule, read from one place, so whoever
//! writes hears it when they write.

use std::path::{Path, PathBuf};

/// A remote destination and the exact ref that a publication may update.
///
/// Its fields intentionally stay private: neither a remote URL nor a ref name
/// belongs in a privacy refusal.
#[derive(Clone, PartialEq, Eq)]
pub struct PushPlan {
    remote: String,
    destination: String,
    head: String,
}

impl PushPlan {
    pub fn remote(&self) -> &str {
        &self.remote
    }

    pub fn destination(&self) -> &str {
        &self.destination
    }

    pub fn head(&self) -> &str {
        &self.head
    }
}

/// Why publication cannot proceed. The variants carry no private values, so a
/// caller can report the refusal without turning the guard into a leak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationPreflight {
    CannotProve,
    PrivateMaterial,
}

/// Filenames that hold a person's credentials or local account state. Their
/// contents do not make them safe to publish.
pub const RESERVED_ARTIFACT_FILENAMES: &[&str] = &[
    ".credentials.json",
    "credentials.json",
    "auth.json",
    "profili.json",
    "cooldowns.json",
    "budgets.json",
    ".env",
];

/// Where a machine keeps the names that must not be committed, below the home:
/// one per line, `#` opens a comment. `SAILOR_PRIVATE_NAMES` names it outright.
pub const PRIVATE_NAMES_BELOW_HOME: &str = "personal/.sailor-notes/private-names";

/// The declared list, or `None` when nothing declares one. A machine that
/// declares nothing forbids nothing, and that is not an error.
pub fn where_the_names_are(declared: Option<String>, home: Option<String>) -> Option<PathBuf> {
    if let Some(path) = declared.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(path));
    }
    Some(PathBuf::from(home.filter(|value| !value.is_empty())?).join(PRIVATE_NAMES_BELOW_HOME))
}

/// Reads the privacy inputs a publisher needs. An absent or unreadable list is
/// not an unarmed publisher: it is a publisher that cannot prove its guard.
pub fn required_input(
    declared: Option<String>,
    home: Option<String>,
) -> Result<(Vec<String>, String), PublicationPreflight> {
    let home = home.filter(|value| !value.is_empty()).ok_or(PublicationPreflight::CannotProve)?;
    let list = where_the_names_are(declared, Some(home.clone()))
        .ok_or(PublicationPreflight::CannotProve)?;
    let text = std::fs::read_to_string(list).map_err(|_| PublicationPreflight::CannotProve)?;
    Ok((names_in(&text), home))
}

/// Proves the remote, destination ref and commit range a normal branch push
/// would update. It accepts only a branch with a configured upstream, then
/// callers push the recorded `HEAD` to that recorded ref instead of trusting a
/// later configuration read.
pub fn push_plan(root: &Path) -> Result<PushPlan, PublicationPreflight> {
    let branch = git_text(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    let remote = git_text(root, &["config", "--get", &format!("branch.{branch}.remote")])?;
    let destination = git_text(root, &["config", "--get", &format!("branch.{branch}.merge")])?;
    if remote.is_empty()
        || !destination.starts_with("refs/heads/")
        || git_text(root, &["remote", "get-url", "--push", &remote])?.is_empty()
    {
        return Err(PublicationPreflight::CannotProve);
    }

    let remote_branch = destination.trim_start_matches("refs/heads/");
    let tracked = format!("refs/remotes/{remote}/{remote_branch}");
    git_text(root, &["rev-parse", "--verify", &tracked])?;
    let head = git_text(root, &["rev-parse", "--verify", "HEAD"])?;
    git_success(root, &["merge-base", "--is-ancestor", &tracked, &head])?;
    Ok(PushPlan { remote, destination, head })
}

/// Refuses when the exact outgoing range, plus messages a caller is about to
/// create, contains a declared private name or this machine's home path.
pub fn outgoing_metadata_is_private(
    root: &Path,
    plan: &PushPlan,
    names: &[String],
    home: &str,
    planned_messages: &[String],
) -> Result<bool, PublicationPreflight> {
    let remote_branch = plan.destination.trim_start_matches("refs/heads/");
    let tracked = format!("refs/remotes/{}/{remote_branch}", plan.remote);
    let range = format!("{tracked}..{}", plan.head);
    let messages = git_text(root, &["log", "--format=%B%x1e", &range])?;
    Ok(messages
        .split('\u{1e}')
        .chain(planned_messages.iter().map(String::as_str))
        .any(|message| !what_cannot_be_published(message, names, Some(home)).is_empty()))
}

fn git_text(root: &Path, args: &[&str]) -> Result<String, PublicationPreflight> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|_| PublicationPreflight::CannotProve)?;
    if !output.status.success() {
        return Err(PublicationPreflight::CannotProve);
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|_| PublicationPreflight::CannotProve)
}

fn git_success(root: &Path, args: &[&str]) -> Result<(), PublicationPreflight> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|_| PublicationPreflight::CannotProve)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(PublicationPreflight::CannotProve)
    }
}

/// The names a list declares. Blank lines and `#` lines are not names.
pub fn names_in(list: &str) -> Vec<String> {
    list.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Why a text cannot be published. **THE NAME IS NEVER CARRIED**: this is read
/// on a terminal that may be recorded, and a refusal that echoes the secret to
/// explain itself has published it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// A name the machine declares private, and where it starts.
    APrivateName { at: usize },
    /// An absolute path of this machine, which is true on no other.
    APathOfThisMachine { at: usize },
    /// A home, process number, session id, tty or loopback of any machine.
    AShapeOfAMachine { at: usize },
    /// A passage about the machine the text was written on.
    APassageAboutTheMachine { at: usize },
}

impl Reason {
    pub fn at(&self) -> usize {
        match self {
            Reason::APrivateName { at }
            | Reason::APathOfThisMachine { at }
            | Reason::AShapeOfAMachine { at }
            | Reason::APassageAboutTheMachine { at } => *at,
        }
    }
}

/// What in this text the repository could not publish, in the order found.
///
/// The comparison is case-insensitive because a name written differently is
/// the same name to whoever reads it afterwards.
pub fn what_cannot_be_published(text: &str, names: &[String], home: Option<&str>) -> Vec<Reason> {
    let lowered = text.to_lowercase();
    let mut found = Vec::new();
    for name in names {
        if name.is_empty() {
            continue;
        }
        if let Some(at) = names_at(&lowered, &name.to_lowercase()) {
            found.push(Reason::APrivateName { at });
        }
    }
    if let Some(home) = home.filter(|value| !value.is_empty()) {
        if let Some(at) = text.find(home) {
            found.push(Reason::APathOfThisMachine { at });
        }
    }
    found.sort_by_key(Reason::at);
    found
}

/// Homes that belong to nobody, as `scripts/privacy-scan.sh` declares them.
const PLACEHOLDER_HOMES: &[&str] = &["someone", "somebody", "pilot", "user", "you", "example"];

/// What a text written for the public may not carry: everything
/// [`what_cannot_be_published`] refuses, plus what `scripts/privacy-scan.sh
/// --text` refuses with no list. Commit messages keep the narrower check:
/// the scan lets them speak about this machine.
pub fn what_a_public_text_cannot_carry(text: &str, names: &[String], home: Option<&str>) -> Vec<Reason> {
    let mut found = what_cannot_be_published(text, names, home);
    let shapes = [
        a_home_of_anybody(text),
        a_process_number(text),
        a_session_id(text),
        a_tty(text),
        text.find("127.0.0.1"),
    ];
    for at in shapes.into_iter().flatten() {
        found.push(Reason::AShapeOfAMachine { at });
    }
    if let Some(at) = text.to_lowercase().find("this machine") {
        found.push(Reason::APassageAboutTheMachine { at });
    }
    found.sort_by_key(Reason::at);
    found
}

/// `name@host.tld`: an account written the way a person signs in. How a flow
/// names an account, `tool@account`, has no dot after the `@` and is not this.
pub fn an_account_address(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    for (at, _) in text.match_indices('@') {
        let local = bytes[..at]
            .iter()
            .rev()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-'))
            .count();
        if local == 0 {
            continue;
        }
        let rest = &text[at + 1..];
        let end = rest
            .find(|letter: char| !(letter.is_ascii_alphanumeric() || matches!(letter, '.' | '-')))
            .unwrap_or(rest.len());
        let domain = rest[..end].trim_end_matches('.');
        let Some((host, top)) = domain.rsplit_once('.') else {
            continue;
        };
        if !host.is_empty() && top.len() >= 2 && top.chars().all(|letter| letter.is_ascii_alphabetic()) {
            return Some(at - local);
        }
    }
    None
}

/// An address only one house reaches: an IPv4 address of a private range, or
/// a host under `.local`, `.lan`, `.home.arpa` or `.internal`.
pub fn a_private_network_address(text: &str) -> Option<usize> {
    let is_part = |letter: char| letter.is_ascii_alphanumeric() || matches!(letter, '.' | '-');
    let mut start = None;
    for (index, letter) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        match (start, is_part(letter)) {
            (None, true) => start = Some(index),
            (Some(begin), false) => {
                if names_a_private_host(text[begin..index].trim_end_matches('.')) {
                    return Some(begin);
                }
                start = None;
            }
            _ => {}
        }
    }
    None
}

fn names_a_private_host(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() == 4 {
        if let Ok(octets) = parts.iter().map(|part| part.parse::<u8>()).collect::<Result<Vec<u8>, _>>() {
            return octets[0] == 10
                || (octets[0] == 192 && octets[1] == 168)
                || (octets[0] == 172 && (16..=31).contains(&octets[1]));
        }
    }
    let lowered = token.to_ascii_lowercase();
    [".local", ".lan", ".home.arpa", ".internal"]
        .iter()
        .any(|suffix| lowered.len() > suffix.len() && lowered.ends_with(suffix))
}

/// `/Users/<segment>/` or `/home/<segment>/`, unless the segment is a placeholder.
fn a_home_of_anybody(text: &str) -> Option<usize> {
    for root in ["/Users/", "/home/"] {
        let mut from = 0;
        while let Some(found) = text[from..].find(root) {
            let at = from + found;
            from = at + root.len();
            let rest = &text[from..];
            let segment = rest
                .find(|letter: char| !(letter.is_ascii_alphanumeric() || matches!(letter, '.' | '_' | '-')))
                .map_or(rest, |end| &rest[..end]);
            if !segment.is_empty()
                && rest[segment.len()..].starts_with('/')
                && !PLACEHOLDER_HOMES.contains(&segment)
            {
                return Some(at);
            }
        }
    }
    None
}

/// `pid`, not glued to a word before it, then an optional `:` and space, then
/// three digits or more.
fn a_process_number(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(found) = text[from..].find("pid") {
        let at = from + found;
        from = at + 3;
        if at > 0 && is_word_byte(bytes[at - 1]) {
            continue;
        }
        let mut next = at + 3;
        if bytes.get(next) == Some(&b':') {
            next += 1;
        }
        if bytes.get(next) == Some(&b' ') {
            next += 1;
        }
        if bytes[next..].iter().take_while(|byte| byte.is_ascii_digit()).count() >= 3 {
            return Some(at);
        }
    }
    None
}

/// Lowercase hex in groups of 8-4-4-4-12.
fn a_session_id(text: &str) -> Option<usize> {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let bytes = text.as_bytes();
    let hex = |byte: &u8| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte);
    (0..bytes.len()).find(|&start| {
        let mut at = start;
        GROUPS.iter().enumerate().all(|(index, &length)| {
            if index > 0 {
                if bytes.get(at) != Some(&b'-') {
                    return false;
                }
                at += 1;
            }
            let whole = bytes.get(at..at + length).is_some_and(|group| group.iter().all(hex));
            at += length;
            whole
        })
    })
}

/// `ttys` and exactly three digits.
fn a_tty(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(found) = text[from..].find("ttys") {
        let at = from + found;
        from = at + 4;
        if bytes[from..].iter().take_while(|byte| byte.is_ascii_digit()).count() == 3 {
            return Some(at);
        }
    }
    None
}

/// Where `name` appears **as a name** in already-lowercased `text`.
///
/// **A NAME IS NOT A SUBSTRING**: a short one lives inside ordinary words and
/// inside an account handle, and a guard red on those is one people skip, so
/// the character on each side must not be a letter, a digit or `_`. It misses
/// a name glued inside a longer word — not the shape a leak takes.
pub fn names_at(text: &str, name: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(found) = text[from..].find(name) {
        let at = from + found;
        let before_is_word = at > 0 && is_word_byte(bytes[at - 1]);
        let after = at + name.len();
        let after_is_word = after < bytes.len() && is_word_byte(bytes[after]);
        if !before_is_word && !after_is_word {
            return Some(at);
        }
        from = at + name.len();
    }
    None
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The names this machine declares, read from disk. Empty when unarmed.
pub fn declared_here(read: &dyn Fn(&Path) -> Option<String>) -> Vec<String> {
    let where_it_is = where_the_names_are(
        std::env::var("SAILOR_PRIVATE_NAMES").ok(),
        std::env::var("HOME").ok(),
    );
    where_it_is
        .and_then(|path| read(&path))
        .map(|text| names_in(&text))
        .unwrap_or_default()
}

/// Every place a declared private name appears among the files git tracks, as
/// `path:line`. **THE NAME IS NEVER ECHOED**: what comes back is where. It
/// lives here and not in a judge because a judge cannot guard a push — a
/// release's suite runs on an extract that is no repository.
pub fn where_names_are_tracked(root: &Path, names: &[String]) -> Vec<String> {
    let mut hits = Vec::new();
    for path in tracked_under(root) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lowered = text.to_lowercase();
        for name in names {
            let needle = name.to_lowercase();
            for (number, line) in lowered.lines().enumerate() {
                if names_at(line, &needle).is_some() {
                    let shown = path.strip_prefix(root).unwrap_or(&path);
                    hits.push(format!("{}:{}", shown.display(), number + 1));
                }
            }
        }
    }
    hits
}

/// Tracked artifacts whose basename is reserved for credentials or a person's
/// account state. A publisher cannot treat a failed Git reading as clean.
pub fn tracked_reserved_artifacts(root: &Path) -> Result<Vec<String>, PublicationPreflight> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|_| PublicationPreflight::CannotProve)?;
    if !output.status.success() {
        return Err(PublicationPreflight::CannotProve);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            RESERVED_ARTIFACT_FILENAMES.contains(&name)
        })
        .map(str::to_owned)
        .collect())
}

/// The files git tracks under `root`. An empty list where git will not answer:
/// the caller decides what that means, and a publisher must refuse.
pub fn tracked_under(root: &Path) -> Vec<PathBuf> {
    let Ok(out) = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(|name| root.join(name))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// Declared as private, a short name fired on a dozen ordinary words and on
    /// an account handle — none of them the name. **The name here is invented**,
    /// and not for decoration: a test using a declared one would put it in the
    /// repository, which is what this file exists to prevent.
    #[test]
    fn a_short_name_does_not_fire_inside_ordinary_words() {
        let names = vec!["art".to_owned()];

        for innocent in [
            "struct StartsTheOthers {",
            "the party had already charted a course",
            "https://github.com/SIRART/sailor.git",
        ] {
            assert_eq!(
                what_cannot_be_published(innocent, &names, None),
                Vec::new(),
                "«{innocent}» is not the name"
            );
        }
    }

    /// And it still fires where the name really is: a path segment, a quoted
    /// value, a word in a sentence.
    #[test]
    fn the_name_written_to_be_read_is_still_caught() {
        let names = vec!["art".to_owned()];

        for guilty in [
            "home: \"/home/art/.config/sailor\"",
            "{\"who\": \"art\"}",
            "it is Art's call, and it stays theirs",
        ] {
            assert!(
                !what_cannot_be_published(guilty, &names, None).is_empty(),
                "«{guilty}» carries the name"
            );
        }
    }

    use super::*;

    #[test]
    fn a_declared_list_beats_the_one_below_the_home() {
        assert_eq!(
            where_the_names_are(Some("/altrove/elenco".to_owned()), Some("/casa".to_owned())),
            Some(PathBuf::from("/altrove/elenco"))
        );
        assert_eq!(
            where_the_names_are(None, Some("/casa".to_owned())),
            Some(PathBuf::from("/casa").join(PRIVATE_NAMES_BELOW_HOME))
        );
        // A variable exported empty by a script that could not find the home
        // would put the list at the root of the disk.
        assert_eq!(where_the_names_are(Some(String::new()), None), None);
        assert_eq!(where_the_names_are(None, None), None);
    }

    #[test]
    fn comments_and_blank_lines_are_not_names() {
        let names = names_in("# the ones of this machine\n\nanengine\n  homeless  \n");
        assert_eq!(names, vec!["anengine", "homeless"]);
    }

    /// **A REFUSAL THAT ECHOES THE SECRET HAS PUBLISHED IT.** What comes back
    /// says where, never what.
    #[test]
    fn a_private_name_is_found_and_never_carried_back() {
        let names = vec!["anengine".to_owned()];
        let found = what_cannot_be_published("the step calls anengine and stops", &names, None);
        assert_eq!(found, vec![Reason::APrivateName { at: 15 }]);

        let said = format!("{found:?}");
        assert!(!said.contains("anengine"), "the refusal carries the name: {said}");
    }

    /// Written another way it is the same name to whoever reads it after.
    #[test]
    fn a_name_written_in_capitals_is_the_same_name() {
        let names = vec!["anengine".to_owned()];
        assert_eq!(
            what_cannot_be_published("AnEngine answers", &names, None),
            vec![Reason::APrivateName { at: 0 }]
        );
    }

    /// The home path is the other half: true on this machine and on no other.
    #[test]
    fn a_path_of_this_machine_is_found_too() {
        let found = what_cannot_be_published(
            "measured in /home/anybody/personal/sailor",
            &[],
            Some("/home/anybody"),
        );
        assert_eq!(found, vec![Reason::APathOfThisMachine { at: 12 }]);
    }

    /// A machine that declares nothing forbids nothing, and says so by
    /// answering an empty list rather than by refusing everything.
    #[test]
    fn an_unarmed_machine_forbids_nothing() {
        assert!(what_cannot_be_published("anything at all", &[], None).is_empty());
        assert!(what_cannot_be_published("anything at all", &[String::new()], Some("")).is_empty());
    }

    /// **A PUBLIC TEXT IS HELD TO THE RELEASE SCAN**, shape by shape, and the
    /// homes that belong to nobody pass as the scan lets them.
    #[test]
    fn a_public_text_refuses_every_shape_the_release_scan_refuses() {
        for (guilty, what) in [
            ("the relay stopped pid 91964 and left", "a process number"),
            ("the relay stopped pid:91964 and left", "a process number"),
            ("it ran on ttys008 all night", "a tty"),
            ("session 0f8e6a4c-1b2d-4e3f-9a8b-7c6d5e4f3a2b stopped", "a session id"),
            ("the server answered on 127.0.0.1", "the loopback"),
            ("measured in /Users/anybody/work", "another user's home"),
            ("kept under /home/anybody/flows", "another user's home"),
        ] {
            assert!(
                what_a_public_text_cannot_carry(guilty, &[], None)
                    .iter()
                    .any(|reason| matches!(reason, Reason::AShapeOfAMachine { .. })),
                "{what} passed: «{guilty}»"
            );
        }
        assert!(
            matches!(
                what_a_public_text_cannot_carry("On This Machine the token lives elsewhere.", &[], None)
                    .as_slice(),
                [Reason::APassageAboutTheMachine { .. }]
            ),
            "a passage about this machine passed"
        );
        for innocent in [
            "a flow kept in /home/pilot/flows",
            "the pidgin three engines speak",
            "The window forgets a flow you renamed.",
        ] {
            assert_eq!(
                what_a_public_text_cannot_carry(innocent, &[], None),
                Vec::new(),
                "«{innocent}» carries no shape of a machine"
            );
        }
    }

    /// **AN UNREADABLE LIST IS NOT AN EMPTY ONE**: a guard that cannot read what
    /// it guards cannot say a text is clean.
    #[test]
    fn a_list_of_names_that_cannot_be_read_is_a_refusal() {
        let nowhere = std::env::temp_dir()
            .join(format!("sailor-no-names-{}", std::process::id()))
            .join("private-names");
        assert_eq!(
            required_input(Some(nowhere.display().to_string()), Some("/home/pilot".to_owned())),
            Err(PublicationPreflight::CannotProve)
        );
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?}");
    }

    fn repository_with_an_outgoing_commit(message: &str) -> (PathBuf, PathBuf) {
        let scratch = std::env::temp_dir().join(format!(
            "sailor-publication-preflight-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        let remote = scratch.join("remote.git");
        let repo = scratch.join("repo");
        std::fs::create_dir_all(&repo).expect("scratch repository");
        assert!(
            Command::new("git")
            .args(["init", "--bare", "--quiet"])
            .arg(&remote)
            .status()
            .expect("git runs")
            .success(),
            "the local remote is created"
        );
        git(&repo, &["init", "--quiet"]);
        git(&repo, &["config", "user.email", "test@example.invalid"]);
        git(&repo, &["config", "user.name", "test"]);
        std::fs::write(repo.join("tracked.txt"), "first\n").expect("first file");
        git(&repo, &["add", "tracked.txt"]);
        git(&repo, &["commit", "--quiet", "-m", "first"]);
        git(&repo, &["remote", "add", "origin", remote.to_str().expect("utf-8 path")]);
        git(&repo, &["push", "--quiet", "-u", "origin", "HEAD"]);
        std::fs::write(repo.join("tracked.txt"), "second\n").expect("second file");
        git(&repo, &["add", "tracked.txt"]);
        git(&repo, &["commit", "--quiet", "-m", message]);
        (scratch, repo)
    }

    #[test]
    fn a_private_commit_message_in_the_exact_outgoing_range_is_refused() {
        let (scratch, repo) = repository_with_an_outgoing_commit("a note about mylberry");
        let plan = push_plan(&repo).expect("the remote range is proven");
        let private = outgoing_metadata_is_private(
            &repo,
            &plan,
            &["mylberry".to_owned()],
            "/home/tester",
            &[],
        )
        .expect("the metadata is readable");
        let _ = std::fs::remove_dir_all(scratch);

        assert!(private, "the outgoing commit message was not inspected");
    }

    #[test]
    fn a_clean_outgoing_commit_message_is_not_refused() {
        let (scratch, repo) = repository_with_an_outgoing_commit("a routine change");
        let plan = push_plan(&repo).expect("the remote range is proven");
        let private = outgoing_metadata_is_private(
            &repo,
            &plan,
            &["mylberry".to_owned()],
            "/home/tester",
            &[],
        )
        .expect("the metadata is readable");
        let _ = std::fs::remove_dir_all(scratch);

        assert!(!private, "a clean outgoing commit was refused");
    }

    #[test]
    fn an_unconfigured_destination_cannot_be_proven() {
        let scratch = std::env::temp_dir().join(format!(
            "sailor-publication-destination-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("scratch repository");
        git(&scratch, &["init", "--quiet"]);
        let plan = push_plan(&scratch);
        let _ = std::fs::remove_dir_all(&scratch);

        assert!(matches!(plan, Err(PublicationPreflight::CannotProve)));
    }

    #[test]
    fn an_absent_privacy_input_cannot_be_proven() {
        assert_eq!(
            required_input(None, None),
            Err(PublicationPreflight::CannotProve)
        );
    }

    #[test]
    fn an_account_address_is_found_and_a_flow_s_tool_at_account_is_not() {
        let address = format!("write to someone{}example.org today", '@');
        assert_eq!(super::an_account_address(&address), Some(9));
        assert_eq!(super::an_account_address("the chain is claude-code@team"), None);
        assert_eq!(super::an_account_address("an @ alone, and a@b"), None);
    }

    #[test]
    fn a_private_network_address_is_found_and_a_public_one_is_not() {
        let lan = format!("the server at http://{}.{}.1.20:8080 answers", 192, 168);
        assert_eq!(super::a_private_network_address(&lan), Some(21));
        assert!(super::a_private_network_address(&format!("{}.0.0.7", 10)).is_some());
        assert!(super::a_private_network_address(&format!("{}.20.0.1", 172)).is_some());
        assert!(super::a_private_network_address("the box on printer.local is off").is_some());
        assert_eq!(super::a_private_network_address(&format!("{}.32.0.1 and 8.8.8.8", 172)), None);
        assert_eq!(super::a_private_network_address("version 1.2.3 of «a.b»"), None);
    }
}
