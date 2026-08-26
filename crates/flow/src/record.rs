use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_SAID_BYTES: usize = 16 * 1024;

/// Un passo, come record durevole.
/// L'intenzione si scrive PRIMA di eseguire; l'esito DOPO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepRecord {
    pub run_id: String,
    pub step_id: String,
    pub attempt: u32,
    /// Monotona. Un tentativo dato per morto che torna non riscrive stato vecchio.
    pub epoch: u64,
    pub deps: Vec<String>,
    /// Impronta degli ingressi: due esecuzioni con la stessa impronta sono la stessa.
    pub input_digest: String,
    pub input: Value,
    /// I freni attivi quando il passo è partito.
    pub gates: Vec<String>,
    pub started_at: i64,

    // Scritti alla chiusura. `deserialize_with` rende obbligatoria anche la
    // presenza dei campi nulli: un record troncato non può sembrare valido.
    #[serde(deserialize_with = "required_option")]
    pub outcome: Option<Outcome>,
    #[serde(deserialize_with = "required_option")]
    pub output: Option<Value>,
    /// Testo grezzo, troncato. Serve a una persona quando qualcosa va storto.
    /// NON è il canale dati: nessuna condizione si valuta su questo.
    #[serde(deserialize_with = "required_option")]
    pub said: Option<String>,
    /// Popolata dal motore, non da un modello. È una classe, non una diagnosi.
    #[serde(deserialize_with = "required_option")]
    pub failure_class: Option<String>,
    #[serde(deserialize_with = "required_option")]
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Went,
    Broke,
    Waiting,
    Stopped,
    Skipped,
}

impl StepRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn started(
        run_id: impl Into<String>,
        step_id: impl Into<String>,
        attempt: u32,
        epoch: u64,
        deps: Vec<String>,
        input: Value,
        gates: Vec<String>,
        started_at: i64,
    ) -> Self {
        let input_digest = digest_input(&input);
        Self {
            run_id: run_id.into(),
            step_id: step_id.into(),
            attempt,
            epoch,
            deps,
            input_digest,
            input,
            gates,
            started_at,
            outcome: None,
            output: None,
            said: None,
            failure_class: None,
            ended_at: None,
        }
    }
}

pub fn digest_input(value: &Value) -> String {
    let canonical = canonical_value(value);
    let bytes = serde_json::to_vec(&canonical)
        .expect("serializzare un serde_json::Value in memoria non può fallire");
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

pub fn truncate_said(value: String) -> String {
    if value.len() <= MAX_SAID_BYTES {
        return value;
    }
    let mut end = MAX_SAID_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_value).collect()),
        Value::Object(values) => {
            let sorted: BTreeMap<_, _> = values
                .iter()
                .map(|(key, value)| (key.clone(), canonical_value(value)))
                .collect();
            Value::Object(sorted.into_iter().collect())
        }
        other => other.clone(),
    }
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn digest_does_not_depend_on_object_insertion_order() {
        let first = json!({"a": 1, "b": {"c": 2, "d": 3}});
        let second = json!({"b": {"d": 3, "c": 2}, "a": 1});
        assert_eq!(digest_input(&first), digest_input(&second));
    }

    #[test]
    fn incomplete_record_is_rejected() {
        let record = StepRecord::started("run", "step", 1, 1, vec![], json!(null), vec![], 1);
        let mut value = serde_json::to_value(record).expect("record serializzabile");
        value
            .as_object_mut()
            .expect("il record è un oggetto")
            .remove("gates");
        assert!(serde_json::from_value::<StepRecord>(value).is_err());
    }

    #[test]
    fn null_closing_field_must_still_be_present() {
        let record = StepRecord::started("run", "step", 1, 1, vec![], json!(null), vec![], 1);
        let mut value = serde_json::to_value(record).expect("record serializzabile");
        value
            .as_object_mut()
            .expect("il record è un oggetto")
            .remove("ended_at");
        assert!(serde_json::from_value::<StepRecord>(value).is_err());
    }

    #[test]
    fn truncate_said_preserves_a_multibyte_character_boundary() {
        let value = format!("{}é", "a".repeat(MAX_SAID_BYTES - 1));
        let truncated = truncate_said(value);
        assert_eq!(truncated, "a".repeat(MAX_SAID_BYTES - 1));
    }
}
