//! Routing: what the user types is looked at **before** it runs, and goes to a
//! flow or to the terminal.
//!
//! **THE DEFAULT IS THE TERMINAL, ALWAYS.** Routing is an addition to a
//! working terminal, not a layer the terminal passes through. Every way out
//! of [`Router::route`] which is not a fired rule leads to [`Routed::Command`]:
//! no rule, wrong rule, no descriptor, empty list — it passes. A terminal
//! which now and then does not run what you type is worse than one which does
//! not route at all, because it turns unpredictable, and a terminal's
//! unpredictability is paid on every line written after, not just on that one.
//!
//! **HOW IT DECIDES IS DATA, NOT A BRANCH OF CODE.** No flow name and no word
//! to look for appears in this file: the rules are descriptors — shipped with
//! the binary, rewritable in `~/.config/sailor/routes.d/` — and changing them
//! recompiles nothing. It is the standing constraint «we write as code only
//! what touches the world»: here the code is the guard, which measures the
//! machine; the rest is a list.
//!
//! **THE GUARD IS CODE AND NO DESCRIPTOR CAN SWITCH IT OFF.** It lives here,
//! and the way it leans is declared: data can only ask to route, the guard can
//! only let through. A badly written rule misses a routing; it does not eat a
//! `git status`. The one exception is an `explicit` rule, which the user marked
//! themselves with a prefix no shell would run — and loading refuses explicit
//! rules whose prefix could start a command, so the exception does not widen by
//! itself.
//!
//! **WHY `toolbox::probe::look_up` AND NOT A `which` WRITTEN HERE.** Because it
//! can answer «I don't know»: if even a single directory of the path could not
//! be read, the answer is not «it isn't there». That is exactly the shape the
//! rule «when in doubt, pass» needs — a doubt about the machine turns into a
//! command run, never a request hijacked.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use toolbox::{Look, Machine};

/// The rules shipped with the product, embedded in the binary: there is no
/// install path to guess, and they stay data.
pub const BUILTIN: &str = include_str!("../descriptors/default.json");

/// The provenance written into `Loaded` and `Problem`, so it is a value and not
/// a label. Spelled as `trigger` and `models` already spell theirs: a reader
/// comparing sources across the three must not have to know they differ.
pub const BUILTIN_SOURCE: &str = "built-in";

/// Where the rules are taken from. The same three shapes as the tool and
/// trigger descriptors, and on purpose: whoever has learnt where a row gets
/// added must not learn it a second time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Builtin,
    File(PathBuf),
    /// Every `*.json` inside a directory, in name order.
    Dir(PathBuf),
}

/// Which shape of line a rule recognises.
///
/// **TWO, AND NEITHER IS A REGULAR EXPRESSION.** A routing rule is written by
/// whoever uses Sailor, in a JSON file, untested: a wrong regular expression is
/// mute, catches more than its author believes, and the way they find out is a
/// command which did not run. These two shapes read aloud and have no corners.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Match {
    /// The line begins with this text. The comparison ignores case.
    StartsWith { text: String },
    /// All of these words appear in the line, as whole words.
    ContainsAll { words: Vec<String> },
}

/// One row of the routing rule list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub id: String,
    /// The id of the flow to send the request to. It stays in Italian when
    /// the flow is named that way: it is data, and data is not renamed for
    /// style.
    pub flow: String,
    #[serde(default)]
    pub label: String,
    pub when: Match,
    /// The user marked this row a request themselves: the guard does not
    /// apply. Allowed on a marker which cannot start a command, and no other.
    #[serde(default)]
    pub explicit: bool,
    /// Takes the recognised text out of what gets handed to the flow.
    #[serde(default)]
    pub strip_match: bool,
    /// Below this word count the rule does not fire. The second defence, for
    /// word rules: a request in language is long, a command line is short.
    #[serde(default)]
    pub minimum_words: usize,
    /// For whoever reads the list. It enters no decision.
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub route: Route,
    pub source: String,
}

/// Something which failed to load, with the why and the where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub source: String,
    pub about: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub routes: Vec<Loaded>,
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
                        // A missing directory is the normal case of somebody
                        // who never wrote a rule of their own; one there and
                        // unreadable is a fault, and the disk tells them apart.
                        if dir.exists() {
                            catalog.problems.push(Problem {
                                source: dir.to_string_lossy().into_owned(),
                                about: "la cartella".to_string(),
                                reason: "non si è potuta leggere".to_string(),
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
                about: "il file".to_string(),
                reason: format!("non si è potuto leggere: {error}"),
            }),
        }
    }

    /// The text is read twice on purpose: item by item, so a stray comma at
    /// the end does not wipe out the good rules above it.
    pub fn absorb(&mut self, source: &str, text: &str) {
        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(error) => {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about: "il file".to_string(),
                    reason: format!("non è JSON valido: {error}"),
                });
                return;
            }
        };
        let items = match &value {
            Value::Array(items) => items.clone(),
            Value::Object(map) => match map.get("routes") {
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
                // An entry that declares no `id` still has to be named in a
                // problem, and this stands in as the identifier. Same words as
                // `trigger` and `toolbox` use for the same stand-in.
                .unwrap_or_else(|| format!("entry number {}", index + 1));
            let route: Route = match serde_json::from_value(item.clone()) {
                Ok(route) => route,
                Err(error) => {
                    self.problems.push(Problem {
                        source: source.to_string(),
                        about,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            if let Err(reason) = coherent(&route) {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about,
                    reason,
                });
                continue;
            }
            self.replace(Loaded {
                route,
                source: source.to_string(),
            });
        }
    }

    fn malformed(&mut self, source: &str) {
        self.problems.push(Problem {
            source: source.to_string(),
            about: "il file".to_string(),
            reason: "non contiene né un array né un campo `routes`".to_string(),
        });
    }

    fn replace(&mut self, loaded: Loaded) {
        match self
            .routes
            .iter_mut()
            .find(|found| found.route.id == loaded.route.id)
        {
            Some(existing) => *existing = loaded,
            None => self.routes.push(loaded),
        }
    }

    /// The live ones, in stable `id` order: two reads in a row must give the
    /// same sequence, or two terminals would route the same line differently
    /// when two rules both recognise it.
    pub fn live(&self) -> Vec<&Loaded> {
        let mut out: Vec<&Loaded> = self
            .routes
            .iter()
            .filter(|loaded| !loaded.route.disabled)
            .collect();
        out.sort_by(|left, right| left.route.id.cmp(&right.route.id));
        out
    }

    pub fn known(&self) -> Vec<String> {
        self.live()
            .into_iter()
            .map(|loaded| loaded.route.id.clone())
            .collect()
    }
}

/// An explicit rule which is not a marker does not load.
///
/// **THIS IS WHERE THE EXCEPTION TO THE GUARD IS KEPT SMALL.** `explicit` lets
/// the user say «this is a request» about a phrase which looks like a command.
/// Were a rule beginning with `git` markable `explicit`, the descriptor would
/// hold the power to eat a command — exactly the power this crate denies it.
/// The day somebody writes one is the one day it is easy to notice.
fn coherent(route: &Route) -> Result<(), String> {
    if route.flow.trim().is_empty() {
        return Err("una regola deve dire a quale flusso manda: `flow` è vuoto".to_string());
    }
    if route.strip_match && !matches!(route.when, Match::StartsWith { .. }) {
        return Err(
            "`strip_match` toglie un prefisso, quindi vale solo con `starts_with`: da una regola per parole non c'è niente di preciso da togliere"
                .to_string(),
        );
    }
    if !route.explicit {
        return Ok(());
    }
    let Match::StartsWith { text } = &route.when else {
        return Err(
            "una regola esplicita scavalca la guardia, e può farlo solo se è un marcatore: serve `starts_with`, non una regola per parole"
                .to_string(),
        );
    };
    match text.chars().next() {
        None => Err("una regola esplicita ha un marcatore vuoto, che sta all'inizio di ogni riga: smisterebbe tutto".to_string()),
        Some(first) if can_start_a_command(first) => Err(format!(
            "il marcatore «{text}» comincia con «{first}», che può iniziare un comando: una regola esplicita scavalca la guardia, quindi il suo marcatore deve essere qualcosa che nessuna shell eseguirebbe"
        )),
        Some(_) => Ok(()),
    }
}

/// The characters a command line can begin with: a letter, a digit, and the
/// few signs which name a file or a variable.
fn can_start_a_command(first: char) -> bool {
    first.is_alphanumeric()
        || matches!(first, '.' | '/' | '~' | '_' | '-' | '$' | '\\' | '\'' | '"')
}

/// Whoever can say whether a word is a command **on this machine**.
///
/// A trait and not a function because it is the part which touches the world,
/// and a test must be able to declare a world of its own: without that, «the
/// guard stops `git`» would hold true just where `git` is installed, and the
/// battery would tell of the machine rather than of the code.
pub trait CommandLookup: Send + Sync {
    /// **`true` ALSO MEANS «I DON'T KNOW».** An implementor of this trait must
    /// answer `true` when it failed to look everywhere: a doubt about the
    /// machine must turn into a command run, never a request hijacked.
    fn is_command(&self, word: &str) -> bool;
}

/// The real machine: the directories of the path, as the shell looks at them.
pub struct PathLookup {
    machine: Machine,
}

impl PathLookup {
    pub fn current() -> PathLookup {
        PathLookup {
            machine: Machine::current(),
        }
    }

    pub fn on(machine: Machine) -> PathLookup {
        PathLookup { machine }
    }
}

impl CommandLookup for PathLookup {
    fn is_command(&self, word: &str) -> bool {
        match toolbox::probe::look_up(word, &self.machine) {
            Look::Found(_) => true,
            // «I could not look everywhere» weighs as «it is there»: the rule
            // «when in doubt, pass», written where the decision is made.
            Look::Blocked(_) => true,
            Look::Missing => false,
        }
    }
}

/// The words a shell runs without looking for any binary.
///
/// **THEY ARE CODE AND NOT DESCRIPTORS, DELIBERATELY.** Not a configuration
/// choice: a fact about the shell, of the same kind as the search path. In
/// data a descriptor could *weaken* the guard by deleting `cd` from the list —
/// and this file is safe because data can only ask to route.
const SHELL_BUILTINS: &[&str] = &[
    ".", ":", "[", "alias", "bg", "bind", "break", "builtin", "case", "cd", "command", "continue",
    "declare", "dirs", "disown", "do", "done", "echo", "elif", "else", "esac", "eval", "exec",
    "exit", "export", "false", "fc", "fg", "fi", "for", "getopts", "hash", "help", "history", "if",
    "jobs", "kill", "let", "local", "logout", "popd", "printf", "pushd", "pwd", "read", "readonly",
    "return", "set", "shift", "shopt", "source", "test", "then", "time", "times", "trap", "true",
    "type", "typeset", "ulimit", "umask", "unalias", "unset", "until", "wait", "where", "which",
    "while",
];

/// The signs which make a line shell syntax, and never a sentence.
///
/// The same asymmetry as everywhere else holds: their presence makes a line
/// **pass**, so one sign too many in this list costs a missed routing, not an
/// eaten command.
const SHELL_SIGNS: &[&str] = &["|", "&&", "||", ">>", "<<", ">", "<", ";", "$(", "`", "&"];

/// Why a line went to the terminal rather than to a flow.
///
/// **EVERY PASS STATES ITS OWN REASON**, and that is no cosmetics: a routing
/// which does not fire is mute by definition — the line runs, and it all looks
/// normal. Without the reason, whoever wrote a rule which does not work has
/// nothing to look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Passed {
    /// There was nothing to route.
    Empty,
    /// The first word is a command on this machine — or it could not be ruled
    /// out that it was one.
    RunnableFirstWord(String),
    /// A word of the shell: `cd`, `export`, `exit`.
    ShellWord(String),
    /// A sign which none but a shell reads.
    ShellSign(String),
    /// The first word names a file: `./x`, `/usr/bin/x`, `~/x`.
    PathLike(String),
    /// A variable assignment: `FOO=1 command`.
    Assignment(String),
    /// The guard would have let it route, but no rule recognised it.
    NoRuleMatched,
}

/// Where a line typed in a terminal goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// It passes: the terminal runs it as it stands.
    Command { line: String, why: Passed },
    /// It goes to the flow, with the request to hand over.
    Flow {
        /// The `id` of the rule which recognised the line: a reader must be
        /// able to trace back to the JSON row which decided, not to the flow
        /// alone.
        route: String,
        flow: String,
        /// The text to hand to the flow.
        text: String,
    },
}

/// The loaded rules, plus the machine the guard is measured on.
pub struct Router {
    routes: Vec<Route>,
    lookup: Arc<dyn CommandLookup>,
}

impl Router {
    pub fn new(catalog: &Catalog, lookup: Arc<dyn CommandLookup>) -> Router {
        Router {
            routes: catalog
                .live()
                .into_iter()
                .map(|loaded| loaded.route.clone())
                .collect(),
            lookup,
        }
    }

    /// The rules shipped with the product, on this machine.
    pub fn current() -> Router {
        let machine = Machine::current();
        let catalog = Catalog::load(&default_sources(&machine));
        Router::new(&catalog, Arc::new(PathLookup::on(machine)))
    }

    /// A terminal with no rules at all: routes nothing, runs everything.
    pub fn without_routes(lookup: Arc<dyn CommandLookup>) -> Router {
        Router {
            routes: Vec::new(),
            lookup,
        }
    }

    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    /// **WHERE THIS LINE GOES.** Three passes, in this order, and the order is
    /// the defence:
    ///
    /// 1. the explicit markers — the user has said themselves it is no
    ///    command, and loading has guaranteed no shell would run a line
    ///    beginning that way;
    /// 2. the guard: if the line has any shape of a command, it passes, and no
    ///    rule sees it;
    /// 3. the remaining rules.
    ///
    /// Outside these three, it passes.
    pub fn route(&self, line: &str) -> Routed {
        let text = line.trim();
        if text.is_empty() {
            return Routed::Command {
                line: line.to_string(),
                why: Passed::Empty,
            };
        }

        for route in self.routes.iter().filter(|route| route.explicit) {
            if let Some(request) = recognised(route, text) {
                return Routed::Flow {
                    route: route.id.clone(),
                    flow: route.flow.clone(),
                    text: request,
                };
            }
        }

        if let Some(why) = looks_like_a_command(text, self.lookup.as_ref()) {
            return Routed::Command {
                line: line.to_string(),
                why,
            };
        }

        for route in self.routes.iter().filter(|route| !route.explicit) {
            if let Some(request) = recognised(route, text) {
                return Routed::Flow {
                    route: route.id.clone(),
                    flow: route.flow.clone(),
                    text: request,
                };
            }
        }

        Routed::Command {
            line: line.to_string(),
            why: Passed::NoRuleMatched,
        }
    }
}

/// If `route` recognises `text`, the text to hand to the flow.
fn recognised(route: &Route, text: &str) -> Option<String> {
    if text.split_whitespace().count() < route.minimum_words {
        return None;
    }
    match &route.when {
        Match::StartsWith { text: marker } => {
            let lowered = text.to_lowercase();
            if !lowered.starts_with(&marker.to_lowercase()) {
                return None;
            }
            let request = if route.strip_match {
                // `get` and not a direct slice: the comparison happened on the
                // lowercased version, which for certain letters is not the
                // same byte length as the original. A cut mid-character would
                // be a panic inside a terminal.
                text.get(marker.len()..)?.trim().to_string()
            } else {
                text.to_string()
            };
            // A marker with nothing behind it is no request: sending it to a
            // flow would mean paying for a run on an empty line.
            if request.is_empty() {
                None
            } else {
                Some(request)
            }
        }
        Match::ContainsAll { words } => {
            if words.is_empty() {
                // A rule which asks for nothing would recognise every line.
                return None;
            }
            let present: Vec<String> = text
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .map(str::to_lowercase)
                .collect();
            let all = words
                .iter()
                .all(|wanted| present.iter().any(|word| word == &wanted.to_lowercase()));
            if all {
                Some(text.to_string())
            } else {
                None
            }
        }
    }
}

/// The guard. `Some(reason)` means «this is a command line: pass it».
///
/// **IT LEANS ENTIRELY ONE WAY.** Every check in here can add a reason to pass,
/// and nothing else. A wrong check costs a missed routing — which the user sees
/// at once, because the request lands in the shell and the shell complains —
/// while the opposite mistake costs a command which did not run, which the user
/// finds out later, and which takes away their trust in the terminal.
fn looks_like_a_command(text: &str, lookup: &dyn CommandLookup) -> Option<Passed> {
    for sign in SHELL_SIGNS {
        if text.contains(sign) {
            return Some(Passed::ShellSign((*sign).to_string()));
        }
    }
    let first = text.split_whitespace().next()?;
    if first.starts_with('/')
        || first.starts_with("./")
        || first.starts_with("../")
        || first.starts_with("~/")
        || first.contains('/')
    {
        return Some(Passed::PathLike(first.to_string()));
    }
    // `FOO=1 command`: an equals ahead of any space is an assignment, and no
    // sentence in language carries one in its first word.
    if let Some(at) = first.find('=') {
        if at > 0 {
            return Some(Passed::Assignment(first.to_string()));
        }
    }
    if SHELL_BUILTINS.contains(&first) {
        return Some(Passed::ShellWord(first.to_string()));
    }
    if lookup.is_command(first) {
        return Some(Passed::RunnableFirstWord(first.to_string()));
    }
    None
}

/// The sources the rules are taken from on a machine.
///
/// In the order they win: the shipped ones first, the user's after. With
/// `SAILOR_ROUTE_DESCRIPTORS` (paths separated by `:`, files or directories)
/// one adds wherever they like without touching the home.
pub fn default_sources(machine: &Machine) -> Vec<Source> {
    let mut out = vec![Source::Builtin];
    out.push(Source::Dir(
        toolbox::sailor_home_for(machine).join("routes.d"),
    ));
    if let Some(extra) = machine.env.get("SAILOR_ROUTE_DESCRIPTORS") {
        for raw in extra.split(':').filter(|s| !s.is_empty()) {
            let path = PathBuf::from(machine.expand(raw));
            if path.is_dir() {
                out.push(Source::Dir(path));
            } else {
                out.push(Source::File(path));
            }
        }
    }
    out
}
