//! What each account was asked for, what it cost, and when its quota last ran
//! out. The sums are made here and nowhere else: two places would give two
//! figures and nobody would know which to believe.

use crate::{Ledger, LedgerError};
use serde_json::Value;
use std::collections::BTreeMap;

/// The error types an engine reports when it has nothing left to give.
const RAN_OUT: [&str; 2] = ["quota_exhausted", "exhausted"];

/// One account of one command line, with everything the store knows it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountStanding {
    pub cli: String,
    /// Absent when the call inherited whatever home the terminal was in.
    pub profile: Option<String>,
    pub home: Option<String>,
    pub calls: u64,
    pub spent_micros: i64,
    pub tokens: u64,
    pub last_call_at: i64,
    pub ran_out_at: Option<i64>,
}

impl AccountStanding {
    /// The same account reached through two command-line names is one account.
    fn fold(&mut self, other: &AccountStanding) {
        self.calls += other.calls;
        self.spent_micros += other.spent_micros;
        self.tokens += other.tokens;
        self.last_call_at = self.last_call_at.max(other.last_call_at);
        self.ran_out_at = self.ran_out_at.max(other.ran_out_at);
        if self.home.is_none() {
            self.home = other.home.clone();
        }
    }
}

impl Ledger {
    /// Every account that answered since `since`, the most expensive first.
    ///
    /// Calls are grouped by the account the engine declared, not by the
    /// descriptor that reached it: `codex` and `codex-repair` spend the same
    /// quota, and a reading that splits them hides how close it is to the end.
    pub fn accounts_standing(&self, since: i64) -> Result<Vec<AccountStanding>, LedgerError> {
        let connection = self.lock()?;
        let ran_out = RAN_OUT.map(|kind| format!("'{kind}'")).join(", ");
        let mut statement = connection.prepare(&format!(
            "SELECT cli, COALESCE(engine_identity, ''),
                    COUNT(*),
                    COALESCE(SUM(cost_micros), 0),
                    COALESCE(SUM(CAST(total_tokens AS INTEGER)), 0),
                    COALESCE(MAX(started_at), 0),
                    MAX(CASE WHEN error_type IN ({ran_out}) THEN started_at END)
             FROM model_calls
             WHERE started_at >= ?1
             GROUP BY cli, COALESCE(engine_identity, '')"
        ))?;
        let rows = statement
            .query_map([since], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(worst_spender_first(rows.into_iter().map(as_standing)))
    }
}

fn as_standing(
    (cli, identity, calls, spent_micros, tokens, last_call_at, ran_out_at): (
        String,
        String,
        i64,
        i64,
        i64,
        i64,
        Option<i64>,
    ),
) -> AccountStanding {
    let declared: Option<Value> = serde_json::from_str(&identity).ok();
    let said = |key: &str| {
        declared
            .as_ref()
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .filter(|found| !found.is_empty())
            .map(str::to_owned)
    };
    AccountStanding {
        cli: said("cli_id").unwrap_or(cli),
        profile: said("profile_name"),
        home: said("home_dir"),
        calls: calls.max(0) as u64,
        spent_micros,
        tokens: tokens.max(0) as u64,
        last_call_at,
        ran_out_at,
    }
}

fn worst_spender_first(
    standings: impl Iterator<Item = AccountStanding>,
) -> Vec<AccountStanding> {
    let mut folded: BTreeMap<(String, Option<String>), AccountStanding> = BTreeMap::new();
    for standing in standings {
        let key = (standing.cli.clone(), standing.profile.clone());
        match folded.get_mut(&key) {
            Some(held) => held.fold(&standing),
            None => {
                folded.insert(key, standing);
            }
        }
    }
    let mut found: Vec<_> = folded.into_values().collect();
    found.sort_by(|left, right| {
        right
            .spent_micros
            .cmp(&left.spent_micros)
            .then(right.calls.cmp(&left.calls))
            .then(left.cli.cmp(&right.cli))
            .then(left.profile.cmp(&right.profile))
    });
    found
}
