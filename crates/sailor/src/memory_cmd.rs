//! `sailor memory page`: the page of memories as the one file every command
//! line reads at its start, written under Sailor's home or printed.
//! `sailor memory where`: which engines read a file that names that page, and
//! which do not — said, never written into their files.
//! `sailor memory link` and `sailor memory unlink`: the same files, written
//! into and cleaned out again, and only the ones this tree owns unless asked.

use profiles::{KnownCli, Profile, ProfileStore};
use std::path::{Path, PathBuf};

pub const USAGE: &[crate::Form] = &[
    crate::Form {
        form: "sailor memory page [--print] [--tree <path>]",
        says_key: "cli.memory.page_says",
    },
    crate::Form {
        form: "sailor memory where",
        says_key: "cli.memory.where_says",
    },
    crate::Form {
        form: "sailor memory link [--home]",
        says_key: "cli.memory.link_says",
    },
    crate::Form {
        form: "sailor memory unlink [--home]",
        says_key: "cli.memory.unlink_says",
    },
];

pub fn run(args: &[String]) -> i32 {
    match args.split_first() {
        Some((form, options)) if form == "page" => match page_options(options) {
            Ok(asked) => write_or_print(&asked),
            Err(message) => {
                eprintln!("sailor memory: {message}");
                2
            }
        },
        Some((form, [])) if form == "where" => where_the_page_is_read(),
        Some((form, options)) if form == "link" || form == "unlink" => {
            match home_option(options, form) {
                Ok(home_too) => point_the_engines(form == "link", home_too),
                Err(message) => {
                    eprintln!("sailor memory: {message}");
                    2
                }
            }
        }
        _ => {
            let forms: Vec<&str> = USAGE.iter().map(|form| form.form).collect();
            eprintln!(
                "sailor memory: {}",
                catalogue::say("cli.memory.wants_a_form", &[("usage", &forms.join(", "))])
            );
            2
        }
    }
}

/// What `sailor memory page` was asked: the machine's page written or printed,
/// or one tree's page — only ever printed, since the file on disk is the
/// machine's.
#[derive(Debug, PartialEq, Eq)]
struct PageAsked {
    print: bool,
    tree: Option<PathBuf>,
}

fn page_options(options: &[String]) -> Result<PageAsked, String> {
    let mut asked = PageAsked { print: false, tree: None };
    let mut rest = options.iter();
    while let Some(word) = rest.next() {
        match word.as_str() {
            "--print" => asked.print = true,
            "--tree" => {
                asked.tree = Some(PathBuf::from(rest.next().ok_or_else(|| {
                    catalogue::say("cli.option_wants_a_value", &[("option", "--tree")])
                })?))
            }
            other => {
                return Err(catalogue::say(
                    "cli.memory.unknown_option",
                    &[("option", other), ("usage", USAGE[0].form)],
                ))
            }
        }
    }
    Ok(asked)
}

fn write_or_print(asked: &PageAsked) -> i32 {
    let ledger = match ledger::Ledger::open(ui::gather::default_ledger_dir()) {
        Ok(ledger) => ledger,
        Err(error) => {
            eprintln!("sailor memory: {error}");
            return 1;
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let memories = match actions::memory::remembered(&ledger, now) {
        Ok(memories) => memories,
        Err(error) => {
            eprintln!("sailor memory: {error}");
            return 1;
        }
    };
    if let Some(tree) = &asked.tree {
        println!("{}", actions::memory::page(&memories, &actions::memory::tree_of(tree)));
        return 0;
    }
    if asked.print {
        println!("{}", actions::memory::page_of_every_tree(&memories));
        return 0;
    }
    let Some(home) = ledger::sailor_home() else {
        eprintln!("sailor memory: {}", catalogue::say("cli.memory.no_home", &[]));
        return 1;
    };
    match actions::memory::write_page(&ledger, &home) {
        Ok(path) => {
            println!(
                "{}",
                catalogue::say(
                    "cli.memory.written",
                    &[("count", &memories.len().to_string()), ("path", &path.display().to_string())],
                )
            );
            0
        }
        Err(error) => {
            eprintln!("sailor memory: {}", catalogue::say(&format!("run.failure.{}", error.class), &[]));
            eprintln!("   {}", error.said);
            1
        }
    }
}

/// Where the page is, whose home the engines start in, and which tree the
/// command was typed in. The project root is that tree: it is where an engine
/// opened here would be started.
struct Places {
    page: PathBuf,
    user_home: PathBuf,
    store: ProfileStore,
    project_root: PathBuf,
}

fn the_places() -> Result<Places, String> {
    let home = ledger::sailor_home().ok_or_else(|| catalogue::say("cli.memory.no_home", &[]))?;
    let user_home = profiles::store_io::home_dir()?;
    let store = profiles::store_io::load_store()?;
    let project_root = std::env::current_dir().map_err(|error| error.to_string())?;
    Ok(Places { page: actions::memory::page_path(&home), user_home, store, project_root })
}

/// The engines of this machine, one line each, under the profile in force.
fn where_the_page_is_read() -> i32 {
    let places = match the_places() {
        Ok(places) => places,
        Err(why) => {
            eprintln!("sailor memory: {why}");
            return 1;
        }
    };
    let lines = where_lines(
        profiles::known_clis(),
        &places.store,
        &places.project_root,
        &places.user_home,
        &places.page,
    );
    for line in lines {
        println!("{line}");
    }
    0
}

/// The pointer written into those same files, or taken back out of them.
fn point_the_engines(linking: bool, home_too: bool) -> i32 {
    let places = match the_places() {
        Ok(places) => places,
        Err(why) => {
            eprintln!("sailor memory: {why}");
            return 1;
        }
    };
    let files = files_the_engines_read(
        profiles::known_clis(),
        &places.store,
        &places.project_root,
        &places.user_home,
        &places.page,
    );
    let lines = if linking {
        link_files(&files, &places.project_root, home_too, &block(&places.page))
    } else {
        unlink_files(&files, &places.project_root, home_too)
    };
    for line in lines {
        println!("{line}");
    }
    0
}

fn home_option(options: &[String], form: &str) -> Result<bool, String> {
    let mut home_too = false;
    for word in options {
        match word.as_str() {
            "--home" => home_too = true,
            other => {
                let usage = USAGE.iter().find(|shape| shape.form.split(' ').nth(2) == Some(form));
                return Err(catalogue::say(
                    "cli.memory.link_unknown_option",
                    &[("option", other), ("usage", usage.map(|shape| shape.form).unwrap_or_default())],
                ));
            }
        }
    }
    Ok(home_too)
}

/// What an engine sees of the page at its start: the files it reads, and the
/// first of them that names the page's path. Nothing is written into them.
pub struct Sight {
    pub reads: Vec<PathBuf>,
    pub names_the_page: Option<PathBuf>,
}

impl Sight {
    pub fn of(
        cli: &KnownCli,
        project_root: &Path,
        home: &Path,
        profile_home: Option<&Path>,
        page: &Path,
    ) -> Sight {
        let reads = profiles::instruction_files(cli, project_root, home, profile_home);
        let needle = page.display().to_string();
        // The block is the mark; a bare path is one too, so a file linked
        // before the block named a gesture keeps its answer.
        let names_the_page = reads
            .iter()
            .find(|file| {
                std::fs::read_to_string(file)
                    .is_ok_and(|text| holds_the_block(&text) || text.contains(&needle))
            })
            .cloned();
        Sight { reads, names_the_page }
    }

    /// The files, as a reader lists them.
    pub fn listed(&self) -> String {
        let named: Vec<String> = self.reads.iter().map(|file| file.display().to_string()).collect();
        named.join(", ")
    }
}

/// The profile in force for a command line, when the store names one that exists.
pub fn active_profile<'a>(store: &'a ProfileStore, cli_id: &str) -> Option<&'a Profile> {
    let name = store.active.get(cli_id)?;
    store
        .profiles
        .iter()
        .find(|profile| profile.cli_id == cli_id && &profile.name == name)
}

/// The page first, then one line per engine: what it reads, and whether one
/// of those files names the page. An engine nobody looked into is said to be
/// that, never «it does not see».
pub fn where_lines(
    clis: &[KnownCli],
    store: &ProfileStore,
    project_root: &Path,
    home: &Path,
    page: &Path,
) -> Vec<String> {
    let mut lines = vec![catalogue::say(
        "cli.memory.where_page",
        &[("path", &page.display().to_string())],
    )];
    for cli in clis {
        let profile = active_profile(store, &cli.id);
        let under = profile.map(|profile| format!(" ({})", profile.name)).unwrap_or_default();
        let sight = Sight::of(cli, project_root, home, profile.map(|profile| profile.home_dir.as_path()), page);
        let listed = sight.listed();
        let file = sight
            .names_the_page
            .as_ref()
            .map(|file| file.display().to_string())
            .unwrap_or_default();
        let mut values = vec![("engine", cli.display_name.as_str()), ("profile", under.as_str())];
        lines.push(if sight.reads.is_empty() {
            catalogue::say("cli.memory.where_unknown", &values)
        } else if sight.names_the_page.is_some() {
            values.push(("files", listed.as_str()));
            values.push(("file", file.as_str()));
            catalogue::say("cli.memory.where_sees", &values)
        } else {
            values.push(("files", listed.as_str()));
            catalogue::say("cli.memory.where_blind", &values)
        });
    }
    lines
}

/// The two lines that bound what Sailor writes into somebody else's file.
/// Everything outside them belongs to whoever wrote it and is kept byte for
/// byte.
pub const BLOCK_OPENS: &str = "<!-- sailor:memories -->";
pub const BLOCK_CLOSES: &str = "<!-- /sailor:memories -->";

/// The file this tree keeps its own rules in, named to the engine so it stops
/// reaching for tools of its own.
pub const RULES_OF_THIS_TREE: &str = "AGENTS.md";

/// Every file the engines read at their start, once each, in the order the
/// engines declare them: the very list `where` reports on.
pub fn files_the_engines_read(
    clis: &[KnownCli],
    store: &ProfileStore,
    project_root: &Path,
    home: &Path,
    page: &Path,
) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for cli in clis {
        let profile = active_profile(store, &cli.id);
        let sight = Sight::of(cli, project_root, home, profile.map(|it| it.home_dir.as_path()), page);
        for file in sight.reads {
            if !found.contains(&file) {
                found.push(file);
            }
        }
    }
    found
}

/// What goes between the markers, from the catalogue: the gesture that renders
/// the page, and where this tree's rules are. **IT NAMES A GESTURE, NOT A
/// PATH** — the page sits under one person's home, and a block carrying that
/// path could never be committed.
pub fn block(_page: &Path) -> String {
    format!(
        "{BLOCK_OPENS}\n{}\n{}\n{BLOCK_CLOSES}",
        catalogue::say("cli.memory.link_block_page", &[]),
        catalogue::say("cli.memory.link_block_rules", &[("rules", RULES_OF_THIS_TREE)]),
    )
}

/// Where the block sits: the opening marker's first byte, and the byte after
/// the closing one.
fn markers(text: &str) -> Option<(usize, usize)> {
    let opens = text.find(BLOCK_OPENS)?;
    let closes = text[opens..].find(BLOCK_CLOSES)? + opens + BLOCK_CLOSES.len();
    Some((opens, closes))
}

pub fn holds_the_block(text: &str) -> bool {
    markers(text).is_some()
}

/// `text` with the block in it: what stands between the markers is replaced
/// where it stands, so everything else is kept byte for byte. With no block
/// yet it is appended after one newline — the one `without_block` takes off.
pub fn with_block(text: &str, block: &str) -> String {
    match markers(text) {
        Some((opens, closes)) => format!("{}{block}{}", &text[..opens], &text[closes..]),
        None if text.is_empty() => format!("{block}\n"),
        None => format!("{text}\n{block}\n"),
    }
}

/// `text` without the block, and without the newline on either side of it: a
/// file linked and then unlinked holds the bytes it held before.
pub fn without_block(text: &str) -> String {
    let Some((opens, closes)) = markers(text) else {
        return text.to_owned();
    };
    let from = if text[..opens].ends_with('\n') { opens - 1 } else { opens };
    let to = if text[closes..].starts_with('\n') { closes + 1 } else { closes };
    format!("{}{}", &text[..from], &text[to..])
}

/// Whether a file is this tree's to write into. One outside it is the
/// person's own configuration, and it is written only when asked for.
fn ours(file: &Path, project_root: &Path, home_too: bool) -> bool {
    home_too || file.starts_with(project_root)
}

fn said(key: &str, path: &Path) -> String {
    catalogue::say(key, &[("path", &path.display().to_string())])
}

fn unwritten(path: &Path, why: &std::io::Error) -> String {
    catalogue::say(
        "cli.memory.link_unwritten",
        &[("path", &path.display().to_string()), ("why", &why.to_string())],
    )
}

/// What the file holds now, and nothing when it is not there yet: that is the
/// one case linking creates a file instead of appending to one.
fn held(file: &Path) -> Result<String, std::io::Error> {
    match std::fs::read_to_string(file) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        other => other,
    }
}

fn write_it(file: &Path, text: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, text)
}

/// The block written into the files this tree owns, one line said per file.
/// A file outside the tree is named and left alone unless `home_too`.
pub fn link_files(files: &[PathBuf], project_root: &Path, home_too: bool, block: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for file in files {
        if !ours(file, project_root, home_too) {
            lines.push(said("cli.memory.link_left_alone", file));
            continue;
        }
        let written = held(file).and_then(|text| write_it(file, &with_block(&text, block)));
        lines.push(match written {
            Ok(()) => said("cli.memory.linked", file),
            Err(error) => unwritten(file, &error),
        });
    }
    lines
}

/// The block taken back out. A file left holding nothing is one Sailor made,
/// so it goes with the block rather than staying behind empty.
pub fn unlink_files(files: &[PathBuf], project_root: &Path, home_too: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for file in files {
        if !ours(file, project_root, home_too) {
            lines.push(said("cli.memory.link_left_alone", file));
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap_or_default();
        if !holds_the_block(&text) {
            lines.push(said("cli.memory.link_nothing", file));
            continue;
        }
        let left = without_block(&text);
        let (key, done) = if left.is_empty() {
            ("cli.memory.link_removed", std::fs::remove_file(file))
        } else {
            ("cli.memory.unlinked", write_it(file, &left))
        };
        lines.push(match done {
            Ok(()) => said(key, file),
            Err(error) => unwritten(file, &error),
        });
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Scratch {
            let directory = std::env::temp_dir().join(format!(
                "sailor-memory-cmd-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default()
            ));
            std::fs::create_dir_all(&directory).expect("the scratch directory");
            Scratch(directory)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// `--tree` takes the path after it and `--print` stands alone; a word
    /// that is neither is refused, and so is a `--tree` with nothing after it.
    #[test]
    fn the_page_options_are_read_as_typed() {
        let words = |list: &[&str]| list.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
        let both = page_options(&words(&["--tree", "/a/tree", "--print"])).expect("read");
        assert_eq!(both, PageAsked { print: true, tree: Some(PathBuf::from("/a/tree")) });
        assert_eq!(page_options(&[]).expect("read"), PageAsked { print: false, tree: None });
        assert!(page_options(&words(&["--tree"])).is_err(), "a --tree with no path after it");
        assert!(page_options(&words(&["--loud"])).is_err(), "a word that is no option");
    }

    /// **ONE LOOK SAYS WHICH ENGINE SEES THE MEMORIES.** The file is read where
    /// the active profile starts the engine; the page's path in it is what
    /// counts; an engine with nothing declared is told apart from one that is
    /// blind; and nothing is written into any of those files.
    #[test]
    fn one_line_per_engine_says_whether_its_files_name_the_page() {
        let scratch = Scratch::new("where");
        let home = scratch.0.join("home");
        let project = scratch.0.join("tree");
        let profile_home = scratch.0.join("profiles").join("un-motore").join("work");
        std::fs::create_dir_all(&project).expect("the tree");
        std::fs::create_dir_all(&profile_home).expect("the profile home");
        let page = scratch.0.join("sailor").join("state").join("memory.md");
        let table = profiles::parse_command_lines(
            r#"{"command_lines": [
                 {"id": "un-motore", "executable": "unmotore",
                  "home": {"variable": "UNMOTORE_HOME", "already_at": ".unmotore"},
                  "reads_instructions_from": ["~/.unmotore/RULES.md", "RULES.md"]},
                 {"id": "un-altro", "executable": "unaltro"}
               ]}"#,
        )
        .expect("it parses");
        let mut store = ProfileStore::default();
        store.profiles.push(Profile {
            name: "work".to_owned(),
            cli_id: "un-motore".to_owned(),
            home_dir: profile_home.clone(),
            endpoint: None,
        });
        store.active.insert("un-motore".to_owned(), "work".to_owned());
        let rules_in_the_profile = profile_home.join("RULES.md");
        let rules_in_the_tree = project.join("RULES.md");
        let files = format!("{}, {}", rules_in_the_profile.display(), rules_in_the_tree.display());
        let said = |sight_key: &str, extra: &[(&str, &str)]| {
            let mut values = vec![("engine", "un-motore"), ("profile", " (work)")];
            values.extend_from_slice(extra);
            catalogue::say(sight_key, &values)
        };

        std::fs::write(&rules_in_the_tree, "rules, and nothing about any page").expect("a file");
        let blind = where_lines(&table, &store, &project, &home, &page);
        assert_eq!(blind[0], catalogue::say("cli.memory.where_page", &[("path", &page.display().to_string())]));
        assert_eq!(blind[1], said("cli.memory.where_blind", &[("files", &files)]));
        assert_eq!(
            blind[2],
            catalogue::say("cli.memory.where_unknown", &[("engine", "un-altro"), ("profile", "")])
        );

        std::fs::write(&rules_in_the_profile, format!("read {} first", page.display())).expect("a file");
        let seeing = where_lines(&table, &store, &project, &home, &page);
        assert_eq!(
            seeing[1],
            said(
                "cli.memory.where_sees",
                &[("files", &files), ("file", &rules_in_the_profile.display().to_string())]
            )
        );
        assert_eq!(
            std::fs::read_to_string(&rules_in_the_tree).expect("still a file"),
            "rules, and nothing about any page",
            "a file an engine reads was written into"
        );
    }
}
