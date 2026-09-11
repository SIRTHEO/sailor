//! `sailor remaining`: how much quota **a person** has already spent, read off
//! the engine rather than asked of whoever is working.
//!
//! **A COMMAND OF ITS OWN, NOT A LINE INSIDE `flow cost`.** It answers another
//! question, and the same report would make the two read as one. `flow cost`
//! answers «what did **this run** cost»; this answers «what is **left to her**,
//! counting everything she did elsewhere». One number under the other, in the
//! same box, gets subtracted — that is how a right measure becomes a false
//! conclusion.
//!
//! **IT COSTS NOTHING.** It invokes no engine: it asks an address how much was
//! already spent. So it can be called before deciding whether to launch
//! anything, the one moment it is useful.

use models::remaining::Remaining;

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("sailor remaining: {message}");
            1
        }
    }
}

/// The shape of `sailor remaining`. See `flow_cmd::USAGE`.
pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor remaining",
    says_key: "",
}];

fn dispatch(args: &[String]) -> Result<String, String> {
    if !args.is_empty() {
        return Err(format!(
            "{} {}",
            catalogue::say("cli.usage_heading", &[]),
            USAGE[0].form
        ));
    }
    let now = now_secs()?;
    // **EVERY ENGINE THAT DECLARES A CHANNEL, NONE NAMED HERE.** The catalogue
    // says who can be asked; an engine that cannot is not in the list, and a
    // channel that does not answer is a line saying so, never a zero.
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    let readings = per_profile(&catalog, &machine, now);
    // **AN ENGINE ASKED ONCE PER ACCOUNT WAS STILL ASKED.** The line carries
    // the account beside the engine, and the list of the unasked is about
    // engines: matched whole, every engine with profiles would read as one
    // nobody looked at.
    let asked: Vec<String> = readings
        .iter()
        .map(|reading| engine_of(&reading.engine))
        .collect();
    let mut found = Vec::new();
    let mut refused = Vec::new();
    for reading in readings {
        match reading.result {
            Ok(windows) => found.extend(windows),
            Err(why) => refused.push(format!("{} · cannot read: {why}", reading.engine)),
        }
    }
    // **AN ENGINE ON THIS MACHINE THAT NOBODY ASKED IS A LINE, NOT A SILENCE.**
    // Only the engines whose descriptor declares a channel are read, and the
    // rest used to be absent from the report altogether — so a person reading
    // one engine's three windows concluded the others have no quota, when in
    // fact nobody looked. That is the same mistake as a zero standing in for
    // «I did not look», one step further out.
    let report_of_the_machine = toolbox::detect(&catalog, &machine);
    let present: Vec<String> = report_of_the_machine
        .findings
        .into_iter()
        .filter(|found| found.family == "ai_cli" && found.presence.is_present())
        .map(|found| found.descriptor_id)
        .collect();
    let unasked = never_asked(&present, &asked);
    if found.is_empty() && refused.is_empty() && unasked.is_empty() {
        return Err(catalogue::say("cli.remaining.no_channel", &[]));
    }
    if found.is_empty() && refused.is_empty() {
        return Err(said_of_the_unasked(&unasked, &catalog).join("\n"));
    }
    if found.is_empty() {
        return Err(refused
            .into_iter()
            .chain(said_of_the_unasked(&unasked, &catalog))
            .collect::<Vec<_>>()
            .join("\n"));
    }
    let mut said = report(&found);
    for line in refused.into_iter().chain(said_of_the_unasked(&unasked, &catalog)) {
        said.push('\n');
        said.push_str(&line);
    }
    Ok(said)
}

/// Every account, not only the one whose home the engine would use on its own.
///
/// **A QUOTA BELONGS TO AN ACCOUNT, AND A PERSON HOLDS SEVERAL.** Read at the
/// engine's usual home the answer is one account's under the engine's name, so
/// the others look like they have none. Where a profile moves the home the
/// reading follows it; where no profile names that engine, the usual home answers.
fn per_profile(
    catalog: &toolbox::Catalog,
    machine: &toolbox::Machine,
    now: i64,
) -> Vec<toolbox::quota::Reading> {
    let store = profiles::store_io::load_store().unwrap_or_default();
    let mut out = Vec::new();
    for loaded in catalog.live() {
        let descriptor = &loaded.descriptor;
        if toolbox::quota::channel_of(descriptor, machine).is_none() {
            continue;
        }
        let mut asked_for_one = false;
        for profile in homes_for(&store, descriptor) {
            asked_for_one = true;
            if let Some(reading) = toolbox::quota::read_in_home(descriptor, machine, &profile.home_dir, now) {
                let whose = format!("{} · {}", reading.engine, profile.name);
                // **THE LINE SAYS WHOSE IT IS, MEASURED OR REFUSED ALIKE.** The
                // window carries the engine's name from the channel, and three
                // accounts of one engine printed under that name read as one
                // account measured three times.
                out.push(toolbox::quota::Reading {
                    engine: whose.clone(),
                    result: reading.result.map(|windows| {
                        windows
                            .into_iter()
                            .map(|window| models::remaining::Remaining {
                                engine: whose.clone(),
                                ..window
                            })
                            .collect()
                    }),
                });
            }
        }
        if !asked_for_one {
            out.extend(toolbox::quota::read_one(descriptor, machine, now));
        }
    }
    out
}

/// The profiles whose command line is the one this descriptor detects. **THE
/// LINK IS THE EXECUTABLE, NEVER THE NAME**: the descriptors call it
/// `claude-code` and the profiles call it `claude`, and matching on either
/// name would hand one command line another one's accounts.
fn homes_for<'a>(
    store: &'a profiles::ProfileStore,
    descriptor: &toolbox::Descriptor,
) -> Vec<&'a profiles::Profile> {
    let Some(detected) = descriptor.detect.as_ref().and_then(|probes| probes.as_slice().first()).and_then(|probe| probe.command.as_deref()) else {
        return Vec::new();
    };
    store
        .profiles
        .iter()
        .filter(|profile| {
            profiles::find_cli(&profile.cli_id).is_ok_and(|cli| cli.executable == detected)
        })
        .collect()
}

/// The engine's own name, without the account the line names beside it.
fn engine_of(said: &str) -> String {
    said.split(" · ").next().unwrap_or(said).to_string()
}

/// The engines on this machine that nothing asked, in the order they were
/// found. **Nobody looked is not «nothing to look at»**, and the difference is
/// the whole reason this list exists.
pub fn never_asked(present: &[String], asked: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for engine in present {
        if !asked.iter().any(|one| one == engine) && !out.contains(engine) {
            out.push(engine.clone());
        }
    }
    out
}

/// One line each, saying what is missing rather than what was measured — or,
/// where somebody looked and found nothing to read, what they found.
fn said_of_the_unasked(engines: &[String], catalog: &toolbox::Catalog) -> Vec<String> {
    engines
        .iter()
        .map(|engine| match written_off(catalog, engine) {
            Some(why) => format!("{engine} · no quota to read: {why}"),
            None => catalogue::say("cli.remaining.no_channel_declared", &[("engine", engine)]),
        })
        .collect()
}

/// What the descriptor says about the absence, when it says anything.
fn written_off(catalog: &toolbox::Catalog, engine: &str) -> Option<String> {
    catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == engine)
        .and_then(|loaded| toolbox::quota::declared_absent(&loaded.descriptor).map(str::to_owned))
}

/// One person's quotas, one per line.
///
/// **IT SAYS «USED», NOT «LEFT».** The provider declares how much has gone; the
/// rest would be a subtraction of ours, and on a window that never says what its
/// ceiling is a subtraction is an invention. The command's name states the
/// question, the line states the measure.
fn report(found: &[Remaining]) -> String {
    if found.is_empty() {
        return catalogue::say("cli.remaining.no_window", &[]);
    }
    let mut lines = vec![catalogue::say("cli.remaining.whose_quota", &[])];
    for entry in found {
        let resets = match &entry.resets_at {
            Some(when) => format!(", resets on {when}"),
            None => String::new(),
        };
        lines.push(format!(
            "{} · {}: used {:.1}%{resets}",
            entry.engine,
            entry.unit,
            entry.used_fraction * 100.0
        ));
    }
    lines.join("\n")
}

fn now_secs() -> Result<i64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .map_err(|error| catalogue::say("cli.clock_before_epoch", &[("error", &error.to_string())]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::remaining::RemainingError;

    fn a_window(unit: &str, used_fraction: f64, resets_at: Option<&str>) -> Remaining {
        Remaining {
            engine: "an-engine".to_owned(),
            unit: unit.to_owned(),
            used_fraction,
            resets_at: resets_at.map(str::to_owned),
            observed_at: 1_788_000_000,
        }
    }

    /// **AN ENGINE NOBODY ASKED IS NOT AN ENGINE WITHOUT A QUOTA.** The report
    /// read only those declaring a channel and the rest vanished from it: a
    /// zero standing in for «I did not look», one step further out.
    #[test]
    fn an_engine_nobody_asked_is_not_an_engine_without_a_quota() {
        let present = vec![
            "unmotore".to_owned(),
            "un-altro".to_owned(),
            "senza-casa".to_owned(),
        ];
        let asked = vec!["un-altro".to_owned()];
        assert_eq!(
            never_asked(&present, &asked),
            vec!["unmotore".to_owned(), "senza-casa".to_owned()]
        );

        // The absurd control: all asked, no extra line. Without this arm the
        // function could list everything always and still pass.
        assert!(never_asked(&present, &present).is_empty());
        // And an engine the detection names twice stays one line.
        assert_eq!(
            never_asked(&["unmotore".to_owned(), "unmotore".to_owned()], &[]),
            vec!["unmotore".to_owned()]
        );
    }

    /// **THE LINE SAYS WHOSE QUOTA IT IS BEFORE SAYING HOW MUCH.** That warning
    /// is what makes the number usable: without it, a reader who meets it under
    /// a spend report attributes it to the run just looked at, and a seven-day
    /// quota attributed to a ten-minute run is off by two orders of magnitude.
    #[test]
    fn the_report_says_whose_quota_it_is_before_saying_how_much() {
        let said = report(&[a_window("seven_day", 0.32, None)]);
        let first = said.lines().next().expect("at least one line");
        assert!(
            first.contains("PERSON") && first.contains("not a run"),
            "the warning goes at the top, not the bottom: {said}"
        );
    }

    #[test]
    fn every_window_is_a_line_with_its_reset() {
        let said = report(&[
            a_window("five_hour", 0.5, Some("2026-09-01T03:29:59+00:00")),
            a_window("seven_day", 0.32, None),
        ]);
        assert!(
            said.contains("an-engine · five_hour: used 50.0%"),
            "{said}"
        );
        assert!(
            said.contains("resets on 2026-09-01T03:29:59+00:00"),
            "{said}"
        );
        assert!(
            said.contains("an-engine · seven_day: used 32.0%"),
            "{said}"
        );
        assert!(
            !said.contains("seven_day: used 32.0%, resets"),
            "an instant the provider does not give is not invented: {said}"
        );
    }

    /// **NO WINDOW IS NOT «QUOTA FREE».** An answer carrying no measures is an
    /// answer carrying no measures, and saying so with a zero would send people
    /// launching at exactly the moment nobody knows.
    #[test]
    fn no_window_at_all_is_said_and_never_shown_as_zero() {
        let said = report(&[]);
        assert!(said.contains("no quota window"), "{said}");
        assert!(!said.contains("0.0%"), "{said}");
    }

    /// The channel is beta: when it stops answering, the command says what
    /// happened instead of pretending it measured something.
    #[test]
    fn a_channel_that_does_not_answer_is_reported_and_not_guessed() {
        let said = format!("{}", RemainingError::NotUnderstood);
        assert!(said.contains("beta"), "{said}");
    }
}
