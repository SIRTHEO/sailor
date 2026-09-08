//! Every child process starts by one road: `Process::start` in the supervisor
//! is the only place that spawns and then writes the `processes` table, and
//! nothing in the language forces a caller through it — see fault 4. So the
//! places that start a child elsewhere are counted, and the count only falls.

use std::path::{Path, PathBuf};
use workspace::ratchet::{weigh, Weighed};

/// The shapes that leave a live child behind. `.spawn()` takes no argument
/// on a `Command`, std or tokio, while every thread or task spawn takes a
/// closure: the empty parentheses are what tells the two apart. `.output(`
/// and `.status(` wait for the child and are not roads.
const ROADS: &[&str] = &[".spawn()", "daemon(", "fork("];

/// One line of code per road, none of them another road: the fixture that
/// shows each road is seen, and would stop being seen if its entry left.
const ONE_LINE_PER_ROAD: &[&str] = &[
    "let child = std::process::Command::new(\"sleep\").spawn();",
    "let went = unsafe { libc::daemon(0, 0) };",
    "let pid = unsafe { libc::fork() };",
];

/// The file that holds `Process::start`: the road every other one should be.
const THE_ONE_ROAD: &str = "crates/supervisor/src/child.rs";

/// Roads open outside the one road today, downwards only: in `actions`,
/// `ledger`, `models`, `sailor` and `terminal`, sources and their in-source
/// test modules alike.
const ROADS_TODAY: usize = 12;

const WHERE_CODE_IS_WRITTEN: &str = "crates";
const SOURCE_TREE: &str = "src";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Road {
    path: PathBuf,
    line: usize,
    road: &'static str,
}

/// A reader that reached the end of a file inside a comment or a string has
/// blanked everything after the opening: it says so instead of counting zero.
#[derive(Debug, PartialEq, Eq)]
struct Blind {
    line: usize,
    inside: &'static str,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

fn rust_sources_under(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "target" {
                rust_sources_under(&path, found);
            }
        } else if name.ends_with(".rs") {
            found.push(path);
        }
    }
}

/// Only `crates/<crate>/src/`: integration tests and the window are outside.
fn sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(crates) = std::fs::read_dir(root.join(WHERE_CODE_IS_WRITTEN)) else {
        return found;
    };
    for entry in crates.flatten() {
        rust_sources_under(&entry.path().join(SOURCE_TREE), &mut found);
    }
    found.sort();
    found
}

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

fn line_of(chars: &[char], at: usize) -> usize {
    chars[..at].iter().filter(|c| **c == '\n').count() + 1
}

/// The text with comments and string literals blanked, newlines kept so line
/// numbers still point where they did. A road named in a message or a comment
/// is a word about a road, not one being taken.
fn code_only(text: &str) -> Result<String, Blind> {
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
            let opened = line_of(&chars, i);
            let mut depth = 0;
            let mut closed = false;
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
                        closed = true;
                        break;
                    }
                } else {
                    out.push(blank(chars[i]));
                    i += 1;
                }
            }
            if !closed {
                return Err(Blind { line: opened, inside: "a block comment" });
            }
        } else if c == '"' {
            let opened = line_of(&chars, i);
            let raw = raw_hashes_before(&chars, i);
            out.push(' ');
            i += 1;
            let mut closed = false;
            while i < chars.len() {
                match raw {
                    Some(hashes) => {
                        if chars[i] == '"' && (1..=hashes).all(|k| chars.get(i + k) == Some(&'#')) {
                            out.push_str(&" ".repeat(hashes + 1));
                            i += hashes + 1;
                            closed = true;
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
                            closed = true;
                            break;
                        }
                    }
                }
                out.push(blank(chars[i]));
                i += 1;
            }
            if !closed {
                return Err(Blind { line: opened, inside: "a string" });
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
    Ok(out)
}

/// A road found at `at` stands in code when the blanked line still carries
/// the original character there, and when a road that is a bare name begins
/// a token: `pitchfork(` is not `fork(`, while `cmd.spawn()` is a road.
fn stands_in_code(line: &str, blanked: &str, at: usize, road: &str) -> bool {
    let index = line[..at].chars().count();
    let is_a_name = road.chars().next().is_some_and(|first| first.is_alphanumeric() || first == '_');
    let own_token = !is_a_name
        || index == 0
        || line
            .chars()
            .nth(index - 1)
            .is_none_or(|before| !(before.is_alphanumeric() || before == '_'));
    own_token && line.chars().nth(index) == blanked.chars().nth(index)
}

fn roads_in(path: &Path, text: &str) -> Result<Vec<Road>, Blind> {
    let code = code_only(text)?;
    let mut found = Vec::new();
    for (index, (line, blanked)) in text.lines().zip(code.lines()).enumerate() {
        for road in ROADS {
            for (at, _) in line.match_indices(road) {
                if stands_in_code(line, blanked, at, road) {
                    found.push(Road {
                        path: path.to_path_buf(),
                        line: index + 1,
                        road,
                    });
                }
            }
        }
    }
    Ok(found)
}

fn read_or_blind(path: &Path) -> Result<Vec<Road>, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
    roads_in(path, &text).map_err(|blind| {
        format!(
            "{}:{} opens {} that never closes: the reader would blank the rest of the file",
            path.display(),
            blind.line,
            blind.inside
        )
    })
}

/// The one road is left out of the count only after it is found where the
/// judge looks and seen to hold exactly one road: an exclusion that matches
/// nothing would hide a renamed file.
fn measure(root: &Path) -> Vec<Road> {
    let the_one_road = root.join(THE_ONE_ROAD);
    let sources = sources(root);
    assert!(
        sources.contains(&the_one_road),
        "the one road {THE_ONE_ROAD} is not where the judge looks: move THE_ONE_ROAD with it"
    );
    let inside = read_or_blind(&the_one_road).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        inside.len(),
        1,
        "the one road holds {} roads, not one:{}",
        inside.len(),
        listed(root, &inside)
    );
    let mut found = Vec::new();
    let mut blind = Vec::new();
    for path in &sources {
        if *path == the_one_road {
            continue;
        }
        match read_or_blind(path) {
            Ok(roads) => found.extend(roads),
            Err(why) => blind.push(format!("\n  {why}")),
        }
    }
    assert!(blind.is_empty(), "the reader went blind on {} sources:{}", blind.len(), blind.concat());
    workspace::measured_against(
        sources.len(),
        "sources read for roads that start a child",
        ONE_LINE_PER_ROAD.len(),
        "shapes of a road",
    );
    found.sort();
    found
}

fn listed(root: &Path, roads: &[Road]) -> String {
    roads
        .iter()
        .map(|road| {
            let shown = road.path.strip_prefix(root).unwrap_or(&road.path);
            format!("\n  {}:{} {}", shown.display(), road.line, road.road)
        })
        .collect()
}

#[test]
fn no_child_starts_by_a_road_that_was_not_open_today() {
    let root = root();
    let roads = measure(&root);
    if let Weighed::TreeIsAbove(more) = weigh(ROADS_TODAY, roads.len()) {
        panic!(
            "{} roads start a child outside {THE_ONE_ROAD}, {more} more than the seed's {}. \
             A child started there is never written to the `processes` table, so nothing can \
             stop or resume it: route the call through `supervisor::child::Process::start`. \
             Open today:{}",
            roads.len(),
            ROADS_TODAY,
            listed(&root, &roads)
        );
    }
}

/// The other side of the ratchet: a seed above the tree lets the next road
/// open unseen, so the seed follows the tree down as strictly as it holds it.
#[test]
fn a_seed_that_no_longer_describes_the_tree_is_a_seed_nobody_re_measured() {
    let root = root();
    let roads = measure(&root);
    if let Weighed::TreeIsBelow(apart) = weigh(ROADS_TODAY, roads.len()) {
        panic!(
            "the seed says {} roads and the tree holds {}, {apart} apart: write \
             ROADS_TODAY = {}. Open today:{}",
            ROADS_TODAY,
            roads.len(),
            roads.len(),
            listed(&root, &roads)
        );
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!("sailor-roads-{label}-{}", std::process::id()));
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

fn shown(root: &Path, roads: &[Road]) -> Vec<(String, usize, &'static str)> {
    roads
        .iter()
        .map(|road| {
            let path = road.path.strip_prefix(root).unwrap_or(&road.path);
            (path.display().to_string(), road.line, road.road)
        })
        .collect()
}

/// Whoever measures gets measured: of five `.spawn()` in one file, the one in
/// code is seen with its line, and the four in comments and strings are not.
#[test]
fn the_reader_sees_one_spawn_among_the_words_about_spawning() {
    let text = "// a comment about how we .spawn() the shell\n\
                /* the block says .spawn() too,\n   over two lines */\n\
                let message = \"never .spawn() without a token\";\n\
                let raw = r#\"the \" quote does not end .spawn() here\"#;\n\
                let child = std::process::Command::new(\"sleep\").arg(\"1\").spawn();\n\
                let waited = std::process::Command::new(\"true\").output();\n";
    let found: Vec<(usize, &str)> = roads_in(Path::new("crates/one/src/lib.rs"), text)
        .expect("a text that closes everything it opens")
        .into_iter()
        .map(|road| (road.line, road.road))
        .collect();
    assert_eq!(found, vec![(6, ".spawn()")]);
}

/// A thread, a scoped thread and a task are spawned with a closure; a child
/// that is waited for is not a road; a word that ends in `fork(` is not one.
#[test]
fn a_spawn_that_takes_a_closure_or_waits_for_the_child_is_not_a_road() {
    let text = "let t = std::thread::spawn(|| work());\n\
                let s = scope.spawn(move || drain(&mut pipe));\n\
                let out = Command::new(\"git\").output();\n\
                let st = Command::new(\"git\").status();\n\
                let tine = pitchfork(); let daemons = undaemon(true);\n";
    let found = roads_in(Path::new("crates/one/src/lib.rs"), text).expect("closed text");
    assert!(found.is_empty(), "these are not roads:{}", listed(Path::new(""), &found));
}

#[test]
fn every_road_is_seen_when_it_stands_alone() {
    assert_eq!(ROADS.len(), ONE_LINE_PER_ROAD.len(), "one fixture line per road");
    let mut wrong = Vec::new();
    for (road, line) in ROADS.iter().zip(ONE_LINE_PER_ROAD) {
        let text = format!("fn f() {{\n    {line}\n}}\n");
        let found: Vec<&'static str> = roads_in(Path::new("crates/one/src/lib.rs"), &text)
            .expect("closed text")
            .into_iter()
            .map(|found| found.road)
            .collect();
        if found != vec![*road] {
            wrong.push(format!("\n  «{line}» takes {found:?}, not exactly «{road}»"));
        }
    }
    assert!(wrong.is_empty(), "{} fixture lines are not seen as their own road:{}", wrong.len(), wrong.concat());
}

/// Integration tests, the window and the one road itself are outside the
/// count; a road under any crate's `src` is inside it, test module or not.
#[test]
fn the_judge_reads_the_sources_of_the_crates_and_leaves_the_one_road_out() {
    let scratch = Scratch::new("where");
    scratch.write(THE_ONE_ROAD, "pub fn start() { let c = command.spawn(); }\n");
    scratch.write("crates/one/src/lib.rs", "fn a() { let c = Command::new(\"x\").spawn(); }\n");
    scratch.write(
        "crates/one/src/deep/tests.rs",
        "#[cfg(test)]\nmod tests {\n    fn t() { let c = Command::new(\"x\").spawn(); }\n}\n",
    );
    scratch.write("crates/one/tests/outside.rs", "fn b() { let c = Command::new(\"x\").spawn(); }\n");
    scratch.write("desktop/src-tauri/src/main.rs", "fn w() { let c = Command::new(\"x\").spawn(); }\n");
    scratch.write("crates/one/target/debug/built.rs", "fn e() { let c = Command::new(\"x\").spawn(); }\n");
    let found = shown(&scratch.0, &measure(&scratch.0));
    assert_eq!(
        found,
        vec![
            ("crates/one/src/deep/tests.rs".to_string(), 3, ".spawn()"),
            ("crates/one/src/lib.rs".to_string(), 1, ".spawn()"),
        ]
    );
}

/// A reader that could blank a whole file in silence is no judge: a comment
/// or a string left open turns it red and names the line that opened it.
#[test]
fn a_reader_that_would_go_blind_says_so_instead_of_counting_zero() {
    let comment = "fn a() {}\n/* opened here\nlet c = Command::new(\"x\").spawn();\n";
    assert_eq!(
        roads_in(Path::new("crates/one/src/lib.rs"), comment),
        Err(Blind { line: 2, inside: "a block comment" })
    );
    let string = "fn a() {}\nlet s = \"opened here\nlet c = Command::new(x).spawn();\n";
    assert_eq!(
        roads_in(Path::new("crates/one/src/lib.rs"), string),
        Err(Blind { line: 2, inside: "a string" })
    );
    let scratch = Scratch::new("blind");
    scratch.write(THE_ONE_ROAD, "pub fn start() { let c = command.spawn(); }\n");
    scratch.write("crates/one/src/lib.rs", comment);
    let outcome = std::panic::catch_unwind(|| measure(&scratch.0));
    let why = outcome.expect_err("a blind reader is red");
    let why = why.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(why.contains("crates/one/src/lib.rs:2"), "the message names the file and the line: {why}");
}
