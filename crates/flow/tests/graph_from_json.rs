use flow::{DependencyEdge, Graph, Step, ValueSchema};

fn step(id: &str, deps: &[&str]) -> Step {
    Step {
        id: id.to_owned(),
        deps: deps
            .iter()
            .map(|dependency| (*dependency).to_owned())
            .collect(),
        input_schema: ValueSchema::Any,
        output_schema: ValueSchema::String,
        with: None,
        when: None,
        action: id.to_owned(),
        max_attempts: 1,
    }
}

#[test]
fn graph_round_trips_through_json() {
    // Without this, the serialised form could stop reading back, or lose steps.
    let graph =
        Graph::new(vec![step("fetch", &[]), step("publish", &["fetch"])]).expect("valid graph");

    let json = serde_json::to_string(&graph).expect("serialisable graph");
    let decoded: Graph = serde_json::from_str(&json).expect("graph reads back");

    assert_eq!(decoded, graph);
}

#[test]
fn step_values_round_trip_and_absent_values_stay_omitted() {
    // The predecessor must produce an object: merging values over a scalar
    // output would erase it, and the graph refuses that on purpose.
    let mut source = step("fetch", &[]);
    source.output_schema = ValueSchema::Any;
    let mut configured = step("send", &["fetch"]);
    configured.with = Some(serde_json::json!({"text": "/clear"}));
    let graph = Graph::new(vec![source, configured]).expect("valid graph");

    let json = serde_json::to_string(&graph).expect("serialisable graph");
    let encoded: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let decoded: Graph = serde_json::from_str(&json).expect("graph reads back");

    assert!(encoded["steps"][0].get("with").is_none());
    assert_eq!(
        encoded["steps"][1]["with"],
        serde_json::json!({"text": "/clear"})
    );
    assert_eq!(decoded, graph);
}

#[test]
fn cycle_in_json_is_rejected_while_loading() {
    // Without this, a cycle declared in the file could reach execution.
    let json = graph_json(
        r#"
        {"id":"first","deps":["second"],"input_schema":{"type":"any"},"output_schema":{"type":"string"},"when":null,"action":"first","max_attempts":1},
        {"id":"second","deps":["first"],"input_schema":{"type":"any"},"output_schema":{"type":"string"},"when":null,"action":"second","max_attempts":1}
        "#,
    );

    let error = serde_json::from_str::<Graph>(&json).expect_err("the cycle must be refused");

    assert!(error.to_string().contains("backward dependency"), "{error}");
}

#[test]
fn missing_dependency_in_json_is_rejected_while_loading() {
    // Without this, a dangling reference in the file could build a partial graph.
    let json = graph_json(
        r#"
        {"id":"publish","deps":["missing"],"input_schema":{"type":"any"},"output_schema":{"type":"string"},"when":null,"action":"publish","max_attempts":1}
        "#,
    );

    let error = serde_json::from_str::<Graph>(&json)
        .expect_err("the missing dependency must be refused");

    assert!(
        error.to_string().contains("depends on missing step"),
        "{error}"
    );
}

#[test]
fn zero_max_attempts_in_json_is_rejected_while_loading() {
    // Without this, a step configured never to try could be accepted from the file.
    let json = graph_json(
        r#"
        {"id":"work","deps":[],"input_schema":{"type":"any"},"output_schema":{"type":"string"},"when":null,"action":"work","max_attempts":0}
        "#,
    );

    let error =
        serde_json::from_str::<Graph>(&json).expect_err("zero attempts must be refused");

    assert!(
        error.to_string().contains("allows zero attempts"),
        "{error}"
    );
}

#[test]
fn handwritten_json_with_skippable_dependency_builds_expected_graph() {
    // Without this, hand-written graphs could lose skippable edges at load.
    let json = r#"
    {
      "steps": [
        {
          "id": "fetch",
          "deps": [],
          "input_schema": { "type": "any" },
          "output_schema": { "type": "string" },
          "when": null,
          "action": "fetch-url",
          "max_attempts": 2
        },
        {
          "id": "publish",
          "deps": ["fetch"],
          "input_schema": { "type": "any" },
          "output_schema": { "type": "string" },
          "when": null,
          "action": "publish-text",
          "max_attempts": 3
        }
      ],
      "skippable_dependencies": [
        { "step": "publish", "dependency": "fetch" }
      ]
    }
    "#;
    let expected = Graph::with_skippable_dependencies(
        vec![
            Step {
                action: "fetch-url".to_owned(),
                max_attempts: 2,
                ..step("fetch", &[])
            },
            Step {
                action: "publish-text".to_owned(),
                max_attempts: 3,
                ..step("publish", &["fetch"])
            },
        ],
        [DependencyEdge::new("publish", "fetch")],
    )
    .expect("the expected graph is valid");

    let graph: Graph = serde_json::from_str(json).expect("valid hand-written JSON");

    assert_eq!(graph, expected);
    assert!(graph.dependency_is_skippable("publish", "fetch"));
}

fn graph_json(steps: &str) -> String {
    format!(r#"{{"steps":[{steps}]}}"#)
}
