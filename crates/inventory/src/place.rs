//! A declared place on disk, weighed like every other entry of the census —
//! reusing `inventory_items` and its three lifecycle columns whole, never a
//! second table that knows the same thing. Undeclared and unweighable places
//! stay silent rather than guessed: this module never invents a plausible
//! answer where it has none.

use std::path::{Path, PathBuf};

/// What kind of thing a place holds, named for the question that decides if
/// it can be let go: does making it again cost nothing (`Regenerable`), is it
/// a cache of something that still exists elsewhere (`Cache`), or is it the
/// only copy of something (`Data`, `Saved`)?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Regenerable,
    Cache,
    Data,
    Saved,
}

/// The four things a place must say about itself before anything proposes
/// touching it. Written by whoever owns the place, never guessed from its
/// name or its age: a name promises nothing (`socraticode-ollama` did not
/// use `~/.ollama`), and a date does not tell a paused build from a dead one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub category: Category,
    /// The command or gesture that would produce this place again.
    pub rebuilt_by: String,
    /// A question with an executable answer, never a date: `git worktree
    /// list` no longer naming this checkout, no process in the process table
    /// standing in this directory, no running container mounting this path.
    pub proven_unneeded_by: String,
    /// What getting it back costs: minutes of a rebuild, hours of a download.
    pub cost_to_rebuild: String,
}

/// One place on disk, weighed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    pub path: PathBuf,
    /// `None` when the weigh itself could not say — a permission refusal, or
    /// a path already gone between naming it and weighing it.
    pub weight: Option<u64>,
    /// `None` for a place found but not yet declared: it is counted, and it
    /// stays out of anything that proposes freeing space — censused, never
    /// proposable, until it declares itself.
    pub declaration: Option<Declaration>,
}

impl Place {
    /// Weighs `path` and pairs it with a declaration, if one is given.
    pub fn weighed(name: &str, path: &Path, declaration: Option<Declaration>) -> Place {
        Place {
            name: name.to_owned(),
            path: path.to_owned(),
            weight: machine::weight_of(path),
            declaration,
        }
    }

    /// A place can be proposed for freeing only once both are true: it
    /// declared itself, and its own proof of not being needed holds. The
    /// proof itself is not evaluated here — only the record's own two-part
    /// shape is asserted; running the proof is the next gradino's work.
    pub fn is_proposable(&self) -> bool {
        self.declaration.is_some() && self.weight.is_some()
    }

    /// The row this place would take in `inventory_items`, under the family
    /// `"place"`. The kind column is a plain string in that table already —
    /// no `inventory::Kind` variant is spent on this, and nothing that
    /// matches on `Kind` today has to learn a new arm to keep compiling.
    pub fn as_inventory_item(&self) -> ledger::records::InventoryItem {
        ledger::records::InventoryItem {
            kind: "place".to_owned(),
            name: self.name.clone(),
            origin: "space".to_owned(),
            path: self.path.to_string_lossy().into_owned(),
            reach: if self.weight.is_some() { "active" } else { "unknown" }.to_owned(),
            reason: self.weight.is_none().then(|| "could not weigh this path".to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared() -> Declaration {
        Declaration {
            category: Category::Cache,
            rebuilt_by: "cargo build".to_owned(),
            proven_unneeded_by: "git worktree list no longer names this checkout".to_owned(),
            cost_to_rebuild: "a few minutes of compiling".to_owned(),
        }
    }

    #[test]
    fn an_undeclared_place_is_weighed_and_stays_unproposable() {
        let root = std::env::temp_dir().join(format!("place-undeclared-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("a scratch place");
        std::fs::write(root.join("f"), vec![0u8; 1_000]).expect("a file inside it");

        let place = Place::weighed("scratch", &root, None);
        assert!(place.weight.is_some_and(|w| w >= 1_000));
        assert!(!place.is_proposable(), "no declaration means no proposal, ever");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_declared_place_that_could_not_be_weighed_is_still_not_proposable() {
        let gone = std::env::temp_dir().join("place-that-never-existed-at-all");
        let place = Place::weighed("gone", &gone, Some(declared()));
        assert_eq!(place.weight, None);
        assert!(!place.is_proposable(), "an unweighed place cannot be sized up for freeing");
    }

    #[test]
    fn only_a_weighed_and_declared_place_is_proposable() {
        let root = std::env::temp_dir().join(format!("place-declared-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("a scratch place");

        let place = Place::weighed("scratch", &root, Some(declared()));
        assert!(place.is_proposable());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unweighable_place_reaches_the_ledger_as_unknown_not_as_empty() {
        let gone = std::env::temp_dir().join("place-that-never-existed-either");
        let place = Place::weighed("gone", &gone, None);
        let item = place.as_inventory_item();
        assert_eq!(item.kind, "place");
        assert_eq!(item.reach, "unknown");
        assert!(item.reason.is_some(), "silence must say why, never read as zero");
    }
}
