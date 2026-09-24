use super::*;
use serde_json::json;

fn flow(extra: Value) -> FlowFile {
    let mut document = json!({
        "id": "f",
        "description": "a flow",
        "graph": {"steps": []},
        "inputs": {}
    });
    if let (Some(fields), Value::Object(more)) = (document.as_object_mut(), extra) {
        fields.extend(more);
    }
    serde_json::from_value(document).expect("a flow with no steps loads")
}

/// The first shipped flow, by the name and version it ships with.
fn a_shipped_flow() -> (&'static str, u32) {
    let (name, _) = FLOWS[0];
    let version = shipped_version(name).expect("every shipped flow says its version");
    (name, version)
}

#[test]
fn a_copy_of_an_older_version_is_stale_and_one_of_the_current_is_not() {
    let shipped = flow(json!({"version": 3}));

    assert_eq!(
        stale(&flow(json!({"replaces": 2})), &shipped),
        Some(Stale {
            shipped: 3,
            copied: Some(2)
        })
    );
    assert_eq!(stale(&flow(json!({"replaces": 3})), &shipped), None);
    assert_eq!(
        stale(&flow(json!({"version": 3})), &shipped),
        None,
        "a copy made by hand still carries the version it was copied from"
    );
    assert_eq!(
        stale(&flow(json!({})), &shipped),
        Some(Stale {
            shipped: 3,
            copied: None
        }),
        "a copy that never said what it replaces cannot be told current"
    );
    assert_eq!(
        stale(&flow(json!({})), &flow(json!({}))),
        None,
        "a shipped flow with no version gives nothing to compare"
    );
}

#[test]
fn a_saved_copy_says_which_version_it_replaces_and_carries_no_version_of_its_own() {
    let (name, version) = a_shipped_flow();

    let mut copied_by_hand = json!({"id": name, "version": version});
    stamp_the_copy(&mut copied_by_hand, None);
    assert_eq!(copied_by_hand, json!({"id": name, "replaces": version}));

    let mut written_anew = json!({"id": name});
    stamp_the_copy(&mut written_anew, None);
    assert_eq!(written_anew, json!({"id": name, "replaces": version}));

    let mut rewritten_in_place = json!({"id": name});
    stamp_the_copy(&mut rewritten_in_place, Some(&json!({"id": name})));
    assert_eq!(
        rewritten_in_place,
        json!({"id": name}),
        "a copy that never said what it replaces is not told it is current"
    );
    let mut rewritten_in_place = json!({"id": name});
    stamp_the_copy(&mut rewritten_in_place, Some(&json!({"id": name, "replaces": 0})));
    assert_eq!(
        rewritten_in_place,
        json!({"id": name, "replaces": 0}),
        "what the copy on disk said is kept when the rewrite drops it"
    );

    let mut already_said = json!({"id": name, "replaces": 0, "version": version});
    stamp_the_copy(&mut already_said, None);
    assert_eq!(
        already_said,
        json!({"id": name, "replaces": 0}),
        "what a copy declares it replaces is kept, even when older"
    );
    let again = already_said.clone();
    stamp_the_copy(&mut already_said, None);
    assert_eq!(already_said, again, "saving twice stamps the same number");
}

#[test]
fn a_flow_no_product_ships_is_left_as_written() {
    let mut own = json!({"id": "a-flow-of-a-persons-own", "version": 4});
    stamp_the_copy(&mut own, None);
    assert_eq!(own, json!({"id": "a-flow-of-a-persons-own", "version": 4}));
}

/// A shipped flow says which version it is and replaces nothing: without
/// the first, a copy of it can never be told stale.
#[test]
fn every_shipped_flow_says_its_version_and_replaces_nothing() {
    for (name, text) in FLOWS {
        let shipped: FlowFile = serde_json::from_str(text).expect("a shipped flow loads");
        assert!(
            shipped.version.is_some_and(|version| version >= 1),
            "{name} declares no version"
        );
        assert_eq!(
            shipped.replaces, None,
            "{name} is shipped, it replaces nothing"
        );
    }
}

/// Read the way every list reads it: a copy on disk of an older version is
/// marked on its chain, a current one is not, and neither field counts as
/// a change to the flow it replaces.
#[test]
fn the_chain_of_a_copy_says_when_the_shipped_flow_rose_past_it() {
    use crate::system::{resolve, what_yours_changes, FlowSource};
    let (name, version) = a_shipped_flow();
    let dir = std::env::temp_dir().join(format!("flow-versions-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a folder of flows");
    let (_, text) = FLOWS[0];
    let shipped: Value = serde_json::from_str(text).expect("a shipped flow parses");
    let sources = [
        FlowSource::builtin(),
        FlowSource {
            origin: "yours",
            dir: dir.clone(),
        },
    ];
    let chain_with = |replaces: u32| {
        let mut copy = shipped.clone();
        copy["replaces"] = json!(replaces);
        copy.as_object_mut().expect("an object").remove("version");
        std::fs::write(
            dir.join(format!("{name}.flow.json")),
            serde_json::to_string(&copy).expect("serialises"),
        )
        .expect("the copy is written");
        resolve(&sources, None)
            .into_iter()
            .find(|resolved| resolved.chain.name == name)
            .expect("the name resolves")
    };

    let older = chain_with(version - 1);
    assert_eq!(
        older.chain.stale,
        Some(Stale {
            shipped: version,
            copied: Some(version - 1)
        })
    );
    let current = chain_with(version);
    assert_eq!(current.chain.stale, None);
    let yours = current.entry.expect("the copy loads");
    let theirs: FlowFile = serde_json::from_value(shipped).expect("loads");
    assert!(what_yours_changes(&yours, &theirs).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Through the one door every writer uses: the window saving a shipped flow
/// it loaded writes a copy that says what it replaces, and no version.
#[test]
fn saving_a_shipped_flow_writes_a_copy_stamped_with_the_version_it_replaces() {
    let (name, version) = a_shipped_flow();
    let dir = std::env::temp_dir().join(format!("flow-stamped-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (_, text) = FLOWS[0];
    let loaded: Value = serde_json::from_str(text).expect("a shipped flow parses");

    crate::system::save_document_in(&dir, &loaded).expect("the copy is saved");

    let written: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(format!("{name}.flow.json"))).expect("the copy reads"),
    )
    .expect("the copy parses");
    assert_eq!(written.get("replaces"), Some(&json!(version)), "{written}");
    assert_eq!(written.get("version"), None, "{written}");
    let _ = std::fs::remove_dir_all(&dir);
}
