//! Fixtures the tests of every `flow_cmd` module share.

use flow::Clock;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub(super) struct TestDirectory(pub(super) PathBuf);

impl TestDirectory {
    pub(super) fn new() -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("sailor-flow-test-{}-{serial}", std::process::id()));
        fs::create_dir(&path).expect("creare la cartella di prova");
        Self(path)
    }

    pub(super) fn write(&self, name: &str, contents: &str) {
        fs::write(self.0.join(name), contents).expect("scrivere il flusso di prova");
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Un orologio finto che avanza di uno a ogni domanda. Il contatore è
/// atomico perché l'orologio ora è condiviso fra i fili di un fronte: un
/// `i64` mutabile qui non compilerebbe, ed è la stessa ragione per cui il
/// tratto chiede `&self`.
pub(super) struct Tick(std::sync::atomic::AtomicI64);

impl Tick {
    pub(super) fn new(start: i64) -> Self {
        Tick(std::sync::atomic::AtomicI64::new(start))
    }
}

impl Clock for Tick {
    fn now(&self) -> Result<i64, flow::FlowError> {
        Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

pub(super) fn flow_json(action: &str, dependencies: &str, inputs: &str) -> String {
    format!(
        r#"{{
            "id": "prova",
            "description": "flusso di prova",
            "graph": {{
                "steps": [{{
                    "id": "root",
                    "deps": {dependencies},
                    "action": "{action}",
                    "max_attempts": 1,
                    "when": null,
                    "input_schema": {{"type": "any"}},
                    "output_schema": {{"type": "any"}}
                }}],
                "skippable_dependencies": []
            }},
            "inputs": {inputs}
        }}"#
    )
}

pub(super) fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}
