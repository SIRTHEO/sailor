//! `sailor terminal`: runs a command line inside a terminal Sailor owns,
//! inside whatever emulator the person is already using.
//!
//! `session` tracks and never enters, which is right for tracking and wrong
//! for acting: nothing can be typed into a terminal nobody owns. Here Sailor
//! owns the descriptor, so the emulator never has to be recognised.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::fd::FromRawFd;
use sessions::fullness::{self, Model};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrder};
use std::sync::Arc;
use terminal::bridge::{self, Keys, RawMode};
use terminal::inbox::{self, Inbox};
use terminal::mandate;
use terminal::pty::Pty;
use terminal::tally::{self, Tally};
use terminal::Workspace;

pub const USAGE: &[&str] = &[
    "sailor terminal run [--store <dir>] -- <cli> [args...]   runs a command line in a terminal Sailor owns",
    "sailor terminal press --tty <name> --text <line> [--store <dir>]   types a line into a terminal Sailor holds",
    "sailor terminal reset --tty <name> --cli <id> [--store <dir>]   empties a running session the way its descriptor says",
    "sailor terminal mandate [--tty <name>] [--store <dir>]   < text   leaves the work for whoever comes next",
    "sailor terminal list [--ceiling <tokens>] [--store <dir>]   which terminals can be typed into, and how full",
];

const FORMS: &[&str] = &["run", "press", "reset", "mandate", "list"];

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
        return Err(format!("«{form}» is not a form of this command\n{}", usage_text()));
    }
    match form.as_str() {
        "run" => hold(&args[1..]),
        "press" => press(&args[1..]),
        "reset" => reset(&args[1..]),
        "mandate" => leave_mandate(&args[1..], &mut std::io::stdin()),
        "list" => list(&args[1..]),
        other => Err(format!("«{other}» is not a form of this command")),
    }
}

fn usage_text() -> String {
    USAGE.join("\n")
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
    ledger::default_directory().ok_or_else(|| "I cannot tell where the store lives".to_owned())
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
        return Err("nothing to run: give a command line after `--`".to_owned());
    };
    let rest: Vec<&OsStr> = line[1..].iter().map(OsStr::new).collect();

    let here = std::env::current_dir().map_err(|error| error.to_string())?;
    let workspace = Workspace::open(&here).map_err(|error| error.to_string())?;
    let size = bridge::size_of(0).unwrap_or_default();
    let inner = Arc::new(
        Pty::open(&workspace, OsStr::new(program), &rest, size, &[])
            .map_err(|error| error.to_string())?,
    );

    // The letterbox is named after the terminal the program inside sees, not
    // the one this process was started from. Those are two different ttys, and
    // the tracking store records the inner one: keying on the outer would
    // leave whoever reads that store knocking at an address nobody holds.
    let tty = sessions::tty::short_name(inner.device());
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

    let counted = Counters::new();
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

/// The bytes each direction has moved, shared with whoever writes them down.
struct Counters {
    shown: Arc<AtomicU64>,
    typed: Arc<AtomicU64>,
}

impl Counters {
    fn new() -> Counters {
        Counters {
            shown: Arc::new(AtomicU64::new(0)),
            typed: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Keeps the count on disk while the session runs.
    ///
    /// On disk and not in here, because whoever reads it is another process
    /// that may start after this one and must not have to ask it anything.
    fn recorded_into(&self, path: PathBuf) -> Recording {
        let running = Arc::new(AtomicBool::new(true));
        let shown = Arc::clone(&self.shown);
        let typed = Arc::clone(&self.typed);
        let going = Arc::clone(&running);
        let writing = std::thread::spawn(move || {
            let mut last = Tally::default();
            while going.load(AtomicOrder::Relaxed) {
                last = record(&path, &shown, &typed, last);
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            record(&path, &shown, &typed, last);
        });
        Recording { running, writing }
    }
}

/// Writes the count if it moved, and gives back what is now on disk.
fn record(path: &Path, shown: &AtomicU64, typed: &AtomicU64, last: Tally) -> Tally {
    let now = Tally {
        shown: shown.load(AtomicOrder::Relaxed),
        typed: typed.load(AtomicOrder::Relaxed),
        at: sessions::now(),
    };
    if now.shown == last.shown && now.typed == last.typed {
        return last;
    }
    let _ = tally::write(path, &now);
    now
}

struct Recording {
    running: Arc<AtomicBool>,
    writing: std::thread::JoinHandle<()>,
}

impl Recording {
    /// Stops and waits, so the last count is on disk before the terminal's row
    /// disappears: a session that ended full must not read as one that ended
    /// empty.
    fn stop(self) {
        self.running.store(false, AtomicOrder::Relaxed);
        let _ = self.writing.join();
    }
}

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
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;
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
    let tty = named(&options, "tty", "which terminal? give --tty <name>")?;
    let text = named(&options, "text", "what should be typed? give --text <line>")?;
    press_into(&options, &tty, &text)?;
    println!("typed into {tty}");
    Ok(0)
}

/// One option by name, or the sentence that says what is missing.
fn named(options: &[(String, String)], name: &str, missing: &str) -> Result<String, String> {
    options
        .iter()
        .find(|(written, _)| written == name)
        .map(|(_, value)| value.clone())
        .ok_or_else(|| missing.to_owned())
}

/// Types a line into a terminal Sailor holds.
///
/// The carriage return is what a terminal receives when someone presses Enter;
/// a newline would leave the line sitting there unsent.
fn press_into(options: &[(String, String)], tty: &str, line: &str) -> Result<(), String> {
    let address = mailroom(options)?.join(format!("{tty}.sock"));
    let mut typed = line.as_bytes().to_vec();
    typed.push(b'\r');
    inbox::press(&address, &typed).map_err(|error| {
        format!(
            "{}: Sailor is not holding this terminal ({error})",
            address.display()
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
    let tty = named(&options, "tty", "which terminal? give --tty <name>")?;
    let cli = named(&options, "cli", "which command line is running there? give --cli <id>")?;

    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    let line = reset_line_of(&catalog, &cli)?;

    press_into(&options, &tty, &line)?;
    println!("typed {line} into {tty}");
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
        .ok_or_else(|| format!("«{cli}»: no descriptor of that name is loaded"))?;
    known
        .descriptor
        .reset_line()
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "«{cli}» does not declare how a running session of it is emptied. \
                 Nobody has measured it, which is not the same as it being impossible: \
                 add `reset_context` to its descriptor rather than guessing a line here"
            )
        })
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
            .ok_or_else(|| "this is not running in a terminal: give --tty <name>".to_owned())?,
    };
    let mut text = String::new();
    from.read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    if text.trim().is_empty() {
        return Err("nothing to hand on: the mandate arrives on standard input".to_owned());
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
    println!("mandate left for {tty}");
    Ok(0)
}

fn list(args: &[String]) -> Result<i32, String> {
    let options = options_of(args)?;
    let ceiling = declared_ceiling(&options)?;
    let room = mailroom(&options)?;
    let Ok(entries) = std::fs::read_dir(&room) else {
        println!("Sailor is holding no terminal");
        return Ok(0);
    };
    let mut found = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|kind| kind.to_str()) != Some("sock") {
            continue;
        }
        // A file left behind by a process that died badly answers nobody: the
        // knock is what tells a live terminal from its leftovers.
        if std::os::unix::net::UnixStream::connect(&path).is_err() {
            continue;
        }
        let Some(name) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
            continue;
        };
        println!("{}", how_full(&room, &name, ceiling));
        found += 1;
    }
    if found == 0 {
        println!("Sailor is holding no terminal");
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
        return format!("{tty}   can be typed into, nothing counted yet");
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
            .map_err(|_| format!("«{written}» is not a number of tokens")),
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
            return Err(format!("«{word}» is not an option"));
        };
        let value = rest
            .next()
            .ok_or_else(|| format!("«--{name}» wants a value after it"))?;
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
        let error = dispatch(&words(&["invented"]))
            .err()
            .expect("an invented form is refused");
        assert!(error.contains("invented"), "the refusal must name it: {error}");
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
            .err()
            .expect("an undeclared command line must refuse");
        assert!(refusal.contains("does not declare"), "{refusal}");
        assert!(
            refusal.contains("reset_context"),
            "the refusal must say where to write it: {refusal}"
        );
    }

    #[test]
    fn a_command_line_nobody_ever_heard_of_is_refused_by_name() {
        let refusal = reset_line_of(&shipped(), "rossignol")
            .err()
            .expect("an unknown command line must refuse");
        assert!(refusal.contains("rossignol"), "{refusal}");
    }

    fn scratch(name: &str) -> PathBuf {
        let directory =
            PathBuf::from("/tmp").join(format!("sr-cmd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create the test directory");
        directory
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
            .err()
            .expect("an empty mandate is refused");
        assert!(refusal.contains("nothing to hand on"), "{refusal}");
        assert_eq!(mandate::read(&mandate::address_in(&directory, "ttys004")), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn running_nothing_is_refused_instead_of_opening_an_empty_terminal() {
        let error = hold(&words(&["--"]))
            .err()
            .expect("an empty command line is refused");
        assert!(error.contains("nothing to run"), "{error}");
    }
}
