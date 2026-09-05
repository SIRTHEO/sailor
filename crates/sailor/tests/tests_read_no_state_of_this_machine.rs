//! Tests read no state of this machine: the doors through which a test can
//! read the home, the tools, the registers or the environment of whoever runs
//! it are counted in test code, and the count only ever falls — see fault 5.
//! A test that reads the machine turns red after a cleanup with the code
//! unchanged, and stays green on a machine set up like its author's.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The doors, as they are written in code. Each consults the process
/// environment or a path derived from it; every one has a parameterised form
/// a test hands a scratch to instead.
const DOORS: &[&str] = &[
    "Machine::current()",
    "Tools::current()",
    "SessionAbilities::current()",
    "PathLookup::current()",
    "Router::current()",
    "default_registry(",
    "env::var(\"HOME\")",
    "env::var_os(\"HOME\")",
    "env::vars()",
    "home_dir()",
    "sailor_home()",
    "default_directory()",
    "default_ledger_dir()",
    "load_store()",
    "save_store(",
    "state_path()",
    "profiles_root()",
    "default_path()",
    "default_roots(",
    "dirs::",
];

/// One line of test code per door, none of them a door of another kind: the
/// fixture that shows each door is seen, and would stop being seen if its
/// entry left the list above.
const ONE_LINE_PER_DOOR: &[&str] = &[
    "let machine = toolbox::Machine::current();",
    "let tools = toolbox::Tools::current();",
    "let abilities = SessionAbilities::current();",
    "let lookup = terminal::PathLookup::current();",
    "let router = Router::current();",
    "let registry = registry::default_registry(None, None);",
    "let home = std::env::var(\"HOME\").ok();",
    "let home = std::env::var_os(\"HOME\");",
    "let all: Vec<_> = std::env::vars().collect();",
    "let home = profiles::store_io::home_dir();",
    "let home = ledger::sailor_home();",
    "let store = ledger::default_directory();",
    "let store = ui::gather::default_ledger_dir();",
    "let profiles = profiles::store_io::load_store();",
    "profiles::store_io::save_store(&store).expect(\"saved\");",
    "let path = profiles::store_io::state_path();",
    "let root = profiles::store_io::profiles_root();",
    "let register = faults::Faults::default_path();",
    "let survey = inventory::default_roots(None);",
    "let home = dirs::config_dir();",
];

/// Doors still open in test code today. Downwards only. Three are the gate
/// that checks nothing of this machine is published, which has to read the
/// machine to know what to look for; two are the window's wiring test, which
/// asks both places for the ledger after declaring where it is; three are the
/// window's shell, whose tests still build the registry of this machine.
const DOORS_TODAY: usize = 8;

/// How far the seed may sit above the tree. Zero: a seed nobody re-measured
/// lets the next door open in silence.
const HOW_STALE_A_SEED_MAY_BE: usize = 0;

/// Where test code is written: the crates, and the window's shell.
const WHERE_CODE_IS_WRITTEN: &[&str] = &["crates", "desktop/src-tauri"];

const TEST_MODULE_MARK: &str = "#[cfg(test)]";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Door {
    path: PathBuf,
    line: usize,
    door: &'static str,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

fn rust_sources(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | "node_modules" | ".git") {
                rust_sources(&path, found);
            }
        } else if name.ends_with(".rs") {
            found.push(path);
        }
    }
}

/// How many `#` a raw string opening at `quote` carries; `None` when the
/// quote opens an ordinary string, a byte string included.
fn raw_hashes_before(chars: &[char], quote: usize) -> Option<usize> {
    let mut at = quote;
    let mut hashes = 0;
    while at > 0 && chars[at - 1] == '#' {
        at -= 1;
        hashes += 1;
    }
    if at == 0 || chars[at - 1] != 'r' {
        return None;
    }
    at -= 1;
    if at > 0 && chars[at - 1] == 'b' {
        at -= 1;
    }
    let starts_the_token = at == 0 || !(chars[at - 1].is_alphanumeric() || chars[at - 1] == '_');
    starts_the_token.then_some(hashes)
}

fn blank(c: char) -> char {
    if c == '\n' {
        '\n'
    } else {
        ' '
    }
}

/// The text with comments and string literals blanked out, newlines kept so
/// that line numbers still point where they did. A door named in a message
/// or a comment is a word about a door, not one being opened.
fn code_only(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(blank(chars[i]));
                    i += 1;
                }
            }
        } else if c == '"' {
            let raw = raw_hashes_before(&chars, i);
            out.push(' ');
            i += 1;
            while i < chars.len() {
                match raw {
                    Some(hashes) => {
                        if chars[i] == '"' && (1..=hashes).all(|k| chars.get(i + k) == Some(&'#')) {
                            out.push_str(&" ".repeat(hashes + 1));
                            i += hashes + 1;
                            break;
                        }
                    }
                    None => {
                        if chars[i] == '\\' {
                            out.push(' ');
                            out.push(chars.get(i + 1).copied().map(blank).unwrap_or(' '));
                            i += 2;
                            continue;
                        }
                        if chars[i] == '"' {
                            out.push(' ');
                            i += 1;
                            break;
                        }
                    }
                }
                out.push(blank(chars[i]));
                i += 1;
            }
        } else if c == '\'' && next == Some('\\') {
            let mut end = i + 2;
            while end < chars.len() && chars[end] != '\'' && chars[end] != '\n' {
                end += 1;
            }
            let consumed = if end < chars.len() { end - i + 1 } else { chars.len() - i };
            out.push_str(&" ".repeat(consumed));
            i += consumed;
        } else if c == '\'' && chars.get(i + 2) == Some(&'\'') {
            out.push_str("   ");
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Whether the whole file is test code by where it sits: under a `tests`
/// directory, or named `tests.rs` beside the source that declares it.
fn is_wholly_tests(path: &Path) -> bool {
    path.components().any(|part| part.as_os_str() == "tests")
        || path.file_name().is_some_and(|name| name == "tests.rs")
}

/// What a test mark stands on: a module written inline, whose braces bound
/// the region, or one declared and kept in a file of its own. A mark on a
/// lone function or impl opens nothing.
enum Marked {
    Inline { from: usize },
    Declared(String),
}

fn marked_modules(code: &str) -> Vec<Marked> {
    let lines: Vec<&str> = code.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() != TEST_MODULE_MARK {
            continue;
        }
        let Some(next) = lines.get(index + 1).map(|next| next.trim()) else {
            continue;
        };
        let Some(rest) = next.strip_prefix("pub mod ").or_else(|| next.strip_prefix("mod ")) else {
            continue;
        };
        if rest.ends_with(';') {
            let name = rest.trim_end_matches(';').trim().to_owned();
            found.push(Marked::Declared(name));
        } else {
            found.push(Marked::Inline { from: index + 1 });
        }
    }
    found
}

/// Which lines of a source file are test code: the inline test modules, each
/// from its `mod` line to the brace that closes it, counted on blanked code.
fn test_lines(code: &str) -> Vec<bool> {
    let lines: Vec<&str> = code.lines().collect();
    let mut in_tests = vec![false; lines.len()];
    for marked in marked_modules(code) {
        let Marked::Inline { from } = marked else {
            continue;
        };
        let mut depth: i64 = 0;
        for (index, line) in lines.iter().enumerate().skip(from) {
            in_tests[index] = true;
            depth += line.matches('{').count() as i64;
            depth -= line.matches('}').count() as i64;
            if depth <= 0 {
                break;
            }
        }
    }
    in_tests
}

/// The files that source files declare as their test modules, by the places
/// the language looks for a declared module.
fn declared_test_files(sources: &[PathBuf]) -> BTreeSet<PathBuf> {
    let mut declared = BTreeSet::new();
    for path in sources {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Some(parent) = path.parent() else {
            continue;
        };
        for marked in marked_modules(&code_only(&text)) {
            let Marked::Declared(name) = marked else {
                continue;
            };
            declared.insert(parent.join(format!("{name}.rs")));
            declared.insert(parent.join(&name).join("mod.rs"));
            if let Some(stem) = path.file_stem() {
                declared.insert(parent.join(stem).join(format!("{name}.rs")));
            }
        }
    }
    declared
}

/// Whether the door found at `at` in `line` stands in code: the blanked line
/// still carries the original character there. A door inside a string keeps
/// its quotes, so it is looked for in the original and accepted by the blank.
fn stands_in_code(line: &str, blanked: &str, at: usize) -> bool {
    let index = line[..at].chars().count();
    line.chars().nth(index) == blanked.chars().nth(index)
}

/// The doors open in the test code of one file, with their lines. `path` is
/// read below `root` to tell a `tests` directory from the machine's own;
/// `declared` holds the files other sources declare as their test modules.
fn doors_in(root: &Path, path: &Path, text: &str, declared: &BTreeSet<PathBuf>) -> Vec<Door> {
    let code = code_only(text);
    let wholly = declared.contains(path) || is_wholly_tests(path.strip_prefix(root).unwrap_or(path));
    let mask = if wholly {
        vec![true; code.lines().count()]
    } else {
        test_lines(&code)
    };
    let mut found = Vec::new();
    for (index, (line, blanked)) in text.lines().zip(code.lines()).enumerate() {
        if !mask.get(index).copied().unwrap_or(false) {
            continue;
        }
        for door in DOORS {
            for (at, _) in line.match_indices(door) {
                if stands_in_code(line, blanked, at) {
                    found.push(Door {
                        path: path.to_path_buf(),
                        line: index + 1,
                        door,
                    });
                }
            }
        }
    }
    found
}

fn measure(root: &Path) -> Vec<Door> {
    let mut sources = Vec::new();
    for place in WHERE_CODE_IS_WRITTEN {
        rust_sources(&root.join(place), &mut sources);
    }
    let declared = declared_test_files(&sources);
    let mut found: Vec<Door> = sources
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok().map(|text| (path, text)))
        .flat_map(|(path, text)| doors_in(root, path, &text, &declared))
        .collect();
    found.sort();
    found
}

fn listed(root: &Path, doors: &[Door]) -> String {
    doors
        .iter()
        .map(|door| {
            let shown = door.path.strip_prefix(root).unwrap_or(&door.path);
            format!("\n  {}:{} {}", shown.display(), door.line, door.door)
        })
        .collect()
}

#[test]
fn no_test_opens_a_door_to_this_machine_that_was_not_open_today() {
    let root = root();
    let doors = measure(&root);
    assert!(
        doors.len() <= DOORS_TODAY,
        "{} doors to this machine are open in test code, the seed says {}. A test that \
         reads the machine turns red after a cleanup with the code unchanged: hand it a \
         scratch, a `House::under`, or a `Machine::bare` instead. Open today:{}",
        doors.len(),
        DOORS_TODAY,
        listed(&root, &doors)
    );
}

/// The other side of the ratchet: a seed above the tree lets the next door
/// open unseen, so the seed follows the tree down as strictly as it holds it.
#[test]
fn a_seed_that_no_longer_describes_the_tree_is_a_seed_nobody_re_measured() {
    let root = root();
    let doors = measure(&root);
    assert!(
        DOORS_TODAY <= doors.len() + HOW_STALE_A_SEED_MAY_BE,
        "the seed says {} doors, the tree holds {}: lower DOORS_TODAY to {}. Open today:{}",
        DOORS_TODAY,
        doors.len(),
        doors.len(),
        listed(&root, &doors)
    );
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!("sailor-doors-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch tree");
        Scratch(path)
    }

    fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory");
        std::fs::write(&path, text).expect("write the fixture");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn shown(root: &Path, doors: &[Door]) -> Vec<(String, usize, &'static str)> {
    doors
        .iter()
        .map(|door| {
            let path = door.path.strip_prefix(root).unwrap_or(&door.path);
            (path.display().to_string(), door.line, door.door)
        })
        .collect()
}

/// Whoever measures gets measured: a door in a test file, in a `tests.rs`
/// and inside an in-source test module is seen with its line; one in
/// production code, in a built file, in a comment or in a string is not.
#[test]
fn the_scanner_sees_a_door_in_test_code_and_nothing_else() {
    let scratch = Scratch::new("shape");
    scratch.write(
        "crates/one/tests/reads.rs",
        "// Machine::current() in a comment is a word\n\
         fn a() {\n    let machine = Machine::current();\n}\n\
         const SAID: &str = \"ask ledger::sailor_home() instead\";\n\
         fn b() {\n    panic!(\"two lines\n        of default_directory() in a string\");\n}\n",
    );
    scratch.write("crates/one/src/tests.rs", "fn c() { let x = Tools::current(); }\n");
    scratch.write(
        "crates/one/src/lib.rs",
        "#[cfg(test)]\nmod support;\n\
         pub fn production() -> Option<PathBuf> { ledger::sailor_home() }\n\
         #[cfg(test)]\nfn helper() { let _ = env::vars(); }\n\
         #[cfg(test)]\nmod tests {\n    fn d() { let r = default_registry(None, None); }\n}\n\
         pub fn after() { let _ = PathLookup::current(); }\n",
    );
    scratch.write("crates/one/src/support.rs", "pub fn help() { let _ = Router::current(); }\n");
    scratch.write("crates/one/target/debug/built.rs", "fn e() { Machine::current(); }\n");
    scratch.write(
        "desktop/src-tauri/src/run.rs",
        "fn f() { PathLookup::current(); }\n#[cfg(test)]\nmod tests {\n    fn g() { let h = std::env::var(\"HOME\"); }\n}\n",
    );

    let doors = shown(&scratch.0, &measure(&scratch.0));
    assert_eq!(
        doors,
        vec![
            ("crates/one/src/lib.rs".to_owned(), 8, "default_registry("),
            ("crates/one/src/support.rs".to_owned(), 1, "Router::current()"),
            ("crates/one/src/tests.rs".to_owned(), 1, "Tools::current()"),
            ("crates/one/tests/reads.rs".to_owned(), 3, "Machine::current()"),
            ("desktop/src-tauri/src/run.rs".to_owned(), 4, "env::var(\"HOME\")"),
        ]
    );
}

/// Every door on the list is seen when it stands alone on a line of test
/// code, and is the only door seen there: an entry taken off the list turns
/// this red on its own line, and two entries that overlapped would too.
#[test]
fn every_door_is_seen_when_it_stands_alone() {
    assert_eq!(DOORS.len(), ONE_LINE_PER_DOOR.len(), "one fixture line per door");
    let scratch = Scratch::new("each");
    let mut wrong = Vec::new();
    for (index, (door, line)) in DOORS.iter().zip(ONE_LINE_PER_DOOR).enumerate() {
        let path = scratch.write(&format!("crates/one/tests/door_{index}.rs"), &format!("fn f() {{\n    {line}\n}}\n"));
        let text = std::fs::read_to_string(&path).expect("the fixture");
        let found: Vec<&'static str> = doors_in(&scratch.0, &path, &text, &BTreeSet::new())
            .into_iter()
            .map(|found| found.door)
            .collect();
        if found != vec![*door] {
            wrong.push(format!("\n  «{line}» opens {found:?}, not exactly «{door}»"));
        }
    }
    assert!(wrong.is_empty(), "{} fixture lines are not seen as their own door:{}", wrong.len(), wrong.concat());
}

/// Strings and comments are blanked and nothing else is: a char literal
/// holding a quote, a raw string with hashes, a nested block comment and an
/// escaped quote all close where the language closes them.
#[test]
fn the_blanking_follows_the_language_and_keeps_every_line() {
    let text = "let q = '\"'; let a = Machine::current();\n\
                let r = r#\"a \" default_registry( inside\"#; let b = Tools::current();\n\
                /* sailor_home() /* nested */ still a comment */ let c = env::vars();\n\
                let s = \"escaped \\\" quote default_directory()\"; let d = home_dir();\n\
                let lifetime: &'a str = load_store(); // state_path() is prose\n";
    let code = code_only(text);
    assert_eq!(code.lines().count(), text.lines().count(), "every line stays in place");
    let path = Path::new("crates/one/tests/blank.rs");
    let found: Vec<(usize, &str)> = doors_in(Path::new(""), path, text, &BTreeSet::new())
        .into_iter()
        .map(|door| (door.line, door.door))
        .collect();
    assert_eq!(
        found,
        vec![
            (1, "Machine::current()"),
            (2, "Tools::current()"),
            (3, "env::vars()"),
            (4, "home_dir()"),
            (5, "load_store()"),
        ]
    );
}

/// A mark on a function or an impl opens no test region; the mark on an inline
/// module opens one that its braces close, and two modules are two regions.
#[test]
fn only_the_mark_on_a_module_opens_the_test_region_and_its_braces_close_it() {
    let none = BTreeSet::new();
    let lone = "#[cfg(test)]\nfn helper() { Machine::current(); }\nfn code() { Tools::current(); }\n";
    let source = Path::new("crates/one/src/lib.rs");
    assert!(doors_in(Path::new(""), source, lone, &none).is_empty());

    let module = "fn code() { Tools::current(); }\n\
                  #[cfg(test)]\npub mod tests {\n    fn t() { Machine::current(); }\n}\n\
                  fn after() { Tools::current(); }\n\
                  #[cfg(test)]\nmod more {\n    fn u() {\n        env::vars();\n    }\n}\n\
                  fn last() { Tools::current(); }\n";
    let found: Vec<usize> = doors_in(Path::new(""), source, module, &none)
        .into_iter()
        .map(|door| door.line)
        .collect();
    assert_eq!(found, vec![4, 10]);
}
