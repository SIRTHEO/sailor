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
    // Senza questa prova, la rappresentazione serializzata potrebbe non essere rileggibile o perdere passi.
    let graph =
        Graph::new(vec![step("fetch", &[]), step("publish", &["fetch"])]).expect("grafo valido");

    let json = serde_json::to_string(&graph).expect("grafo serializzabile");
    let decoded: Graph = serde_json::from_str(&json).expect("grafo rileggibile");

    assert_eq!(decoded, graph);
}

#[test]
fn step_values_round_trip_and_absent_values_stay_omitted() {
    // Il predecessore deve produrre un oggetto: fondere valori sopra un'uscita
    // scalare la farebbe sparire, e il grafo lo rifiuta apposta.
    let mut source = step("fetch", &[]);
    source.output_schema = ValueSchema::Any;
    let mut configured = step("send", &["fetch"]);
    configured.with = Some(serde_json::json!({"text": "/clear"}));
    let graph = Graph::new(vec![source, configured]).expect("grafo valido");

    let json = serde_json::to_string(&graph).expect("grafo serializzabile");
    let encoded: serde_json::Value = serde_json::from_str(&json).expect("JSON valido");
    let decoded: Graph = serde_json::from_str(&json).expect("grafo rileggibile");

    assert!(encoded["steps"][0].get("with").is_none());
    assert_eq!(
        encoded["steps"][1]["with"],
        serde_json::json!({"text": "/clear"})
    );
    assert_eq!(decoded, graph);
}

#[test]
fn cycle_in_json_is_rejected_while_loading() {
    // Senza questa prova, un ciclo dichiarato nel file potrebbe arrivare fino all'esecuzione.
    let json = graph_json(
        r#"
        {"id":"first","deps":["second"],"input_schema":{"type":"any"},"output_schema":{"type":"string"},"when":null,"action":"first","max_attempts":1},
        {"id":"second","deps":["first"],"input_schema":{"type":"any"},"output_schema":{"type":"string"},"when":null,"action":"second","max_attempts":1}
        "#,
    );

    let error = serde_json::from_str::<Graph>(&json).expect_err("il ciclo deve essere rifiutato");

    assert!(error.to_string().contains("backward dependency"), "{error}");
}

#[test]
fn missing_dependency_in_json_is_rejected_while_loading() {
    // Senza questa prova, un riferimento inesistente nel file potrebbe produrre un grafo incompleto.
    let json = graph_json(
        r#"
        {"id":"publish","deps":["missing"],"input_schema":{"type":"any"},"output_schema":{"type":"string"},"when":null,"action":"publish","max_attempts":1}
        "#,
    );

    let error = serde_json::from_str::<Graph>(&json)
        .expect_err("la dipendenza inesistente deve essere rifiutata");

    assert!(
        error.to_string().contains("depends on missing step"),
        "{error}"
    );
}

#[test]
fn zero_max_attempts_in_json_is_rejected_while_loading() {
    // Senza questa prova, un passo configurato per non tentare mai potrebbe essere accettato dal file.
    let json = graph_json(
        r#"
        {"id":"work","deps":[],"input_schema":{"type":"any"},"output_schema":{"type":"string"},"when":null,"action":"work","max_attempts":0}
        "#,
    );

    let error =
        serde_json::from_str::<Graph>(&json).expect_err("zero tentativi deve essere rifiutato");

    assert!(
        error.to_string().contains("allows zero attempts"),
        "{error}"
    );
}

#[test]
fn handwritten_json_with_skippable_dependency_builds_expected_graph() {
    // Senza questa prova, i grafi scritti da persone potrebbero perdere gli archi saltabili al caricamento.
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
    .expect("grafo atteso valido");

    let graph: Graph = serde_json::from_str(json).expect("JSON scritto a mano valido");

    assert_eq!(graph, expected);
    assert!(graph.dependency_is_skippable("publish", "fetch"));
}

fn graph_json(steps: &str) -> String {
    format!(r#"{{"steps":[{steps}]}}"#)
}
