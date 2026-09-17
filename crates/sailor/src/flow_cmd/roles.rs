//! `sailor flow role`: the chain of engines a role stands for, read and
//! written in the ledger's `roles` collection a step's `role` resolves against.

use ledger::{Ledger, StoreRecord};
use serde_json::json;
use std::fmt::Write as _;

const ROLES: &str = "roles";

pub(super) fn roles() -> Result<String, String> {
    roles_in(&open_ledger()?)
}

pub(super) fn set_role(name: &str, tools: &str, account: Option<&str>) -> Result<String, String> {
    let known = toolbox::resolver::Tools::current();
    set_role_in(&open_ledger()?, name, tools, account, |id| known.declares(id), super::now_secs()?)
}

fn open_ledger() -> Result<Ledger, String> {
    Ledger::open(super::default_ledger_dir()?).map_err(|error| error.to_string())
}

fn roles_in(ledger: &Ledger) -> Result<String, String> {
    let rows = ledger.records_in(ROLES).map_err(|error| error.to_string())?;
    if rows.is_empty() {
        return Ok(catalogue::say("cli.flow.role.none", &[]));
    }
    let mut report = String::new();
    for row in rows {
        let tools = row.value["tools"]
            .as_array()
            .map(|tools| tools.iter().filter_map(|tool| tool.as_str()).collect::<Vec<_>>().join(","))
            .unwrap_or_default();
        let account = row.value["account"].as_str().unwrap_or("-");
        let _ = writeln!(report, "{}\t{tools}\t{account}", row.key);
    }
    Ok(report.trim_end().to_owned())
}

/// A role naming a tool no descriptor declares is refused here, while whoever
/// writes it is present, not at the step that resolves it. See fault 189.
fn set_role_in(
    ledger: &Ledger,
    name: &str,
    tools: &str,
    account: Option<&str>,
    declared: impl Fn(&str) -> bool,
    now: i64,
) -> Result<String, String> {
    let chain: Vec<&str> = tools.split(',').map(str::trim).filter(|tool| !tool.is_empty()).collect();
    if name.trim().is_empty() || chain.is_empty() {
        return Err(catalogue::say("cli.flow.role.needs_tools", &[("role", name)]));
    }
    if let Some(unknown) = chain.iter().find(|tool| !declared(tool)) {
        return Err(catalogue::say("cli.flow.role.unknown_tool", &[("role", name), ("tool", unknown)]));
    }
    let mut value = json!({ "tools": chain });
    if let Some(account) = account {
        value["account"] = json!(account);
    }
    ledger
        .put_record(&StoreRecord {
            collection: ROLES.to_owned(),
            key: name.to_owned(),
            value,
            written_by: "sailor flow role".to_owned(),
            written_at: now,
        })
        .map_err(|error| error.to_string())?;
    Ok(catalogue::say("cli.flow.role.written", &[("role", name), ("tools", &chain.join(","))]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sailor-flow-role-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_role_written_is_the_chain_a_step_resolves() {
        let dir = scratch("written");
        let ledger = Ledger::open(&dir).expect("a ledger of the test's own");

        set_role_in(&ledger, "CHEAP_WORKER", "codex, claude-code", Some("team"), |_| true, 7)
            .expect("both tools are declared");

        let resolved = actions::resolve_role(&json!({"role": "CHEAP_WORKER"}), Some(&ledger))
            .expect("the role resolves");
        assert_eq!(resolved["tool"], json!(["codex@team", "claude-code@team"]));
        assert_eq!(roles_in(&ledger).unwrap(), "CHEAP_WORKER\tcodex,claude-code\tteam");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tool_no_descriptor_declares_is_refused_and_nothing_is_written() {
        let dir = scratch("refused");
        let ledger = Ledger::open(&dir).expect("a ledger of the test's own");

        let refused = set_role_in(&ledger, "CHEAP_WORKER", "codex,claude-opus-5", None, |id| id == "codex", 7);

        assert!(refused.unwrap_err().contains("claude-opus-5"));
        assert!(ledger.read_record(ROLES, "CHEAP_WORKER").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
