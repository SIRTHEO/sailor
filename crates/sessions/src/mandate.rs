//! The record one session leaves for the one that comes after it.
//!
//! **TWO HALVES, AND ONLY ONE OF THEM IS A CLAIM.** Sailor writes the half
//! that can be read off the world; the session writes the half only it knows.
//! A successor verifies the first against the world before it trusts the
//! second, so a mandate is never believed whole on the strength of its author.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// Where the mandates of a store live.
pub const MANDATES: &str = "mandates";

/// What Sailor fills in at the moment the mandate is asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Written {
    pub tree: String,
    pub tty: String,
    pub session: String,
    pub engine: String,
    #[serde(default)]
    pub model: Option<String>,
    pub tokens: u64,
    pub at: i64,
    pub branch: String,
    pub head: String,
    /// A digest of everything uncommitted, so a successor can tell the tree it
    /// was told about from the tree it finds. A digest and not the list: the
    /// list is somebody else's work half the time, and it grows without bound.
    pub uncommitted: String,
    /// The files this segment touched, in the order they are worth reading
    /// again. Measured over three thousand sessions, the window after a reset
    /// holds more reads than any other: this list is what actually travels.
    #[serde(default)]
    pub reread: Vec<String>,
    #[serde(default)]
    pub transcript: Option<String>,
    /// The other terminals alive in the same tree, whose uncommitted work is
    /// theirs and not the successor's to explain.
    #[serde(default)]
    pub alongside: Vec<String>,
}

/// One thing the session believes about the world, and whether it looked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub said: String,
    /// Checked in this turn against the world, not remembered from earlier.
    pub verified: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub decided: String,
    pub authorised_by: String,
}

/// A rule the successor must keep, with everything it needs to keep it.
///
/// **FOUR FIELDS AND NOT ONE.** «Do not push» alone leaves a successor unable
/// to tell the rule it may lift from the rule it may not, and the one it lifts
/// is the one nobody could argue about afterwards.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraint {
    pub holds: String,
    /// What must be true before it can be met at all.
    pub prerequisite: String,
    /// Who set it: a person, a judge, a rule of the tree.
    pub authority: String,
    /// What to do instead when it cannot be met.
    pub fallback: String,
    /// What happens if it is broken.
    pub consequence: String,
}

/// What only the session knows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Work {
    pub goal: String,
    /// The person's last instruction, in their words.
    pub asked: String,
    #[serde(default)]
    pub state: Vec<Claim>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// What was tried and did not work, so it is not tried again.
    #[serde(default)]
    pub failed: Vec<String>,
    /// One, and exactly one.
    pub next: String,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub never: Vec<String>,
}

/// Who took it, and when. A mandate is consumed once.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Taken {
    pub by: String,
    pub at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mandate {
    pub written: Written,
    pub work: Work,
    #[serde(default)]
    pub taken: Option<Taken>,
}

/// Every field left blank, named at once.
///
/// **AT DEPOSIT AND NOT AT RESUME.** A gap found when the successor is already
/// alive is a gap nobody can fill: its author is gone.
pub fn blank_fields(mandate: &Mandate) -> Vec<String> {
    let written = &mandate.written;
    let work = &mandate.work;
    let mut missing = Vec::new();
    for (name, value) in [
        ("written.tree", &written.tree),
        ("written.tty", &written.tty),
        ("written.session", &written.session),
        ("written.engine", &written.engine),
        ("work.goal", &work.goal),
        ("work.asked", &work.asked),
        ("work.next", &work.next),
    ] {
        if value.trim().is_empty() {
            missing.push(name.to_owned());
        }
    }
    for (index, claim) in work.state.iter().enumerate() {
        if claim.said.trim().is_empty() {
            missing.push(format!("work.state[{index}].said"));
        }
    }
    for (index, decision) in work.decisions.iter().enumerate() {
        for (name, value) in [
            ("decided", &decision.decided),
            ("authorised_by", &decision.authorised_by),
        ] {
            if value.trim().is_empty() {
                missing.push(format!("work.decisions[{index}].{name}"));
            }
        }
    }
    for (index, held) in work.constraints.iter().enumerate() {
        for (name, value) in [
            ("holds", &held.holds),
            ("prerequisite", &held.prerequisite),
            ("authority", &held.authority),
            ("fallback", &held.fallback),
            ("consequence", &held.consequence),
        ] {
            if value.trim().is_empty() {
                missing.push(format!("work.constraints[{index}].{name}"));
            }
        }
    }
    missing
}

/// How far the tree has moved from what the mandate says about it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Freshness {
    pub stale: bool,
    pub head_moved: bool,
    pub uncommitted_moved: bool,
}

/// **STALE IS NOT WRONG.** A mandate whose tree moved is still the work; the
/// successor is told what moved and reads it, which is the answer a refusal
/// cannot give.
pub fn freshness(mandate: &Mandate, head: &str, uncommitted: &str) -> Freshness {
    let head_moved = mandate.written.head != head;
    let uncommitted_moved = mandate.written.uncommitted != uncommitted;
    Freshness {
        stale: head_moved || uncommitted_moved,
        head_moved,
        uncommitted_moved,
    }
}

/// Where a terminal's mandate waits.
pub fn address_in(store: &Path, tty: &str) -> PathBuf {
    store.join(MANDATES).join(format!("{tty}.json"))
}

/// Where a mandate goes when a second one is deposited over it.
pub fn archive_in(store: &Path, tty: &str, at: i64) -> PathBuf {
    store
        .join(MANDATES)
        .join("archive")
        .join(format!("{tty}-{at}.json"))
}

/// Writes it whole or not at all, and moves any earlier one aside first.
///
/// **A SECOND DEPOSIT ARCHIVES THE FIRST.** Overwriting loses the one record
/// of what the predecessor believed, which is the only place a wrong handover
/// can be read back from.
pub fn deposit(store: &Path, mandate: &Mandate) -> io::Result<Option<PathBuf>> {
    let path = address_in(store, &mandate.written.tty);
    let mut archived = None;
    if let Some(earlier) = read(&path) {
        let aside = archive_in(store, &mandate.written.tty, earlier.written.at);
        if let Some(parent) = aside.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&path, &aside)?;
        archived = Some(aside);
    }
    write_at(&path, mandate)?;
    Ok(archived)
}

fn write_at(path: &Path, mandate: &Mandate) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(mandate).map_err(io::Error::other)?;
    let beside = path.with_extension("writing");
    std::fs::write(&beside, text)?;
    std::fs::rename(&beside, path)
}

/// The mandate, or nothing when none was left or it cannot be understood.
pub fn read(path: &Path) -> Option<Mandate> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Marks it consumed by one session.
///
/// It stays on disk, marked. Removing it would make a second resume look like
/// a first one that found nothing, and «found nothing» is how a successor goes
/// off to find work of its own.
pub fn consume(path: &Path, by: &str, at: i64) -> io::Result<()> {
    let mut mandate = read(path).ok_or_else(|| io::Error::other("no mandate to consume"))?;
    mandate.taken = Some(Taken {
        by: by.to_owned(),
        at,
    });
    write_at(path, &mandate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("sailor-mandate-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a directory to write in");
        path
    }

    fn filled(tty: &str) -> Mandate {
        Mandate {
            written: Written {
                tree: "/a/tree".to_owned(),
                tty: tty.to_owned(),
                session: "a-session".to_owned(),
                engine: "a-command-line".to_owned(),
                head: "aaaa".to_owned(),
                uncommitted: "M one.rs".to_owned(),
                at: 10,
                ..Written::default()
            },
            work: Work {
                goal: "finish the conduit".to_owned(),
                asked: "carry on".to_owned(),
                next: "measure it".to_owned(),
                ..Work::default()
            },
            taken: None,
        }
    }

    #[test]
    fn a_mandate_deposited_comes_back_the_same() {
        let store = scratch("roundtrip");
        let left = filled("ttys001");
        deposit(&store, &left).expect("deposit it");
        assert_eq!(read(&address_in(&store, "ttys001")), Some(left));
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_second_deposit_moves_the_first_aside_instead_of_over_it() {
        let store = scratch("archive");
        let first = filled("ttys002");
        deposit(&store, &first).expect("deposit the first");
        let mut second = filled("ttys002");
        second.written.at = 20;
        let archived = deposit(&store, &second)
            .expect("deposit the second")
            .expect("the first was moved aside");
        assert_eq!(read(&archived), Some(first));
        assert_eq!(read(&address_in(&store, "ttys002")), Some(second));
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_consumed_mandate_says_who_took_it() {
        let store = scratch("consume");
        deposit(&store, &filled("ttys003")).expect("deposit it");
        let path = address_in(&store, "ttys003");
        consume(&path, "the-successor", 99).expect("consume it");
        let taken = read(&path).expect("it is still there").taken;
        assert_eq!(
            taken,
            Some(Taken {
                by: "the-successor".to_owned(),
                at: 99
            })
        );
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_tree_that_moved_makes_the_mandate_stale_and_says_which_way() {
        let mandate = filled("ttys004");
        assert_eq!(
            freshness(&mandate, "aaaa", "M one.rs"),
            Freshness::default(),
            "the tree it was written about"
        );
        let moved = freshness(&mandate, "bbbb", "M one.rs");
        assert!(moved.stale && moved.head_moved && !moved.uncommitted_moved);
        let dirtied = freshness(&mandate, "aaaa", "M one.rs\nM two.rs");
        assert!(dirtied.stale && !dirtied.head_moved && dirtied.uncommitted_moved);
    }

    #[test]
    fn a_constraint_missing_one_of_its_four_fields_is_named() {
        let mut mandate = filled("ttys005");
        mandate.work.constraints.push(Constraint {
            holds: "do not push".to_owned(),
            prerequisite: "the suite is green".to_owned(),
            authority: "the person".to_owned(),
            consequence: "a broken trunk".to_owned(),
            ..Constraint::default()
        });
        assert_eq!(
            blank_fields(&mandate),
            vec!["work.constraints[0].fallback".to_owned()]
        );
    }

    #[test]
    fn a_mandate_with_every_field_filled_names_nothing() {
        assert!(blank_fields(&filled("ttys006")).is_empty());
    }
}
