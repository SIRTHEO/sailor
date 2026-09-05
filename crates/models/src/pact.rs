//! The data pact of a model: whether what is sent to it trains the provider's
//! next model. Three words and no fourth; `unknown` is what nobody measured,
//! and it is never read as `does_not_train`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataPact {
    Trains,
    DoesNotTrain,
    #[default]
    Unknown,
}

impl DataPact {
    /// Whether a private step may go to a model under this pact: only a
    /// measured «does not train» does, and `unknown` is not a yes.
    pub fn accepts_private(self) -> bool {
        self == DataPact::DoesNotTrain
    }
}

impl fmt::Display for DataPact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DataPact::Trains => "trains",
            DataPact::DoesNotTrain => "does_not_train",
            DataPact::Unknown => "unknown",
        })
    }
}

/// The pacts shipped with the product, per model id, embedded like the price
/// list; `$SAILOR_HOME/pacts.json` overrides by id.
pub const BUILTIN: &str = include_str!("../pacts.default.json");

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Pacts {
    #[serde(default)]
    pub pacts: BTreeMap<String, DataPact>,
}

impl Pacts {
    /// Reads a pacts file. One entry with a word outside the three refuses
    /// the whole file, naming the error: a pact half read is a pact guessed.
    pub fn parse(text: &str) -> Result<Pacts, String> {
        serde_json::from_str(text).map_err(|error| format!("pacts file: {error}"))
    }

    pub fn shipped() -> Pacts {
        Pacts::parse(BUILTIN).expect("the pacts shipped with the product parse")
    }

    /// The shipped pacts with the home file's laid over them by id; a home
    /// file that does not read is an error, never «no pacts».
    pub fn in_force(home: Option<&str>) -> Result<Pacts, String> {
        let mut pacts = Pacts::shipped();
        if let Some(text) = home {
            pacts.pacts.extend(Pacts::parse(text)?.pacts);
        }
        Ok(pacts)
    }

    pub fn of(&self, model_id: &str) -> DataPact {
        self.pacts.get(model_id).copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fourth_word_refuses_the_file_and_names_itself() {
        let refused = Pacts::parse(r#"{"pacts": {"a/model": "sometimes"}}"#)
            .expect_err("a word outside the three is refused");
        assert!(refused.contains("sometimes"), "{refused}");
        let read = Pacts::parse(r#"{"pacts": {"a/model": "does_not_train", "b/model": "trains"}}"#)
            .expect("the three words read");
        assert_eq!(read.of("a/model"), DataPact::DoesNotTrain);
        assert_eq!(read.of("b/model"), DataPact::Trains);
        assert_eq!(read.of("nobody/measured"), DataPact::Unknown);
    }

    #[test]
    fn the_shipped_pacts_parse_and_the_home_file_lays_over_them_by_id() {
        Pacts::shipped();
        let laid = Pacts::in_force(Some(r#"{"pacts": {"a/model": "trains"}}"#)).expect("reads");
        assert_eq!(laid.of("a/model"), DataPact::Trains);
        assert!(Pacts::in_force(Some("{ not json")).is_err(), "a broken home file is not silence");
    }
}
