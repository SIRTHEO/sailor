//! A spend cap per engine on a declared window, kept by the person in a file
//! beside the profiles. It **excludes** an engine whose window is full and
//! never reorders the chain: the order stays the flow author's.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// The cap, in currency micro-units, over one window.
    pub cap_micros: i64,
    /// How long a window is; the current one started `window_secs` ago.
    pub window_secs: i64,
}

/// Where the budgets live: `SAILOR_BUDGETS`, or `budgets.json` in the home.
pub fn default_path() -> Option<PathBuf> {
    if let Some(declared) = std::env::var_os("SAILOR_BUDGETS").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(declared));
    }
    ledger::sailor_home().map(|home| home.join("budgets.json"))
}

/// The budgets declared, by engine id. A missing file declares none; a file
/// that does not read is an error, never «no caps»: a typo must not lift them.
pub fn declared(path: &Path) -> Result<BTreeMap<String, Budget>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(format!("{} cannot be read: {error}", path.display())),
    };
    serde_json::from_str(&text).map_err(|error| format!("{} does not parse: {error}", path.display()))
}

/// Why an engine is over its budget, for the refusal; `None` while it fits.
pub fn over(budget: &Budget, spent: &flow::Spend) -> Option<String> {
    if spent.micros < budget.cap_micros {
        return None;
    }
    let unknown = if spent.calls_without_cost > 0 {
        format!(", and {} calls of unknown cost besides", spent.calls_without_cost)
    } else {
        String::new()
    };
    Some(format!(
        "over its budget: spent {} of {} in the last {} seconds{unknown}",
        money(spent.micros),
        money(budget.cap_micros),
        budget.window_secs,
    ))
}

fn money(micros: i64) -> String {
    format!("{:.4} $", micros as f64 / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spend(micros: i64, without: i64) -> flow::Spend {
        flow::Spend {
            micros,
            calls: 3,
            calls_without_cost: without,
            dearest_micros: Some(micros),
        }
    }

    #[test]
    fn an_engine_under_its_cap_fits_and_one_at_it_does_not() {
        let budget = Budget { cap_micros: 1_000_000, window_secs: 3600 };
        // The control first: under the cap nothing is said.
        assert_eq!(over(&budget, &spend(999_999, 0)), None);
        let why = over(&budget, &spend(1_000_000, 0)).expect("at the cap it is over");
        assert!(why.contains("1.0000 $ of 1.0000 $ in the last 3600 seconds"), "{why}");
        let with_unknowns = over(&budget, &spend(2_000_000, 2)).expect("over");
        assert!(with_unknowns.contains("2 calls of unknown cost"), "{with_unknowns}");
    }

    #[test]
    fn a_missing_file_declares_no_budget_and_a_broken_one_is_an_error() {
        assert!(declared(Path::new("/nowhere/budgets.json")).expect("missing is none").is_empty());
        let dir = std::env::temp_dir().join(format!("budgets-broken-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let broken = dir.join("budgets.json");
        std::fs::write(&broken, "{ not json").expect("write");
        let error = declared(&broken).expect_err("a broken file is not «no caps»");
        assert!(error.contains("does not parse"), "{error}");
    }
}
