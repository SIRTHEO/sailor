//! The trigger as a flow step.
//!
//! **WHAT IT REALLY DOES.** It reads which source the step declared, looks it
//! up in the descriptor list and — if that source carries the signal with it —
//! returns the signal in the shape the steps downstream read. It executes
//! nothing and touches nothing in the world.

use crate::{default_sources, Catalog, Kind, Listen, Signal, Source, TriggerDescriptor};
use flow::{Action, ActionError, ActionOutcome, SharedState, StepSpecies};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use toolbox::Machine;

/// The name the action registers under.
pub const TRIGGER_ACTION: &str = "trigger";

pub fn register_default(registry: &mut flow::ActionRegistry) {
    registry.register(TRIGGER_ACTION, TriggerAction);
}

#[derive(Debug, Deserialize)]
struct TriggerSpec {
    /// The source descriptor's `id`. Required: "where the work comes from" has
    /// no reasonable default.
    source: String,
    /// The delivery text, for a source that carries it.
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    who: Option<String>,
    #[serde(default, rename = "where")]
    where_from: Option<String>,
    /// Descriptor files or directories to use beyond the usual ones.
    #[serde(default)]
    descriptor_paths: Vec<String>,
    /// Whether to add to the usual ones or replace them.
    #[serde(default = "yes")]
    include_defaults: bool,
}

fn yes() -> bool {
    true
}

/// A flow's entry node: it waits for a signal and offers it downstream.
pub struct TriggerAction;

impl Action for TriggerAction {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        let spec: TriggerSpec = serde_json::from_value(input.clone())
            .map_err(|error| ActionError::new("invalid_input", error.to_string()))?;
        let machine = Machine::current();
        let mut sources: Vec<Source> = if spec.include_defaults {
            default_sources(&machine)
        } else {
            Vec::new()
        };
        for raw in &spec.descriptor_paths {
            let path = PathBuf::from(machine.expand(raw));
            if path.is_dir() {
                sources.push(Source::Dir(path));
            } else {
                sources.push(Source::File(path));
            }
        }
        let catalog = Catalog::load(&sources);
        let Some(loaded) = catalog.find(&spec.source) else {
            // A list that does not say what it holds forces a hunt through the
            // files to find out how the right line is written.
            let known = catalog.known();
            let known = if known.is_empty() {
                "no source is switched on".to_owned()
            } else {
                format!("the sources switched on are: {}", known.join(", "))
            };
            let mut said = format!(
                "the step asks for the signal source «{}», which no descriptor declares; {known}",
                spec.source
            );
            for problem in &catalog.problems {
                said.push_str(&format!(
                    "\n(a descriptor did not load: {} in {} — {})",
                    problem.about, problem.source, problem.reason
                ));
            }
            return Err(ActionError::new("unknown_trigger_source", said));
        };
        let descriptor = &loaded.descriptor;
        match descriptor.kind {
            Kind::Manual => {
                let text = spec.text.ok_or_else(|| {
                    ActionError::new(
                        "empty_signal",
                        format!(
                            "the trigger «{}» carries the signal with it, but the step gave it no `text`: whoever launches has to put the delivery there",
                            descriptor.id
                        ),
                    )
                })?;
                let signal = Signal {
                    text,
                    who: spec.who.unwrap_or_default(),
                    where_from: spec.where_from.unwrap_or_default(),
                    source: descriptor.id.clone(),
                    kind: "manual".to_owned(),
                };
                Ok(ActionOutcome::Went(
                    serde_json::to_value(signal)
                        .expect("a signal of plain texts always serialises"),
                ))
            }
            // **WHERE IT STOPS, AND WHY IT STOPS INSTEAD OF PRETENDING.** A
            // terminal source is not listened to: the step breaks with a
            // message saying what is missing, because anything returned here
            // would be invented. A green flow would say somebody spoke when
            // nobody did — the worst defect possible here, since it costs real
            // calls downstream.
            Kind::Terminal => Err(ActionError::new(
                "listening_not_built",
                not_listening_yet(descriptor),
            )),
        }
    }

    /// Redoing a manual trigger is safe: it reshapes what it was given and
    /// touches nothing. The day a trigger *consumes* a signal — taking it off a
    /// queue — the species changes, because redoing it would skip a delivery.
    fn species(&self) -> StepSpecies {
        StepSpecies::Repeatable
    }
}

/// The border, written out inside the message: whoever reads it needs to know
/// what to build, not only that something is missing.
///
/// The two missing things are named because neither is in this action: no
/// Sailor process stays up waiting for a signal, and no reader keeps a cursor
/// on a session.
fn not_listening_yet(descriptor: &TriggerDescriptor) -> String {
    let where_it_would_look = match &descriptor.listen {
        Some(Listen::AppendedLines { files, .. }) => {
            format!("it would watch the new lines of {}", files.join(", "))
        }
        Some(Listen::CursorCommand {
            tool,
            args,
            cursor_argument,
        }) => format!(
            "it would call the tool «{tool}» with {} and the cursor in {cursor_argument}",
            args.join(" ")
        ),
        // Loading prevents this; reaching it means the fault is there.
        None => "it does not declare where to look".to_owned(),
    };
    let missing_reader = match &descriptor.listen {
        Some(Listen::AppendedLines { files, .. }) => format!(
            "a reader that keeps a cursor on {} and recognises a new line without losing what appeared while nobody was watching",
            files.join(", ")
        ),
        Some(Listen::CursorCommand { tool, .. }) => format!(
            "a reader that invokes «{tool}» and keeps the point already read between one run and the next",
        ),
        None => "a reader".to_owned(),
    };
    let mut said = format!(
        "the trigger «{}» listens to a terminal, and Sailor cannot listen yet: {where_it_would_look}. \
         Two things are missing before it becomes real, and neither belongs in this step: \
         (1) a process that stays up — `sailor flow run` walks the graph once and ends, \
         so nobody waits for a signal and starts a run when it arrives; \
         (2) {missing_reader}. \
         Until then the only source that works is the manual one, which carries the signal with it.",
        descriptor.id
    );
    if !descriptor.note.is_empty() {
        said.push_str(&format!(" Descriptor note: {}", descriptor.note));
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fire(input: Value) -> Result<Value, ActionError> {
        match TriggerAction.execute(&input, &mut SharedState::new())? {
            ActionOutcome::Went(output) => Ok(output),
            ActionOutcome::Waiting(reason) => panic!("no trigger stays waiting: {reason}"),
        }
    }

    /// **THE ENTRY NODE IS REAL.** The text the signal carried comes out in the
    /// field the steps downstream read, with who sent it and from where.
    #[test]
    fn a_manual_signal_hands_down_what_it_carried() {
        let output = fire(json!({
            "source": "manual",
            "text": "trova i residui di configurazione",
            "who": "someone",
            "where": "la finestra"
        }))
        .expect("the manual trigger ships with the product");

        assert_eq!(output["text"], "trova i residui di configurazione");
        assert_eq!(output["who"], "someone");
        assert_eq!(output["where"], "la finestra");
        assert_eq!(output["source"], "manual");
        assert_eq!(output["kind"], "manual");
    }

    /// Fields the source does not know stay empty texts, never absent: the next
    /// step joins text with `$join`, and an absent value would break it instead
    /// of the trigger.
    #[test]
    fn a_signal_that_does_not_know_who_sent_it_still_answers_with_texts() {
        let output = fire(json!({"source": "manual", "text": "vai"})).expect("the text is enough");

        assert_eq!(output["who"], "");
        assert_eq!(output["where"], "");
        assert!(output["who"].is_string() && output["where"].is_string());
    }

    #[test]
    fn a_manual_trigger_without_a_text_says_who_should_have_given_it() {
        let error = fire(json!({"source": "manual"})).expect_err("an empty signal is not a signal");
        assert_eq!(error.class, "empty_signal");
        assert!(error.said.contains("text"), "{}", error.said);
    }

    /// **THE BORDER, TESTED.** A terminal source does not answer with a fake
    /// signal: it breaks the step and says what is missing. The mutant that
    /// fells this test is returning an empty signal instead of the error — the
    /// exact defect it guards, since an empty signal starts the engines
    /// downstream and costs real calls.
    #[test]
    fn a_terminal_source_refuses_to_pretend_it_listened() {
        let error =
            fire(json!({"source": "sailor-terminal"})).expect_err("nobody listens to a terminal");

        assert_eq!(error.class, "listening_not_built");
        assert!(error.said.contains("stays up"), "{}", error.said);
        assert!(error.said.contains("cursor"), "{}", error.said);
    }

    /// The two terminal entries on this machine are a list, not a branch of
    /// code: both behave the same way, and each carries its own place inside
    /// the message.
    #[test]
    fn every_shipped_terminal_source_stops_at_the_same_border() {
        let catalog = Catalog::load(&[Source::Builtin]);
        let terminals: Vec<String> = catalog
            .live()
            .into_iter()
            .filter(|loaded| loaded.descriptor.kind == Kind::Terminal)
            .map(|loaded| loaded.descriptor.id.clone())
            .collect();
        assert_eq!(
            terminals.len(),
            2,
            "the two entries measured: {terminals:?}"
        );
        for id in terminals {
            let error = fire(json!({"source": id})).expect_err("neither one listens");
            assert_eq!(error.class, "listening_not_built");
            assert!(error.said.contains(&id), "{}", error.said);
        }
    }

    #[test]
    fn an_unknown_source_lists_the_ones_that_exist() {
        let error = fire(json!({"source": "il-citofono", "text": "x"}))
            .expect_err("no descriptor declares it");

        assert_eq!(error.class, "unknown_trigger_source");
        assert!(error.said.contains("il-citofono"), "{}", error.said);
        assert!(error.said.contains("manual"), "{}", error.said);
    }

    /// **WHOEVER USES SAILOR FILLS THE LIST DIFFERENTLY**, and recompiles
    /// nothing: a descriptor file of their own, and their source exists.
    #[test]
    fn a_source_declared_by_the_user_works_without_recompiling() {
        let dir = std::env::temp_dir().join(format!("sailor-trigger-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("creating the test directory");
        let file = dir.join("miei.json");
        std::fs::write(
            &file,
            r#"[{"id": "il-citofono", "kind": "manual", "label": "Il citofono"}]"#,
        )
        .expect("writing the test descriptors");

        let output = fire(json!({
            "source": "il-citofono",
            "text": "aprimi",
            "descriptor_paths": [file.to_string_lossy()],
            "include_defaults": false
        }))
        .expect("the user-declared source exists");

        assert_eq!(output["source"], "il-citofono");
        assert_eq!(output["text"], "aprimi");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
