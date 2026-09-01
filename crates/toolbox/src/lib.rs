//! What can be used on this machine: AI command lines, MCP servers, and any
//! other tool a flow might invoke.
//!
//! THE CONSTRAINT THAT DECIDES THE WHOLE PROJECT: the list of what to look for
//! is data, not code. No tool name appears in this crate — no `if id ==
//! "docker"`, no path belonging to this machine.

pub mod action;
pub mod descriptor;
pub mod needs;
pub mod probe;
pub mod resolver;
pub mod session;

pub use action::{register_default, DetectToolsAction, DETECT_TOOLS_ACTION};
pub use needs::{register_needs, Need, ToolNeedsAction, TOOL_NEEDS_ACTION};
pub use descriptor::{
    builtin_catalog, Capability, CapabilityForm, CapabilityState, Catalog, Contradiction,
    Descriptor, Loaded, Problem, Source, ASK_WITHOUT_INTERACTION, BUILTIN_CATALOGS,
};
pub use probe::{Look, Machine, VersionReading};
pub use resolver::Tools;
pub use session::{SessionAbilities, SessionAbility};

// THE OUTCOME TYPES ARE ALSO READ AS INPUT: a step that receives the detection
// done by the step before it deserializes it like any other data. Without
// `Deserialize` that step would have to rummage through a `serde_json::Value`
// with pointers, and the link between the two steps would become a convention
// between strings instead of a type.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Here, not here, or could not be looked at — always with the reason.
///
/// **THE TWO ANSWERS THAT MUST NOT BE MIXED.** "Not installed" and "I could not
/// check" are different things, and an inventory that mixes them is useless: the
/// reader installs a second copy of what they already had, or gives up a tool
/// that was there. Every "don't know" here carries the measured reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "lowercase")]
pub enum Presence {
    Present(String),
    Absent(String),
    Undetermined(String),
}

impl Presence {
    pub fn is_present(&self) -> bool {
        matches!(self, Presence::Present(_))
    }
}

/// A place where a tool's configuration lives, and whether it is there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigPath {
    pub path: String,
    pub presence: Presence,
}

/// Something found (or not found), with everything needed to trace the why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// What the thing is called: the descriptor's `id`, or — for a descriptor
    /// that discovers several entries from a file — the name of the entry found.
    pub name: String,
    pub family: String,
    pub label: String,
    /// Which descriptor recognised it, and where that descriptor came from:
    /// without these two lines "why is this in the list?" has no answer, and a
    /// list that cannot be held to account does not get corrected.
    pub descriptor_id: String,
    pub descriptor_source: String,
    pub presence: Presence,
    /// Where its executable is, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    pub version: VersionReading,
    pub config: Vec<ConfigPath>,
    /// The descriptor's note, for the reader.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// The outcome of a detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    /// The descriptors that could not be read. They live here and not among the
    /// entries: a bad line in the list of what to look for is not a missing
    /// tool, it is a fault of whoever wrote the list.
    pub problems: Vec<Problem>,
    /// The directories an executable was looked for in, spelled out: a list that
    /// does not say where it looked cannot be contradicted.
    pub looked_in: Vec<String>,
}

impl Report {
    pub fn of_family<'a>(&'a self, family: &str) -> Vec<&'a Finding> {
        self.findings
            .iter()
            .filter(|f| f.family == family)
            .collect()
    }

    pub fn present(&self) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.presence.is_present())
            .collect()
    }

    /// The families seen, in order: whoever shows the list must not have to know
    /// them in advance, or a new family would need a recompile of the display.
    pub fn families(&self) -> Vec<String> {
        let mut out: Vec<String> = self.findings.iter().map(|f| f.family.clone()).collect();
        out.sort();
        out.dedup();
        out
    }
}

/// Sailor's home for a described machine.
///
/// **ONE RULE, AND IT LIVES IN THE LEDGER.** The hand-written copy that sat here
/// ignored `XDG_CONFIG_HOME` and fell back to `~/.sailor`; `ledger::sailor_home`,
/// which decides where store and price list live, falls back to
/// `~/.config/sailor`. Descriptors in one home, prices in the other, unnoticed.
pub fn sailor_home_for(machine: &Machine) -> PathBuf {
    ledger::sailor_home_in(
        machine.env.get("SAILOR_HOME").map(PathBuf::from),
        machine.env.get("XDG_CONFIG_HOME").map(PathBuf::from),
        machine.home.clone(),
    )
}

/// The sources descriptors are taken from on this machine.
///
/// In the order they win: the shipped ones first — embedded in the binary so
/// there is no installation path to guess, rewritten or switched off **by `id`**
/// from a user file with no recompile — then the user's. And
/// `SAILOR_TOOL_DESCRIPTORS` (paths split by `:`, files or dirs) adds more.
pub fn default_sources(machine: &Machine) -> Vec<Source> {
    let mut out = vec![Source::Builtin];
    out.push(Source::Dir(sailor_home_for(machine).join("tools.d")));
    if let Some(extra) = machine.env.get("SAILOR_TOOL_DESCRIPTORS") {
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

/// The detection: runs every live descriptor and collects what it answered.
///
/// The code knows three forms of check and no more: find an executable among the
/// path directories, see whether a file is there, read the keys of a config
/// file. Which executables, which files and which keys is said by the
/// descriptors, and one more is added by writing a line of JSON.
pub fn detect(catalog: &Catalog, machine: &Machine) -> Report {
    let mut findings = Vec::new();
    for loaded in catalog.live() {
        match &loaded.descriptor.enumerate {
            Some(enumerate) => findings.extend(discovered(loaded, enumerate, machine)),
            None => findings.push(probed(loaded, machine)),
        }
    }
    findings.sort_by(|a, b| (&a.family, &a.name).cmp(&(&b.family, &b.name)));
    Report {
        findings,
        problems: catalog.problems.clone(),
        looked_in: machine
            .path_dirs
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
    }
}

/// Looks for **one** tool, by identifier.
///
/// Not `detect` filtered afterwards: that would ask the version of every
/// installed program — dozens of processes — each time a flow step starts; here
/// one line of the list runs. Only for a descriptor with `detect`: one that
/// *discovers* entries has no executable, told apart by looking at `detect`.
pub fn probe_one(loaded: &Loaded, machine: &Machine) -> Finding {
    probed(loaded, machine)
}

/// A descriptor that says "this thing is either here or not".
fn probed(loaded: &Loaded, machine: &Machine) -> Finding {
    let descriptor = &loaded.descriptor;
    let probes = descriptor
        .detect
        .as_ref()
        .map(|p| p.as_slice())
        .unwrap_or(&[]);
    let mut executable: Option<PathBuf> = None;
    let mut presence: Option<Presence> = None;
    let mut blocked: Vec<String> = Vec::new();
    let mut searched: Vec<String> = Vec::new();
    for single in probes {
        if let Some(command) = &single.command {
            searched.push(format!("the executable `{command}`"));
            match probe::look_up(command, machine) {
                Look::Found(path) => {
                    presence = Some(Presence::Present(format!(
                        "found `{command}` in {}",
                        path.to_string_lossy()
                    )));
                    executable = Some(path);
                    break;
                }
                Look::Missing => {}
                Look::Blocked(reason) => blocked.push(reason),
            }
        }
        if let Some(raw) = &single.path {
            searched.push(format!("the path `{raw}`"));
            for candidate in machine.resolve(raw) {
                match probe::look_at(&candidate) {
                    Look::Found(path) => {
                        presence =
                            Some(Presence::Present(format!("found {}", path.to_string_lossy())));
                        break;
                    }
                    Look::Missing => {}
                    Look::Blocked(reason) => blocked.push(reason),
                }
            }
            if presence.is_some() {
                break;
            }
        }
    }
    let presence = presence.unwrap_or_else(|| {
        if blocked.is_empty() {
            Presence::Absent(format!("looked for {}: nothing", searched.join(", ")))
        } else {
            // "ABSENT" IS NOT SAID WHERE NOTHING COULD BE LOOKED AT, which is
            // why this arm exists separately from the other.
            Presence::Undetermined(blocked.join("; "))
        }
    });
    let version = match (&presence, &executable, &descriptor.version) {
        (Presence::Present(_), Some(bin), Some(spec)) if machine.version_probes => {
            probe::read_version(
                bin,
                &spec.args,
                &spec.must_contain,
                Duration::from_secs(spec.timeout_secs),
            )
        }
        (Presence::Present(_), Some(_), Some(_)) => {
            VersionReading::NotAsked("executions are switched off".to_string())
        }
        (Presence::Present(_), _, None) => {
            VersionReading::NotAsked("the descriptor does not say how to ask for it".to_string())
        }
        (Presence::Present(_), None, Some(_)) => {
            VersionReading::NotAsked("there is no executable to ask".to_string())
        }
        _ => VersionReading::NotAsked("it is not here".to_string()),
    };
    Finding {
        name: descriptor.id.clone(),
        family: descriptor.family.clone(),
        label: label_of(descriptor),
        descriptor_id: descriptor.id.clone(),
        descriptor_source: loaded.source.clone(),
        presence,
        executable: executable.map(|p| p.to_string_lossy().into_owned()),
        version,
        config: config_of(&descriptor.config, machine),
        note: descriptor.note.clone(),
    }
}

/// A descriptor that discovers its entries by reading a configuration file.
fn discovered(
    loaded: &Loaded,
    enumerate: &descriptor::Enumerate,
    machine: &Machine,
) -> Vec<Finding> {
    let descriptor = &loaded.descriptor;
    let mut out: Vec<Finding> = Vec::new();
    let mut blocked: Vec<String> = Vec::new();
    // Where it looked, spelled out: this is what ends up in the reason for a
    // "not here", and an empty list that does not say where it searched cannot
    // be contradicted.
    let mut looked: Vec<String> = Vec::new();
    // True once at least one place has really been read. Without this
    // distinction "no entries" and "I looked at nothing" are written the same.
    let mut read_any = false;

    if let Some(json_keys) = &enumerate.json_keys {
        looked.push(format!(
            "the keys under {} in {}",
            json_keys.pointer.join("/"),
            json_keys.files.join(", ")
        ));
        for raw in &json_keys.files {
            for path in machine.resolve(raw) {
                let text = match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        blocked.push(format!("{}: {error}", path.to_string_lossy()));
                        continue;
                    }
                };
                let value: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(error) => {
                        // AN UNREADABLE FILE IS NOT A FILE WITHOUT ENTRIES.
                        // Counting it as zero would silently erase everything it
                        // declares.
                        blocked.push(format!(
                            "{} is not valid JSON: {error}",
                            path.to_string_lossy()
                        ));
                        continue;
                    }
                };
                read_any = true;
                for name in probe::json_keys(&value, &json_keys.pointer) {
                    let evidence = format!(
                        "declared in {} under {}",
                        path.to_string_lossy(),
                        json_keys.pointer.join("/")
                    );
                    out.push(Finding {
                        name,
                        family: descriptor.family.clone(),
                        label: label_of(descriptor),
                        descriptor_id: descriptor.id.clone(),
                        descriptor_source: loaded.source.clone(),
                        presence: Presence::Present(evidence),
                        executable: None,
                        version: VersionReading::NotAsked(
                            "a configuration entry, not a binary".to_string(),
                        ),
                        config: vec![ConfigPath {
                            path: path.to_string_lossy().into_owned(),
                            presence: Presence::Present("read".to_string()),
                        }],
                        note: descriptor.note.clone(),
                    });
                }
            }
        }
    }

    if let Some(patterns) = &enumerate.paths {
        looked.push(format!("the files matching {}", patterns.join(", ")));
        for raw in patterns {
            let found = machine.resolve(raw);
            if raw.contains('*') {
                // A DIRECTORY THAT CANNOT BE READ IS NOT AN EMPTY DIRECTORY, and
                // `glob` does not tell the two apart: it swallows the `read_dir`
                // error and returns zero paths either way. Here the root of the
                // pattern is asked directly, because "you have nothing to
                // migrate" told to someone with twenty services is the worst lie
                // this list can tell.
                match read_dir_state(&machine.expand(raw)) {
                    DirState::Readable => read_any = true,
                    DirState::Missing(where_) => {
                        looked.push(format!("{where_} does not exist"));
                    }
                    DirState::Blocked(reason) => blocked.push(reason),
                }
            }
            for path in found {
                match probe::look_at(&path) {
                    Look::Found(path) => {
                        read_any = true;
                        let shown = path.to_string_lossy().into_owned();
                        out.push(Finding {
                            // THE NAME IS THE WHOLE PATH. Two files with the same
                            // name in two directories are two different
                            // automations, and the merge below would count them
                            // as one.
                            name: shown.clone(),
                            family: descriptor.family.clone(),
                            label: label_of(descriptor),
                            descriptor_id: descriptor.id.clone(),
                            descriptor_source: loaded.source.clone(),
                            presence: Presence::Present(format!("the file is there: {shown}")),
                            executable: None,
                            version: VersionReading::NotAsked(
                                "a file, not a binary to interrogate".to_string(),
                            ),
                            config: vec![ConfigPath {
                                path: shown,
                                presence: Presence::Present("here".to_string()),
                            }],
                            note: descriptor.note.clone(),
                        });
                    }
                    Look::Missing => read_any = true,
                    Look::Blocked(reason) => {
                        blocked.push(format!("{}: {reason}", path.to_string_lossy()))
                    }
                }
            }
        }
    }

    // THE SAME SERVER DECLARED IN TWO PLACES IS ONE SERVER, but both places must
    // be named: whoever has to change its configuration needs to know which file
    // to touch.
    out.sort_by(|a, b| a.name.cmp(&b.name));
    let mut merged: Vec<Finding> = Vec::new();
    for finding in out {
        match merged.iter_mut().find(|f| f.name == finding.name) {
            Some(existing) => existing.config.extend(finding.config),
            None => merged.push(finding),
        }
    }
    if merged.is_empty() {
        // No entries: and now the difference that counts. If at least one place
        // was read, "there are none" is a measurement; if none was, nothing was
        // looked at and saying so would be inventing.
        let presence = if !blocked.is_empty() {
            Presence::Undetermined(blocked.join("; "))
        } else if read_any {
            Presence::Absent(format!("no entries in: {}", looked.join("; ")))
        } else {
            Presence::Absent(format!(
                "there was nothing to read in: {}",
                looked.join("; ")
            ))
        };
        merged.push(Finding {
            name: descriptor.id.clone(),
            family: descriptor.family.clone(),
            label: label_of(descriptor),
            descriptor_id: descriptor.id.clone(),
            descriptor_source: loaded.source.clone(),
            presence,
            executable: None,
            version: VersionReading::NotAsked("it is not here".to_string()),
            config: config_of(&descriptor.config, machine),
            note: descriptor.note.clone(),
        });
    } else if !blocked.is_empty() {
        // Something was found and something could not be looked at: the list is
        // partial, and that must be said instead of letting it pass for whole.
        merged.push(Finding {
            name: format!("{} (partial)", descriptor.id),
            family: descriptor.family.clone(),
            label: label_of(descriptor),
            descriptor_id: descriptor.id.clone(),
            descriptor_source: loaded.source.clone(),
            presence: Presence::Undetermined(blocked.join("; ")),
            executable: None,
            version: VersionReading::NotAsked("not a binary".to_string()),
            config: Vec::new(),
            note: descriptor.note.clone(),
        });
    }
    merged
}

/// What the directory a `*` pattern starts from answered.
enum DirState {
    Readable,
    Missing(String),
    Blocked(String),
}

/// The root of a pattern is the part before the first component holding a `*`;
/// it is the same split [`Machine::resolve`] makes, and it serves here to ask
/// the directory what it answered instead of inferring it from zero results.
fn read_dir_state(expanded: &str) -> DirState {
    let root: String = expanded
        .split('/')
        .take_while(|part| !part.contains('*'))
        .collect::<Vec<_>>()
        .join("/");
    let root = if root.is_empty() { "." } else { root.as_str() };
    match std::fs::read_dir(root) {
        Ok(_) => DirState::Readable,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            DirState::Missing(root.to_string())
        }
        Err(error) => DirState::Blocked(format!("{root}: {error}")),
    }
}

fn label_of(descriptor: &Descriptor) -> String {
    if descriptor.label.is_empty() {
        descriptor.id.clone()
    } else {
        descriptor.label.clone()
    }
}

fn config_of(raw: &[String], machine: &Machine) -> Vec<ConfigPath> {
    let mut out = Vec::new();
    for pattern in raw {
        let resolved = machine.resolve(pattern);
        if resolved.is_empty() {
            out.push(ConfigPath {
                path: machine.expand(pattern),
                presence: Presence::Absent("no path matched".to_string()),
            });
            continue;
        }
        for path in resolved {
            let presence = match probe::look_at(&path) {
                Look::Found(_) => Presence::Present("here".to_string()),
                Look::Missing => Presence::Absent("not here".to_string()),
                Look::Blocked(reason) => Presence::Undetermined(reason),
            };
            out.push(ConfigPath {
                path: path.to_string_lossy().into_owned(),
                presence,
            });
        }
    }
    out
}
