//! Every flow path the code names exists. A commit once deleted eleven flows
//! while the code went on naming them, and nothing went red — see fault 52.
//! A path in a string literal is a promise the tree has to keep; the same
//! path in a comment is chronicle, and is left to the commit that tells it.

use std::path::{Path, PathBuf};

/// The two homes of a flow file, written the way the code writes them.
const HOMES_OF_FLOWS: &[&str] = &["flows/", "crates/flow/system/"];

const FLOW_SUFFIX: &str = ".flow.json";

/// Where the code is read: the crates, the window, and the window's shell.
const WHERE_THE_CODE_IS: &[&str] = &["crates", "desktop/src", "desktop/src-tauri/src"];

/// Itself excluded, or every path its fixtures name would be its own hit.
const THIS_JUDGE: &str = "every_flow_path_the_code_names_exists.rs";

/// Which quotes open a string: the web has three, Rust one and the raw form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Language {
    Rust,
    Web,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

fn language_of(path: &Path) -> Option<Language> {
    match path.extension().and_then(|kind| kind.to_str()) {
        Some("rs") => Some(Language::Rust),
        Some("ts") | Some("tsx") => Some(Language::Web),
        _ => None,
    }
}

fn sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for place in WHERE_THE_CODE_IS {
        walk(&root.join(place), &mut found);
    }
    found.sort();
    found
}

fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "target" | "node_modules" | "dist" | ".git") {
                walk(&path, found);
            }
        } else if name != THIS_JUDGE && language_of(&path).is_some() {
            found.push(path);
        }
    }
}

/// A raw string opened at `at`, when one is: its text, and where the code
/// resumes. `r#type` is a raw identifier and opens nothing.
fn raw_string_at(letters: &[char], at: usize, language: Language) -> Option<(String, usize)> {
    if language != Language::Rust || letters[at] != 'r' {
        return None;
    }
    let mut opens = at + 1;
    let mut hashes = 0usize;
    while letters.get(opens) == Some(&'#') {
        hashes += 1;
        opens += 1;
    }
    if letters.get(opens) != Some(&'"') {
        return None;
    }
    let start = opens + 1;
    let mut end = start;
    while end < letters.len()
        && !(letters[end] == '"' && (1..=hashes).all(|more| letters.get(end + more) == Some(&'#')))
    {
        end += 1;
    }
    Some((letters[start..end].iter().collect(), (end + 1 + hashes).min(letters.len())))
}

/// A quoted string opened at `at`: its text, and where the code resumes. An
/// escaped character is kept as written, so `\"` does not close it.
fn quoted_string(letters: &[char], at: usize) -> (String, usize) {
    let quote = letters[at];
    let mut text = String::new();
    let mut ahead = at + 1;
    while ahead < letters.len() && letters[ahead] != quote {
        if letters[ahead] == '\\' && ahead + 1 < letters.len() {
            text.push(letters[ahead]);
            ahead += 1;
        }
        text.push(letters[ahead]);
        ahead += 1;
    }
    (text, (ahead + 1).min(letters.len()))
}

/// Every string literal of a file with the line it opens on. Comments are
/// dropped as it goes, a raw string is read whole, and a char literal is not
/// a quote: read as one, `'"'` swallows the code up to the next quote below.
fn literals(text: &str, language: Language) -> Vec<(usize, String)> {
    let letters: Vec<char> = text.chars().collect();
    let quotes: &[char] = match language {
        Language::Rust => &['"'],
        Language::Web => &['"', '\'', '`'],
    };
    let mut found = Vec::new();
    let mut at = 0usize;
    let mut line = 1usize;
    while at < letters.len() {
        let letter = letters[at];
        let next = letters.get(at + 1).copied();
        if letter == '\n' {
            line += 1;
            at += 1;
        } else if letter == '/' && next == Some('/') {
            while at < letters.len() && letters[at] != '\n' {
                at += 1;
            }
        } else if letter == '/' && next == Some('*') {
            let closes = (at + 2..letters.len())
                .find(|&index| letters[index] == '*' && letters.get(index + 1) == Some(&'/'))
                .unwrap_or(letters.len());
            line += letters[at..closes].iter().filter(|&&each| each == '\n').count();
            at = (closes + 2).min(letters.len());
        } else if language == Language::Rust && letter == '\'' {
            let closes = if next == Some('\\') { at + 3 } else { at + 2 };
            at = if letters.get(closes) == Some(&'\'') { closes + 1 } else { at + 1 };
        } else if let Some((text, resumes)) = raw_string_at(&letters, at, language) {
            line += text.matches('\n').count();
            found.push((line - text.matches('\n').count(), text));
            at = resumes;
        } else if quotes.contains(&letter) {
            let (text, resumes) = quoted_string(&letters, at);
            line += text.matches('\n').count();
            found.push((line - text.matches('\n').count(), text));
            at = resumes;
        } else {
            at += 1;
        }
    }
    found
}

fn is_name_char(letter: char) -> bool {
    letter.is_ascii_alphanumeric() || matches!(letter, '_' | '-' | '.')
}

/// The flow paths one literal names: a home, one file name, the suffix. A
/// template's `{name}` and a glob's `*` name nothing, and neither does a
/// home glued to the end of a longer word.
fn flow_paths_named(literal: &str) -> Vec<String> {
    let mut named = Vec::new();
    for home in HOMES_OF_FLOWS {
        let mut from = 0usize;
        while let Some(found) = literal[from..].find(home) {
            let start = from + found;
            let after = start + home.len();
            let name: String = literal[after..].chars().take_while(|&letter| is_name_char(letter)).collect();
            let boundary = literal[..start].chars().next_back().is_none_or(|letter| !is_name_char(letter));
            if boundary && name.len() > FLOW_SUFFIX.len() && name.ends_with(FLOW_SUFFIX) {
                named.push(format!("{home}{name}"));
            }
            from = after;
        }
    }
    named
}

/// One place in the code naming one flow path.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Naming {
    file: String,
    line: usize,
    path: String,
}

fn namings_in(root: &Path) -> Vec<Naming> {
    let mut found = Vec::new();
    for file in sources(root) {
        let Some(language) = language_of(&file) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let shown = file.strip_prefix(root).unwrap_or(&file).display().to_string();
        for (line, literal) in literals(&text, language) {
            for path in flow_paths_named(&literal) {
                found.push(Naming { file: shown.clone(), line, path });
            }
        }
    }
    found
}

/// The namings the tree does not keep: the file they promise is not there.
fn broken(root: &Path) -> Vec<Naming> {
    namings_in(root)
        .into_iter()
        .filter(|naming| !root.join(&naming.path).is_file())
        .collect()
}

fn said(namings: &[Naming]) -> Vec<String> {
    namings
        .iter()
        .map(|naming| format!("{}:{} names {}", naming.file, naming.line, naming.path))
        .collect()
}

#[test]
fn every_flow_path_the_code_names_exists() {
    let root = root();
    let sources = sources(&root);
    assert!(
        sources.len() > 100,
        "the sources were not read: {} files found",
        sources.len()
    );
    let named = namings_in(&root);
    println!("today: {} sources read, {} flow paths named", sources.len(), named.len());
    let broken = broken(&root);
    assert!(
        broken.is_empty(),
        "flow paths the code names and the tree does not hold:\n  {}\n\
         Restore the file or stop naming it: a path in a literal is a promise, \
         and a deleted flow does not fail, it vanishes",
        said(&broken).join("\n  ")
    );
}

/// The reader is measured too: a literal is one in either language, a
/// comment is not, and the quote inside a char literal opens nothing.
#[test]
fn the_reader_keeps_literals_and_drops_comments() {
    let rust = "let a = \"one\"; // \"not this\"\n/* nor \"this\"\n */ let q = '\"'; let b = \"two\";\nlet c = r#\"raw \"three\"\"#;\nlet d = \"multi\nline\"; let e = \"five\";\n";
    assert_eq!(
        literals(rust, Language::Rust),
        vec![
            (1, "one".to_owned()),
            (3, "two".to_owned()),
            (4, "raw \"three\"".to_owned()),
            (5, "multi\nline".to_owned()),
            (6, "five".to_owned()),
        ]
    );
    let web = "const a = 'one'; // 'no'\nconst b = `two ${x}`; /* \"no\" */ const c = \"three\";\n";
    assert_eq!(
        literals(web, Language::Web),
        vec![(1, "one".to_owned()), (2, "two ${x}".to_owned()), (2, "three".to_owned())]
    );
    let nothing: Vec<String> = Vec::new();
    assert_eq!(flow_paths_named("../../crates/flow/system/*.flow.json"), nothing, "a glob names no file");
    assert_eq!(flow_paths_named("crates/flow/system/{name}.flow.json"), nothing, "a template names no file");
    assert_eq!(flow_paths_named("myflows/x.flow.json"), nothing, "a longer word is not the home");
    assert_eq!(flow_paths_named("flows/x.flow.json.bak"), nothing, "another suffix is another file");
    assert_eq!(
        flow_paths_named("see flows/a.flow.json and ../crates/flow/system/b.flow.json"),
        vec!["flows/a.flow.json", "crates/flow/system/b.flow.json"]
    );
}

/// A named path that exists passes; one that does not is reported with its
/// file and line, from every place the code is read and from none it is not.
#[test]
fn a_named_path_that_exists_passes_and_a_missing_one_is_reported_with_file_and_line() {
    let scratch = std::env::temp_dir().join(format!("sailor-flow-paths-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let shipped = scratch.join("crates").join("flow").join("system");
    let package = scratch.join("crates").join("one");
    let window = scratch.join("desktop").join("src");
    let shell = scratch.join("desktop").join("src-tauri").join("src");
    for directory in [
        scratch.join("flows"),
        shipped.clone(),
        package.join("src"),
        package.join("target"),
        window.clone(),
        shell.clone(),
        scratch.join("docs"),
    ] {
        std::fs::create_dir_all(&directory).expect("the scratch tree");
    }
    let write = |path: PathBuf, text: &str| std::fs::write(&path, text).expect("write");
    write(scratch.join("flows").join("here.flow.json"), "{}");
    write(shipped.join("shipped.flow.json"), "{}");
    write(
        package.join("src").join("lib.rs"),
        concat!(
            "const A: &str = \"flows/here.flow.json\";\n",
            "const B: &str = \"flows/gone.flow.json\";\n",
            "// flows/chronicle.flow.json is a comment\n",
            "const C: &str = \"crates/flow/system/shipped.flow.json\";\n",
            "const D: &str = \"crates/flow/system/vanished.flow.json\";\n",
            "let t = format!(\"crates/flow/system/{name}.flow.json\");\n",
            "let q = '\"'; const E: &str = \"flows/after-a-char.flow.json\";\n",
            "const F: &str = r#\"{\"flow\": \"flows/raw.flow.json\"}\"#;\n",
        ),
    );
    write(package.join("target").join("built.rs"), "const G: &str = \"flows/built.flow.json\";\n");
    write(
        window.join("window.ts"),
        concat!(
            "const g = \"../../crates/flow/system/*.flow.json\";\n",
            "import here from '../../flows/here.flow.json';\n",
            "const single = `flows/quoted.flow.json`;\n",
        ),
    );
    write(shell.join("run.rs"), "const H: &str = \"flows/shell.flow.json\";\n");
    write(scratch.join("docs").join("notes.md"), "`flows/prose.flow.json`\n");

    let mut reported = said(&broken(&scratch));
    let _ = std::fs::remove_dir_all(&scratch);
    reported.sort();

    assert_eq!(
        reported,
        vec![
            "crates/one/src/lib.rs:2 names flows/gone.flow.json",
            "crates/one/src/lib.rs:5 names crates/flow/system/vanished.flow.json",
            "crates/one/src/lib.rs:7 names flows/after-a-char.flow.json",
            "crates/one/src/lib.rs:8 names flows/raw.flow.json",
            "desktop/src-tauri/src/run.rs:1 names flows/shell.flow.json",
            "desktop/src/window.ts:3 names flows/quoted.flow.json",
        ]
    );
}
