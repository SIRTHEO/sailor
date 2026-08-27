use flow::{Condition, Graph, Step, ValueSchema};
use serde_json::json;

fn strings() -> ValueSchema {
    ValueSchema::Array {
        items: Box::new(ValueSchema::String),
    }
}

fn object(properties: impl IntoIterator<Item = (&'static str, ValueSchema)>) -> ValueSchema {
    let properties: Vec<_> = properties
        .into_iter()
        .map(|(name, schema)| (name.to_owned(), schema))
        .collect();
    let required = properties
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    ValueSchema::object(properties, required)
}

fn config() -> ValueSchema {
    object([
        ("state_dir", ValueSchema::String),
        ("deleting", ValueSchema::Boolean),
    ])
}

fn target() -> ValueSchema {
    object([
        ("name", ValueSchema::String),
        ("kind", ValueSchema::String),
        ("session", ValueSchema::String),
        ("liveness", ValueSchema::String),
    ])
}

fn classified_marker() -> ValueSchema {
    object([
        ("name", ValueSchema::String),
        ("verdict", ValueSchema::String),
    ])
}

fn classification(legacy: bool) -> ValueSchema {
    let mut fields = vec![
        ("config", config()),
        ("looked", strings()),
        (
            "classified",
            ValueSchema::Array {
                items: Box::new(classified_marker()),
            },
        ),
        (
            "condemned",
            ValueSchema::Array {
                items: Box::new(target()),
            },
        ),
    ];
    if legacy {
        fields.push(("live_ids", strings()));
    }
    object(fields)
}

fn standard_marker() -> ValueSchema {
    object([
        ("name", ValueSchema::String),
        ("session", ValueSchema::String),
        ("liveness", ValueSchema::String),
        ("age_secs", ValueSchema::Number),
    ])
}

fn legacy_marker() -> ValueSchema {
    object([
        ("name", ValueSchema::String),
        ("hex", ValueSchema::String),
        ("path", ValueSchema::String),
        ("path_known", ValueSchema::Boolean),
        ("age_secs", ValueSchema::Number),
    ])
}

fn scan() -> ValueSchema {
    object([
        ("config", config()),
        (
            "standard",
            ValueSchema::Array {
                items: Box::new(standard_marker()),
            },
        ),
        (
            "legacy",
            ValueSchema::Array {
                items: Box::new(legacy_marker()),
            },
        ),
    ])
}

fn live_sessions() -> ValueSchema {
    object([
        ("ids", strings()),
        ("unreadable", ValueSchema::Number),
        ("observed", ValueSchema::Number),
    ])
}

fn plan() -> ValueSchema {
    object([
        ("state_dir", ValueSchema::String),
        ("deleting", ValueSchema::Boolean),
        ("looked", strings()),
        ("orphan", strings()),
        (
            "targets",
            ValueSchema::Array {
                items: Box::new(target()),
            },
        ),
    ])
}

fn trace() -> ValueSchema {
    object([
        ("looked", strings()),
        ("orphan", strings()),
        ("removed", strings()),
        ("spared", strings()),
        ("vanished", strings()),
        ("remove_failed", strings()),
        ("recovered", ValueSchema::Boolean),
    ])
}

fn step(id: &str, deps: &[&str], input: ValueSchema, output: ValueSchema) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps.iter().map(|value| (*value).to_owned()).collect(),
        input_schema: input,
        output_schema: output,
        when: None,
        action: id.to_owned(),
        max_attempts: 1,
    }
}

pub fn sweep_graph() -> Graph {
    let mut remove = step("remove_markers", &["plan_removals"], plan(), trace());
    remove.when = Some(Condition::PointerEquals {
        pointer: "/deleting".to_owned(),
        value: json!(true),
    });
    Graph::new(vec![
        step("scan_markers", &[], config(), scan()),
        step("read_live_sessions", &[], config(), live_sessions()),
        step(
            "classify_standard",
            &["scan_markers"],
            scan(),
            classification(false),
        ),
        step(
            "classify_legacy",
            &["scan_markers", "read_live_sessions"],
            object([
                ("scan_markers", scan()),
                ("read_live_sessions", live_sessions()),
            ]),
            classification(true),
        ),
        step(
            "plan_removals",
            &["classify_standard", "classify_legacy"],
            object([
                ("classify_standard", classification(false)),
                ("classify_legacy", classification(true)),
            ]),
            plan(),
        ),
        remove,
    ])
    .expect("il grafo statico di sweep deve essere valido")
}
