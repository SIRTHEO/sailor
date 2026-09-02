//! How installed things are found: load roots, disabled-plugin filter, and the
//! frontmatter that yields name and description. NOT REWRITTEN — moved word for
//! word out of `claude-hooks::skill_nudge`: two callers need it, the nudge and
//! the inventory, and a second copy would diverge at the first Claude Code
//! change. Its proved equivalence with the Python against
//! `tools/oracle/skill-nudge.json` is the net: change a behaviour, it says so.

use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The places a command line loads skills from, in the order it loads them.
///
/// **DECLARED, NOT COMPILED.** They used to be four joins written here, naming
/// one product. Now they come from [`crate::extensions`], and a second product
/// is a file rather than a branch.
pub fn skill_sources(h: &Path) -> Vec<(PathBuf, String)> {
    places(h, |product| &product.skills)
}

pub fn agent_sources(h: &Path) -> Vec<(PathBuf, String)> {
    places(h, |product| &product.agents)
}

fn places(
    h: &Path,
    which: impl Fn(&crate::extensions::Product) -> &Vec<crate::extensions::Place>,
) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    for product in crate::extensions::declared(Some(h)) {
        for place in which(&product) {
            found.push((h.join(&place.under), place.glob.clone()));
        }
    }
    found
}

/// `Path.glob` for the only patterns needed here: literal components and `*`.
///
/// `*` does NOT match names beginning with a dot, as in `pathlib` — which is
/// why `.claude-plugin/` never shows up among the results and has to be looked
/// up separately.
pub fn glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut current = vec![root.to_path_buf()];
    let parts: Vec<&str> = pattern.split('/').collect();
    for (depth, part) in parts.iter().enumerate() {
        let last = depth + 1 == parts.len();
        let mut next = Vec::new();
        for dir in &current {
            // The star may carry a suffix (`*.md`), and then it hooks on by
            // prefix and tail: those are the only two forms the patterns here
            // use.
            if let Some((head, tail)) = part.split_once('*') {
                // The order is `readdir`'s, the same one Python's `os.scandir`
                // starts from: two reads of one directory give the same
                // sequence, and on that hangs which of two same-named skills
                // wins its description.
                let Ok(entries) = fs::read_dir(dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.')
                        || !name.starts_with(head)
                        || !name.ends_with(tail)
                        || name.len() < head.len() + tail.len()
                    {
                        continue;
                    }
                    let path = entry.path();
                    if last || path.is_dir() {
                        next.push(path);
                    }
                }
            } else {
                let path = dir.join(part);
                if path.exists() {
                    next.push(path);
                }
            }
        }
        current = next;
    }
    current
}

/// Where a skill comes from, and therefore **who may switch it off**. A plugin
/// sits in the cache and `enabledPlugins` decides whether it is on; a collection
/// installed as a folder is not in that list, so asking whether it is enabled
/// does not apply — the "no" that came out was false, not negative. The
/// difference used to be covered by `plugin.contains("mattpocock")`, the name of
/// the one collection somebody happened to have at hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Directly under `.claude/skills/`: always loaded.
    Home,
    /// In the plugin cache: `enabledPlugins` governs it.
    Plugin(String),
    /// A folder with a `skills/` of its own: no list governs it.
    Collection(String),
}

impl Origin {
    /// The prefix the skill has to be invoked with: `name:`, or nothing.
    pub fn prefix(&self) -> String {
        match self {
            Origin::Home => String::new(),
            Origin::Plugin(name) | Origin::Collection(name) => format!("{name}:"),
        }
    }
}

pub fn origin(path: &Path) -> Origin {
    let parts: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();

    if let Some(i) = parts.iter().position(|p| p == "cache") {
        // cache/<marketplace>/<plugin>/…
        return match parts.get(i + 2) {
            Some(name) => Origin::Plugin(name.clone()),
            None => Origin::Home,
        };
    }

    // `.claude/skills/<collection>/skills/…`: the second `skills` is what tells
    // a collection from a loose skill, and its name is not needed to see it.
    if let Some(i) = parts.iter().position(|p| p == "skills") {
        if parts.get(i + 2).map(String::as_str) == Some("skills") {
            if let Some(name) = parts.get(i + 1) {
                return Origin::Collection(name.clone());
            }
        }
    }

    Origin::Home
}

/// The prefix the skill has to be invoked with: `name:`, or nothing.
pub fn prefix(path: &Path) -> String {
    origin(path).prefix()
}

/// Which plugins are switched on. A disabled one offers no skills.
///
/// The list lives in `settings.json`, not in `~/.claude.json`, where the key
/// exists but is `null`: reading the wrong place raises no error, it yields
/// zero enabled plugins and hence a catalogue that stays silent about plugins.
pub fn enabled_plugins(h: &Path) -> BTreeSet<String> {
    let declared: Vec<PathBuf> = crate::extensions::declared(Some(h))
        .iter()
        .flat_map(|product| product.settings.clone())
        .map(|rest| h.join(rest))
        .collect();
    for path in declared {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(map) = value.get("enabledPlugins").and_then(|v| v.as_object()) else {
            continue;
        };
        if map.is_empty() {
            continue;
        }
        return map
            .iter()
            .filter(|(_, v)| truthy(v))
            .map(|(k, _)| k.split('@').next().unwrap_or(k).to_string())
            .collect();
    }
    BTreeSet::new()
}

/// Python's `bool(v)`: true also for a number that is not zero and a string
/// that is not empty, not only for `true`.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// The manifest's own path inside a plugin, declared rather than written here.
/// Only what ships is read: a manifest name is a shape of the product, not of
/// the machine.
fn manifest_name() -> String {
    crate::extensions::declared(None)
        .into_iter()
        .map(|product| product.plugin_manifest)
        .find(|declared| !declared.is_empty())
        .unwrap_or_default()
}

/// The names declared in the `plugin.json` that governs this skill.
///
/// Skills a plugin does not load stay on disk: one may declare 25 and keep 35
/// folders. `None` means "no filter" — the whole-folder case (`["./skills/"]`),
/// which read as a name would silently drop every skill of the plugin. The
/// manifest is found by walking up: its distance from the skill varies.
pub fn manifest(from: &Path) -> Option<BTreeSet<String>> {
    let mut cur = from.to_path_buf();
    for _ in 0..5 {
        cur = cur.parent()?.to_path_buf();
        let p = cur.join(&manifest_name());
        if !p.exists() {
            continue;
        }
        let entries = fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("skills").cloned());
        let Some(Value::Array(items)) = entries else {
            return None;
        };
        if items.is_empty() {
            return None;
        }
        // "The whole folder" is recognised by the SHAPE of the path — a single
        // hop, like `./skills/` — not by the last word: a skill may itself be
        // named something that ends in `-skills`.
        let segments = |v: &str| {
            v.trim_matches(|c| c == '.' || c == '/')
                .split('/')
                .filter(|x| !x.is_empty())
                .count()
        };
        let declared: Vec<&str> = items.iter().filter_map(|v| v.as_str()).collect();
        if declared.len() != items.len() {
            // `Path(v)` raises when an entry is not text: as above, the hook
            // dies in silence — but the exception lands inside `_scandisci`,
            // which `catalogo()`'s `try` already wraps, so it means "empty
            // catalogue". `None` means "no filter": the opposite outcome, the
            // Python drops every skill and this drops none. A real divergence.
            return None;
        }
        if declared.iter().any(|v| segments(v) <= 1) {
            return None;
        }
        return Some(
            declared
                .iter()
                .map(|v| {
                    Path::new(v)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .collect(),
        );
    }
    None
}

/// (name, description) from the frontmatter, or nothing if it is not invocable.
pub fn frontmatter(path: &Path) -> Option<(String, String)> {
    let matter = matter_of(path)?;
    let name = Regex::new(r"(?m)^name:\s*(\S+)")
        .ok()?
        .captures(&matter)?
        .get(1)?
        .as_str()
        .trim()
        .to_string();
    Some((name, description(&matter, false)))
}

/// (name, description) of a command in `commands/`.
///
/// It does not reuse `frontmatter`, which demands a `name` field a command does
/// not have — a command is named after its file. Without commands the catalogue
/// said `handoff` did not exist, and this hook stayed silent about the very
/// advice that was needed most: measured, on the real disk, not supposed.
pub fn command(path: &Path) -> Option<(String, String)> {
    let matter = matter_of(path)?;
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    Some((stem, description(&matter, true)))
}

/// The plugin folders Claude Code actually loads, one per plugin. WALKING THE
/// CACHE IS NOT ENOUGH: **all** versions ever downloaded stay under
/// `plugins/cache/`, and the first count here found 756 agents, 7 of them real
/// — `pr-review-toolkit` alone kept dozens. Only `installed_plugins.json` knows
/// the one in use: on disk the copies are indistinguishable. Missing or
/// unreadable gives an empty list — "I don't know", never "nothing installed".
pub fn installed_paths(h: &Path) -> BTreeSet<PathBuf> {
    let Some(rest) = crate::extensions::declared(Some(h))
        .into_iter()
        .map(|product| product.installed_plugins)
        .find(|declared| !declared.is_empty())
    else {
        return BTreeSet::new();
    };
    let path = h.join(rest);
    let Ok(text) = fs::read_to_string(&path) else {
        return BTreeSet::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return BTreeSet::new();
    };
    let Some(plugins) = value.get("plugins").and_then(|v| v.as_object()) else {
        return BTreeSet::new();
    };
    plugins
        .values()
        .filter_map(|v| v.as_array())
        .flatten()
        .filter_map(|entry| entry.get("installPath").and_then(|v| v.as_str()))
        .map(PathBuf::from)
        .collect()
}

/// The raw frontmatter, and whether the model may invoke what it describes.
///
/// THE DIFFERENCE WITH `matter_of` IS THE POINT. The nudge has to stay silent
/// about a skill the model cannot invoke, so `matter_of` drops it. The
/// inventory has to show it: `/learn` and `/work-loop-headless` carry
/// `disable-model-invocation: true`, exist, and a person invokes them by hand.
pub fn matter_and_invocability(path: &Path) -> Option<(String, bool)> {
    let raw = fs::read(path).ok()?;
    let text: String = String::from_utf8_lossy(&raw).chars().take(4000).collect();
    if !text.starts_with("---") {
        return None;
    }
    let end = text[3..].find("\n---").map(|i| i + 3);
    let matter = match end {
        Some(i) if i > 0 => text[3..i].to_string(),
        _ => text[3..].to_string(),
    };
    let by_model = !matter.contains("disable-model-invocation: true");
    Some((matter, by_model))
}

/// The raw frontmatter, with the same cuts as the original.
pub fn matter_of(path: &Path) -> Option<String> {
    let raw = fs::read(path).ok()?;
    // `errors='replace'` and then `[:4000]`: **characters**, not bytes.
    let text: String = String::from_utf8_lossy(&raw).chars().take(4000).collect();
    if !text.starts_with("---") {
        return None;
    }
    let end = text[3..].find("\n---").map(|i| i + 3);
    // `text.find('\n---', 3)` returns -1 when there is none, and Python's
    // `text[3:-1]` would be everything but the last character; the original
    // compares `end_ > 0`, so the "not found" case takes `text[3:4000]`.
    let matter = match end {
        Some(i) if i > 0 => text[3..i].to_string(),
        _ => text[3..].to_string(),
    };
    if matter.contains("disable-model-invocation: true") {
        return None;
    }
    Some(matter)
}

/// `^description:\s*(.+?)(?=\n\w+:|\Z)` with `re.M | re.S`, rewritten by hand.
///
/// LOOKAHEAD DOES NOT EXIST in the `regex` crate, and consuming the next
/// character is no option here as it is in the skill patterns: the captured
/// group **is** the value, so eating the following field's newline would dirty
/// it. So the semantics are reproduced directly, in the loop below.
pub fn description(matter: &str, hyphen_in_field: bool) -> String {
    let Some(start) = field_start(matter, "description:") else {
        return String::new();
    };
    let rest = &matter[start..];
    let value = rest.trim_start_matches([' ', '\t', '\r', '\n', '\u{b}', '\u{c}']);
    // WHAT THE LOOP COMPUTES: the group is the shortest string that is not
    // empty, after which there begins either the end of the text, or a newline
    // followed by a field name and a colon. The group is `(.+?)`, hence at
    // least one character, so the hunt for the terminator starts after the
    // first. `commands/` uses `\w[\w-]*` instead of `\w+`, and the difference
    // is real: a hyphenated field (`argument-hint:`) closes it there.
    let bytes: Vec<(usize, char)> = value.char_indices().collect();
    let mut cut = value.len();
    for (i, c) in bytes.iter().skip(1) {
        if *c != '\n' {
            continue;
        }
        let after = &value[i + 1..];
        let mut chars = after.chars();
        let first = chars.next();
        let is_word = |c: Option<char>| c.map(|c| c.is_alphanumeric() || c == '_') == Some(true);
        if !is_word(first) {
            continue;
        }
        let mut end = 1;
        for c in chars {
            if c.is_alphanumeric() || c == '_' || (hyphen_in_field && c == '-') {
                end += c.len_utf8();
            } else {
                break;
            }
        }
        if after[end..].starts_with(':') {
            cut = *i;
            break;
        }
    }
    // `' '.join(x.split())` and then `.strip('>|- ')`.
    let collapsed = value[..cut]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    collapsed
        .trim_matches(|c| c == '>' || c == '|' || c == '-' || c == ' ')
        .to_string()
}

/// The start of a field's value at the beginning of a line (`(?m)^field:`).
pub fn field_start(matter: &str, field: &str) -> Option<usize> {
    let mut offset = 0;
    for line in matter.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix(field) {
            return Some(offset + line.len() - rest.len());
        }
        offset += line.len();
    }
    None
}
