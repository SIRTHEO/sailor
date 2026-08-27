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
    /// Impronta del solo input tipato; non comprende i freni attivi del passo.
    pub input_digest: String,
    pub input: Value,
    /// I freni attivi quando il passo è partito.
    pub gates: Vec<String>,
    /// Relazione della ripresa con l'identità già registrata, calcolata dal motore.
    #[serde(default)]
    pub attempt_relation: Option<AttemptRelation>,
    /// Il pid del processo che teneva il passo mentre girava, scritto dal
    /// motore all'apertura. È un campo, non una convenzione dentro `input`:
    /// chi riprende deve poter chiedere al kernel se quel processo è vivo
    /// senza sapere come il chiamante ha chiamato la propria chiave.
    #[serde(default)]
    pub held_by_pid: Option<u32>,
    /// La specie del passo, congelata all'apertura: dichiara se rifarlo è
    /// sicuro. `None` è un record scritto prima che la specie esistesse, e
    /// vale quanto `HandToHuman` — mai quanto `Repeatable`.
    #[serde(default)]
    pub species: Option<StepSpecies>,
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
    /// Conteggio totale dei byte emessi; non fa parte del canale dati tipato.
    #[serde(default)]
    pub bytes_seen: Option<u64>,
    /// Byte tagliati perché oltre il tetto configurato.
    #[serde(default)]
    pub bytes_discarded: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Went,
    Broke,
    Waiting,
    Stopped,
    Skipped,
}

/// Le tre specie di passo: un'azione o si può rifare tale e quale, o si può
/// disfare e rifare, o va lasciata a una persona. Non esiste una quarta
/// strada implicita — la specie sconosciuta vale `HandToHuman`, mai
/// `Repeatable`, perché l'errore da cui questa distinzione difende è
/// duplicare un effetto già avvenuto sul mondo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepSpecies {
    /// Si può rilanciare tale e quale: nessun effetto residuo da disfare.
    Repeatable,
    /// L'effetto già prodotto si può disfare, poi il passo si rifà.
    Compensable,
    /// Né l'uno né l'altro: nessuna azione automatica è sicura.
    HandToHuman,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptRelation {
    SameInput,
    SameInputGatesChanged,
    DifferentInput,
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
            attempt_relation: None,
            held_by_pid: None,
            species: None,
            started_at,
            outcome: None,
            output: None,
            said: None,
            failure_class: None,
            ended_at: None,
            bytes_seen: None,
            bytes_discarded: None,
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

    #[test]
    fn legacy_record_without_byte_counts_is_accepted() {
        let json_str = r#"{
            "run_id": "run-legacy",
            "step_id": "step",
            "attempt": 1,
            "epoch": 1,
            "deps": [],
            "input_digest": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "input": null,
            "gates": [],
            "started_at": 1,
            "outcome": "Went",
            "output": null,
            "said": null,
            "failure_class": null,
            "ended_at": 2
        }"#;
        let record: StepRecord = serde_json::from_str(json_str).expect("record vecchio leggibile");
        assert_eq!(record.bytes_seen, None);
        assert_eq!(record.bytes_discarded, None);
        // Scritto prima che la specie esistesse: resta leggibile, e non
        // eredita una specie che nessuno ha dichiarato.
        assert_eq!(record.species, None);
        assert_eq!(record.held_by_pid, None);
    }

    #[test]
    fn species_and_holder_survive_a_round_trip() {
        let mut record = StepRecord::started("run", "step", 1, 1, vec![], json!(null), vec![], 1);
        record.species = Some(StepSpecies::Compensable);
        record.held_by_pid = Some(4321);
        let text = serde_json::to_string(&record).expect("record serializzabile");
        assert!(text.contains("\"compensable\""), "la specie va scritta in minuscolo: {text}");
        let back: StepRecord = serde_json::from_str(&text).expect("record rileggibile");
        assert_eq!(back.species, Some(StepSpecies::Compensable));
        assert_eq!(back.held_by_pid, Some(4321));
    }
}
