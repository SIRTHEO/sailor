//! A flow started by a sensor: the beat reads, compares with what it kept, and
//! starts the flow only when the reading changed (ADR-024).

use flow::{Action, ActionError, ActionOutcome, ActionRegistry, FlowFile, SharedState};
use ledger::Ledger;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use trigger::sensor::{kept_for, read, refusal_of, sense, Eyes, Sensed, Sensor};

/// Reads whatever the test last put in its hand, and declares it only reads.
struct Reading(Arc<Mutex<Value>>);

impl Action for Reading {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(self.0.lock().expect("the reading").clone()))
    }
    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }
    fn only_reads(&self, _declared: Option<&Value>) -> bool {
        true
    }
}

struct Breaking;

impl Action for Breaking {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Err(ActionError::new("unreadable", "the thing watched is not there"))
    }
    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }
    fn only_reads(&self, _declared: Option<&Value>) -> bool {
        true
    }
}

struct Hanging;

impl Action for Hanging {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        std::thread::sleep(Duration::from_secs(2));
        Ok(ActionOutcome::Went(json!({"head": "late"})))
    }
    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }
    fn only_reads(&self, _declared: Option<&Value>) -> bool {
        true
    }
}

/// Reads and costs, like a question put to a model.
struct Spending;

impl Action for Spending {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!("an answer")))
    }
    fn only_reads(&self, _declared: Option<&Value>) -> bool {
        true
    }
}

/// Free, and says nothing about writing: the default refuses it.
struct Writing;

impl Action for Writing {
    fn execute(&self, _input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(json!({"written": true})))
    }
    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }
}

/// Hands back its own input, as far as it knows `workdir`.
struct Echoing;

impl Action for Echoing {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }
    fn only_reads(&self, _declared: Option<&Value>) -> bool {
        true
    }
}

/// The same, for an action whose input has no place for a `workdir`.
struct Closed;

impl Action for Closed {
    fn execute(&self, input: &Value, _shared: &SharedState) -> Result<ActionOutcome, ActionError> {
        Ok(ActionOutcome::Went(input.clone()))
    }
    fn unknown_fields(&self, declared: &Value) -> Vec<String> {
        declared
            .as_object()
            .map(|fields| fields.keys().filter(|key| *key != "collection").cloned().collect())
            .unwrap_or_default()
    }
    fn may_spend(&self, _declared: Option<&Value>) -> bool {
        false
    }
    fn only_reads(&self, _declared: Option<&Value>) -> bool {
        true
    }
}

struct World {
    ledger: Ledger,
    registry: Arc<ActionRegistry>,
    watched: Arc<Mutex<Value>>,
    started: Vec<String>,
    scratch: PathBuf,
}

impl World {
    fn new(name: &str) -> World {
        let scratch =
            std::env::temp_dir().join(format!("sailor-sensor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("a scratch directory");
        let watched = Arc::new(Mutex::new(json!({"head": "a1"})));
        let mut registry = ActionRegistry::default();
        registry.register("reading", Reading(Arc::clone(&watched)));
        registry.register("breaking", Breaking);
        registry.register("hanging", Hanging);
        registry.register("spending", Spending);
        registry.register("writing", Writing);
        registry.register("echoing", Echoing);
        registry.register("closed", Closed);
        World {
            ledger: Ledger::open(&scratch).expect("a scratch ledger"),
            registry: Arc::new(registry),
            watched,
            started: Vec::new(),
            scratch,
        }
    }

    fn watch(&self, value: Value) {
        *self.watched.lock().expect("the reading") = value;
    }

    fn eyes(&self, root: Option<&str>) -> Eyes {
        Eyes {
            registry: Arc::clone(&self.registry),
            root: root.map(PathBuf::from),
            timeout: Duration::from_millis(300),
        }
    }

    fn beat(&mut self, sensor: &Sensor, now: i64) -> Sensed {
        let eyes = self.eyes(None);
        let started = &mut self.started;
        sense(
            &self.ledger,
            "watching-flow",
            sensor,
            &eyes,
            now,
            &mut |text| {
                started.push(text.to_owned());
                Ok(format!("run {}", started.len()))
            },
        )
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

fn sensor(action: &str, cooldown_secs: u64) -> Sensor {
    Sensor {
        action: action.to_owned(),
        with: json!({}),
        pointer: Some("/head".to_owned()),
        cooldown_secs,
    }
}

#[test]
fn a_first_reading_is_kept_as_the_baseline_and_starts_nothing() {
    let mut world = World::new("baseline");
    let sensed = world.beat(&sensor("reading", 0), 1_000);

    assert_eq!(sensed.word, "hold", "{sensed:?}");
    assert!(world.started.is_empty(), "a first reading is not a change");
    let kept = kept_for(&world.ledger, "watching-flow")
        .expect("the store reads")
        .expect("the baseline is kept");
    assert_eq!(kept.value, json!("a1"));
    assert_eq!(kept.last_started_at, None);
}

#[test]
fn a_change_starts_one_run_that_carries_the_reading_before_and_after() {
    let mut world = World::new("change");
    world.beat(&sensor("reading", 0), 1_000);
    // Two changes between beats are one change: state is compared, not counted.
    world.watch(json!({"head": "b2"}));
    world.watch(json!({"head": "c3"}));
    let sensed = world.beat(&sensor("reading", 0), 1_060);

    assert_eq!(sensed.word, "ran", "{sensed:?}");
    assert_eq!(world.started.len(), 1, "one change, one run");
    let text: Value = serde_json::from_str(&world.started[0]).expect("the text is one JSON object");
    assert_eq!(text["before"], json!("a1"));
    assert_eq!(text["after"], json!("c3"));
    assert_eq!(text["fingerprint_before"], json!(flow::digest_input(&json!("a1"))));
    assert_eq!(text["fingerprint_after"], json!(flow::digest_input(&json!("c3"))));

    world.beat(&sensor("reading", 0), 1_120);
    assert_eq!(world.started.len(), 1, "the change was consumed by the run it started");
}

#[test]
fn a_reading_that_did_not_change_starts_nothing() {
    let mut world = World::new("unchanged");
    world.beat(&sensor("reading", 0), 1_000);
    let sensed = world.beat(&sensor("reading", 0), 1_060);

    assert_eq!(sensed.word, "hold", "{sensed:?}");
    assert!(world.started.is_empty());
}

#[test]
fn a_change_inside_the_cooldown_waits_and_fires_once_it_ends() {
    let mut world = World::new("cooldown");
    let cooling = sensor("reading", 300);
    world.beat(&cooling, 1_000);
    world.watch(json!({"head": "b2"}));
    world.beat(&cooling, 1_060);
    assert_eq!(world.started.len(), 1, "the first change starts at once");

    world.watch(json!({"head": "c3"}));
    let held = world.beat(&cooling, 1_100);
    assert_eq!(held.word, "hold", "{held:?}");
    assert_eq!(world.started.len(), 1, "inside the cooldown nothing starts");
    let kept = kept_for(&world.ledger, "watching-flow").expect("reads").expect("kept");
    assert_eq!(kept.value, json!("b2"), "and nothing is recorded, so the change is not lost");

    let fired = world.beat(&cooling, 1_360);
    assert_eq!(fired.word, "ran", "{fired:?}");
    assert_eq!(world.started.len(), 2);
    let text: Value = serde_json::from_str(&world.started[1]).expect("JSON");
    assert_eq!((text["before"].clone(), text["after"].clone()), (json!("b2"), json!("c3")));
}

#[test]
fn a_sensor_that_fails_or_hangs_starts_nothing_and_keeps_nothing() {
    let mut world = World::new("blind");
    let failed = world.beat(&sensor("breaking", 0), 1_000);
    assert_eq!(failed.word, "blind", "{failed:?}");
    assert!(failed.said.contains("the thing watched is not there"), "{failed:?}");

    let hung = world.beat(&sensor("hanging", 0), 1_060);
    assert_eq!(hung.word, "blind", "{hung:?}");
    assert!(hung.said.contains("did not answer within"), "{hung:?}");

    let lost = Sensor {
        pointer: Some("/nowhere".to_owned()),
        ..sensor("reading", 0)
    };
    assert_eq!(world.beat(&lost, 1_120).word, "blind");

    assert!(world.started.is_empty());
    assert_eq!(kept_for(&world.ledger, "watching-flow").expect("reads"), None);
}

fn flow_watching(sensor: Value) -> FlowFile {
    serde_json::from_value(json!({
        "id": "watching-flow",
        "description": "a flow started by what it watches",
        "graph": {"steps": [{
            "id": "trigger", "deps": [], "action": "trigger", "max_attempts": 1, "when": null,
            "with": {"source": "sensor", "sensor": sensor},
            "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
        }]},
        "inputs": {}
    }))
    .expect("a flow with a sensor trigger")
}

#[test]
fn the_check_refuses_a_sensor_that_writes_spends_or_is_unknown_and_accepts_one_that_reads() {
    let world = World::new("check");
    let refused = |declared: Value| refusal_of(&flow_watching(declared), &world.registry);

    assert_eq!(refused(json!({"action": "reading", "pointer": "/head"})), None);
    for (declared, reason) in [
        (json!({"action": "writing"}), "does not declare that it only reads"),
        (json!({"action": "spending"}), "may spend"),
        (json!({"action": "nobody-registers-this"}), "which nothing registers"),
        (json!({"action": "reading", "pointer": "head"}), "is not a JSON pointer"),
        (json!({"action": "reading", "cooldown_secs": -5}), "zero or more"),
    ] {
        let said = refused(declared.clone())
            .unwrap_or_else(|| panic!("{declared} must be refused"));
        assert!(said.contains("watching-flow"), "the flow is named: {said}");
        assert!(said.contains(reason), "{declared}: {said}");
    }
}

#[test]
fn the_trigger_step_of_a_sensor_flow_hands_the_change_downstream_as_a_record() {
    let text = json!({"before": "a1", "after": "b2", "fingerprint_before": "x", "fingerprint_after": "y"});
    let input = json!({
        "source": "sensor",
        "sensor": {"action": "reading"},
        "text": text.to_string()
    });
    let outcome = trigger::TriggerAction
        .execute(&input, &SharedState::new())
        .expect("the shipped sensor source carries its signal");
    let ActionOutcome::Went(signal) = outcome else {
        panic!("a sensor's trigger does not wait: {outcome:?}");
    };
    assert_eq!(signal["kind"], "sensor");
    assert_eq!(signal["carried"]["after"], "b2");
}

#[test]
fn a_sensor_works_where_a_step_of_its_flow_would() {
    let world = World::new("workdir");
    let eyes = world.eyes(Some("/a/project"));
    let echo = |action: &str, with: Value| {
        let sensor = Sensor {
            action: action.to_owned(),
            with,
            pointer: None,
            cooldown_secs: 0,
        };
        read(&sensor, &eyes)
    };

    let absent = echo("echoing", json!({})).expect("it reads");
    assert_eq!(absent["workdir"], "/a/project", "an absent workdir is the root");
    let relative = echo("echoing", json!({"workdir": "crates/flow"})).expect("it reads");
    assert_eq!(relative["workdir"], "/a/project/crates/flow");
    let refused = echo("echoing", json!({"workdir": "/elsewhere"})).expect_err("absolute");
    assert!(refused.contains("/elsewhere"), "{refused}");
    let closed = echo("closed", json!({"collection": "c"})).expect("it reads");
    assert!(closed.get("workdir").is_none(), "no field its action never asked for: {closed}");
}
