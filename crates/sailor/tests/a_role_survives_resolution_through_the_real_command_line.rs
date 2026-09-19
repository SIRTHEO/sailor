//! `sailor flow run` resolves every step's `role` into a `tool` before the
//! graph executes; an `InProcessExecutor` built by hand skips that pass, so
//! only the real binary can prove what reaches the ledger.

use serde_json::json;
use std::path::PathBuf;
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sailor-role-survives-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    dir
}

/// A descriptor naming one engine, `fake-cheap-worker`, that never leaves this
/// test's own directory: `SAILOR_TOOL_DESCRIPTORS` adds it beside the shipped
/// ones without touching `~/.config/sailor/tools.d`, which is read-only here.
fn write_descriptor(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("descriptors.json");
    std::fs::write(
        &path,
        r#"[{
            "id": "fake-cheap-worker",
            "family": "ai_cli",
            "label": "Fake Cheap Worker",
            "detect": {"command": "fake-cheap-worker"},
            "ask": {"args": [], "prompt": "stdin"}
        }]"#,
    )
    .expect("write the test descriptor");
    path
}

/// The engine itself: a shell script on the test's own `PATH`, answering
/// something without spending anything real.
fn write_fake_engine(dir: &std::path::Path) {
    let path = dir.join("fake-cheap-worker");
    std::fs::write(&path, "#!/bin/sh\ncat > /dev/null\nprintf 'answered'\n")
        .expect("write the fake engine");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make it executable");
    }
}

/// One step, one role, nothing else: the smallest flow that can show whether
/// `role` and `role_resolved_to` reach the ledger through the real command.
fn write_flow(dir: &std::path::Path) {
    let flows = dir.join("flows-declared");
    std::fs::create_dir_all(&flows).expect("the declared flows directory");
    std::fs::write(
        flows.join("role-survives-the-real-command-line.flow.json"),
        r#"{
            "id": "role-survives-the-real-command-line",
            "description": "one role, one step, run through the built binary",
            "graph": {
                "steps": [{
                    "id": "asked",
                    "deps": [],
                    "action": "external_engine",
                    "max_attempts": 1,
                    "when": null,
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"},
                    "with": {"role": "TEST_ROLE", "stdin": "hello", "timeout_secs": 10}
                }]
            },
            "inputs": {}
        }"#,
    )
    .expect("write the test flow");
}

#[test]
fn a_role_asked_of_the_real_binary_is_named_beside_what_it_resolved_to() {
    let dir = scratch("role-survives");
    let ledger_dir = dir.join("ledger");
    let home_dir = dir.join("home");
    std::fs::create_dir_all(&home_dir).expect("the fake home");
    write_descriptor(&dir);
    write_fake_engine(&dir);
    write_flow(&dir);

    {
        let ledger = ledger::Ledger::open(&ledger_dir).expect("open the ledger");
        ledger
            .put_record(&ledger::StoreRecord {
                collection: "roles".to_owned(),
                key: "TEST_ROLE".to_owned(),
                value: json!({"tools": ["fake-cheap-worker"]}),
                written_by: "test".to_owned(),
                written_at: 0,
            })
            .expect("the role is written before the run reads it");
    }

    let path_with_the_fake_engine =
        format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new(env!("CARGO_BIN_EXE_sailor"))
        .args(["flow", "run", "role-survives-the-real-command-line"])
        .env("SAILOR_LEDGER", &ledger_dir)
        .env("SAILOR_HOME", &home_dir)
        .env("SAILOR_FLOWS", dir.join("flows-declared"))
        .env("SAILOR_TOOL_DESCRIPTORS", dir.join("descriptors.json"))
        .env("PATH", &path_with_the_fake_engine)
        .output()
        .expect("the built binary runs");
    assert!(
        output.status.success(),
        "the run itself must succeed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let ledger = ledger::Ledger::open(&ledger_dir).expect("reopen the ledger");
    let answer = ledger
        .browse("SELECT role, role_resolved_to FROM model_calls", 10)
        .expect("model_calls reads");
    let role_at = answer
        .columns
        .iter()
        .position(|column| column == "role")
        .expect("a role column");
    let resolved_at = answer
        .columns
        .iter()
        .position(|column| column == "role_resolved_to")
        .expect("a role_resolved_to column");
    assert_eq!(answer.rows.len(), 1, "one call, one row: {:?}", answer.rows);
    assert_eq!(
        answer.rows[0][role_at],
        json!("TEST_ROLE"),
        "the role the step asked for must survive the real command line: {:?}",
        answer.rows[0]
    );
    let resolved_to = answer.rows[0][resolved_at].to_string();
    assert!(
        resolved_to.contains("fake-cheap-worker"),
        "the tool the role became must be named, not NULL: {resolved_to}"
    );
}
