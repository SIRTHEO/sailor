//! What is installed on this machine, where it comes from, and whether it is
//! reachable. WHY IT EXISTS, WITH THE MEASURE: knowing what one person had
//! meant looking in **nineteen different directories** — six for skills, six
//! for rules, three for commands, two for agents, two for hooks. Not one of
//! those categories answered to a command: you found them by walking the
//! filesystem, and so you did not find them.

// THE REAL CONSEQUENCE of having no single list came out of the same census:
// `codex` runs **two divergent configurations** — the one you read by hand and
// the one Orca actually passes it — and nobody was flagging it.
//
// TWO SEPARATE RESPONSIBILITIES, as in the rest of the system: `discovery`
// knows where Claude Code loads things from, and here the list gets built.
pub mod discovery;
pub mod extensions;

use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The families of things one can have installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Skill,
    Agent,
    Command,
    Rule,
    Hook,
}

impl Kind {
    /// The name the family goes by for whoever reads the list.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Skill => "skill",
            Kind::Agent => "agent",
            Kind::Command => "command",
            Kind::Rule => "rule",
            Kind::Hook => "hook",
        }
    }
}

/// Can it actually be reached?
///
/// The third variant is not a cop-out. A skill inside a disabled plugin is
/// provably unreachable; a rule in a repo depends on who opens the session and
/// where, so calling it active would be a convenient lie. `Unknown` carries the
/// reason, so the reader knows what would have to be checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "reason", rename_all = "lowercase")]
pub enum Reach {
    Active,
    Inactive(String),
    Unknown(String),
}

/// One entry of the inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub kind: Kind,
    /// The name it is invoked by: `handoff`, `plugin:skill`, `builder`.
    pub name: String,
    pub description: String,
    /// Where it comes from: `home`, `plugin <name>`, `repo <name>`.
    pub origin: String,
    pub path: String,
    pub reach: Reach,
    /// Whether the model may invoke it, or only the person typing.
    /// `disable-model-invocation: true` does not mean "it is not there".
    pub by_model: bool,
}

/// A root things load from: the home, or a repo with its own `.claude/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    /// What it is called for the reader: `home`, or the repo's name.
    pub label: String,
    /// The directory that contains `.claude/`, not `.claude/` itself.
    pub path: PathBuf,
    /// The home always loads; a repo only for a session opened inside it.
    pub is_home: bool,
    /// A skills directory **Claude Code does not load**. "Nobody loads it"
    /// stood here half a day: `~/.factory` and `~/.commandcode` hold **767
    /// links each** into it, **1508 of them broken**, and it shrank from 767+
    /// entries to 33 with neither side noticing. The verdict is therefore
    /// narrowed to what this program can know — from **here** they are not
    /// invocable, and whoever wants them links them.
    pub is_warehouse: bool,
}

impl Root {
    pub fn home(path: &Path) -> Root {
        Root {
            label: "home".to_string(),
            path: path.to_path_buf(),
            is_home: true,
            is_warehouse: false,
        }
    }

    pub fn repo(path: &Path) -> Root {
        Root {
            label: named_after(path),
            path: path.to_path_buf(),
            is_home: false,
            is_warehouse: false,
        }
    }

    /// A skills directory no configuration loads. `path` is the directory that
    /// holds them directly, not a repo root.
    pub fn warehouse(label: &str, path: &Path) -> Root {
        Root {
            label: label.to_string(),
            path: path.to_path_buf(),
            is_home: false,
            is_warehouse: true,
        }
    }
}

fn named_after(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The whole inventory, in a stable order: two reads in a row give the same
/// sequence, or comparing one day against the next is worth nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inventory {
    pub entries: Vec<Entry>,
    /// The roots actually walked, spelled out: a list that does not say where
    /// it looked cannot be contradicted.
    pub roots: Vec<String>,
    /// Plugin copies left in the cache that are not the installed one. Not
    /// inventory entries — nothing loads them — but they are disk space, and
    /// until someone counts them nobody removes them.
    pub stale_plugin_copies: usize,
    /// **WHERE IT COULD NOT LOOK**, with the reason. It completes `roots`:
    /// saying where one searched makes the list refutable, saying where one
    /// failed to search makes it honest.
    #[serde(default)]
    pub unseen: Vec<String>,
    /// Whether anyone declared where to look. When `false`, a thin inventory
    /// does not say the machine is empty: it says nobody said where to look.
    #[serde(default)]
    pub bases_declared: bool,
}

impl Inventory {
    pub fn of(&self, kind: Kind) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.kind == kind).collect()
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }
}

/// The inventory of a survey, **including what it could not see**.
///
/// It stands beside `collect` rather than replacing it because whoever builds
/// the roots by hand — the tests — has no survey to pass, and forcing an empty
/// one would move the lie one step instead of removing it.
pub fn collect_survey(survey: &Survey) -> Inventory {
    let mut out = collect(&survey.roots);
    out.unseen = survey
        .unreadable
        .iter()
        .map(|u| format!("{}: {}", u.path.display(), u.reason))
        .collect();
    out.bases_declared = survey.bases_declared;
    out
}

/// Builds the inventory by walking every root.
///
/// WHAT IT DOES NOT DO: it does not judge whether a skill is any good, it does
/// not delete it and does not move it — it lists, and the decision to remove
/// stays with whoever reads. Whoever shows the list — the command line or the
/// window — knows nothing about paths.
pub fn collect(roots: &[Root]) -> Inventory {
    let mut entries = Vec::new();
    let mut stale = 0usize;
    let home = roots.iter().find(|r| r.is_home).map(|r| r.path.as_path());
    for root in roots {
        if root.is_home {
            let (found, dropped) = home_skills(&root.path);
            entries.extend(found);
            stale += dropped;
            let (found, dropped) = home_agents(&root.path);
            entries.extend(found);
            stale += dropped;
        } else if root.is_warehouse {
            entries.extend(warehouse_skills(root, home));
            continue;
        } else {
            entries.extend(repo_dir(root, "skills", Kind::Skill));
            entries.extend(repo_dir(root, "agents", Kind::Agent));
        }
        entries.extend(commands_of(root));
        entries.extend(rules_of(root));
        entries.extend(hooks_of(root));
    }
    entries.sort_by(|a, b| {
        (a.kind, &a.name, &a.origin, &a.path).cmp(&(b.kind, &b.name, &b.origin, &b.path))
    });
    // THE DESCRIPTION IS PART OF THE IDENTITY, and that is not a detail:
    // several different hooks share one event and one matcher, told apart only
    // by the command they run. Without this field the count said 29 hooks where
    // there were 57 — measured against `settings.json` read by hand.
    entries.dedup_by(|a, b| {
        a.kind == b.kind && a.name == b.name && a.path == b.path && a.description == b.description
    });
    Inventory {
        entries,
        roots: roots
            .iter()
            .map(|r| format!("{}: {}", r.label, r.path.to_string_lossy()))
            .collect(),
        stale_plugin_copies: stale,
        // Empty because whoever calls `collect` has no survey: they built the
        // roots themselves and know what they passed. `collect_survey` fills
        // these in, because it starts from a survey and knows what it missed.
        unseen: Vec::new(),
        bases_declared: true,
    }
}

/// Is the file inside the plugin version Claude Code actually loads?
///
/// Outside the plugin cache the question does not arise: it always holds.
/// Inside, it holds only under one of the declared `installPath`s — and if that
/// list is empty (file missing or unreadable) nothing is discarded: "I don't
/// know" must not turn into "it is not there".
fn is_the_installed_copy(path: &Path, installed: &BTreeSet<PathBuf>) -> bool {
    if !path.to_string_lossy().contains("/plugins/cache/") || installed.is_empty() {
        return true;
    }
    installed.iter().any(|root| path.starts_with(root))
}

/// The home's skills, plugins included, with disabled plugins accounted for
/// instead of hidden. THE DIFFERENCE FROM THE NUDGE IS DELIBERATE: `skill_nudge`
/// drops the unreachable in silence, because it may only suggest invocable
/// things. Here a disabled skill stays in the list with the reason it is off —
/// the case shown nowhere today, without which "I have fourteen plugins" and
/// "six of them work" look like the same sentence.
fn home_skills(home: &Path) -> (Vec<Entry>, usize) {
    let on = discovery::enabled_plugins(home);
    let installed = discovery::installed_paths(home);
    let mut out = Vec::new();
    let mut stale = 0usize;
    for (root, pattern) in discovery::skill_sources(home) {
        for path in discovery::glob(&root, &pattern) {
            if !is_the_installed_copy(&path, &installed) {
                stale += 1;
                continue;
            }
            let Some((name, description, by_model)) = named(&path) else {
                continue;
            };
            let origin = discovery::origin(&path);
            let prefix = origin.prefix();
            // **WHO MAY SWITCH IT OFF DEPENDS ON WHERE IT COMES FROM**, and the
            // `match` says so in three lines. This used to be three arms whose
            // first two returned the same thing, with a person's name hidden in
            // the second: a condition whose outcome changes nothing is not read,
            // it is skimmed — and that is why that line survived so long.
            let reach = match &origin {
                discovery::Origin::Home | discovery::Origin::Collection(_) => Reach::Active,
                discovery::Origin::Plugin(name) if on.contains(name) => Reach::Active,
                discovery::Origin::Plugin(name) => {
                    Reach::Inactive(format!("the plugin {name} is not enabled"))
                }
            };
            let declared = discovery::manifest(&path);
            let reach = match (&declared, path.parent().and_then(|p| p.file_name())) {
                (Some(names), Some(own))
                    if !names.contains(&own.to_string_lossy().into_owned()) =>
                {
                    Reach::Inactive(format!(
                        "on disk, but the manifest of {} does not declare it",
                        prefix.trim_end_matches(':')
                    ))
                }
                _ => reach,
            };
            out.push(Entry {
                kind: Kind::Skill,
                name: format!("{prefix}{name}"),
                description,
                // **WHERE IT COMES FROM NEEDS THE RIGHT WORD**: a collection
                // installed as a folder is not a plugin, and calling it one in
                // a list a person reads sends them hunting among the plugins,
                // where it is not.
                origin: match &origin {
                    discovery::Origin::Home => "home".to_string(),
                    discovery::Origin::Plugin(name) => format!("plugin {name}"),
                    discovery::Origin::Collection(name) => format!("collection {name}"),
                },
                path: path.to_string_lossy().into_owned(),
                reach,
                by_model,
            });
        }
    }
    (out, stale)
}

fn home_agents(home: &Path) -> (Vec<Entry>, usize) {
    let installed = discovery::installed_paths(home);
    let mut out = Vec::new();
    let mut stale = 0usize;
    for (root, pattern) in discovery::agent_sources(home) {
        for path in discovery::glob(&root, &pattern) {
            if !is_the_installed_copy(&path, &installed) {
                stale += 1;
                continue;
            }
            let Some((name, description, by_model)) = named(&path) else {
                continue;
            };
            let plugin = discovery::prefix(&path);
            let plugin = plugin.strip_suffix(':').unwrap_or("").to_string();
            out.push(Entry {
                kind: Kind::Agent,
                name,
                description,
                origin: if plugin.is_empty() {
                    "home".to_string()
                } else {
                    format!("plugin {plugin}")
                },
                path: path.to_string_lossy().into_owned(),
                reach: Reach::Active,
                by_model,
            });
        }
    }
    (out, stale)
}

/// (name, description, invocable by the model) of whatever declares a `name:`.
///
/// It does not reuse `discovery::frontmatter`, which drops what the model
/// cannot invoke; here that has to be shown. The difference is spelled out
/// next to `matter_and_invocability`.
fn named(path: &Path) -> Option<(String, String, bool)> {
    let (matter, by_model) = discovery::matter_and_invocability(path)?;
    let start = discovery::field_start(&matter, "name:")?;
    let name = matter[start..]
        .lines()
        .next()?
        .split_whitespace()
        .next()?
        .to_string();
    Some((name, discovery::description(&matter, false), by_model))
}

/// The skills of a directory no configuration loads.
///
/// There is one today, and it is worth saying why that is not a corner case:
/// **95 skills written, five reachable**. It is not a defect to repair here —
/// linking them is a decision, not maintenance — but while the list stays
/// silent that decision never gets taken, because nobody knows it is there.
fn warehouse_skills(root: &Root, home: Option<&Path>) -> Vec<Entry> {
    discovery::glob(&root.path, "*/SKILL.md")
        .into_iter()
        .filter_map(|path| {
            let (name, description, by_model) = named(&path)?;
            let folder = path.parent()?.file_name()?.to_string_lossy().into_owned();
            // A LINKED SKILL IS REACHABLE, and it has to be said: five of these
            // are. Declaring them all off because they sit in the warehouse
            // would be the same error inverted, and would cost the inventory
            // its credit on exactly the entries it can judge.
            let linked = home
                .map(|h| {
                    extensions::declared(Some(h))
                        .iter()
                        .any(|product| h.join(&product.home).join("skills").join(&folder).exists())
                })
                .unwrap_or(false);
            Some(Entry {
                kind: Kind::Skill,
                name,
                description,
                origin: format!("warehouse {}", root.label),
                path: path.to_string_lossy().into_owned(),
                reach: if linked {
                    Reach::Active
                } else {
                    Reach::Inactive(
                        "it sits in a directory no configuration loads: to invoke it, link it \
                         among the home's skills"
                            .to_string(),
                    )
                },
                by_model,
            })
        })
        .collect()
}

/// Skills and agents declared inside a repo.
///
/// They hold only for whoever opens a session in there: `Unknown`, with the
/// reason.
fn repo_dir(root: &Root, folder: &str, kind: Kind) -> Vec<Entry> {
    let base = first_project_dir(root).join(folder);
    let pattern = if kind == Kind::Skill {
        "*/SKILL.md"
    } else {
        "*.md"
    };
    discovery::glob(&base, pattern)
        .into_iter()
        .filter_map(|path| {
            let (name, description, by_model) = named(&path)?;
            Some(Entry {
                kind,
                name,
                description,
                origin: format!("repo {}", root.label),
                path: path.to_string_lossy().into_owned(),
                reach: Reach::Unknown(format!("only for a session opened inside {}", root.label)),
                by_model,
            })
        })
        .collect()
}

/// The project directory of the first product that declares one.
///
/// One and not all, because every caller below reads a single directory: a
/// second product with its own is a second call, not a merged path.
fn first_project_dir(root: &Root) -> PathBuf {
    match extensions::project_dirs().first() {
        Some(dir) => root.path.join(dir),
        None => root.path.clone(),
    }
}

fn commands_of(root: &Root) -> Vec<Entry> {
    let base = first_project_dir(root).join("commands");
    discovery::glob(&base, "*.md")
        .into_iter()
        .filter_map(|path| {
            // A COMMAND WITHOUT FRONTMATTER IS STILL A COMMAND. Claude Code
            // takes the whole file as the prompt; demanding frontmatter made
            // `/work-loop-headless` vanish from the list of what exists — and a
            // list that stays quiet about what is there is worse than no list.
            let (matter, by_model) =
                discovery::matter_and_invocability(&path).unwrap_or_else(|| (String::new(), true));
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let description = match discovery::description(&matter, true) {
                empty if empty.is_empty() => first_heading(&path),
                found => found,
            };
            Some(Entry {
                kind: Kind::Command,
                name: format!("/{name}"),
                description,
                origin: origin_of(root),
                path: path.to_string_lossy().into_owned(),
                reach: reach_of(root),
                by_model,
            })
        })
        .collect()
}

/// Rules carry no frontmatter: they are named after the file, and their
/// description is their first heading. Reading the whole text to distil one
/// line would cost more than it returns on a directory people skim.
fn rules_of(root: &Root) -> Vec<Entry> {
    let base = first_project_dir(root).join("rules");
    let mut out = Vec::new();
    // One level of subdirectories, because rules group by subject
    // (`rules/common/`, `rules/typescript/`) and stopping at the top level lost
    // a third of them without saying so.
    let found: Vec<PathBuf> = discovery::glob(&base, "*.md")
        .into_iter()
        .chain(discovery::glob(&base, "*/*.md"))
        .collect();
    for path in found {
        let name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(Entry {
            kind: Kind::Rule,
            name,
            description: first_heading(&path),
            origin: origin_of(root),
            path: path.to_string_lossy().into_owned(),
            reach: reach_of(root),
            // A rule is not invoked, it is applied. That holds for everyone.
            by_model: true,
        });
    }
    out
}

/// The title of a Markdown document, or empty when it has none.
fn first_heading(path: &Path) -> String {
    let Ok(text) = fs::read_to_string(path) else {
        return String::new();
    };
    for line in text.lines().take(40) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("# ") {
            return rest.trim().to_string();
        }
    }
    String::new()
}

/// The hooks declared in `settings.json`, event by event.
///
/// THE COMMAND IS CHECKED, NOT BELIEVED. A hook pointing at a file that is gone
/// raises no error for anyone: it goes quiet, and whoever wrote it keeps
/// believing it defends something. Four scripts were still pointing at
/// `~/.claude/rust/…`, deleted the day this was measured.
fn hooks_of(root: &Root) -> Vec<Entry> {
    let path = first_project_dir(root).join("settings.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(events) = value.get("hooks").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (event, groups) in events {
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for group in groups {
            let matcher = group
                .get("matcher")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();
            let Some(hooks) = group.get("hooks").and_then(|v| v.as_array()) else {
                continue;
            };
            for hook in hooks {
                let Some(command) = hook.get("command").and_then(|v| v.as_str()) else {
                    continue;
                };
                out.push(Entry {
                    kind: Kind::Hook,
                    name: format!("{event} · {matcher} · {}", hook_label(command)),
                    description: command.to_string(),
                    origin: origin_of(root),
                    path: path.to_string_lossy().into_owned(),
                    reach: match missing_file(command, &root.path) {
                        Some(missing) => {
                            Reach::Inactive(format!("points at {missing}, which does not exist"))
                        }
                        None => reach_of(root),
                    },
                    by_model: true,
                });
            }
        }
    }
    out
}

/// What a hook is called for whoever reads it: not the whole command, but the
/// part that tells it apart. WITHOUT THIS TWO HOOKS BECOME ONE — event and
/// matcher do not identify them: **eight** live on `PreToolUse · Bash`, and
/// storing them under one key made seven vanish in silence, 30 entries lost out
/// of 358. A list that loses to collisions is worse than no list, because it
/// looks complete.
fn hook_label(command: &str) -> String {
    // OPTIONS TELL HOOKS APART, and the first version threw them away: two
    // hooks running the same subcommand with different options — `orca-cleanup
    // --close` and `orca-cleanup --names --rename` — are two hooks, and
    // dropping the options collapsed them into one.
    const SHELL_NOISE: &[&str] = &[
        "cd", "||", "&&", ";", "&", "true", "exec", "nohup", "sh", "-c",
    ];
    let words: Vec<&str> = command.split_whitespace().collect();
    // The executable is the last word that is a path: before it there is only
    // shell preamble, after it the arguments that matter.
    let executable = words.iter().rposition(|word| {
        word.contains('/') && !word.starts_with('>') && !word.contains(">/") && !word.contains("</")
    });
    let mut label: Vec<&str> = words
        .iter()
        .skip(executable.map_or(0, |index| index + 1))
        .copied()
        .filter(|word| {
            !word.contains('/')
                && !word.starts_with('>')
                && !SHELL_NOISE.contains(word)
                && !word.is_empty()
        })
        .collect();
    if label.is_empty() {
        // No arguments: then the hook is the script it runs.
        if let Some(index) = executable {
            label.push(words[index].rsplit('/').next().unwrap_or(words[index]));
        }
    }
    if label.is_empty() {
        return command.to_string();
    }
    let joined = label
        .join(" ")
        .trim_matches(|c: char| c == '"' || c == '\'' || c == ';')
        .to_string();
    // A hook written as an inline program has no name, it has a body. Truncate
    // it: the name is there to tell it apart, not to tell its story, and the
    // whole command stays in the description, where whoever wants it finds it.
    const NAME_CEILING: usize = 48;
    if joined.chars().count() <= NAME_CEILING {
        return joined;
    }
    let short: String = joined.chars().take(NAME_CEILING).collect();
    format!("{short}…")
}

/// The first path named by the command that does not exist on disk.
///
/// Deliberately naive: it looks at words that look like a file to run, it does
/// not interpret the shell. THE TWO NARROWINGS BELOW COME FROM TWO REAL FALSE
/// ALARMS, both caught on the first run against the real disk.
fn missing_file(command: &str, home: &Path) -> Option<String> {
    for word in command.split_whitespace() {
        // Shell punctuation stays glued to the word — `…/claude-hook.sh';` —
        // and has to come off, or a hook that is alive reads as dead.
        let word = word.trim_matches(|c: char| {
            c == '"' || c == '\'' || c == '`' || c == ';' || c == ',' || c == ')' || c == '('
        });
        let expanded = match word.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None if word.starts_with('/') => PathBuf::from(word),
            None => continue,
        };
        // And `/clear` begins with `/` without being a file: it is the argument
        // of a `SessionStart` hook. Hence an extension in the last segment as
        // well — whoever points at a script writes it with its own, and a word
        // that has none is not a file to go looking for.
        let looks_like_a_file = expanded
            .file_name()
            .map(|n| n.to_string_lossy().contains('.'))
            .unwrap_or(false);
        if looks_like_a_file && !expanded.exists() {
            return Some(word.to_string());
        }
    }
    None
}

fn origin_of(root: &Root) -> String {
    if root.is_home {
        "home".to_string()
    } else {
        format!("repo {}", root.label)
    }
}

fn reach_of(root: &Root) -> Reach {
    if root.is_home {
        Reach::Active
    } else {
        Reach::Unknown(format!("only for a session opened inside {}", root.label))
    }
}

/// What a survey found, **and what it could not look at**. TWO LISTS EXIST
/// BECAUSE ONE LIES: `repos_under` met an unreadable base, did `continue` and
/// returned a shorter list, so "there are none" and "I could not look" reached
/// the reader in the same shape — and they lead to opposite decisions. It is
/// fault 12: inside a sandbox `launchctl` answers empty instead of refusing,
/// and a piped `ps` read five running CLIs as none.
#[derive(Debug, Default)]
pub struct Survey {
    /// The roots actually found.
    pub roots: Vec<Root>,
    /// The bases that could not be read, with the reason.
    pub unreadable: Vec<Unreadable>,
    /// Whether anyone declared where to look. `false` means an empty list says
    /// nothing about the world: it says nobody said where to search.
    pub bases_declared: bool,
}

/// A base that could not be read, and why.
#[derive(Debug)]
pub struct Unreadable {
    pub path: PathBuf,
    pub reason: String,
}

/// The roots to look at on this machine: the home, and the working repos.
///
/// IT LIVES HERE AND NOT IN THE TWO CALLERS. The command line and the window
/// must report the same number on the same machine: if each picked its own
/// roots, the first time one changed they would diverge again — which is the
/// defect this crate exists to remove.
pub fn default_roots(config_dir: Option<&Path>) -> Survey {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    // THE BASES ARE DECLARED, NOT SEARCHED ACROSS THE WHOLE DISK: walking from
    // `/` would also find the worktrees, where the same rules reappear as
    // links.
    default_roots_from(&home, &declared_bases(config_dir))
}

/// The same rule applied to a given home and given declared bases, instead of
/// the ones belonging to this process.
///
/// **SEPARATE, OR IT CANNOT BE TESTED.** A function that reads the environment
/// can only be tested by changing the process environment, and tests run in
/// parallel: the first to touch a variable falsifies the others.
pub fn default_roots_from(home: &Path, bases: &[PathBuf]) -> Survey {
    let mut survey = repos_under(bases);
    survey.roots.insert(0, Root::home(home));
    // WAREHOUSES ARE LOOKED FOR WHERE WE LOOK, not where one person's used to
    // be. The first is the home's; the others sit under the declared bases, and
    // if none is declared there are none — which is the truth, not a fallback.
    let mut warehouses = vec![(".agents".to_string(), home.join(".agents").join("skills"))];
    for base in bases {
        let label = base
            .file_name()
            .map(|n| format!("{}/.agents", n.to_string_lossy()))
            .unwrap_or_else(|| ".agents".to_string());
        warehouses.push((label, base.join(".agents").join("skills")));
    }
    // TWO WAREHOUSES REALLY ARE TWO: the links inside the home's skills point
    // at the first, never at the second. Treating them as one made the list say
    // "none is linked" even about the ones that are.
    for (label, path) in warehouses {
        if path.is_dir() {
            survey.roots.push(Root::warehouse(&label, &path));
        }
    }
    survey
}

/// The **declared** working bases: `SAILOR_WORK_ROOTS` if set, otherwise the
/// `work-roots` file in Sailor's config directory, one base per line.
/// Two personal working directories used to be compiled in here: on that
/// machine they existed, so the defect was invisible; anywhere else the
/// inventory answered "zero repos" with exit 0, indistinguishable from a
/// machine that really was empty.
pub fn declared_bases(config_dir: Option<&Path>) -> Vec<PathBuf> {
    if let Ok(declared) = std::env::var("SAILOR_WORK_ROOTS") {
        let bases: Vec<PathBuf> = declared
            .split(':')
            .filter(|piece| !piece.trim().is_empty())
            .map(PathBuf::from)
            .collect();
        if !bases.is_empty() {
            return bases;
        }
    }
    let Some(dir) = config_dir else {
        // Whoever declares nothing gets `bases_declared` at `false`, so the
        // reader can say *you never told me* instead of *there is nothing*.
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(dir.join("work-roots")) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(PathBuf::from)
        .collect()
}

/// The repos carrying a `.claude/`, searched under the working directories.
///
/// Depth two and no deeper: `<base>/suite` is a repo, but going further would
/// walk into the worktrees, where the same rules reappear as links — and the
/// inventory would claim twenty times what it has.
/// Whether a directory holds any declared product's extensions.
fn carries_extensions(at: &Path) -> bool {
    extensions::project_dirs()
        .iter()
        .any(|dir| at.join(dir).is_dir())
}

pub fn repos_under(bases: &[PathBuf]) -> Survey {
    let mut found: BTreeSet<PathBuf> = BTreeSet::new();
    let mut unreadable = Vec::new();
    for base in bases {
        if carries_extensions(base) {
            found.insert(base.clone());
        }
        // THE `continue` THAT USED TO BE HERE ATE THE REASON. A base that will
        // not open and an empty base produced the same shorter list, and the
        // reader concluded "there are none" in both cases.
        let entries = match fs::read_dir(base) {
            Ok(entries) => entries,
            Err(why) => {
                unreadable.push(Unreadable {
                    path: base.clone(),
                    reason: why.to_string(),
                });
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && carries_extensions(&path) {
                found.insert(path);
            }
        }
    }
    Survey {
        roots: found.iter().map(|p| Root::repo(p)).collect(),
        unreadable,
        bases_declared: !bases.is_empty(),
    }
}
