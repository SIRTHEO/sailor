//! The greeting a terminal is shown when it opens: which tree it is in, what
//! is still open in the ledger, who else is here, and what has not been read.
//! It only reads; the bookkeeping that writes the terminals is next door.

use super::*;

/// The moments this command line has no event for, which are the ones the
/// report has to name. A pure answer, so it can be asked of a descriptor
/// nobody ships and the check needs no product to exist.
pub(super) fn moments_without_an_event(tool: &toolbox::descriptor::Descriptor) -> Vec<&'static str> {
    what_we_do_at_each()
        .map(|(moment, _)| moment)
        .filter(|moment| tool.event_for(moment).is_none())
        .collect()
}

/// Who else the register holds open in this tree, for the greeting. Nothing at
/// all when nobody else is here: a greeting that reports emptiness every time
/// teaches nobody to read it.
pub(super) fn neighbours(arrival: &Arrival, store: &Sessions) -> Option<String> {
    let rows = store.terminals().ok()?;
    let asked: std::cell::RefCell<std::collections::HashMap<String, Option<String>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    let repository_of = |path: &str| {
        if let Some(known) = asked.borrow().get(path) {
            return known.clone();
        }
        let found = repository_holding(path);
        asked.borrow_mut().insert(path.to_owned(), found.clone());
        found
    };
    let here = &arrival.anchor.worktree;
    let others = sessions::others_in_the_tree(&rows, &arrival.anchor.tty, here, &repository_of);
    if others.is_empty() {
        return None;
    }
    let abandoned = sessions::census::Census::of(&sessions::census::LocalMachine).abandoned(&rows);
    who_is_here(&others, &abandoned, here)
}

/// The sentence itself, apart from the machine that answered: which of the
/// rows still hold somebody, and how many are only rows.
///
/// **A ROW THAT HOLDS NOBODY IS NOT A NEIGHBOUR**, and it is not silence
/// either — it is counted and said, because a reader who is told nothing goes
/// looking for a terminal that is not there.
pub(super) fn who_is_here(
    others: &[&sessions::TerminalRow],
    abandoned: &sessions::census::Abandoned,
    here: &str,
) -> Option<String> {
    let mut alive = Vec::new();
    let mut stale = 0usize;
    let mut unknown = false;
    for row in others {
        match standing_of(row, abandoned) {
            Standing::NobodyThere => stale += 1,
            Standing::Unknown => {
                unknown = true;
                alive.push(named(row, here));
            }
            Standing::Open | Standing::Closed => alive.push(named(row, here)),
        }
    }
    // **THE OLD SENTENCE DECLARED WHAT IT COULD NOT ANSWER, AND THAT STAYS
    // TRUE WHEN THE MACHINE WOULD NOT SAY.** Answering for a census we were
    // refused would turn a limit we can state into a claim we cannot.
    if unknown {
        return Some(catalogue::say(
            "cli.session.others_in_this_tree",
            &[
                ("count", &alive.len().to_string()),
                ("who", &alive.join(", ")),
            ],
        ));
    }
    let mut said = Vec::new();
    if !alive.is_empty() {
        said.push(catalogue::say(
            "cli.session.others_alive",
            &[
                ("count", &alive.len().to_string()),
                ("who", &alive.join(", ")),
            ],
        ));
    }
    if stale > 0 {
        said.push(catalogue::say(
            "cli.session.others_stale",
            &[("count", &stale.to_string())],
        ));
    }
    if said.is_empty() {
        return None;
    }
    Some(said.join(" "))
}

/// The repository a directory belongs to, in git's own words, so that a
/// worktree and the checkout it was cut from come back as one place. `None`
/// where git says nothing: outside a repository, or with no git to ask.
pub(super) fn repository_holding(path: &str) -> Option<String> {
    let said = std::process::Command::new("git")
        .args([
            "-C",
            path,
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])
        .output()
        .ok()?;
    if !said.status.success() {
        return None;
    }
    let found = String::from_utf8_lossy(&said.stdout).trim().to_owned();
    (!found.is_empty()).then_some(found)
}

/// **ONLY WHERE THERE IS A PAGE TO SEE**: with no page on disk there is nothing
/// to miss, and an engine nobody looked into is not called blind.
pub(super) fn page_unseen(started: &Started<'_>, page: Option<&PageOnDisk>) -> Option<PageUnseen> {
    let (engine, page, home) = (started.engine?, page?, started.home.as_deref()?);
    let sight = crate::memory_cmd::Sight::of(
        engine,
        &started.worktree,
        home,
        started.profile_home.as_deref(),
        &page.path,
    );
    if sight.reads.is_empty() || sight.names_the_page.is_some() {
        return None;
    }
    Some(PageUnseen {
        engine: engine.display_name.clone(),
        files: sight.reads,
    })
}

/// The page, where the home has one. **A missing file is `None`**, not an
/// empty page: the greeting must not send a reader to a file that is not there.
///
/// **ITS ADDRESS AND NOT ITS OPENING.** The greeting is read again on every
/// call of the session; the first memories of the page, chosen by date and not
/// by what the session is about, were three quarters of it.
pub(super) fn page_on_disk(home: Option<&std::path::Path>) -> Option<PageOnDisk> {
    let path = actions::memory::page_path(home?);
    path.is_file().then_some(PageOnDisk { path })
}

/// The two lists and the memories this tree is handed, from a ledger already
/// open: a fact about one repository is not news in the next.
pub(super) fn still_open_in(
    deposit: &ledger::Ledger,
    home: Option<&std::path::Path>,
    started: &Started<'_>,
    tty: &str,
) -> Result<StillOpen, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let page = page_on_disk(home);
    let tree = workspace::tree_around(&started.worktree).map(|tree| tree.display().to_string());
    let mut elsewhere = 0;
    let mut born_here = |runs: Vec<ledger::WaitingRun>| {
        let (here, there): (Vec<_>, Vec<_>) = runs
            .into_iter()
            .partition(|run| run.tree.is_empty() || Some(&run.tree) == tree.as_ref());
        elsewhere += there.len();
        here
    };
    let waiting = born_here(deposit.waiting_runs().map_err(|error| error.to_string())?);
    let ask_again = born_here(
        deposit
            .runs_to_ask_again()
            .map_err(|error| error.to_string())?,
    );
    Ok(StillOpen {
        waiting,
        ask_again,
        elsewhere,
        remembered: actions::memory::seen_from(
            actions::memory::remembered(deposit, now).map_err(|error| error.to_string())?,
            tree.as_deref(),
        ),
        page_unseen: page_unseen(started, page.as_ref()),
        page,
        handover: deposit
            .handovers_owed_and_missed(1)
            .unwrap_or_default()
            .into_iter()
            .find(|missed| missed.tty == tty),
    })
}

/// This machine's ledger, or the one `--ledger` names. `Ok(None)` where there
/// is no home to look in, which is not the same as a home holding nothing.
/// `--ledger` is the twin of `--store`: the road down to a ledger that will
/// not open must be walkable without the machine's own home in it.
pub(super) fn deposit(declared: Option<&str>) -> Result<Option<ledger::Ledger>, String> {
    let directory = match declared {
        Some(declared) => PathBuf::from(declared),
        None => match ledger::default_directory() {
            Some(directory) => directory,
            None => return Ok(None),
        },
    };
    if !directory.exists() {
        return Ok(None);
    }
    ledger::Ledger::open(&directory)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// The same, from this machine's home.
pub(super) fn still_open(
    started: &Started<'_>,
    declared: Option<&str>,
    tty: &str,
) -> Result<Option<StillOpen>, String> {
    match deposit(declared)? {
        Some(deposit) => {
            still_open_in(&deposit, ledger::sailor_home().as_deref(), started, tty).map(Some)
        }
        None => Ok(None),
    }
}

/// The branch a directory is on, in git's own words. `None` where git says
/// nothing: a detached head, or no git to ask.
pub(super) fn branch_of(path: &str) -> Option<String> {
    let said = std::process::Command::new("git")
        .args(["-C", path, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !said.status.success() {
        return None;
    }
    let found = String::from_utf8_lossy(&said.stdout).trim().to_owned();
    (!found.is_empty() && found != "HEAD").then_some(found)
}

/// **THE TERMINAL ANNOUNCES ITSELF TO THE OTHER AGENTS**, and renews at every
/// event. Held by the terminal — not by the process that writes, which is a new
/// one at every keystroke, and not by the name of the command line, which
/// changes under the same terminal.
pub(super) fn announce(request: &Request<'_>, arrival: &Arrival, state: &str) -> Result<(), String> {
    let deposit = match request.deposit {
        // Nobody asked for a ledger here, so nothing was refused and there is
        // nothing to report: silence is the true answer.
        TheDeposit::NobodyNeedsItHere => return Ok(()),
        TheDeposit::WouldNotOpen(why) => return Err((*why).to_owned()),
        TheDeposit::Open(deposit) => deposit,
    };
    let workdir = arrival.anchor.worktree.clone();
    let record = actions::presence::claim_record(&actions::presence::Claim {
        agent: agent_of(request),
        key: actions::presence::terminal_claim_key(&arrival.anchor.tty),
        repository: repository_holding(&workdir).unwrap_or_else(|| workdir.clone()),
        branch: branch_of(&workdir),
        workdir: Some(workdir),
        // A terminal takes the tree: what an agent will touch is not known when
        // it arrives, and the prudent answer is the one already written down.
        paths: Vec::new(),
        doing: None,
        pid: std::process::id(),
        at: request.at,
        lease_seconds: actions::presence::DEFAULT_LEASE_SECONDS,
        conversation: arrival.session_id.clone(),
        state: state.to_owned(),
    });
    deposit
        .put_record(&record)
        .map_err(|error| error.to_string())
}

/// The other end: the terminal closes, and stops holding anything.
pub(super) fn stop_announcing(request: &Request<'_>, arrival: &Arrival) -> Result<(), String> {
    let deposit = match request.deposit {
        TheDeposit::NobodyNeedsItHere => return Ok(()),
        // A terminal that cannot stop announcing keeps its claim until the
        // lease runs out: worth saying, never worth failing the hook for.
        TheDeposit::WouldNotOpen(why) => return Err((*why).to_owned()),
        TheDeposit::Open(deposit) => deposit,
    };
    let key = actions::presence::terminal_claim_key(&arrival.anchor.tty);
    actions::presence::release_claim(deposit, &key, request.at)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// What the greeting says about them, and **nothing where there is nothing**:
/// a greeting that reports emptiness every time teaches nobody to read it.
pub(super) fn what_is_still_open(found: &StillOpen) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    for (runs, key) in [
        (&found.waiting, "cli.session.runs_waiting_for_a_person"),
        (&found.ask_again, "cli.session.runs_to_ask_again"),
    ] {
        if runs.is_empty() {
            continue;
        }
        let which: Vec<String> = runs.iter().map(run_named).collect();
        lines.push(catalogue::say(
            key,
            &[
                ("count", &runs.len().to_string()),
                ("which", &which.join(", ")),
                ("first", &runs[0].run_id),
            ],
        ));
    }
    if found.elsewhere > 0 {
        lines.push(catalogue::say(
            "cli.session.runs_waiting_elsewhere",
            &[("count", &found.elsewhere.to_string())],
        ));
    }
    if !found.remembered.is_empty() {
        let recent: Vec<String> = found
            .remembered
            .iter()
            .take(3)
            .map(|memory| format!("«{}»", memory.label))
            .collect();
        lines.push(catalogue::say(
            "cli.session.remembered",
            &[
                ("count", &found.remembered.len().to_string()),
                ("recent", &recent.join(", ")),
            ],
        ));
    }
    if let Some(missed) = found.handover.as_ref().filter(|m| m.missed() > 0) {
        lines.push(catalogue::say(
            "cli.session.handovers_missed",
            &[
                ("missed", &missed.missed().to_string()),
                ("owed", &missed.owed.to_string()),
                ("said", missed.said.as_deref().unwrap_or("")),
            ],
        ));
    }
    if let Some(page) = &found.page {
        lines.push(catalogue::say(
            "cli.session.memory_page",
            &[("path", &page.path.display().to_string())],
        ));
    }
    if let Some(unseen) = &found.page_unseen {
        let files: Vec<String> = unseen
            .files
            .iter()
            .map(|file| file.display().to_string())
            .collect();
        lines.push(catalogue::say(
            "cli.session.page_unseen",
            &[("engine", &unseen.engine), ("files", &files.join(", "))],
        ));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// The welcome, in the wrapper that gets injected into the session's context.
///
/// **A WRAPPER AND NOT A PRINTED LINE** because `SessionStart` is one of the
/// four moments where what the hook writes becomes context the agent reads. A
/// plain line would be read by the person at the screen and not by the agent,
/// and detaching would stay a thing that exists and nobody knows about.
pub(super) fn welcome(
    arrival: &Arrival,
    store: Option<&Sessions>,
    open: &Result<Option<StillOpen>, String>,
    announced: &Result<(), String>,
    handed_on: Option<String>,
) -> String {
    let mut text = catalogue::say(
        "cli.session.welcome",
        &[
            ("tty", &arrival.anchor.tty),
            ("worktree", &arrival.anchor.worktree),
        ],
    );
    // THE RULES OF THE TREE TRAVEL ON THE SAME CHANNEL AS THE GREETING, which
    // was already the only one that reaches whoever works here.
    let rules = rules_in(std::path::Path::new(&arrival.anchor.worktree));
    let here: Vec<&str> = rules
        .iter()
        .filter(|rule| rule.there)
        .map(|rule| rule.name.as_str())
        .collect();
    if !here.is_empty() {
        text.push('\n');
        text.push_str(&catalogue::say(
            "cli.session.rules",
            &[("files", &here.join(", "))],
        ));
    }
    // **A DECLARATION THAT MISSES IS NEWS.** The project still points whoever
    // arrives at a document that is not there; said nowhere, the declaration
    // stays wrong until somebody opens the file for another reason.
    let gone: Vec<&str> = rules
        .iter()
        .filter(|rule| !rule.there)
        .map(|rule| rule.name.as_str())
        .collect();
    if !gone.is_empty() {
        text.push('\n');
        text.push_str(&catalogue::say(
            "cli.session.rules_gone",
            &[("files", &gone.join(", "))],
        ));
    }
    if let Some(said) = store.and_then(|store| neighbours(arrival, store)) {
        text.push('\n');
        text.push_str(&said);
    }
    // **«I COULD NOT LOOK» IS NOT «NOTHING IS OPEN»**, and the greeting is the
    // one place where the two get confused: a silent line reads like a quiet
    // machine. So a ledger that would not open says so, with the reason.
    match open {
        Ok(Some(found)) => {
            if let Some(said) = what_is_still_open(found) {
                text.push('\n');
                text.push_str(&said);
            }
        }
        Ok(None) => {}
        Err(why) => {
            text.push('\n');
            text.push_str(&catalogue::say(
                "cli.session.ledger_did_not_open",
                &[("why", why)],
            ));
        }
    }
    // **AN ANNOUNCEMENT THAT DID NOT GO IS SAID HERE**, because the survey will
    // then show this terminal as nobody, and whoever reads it will conclude the
    // tree is empty when it is not.
    if let Err(why) = announced {
        text.push('\n');
        text.push_str(&catalogue::say(
            "cli.session.not_announced",
            &[("why", why)],
        ));
    }
    // **LAST, AND NOT FIRST.** What the session before left is the longest
    // thing on this channel, and a greeting that opens with it buries the
    // terminal's own name under somebody else's work.
    if let Some(said) = handed_on {
        text.push('\n');
        text.push_str(&said);
    }
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text,
        }
    })
    .to_string()
}
