//! The strengths table: for a kind of work, the engines to try first, across
//! companies, in order. A file and not code, so the person rewrites it; a kind
//! without a row falls to the chain as the flow wrote it.

use serde::Deserialize;
use std::collections::BTreeMap;

/// The table shipped with the product; `$SAILOR_HOME/strengths.json` replaces it whole.
pub const BUILTIN: &str = include_str!("../strengths.default.json");

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Strengths {
    /// What the rows were written on: the ledger's calls per kind, or the
    /// admission that nothing was measured yet.
    #[serde(default)]
    pub measured_on: String,
    #[serde(default)]
    pub rows: BTreeMap<String, Vec<String>>,
}

impl Strengths {
    pub fn parse(text: &str) -> Result<Strengths, String> {
        serde_json::from_str(text).map_err(|error| format!("strengths table: {error}"))
    }

    pub fn shipped() -> Strengths {
        Strengths::parse(BUILTIN).expect("the strengths table shipped with the product parses")
    }

    /// The engines to try first for `kind`, in order; empty without a row.
    pub fn first_for(&self, kind: &str) -> &[String] {
        self.rows.get(kind).map(Vec::as_slice).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_without_a_row_names_nobody() {
        let table = Strengths::parse(r#"{"rows": {"mechanical": ["local", "cheap"]}}"#).expect("parses");
        assert_eq!(table.first_for("mechanical"), ["local", "cheap"]);
        assert!(table.first_for("judgement").is_empty());
    }

    #[test]
    fn the_shipped_table_parses_and_says_what_it_was_measured_on() {
        let shipped = Strengths::shipped();
        assert!(!shipped.measured_on.is_empty(), "a table without its measure is a guess");
    }
}
