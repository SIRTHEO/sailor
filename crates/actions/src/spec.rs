//! The typed input of an engine step, as the flow wrote it.

use flow::{Refusal, ValueSchema};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// What a step asks of the engine's session.
///
/// **A STEP NAMES A STEP, NOT AN IDENTIFIER.** A session's identifier is born
/// while the run goes; whoever writes a flow cannot know it, and a flow holding
/// one would be good for a single run. So the step names who it continues from
/// — `{"fork": "scopri"}` — and the ledger looks the identifier up, since that
/// is where the `scopri` step laid it down.
///
/// **RESUMING AND FORKING ARE NOT THE SAME, AND CONFUSING THEM COSTS.** A
/// resume continues the session: two steps resuming the same trunk write over
/// each other, and on a parallel front nobody decides in which order. A fork
/// starts from the same context and goes its own way: the right shape for three
/// independent steps looking at one tree, and the one that pays best — the
/// discovery is bought once instead of three times.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SessionUse {
    /// `"session": "open"` — opens a new session and records it, so later
    /// steps can continue it.
    Open,
    /// `"session": {"resume": "scopri"}` — continues the named step's session.
    Resume(String),
    /// `"session": {"fork": "scopri"}` — starts from the named step's context
    /// without touching it.
    Fork(String),
}

impl SessionUse {
    /// The mode, in words, for whoever is watching.
    pub(crate) fn word(&self) -> &'static str {
        match self {
            SessionUse::Open => "open a session",
            SessionUse::Resume(_) => "resume a session",
            SessionUse::Fork(_) => "fork a session",
        }
    }
}

/// Whom to run: one engine, or a chain of engines to try in order.
///
/// **WHY A CHAIN AND NOT A SINGLE FALLBACK.** One fallback covers tonight's
/// case and not tomorrow's: engines run out in stages, and whoever has three
/// installed wants the work to find the first that can do it. The chain is read
/// in the order written, and that order is a choice made by whoever wrote the
/// flow — the best first, not the cheapest.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ToolChoice {
    One(String),
    Chain(Vec<String>),
}

impl ToolChoice {
    pub(crate) fn ids(&self) -> &[String] {
        match self {
            ToolChoice::One(id) => std::slice::from_ref(id),
            ToolChoice::Chain(ids) => ids,
        }
    }
}

/// The engines a step's `with` names, in the order written. One name is a
/// chain of one — the shape `ToolChoice` already accepts — and a `tool` of
/// any other shape names none. Every reader of a flow's chains asks here, so
/// a check cannot read a step one way and the run another.
pub fn engines_named_in(with: &Value) -> Vec<String> {
    with.get("tool")
        .and_then(|tool| serde_json::from_value::<ToolChoice>(tool.clone()).ok())
        .map(|choice| choice.ids().to_vec())
        .unwrap_or_default()
}

/// The per-call ceiling a step's `with` declares, for whoever checks a flow.
///
/// **A `with` UNDER A CHECK IS NOT RESOLVED YET**: its prompt is still the
/// reference that will compose it, so the struct the run obeys does not parse
/// and reading a ceiling through it read nothing. The two ceiling fields are
/// scalars, so they are read alone, and a test holds the two readers as one.
pub fn ceiling_declared_in(with: &Value) -> crate::reserve::Declared {
    serde_json::from_value::<crate::reserve::Declared>(with.clone()).unwrap_or_default()
}

/// The model a step's `with` asks of each engine, by engine id.
///
/// **THE ONE FIELD AND NOT THE WHOLE STEP**, as `engines_named_in` reads
/// `tool`: before the run a `stdin` is still a reference and no `EngineSpec`
/// parses, so reading the struct would find no model in the very steps that
/// name one. An engine absent from the map runs on a default nobody chose.
pub fn models_named_in(with: &Value) -> BTreeMap<String, String> {
    with.get("model")
        .and_then(|model| serde_json::from_value(model.clone()).ok())
        .unwrap_or_default()
}

/// The ceiling this step declares, in every unit an engine may take one in.
pub(crate) fn ceiling_of(spec: &EngineSpec) -> crate::reserve::Declared {
    crate::reserve::Declared {
        max_spend_micros: spec.max_spend_micros,
        max_tokens: spec.max_tokens,
    }
}

/// Whether a step's `with` declares its text private. Absent is public, and
/// any other word is not private either: the run refuses it when it parses the
/// step, and a check must not call a malformed flow private on its own.
pub fn private_data_asked_in(with: &Value) -> bool {
    with.get("data").and_then(Value::as_str) == Some("private")
}

/// **WHY THIS STRUCT COLLECTS WHAT IT DOES NOT KNOW INSTEAD OF DROPPING IT.**
///
/// A flow that wrote `"prompt"` where `"stdin"` goes still started the step, the
/// engine got a truncated command line, and the error that came back was its
/// own: «Input must be provided either through stdin». A paid call spent on a
/// typo, with nobody able to say so beforehand. That is fault 20.
///
/// **`deny_unknown_fields` IS NOT THE ANSWER, AND WOULD BREAK EVERYTHING.** At
/// run time a step's input *is* its dependency's output with the `with` overlaid,
/// so every field the previous step produced arrives here. Refusing them would
/// make any step with a dependency impossible — the reason
/// `toolbox::needs::NeedsSpec` declares and refuses it.
///
/// Unrecognised fields land in `extra` and are ignored as before. The difference
/// is that now **they can be asked for**, and `flow check` asks them of the
/// `with` — hand-written text, where a stray field is nobody's output: a typo.
#[derive(Debug, Deserialize)]
pub(crate) struct EngineSpec {
    /// The command as written. It stays for an arbitrary command — `sh`, `cat`,
    /// a script — not for an engine: an engine is asked for by identifier, or
    /// the flow runs only where that name is on the caller's path.
    #[serde(default)]
    pub(crate) bin: Option<String>,
    /// The wanted tool's identifier — the same one the machine's detector
    /// returns — or a **chain** of identifiers to try in order.
    #[serde(default)]
    pub(crate) tool: Option<ToolChoice>,
    /// What the text of this step is: `private` never resolves to an engine
    /// whose data pact is `trains` or `unknown`. Absent is `public`.
    #[serde(default)]
    pub(crate) data: Option<DataClass>,
    /// The kind of work (`mechanical`, `research`, `implementation`,
    /// `judgement`, `writing`): the strengths table puts its engines first.
    #[serde(default)]
    pub(crate) kind: Option<String>,
    /// This step is given what it is handed and nothing else: no session of
    /// another step is continued, whatever it asks. Whoever writes the flow
    /// declares it — a step is not read as a judge by the words in it.
    #[serde(default)]
    pub(crate) blind: bool,
    /// `fuel`: among the chain, the engine whose subscription window would
    /// otherwise expire unused goes first, and the why is said.
    #[serde(default)]
    pub(crate) prefer: Option<String>,
    /// Which model is wanted of each engine of the chain, by engine id.
    ///
    /// **A MAP AND NOT ONE NAME.** A model's name is coined by its provider: as
    /// a string it would fit the engine it was written for and be noise to the
    /// others, tying the step back to the engine the chain exists to loosen. An
    /// engine of the chain with no entry runs on its own default.
    #[serde(default)]
    pub(crate) model: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) workdir: Option<String>,
    /// `"tree": "own"` gives this step a git worktree of the project to
    /// itself, named after the run and the step. Two steps of one front then
    /// write over each other's work only if somebody wrote the same name
    /// twice, which the graph does not allow.
    #[serde(default)]
    pub(crate) tree: Option<String>,
    /// The text of stdin, when the engine reads it from there instead of from
    /// an argument: JSON carries no raw bytes, so an engine wanting binary on
    /// stdin is not a case this action covers.
    #[serde(default)]
    pub(crate) stdin: Option<String>,
    /// The failure outcomes this step declares acceptable instead of red. Empty
    /// — the default — means every failure breaks the step.
    #[serde(default)]
    pub(crate) accept: Vec<String>,
    /// The shape this step demands of its own answer.
    ///
    /// **A CONTRACT, NOT ONE MORE CHECK.** Without it a step returns a block of
    /// free text and the next step fishes in it with a reference, hoping the
    /// shape holds: an engine that answers more verbosely one day breaks the
    /// chain in silence. With it the shape is written once, asked of the engine
    /// (it must appear in the prompt, and that is checked here) and enforced on
    /// the answer.
    ///
    /// **AND ONLY WHAT THE SHAPE DECLARES GETS THROUGH.** Preambles, reasoning
    /// and pleasantries stay out of the step's output: the next step receives
    /// the object pruned to the declared fields. That saving is collected on
    /// every downstream call, which is why the pruning happens even where the
    /// shape would tolerate extra fields.
    #[serde(default)]
    pub(crate) answer_shape: Option<ValueSchema>,
    /// The capabilities this step asks of the engine — `response_shape`,
    /// `resume_session`, any name a descriptor declares. Read by `flow check`
    /// alone, which warns before spending when the chosen engine lacks one;
    /// the run does not change, and asking in the prompt stays the fallback.
    /// Declared here so the check does not call an honest field a typo.
    #[allow(dead_code)]
    #[serde(default)]
    pub(crate) needs_capabilities: Vec<String>,
    /// What this step needs installed beside the engine, each `kind:name` as
    /// `sailor inventory` lists them (`skill:<name>`, `command:/<name>`).
    /// Read by `flow check` alone, which says per step what this machine has
    /// and lacks; an absence is a warning, because the step must then work
    /// worse, not silently. See fault 17.
    #[allow(dead_code)]
    #[serde(default)]
    pub(crate) needs_extensions: Vec<String>,
    /// Why the previous attempt's answer was refused, written by the executor
    /// when it retries the step. In a `with` it would tell the engine of a
    /// refusal that never happened.
    #[serde(default)]
    pub(crate) after_refusal: Option<Refusal>,
    /// Whether this step opens a session, resumes one, or forks one.
    ///
    /// Absent — the default — means the step opens a process that knows nothing
    /// of what has already been read: how it has always worked, and measured to
    /// cost 2.79 times a single prompt, because four steps rediscovered the same
    /// tree four times.
    #[serde(default)]
    pub(crate) session: Option<SessionUse>,
    /// The most this step's call may spend, in micro-units, for an engine that
    /// can be told a ceiling in currency. It is what a guaranteed cap is made
    /// of: without it the run's cap can only stop the call *after* this one.
    #[serde(default)]
    pub(crate) max_spend_micros: Option<i64>,
    /// The same ceiling for an engine that takes one in tokens. Two fields and
    /// not one converted: converting them here would need a tariff, and a
    /// ceiling derived from a tariff that may be missing is not a ceiling.
    #[serde(default)]
    pub(crate) max_tokens: Option<models::pricing::TokenCounts>,
    pub(crate) timeout_secs: u64,
    /// Everything this action does not recognise.
    ///
    /// At run time it is the dependency's output and is ignored; at check time,
    /// on the `with` alone, it is the list of typos. See the comment above the
    /// struct.
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

/// What the text of a step is, for the pact an engine must hold to read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DataClass {
    Private,
    Public,
}

/// The field a step declares to be given nothing it was not handed.
pub const BLIND: &str = "blind";

/// The field a step declares to work in a tree nobody else touches, and the
/// only word it takes.
pub const TREE: &str = "tree";
pub const A_TREE_OF_ITS_OWN: &str = "own";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A name written as a word and the same name written as a list of one
    /// are the same chain; a `tool` of any other shape names nobody.
    #[test]
    fn one_name_is_a_chain_of_one_and_a_list_keeps_its_order() {
        assert_eq!(engines_named_in(&json!({"tool": "uno"})), vec!["uno"]);
        assert_eq!(
            engines_named_in(&json!({"tool": ["uno", "due"]})),
            vec!["uno", "due"]
        );
        assert!(engines_named_in(&json!({"tool": 3})).is_empty());
        assert!(engines_named_in(&json!({"bin": "sh"})).is_empty());
    }

    /// **A CHECK RUNS BEFORE ANYTHING IS RESOLVED**, so a prompt is still the
    /// reference that will compose it. Every shipped step writes one, and each
    /// was told it declared no ceiling — a step held to a ceiling the check
    /// could not see is exactly what the ceiling exists to prevent.
    #[test]
    fn a_ceiling_is_read_from_a_with_whose_prompt_is_still_a_reference() {
        let with = json!({
            "tool": "uno",
            "timeout_secs": 900,
            "stdin": {"$join": ["do this: ", {"$from": "/trigger/text"}]},
            "max_spend_micros": 6_000_000,
        });
        assert_eq!(
            ceiling_declared_in(&with).max_spend_micros,
            Some(6_000_000),
            "the ceiling is a number beside the reference, not inside it"
        );
    }

    /// The reader of a partial `with` and the struct the run obeys are one
    /// declaration: where the whole spec parses, the two must agree, or a run
    /// obeys a ceiling nobody checked.
    #[test]
    fn the_check_reads_the_same_ceiling_the_run_obeys() {
        for with in [
            json!({"tool": "uno", "timeout_secs": 1, "max_spend_micros": 500_000}),
            json!({"tool": "uno", "timeout_secs": 1}),
            json!({"tool": "uno", "timeout_secs": 1, "max_tokens": {"input": 10, "output": 20}}),
        ] {
            let spec: EngineSpec =
                serde_json::from_value(with.clone()).expect("this `with` is whole");
            assert_eq!(ceiling_declared_in(&with), ceiling_of(&spec));
        }
    }

    /// **WHOEVER PRICES A RUN BEFORE IT RUNS READS THE STEP, NOT THE PAST.**
    /// An engine of the chain with no entry is absent from the map, and that
    /// absence is the fact a check has to say out loud. **The step still holds
    /// an unresolved reference**, which is how every step looks before it runs:
    /// a reader of the whole `EngineSpec` finds nothing here, and finds it
    /// silently.
    #[test]
    fn the_model_a_step_asks_of_each_engine_is_read_off_the_step() {
        let with = json!({
            "tool": ["uno", "due"],
            "model": {"uno": "modello-uno"},
            "stdin": {"$from": "/trigger/text"}
        });

        let named = models_named_in(&with);

        assert_eq!(named.get("uno").map(String::as_str), Some("modello-uno"));
        assert!(!named.contains_key("due"), "{named:?}");
        assert!(models_named_in(&json!({"tool": "uno"})).is_empty());
    }
}
