//! The descriptor format and how it loads.
//!
//! A DESCRIPTOR IS DATA, NOT A CODE BRANCH. No tool name appears here: the code
//! knows how to run *a form* of check, never one check in particular. Adding a
//! command line — OpenRouter's CLI, tomorrow's — is writing a JSON object, not
//! recompiling.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The descriptors the product carries with it.
///
/// Embedded in the binary, not looked for in an install directory: a binary
/// copied elsewhere keeps answering, and there is no path to guess. They are
/// still data — switch one off by `id`, or rewrite it by `id`.
pub const BUILTIN: &str = include_str!("../descriptors/default.json");

/// The catalogs shipped with the product, by name.
///
/// **WHY MORE THAN ONE.** A descriptor answers "is this here?" and "what entries
/// does this file declare?" — good for far more than the tools a step invokes.
/// Migration to Sailor, finding the hooks and services a person already has, is
/// the same question on other paths; rewriting it would be a diverging copy.
pub const BUILTIN_CATALOGS: &[(&str, &str)] = &[
    ("tools", BUILTIN),
    (
        "automations",
        include_str!("../descriptors/automations.json"),
    ),
];

/// How a descriptor's provenance is spelled out for the reader.
///
/// Still in Italian because it is a literal other code compares:
/// `sailor/tests/system_flows.rs` asserts `"descriptor_source": "incorporato"`.
/// `models::pricing` and `terminal::routing` each declare a `BUILTIN_SOURCE` of
/// their own; nothing compares the three, and two of them do not even agree.
pub const BUILTIN_SOURCE: &str = "incorporato";

/// Where descriptors are taken from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The ones shipped with the product: the `tools` catalog.
    Builtin,
    /// Another shipped catalog, by name. **THEY STAY SEPARATE, AND THAT IS NOT
    /// FUSSINESS**: someone else's automation is not a tool a step can invoke —
    /// in one catalog its `id` would show up among the ones
    /// [`crate::Tools::resolve`] offers to whoever mistyped a name. A name no
    /// catalog carries becomes a problem: mistyping it in a flow would otherwise
    /// give an empty list, indistinguishable from "there is nothing here".
    BuiltinNamed(String),
    /// A single JSON file.
    File(PathBuf),
    /// Every `*.json` in a directory, in name order.
    Dir(PathBuf),
}

/// How the presence of a thing is checked.
///
/// Two shapes, and both are needed: a command line is recognised by a reachable
/// executable, while an MCP server often has no executable of its own — its host
/// starts it — and is recognised by the file that declares it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    /// The name of an executable to look for on the search path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// A path that must exist. Accepts `~/`, `$VAR` and `*`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// One or more probes. JSON with a single object is written without the square
/// brackets: whoever adds a tool in the simple case need not know the
/// complicated one.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Probes {
    One(Probe),
    Many(Vec<Probe>),
}

impl Probes {
    pub fn as_slice(&self) -> &[Probe] {
        match self {
            Probes::One(probe) => std::slice::from_ref(probe),
            Probes::Many(probes) => probes,
        }
    }
}

/// How the version is asked for: the arguments to pass the executable found.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VersionProbe {
    pub args: Vec<String>,
    /// The time limit. A binary that settles down to wait on its input must not
    /// be able to stop the detection of the others.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// The right line is the one containing this text.
    ///
    /// IT IS NEEDED, AND IT WAS SEEN ON THIS MACHINE: `ollama --version` prints a
    /// warning about an unreachable service first, and taking the first line
    /// recorded that warning as a version number. The remedy is in the data — the
    /// descriptor's author knows the shape — not in a branch for that binary.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub must_contain: String,
}

fn default_timeout() -> u64 {
    10
}

/// How a one-shot question is put to an engine, and how it says it cannot work.
///
/// **WHY IT LIVES IN THE DESCRIPTOR, NOT IN THE FLOW.** While `-p` for one and
/// `--mode plan --print` for another sit inside the steps, a flow is tied to the
/// engine it was written for, and "model independent" stays a phrase. Here it is
/// data: a new engine is a descriptor — no recompile, and no flow changed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Ask {
    /// The options that want one answer and not a conversation, **without** the
    /// text of the question.
    #[serde(default)]
    pub args: Vec<String>,
    /// Where the question's text goes: `stdin` or `last_arg`.
    #[serde(default)]
    pub prompt: PromptPlace,
    /// The options that must sit **immediately before the question**, after
    /// everything else — usage options included. Appending them slips them
    /// between the flag introducing the question and the question: `agy` answers
    /// `--print took "--output-format" as its prompt`. Reordering `args` does not
    /// cure it — the constraint is not inside one block. Nor does a global order:
    /// `codex` wants its `exec` subcommand first, `agy` wants `--print` last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args_before_prompt: Vec<String>,
    /// How this engine says it **cannot work** — quota spent, credentials
    /// missing — as opposed to saying the work was wrong. It is what lets a step
    /// with a chain of engines move on to the next; an engine that does not
    /// declare it fires no fallback, so things work worse but never in silence.
    /// And it is **the provider's own words**, not a general rule: "error" would
    /// match any failure and send a wrong brief down the chain to the end.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unusable_when: Vec<String>,
    /// The words among those that mean **the quota is spent**, as opposed to
    /// credentials missing: they make the call `quota_exhausted`, a thing that
    /// passes by itself. Empty means nobody told the two apart.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exhausted_when: Vec<String>,
    /// How long the engine is set aside after saying its quota is spent, so a
    /// chain does not knock again on a door known to be shut. Absent means it
    /// is tried again every time, as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_secs: Option<u64>,
    /// The words after which this engine only waits for a person — a browser
    /// to visit, a code to type. A step has nobody to send, so on seeing one
    /// the engine is stopped at once instead of being paid its wait, and what
    /// it had said is read as its refusal. Empty means it is waited for in full.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waits_for_a_person_when: Vec<String>,
    /// How this engine refuses the line **composed without the question**: the
    /// only harmless way to try a real command line, since no provider is called
    /// yet the same argument parsing runs. Data, because each refuses in its own
    /// words ("No prompt provided via stdin.", "flag needs an argument: -print")
    /// and the four refuse with **two exit codes only** — `agy`'s sound refusal
    /// and its malformed line both exit 2. Empty means "nobody looked".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refuses_without_prompt: Vec<String>,
    /// Measured: the line composed without a question exits quietly, with
    /// nothing on stdout, instead of refusing in words.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub silent_without_prompt: bool,
    /// The fields not understood, for the same reason as `Descriptor::extra`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// How an engine is asked **whether the home it starts from is authenticated**.
///
/// **THE DISK IS NOT INSPECTED**: hunting for `auth.json` would be a second copy
/// of the truth. The engine is what can answer, so only **how it is asked** and
/// **the words it answers with** are declared. `flow check`'s dry run cannot do
/// it: `codex exec < /dev/null` says the same and exits 1, empty home or full.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LoginStatus {
    /// The options, or subcommand, the question is asked with: `["login",
    /// "status"]` for `codex`, `["auth", "status"]` for `claude`.
    ///
    /// **IT MUST BE A QUESTION, NEVER A GESTURE**, because the check really runs
    /// it: `codex login` and `claude auth login` open a browser and change the
    /// machine's state, so a routine check would re-authenticate its launcher.
    #[serde(default)]
    pub args: Vec<String>,
    /// Where the answer sits inside what the engine said. It is `usage`'s
    /// pointer, not a second mechanism: in prose — `codex` answers "Logged in
    /// using ChatGPT" — there is nothing to point at and the subject is all it
    /// said, while `claude` puts the answer in a boolean field, `{"loggedIn":
    /// true}`, reached by `["loggedIn"]`. Absent means "the subject is the whole
    /// output", the commonest case, not "do not look".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Where>,
    /// The words this engine declares it **is** authenticated with.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logged_in_when: Vec<String>,
    /// The words it declares it is **not**. Both lists are needed: an engine
    /// able to recognise only the yes would call every no "not understood". And
    /// the no is read before the yes, because "Not logged in" contains "logged
    /// in" — the way of saying no is nearly always the way of saying yes with a
    /// negation in front. `judge_login_status` imposes that order, so it does
    /// not depend on how careful the descriptor's author was.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logged_out_when: Vec<String>,
    /// The fields not understood, for the same reason as `Descriptor::extra`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// How **an engine's usage** is read, as the descriptor declares it: measuring
/// a new engine must not mean recompiling. The field is optional, and an engine
/// without it is invoked as before and leaves its tokens at **unknown** — never
/// at zero, which no downstream view could correct.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Usage {
    /// The options to add to the question so it reports its usage — for example
    /// `["--output-format", "json"]`. They are appended to `ask`'s **only** when
    /// the descriptor's recipe dictates the command line: a step writing its own
    /// arguments is saying something precise about *that* call, and lengthening
    /// it behind its back would be deciding in its place. There the usage stays
    /// unknown, which is the right price to pay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Which shape to read the output in: `json` (pointers are key paths) or
    /// `text` (pointers are regular expressions with a capture group).
    #[serde(default)]
    pub read: ReadAs,
    /// Which pipe carries the usage: `stdout` (the default), `stderr`, `both`.
    #[serde(default)]
    pub from: ReadFrom,
    /// Whose numbers those are: `per_call` (the default, and how every reading
    /// has been treated) or `per_session`, written only where it was measured.
    #[serde(default)]
    pub reports: models::usage::Reports,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<Where>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<Where>,
    /// The input tokens **read from the cache**. Declared separately because
    /// they cost separately, often an order of magnitude less: a single input
    /// number would make the measurement false exactly where it counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<Where>,
    /// The input tokens **written to the cache**. Not the same as reading it:
    /// writing costs **more** than plain input, reading much less. On one
    /// measured call this entry alone was 96% of the spend — leaving it out
    /// gets the bill wrong on the low side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<Where>,
    /// The tokens written to a **long-lived** cache, where one exists and has a
    /// price of its own, higher than the short one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_long_tokens: Option<Where>,
    /// The total, for engines that report only that without splitting the sides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<Where>,
    /// How many turns the call took: how many times the model came back to speak
    /// inside one invocation. **It is the figure that explains the bill of a
    /// chain of steps.** Measured: a four-step flow reads 8% more per turn than
    /// a single session doing the same work, but takes twice the turns — and
    /// costs twice as much. Making a flow cheaper means moving this number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<Where>,
    /// Where the engine declares what it charged. Recorded as a cross-check: the
    /// local price list stays the source of truth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Where>,
    /// Where the engine names the model that actually served the call. It is the
    /// only honest link between a command line and a price-list entry: an engine
    /// that does not declare it leaves the cost unknown, and that is fine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Where>,
    /// Where the answer's text sits inside the envelope. **Whoever asks for an
    /// envelope must declare it**: asking for tokens in JSON wraps the answer
    /// too, so without this pointer a downstream step would receive the envelope
    /// instead of the text, and a flow declaring the shape of its own answer
    /// would go red over a measurement it never asked for. Measuring must not
    /// change what is measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Where>,
    /// The fields not understood, for the same reason as `Descriptor::extra`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// The shape an engine reports its usage in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadAs {
    /// The output is a JSON envelope.
    #[default]
    Json,
    /// The output is plain text.
    Text,
}

/// The pipe an engine states its usage on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadFrom {
    #[default]
    Stdout,
    Stderr,
    Both,
}

/// Where a value sits inside what the engine said, and **the shape is said by
/// the pointer**, as in `models::usage::read_text`: a key path holds on a JSON
/// envelope, a regular expression on text. A shape field of its own would let a
/// descriptor contradict itself into an unknown answer with no visible cause.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Where {
    /// A key path, for what is read as JSON.
    Path(Vec<String>),
    /// A regular expression with the value in the first group, for what is read
    /// as text. A pointer of the wrong shape finds nothing and leaves the value
    /// unknown: an imprecise descriptor degrades the measurement, does not break
    /// the call it was measuring, and never invents a number in place of one it
    /// did not find.
    Pattern(String),
    /// `{"first_key_of": ["modelUsage"]}` — the value is **the name of the first
    /// key** of the object at that path. Needed by engines that put the model not
    /// in a field but in a key. Without this shape that name is unreachable, and
    /// without the name no price-list entry is found: the cost stays unknown even
    /// with every token counted and a correct price list to hand.
    FirstKeyOf { first_key_of: Vec<String> },
}

/// Where the question's text goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptPlace {
    /// On standard input. The commonest case, and the default.
    #[default]
    Stdin,
    /// As the last argument of the command line, with the options before it.
    LastArg,
}

/// One of the forms a capability is obtained in: the options to put on the
/// command line, and whether the last of them wants a value after it. **A form
/// and not a string**, because `--session-id <uuid>` and `--fork-session` are
/// written alike in a table and compose differently: whoever composes the line
/// must read that from the data, not guess it from the name.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct CapabilityForm {
    /// The options, or the subcommand, the capability is obtained with.
    /// `["exec", "resume"]` is as valid as `["--resume"]`: a subcommand is an
    /// argument like any other.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// The last of `args` wants a value straight after it.
    #[serde(default)]
    pub takes_value: bool,
    /// The permitted values, when they are a closed and measured set — the
    /// sources of `--setting-sources`, the formats of `--output-format`. Empty
    /// does not mean "none": it means the set is not closed, or nobody wrote it
    /// down.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    /// The constraint the table cannot carry: "only with `--print`", "loses the
    /// credentials". It is text for the reader, and enters no decision.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// The unit of a value to convert — `usd`, `tokens`. Empty converts nothing.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    /// The fields not understood, for the same reason as `Descriptor::extra`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// What a descriptor declares about a capability. **Three states and not two,
/// and that is the point of the whole block**: `false` says "someone looked and
/// it is not there", silence says "nobody looked". A block that only let you
/// list what is there would make every absence look measured — including that of
/// a tool nobody ever opened. And declaring nothing is not an error: whoever
/// lacks a capability pays more, they do not stop.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Capability {
    /// `false`: measured and not there. `true`: there, with no options to
    /// declare — the capability is in the behaviour, not in a flag.
    Known(bool),
    /// One way only, written without square brackets: whoever declares the
    /// simple case need not know the complicated one. Same choice as [`Probes`].
    One(CapabilityForm),
    /// Several ways to the same capability: `claude` resumes a session with
    /// `--resume`, with `--session-id` or with `--continue`, and that is three.
    Many(Vec<CapabilityForm>),
}

impl Capability {
    /// True when the tool offers it.
    pub fn is_available(&self) -> bool {
        match self {
            Capability::Known(has) => *has,
            Capability::One(_) => true,
            // An empty list declares that someone looked and found no way at
            // all: an absence, not a presence without instructions.
            Capability::Many(forms) => !forms.is_empty(),
        }
    }

    /// The declared ways, which can be zero even when the capability is there.
    pub fn forms(&self) -> &[CapabilityForm] {
        match self {
            Capability::Known(_) => &[],
            Capability::One(form) => std::slice::from_ref(form),
            Capability::Many(forms) => forms,
        }
    }
}

/// How a tool stands against a capability a step asked for.
///
/// **A BOOLEAN IS NOT ENOUGH.** It is the same distinction [`crate::Presence`]
/// keeps between "not there" and "could not look", carried from the world into
/// the vocabulary: whoever reads a warning needs to know whether the remedy is
/// to change engine or to measure the one they have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityState {
    /// Declared, and obtainable.
    Available,
    /// Declared absent: someone looked, and this tool does not have it. Lacking
    /// one costs more, it does not stop anything — the answer's shape asked for
    /// in the prompt instead of imposed by the engine. That is the permanent
    /// constraint "model independence", and today's fallback stays the fallback.
    Absent,
    /// The descriptor does not name it. Not "it does not have it": "nobody
    /// looked", and the remedy is to measure, not to change engine.
    NotLookedAt,
}

/// A descriptor that, instead of saying "this thing is either here or not",
/// **discovers** several entries by reading a configuration file.
///
/// AND IT IS NOT A SPECIAL CASE FOR MCP SERVERS. Listing a machine's servers by
/// hand would be the hard-coded list this crate exists to avoid: they change
/// when the user adds one, and nobody recompiles for that.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JsonKeys {
    /// The files to read — *where to look*. They accept `~/`, `$VAR` and `*`.
    pub files: Vec<String>,
    /// The path down to the object whose keys are the entries — *under which
    /// key*. A `*` stands for "every key at this level": `["projects", "*",
    /// "mcpServers"]` collects the servers declared project by project. The code
    /// opens the file and reports the keys it finds, without knowing what they
    /// are, and the same mechanism lists another tool's profiles the day somebody
    /// writes that descriptor.
    pub pointer: Vec<String>,
}

/// How several entries are discovered instead of answering "here or not here".
///
/// Two shapes, and the second was born with the automations catalog: the keys of
/// a JSON file say which hooks a command line declares, but an operating-system
/// service is **one file per service** in a directory, and knowing the directory
/// is not empty helps nobody decide what to migrate. The reader wants the names.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Enumerate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_keys: Option<JsonKeys>,
    /// The path patterns whose existing files are the entries. They accept `~/`,
    /// `$VAR` and `*`. An entry is the path itself, in full: two files with the
    /// same name in two directories are two different automations, and naming
    /// them alike would count them as one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
}

impl Enumerate {
    /// True when it says nowhere to look at all.
    pub fn is_empty(&self) -> bool {
        self.json_keys.is_none() && self.paths.is_none()
    }
}

/// The gesture that signs a command line in: the options it is started with,
/// and whether the sign-in then happens inside the program (a browser opens,
/// a code is typed) rather than on the line. **A GESTURE, NEVER RUN BY A
/// CHECK**: it changes the machine's state, so only a person starts it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Login {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// The channel a person's spent quota is read from, as data. **THE CODE KNOWS
/// A KIND OF CHANNEL, NEVER A PROVIDER**: `oauth_usage` is a credentials file,
/// a pointer to the token inside it, an address and its headers; a second
/// provider with the same kind of channel is a descriptor, not a recompile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Quota {
    /// The kind of channel: `oauth_usage` is the only one read today.
    pub reader: String,
    /// The credentials file; accepts `~/` and `$VAR`.
    pub credentials: String,
    /// The path of keys down to the token inside that file.
    pub token_pointer: Vec<String>,
    pub url: String,
    /// Whole header lines, `name: value`, sent beside the bearer token.
    #[serde(default)]
    pub headers: Vec<String>,
    /// Where the windows sit and what their fields are called; absent, the
    /// root with `utilization` and `resets_at`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<QuotaShape>,
    /// A command that prints the credentials, where they are not in a file.
    ///
    /// **A KEYRING IS NOT A FILE, AND SEVERAL ACCOUNTS LIVE IN ONE.** `{home}`
    /// becomes the account's home and `{home_digest}` the first eight hex of
    /// its sha256 — the two ways a keeper of secrets tells one home's entry
    /// from another's. The words are the product's and belong here, in data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held_by: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// The words one provider uses for the windows. Each field names a key.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaShape {
    /// The keys down to the windows; empty is the root.
    #[serde(default)]
    pub windows_at: Vec<String>,
    /// The key holding how much of a window is already gone.
    pub used: String,
    /// `percent` for 50 meaning half, `fraction` for 0.5.
    #[serde(default)]
    pub used_in: String,
    #[serde(default)]
    pub resets: String,
    /// `text` for an instant written out, `epoch_seconds` for a number.
    #[serde(default)]
    pub resets_in: String,
}

/// The one line that installs a command line, and how it was established.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    pub line: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// The line that empties a session that is already open, typed as a person
/// would type it.
///
/// A line and not a flag. A flag is how a command line is launched; this is
/// what is said to one already running, and no launch flag can reach a session
/// that is already open.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ResetContext {
    pub line: String,
    /// For whoever reads: how it was established, and what was not checked.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// How another program is asked to type into the terminals it keeps. A session
/// Sailor did not open belongs to whoever opened it, and the only way in is the
/// door that owner declares. `{handle}` and `{line}` are filled in.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct KeepsTerminals {
    /// Variables a kept session carries: the first one present names it.
    pub known_by: Vec<String>,
    pub reads_the_screen: Vec<String>,
    pub types_a_line: Vec<String>,
    /// What prints every terminal this program keeps. Absent where the name a
    /// session carries is already the one its keeper wants.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lists_them: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_the_list: Option<InTheList>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Where the handle is in what a keeper prints, and how a session is matched to
/// it. **A SESSION NAMES ITSELF ONE WAY AND ITS KEEPER ANOTHER**, and the join
/// between the two is measured, never guessed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct InTheList {
    /// The keys to walk to reach the array of terminals.
    pub at: Vec<String>,
    /// The fields of an entry that, joined, make the name a session carries.
    pub known_by: Vec<String>,
    pub joined_by: String,
    pub the_handle_is: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// What is painted when nobody is waiting on a session of this line.
/// **SILENCE IS NOT FREEDOM**: absent means nobody measured it, never «free».
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FreeWhen {
    pub the_prompt_shows: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub and_none_of_these: Vec<String>,
    /// How long the screen must have stood still: a session at work repaints.
    pub and_still_for_seconds: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// The moment a session begins: the one whose answer reaches the agent.
pub const SESSION_START: &str = "session_start";

/// The moments Sailor needs to hear about, in Sailor's own words.
///
/// Named here because they are what Sailor needs, not what any one product
/// offers. A command line maps them onto its own event names, and stays silent
/// about the ones it cannot report. Whoever acts at each asks this list.
pub const MOMENTS: &[&str] = &[SESSION_START, "alive", "asked", "compacting"];

/// How a settings file is written, because grafting has to read it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileFormat {
    #[default]
    Json,
    Toml,
}

/// Where a command line keeps a file: a variable, and where to look without it.
///
/// **NEITHER HALF IS ENOUGH ALONE.** `CODEX_HOME` is unset in a plain shell and
/// points elsewhere under a host that declares it — so a wired path is wrong in
/// the second case and a bare variable name in the first. Both, and the
/// fallback below the home is never empty.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HooksFile {
    /// The environment variable holding the root, when this line has one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root_var: String,
    /// The path below that variable, when it is set.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub below_root: String,
    /// Where to look when it is not: below the home.
    pub below_home: String,
    #[serde(default)]
    pub format: FileFormat,
    /// The key the hooks sit under, nested where a line nests them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl HooksFile {
    /// The file itself, given what the environment says. Both values are
    /// arguments rather than read here, so the choice can be tested without a
    /// machine that happens to declare one of them.
    pub fn path(&self, root: Option<&str>, home: &str) -> Option<PathBuf> {
        match root.filter(|value| !value.is_empty()) {
            Some(root) if !self.below_root.is_empty() => {
                Some(Path::new(root).join(&self.below_root))
            }
            _ => (!self.below_home.is_empty()).then(|| Path::new(home).join(&self.below_home)),
        }
    }
}

/// How this command line is made to say that a session started, is alive, was
/// asked something, is about to be compacted.
///
/// Absent means nobody measured it, never «it cannot be done»: the same
/// direction as `login_status` and `reset_context`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SessionHooks {
    pub file: HooksFile,
    /// Sailor's moments mapped onto this line's own event names.
    ///
    /// **A MISSING KEY MEANS «THIS LINE CANNOT SAY IT», NEVER «USE THE NEAREST
    /// EVENT».** One engine has no session-start of its own, and its closest
    /// event fires before every turn: filling the gap with it would put the
    /// welcome in front of every turn instead of once.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub events: BTreeMap<String, String>,
    /// Where the words a user types live, when this line has such a thing.
    ///
    /// Absent, and the welcome must promise none: a word promised and missing
    /// is worse than a word never offered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<HooksFile>,
    /// For whoever reads: how it was established, and what was not checked.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// One line of the list of what to look for.
///
/// **IT DOES NOT REFUSE FIELDS IT DOES NOT KNOW.** `deny_unknown_fields` threw a
/// descriptor written for a newer Sailor out whole over one extra field, and the
/// tool vanished with it. Now the field lands in `extra`, the entry lives, the
/// loader notes it by name, and `extra` is serialized so rewriting keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Descriptor {
    /// The line's identity. Two descriptors with the same `id` do not coexist:
    /// the last one loaded wins, and that is how a user rewrites a shipped
    /// descriptor without having to delete it.
    pub id: String,
    /// Which family it belongs to: `ai_cli`, `mcp_server`, `tool`, or any other
    /// word. The code knows none of them — it only groups and filters by the
    /// value, and a new name works the day somebody writes it.
    pub family: String,
    /// What it is called for the reader.
    #[serde(default)]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detect: Option<Probes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enumerate: Option<Enumerate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionProbe>,
    /// How a one-shot question is put to it, for engines that accept one.
    /// Without this, a step wanting to use it must write the options itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<Ask>,
    /// How its usage is read. Optional, and must stay optional: a descriptor
    /// written before this field existed must keep loading identically, or a new
    /// Sailor would switch off the tools declared with the old one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// How it is asked whether the home it starts from is authenticated.
    ///
    /// **AN ENGINE THAT DOES NOT DECLARE IT TRIGGERS NOTHING, AND THE DIRECTION
    /// IS THE DECISION.** Absent means "nobody looked", never "authenticated": a
    /// default saying yes would silence exactly the condition this block exists
    /// to make visible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_status: Option<LoginStatus>,
    /// How a person signs it in, as a gesture to run in a terminal of theirs.
    /// Absent means nobody measured it, never «it needs no sign-in».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<Login>,
    /// How it is put on a machine that lacks it: one line, typed for a person
    /// to confirm. Absent means nobody measured where it installs from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<Install>,
    /// The channel that says how much of the person's quota is spent. Absent
    /// means no channel is known, and the engine works without knowing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota: Option<Quota>,
    /// Whether what is sent through it trains the provider's next model, for
    /// the models its own subscription reaches. Absent is unknown, never a no.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_pact: Option<models::pact::DataPact>,
    /// What this tool can do beyond answering: resume a session, impose a shape
    /// on the answer, isolate itself from its host's configuration, receive an
    /// allowance, keep a spend cap of its own. **The code knows no capability
    /// name, and must not**: it is a map from name to declaration, so adding one
    /// to a new tool is a JSON file and never a recompile. It is the permanent
    /// constraint "we write code only for what touches the world".
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub capabilities: BTreeMap<String, Capability>,
    /// How a session of this command line that is already running is told to
    /// drop what it holds.
    ///
    /// Absent means nobody measured it, never «it cannot be done». Anything
    /// that read absence as a default would type a guess into a working
    /// session, and a wrong line typed into one cannot be taken back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_context: Option<ResetContext>,
    /// Absent means nobody measured it, and then nothing is typed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free_when: Option<FreeWhen>,
    /// For a program that opens terminals of its own: how it is asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keeps_terminals: Option<KeepsTerminals>,
    /// Variables a session of this line must not inherit when Sailor hosts it.
    /// Absent takes nothing away.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_inherited: Vec<String>,
    /// How this line is grafted so that it reports a session's moments.
    ///
    /// Absent means it is not grafted and Sailor says so. Never a nearest
    /// guess: the address of a settings file is not a detail, it is the
    /// coupling, and it belongs in data where a reader can check it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_hooks: Option<SessionHooks>,
    /// Where its configuration lives. Accepts `~/`, `$VAR` and `*`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config: Vec<String>,
    /// A note for whoever reads the list: where it installs from, what the
    /// package is called. It enters no decision.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Switches a descriptor off without deleting it. It is the way to get rid
    /// of a shipped one: rewrite its `id` with `disabled: true`.
    #[serde(default)]
    pub disabled: bool,
    /// The fields this version of Sailor does not know.
    ///
    /// They live here instead of taking the entry down, and are written back
    /// unchanged when the descriptor is serialized. The loader names them in a
    /// note: ignoring them in silence would be the same silent fault elsewhere.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Descriptor {
    /// The fields that were not understood, with the path to find them.
    ///
    /// It looks inside `ask` and `usage` too: an unknown field down there took
    /// the entry down exactly like a top-level one, and whoever reads the note
    /// needs to know **where** to look for it, not only that it is there.
    pub fn unknown_fields(&self) -> Vec<String> {
        let mut found: Vec<String> = self.extra.keys().cloned().collect();
        if let Some(ask) = &self.ask {
            found.extend(ask.extra.keys().map(|key| format!("ask.{key}")));
        }
        if let Some(usage) = &self.usage {
            found.extend(usage.extra.keys().map(|key| format!("usage.{key}")));
        }
        if let Some(login) = &self.login_status {
            found.extend(login.extra.keys().map(|key| format!("login_status.{key}")));
        }
        if let Some(free) = &self.free_when {
            found.extend(free.extra.keys().map(|key| format!("free_when.{key}")));
        }
        if let Some(keeps) = &self.keeps_terminals {
            found.extend(
                keeps
                    .extra
                    .keys()
                    .map(|key| format!("keeps_terminals.{key}")),
            );
            if let Some(list) = &keeps.in_the_list {
                found.extend(
                    list.extra
                        .keys()
                        .map(|key| format!("keeps_terminals.in_the_list.{key}")),
                );
            }
        }
        if let Some(reset) = &self.reset_context {
            found.extend(reset.extra.keys().map(|key| format!("reset_context.{key}")));
        }
        if let Some(hooks) = &self.session_hooks {
            found.extend(hooks.extra.keys().map(|key| format!("session_hooks.{key}")));
            found.extend(
                hooks
                    .file
                    .extra
                    .keys()
                    .map(|key| format!("session_hooks.file.{key}")),
            );
        }
        // A capability's name is not an unknown field — no name is, by
        // construction. What is unknown sits inside one of its forms.
        for (name, capability) in &self.capabilities {
            for form in capability.forms() {
                found.extend(
                    form.extra
                        .keys()
                        .map(|key| format!("capabilities.{name}.{key}")),
                );
            }
        }
        found
    }

    /// What this line calls one of Sailor's moments, when it can report it.
    ///
    /// `None` is an answer and travels as one: this command line cannot say
    /// that moment, so whoever reads it grafts the others and declares this
    /// one ungrafted. A neighbouring event put in its place would arrive
    /// where nobody asked for it.
    pub fn event_for(&self, moment: &str) -> Option<&str> {
        self.session_hooks
            .as_ref()?
            .events
            .get(moment)
            .map(String::as_str)
            .filter(|name| !name.is_empty())
    }

    /// The line that empties a running session, when somebody has measured it.
    ///
    /// `None` is the answer that must reach whoever asks: it says the machine
    /// does not know, and knowing nothing is a reason to refuse, never a reason
    /// to fall back on a line that belongs to a different command line.
    pub fn reset_line(&self) -> Option<&str> {
        self.reset_context
            .as_ref()
            .map(|reset| reset.line.as_str())
            .filter(|line| !line.is_empty())
    }

    /// How this tool stands against a capability that was asked for.
    pub fn capability(&self, name: &str) -> CapabilityState {
        match self.capabilities.get(name) {
            None => CapabilityState::NotLookedAt,
            Some(capability) if capability.is_available() => CapabilityState::Available,
            Some(_) => CapabilityState::Absent,
        }
    }

    /// The options a model's name is written after. A form without a value has
    /// nowhere to put the name, so it answers `None` as an undeclared
    /// capability does: neither can be told which model to answer with.
    pub fn model_option(&self) -> Option<Vec<String>> {
        let forms = self.capabilities.get(CHOOSE_MODEL)?.forms();
        let carries = |form: &&CapabilityForm| form.takes_value && !form.args.is_empty();
        forms.iter().find(carries).map(|form| form.args.clone())
    }

    /// Where this descriptor says two different things about the same fact. The
    /// defect is never in one file: it is in never having compared the two
    /// blocks. **It lives in the library and not inside a test** because a test
    /// sees only the shipped descriptors, while someone writing one in
    /// `~/.config/sailor/tools.d/` would sit outside every check. An empty list
    /// means the blocks hold together — not that they are right.
    pub fn contradictions(&self) -> Vec<String> {
        let mut found = Vec::new();
        let can_be_asked = self.ask.is_some();
        let says_it_can = self.capability(ASK_WITHOUT_INTERACTION) == CapabilityState::Available;

        // **THE TWO DIRECTIONS ARE TWO DIFFERENT DEFECTS, AND BOTH ARE NEEDED.**
        // An engine with the line that stays silent about the capability makes
        // readers believe it cannot be interrogated; one declaring the capability
        // without the line makes them believe the opposite.
        if can_be_asked && !says_it_can {
            found.push(format!(
                "declares how a question is put to it (an `ask` block) and does not declare \
                 that it can receive one (`capabilities.{ASK_WITHOUT_INTERACTION}`): two \
                 blocks of the same file contradicting each other"
            ));
        }
        if says_it_can && !can_be_asked {
            found.push(format!(
                "declares `capabilities.{ASK_WITHOUT_INTERACTION}` and has no `ask` block: \
                 no command line can be composed to put the question, so the capability is \
                 true and unusable"
            ));
        }

        if let (Some(ask), Some(capability)) = (
            self.ask.as_ref(),
            self.capabilities.get(ASK_WITHOUT_INTERACTION),
        ) {
            // The line that actually gets composed is `ask`'s: an option declared
            // only among the capabilities is describing some other engine.
            let composed: Vec<&str> = ask
                .args
                .iter()
                .chain(ask.args_before_prompt.iter())
                .map(String::as_str)
                .collect();
            for form in capability.forms() {
                for option in &form.args {
                    if !composed.contains(&option.as_str()) {
                        found.push(format!(
                            "declares the capability with «{option}», which does not appear in \
                             its `ask` block: the line that actually gets composed is `ask`'s"
                        ));
                    }
                }
            }
        }

        // **AN EMPTY FRAGMENT IS A DECLARATION THAT MATCHES EVERYTHING.** Its
        // author would never notice: the descriptor works, and answers yes to
        // any output at all. In `unusable_when` it would walk the chain down on
        // every failure — the very defect the chain exists not to introduce —
        // and in `refuses_without_prompt` it would pass every broken line as
        // sound.
        if let Some(ask) = self.ask.as_ref() {
            for (field, marks) in [
                ("unusable_when", &ask.unusable_when),
                ("exhausted_when", &ask.exhausted_when),
                ("waits_for_a_person_when", &ask.waits_for_a_person_when),
                ("refuses_without_prompt", &ask.refuses_without_prompt),
            ] {
                if marks.iter().any(|mark| mark.trim().is_empty()) {
                    found.push(format!(
                        "declares an empty fragment in `ask.{field}`, which is contained in \
                         any output at all: it would always match"
                    ));
                }
            }
            // A word that means a spent quota must also mean it cannot work:
            // the run reads the first list, the dry run and the fallback rule
            // read the second, and a word in the first alone splits the verdict.
            let unusable: Vec<String> = ask
                .unusable_when
                .iter()
                .map(|mark| mark.trim().to_lowercase())
                .filter(|mark| !mark.is_empty())
                .collect();
            for mark in ask.exhausted_when.iter().filter(|mark| !mark.trim().is_empty()) {
                let said = mark.trim().to_lowercase();
                if !unusable.iter().any(|covering| said.contains(covering)) {
                    found.push(format!(
                        "declares «{mark}» in `ask.exhausted_when` and no word of \
                         `ask.unusable_when` covers it: the run would class that output a \
                         spent quota while the dry run and the fallback rule never see it"
                    ));
                }
            }
        }

        found
    }

    /// Why this tool cannot serve as a fallback inside a chain, when it cannot.
    /// `None` when it can. **It is a rule of position, not of descriptor**: an
    /// empty `unusable_when` means "nobody looked", never "it is fine", and it
    /// becomes a defect only where somebody puts another engine behind it —
    /// there its running out passes for an ordinary failure, the step dies, and
    /// the engines after it never start. Last in a chain, it costs nothing.
    pub fn cannot_be_a_fallback(&self) -> Option<String> {
        let Some(ask) = self.ask.as_ref() else {
            return Some(
                "its descriptor does not declare how a question is put to it (`ask`), so \
                 there is no command line to compose when the work reaches it"
                    .to_owned(),
            );
        };
        if ask.unusable_when.iter().any(|mark| !mark.trim().is_empty()) {
            return None;
        }
        Some(
            "it does not declare the words it says it cannot work with (`ask.unusable_when`), \
             so its running out passes for an ordinary failure: the step dies on it and the \
             engines after it never start"
                .to_owned(),
        )
    }
}

/// The name of the capability that speaks of one-shot questions.
///
/// It answers the same question — "can this engine be interrogated without
/// opening a conversation?" — that `ask` answers by composing a line, and two
/// copies of one truth drift apart uncompared. The comparison needs the name.
pub const ASK_WITHOUT_INTERACTION: &str = "ask_without_interaction";

/// The name of the capability saying how an engine is told which model to
/// answer with. The code reads a form; the options are the descriptor's.
pub const CHOOSE_MODEL: &str = "choose_model";

/// A descriptor that says two different things about the same fact.
///
/// It carries the tool's name because whoever reads a list of contradictions
/// needs to know which entry to open: "a descriptor contradicts itself" cannot
/// be repaired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Contradiction {
    pub tool: String,
    pub said: String,
}

impl Contradiction {
    /// One line for whoever reads a report.
    pub fn line(&self) -> String {
        format!("«{}»: {}", self.tool, self.said)
    }
}

/// A loaded descriptor, with where it came from: whoever reads the result must
/// be able to trace the file that produced it, or "from which descriptor" is not
/// a checkable answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub descriptor: Descriptor,
    pub source: String,
}

/// Something that could not be loaded, with the why and the where.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub source: String,
    /// The `id` when it could be read, otherwise the position in the file.
    pub about: String,
    pub reason: String,
}

/// The list of what to look for, plus the lines that did not read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub descriptors: Vec<Loaded>,
    /// The entries that were **lost**: not in the catalog, and here is why.
    pub problems: Vec<Problem>,
    /// The entries that were **kept**, with something ignored about them.
    ///
    /// **THEY ARE NOT IN `problems`, AND THE DIFFERENCE IS EVERYTHING.** A
    /// problem says "this tool is not here"; a note says "it is here, and I
    /// ignored one field of it". Merging them would count working entries as
    /// faults — and there are tests counting `problems` one by one.
    pub notes: Vec<Problem>,
}

/// The text of a shipped catalog, by name.
pub fn builtin_catalog(name: &str) -> Option<&'static str> {
    BUILTIN_CATALOGS
        .iter()
        .find(|(catalog, _)| *catalog == name)
        .map(|(_, text)| *text)
}

impl Catalog {
    /// Loads in order: whoever arrives later wins on the `id` already there.
    pub fn load(sources: &[Source]) -> Catalog {
        let mut catalog = Catalog::default();
        for source in sources {
            match source {
                Source::Builtin => catalog.absorb(BUILTIN_SOURCE, BUILTIN),
                Source::BuiltinNamed(name) => match builtin_catalog(name) {
                    Some(text) => catalog.absorb(&format!("{BUILTIN_SOURCE}:{name}"), text),
                    None => catalog.problems.push(Problem {
                        source: BUILTIN_SOURCE.to_string(),
                        about: name.clone(),
                        reason: format!(
                            "no shipped catalog is called that; the shipped ones are: {}",
                            BUILTIN_CATALOGS
                                .iter()
                                .map(|(name, _)| *name)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    }),
                },
                Source::File(path) => catalog.absorb_file(path),
                Source::Dir(dir) => {
                    let Ok(entries) = fs::read_dir(dir) else {
                        // A directory that is not there is not a fault: it is the
                        // ordinary case of someone who never added a descriptor
                        // of their own. One that is there and does not read is a
                        // fault, and the two are told apart by looking at the
                        // disk, not at the error.
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
                        .map(|e| e.path())
                        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
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

    /// THE TEXT IS READ TWICE, ON PURPOSE, BECAUSE A BROKEN FILE MUST NOT BRING
    /// THE DETECTION DOWN. First as generic JSON, then item by item: reading the
    /// whole array as `Vec<Descriptor>` would drop twenty good descriptors over
    /// one misplaced comma in the twenty-first, and the problem would not even
    /// say which. An inventory that falls silent because one line was wrong is
    /// worse than an incomplete one: it looks empty rather than partial.
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
        // A bare array or `{"tools": [...]}`: whoever adds a tool writes whichever
        // shape comes to them, and neither is wrong.
        let items = match &value {
            Value::Array(items) => items.clone(),
            Value::Object(map) => match map.get("tools") {
                Some(Value::Array(items)) => items.clone(),
                _ => {
                    self.problems.push(Problem {
                        source: source.to_string(),
                        about: "the file".to_string(),
                        reason: "contains neither an array nor a `tools` field".to_string(),
                    });
                    return;
                }
            },
            _ => {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about: "the file".to_string(),
                    reason: "contains neither an array nor a `tools` field".to_string(),
                });
                return;
            }
        };
        for (index, item) in items.iter().enumerate() {
            let about = item
                .get("id")
                .and_then(|v| v.as_str())
                .map(|id| id.to_string())
                .unwrap_or_else(|| format!("entry number {}", index + 1));
            let descriptor: Descriptor = match serde_json::from_value(item.clone()) {
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
            // **AN UNKNOWN FIELD IS A NOTE, NOT A REFUSAL.** It used to take the
            // whole entry down, and the tool vanished with it. The note goes in
            // `notes` and not in `problems`, because the descriptor is there and
            // alive — whoever counts problems is counting lost entries.
            let unknown = descriptor.unknown_fields();
            if !unknown.is_empty() {
                self.notes.push(Problem {
                    source: source.to_string(),
                    about: about.clone(),
                    reason: format!(
                        "fields this version does not know, ignored: {}",
                        unknown.join(", ")
                    ),
                });
            }
            if descriptor.detect.is_none() && descriptor.enumerate.is_none() {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about,
                    reason: "does not say how it is checked: no `detect` and no `enumerate`"
                        .to_string(),
                });
                continue;
            }
            // AN EMPTY `enumerate` DISCOVERS NOTHING, and without this line it
            // would answer "no entries" — which reads as "there is nothing here"
            // instead of "the list is written badly".
            if descriptor
                .enumerate
                .as_ref()
                .is_some_and(|enumerate| enumerate.is_empty())
            {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about,
                    reason: "`enumerate` does not say where to look: no `json_keys` and no `paths`"
                        .to_string(),
                });
                continue;
            }
            self.replace(Loaded {
                descriptor,
                source: source.to_string(),
            });
        }
    }

    fn replace(&mut self, loaded: Loaded) {
        match self
            .descriptors
            .iter_mut()
            .find(|l| l.descriptor.id == loaded.descriptor.id)
        {
            Some(existing) => *existing = loaded,
            None => self.descriptors.push(loaded),
        }
    }

    /// Every contradiction of every live descriptor, in a stable order.
    ///
    /// It looks at the live ones and not the disabled: a switched-off descriptor
    /// composes no line and declares nothing to anybody — calling it
    /// contradictory would send someone to repair a file that is out of service.
    pub fn contradictions(&self) -> Vec<Contradiction> {
        let mut found = Vec::new();
        for loaded in self.live() {
            for said in loaded.descriptor.contradictions() {
                found.push(Contradiction {
                    tool: loaded.descriptor.id.clone(),
                    said,
                });
            }
        }
        found
    }

    /// The ones to run: without the switched-off, in a stable order by `id`,
    /// because two reads in a row must give the same sequence or comparing one
    /// day with the next is worth nothing.
    pub fn live(&self) -> Vec<&Loaded> {
        let mut out: Vec<&Loaded> = self
            .descriptors
            .iter()
            .filter(|l| !l.descriptor.disabled)
            .collect();
        out.sort_by(|a, b| {
            (&a.descriptor.family, &a.descriptor.id).cmp(&(&b.descriptor.family, &b.descriptor.id))
        });
        out
    }
}

/// Its tests, in a file of their own: 756 lines of them took this one over.
#[cfg(test)]
mod tests;
