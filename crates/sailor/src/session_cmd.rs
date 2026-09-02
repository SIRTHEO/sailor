//! `sailor session`: **the one door** into terminal tracking.
//!
//! **THE PRINCIPLE.** Sailor does not walk into the terminal: the agent — or
//! the shell — announces itself. A hook sends its payload on standard input and
//! this command records it. There is no other way in, and no product-specific
//! code: **it reads no program's environment variable and names no terminal.**
//!
//! **THE ANCHOR IS `(tty, tree, progenitor)`.** The tty from our own descriptor
//! or from the first ancestor that has one, the tree from the payload, the
//! progenitor from the census — and the progenitor **is a label**: it is
//! printed and recorded, no condition reads it. `no_product_name_decides_anything`
//! holds the iron rule: a product's name may appear in a label, never in a
//! condition.
//!
//! **THE CENSUS IS TRIGGERED, NOT ON A CLOCK.** The machine is looked at when
//! an event arrives and at no other moment: in here there is no timer, no loop
//! and no waiting.
//!
//! **A REFUSED CENSUS DOES NOT FAIL A RECORDING.** A hook that exits badly is a
//! hook that disturbs whoever is working: if we were not allowed to look at the
//! machine the progenitor stays unknown and the row is written all the same.
//! Only `sailor session census` is made to exit 3 by a refusal, because it is
//! the only one whose answer *is* the census.

use sessions::census::{Census, LocalMachine};
use sessions::{anchor_from, now, Anchor, Arrival, Payload, Sessions, TerminalEvent};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

/// The forms of `sailor session`, one per line.
///
/// **A LIST AND NOT A BLOCK OF TEXT BECAUSE THE WINDOW READS IT TOO.**
/// `Command::usage` in `lib.rs` wants lines a program can question: one single
/// string would force the window's help page to split it itself, which is to
/// hold a second idea of where a form ends.
pub const USAGE: &[&str] = &[
    "sailor session open      < payload.json   records a terminal and who arrived at it",
    "sailor session event     < payload.json   records a fact about the session",
    "sailor session close     [--tty <name>]   closes a terminal's row",
    "sailor session list      [--json]         what is on record as tracked",
    "sailor session detach    [--tty <name>]   leave this window alone",
    "sailor session attach    [--tty <name>]   follow it again",
    "sailor session census    [--json]         what is on the machine right now",
    "sailor session install   [--tool <id> --settings <file>]  grafts every command line that declares how, and names those that do not",
    "sailor session uninstall [--tool <id> --settings <file>]  takes the graft back out, and names what it could not take out and why",
];

/// The options that hold for several forms, kept out of the list because they
/// are not forms: put there, whoever counts the lines would count them as such.
const COMMON_OPTIONS: &str = "common options: --tty <name> to say the terminal instead of deducing it,\n\
                              \x20               --store <file> to write somewhere other than beside the ledger";

/// The help as whoever types reads it, built from the list rather than copied
/// out beside it.
fn usage_text() -> String {
    format!("usage: {}\n\n{COMMON_OPTIONS}", USAGE.join("\n     "))
}

/// The forms this command knows, in one place: the list `--help` prints and the
/// one dispatch accepts must be the same, or a form that is documented and not
/// accepted gets discovered in the hands of whoever typed it.
const FORMS: &[&str] = &[
    "open",
    "event",
    "close",
    "list",
    "detach",
    "attach",
    "census",
    "install",
    "uninstall",
];

/// The forms that speak of **one** terminal, and so must know its name.
/// `list` and `census` are not here: they speak of all of them.
const NEEDS_A_TERMINAL: &[&str] = &["open", "event", "close", "detach", "attach"];

/// The options that want no value after them.
const WITHOUT_VALUE: &[&str] = &["json"];

/// What to say, and with which code to leave.
#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub message: String,
    pub code: i32,
}

impl Report {
    fn spoken(message: impl Into<String>) -> Report {
        Report {
            message: message.into(),
            code: 0,
        }
    }
}

/// The exit code for a census we were not allowed to take. It is not an error
/// of the command: it is the answer, and it is worth recognising from a script
/// without reading the text.
pub const REFUSED: i32 = 3;

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(report) => {
            if !report.message.is_empty() {
                println!("{}", report.message);
            }
            report.code
        }
        Err(message) => {
            eprintln!("sailor session: {message}");
            1
        }
    }
}

fn dispatch(args: &[String]) -> Result<Report, String> {
    let Some(verb) = args.first().map(String::as_str) else {
        return Err(usage_text());
    };
    if !FORMS.contains(&verb) {
        return Err(format!(
            "«{verb}» is not a form of this command; there are {}\n{}",
            FORMS.join(", "),
            usage_text()
        ));
    }
    let options = options_of(&args[1..])?;

    // Standard input is read **only** where it is needed: reading it for `list`
    // from an interactive terminal would block the command without saying why.
    let raw = if verb == "open" || verb == "event" {
        std::io::read_to_string(std::io::stdin())
            .map_err(|error| format!("cannot read the payload: {error}"))?
    } else {
        String::new()
    };
    let payload = Payload::parse(&raw)?;

    // **ONLY WHOEVER SPEAKS OF A TERMINAL DEMANDS ONE.** `list` and `census`
    // speak of all of them: asking them for a tty makes them fail wherever the
    // output is captured, which is every script and every hook.
    let tty = match options.get("tty") {
        Some(declared) => declared.clone(),
        // **TWO QUESTIONS, NOT ONE.** Our own descriptors first: they run
        // nothing and cross no perimeter. Then the parent chain, because a hook
        // has a pipe on all three and the window is a step or two above it.
        // Asking only the first made this command exit **1 on every hook**,
        // against the principle at the head of this module.
        None if NEEDS_A_TERMINAL.contains(&verb) => sessions::tty::current()
            .or_else(|| sessions::census::tty_of_nearest_ancestor(&LocalMachine))
            .ok_or_else(|| {
                "there is no telling which terminal this process runs on: none of its \
                 three descriptors is a tty, and no process above it on the parent \
                 chain has one either. Say it with --tty <name>"
                    .to_owned()
            })?,
        None => String::new(),
    };

    // **ONLY WHOEVER READS THE LEDGER OPENS IT**, for the same reason as the
    // line above about the terminal — and this line had not received that
    // reason, because it was written as a comment beside the other one. A
    // principle holds where there is a list applying it.
    let store = if NEEDS_THE_STORE.contains(&verb) {
        let path = match options.get("store") {
            Some(declared) => PathBuf::from(declared),
            None => Sessions::default_path().map_err(|error| error.to_string())?,
        };
        Some(Sessions::open(&path).map_err(|error| format!("{}: {error}", path.display()))?)
    } else {
        None
    };

    // Here, and only here, the machine is looked at: an event has arrived.
    let census = Census::of(&LocalMachine);

    act(&Request {
        verb,
        options: &options,
        payload: &payload,
        raw: &raw,
        store: store.as_ref(),
        census: &census,
        tty: &tty,
        at: now(),
    })
}

/// Everything needed to act, already in hand.
///
/// **IT EXISTS SO [`act`] CAN BE TESTED.** `dispatch` reads standard input,
/// opens the real file and questions the real machine: three things a test
/// cannot have without measuring the machine that runs it. With this struct the
/// same decisions are tested on a throwaway file, a payload written by hand and
/// a census built on purpose — the refused one included.
struct Request<'a> {
    verb: &'a str,
    options: &'a BTreeMap<String, String>,
    payload: &'a Payload,
    raw: &'a str,
    /// **`None` WHEN NOBODY NEEDS IT**, and that is no convenience: `census`
    /// must be able to answer "I do not know" even where the ledger will not
    /// open. While it was mandatory, the one form that exists in order not to
    /// lie died before it could speak.
    store: Option<&'a Sessions>,
    census: &'a Census,
    tty: &'a str,
    at: i64,
}

impl<'a> Request<'a> {
    /// This form's ledger.
    ///
    /// **AN ERROR HERE IS A DEFECT OF THIS FILE, NOT A FAULT OF WHOEVER
    /// TYPED**, and the message says so: it means a form reads the ledger
    /// without being listed in [`NEEDS_THE_STORE`].
    fn store(&self) -> Result<&'a Sessions, String> {
        self.store
            .ok_or_else(|| catalogue::say("cli.session.form_not_listed", &[("form", self.verb)]))
    }

    /// Whether whoever speaks is a start-up hook.
    ///
    /// **THE SHAPE OF THE ANSWER FOLLOWS WHOEVER ASKS**, and who asks is in the
    /// payload: a `SessionStart` is answered with the wrapper that gets injected
    /// into the context, a person with a sentence.
    fn is_a_session_start(&self) -> bool {
        self.payload.hook_event_name.as_deref() == Some("SessionStart")
    }
}

/// The forms that really do read the ledger.
///
/// **A LIST OF WHO NEEDS IT, NOT OF THE EXCEPTIONS**, and the difference shows
/// on the form added tomorrow: a list of exceptions lets it through in silence,
/// this one does not. Until 01/09/2026 `dispatch` opened `sessions.db` before
/// knowing which form had been asked for, so `census` — which never touches the
/// ledger — died with the file's error **in place of its own answer**, and its
/// answer is precisely "I do not know".
///
/// Watched by `a_form_that_never_reads_the_store_survives_a_store_that_will_not_open`,
/// which runs on **every** form not listed here.
const NEEDS_THE_STORE: &[&str] = &["open", "event", "close", "list", "detach", "attach"];

/// What Sailor does at each of its moments. What a line calls that moment is
/// said by the descriptor, never here.
///
/// **FOUR, AND NO MORE:** one more hook is one more process at every event of
/// every session. `session_start` carries the welcome and is the only one whose
/// text reaches the agent; the others say alive, asked, about to be compacted.
const WHAT_WE_DO_AT_EACH: &[(&str, &str)] = &[
    ("session_start", "open"),
    ("alive", "event"),
    ("asked", "event"),
    ("compacting", "event"),
];

/// How one of our hooks is told from anyone else's: by the fact that it invokes
/// **this** command. Not by a name written beside it, which can be changed
/// without changing what it does.
const MARK: &str = " session ";

/// The other half of the same recognition: our own name inside the command
/// line. On its own «session» is a word anybody's hook may hold.
const WHAT_WE_ARE: &str = "sailor";

/// The words that all have to be there, as one list.
///
/// **THE LIST IS THE DEFINITION, AND EVERY ASKER READS IT.** [`ours`] answers
/// on a piece of text; the TOML graft is handed the same words and looks for
/// them itself. Written out twice they could drift apart, and a graft and an
/// inverse recognising different lines is a graft that cannot be undone.
const MARKS: &[&str] = &[MARK, WHAT_WE_ARE];

/// Whether a piece of writing is ours, asked of a serialised hook entry and of
/// a command file alike.
///
/// **ONE QUESTION, ASKED BY THE GRAFT AND BY ITS INVERSE.** Two answers would
/// mean one of them leaves behind what the other cannot see, and neither would
/// report it: each is right on its own terms.
fn ours(text: &str) -> bool {
    MARKS.iter().all(|mark| text.contains(mark))
}

/// Walks every command line and hands the resolved addresses to `work`, which
/// answers whether it did anything there.
///
/// **THE ADDRESSES ARE RESOLVED IN ONE PLACE.** A second walk would be a second
/// idea of where Sailor wrote, and a graft and an inverse disagreeing about it
/// is a graft that cannot be undone.
fn each_command_line(
    request: &Request<'_>,
    catalog: &toolbox::descriptor::Catalog,
    machine: &toolbox::Machine,
    said: &mut Vec<String>,
    mut work: impl FnMut(
        &toolbox::descriptor::Descriptor,
        &std::path::Path,
        Option<&std::path::Path>,
        &mut Vec<String>,
    ) -> Result<bool, String>,
) -> Result<bool, String> {
    // **THE LIST AND THE MACHINE ARE HANDED OVER, NOT READ HERE.** A check must
    // be able to ask this about a command line nobody ships, and asking through
    // the shipped list would put a product's name inside the check.

    // `--settings` stays, and stays one file: it serves the tests and whoever
    // moved their own. With it, the descriptor says only what the events are
    // called, no longer where to write them.
    let home = machine.env.get("HOME").cloned().unwrap_or_default();
    let only = request.options.get("tool");
    let declared_file = request.options.get("settings").map(PathBuf::from);
    if declared_file.is_some() && only.is_none() {
        return Err(catalogue::say("cli.session.settings_without_tool", &[]));
    }

    let mut worked_anywhere = false;
    for loaded in &catalog.descriptors {
        let tool = &loaded.descriptor;
        if tool.family != "ai_cli" || tool.disabled {
            continue;
        }
        if only.is_some_and(|wanted| *wanted != tool.id) {
            continue;
        }
        let Some(hooks) = &tool.session_hooks else {
            said.push(catalogue::say(
                "cli.session.declares_no_hooks",
                &[("tool", &tool.id)],
            ));
            continue;
        };

        // It applies to the line `--tool` names, never to «the one the code
        // knows»: a fallback choosing for itself would put a product name back
        // in a condition, through another door.
        let file = match &declared_file {
            Some(path) => path.clone(),
            None => {
                let root = machine.env.get(&hooks.file.root_var).cloned();
                match hooks.file.path(root.as_deref(), &home) {
                    Some(path) => path,
                    None => {
                        said.push(catalogue::say(
                            "cli.session.no_settings_address",
                            &[("tool", &tool.id)],
                        ));
                        continue;
                    }
                }
            }
        };

        // Under `--settings` the words follow the declared file instead of
        // their own address: whoever diverts the graft diverts all of it,
        // and a test writing half into its scratch and half into the real
        // home would leave that half behind.
        let directory = hooks.words.as_ref().and_then(|words| match &declared_file {
            Some(path) => path.parent().map(|beside| {
                beside.join(
                    std::path::Path::new(&words.below_home)
                        .file_name()
                        .unwrap_or_default(),
                )
            }),
            None => words.path(machine.env.get(&words.root_var).map(String::as_str), &home),
        });

        worked_anywhere |= work(tool, &file, directory.as_deref(), said)?;
    }
    Ok(worked_anywhere)
}

/// Grafts every command line that declares how, and **names each one that does
/// not**.
///
/// The settings address used to live here, under a comment arguing it was «the
/// address of what we are grafting» and so not a coupling. It was: two other
/// lines with the same four moments got nothing, and silence reads as success.
fn install_hooks(request: &Request<'_>) -> Result<Report, String> {
    let machine = toolbox::Machine::current();
    let catalog = toolbox::descriptor::Catalog::load(&toolbox::default_sources(&machine));
    grafting(request, &catalog, &machine)
}

/// The same graft, with the list and the machine handed over, so a check can
/// run the whole road over a command line nobody ships.
fn grafting(
    request: &Request<'_>,
    catalog: &toolbox::descriptor::Catalog,
    machine: &toolbox::Machine,
) -> Result<Report, String> {
    let mut said = Vec::new();
    let declared_file = request.options.contains_key("settings");
    let grafted_any = each_command_line(
        request,
        catalog,
        machine,
        &mut said,
        |tool, file, words, said| {
            let missing = moments_without_an_event(tool);
            // **NO CATCH-ALL ARM.** A format that gets a variant and no arm is a
            // compile error, which is the same promise the old arm made in prose:
            // a format declared and not written must never pass in silence.
            match format_of(tool) {
                toolbox::descriptor::FileFormat::Json => said.push(grafted_into(tool, file)?),
                toolbox::descriptor::FileFormat::Toml => {
                    said.push(grafted_into_toml(tool, file, &key_of(tool))?)
                }
            }

            // **WHICH HOME, AND WHY THAT ONE.** A line whose file moves with a
            // variable has two addresses, and a graft that names only the file it
            // wrote leaves whoever reads unable to tell it went to the one their
            // sessions actually read.
            let root_var = root_var_of(tool);
            if !declared_file && !root_var.is_empty() {
                said.push(which_home(tool, &root_var, machine));
            }

            if !missing.is_empty() {
                said.push(format!(
                    "  {}: {:?} have no event on this command line, so they are not \
                 grafted. Not filled with the nearest one: a welcome on the wrong \
                 event arrives at every turn instead of once",
                    tool.id, missing
                ));
            }

            match words {
                Some(directory) => said.push(wrote_the_two_commands(directory)?),
                None => said.push(format!(
                    "  {}: no words a user types, so the welcome promises none",
                    tool.id
                )),
            }
            // Every format the walk reaches is written now, so reaching a line is
            // grafting it. What is left below answers for the lines never reached.
            Ok(true)
        },
    )?;

    if !grafted_any {
        said.push(
            "nothing was grafted anywhere. That is a statement, not a silence: \
             read the lines above for which command line refused and why"
                .to_owned(),
        );
    }
    Ok(Report::spoken(said.join("\n")))
}

/// Takes the graft back out of every command line it went into, and **names
/// each thing it could not take out and why**.
///
/// The owner's requirement is that a command line be left as Sailor found it.
/// A silent failure here is worse than at the graft: whoever ran this believes
/// the file is clean and has no reason to look again.
fn uninstall_hooks(request: &Request<'_>) -> Result<Report, String> {
    let machine = toolbox::Machine::current();
    let catalog = toolbox::descriptor::Catalog::load(&toolbox::default_sources(&machine));
    taking_out(request, &catalog, &machine)
}

/// The same inverse, with the list and the machine handed over, for the same
/// reason [`grafting`] has it: a check must be able to ask about a command line
/// nobody ships.
fn taking_out(
    request: &Request<'_>,
    catalog: &toolbox::descriptor::Catalog,
    machine: &toolbox::Machine,
) -> Result<Report, String> {
    let mut said = Vec::new();
    let looked_anywhere = each_command_line(
        request,
        catalog,
        machine,
        &mut said,
        |tool, file, words, said| {
            let mut looked = false;
            // No catch-all here either, and the arms no longer say the same
            // thing: the graft writes one of these formats and not the other.
            match format_of(tool) {
                toolbox::descriptor::FileFormat::Json => {
                    said.push(uninstalled(file)?);
                    looked = true;
                }
                // **THE GRAFT CAN HAVE BEEN HERE, AND THE INVERSE CANNOT YET
                // REACH IT.** The old arm claimed there was nothing of ours in
                // a format we could not write; the graft writes this one now,
                // so that claim would be a lie. Nothing is taken out, the file
                // is named, and whoever ran this is told to look.
                toolbox::descriptor::FileFormat::Toml => said.push(catalogue::say(
                    "cli.session.uninstall.format_not_taken_back",
                    &[("tool", &tool.id), ("file", &file.display().to_string())],
                )),
            }
            if let Some(directory) = words {
                said.push(took_the_two_commands_out(directory)?);
            }
            Ok(looked)
        },
    )?;

    if !looked_anywhere {
        said.push(catalogue::say("cli.session.uninstall.nothing_read", &[]));
    }
    Ok(Report::spoken(said.join("\n")))
}

/// How this command line writes the file the hooks sit in. Asked of the
/// descriptor rather than assumed: the walk above has already refused every
/// line that declares no hooks at all.
fn format_of(tool: &toolbox::descriptor::Descriptor) -> toolbox::descriptor::FileFormat {
    tool.session_hooks
        .as_ref()
        .map(|hooks| hooks.file.format)
        .unwrap_or_default()
}

/// The key this command line keeps its hooks under, as it declares it.
///
/// **THE KEY COMES FROM THE DESCRIPTOR**, never from here: where a line keeps
/// its hooks is the coupling, and a coupling in the code is one nobody reading
/// the data can check.
fn key_of(tool: &toolbox::descriptor::Descriptor) -> Vec<String> {
    tool.session_hooks
        .as_ref()
        .map(|hooks| hooks.file.key.clone())
        .unwrap_or_default()
}

/// The variable this command line's file moves with, empty when it has none.
fn root_var_of(tool: &toolbox::descriptor::Descriptor) -> String {
    tool.session_hooks
        .as_ref()
        .map(|hooks| hooks.file.root_var.clone())
        .unwrap_or_default()
}

/// The two words the welcome promises, written where they are looked for.
///
/// **IF THEY ARE MISSING, THE WELCOME LIES.** The greeting says «to detach it:
/// /sailor-off», and a promised word that does not exist is worse than one
/// never promised: whoever types it believes they detached.
/// `the_welcome_only_promises_words_that_exist` holds the two together.
fn wrote_the_two_commands(directory: &std::path::Path) -> Result<String, String> {
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    for (name, verb, what) in [
        (
            "sailor-off",
            "detach",
            "Detach this terminal from Sailor: it stops being tracked, and so do \
             the sessions opened here afterwards.",
        ),
        (
            "sailor-on",
            "attach",
            "Attach this terminal to Sailor again, if it had been detached.",
        ),
    ] {
        let body = format!(
            "---\ndescription: {what}\nallowed-tools: Bash(sailor session {verb}:*)\n---\n\n\
             Run `sailor session {verb}` and report in one line what it answered. \
             Do nothing else.\n"
        );
        let path = directory.join(format!("{name}.md"));
        std::fs::write(&path, body).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(format!(
        "/sailor-off and /sailor-on written in {}",
        directory.display()
    ))
}

/// Takes those same two words back out, **and only if they are ours**.
///
/// A file with one of those names that Sailor did not write belongs to whoever
/// did: it stays, and it is named. The directory stays too - it is the command
/// line's own, and it held other words before Sailor arrived.
fn took_the_two_commands_out(directory: &std::path::Path) -> Result<String, String> {
    let mut taken = Vec::new();
    let mut left = Vec::new();
    for name in ["sailor-off", "sailor-on"] {
        let path = directory.join(format!("{name}.md"));
        match std::fs::read_to_string(&path) {
            Ok(body) if ours(&body) => {
                std::fs::remove_file(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                taken.push(format!("/{name}"));
            }
            Ok(_) => left.push(catalogue::say(
                "cli.session.uninstall.word_not_ours",
                &[("word", name), ("file", &path.display().to_string())],
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => left.push(catalogue::say(
                "cli.session.uninstall.word_unreadable",
                &[
                    ("word", name),
                    ("file", &path.display().to_string()),
                    ("error", &error.to_string()),
                ],
            )),
        }
    }
    let mut said = match taken.is_empty() {
        true => catalogue::say(
            "cli.session.uninstall.no_words",
            &[("directory", &directory.display().to_string())],
        ),
        false => catalogue::say(
            "cli.session.uninstall.words_taken",
            &[
                ("words", &taken.join(" and ")),
                ("directory", &directory.display().to_string()),
            ],
        ),
    };
    for one in left {
        said.push_str(&format!("\n  {one}"));
    }
    Ok(said)
}

/// Takes our hooks out of a settings file, **by subtraction**.
///
/// It reads every event the file holds, not the ones the descriptor names
/// today: a command line that renames an event would otherwise leave ours
/// behind for ever, in the one place nobody would think to look. What may be
/// taken is settled by [`ours`], which is also what the graft asks.
fn uninstalled(settings: &std::path::Path) -> Result<String, String> {
    let text = match std::fs::read_to_string(settings) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(catalogue::say(
                "cli.session.uninstall.no_file",
                &[("file", &settings.display().to_string())],
            ))
        }
        Err(error) => return Err(format!("{}: {error}", settings.display())),
    };
    if text.trim().is_empty() {
        return Ok(catalogue::say(
            "cli.session.uninstall.file_empty",
            &[("file", &settings.display().to_string())],
        ));
    }
    // **A FILE WE CANNOT READ IS NOT REWRITTEN**, the same rule as the graft:
    // rewriting it with our part taken out would erase the configuration of
    // whoever uses it, over a typo.
    let mut root: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("{}: not valid JSON ({error})", settings.display()))?;

    let Some(hooks) = root.get_mut("hooks").and_then(|at| at.as_object_mut()) else {
        return Ok(catalogue::say(
            "cli.session.uninstall.no_hooks",
            &[("file", &settings.display().to_string())],
        ));
    };

    let mut taken = Vec::new();
    let mut emptied = Vec::new();
    let mut unreadable = Vec::new();
    for (event, entries) in hooks.iter_mut() {
        let Some(list) = entries.as_array_mut() else {
            unreadable.push(event.clone());
            continue;
        };
        let before = list.len();
        list.retain(|entry| {
            !serde_json::to_string(entry)
                .map(|written| ours(&written))
                .unwrap_or(false)
        });
        if list.len() == before {
            continue;
        }
        taken.push(event.clone());
        if list.is_empty() {
            emptied.push(event.clone());
        }
    }

    let mut said = Vec::new();
    for event in &unreadable {
        said.push(catalogue::say(
            "cli.session.uninstall.not_an_array",
            &[("event", event), ("file", &settings.display().to_string())],
        ));
    }
    if taken.is_empty() {
        said.insert(
            0,
            catalogue::say(
                "cli.session.uninstall.nothing_of_ours",
                &[("file", &settings.display().to_string())],
            ),
        );
        return Ok(said.join("\n"));
    }

    // An event left holding an empty array is a trace of the graft too, and it
    // goes - but only where we are the ones who emptied it.
    for event in &emptied {
        hooks.remove(event);
    }
    let all_gone = hooks.is_empty();
    if let Some(object) = root.as_object_mut() {
        if all_gone {
            object.remove("hooks");
        }
        if object.is_empty() {
            said.push(catalogue::say(
                "cli.session.uninstall.empty_object_left",
                &[("file", &settings.display().to_string())],
            ));
        }
    }

    let written = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    std::fs::write(settings, format!("{written}\n"))
        .map_err(|error| format!("{}: {error}", settings.display()))?;
    said.insert(
        0,
        catalogue::say(
            "cli.session.uninstall.taken_out",
            &[
                ("file", &settings.display().to_string()),
                ("events", &taken.join(", ")),
            ],
        ),
    );
    Ok(said.join("\n"))
}

/// The moments this command line has no event for, which are the ones the
/// report has to name. A pure answer, so it can be asked of a descriptor
/// nobody ships and the check needs no product to exist.
fn moments_without_an_event(tool: &toolbox::descriptor::Descriptor) -> Vec<&'static str> {
    WHAT_WE_DO_AT_EACH
        .iter()
        .map(|(moment, _)| *moment)
        .filter(|moment| tool.event_for(moment).is_none())
        .collect()
}

/// Grafts the moments **this** command line can report, under its own names.
/// The ones it cannot report do not enter and are named elsewhere: there is no
/// fallback here, because a fallback would be invisible to whoever reads.
fn grafted_into(
    tool: &toolbox::descriptor::Descriptor,
    settings: &std::path::Path,
) -> Result<String, String> {
    let named = events_this_line_can_report(tool);
    if named.is_empty() {
        return Ok(nothing_to_graft(tool));
    }
    installed(settings, &named)
}

/// The moments this line can report, paired with the verb we run at each.
fn events_this_line_can_report(tool: &toolbox::descriptor::Descriptor) -> Vec<(&str, &str)> {
    WHAT_WE_DO_AT_EACH
        .iter()
        .filter_map(|(moment, verb)| tool.event_for(moment).map(|event| (event, *verb)))
        .collect()
}

fn nothing_to_graft(tool: &toolbox::descriptor::Descriptor) -> String {
    catalogue::say("cli.session.nothing_to_graft", &[("tool", &tool.id)])
}

/// The same graft into a settings file written in TOML.
fn grafted_into_toml(
    tool: &toolbox::descriptor::Descriptor,
    settings: &std::path::Path,
    under: &[String],
) -> Result<String, String> {
    let named = events_this_line_can_report(tool);
    if named.is_empty() {
        return Ok(nothing_to_graft(tool));
    }
    let binary = std::env::current_exe()
        .map_err(|error| format!("there is no telling where I am: {error}"))?
        .display()
        .to_string();
    let commands: Vec<(&str, String)> = named
        .iter()
        .map(|(event, verb)| (*event, format!("{binary} session {verb}")))
        .collect();

    // **A FILE THAT IS THERE AND WILL NOT BE READ STOPS THE GRAFT.** Treating
    // an unreadable file as an empty one appends to nothing and writes back a
    // file holding our lines alone, which is the configuration of whoever uses
    // it deleted over a permission.
    let existing = match std::fs::read_to_string(settings) {
        Ok(text) => text,
        Err(_) if !settings.exists() => String::new(),
        Err(error) => return Err(format!("{}: {error}", settings.display())),
    };
    let graft = crate::toml_graft::appended(&existing, under, &commands, MARKS)
        .map_err(|reason| format!("{}: {reason}", settings.display()))?;
    if graft.added.is_empty() {
        return Ok(already_grafted(settings));
    }
    if let Some(parent) = settings.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    std::fs::write(settings, &graft.text)
        .map_err(|error| format!("{}: {error}", settings.display()))?;
    Ok(just_grafted(settings, &graft.added.join(", ")))
}

/// Which of a line's two addresses was grafted, and what that leaves open.
///
/// A file whose place moves with a variable has two homes, and the one a
/// session reads is the one set where that session starts. Naming only the file
/// written would let a graft that landed in the other read as done.
fn which_home(
    tool: &toolbox::descriptor::Descriptor,
    root_var: &str,
    machine: &toolbox::Machine,
) -> String {
    match machine.env.get(root_var).filter(|root| !root.is_empty()) {
        Some(root) => catalogue::say(
            "cli.session.home_the_variable_names",
            &[("tool", &tool.id), ("variable", root_var), ("root", root)],
        ),
        None => catalogue::say(
            "cli.session.home_below_yours",
            &[("tool", &tool.id), ("variable", root_var)],
        ),
    }
}

/// The two sentences the report ends on, said the same for every format: two
/// formats wording it differently would read as two different things done.
fn already_grafted(settings: &std::path::Path) -> String {
    let file = settings.display().to_string();
    catalogue::say("cli.session.already_grafted", &[("file", &file)])
}

fn just_grafted(settings: &std::path::Path, events: &str) -> String {
    let file = settings.display().to_string();
    catalogue::say(
        "cli.session.grafted",
        &[("file", &file), ("events", events)],
    )
}

/// Grafts the hooks into a settings file, **by adding**.
///
/// The binary's path is the one running right now (`current_exe`): a graft
/// writing plain `sailor` would work only where that name is already on the
/// `PATH` of whoever opens the terminal, which is not something knowable here.
fn installed(settings: &std::path::Path, events: &[(&str, &str)]) -> Result<String, String> {
    let mut root: serde_json::Value = match std::fs::read_to_string(settings) {
        Ok(text) if text.trim().is_empty() => serde_json::json!({}),
        // **A FILE WE CANNOT READ IS NOT REWRITTEN.** Replacing it with our own
        // part alone would erase the configuration of whoever uses it, over a
        // typo.
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| format!("{}: not valid JSON ({error})", settings.display()))?,
        Err(_) => serde_json::json!({}),
    };

    let binary = std::env::current_exe()
        .map_err(|error| format!("there is no telling where I am: {error}"))?
        .display()
        .to_string();

    let hooks = root
        .as_object_mut()
        .ok_or_else(|| format!("{}: the root is not an object", settings.display()))?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{}: «hooks» is not an object", settings.display()))?;

    let mut added = Vec::new();
    for (event, verb) in events {
        let command = format!("{binary} session {verb}");
        let list = hooks
            .entry(*event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("{}: «{event}» is not an array", settings.display()))?;

        // Already grafted: told by the command, not by the position.
        let already = list.iter().any(|entry| {
            serde_json::to_string(entry)
                .map(|written| ours(&written))
                .unwrap_or(false)
        });
        if already {
            continue;
        }
        list.push(serde_json::json!({
            "hooks": [{"type": "command", "command": command}]
        }));
        added.push(*event);
    }

    if added.is_empty() {
        return Ok(already_grafted(settings));
    }
    if let Some(parent) = settings.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    std::fs::write(settings, format!("{text}\n"))
        .map_err(|error| format!("{}: {error}", settings.display()))?;
    Ok(just_grafted(settings, &added.join(", ")))
}

fn act(request: &Request<'_>) -> Result<Report, String> {
    match request.verb {
        "open" => open_terminal(request),
        "event" => record_event(request),
        "close" => close_terminal(request),
        "detach" => detach_terminal(request),
        "attach" => attach_terminal(request),
        "list" => list_terminals(request),
        "census" => report_census(request),
        "install" => install_hooks(request),
        "uninstall" => uninstall_hooks(request),
        other => Err(format!("«{other}» is not a form of this command")),
    }
}

fn anchor_of(request: &Request<'_>) -> Anchor {
    anchor_from(request.payload, request.tty.to_owned(), request.census)
}

fn arrival_of(request: &Request<'_>) -> Arrival {
    Arrival {
        anchor: anchor_of(request),
        session_id: request.payload.session_id.clone(),
        transcript_path: request.payload.transcript_path.clone(),
        at: request.at,
    }
}

/// The fact's name: the one the payload declares, or the verb's own.
fn event_named(request: &Request<'_>, fallback: &str) -> TerminalEvent {
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
        // What we do not read today is kept as it arrived: a field thrown away
        // is not recovered by looking harder tomorrow.
        payload: (!request.raw.trim().is_empty()).then(|| request.raw.to_owned()),
    }
}

fn open_terminal(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    let arrival = arrival_of(request);

    // **DETACHED MEANS DETACHED**, and it holds for the facts before the text:
    // no row, no event, no greeting. A detachment that recorded anyway would be
    // a silence for show.
    let detached = store
        .terminal(&arrival.anchor.tty)
        .map_err(|error| error.to_string())?
        .is_some_and(|row| row.is_detached());
    if detached {
        return Ok(Report::spoken(String::new()));
    }

    store
        .open_terminal(&arrival)
        .map_err(|error| error.to_string())?;
    store
        .record_event(&event_named(request, "open"))
        .map_err(|error| error.to_string())?;

    if request.is_a_session_start() {
        return Ok(Report::spoken(welcome(&arrival)));
    }
    Ok(Report::spoken(described(&arrival)))
}

/// The welcome, in the wrapper that gets injected into the session's context.
///
/// **A WRAPPER AND NOT A PRINTED LINE** because `SessionStart` is one of the
/// four moments where what the hook writes becomes context the agent reads. A
/// plain line would be read by the person at the screen and not by the agent,
/// and detaching would stay a thing that exists and nobody knows about.
fn welcome(arrival: &Arrival) -> String {
    let text = catalogue::say(
        "cli.session.welcome",
        &[
            ("tty", &arrival.anchor.tty),
            ("worktree", &arrival.anchor.worktree),
        ],
    );
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text,
        }
    })
    .to_string()
}

fn record_event(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    let arrival = arrival_of(request);
    store
        .remember_terminal(&arrival)
        .map_err(|error| error.to_string())?;
    let happened = event_named(request, "event");
    store
        .record_event(&happened)
        .map_err(|error| error.to_string())?;
    Ok(Report::spoken(format!(
        "{} su {}",
        happened.name, happened.tty
    )))
}

fn close_terminal(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    let closed = store
        .close_terminal(request.tty, request.at)
        .map_err(|error| error.to_string())?;
    store
        .record_event(&event_named(request, "close"))
        .map_err(|error| error.to_string())?;
    let key = if closed {
        "cli.session.closed"
    } else {
        "cli.session.had_no_open_row"
    };
    Ok(Report::spoken(catalogue::say(key, &[("tty", request.tty)])))
}

fn detach_terminal(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    store
        .detach(&anchor_of(request), request.at)
        .map_err(|error| error.to_string())?;
    store
        .record_event(&event_named(request, "detach"))
        .map_err(|error| error.to_string())?;
    Ok(Report::spoken(format!(
        "{} is detached: it stays so for whoever arrives here later",
        request.tty
    )))
}

fn attach_terminal(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    let was_detached = store
        .attach(request.tty)
        .map_err(|error| error.to_string())?;
    store
        .record_event(&event_named(request, "attach"))
        .map_err(|error| error.to_string())?;
    Ok(Report::spoken(if was_detached {
        format!("{} is followed again", request.tty)
    } else {
        format!("{} was not detached", request.tty)
    }))
}

fn list_terminals(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    let rows = store.terminals().map_err(|error| error.to_string())?;
    if request.options.contains_key("json") {
        let text = serde_json::to_string_pretty(&rows).map_err(|error| error.to_string())?;
        return Ok(Report::spoken(text));
    }
    if rows.is_empty() {
        return Ok(Report::spoken(catalogue::say(
            "cli.session.none_checked_in",
            &[],
        )));
    }
    let open = catalogue::say("cli.session.row_open", &[]);
    let closed = catalogue::say("cli.session.row_closed", &[]);
    let detached = catalogue::say("cli.session.row_detached", &[]);
    let attached = catalogue::say("cli.session.row_attached", &[]);
    let events = catalogue::say("cli.session.row_events", &[]);
    let mut text = String::new();
    for row in &rows {
        let howmany = store
            .events_on(&row.tty)
            .map(|found| found.len())
            .unwrap_or_default();
        let _ = writeln!(
            text,
            "{:<10} {:<14} {:<8} {:<11} {events}={:<4} {} {}",
            row.tty,
            row.ancestor.as_deref().unwrap_or("?"),
            if row.is_open() { &open } else { &closed },
            if row.is_detached() {
                &detached
            } else {
                &attached
            },
            howmany,
            row.session_id.as_deref().unwrap_or("-"),
            row.worktree,
        );
    }
    Ok(Report::spoken(text.trim_end().to_owned()))
}

fn report_census(request: &Request<'_>) -> Result<Report, String> {
    if request.options.contains_key("json") {
        let text =
            serde_json::to_string_pretty(request.census).map_err(|error| error.to_string())?;
        return Ok(Report {
            code: refusal_code(request.census),
            message: text,
        });
    }
    let message = match request.census {
        Census::Refused(refusal) => catalogue::say(
            "cli.session.census_refused",
            &[("refusal", &refusal.to_string())],
        ),
        Census::NoTerminal => "no process has a terminal, and asking was possible".to_owned(),
        Census::Terminals(terminals) => {
            let mut text = String::new();
            for terminal in terminals {
                let _ = writeln!(
                    text,
                    "{} ({}), {} processi",
                    terminal.tty,
                    terminal
                        .ancestor
                        .as_deref()
                        .unwrap_or("an unknown ancestor"),
                    terminal.inhabitants.len()
                );
                for inhabitant in &terminal.inhabitants {
                    let _ = writeln!(
                        text,
                        "  {:<8} {:<12} {:<40} {}",
                        inhabitant.pid,
                        inhabitant.uptime,
                        inhabitant.command,
                        inhabitant.working_directory.as_deref().unwrap_or("?"),
                    );
                }
            }
            text.trim_end().to_owned()
        }
    };
    Ok(Report {
        code: refusal_code(request.census),
        message,
    })
}

fn refusal_code(census: &Census) -> i32 {
    match census {
        Census::Refused(_) => REFUSED,
        Census::NoTerminal | Census::Terminals(_) => 0,
    }
}

fn described(arrival: &Arrival) -> String {
    format!(
        "{} in {} ({}), session {}",
        arrival.anchor.tty,
        arrival.anchor.worktree,
        arrival
            .anchor
            .ancestor
            .as_deref()
            .unwrap_or("an unknown ancestor"),
        arrival
            .session_id
            .as_deref()
            .unwrap_or("with no identifier"),
    )
}

/// The options written on the line. One that wants a value and has none is an
/// error, not an empty: it would take the next option as its value.
fn options_of(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut found = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(word) = rest.next() {
        let Some(name) = word.strip_prefix("--") else {
            return Err(format!(
                "«{word}» is not something I know\n{}",
                usage_text()
            ));
        };
        if WITHOUT_VALUE.contains(&name) {
            found.insert(name.to_owned(), "true".to_owned());
            continue;
        }
        let value = rest
            .next()
            .ok_or_else(|| format!("«--{name}» wants a value after it"))?;
        if value.starts_with("--") {
            return Err(format!(
                "«--{name}» took «{value}» for a value: the real value is missing"
            ));
        }
        found.insert(name.to_owned(), value.clone());
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sessions::census::{Inhabitant, Refusal, Terminal};
    use sessions::SESSIONS_FILE;

    struct Scratch {
        directory: PathBuf,
    }

    impl Scratch {
        fn new(label: &str) -> Scratch {
            let directory = std::env::temp_dir().join(format!(
                "sailor-session-cmd-{label}-{}-{}",
                std::process::id(),
                now()
            ));
            std::fs::create_dir_all(&directory).expect("creare la cartella");
            Scratch { directory }
        }

        fn store(&self) -> Sessions {
            Sessions::open(self.directory.join(SESSIONS_FILE)).expect("aprire")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn no_options() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn one_terminal() -> Census {
        Census::Terminals(vec![Terminal {
            tty: "ttys004".to_owned(),
            ancestor: Some("Whatever".to_owned()),
            inhabitants: vec![Inhabitant {
                pid: 10,
                parent_pid: 1,
                tty: "ttys004".to_owned(),
                uptime: "01:00".to_owned(),
                command: "/bin/zsh".to_owned(),
                working_directory: Some("/here".to_owned()),
            }],
        }])
    }

    fn refused() -> Census {
        Census::Refused(Refusal {
            tool: "ps".to_owned(),
            reason: "operation not permitted: ps".to_owned(),
        })
    }

    fn ask(
        verb: &str,
        raw: &str,
        store: &Sessions,
        census: &Census,
        options: &BTreeMap<String, String>,
    ) -> Result<Report, String> {
        let payload = Payload::parse(raw).expect("il payload della prova è JSON");
        act(&Request {
            verb,
            options,
            payload: &payload,
            raw,
            store: Some(store),
            census,
            tty: "ttys004",
            at: 1_000,
        })
    }

    /// **EVERY FORM NOT DECLARING IT WANTS THE LEDGER**, not `census` alone: one
    /// added tomorrow is tested here with nobody remembering to. Each also gets
    /// `--settings` inside the scratch — without that line `install` would write
    /// the **real** settings file of whoever runs the battery. *Mutant run*:
    /// opening the ledger unconditionally in `dispatch` turns this red, naming
    /// the form and the file's error.
    #[test]
    fn a_form_that_never_reads_the_store_survives_a_store_that_will_not_open() {
        let scratch = Scratch::new("senza-deposito");
        // A **directory** where a file is expected: SQLite will not open it, and
        // the failure is of the same kind as the real ones — permissions, a full
        // disk, a file from a newer version — without fabricating any.
        let impossible = scratch.directory.join("non-e-un-file");
        std::fs::create_dir_all(&impossible).expect("la cartella di prova");
        let settings = scratch.directory.join("settings-di-prova.json");

        for form in FORMS.iter().filter(|form| !NEEDS_THE_STORE.contains(form)) {
            let words: Vec<String> = vec![
                (*form).to_owned(),
                "--store".to_owned(),
                impossible.display().to_string(),
                "--settings".to_owned(),
                settings.display().to_string(),
                "--tool".to_owned(),
                "claude-code".to_owned(),
            ];
            let report = dispatch(&words).unwrap_or_else(|error| {
                panic!(
                    "«session {form}» non legge il deposito, eppure è morto perché non \
                     si apriva: {error}"
                )
            });
            assert!(
                !report.message.is_empty(),
                "«session {form}» ha risposto senza dire niente"
            );
        }
    }

    /// I ganci di un altro prodotto, già installati, come stanno davvero in
    /// `~/.claude/settings.json` su questa macchina.
    fn settings_of_someone_else() -> &'static str {
        r#"{
          "model": "opusplan",
          "hooks": {
            "Stop": [
              {"hooks": [{"type": "command", "command": "/Users/qualcuno/.altro/gancio.sh"}]}
            ],
            "PreToolUse": [
              {"hooks": [{"type": "command", "command": "/Users/qualcuno/.altro/gancio.sh"}]}
            ]
          }
        }"#
    }

    /// **A MISSING KEY IS NOT FILLED WITH ITS NEIGHBOUR, AND IS SAID ALOUD.**
    ///
    /// A line that can report one moment gets that one, and the other three are
    /// declared ungrafted. The temptation has a name: an event that looks like
    /// a session start and fires every turn would put the welcome every turn.
    #[test]
    fn a_moment_with_no_event_is_left_out_and_said_out_loud() {
        let scratch = Scratch::new("momento-assente");
        let settings = scratch.directory.join("settings.json");
        // Built as the loader would read it, and sent down the real road: a
        // test calling `installed` with the list already filtered stays green
        // when the filter goes, because the filter is the thing that would go.
        let tool: toolbox::descriptor::Descriptor = serde_json::from_str(
            r#"{
              "id": "riga-che-sa-dire-una-cosa-sola",
              "family": "ai_cli",
              "session_hooks": {
                "file": {"below_home": ".da-qualche-parte/settings.json"},
                "events": {"alive": "SoloQuesto"}
              }
            }"#,
        )
        .expect("il descrittore si legge");

        grafted_into(&tool, &settings).expect("l'innesto riesce");

        let written = std::fs::read_to_string(&settings).expect("rileggere");
        let after: serde_json::Value = serde_json::from_str(&written).expect("JSON valido");
        let hooks = after["hooks"].as_object().expect("i ganci sono un oggetto");

        assert_eq!(
            hooks.len(),
            1,
            "innestato più di ciò che questa riga sa dire: {written}"
        );
        assert!(hooks.contains_key("SoloQuesto"), "{written}");
        for nearby in ["SessionStart", "Stop", "UserPromptSubmit", "PreCompact"] {
            assert!(
                !hooks.contains_key(nearby),
                "«{nearby}» è entrato senza che nessun descrittore lo nominasse: \
                 un evento vicino messo al posto di uno assente non si vede, e \
                 il benvenuto finirebbe dove nessuno l'ha chiesto"
            );
        }
    }

    /// And the absence must be **named**, not merely avoided: silence reads as
    /// success, which is the whole fault this block exists to remove. Asked of
    /// an invented descriptor, so no shipped product's name enters the check.
    #[test]
    fn the_moments_with_no_event_are_the_ones_reported() {
        let says_one: toolbox::descriptor::Descriptor = serde_json::from_str(
            r#"{
              "id": "una-riga-qualsiasi",
              "family": "ai_cli",
              "session_hooks": {
                "file": {"below_home": "altrove/settings.json"},
                "events": {"alive": "SoloQuesto"}
              }
            }"#,
        )
        .expect("il descrittore si legge");

        let missing = moments_without_an_event(&says_one);

        assert_eq!(
            missing,
            vec!["session_start", "asked", "compacting"],
            "i momenti senza evento sono quelli che il rapporto deve nominare, \
             e sono esattamente quelli che il descrittore non dichiara"
        );

        let says_all: toolbox::descriptor::Descriptor = serde_json::from_str(
            r#"{
              "id": "una-riga-completa",
              "family": "ai_cli",
              "session_hooks": {
                "file": {"below_home": "altrove/settings.json"},
                "events": {"session_start": "A", "alive": "B", "asked": "C", "compacting": "D"}
              }
            }"#,
        )
        .expect("il descrittore si legge");

        assert!(
            moments_without_an_event(&says_all).is_empty(),
            "una riga che li dichiara tutti e quattro non deve avere niente da \
             dichiarare mancante, o l'avviso diventa rumore e smette di essere letto"
        );
    }

    /// The names one command line gives the four moments, as a descriptor
    /// would give them. Here and not in the code: that is the whole point.
    fn as_one_line_names_them() -> Vec<(&'static str, &'static str)> {
        vec![
            ("SessionStart", "open"),
            ("Stop", "event"),
            ("UserPromptSubmit", "event"),
            ("PreCompact", "event"),
        ]
    }

    /// **IT ADDS, IT DOES NOT REPLACE.** Five hands write into that settings
    /// file, and a graft that rewrites the hooks array silently switches off
    /// whoever was there first — which is how a tracking tool becomes the fault
    /// it was meant to prevent.
    #[test]
    fn installing_leaves_the_hooks_that_were_already_there() {
        let scratch = Scratch::new("innesto");
        let settings = scratch.directory.join("settings.json");
        std::fs::write(&settings, settings_of_someone_else()).expect("scrivere");

        installed(&settings, &as_one_line_names_them()).expect("l'innesto riesce");

        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).expect("rileggere"))
                .expect("resta JSON valido");

        assert_eq!(after["model"], "opusplan", "l'innesto non tocca il resto");
        let stops = after["hooks"]["Stop"]
            .as_array()
            .expect("Stop è un vettore");
        assert_eq!(
            stops.len(),
            2,
            "il gancio di prima è ancora lì, e il nostro è in più"
        );
        assert!(
            serde_json::to_string(&after).unwrap().contains("gancio.sh"),
            "il gancio di chi c'era prima non è stato cancellato"
        );
        assert!(
            after["hooks"]["SessionStart"].is_array(),
            "l'evento che porta il benvenuto dev'esserci"
        );
    }

    /// A repeated graft is one graft: whoever runs it twice for safety must not
    /// end up with two hooks recording the same fact.
    #[test]
    fn installing_twice_does_not_double_anything() {
        let scratch = Scratch::new("innesto-doppio");
        let settings = scratch.directory.join("settings.json");
        std::fs::write(&settings, settings_of_someone_else()).expect("scrivere");

        installed(&settings, &as_one_line_names_them()).expect("primo innesto");
        let once = std::fs::read_to_string(&settings).expect("rileggere");
        installed(&settings, &as_one_line_names_them()).expect("secondo innesto");
        let twice = std::fs::read_to_string(&settings).expect("rileggere");

        assert_eq!(once, twice, "il secondo innesto non deve cambiare niente");
    }

    /// **A FILE WE CANNOT READ IS NOT REWRITTEN.** Overwriting it with our own
    /// part alone would erase the configuration of whoever uses it, over a typo.
    #[test]
    fn a_settings_file_that_does_not_parse_is_left_alone() {
        let scratch = Scratch::new("innesto-rotto");
        let settings = scratch.directory.join("settings.json");
        std::fs::write(&settings, "{ questo non è JSON").expect("scrivere");

        let refused = installed(&settings, &as_one_line_names_them())
            .expect_err("un file illeggibile ferma l'innesto");
        assert!(refused.contains("settings.json"), "{refused}");
        assert_eq!(
            std::fs::read_to_string(&settings).expect("rileggere"),
            "{ questo non è JSON",
            "il file resta esattamente com'era"
        );
    }

    /// A command line that keeps its hooks in TOML, as a descriptor declares
    /// it. Invented on purpose: what is under test is the format and the key,
    /// and no shipped name enters the check.
    fn a_line_that_writes_toml() -> toolbox::descriptor::Descriptor {
        serde_json::from_str(
            r#"{
              "id": "a-line-that-keeps-its-hooks-in-toml",
              "family": "ai_cli",
              "session_hooks": {
                "file": {
                  "root_var": "SOME_HOME",
                  "below_root": "config.toml",
                  "below_home": ".somewhere/config.toml",
                  "format": "toml",
                  "key": ["hooks"]
                },
                "events": {"session_start": "SessionStart", "alive": "Stop"}
              }
            }"#,
        )
        .expect("the descriptor reads")
    }

    /// A file written by hand, with a comment and a section **after** the place
    /// a careless graft would cut in.
    fn a_toml_written_by_hand() -> &'static str {
        "notify = [\"somebody\"]\n\
         \n\
         [hooks.state]\n\
         trusted = \"abc\"\n\
         \n\
         # and the servers, which keep their comment and their keys\n\
         [servers.one]\n\
         command = \"/somewhere\"\n"
    }

    fn the_key_it_declares() -> Vec<String> {
        vec!["hooks".to_owned()]
    }

    /// **A DECLARED FORMAT IS A WRITTEN FORMAT.** The report used to say the
    /// descriptor declared where and how and that nothing had been written, and
    /// what was missing was only the writing.
    #[test]
    fn a_line_that_declares_toml_is_grafted_and_the_rest_is_left_alone() {
        let scratch = Scratch::new("toml-graft");
        let settings = scratch.directory.join("config.toml");
        std::fs::write(&settings, a_toml_written_by_hand()).expect("writing the file");

        let said = grafted_into_toml(
            &a_line_that_writes_toml(),
            &settings,
            &the_key_it_declares(),
        )
        .expect("the graft works");

        let after = std::fs::read_to_string(&settings).expect("reading it back");
        assert!(
            after.starts_with(a_toml_written_by_hand()),
            "the graft rewrote what was above it: {after}"
        );
        assert!(said.contains("SessionStart"), "{said}");
        assert!(after.contains("[[hooks.SessionStart]]"), "{after}");
        assert!(
            !after.contains("UserPromptSubmit"),
            "a moment this line never named entered anyway: {after}"
        );
    }

    #[test]
    fn grafting_toml_twice_does_not_double_anything() {
        let scratch = Scratch::new("toml-graft-twice");
        let settings = scratch.directory.join("config.toml");
        std::fs::write(&settings, a_toml_written_by_hand()).expect("writing the file");

        grafted_into_toml(
            &a_line_that_writes_toml(),
            &settings,
            &the_key_it_declares(),
        )
        .expect("the first graft");
        let once = std::fs::read_to_string(&settings).expect("reading it back");
        let said = grafted_into_toml(
            &a_line_that_writes_toml(),
            &settings,
            &the_key_it_declares(),
        )
        .expect("the second graft");
        let twice = std::fs::read_to_string(&settings).expect("reading it back");

        assert_eq!(once, twice, "the second graft must change nothing");
        assert!(
            said == already_grafted(&settings),
            "the second graft did not say it had already been done: {said}"
        );
    }

    /// A file whose shape leaves no room at the bottom: the graft stops, says
    /// why, and the file stays exactly as it was.
    #[test]
    fn a_toml_that_cannot_take_the_graft_is_left_alone_and_said_out_loud() {
        let scratch = Scratch::new("toml-graft-impossible");
        let settings = scratch.directory.join("config.toml");
        let awkward = "[hooks.SessionStart]\nwhatever = 1\n";
        std::fs::write(&settings, awkward).expect("writing the file");

        let refused = grafted_into_toml(
            &a_line_that_writes_toml(),
            &settings,
            &the_key_it_declares(),
        )
        .expect_err("a shape that cannot take it stops the graft");

        assert!(refused.contains("config.toml"), "{refused}");
        assert!(refused.contains("nothing was written"), "{refused}");
        assert_eq!(
            std::fs::read_to_string(&settings).expect("reading it back"),
            awkward,
            "the file stays exactly as it was"
        );
    }

    /// **THE WHOLE ROAD, NOT THE LAST STEP.** The format used to be read here
    /// and answered with «Sailor does not write that yet»; a check calling the
    /// writer straight would stay green with that answer back in place.
    #[test]
    fn the_report_of_a_line_that_declares_toml_says_it_was_grafted() {
        let scratch = Scratch::new("toml-graft-whole-road");
        let settings = scratch.directory.join("config.toml");
        std::fs::write(&settings, a_toml_written_by_hand()).expect("writing the file");
        let tool = a_line_that_writes_toml();
        let catalog = toolbox::descriptor::Catalog {
            descriptors: vec![toolbox::descriptor::Loaded {
                descriptor: tool.clone(),
                source: "the check".to_owned(),
            }],
            ..Default::default()
        };
        let options = BTreeMap::from([
            ("settings".to_owned(), settings.display().to_string()),
            ("tool".to_owned(), tool.id.clone()),
        ]);
        let payload = Payload::parse("{}").expect("an empty payload");
        let request = Request {
            verb: "install",
            options: &options,
            payload: &payload,
            raw: "",
            store: None,
            census: &one_terminal(),
            tty: "",
            at: 1_000,
        };

        let report = grafting(&request, &catalog, &a_machine_saying(None)).expect("the graft");

        assert!(
            report
                .message
                .contains(&just_grafted(&settings, "SessionStart, Stop")),
            "a declared format that is written must not be reported as refused: {}",
            report.message
        );
        assert!(
            !report.message.contains(&nothing_to_graft(&tool)),
            "the report says there was nothing to graft into a file it wrote: {}",
            report.message
        );
        assert!(
            std::fs::read_to_string(&settings)
                .expect("reading it back")
                .contains("[[hooks.SessionStart]]"),
            "the report said grafted and the file has nothing in it"
        );
    }

    fn a_machine_saying(root: Option<&str>) -> toolbox::Machine {
        let mut env = BTreeMap::new();
        if let Some(root) = root {
            env.insert("SOME_HOME".to_owned(), root.to_owned());
        }
        toolbox::Machine {
            path_dirs: Vec::new(),
            home: PathBuf::from("/home/somebody"),
            env,
            version_probes: false,
        }
    }

    /// **TWO ADDRESSES, AND THE REPORT SAYS WHICH ONE.** A file whose place
    /// moves with a variable is grafted where that variable says right here,
    /// and a session started elsewhere reads the other one. Naming only the
    /// file written would let a graft that landed in the wrong home read as
    /// done — measured on a real machine, twice in two days.
    #[test]
    fn the_report_says_which_of_the_two_homes_was_grafted() {
        let tool = a_line_that_writes_toml();

        let set = which_home(&tool, "SOME_HOME", &a_machine_saying(Some("/elsewhere")));
        let unset = which_home(&tool, "SOME_HOME", &a_machine_saying(None));

        assert!(set.contains("/elsewhere"), "{set}");
        assert_ne!(
            set, unset,
            "the two homes must not read the same, or the report answers nothing"
        );
        for said in [&set, &unset] {
            assert!(
                said.contains("SOME_HOME"),
                "the report must name the variable it looked at: {said}"
            );
        }
    }

    /// **THE INVERSE DOES NOT REACH THIS FORMAT, AND SAYS SO.** The graft
    /// writes TOML now, so the old answer — «a format we cannot write is one we
    /// cannot have written into» — would send whoever ran `uninstall` away
    /// believing the file was clean.
    #[test]
    fn taking_the_graft_out_of_a_toml_says_it_could_not() {
        let scratch = Scratch::new("toml-uninstall");
        let settings = scratch.directory.join("config.toml");
        std::fs::write(&settings, a_toml_written_by_hand()).expect("writing the file");
        let tool = a_line_that_writes_toml();
        let catalog = toolbox::descriptor::Catalog {
            descriptors: vec![toolbox::descriptor::Loaded {
                descriptor: tool.clone(),
                source: "the check".to_owned(),
            }],
            ..Default::default()
        };
        let options = BTreeMap::from([
            ("settings".to_owned(), settings.display().to_string()),
            ("tool".to_owned(), tool.id.clone()),
        ]);
        let payload = Payload::parse("{}").expect("an empty payload");
        let request = Request {
            verb: "uninstall",
            options: &options,
            payload: &payload,
            raw: "",
            store: None,
            census: &one_terminal(),
            tty: "",
            at: 1_000,
        };

        let report =
            taking_out(&request, &catalog, &a_machine_saying(None)).expect("the inverse answers");

        let owned_up = catalogue::say(
            "cli.session.uninstall.format_not_taken_back",
            &[
                ("tool", &tool.id),
                ("file", &settings.display().to_string()),
            ],
        );
        assert!(
            report.message.contains(&owned_up),
            "the inverse let a format it cannot undo pass as done: {}",
            report.message
        );
        assert_eq!(
            std::fs::read_to_string(&settings).expect("reading it back"),
            a_toml_written_by_hand(),
            "the inverse touched a file it says it did not"
        );
    }

    /// The iron rule holds for what the graft **writes** too: the command that
    /// ends up in the hooks names no product.
    #[test]
    fn what_the_install_writes_names_no_product() {
        let scratch = Scratch::new("innesto-neutro");
        let settings = scratch.directory.join("settings.json");
        installed(&settings, &as_one_line_names_them())
            .expect("l'innesto riesce anche su un file che non c'era");

        let written = std::fs::read_to_string(&settings).expect("rileggere");
        for product in ["orca", "warp", "vscode", "iterm", "tmux"] {
            assert!(
                !written.to_lowercase().contains(product),
                "l'innesto ha scritto «{product}» in settings.json: {written}"
            );
        }
    }

    /// **THE WELCOME PROMISES ONLY WORDS THAT EXIST.** It says «to detach it:
    /// /sailor-off»; if the graft did not write that command, whoever typed it
    /// would believe they had detached. The two live in different files and no
    /// compiler ties them: this test does.
    #[test]
    fn the_welcome_only_promises_words_that_exist() {
        let scratch = Scratch::new("parola-mantenuta");
        let settings = scratch.directory.join("settings.json");
        let request = Request {
            verb: "install",
            // `--settings` without `--tool` is refused on purpose: it says
            // where to write and not for whom, and which line to graft is
            // named by the descriptor, never by the code.
            options: &BTreeMap::from([
                ("settings".to_owned(), settings.display().to_string()),
                ("tool".to_owned(), "claude-code".to_owned()),
            ]),
            payload: &Payload::parse("{}").expect("payload vuoto"),
            raw: "",
            store: None,
            census: &one_terminal(),
            tty: "",
            at: 1_000,
        };
        act(&request).expect("l'innesto riesce");

        let saluto = welcome(&Arrival {
            anchor: sessions::Anchor {
                tty: "ttys004".to_owned(),
                worktree: "/qui".to_owned(),
                ancestor: None,
            },
            session_id: None,
            transcript_path: None,
            at: 1_000,
        });

        for word in ["/sailor-off", "/sailor-on"] {
            if !saluto.contains(word) {
                continue;
            }
            let file = scratch
                .directory
                .join("commands")
                .join(format!("{}.md", word.trim_start_matches('/')));
            assert!(
                file.exists(),
                "il benvenuto promette «{word}» e l'innesto non lo scrive: {}",
                file.display()
            );
        }
        assert!(
            saluto.contains("/sailor-off"),
            "il saluto deve promettere lo stacco, o lo stacco non lo sa nessuno"
        );
    }

    /// The options that send a graft, and its inverse, into a scratch instead
    /// of the real settings file of whoever runs the battery.
    fn into_the_scratch(settings: &std::path::Path) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("settings".to_owned(), settings.display().to_string()),
            ("tool".to_owned(), "claude-code".to_owned()),
        ])
    }

    /// One form, down the road dispatch sends it: the catalogue is the real
    /// one, and only the addresses are diverted.
    fn ran(verb: &str, options: &BTreeMap<String, String>) -> Result<Report, String> {
        act(&Request {
            verb,
            options,
            payload: &Payload::parse("{}").expect("the empty payload"),
            raw: "",
            store: None,
            census: &one_terminal(),
            tty: "",
            at: 1_000,
        })
    }

    /// **THE INVERSE LEAVES THE FILE AS THE GRAFT FOUND IT**, and the whole
    /// file is what gets compared. Asserting «something was removed» would stay
    /// green over an emptied array left behind and over somebody else's hook
    /// dropped along the way, which are the two ways this can go wrong.
    #[test]
    fn uninstalling_leaves_the_settings_file_as_the_graft_found_it() {
        let scratch = Scratch::new("stacco");
        let settings = scratch.directory.join("settings.json");
        std::fs::write(&settings, settings_of_someone_else()).expect("write");
        let before: serde_json::Value =
            serde_json::from_str(settings_of_someone_else()).expect("the fixture is JSON");

        installed(&settings, &as_one_line_names_them()).expect("the graft runs");
        let grafted = std::fs::read_to_string(&settings).expect("read back");
        assert!(ours(&grafted), "the graft wrote nothing of ours: {grafted}");

        uninstalled(&settings).expect("the inverse runs");

        let text = std::fs::read_to_string(&settings).expect("read back");
        let after: serde_json::Value = serde_json::from_str(&text).expect("it is still valid JSON");
        assert_eq!(
            before, after,
            "the file is not what the graft found. What is left over, or what \
             went missing, is the difference between these two: {text}"
        );
    }

    /// Taking out twice is taking out once: whoever runs it a second time to be
    /// sure must not be told a second removal happened, and must not fail.
    #[test]
    fn uninstalling_twice_takes_nothing_the_second_time() {
        let scratch = Scratch::new("stacco-doppio");
        let settings = scratch.directory.join("settings.json");
        std::fs::write(&settings, settings_of_someone_else()).expect("write");
        installed(&settings, &as_one_line_names_them()).expect("the graft runs");

        uninstalled(&settings).expect("the first removal");
        let once = std::fs::read_to_string(&settings).expect("read back");
        let said = uninstalled(&settings).expect("the second removal");
        let twice = std::fs::read_to_string(&settings).expect("read back");

        assert_eq!(once, twice, "the second removal changed the file");
        // Asked of the catalogue rather than written out here: the sentence is
        // translated, and a literal would make this test fail in one language.
        let found_nothing = catalogue::say(
            "cli.session.uninstall.nothing_of_ours",
            &[("file", &settings.display().to_string())],
        );
        assert_eq!(
            said, found_nothing,
            "the second removal claims to have taken something out: {said}"
        );
    }

    /// **A FILE WE CANNOT READ IS NOT REWRITTEN**, the same rule the graft
    /// holds: the removal has to refuse, name the file, and touch nothing.
    #[test]
    fn a_settings_file_that_does_not_parse_survives_the_uninstall() {
        let scratch = Scratch::new("stacco-rotto");
        let settings = scratch.directory.join("settings.json");
        std::fs::write(&settings, "{ this is not JSON").expect("write");

        let refused = uninstalled(&settings).expect_err("an unreadable file stops the removal");
        assert!(refused.contains("settings.json"), "{refused}");
        assert_eq!(
            std::fs::read_to_string(&settings).expect("read back"),
            "{ this is not JSON",
            "the file is exactly as it was"
        );
    }

    /// The two words go, and nothing else in that directory does. A file with
    /// one of their names that Sailor did not write belongs to whoever did:
    /// deleting it by its name alone is how a removal becomes a loss.
    #[test]
    fn uninstalling_takes_out_its_own_words_and_leaves_the_others() {
        let scratch = Scratch::new("parole-tolte");
        let settings = scratch.directory.join("settings.json");
        let options = into_the_scratch(&settings);
        ran("install", &options).expect("the graft runs");

        let commands = scratch.directory.join("commands");
        let someone_elses = commands.join("sailor-off.md");
        std::fs::write(&someone_elses, "written by somebody else\n").expect("write");
        let unrelated = commands.join("a-word-of-my-own.md");
        std::fs::write(&unrelated, "nothing to do with the tracking\n").expect("write");

        let report = ran("uninstall", &options).expect("the inverse runs");

        assert!(
            !commands.join("sailor-on.md").exists(),
            "a word the graft wrote is still there: {}",
            report.message
        );
        assert_eq!(
            std::fs::read_to_string(&someone_elses).unwrap_or_default(),
            "written by somebody else\n",
            "a file Sailor did not write was taken by its name alone: {}",
            someone_elses.display()
        );
        assert!(unrelated.exists(), "an unrelated word was taken away");
        assert!(
            report.message.contains("sailor-off"),
            "the file it could not take out is not named: {}",
            report.message
        );
    }

    /// **TAKING THE GRAFT OUT IS A GESTURE ON THE CONFIGURATION, NOT ON THE
    /// DATA.** The ledger is not read, not written and not created: a removal
    /// that opened it would be a removal that could lose what was recorded.
    #[test]
    fn uninstalling_does_not_so_much_as_open_the_ledger() {
        let scratch = Scratch::new("stacco-senza-registro");
        let ledger = scratch.directory.join("a-ledger-nobody-asked-for.db");
        let settings = scratch.directory.join("settings.json");
        let words: Vec<String> = vec![
            "uninstall".to_owned(),
            "--store".to_owned(),
            ledger.display().to_string(),
            "--settings".to_owned(),
            settings.display().to_string(),
            "--tool".to_owned(),
            "claude-code".to_owned(),
        ];

        dispatch(&words).expect("the inverse runs");

        assert!(
            !ledger.exists(),
            "the removal created {}: it opened the ledger to take hooks out of a \
             settings file",
            ledger.display()
        );
    }

    /// A real `SessionStart` payload, in the shape it arrives in.
    fn a_session_start(session: &str) -> String {
        format!(
            r#"{{"session_id":"{session}","hook_event_name":"SessionStart",
                 "startup_reason":"startup","cwd":"/qui/dentro"}}"#
        )
    }

    /// **THE WELCOME ENTERS THE AGENT'S CONTEXT, NOT THE TERMINAL.**
    /// `SessionStart` is one of the four moments where what the hook writes is
    /// added to the context: it is the pillar the full promise stands on. If it
    /// stops holding, the greeting becomes a line the person reads and the agent
    /// does not — it still works, and it is another thing.
    #[test]
    fn the_welcome_enters_the_context_of_the_agent() {
        let scratch = Scratch::new("benvenuto");
        let store = scratch.store();
        let report = ask(
            "open",
            &a_session_start("s-1"),
            &store,
            &one_terminal(),
            &no_options(),
        )
        .expect("l'apertura riesce");

        let spoken: serde_json::Value =
            serde_json::from_str(&report.message).expect("un gancio SessionStart risponde in JSON");
        let context = spoken["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("il saluto viaggia in hookSpecificOutput.additionalContext");

        assert!(context.contains("Sailor"), "{context}");
        assert!(
            context.contains("/sailor-off"),
            "il saluto deve dire come staccarsi, o lo stacco esiste e non lo sa nessuno: {context}"
        );
        assert!(
            context.contains("ttys004"),
            "il saluto nomina il terminale di cui parla: {context}"
        );
        assert_eq!(
            spoken["hookSpecificOutput"]["hookEventName"], "SessionStart",
            "l'involucro dichiara di quale evento è la risposta"
        );
    }

    /// **STACCATO VUOL DIRE STACCATO.** Nessun saluto, e nessuna riga scritta:
    /// if opening recorded anyway, «leave this window alone» would hold for the
    /// text and not for the facts.
    #[test]
    fn a_detached_terminal_is_greeted_by_silence() {
        let scratch = Scratch::new("staccato");
        let store = scratch.store();
        let census = one_terminal();

        ask("detach", "{}", &store, &census, &no_options()).expect("lo stacco riesce");
        let report = ask(
            "open",
            &a_session_start("s-2"),
            &store,
            &census,
            &no_options(),
        )
        .expect("l'apertura su un terminale staccato non è un errore");

        assert_eq!(
            report.message, "",
            "un terminale staccato non riceve saluti"
        );
        assert!(
            store
                .events_on("ttys004")
                .expect("leggere gli eventi")
                .iter()
                .all(|event| event.name != "SessionStart"),
            "un terminale staccato non lascia eventi: staccato vale per i fatti, non solo per il testo"
        );
    }

    /// **THE CENSUS NEEDS NO LEDGER**, and while it did it could die before
    /// saying "I do not know" — the very thing it exists for. Seen by running
    /// the binary inside the perimeter: it opened the ledger for every form, and
    /// with the default one unwritable it exited 1 with an SQLite error instead
    /// of 3 with the right sentence.
    #[test]
    fn the_census_answers_even_without_a_store() {
        let refused = Census::Refused(Refusal {
            tool: "ps".to_owned(),
            reason: "Operation not permitted".to_owned(),
        });
        let report = act(&Request {
            verb: "census",
            options: &no_options(),
            payload: &Payload::parse("{}").expect("payload vuoto"),
            raw: "",
            store: None,
            census: &refused,
            tty: "",
            at: 1_000,
        })
        .expect("il censimento risponde");

        assert_eq!(report.code, REFUSED);
        assert!(
            report.message.contains("I DO NOT KNOW"),
            "{}",
            report.message
        );
    }

    /// The list printed and the list accepted are the same one: a form that is
    /// documented and not accepted is discovered only by typing it.
    #[test]
    fn the_usage_names_every_form_the_dispatch_accepts() {
        for form in FORMS {
            assert!(
                USAGE
                    .iter()
                    .any(|line| line.contains(&format!("session {form}"))),
                "«{form}» è accettata e non è scritta in USAGE"
            );
        }
    }

    /// The anchor is `(tty, tree, progenitor)`, and it shows in the row written.
    #[test]
    fn an_arrival_is_anchored_to_the_tty_the_tree_and_the_ancestor() {
        let scratch = Scratch::new("anchor");
        let store = scratch.store();
        let report = ask(
            "open",
            r#"{"session_id":"abc","cwd":"/work/sailor",
                "transcript_path":"/tmp/abc.jsonl","hook_event_name":"SessionStart"}"#,
            &store,
            &one_terminal(),
            &no_options(),
        )
        .expect("registrare");
        assert_eq!(report.code, 0);

        let row = store.terminal("ttys004").expect("leggere").expect("c'è");
        assert_eq!(row.worktree, "/work/sailor");
        assert_eq!(row.ancestor.as_deref(), Some("Whatever"));
        assert_eq!(row.session_id.as_deref(), Some("abc"));
        let events = store.events_on("ttys004").expect("gli eventi");
        assert_eq!(events[0].name, "SessionStart", "il nome viene dal payload");
        assert!(
            events[0].payload.is_some(),
            "il payload si conserva com'è arrivato"
        );
    }

    /// **A MISSING FIELD FAILS NOTHING**: we make do with what there is. An
    /// empty payload still has a tty.
    #[test]
    fn a_payload_with_nothing_in_it_still_registers_the_terminal() {
        let scratch = Scratch::new("empty-payload");
        let store = scratch.store();
        ask("open", "{}", &store, &one_terminal(), &no_options()).expect("registrare");
        let row = store.terminal("ttys004").expect("leggere").expect("c'è");
        assert_eq!(row.session_id, None);
        assert!(
            !row.worktree.is_empty(),
            "l'albero cade sulla cartella corrente"
        );
        assert_eq!(
            store.events_on("ttys004").expect("gli eventi")[0].name,
            "open"
        );
    }

    /// **A REFUSED CENSUS DOES NOT BREAK A HOOK.** The row is written all the
    /// same, and the progenitor stays unknown instead of being invented.
    #[test]
    fn a_refused_census_does_not_stop_the_registration() {
        let scratch = Scratch::new("refused-open");
        let store = scratch.store();
        let report = ask(
            "open",
            r#"{"session_id":"abc","cwd":"/here"}"#,
            &store,
            &refused(),
            &no_options(),
        )
        .expect("un censimento negato non deve far fallire la registrazione");
        assert_eq!(report.code, 0);
        let row = store.terminal("ttys004").expect("leggere").expect("c'è");
        assert_eq!(
            row.ancestor, None,
            "un capostipite che non si è potuto leggere resta ignoto, non inventato"
        );
    }

    /// But `census` does: its answer *is* the census, and a refusal is
    /// recognisable without reading the text.
    #[test]
    fn the_census_says_it_does_not_know_and_says_it_with_its_own_code() {
        let scratch = Scratch::new("refused-census");
        let store = scratch.store();
        let report = ask("census", "", &store, &refused(), &no_options()).expect("censire");
        assert_eq!(report.code, REFUSED);
        assert!(
            report.message.contains("I DO NOT KNOW"),
            "un diniego va detto, non trasformato in un elenco vuoto: {}",
            report.message
        );

        let empty = Census::NoTerminal;
        let other = ask("census", "", &store, &empty, &no_options()).expect("censire");
        assert_eq!(other.code, 0);
        assert!(other.message.contains("no process"), "{}", other.message);
    }

    /// Detachment lives on the tty: the one door writes it and takes it off, and
    /// a new session in between does not clear it.
    #[test]
    fn detaching_through_the_command_holds_across_a_new_session() {
        let scratch = Scratch::new("detach");
        let store = scratch.store();
        ask("detach", "", &store, &one_terminal(), &no_options()).expect("staccare");
        ask(
            "open",
            r#"{"session_id":"nuova","cwd":"/here"}"#,
            &store,
            &one_terminal(),
            &no_options(),
        )
        .expect("aprire dopo");
        assert!(store
            .terminal("ttys004")
            .expect("leggere")
            .expect("c'è")
            .is_detached());
        ask("attach", "", &store, &one_terminal(), &no_options()).expect("riattaccare");
        assert!(!store
            .terminal("ttys004")
            .expect("leggere")
            .expect("c'è")
            .is_detached());
    }

    #[test]
    fn the_list_says_what_is_open_and_what_is_detached() {
        let scratch = Scratch::new("list");
        let store = scratch.store();
        ask(
            "open",
            r#"{"session_id":"abc","cwd":"/here"}"#,
            &store,
            &one_terminal(),
            &no_options(),
        )
        .expect("aprire");
        ask("detach", "", &store, &one_terminal(), &no_options()).expect("staccare");
        let report = ask("list", "", &store, &one_terminal(), &no_options()).expect("elencare");
        assert!(report.message.contains("ttys004"), "{}", report.message);
        // Through the catalogue, or the day the list is read in another
        // language this test calls a translation a defect.
        let open = catalogue::say("cli.session.row_open", &[]);
        let detached = catalogue::say("cli.session.row_detached", &[]);
        assert!(report.message.contains(&open), "{}", report.message);
        assert!(report.message.contains(&detached), "{}", report.message);
        assert!(report.message.contains("Whatever"), "{}", report.message);
    }

    /// **ASKING WHAT IS TRACKED NEEDS NO TERMINAL OF ITS OWN.** `list` and
    /// `census` do not speak of the terminal they are invoked from: they speak
    /// of all. Demanding a tty makes them fail wherever the output is captured —
    /// every script, every hook, every test — and the message sends the reader
    /// after an option instead of the defect.
    #[test]
    fn asking_what_is_tracked_does_not_need_a_terminal_of_its_own() {
        let scratch = Scratch::new("no-tty");
        let path = scratch.directory.join(SESSIONS_FILE);
        for form in ["list", "census"] {
            let words: Vec<String> = vec![
                form.to_owned(),
                "--store".to_owned(),
                path.display().to_string(),
            ];
            dispatch(&words)
                .unwrap_or_else(|error| panic!("«session {form}» ha preteso un tty: {error}"));
        }
    }

    #[test]
    fn an_unknown_form_names_the_ones_that_exist() {
        let message = dispatch(&["sweep".to_owned()]).expect_err("una forma ignota è un errore");
        for form in FORMS {
            assert!(message.contains(form), "{message} non nomina «{form}»");
        }
    }

    #[test]
    fn an_option_without_its_value_is_an_error() {
        let words: Vec<String> = ["--tty".to_owned()].into();
        assert!(options_of(&words).is_err());
        let pair: Vec<String> = ["--tty".to_owned(), "--json".to_owned()].into();
        assert!(options_of(&pair).is_err());
        let bare: Vec<String> = ["--json".to_owned()].into();
        assert_eq!(
            options_of(&bare).expect("--json non vuole valori")["json"],
            "true"
        );
    }
}
