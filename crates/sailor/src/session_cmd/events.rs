//! The vocabulary of terminal events: which names a command line can report,
//! what each one starts, and what a person actually typed. Words and their
//! meanings, with nothing that writes.

use super::*;

/// What Sailor does at a moment. The moments are toolbox's list, asked rather
/// than copied here; what a line calls each is said by its descriptor.
/// `session_start` carries the welcome and is the only one whose text reaches
/// the agent, so it opens; every other moment is an event.
pub(super) fn what_we_do_at(moment: &str) -> &'static str {
    if moment == toolbox::descriptor::SESSION_START {
        "open"
    } else {
        "event"
    }
}

/// Every moment with its verb, in the order toolbox names them. **FOUR, AND
/// NO MORE**: one more hook is one more process at every event of every session.
pub(super) fn what_we_do_at_each() -> impl Iterator<Item = (&'static str, &'static str)> {
    toolbox::descriptor::MOMENTS
        .iter()
        .map(|moment| (*moment, what_we_do_at(moment)))
}

/// The moments this line can report, paired with the verb we run at each.
pub(super) fn events_this_line_can_report(tool: &toolbox::descriptor::Descriptor) -> Vec<(&str, &str)> {
    what_we_do_at_each()
        .filter_map(|(moment, verb)| tool.event_for(moment).map(|event| (event, verb)))
        .collect()
}

pub(super) fn arrival_of(request: &Request<'_>) -> Arrival {
    Arrival {
        anchor: anchor_of(request),
        session_id: request.payload.session_id.clone(),
        transcript_path: request.payload.transcript_path.clone(),
        at: request.at,
    }
}

/// The fact's name: the one the payload declares, or the verb's own.
pub(super) fn event_named(request: &Request<'_>, fallback: &str) -> TerminalEvent {
    let anchor = anchor_of(request);
    TerminalEvent {
        tty: anchor.tty.clone(),
        session_id: request.payload.session_id.clone(),
        worktree: Some(anchor.worktree.clone()),
        ancestor: anchor.ancestor.clone(),
        name: request
            .payload
            .hook_event_name
            .clone()
            .filter(|found| !found.is_empty())
            .unwrap_or_else(|| fallback.to_owned()),
        transcript_path: request.payload.transcript_path.clone(),
        occurred_at: request.at,
        // The keys `terminal::keeping` allows, and nothing else. A hook sends
        // what a person typed and what the engine answered; keeping those
        // would put a secret typed once on this disk for good.
        payload: terminal::keeping::what_is_kept(
            request.raw,
            std::env::var(terminal::keeping::KEEP_BODIES).ok(),
        ),
    }
}

/// The name the others see in the survey: **the command line and the account
/// its home answers as**, which is what tells two terminals of the same tree
/// apart when the tree is all they have in common.
pub(super) fn agent_of(request: &Request<'_>) -> String {
    // A hook grafted before the line learnt to name its command line says
    // nothing here, and the survey shows that instead of guessing a name.
    let Some(cli) = request.options.get("cli").filter(|id| !id.is_empty()) else {
        return catalogue::say("cli.session.a_line_that_did_not_say", &[]);
    };
    let declared = profiles::store_io::load_store()
        .unwrap_or_default()
        .profiles;
    let account = super::engine_named(cli).and_then(|engine| {
        account_of_this_session(
            engine,
            &|name| std::env::var(name).ok(),
            &|path| std::fs::read_to_string(path).ok(),
            &declared,
        )
    });
    match account {
        Some(account) => format!("{cli} ({account})"),
        None => cli.clone(),
    }
}

/// The account the session a hook runs in answers as, from that session's own
/// variable, or from the engine's own home when the variable is absent. A home
/// that cannot tell is named by the profile declared on it.
/// **NEVER THE STORE'S ACTIVE PROFILE**: it can be switched while a session
/// keeps the account it started with.
pub(super) fn account_of_this_session(
    engine: &profiles::KnownCli,
    key_of: &dyn Fn(&str) -> Option<String>,
    read: &dyn Fn(&std::path::Path) -> Option<String>,
    declared: &[profiles::Profile],
) -> Option<String> {
    let moved_to = match &engine.home {
        profiles::HomeMechanism::EnvVar(variable) => key_of(variable).filter(|dir| !dir.is_empty()),
        _ => None,
    };
    let (home, by_variable) = match moved_to {
        Some(dir) => (std::path::PathBuf::from(dir), true),
        None => (
            profiles::existing_home(engine, std::path::Path::new(&key_of("HOME")?))?,
            false,
        ),
    };
    let identity = if by_variable {
        profiles::identity_of_home(engine, &home, read)
    } else {
        profiles::identity_of_own_home(engine, &home, read)
    };
    match identity {
        profiles::HomeIdentity::Answers(account) => Some(account),
        profiles::HomeIdentity::CannotTell(_) => declared
            .iter()
            .find(|profile| profile.cli_id == engine.id && profile.home_dir == home)
            .map(|profile| profile.name.clone()),
    }
}

/// A run named for a reader: the flow, then the identifier the resume needs.
pub(super) fn run_named(run: &ledger::WaitingRun) -> String {
    if run.entity.is_empty() {
        return run.run_id.clone();
    }
    format!("{} ({})", run.entity, run.run_id)
}

/// The flows this event starts, judged and started before the hook returns.
///
/// **NOTHING IS WAITED FOR.** Every run is let go the instant it is lit: a
/// person's prompt is waiting on this call, and a hook that waits for a flow
/// is a hook that stops the work it was meant to help.
pub(super) fn what_this_event_starts(
    request: &Request<'_>,
    store: &Sessions,
    event_id: i64,
    happened: &TerminalEvent,
) -> String {
    let asked = crate::arc_cmd::Happened {
        event: happened.name.clone(),
        tree: happened.worktree.clone().unwrap_or_default(),
        tty: happened.tty.clone(),
        session: happened.session_id.clone().unwrap_or_default(),
        prompt: what_a_person_typed(request.raw),
        transcript: happened.transcript_path.clone(),
    };
    let verdicts = crate::arc_cmd::evaluate(
        store,
        event_id,
        &asked,
        &ui::gather::flow_sources(),
        request.at,
        &mut crate::arc_cmd::launch_detached,
        &mut crate::arc_cmd::parked_for,
    );
    let acted: Vec<&sessions::Verdict> = verdicts
        .iter()
        .filter(|row| row.verdict != sessions::DEFERRED)
        .collect();
    if acted.is_empty() {
        return String::new();
    }
    acted
        .iter()
        .map(|row| {
            format!(
                "{}\t{}\t{}",
                row.flow,
                row.verdict,
                row.why.clone().unwrap_or_default()
            )
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// What the person typed, read from the payload and never written down.
///
/// A prompt is a body: it is matched here, in memory, and goes no further —
/// not into the store, not into an argument list, not into the record of the
/// child a match starts.
pub(super) fn what_a_person_typed(raw: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    value
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .filter(|typed| !typed.trim().is_empty())
        .map(str::to_owned)
}
