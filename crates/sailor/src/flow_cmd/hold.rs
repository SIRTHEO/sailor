//! `sailor flow hold` and `sailor flow unhold`: a person stops a flow from
//! starting by itself, and says who and why. By hand it still runs: a hold
//! is about what starts without anybody, not about what a person asks for.

use super::{default_ledger_dir, now_secs, one_flow};
use ledger::flow_holds::Hold;
use ledger::Ledger;
use std::collections::BTreeMap;
use ui::gather::FlowSource;

pub(super) fn hold_flow(
    sources: &[FlowSource],
    name: &str,
    words: &[String],
) -> Result<String, String> {
    let (who, why) = who_and_why(words)?;
    let (flow, _) = one_flow(sources, name)?;
    open()?
        .hold_flow(&flow.id, &why, &who, now_secs()?)
        .map_err(|error| error.to_string())?;
    Ok(catalogue::say(
        "cli.flow.hold_written",
        &[("flow", &flow.id)],
    ))
}

pub(super) fn unhold_flow(
    sources: &[FlowSource],
    name: &str,
    words: &[String],
) -> Result<String, String> {
    let (who, why) = who_and_why(words)?;
    let (flow, _) = one_flow(sources, name)?;
    let ledger = open()?;
    if ledger
        .flow_hold(&flow.id)
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Err(catalogue::say("cli.flow.not_held", &[("flow", &flow.id)]));
    }
    ledger
        .release_flow(&flow.id, &why, &who, now_secs()?)
        .map_err(|error| error.to_string())?;
    Ok(catalogue::say(
        "cli.flow.unhold_written",
        &[("flow", &flow.id)],
    ))
}

/// Every hold standing on this machine, for the list. A ledger that is absent
/// or will not open holds nothing: a list must not fail over a lock.
pub(crate) fn standing_holds() -> BTreeMap<String, Hold> {
    default_ledger_dir()
        .ok()
        .and_then(|dir| Ledger::open(&dir).ok())
        .and_then(|ledger| ledger.flow_holds().ok())
        .unwrap_or_default()
}

/// How a hold reads beside its flow.
pub fn hold_said(hold: &Hold) -> String {
    catalogue::say("cli.flow.held", &[("by", &hold.by), ("why", &hold.why)])
}

fn open() -> Result<Ledger, String> {
    Ledger::open(&default_ledger_dir()?).map_err(|error| error.to_string())
}

/// `--as <who> <why…>`: both are required, since a hold nobody can attribute
/// or explain is the silent copy this replaces.
fn who_and_why(words: &[String]) -> Result<(String, String), String> {
    match words {
        [flag, who, why @ ..] if flag == "--as" && !who.trim().is_empty() && !why.is_empty() => {
            Ok((who.clone(), why.join(" ")))
        }
        _ => Err(catalogue::say("cli.flow.hold_needs_who_and_why", &[])),
    }
}
