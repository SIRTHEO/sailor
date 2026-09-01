//! The shape of a trigger descriptor, and how it is loaded.
//!
//! **WHY IT DOES NOT REUSE THE TOOL LOADER.** `toolbox::Catalog` makes the same
//! gestures on another body. Making it generic over the item is the right move
//! **when there is a third list**; doing it with two, today, would cost a type
//! parameter in every signature of that crate to save fifty lines here.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// The triggers the product carries, compiled into the binary: no install path
/// to guess, and they stay data — rewritten by `id`, switched off with
/// `disabled`.
pub const BUILTIN: &str = include_str!("../descriptors/default.json");

pub const BUILTIN_SOURCE: &str = "built-in";

/// Where descriptors are taken from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Builtin,
    File(PathBuf),
    /// Every `*.json` inside a directory, in name order.
    Dir(PathBuf),
}

/// The shape of a signal source. **Two, and the code knows no others**: which
/// terminal, which window, which product is what the descriptors say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Somebody presses and it starts, carrying a text. The signal has already
    /// arrived: there is nothing to wait for, which is why this is the only
    /// shape that really works today.
    Manual,
    /// The signal would appear in a terminal session. Declared today, not
    /// listened to: see `action`.
    Terminal,
}

/// Where a signal would be seen appearing in a terminal session.
///
/// **THE TWO SHAPES ARE MEASURED ON THIS MACHINE, NOT IMAGINED**: either a file
/// grows and is never rewritten, one line per message, and then it is read
/// keeping the point reached; or a command, given a cursor, prints what appeared
/// after it, and then that command is what gets called.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Listen {
    /// An append-only file: one JSON line per message.
    AppendedLines {
        /// The files to follow. `~/`, `$VAR` and `*` are allowed.
        files: Vec<String>,
        /// Where the message text sits inside a line.
        text_pointer: Vec<String>,
        /// Where the writer sits. Empty: the source does not know.
        #[serde(default)]
        who_pointer: Vec<String>,
        /// Where the originating session or pane sits.
        #[serde(default)]
        where_pointer: Vec<String>,
    },
    /// A command that prints what appeared after a cursor.
    ///
    /// **A LOG OF TERMINAL BYTES IS NOT AN HONEST SOURCE**: those are screen
    /// redraws, not messages, and rebuilding the text out of them means writing
    /// a terminal emulator. Where only such a log exists, this is the road.
    CursorCommand {
        /// The tool's id, not a binary: the same list that resolves the engines
        /// of the steps.
        tool: String,
        args: Vec<String>,
        /// The argument the reached cursor is written into.
        cursor_argument: String,
    },
}

/// One line of the signal-source list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerDescriptor {
    pub id: String,
    pub kind: Kind,
    #[serde(default)]
    pub label: String,
    /// How the signal would be seen arriving. Required for a terminal,
    /// forbidden for a manual one: a manual that declares where to listen is
    /// describing two different sources under one name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen: Option<Listen>,
    /// For whoever reads the list: what was measured, what is missing. It
    /// enters no decision.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub descriptor: TriggerDescriptor,
    pub source: String,
}

/// Something that could not be loaded, with the why and the where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub source: String,
    pub about: String,
    pub reason: String,
}

/// **THE RULES ARE WRITTEN TWICE, AND THIS IS THE TIE BETWEEN THE TWO COPIES.**
/// The same three gestures as `toolbox::Catalog`: read in order, the last `id`
/// wins, a wrong line becomes a report instead of making the others disappear.
/// Whoever changes one of them should look at the other.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub descriptors: Vec<Loaded>,
    pub problems: Vec<Problem>,
}

impl Catalog {
    pub fn load(sources: &[Source]) -> Catalog {
        let mut catalog = Catalog::default();
        for source in sources {
            match source {
                Source::Builtin => catalog.absorb(BUILTIN_SOURCE, BUILTIN),
                Source::File(path) => catalog.absorb_file(path),
                Source::Dir(dir) => {
                    let Ok(entries) = fs::read_dir(dir) else {
                        // A directory that is not there is the ordinary case of
                        // somebody who never added a trigger; one that is there
                        // and will not open is a fault, and the disk tells them
                        // apart.
                        if dir.exists() {
                            catalog.problems.push(Problem {
                                source: dir.to_string_lossy().into_owned(),
                                about: "the directory".to_string(),
                                reason: "could not be read".to_string(),
                            });
                        }
                        continue;
                    };
                    let mut files: Vec<PathBuf> = entries
                        .flatten()
                        .map(|entry| entry.path())
                        .filter(|path| path.extension().is_some_and(|end| end == "json"))
                        .collect();
                    files.sort();
                    for file in files {
                        catalog.absorb_file(&file);
                    }
                }
            }
        }
        catalog
    }

    fn absorb_file(&mut self, path: &Path) {
        let label = path.to_string_lossy().into_owned();
        match fs::read_to_string(path) {
            Ok(text) => self.absorb(&label, &text),
            Err(error) => self.problems.push(Problem {
                source: label,
                about: "the file".to_string(),
                reason: format!("could not be read: {error}"),
            }),
        }
    }

    /// The text is read twice on purpose: item by item, so a stray comma at the
    /// end does not wipe out the good triggers above it.
    fn absorb(&mut self, source: &str, text: &str) {
        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(error) => {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about: "the file".to_string(),
                    reason: format!("is not valid JSON: {error}"),
                });
                return;
            }
        };
        let items = match &value {
            Value::Array(items) => items.clone(),
            Value::Object(map) => match map.get("triggers") {
                Some(Value::Array(items)) => items.clone(),
                _ => {
                    self.malformed(source);
                    return;
                }
            },
            _ => {
                self.malformed(source);
                return;
            }
        };
        for (index, item) in items.iter().enumerate() {
            let about = item
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("entry number {}", index + 1));
            let descriptor: TriggerDescriptor = match serde_json::from_value(item.clone()) {
                Ok(descriptor) => descriptor,
                Err(error) => {
                    self.problems.push(Problem {
                        source: source.to_string(),
                        about,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            if let Err(reason) = coherent(&descriptor) {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about,
                    reason,
                });
                continue;
            }
            self.replace(Loaded {
                descriptor,
                source: source.to_string(),
            });
        }
    }

    fn malformed(&mut self, source: &str) {
        self.problems.push(Problem {
            source: source.to_string(),
            about: "the file".to_string(),
            reason: "holds neither an array nor a `triggers` field".to_string(),
        });
    }

    fn replace(&mut self, loaded: Loaded) {
        match self
            .descriptors
            .iter_mut()
            .find(|found| found.descriptor.id == loaded.descriptor.id)
        {
            Some(existing) => *existing = loaded,
            None => self.descriptors.push(loaded),
        }
    }

    /// The live ones, in stable `id` order: two reads in a row must give the
    /// same sequence, or the list shown cannot be compared with anything.
    pub fn live(&self) -> Vec<&Loaded> {
        let mut out: Vec<&Loaded> = self
            .descriptors
            .iter()
            .filter(|loaded| !loaded.descriptor.disabled)
            .collect();
        out.sort_by(|left, right| left.descriptor.id.cmp(&right.descriptor.id));
        out
    }

    pub fn find(&self, id: &str) -> Option<&Loaded> {
        self.live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == id)
    }

    /// The live `id`s, for the message given to whoever asked for one that is
    /// not there: an error that does not say which exist forces a hunt through
    /// the file.
    pub fn known(&self) -> Vec<String> {
        self.live()
            .into_iter()
            .map(|loaded| loaded.descriptor.id.clone())
            .collect()
    }
}

/// A descriptor that declares one shape and describes another does not load:
/// the day somebody writes it is the only day it is easy to notice.
fn coherent(descriptor: &TriggerDescriptor) -> Result<(), String> {
    match (descriptor.kind, descriptor.listen.is_some()) {
        (Kind::Manual, true) => Err(
            "a manual trigger carries the signal with it: it cannot also declare where to listen"
                .to_string(),
        ),
        (Kind::Terminal, false) => Err(
            "a terminal trigger must say where the signal would be seen appearing: `listen` is missing"
                .to_string(),
        ),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_descriptors_all_load() {
        let catalog = Catalog::load(&[Source::Builtin]);
        assert!(
            catalog.problems.is_empty(),
            "the shipped descriptors do not read: {:?}",
            catalog.problems
        );
        assert!(!catalog.live().is_empty());
    }

    /// The manual trigger is what the window rests on: if it disappeared from
    /// the shipped descriptors, the launch button would have no source left.
    #[test]
    fn a_manual_source_is_shipped_with_the_product() {
        let catalog = Catalog::load(&[Source::Builtin]);
        let manual = catalog
            .find("manual")
            .expect("the manual trigger ships with the product");
        assert_eq!(manual.descriptor.kind, Kind::Manual);
    }

    #[test]
    fn a_terminal_source_without_a_place_to_listen_is_refused() {
        let mut catalog = Catalog::default();
        catalog.absorb("prova", r#"[{"id": "vuoto", "kind": "terminal"}]"#);
        assert!(catalog.descriptors.is_empty());
        assert_eq!(catalog.problems.len(), 1);
        assert!(catalog.problems[0].reason.contains("listen"));
    }

    #[test]
    fn a_manual_source_that_also_listens_is_refused() {
        let mut catalog = Catalog::default();
        catalog.absorb(
            "prova",
            r#"[{"id": "confuso", "kind": "manual",
                 "listen": {"kind": "appended_lines", "files": ["~/x.jsonl"],
                            "text_pointer": ["testo"]}}]"#,
        );
        assert!(catalog.descriptors.is_empty());
        assert_eq!(catalog.problems.len(), 1);
    }

    /// One broken line does not delete the good ones: without this rule a
    /// partial list would look empty, which is worse.
    #[test]
    fn a_broken_entry_does_not_take_the_good_ones_with_it() {
        let mut catalog = Catalog::default();
        catalog.absorb(
            "prova",
            r#"[{"id": "buono", "kind": "manual"},
                {"id": "rotto", "kind": "inventato"}]"#,
        );
        assert_eq!(catalog.live().len(), 1);
        assert_eq!(catalog.problems.len(), 1);
        assert_eq!(catalog.problems[0].about, "rotto");
    }

    /// The same `id` written twice: the last loaded wins, and that is how
    /// somebody rewrites a shipped trigger without deleting it.
    #[test]
    fn the_last_descriptor_with_an_id_wins() {
        let mut catalog = Catalog::default();
        catalog.absorb("spedito", r#"[{"id": "x", "kind": "manual", "label": "primo"}]"#);
        catalog.absorb("mio", r#"[{"id": "x", "kind": "manual", "label": "secondo"}]"#);
        assert_eq!(catalog.live().len(), 1);
        assert_eq!(catalog.live()[0].descriptor.label, "secondo");
        assert_eq!(catalog.live()[0].source, "mio");
    }

    #[test]
    fn a_disabled_descriptor_disappears_from_the_live_list() {
        let mut catalog = Catalog::default();
        catalog.absorb("spedito", r#"[{"id": "x", "kind": "manual"}]"#);
        catalog.absorb("mio", r#"[{"id": "x", "kind": "manual", "disabled": true}]"#);
        assert!(catalog.live().is_empty());
        assert!(catalog.known().is_empty());
    }
}
