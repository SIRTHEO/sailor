//! `sailor memory link` writes the page into the files the engines read, and
//! `unlink` takes it back out. What is proved here: the tree is written into
//! and the person's home is not, the block is replaced rather than repeated,
//! nothing outside the markers moves by a byte, and `where` changes its mind
//! afterwards.

use profiles::{Profile, ProfileStore};
use sailor::memory_cmd::{
    block, files_the_engines_read, link_files, unlink_files, where_lines, with_block,
    without_block, BLOCK_OPENS,
};
use std::path::{Path, PathBuf};

/// The engines of the fixture: each reads a file under `~` and one in the
/// tree, so both sides of the `--home` line come out of the same table.
const TWO_ENGINES: &str = r#"{"command_lines": [
     {"id": "un-motore", "executable": "unmotore",
      "home": {"variable": "UNMOTORE_HOME", "already_at": ".unmotore"},
      "reads_instructions_from": ["~/.unmotore/RULES.md", "RULES.md"]},
     {"id": "un-altro", "executable": "unaltro",
      "reads_instructions_from": ["~/OTHER.md", "OTHER.md"]}
   ]}"#;

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        let directory = std::env::temp_dir().join(format!(
            "sailor-memory-link-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
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

/// A tree, a home nobody has but this run, and the page under neither.
struct Fixture {
    _scratch: Scratch,
    tree: PathBuf,
    home: PathBuf,
    page: PathBuf,
}

fn fixture(label: &str) -> Fixture {
    let scratch = Scratch::new(label);
    let tree = scratch.0.join("tree");
    let home = scratch.0.join("home");
    std::fs::create_dir_all(&tree).expect("the tree");
    std::fs::create_dir_all(&home).expect("the home");
    let page = scratch.0.join("sailor").join("state").join("memory.md");
    Fixture { tree, home, page, _scratch: scratch }
}

impl Fixture {
    fn table(&self) -> Vec<profiles::KnownCli> {
        profiles::parse_command_lines(TWO_ENGINES).expect("the table parses")
    }

    /// The store the engines run under: the first one has a profile whose home
    /// is where its home already sits, so the file it reads is the same one.
    fn store(&self) -> ProfileStore {
        let mut store = ProfileStore::default();
        store.profiles.push(Profile {
            name: "work".to_owned(),
            cli_id: "un-motore".to_owned(),
            home_dir: self.home.join(".unmotore"),
            endpoint: None,
        });
        store.active.insert("un-motore".to_owned(), "work".to_owned());
        store
    }

    fn files(&self) -> Vec<PathBuf> {
        files_the_engines_read(&self.table(), &self.store(), &self.tree, &self.home, &self.page)
    }

    fn under_the_home(&self) -> Vec<PathBuf> {
        vec![self.home.join(".unmotore").join("RULES.md"), self.home.join("OTHER.md")]
    }

    fn under_the_tree(&self) -> Vec<PathBuf> {
        vec![self.tree.join("RULES.md"), self.tree.join("OTHER.md")]
    }
}

fn said(key: &str, path: &Path) -> String {
    catalogue::say(key, &[("path", &path.display().to_string())])
}

/// **THE HOME IS SOMEBODY'S OWN CONFIGURATION.** Linking writes the files
/// under the tree and does not create, touch or write a byte under the home;
/// each of those is named as left alone instead. With `--home` the same call
/// writes all four.
#[test]
fn a_file_under_the_home_is_left_alone_until_the_home_is_asked_for() {
    let fixture = fixture("home");
    let block = block(&fixture.page);
    let files = fixture.files();

    let lines = link_files(&files, &fixture.tree, false, &block);
    assert_eq!(
        lines,
        vec![
            said("cli.memory.link_left_alone", &fixture.under_the_home()[0]),
            said("cli.memory.linked", &fixture.under_the_tree()[0]),
            said("cli.memory.link_left_alone", &fixture.under_the_home()[1]),
            said("cli.memory.linked", &fixture.under_the_tree()[1]),
        ],
        "a file under the home is named as left alone, never written"
    );
    for file in fixture.under_the_home() {
        assert!(!file.exists(), "{} was written into without --home", file.display());
    }
    assert!(
        !fixture.home.join(".unmotore").exists(),
        "a directory was dug under the home without --home"
    );
    for file in fixture.under_the_tree() {
        assert!(std::fs::read_to_string(&file).expect("a file").contains(BLOCK_OPENS));
    }

    let asked = link_files(&files, &fixture.tree, true, &block);
    for (line, file) in asked.iter().zip(&files) {
        assert_eq!(line, &said("cli.memory.linked", file), "--home leaves nothing out");
    }
    for file in fixture.under_the_home() {
        assert!(
            std::fs::read_to_string(&file).expect("a file").contains(BLOCK_OPENS),
            "{} was not written even with --home",
            file.display()
        );
    }
}

/// A file with content before the block and content after it: linking again
/// replaces the block where it stands, and the second write differs from the
/// first by zero bytes.
#[test]
fn linking_twice_writes_one_block_and_moves_not_a_byte_around_it() {
    let fixture = fixture("twice");
    let file = fixture.tree.join("RULES.md");
    let head = "head of the file\n\nand a second paragraph\n";
    std::fs::write(&file, head).expect("a file");
    let block = block(&fixture.page);
    let files = fixture.files();

    link_files(&files, &fixture.tree, false, &block);
    let once = std::fs::read_to_string(&file).expect("a file");
    std::fs::write(&file, format!("{once}\ntail written after the block\n")).expect("a file");
    let before = std::fs::read_to_string(&file).expect("a file");

    link_files(&files, &fixture.tree, false, &block);
    let twice = std::fs::read_to_string(&file).expect("a file");
    assert_eq!(twice.matches(BLOCK_OPENS).count(), 1, "two blocks in one file:\n{twice}");
    assert_eq!(twice.len(), before.len(), "linking twice changed the file by bytes");
    assert_eq!(twice, before, "linking twice changed bytes outside the markers");
    assert!(twice.starts_with(head), "the head of the file moved");
    assert!(twice.ends_with("\ntail written after the block\n"), "the tail moved");
    assert!(twice.contains(&fixture.page.display().to_string()), "the block names no page");
}

/// Byte for byte, both ways: a file that was there is left holding exactly
/// what it held, and one Sailor made goes away with the block.
#[test]
fn unlinking_leaves_the_file_the_bytes_it_held_before_linking() {
    let fixture = fixture("back");
    let held = "before the block\n\nstill before it\n";
    let file = fixture.tree.join("RULES.md");
    std::fs::write(&file, held).expect("a file");
    let made = fixture.tree.join("OTHER.md");
    let files = fixture.files();

    link_files(&files, &fixture.tree, false, &block(&fixture.page));
    assert!(made.exists(), "a file that was not there is created");
    let lines = unlink_files(&files, &fixture.tree, false);
    assert_eq!(
        std::fs::read_to_string(&file).expect("a file"),
        held,
        "unlinking left the file something it never held"
    );
    assert!(!made.exists(), "{} held nothing but the block and should be gone", made.display());
    assert_eq!(
        lines,
        vec![
            said("cli.memory.link_left_alone", &fixture.under_the_home()[0]),
            said("cli.memory.unlinked", &file),
            said("cli.memory.link_left_alone", &fixture.under_the_home()[1]),
            said("cli.memory.link_removed", &made),
        ]
    );
    assert_eq!(
        unlink_files(&files, &fixture.tree, false)[1],
        said("cli.memory.link_nothing", &file),
        "a file with no block of ours is said to have none"
    );
    assert_eq!(std::fs::read_to_string(&file).expect("a file"), held);
}

/// **THE END TO END.** `where` says every engine is blind, `link` runs, and
/// `where` says the engines see the memories: the same reckoning of which
/// files matter, asked twice with one write in between.
#[test]
fn after_linking_the_engines_are_said_to_see_the_page() {
    let fixture = fixture("where");
    let (table, store) = (fixture.table(), fixture.store());
    let listed: Vec<String> = fixture
        .under_the_home()
        .iter()
        .zip(fixture.under_the_tree())
        .map(|(home, tree)| format!("{}, {}", home.display(), tree.display()))
        .collect();
    let engines = [("un-motore", " (work)"), ("un-altro", "")];
    let sentence = |key: &str, at: usize, file: &str| {
        let mut values = vec![
            ("engine", engines[at].0),
            ("profile", engines[at].1),
            ("files", listed[at].as_str()),
        ];
        if !file.is_empty() {
            values.push(("file", file));
        }
        catalogue::say(key, &values)
    };

    let blind = where_lines(&table, &store, &fixture.tree, &fixture.home, &fixture.page);
    assert_eq!(blind[1], sentence("cli.memory.where_blind", 0, ""));
    assert_eq!(blind[2], sentence("cli.memory.where_blind", 1, ""));

    let files = fixture.files();
    link_files(&files, &fixture.tree, false, &block(&fixture.page));

    let seeing = where_lines(&table, &store, &fixture.tree, &fixture.home, &fixture.page);
    for at in 0..engines.len() {
        let names_it = fixture.under_the_tree()[at].display().to_string();
        assert_eq!(
            seeing[at + 1],
            sentence("cli.memory.where_sees", at, &names_it),
            "the engine is still blind after linking"
        );
    }
}

/// The two halves of the edit on their own: the block goes into a text of any
/// shape, a second link adds no second block, and taking it out gives the
/// text back unchanged.
#[test]
fn the_block_goes_in_and_comes_out_of_any_text_it_finds() {
    let block = format!("{BLOCK_OPENS}\nsaid\n<!-- /sailor:memories -->");
    for held in ["", "a\n", "a", "a\n\n", "a\nb\n", "\n"] {
        let linked = with_block(held, &block);
        assert!(linked.contains(BLOCK_OPENS), "«{held}» took no block");
        assert_eq!(with_block(&linked, &block), linked, "a second block was added to «{held}»");
        assert_eq!(without_block(&linked), held, "«{held}» did not come back");
    }
}
