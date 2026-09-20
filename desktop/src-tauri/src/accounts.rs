//! The accounts Sailor can reach, as the window draws them.
//!
//! **THE JOIN IS `sailor::accounts_cmd`'S, NOT THIS MODULE'S.** What an account
//! is gets decided in the function `sailor accounts` itself calls. Here a row
//! only gains what a drawing needs and a line of text does not: a brand, and a
//! name a person reads instead of an identifier.

use sailor::accounts_cmd::{self, AccountView};
use serde::Serialize;

/// One window of an allowance, already named for a person.
#[derive(Serialize)]
pub(crate) struct Window {
    pub unit: String,
    /// The length where anything measured one, the provider's word where not.
    pub word: String,
    /// Spent, from `0.0` to `1.0`. **SPENT, NEVER LEFT.**
    pub used_fraction: f64,
    pub resets_at: Option<String>,
    pub lasts_seconds: Option<u64>,
}

/// What one model did under this account over the window asked for.
#[derive(Serialize)]
pub(crate) struct ByModel {
    pub model: String,
    pub input: u64,
    pub output: u64,
}

/// What an account did in its own home, read off the engine's own records.
#[derive(Serialize)]
pub(crate) struct Worked {
    pub calls: u64,
    pub sessions: usize,
    pub by_model: Vec<ByModel>,
}

#[derive(Serialize)]
pub(crate) struct Row {
    pub cli: String,
    /// What the descriptor calls this command line; the id where none does.
    pub label: String,
    /// The slug the brand mark is looked up by, empty where none is declared.
    pub brand: String,
    pub profile: Option<String>,
    pub active: bool,
    /// `ready` | `ran_out` | `shut` | `unknown`.
    pub standing: &'static str,
    /// The engine's own words. **NEVER DERIVED FROM `standing`**, and the
    /// reason the window can say why rather than only what.
    pub said: String,
    pub calls: u64,
    pub spent_micros: i64,
    pub tokens: u64,
    pub last_call_at: i64,
    pub ran_out_at: Option<i64>,
    pub windows: Vec<Window>,
    /// Why there is no allowance to draw, when there is none.
    pub quota_refused: Option<String>,
    pub worked: Option<Worked>,
    /// The line a person runs to cure this row, where the engine declares one.
    pub repair: Option<String>,
}

/// The descriptor a command line belongs to, found by its **executable**.
///
/// Not by the id: the descriptors call `claude-code` what the profile table
/// calls `claude`, and the binary is the one name both lists agree on.
fn drawn_as(cli: &str) -> (String, String) {
    let Ok(known) = profiles::find_cli(cli) else {
        return (cli.to_owned(), String::new());
    };
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    catalog
        .descriptors
        .iter()
        .map(|loaded| &loaded.descriptor)
        .find(|descriptor| {
            descriptor.detect.as_ref().is_some_and(|probes| {
                probes
                    .as_slice()
                    .iter()
                    .any(|probe| probe.command.as_deref() == Some(known.executable.as_str()))
            })
        })
        .map(|descriptor| {
            let label = if descriptor.label.is_empty() {
                descriptor.id.clone()
            } else {
                descriptor.label.clone()
            };
            (label, descriptor.brand.clone())
        })
        .unwrap_or_else(|| (known.display_name.to_owned(), String::new()))
}

fn drawn(view: &AccountView, label: &str, brand: &str) -> Row {
    Row {
        cli: view.cli.clone(),
        label: label.to_owned(),
        brand: brand.to_owned(),
        profile: view.profile.clone(),
        active: view.active,
        standing: view.standing.key(),
        said: view.said.clone(),
        calls: view.calls,
        spent_micros: view.spent_micros,
        tokens: view.tokens,
        last_call_at: view.last_call_at,
        ran_out_at: view.ran_out_at,
        windows: view
            .quota
            .as_ref()
            .map(|seen| {
                seen.windows
                    .iter()
                    .map(|window| Window {
                        unit: window.unit.clone(),
                        word: window.word(),
                        used_fraction: window.used_fraction,
                        resets_at: window.resets_at.clone(),
                        lasts_seconds: window.lasts_seconds,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        quota_refused: view.quota.as_ref().and_then(|seen| seen.refused.clone()),
        worked: view.worked.as_ref().map(|worked| Worked {
            calls: worked.calls,
            sessions: worked.sessions,
            by_model: worked
                .by_model
                .iter()
                .map(|(model, tokens)| ByModel {
                    model: model.clone(),
                    input: tokens.input,
                    output: tokens.output,
                })
                .collect(),
        }),
        repair: view.repair.clone(),
    }
}

/// **`with_quota` CALLS THE ENGINES**: seconds, and the network. Without it
/// this answer is the store and the profile homes, and costs nothing — which
/// is why the window opens without it and fills the bars afterwards.
#[tauri::command]
pub(crate) fn accounts(hours: Option<i64>, with_quota: bool) -> Result<Vec<Row>, String> {
    let views = accounts_cmd::the_accounts(hours.unwrap_or(24), with_quota)?;
    let mut drawn_rows = Vec::with_capacity(views.len());
    for view in &views {
        let (label, brand) = drawn_as(&view.cli);
        drawn_rows.push(drawn(view, &label, &brand));
    }
    Ok(drawn_rows)
}

#[cfg(test)]
mod tests;
