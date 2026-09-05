//! `sailor terminal`: runs a command line inside a terminal Sailor owns,
//! inside whatever emulator the person is already using.
//!
//! `session` tracks and never enters, which is right for tracking and wrong
//! for acting: nothing can be typed into a terminal nobody owns. Here Sailor
//! owns the descriptor, so the emulator never has to be recognised.

use crate::Form;
use sessions::fullness::{self, Model};
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrder};
use std::sync::Arc;
use terminal::bridge::{self, Keys, RawMode};
use terminal::inbox::{self, Inbox};
use terminal::mandate;
use terminal::pty::Pty;
use terminal::tally;
use terminal::Workspace;

pub const USAGE: &[Form] = &[
    Form {
        form: "sailor terminal run [--store <dir>] -- <cli> [args...]",
        says_key: "cli.terminal.form.run",
    },
    Form {
        form: "sailor terminal press --tty <name> --text <line> [--store <dir>]",
        says_key: "cli.terminal.form.press",
    },
    Form {
        form: "sailor terminal reset --tty <name> --cli <id> [--store <dir>]",
        says_key: "cli.terminal.form.reset",
    },
    Form {
        form: "sailor terminal mandate [--tty <name>] [--store <dir>] < text",
        says_key: "cli.terminal.form.mandate",
    },
    Form {
        form: "sailor terminal list [--ceiling <tokens>] [--store <dir>]",
        says_key: "cli.terminal.form.list",
    },
    Form {
        form: "sailor terminal host [--store <dir>]",
        says_key: "cli.terminal.form.host",
    },
];

const FORMS: &[&str] = &["run", "press", "reset", "mandate", "list", "host"];

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(code) => code,
        Err(complaint) => {
            eprintln!("sailor terminal: {complaint}");
            2
        }
    }
}

fn dispatch(args: &[String]) -> Result<i32, String> {
    let Some(form) = args.first() else {
        return Err(usage_text());
    };
    if form == "--help" || form == "-h" {
        println!("{}", usage_text());
        return Ok(0);
    }
    if !FORMS.contains(&form.as_str()) {
        return Err(format!(
            "{}\n{}",
            catalogue::say("cli.no_such_form", &[("verb", form)]),
            usage_text()
        ));
    }
    match form.as_str() {
        "run" => hold(&args[1..]),
        "press" => press(&args[1..]),
        "reset" => reset(&args[1..]),
        "mandate" => leave_mandate(&args[1..], &mut std::io::stdin()),
        "list" => list(&args[1..]),
        "host" => host(&args[1..]),
        other => Err(catalogue::say("cli.no_such_form", &[("verb", other)])),
    }
}

fn usage_text() -> String {
    crate::forms_as_lines(USAGE).join("\n")
}

/// Where the letterboxes live: beside the store, unless told otherwise.
///
/// The override is not a convenience for tests. Without it every check of this
/// command would have to write into the store of whoever runs it, and a test
/// that touches the real machine is the fault this repo has already paid for.
fn store_root(options: &[(String, String)]) -> Result<PathBuf, String> {
    if let Some((_, declared)) = options.iter().find(|(name, _)| name == "store") {
        return Ok(PathBuf::from(declared));
    }
    ledger::default_directory().ok_or_else(|| catalogue::say("cli.terminal.no_store", &[]))
}

fn mailroom(options: &[(String, String)]) -> Result<PathBuf, String> {
    Ok(inbox::mailroom(&store_root(options)?))
}

/// Opens a terminal we own, runs the command line inside it, and holds the
/// pipe until the program in there ends.
fn hold(args: &[String]) -> Result<i32, String> {
    let (before, line) = split_at_the_dashes(args);
    let options = options_of(before)?;
    let Some(program) = line.first() else {
        return Err(catalogue::say("cli.terminal.nothing_to_run", &[]));
    };
    let rest: Vec<&OsStr> = line[1..].iter().map(OsStr::new).collect();

    let here = std::env::current_dir().map_err(|error| error.to_string())?;
    let workspace = Workspace::open(&here).map_err(|error| error.to_string())?;
    let size = bridge::size_of(0).unwrap_or_default();
    let opening = under_the_active_profile();
    for why in &opening.refused {
        eprintln!("sailor terminal: {why}");
    }
    let inner = Arc::new(
        Pty::open(&workspace, OsStr::new(program), &rest, size, &opening.environment)
            .map_err(|error| error.to_string())?,
    );

    // The letterbox is named after the terminal the program inside sees, not
    // the one this process was started from. Those are two different ttys, and
    // the tracking store records the inner one: keying on the outer would
    // leave whoever reads that store knocking at an address nobody holds.
    let tty = inner.tty().to_owned();
    let address = mailroom(&options)?.join(format!("{tty}.sock"));
    let letterbox = Inbox::open(&address).map_err(|error| {
        let _ = inner.close();
        error.to_string()
    })?;

    // The outer terminal goes raw before the first byte moves, and comes back
    // when this guard drops.
    let restore = bridge::is_a_terminal(0)
        .then(|| RawMode::take(0))
        .transpose()
        .map_err(|error| error.to_string())?;
    bridge::notice_resizes().map_err(|error| error.to_string())?;

    let counted = tally::Counters::new();
    let counting = counted.recorded_into(mailroom(&options)?.join(format!("{tty}.seen")));

    typing_reaches(&inner, letterbox, Arc::clone(&counted.typed));
    keystrokes_reach(&inner, Arc::clone(&counted.typed));
    let showing = show_output(&inner, Arc::clone(&counted.shown));

    counting.stop();
    drop(restore);
    let _ = std::fs::remove_file(&address);
    let _ = std::fs::remove_file(mailroom(&options)?.join(format!("{tty}.seen")));
    showing.map_err(|error| error.to_string())?;
    Ok(exit_code_of(&inner))
}

/// What a terminal of this command opens with. **THE WINDOW DID THIS AND THE
/// COMMAND DID NOT**: a terminal opened here ran under whatever the outer
/// shell happened to carry, so an engine lit from it answered as somebody the
/// profile in force does not name. A store that cannot be read is said out
/// loud, never passed for a store with nothing active.
fn under_the_active_profile() -> profiles::ActiveEnvironment {
    match profiles::store_io::load_store() {
        Ok(store) => profiles::active_environment_with(&store, &|name| std::env::var(name).ok()),
        Err(why) => profiles::ActiveEnvironment {
            environment: Vec::new(),
            refused: vec![why],
        },
    }
}

/// Holds the terminals the window opens, in a process that is not the window.
///
/// **THE WINDOW CLOSES; THIS DOES NOT.** The leader end of every pseudo-terminal
/// lives in whoever opened it, and a leader that goes away ends the shell
/// inside. So the window never opens one: it asks this process, which sits in
/// a session of its own and answers on a socket beside the letterboxes.
///
/// Registered in the ledger like everything Sailor starts, so `sailor-live
/// --list` can name it and `--stop` can end it — fault 4 by the other door.
fn host(args: &[String]) -> Result<i32, String> {
    let options = options_of(args)?;
    let store = store_root(&options)?;
    let room = inbox::mailroom(&store);

    // A session of its own, or the terminal that started the window would take
    // this process with it when it closes.
    unsafe {
        libc::setsid();
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }

    let registered = ledger::Ledger::open(&store).ok().and_then(|ledger| {
        let record = ledger::ProcessRecord {
            process_id: HOST_PROCESS_ID.to_owned(),
            pid: std::process::id(),
            command: "sailor".to_owned(),
            args: vec![
                "terminal".to_owned(),
                "host".to_owned(),
                "--store".to_owned(),
                store.display().to_string(),
            ],
            working_directory: std::env::current_dir()
                .map(|here| here.display().to_string())
                .unwrap_or_default(),
            port: None,
            purpose: catalogue::say("cli.terminal.host_purpose", &[]),
            started_by: "sailor terminal host".to_owned(),
            run_id: None,
            started_at: tally::now(),
        };
        ledger.record_process_started(&record).ok().map(|_| ledger)
    });

    let terminals = terminal::Terminals::current().with_mailroom(room);
    let served = terminal::host::serve(
        Arc::new(terminal::host::Host::new(terminals)),
        &terminal::host::address_in(&store),
    );
    if let Some(ledger) = registered {
        let _ = ledger.record_process_ended(&ledger::ProcessEndRecord {
            process_id: HOST_PROCESS_ID.to_owned(),
            exit_code: None,
            ended_at: tally::now(),
        });
    }
    served.map_err(|error| error.to_string())?;
    Ok(0)
}

/// The name the ledger finds the host back by. Not the pid: pids get reused.
pub const HOST_PROCESS_ID: &str = "terminal-host";

/// The thread that hands whatever a stranger left straight to the terminal.
fn typing_reaches(inner: &Arc<Pty>, letterbox: Inbox, typed: Arc<AtomicU64>) {
    let inner = Arc::clone(inner);
    std::thread::spawn(move || {
        letterbox.serve(|bytes| {
            if inner.write(bytes).is_ok() {
                typed.fetch_add(bytes.len() as u64, AtomicOrder::Relaxed);
            }
        });
    });
}

/// The thread that carries what the person types.
fn keystrokes_reach(inner: &Arc<Pty>, typed: Arc<AtomicU64>) {
    let inner = Arc::clone(inner);
    std::thread::spawn(move || {
        let from = unsafe { File::from_raw_fd(libc::dup(0)) };
        let into = bridge::Counted::new(Keys(&inner), typed);
        let _ = bridge::pump(from, into, || follow_the_window(&inner));
    });
}

/// The output, on the thread that stays: when it ends, the program inside has.
fn show_output(inner: &Arc<Pty>, shown: Arc<AtomicU64>) -> io::Result<()> {
    let from = inner
        .reader()
        .map_err(|error| io::Error::other(error.to_string()))?;
    let into = bridge::Counted::new(unsafe { File::from_raw_fd(libc::dup(1)) }, shown);
    bridge::pump(from, into, || follow_the_window(inner))
}

/// A window change is noticed on whichever read the signal happened to
/// interrupt, so both pumps ask the same question.
fn follow_the_window(inner: &Pty) {
    if !bridge::resize_was_noticed() {
        return;
    }
    if let Ok(size) = bridge::size_of(0) {
        let _ = inner.resize(size);
    }
}

/// What the program inside exited with.
///
/// A terminal whose output has ended has almost always lost its child too, but
/// the two are not the same event: reporting a code we never read would be
/// reporting success we did not see.
fn exit_code_of(inner: &Pty) -> i32 {
    for _ in 0..50 {
        match inner.finished() {
            Some(terminal::Ending::Exited(code)) => return code,
            Some(terminal::Ending::Killed) => return 130,
            _ => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
    0
}

fn press(args: &[String]) -> Result<i32, String> {
    let options = options_of(args)?;
    let tty = named(
        &options,
        "tty",
        &catalogue::say("cli.terminal.which_terminal", &[]),
    )?;
    let text = named(
        &options,
        "text",
        &catalogue::say("cli.terminal.what_text", &[]),
    )?;
    press_into(&options, &tty, &text)?;
    println!(
        "{}",
        catalogue::say("cli.terminal.typed_into", &[("tty", &tty)])
    );
    Ok(0)
}

/// One option by name, or the sentence that says what is missing. The sentence
/// arrives already said: asked for by key in here, no scan would see it.
fn named(options: &[(String, String)], name: &str, missing: &str) -> Result<String, String> {
    options
        .iter()
        .find(|(written, _)| written == name)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| missing.to_owned())
}

/// Types a line into a terminal Sailor holds.
///
/// How a line is typed lives in the letterbox and not here: the relay types
/// too, and the two copies of it were drifting apart on the one detail that
/// decides whether the line is ever sent.
fn press_into(options: &[(String, String)], tty: &str, line: &str) -> Result<(), String> {
    let address = mailroom(options)?.join(format!("{tty}.sock"));
    inbox::press_line(&address, line).map_err(|error| {
        catalogue::say(
            "cli.terminal.not_held",
            &[
                ("address", &address.display().to_string()),
                ("error", &error.to_string()),
            ],
        )
    })
}

/// Empties a running session, by typing the line its own descriptor declares.
///
/// The line is never written here. What empties a context is a fact about one
/// command line, and a fact about one command line put into this file would
/// make the relay work for that one and quietly misfire on every other.
fn reset(args: &[String]) -> Result<i32, String> {
    let options = options_of(args)?;
    let tty = named(
        &options,
        "tty",
        &catalogue::say("cli.terminal.which_terminal", &[]),
    )?;
    let cli = named(
        &options,
        "cli",
        &catalogue::say("cli.terminal.which_cli", &[]),
    )?;

    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    let line = reset_line_of(&catalog, &cli)?;

    press_into(&options, &tty, &line)?;
    println!(
        "{}",
        catalogue::say("cli.terminal.typed_line", &[("line", &line), ("tty", &tty)])
    );
    Ok(0)
}

/// What empties a session of this command line, or the sentence that refuses.
///
/// Split out so the refusal can be checked against the shipped catalog alone.
/// Asking the machine here would make the answer depend on what the person
/// running the tests happens to have declared at home.
fn reset_line_of(catalog: &toolbox::Catalog, cli: &str) -> Result<String, String> {
    let known = catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == cli)
        .ok_or_else(|| catalogue::say("cli.terminal.no_such_descriptor", &[("cli", cli)]))?;
    known
        .descriptor
        .reset_line()
        .map(str::to_owned)
        .ok_or_else(|| catalogue::say("cli.terminal.no_reset_declared", &[("cli", cli)]))
}

/// Leaves the work for whoever comes next, written by the session itself.
///
/// Written and not scraped. Looking for a phrase in what the terminal showed
/// is the way a successor is born crippled: the phrase is missing far more
/// often than it is there, and nothing says so until the successor is already
/// running on nothing.
fn leave_mandate(args: &[String], from: &mut impl std::io::Read) -> Result<i32, String> {
    let options = options_of(args)?;
    let tty = match options.iter().find(|(name, _)| name == "tty") {
        Some((_, declared)) => declared.clone(),
        None => sessions::tty::current()
            .ok_or_else(|| catalogue::say("cli.terminal.not_in_a_terminal", &[]))?,
    };
    let mut text = String::new();
    from.read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    if text.trim().is_empty() {
        return Err(catalogue::say("cli.terminal.nothing_to_hand_on", &[]));
    }

    let path = mandate::address_in(&store_root(&options)?, &tty);
    mandate::write(
        &path,
        &mandate::Mandate {
            text,
            at: sessions::now(),
        },
    )
    .map_err(|error| error.to_string())?;
    println!(
        "{}",
        catalogue::say("cli.terminal.mandate_left", &[("tty", &tty)])
    );
    Ok(0)
}

fn list(args: &[String]) -> Result<i32, String> {
    let options = options_of(args)?;
    let ceiling = declared_ceiling(&options)?;
    let room = mailroom(&options)?;
    let Ok(entries) = std::fs::read_dir(&room) else {
        println!("{}", catalogue::say("cli.terminal.none_held", &[]));
        return Ok(0);
    };
    let mut found = 0;
    let host_address = terminal::host::address_in(&store_root(&options)?);
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|kind| kind.to_str()) != Some("sock") {
            continue;
        }
        // The host answers here too, and it is not a terminal.
        if path == host_address {
            continue;
        }
        // A file left behind by a process that died badly answers nobody: the
        // knock is what tells a live terminal from its leftovers.
        if std::os::unix::net::UnixStream::connect(&path).is_err() {
            continue;
        }
        let Some(name) = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
        else {
            continue;
        };
        println!("{}", how_full(&room, &name, ceiling));
        found += 1;
    }
    if found == 0 {
        println!("{}", catalogue::say("cli.terminal.none_held", &[]));
    }
    Ok(0)
}

/// One line about a terminal: what it has moved, and what that is worth.
///
/// A count that is missing says so instead of reading as zero. A terminal
/// whose count has not landed yet is not an empty one, and the two must not
/// print the same.
fn how_full(room: &Path, tty: &str, ceiling: u64) -> String {
    let Some(counted) = tally::read(&room.join(format!("{tty}.seen"))) else {
        return catalogue::say("cli.terminal.nothing_counted_yet", &[("tty", tty)]);
    };
    let reading = fullness::measure(counted.total(), &Model::default(), ceiling);
    let verdict = match (ceiling, reading.past_the_ceiling) {
        (0, _) => "no ceiling declared".to_owned(),
        (_, true) => format!("past the {ceiling} ceiling"),
        (_, false) => format!("under the {ceiling} ceiling"),
    };
    format!(
        "{tty}   {} bytes, about {} tokens, {verdict}",
        counted.total(),
        reading.estimated_tokens
    )
}

/// The ceiling somebody declared, or none.
///
/// None and not a number chosen here: what counts as too full is a decision
/// about a budget, and one taken quietly in this file could not be argued with.
fn declared_ceiling(options: &[(String, String)]) -> Result<u64, String> {
    match options.iter().find(|(name, _)| name == "ceiling") {
        Some((_, written)) => written
            .parse()
            .map_err(|_| catalogue::say("cli.terminal.not_a_token_count", &[("written", written)])),
        None => Ok(0),
    }
}

/// What was written before the dashes, and the command line after them.
///
/// The separator is what keeps a command line from being read as options: a
/// program called `list` is a program, not this command's own form.
fn split_at_the_dashes(args: &[String]) -> (&[String], &[String]) {
    match args.iter().position(|word| word == "--") {
        Some(at) => (&args[..at], &args[at + 1..]),
        None => (&[], args),
    }
}

/// `--name value` pairs, in the order they were written.
fn options_of(args: &[String]) -> Result<Vec<(String, String)>, String> {
    let mut found = Vec::new();
    let mut rest = args.iter();
    while let Some(word) = rest.next() {
        let Some(name) = word.strip_prefix("--") else {
            return Err(catalogue::say("cli.unknown_option", &[("option", word)]));
        };
        let value = rest.next().ok_or_else(|| {
            catalogue::say(
                "cli.option_wants_a_value",
                &[("option", &format!("--{name}"))],
            )
        })?;
        found.push((name.to_owned(), value.clone()));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(of: &[&str]) -> Vec<String> {
        of.iter().map(|word| (*word).to_owned()).collect()
    }

    #[test]
    fn every_form_the_usage_promises_is_accepted() {
        for line in USAGE {
            let promised = line
                .form
                .split_whitespace()
                .nth(2)
                .expect("every usage line names its own form");
            assert!(
                FORMS.contains(&promised),
                "«{promised}» is promised by the help and the dispatch does not accept it"
            );
        }
    }

    #[test]
    fn a_form_nobody_wrote_is_refused_by_name() {
        let error = dispatch(&words(&["invented"])).expect_err("an invented form is refused");
        assert!(
            error.contains("invented"),
            "the refusal must name it: {error}"
        );
    }

    #[test]
    fn options_come_out_in_pairs() {
        let read = options_of(&words(&["--tty", "ttys004"])).expect("two words");
        assert_eq!(read, vec![("tty".to_owned(), "ttys004".to_owned())]);
    }

    #[test]
    fn an_option_without_a_value_is_refused() {
        assert!(options_of(&words(&["--tty"])).is_err());
    }

    /// A program called `list` is a program, not this command's own form.
    #[test]
    fn what_comes_after_the_dashes_is_never_read_as_an_option() {
        let written = words(&["--store", "/tmp/x", "--", "list"]);
        let (before, line) = split_at_the_dashes(&written);
        assert_eq!(before, words(&["--store", "/tmp/x"]).as_slice());
        assert_eq!(line, words(&["list"]).as_slice());
    }

    fn shipped() -> toolbox::Catalog {
        toolbox::Catalog::load(&[toolbox::Source::Builtin])
    }

    #[test]
    fn a_command_line_that_declares_how_it_empties_gives_its_line() {
        let line = reset_line_of(&shipped(), "claude-code").expect("it is declared");
        assert!(!line.is_empty());
    }

    /// The whole point. A command line nobody has measured must stop the relay,
    /// not inherit a line that belongs to a different one.
    #[test]
    fn a_command_line_nobody_measured_is_refused_and_told_where_to_say_it() {
        let refusal = reset_line_of(&shipped(), "codex")
            .expect_err("an undeclared command line must refuse");
        assert!(refusal.contains("does not declare"), "{refusal}");
        assert!(
            refusal.contains("reset_context"),
            "the refusal must say where to write it: {refusal}"
        );
    }

    #[test]
    fn a_command_line_nobody_ever_heard_of_is_refused_by_name() {
        let refusal = reset_line_of(&shipped(), "rossignol")
            .expect_err("an unknown command line must refuse");
        assert!(refusal.contains("rossignol"), "{refusal}");
    }

    fn scratch(name: &str) -> PathBuf {
        terminal::scratch::directory(&format!("cmd-{name}"))
    }

    #[test]
    fn a_mandate_left_is_a_mandate_the_next_beat_can_read() {
        let directory = scratch("mandate");
        let written = words(&[
            "--tty",
            "ttys004",
            "--store",
            directory.to_str().expect("a path"),
        ]);
        let code = leave_mandate(&written, &mut "carry the conduit on".as_bytes())
            .expect("leaving a mandate works");
        assert_eq!(code, 0);

        let found = mandate::read(&mandate::address_in(&directory, "ttys004"))
            .expect("the mandate is there to be read");
        assert_eq!(found.text, "carry the conduit on");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Nothing on standard input is a mistake, not an empty mandate. Written
    /// as one it would start a successor with nothing to do.
    #[test]
    fn handing_on_nothing_is_refused() {
        let directory = scratch("mandate-empty");
        let written = words(&[
            "--tty",
            "ttys004",
            "--store",
            directory.to_str().expect("a path"),
        ]);
        let refusal = leave_mandate(&written, &mut "   \n".as_bytes())
            .expect_err("an empty mandate is refused");
        assert!(refusal.contains("nothing to hand on"), "{refusal}");
        assert_eq!(
            mandate::read(&mandate::address_in(&directory, "ttys004")),
            None
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn running_nothing_is_refused_instead_of_opening_an_empty_terminal() {
        let error = hold(&words(&["--"])).expect_err("an empty command line is refused");
        assert!(error.contains("nothing to run"), "{error}");
    }
}
