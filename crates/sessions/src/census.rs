//! What is on the machine right now: which terminals exist, who lives inside
//! them, and where they were opened from.
//!
//! **THE DISTINCTION THIS MODULE EXISTS TO MAKE.** Inside a restricted sandbox
//! `ps` is denied, and the denial goes silent the moment its output is piped:
//! `ps -e | wc -l` answers `0` with **exit status 0** and no error at all.

//! Whoever gets back an empty list has no way of telling a deserted machine
//! from one they were not allowed to look at, and the two lead to opposite
//! decisions. So the result is not a vector: it is [`Census`], which has three
//! states and lets no one of them be ignored.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

/// Why we were not allowed to look.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refusal {
    /// Who the question was put to.
    pub tool: String,
    /// What it answered, in the words it used: a denial recognised by its own
    /// text can be reported, a deduced one cannot.
    pub reason: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.tool, self.reason)
    }
}

/// A process living on a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inhabitant {
    pub pid: u32,
    pub parent_pid: u32,
    pub tty: String,
    /// How long it has been alive, as `ps` writes it: not converted, because a
    /// rewritten format is one that can drift from its source.
    pub uptime: String,
    pub command: String,
    /// `None` means **we do not know**, not "none".
    pub working_directory: Option<String>,
}

/// A terminal, with everything running inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Terminal {
    pub tty: String,
    /// Who drew the window, found by walking up the parent chain.
    /// **A label**: no decision reads it.
    pub ancestor: Option<String>,
    pub inhabitants: Vec<Inhabitant>,
}

/// What is on the machine, in three states that do not blur together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Census {
    /// At least one terminal. **Never empty**: [`Census::of`] guarantees it,
    /// and a test holds it there.
    Terminals(Vec<Terminal>),
    /// We looked, and no process has a terminal.
    NoTerminal,
    /// We did not look.
    Refused(Refusal),
}

impl Census {
    /// The terminals we saw, or nothing when we did not see them. Calling this
    /// instead of matching the variants **throws the distinction away**, which
    /// is why it is named so: asking for "the ones I saw" says by itself that
    /// there is a case where none were seen for another reason.
    pub fn seen(&self) -> &[Terminal] {
        match self {
            Self::Terminals(terminals) => terminals,
            Self::NoTerminal | Self::Refused(_) => &[],
        }
    }

    /// A tty's ancestor, if the census has one to give.
    pub fn ancestor_of(&self, tty: &str) -> Option<&str> {
        self.seen()
            .iter()
            .find(|terminal| terminal.tty == tty)
            .and_then(|terminal| terminal.ancestor.as_deref())
    }
}

/// Who answers questions about the machine. **A trait because the denial has to
/// be provable**: inside the sandbox the tests run in `ps` is really denied, so
/// a test that invoked it would measure the sandbox and not the code. A fake
/// answering "not permitted" walks the path that denial takes, a fake answering
/// a real table walks the reading path, and neither test depends on where it
/// runs.
pub trait Machine {
    /// `pid ppid tty etime comm`, one line per process, no header.
    fn process_table(&self) -> Result<String, Refusal>;
    /// A pid's working directory. `None` is "I do not know".
    fn working_directory(&self, pid: u32) -> Option<String>;
    /// The pid of whoever is asking: the canary.
    fn own_pid(&self) -> u32;
}

/// One row of the process table, before we know whether it has a terminal.
#[derive(Debug, Clone)]
struct Row {
    pid: u32,
    parent_pid: u32,
    tty: String,
    uptime: String,
    command: String,
}

/// The tty `ps` writes when a process has none.
const NO_TTY: &str = "??";

/// Reads the table and splits it into rows. Whatever will not parse is skipped:
/// a header or a truncated line must not bring the census down.
fn parse_table(text: &str) -> Vec<Row> {
    text.lines().filter_map(parse_row).collect()
}

fn parse_row(line: &str) -> Option<Row> {
    let mut fields = line.split_whitespace();
    let pid = fields.next()?.parse().ok()?;
    let parent_pid = fields.next()?.parse().ok()?;
    let tty = fields.next()?.to_owned();
    let uptime = fields.next()?.to_owned();
    // A command can contain spaces (`npm exec something`): it is the whole rest
    // of the line, not the next field.
    let command = fields.collect::<Vec<_>>().join(" ");
    if command.is_empty() {
        return None;
    }
    Some(Row {
        pid,
        parent_pid,
        tty,
        uptime,
        command,
    })
}

/// The label of a command.
///
/// A convention of the system, not knowledge of a product: on macOS an
/// application is a `<Name>.app` directory whose binary has a long path that
/// tells a reader nothing. The first `.app` in the path is the host
/// application; inner wrappers have their own further down. No name goes here.
pub fn label_for(command: &str) -> String {
    if let Some((head, _)) = command.split_once(".app/") {
        let name = head.rsplit('/').next().unwrap_or(head);
        if !name.is_empty() {
            return name.to_owned();
        }
    }
    command.rsplit('/').next().unwrap_or(command).to_owned()
}

/// A process's ancestor: walk up while there is a parent to walk to. Also
/// returns **how many steps** were climbed, and that number is used.
///
/// The walk stops below `launchd` (pid 1), because the ancestor of everything
/// would always be launchd, and a label shared by all labels nothing.
fn ancestry_of(start: u32, table: &BTreeMap<u32, Row>) -> Option<(usize, String)> {
    let mut current = start;
    let mut steps = 0;
    let mut walked = BTreeSet::new();
    walked.insert(current);
    while let Some(row) = table.get(&current) {
        let parent = row.parent_pid;
        if parent <= 1 || !table.contains_key(&parent) || !walked.insert(parent) {
            break;
        }
        current = parent;
        steps += 1;
    }
    table
        .get(&current)
        .map(|row| (steps, label_for(&row.command)))
}

/// A terminal's ancestor: **the longest chain among its inhabitants**, not the
/// first one found. A walk also stops when the parent is missing from the table
/// — a reparented process, or one that has just died — and then the process
/// becomes its own ancestor: a terminal opened by an application came out
/// labelled `caffeinate`, the group's first pid, which had climbed nothing.
/// Whoever climbed highest saw most, and carries the true label.
fn ancestor_of_terminal(rows: &[&Row], table: &BTreeMap<u32, Row>) -> Option<String> {
    rows.iter()
        .filter_map(|row| ancestry_of(row.pid, table))
        .max_by_key(|(steps, _)| *steps)
        .map(|(_, found)| found)
}

impl Census {
    /// The census, taken on whichever machine answers.
    ///
    /// A denied `ps` can also exit clean with empty output, and then no error
    /// code betrays it. But whoever asks for the process table **is** a
    /// process: a table without the asker's own pid is not the machine. That
    /// canary is the only check that holds when a denial will not name itself.
    pub fn of(machine: &dyn Machine) -> Census {
        let text = match machine.process_table() {
            Ok(text) => text,
            Err(refusal) => return Census::Refused(refusal),
        };
        let rows = parse_table(&text);
        let own = machine.own_pid();
        if !rows.iter().any(|row| row.pid == own) {
            return Census::Refused(Refusal {
                tool: "ps".to_owned(),
                reason: format!(
                    "the process table does not contain the pid of the process that asked for \
                     it ({own}): {} rows read. A machine missing whoever queries it is not an \
                     empty machine, it is an answer we did not get",
                    rows.len()
                ),
            });
        }

        let table: BTreeMap<u32, Row> = rows.iter().map(|row| (row.pid, row.clone())).collect();
        let mut grouped: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
        for row in rows.iter().filter(|row| row.tty != NO_TTY) {
            grouped.entry(row.tty.clone()).or_default().push(row);
        }
        if grouped.is_empty() {
            return Census::NoTerminal;
        }

        let terminals = grouped
            .into_iter()
            .map(|(tty, rows)| Terminal {
                ancestor: ancestor_of_terminal(&rows, &table),
                inhabitants: rows
                    .into_iter()
                    .map(|row| Inhabitant {
                        pid: row.pid,
                        parent_pid: row.parent_pid,
                        tty: row.tty.clone(),
                        uptime: row.uptime.clone(),
                        command: row.command.clone(),
                        working_directory: machine.working_directory(row.pid),
                    })
                    .collect(),
                tty,
            })
            .collect();
        Census::Terminals(terminals)
    }
}

/// The real machine, asked with the tools every Unix has.
pub struct LocalMachine;

impl Machine for LocalMachine {
    fn process_table(&self) -> Result<String, Refusal> {
        read_from("ps", &["-e", "-o", "pid=,ppid=,tty=,etime=,comm="])
    }

    fn working_directory(&self, pid: u32) -> Option<String> {
        let text = read_from("lsof", &["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"]).ok()?;
        // `-Fn` writes one field per line, with the field letter in front:
        // `p<pid>`, `fcwd`, `n<path>`.
        text.lines()
            .find_map(|line| line.strip_prefix('n'))
            .map(str::to_owned)
    }

    fn own_pid(&self) -> u32 {
        std::process::id()
    }
}

/// **NO PIPES, EVER.** The output is captured directly, so a denial arrives
/// with its own text and its own exit code instead of vanishing into the next
/// command. `ps -e | wc -l` answers `0` with exit `0`; `Command::output`
/// answers with the error that is there.
fn read_from(tool: &str, args: &[&str]) -> Result<String, Refusal> {
    let output = Command::new(tool)
        .args(args)
        .output()
        .map_err(|error| Refusal {
            tool: tool.to_owned(),
            reason: format!("did not start: {error}"),
        })?;
    let complaint = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if !output.status.success() {
        return Err(Refusal {
            tool: tool.to_owned(),
            reason: format!(
                "exited with {}{}",
                output.status,
                if complaint.is_empty() {
                    String::new()
                } else {
                    format!(": {complaint}")
                }
            ),
        });
    }
    if !complaint.is_empty() {
        return Err(Refusal {
            tool: tool.to_owned(),
            reason: complaint,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
