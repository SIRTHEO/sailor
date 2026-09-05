//! `sailor memory page`: the page of memories as the one file every command
//! line reads at its start, written under Sailor's home or printed.
//! `sailor memory where`: which engines read a file that names that page, and
//! which do not — said, never written into their files.

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

/// The engines of this machine, one line each, under the profile in force.
/// The project root is where the command is typed: that is the tree the
/// engine would be started in.
fn where_the_page_is_read() -> i32 {
    let Some(home) = ledger::sailor_home() else {
        eprintln!("sailor memory: {}", catalogue::say("cli.memory.no_home", &[]));
        return 1;
    };
    let gathered = profiles::store_io::home_dir().and_then(|user_home| {
        let store = profiles::store_io::load_store()?;
        let project_root = std::env::current_dir().map_err(|error| error.to_string())?;
        Ok((user_home, store, project_root))
    });
    let (user_home, store, project_root) = match gathered {
        Ok(gathered) => gathered,
        Err(why) => {
            eprintln!("sailor memory: {why}");
            return 1;
        }
    };
    let page = actions::memory::page_path(&home);
    for line in where_lines(profiles::known_clis(), &store, &project_root, &user_home, &page) {
        println!("{line}");
    }
    0
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
        let names_the_page = reads
            .iter()
            .find(|file| std::fs::read_to_string(file).is_ok_and(|text| text.contains(&needle)))
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
